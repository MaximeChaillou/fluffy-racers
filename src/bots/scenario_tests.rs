//! Headless bot scenarios (issue #6): the whole game (`GamePlugin`) with `MinimalPlugins`, one
//! fixed tick per `app.update()`. Run with `--nocapture` to see each race's figures.
//!
//! **Races of bots over a set of seeds (issue #40).** A race of 12 bots is chaotic: another
//! race seed, or any change to the karts, the circuit or the bots, sends it down another path
//! (a shunt at the start, a kart over a wall). A race test therefore runs one race per seed of
//! a fixed set ([`RACE_SEEDS`], each race in its own thread) and checks the behaviour over the
//! set ([`Checks`]): every seed must clear a worst-case threshold, and the mean of the seeds a
//! mean threshold, with the margins printed. The tows of the pelican are sorted by cause
//! ([`TowCause`]): a pack incident is not a bot missing a turn. To try the thresholds on other
//! seeds: `FLUFFY_RACE_SEEDS=100-109 cargo test --lib bots::scenario_tests -- --nocapture`.
//!
//! **Without items (issue #16).** These races measure the bots' driving: they run with the items
//! switched off (`ItemTuning::enabled`), so that a spin out or a tumble does not blur what they
//! measure, with the same thresholds as before the items. One more race test runs with the
//! items, as in the game ([`bots_race_with_items`]): every race ends, the bots take boxes and use
//! their items, karts get hit, and no bot gets towed in free driving.
//!
//! **Without interactions (issue #34)** either: the paw strikes and the grabs between karts are
//! switched off in these races too (`InteractionTuning::enabled`), same thresholds. One more race
//! test runs with them ([`bots_race_with_interactions`]): the bots strike and grab, every race
//! ends, and no bot gets towed in free driving (a kart struck or held counts as an incident).

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    DefaultRapierContext, RapierContextColliders, RapierContextSimulation, RapierRigidBodySet,
};

use super::*;
use crate::TICK_HZ;
use crate::croquettes::Croquettes;
use crate::interactions::{InteractionTuning, Paw, PawStats};
use crate::items::{ItemStats, ItemTuning};
use crate::kart::{
    KartBodyPivot, KartInput, KartState, KartTuning, MiniTurbo, PlayerKart, StatEffects,
};
use crate::race::{
    Course, DrivesOnAfterFinish, Race, RaceGrid, RacePhase, RaceProgress, RaceTuning, Racer, Tow,
    TowReason, ticks_to_seconds,
};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};
use crate::track::{TrackChoice, TrackData, TrackHandle, TrackLayout};
use crate::ui::{DebugOverlay, HudText, SimulationCost};

// --- Seeds -------------------------------------------------------------------------------------

/// Race seeds of the race tests (`RaceGrid::seed`): the first eight, from the game's own seed (0)
/// on, none picked for its results.
const RACE_SEEDS: [u64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// Replaces [`RACE_SEEDS`]: a list (`7,12`), a range (`100-109`) or both (`1,100-104`).
const SEEDS_VARIABLE: &str = "FLUFFY_RACE_SEEDS";

/// The seeds the race tests run: [`RACE_SEEDS`], or those of [`SEEDS_VARIABLE`].
fn race_seeds() -> Vec<u64> {
    let Ok(spec) = std::env::var(SEEDS_VARIABLE) else {
        return RACE_SEEDS.to_vec();
    };
    let seed = |text: &str| {
        text.trim()
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("{SEEDS_VARIABLE}: {text:?} is not a seed"))
    };
    let seeds: Vec<u64> = spec
        .split(',')
        .flat_map(|part| match part.split_once('-') {
            Some((first, last)) => (seed(first)..=seed(last)).collect(),
            None => vec![seed(part)],
        })
        .collect();
    assert!(!seeds.is_empty(), "{SEEDS_VARIABLE}={spec:?}: no seed");
    seeds
}

/// Runs `race` once per seed, each in its own thread with its own App, side by side; the
/// results come back in the order of `seeds`.
fn for_each_seed<T: Send>(seeds: &[u64], race: impl Fn(u64) -> T + Sync) -> Vec<T> {
    let race = &race;
    std::thread::scope(|scope| {
        let runs: Vec<_> = seeds
            .iter()
            .map(|&seed| {
                std::thread::Builder::new()
                    .name(format!("race seed {seed}"))
                    .spawn_scoped(scope, move || race(seed))
                    .unwrap()
            })
            .collect();
        runs.into_iter()
            .map(|run| {
                run.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

// --- Race log ----------------------------------------------------------------------------------

/// A tow comes after a pack incident when the kart touched another kart at most this long
/// before its trouble began (s).
const PACK_WINDOW_SECONDS: f32 = 3.0;

/// Why the pelican came for a kart (issue #40).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TowCause {
    /// **Pack incident**: the kart touched another kart (their bodies in contact in Rapier, at
    /// any speed, a scrape included), or was hit by an item (issue #16: it spun out or
    /// tumbled), within [`PACK_WINDOW_SECONDS`] before its trouble began: a pile-up at the
    /// start, a shunt in the pack, an item. The trouble begins when the pelican starts
    /// counting: `outside_seconds` before the pick-up for a kart out of the track,
    /// `stuck_seconds` before it for a stuck kart, at the pick-up for a kart that fell.
    Pack,
    /// **Free driving**: no kart touched it in that time. The bot left the track or got stuck
    /// by itself: the sign of a real regression.
    FreeDriving,
}

/// How long the pelican waits before coming for `reason` (s): the kart's trouble began that
/// long before the pick-up.
fn pelican_wait(reason: TowReason, tuning: &RaceTuning) -> f32 {
    match reason {
        TowReason::OutOfTrack => tuning.outside_seconds,
        TowReason::Stuck => tuning.stuck_seconds,
        TowReason::Fell | TowReason::Requested => 0.0,
    }
}

/// The cause of a tow (issue #40): the kart last touched another kart `since_contact` seconds
/// before the pick-up (`None`: never), and the pelican waited `wait` seconds before coming.
fn tow_cause(since_contact: Option<f32>, wait: f32) -> TowCause {
    match since_contact {
        Some(seconds) if seconds <= wait + PACK_WINDOW_SECONDS => TowCause::Pack,
        _ => TowCause::FreeDriving,
    }
}

/// Issue #40: a tow follows a pack incident when the kart touched another kart within 3 s
/// before its trouble began (the pelican's wait before the pick-up), free driving otherwise.
#[test]
fn tows_are_sorted_by_cause() {
    let tuning = RaceTuning::default();
    let (fell, out, stuck) = (
        pelican_wait(TowReason::Fell, &tuning),
        pelican_wait(TowReason::OutOfTrack, &tuning),
        pelican_wait(TowReason::Stuck, &tuning),
    );
    assert_eq!((fell, out, stuck), (0.0, 1.0, 3.0));
    // Fell: a contact up to 3 s before the pick-up.
    assert_eq!(tow_cause(Some(0.0), fell), TowCause::Pack);
    assert_eq!(tow_cause(Some(3.0), fell), TowCause::Pack);
    assert_eq!(tow_cause(Some(3.1), fell), TowCause::FreeDriving);
    // Out of the track for 1 s: 4 s; stuck for 3 s: 6 s.
    assert_eq!(tow_cause(Some(4.0), out), TowCause::Pack);
    assert_eq!(tow_cause(Some(4.1), out), TowCause::FreeDriving);
    assert_eq!(tow_cause(Some(6.0), stuck), TowCause::Pack);
    assert_eq!(tow_cause(Some(6.1), stuck), TowCause::FreeDriving);
    // Never touched another kart.
    assert_eq!(tow_cause(None, stuck), TowCause::FreeDriving);
}

/// A tow of the pelican, as seen by the race log.
#[derive(Clone, Copy, Debug)]
struct TowEvent {
    reason: TowReason,
    cause: TowCause,
    /// Seconds since GO at the pick-up, and since the kart last touched another kart.
    race_time: f32,
    since_contact: Option<f32>,
    /// The lap it happened in (0: the first), and the time the pelican's rules took (s): its
    /// wait before coming ([`pelican_wait`]) and the carry.
    lap: usize,
    lost: f32,
    /// Centerline distance where the kart was picked up (m), and how far from the centerline
    /// compared to the inner face of the walls.
    at: f32,
    lateral: f32,
    wall: f32,
    finished: bool,
}

/// What happened to a kart during a race.
#[derive(Clone, Debug, Default)]
struct KartLog {
    slot: usize,
    difficulty: Option<BotDifficulty>,
    /// Mini-turbos fired, per tier, and their sequence.
    mini_turbos: [u32; 3],
    sequence: Vec<MiniTurbo>,
    /// Each mini-turbo fired, with where its drift started along the lap (m), and where the
    /// current drift started (issue #30: which big turn gave it).
    fired_at: Vec<(MiniTurbo, f32)>,
    drift_from: Option<f32>,
    /// Drifts released before the blue tier (no boost), and drifts that broke with the button
    /// still held (wall, too slow, too long in the air).
    wasted: u32,
    broken: u32,
    tows: Vec<TowEvent>,
    previous: Option<KartState>,
    /// Croquettes picked up and lost (issue #14), most held at once.
    croquettes_picked: u32,
    croquettes_lost: u32,
    croquettes_most: u8,
    croquettes_held: u8,
    /// Issue #40: the last tick of the log this kart touched another kart, and how many ticks
    /// it did.
    last_contact: Option<u32>,
    contact_ticks: u32,
}

impl KartLog {
    fn fired(&self) -> u32 {
        self.mini_turbos.iter().sum()
    }

    /// Oranges and purples.
    fn oranges(&self) -> u32 {
        self.mini_turbos[1] + self.mini_turbos[2]
    }

    /// Mini-turbos of at least `tier` whose drift started between `from` and `to` along the
    /// lap (m).
    fn fired_between(&self, tier: MiniTurbo, from: f32, to: f32) -> usize {
        self.fired_at
            .iter()
            .filter(|(t, at)| *t >= tier && (from..=to).contains(at))
            .count()
    }

    /// Longest run of consecutive mini-turbos of at least `tier`, measured in charge: the sum
    /// of their tiers' thresholds (`thresholds`, the kart's `mini_turbo_thresholds`).
    fn longest_chain_charge(&self, tier: MiniTurbo, thresholds: &[f32; 3]) -> f32 {
        let (mut best, mut run) = (0.0f32, 0.0);
        for t in &self.sequence {
            run = if *t >= tier {
                run + thresholds[t.index()]
            } else {
                0.0
            };
            best = best.max(run);
        }
        best
    }

    /// Tows of `cause`.
    fn tows_of(&self, cause: TowCause) -> usize {
        self.tows.iter().filter(|t| t.cause == cause).count()
    }
}

/// The contact graph of Rapier, to see which karts touch.
type RapierView = (
    &'static RapierContextSimulation,
    &'static RapierContextColliders,
    &'static RapierRigidBodySet,
);

/// Every kart of a race, followed tick by tick.
struct RaceLog {
    karts: Vec<(Entity, KartLog)>,
    /// Lateral positions of the racing karts on the start straight of Prairie Pompon (m).
    straight_laterals: Vec<f32>,
    ticks: u32,
    rapier: QueryState<RapierView, With<DefaultRapierContext>>,
}

impl RaceLog {
    fn new(app: &mut App) -> Self {
        let world = app.world_mut();
        let mut karts: Vec<(Entity, KartLog)> = world
            .query::<(Entity, &Racer, Option<&Bot>)>()
            .iter(world)
            .map(|(entity, racer, bot)| {
                let log = KartLog {
                    slot: racer.grid_slot,
                    difficulty: bot.map(|b| b.difficulty),
                    ..default()
                };
                (entity, log)
            })
            .collect();
        karts.sort_by_key(|(_, log)| log.slot);
        Self {
            karts,
            straight_laterals: Vec::new(),
            ticks: 0,
            rapier: world.query_filtered(),
        }
    }

    /// Records the tick that just ran: mini-turbos fired (a drift with a tier reached released
    /// with the button), contacts between karts, tows and their cause, lateral positions on the
    /// start straight. The tier is read with the kart's own thresholds (issue #15: its
    /// Mini-turbo stat moves them).
    fn record(&mut self, app: &App) {
        self.ticks += 1;
        let world = app.world();
        let base = world.resource::<KartTuning>();
        let course = world.resource::<Course>();
        let race_tuning = world.resource::<RaceTuning>();
        let race_time = ticks_to_seconds(world.resource::<Race>().race_ticks());
        let (simulation, colliders, bodies) = self.rapier.single(world).unwrap();
        for (entity, log) in &mut self.karts {
            // Issue #40: touching another kart this tick (any contact of the two bodies).
            let touching = simulation
                .contact_pairs_with(colliders, bodies, *entity)
                .any(|pair| {
                    let other = if pair.collider1() == Some(*entity) {
                        pair.collider2()
                    } else {
                        pair.collider1()
                    };
                    pair.has_any_active_contact()
                        && other.is_some_and(|other| world.get::<Racer>(other).is_some())
                });
            if touching {
                log.last_contact = Some(self.ticks);
                log.contact_ticks += 1;
            }

            let state = *world.get::<KartState>(*entity).unwrap();
            // Issue #16: hit by an item is an incident too (never in the races without items).
            if state.is_hit() {
                log.last_contact = Some(self.ticks);
            }
            // Issue #34: struck by a paw (its protection runs for 1 s after) or in a tether, too
            // (never in the races without interactions).
            if world
                .get::<Paw>(*entity)
                .is_some_and(|paw| paw.protected > 0 || paw.linked())
            {
                log.last_contact = Some(self.ticks);
            }
            let input = *world.get::<KartInput>(*entity).unwrap();
            let progress = world.get::<RaceProgress>(*entity).unwrap();
            let drifting = matches!(state, KartState::Drifting { .. });
            if drifting && !matches!(log.previous, Some(KartState::Drifting { .. })) {
                log.drift_from = Some(course.center.wrap(progress.distance));
            }
            if let Some(previous @ KartState::Drifting { .. }) = log.previous
                && !drifting
            {
                let tuning = world.get::<StatEffects>(*entity).unwrap().tuning(base);
                match (input.jump_drift, previous.mini_turbo(&tuning, false)) {
                    (true, _) => log.broken += 1,
                    (false, Some(tier)) => {
                        log.mini_turbos[tier.index()] += 1;
                        log.sequence.push(tier);
                        log.fired_at.push((tier, log.drift_from.unwrap_or(-1.0)));
                    }
                    (false, None) => log.wasted += 1,
                }
            }
            let tow = world.get::<Tow>(*entity).unwrap();
            if let Some(carry) = tow.carry
                && log.previous != Some(KartState::Respawning)
            {
                let p = course.project_anywhere(carry.from.translation);
                let counting = pelican_wait(carry.reason, race_tuning);
                let since_contact = log
                    .last_contact
                    .map(|tick| ticks_to_seconds(self.ticks - tick));
                log.tows.push(TowEvent {
                    reason: carry.reason,
                    cause: tow_cause(since_contact, counting),
                    race_time,
                    since_contact,
                    lap: progress.lap_times.len(),
                    lost: counting + race_tuning.tow_seconds,
                    at: p.distance,
                    lateral: p.lateral,
                    wall: p.wall,
                    finished: progress.finished(),
                });
            }
            log.previous = Some(state);
            let held = world.get::<Croquettes>(*entity).map_or(0, |c| c.count);
            if held > log.croquettes_held {
                log.croquettes_picked += (held - log.croquettes_held) as u32;
            } else {
                log.croquettes_lost += (log.croquettes_held - held) as u32;
            }
            log.croquettes_held = held;
            log.croquettes_most = log.croquettes_most.max(held);

            if progress.finished() || state == KartState::Respawning {
                continue;
            }
            let along = course.center.wrap(progress.distance);
            if (10.0..80.0).contains(&along) {
                let position = world.get::<Transform>(*entity).unwrap().translation;
                self.straight_laterals
                    .push(course.project(progress.passed, position).lateral);
            }
        }
    }

    /// Tows of karts that had finished.
    fn tows_after_the_finish(&self) -> Vec<(usize, TowEvent)> {
        self.karts
            .iter()
            .flat_map(|(_, log)| log.tows.iter().map(|t| (log.slot, *t)))
            .filter(|(_, t)| t.finished)
            .collect()
    }

    /// Every kart of the race and its tows, for `--nocapture`.
    fn report(&self, app: &App, label: &str) -> String {
        use std::fmt::Write;
        let mut out = format!("--- {label}: {} ticks\n", self.ticks);
        for (entity, log) in &self.karts {
            let progress = app.world().get::<RaceProgress>(*entity).unwrap();
            let laps: Vec<String> = progress
                .lap_times
                .iter()
                .map(|t| format!("{:.2}", ticks_to_seconds(*t)))
                .collect();
            let _ = writeln!(
                out,
                "slot {:2} {:?}: finish {:?} s, laps {:?}, mini-turbos blue/orange/purple {:?}, \
                 drifts without boost {}, broken {}, croquettes picked {} lost {} (at most {}, \
                 {} at the end), {:.1} s touching other karts",
                log.slot,
                log.difficulty,
                progress.finish.map(ticks_to_seconds),
                laps,
                log.mini_turbos,
                log.wasted,
                log.broken,
                log.croquettes_picked,
                log.croquettes_lost,
                log.croquettes_most,
                log.croquettes_held,
                ticks_to_seconds(log.contact_ticks),
            );
            if let Some(paws) = app
                .world()
                .get::<PawStats>(*entity)
                .filter(|paws| **paws != PawStats::default())
            {
                let _ = writeln!(
                    out,
                    "    paw: {} strikes, {} landed, {} missed, {} grabs, {} throws; struck {} \
                     times, held {} times, broke free {} times",
                    paws.strikes,
                    paws.landed,
                    paws.missed,
                    paws.grabs,
                    paws.throws,
                    paws.struck,
                    paws.held,
                    paws.escapes
                );
            }
            if let Some(items) = app
                .world()
                .get::<ItemStats>(*entity)
                .filter(|items| items.boxes > 0)
            {
                let _ = writeln!(
                    out,
                    "    items: {} boxes, {} used, hit {} times, {} hits landed, {} blocked",
                    items.boxes, items.used, items.hits_taken, items.hits_landed, items.blocked
                );
            }
            for tow in &log.tows {
                let _ = writeln!(
                    out,
                    "    towed ({:?}, {:?}) {:.1} s after GO (lap {}), last contact {} ago, at \
                     {:.0} m along the lap, {:.1} m from the centerline (walls at {:.1} m){}",
                    tow.reason,
                    tow.cause,
                    tow.race_time,
                    tow.lap + 1,
                    tow.since_contact
                        .map_or("never".to_string(), |s| format!("{s:.1} s")),
                    tow.at,
                    tow.lateral,
                    tow.wall,
                    if tow.finished {
                        ", after the finish"
                    } else {
                        ""
                    }
                );
            }
        }
        if !self.straight_laterals.is_empty() {
            let _ = writeln!(
                out,
                "lateral spread on the start straight: {:.2} m ({} samples)",
                spread(&self.straight_laterals),
                self.straight_laterals.len()
            );
        }
        out
    }
}

// --- Checks over the seeds -----------------------------------------------------------------------

/// How a value compares to its threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    AtLeast,
    Above,
    AtMost,
    Below,
}

impl Bound {
    fn holds(self, value: f32, threshold: f32) -> bool {
        match self {
            Bound::AtLeast => value >= threshold,
            Bound::Above => value > threshold,
            Bound::AtMost => value <= threshold,
            Bound::Below => value < threshold,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Bound::AtLeast => ">=",
            Bound::Above => ">",
            Bound::AtMost => "<=",
            Bound::Below => "<",
        }
    }

    /// How far `value` is from failing (positive: it holds).
    fn margin(self, value: f32, threshold: f32) -> f32 {
        match self {
            Bound::AtLeast | Bound::Above => value - threshold,
            Bound::AtMost | Bound::Below => threshold - value,
        }
    }

    /// The worst of `values` for this bound (NaN ignored: `f32::min` and `max` skip it).
    fn worst(self, values: &[f32]) -> f32 {
        let fold = |f: fn(f32, f32) -> f32, start| values.iter().copied().fold(start, f);
        match self {
            Bound::AtLeast | Bound::Above => fold(f32::min, f32::INFINITY),
            Bound::AtMost | Bound::Below => fold(f32::max, f32::NEG_INFINITY),
        }
    }
}

/// A number for the table: integers without decimals.
fn number(value: f32) -> String {
    if value.is_nan() {
        "-".to_string()
    } else if value.is_infinite() {
        "inf".to_string()
    } else if value.fract() == 0.0 && value.abs() < 1e6 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

/// The checks of a race test over its seeds (issue #40). Each one takes a value per seed (one
/// race each): every seed must clear the worst-case threshold, their mean the mean threshold. A
/// value that does not apply to a race is NaN (shown as "-"): it counts in neither. `finish`
/// prints the table, with the margin left on each threshold, then fails with every check that
/// does not hold.
struct Checks {
    title: String,
    seeds: Vec<u64>,
    table: String,
    failures: Vec<String>,
}

impl Checks {
    fn new(title: &str, seeds: &[u64]) -> Self {
        Self {
            title: title.to_string(),
            seeds: seeds.to_vec(),
            table: String::new(),
            failures: Vec::new(),
        }
    }

    /// `values`, one per seed, against `worst` (every seed) and `mean` (their mean).
    fn check(
        &mut self,
        name: &str,
        values: &[f32],
        bound: Bound,
        worst: Option<f32>,
        mean: Option<f32>,
    ) {
        use std::fmt::Write;
        assert_eq!(values.len(), self.seeds.len(), "{name}");
        let observed_worst = bound.worst(values);
        // With no value that applies, the check fails (NaN holds against no threshold).
        let applying: Vec<f32> = values.iter().copied().filter(|v| !v.is_nan()).collect();
        let (observed_worst, observed_mean) = if applying.is_empty() {
            (f32::NAN, f32::NAN)
        } else {
            (
                observed_worst,
                applying.iter().sum::<f32>() / applying.len() as f32,
            )
        };
        let mut row = format!(
            "{name:<58} [{}]",
            values
                .iter()
                .map(|v| number(*v))
                .collect::<Vec<_>>()
                .join(" ")
        );
        for (label, threshold, observed) in [
            ("worst", worst, observed_worst),
            ("mean", mean, observed_mean),
        ] {
            let Some(threshold) = threshold else {
                continue;
            };
            let holds = bound.holds(observed, threshold);
            let _ = write!(
                row,
                "  {label} {} {} {} (margin {}){}",
                number(observed),
                bound.symbol(),
                number(threshold),
                number(bound.margin(observed, threshold)),
                if holds { "" } else { "  FAILED" }
            );
            if !holds {
                self.failures.push(format!(
                    "{name}: {label} {} not {} {} (values {values:?})",
                    number(observed),
                    bound.symbol(),
                    number(threshold)
                ));
            }
        }
        let _ = writeln!(self.table, "{row}");
    }

    fn at_least(&mut self, name: &str, values: &[f32], worst: Option<f32>, mean: Option<f32>) {
        self.check(name, values, Bound::AtLeast, worst, mean);
    }

    fn above(&mut self, name: &str, values: &[f32], worst: Option<f32>, mean: Option<f32>) {
        self.check(name, values, Bound::Above, worst, mean);
    }

    fn at_most(&mut self, name: &str, values: &[f32], worst: Option<f32>, mean: Option<f32>) {
        self.check(name, values, Bound::AtMost, worst, mean);
    }

    fn below(&mut self, name: &str, values: &[f32], worst: Option<f32>, mean: Option<f32>) {
        self.check(name, values, Bound::Below, worst, mean);
    }

    /// A rule every race must follow (no margin to show).
    fn every_race(&mut self, name: &str, holds: impl IntoIterator<Item = bool>) {
        use std::fmt::Write;
        let failed: Vec<u64> = self
            .seeds
            .iter()
            .zip(holds)
            .filter(|(_, h)| !*h)
            .map(|(s, _)| *s)
            .collect();
        let _ = writeln!(
            self.table,
            "{name:<58} {}",
            if failed.is_empty() {
                "every seed".to_string()
            } else {
                format!("FAILED on seeds {failed:?}")
            }
        );
        if !failed.is_empty() {
            self.failures
                .push(format!("{name}: fails on seeds {failed:?}"));
        }
    }

    fn finish(self) {
        println!(
            "=== {} — seeds {:?}\n{}",
            self.title, self.seeds, self.table
        );
        assert!(
            self.failures.is_empty(),
            "{} (seeds {:?}):\n{}",
            self.title,
            self.seeds,
            self.failures.join("\n")
        );
    }
}

/// Standard deviation.
fn spread(values: &[f32]) -> f32 {
    let n = values.len().max(1) as f32;
    let mean = values.iter().sum::<f32>() / n;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len().max(1) as f32
}

fn lowest(values: &[f32]) -> f32 {
    values.iter().copied().fold(f32::INFINITY, f32::min)
}

fn highest(values: &[f32]) -> f32 {
    values.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}

fn race(app: &App) -> &Race {
    app.world().resource::<Race>()
}

fn progress(app: &App, kart: Entity) -> RaceProgress {
    app.world().get::<RaceProgress>(kart).unwrap().clone()
}

/// Runs updates until the race and the `bots` bots exist.
fn wait_for_the_bots(app: &mut App, bots: usize) {
    for _ in 0..10 {
        let world = app.world_mut();
        let spawned = world.query::<&Bot>().iter(world).count();
        if spawned == bots && world.get_resource::<Race>().is_some() {
            return;
        }
        app.update();
    }
    panic!("the bots or the race never came");
}

/// The game on `track` with `bots` bots and the race seed `seed`, before its first update,
/// without items (issue #16) nor interactions between karts (issue #34): these races measure the
/// driving.
fn seeded_app(bots: usize, seed: u64) -> App {
    seeded_app_with(bots, seed, false)
}

/// The same, with or without the items (and without the interactions).
fn seeded_app_with(bots: usize, seed: u64, items: bool) -> App {
    seeded_app_full(bots, seed, items, false)
}

/// The same, with or without the items and the interactions between karts.
fn seeded_app_full(bots: usize, seed: u64, items: bool, interactions: bool) -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), bots);
    app.world_mut().resource_mut::<RaceGrid>().seed = seed;
    app.world_mut().resource_mut::<ItemTuning>().enabled = items;
    app.world_mut().resource_mut::<InteractionTuning>().enabled = interactions;
    single_threaded(&mut app);
    app
}

/// The game on Prairie Pompon with `bots` bots and the race seed `seed`, once the race and the
/// bots exist.
fn race_with_bots(bots: usize, seed: u64) -> App {
    let mut app = seeded_app(bots, seed);
    wait_for_the_player(&mut app);
    wait_for_the_bots(&mut app, bots);
    app
}

/// The game on the circuit `data` (built in the test) with `bots` bots and the race seed `seed`.
fn race_on(data: TrackData, bots: usize, seed: u64) -> App {
    let mut app = seeded_app(bots, seed);
    // A circuit handed over directly: no file is loaded.
    let handle = app
        .world_mut()
        .resource_mut::<Assets<TrackData>>()
        .add(data);
    app.insert_resource(TrackHandle(handle));
    wait_for_the_player(&mut app);
    wait_for_the_bots(&mut app, bots);
    app
}

/// A round circuit (radius 28 m, 176 m): one endless right turn, long enough for an orange
/// mini-turbo, unlike the 180° turns of Prairie Pompon.
fn roundabout() -> TrackData {
    let points: String = (0..12)
        .map(|k| {
            let a = (k as f32 * 30.0).to_radians();
            format!(
                "(pos: ({:.3}, 0.0, {:.3})),",
                28.0 * a.cos(),
                28.0 * a.sin()
            )
        })
        .collect();
    let data = TrackData::from_ron(&format!("(name: \"Rond-point\", points: [{points}])")).unwrap();
    data.validate().unwrap();
    data
}

/// The two big turns of Prairie Pompon, the hairpin then the banked turn, as the stretch along
/// the lap where a drift for them starts (m): from a little before the turn (a bot hops just
/// before it) to its end.
fn big_turns(app: &App) -> Vec<(f32, f32)> {
    let line = app.world().resource::<RacingLine>();
    line.turns()
        .iter()
        .filter(|turn| turn.angle > 2.4)
        .map(|turn| {
            let start = turn.start as f32 * line.step();
            (start - 20.0, start + turn.len as f32 * line.step())
        })
        .collect()
}

fn set_difficulties(app: &mut App, difficulty: impl Fn(usize) -> BotDifficulty) {
    let world = app.world_mut();
    for (mut bot, racer) in world.query::<(&mut Bot, &Racer)>().iter_mut(world) {
        bot.difficulty = difficulty(racer.grid_slot);
    }
}

/// The player's kart is driven by a bot too (a race of bots only), with the seed of its slot.
fn player_becomes_a_bot(app: &mut App, difficulty: BotDifficulty) {
    let kart = player(app);
    let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
    let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
    let tuning = app.world().resource::<BotTuning>().clone();
    app.world_mut()
        .entity_mut(kart)
        .remove::<PlayerKart>()
        .insert(bot(difficulty, seed, &tuning));
}

/// Runs the race until every racer has finished (or `max_seconds` of race), logging every tick.
fn run_race(app: &mut App, log: &mut RaceLog, max_seconds: f32) {
    let max_ticks = (max_seconds * TICK_HZ as f32) as u32 + race(app).timings.before_go();
    while log.ticks < max_ticks {
        app.update();
        log.record(app);
        let world = app.world_mut();
        if world
            .query::<&RaceProgress>()
            .iter(world)
            .all(|p| p.finished())
        {
            return;
        }
    }
}

/// A kart at the end of a race, in plain numbers (the App stays in its thread).
#[derive(Clone, Debug)]
struct KartResult {
    log: KartLog,
    finish: Option<f32>,
    laps: Vec<f32>,
    /// Its laps without the time the pelican's rules took after a pack incident (its wait
    /// before coming and the carry): what the bot's driving made of the lap.
    laps_without_pelican: Vec<f32>,
    /// Its own mini-turbo thresholds (issue #15: its Mini-turbo stat).
    thresholds: [f32; 3],
    /// Its items (issue #16): boxes, items used, hits taken and landed.
    items: ItemStats,
    /// Its paw (issue #34): strikes, grabs, throws, struck, held, escapes.
    paws: PawStats,
}

impl KartResult {
    fn is(&self, difficulty: BotDifficulty) -> bool {
        self.log.difficulty == Some(difficulty)
    }
}

/// A race of the seed set, in plain numbers.
#[derive(Clone, Debug)]
struct RaceResult {
    karts: Vec<KartResult>,
    /// Laps driven by all the karts, partial laps included.
    kart_laps: f32,
    lateral_spread: f32,
    /// Printed with `--nocapture`, in the order of the seeds.
    report: String,
}

impl RaceResult {
    fn of(app: &App, log: &RaceLog, label: &str) -> Self {
        let base = app.world().resource::<KartTuning>();
        let karts = log
            .karts
            .iter()
            .map(|(entity, kart)| {
                let p = progress(app, *entity);
                let laps: Vec<f32> = p.lap_times.iter().map(|t| ticks_to_seconds(*t)).collect();
                let laps_without_pelican = laps
                    .iter()
                    .enumerate()
                    .map(|(lap, time)| {
                        let pelican: f32 = kart
                            .tows
                            .iter()
                            .filter(|t| t.lap == lap && t.cause == TowCause::Pack)
                            .map(|t| t.lost)
                            .sum();
                        time - pelican
                    })
                    .collect();
                KartResult {
                    log: kart.clone(),
                    finish: p.finish.map(ticks_to_seconds),
                    laps,
                    laps_without_pelican,
                    thresholds: app
                        .world()
                        .get::<StatEffects>(*entity)
                        .unwrap()
                        .tuning(base)
                        .mini_turbo_thresholds,
                    items: app
                        .world()
                        .get::<ItemStats>(*entity)
                        .copied()
                        .unwrap_or_default(),
                    paws: app
                        .world()
                        .get::<PawStats>(*entity)
                        .copied()
                        .unwrap_or_default(),
                }
            })
            .collect();
        let course = app.world().resource::<Course>();
        let kart_laps = log
            .karts
            .iter()
            .map(|(entity, _)| (progress(app, *entity).distance / course.length()).max(0.0))
            .sum();
        Self {
            karts,
            kart_laps,
            lateral_spread: spread(&log.straight_laterals),
            report: log.report(app, label),
        }
    }

    fn of_difficulty(&self, difficulty: BotDifficulty) -> impl Iterator<Item = &KartResult> {
        self.karts.iter().filter(move |k| k.is(difficulty))
    }

    fn all_finished(&self) -> bool {
        self.karts.iter().all(|k| k.finish.is_some())
    }

    fn finishes(&self) -> Vec<f32> {
        self.karts.iter().filter_map(|k| k.finish).collect()
    }

    /// Tows of `cause`.
    fn tows_of(&self, cause: TowCause) -> usize {
        self.karts.iter().map(|k| k.log.tows_of(cause)).sum()
    }

    fn tows(&self) -> usize {
        self.tows_of(TowCause::Pack) + self.tows_of(TowCause::FreeDriving)
    }

    /// The times of the laps `laps` (0: the first) of every kart.
    fn laps(&self, laps: std::ops::Range<usize>) -> Vec<f32> {
        self.karts
            .iter()
            .flat_map(|k| k.laps.get(laps.clone()).unwrap_or_default())
            .copied()
            .collect()
    }

    /// The same, without the pelican's time after a pack incident.
    fn laps_without_pelican(&self, laps: std::ops::Range<usize>) -> Vec<f32> {
        self.karts
            .iter()
            .flat_map(|k| k.laps_without_pelican.get(laps.clone()).unwrap_or_default())
            .copied()
            .collect()
    }
}

/// Tows in free driving (issue #40, [`TowCause::FreeDriving`]), as a share of the laps the karts
/// drove: at most this on average over the seeds (none in the 100 races measured for #40).
const FREE_DRIVING_TOWS_PER_KART_LAP: f32 = 0.01;

/// The tows of a race test (issue #40): of every cause, at most `worst` in a race and `mean` a
/// race on average; in free driving, never two in a race, and at most
/// [`FREE_DRIVING_TOWS_PER_KART_LAP`].
fn tow_checks(checks: &mut Checks, races: &[&RaceResult], worst: f32, mean: f32) {
    checks.at_most(
        "tows (every cause)",
        &per_race(races, |r| r.tows() as f32),
        Some(worst),
        Some(mean),
    );
    checks.at_most(
        "tows in free driving",
        &per_race(races, |r| r.tows_of(TowCause::FreeDriving) as f32),
        Some(1.0),
        None,
    );
    checks.at_most(
        "tows in free driving per kart-lap",
        &per_race(races, |r| {
            r.tows_of(TowCause::FreeDriving) as f32 / r.kart_laps.max(1.0)
        }),
        None,
        Some(FREE_DRIVING_TOWS_PER_KART_LAP),
    );
}

/// `f` of each race, as the values of a check.
fn per_race<R>(races: &[R], f: impl Fn(&R) -> f32) -> Vec<f32> {
    races.iter().map(f).collect()
}

/// The log of every race, in the order of the seeds (`--nocapture`).
fn print_reports<'a>(seeds: &[u64], races: impl Iterator<Item = &'a RaceResult>) {
    for (seed, race) in seeds.iter().zip(races) {
        println!("seed {seed}\n{}", race.report);
    }
}

/// Issue #6 acceptance (load test): the game's grid on Prairie Pompon, 11 bots, plus the
/// player's kart driven by a bot too: 12 karts race 3 laps without getting stuck. Issue #40: one
/// race per seed.
#[test]
fn twelve_karts_race_three_laps_without_getting_stuck() {
    // The game's default: 11 bots, all Normal, the game's seed.
    let grid = RaceGrid::default();
    assert_eq!((grid.bots, grid.seed), (11, RaceGrid::GAME_SEED));
    assert_eq!(BotTuning::default().grid_difficulty, BotDifficulty::Normal);
    struct Run {
        race: RaceResult,
        over: bool,
        turns: Vec<(f32, f32)>,
        driven_after: Vec<f32>,
        drive_on: bool,
        /// Tows of karts that had finished (slot, tow): the 5 s after the finish included.
        tows_after_the_finish: Vec<(usize, TowEvent)>,
    }
    let seeds = race_seeds();
    let runs = for_each_seed(&seeds, |seed| {
        let mut app = race_with_bots(11, seed);
        player_becomes_a_bot(&mut app, BotDifficulty::Normal);
        let mut log = RaceLog::new(&mut app);
        assert_eq!(log.karts.len(), 12);
        let started = std::time::Instant::now();
        run_race(&mut app, &mut log, 120.0);
        let elapsed = started.elapsed().as_secs_f64();
        let mut result = RaceResult::of(&app, &log, "12 karts, Normal");
        result.report += &format!(
            "wall time {elapsed:.2} s for {} ticks: {:.3} ms per tick (test profile, whole app)\n",
            log.ticks,
            elapsed * 1000.0 / log.ticks as f64
        );
        // Without a player, the race is over once everybody has finished.
        app.update();
        let over = race(&app).phase() == RacePhase::Finished;

        // After the finish the bots drive on (they do not block the line), and the pelican does
        // not mistake them for karts out of the track.
        let position =
            |app: &App, entity: Entity| app.world().get::<Transform>(entity).unwrap().translation;
        let mut last: Vec<Vec3> = log.karts.iter().map(|(e, _)| position(&app, *e)).collect();
        let mut driven = vec![0.0; last.len()];
        for _ in 0..(5.0 * TICK_HZ) as usize {
            app.update();
            log.record(&app);
            for (i, (entity, _)) in log.karts.iter().enumerate() {
                let now = position(&app, *entity);
                driven[i] += now.distance(last[i]);
                last[i] = now;
            }
        }
        let drive_on = log
            .karts
            .iter()
            .all(|(e, _)| app.world().get::<DrivesOnAfterFinish>(*e).is_some());
        let tows_after_the_finish = log.tows_after_the_finish();
        for (slot, tow) in &tows_after_the_finish {
            result.report += &format!("after the finish, slot {slot} towed: {tow:?}\n");
        }
        Run {
            race: result,
            over,
            turns: big_turns(&app),
            driven_after: driven,
            drive_on,
            tows_after_the_finish,
        }
    });
    print_reports(&seeds, runs.iter().map(|r| &r.race));

    let mut checks = Checks::new("12 karts, Normal grid (the game's)", &seeds);
    checks.every_race(
        "every kart finishes",
        runs.iter().map(|r| r.race.all_finished()),
    );
    checks.every_race(
        "the race is over once all have finished",
        runs.iter().map(|r| r.over),
    );
    let finishes = |r: &Run| r.race.finishes();
    checks.below(
        "slowest finish (s)",
        &per_race(&runs, |r| highest(&finishes(r))),
        Some(75.0),
        None,
    );
    checks.at_least(
        "fastest finish (s)",
        &per_race(&runs, |r| lowest(&finishes(r))),
        Some(50.0),
        None,
    );
    checks.at_least(
        "average finish (s)",
        &per_race(&runs, |r| mean(&finishes(r))),
        Some(56.5),
        Some(57.5),
    );
    checks.at_most(
        "average finish (s)",
        &per_race(&runs, |r| mean(&finishes(r))),
        Some(62.5),
        Some(61.5),
    );
    // About 19 s a lap; the first one starts from the grid, up to 38 m behind the line, in the
    // traffic of the start. Issue #40: a lap where the kart was towed after a pack incident is
    // timed without the pelican's own time (its wait and the carry).
    let first_laps = |r: &Run| r.race.laps_without_pelican(0..1);
    let later_laps = |r: &Run| r.race.laps_without_pelican(1..3);
    checks.below(
        "slowest first lap, without the pelican (s)",
        &per_race(&runs, |r| highest(&first_laps(r))),
        Some(32.0),
        None,
    );
    checks.below(
        "slowest lap 2 or 3, without the pelican (s)",
        &per_race(&runs, |r| highest(&later_laps(r))),
        Some(26.0),
        None,
    );
    checks.at_least(
        "fastest lap (s)",
        &per_race(&runs, |r| lowest(&r.race.laps(0..3))),
        Some(16.0),
        None,
    );
    checks.at_least(
        "average lap 2 and 3 (s)",
        &per_race(&runs, |r| mean(&r.race.laps(1..3))),
        Some(17.5),
        Some(18.0),
    );
    checks.at_most(
        "average lap 2 and 3 (s)",
        &per_race(&runs, |r| mean(&r.race.laps(1..3))),
        Some(20.5),
        Some(20.0),
    );
    // Hardly ever towed: a pile-up at the start or a shunt in the pack may cost a few tows (a
    // kart thrown over a wall; far fewer since #27), the bots alone (almost) never.
    tow_checks(
        &mut checks,
        &runs.iter().map(|r| &r.race).collect::<Vec<_>>(),
        5.0,
        1.5,
    );
    // The pelican does not mistake a kart that drives on after the finish for a kart out of
    // the track (a bug of #6): no tow after the finish in free driving. A shunt with the karts
    // still racing may still send one over a wall.
    checks.every_race(
        "no tow after the finish in free driving",
        runs.iter().map(|r| {
            r.tows_after_the_finish
                .iter()
                .all(|(_, t)| t.cause == TowCause::Pack)
        }),
    );
    // They drift. Since issue #30 the big turns of Prairie Pompon are long enough for an orange,
    // which the Normal bots aim for: most of their mini-turbos are orange, in both turns.
    checks.at_least(
        "karts firing a mini-turbo",
        &per_race(&runs, |r| {
            r.race.karts.iter().filter(|k| k.log.fired() > 0).count() as f32
        }),
        Some(10.0),
        Some(11.5),
    );
    let total = |r: &Run| r.race.karts.iter().map(|k| k.log.fired()).sum::<u32>() as f32;
    let oranges = |r: &Run| {
        r.race
            .karts
            .iter()
            .map(|k| k.log.mini_turbos[1])
            .sum::<u32>() as f32
    };
    checks.at_least(
        "mini-turbos",
        &per_race(&runs, total),
        Some(24.0),
        Some(40.0),
    );
    checks.at_least(
        "orange mini-turbos",
        &per_race(&runs, oranges),
        Some(24.0),
        Some(35.0),
    );
    checks.at_least(
        "orange share of the mini-turbos",
        &per_race(&runs, |r| oranges(r) / total(r).max(1.0)),
        Some(0.5),
        Some(0.75),
    );
    checks.every_race("two big turns", runs.iter().map(|r| r.turns.len() == 2));
    checks.at_least(
        "oranges in the big turn with the fewest",
        &per_race(&runs, |r| {
            lowest(
                &r.turns
                    .iter()
                    .map(|(from, to)| {
                        r.race
                            .karts
                            .iter()
                            .map(|k| k.log.fired_between(MiniTurbo::Orange, *from, *to))
                            .sum::<usize>() as f32
                    })
                    .collect::<Vec<_>>(),
            )
        }),
        Some(10.0),
        Some(15.0),
    );
    // The bots release their drifts themselves: hardly any drift breaks (wall, too slow).
    checks.at_most(
        "broken drifts",
        &per_race(&runs, |r| {
            r.race.karts.iter().map(|k| k.log.broken).sum::<u32>() as f32
        }),
        Some(3.0),
        Some(1.0),
    );
    // Issue #14: driving over them, the bots pick the croquettes of the circuit up (the leaders
    // the most: the lines are empty for 10 s after them).
    checks.at_least(
        "karts picking croquettes up",
        &per_race(&runs, |r| {
            r.race
                .karts
                .iter()
                .filter(|k| k.log.croquettes_picked > 0)
                .count() as f32
        }),
        Some(2.0),
        Some(4.2),
    );
    checks.at_least(
        "croquettes picked up",
        &per_race(&runs, |r| {
            r.race
                .karts
                .iter()
                .map(|k| k.log.croquettes_picked)
                .sum::<u32>() as f32
        }),
        Some(12.0),
        Some(20.0),
    );
    // No single file: spread across the straight, and spread at the finish.
    checks.above(
        "lateral spread on the start straight (m)",
        &per_race(&runs, |r| r.race.lateral_spread),
        Some(0.8),
        Some(1.4),
    );
    checks.above(
        "first to last at the finish (s)",
        &per_race(&runs, |r| highest(&finishes(r)) - lowest(&finishes(r))),
        Some(1.0),
        Some(4.0),
    );
    // After the finish they drive on (5 s once all have finished; the first ones may be in the
    // hairpin by then).
    checks.above(
        "least distance driven in the 5 s after the finish (m)",
        &per_race(&runs, |r| lowest(&r.driven_after)),
        Some(40.0),
        Some(100.0),
    );
    checks.every_race(
        "every kart drives on after the finish",
        runs.iter().map(|r| r.drive_on),
    );
    checks.finish();
}

/// Issue #6: the three difficulties drive differently. A mixed grid (Easy, Normal, Hard in
/// turn): Hard finishes first on average, then Normal, then Easy; Easy releases its drifts at
/// blue only; Hard fires a mini-turbo in most big turns.
///
/// Issue #30 acceptance: with the wider drift, the big turns of Prairie Pompon give an orange.
/// Every Hard bot fires one in the hairpin and one in the banked turn (the drift of #4 gave blue
/// only, see `charge_per_radian`), and Normal, which aims for orange too, fires some.
///
/// Issue #40: one race per seed. What held in the one race is now a rate over the races: a shunt
/// can hold a Hard bot back for a lap (an orange missed, three Hard bots behind slower ones).
#[test]
fn the_three_difficulties_drive_differently() {
    let difficulty = |slot: usize| BotDifficulty::ALL[slot % 3];
    struct Run {
        race: RaceResult,
        turns: Vec<(f32, f32)>,
    }
    let seeds = race_seeds();
    let runs = for_each_seed(&seeds, |seed| {
        let mut app = race_with_bots(11, seed);
        set_difficulties(&mut app, difficulty);
        player_becomes_a_bot(&mut app, difficulty(11));
        let mut log = RaceLog::new(&mut app);
        run_race(&mut app, &mut log, 120.0);
        Run {
            race: RaceResult::of(&app, &log, "12 karts, mixed difficulties"),
            turns: big_turns(&app),
        }
    });
    print_reports(&seeds, runs.iter().map(|r| &r.race));

    let mut checks = Checks::new("12 karts, mixed grid (Easy, Normal, Hard)", &seeds);
    checks.every_race(
        "every kart finishes",
        runs.iter().map(|r| r.race.all_finished()),
    );
    checks.every_race("two big turns", runs.iter().map(|r| r.turns.len() == 2));
    use BotDifficulty::{Easy, Hard, Normal};
    let of = |r: &Run, d: BotDifficulty| r.race.of_difficulty(d).cloned().collect::<Vec<_>>();
    let average = |r: &Run, d| mean(&of(r, d).iter().filter_map(|k| k.finish).collect::<Vec<_>>());
    // Hard finishes first on average, then Normal, then Easy: by seconds over the seeds, in
    // most races (seed 308: three Hard bots stuck behind slower ones finished just after the
    // Normal ones), and never far the other way.
    checks.at_least(
        "races in the order Hard, Normal, Easy",
        &per_race(&runs, |r| {
            let (easy, normal, hard) = (average(r, Easy), average(r, Normal), average(r, Hard));
            if hard < normal && normal < easy {
                1.0
            } else {
                0.0
            }
        }),
        None,
        Some(0.75),
    );
    checks.above(
        "Normal minus Hard, average finish (s)",
        &per_race(&runs, |r| average(r, Normal) - average(r, Hard)),
        Some(-2.0),
        Some(3.0),
    );
    checks.above(
        "Easy minus Normal, average finish (s)",
        &per_race(&runs, |r| average(r, Easy) - average(r, Normal)),
        Some(-2.0),
        Some(2.5),
    );
    // Lap times (laps 2 and 3, out of the start, without the pelican's time after a pack
    // incident) in each difficulty's own window: tight on average, wider in a race (a shunt
    // slows down the karts around it).
    for (d, [low, mean_low, mean_high, high]) in [
        (Easy, [18.5, 19.6, 20.9, 23.0]),
        (Normal, [17.0, 18.2, 19.5, 21.0]),
        (Hard, [15.5, 16.6, 17.8, 19.5]),
    ] {
        let laps = |r: &Run| {
            mean(
                &of(r, d)
                    .iter()
                    .flat_map(|k| k.laps_without_pelican.iter().skip(1).copied())
                    .collect::<Vec<_>>(),
            )
        };
        let name = format!("{d:?}: average lap 2 and 3 (s)");
        checks.at_least(&name, &per_race(&runs, laps), Some(low), Some(mean_low));
        checks.at_most(&name, &per_race(&runs, laps), Some(high), Some(mean_high));
    }
    // Easy releases at blue.
    checks.at_most(
        "Easy: oranges and purples",
        &per_race(&runs, |r| {
            of(r, Easy).iter().map(|k| k.log.oranges()).sum::<u32>() as f32
        }),
        Some(0.0),
        None,
    );
    // Two big turns a lap: 6 chances for a mini-turbo.
    let fired = |r: &Run, d| {
        of(r, d)
            .iter()
            .map(|k| k.log.fired() as f32)
            .collect::<Vec<_>>()
    };
    checks.at_least(
        "Hard: fewest mini-turbos of a kart",
        &per_race(&runs, |r| lowest(&fired(r, Hard))),
        Some(1.0),
        None,
    );
    checks.at_least(
        "Hard: mini-turbos a kart",
        &per_race(&runs, |r| mean(&fired(r, Hard))),
        Some(4.0),
        Some(5.0),
    );
    checks.above(
        "Hard minus Easy, mini-turbos",
        &per_race(&runs, |r| {
            fired(r, Hard).iter().sum::<f32>() - fired(r, Easy).iter().sum::<f32>()
        }),
        Some(0.0),
        Some(8.0),
    );
    tow_checks(
        &mut checks,
        &runs.iter().map(|r| &r.race).collect::<Vec<_>>(),
        5.0,
        1.2,
    );
    // The Hard bots get an orange in the hairpin and in the banked turn, three in the race (even
    // after a tow on the first lap), the Normal bots some.
    let in_turns = |r: &Run| -> Vec<usize> {
        of(r, Hard)
            .iter()
            .flat_map(|k| {
                r.turns
                    .iter()
                    .map(|(from, to)| k.log.fired_between(MiniTurbo::Orange, *from, *to))
            })
            .collect()
    };
    checks.at_least(
        "Hard: share of (kart, big turn) with an orange",
        &per_race(&runs, |r| {
            let pairs = in_turns(r);
            pairs.iter().filter(|n| **n > 0).count() as f32 / pairs.len().max(1) as f32
        }),
        Some(0.5),
        Some(0.9),
    );
    let oranges = |r: &Run, d| {
        of(r, d)
            .iter()
            .map(|k| k.log.oranges() as f32)
            .collect::<Vec<_>>()
    };
    checks.at_least(
        "Hard: share of karts with 3 oranges",
        &per_race(&runs, |r| {
            let o = oranges(r, Hard);
            o.iter().filter(|n| **n >= 3.0).count() as f32 / o.len().max(1) as f32
        }),
        Some(0.25),
        Some(0.8),
    );
    // 4 Hard bots, 2 big turns, 3 laps: an orange in two thirds of the big turns.
    checks.at_least(
        "Hard: oranges",
        &per_race(&runs, |r| oranges(r, Hard).iter().sum()),
        Some(12.0),
        Some(18.0),
    );
    checks.at_least(
        "Normal: oranges",
        &per_race(&runs, |r| oranges(r, Normal).iter().sum()),
        Some(5.0),
        Some(12.0),
    );
    checks.finish();
}

/// Issue #6 acceptance: at the highest difficulty the bots chain orange mini-turbos, in a turn
/// long enough for it (one endless turn), while Easy bots release at blue.
///
/// Issue #30: with the wider drift, a Hard bot charges a purple in this endless turn (870 units,
/// 174 ticks steering inward) rather than an orange (570, 114 ticks): fewer, longer mini-turbos
/// in the same three laps. So the chain is measured in charge: at least three oranges' worth in
/// a row (3 × 570), which is what "three oranges in a row" meant before; two purples are worth
/// more.
///
/// Issue #40: one race per seed. A shunt can cut a chain, and an Easy bot rolls only once
/// whether to drift this endless turn: rates over the races, not every kart of a race.
#[test]
fn hard_bots_chain_orange_mini_turbos_in_a_long_turn() {
    let difficulty = |slot: usize| {
        if slot.is_multiple_of(2) {
            BotDifficulty::Hard
        } else {
            BotDifficulty::Easy
        }
    };
    let seeds = race_seeds();
    let runs = for_each_seed(&seeds, |seed| {
        let mut app = race_on(roundabout(), 4, seed);
        set_difficulties(&mut app, difficulty);
        player_becomes_a_bot(&mut app, difficulty(4));
        assert_eq!(app.world().resource::<TrackLayout>().name, "Rond-point");
        let mut log = RaceLog::new(&mut app);
        run_race(&mut app, &mut log, 60.0);
        RaceResult::of(&app, &log, "round circuit, Hard and Easy")
    });
    print_reports(&seeds, runs.iter());

    use BotDifficulty::{Easy, Hard};
    let mut checks = Checks::new("round circuit, Hard and Easy", &seeds);
    checks.every_race("every kart finishes", runs.iter().map(|r| r.all_finished()));
    let of = |r: &RaceResult, d: BotDifficulty| r.of_difficulty(d).cloned().collect::<Vec<_>>();
    // The Hard bots chain oranges, measured in charge (issue #15: each kart's own thresholds,
    // its Mini-turbo stat; the default ones, 270 / 570 / 870, at the middle level). A shunt can
    // cut a chain: most of them, not each one, must reach three oranges' worth.
    let chains = |r: &RaceResult| -> Vec<f32> {
        of(r, Hard)
            .iter()
            .map(|k| {
                k.log.longest_chain_charge(MiniTurbo::Orange, &k.thresholds)
                    / k.thresholds[MiniTurbo::Orange.index()]
            })
            .collect()
    };
    checks.at_least(
        "Hard: share of karts chaining 3 oranges' worth",
        &per_race(&runs, |r| {
            let c = chains(r);
            c.iter().filter(|x| **x >= 3.0).count() as f32 / c.len().max(1) as f32
        }),
        Some(0.3),
        Some(0.7),
    );
    checks.at_least(
        "Hard: longest chain of a kart, oranges' worth (average)",
        &per_race(&runs, |r| mean(&chains(r))),
        Some(2.0),
        Some(3.8),
    );
    // Easy releases at blue. It rolls once whether to drift a turn (`drift_chance`, 0.5), and
    // this circuit is one endless turn: an Easy bot drifts the whole race, or not at all.
    checks.at_most(
        "Easy: oranges and purples",
        &per_race(&runs, |r| {
            of(r, Easy).iter().map(|k| k.log.oranges()).sum::<u32>() as f32
        }),
        Some(0.0),
        None,
    );
    // (A race where no Easy bot drifts does not count: "-".)
    checks.at_least(
        "Easy: fewest blues of a kart that drifts",
        &per_race(&runs, |r| {
            let blues: Vec<f32> = of(r, Easy)
                .iter()
                .filter(|k| k.log.fired() > 0)
                .map(|k| k.log.mini_turbos[0] as f32)
                .collect();
            if blues.is_empty() {
                f32::NAN
            } else {
                lowest(&blues)
            }
        }),
        Some(2.0),
        Some(5.0),
    );
    checks.at_least(
        "Easy: share of karts that drift",
        &per_race(&runs, |r| {
            let easy = of(r, Easy);
            easy.iter().filter(|k| k.log.fired() > 0).count() as f32 / easy.len().max(1) as f32
        }),
        None,
        Some(0.15),
    );
    tow_checks(&mut checks, &runs.iter().collect::<Vec<_>>(), 3.0, 1.25);
    checks.finish();
}

/// Issue #16 acceptance: the game's grid (11 Normal bots, the player's kart driven by a bot too)
/// races 3 laps of Prairie Pompon **with the items**, as in the game. Every race ends, in a
/// bounded time; the bots take boxes and use their items (most of them); karts get hit; and the
/// items never send a bot off by itself (no tow in free driving: a kart hit by an item counts as
/// an incident, like a kart touched by another). The driving itself is measured without items
/// (the other race tests).
#[test]
fn bots_race_with_items() {
    struct Run {
        race: RaceResult,
        over: bool,
    }
    let seeds = race_seeds();
    let runs = for_each_seed(&seeds, |seed| {
        let mut app = seeded_app_with(11, seed, true);
        wait_for_the_player(&mut app);
        wait_for_the_bots(&mut app, 11);
        player_becomes_a_bot(&mut app, BotDifficulty::Normal);
        let mut log = RaceLog::new(&mut app);
        run_race(&mut app, &mut log, 150.0);
        let race = RaceResult::of(&app, &log, "12 karts, Normal, with items");
        app.update();
        let over = race_phase(&app) == RacePhase::Finished;
        Run { race, over }
    });
    print_reports(&seeds, runs.iter().map(|r| &r.race));

    let mut checks = Checks::new("12 karts, Normal grid, with items", &seeds);
    checks.every_race(
        "every kart finishes",
        runs.iter().map(|r| r.race.all_finished()),
    );
    checks.every_race(
        "the race is over once all have finished",
        runs.iter().map(|r| r.over),
    );
    let finishes = |r: &Run| r.race.finishes();
    checks.below(
        "slowest finish (s)",
        &per_race(&runs, |r| highest(&finishes(r))),
        Some(85.0),
        Some(70.0),
    );
    checks.at_least(
        "fastest finish (s)",
        &per_race(&runs, |r| lowest(&finishes(r))),
        Some(50.0),
        None,
    );
    let items = |r: &Run| r.race.karts.iter().map(|k| k.items).collect::<Vec<_>>();
    let count =
        |r: &Run, f: fn(&ItemStats) -> bool| items(r).iter().filter(|i| f(i)).count() as f32;
    let sum = |r: &Run, f: fn(&ItemStats) -> u32| items(r).iter().map(f).sum::<u32>() as f32;
    checks.at_least(
        "karts that took a box",
        &per_race(&runs, |r| count(r, |i| i.boxes > 0)),
        Some(5.0),
        Some(9.0),
    );
    checks.at_least(
        "boxes taken",
        &per_race(&runs, |r| sum(r, |i| i.boxes)),
        Some(15.0),
        Some(25.0),
    );
    checks.at_least(
        "karts that used an item",
        &per_race(&runs, |r| count(r, |i| i.used > 0)),
        Some(5.0),
        Some(9.0),
    );
    checks.at_least(
        "items used",
        &per_race(&runs, |r| sum(r, |i| i.used)),
        Some(20.0),
        Some(35.0),
    );
    checks.at_least(
        "hits (spins and tumbles)",
        &per_race(&runs, |r| sum(r, |i| i.hits_taken)),
        Some(5.0),
        Some(10.0),
    );
    checks.at_most(
        "tows (every cause)",
        &per_race(&runs, |r| r.race.tows() as f32),
        Some(5.0),
        Some(2.0),
    );
    checks.at_most(
        "tows in free driving",
        &per_race(&runs, |r| r.race.tows_of(TowCause::FreeDriving) as f32),
        Some(0.0),
        None,
    );
    checks.finish();
}

/// Issue #34 acceptance: a mixed grid (Easy, Normal, Hard, the player's kart driven by a bot too)
/// races 3 laps of Prairie Pompon **with the interactions between karts** (without items, which
/// have their own race). The bots strike their neighbours and grab the faster karts (GD § 15,
/// each with its level), the karts held break free; every race ends, in the time windows of the
/// race without interactions ([`the_three_difficulties_drive_differently`]: GD § 15, "les temps
/// et l'absence de pélican mesurés sans interactions restent la référence"), and the
/// interactions never send a bot off by itself (no tow in free driving: a kart struck or held
/// counts as an incident).
#[test]
fn bots_race_with_interactions() {
    let difficulty = |slot: usize| BotDifficulty::ALL[slot % 3];
    struct Run {
        race: RaceResult,
        over: bool,
    }
    let seeds = race_seeds();
    let runs = for_each_seed(&seeds, |seed| {
        let mut app = seeded_app_full(11, seed, false, true);
        wait_for_the_player(&mut app);
        wait_for_the_bots(&mut app, 11);
        set_difficulties(&mut app, difficulty);
        player_becomes_a_bot(&mut app, difficulty(11));
        let mut log = RaceLog::new(&mut app);
        run_race(&mut app, &mut log, 150.0);
        let race = RaceResult::of(&app, &log, "12 karts, mixed, with interactions");
        app.update();
        let over = race_phase(&app) == RacePhase::Finished;
        Run { race, over }
    });
    print_reports(&seeds, runs.iter().map(|r| &r.race));

    let mut checks = Checks::new("12 karts, mixed grid, with interactions", &seeds);
    checks.every_race(
        "every kart finishes",
        runs.iter().map(|r| r.race.all_finished()),
    );
    checks.every_race(
        "the race is over once all have finished",
        runs.iter().map(|r| r.over),
    );
    let finishes = |r: &Run| r.race.finishes();
    checks.below(
        "slowest finish (s)",
        &per_race(&runs, |r| highest(&finishes(r))),
        Some(85.0),
        Some(70.0),
    );
    // The Hard bots in front: below 50 s (47.9 s at the fastest on the seeds 0 to 7). A tow
    // never makes a lap shorter than that.
    checks.at_least(
        "fastest finish (s)",
        &per_race(&runs, |r| lowest(&finishes(r))),
        Some(45.0),
        None,
    );
    let paws = |r: &Run| r.race.karts.iter().map(|k| k.paws).collect::<Vec<_>>();
    let count = |r: &Run, f: fn(&PawStats) -> bool| paws(r).iter().filter(|p| f(p)).count() as f32;
    let sum = |r: &Run, f: fn(&PawStats) -> u32| paws(r).iter().map(f).sum::<u32>() as f32;
    checks.at_least(
        "karts that struck",
        &per_race(&runs, |r| count(r, |p| p.strikes > 0)),
        Some(3.0),
        Some(6.0),
    );
    checks.at_least(
        "strikes landed",
        &per_race(&runs, |r| sum(r, |p| p.landed)),
        Some(4.0),
        Some(10.0),
    );
    checks.at_least(
        "karts struck",
        &per_race(&runs, |r| count(r, |p| p.struck > 0)),
        Some(2.0),
        Some(5.0),
    );
    // A grab needs a faster kart passing: some races have only one (1 at the fewest on 48
    // seeds): the mean is the requirement.
    checks.at_least(
        "grabs (tethers)",
        &per_race(&runs, |r| sum(r, |p| p.grabs)),
        None,
        Some(3.0),
    );
    checks.at_least(
        "karts held that broke free (a jump)",
        &per_race(&runs, |r| sum(r, |p| p.escapes)),
        None,
        Some(1.0),
    );
    // Lap times (laps 2 and 3, without the pelican's time after a pack incident) no slower than
    // the windows of the race without interactions.
    use BotDifficulty::{Easy, Hard, Normal};
    for (d, [mean_high, high]) in [
        (Easy, [20.9, 23.0]),
        (Normal, [19.5, 21.0]),
        (Hard, [17.8, 19.5]),
    ] {
        let laps = |r: &Run| {
            mean(
                &r.race
                    .of_difficulty(d)
                    .flat_map(|k| k.laps_without_pelican.iter().skip(1).copied())
                    .collect::<Vec<_>>(),
            )
        };
        checks.at_most(
            &format!("{d:?}: average lap 2 and 3 (s)"),
            &per_race(&runs, laps),
            Some(high),
            Some(mean_high),
        );
    }
    tow_checks(
        &mut checks,
        &runs.iter().map(|r| &r.race).collect::<Vec<_>>(),
        5.0,
        2.0,
    );
    checks.at_most(
        "tows in free driving",
        &per_race(&runs, |r| r.race.tows_of(TowCause::FreeDriving) as f32),
        Some(0.0),
        None,
    );
    checks.finish();
}

fn race_phase(app: &App) -> RacePhase {
    race(app).phase()
}

/// Issue #6: the game's grid is 1 player and 11 bots; the player starts last, nobody moves
/// before GO, the HUD counts 12 racers, the bots leave the idle player behind (and pass it a
/// lap later), the catch-up slows down the bots far ahead of it, and F3 shows the overlay.
///
/// Issue #40: one race per seed. What does not depend on the seed is asserted in every race;
/// the race is logged from the grid on (tows at the start included).
#[test]
fn the_game_fills_the_grid_with_eleven_bots() {
    struct Run {
        /// Distance covered by each bot in its first 8 s (m).
        moved: Vec<f32>,
        leader_scale: f32,
        /// When all the bots had passed the idle player again (s after the first 8 s and F3).
        pass_again: f32,
        /// Tows from then on.
        late_tows: usize,
        race: RaceResult,
    }
    let seeds = race_seeds();
    let runs = for_each_seed(&seeds, |seed| {
        let mut app = race_with_bots(11, seed);
        let layout = app.world().resource::<TrackLayout>().clone();
        let me = player(&mut app);
        assert_eq!(app.world().get::<Racer>(me).unwrap().grid_slot, 11);
        let start = app.world().get::<Transform>(me).unwrap().translation;
        assert!(start.distance(layout.grid[11].translation) < 0.5, "{start}");
        assert!(app.world().get::<DrivesOnAfterFinish>(me).is_none());

        let world = app.world_mut();
        let mut slots: Vec<usize> = world
            .query_filtered::<&Racer, With<Bot>>()
            .iter(world)
            .map(|r| r.grid_slot)
            .collect();
        slots.sort();
        assert_eq!(slots, (0..11).collect::<Vec<_>>());
        let bots: Vec<(Entity, usize, Vec3)> = world
            .query_filtered::<(Entity, &Racer, &Transform), With<Bot>>()
            .iter(world)
            .map(|(e, r, t)| (e, r.grid_slot, t.translation))
            .collect();
        for (_, slot, position) in &bots {
            assert!(position.distance(layout.grid[*slot].translation) < 0.5);
        }
        // One visual per kart.
        assert_eq!(world.query::<&KartBodyPivot>().iter(world).count(), 12);

        let mut log = RaceLog::new(&mut app);
        let step = |app: &mut App, log: &mut RaceLog| {
            app.update();
            log.record(app);
        };
        // Nobody moves before GO.
        while !race(&app).started() {
            step(&mut app, &mut log);
        }
        for (bot, slot, position) in &bots {
            let now = app.world().get::<Transform>(*bot).unwrap().translation;
            let moved = (now - *position).with_y(0.0).length();
            assert!(moved < 0.1, "slot {slot} moved {moved} m before GO");
        }
        let hud = |app: &mut App, field: HudText| {
            let world = app.world_mut();
            world
                .query::<(&HudText, &Text)>()
                .iter(world)
                .find(|(f, _)| **f == field)
                .map(|(_, t)| t.0.clone())
                .unwrap()
        };
        step(&mut app, &mut log);
        assert_eq!(hud(&mut app, HudText::Rank), "12e");
        assert_eq!(hud(&mut app, HudText::RacerCount), "/ 12");

        // The player stays on the grid: the bots go, the far ones lose a little top speed.
        for _ in 0..(8.0 * TICK_HZ) as usize {
            step(&mut app, &mut log);
        }
        let tuning = app.world().resource::<BotTuning>().clone();
        let mut leader_scale = 1.0;
        let mut moved = Vec::new();
        for (bot, slot, _) in &bots {
            let p = progress(&app, *bot);
            // Along the track from its grid slot (since issue #14 the croquettes make the
            // leaders fast enough to be on the back straight by now, which runs back toward the
            // grid: a straight line from the grid is no longer a measure of how far they went).
            let course = app.world().resource::<Course>();
            let distance = p.distance - course.project(0, layout.grid[*slot].translation).distance;
            moved.push(distance);
            // The catch-up is one factor of the top speed; since issue #14 the croquettes held
            // are another one, and `MaxSpeedScale` is their product.
            let factors = *app.world().get::<MaxSpeedFactors>(*bot).unwrap();
            let scale = factors.catch_up;
            let expected = catch_up_scale(progress(&app, me).distance - p.distance, &tuning);
            assert!(
                (scale - expected).abs() < 0.003,
                "slot {slot}: {scale} vs {expected}"
            );
            let combined = app.world().get::<MaxSpeedScale>(*bot).unwrap().0;
            assert!(
                (combined - factors.product()).abs() < 1e-6,
                "slot {slot}: {combined} vs {factors:?}"
            );
            if p.rank == 1 {
                leader_scale = scale;
            }
        }
        assert_eq!(app.world().get::<MaxSpeedScale>(me).unwrap().0, 1.0);
        assert_eq!(hud(&mut app, HudText::Rank), "12e");

        // F3: the debug overlay, with the 12 karts.
        let overlay_visible = |app: &mut App| {
            let world = app.world_mut();
            *world
                .query_filtered::<&Visibility, With<DebugOverlay>>()
                .single(world)
                .unwrap()
                != Visibility::Hidden
        };
        let tap = |app: &mut App, log: &mut RaceLog| {
            for state in [ButtonState::Pressed, ButtonState::Released] {
                app.world_mut().write_message(KeyboardInput {
                    key_code: KeyCode::F3,
                    logical_key: Key::F3,
                    state,
                    text: None,
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                step(app, log);
            }
        };
        assert!(!overlay_visible(&mut app), "hidden by default");
        tap(&mut app, &mut log);
        assert!(overlay_visible(&mut app));
        step(&mut app, &mut log);
        let world = app.world_mut();
        let text = world
            .query::<&Text>()
            .iter(world)
            .find(|t| t.0.starts_with("FPS"))
            .map(|t| t.0.clone())
            .unwrap();
        assert!(text.contains("Karts 12"), "{text}");
        assert!(
            app.world()
                .resource::<SimulationCost>()
                .average_ms
                .is_some()
        );
        tap(&mut app, &mut log);
        assert!(!overlay_visible(&mut app));

        // A lap later the bots come back past the idle player on the grid: nobody gets stuck.
        let n = app.world().resource::<Course>().checkpoint_count();
        let from = log.ticks;
        let earlier_tows: usize = log.karts.iter().map(|(_, k)| k.tows.len()).sum();
        let mut pass_again = f32::INFINITY;
        for _ in 0..(35.0 * TICK_HZ) as usize {
            step(&mut app, &mut log);
            if bots.iter().all(|(b, ..)| progress(&app, *b).passed > n + 2) {
                pass_again = ticks_to_seconds(log.ticks - from);
                break;
            }
        }
        let tows: usize = log.karts.iter().map(|(_, k)| k.tows.len()).sum();
        Run {
            moved,
            leader_scale,
            pass_again,
            late_tows: tows - earlier_tows,
            race: RaceResult::of(&app, &log, "11 bots and the idle player"),
        }
    });
    print_reports(&seeds, runs.iter().map(|r| &r.race));

    let tuning = BotTuning::default();
    let mut checks = Checks::new("11 bots and the idle player", &seeds);
    // The bots go: a pile-up at the start may hold one or two back (a tow costs 2.5 s and
    // more), but they all leave the grid, most of them far.
    let moved = |r: &Run| r.moved.clone();
    checks.above(
        "least distance covered by a bot in 8 s (m)",
        &per_race(&runs, |r| lowest(&moved(r))),
        Some(10.0),
        None,
    );
    checks.at_least(
        "bots past 100 m in 8 s",
        &per_race(&runs, |r| {
            moved(r).iter().filter(|d| **d > 100.0).count() as f32
        }),
        Some(5.0),
        Some(9.0),
    );
    checks.above(
        "average distance covered by a bot in 8 s (m)",
        &per_race(&runs, |r| mean(&moved(r))),
        Some(120.0),
        Some(160.0),
    );
    // The catch-up saturates for the leader, far ahead of the idle player.
    checks.at_least(
        "catch-up of the leader",
        &per_race(&runs, |r| r.leader_scale),
        Some(1.0 - tuning.catch_up_max),
        None,
    );
    checks.below(
        "catch-up of the leader",
        &per_race(&runs, |r| r.leader_scale),
        Some(1.0),
        None,
    );
    checks.below(
        "all 11 bots past the idle player again (s)",
        &per_race(&runs, |r| r.pass_again),
        Some(30.0),
        Some(21.0),
    );
    checks.at_most(
        "tows while they pass the idle player again",
        &per_race(&runs, |r| r.late_tows as f32),
        Some(2.0),
        Some(0.6),
    );
    tow_checks(
        &mut checks,
        &runs.iter().map(|r| &r.race).collect::<Vec<_>>(),
        5.0,
        1.6,
    );
    checks.finish();
}

/// Load test measurement, run by hand (numbers go in STATUS.md):
/// `cargo test --release --lib measure_tick_cost -- --ignored --nocapture`.
/// Headless native: no rendering, so this is the simulation's share of a frame only. With
/// Bevy's multi-threaded executor, as the native game (the race tests run single-threaded).
#[test]
#[ignore]
fn measure_tick_cost_with_twelve_karts() {
    let mut app = game_app_unloaded(TrackChoice::default(), 11);
    wait_for_the_player(&mut app);
    wait_for_the_bots(&mut app, 11);
    player_becomes_a_bot(&mut app, BotDifficulty::Normal);
    // Countdown, then 60 s of race with 12 karts (the slowest finish around 60 s).
    while !race(&app).started() {
        app.update();
    }
    let mut durations = Vec::with_capacity(3600);
    for _ in 0..3600 {
        let start = std::time::Instant::now();
        app.update();
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(f64::total_cmp);
    let mean = durations.iter().sum::<f64>() / durations.len() as f64;
    let at = |q: f64| durations[((durations.len() - 1) as f64 * q) as usize];
    let sim = app.world().resource::<SimulationCost>().average_ms;
    println!(
        "12 karts, one tick per update: mean {mean:.3} ms, median {:.3} ms, p95 {:.3} ms, max {:.3} ms; simulation alone (smoothed) {sim:?} ms",
        at(0.5),
        at(0.95),
        at(1.0)
    );
}
