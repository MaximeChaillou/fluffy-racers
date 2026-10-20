//! Issue #122: **the championship online** (GD § 1, `docs/ARCHITECTURE.md` § « Championnat en
//! ligne »), pure logic: no Bevy, no socket; unit-tested.
//!
//! - **The choice** ([`race_setup_of`], [`room_setup_of`], [`changed`]): in the lobby, the host
//!   chooses as in « Jouer solo » (`modes::RaceSetup`, its lines, values and helps), without the
//!   time trial (GD § 1: not online): « Circuit seul » or « Championnat » (a cup of the manifest,
//!   or a custom one: its number of races, its circuits chosen or drawn at random), the mirror,
//!   the bots and the items. The room's settings ([`RoomSetup`]) carry it to everyone, with the
//!   program: **the host draws** a random championship ([`room_seed`]: the seed of its room, and
//!   the number of the draw), the others show its draw.
//! - **The host carries it** ([`Championship`]): its entrants (the bots that fill the grid at its
//!   start, then the players seated then, in their order: each keeps its number to the end), the
//!   seed of its bots (the same bots, animal, parts, color and name, from one race to the next),
//!   the score of each race from the host's closed results ([`Championship::score`]: the points
//!   of GD § 1, the ties broken by the last race), the overall standings, and the grid of the
//!   next race ([`Championship::next_grid`]: the reverse of the standings, the leader last; the
//!   first race lines up as any race, the bots in front). It tells everyone its state
//!   ([`ChampionshipState`], `Message::Championship`): every peer shows the same standings.
//! - **A player who leaves** keeps its points; it is out of the next races (the grid closes up)
//!   and wins nothing more: last of each one, without points (the race it left included: it gave
//!   up). **A player who arrives** watches (its races, the standings, the podium) and is of the
//!   next championship.
//! - **What a peer shows** ([`entrant_at`], [`grid_lineup`], [`standings_view`]): the entrant on
//!   each grid slot (the podium, `RaceGrid::lineup`), and the championship as a
//!   `race::GrandPrix` (the screens and the podium of `crate::modes`, online: the names, our
//!   entrant, the host's standings).

use super::protocol::{
    ChampionshipSetup, ChampionshipState, KartId, MAX_PROGRAM, PeerKey, Program, RaceResults,
    RaceStart, RoomSetup,
};
use super::room::RoomCode;
use crate::bots::BotDifficulty;
use crate::modes::{BotsChoice, Catalog, Formula, GameMode, MenuRow, Pick, RaceSetup};
use crate::race::{CupRace, GrandPrix, GrandPrixStage, OnlineStandings, ResultTime};
use crate::track::GRID_SLOTS;

/// No entrant on a grid slot (online, a slot past the racers): in `RaceGrid::lineup`, never a
/// bot.
pub const NO_ENTRANT: u8 = u8::MAX;

/// The seed of the host's draws in room `room` (`modes::draw_circuits`, with the number of the
/// draw): two rooms draw differently, a room the same for the same number. FNV-1a of its code.
pub fn room_seed(room: &RoomCode) -> u64 {
    room.as_str()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash: u64, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// The room's settings as « Jouer solo »'s choice (`modes::RaceSetup`): the lobby's lines are
/// its lines. A custom championship's circuits are its program (drawn, they are the first of the
/// circuits chosen when the host goes to « Au choix »).
pub fn race_setup_of(setup: &RoomSetup) -> RaceSetup {
    let plan = setup.plan;
    let program = plan.program.circuits();
    let defaults = RaceSetup::default();
    RaceSetup {
        mode: if setup.championship {
            GameMode::GrandPrix
        } else {
            GameMode::FreeRace
        },
        circuit: usize::from(setup.track),
        mirror: setup.mirror,
        // Issue #153: the room's bots at their level, or none.
        bots: BotsChoice::of(
            setup.bots,
            BotDifficulty::from_index(setup.difficulty).unwrap_or_default(),
        ),
        items: setup.items,
        formula: if plan.custom {
            Formula::Custom
        } else {
            Formula::Cup
        },
        cup: usize::from(plan.cup),
        races: if plan.custom && !program.is_empty() {
            program.len()
        } else {
            defaults.races
        },
        pick: if plan.chosen {
            Pick::Chosen
        } else {
            Pick::Random
        },
        chosen: if plan.custom { program } else { Vec::new() },
        draw: plan.draw,
    }
}

/// « Jouer solo »'s choice as the room's settings, with what its championship races (its
/// program: the cup's circuits, the chosen ones, or those drawn from `seed`, the room's), kept
/// while the mode is the circuit alone. Never the time trial (not online), never more races than
/// the protocol carries ([`MAX_PROGRAM`]).
pub fn room_setup_of(race: &RaceSetup, seed: u64, catalog: &Catalog) -> RoomSetup {
    let mut championship = race.clone();
    championship.mode = GameMode::GrandPrix;
    championship.races = championship.races.min(MAX_PROGRAM);
    championship.chosen.truncate(MAX_PROGRAM);
    let program = championship.program(seed, catalog);
    RoomSetup {
        track: race.circuit.min(255) as u8,
        mirror: race.mirror,
        bots: race.bots.on(),
        // Issue #153: without bots, Normal (a level only matters with them).
        difficulty: race.bots.difficulty().unwrap_or_default().index() as u8,
        items: race.items,
        championship: race.mode == GameMode::GrandPrix,
        plan: ChampionshipSetup {
            custom: race.formula == Formula::Custom,
            cup: race.cup.min(255) as u8,
            chosen: race.pick == Pick::Chosen,
            draw: race.draw,
            program: Program::new(&program),
        },
    }
}

/// The lobby's lines for `setup` (`modes::RaceSetup::rows`): the mode first, then what it needs,
/// the mirror, the bots, the items, and a custom championship's races when chosen one by one.
pub fn rows(setup: &RoomSetup) -> Vec<MenuRow> {
    race_setup_of(setup).rows()
}

/// `setup` with the line `row` changed by `step` values (`modes::RaceSetup::changed`): the modes
/// go around without the time trial; a random championship drawn again from `seed` when its
/// number of races changes or the host goes back to « Au hasard ».
pub fn changed(
    setup: RoomSetup,
    row: MenuRow,
    step: i32,
    seed: u64,
    catalog: &Catalog,
) -> RoomSetup {
    let mut race = race_setup_of(&setup).changed(row, step, catalog);
    if race.mode == GameMode::TimeTrial {
        race = race.changed(MenuRow::Mode, step, catalog);
    }
    room_setup_of(&race, seed, catalog)
}

/// `setup` once its championship was raced: a random one is drawn again (a new number), like
/// « Jouer solo » after each championship started.
pub fn after_a_championship(setup: RoomSetup, seed: u64, catalog: &Catalog) -> RoomSetup {
    if !setup.championship || !setup.plan.custom || setup.plan.chosen {
        return setup;
    }
    let mut race = race_setup_of(&setup);
    race.draw = race.draw.wrapping_add(1);
    room_setup_of(&race, seed, catalog)
}

/// The name of the championship of `plan` (its standings' title): its cup's, or « Championnat ».
pub fn title_of(plan: &ChampionshipSetup, catalog: &Catalog) -> String {
    let setup = RoomSetup {
        championship: true,
        plan: *plan,
        ..RoomSetup::default()
    };
    race_setup_of(&setup).title(catalog)
}

/// The entrant on grid slot `slot` of race `start`, a race of the championship `state`: a bot
/// is its number, a player its place in the championship's players. `None`: nobody there, or a
/// player who is not one of its entrants.
pub fn entrant_at(state: &ChampionshipState, start: &RaceStart, slot: usize) -> Option<usize> {
    if slot >= start.racers() {
        return None;
    }
    let racer = start.racer_at(slot);
    let bots = usize::from(start.bots);
    if racer < bots {
        return (racer < usize::from(state.bots)).then_some(racer);
    }
    start
        .players
        .get(racer - bots)
        .and_then(|peer| state.entrant_of(*peer))
}

/// The entrant on each grid slot of race `start` (`RaceGrid::lineup`: the podium's places, the
/// host's bots on their slots), [`NO_ENTRANT`] where nobody of the championship starts.
pub fn grid_lineup(state: &ChampionshipState, start: &RaceStart) -> [u8; GRID_SLOTS] {
    std::array::from_fn(|slot| {
        entrant_at(state, start, slot).map_or(NO_ENTRANT, |entrant| entrant as u8)
    })
}

/// The championship in progress, as its host carries it (see the module's documentation): what
/// every peer is told, and its scoring (each race's order: the ties).
#[derive(Clone, Debug, PartialEq)]
pub struct Championship {
    pub state: ChampionshipState,
    scoring: GrandPrix,
}

impl Championship {
    /// A new championship on `setup`'s program: `bots` bots (the bots of its seed, `seed`), then
    /// `players` (in their order), nobody scored yet.
    pub fn new(setup: ChampionshipSetup, seed: u64, bots: u8, players: Vec<PeerKey>) -> Self {
        let bots = bots.min(GRID_SLOTS as u8);
        let mut players = players;
        players.truncate(GRID_SLOTS - usize::from(bots));
        let entrants = usize::from(bots) + players.len();
        let cup = setup
            .program
            .as_slice()
            .iter()
            .map(|circuit| CupRace {
                name: String::new(),
                path: circuit.to_string(),
            })
            .collect();
        let scoring = GrandPrix::with_entrants(cup, entrants);
        let state = ChampionshipState {
            setup,
            seed,
            bots,
            players,
            scored: 0,
            podium: false,
            points: vec![0; entrants],
            last: vec![0; entrants],
            standings: (0..entrants as u8).collect(),
        };
        Self { state, scoring }
    }

    /// The round of its next race (1 for the first), if it has one left.
    pub fn next_round(&self) -> Option<u8> {
        (usize::from(self.state.scored) < self.state.setup.program.len())
            .then(|| self.state.scored + 1)
    }

    /// The circuit of its next race.
    pub fn next_track(&self) -> Option<u8> {
        let next = usize::from(self.state.scored);
        self.state.setup.program.as_slice().get(next).copied()
    }

    /// Every race was scored: the podium comes.
    pub fn finished(&self) -> bool {
        self.next_round().is_none()
    }

    /// The grid of its next race, with the players seated now (`seated`): its players (those of
    /// the championship still there, in its order) and who starts where (`RaceStart::lineup`).
    /// The first race lines up as any race (empty: the bots in front, the players behind); after
    /// it, the reverse of the overall standings, the leader last, among the racers there (the
    /// grid closes up over a player who left).
    pub fn next_grid(&self, seated: &[PeerKey]) -> (Vec<PeerKey>, Vec<u8>) {
        let state = &self.state;
        let players: Vec<PeerKey> = state
            .players
            .iter()
            .filter(|peer| seated.contains(peer))
            .copied()
            .collect();
        if state.scored == 0 {
            return (players, Vec::new());
        }
        let bots = usize::from(state.bots);
        let racer = |entrant: usize| -> Option<u8> {
            if entrant < bots {
                return Some(entrant as u8);
            }
            let peer = state.player_of(entrant)?;
            let index = players.iter().position(|p| *p == peer)?;
            Some((bots + index) as u8)
        };
        let lineup = state
            .standings
            .iter()
            .rev()
            .filter_map(|entrant| racer(usize::from(*entrant)))
            .collect();
        (players, lineup)
    }

    /// Scores its race `start` from the host's closed results (`results`, `points`: the points of
    /// each place, `race::GrandPrixTuning::points`), if it is its next round: the racers in their
    /// places; a player who gave up (it left during the race) or who was not on the grid (it
    /// left before) last, without points. Returns whether it scored.
    pub fn score(&mut self, start: &RaceStart, results: &RaceResults, points: &[u32]) -> bool {
        let state = &self.state;
        if start.round == 0
            || Some(start.round) != self.next_round()
            || results.race != start.race
            || start.bots != state.bots
        {
            return false;
        }
        let entrants = state.entrants();
        let of = |kart: KartId| entrant_at(state, start, usize::from(kart));
        let on_grid: Vec<usize> = (0..start.racers())
            .filter_map(|slot| entrant_at(state, start, slot))
            .collect();
        let gave_up: Vec<usize> = results
            .lines
            .iter()
            .filter(|line| line.time == ResultTime::GaveUp)
            .filter_map(|line| of(line.id))
            .collect();
        let out: Vec<usize> = (0..entrants)
            .filter(|entrant| !on_grid.contains(entrant) || gave_up.contains(entrant))
            .collect();
        let order: Vec<usize> = results
            .lines
            .iter()
            .filter_map(|line| of(line.id))
            .collect();
        self.scoring.race = usize::from(state.scored);
        self.scoring.score_without(&order, &out, points);
        let clamp = |value: u32| value.min(u32::from(u16::MAX)) as u16;
        let state = &mut self.state;
        state.points = self.scoring.points.iter().map(|p| clamp(*p)).collect();
        state.last = self.scoring.last_points.iter().map(|p| clamp(*p)).collect();
        state.standings = self
            .scoring
            .standings()
            .iter()
            .map(|entrant| *entrant as u8)
            .collect();
        state.scored += 1;
        true
    }

    /// After its last race and its standings: the podium.
    pub fn podium(&mut self) {
        self.state.podium = true;
    }
}

/// The championship `state` as this peer shows it (`race::GrandPrix`, for the screens and the
/// podium of `crate::modes`): its title, its races (`catalog`'s circuits), the race being shown
/// (`round`: its round, 1 for the first), the stage (the race, its standings once it was scored,
/// the podium), the points, and online what the host told: the names of the entrants (`names`),
/// ours (`me`), the standings, whether we host it.
pub fn standings_view(
    state: &ChampionshipState,
    catalog: &Catalog,
    round: u8,
    names: Vec<String>,
    me: Option<usize>,
    hosting: bool,
) -> GrandPrix {
    let program = state.setup.program.circuits();
    let mut view = GrandPrix::with_entrants(catalog.races(&program), state.entrants());
    view.title = title_of(&state.setup, catalog);
    view.race = usize::from(round.max(1) - 1).min(program.len().saturating_sub(1));
    view.stage = if state.podium {
        GrandPrixStage::Podium
    } else if state.scored >= round.max(1) {
        GrandPrixStage::Standings
    } else {
        GrandPrixStage::Racing
    };
    let entrants = view.entrants;
    let value = |list: &[u16], entrant: usize| u32::from(list.get(entrant).copied().unwrap_or(0));
    view.points = (0..entrants).map(|e| value(&state.points, e)).collect();
    view.last_points = (0..entrants).map(|e| value(&state.last, e)).collect();
    view.online = Some(OnlineStandings {
        names,
        me,
        standings: state.standings.iter().map(|e| usize::from(*e)).collect(),
        hosting,
    });
    view
}

/// The name of a bot of the championship (its number, as on the results): « Kart N ».
pub fn bot_name(entrant: usize) -> String {
    format!("Kart {}", entrant + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::tracks::{CupEntry, CupIcon, Manifest, TrackEntry};
    use crate::net::protocol::Message;
    use crate::race::{GrandPrixTuning, ResultLine};

    const A: PeerKey = PeerKey(1 << 100);
    const B: PeerKey = PeerKey(2 << 100);
    const C: PeerKey = PeerKey(3 << 100);

    fn catalog(circuits: usize) -> Catalog {
        let manifest = Manifest {
            circuits: (0..circuits)
                .map(|k| TrackEntry {
                    name: format!("Circuit {k}"),
                    file: format!("c{k}.track.ron"),
                })
                .collect(),
            cups: (0..circuits.div_ceil(4))
                .map(|k| CupEntry {
                    name: format!("Coupe {k}"),
                    icon: CupIcon::ALL[k % CupIcon::ALL.len()],
                })
                .collect(),
        };
        Catalog::of(&manifest, 4)
    }

    /// Goes through `row` until `done`, `step` at a time (at most 20 steps).
    fn until(
        mut setup: RoomSetup,
        row: MenuRow,
        step: i32,
        catalog: &Catalog,
        done: impl Fn(&RoomSetup) -> bool,
    ) -> RoomSetup {
        for _ in 0..20 {
            if done(&setup) {
                return setup;
            }
            setup = changed(setup, row, step, 7, catalog);
        }
        panic!("{row:?} never got there: {setup:?}");
    }

    /// Issue #122: the host's modes are « Circuit seul » and « Championnat », around (never the
    /// time trial: not online, GD § 1); a cup races its four circuits; a custom one its races,
    /// drawn (the same for the same room and number, no circuit twice) or chosen one by one, in
    /// order; the championship is kept while the mode is the circuit alone.
    #[test]
    fn the_host_chooses_the_mode_and_the_championship_as_solo() {
        let catalog = catalog(6);
        let single = RoomSetup::default();
        assert_eq!(rows(&single)[..2], [MenuRow::Mode, MenuRow::Circuit]);
        let mut seen = Vec::new();
        let mut setup = single;
        for _ in 0..4 {
            setup = changed(setup, MenuRow::Mode, 1, 7, &catalog);
            seen.push(race_setup_of(&setup).mode);
        }
        assert_eq!(
            seen,
            [
                GameMode::GrandPrix,
                GameMode::FreeRace,
                GameMode::GrandPrix,
                GameMode::FreeRace
            ]
        );
        let back = changed(single, MenuRow::Mode, -1, 7, &catalog);
        assert!(
            back.championship,
            "backwards, the time trial is skipped too"
        );
        // A cup: its four circuits, in order.
        let cup = changed(single, MenuRow::Mode, 1, 7, &catalog);
        assert!(cup.championship && !cup.plan.custom);
        assert_eq!(cup.plan.program.as_slice(), [0, 1, 2, 3]);
        assert_eq!(
            rows(&cup),
            [
                MenuRow::Mode,
                MenuRow::Formula,
                MenuRow::Cup,
                MenuRow::Mirror,
                MenuRow::Bots,
                MenuRow::Items
            ]
        );
        assert_eq!(title_of(&cup.plan, &catalog), "Coupe 0");
        // A custom one, drawn: as many races as asked, each circuit once, the same again for the
        // same number; another number, another draw (from 3 races to 4 and back).
        let custom = changed(cup, MenuRow::Formula, 1, 7, &catalog);
        assert!(custom.plan.custom && !custom.plan.chosen);
        assert_eq!(title_of(&custom.plan, &catalog), "Championnat");
        let three = until(custom, MenuRow::Races, 1, &catalog, |s| {
            s.plan.program.len() == 3
        });
        let drawn = three.plan.program.circuits();
        let mut sorted = drawn.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "no circuit twice: {drawn:?}");
        assert_eq!(room_setup_of(&race_setup_of(&three), 7, &catalog), three);
        assert_ne!(
            room_setup_of(&race_setup_of(&three), 8, &catalog)
                .plan
                .program,
            three.plan.program,
            "another room, another draw"
        );
        // Chosen one by one: the draw is where it starts, each race its own line, raced in order.
        let chosen = changed(three, MenuRow::Pick, 1, 7, &catalog);
        assert!(chosen.plan.chosen);
        assert_eq!(chosen.plan.program.circuits(), drawn);
        assert_eq!(
            rows(&chosen)[rows(&chosen).len() - 3..],
            [MenuRow::Race(0), MenuRow::Race(1), MenuRow::Race(2)]
        );
        let first = changed(chosen, MenuRow::Race(0), 1, 7, &catalog);
        assert_eq!(first.plan.program.as_slice()[0], ((drawn[0] + 1) % 6) as u8);
        assert_eq!(
            first.plan.program.as_slice()[1..],
            chosen.plan.program.as_slice()[1..]
        );
        // Back to the circuit alone: the championship stays chosen for later.
        let alone = changed(first, MenuRow::Mode, 1, 7, &catalog);
        assert!(!alone.championship);
        assert_eq!(alone.plan, first.plan);
        assert_eq!(
            changed(alone, MenuRow::Mode, 1, 7, &catalog).plan,
            first.plan
        );
        // The settings travel whole.
        for setup in [single, cup, three, chosen, first, alone] {
            let message = Message::Setup(setup);
            assert_eq!(Message::decode(&message.to_bytes()), Ok(message));
        }
        // After a random championship, another draw; a cup or a chosen one stays.
        let next = after_a_championship(three, 7, &catalog);
        assert_eq!(next.plan.draw, three.plan.draw.wrapping_add(1));
        assert_eq!(after_a_championship(cup, 7, &catalog), cup);
        assert_eq!(after_a_championship(chosen, 7, &catalog), chosen);
        // A room's seed: the same code, the same seed.
        let room = |code: &str| room_seed(&RoomCode::parse(code).unwrap());
        assert_eq!(room("ABCDE"), room("abcde"));
        assert_ne!(room("ABCDE"), room("ABCDF"));
    }

    /// A start of round `round` of `championship` for `seated`, its grid the championship's.
    fn start_of(championship: &Championship, race: u16, seated: &[PeerKey]) -> RaceStart {
        let (players, lineup) = championship.next_grid(seated);
        RaceStart {
            race,
            seed: championship.state.seed,
            track: championship.next_track().unwrap(),
            laps: 3,
            bots: championship.state.bots,
            difficulty: 1,
            mirror: false,
            items: true,
            start_tick: 0,
            round: championship.next_round().unwrap(),
            lineup,
            players,
        }
    }

    /// The host's closed results of `start`: the racers of `slots` first to last, finished.
    fn results_of(start: &RaceStart, slots: &[KartId]) -> RaceResults {
        RaceResults {
            race: start.race,
            lines: slots
                .iter()
                .enumerate()
                .map(|(place, slot)| ResultLine {
                    id: *slot,
                    time: ResultTime::Finished(1000 + place as u32),
                })
                .collect(),
        }
    }

    /// Issue #122 acceptance (pure): a championship of 3 players and 9 bots over two races: the
    /// points of GD § 1 from the host's results, the standings, and the grid of the second race
    /// the reverse of the standings (the leader last), each bot keeping its number (its parts,
    /// color and name) wherever it starts; the first race lines up as any race. Then a player
    /// leaves: it keeps its points, the third race's grid closes up over it, it wins nothing more.
    #[test]
    fn the_host_scores_the_races_and_reverses_the_grid() {
        let points = GrandPrixTuning::default().points;
        let setup = ChampionshipSetup {
            program: Program::new(&[0, 1, 2]),
            ..ChampionshipSetup::default()
        };
        let mut championship = Championship::new(setup, 0xbeef, 9, vec![A, B, C]);
        assert_eq!(championship.state.entrants(), 12);
        assert_eq!(championship.next_round(), Some(1));
        let first = start_of(&championship, 5, &[A, B, C]);
        assert!(
            first.lineup.is_empty(),
            "the bots in front, the players behind"
        );
        assert_eq!(first.slot_of(A), Some(9));
        for slot in 0..12 {
            assert_eq!(entrant_at(&championship.state, &first, slot), Some(slot));
        }
        // Race 1: C (slot 11) wins, then A (9), then the bots, B (10) last.
        let order: Vec<KartId> = [11, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8, 10].to_vec();
        assert!(championship.score(&first, &results_of(&first, &order), &points));
        assert!(
            !championship.score(&first, &results_of(&first, &order), &points),
            "scored once"
        );
        let state = championship.state.clone();
        assert_eq!(state.scored, 1);
        assert_eq!(
            (state.points[11], state.points[9], state.points[10]),
            (15, 12, 1)
        );
        assert_eq!(state.points[0], 10);
        assert_eq!(state.standings[..3], [11, 9, 0]);
        assert_eq!(state.last, state.points);
        // Race 2: the reverse of the standings: B (the last) on the pole, C (the leader) last.
        let second = start_of(&championship, 6, &[A, B, C]);
        assert_eq!(second.round, 2);
        assert_eq!(second.slot_of(B), Some(0));
        assert_eq!(second.slot_of(C), Some(11));
        assert_eq!(second.slot_of(A), Some(10));
        let lineup = grid_lineup(&championship.state, &second);
        assert_eq!(lineup[11], 11);
        assert_eq!(lineup[0], 10);
        // Bot 0 (third) starts 10th from the back: slot 9, still bot 0.
        assert_eq!(second.bot_at(9), Some(0));
        assert_eq!(lineup[9], 0);
        // Race 2: the slots in order of the grid, the pole first (B wins it).
        let order: Vec<KartId> = (0..12).collect();
        assert!(championship.score(&second, &results_of(&second, &order), &points));
        let state = championship.state.clone();
        // B: 1 + 15; A: 12 + 2; C: 15 + 1.
        assert_eq!(
            (state.points[10], state.points[9], state.points[11]),
            (16, 14, 16)
        );
        assert_eq!(state.last[10], 15);
        // B and C at 16: B was better in the last race.
        assert_eq!(state.standings[..2], [10, 11]);
        // C leaves: the third race has A and B, the grid closes up (11 racers), C wins nothing.
        let third = start_of(&championship, 7, &[A, B]);
        assert_eq!(third.players, [A, B]);
        assert_eq!(third.racers(), 11);
        assert_eq!(third.slot_of(B), Some(10), "the leader last");
        assert!(
            grid_lineup(&championship.state, &third)
                .iter()
                .all(|e| *e != 11)
        );
        let order: Vec<KartId> = (0..11).collect();
        assert!(championship.score(&third, &results_of(&third, &order), &points));
        let state = championship.state.clone();
        assert_eq!(state.points[11], 16, "C keeps its points");
        assert_eq!(state.last[11], 0, "and wins nothing more");
        assert!(championship.finished());
        assert_eq!(championship.next_track(), None);
        championship.podium();
        assert!(championship.state.podium);
        // Everything travels.
        let message = Message::Championship(championship.state.clone());
        assert_eq!(Message::decode(&message.to_bytes()), Ok(message));
    }

    /// Issue #122: a player who leaves during a race gave up: last of it, without points (the
    /// others as if it were not there). A race that is not the next round, or of another number,
    /// is not scored.
    #[test]
    fn a_player_who_gives_up_wins_nothing() {
        let points = GrandPrixTuning::default().points;
        let setup = ChampionshipSetup {
            custom: true,
            chosen: true,
            program: Program::new(&[3, 3]),
            ..ChampionshipSetup::default()
        };
        let mut championship = Championship::new(setup, 1, 0, vec![A, B, C]);
        let first = start_of(&championship, 1, &[A, B, C]);
        let mut results = results_of(&first, &[2, 0, 1]);
        results.lines[0].time = ResultTime::GaveUp;
        let other = RaceResults {
            race: 9,
            ..results.clone()
        };
        assert!(!championship.score(&first, &other, &points));
        let alone = RaceStart {
            round: 0,
            ..first.clone()
        };
        assert!(!championship.score(&alone, &results, &points));
        assert!(championship.score(&first, &results, &points));
        let state = &championship.state;
        // A first (15), B second (12), C gave up: nothing.
        assert_eq!(state.points, [15, 12, 0]);
        assert_eq!(state.standings, [0, 1, 2]);
        let view = standings_view(
            state,
            &catalog(4),
            1,
            vec!["Ana".into(), "Bob".into(), "Cyd".into()],
            Some(1),
            false,
        );
        assert_eq!(view.stage, GrandPrixStage::Standings);
        assert_eq!(view.title, "Championnat");
        assert_eq!(view.cup.len(), 2);
        assert_eq!(view.cup[0].name, "Circuit 3");
        assert_eq!(view.standings(), [0, 1, 2]);
        assert!(view.is_player(1) && !view.is_player(2));
        assert_eq!(view.points, [15, 12, 0]);
        // The next race shown: racing again; the podium after the last.
        assert_eq!(
            standings_view(state, &catalog(4), 2, vec![], None, true).stage,
            GrandPrixStage::Racing
        );
        let mut done = state.clone();
        done.podium = true;
        assert_eq!(
            standings_view(&done, &catalog(4), 2, vec![], None, true).stage,
            GrandPrixStage::Podium
        );
    }
}
