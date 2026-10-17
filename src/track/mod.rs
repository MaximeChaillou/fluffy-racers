//! Tracks and terrain: circuits generated from data files (`assets/tracks/*.track.ron`,
//! issue #3), dressed after their biome (issue #11: colors, decor of 3D models), with their start
//! line and its arch (issue #79), its anti-gravity zones (issue #78), and the flat test ground of
//! issue #1.

pub mod anti_gravity;
pub mod biome;
pub mod circuit;
pub mod data;
pub mod decor;
pub mod dressing;
pub mod layout;
pub mod mesh;
pub mod start_line;
pub mod test_ground;
pub mod water;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::level::LevelScoped;

pub use anti_gravity::{AntiGravity, AntiGravityZone, Bend, anti_gravity_at, bend_at};
pub use circuit::{
    BOOST_PAD_SIZE, BoostPad, CROQUETTE_HEIGHT, CroquetteSpot, GlideRamp, ITEM_BOX_HEIGHT,
    ItemBoxSpot, SpotIndex, TrackHandle, TrickRamp,
};
pub use data::{OffroadZone, PropData, TrackData};
pub use dressing::CircuitDecor;
pub use layout::{CenterLine, Checkpoint, GRID_HEIGHT, GRID_SLOTS, TrackLayout, TrackSample};
pub use start_line::StartLine;
pub use water::{Gap, Lake, gap_at, water_depth};

/// Which track to build. Defaults to the test circuit.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub enum TrackChoice {
    /// A circuit file, as an asset path (relative to `assets/`).
    Circuit(String),
    /// The flat physics test ground of issue #1.
    TestGround,
}

impl Default for TrackChoice {
    fn default() -> Self {
        Self::Circuit(DEFAULT_CIRCUIT.to_string())
    }
}

/// The test circuit of issue #3.
pub const DEFAULT_CIRCUIT: &str = "tracks/prairie.track.ron";

/// The mirror mode (issue #19, GD § 1): the circuits are built as the mirror image of their file
/// (`TrackData::mirrored`: the track, its spots, its decor and its grid; the bots' racing line
/// follows from the track). Read when a circuit is built: `crate::level::LoadLevel` applies a
/// change. Online (issue #99), the host's choice: `crate::net` sets it for the room's circuit and
/// races, and gives the solo one back out of the room.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MirrorMode(pub bool);

/// Issue #99 (« Objets : non », GD § 1): the circuits are built **without their item boxes**
/// (nothing to draw an item from; the croquettes stay). Read when a circuit is built:
/// `crate::level::LoadLevel` applies a change. Set by the choice of « Jouer solo »
/// (`crate::modes`), or online by the host's (`crate::net`, the solo one given back out of the
/// room). **Only there while the items are off**: the default world, the bot races' one, never
/// has it (a resource is an entity in Bevy 0.20: one more archetype before the grid would change
/// every bot race, `STATUS.md`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoItemBoxes;

/// Where the local player's kart starts, available once the track is built. On a circuit it is
/// the pole; the player's actual grid slot comes from `race::RaceGrid` (behind the bots).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct PlayerStart(pub Transform);

/// What a static collider of the track is to a kart's wheels. Put it on every one of them: the
/// suspension rays only stand on a collider whose surface [`is_ground`](Self::is_ground), never
/// on a wall, an obstacle or another kart (issue #27). A collider without it is not ground.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Road,
    /// Grass, sand...: the max speed drops (GD § 2, `KartTuning::offroad_speed_factor`).
    Offroad,
    /// A wall or an obstacle: the karts bump into it (GD § 2) but never drive on it. The
    /// suspension rays go through it: a kart tipped against a low wall no longer finds ground on
    /// its flat top, which helped lift that side of the kart over the wall (issue #27).
    Wall,
}

impl Surface {
    /// Whether the wheels can stand on it: the road and the off-road, not a wall.
    pub fn is_ground(self) -> bool {
        self != Self::Wall
    }
}

/// Bounciness of walls and obstacles: the kart bounces back and never stops dead (GD § 2).
/// 0.3 = a head-on hit at 26 m/s sends the kart back at about 8 m/s.
pub const WALL_RESTITUTION: f32 = 0.3;

/// Spawns a static box collider (physics only, no mesh): ground slab, wall, obstacle, ramp.
/// `surface`: `Surface::Wall` for a wall or an obstacle (the wheels never stand on it).
/// `restitution`: 0 for the ground, `WALL_RESTITUTION` for things the kart bumps into. It belongs
/// to the level (`LevelScoped`): reloading the level removes it.
pub fn spawn_static_box(
    commands: &mut Commands,
    transform: Transform,
    half_extents: Vec3,
    surface: Surface,
    restitution: f32,
) -> Entity {
    commands
        .spawn((
            LevelScoped,
            transform,
            RigidBody::Fixed,
            Collider::cuboid(half_extents.x, half_extents.y, half_extents.z),
            Restitution::coefficient(restitution),
            // No friction: scraping a wall or the ground slides instead of stopping the kart.
            Friction::coefficient(0.0),
            surface,
        ))
        .id()
}

/// Transform of a ramp box whose top face starts at ground level at `start` and climbs
/// toward -Z over `length` meters with a slope of `angle` radians.
pub fn ramp_transform(start: Vec3, length: f32, angle: f32, half_thickness: f32) -> Transform {
    ramp_transform_on(start, Quat::IDENTITY, length, angle, half_thickness)
}

/// Same as [`ramp_transform`] on a surface oriented by `base` (its local -Z climbs).
pub fn ramp_transform_on(
    start: Vec3,
    base: Quat,
    length: f32,
    angle: f32,
    half_thickness: f32,
) -> Transform {
    let height = length * angle.sin();
    let run = length * angle.cos();
    let rotation = base * Quat::from_rotation_x(angle);
    // Middle of the top face, then down to the box center along the ramp's normal.
    let top_center = start + base * Vec3::new(0.0, height / 2.0, -run / 2.0);
    let center = top_center - rotation * Vec3::Y * half_thickness;
    Transform::from_translation(center).with_rotation(rotation)
}

pub struct TrackPlugin;

impl Plugin for TrackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrackChoice>()
            .init_resource::<MirrorMode>()
            .init_asset::<TrackData>()
            .init_asset_loader::<circuit::TrackLoader>()
            .add_systems(
                Startup,
                (
                    test_ground::spawn_test_ground.run_if(resource_equals(TrackChoice::TestGround)),
                    load_circuit,
                ),
            )
            .add_systems(
                Update,
                // In this order: the fallback removes the `TrackHandle` the build reads (left
                // unordered, a sync point between them made the build fail, issue #6).
                (
                    circuit::build_circuit_when_loaded,
                    circuit::fall_back_when_loading_fails,
                )
                    .chain()
                    .run_if(resource_exists::<TrackHandle>)
                    .run_if(not(resource_exists::<TrackLayout>)),
            )
            // Issue #11: the decor, once the circuit is built and its models baked (with a
            // renderer only).
            .add_systems(
                Update,
                dressing::dress_circuit
                    .after(crate::models::ModelSystems)
                    .run_if(resource_exists::<TrackLayout>)
                    .run_if(dressing::can_load_models),
            );
    }
}

/// Loads the chosen circuit file, unless a circuit was handed over directly (a `TrackHandle`
/// already there, e.g. a circuit built by a test).
fn load_circuit(
    mut commands: Commands,
    choice: Res<TrackChoice>,
    assets: Res<AssetServer>,
    given: Option<Res<TrackHandle>>,
) {
    if given.is_some() {
        return;
    }
    if let TrackChoice::Circuit(path) = &*choice {
        commands.insert_resource(TrackHandle(assets.load(path.clone())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_top_face_starts_on_the_ground_and_climbs_forward() {
        let angle = 12f32.to_radians();
        let start = Vec3::new(0.0, 0.0, -70.0);
        let t = ramp_transform(start, 8.0, angle, 0.5);
        let top_start = t.transform_point(Vec3::new(0.0, 0.5, 4.0));
        let top_end = t.transform_point(Vec3::new(0.0, 0.5, -4.0));
        assert!(top_start.distance(start) < 1e-4, "{top_start}");
        assert!((top_end.y - 8.0 * angle.sin()).abs() < 1e-4);
        assert!(top_end.z < start.z);
    }

    #[test]
    fn ramp_on_a_turned_surface_climbs_along_it() {
        let angle = 12f32.to_radians();
        let start = Vec3::new(5.0, 1.0, 3.0);
        // Surface facing +X (local -Z = world +X).
        let base = Transform::default().looking_to(Vec3::X, Vec3::Y).rotation;
        let t = ramp_transform_on(start, base, 8.0, angle, 0.5);
        let top_start = t.transform_point(Vec3::new(0.0, 0.5, 4.0));
        let top_end = t.transform_point(Vec3::new(0.0, 0.5, -4.0));
        assert!(top_start.distance(start) < 1e-4, "{top_start}");
        assert!((top_end.y - start.y - 8.0 * angle.sin()).abs() < 1e-4);
        assert!(top_end.x > start.x + 7.0);
    }
}

#[cfg(test)]
mod scenario_tests;
