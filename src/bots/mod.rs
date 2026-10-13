//! Bots (GD § 12, issue #6): karts driven by the computer, to fill the grid of 12.
//!
//! A bot is a kart with a [`Bot`] component. Every tick, in `KartSystems::Input` like the
//! keyboard for the player, its driver (`driver.rs`, pure logic) writes the kart's `KartInput`
//! from what it sees: its position on the racing line of the circuit (`line.rs`), the turns
//! ahead, its drift charge and the karts around. It never touches the kart otherwise: the kart
//! rules, the race rules and the tow pelican treat it like any racer. The only extra is the
//! light catch-up of GD § 12, a top speed factor (`MaxSpeedFactors::catch_up`) within ±3 %.
//!
//! Three difficulties ([`BotDifficulty`]): trajectory precision, drift tier aimed for and
//! anticipation of the turns, all in [`BotTuning`]. Each bot also has its own lane, pace and
//! random choices from a deterministic generator seeded by its grid slot (variety, no single
//! file, reproducible tests). They also try the rocket start and the tricks (issue #13), through
//! their `KartInput` like a player. Using items comes with the item system (issue #16).
//!
//! Each bot also has its own character and parts (issue #15, [`bot_loadout`]): the weight
//! classes take turns along the grid, so that the 9 animals line up and every stretch of the
//! grid mixes light, medium and heavy karts; the kart, wheels and wing come from a generator
//! seeded by the grid slot. A bot drives with its kart's own tuning (its stats applied): it
//! plans its drifts with its own radii and aims at its own tiers.

mod driver;
mod line;
mod rng;
mod tuning;

#[cfg(test)]
mod scenario_tests;

pub use driver::{BotBrain, DriftPlan, Neighbor, Senses, catch_up_scale, charge_per_radian, drive};
pub use line::{Located, RacingLine, Turn};
pub use rng::BotRng;
pub use tuning::{BotDifficulty, BotSkill, BotTuning};

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::TICK_HZ;
use crate::characters::{DriverLooks, Loadout, PartKind, Roster, WeightClass};
use crate::kart::{
    Boost, Kart, KartAssists, KartInput, KartState, KartSystems, KartTuning, KartVisualAssets,
    MaxSpeedFactors, MaxSpeedScale, PlayerKart, StatEffects, StatsTuning, TrickTracker,
    kart_body_with_stats, spawn_kart_visual_with_driver,
};
use crate::race::{DrivesOnAfterFinish, Race, RaceGrid, RaceMode, RaceProgress, Racer};
use crate::track::TrackLayout;

/// A kart driven by the computer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bot {
    pub difficulty: BotDifficulty,
}

/// Body colors of the karts by grid slot (primitive placeholders until the real models, #11).
/// The player's kart is pink.
pub const KART_COLORS: [Color; 12] = [
    Color::srgb(0.90, 0.20, 0.20), // red
    Color::srgb(1.00, 0.55, 0.15), // orange
    Color::srgb(0.95, 0.85, 0.20), // yellow
    Color::srgb(0.55, 0.85, 0.25), // lime
    Color::srgb(0.15, 0.60, 0.30), // green
    Color::srgb(0.10, 0.70, 0.70), // teal
    Color::srgb(0.35, 0.70, 1.00), // sky blue
    Color::srgb(0.20, 0.30, 0.85), // blue
    Color::srgb(0.55, 0.30, 0.85), // violet
    Color::srgb(0.55, 0.35, 0.20), // brown
    Color::srgb(0.95, 0.95, 0.95), // white
    Color::srgb(0.25, 0.25, 0.30), // charcoal
];

/// Color of the bot on grid slot `slot`.
pub fn bot_color(slot: usize) -> Color {
    KART_COLORS[slot % KART_COLORS.len()]
}

/// Seed of the bot on grid slot `slot`: each slot always drives the same way.
pub fn bot_seed(slot: usize) -> u64 {
    0xF1_0FF1 + slot as u64
}

/// Mixed into the seed of a bot's parts (their own generator: drawing them does not
/// shift the bot's driving choices).
const LOADOUT_SEED: u64 = 0x15_CA27_5EED;

/// Weight class of the bot on grid slot `slot` (issue #15): the classes take turns, shifted by
/// one every three slots (light, medium, heavy, then medium, heavy, light, then heavy, light,
/// medium...). Any three neighbouring slots, and the slots taken one in three, mix the three
/// classes: no stretch of the grid is all heavy or all light.
pub fn bot_class(slot: usize) -> WeightClass {
    WeightClass::ALL[(slot + slot / 3) % 3]
}

/// The character and parts of the bot on grid slot `slot` (issue #15). Its character: the next
/// animal of its weight class ([`bot_class`]), so the first nine slots show the nine animals;
/// its kart, wheels and wing: drawn from a generator seeded by the slot. Always the same for a
/// slot.
pub fn bot_loadout(roster: &Roster, slot: usize) -> Loadout {
    let class = bot_class(slot);
    let turn = (0..slot).filter(|s| bot_class(*s) == class).count();
    let animals: Vec<usize> = roster.characters_of(class).collect();
    let mut loadout = roster.default_loadout();
    if !animals.is_empty() {
        loadout.character = animals[turn % animals.len()];
    }
    let mut rng = BotRng::new(bot_seed(slot) ^ LOADOUT_SEED);
    for kind in [PartKind::Kart, PartKind::Wheels, PartKind::Wing] {
        let count = roster.count(kind).max(1) as u64;
        loadout.set(kind, (rng.next_u64() % count) as usize);
    }
    loadout
}

/// What turns a racing kart into a bot of `difficulty` (its seed comes from `grid_slot`). Bots
/// keep driving after the finish line, so that they do not block it.
pub fn bot(difficulty: BotDifficulty, grid_slot: usize, tuning: &BotTuning) -> impl Bundle {
    (
        Bot { difficulty },
        BotBrain::new(bot_seed(grid_slot), tuning),
        DrivesOnAfterFinish,
    )
}

pub struct BotPlugin;

impl Plugin for BotPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BotTuning>()
            .add_systems(
                Update,
                // No bot in free driving (admin panel, issue #36).
                (build_racing_line, spawn_bots)
                    .run_if(resource_added::<TrackLayout>)
                    .run_if(not(resource_equals(RaceMode::FreeRoam))),
            )
            .add_systems(
                FixedUpdate,
                (
                    catch_up.run_if(resource_exists::<Race>),
                    drive_bots.run_if(resource_exists::<RacingLine>),
                )
                    .in_set(KartSystems::Input),
            );
    }
}

fn build_racing_line(mut commands: Commands, layout: Res<TrackLayout>, tuning: Res<BotTuning>) {
    commands.insert_resource(RacingLine::new(
        &layout,
        tuning.road_margin,
        tuning.sand_margin,
        tuning.line_margin,
    ));
}

/// Fills the front of the grid with bots once the circuit is built (`RaceGrid`), each with its
/// own character and parts ([`bot_loadout`]).
fn spawn_bots(
    mut commands: Commands,
    layout: Res<TrackLayout>,
    grid: Res<RaceGrid>,
    tuning: Res<BotTuning>,
    kart_tuning: Res<KartTuning>,
    stats_tuning: Res<StatsTuning>,
    roster: Res<Roster>,
    looks: Option<Res<DriverLooks>>,
    visuals: Option<Res<KartVisualAssets>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    for slot in grid.bot_slots() {
        let Some(spawn) = layout.grid.get(slot).copied() else {
            break;
        };
        let loadout = bot_loadout(&roster, slot);
        let body = commands
            .spawn((
                Name::new(format!("Bot {}", slot + 1)),
                kart_body_with_stats(&kart_tuning, &stats_tuning, roster.stats(&loadout), spawn),
                Racer::new(slot),
                bot(tuning.grid_difficulty, slot, &tuning),
            ))
            .id();
        if let (Some(visuals), Some(materials)) = (&visuals, materials.as_mut()) {
            let driver = looks.as_ref().and_then(|l| l.get(loadout.character));
            spawn_kart_visual_with_driver(
                &mut commands,
                visuals,
                materials,
                body,
                bot_color(slot),
                driver,
            );
        }
    }
}

/// GD § 12: bots far behind the reference get a little more top speed, bots far ahead a little
/// less. The reference is the best placed local player; with no player (a race of bots only),
/// the field's average race distance. It is the catch-up factor of the kart's top speed; the
/// kart combines it with the others (croquettes...) into its `MaxSpeedScale`.
fn catch_up(
    tuning: Res<BotTuning>,
    racers: Query<(&RaceProgress, Has<PlayerKart>), With<Racer>>,
    mut bots: Query<(&RaceProgress, &mut MaxSpeedFactors), With<Bot>>,
) {
    let (mut player, mut sum, mut count) = (None::<f32>, 0.0, 0);
    for (progress, is_player) in &racers {
        sum += progress.distance;
        count += 1;
        if is_player {
            player = Some(player.map_or(progress.distance, |d| d.max(progress.distance)));
        }
    }
    if count == 0 {
        return;
    }
    let reference = player.unwrap_or(sum / count as f32);
    for (progress, mut factors) in &mut bots {
        let s = if progress.finished() {
            1.0
        } else {
            catch_up_scale(reference - progress.distance, &tuning)
        };
        if factors.catch_up != s {
            factors.catch_up = s;
        }
    }
}

/// Every bot writes its kart's input for this tick. The other karts are gathered once in a
/// buffer kept between ticks (no allocation per tick).
fn drive_bots(
    time: Res<Time>,
    line: Res<RacingLine>,
    tuning: Res<BotTuning>,
    kart_tuning: Res<KartTuning>,
    race: Option<Res<Race>>,
    mut neighbors: Local<Vec<Neighbor>>,
    karts: Query<(Entity, &Transform, &Velocity), With<Kart>>,
    mut bots: Query<(
        Entity,
        &Bot,
        &mut BotBrain,
        &Transform,
        &Velocity,
        &KartState,
        &KartAssists,
        &MaxSpeedScale,
        &StatEffects,
        &TrickTracker,
        &Boost,
        &mut KartInput,
    )>,
) {
    let racing = race.as_ref().is_none_or(|race| race.started());
    // The countdown, for the rocket start: seconds since the "2", until GO.
    let since_two = race
        .as_ref()
        .filter(|race| !race.started())
        .and_then(|race| race.flow.ticks_since_two(&race.timings))
        .map(|ticks| ticks as f32 / TICK_HZ as f32);
    let trick_window = (kart_tuning.trick_window * TICK_HZ as f32).round() as u32;
    neighbors.clear();
    neighbors.extend(karts.iter().map(|(entity, transform, velocity)| Neighbor {
        entity,
        position: transform.translation,
        velocity: velocity.linear,
    }));
    for (
        entity,
        bot,
        mut brain,
        transform,
        velocity,
        state,
        assists,
        scale,
        effects,
        tricks,
        boost,
        mut input,
    ) in &mut bots
    {
        let senses = Senses {
            entity,
            position: transform.translation,
            forward: *transform.forward(),
            right: *transform.right(),
            velocity: velocity.linear,
            yaw_rate: -velocity.angular.dot(*transform.up()),
            boost_left: boost.remaining,
            state: *state,
            assisted: assists.assisted_steering,
            speed_scale: scale.0,
            racing,
            since_two,
            trick_window: tricks.window_open(trick_window),
            dt: time.delta_secs(),
            neighbors: &neighbors,
        };
        // The bot knows its own kart: its stats applied (drift radii, tiers, steering).
        let new = drive(
            &mut brain,
            tuning.skill(bot.difficulty),
            &tuning,
            &effects.tuning(&kart_tuning),
            &line,
            &senses,
        );
        input.set_if_neq(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kart_has_its_own_color_and_seed() {
        for (i, color) in KART_COLORS.iter().enumerate() {
            assert_ne!(*color, crate::PLAYER_COLOR, "slot {i}");
            for other in &KART_COLORS[i + 1..] {
                assert_ne!(color, other);
            }
        }
        assert_eq!(bot_color(3), KART_COLORS[3]);
        let mut seeds: Vec<u64> = (0..12).map(bot_seed).collect();
        seeds.dedup();
        assert_eq!(seeds.len(), 12);
    }

    /// Issue #15: the bots' characters and parts vary along the grid, always the same for a
    /// slot: the nine animals on the first nine slots, the three weight classes in any three
    /// neighbouring slots and in the slots taken one in three, several karts, wheels and wings.
    #[test]
    fn bots_get_varied_and_deterministic_loadouts() {
        let roster = Roster::embedded();
        let loadouts: Vec<Loadout> = (0..11).map(|slot| bot_loadout(&roster, slot)).collect();
        let again: Vec<Loadout> = (0..11).map(|slot| bot_loadout(&roster, slot)).collect();
        assert_eq!(loadouts, again);
        let mut animals: Vec<usize> = loadouts[..9].iter().map(|l| l.character).collect();
        animals.sort();
        assert_eq!(animals, (0..9).collect::<Vec<_>>());
        let class = |slot: usize| roster.character(&loadouts[slot]).class;
        for start in 0..9 {
            let mut three: Vec<WeightClass> = (start..start + 3).map(class).collect();
            three.dedup();
            assert_eq!(three.len(), 3, "slots {start}..{}", start + 3);
            assert_eq!(bot_class(start), class(start));
        }
        for first in 0..3 {
            let classes: Vec<WeightClass> = (first..9).step_by(3).map(class).collect();
            for c in WeightClass::ALL {
                assert!(classes.contains(&c), "slots {first}, {}, ...", first + 3);
            }
        }
        for kind in [PartKind::Kart, PartKind::Wheels, PartKind::Wing] {
            let mut used: Vec<usize> = loadouts.iter().map(|l| l.get(kind)).collect();
            used.sort();
            used.dedup();
            assert_eq!(used.len(), roster.count(kind), "{kind:?}: {used:?}");
        }
        // Different stats along the grid.
        let mut speeds: Vec<u8> = loadouts
            .iter()
            .map(|l| roster.stats(l).level(crate::kart::Stat::Speed))
            .collect();
        speeds.sort();
        speeds.dedup();
        assert!(speeds.len() >= 4, "{speeds:?}");
    }
}
