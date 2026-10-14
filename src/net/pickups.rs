//! Who picked up what (issue #53, `docs/ARCHITECTURE.md` § « Objets et croquettes en ligne »):
//! the host decides which kart got an item box, a croquette of the track or a scattered
//! croquette. Pure logic: no Bevy, no socket; unit-tested.
//!
//! - **Claims**: a kart touching a box or a croquette takes it at once on its own screen (its
//!   roulette spins, its croquette counts: a prediction), and its peer sends a [`Claim`] to every
//!   peer: the spot, the kart, the common tick.
//! - **The host decides** ([`Arbiter::decide`]): the claims it has, in order of their tick, then
//!   of the better grid slot (the smaller id). A claim is granted when no other kart got its spot
//!   within the spot's respawn time of it (a box: 2 s, a croquette of the track: 10 s, a scattered
//!   croquette: ever); otherwise it is refused. A claim that comes after a grant is refused, even
//!   if its tick is earlier: the first to reach the host wins. Every [`Verdict`] goes to every peer.
//! - **Every peer keeps the same record** ([`Arbiter::record`]): the grants so far, and the
//!   claims not decided yet (every peer receives every claim). When the host leaves, the next one
//!   decides what the old one left undecided, from the same grants: nothing is given twice, and a
//!   claim is never lost. A verdict that comes twice (the old host's and the new one's) counts
//!   once: the first one stands.

use super::protocol::{Claim, KartId, PeerKey, Spot, Verdict};

/// A claim, whatever its draws: its spot, its kart, its tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClaimKey {
    pub spot: Spot,
    pub kart: KartId,
    pub tick: u32,
}

impl ClaimKey {
    pub fn of(claim: &Claim) -> Self {
        Self {
            spot: claim.pickup.spot(),
            kart: claim.kart,
            tick: claim.tick,
        }
    }
}

/// The refused claims kept to recognize a repeat, at most.
const REFUSED_KEPT: usize = 256;

/// The record of the pickups of a session (a race, or free driving), the same on every peer;
/// the host decides with it (see the module's documentation).
#[derive(Clone, Debug, Default)]
pub struct Arbiter {
    /// Every grant, in the host's order.
    grants: Vec<ClaimKey>,
    /// The latest refused claims.
    refused: Vec<ClaimKey>,
    /// The claims not decided yet, with the peer that sent them.
    pending: Vec<(PeerKey, Claim)>,
}

impl Arbiter {
    /// Whether this claim was decided already, and how (`Some(true)`: granted).
    pub fn decided(&self, key: &ClaimKey) -> Option<bool> {
        if self.grants.contains(key) {
            Some(true)
        } else if self.refused.contains(key) {
            Some(false)
        } else {
            None
        }
    }

    /// A claim seen (ours, or another peer's): kept until a verdict decides it. Returns false
    /// when it was decided or seen already.
    pub fn claim(&mut self, from: PeerKey, claim: Claim) -> bool {
        let key = ClaimKey::of(&claim);
        if self.decided(&key).is_some() || self.pending.iter().any(|(_, c)| ClaimKey::of(c) == key)
        {
            return false;
        }
        self.pending.push((from, claim));
        true
    }

    /// The claims not decided yet.
    pub fn pending(&self) -> impl Iterator<Item = &Claim> {
        self.pending.iter().map(|(_, claim)| claim)
    }

    /// Who got `spot` within `respawn` ticks of `tick` (`None`: ever), if anyone.
    pub fn taken(&self, spot: Spot, tick: u32, respawn: Option<u32>) -> Option<&ClaimKey> {
        self.grants.iter().rev().find(|grant| {
            grant.spot == spot && respawn.is_none_or(|ticks| grant.tick.abs_diff(tick) < ticks)
        })
    }

    /// The host decides every claim it has (see the module's documentation): `respawn` gives the
    /// respawn time of a spot in ticks (`None`: it never comes back). The verdicts go into `out`,
    /// in their order, and into the record.
    pub fn decide(&mut self, respawn: impl Fn(Spot) -> Option<u32>, out: &mut Vec<Verdict>) {
        if self.pending.is_empty() {
            return;
        }
        // Earlier first; on the same tick, the better grid slot.
        self.pending
            .sort_by_key(|(_, claim)| (claim.tick, claim.kart));
        let pending = std::mem::take(&mut self.pending);
        for (_, claim) in &pending {
            let key = ClaimKey::of(claim);
            let granted = self.taken(key.spot, key.tick, respawn(key.spot)).is_none();
            let verdict = Verdict {
                claim: *claim,
                granted,
            };
            self.record(&verdict);
            out.push(verdict);
        }
        // Keep the allocation of the pending list.
        self.pending = pending;
        self.pending.clear();
    }

    /// A verdict (the host's, or ours as the host): into the record, its claim no longer
    /// pending. Returns false when that claim was decided already (a repeat after a change of
    /// host: the first verdict stands).
    pub fn record(&mut self, verdict: &Verdict) -> bool {
        let key = ClaimKey::of(&verdict.claim);
        self.pending.retain(|(_, claim)| ClaimKey::of(claim) != key);
        if self.decided(&key).is_some() {
            return false;
        }
        if verdict.granted {
            self.grants.push(key);
        } else {
            if self.refused.len() >= REFUSED_KEPT {
                self.refused.remove(0);
            }
            self.refused.push(key);
        }
        true
    }

    /// `peer` left the room: its claims are dropped (its karts are gone, or another peer drives
    /// them now).
    pub fn forget(&mut self, peer: PeerKey) {
        self.pending.retain(|(from, _)| *from != peer);
    }

    /// Every grant so far, in the host's order.
    pub fn grants(&self) -> &[ClaimKey] {
        &self.grants
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::{LooseId, Pickup};

    const ANA: PeerKey = PeerKey(1 << 100);
    const BOB: PeerKey = PeerKey(2 << 100);
    const CYD: PeerKey = PeerKey(3 << 100);

    /// 2 s for a box, 10 s for a croquette, once for a scattered one.
    fn respawn(spot: Spot) -> Option<u32> {
        match spot {
            Spot::Box(_) => Some(120),
            Spot::Croquette(_) => Some(600),
            Spot::Loose(_) => None,
        }
    }

    fn on_box(index: u16, kart: KartId, tick: u32) -> Claim {
        Claim {
            session: 1,
            pickup: Pickup::Box {
                index,
                draw: 0,
                draws: 1,
                gap: 0.0,
            },
            kart,
            tick,
        }
    }

    fn on_croquette(pickup: Pickup, kart: KartId, tick: u32) -> Claim {
        Claim {
            session: 1,
            pickup,
            kart,
            tick,
        }
    }

    fn decide(arbiter: &mut Arbiter) -> Vec<Verdict> {
        let mut out = Vec::new();
        arbiter.decide(respawn, &mut out);
        out
    }

    fn granted(verdicts: &[Verdict]) -> Vec<(KartId, u32)> {
        verdicts
            .iter()
            .filter(|v| v.granted)
            .map(|v| (v.claim.kart, v.claim.tick))
            .collect()
    }

    /// Issue #53: two karts on the same box on the same tick: the better grid slot gets it, the
    /// other is refused. On different ticks: the earlier one, whatever the order they came in.
    #[test]
    fn two_karts_on_one_box_only_one_gets_it() {
        let mut arbiter = Arbiter::default();
        assert!(arbiter.claim(BOB, on_box(3, 7, 500)));
        assert!(arbiter.claim(ANA, on_box(3, 4, 500)));
        let verdicts = decide(&mut arbiter);
        assert_eq!(verdicts.len(), 2);
        assert_eq!(granted(&verdicts), vec![(4, 500)]);
        assert!(!verdicts.iter().find(|v| v.claim.kart == 7).unwrap().granted);
        // Another box, the later tick first: the earlier tick wins.
        arbiter.claim(ANA, on_box(5, 4, 601));
        arbiter.claim(BOB, on_box(5, 7, 600));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(7, 600)]);
        assert_eq!(arbiter.grants().len(), 2);
        assert_eq!(arbiter.pending().count(), 0);
    }

    /// Issue #53: a box is taken once; it can be taken again once it is back (2 s later), and a
    /// claim that comes after a grant is refused even with an earlier tick (the first to reach
    /// the host wins: no grant is ever taken back).
    #[test]
    fn a_box_is_taken_once_until_it_comes_back() {
        let mut arbiter = Arbiter::default();
        arbiter.claim(ANA, on_box(0, 4, 1_000));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(4, 1_000)]);
        // Late, with an earlier tick: refused.
        arbiter.claim(BOB, on_box(0, 7, 998));
        assert!(granted(&decide(&mut arbiter)).is_empty());
        // Still there 1.98 s later: refused; 2 s later, back: granted.
        arbiter.claim(BOB, on_box(0, 7, 1_119));
        assert!(granted(&decide(&mut arbiter)).is_empty());
        arbiter.claim(BOB, on_box(0, 7, 1_120));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(7, 1_120)]);
        assert_eq!(
            arbiter
                .taken(Spot::Box(0), 1_150, Some(120))
                .map(|g| g.kart),
            Some(7)
        );
        assert!(arbiter.taken(Spot::Box(0), 1_300, Some(120)).is_none());
    }

    /// Issue #53: a croquette of the track is taken once for 10 s; a scattered croquette once and
    /// for good.
    #[test]
    fn a_croquette_is_picked_up_once() {
        let mut arbiter = Arbiter::default();
        let track = Pickup::Croquette { index: 12 };
        let loose = Pickup::Loose(LooseId {
            kart: 2,
            loss: 0,
            index: 1,
        });
        arbiter.claim(ANA, on_croquette(track, 4, 100));
        arbiter.claim(BOB, on_croquette(track, 7, 101));
        arbiter.claim(BOB, on_croquette(loose, 7, 100));
        arbiter.claim(ANA, on_croquette(loose, 4, 100));
        let verdicts = decide(&mut arbiter);
        // The croquette of the track to the earlier tick, the scattered one (the same tick) to
        // the better grid slot.
        assert_eq!(granted(&verdicts), vec![(4, 100), (4, 100)]);
        assert_eq!(
            arbiter
                .grants()
                .iter()
                .map(|g| (g.spot, g.kart))
                .collect::<Vec<_>>(),
            vec![(track.spot(), 4), (loose.spot(), 4)]
        );
        assert_eq!(verdicts.iter().filter(|v| !v.granted).count(), 2);
        // Back after 10 s; the scattered one never.
        arbiter.claim(BOB, on_croquette(track, 7, 700));
        arbiter.claim(ANA, on_croquette(loose, 4, 5_000));
        assert_eq!(granted(&decide(&mut arbiter)), vec![(7, 700)]);
    }

    /// The same claim twice, or a claim decided already: kept once, decided once. A verdict that
    /// comes twice counts once, and the first one stands.
    #[test]
    fn a_claim_is_decided_once() {
        let mut arbiter = Arbiter::default();
        let claim = on_box(1, 4, 50);
        assert!(arbiter.claim(ANA, claim));
        assert!(!arbiter.claim(ANA, claim), "seen already");
        assert_eq!(decide(&mut arbiter).len(), 1);
        assert!(!arbiter.claim(BOB, claim), "decided already");
        assert!(decide(&mut arbiter).is_empty());
        let refused = Verdict {
            claim,
            granted: false,
        };
        assert!(!arbiter.record(&refused), "the first verdict stands");
        assert_eq!(arbiter.decided(&ClaimKey::of(&claim)), Some(true));
    }

    /// Issue #53: the host leaves in the middle of the arbitration. Every peer saw every claim
    /// and every verdict that reached it: the new host decides what the old one left, from the
    /// same grants, without giving anything twice. A verdict of the old host that never reached
    /// the new one is decided again, the same way; when it then reaches a peer that had it, it
    /// counts once.
    #[test]
    fn a_new_host_decides_what_the_old_one_left() {
        // Ana hosts; Bob and Cyd keep the record.
        let mut ana = Arbiter::default();
        let mut bob = Arbiter::default();
        let mut cyd = Arbiter::default();
        let claims = [
            (BOB, on_box(2, 7, 300)),
            (CYD, on_box(2, 9, 301)),
            (CYD, on_box(4, 9, 302)),
            (BOB, on_box(4, 7, 303)),
            (BOB, on_box(6, 7, 304)),
        ];
        // The first three reach everyone; Ana decides them.
        for (from, claim) in &claims[..3] {
            for arbiter in [&mut ana, &mut bob, &mut cyd] {
                arbiter.claim(*from, *claim);
            }
        }
        let first = decide(&mut ana);
        assert_eq!(granted(&first), vec![(7, 300), (9, 302)]);
        // Bob gets every verdict; Cyd only the first one: Ana leaves with the others on the way.
        for verdict in &first {
            assert!(bob.record(verdict));
        }
        assert!(cyd.record(&first[0]));
        // The last two claims reach Bob and Cyd, never Ana.
        for (from, claim) in &claims[3..] {
            bob.claim(*from, *claim);
            cyd.claim(*from, *claim);
        }
        // Ana's own claims, if any, are dropped with her.
        bob.forget(ANA);
        // Bob hosts now: he decides what is left, the same way Ana would have.
        assert_eq!(bob.pending().count(), 2);
        let second = decide(&mut bob);
        assert_eq!(granted(&second), vec![(7, 304)]);
        assert!(!second[0].granted, "box 4 is Cyd's kart's: refused");
        // Cyd decided nothing; he records Bob's verdicts, then Ana's late ones (repeats).
        for verdict in &second {
            assert!(cyd.record(verdict));
        }
        assert_eq!(cyd.pending().count(), 2, "Ana's last verdicts still owed");
        // Had Cyd become the host, he would have decided them again, the same way.
        let mut repeat = cyd.clone();
        let decided = decide(&mut repeat);
        assert_eq!(
            decided.iter().map(|v| v.granted).collect::<Vec<_>>(),
            first[1..].iter().map(|v| v.granted).collect::<Vec<_>>(),
            "decided again, the same way"
        );
        for verdict in &first[1..] {
            assert!(cyd.record(verdict));
            assert!(!bob.record(verdict), "a repeat counts once");
        }
        // Everyone ends with the same grants: nothing twice, nothing lost.
        let spots = |a: &Arbiter| {
            let mut all: Vec<(Spot, KartId)> =
                a.grants().iter().map(|g| (g.spot, g.kart)).collect();
            all.sort_by_key(|(spot, kart)| (format!("{spot:?}"), *kart));
            all
        };
        assert_eq!(spots(&bob), spots(&cyd));
        assert_eq!(spots(&bob).len(), 3);
        assert_eq!(cyd.pending().count(), 0);
    }
}
