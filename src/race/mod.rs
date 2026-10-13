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
//! 3. [`RaceSystems::Rules`], after the physics step: checkpoints, laps, finish, tow pelican,
//!    ranking and distance to the leader.
//!
//! The race exists only on a circuit (`TrackLayout`): on the flat test ground there is none.

mod course;
mod flow;
mod pelican;
mod progress;
mod ranking;
mod tow;
mod tuning;

#[cfg(test)]
mod scenario_tests;
/// Prairie Pompon's course, for unit tests elsewhere.
#[cfg(test)]
pub(crate) use course::tests::prairie as course_for_tests;

pub use course::{Course, Projection, drop_point};
pub use flow::{FlowTimings, RaceFlow, RacePhase, race_over};
pub use pelican::{PelicanPlugin, PelicanVisual};
pub use progress::{ProgressEvent, RaceProgress};
pub use ranking::{RankEntry, distance_to_leader, rank};
pub use tow::{Carry, Tow, TowObservation, TowReason, tow_pose};
pub use tuning::{RaceTuning, seconds_to_ticks, ticks_to_seconds};

use bevy::prelude::*;
use bevy_rapier3d::prelude::{PhysicsSet, Velocity};

use crate::TICK_HZ;
use crate::interpolation::PhysicsPose;
use crate::kart::{
    Boost, Kart, KartInput, KartState, KartSystems, KartTuning, PlayerKart, Stall, StartOutcome,
    StartPress, start_outcome,
};
use crate::track::TrackLayout;

/// A kart taking part in the race. `grid_slot`: its starting place (0 = pole), where a restart
/// puts it back. See [`RaceGrid`] for who takes which slot.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(RaceProgress, Tow, RocketStart)]
pub struct Racer {
    pub grid_slot: usize,
}

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
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RaceGrid {
    /// Bots on the grid (issue #6), at most `GRID_SLOTS - 1`.
    pub bots: usize,
}

impl Default for RaceGrid {
    fn default() -> Self {
        Self {
            bots: crate::track::GRID_SLOTS - 1,
        }
    }
}

impl RaceGrid {
    /// Bots actually on the grid: one slot always stays for the player.
    pub fn bot_count(&self) -> usize {
        self.bots.min(crate::track::GRID_SLOTS - 1)
    }

    /// Grid slots of the bots: the front of the grid.
    pub fn bot_slots(&self) -> std::ops::Range<usize> {
        0..self.bot_count()
    }

    /// Grid slot of the local player: right behind the bots.
    pub fn player_slot(&self) -> usize {
        self.bot_count()
    }
}

/// A racer that keeps driving after the finish line instead of coasting (bots, issue #6:
/// coasting bots would block the straight for the racers still finishing).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DrivesOnAfterFinish;

/// The race in progress, created with the circuit. The static part lives in [`Course`].
#[derive(Resource, Debug)]
pub struct Race {
    pub flow: RaceFlow,
    pub timings: FlowTimings,
    /// Racers from first to last, refreshed every tick.
    pub standings: Vec<Entity>,
    /// A local player asked to race again (taken into account on the results screen only).
    pub restart_requested: bool,
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
}

/// Order of the race systems inside `FixedUpdate` (see the module documentation).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RaceSystems {
    Flow,
    Gate,
    Rules,
}

/// Keys and buttons of the results screen: race again (GD § 13 has no menu yet).
const RESTART_KEYS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space];
const RESTART_BUTTONS: [GamepadButton; 2] = [GamepadButton::South, GamepadButton::Start];

/// The race rules. Headless: tests run them with `MinimalPlugins`. The visual of the pelican is
/// [`PelicanPlugin`], the HUD is `crate::ui`.
pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RaceTuning>()
            .init_resource::<RaceGrid>()
            .configure_sets(
                FixedUpdate,
                (
                    RaceSystems::Flow.before(KartSystems::Input),
                    RaceSystems::Gate
                        .after(KartSystems::Input)
                        .before(KartSystems::Drive),
                    RaceSystems::Rules.after(PhysicsSet::Writeback),
                ),
            )
            .add_systems(
                Update,
                (
                    setup_race
                        .run_if(resource_exists::<TrackLayout>)
                        .run_if(not(resource_exists::<Race>)),
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
                    (update_progress, rank_racers)
                        .chain()
                        .in_set(RaceSystems::Rules),
                )
                    .run_if(resource_exists::<Race>),
            );
    }
}

fn setup_race(mut commands: Commands, layout: Res<TrackLayout>, tuning: Res<RaceTuning>) {
    commands.insert_resource(Course::new(&layout, &tuning));
    commands.insert_resource(Race::new(&tuning));
}

/// Results screen: Enter / Space or the gamepad's A / Start races again.
fn request_restart(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    mut race: ResMut<Race>,
) {
    if race.phase() != RacePhase::Results {
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

/// Moves the race clock; on a restart, puts every racer back on its grid slot.
fn advance_race(
    mut race: ResMut<Race>,
    course: Res<Course>,
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
        ),
        With<Kart>,
    >,
) {
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
    ) in &mut racers
    {
        let spawn = course.grid[racer.grid_slot.min(course.grid.len() - 1)];
        *transform = spawn;
        *velocity = Velocity::zero();
        *state = KartState::default();
        *boost = Boost::default();
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
/// finished racers coast (they keep their steering), unless they drive on (bots).
fn gate_inputs(
    race: Res<Race>,
    mut racers: Query<
        (
            &mut KartInput,
            &mut Boost,
            &mut Stall,
            &RaceProgress,
            Has<DrivesOnAfterFinish>,
        ),
        With<Racer>,
    >,
) {
    let started = race.started();
    for (mut input, mut boost, mut stall, progress, drives_on) in &mut racers {
        if !started {
            input.set_if_neq(KartInput::default());
            boost.set_if_neq(Boost::default());
            stall.set_if_neq(Stall::default());
        } else if progress.finished() && !drives_on {
            let coast = KartInput {
                steer: input.steer,
                ..default()
            };
            input.set_if_neq(coast);
        }
    }
}

/// Checkpoints, laps and finish; the tow pelican's watch and carry.
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
        ),
        With<Kart>,
    >,
) {
    let started = race.started();
    let race_ticks = started.then_some(race.race_ticks());
    for (racer, mut progress, mut tow, input, mut transform, mut velocity, mut state, mut boost) in
        &mut racers
    {
        let projection = course.project(progress.passed, transform.translation);
        progress.update(&course, &projection, race_ticks, !tow.is_carrying());

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
        } else if started {
            // A finished racer's progress no longer follows it: if it drives on (bots), watch it
            // wherever it is on the track.
            let watched = if progress.finished() {
                course.project_anywhere(transform.translation)
            } else {
                projection
            };
            let observation = TowObservation {
                position: transform.translation,
                trying_to_move: input.throttle > 0.5 || input.brake > 0.5,
                outside: course.outside_walls(&watched, tuning.outside_margin),
                height: watched.height,
            };
            if let Some(reason) = tow.observe(&tuning, &observation) {
                // Picked up: the drift, its charge and any boost are lost.
                let to = course.drop_transform(
                    progress.passed as i64 - 1,
                    racer.drop_lane(),
                    tuning.tow_drop_lane,
                );
                tow.carry = Some(Carry::new(&tuning, reason, *transform, to));
                *state = KartState::Respawning;
                *boost = Boost::default();
                *velocity = Velocity::zero();
            }
        }
    }
}

/// Live ranking and distance to the leader.
fn rank_racers(mut race: ResMut<Race>, mut racers: Query<(Entity, &Racer, &mut RaceProgress)>) {
    let race = &mut *race;
    race.entries.clear();
    race.entries
        .extend(racers.iter().map(|(entity, racer, progress)| RankEntry {
            id: entity,
            grid_slot: racer.grid_slot,
            distance: progress.distance,
            finish: progress.finish,
        }));
    rank(&mut race.entries);
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
        let solo = RaceGrid { bots: 0 };
        assert_eq!(solo.player_slot(), 0);
        assert!(solo.bot_slots().is_empty());
        // Never more than 12 racers: one slot always stays for the player.
        let crowded = RaceGrid { bots: 30 };
        assert_eq!(crowded.bot_count(), 11);
        assert_eq!(crowded.player_slot(), 11);
    }
}
