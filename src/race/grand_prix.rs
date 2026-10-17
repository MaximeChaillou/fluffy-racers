//! The Grand Prix (GD § 1, issue #19), pure: no ECS, unit-tested. The systems that run it (the
//! next race, the screens, the podium) are in `crate::modes`.
//!
//! - **Four races** on the circuits of the cup (the first four of `assets/tracks/index.ron`), in
//!   its order, in the mirror mode chosen for the whole Grand Prix.
//! - **Points** (GD § 1, MK8): 15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1 from the 1st to the 12th
//!   ([`GrandPrixTuning::points`]), from the closed results of each race (issue #70: the racers
//!   still on track get their estimated place).
//! - **Ties** (the GD says nothing; our choice, like the karting games of reference): at equal
//!   points, the better place in the **last race run** comes first (then the race before, and so
//!   on). Every racer has a place in every race, so two racers are never tied after that.
//! - **The grid of the next race** (the GD says nothing; MK8DX's rule): the reverse of the overall
//!   standings, the leader last ([`GrandPrix::next_lineup`]). The first race lines up as any race:
//!   the bots in front, the player last.
//! - **Entrants**: the racers of the whole Grand Prix (`race::RaceGrid`): entrants `0..bots` are
//!   the bots (each keeps its character, parts, color and name from one race to the next),
//!   entrant `bots` the player.
//!
//! Issue #99: « Championnat » on the screens; its races are a cup's (four circuits of the
//! manifest) or a custom championship's (its races chosen or drawn: `modes::setup`), the same
//! rules for both.
//!
//! Issue #122: **online**, the host carries the championship (`net::championship`): it scores
//! the races with this logic (several players among the entrants, [`GrandPrix::with_entrants`];
//! a player who left wins nothing, [`GrandPrix::score_without`]) and tells everyone; every peer
//! shows what it told ([`GrandPrix::online`]: the names, the standings, our entrant), the screens
//! and the podium of `crate::modes` as in solo, without its keys.

use bevy::prelude::*;

use crate::track::GRID_SLOTS;

/// The Grand Prix's knobs (GD § 1). Reflected for the admin panel (issue #36).
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct GrandPrixTuning {
    /// Races of a cup (GD § 1: 4): issue #99, the cups of the manifest take its circuits so many
    /// at a time, in its order (`modes::cups`); a custom championship starts with as many races.
    /// Read when the « Jouer solo » screen opens and when a championship starts.
    pub races: u32,
    /// Points of each place, from the 1st (GD § 1: 15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1).
    pub points: [u32; GRID_SLOTS],
}

impl Default for GrandPrixTuning {
    fn default() -> Self {
        Self {
            races: 4,
            points: [15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
        }
    }
}

/// The points of `place` (0 = first) with the table `points`: none beyond it.
pub fn points_for(points: &[u32], place: usize) -> u32 {
    points.get(place).copied().unwrap_or(0)
}

/// Issue #99: the name of a championship that is not a cup's.
pub const CHAMPIONSHIP: &str = "Championnat";

/// A circuit of the cup: its name and its file (an asset path).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CupRace {
    pub name: String,
    pub path: String,
}

/// Where the Grand Prix is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GrandPrixStage {
    /// A race of the cup is on (its results included, until the player moves on).
    #[default]
    Racing,
    /// The overall standings after a race, before the next one.
    Standings,
    /// After the last race: the podium.
    Podium,
}

/// Issue #122: a championship online, as this peer shows it: what the host told (see the
/// module's documentation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnlineStandings {
    /// Every entrant's name: its player's pseudo, « Kart N » for a bot.
    pub names: Vec<String>,
    /// Our entrant (`None`: we watch it, seated after its start).
    pub me: Option<usize>,
    /// The overall standings the host told: entrants, first to last.
    pub standings: Vec<usize>,
    /// We host it: Entrée moves it on.
    pub hosting: bool,
}

/// A Grand Prix in progress (solo): issue #99, « Championnat » on the screens. Exists from its
/// start to the next choice of mode. Issue #122: online too, as the host tells it.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct GrandPrix {
    /// Issue #99: its name on its screens: the cup's (« Coupe Doudou »), or « Championnat » (a
    /// custom one).
    pub title: String,
    /// The circuits, in order: a cup's, or (issue #99) those of a custom championship.
    pub cup: Vec<CupRace>,
    /// Racers of the whole Grand Prix: the bots then the player (see the module's documentation).
    pub entrants: usize,
    /// The race being run (0 = the first): an index of `cup`.
    pub race: usize,
    pub stage: GrandPrixStage,
    /// Each race run, scored: its finishing order (entrants, first to last).
    pub orders: Vec<Vec<usize>>,
    /// Total points of each entrant.
    pub points: Vec<u32>,
    /// Points of each entrant in the last race scored.
    pub last_points: Vec<u32>,
    /// The player asked to move on before the results closed: the standings come once the race
    /// is scored.
    pub advance_when_scored: bool,
    /// Issue #122: online, what the host told (`None`: solo, this peer runs it).
    pub online: Option<OnlineStandings>,
}

impl GrandPrix {
    /// A new Grand Prix on `cup` with `bots` bots and the player.
    pub fn new(cup: Vec<CupRace>, bots: usize) -> Self {
        Self::with_entrants(cup, bots + 1)
    }

    /// Issue #122: a new Grand Prix on `cup` with `entrants` racers (online: the bots, then the
    /// players).
    pub fn with_entrants(cup: Vec<CupRace>, entrants: usize) -> Self {
        let entrants = entrants.clamp(1, GRID_SLOTS);
        Self {
            title: CHAMPIONSHIP.to_string(),
            cup,
            entrants,
            race: 0,
            stage: GrandPrixStage::Racing,
            orders: Vec::new(),
            points: vec![0; entrants],
            last_points: vec![0; entrants],
            advance_when_scored: false,
            online: None,
        }
    }

    /// The player's entrant (after the bots).
    pub fn player(&self) -> usize {
        self.entrants - 1
    }

    /// Whether `entrant` is our player: solo, the one after the bots; issue #122, online, ours
    /// (none when we watch).
    pub fn is_player(&self, entrant: usize) -> bool {
        match &self.online {
            Some(online) => online.me == Some(entrant),
            None => entrant == self.player(),
        }
    }

    /// Whether the race being run has been scored.
    pub fn scored(&self) -> bool {
        self.orders.len() > self.race
    }

    /// Whether the race being run is the last one.
    pub fn last_race(&self) -> bool {
        self.race + 1 >= self.cup.len()
    }

    /// Scores the race being run (once): `order` is its finishing order, entrants first to last
    /// (from the closed results). An entrant missing from it (it should not be) comes last, in
    /// its number's order; an unknown or repeated one is ignored.
    pub fn score(&mut self, order: &[usize], points: &[u32]) {
        self.score_without(order, &[], points);
    }

    /// Issue #122: scores the race being run like [`GrandPrix::score`], the entrants `out` (online:
    /// the players who left, before or during it) last and **without points** (they gave up),
    /// in their number's order. Online only: a race is never run again there (no points to take
    /// back, [`GrandPrix::restart_race`]).
    pub fn score_without(&mut self, order: &[usize], out: &[usize], points: &[u32]) {
        if self.scored() {
            return;
        }
        let mut full: Vec<usize> = Vec::with_capacity(self.entrants);
        for entrant in order {
            if *entrant < self.entrants && !full.contains(entrant) && !out.contains(entrant) {
                full.push(*entrant);
            }
        }
        let racing = (0..self.entrants).filter(|entrant| !out.contains(entrant));
        let missing: Vec<usize> = racing.filter(|entrant| !full.contains(entrant)).collect();
        full.extend(missing);
        let scoring = full.len();
        full.extend((0..self.entrants).filter(|entrant| out.contains(entrant)));
        self.last_points.fill(0);
        for (place, entrant) in full.iter().enumerate().take(scoring) {
            let won = points_for(points, place);
            self.points[*entrant] += won;
            self.last_points[*entrant] = won;
        }
        self.orders.push(full);
    }

    /// The race being run starts over (the player restarted it: « Relancer », a new kart): its
    /// score, if it had one, is taken back (`points`: the table it was scored with), and the
    /// Grand Prix is racing it again.
    pub fn restart_race(&mut self, points: &[u32]) {
        if self.scored()
            && let Some(order) = self.orders.pop()
        {
            for (place, entrant) in order.iter().enumerate() {
                self.points[*entrant] -= points_for(points, place);
            }
        }
        self.last_points.fill(0);
        if let Some(order) = self.orders.last() {
            for (place, entrant) in order.iter().enumerate() {
                self.last_points[*entrant] = points_for(points, place);
            }
        }
        self.stage = GrandPrixStage::Racing;
        self.advance_when_scored = false;
    }

    /// The place of `entrant` (0 = first) in race `race`, if it was scored.
    pub fn place_in(&self, race: usize, entrant: usize) -> Option<usize> {
        self.orders.get(race)?.iter().position(|e| *e == entrant)
    }

    /// The overall standings: entrants from first to last, by points; at equal points, the better
    /// place in the last race run first, then in the race before (see the module's
    /// documentation), then the entrant's number.
    pub fn standings(&self) -> Vec<usize> {
        // Issue #122: online, the host's.
        if let Some(online) = &self.online {
            return online.standings.clone();
        }
        let mut entrants: Vec<usize> = (0..self.entrants).collect();
        entrants.sort_by(|a, b| {
            self.points[*b]
                .cmp(&self.points[*a])
                .then_with(|| {
                    for race in (0..self.orders.len()).rev() {
                        let order = self.place_in(race, *a).cmp(&self.place_in(race, *b));
                        if order.is_ne() {
                            return order;
                        }
                    }
                    std::cmp::Ordering::Equal
                })
                .then(a.cmp(b))
        });
        entrants
    }

    /// The grid of the next race (GD § 1, MK8DX): the reverse of the overall standings, the
    /// leader on the last slot (`lineup[slot]` = the entrant there; the slots past the entrants
    /// keep their own number).
    pub fn next_lineup(&self) -> [u8; GRID_SLOTS] {
        let mut lineup: [u8; GRID_SLOTS] = std::array::from_fn(|slot| slot as u8);
        for (slot, entrant) in self.standings().iter().rev().enumerate() {
            lineup[slot] = *entrant as u8;
        }
        lineup
    }

    /// Moves on to the next race of the cup (its grid: [`GrandPrix::next_lineup`]); false after
    /// the last one.
    pub fn next_race(&mut self) -> bool {
        if self.last_race() {
            return false;
        }
        self.race += 1;
        self.stage = GrandPrixStage::Racing;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cup(races: usize) -> Vec<CupRace> {
        (0..races)
            .map(|i| CupRace {
                name: format!("Circuit {i}"),
                path: format!("tracks/{i}.track.ron"),
            })
            .collect()
    }

    /// GD § 1: 15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1 from the 1st to the 12th; 4 races.
    #[test]
    fn the_points_follow_the_game_design() {
        let tuning = GrandPrixTuning::default();
        assert_eq!(tuning.races, 4);
        let all: Vec<u32> = (0..12).map(|p| points_for(&tuning.points, p)).collect();
        assert_eq!(all, [15, 12, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(points_for(&tuning.points, 12), 0);
        assert_eq!(all.iter().sum::<u32>(), 82);
    }

    /// One race scored: each entrant gets the points of its place, once; the race is scored only
    /// once (a second call changes nothing).
    #[test]
    fn a_race_gives_the_points_of_each_place_once() {
        let points = GrandPrixTuning::default().points;
        let mut gp = GrandPrix::new(cup(4), 11);
        assert_eq!((gp.entrants, gp.player()), (12, 11));
        assert!(!gp.scored());
        let order: Vec<usize> = vec![11, 3, 0, 1, 2, 4, 5, 6, 7, 8, 9, 10];
        gp.score(&order, &points);
        assert!(gp.scored());
        assert_eq!(gp.points[11], 15);
        assert_eq!(gp.points[3], 12);
        assert_eq!(gp.points[0], 10);
        assert_eq!(gp.points[10], 1);
        assert_eq!(gp.last_points, gp.points);
        gp.score(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], &points);
        assert_eq!(gp.points[11], 15, "scored once");
        assert_eq!(gp.standings()[..3], [11, 3, 0]);
        assert_eq!(gp.place_in(0, 3), Some(1));
        assert_eq!(gp.place_in(1, 3), None);
    }

    /// The overall standings over the four races of a Grand Prix: the sums of the points, in
    /// order; the grid of each next race is the reverse of the standings (the leader last).
    #[test]
    fn four_races_make_the_overall_standings() {
        let points = GrandPrixTuning::default().points;
        let mut gp = GrandPrix::new(cup(4), 11);
        // The player (11) wins twice, 3 wins the other two; 0 is always 3rd.
        let races: [[usize; 12]; 4] = [
            [11, 3, 0, 1, 2, 4, 5, 6, 7, 8, 9, 10],
            [3, 11, 0, 2, 1, 4, 5, 6, 7, 8, 10, 9],
            [11, 3, 0, 1, 2, 5, 4, 6, 7, 8, 9, 10],
            [3, 1, 0, 11, 2, 4, 5, 6, 7, 8, 9, 10],
        ];
        for (i, order) in races.iter().enumerate() {
            assert_eq!(gp.race, i);
            gp.score(order, &points);
            if i < 3 {
                let lineup = gp.next_lineup();
                let standings = gp.standings();
                // The leader starts last, the last one on the pole.
                assert_eq!(lineup[11] as usize, standings[0]);
                assert_eq!(lineup[0] as usize, standings[11]);
                // A permutation of the 12 entrants.
                let mut sorted = lineup;
                sorted.sort();
                assert_eq!(sorted, std::array::from_fn::<u8, 12, _>(|i| i as u8));
                gp.stage = GrandPrixStage::Standings;
                assert!(gp.next_race());
                assert_eq!(gp.stage, GrandPrixStage::Racing);
            } else {
                assert!(gp.last_race());
                assert!(!gp.next_race());
            }
        }
        // 11: 15 + 12 + 15 + 9 = 51; 3: 12 + 15 + 12 + 15 = 54; 0: 4 × 10 = 40; 1: 9 + 8 + 9 + 12.
        assert_eq!(gp.points[11], 51);
        assert_eq!(gp.points[3], 54);
        assert_eq!(gp.points[0], 40);
        assert_eq!(gp.points[1], 38);
        assert_eq!(gp.standings()[..4], [3, 11, 0, 1]);
        assert_eq!(gp.points.iter().sum::<u32>(), 4 * 82);
        assert_eq!(gp.last_points[1], 12);
    }

    /// Ties (our rule): at equal points, the better place in the last race comes first; if they
    /// tied there too (not in the same race: a missing race), the race before decides, then the
    /// entrant's number.
    #[test]
    fn ties_go_to_the_better_place_in_the_last_race() {
        let points = GrandPrixTuning::default().points;
        let mut gp = GrandPrix::new(cup(4), 11);
        // Race 1: 5 first (15), 6 second (12).
        gp.score(&[5, 6, 0, 1, 2, 3, 4, 7, 8, 9, 10, 11], &points);
        gp.next_race();
        // Race 2: 6 first (15), 5 second (12): 27 points each; 6 was better in the last race.
        gp.score(&[6, 5, 0, 1, 2, 3, 4, 7, 8, 9, 10, 11], &points);
        assert_eq!(gp.points[5], gp.points[6]);
        let standings = gp.standings();
        assert_eq!(standings[..2], [6, 5]);
        // The reverse grid follows the tie-break: 6 (the leader) starts last, 5 before it.
        let lineup = gp.next_lineup();
        assert_eq!((lineup[11], lineup[10]), (6, 5));
        gp.next_race();
        // Race 3: 5 first, 6 second: 5 leads by 3 points.
        gp.score(&[5, 6, 0, 1, 2, 3, 4, 7, 8, 9, 10, 11], &points);
        assert_eq!(gp.standings()[..2], [5, 6]);
        gp.next_race();
        // Race 4: 6 first (15), 5 second (12): 54 each again; 6 won the last race.
        gp.score(&[6, 5, 0, 1, 2, 3, 4, 7, 8, 9, 10, 11], &points);
        assert_eq!(gp.points[5], 54);
        assert_eq!(gp.points[6], 54);
        assert_eq!(gp.standings()[..2], [6, 5]);
        // The others by their points: 0 (3rd every time, 40) ahead of 1 (4th, 36).
        assert_eq!(gp.standings()[2..4], [0, 1]);
        assert_eq!((gp.points[0], gp.points[1]), (40, 36));

        // A tie in the last race itself cannot happen with full orders; with a missing race, the
        // entrant's number decides last.
        let mut empty = GrandPrix::new(cup(4), 2);
        assert_eq!(empty.standings(), [0, 1, 2]);
        empty.score(&[2, 1, 0], &[1, 1, 1]);
        assert_eq!(empty.standings(), [2, 1, 0], "the last race decides");
    }

    /// A race run again (the player restarted it) gives its points back: the standings are
    /// those before it, and the race is scored again when it ends.
    #[test]
    fn a_race_run_again_gives_its_points_back() {
        let points = GrandPrixTuning::default().points;
        let mut gp = GrandPrix::new(cup(4), 3);
        gp.score(&[0, 1, 2, 3], &points);
        gp.next_race();
        let before = gp.clone();
        gp.score(&[3, 2, 1, 0], &points);
        gp.stage = GrandPrixStage::Standings;
        gp.restart_race(&points);
        assert_eq!(gp, before);
        assert_eq!(gp.last_points, [15, 12, 10, 9]);
        // Not scored yet: nothing to give back.
        gp.restart_race(&points);
        assert_eq!(gp, before);
        gp.score(&[1, 0, 2, 3], &points);
        assert_eq!(gp.points, [27, 27, 20, 18]);
        assert_eq!(gp.orders.len(), 2);
    }

    /// Issue #122: online, several players among the entrants; one who left (before a race, or
    /// during it: it gave up) comes last of it and wins nothing, the others win the points of
    /// their places as if it were not there; it keeps what it won before, and the ties are
    /// broken as ever (the last race first).
    #[test]
    fn a_player_who_left_keeps_its_points_and_wins_no_more() {
        let points = GrandPrixTuning::default().points;
        let mut gp = GrandPrix::with_entrants(cup(3), 4);
        assert_eq!(gp.entrants, 4);
        gp.score(&[3, 2, 1, 0], &points);
        assert_eq!(gp.points, [9, 10, 12, 15]);
        gp.next_race();
        // 3 gave up (it left during the race): last in the results, out of the points.
        gp.score_without(&[2, 3, 1, 0], &[3], &points);
        assert_eq!(gp.orders[1], [2, 1, 0, 3]);
        assert_eq!(gp.last_points, [10, 12, 15, 0]);
        assert_eq!(gp.points, [19, 22, 27, 15]);
        assert_eq!(gp.standings(), [2, 1, 0, 3]);
        gp.next_race();
        // It left before this one: not in the results at all.
        gp.score_without(&[0, 1, 2], &[3], &points);
        assert_eq!(gp.points, [34, 34, 37, 15]);
        assert_eq!(
            gp.standings(),
            [2, 0, 1, 3],
            "0 ahead of 1: better in the last race"
        );
        // Without anyone out, the same as `score`.
        let mut a = GrandPrix::with_entrants(cup(2), 3);
        let mut b = a.clone();
        a.score(&[1, 2], &points);
        b.score_without(&[1, 2], &[], &points);
        assert_eq!(a, b);
        assert_eq!(a.orders[0], [1, 2, 0]);
        assert_eq!(GrandPrix::with_entrants(cup(2), 0).entrants, 1);
        assert_eq!(
            GrandPrix::new(cup(2), 11),
            GrandPrix::with_entrants(cup(2), 12)
        );
    }

    /// An order missing an entrant (or naming one twice, or one that does not exist) still
    /// scores every entrant once: the missing ones last.
    #[test]
    fn a_short_order_still_scores_everybody() {
        let points = GrandPrixTuning::default().points;
        let mut gp = GrandPrix::new(cup(4), 3);
        assert_eq!(gp.entrants, 4);
        gp.score(&[2, 2, 9, 0], &points);
        assert_eq!(gp.orders[0], [2, 0, 1, 3]);
        assert_eq!(gp.points, [12, 10, 15, 9]);
        let lineup = gp.next_lineup();
        assert_eq!(lineup[..4], [3, 1, 0, 2]);
        assert_eq!(lineup[4..], [4, 5, 6, 7, 8, 9, 10, 11]);
    }
}
