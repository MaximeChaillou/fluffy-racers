//! Race rules (GD § 1 and 2, issue #5): 12 racers, 3 laps, ranking, timing, tow pelican.
//!
//! A racer is any kart with a [`Racer`] component, whatever produces its `KartInput` (player,
//! bot, network): the rules only read its position and write its progress. The rules are pure
//! logic, tested on their own (`flow.rs`, `course.rs`, `progress.rs`, `ranking.rs`, `tow.rs`);
//! the systems here only feed them, in `FixedUpdate` at 60 Hz:
//!
//! 1. [`RaceSystems::Flow`], before the karts read their input: race clock and phase (grid,
//!    countdown, race, finish, results), restart.
//! 2. [`RaceSystems::Gate`], between the input and the driving: the rocket start watches the
//!    accelerator during the countdown and is judged on GO (GD § 4, issue #13: a boost, or a
//!    stalled engine); then nothing moves before GO (the racers' `KartInput` is zeroed), and
//!    finished racers coast.
//! 3. [`RaceSystems::Rules`], after the physics step: checkpoints, laps, finish, tow pelican;
//!    then [`RaceSystems::Rank`]: ranking and distance to the leader (online, issue #9, the other
//!    peers' karts get their progress in between), and the closing of the results (issue #70,
//!    `results.rs`: the racers still on track get an estimated time).
//!
//! The race exists only on a circuit (`TrackLayout`): on the flat test ground there is none.

mod course;
mod flow;
mod grand_prix;
mod pelican;
mod progress;
mod ranking;
mod results;
mod tow;
mod tuning;

#[cfg(test)]
mod scenario_tests;
/// Issue #119: the open test circuit's course, for unit tests elsewhere.
#[cfg(test)]
pub(crate) use course::tests::open_circuit as open_course_for_tests;
/// Prairie Pompon's course, for unit tests elsewhere.
#[cfg(test)]
pub(crate) use course::tests::prairie as course_for_tests;

pub use course::{Course, OpenRace, Projection, drop_point};
pub use flow::{FlowTimings, RaceFlow, RacePhase, race_over};
pub use grand_prix::{
    CHAMPIONSHIP, CupRace, GrandPrix, GrandPrixStage, GrandPrixTuning, OnlineStandings, points_for,
};
pub use pelican::{PelicanPlugin, PelicanVisual};
pub use progress::{ProgressEvent, RaceProgress};
pub use ranking::{RankEntry, distance_to_leader, rank, rank_by_order};
pub use results::{ResultLine, ResultTime, Standing, close_results, estimate_finish};
pub use tow::{Carry, Tow, TowObservation, TowReason, tow_pose};
pub use tuning::{HazardTuning, RaceTuning, seconds_to_ticks, ticks_to_seconds};

use bevy::prelude::*;
use bevy_rapier3d::prelude::{PhysicsSet, Velocity};

use crate::TICK_HZ;
use crate::bots::BotDifficulty;
use crate::interpolation::PhysicsPose;
use crate::kart::{
    Boost, Invincibility, Kart, KartEnvironment, KartInput, KartState, KartSystems, KartTuning,
    PlayerKart, Stall, StartOutcome, StartPress, start_outcome,
};
use crate::track::TrackLayout;

/// A kart taking part in the race. `grid_slot`: its starting place (0 = pole), where a restart
/// puts it back. See [`RaceGrid`] for who takes which slot.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(RaceProgress, Tow, RocketStart)]
pub struct Racer {
    pub grid_slot: usize,
}

/// The name of a racer on the results screen (issue #9: another player's, online). Without it:
/// « Toi » for the local player, « Kart N » for the others.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct RacerName(pub String);

/// A racer's rocket start (GD § 4, issue #13): its first press of the accelerator from the
/// race (re)start to GO, judged on GO (`kart::start_outcome`) with the kart's own tuning.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RocketStart {
    pub press: StartPress,
}

impl Racer {
    pub fn new(grid_slot: usize) -> Self {
        Self { grid_slot }
    }

    /// Drop lane of the tow pelican (-1, 0 or 1): karts dropped together do not overlap, and
    /// the pole (the player in solo) gets the centre.
    pub fn drop_lane(&self) -> f32 {
        [0.0, -1.0, 1.0][self.grid_slot % 3]
    }
}

/// Who lines up on the grid (GD § 1: 12 racers): `bots` bots on the front slots, the local
/// player behind them, like the first race of a Grand Prix in MK8DX. With no bot, the player is
/// alone on the pole.
///
/// The racers are **entrants** (issue #19): entrants `0..bots` are the bots (entrant `e` is
/// always the same bot: its character, parts, color and seed come from `e`), entrant `bots` the
/// local player. By default entrant `e` starts on slot `e`; a Grand Prix lines them up in
/// another order after its first race ([`RaceGrid::lineup`]).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RaceGrid {
    /// Bots on the grid (issue #6), at most `GRID_SLOTS - 1`.
    pub bots: usize,
    /// Race seed (issue #40): every random draw of the bots of this race comes from it and from
    /// their grid slot (`bots::bot_seed`): driving, rocket start, tricks, kart parts. Same seed,
    /// same bots: the race tests run several seeds, and the network will have every peer draw
    /// the same bots from the seed of the host. The game keeps [`RaceGrid::GAME_SEED`] (the bots
    /// of every race before #40); a new seed applies at the next restart, when the bots spawn.
    pub seed: u64,
    /// Issue #19: the entrant on each grid slot (`lineup[slot]`), when it is not the slot itself
    /// (a race of a Grand Prix after the first: the reverse of the standings, GD § 1). `None`:
    /// entrant `e` on slot `e`. Read when the racers spawn.
    pub lineup: Option<[u8; crate::track::GRID_SLOTS]>,
    /// Issue #153: the level of its bots (GD § 12), Normal by default: « Jouer solo »'s
    /// « Bots » line (`modes::BotsChoice`), the host's online (`net::RaceStart::difficulty`), or
    /// the admin panel's. Read when the bots spawn, like their number.
    pub difficulty: BotDifficulty,
}

impl Default for RaceGrid {
    fn default() -> Self {
        Self {
            bots: crate::track::GRID_SLOTS - 1,
            seed: Self::GAME_SEED,
            lineup: None,
            difficulty: BotDifficulty::Normal,
        }
    }
}

impl RaceGrid {
    /// The race seed of the game: the bots drive exactly as before the seed existed.
    pub const GAME_SEED: u64 = 0;

    /// Bots actually on the grid: one slot always stays for the player.
    pub fn bot_count(&self) -> usize {
        self.bots.min(crate::track::GRID_SLOTS - 1)
    }

    /// Grid slots of the bots: the front of the grid.
    pub fn bot_slots(&self) -> std::ops::Range<usize> {
        0..self.bot_count()
    }

    /// Grid slot of the local player: right behind the bots (or where the lineup puts it).
    pub fn player_slot(&self) -> usize {
        self.slot_of(self.bot_count())
    }

    /// The entrant starting on grid slot `slot` (see the type's documentation).
    pub fn entrant_at(&self, slot: usize) -> usize {
        self.lineup
            .and_then(|lineup| lineup.get(slot).copied())
            .map_or(slot, usize::from)
    }

    /// The grid slot of entrant `entrant`.
    pub fn slot_of(&self, entrant: usize) -> usize {
        match self.lineup {
            Some(lineup) => lineup
                .iter()
                .position(|e| usize::from(*e) == entrant)
                .unwrap_or(entrant),
            None => entrant,
        }
    }

    /// The bots on the grid: (grid slot, entrant), by slot. Issue #122: on every slot of the grid
    /// (online, several players start among them); solo, the slots past the bots and the player
    /// hold none.
    pub fn bots_on_grid(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..crate::track::GRID_SLOTS)
            .map(|slot| (slot, self.entrant_at(slot)))
            .filter(|(_, entrant)| *entrant < self.bot_count())
    }
}

/// What a circuit is for: a race (the default), or free driving (admin panel, issue #36): no race,
/// no bots, the player alone on the pole, brought back there by the reset key (R) like on the
/// test ground. Read when the level is built (`crate::level::LoadLevel` rebuilds it).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RaceMode {
    #[default]
    Race,
    FreeRoam,
}

/// Online (issue #9, `crate::net`): the race follows the room. The network writes it every tick;
/// without it (solo), the race counts its own ticks, ends when the local players finished and
/// restarts from the results screen.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct OnlineFlow {
    /// The flow tick of this simulation tick (`RaceFlow::tick`): from the race's start tick on
    /// the room's clock, 0 before it.
    pub flow_tick: u32,
    /// The flow tick the host ended the race on.
    pub over_at: Option<u32>,
    /// The host's final order (grid slots, first to last): the standings once the race is over,
    /// the same on every peer ([`rank_by_order`]).
    pub order: Option<Vec<usize>>,
    /// Issue #70: the host's closed results (grid slots, first to last): every screen shows
    /// them ([`Race::results`]).
    pub results: Option<Vec<ResultLine<usize>>>,
}

/// A racer that keeps driving after the finish line instead of coasting (bots, issue #6:
/// coasting bots would block the straight for the racers still finishing).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DrivesOnAfterFinish;

/// A racer whose engine is off for good (issue #19: the podium of a Grand Prix, `crate::modes`):
/// nothing it asks moves it, like before GO.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Parked;

/// A racer on the podium of a Grand Prix (issue #19, `crate::modes`): its place, 0 = the winner.
/// Its driver dances (`crate::characters`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnPodium(pub usize);

/// The race in progress, created with the circuit. The static part lives in [`Course`].
#[derive(Resource, Debug)]
pub struct Race {
    pub flow: RaceFlow,
    pub timings: FlowTimings,
    /// Racers from first to last, refreshed every tick.
    pub standings: Vec<Entity>,
    /// A local player asked to race again (taken into account on the results screen only).
    pub restart_requested: bool,
    /// Issue #70: the closed results, first to last (empty until they close): the results
    /// screen shows them instead of the live standings. Online, the host's.
    pub results: Vec<ResultLine<Entity>>,
    /// Issue #19: close the results now, without waiting `RaceTuning::results_close_seconds`
    /// (the player moves on in a Grand Prix). Solo only.
    pub close_requested: bool,
    /// Scratch buffer of the ranking, kept between ticks so it never allocates.
    entries: Vec<RankEntry<Entity>>,
}

impl Race {
    pub fn new(tuning: &RaceTuning) -> Self {
        Self {
            flow: RaceFlow::default(),
            timings: FlowTimings::new(tuning),
            standings: Vec::with_capacity(crate::track::GRID_SLOTS),
            restart_requested: false,
            results: Vec::with_capacity(crate::track::GRID_SLOTS),
            close_requested: false,
            entries: Vec::with_capacity(crate::track::GRID_SLOTS),
        }
    }

    pub fn phase(&self) -> RacePhase {
        self.flow.phase(&self.timings)
    }

    /// The race clock: ticks since GO (0 before).
    pub fn race_ticks(&self) -> u32 {
        self.flow.race_ticks(&self.timings)
    }

    /// Whether the karts may drive (from GO on).
    pub fn started(&self) -> bool {
        self.flow.started(&self.timings)
    }

    /// Issue #70: whether the results closed ([`Race::results`]).
    pub fn results_closed(&self) -> bool {
        !self.results.is_empty()
    }
}

/// Order of the race systems inside `FixedUpdate` (see the module documentation).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RaceSystems {
    Flow,
    Gate,
    Rules,
    Rank,
}

/// Keys and buttons of the results screen: race again (GD § 13 has no menu yet); in a Grand Prix
/// (issue #19), go on to its standings and its next race.
pub const RESTART_KEYS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space];
pub const RESTART_BUTTONS: [GamepadButton; 2] = [GamepadButton::South, GamepadButton::Start];

/// The race rules. Headless: tests run them with `MinimalPlugins`. The visual of the pelican is
/// [`PelicanPlugin`], the HUD is `crate::ui`.
pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RaceTuning>()
            .init_resource::<GrandPrixTuning>()
            .init_resource::<RaceGrid>()
            .init_resource::<RaceMode>()
            .configure_sets(
                FixedUpdate,
                (
                    RaceSystems::Flow.before(KartSystems::Input),
                    RaceSystems::Gate
                        .after(KartSystems::Input)
                        .before(KartSystems::Drive),
                    RaceSystems::Rules.after(PhysicsSet::Writeback),
                    RaceSystems::Rank.after(RaceSystems::Rules),
                ),
            )
            .add_systems(
                Update,
                (
                    setup_race
                        .run_if(resource_exists::<TrackLayout>)
                        .run_if(not(resource_exists::<Race>))
                        .run_if(resource_equals(RaceMode::Race)),
                    (request_restart, request_tow).run_if(resource_exists::<Race>),
                ),
            )
            .add_systems(
                FixedUpdate,
                (
                    advance_race.in_set(RaceSystems::Flow),
                    (judge_rocket_starts, gate_inputs)
                        .chain()
                        .in_set(RaceSystems::Gate),
                    update_progress.in_set(RaceSystems::Rules),
                    (rank_racers, close_the_results)
                        .chain()
                        .in_set(RaceSystems::Rank),
                )
                    .run_if(resource_exists::<Race>),
            );
    }
}

fn setup_race(mut commands: Commands, layout: Res<TrackLayout>, tuning: Res<RaceTuning>) {
    commands.insert_resource(Course::new(&layout, &tuning));
    commands.insert_resource(Race::new(&tuning));
}

/// Results screen: Enter / Space or the gamepad's A / Start races again. Not while the game is
/// paused (a menu is open: its keys are not the race's), nor in a Grand Prix (issue #19: the same
/// keys go on to its standings, `crate::modes`). Issue #136: not in the game with its screens
/// (`crate::menu::MainMenu`, with a renderer): there, the results have buttons (« Rejouer »,
/// `menu::end`), and Entrée chooses the one with the focus. Headless (the tests), Entrée still
/// does what the first one does.
fn request_restart(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    time: Option<Res<Time<Virtual>>>,
    gamepads: Query<&Gamepad>,
    grand_prix: Option<Res<GrandPrix>>,
    screens: Option<Res<crate::menu::MainMenu>>,
    mut race: ResMut<Race>,
) {
    if race.phase() != RacePhase::Results
        || time.is_some_and(|time| time.is_paused())
        || grand_prix.is_some()
        || screens.is_some()
    {
        return;
    }
    let key = keys.is_some_and(|keys| keys.any_just_pressed(RESTART_KEYS));
    let button = gamepads
        .iter()
        .any(|gamepad| gamepad.any_just_pressed(RESTART_BUTTONS));
    if key || button {
        race.restart_requested = true;
    }
}

/// Debug key R in a race: the tow pelican brings the player back to its last safe point.
fn request_tow(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    race: Res<Race>,
    mut racers: Query<(&mut Tow, &RaceProgress), With<PlayerKart>>,
) {
    let pressed = keys.is_some_and(|keys| keys.just_pressed(crate::kart::DEBUG_RESET_KEY));
    if !pressed || !race.started() {
        return;
    }
    for (mut tow, progress) in &mut racers {
        if !tow.is_carrying() && !progress.finished() {
            tow.requested = true;
        }
    }
}

/// Moves the race clock; on a restart, puts every racer back on its grid slot. Online (issue #9),
/// the clock follows the room's ([`OnlineFlow`]): no local end, no restart.
fn advance_race(
    mut race: ResMut<Race>,
    course: Res<Course>,
    online: Option<Res<OnlineFlow>>,
    mut racers: Query<
        (
            &Racer,
            &mut RaceProgress,
            &mut Tow,
            Has<PlayerKart>,
            &mut Transform,
            &mut Velocity,
            &mut KartState,
            &mut Boost,
            &mut PhysicsPose,
            &mut Invincibility,
        ),
        With<Kart>,
    >,
) {
    if let Some(online) = online {
        race.restart_requested = false;
        race.flow.follow(online.flow_tick, online.over_at);
        return;
    }
    if racers.is_empty() {
        return;
    }
    let over = race_over(
        racers
            .iter()
            .map(|(_, progress, _, player, ..)| (player, progress.finished())),
    );
    let race = &mut *race;
    let restart = std::mem::take(&mut race.restart_requested);
    if !race.flow.advance(&race.timings, over, restart) {
        return;
    }
    race.results.clear();
    race.close_requested = false;
    for (
        racer,
        mut progress,
        mut tow,
        _,
        mut transform,
        mut velocity,
        mut state,
        mut boost,
        mut pose,
        mut invincibility,
    ) in &mut racers
    {
        let spawn = course.grid[racer.grid_slot.min(course.grid.len() - 1)];
        *transform = spawn;
        *velocity = Velocity::zero();
        *state = KartState::default();
        *boost = Boost::default();
        invincibility.set_if_neq(Invincibility::default());
        pose.snap(spawn);
        progress.reset(&course, spawn.translation);
        tow.reset(spawn.translation);
    }
}

/// Rocket start (GD § 4): before GO, records each racer's first press of the accelerator,
/// before the gate zeroes it (its driver wrote it this tick); on GO, gives the start boost or
/// stalls the engine. A (re)start forgets the previous one.
fn judge_rocket_starts(
    race: Res<Race>,
    tuning: Res<KartTuning>,
    mut racers: Query<(&KartInput, &mut RocketStart, &mut Boost, &mut Stall), With<Racer>>,
) {
    let flow = race.flow;
    let race_ticks = race.race_ticks();
    for (input, mut start, mut boost, mut stall) in &mut racers {
        if flow.tick <= 1 {
            start.set_if_neq(RocketStart::default());
        }
        if race_ticks == 0 {
            let mut press = start.press;
            press.observe(flow.ticks_since_two(&race.timings), input.throttle);
            start.set_if_neq(RocketStart { press });
        } else if race_ticks == 1 {
            match start_outcome(&tuning, start.press, TICK_HZ as f32) {
                StartOutcome::Normal => {}
                StartOutcome::Stall(seconds) => stall.remaining = seconds,
                StartOutcome::Boost(strength, duration) => boost.add(strength, duration),
            }
        }
    }
}

/// Nothing moves before GO (no input, and no boost: a boost pushes even without throttle);
/// finished racers coast (they keep their steering), unless they drive on (bots). Looking
/// behind (issue #17) stays allowed: only the camera reads it. A parked racer (issue #19) never
/// moves again. Issue #119: on an open circuit, every finished racer coasts (the bots too: the
/// road ends after the run-out).
fn gate_inputs(
    race: Res<Race>,
    course: Option<Res<Course>>,
    mut racers: Query<
        (
            &mut KartInput,
            &mut Boost,
            &mut Stall,
            &RaceProgress,
            Has<DrivesOnAfterFinish>,
            Has<Parked>,
        ),
        With<Racer>,
    >,
) {
    let started = race.started();
    let open = course.is_some_and(|course| course.is_open());
    for (mut input, mut boost, mut stall, progress, drives_on, parked) in &mut racers {
        let drives_on = drives_on && !open;
        if !started || parked {
            input.set_if_neq(KartInput {
                look_back: input.look_back,
                ..default()
            });
            boost.set_if_neq(Boost::default());
            stall.set_if_neq(Stall::default());
        } else if progress.finished() && !drives_on {
            let coast = KartInput {
                steer: input.steer,
                look_back: input.look_back,
                ..default()
            };
            input.set_if_neq(coast);
        }
    }
}

/// Checkpoints, laps and finish; the tow pelican's watch and carry. Issue #20: a kart under its
/// glider is not out of the track while it flies over the walls (it is if it lands beyond them).
fn update_progress(
    race: Res<Race>,
    course: Res<Course>,
    tuning: Res<RaceTuning>,
    mut racers: Query<
        (
            &Racer,
            &mut RaceProgress,
            &mut Tow,
            &KartInput,
            &mut Transform,
            &mut Velocity,
            &mut KartState,
            &mut Boost,
            &KartEnvironment,
        ),
        With<Kart>,
    >,
) {
    let started = race.started();
    let race_ticks = started.then_some(race.race_ticks());
    for (
        racer,
        mut progress,
        mut tow,
        input,
        mut transform,
        mut velocity,
        mut state,
        mut boost,
        environment,
    ) in &mut racers
    {
        let projection = course.project(progress.passed, transform.translation);
        // Issue #197: a glider crosses a gate at any height, as over a gap: climbing, it flies
        // more than `RaceTuning::gate_height` above the road beyond the gap (it lands far).
        let gate = Projection {
            gap: projection.gap || environment.gliding,
            ..projection
        };
        progress.update(&course, &gate, race_ticks, !tow.is_carrying());

        if let Some(mut carry) = tow.carry {
            // Carried: follow the pelican's path; dropped at a standstill on the drop point.
            let dropped = carry.advance();
            *transform = carry.pose(&tuning);
            *velocity = Velocity::zero();
            if dropped {
                tow.reset(transform.translation);
                *state = KartState::Grounded;
            } else {
                tow.carry = Some(carry);
            }
        } else if started && !(progress.finished() && course.is_open()) {
            // A finished racer's progress no longer follows it: if it drives on (bots), watch it
            // wherever it is on the track. Issue #119: on an open circuit, it coasts to a stop on
            // the run-out: nobody watches it any more.
            let watched = if progress.finished() {
                course.project_anywhere(transform.translation)
            } else {
                projection
            };
            let observation = TowObservation {
                position: transform.translation,
                trying_to_move: input.throttle > 0.5 || input.brake > 0.5,
                outside: !environment.gliding
                    && course.outside_walls(&watched, tuning.outside_margin),
                height: watched.height,
                upside_down: watched.upside_down,
            };
            if let Some(reason) = tow.observe(&tuning, &observation) {
                // Picked up: the drift, its charge and any boost are lost.
                let to = course.drop_transform(
                    progress.passed as i64 - 1,
                    racer.drop_lane(),
                    tuning.tow_drop_lane,
                );
                // Issue #96: out from under a ceiling first, never up through it.
                let clear = course.tow_clearance(transform.translation, tuning.tow_clearance);
                tow.carry = Some(Carry::new(&tuning, reason, *transform, to).clearing(clear));
                *state = KartState::Respawning;
                *boost = Boost::default();
                *velocity = Velocity::zero();
            }
        }
    }
}

/// Live ranking and distance to the leader. Online (issue #9), every racer of the race is here,
/// the other peers' karts too; once the race is over, the host's order.
fn rank_racers(
    mut race: ResMut<Race>,
    online: Option<Res<OnlineFlow>>,
    mut racers: Query<(Entity, &Racer, &mut RaceProgress)>,
) {
    let race = &mut *race;
    race.entries.clear();
    race.entries
        .extend(racers.iter().map(|(entity, racer, progress)| RankEntry {
            id: entity,
            grid_slot: racer.grid_slot,
            distance: progress.distance,
            finish: progress.finish,
        }));
    match online.as_ref().and_then(|online| online.order.as_deref()) {
        Some(order) => rank_by_order(&mut race.entries, order),
        None => rank(&mut race.entries),
    }
    race.standings.clear();
    race.standings.extend(race.entries.iter().map(|e| e.id));
    let Some(leader) = race.entries.first().map(|e| e.distance) else {
        return;
    };
    for (i, entry) in race.entries.iter().enumerate() {
        if let Ok((_, _, mut progress)) = racers.get_mut(entry.id) {
            progress.rank = i as u32 + 1;
            progress.gap_to_leader = distance_to_leader(leader, entry.distance);
        }
    }
}

/// Issue #70: the results close [`RaceTuning::results_close_seconds`] after they appeared: the
/// racers still on track get an estimated time (`results.rs`), in the order of the standings,
/// and the table is final. Online, every peer shows the host's table once it came
/// ([`OnlineFlow::results`]).
fn close_the_results(
    mut race: ResMut<Race>,
    course: Res<Course>,
    tuning: Res<RaceTuning>,
    online: Option<Res<OnlineFlow>>,
    racers: Query<(Entity, &Racer, &RaceProgress)>,
) {
    if race.results_closed() || race.phase() != RacePhase::Results {
        return;
    }
    let race = &mut *race;
    if let Some(online) = online {
        let Some(lines) = online.results.as_deref() else {
            return;
        };
        race.results.extend(lines.iter().filter_map(|line| {
            let (entity, ..) = racers
                .iter()
                .find(|(_, racer, _)| racer.grid_slot == line.id)?;
            Some(ResultLine {
                id: entity,
                time: line.time,
            })
        }));
        return;
    }
    let close = seconds_to_ticks(tuning.results_close_seconds);
    if !race.flow.results_closed(&race.timings, close) && !race.close_requested {
        return;
    }
    let standings = race.standings.iter().filter_map(|entity| {
        let (_, _, progress) = racers.get(*entity).ok()?;
        Some(Standing::of(*entity, progress, &course, false))
    });
    close_results(
        standings,
        race.flow.race_ticks(&race.timings),
        tuning.results_min_speed,
        &mut race.results,
    );
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    #[test]
    fn the_bots_take_the_front_of_the_grid_and_the_player_starts_behind() {
        let grid = RaceGrid::default();
        assert_eq!(grid.bot_count(), 11);
        assert_eq!(grid.bot_slots(), 0..11);
        assert_eq!(grid.player_slot(), 11);
        // Alone: on the pole, as before the bots.
        let solo = RaceGrid {
            bots: 0,
            ..default()
        };
        assert_eq!(solo.player_slot(), 0);
        assert!(solo.bot_slots().is_empty());
        // Never more than 12 racers: one slot always stays for the player.
        let crowded = RaceGrid {
            bots: 30,
            ..default()
        };
        assert_eq!(crowded.bot_count(), 11);
        assert_eq!(crowded.player_slot(), 11);
    }
}
