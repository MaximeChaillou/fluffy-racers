//! Bots (GD § 12, issue #6): karts driven by the computer, to fill the grid of 12.
//!
//! A bot is a kart with a [`Bot`] component. Every tick, in `KartSystems::Input` like the
//! keyboard for the player, its driver (`driver.rs`, pure logic) writes the kart's `KartInput`
//! from what it sees: its position on the racing line of the circuit (`line.rs`), the turns
//! ahead, its drift charge and the karts around. It never touches the kart otherwise: the kart
//! rules, the race rules and the tow pelican treat it like any racer. The only extra is the
//! light catch-up of GD § 12, a top speed multiplier (`MaxSpeedScale`) within ±3 %.
//!
//! Three difficulties ([`BotDifficulty`]): trajectory precision, drift tier aimed for and
//! anticipation of the turns, all in [`BotTuning`]. Each bot also has its own lane, pace and
//! random choices from a deterministic generator seeded by its grid slot (variety, no single
//! file, reproducible tests). Using items comes with the item system (issue #16).

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

use crate::kart::{
    Kart, KartAssists, KartInput, KartState, KartSystems, KartTuning, KartVisualAssets,
    MaxSpeedScale, PlayerKart, kart_body, spawn_kart_visual,
};
use crate::race::{DrivesOnAfterFinish, Race, RaceGrid, RaceProgress, Racer};
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
                (build_racing_line, spawn_bots).run_if(resource_added::<TrackLayout>),
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

/// Fills the front of the grid with bots once the circuit is built (`RaceGrid`).
fn spawn_bots(
    mut commands: Commands,
    layout: Res<TrackLayout>,
    grid: Res<RaceGrid>,
    tuning: Res<BotTuning>,
    kart_tuning: Res<KartTuning>,
    visuals: Option<Res<KartVisualAssets>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    for slot in grid.bot_slots() {
        let Some(spawn) = layout.grid.get(slot).copied() else {
            break;
        };
        let body = commands
            .spawn((
                Name::new(format!("Bot {}", slot + 1)),
                kart_body(&kart_tuning, spawn),
                Racer::new(slot),
                bot(tuning.grid_difficulty, slot, &tuning),
            ))
            .id();
        if let (Some(visuals), Some(materials)) = (&visuals, materials.as_mut()) {
            spawn_kart_visual(&mut commands, visuals, materials, body, bot_color(slot));
        }
    }
}

/// GD § 12: bots far behind the reference get a little more top speed, bots far ahead a little
/// less. The reference is the best placed local player; with no player (a race of bots only),
/// the field's average race distance.
fn catch_up(
    tuning: Res<BotTuning>,
    racers: Query<(&RaceProgress, Has<PlayerKart>), With<Racer>>,
    mut bots: Query<(&RaceProgress, &mut MaxSpeedScale), With<Bot>>,
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
    for (progress, mut scale) in &mut bots {
        let s = if progress.finished() {
            1.0
        } else {
            catch_up_scale(reference - progress.distance, &tuning)
        };
        scale.set_if_neq(MaxSpeedScale(s));
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
        &mut KartInput,
    )>,
) {
    let racing = race.is_none_or(|race| race.started());
    neighbors.clear();
    neighbors.extend(karts.iter().map(|(entity, transform, velocity)| Neighbor {
        entity,
        position: transform.translation,
        velocity: velocity.linear,
    }));
    for (entity, bot, mut brain, transform, velocity, state, assists, scale, mut input) in &mut bots
    {
        let senses = Senses {
            entity,
            position: transform.translation,
            forward: *transform.forward(),
            right: *transform.right(),
            velocity: velocity.linear,
            state: *state,
            assisted: assists.assisted_steering,
            speed_scale: scale.0,
            racing,
            dt: time.delta_secs(),
            neighbors: &neighbors,
        };
        let new = drive(
            &mut brain,
            tuning.skill(bot.difficulty),
            &tuning,
            &kart_tuning,
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
}
