//! The host's bundles of kart states (issue #63, `docs/ARCHITECTURE.md` § « Topologie »). Pure
//! logic, unit-tested.
//!
//! Each client sends the states of its kart to the host only (`Message::KartState`, 30 times a
//! second). The host keeps the newest state of each kart since its last bundle ([`StateRelay`]),
//! and sends each client **one packet** with every kart it does not drive itself: the host's, its
//! bots', the other clients' (`Message::KartStates`, grouped by the peer that drives them: the
//! receiver checks that peer's authority as if the state came from it). The bundles leave with
//! the host's own states (every `NetTuning::send_interval_ticks`), or after that interval when
//! only the clients' came (the host has no kart yet). Nothing allocated once warm: the buffers
//! are reused.

use super::protocol::{KartSnapshot, MAX_BUNDLED_STATES, Message, PeerKey};

/// What the host relays of the kart states (see the module's documentation).
#[derive(Debug, Default)]
pub struct StateRelay {
    /// The newest state of each kart of a client since the last bundles: (its peer, the state).
    fresh: Vec<(PeerKey, KartSnapshot)>,
    /// When the last bundles left (ms, real time).
    last_ms: Option<u64>,
    /// Reused for each bundle.
    bundle: Vec<(PeerKey, KartSnapshot)>,
}

impl StateRelay {
    /// A state of a client's kart arrived: the newest one of that kart goes in the next bundles.
    pub fn push(&mut self, origin: PeerKey, state: KartSnapshot) {
        match self
            .fresh
            .iter_mut()
            .find(|(peer, kart)| *peer == origin && kart.kart == state.kart)
        {
            Some(entry) => {
                if state.tick >= entry.1.tick || state.session != entry.1.session {
                    entry.1 = state;
                }
            }
            None => self.fresh.push((origin, state)),
        }
    }

    /// The states of the clients waiting for the next bundles.
    pub fn waiting(&self) -> usize {
        self.fresh.len()
    }

    /// Whether the bundles leave now (`now_ms`, real time): with our own states (`ours`), or
    /// once `interval_ms` went by with the clients' only.
    pub fn due(&self, now_ms: u64, ours: bool, interval_ms: u64) -> bool {
        ours || (!self.fresh.is_empty()
            && self.last_ms.is_none_or(|last| now_ms >= last + interval_ms))
    }

    /// The bundles for `client`: our states (`own`, as `me`), then the fresh states of the other
    /// clients, grouped by peer; [`MAX_BUNDLED_STATES`] at most in each (usually one bundle).
    /// `send` gets each one.
    pub fn bundles_for(
        &mut self,
        client: PeerKey,
        me: PeerKey,
        own: &[KartSnapshot],
        mut send: impl FnMut(&Message),
    ) {
        self.bundle.clear();
        self.bundle.extend(own.iter().map(|state| (me, *state)));
        let first = self.bundle.len();
        self.bundle.extend(
            self.fresh
                .iter()
                .filter(|(origin, _)| *origin != client && *origin != me)
                .copied(),
        );
        // One group per peer (a client drives one kart: already so).
        self.bundle[first..].sort_unstable_by_key(|(origin, state)| (*origin, state.kart));
        let mut start = 0;
        while start < self.bundle.len() {
            let end = (start + MAX_BUNDLED_STATES).min(self.bundle.len());
            let message = if start == 0 && end == self.bundle.len() {
                Message::KartStates(std::mem::take(&mut self.bundle))
            } else {
                Message::KartStates(self.bundle[start..end].to_vec())
            };
            send(&message);
            if let Message::KartStates(back) = message
                && start == 0
                && end == back.len()
                && self.bundle.is_empty()
            {
                // The buffer comes back for the next one.
                self.bundle = back;
            }
            start = end;
        }
    }

    /// The bundles left at `now_ms`: the next ones wait for new states.
    pub fn sent(&mut self, now_ms: u64) {
        self.fresh.clear();
        self.last_ms = Some(now_ms);
    }

    /// A client left: its states go no further.
    pub fn forget(&mut self, peer: PeerKey) {
        self.fresh.retain(|(origin, _)| *origin != peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: PeerKey = PeerKey(1 << 100);
    const ANA: PeerKey = PeerKey(2 << 100);
    const BOB: PeerKey = PeerKey(3 << 100);

    fn state(kart: u8, tick: u32) -> KartSnapshot {
        KartSnapshot {
            kart,
            tick,
            session: 1,
            ..Default::default()
        }
    }

    fn bundles(relay: &mut StateRelay, client: PeerKey, own: &[KartSnapshot]) -> Vec<Message> {
        let mut out = Vec::new();
        relay.bundles_for(client, HOST, own, |message| out.push(message.clone()));
        out
    }

    /// Issue #63: one packet per client, with the host's karts (its own and its bots) and the
    /// newest state of every other client's kart, never the client's own; the packet reads back
    /// the same (grouped by peer).
    #[test]
    fn each_client_gets_one_packet_with_the_other_karts() {
        let mut relay = StateRelay::default();
        relay.push(ANA, state(5, 100));
        relay.push(BOB, state(6, 101));
        // An older state of Ana's kart, come late: the newest stays.
        relay.push(ANA, state(5, 98));
        relay.push(ANA, state(5, 102));
        assert_eq!(relay.waiting(), 2);
        let own = [state(4, 103), state(0, 103), state(1, 103)];
        let to_ana = bundles(&mut relay, ANA, &own);
        assert_eq!(
            to_ana,
            vec![Message::KartStates(vec![
                (HOST, state(4, 103)),
                (HOST, state(0, 103)),
                (HOST, state(1, 103)),
                (BOB, state(6, 101)),
            ])]
        );
        let to_bob = bundles(&mut relay, BOB, &own);
        assert_eq!(to_bob.len(), 1);
        let Message::KartStates(states) = &to_bob[0] else {
            panic!("{to_bob:?}");
        };
        assert_eq!(states[3], (ANA, state(5, 102)));
        for message in to_ana.iter().chain(&to_bob) {
            assert_eq!(Message::decode(&message.to_bytes()).as_ref(), Ok(message));
        }
        // Sent: nothing waits; Ana's next state, alone, waits for the interval.
        relay.sent(1_000);
        assert_eq!(relay.waiting(), 0);
        assert!(!relay.due(1_010, false, 33));
        relay.push(ANA, state(5, 104));
        assert!(!relay.due(1_020, false, 33), "the interval");
        assert!(relay.due(1_033, false, 33));
        assert!(relay.due(1_010, true, 33), "with our own states, at once");
        // Ana gone: her state goes no further.
        relay.forget(ANA);
        assert_eq!(bundles(&mut relay, BOB, &[]), vec![]);
    }

    /// More karts than a packet holds (12: the host's 8 bots and its kart, and 5 clients, some
    /// of them watching the race): two packets, each a valid one; nothing allocated once warm
    /// for a single packet.
    #[test]
    fn more_karts_than_a_packet_holds_go_in_two() {
        let mut relay = StateRelay::default();
        for n in 0..5u8 {
            relay.push(PeerKey(u128::from(n + 10) << 100), state(9 + n, 50));
        }
        let own: Vec<KartSnapshot> = (0..9).map(|kart| state(kart, 51)).collect();
        let out = bundles(&mut relay, PeerKey(10 << 100), &own);
        let counts: Vec<usize> = out
            .iter()
            .map(|message| match message {
                Message::KartStates(states) => states.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(counts, vec![MAX_BUNDLED_STATES, 1]);
        for message in &out {
            assert!(Message::decode(&message.to_bytes()).is_ok());
        }
        // A single packet: its buffer comes back.
        relay.sent(0);
        relay.push(ANA, state(5, 60));
        bundles(&mut relay, BOB, &[state(4, 61)]);
        let capacity = relay.bundle.capacity();
        assert!(capacity >= 2);
        bundles(&mut relay, BOB, &[state(4, 62)]);
        assert_eq!(relay.bundle.capacity(), capacity);
    }
}
