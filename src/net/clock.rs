//! Latency and clock of a peer (issue #7, `docs/ARCHITECTURE.md` § « Synchronisation »), from
//! the ping / pong exchange. Pure logic, unit-tested.
//!
//! - **Ping** (shown in the lobby): the round trip of the last pongs, smoothed.
//! - **Clock**: each peer counts milliseconds from its own start. A pong carries the clock of the
//!   peer that answers; half the round trip later it reads `clock + rtt / 2`, so its offset from
//!   ours is `clock + rtt / 2 - now`. The sample with the shortest round trip among the last
//!   [`SAMPLES`] is the least disturbed by the network: its offset is kept. Every peer can then
//!   speak in the host's time (`Lobby::host_time_ms`), from which issue #9 counts race ticks.
//! - **Room clock** (issue #9, [`RoomClock`]): the time every peer of a room speaks in, the race
//!   ticks included. It is the host's clock, but it survives a new host: a pong carries the
//!   room's time of the peer that answers when it knows it (its own time otherwise, flagged), so
//!   a new host goes on with the room's time it already knew, and a newcomer that becomes host at
//!   once (a smaller id) first learns it from the others. Nobody's clock jumps when the host
//!   changes: only the small error of each estimate (a few ms, under the 2 ticks the common tick
//!   ignores, `interp::TickClock`).

/// Samples kept to choose the clock offset from.
pub const SAMPLES: usize = 8;

/// A pong older than this is a lost ping's late echo, or garbage: ignored (ms).
pub const MAX_ROUND_TRIP_MS: u32 = 10_000;

/// Weight of a new round trip in the smoothed ping.
const SMOOTHING: f32 = 0.25;

/// A new host that does not know the room's clock waits this long at most for the others to
/// tell it (their pongs), then starts the room's clock from its own (ms).
pub const ROOM_WAIT_MS: u64 = 2_000;

/// What the pongs of one peer told us.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PeerClock {
    /// Smoothed round trip (ms); `None` before the first pong.
    rtt_ms: Option<f32>,
    /// The last samples: (round trip, offset of the peer's clock from ours), ms.
    samples: [(u32, i64); SAMPLES],
    count: usize,
    next: usize,
    /// The samples carry the room's clock of that peer (issue #9), not its own.
    room: bool,
}

impl PeerClock {
    /// A pong arrived at `now_ms` (our clock), echoing our ping sent at `echo_ms` (our clock,
    /// cut to 32 bits like on the wire), with the answering peer's `clock_ms`: its room's clock
    /// when `room` (issue #9), its own otherwise. Returns whether it was kept. A peer that starts
    /// (or stops) telling the room's clock starts its samples over: the two clocks differ.
    pub fn sample(&mut self, now_ms: u64, echo_ms: u32, clock_ms: u32, room: bool) -> bool {
        let rtt = (now_ms as u32).wrapping_sub(echo_ms);
        if rtt > MAX_ROUND_TRIP_MS {
            return false;
        }
        if room != self.room {
            self.room = room;
            self.count = 0;
            self.next = 0;
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

    /// Its pongs carry the room's clock (issue #9), and one arrived.
    pub fn knows_room(&self) -> bool {
        self.room && self.count > 0
    }

    /// It answered at least one ping.
    pub fn answered(&self) -> bool {
        self.count > 0
    }

    /// The fastest recent round trip (ms).
    pub fn best_rtt_ms(&self) -> Option<u32> {
        self.samples[..self.count].iter().map(|(rtt, _)| *rtt).min()
    }
}

/// The room's clock, seen from one peer (issue #9, see the module's documentation): its offset
/// from our own clock. Pure logic: the lobby feeds it ([`RoomClock::update`]) with the clocks of
/// the host and of the others, as their pongs told them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoomClock {
    /// The room's time minus our own (ms), once known.
    offset: Option<i64>,
    /// We host and do not know the room's clock: since when we wait for the others (ms).
    waiting_since: Option<u64>,
}

impl RoomClock {
    /// The room's time minus ours (ms), once known.
    pub fn offset_ms(&self) -> Option<i64> {
        self.offset
    }

    /// The room's time at our `now_ms`, once known.
    pub fn time_ms(&self, now_ms: u64) -> Option<u64> {
        self.offset
            .map(|offset| (now_ms as i64 + offset).max(0) as u64)
    }

    /// What our pong says (cut to 32 bits like on the wire): the room's time when we know it
    /// (`true`), our own otherwise.
    pub fn pong(&self, now_ms: u64) -> (u32, bool) {
        match self.time_ms(now_ms) {
            Some(room) => (room as u32, true),
            None => (now_ms as u32, false),
        }
    }

    /// Once a frame, at `now_ms`: `we_host`; `host` is the host's clock when another peer
    /// hosts (`None` when we do, or nobody is seated yet); `others`: every other peer's clock (the
    /// host's included).
    ///
    /// - Another peer hosts and tells the room's clock: we follow it.
    /// - Otherwise (we host, or the host does not know it yet: a newcomer that just became host),
    ///   we keep what we knew. Not knowing it, we take it from the peer that tells it with the
    ///   fastest round trip; nobody tells it and we host: once every peer answered (or after
    ///   [`ROOM_WAIT_MS`]), the room's clock starts from ours.
    pub fn update<'a>(
        &mut self,
        now_ms: u64,
        we_host: bool,
        host: Option<&PeerClock>,
        others: impl Iterator<Item = &'a PeerClock> + Clone,
    ) {
        if let Some(host) = host.filter(|host| host.knows_room()) {
            self.offset = host.offset_ms();
            self.waiting_since = None;
            return;
        }
        if self.offset.is_some() {
            self.waiting_since = None;
            return;
        }
        let told = others
            .clone()
            .filter(|clock| clock.knows_room())
            .min_by_key(|clock| clock.best_rtt_ms());
        if let Some(clock) = told {
            self.offset = clock.offset_ms();
            self.waiting_since = None;
            return;
        }
        if !we_host {
            self.waiting_since = None;
            return;
        }
        let since = *self.waiting_since.get_or_insert(now_ms);
        let mut others = others;
        if others.all(PeerClock::answered) || now_ms >= since + ROOM_WAIT_MS {
            self.offset = Some(0);
            self.waiting_since = None;
        }
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
        assert!(clock.sample(sent + 40, sent as u32, (sent + 20 + 5_000) as u32, false));
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
            assert!(clock.sample(sent + there + back, sent as u32, their_clock as u32, false));
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
        clock.sample(1_010, 1_000, 1_005 + 100, false);
        // ...then SAMPLES slower ones with the true offset.
        for i in 0..SAMPLES as u64 {
            let sent = 2_000 + 1_000 * i;
            clock.sample(sent + 50, sent as u32, (sent + 25 + 7) as u32, false);
        }
        assert_eq!(clock.offset_ms(), Some(7));
    }

    /// Our clock wraps around 32 bits on the wire after 49 days: still the right round trip.
    #[test]
    fn a_late_or_bogus_pong_is_ignored_and_the_wrap_is_fine() {
        let mut clock = PeerClock::default();
        assert!(!clock.sample(50_000, 10_000, 0, false));
        assert!(
            !clock.sample(1_000, 2_000, 0, false),
            "an echo from the future"
        );
        assert_eq!(clock.rtt_ms(), None);
        // Sent 10 ms before our clock passes 2^32, back 30 ms after.
        let now = u64::from(u32::MAX) + 31;
        assert!(clock.sample(now, u32::MAX - 9, 0, false));
        assert_eq!(clock.rtt_ms(), Some(40.0));
    }

    /// A machine of the tests: its clock reads `origin + t × (1 + drift)` at the true time `t`
    /// (ms).
    #[derive(Clone, Copy, Debug)]
    struct Machine {
        origin: f64,
        drift: f64,
    }

    impl Machine {
        fn new(origin: f64) -> Self {
            Self { origin, drift: 0.0 }
        }

        fn at(&self, t: f64) -> u64 {
            (self.origin + t * (1.0 + self.drift)).round() as u64
        }
    }

    /// `us` pings `them` at the true time `t`: the ping takes `there` ms, the pong `back` ms.
    /// `room`: what `them` knows of the room's clock (its pong carries it).
    fn exchange(
        clock: &mut PeerClock,
        us: Machine,
        them: Machine,
        room: Option<&RoomClock>,
        t: f64,
        (there, back): (f64, f64),
    ) {
        let answered_at = them.at(t + there);
        let (reading, knows) = match room {
            Some(room) => room.pong(answered_at),
            None => (answered_at as u32, false),
        };
        clock.sample(us.at(t + there + back), us.at(t) as u32, reading, knows);
    }

    /// A little generator for the jitter (xorshift).
    struct Jitter(u64);

    impl Jitter {
        /// 0 to `max` ms.
        fn next(&mut self, max: f64) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % 10_000) as f64 / 10_000.0 * max
        }
    }

    /// Issue #9: a path slower one way than the other skews the offset by half the difference:
    /// the pong is assumed to come back as fast as the ping went. Nothing can see it (the round
    /// trip is all there is); it stays within half the round trip.
    #[test]
    fn an_asymmetric_path_errs_by_half_its_difference() {
        let (us, them) = (Machine::new(0.0), Machine::new(5_000.0));
        for (there, back) in [(30.0, 10.0), (10.0, 30.0), (60.0, 0.0), (20.0, 20.0)] {
            let mut clock = PeerClock::default();
            exchange(&mut clock, us, them, None, 1_000.0, (there, back));
            let error = clock.offset_ms().unwrap() - 5_000;
            assert_eq!(error as f64, (there - back) / 2.0, "{there} / {back}");
            assert!(error.unsigned_abs() as f64 <= (there + back) / 2.0);
        }
    }

    /// Issue #9: with jitter (20 to 50 ms each way), the offset comes from the fastest of the
    /// last 8 exchanges: its error is at most half of what that exchange took beyond the fastest
    /// possible one: never more than half the jitter, a few ms on average.
    #[test]
    fn jitter_keeps_the_offset_within_a_few_ms() {
        let (us, them) = (Machine::new(20_000.0), Machine::new(7_655.0));
        let mut clock = PeerClock::default();
        let mut jitter = Jitter(0x5eed);
        let (mut worst, mut sum, mut count) = (0i64, 0i64, 0i64);
        for ping in 0..200 {
            let t = 1_000.0 * f64::from(ping);
            let ways = (20.0 + jitter.next(30.0), 20.0 + jitter.next(30.0));
            exchange(&mut clock, us, them, None, t, ways);
            let error = (clock.offset_ms().unwrap() + 12_345).abs();
            let excess = f64::from(clock.best_rtt_ms().unwrap()) - 40.0;
            assert!(
                error as f64 <= excess / 2.0 + 1.0,
                "ping {ping}: {error} ms"
            );
            if ping >= SAMPLES as u32 {
                worst = worst.max(error);
                sum += error;
                count += 1;
            }
        }
        let mean = sum as f64 / count as f64;
        println!("offset error with 20 to 50 ms each way: mean {mean:.1} ms, worst {worst} ms");
        // Never beyond half the jitter (15 ms), a few ms on average.
        assert!(worst <= 15, "{worst} ms");
        assert!(mean <= 5.0, "{mean} ms");
    }

    /// Issue #9: the estimate follows the host's clock when it drifts (100 ppm: 6 ms a
    /// minute), and recalibrates within 8 exchanges when it jumps (400 ms). The common tick
    /// (`interp::TickClock`, 2 ticks of hysteresis) ignores the drift and the jitter, and moves
    /// once for the jump.
    #[test]
    fn the_estimate_follows_a_drift_and_recalibrates_after_a_jump() {
        let us = Machine::new(0.0);
        let mut host = Machine {
            origin: 70_000.0,
            drift: 100e-6,
        };
        let mut clock = PeerClock::default();
        let mut ticks = crate::net::TickClock::default();
        let mut moves = 0;
        let mut jitter = Jitter(42);
        let mut since_jump = None;
        for ping in 0..180 {
            let t = 1_000.0 * f64::from(ping);
            if ping == 120 {
                host.origin += 400.0;
                since_jump = Some(0);
            }
            let ways = (20.0 + jitter.next(10.0), 20.0 + jitter.next(10.0));
            exchange(&mut clock, us, host, None, t, ways);
            // Where the host's clock really is now, from ours.
            let truth = host.at(t) as i64 - us.at(t) as i64;
            let error = (clock.offset_ms().unwrap() - truth).abs();
            match since_jump.as_mut() {
                None if ping >= SAMPLES as u32 => assert!(error <= 6, "ping {ping}: {error}"),
                Some(count) => {
                    *count += 1;
                    if *count >= SAMPLES {
                        assert!(error <= 6, "{count} after the jump: {error} ms");
                    }
                }
                None => {}
            }
            let local_tick = t / (1_000.0 / 60.0);
            let common = (us.at(t) as i64 + clock.offset_ms().unwrap()) as f64 / (1_000.0 / 60.0);
            if ticks.observe(common - local_tick) && ping > 0 {
                moves += 1;
            }
        }
        assert_eq!(moves, 1, "the common tick moved {moves} times");
    }

    /// Three peers of a room, every one pinging every other once a second.
    struct Room {
        machines: Vec<Machine>,
        /// `clocks[a][b]`: what `a` knows of `b`'s clock.
        clocks: Vec<Vec<PeerClock>>,
        rooms: Vec<RoomClock>,
        here: Vec<bool>,
        host: usize,
        jitter: Jitter,
    }

    impl Room {
        fn new(origins: &[f64]) -> Self {
            let n = origins.len();
            Self {
                machines: origins.iter().map(|o| Machine::new(*o)).collect(),
                clocks: vec![vec![PeerClock::default(); n]; n],
                rooms: vec![RoomClock::default(); n],
                here: vec![true; n],
                host: 0,
                jitter: Jitter(7),
            }
        }

        /// One second: the pings, then every peer's room clock.
        fn second(&mut self, t: f64) {
            let n = self.machines.len();
            for a in 0..n {
                for b in 0..n {
                    if a == b || !self.here[a] || !self.here[b] {
                        continue;
                    }
                    let ways = (15.0 + self.jitter.next(10.0), 15.0 + self.jitter.next(10.0));
                    let room = self.rooms[b];
                    let (us, them) = (self.machines[a], self.machines[b]);
                    exchange(&mut self.clocks[a][b], us, them, Some(&room), t, ways);
                }
            }
            self.update(t + 100.0);
        }

        fn update(&mut self, t: f64) {
            for a in 0..self.machines.len() {
                if !self.here[a] {
                    continue;
                }
                let we_host = a == self.host;
                let host = (!we_host).then(|| &self.clocks[a][self.host]);
                let others = self.clocks[a]
                    .iter()
                    .enumerate()
                    .filter(|(b, _)| *b != a && self.here[*b])
                    .map(|(_, clock)| clock);
                let now = self.machines[a].at(t);
                self.rooms[a].update(now, we_host, host, others);
            }
        }

        /// Every present peer's room time at the true time `t`.
        fn times(&self, t: f64) -> Vec<Option<i64>> {
            (0..self.machines.len())
                .filter(|a| self.here[*a])
                .map(|a| {
                    self.rooms[a]
                        .time_ms(self.machines[a].at(t))
                        .map(|ms| ms as i64)
                })
                .collect()
        }
    }

    /// Issue #9: the host leaves; the next one goes on with the room's clock it knew, the
    /// others follow it. At every moment the peers agree within a few ms, and nobody's room
    /// time jumps (15 to 25 ms each way).
    #[test]
    fn the_room_clock_survives_a_new_host() {
        let mut room = Room::new(&[10_000.0, 3_000.0, 50_000.0]);
        let mut previous: Option<Vec<Option<i64>>> = None;
        for second in 0..40 {
            let t = 1_000.0 * f64::from(second);
            if second == 20 {
                // The host's tab closes; peer 1 hosts now.
                room.here[0] = false;
                room.host = 1;
                previous = previous.map(|times| times[1..].to_vec());
            }
            room.second(t);
            let times = room.times(t + 500.0);
            // Once each peer has its 8 samples of the room's clock.
            if second > SAMPLES as u32 {
                let known: Vec<i64> = times.iter().map(|t| t.expect("unknown")).collect();
                let spread = known.iter().max().unwrap() - known.iter().min().unwrap();
                assert!(spread <= 6, "second {second}: {known:?}");
                // The room's clock is the first host's.
                let truth = room.machines[0].at(t + 500.0) as i64;
                for time in &known {
                    assert!(
                        (time - truth).abs() <= 6,
                        "second {second}: {time} vs {truth}"
                    );
                }
                if let Some(before) = &previous {
                    for (now, then) in known.iter().zip(before) {
                        // An estimate moves by a few ms (the jitter is 10 ms); a host change
                        // that lost the room's clock would jump by seconds (the clocks start 7
                        // to 47 s apart).
                        let step = now - then.unwrap();
                        assert!(
                            (step - 1_000).abs() <= 10,
                            "second {second}: a jump of {step}"
                        );
                    }
                }
            }
            previous = Some(times);
        }
    }

    /// Issue #9: a newcomer with a smaller id becomes host before any pong reached it: it waits
    /// for the others to tell it the room's clock rather than start a new one (the race's ticks
    /// would jump), then goes on with it. Alone, a host starts the room's clock at once; nobody
    /// answering, after `ROOM_WAIT_MS`.
    #[test]
    fn a_new_host_learns_the_room_clock_before_it_counts() {
        let (old, newcomer) = (Machine::new(8_000.0), Machine::new(100.0));
        let mut old_room = RoomClock::default();
        old_room.update(old.at(0.0), true, None, std::iter::empty());
        assert_eq!(
            old_room.offset_ms(),
            Some(0),
            "alone: the room's clock is its own"
        );
        let mut of_old = PeerClock::default();
        let mut room = RoomClock::default();
        room.update(newcomer.at(10.0), true, None, [&of_old].into_iter());
        assert_eq!(room.time_ms(newcomer.at(10.0)), None, "it waits");
        assert_eq!(room.pong(newcomer.at(10.0)), (110, false));
        exchange(
            &mut of_old,
            newcomer,
            old,
            Some(&old_room),
            10.0,
            (20.0, 20.0),
        );
        room.update(newcomer.at(60.0), true, None, [&of_old].into_iter());
        let read = room.time_ms(newcomer.at(60.0)).unwrap() as i64;
        assert!((read - old.at(60.0) as i64).abs() <= 1, "{read}");
        assert!(room.pong(newcomer.at(60.0)).1);
        // Nobody answers: after a while, its own clock.
        let mut lonely = RoomClock::default();
        let silent = PeerClock::default();
        lonely.update(5_000, true, None, [&silent].into_iter());
        lonely.update(5_000 + ROOM_WAIT_MS - 1, true, None, [&silent].into_iter());
        assert_eq!(lonely.offset_ms(), None);
        lonely.update(5_000 + ROOM_WAIT_MS, true, None, [&silent].into_iter());
        assert_eq!(lonely.offset_ms(), Some(0));
        // A host that does not tell the room's clock yet: the others keep theirs.
        let mut follower = RoomClock::default();
        let mut of_host = PeerClock::default();
        exchange(
            &mut of_host,
            newcomer,
            old,
            Some(&old_room),
            0.0,
            (10.0, 10.0),
        );
        follower.update(200, false, Some(&of_host), [&of_host].into_iter());
        let known = follower.offset_ms();
        assert!(known.is_some());
        let mut of_new_host = PeerClock::default();
        exchange(&mut of_new_host, old, newcomer, None, 0.0, (10.0, 10.0));
        follower.update(300, false, Some(&of_new_host), [&of_new_host].into_iter());
        assert_eq!(follower.offset_ms(), known);
    }
}
