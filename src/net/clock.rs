//! Latency and clock of a peer (issue #7, `docs/ARCHITECTURE.md` § « Synchronisation »), from
//! the ping / pong exchange. Pure logic, unit-tested.
//!
//! - **Ping** (shown in the lobby): the round trip of the last pongs, smoothed.
//! - **Clock**: each peer counts milliseconds from its own start. A pong carries the clock of the
//!   peer that answers; half the round trip later it reads `clock + rtt / 2`, so its offset from
//!   ours is `clock + rtt / 2 - now`. The sample with the shortest round trip among the last
//!   [`SAMPLES`] is the least disturbed by the network: its offset is kept. Every peer can then
//!   speak in the host's time (`Lobby::host_time_ms`), from which issue #9 counts race ticks.

/// Samples kept to choose the clock offset from.
pub const SAMPLES: usize = 8;

/// A pong older than this is a lost ping's late echo, or garbage: ignored (ms).
pub const MAX_ROUND_TRIP_MS: u32 = 10_000;

/// Weight of a new round trip in the smoothed ping.
const SMOOTHING: f32 = 0.25;

/// What the pongs of one peer told us.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PeerClock {
    /// Smoothed round trip (ms); `None` before the first pong.
    rtt_ms: Option<f32>,
    /// The last samples: (round trip, offset of the peer's clock from ours), ms.
    samples: [(u32, i64); SAMPLES],
    count: usize,
    next: usize,
}

impl PeerClock {
    /// A pong arrived at `now_ms` (our clock), echoing our ping sent at `echo_ms` (our clock,
    /// cut to 32 bits like on the wire), with the answering peer's `clock_ms`. Returns whether it
    /// was kept.
    pub fn sample(&mut self, now_ms: u64, echo_ms: u32, clock_ms: u32) -> bool {
        let rtt = (now_ms as u32).wrapping_sub(echo_ms);
        if rtt > MAX_ROUND_TRIP_MS {
            return false;
        }
        let rtt_f = rtt as f32;
        self.rtt_ms = Some(match self.rtt_ms {
            Some(smooth) => smooth + SMOOTHING * (rtt_f - smooth),
            None => rtt_f,
        });
        let offset = i64::from(clock_ms) + i64::from(rtt / 2) - (now_ms as u32) as i64;
        self.samples[self.next] = (rtt, offset);
        self.next = (self.next + 1) % SAMPLES;
        self.count = (self.count + 1).min(SAMPLES);
        true
    }

    /// The smoothed round trip, in ms.
    pub fn rtt_ms(&self) -> Option<f32> {
        self.rtt_ms
    }

    /// How far ahead of ours the peer's clock is (ms; negative: behind), from the least
    /// disturbed recent sample.
    pub fn offset_ms(&self) -> Option<i64> {
        self.samples[..self.count]
            .iter()
            .min_by_key(|(rtt, _)| *rtt)
            .map(|(_, offset)| *offset)
    }

    /// Our time `now_ms` on the peer's clock.
    pub fn their_time_ms(&self, now_ms: u64) -> Option<u64> {
        self.offset_ms()
            .map(|offset| (now_ms as i64 + offset).max(0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A peer whose clock is 5 000 ms ahead, 40 ms away (20 each way): the pong reads its clock
    /// 20 ms after our ping left and arrives 20 ms later.
    #[test]
    fn a_clean_exchange_gives_the_ping_and_the_offset() {
        let mut clock = PeerClock::default();
        assert_eq!(clock.rtt_ms(), None);
        assert_eq!(clock.offset_ms(), None);
        let sent = 1_000u64;
        assert!(clock.sample(sent + 40, sent as u32, (sent + 20 + 5_000) as u32));
        assert_eq!(clock.rtt_ms(), Some(40.0));
        assert_eq!(clock.offset_ms(), Some(5_000));
        assert_eq!(clock.their_time_ms(2_000), Some(7_000));
    }

    /// The ping is smoothed; the offset comes from the fastest exchange, not the last one.
    #[test]
    fn jitter_is_smoothed_and_the_fastest_sample_sets_the_clock() {
        let mut clock = PeerClock::default();
        let offset = -300i64;
        // (sent, way there, way back): a slow way back skews the offset of that sample.
        let exchanges = [(1_000u64, 20u64, 20u64), (2_000, 20, 120), (3_000, 15, 15)];
        for (sent, there, back) in exchanges {
            let their_clock = (sent + there) as i64 + offset;
            assert!(clock.sample(sent + there + back, sent as u32, their_clock as u32));
        }
        // 40, then 140, then 30: smoothed between them.
        let rtt = clock.rtt_ms().unwrap();
        assert!(rtt > 40.0 && rtt < 140.0, "{rtt}");
        assert_eq!(clock.offset_ms(), Some(offset));
    }

    #[test]
    fn only_the_last_samples_count() {
        let mut clock = PeerClock::default();
        // A very fast exchange long ago, with another offset...
        clock.sample(1_010, 1_000, 1_005 + 100);
        // ...then SAMPLES slower ones with the true offset.
        for i in 0..SAMPLES as u64 {
            let sent = 2_000 + 1_000 * i;
            clock.sample(sent + 50, sent as u32, (sent + 25 + 7) as u32);
        }
        assert_eq!(clock.offset_ms(), Some(7));
    }

    /// Our clock wraps around 32 bits on the wire after 49 days: still the right round trip.
    #[test]
    fn a_late_or_bogus_pong_is_ignored_and_the_wrap_is_fine() {
        let mut clock = PeerClock::default();
        assert!(!clock.sample(50_000, 10_000, 0));
        assert!(!clock.sample(1_000, 2_000, 0), "an echo from the future");
        assert_eq!(clock.rtt_ms(), None);
        // Sent 10 ms before our clock passes 2^32, back 30 ms after.
        let now = u64::from(u32::MAX) + 31;
        assert!(clock.sample(now, u32::MAX - 9, 0));
        assert_eq!(clock.rtt_ms(), Some(40.0));
    }
}
