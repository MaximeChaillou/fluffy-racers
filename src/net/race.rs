//! The race online (issue #9, `docs/ARCHITECTURE.md` § « Course en ligne »), seen from one peer:
//! who races on which grid slot, who has authority over each kart, how far each one got (its
//! checkpoints, as the peer that drives it tells), when the race is over and its final order.
//! Pure logic: no Bevy, no socket; unit-tested.
//!
//! - **Grid** ([`RaceStart`]): the bots on the front slots (0, 1...), the players behind them in
//!   the host's order. A kart's network id ([`KartId`]) is its grid slot.
//! - **Authority**: a player's kart is its player's; a bot is the host's (issue #63: the room's
//!   creator, from start to end; if it leaves, the room is over).
//! - **Checkpoints** ([`Progress`]): the peer that has authority over a kart tells everyone each
//!   checkpoint its kart validates, with its race tick. Every peer gets the same laps, lap times
//!   and finish (`race::RaceProgress::validate`).
//! - **End** ([`SharedRace::should_end`]): the host ends the race when every player still in the
//!   room finished, or `grace` after the first one did (the others are ranked where they are),
//!   or when no player is left. Its final order ([`SharedRace::final_order`]) is everyone's
//!   results ([`RaceOver`]).
//! - **Departures**: a player who leaves keeps its finish if it had one; otherwise it is ranked
//!   last (it gave up), and no longer awaited.
//! - **Results** (issue #70): the host closes them ([`SharedRace::close`]), with every racer's
//!   time (estimated for those still on track, none for those who gave up): everyone shows its
//!   table ([`RaceResults`]).

use super::protocol::{KartId, PeerKey, Progress, RaceOver, RaceResults, RaceStart};
use crate::race::ResultLine;

/// Who drives a kart of the race.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Driver {
    Player(PeerKey),
    /// A bot: the host simulates it.
    Bot,
}

/// A kart of the race, as this peer knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceKart {
    /// Its grid slot, and its id on the network.
    pub kart: KartId,
    pub driver: Driver,
    /// The race tick of each checkpoint it validated, the start line first (the count is
    /// `RaceProgress::passed`).
    pub checkpoints: Vec<u32>,
    /// Its race tick at the finish line.
    pub finish: Option<u32>,
    /// Its player left the room.
    pub left: bool,
}

impl RaceKart {
    /// Checkpoints validated, the start line included.
    pub fn passed(&self) -> u16 {
        self.checkpoints.len() as u16
    }

    pub fn is_bot(&self) -> bool {
        self.driver == Driver::Bot
    }
}

/// The race online, seen from one peer (see the module's documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedRace {
    pub start: RaceStart,
    karts: Vec<RaceKart>,
    /// Checkpoints to validate to finish (`race::Course::checkpoints_to_finish`), once the
    /// circuit is built here.
    to_finish: Option<u16>,
    /// The race tick the first player finished on.
    first_player_finish: Option<u32>,
    over: Option<RaceOver>,
    results: Option<RaceResults>,
}

impl SharedRace {
    pub fn new(start: RaceStart) -> Self {
        let bots = (0..start.bots).map(|slot| RaceKart {
            kart: slot,
            driver: Driver::Bot,
            checkpoints: Vec::new(),
            finish: None,
            left: false,
        });
        let players = start.players.iter().map(|peer| RaceKart {
            kart: start.slot_of(*peer).unwrap_or(0),
            driver: Driver::Player(*peer),
            checkpoints: Vec::new(),
            finish: None,
            left: false,
        });
        let karts = bots.chain(players).collect();
        Self {
            start,
            karts,
            to_finish: None,
            first_player_finish: None,
            over: None,
            results: None,
        }
    }

    /// The race's number in the room.
    pub fn race(&self) -> u16 {
        self.start.race
    }

    pub fn karts(&self) -> &[RaceKart] {
        &self.karts
    }

    pub fn kart(&self, kart: KartId) -> Option<&RaceKart> {
        self.karts.iter().find(|k| k.kart == kart)
    }

    /// Our grid slot, if we race.
    pub fn slot_of(&self, peer: PeerKey) -> Option<KartId> {
        self.start.slot_of(peer)
    }

    /// Checkpoints to validate to finish, once known.
    pub fn to_finish(&self) -> Option<u16> {
        self.to_finish
    }

    /// The circuit is built here: `to_finish` checkpoints make a finish.
    pub fn set_to_finish(&mut self, to_finish: u16) {
        self.to_finish = Some(to_finish.max(1));
        for index in 0..self.karts.len() {
            self.check_finish(index);
        }
    }

    /// The peer that has authority over `kart` (`host`: the host now): its player (while in the
    /// room), or the host for a bot.
    pub fn authority(&self, kart: KartId, host: Option<PeerKey>) -> Option<PeerKey> {
        match self.kart(kart)?.driver {
            Driver::Player(peer) => Some(peer),
            Driver::Bot => host,
        }
    }

    /// `from` tells a checkpoint of one of its karts (`host`: the host now). Kept when it is
    /// this race's, `from` has authority over that kart and it goes further than what we know.
    /// Returns whether it was kept.
    pub fn receive(&mut self, from: PeerKey, host: Option<PeerKey>, progress: &Progress) -> bool {
        if progress.race != self.start.race
            || self.authority(progress.kart, host) != Some(from)
            || self.kart(progress.kart).is_some_and(|k| k.left)
        {
            return false;
        }
        self.record(progress.kart, progress.passed, progress.tick)
    }

    /// One of our karts (or one we were told of) validated its checkpoints up to `passed`, the
    /// last one at race tick `tick`. Returns whether it went further.
    pub fn record(&mut self, kart: KartId, passed: u16, tick: u32) -> bool {
        let Some(index) = self.karts.iter().position(|k| k.kart == kart) else {
            return false;
        };
        let entry = &mut self.karts[index];
        if passed <= entry.passed() || entry.finish.is_some() {
            return false;
        }
        // Reliable and in order, one at a time; a gap (a checkpoint missed: never seen) takes the
        // same tick.
        while entry.passed() < passed {
            entry.checkpoints.push(tick);
        }
        self.check_finish(index);
        true
    }

    fn check_finish(&mut self, index: usize) {
        let Some(to_finish) = self.to_finish else {
            return;
        };
        let entry = &mut self.karts[index];
        if entry.finish.is_none() && entry.passed() >= to_finish {
            let tick = entry.checkpoints[usize::from(to_finish) - 1];
            entry.finish = Some(tick);
            if !entry.is_bot() {
                self.first_player_finish =
                    Some(self.first_player_finish.map_or(tick, |t| t.min(tick)));
            }
        }
    }

    /// `peer` left the room: its kart gives up (unless it finished), and is no longer awaited.
    pub fn leave(&mut self, peer: PeerKey) {
        for kart in &mut self.karts {
            if kart.driver == Driver::Player(peer) {
                kart.left = true;
            }
        }
    }

    /// The players still in the race (not left).
    pub fn players(&self) -> impl Iterator<Item = &RaceKart> {
        self.karts
            .iter()
            .filter(|kart| !kart.is_bot() && !kart.left)
    }

    /// Whether the host should end the race at race tick `tick`: every player still here
    /// finished, or `grace` ticks went by since the first one did, or no player is left.
    pub fn should_end(&self, tick: u32, grace: u32) -> bool {
        if self.over.is_some() {
            return false;
        }
        let mut players = self.players().peekable();
        if players.peek().is_none() {
            return true;
        }
        if players.all(|kart| kart.finish.is_some()) {
            return true;
        }
        self.first_player_finish
            .is_some_and(|first| tick >= first.saturating_add(grace))
    }

    /// The final order, first to last: those who finished by their finish tick; then those still
    /// racing, by checkpoints then by `distance` (the race distance seen here); then those who
    /// gave up. Ties go to the better grid slot.
    pub fn final_order(&self, distance: impl Fn(KartId) -> f32) -> Vec<KartId> {
        let mut finished: Vec<&RaceKart> =
            self.karts.iter().filter(|k| k.finish.is_some()).collect();
        finished.sort_by_key(|k| (k.finish, k.kart));
        let mut racing: Vec<&RaceKart> = self
            .karts
            .iter()
            .filter(|k| k.finish.is_none() && !k.left)
            .collect();
        racing.sort_by(|a, b| {
            b.passed()
                .cmp(&a.passed())
                .then(distance(b.kart).total_cmp(&distance(a.kart)))
                .then(a.kart.cmp(&b.kart))
        });
        let mut gone: Vec<&RaceKart> = self
            .karts
            .iter()
            .filter(|k| k.finish.is_none() && k.left)
            .collect();
        gone.sort_by(|a, b| b.passed().cmp(&a.passed()).then(a.kart.cmp(&b.kart)));
        finished
            .into_iter()
            .chain(racing)
            .chain(gone)
            .map(|k| k.kart)
            .collect()
    }

    /// We, the host, end the race at race tick `tick` with this order.
    pub fn end(&mut self, tick: u32, order: Vec<KartId>) -> RaceOver {
        let over = RaceOver {
            race: self.start.race,
            tick,
            order,
        };
        self.over = Some(over.clone());
        over
    }

    /// The host (`from`, and `host` the host now) ended the race: kept when it is this race's.
    pub fn receive_over(&mut self, from: PeerKey, host: Option<PeerKey>, over: RaceOver) -> bool {
        if over.race != self.start.race || host != Some(from) || self.over.is_some() {
            return false;
        }
        self.over = Some(over);
        true
    }

    pub fn over(&self) -> Option<&RaceOver> {
        self.over.as_ref()
    }

    /// We, the host, close the results with these lines (issue #70): once, after the end.
    pub fn close(&mut self, lines: Vec<ResultLine<KartId>>) -> RaceResults {
        let results = RaceResults {
            race: self.start.race,
            lines,
        };
        self.results = Some(results.clone());
        results
    }

    /// The host (`from`, and `host` the host now) closed the results: kept when they are this
    /// race's, the first ones.
    pub fn receive_results(
        &mut self,
        from: PeerKey,
        host: Option<PeerKey>,
        results: RaceResults,
    ) -> bool {
        if results.race != self.start.race || host != Some(from) || self.results.is_some() {
            return false;
        }
        self.results = Some(results);
        true
    }

    /// The host's closed results, once they came (or once we closed them, hosting).
    pub fn results(&self) -> Option<&RaceResults> {
        self.results.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANA: PeerKey = PeerKey(1 << 100);
    const BOB: PeerKey = PeerKey(2 << 100);
    const CYD: PeerKey = PeerKey(3 << 100);

    /// 9 bots, then Ana, Bob and Cyd; 64 checkpoints to finish (3 laps of 21, and the line).
    fn race() -> SharedRace {
        let mut race = SharedRace::new(RaceStart {
            race: 4,
            seed: 1,
            track: 0,
            laps: 3,
            bots: 9,
            difficulty: 1,
            mirror: false,
            items: true,
            start_tick: 1_000,
            players: vec![ANA, BOB, CYD],
        });
        race.set_to_finish(64);
        race
    }

    fn progress(kart: KartId, passed: u16, tick: u32) -> Progress {
        Progress {
            race: 4,
            kart,
            passed,
            tick,
        }
    }

    /// A kart drives the whole race: `passed` checkpoints, one every `every` ticks from `from`.
    fn drive(
        race: &mut SharedRace,
        from: PeerKey,
        host: PeerKey,
        kart: KartId,
        to: u16,
        every: u32,
    ) {
        let start = race.kart(kart).unwrap().passed();
        for passed in start + 1..=to {
            assert!(race.receive(
                from,
                Some(host),
                &progress(kart, passed, u32::from(passed) * every)
            ));
        }
    }

    /// The grid: the bots in front, the players behind in the host's order; each kart's
    /// authority, the bots the host's (issue #63: was « whoever it is now »; the room's creator,
    /// whoever it is: a host with a bigger id than a player's too).
    #[test]
    fn the_grid_and_who_drives_what() {
        let race = race();
        assert_eq!(race.karts().len(), 12);
        assert_eq!(race.slot_of(ANA), Some(9));
        assert_eq!(race.slot_of(CYD), Some(11));
        assert_eq!(race.slot_of(PeerKey(99)), None);
        assert_eq!(race.authority(3, Some(ANA)), Some(ANA));
        assert_eq!(
            race.authority(3, Some(CYD)),
            Some(CYD),
            "the host's, whatever its id"
        );
        assert_eq!(race.authority(10, Some(ANA)), Some(BOB));
        assert_eq!(race.authority(12, Some(ANA)), None);
    }

    /// Checkpoints are kept from the kart's authority only, of this race, going forward.
    #[test]
    fn only_the_kart_authority_tells_its_checkpoints() {
        let mut race = race();
        assert!(race.receive(BOB, Some(ANA), &progress(10, 1, 5)));
        assert!(
            !race.receive(ANA, Some(ANA), &progress(10, 2, 9)),
            "Bob's kart"
        );
        assert!(
            !race.receive(BOB, Some(ANA), &progress(2, 1, 9)),
            "a bot: the host's"
        );
        assert!(race.receive(ANA, Some(ANA), &progress(2, 1, 9)));
        assert!(
            !race.receive(BOB, Some(ANA), &progress(10, 1, 6)),
            "not further"
        );
        assert!(
            !race.receive(
                BOB,
                Some(ANA),
                &Progress {
                    race: 3,
                    ..progress(10, 2, 7)
                }
            ),
            "another race"
        );
        // A gap (issue #63: was « a bot taken over by a new host »; the host's checkpoints that
        // skip some): filled with the same tick.
        assert!(race.receive(ANA, Some(ANA), &progress(2, 4, 30)));
        assert_eq!(race.kart(2).unwrap().checkpoints, vec![9, 30, 30, 30]);
    }

    /// The finish: the tick of the last checkpoint; the order by finish ticks, ties to the
    /// better slot, then those still racing by checkpoints and distance, then those who left.
    #[test]
    fn the_final_order_comes_from_the_events() {
        let mut race = race();
        // Bob finishes first (a checkpoint every 50 ticks), bot 0 on the same tick, Ana later.
        drive(&mut race, BOB, ANA, 10, 64, 50);
        drive(&mut race, ANA, ANA, 0, 64, 50);
        drive(&mut race, ANA, ANA, 9, 64, 55);
        assert_eq!(race.kart(10).unwrap().finish, Some(3_200));
        assert!(!race.should_end(3_600, 1_800), "Cyd still racing");
        // The others: on their way, bots 1 and 2 at the same checkpoint (bot 2 further).
        drive(&mut race, CYD, ANA, 11, 40, 50);
        drive(&mut race, ANA, ANA, 1, 30, 50);
        drive(&mut race, ANA, ANA, 2, 30, 50);
        let distance = |kart: KartId| if kart == 2 { 700.0 } else { 600.0 };
        let order = race.final_order(distance);
        assert_eq!(&order[..5], &[0, 10, 9, 11, 2]);
        assert_eq!(order[5], 1);
        assert_eq!(order.len(), 12);
        // The same events in any order of arrival give the same order.
        let mut again = SharedRace::new(race.start.clone());
        again.set_to_finish(64);
        for kart in [11, 2, 1, 9, 0, 10] {
            let from = match kart {
                10 => BOB,
                11 => CYD,
                _ => ANA,
            };
            let to = race.kart(kart).unwrap().passed();
            for (n, tick) in race
                .kart(kart)
                .unwrap()
                .checkpoints
                .clone()
                .into_iter()
                .enumerate()
            {
                let _ = to;
                assert!(again.receive(from, Some(ANA), &progress(kart, n as u16 + 1, tick)));
            }
        }
        assert_eq!(again.final_order(distance), order);
    }

    /// The host ends the race when every player still there finished, or `grace` after the
    /// first one; a player who leaves is no longer awaited, and is ranked last unless it had
    /// finished.
    #[test]
    fn the_end_waits_for_the_players_and_a_departure() {
        let mut race = race();
        drive(&mut race, ANA, ANA, 9, 64, 50);
        assert!(!race.should_end(3_200, 1_800));
        assert!(!race.should_end(4_999, 1_800));
        assert!(race.should_end(5_000, 1_800), "30 s after the first player");
        drive(&mut race, BOB, ANA, 10, 64, 52);
        assert!(!race.should_end(3_400, 1_800), "Cyd still racing");
        // Cyd leaves before the end: no longer awaited.
        drive(&mut race, CYD, ANA, 11, 50, 40);
        race.leave(CYD);
        assert!(race.should_end(3_400, 1_800));
        let order = race.final_order(|_| 0.0);
        assert_eq!(&order[..2], &[9, 10]);
        assert_eq!(order[11], 11, "it gave up: last");
        // Its later events are ignored.
        assert!(!race.receive(CYD, Some(ANA), &progress(11, 51, 2_100)));
        // A player who finished, then left, keeps its place.
        let mut race = super::tests::race();
        drive(&mut race, BOB, ANA, 10, 64, 50);
        race.leave(BOB);
        drive(&mut race, ANA, ANA, 9, 64, 60);
        drive(&mut race, CYD, ANA, 11, 64, 70);
        assert!(race.should_end(4_480, 1_800));
        assert_eq!(&race.final_order(|_| 0.0)[..3], &[10, 9, 11]);
        // Nobody left: over.
        let mut empty = super::tests::race();
        for peer in [ANA, BOB, CYD] {
            empty.leave(peer);
        }
        assert!(empty.should_end(0, 1_800));
    }

    /// The end of the race: from the host only, of this race, once.
    #[test]
    fn only_the_host_ends_the_race() {
        let mut race = race();
        let over = RaceOver {
            race: 4,
            tick: 4_000,
            order: (0..12).collect(),
        };
        assert!(!race.receive_over(BOB, Some(ANA), over.clone()));
        assert!(!race.receive_over(
            ANA,
            Some(ANA),
            RaceOver {
                race: 5,
                ..over.clone()
            }
        ));
        assert!(race.receive_over(ANA, Some(ANA), over.clone()));
        assert_eq!(race.over(), Some(&over));
        assert!(!race.should_end(10_000, 0), "already over");
        // The host's own end.
        let mut mine = super::tests::race();
        let sent = mine.end(3_000, vec![9, 10]);
        assert_eq!(sent.race, 4);
        assert_eq!(mine.over(), Some(&sent));
    }

    /// Issue #70: the closed results, like the end: from the host only, of this race, once.
    #[test]
    fn only_the_host_closes_the_results() {
        use crate::race::ResultTime;
        let mut race = race();
        let results = RaceResults {
            race: 4,
            lines: vec![
                ResultLine {
                    id: 9,
                    time: ResultTime::Finished(3_000),
                },
                ResultLine {
                    id: 2,
                    time: ResultTime::Estimated(3_800),
                },
                ResultLine {
                    id: 11,
                    time: ResultTime::GaveUp,
                },
            ],
        };
        assert!(!race.receive_results(BOB, Some(ANA), results.clone()));
        let other_race = RaceResults {
            race: 5,
            ..results.clone()
        };
        assert!(!race.receive_results(ANA, Some(ANA), other_race));
        assert_eq!(race.results(), None);
        assert!(race.receive_results(ANA, Some(ANA), results.clone()));
        assert_eq!(race.results(), Some(&results));
        let again = RaceResults {
            lines: vec![],
            ..results.clone()
        };
        assert!(!race.receive_results(ANA, Some(ANA), again), "once");
        assert_eq!(race.results(), Some(&results));
        // The host's own.
        let mut mine = super::tests::race();
        let sent = mine.close(results.lines.clone());
        assert_eq!(sent, results);
        assert_eq!(mine.results(), Some(&sent));
    }

    /// Issue #63 (was « a new host takes the bots over », issue #9): the bots are the host's
    /// alone. Another player's word on a bot is refused, before and after the host left; the
    /// host's own kart gives up (the room is over for the others: the game stops the race).
    #[test]
    fn the_bots_are_the_hosts_alone() {
        let mut race = race();
        drive(&mut race, ANA, ANA, 3, 20, 50);
        assert!(!race.receive(BOB, Some(ANA), &progress(3, 21, 1_050)));
        // Ana (the host) leaves: nobody takes her bots, nor tells their checkpoints.
        race.leave(ANA);
        assert!(!race.receive(BOB, Some(ANA), &progress(3, 21, 1_050)));
        assert!(!race.receive(CYD, Some(ANA), &progress(3, 21, 1_050)));
        assert_eq!(race.kart(3).unwrap().passed(), 20);
        assert!(race.kart(9).unwrap().left, "her own kart gave up");
        assert_eq!(race.authority(3, Some(ANA)), Some(ANA));
    }
}
