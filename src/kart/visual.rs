//! Kart visuals: Kenney's kart once its model is baked (issue #11, `model.rs`), primitive shapes
//! until then, and for good headless or when the file is missing.
//!
//! The visual is a separate entity that follows the simulated body with interpolation
//! (`crate::interpolation`). Under it, a pivot ([`KartBodyPivot`]) carries the body, driver and
//! wheels: while drifting it turns the body by the drift's slip angle around the front axle, so
//! the rear slides out of the turn while the front follows the trajectory (issue #30; the slip
//! angle is only shown, the simulated body keeps its grip and the camera keeps following the
//! direction of travel). The wheels follow the suspension, so the kart visibly hugs the ground
//! and bounces on landings. Drift sparks at the rear wheels show the mini-turbo tier (GD § 14).
//!
//! Driving techniques (issue #13), all primitives sharing a handful of unlit materials:
//! exhaust flames during a boost (bigger for a stronger and longer boost: a better rocket
//! start), grey puffs while the engine stalls, wind lines around the kart while its slipstream
//! gauge charges, and a barrel roll of the body during a trick.
//!
//! Hit by an item (issue #16), the body spins out (two turns) or is thrown in a forward flip,
//! then blinks while the kart is invincible.
//!
//! The driver (issue #15) is the character: its size shows its weight class. It is spawned in
//! primitives, a fluffy ball whose color and ears show the animal ([`DriverLook`], one shared
//! material per character); once its animal's model is ready (issue #69, Kenney's Cube Pets),
//! `characters::visual` swaps them for the animated animal (and keeps them headless, or when the
//! file is missing).
//!
//! The body wears a color of the palette (issue #55, `palette.rs`): one shared material per
//! color, for every kart that wears it. It follows the kart's [`KartPaint`], even when it changes.
//!
//! The model (issue #11): every visual is spawned in primitives; once the kart model is baked,
//! [`dress_kart_visuals`] swaps them for its meshes (the body's shell keeps its [`BodyPaint`]
//! and its paint material, the rest of the body and the wheels the models' shared material), and
//! moves the wheels, the driver and the sparks where the model has them. The same meshes for
//! every kart: 12 karts draw the same few meshes.
//!
//! The parts (issue #73, `parts.rs`): each visual shows the parts of its kart ([`PartLooks`], on
//! its pivot): its body (the wicker basket, the soap box: the Car Kit's kart, or the bathtub),
//! its wheels (thread spools, marshmallow tyres, tractor tyres: each wheel put on the road with
//! its own radius) and, while it glides, its wing (the pillowcase or the feather). One set of
//! meshes per part, made once, shared by every kart that has it; the shell still in its color's
//! material, everything else in the models' shared material.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use serde::Deserialize;

use super::drift::{DriftMotion, MiniTurbo, slip_pose};
use super::effects::ItemEffects;
use super::environment::{KartEnvironment, glide_pose};
use super::model::{KART_MODEL, kart_model_data};
use super::palette::{KartColor, KartPaint, PALETTE_SIZE};
use super::parts::{
    BodyLook, PART_MODELS, PartLooks, TRACTOR_MODEL, WheelLook, kart_parts, wheel_radius,
    wheel_scale,
};
use super::physics::HitReaction;
use super::rocket::Stall;
use super::slipstream::Slipstream;
use super::stats::Invincibility;
use super::{Boost, KartAssists, KartState, KartTuning, KartWheels, StatEffects, WHEEL_ANCHORS};
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::models::{Keep, ModelMaterials, ModelSystems, Models};
use crate::track::circuit::to_mesh;
use bevy_rapier3d::prelude::Velocity;

const WHEEL_RADIUS: f32 = 0.28;
const WHEEL_WIDTH: f32 = 0.25;
/// Wheels stick out of the body on the sides.
const WHEEL_X: f32 = 0.7;

/// Placeholder spark colors (GD § 3.4: blue, orange, purple), until real effects.
const SPARK_COLORS: [Color; 3] = [
    Color::srgb(0.25, 0.6, 1.0),
    Color::srgb(1.0, 0.55, 0.1),
    Color::srgb(0.75, 0.3, 1.0),
];
/// Spark size per tier: higher tiers are bigger (m).
const SPARK_SIZES: [f32; 3] = [0.14, 0.19, 0.24];
/// Sparks sit just behind the rear wheels, near the ground (kart local frame).
const SPARK_OFFSET: Vec3 = Vec3::new(0.0, -0.42, 1.05);
/// Spark flicker frequency (rad/s).
const SPARK_FLICKER: f32 = 45.0;

/// Exhaust pipes: two, at the back of the body (kart local frame, the back is +Z).
const EXHAUST_X: f32 = 0.3;
const EXHAUST_Y: f32 = -0.02;
const EXHAUST_Z: f32 = 1.0;
/// Flame length and width at size 1 (m).
const FLAME_LENGTH: f32 = 1.1;
const FLAME_WIDTH: f32 = 0.24;
const FLAME_FLICKER: f32 = 38.0;
/// Stall puffs: size (m) and how fast they pop (rad/s).
const SMOKE_SIZE: f32 = 0.3;
const SMOKE_FLICKER: f32 = 14.0;
/// Wind lines: where they run around the kart (x, y), their length range (m), how far they
/// travel back (m) and how fast (m/s).
const WIND_LINES: [Vec2; 4] = [
    Vec2::new(-1.0, 0.25),
    Vec2::new(1.0, 0.25),
    Vec2::new(-0.55, 1.15),
    Vec2::new(0.6, 1.2),
];
const WIND_LENGTH: [f32; 2] = [0.7, 1.8];
const WIND_TRAVEL: f32 = 4.5;
const WIND_SPEED: f32 = 14.0;
const WIND_THICKNESS: f32 = 0.035;
/// A trick is a full barrel roll of the body taking this long (s).
const TRICK_SPIN_SECONDS: f32 = 0.45;
/// A kart tumbling after an item hit is thrown this high (m, visual only: the body stays on the
/// ground).
const TUMBLE_HEIGHT: f32 = 1.1;
/// A kart invincible after an item hit blinks this many times per second (GD § 8).
const INVINCIBLE_BLINK_HZ: f32 = 10.0;
/// The driver's ball: radius at size 1 (m), and where it sits on the body (its bottom on the
/// seat, whatever its size).
const DRIVER_RADIUS: f32 = 0.45;
const DRIVER_SEAT: Vec3 = Vec3::new(0.0, 0.15, 0.15);
/// The driver's eyes: their size, and how far apart and how high on the ball (at size 1; the
/// front of the ball is -Z).
const EYE_RADIUS: f32 = 0.075;
const EYE_SPREAD: f32 = 0.16;
const EYE_HEIGHT: f32 = 0.58;

/// The ears of a driver in primitives (issue #15): until its model is ready (issue #69),
/// headless, or without its file.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DriverEars {
    /// Two balls (cow, elephant).
    #[default]
    Round,
    /// Two long ones standing up (rabbit).
    Long,
    /// Two cones (cat, fox, pig).
    Pointy,
    /// None (chick, penguin, parrot).
    None,
}

/// How a kart's driver looks (issue #15): its character (its index in the roster, issue #69: its
/// animal's model), its size (1 = medium; the weight class shows), and in primitives its shared
/// material (one per character: its color) and its ears.
#[derive(Clone, Debug, PartialEq)]
pub struct DriverLook {
    pub character: usize,
    pub size: f32,
    pub material: Handle<StandardMaterial>,
    pub ears: DriverEars,
}

/// The body of a kart visual, painted in the color of `kart`'s [`KartPaint`] (issue #55): `shown`
/// is the color of its material now.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyPaint {
    /// The kart (its simulated body, or the body of another peer's kart).
    pub kart: Entity,
    pub shown: KartColor,
}

/// The driver under a kart visual's pivot: its size (scale) shows the character's weight class;
/// `character`: its index in the roster (None: the default driver, in primitives for good).
/// Its children are its primitives ([`DriverPrimitive`]) until its animal is there (issue #69).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DriverVisual {
    pub size: f32,
    pub character: Option<usize>,
}

/// A primitive of a driver (its ball, eyes and ears): removed once its animal is there
/// (issue #69, `characters::visual`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverPrimitive;

/// The pivot under a kart visual that carries every visible part. `slip` is the current visual
/// slip angle (radians, positive = nose to the right); `spin` the time into the current trick's
/// barrel roll (s, `TRICK_SPIN_SECONDS` or more when no roll is playing).
#[derive(Component, Clone, Copy, Debug)]
pub struct KartBodyPivot {
    /// The simulated kart body.
    pub body: Entity,
    pub slip: f32,
    pub spin: f32,
    /// The body was doing a trick on the last frame (a new trick starts a new roll).
    tricking: bool,
    /// The body was spinning out or tumbling on the last frame (issue #16).
    hit: bool,
    /// The body was gliding on the last frame (issue #20): pitched and banked.
    gliding: bool,
    /// Issue #20: its glider's wing was fitted (the first time the kart glided).
    pub(super) wing: bool,
    /// Issue #73: what its kart's parts look like (its body, wheels and wing).
    pub looks: PartLooks,
}

/// An exhaust flame, shown during a boost (GD § 4 and 14).
#[derive(Component, Clone, Copy, Debug)]
pub struct BoostFlame {
    /// Flicker phase.
    pub phase: f32,
}

/// A grey puff at the exhaust, shown while the engine stalls (GD § 4).
#[derive(Component, Clone, Copy, Debug)]
pub struct StallSmoke {
    pub phase: f32,
}

/// A wind line around the kart, shown while its slipstream gauge charges (GD § 6).
#[derive(Component, Clone, Copy, Debug)]
pub struct WindLine {
    /// Index into `WIND_LINES`.
    pub index: usize,
}

/// Size of the boost flames (0 = none): they grow with the boost's strength and with the time
/// it has left, so a better rocket start (a longer boost) shows bigger flames at GO, and every
/// boost's flames shrink as it runs out (GD § 4 and 14).
pub fn flame_size(tuning: &KartTuning, boost: &Boost) -> f32 {
    if !boost.is_active() {
        return 0.0;
    }
    let left = (boost.remaining / tuning.rocket_start_max_duration.max(0.1)).min(1.0);
    let strength = (boost.strength / tuning.rocket_start_strength.max(0.01)).clamp(0.5, 1.3);
    strength * (0.3 + 0.7 * left)
}

/// A wheel of a kart visual: `index` into `WHEEL_ANCHORS` / `KartWheels`; its `radius` puts
/// it on the ground (the primitive's, or the model's, issue #11).
#[derive(Component, Clone, Copy, Debug)]
pub struct WheelVisual {
    pub index: usize,
    pub radius: f32,
    /// Its rotation on the kart, upright (the primitive's, or the model's hub).
    pub rest: Quat,
    /// Issue #78: how far it has pivoted flat (0: upright, 1: flat, in anti-gravity), shown only.
    pub pivot: f32,
}

/// Issue #78 (GD § 10: « les roues pivotent »): the rotation of a wheel `pivot` of the way (0 to 1)
/// from upright (`rest`) to flat under the kart, its outer face down: around the kart's length,
/// the wheels of each side the other way (`side`: the sign of its anchor's x).
pub fn pivoted_wheel(rest: Quat, side: f32, pivot: f32) -> Quat {
    let angle = -side.signum() * pivot.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    Quat::from_rotation_z(angle) * rest
}

/// On the pivot of a kart visual dressed with the kart model (issue #11).
#[derive(Component, Clone, Copy, Debug)]
pub struct KartDressed;

/// A drift spark, shown while the drift has reached exactly this tier.
#[derive(Component, Clone, Copy, Debug)]
pub struct DriftSpark {
    pub tier: MiniTurbo,
    /// Flicker phase, so the sparks do not pulse in sync.
    pub phase: f32,
}

pub struct KartVisualPlugin;

impl Plugin for KartVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (create_kart_visual_assets, request_kart_model))
            .add_systems(
                Update,
                prepare_kart_model.after(ModelSystems).run_if(
                    |assets: Option<Res<KartVisualAssets>>| {
                        assets.is_some_and(|a| a.model.is_none())
                    },
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    dress_kart_visuals.before(place_wheels),
                    place_wheels,
                    pose_bodies,
                    show_drift_sparks,
                    show_boost_flames,
                    show_stall_smoke,
                    show_wind_lines,
                    repaint_bodies,
                    super::environment_visual::show_wings,
                    super::environment_visual::show_water_effects,
                )
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// How small a deflated kart looks (issue #18, `ItemTuning::degonflage_scale`): the item rules
/// keep it up to date, the kart visuals read it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DeflatedScale(pub f32);

/// Meshes and materials shared by every kart visual: 12 karts on the grid reuse the same assets
/// (fewer GPU resources, better batching), the body's material too: one per color of the
/// palette (issue #55).
#[derive(Resource, Clone, Debug)]
pub struct KartVisualAssets {
    body: Handle<Mesh>,
    driver: Handle<Mesh>,
    /// The driver's eyes (issue #11: still primitives, with a face).
    eye: Handle<Mesh>,
    ear: Handle<Mesh>,
    long_ear: Handle<Mesh>,
    pointy_ear: Handle<Mesh>,
    wheel: Handle<Mesh>,
    spark: Handle<Mesh>,
    driver_material: Handle<StandardMaterial>,
    eye_material: Handle<StandardMaterial>,
    wheel_material: Handle<StandardMaterial>,
    spark_materials: [Handle<StandardMaterial>; 3],
    flame_material: Handle<StandardMaterial>,
    smoke_material: Handle<StandardMaterial>,
    wind_material: Handle<StandardMaterial>,
    /// The body's materials, by color ([`KartColor::index`]).
    paints: [Handle<StandardMaterial>; PALETTE_SIZE],
    /// Issue #20: the glider (a pillowcase on two struts) and the bubbles under water.
    pub(super) environment: super::environment_visual::EnvironmentLook,
    /// The kart model's meshes, once baked (issue #11).
    model: Option<KartModel>,
}

/// The meshes of the kart model (issue #11, `model.rs`) and of every part (issue #73,
/// `parts.rs`): one set per part, shared by every kart that has it; where each body's wheels and
/// driver go.
#[derive(Clone, Debug)]
pub struct KartModel {
    /// By [`BodyLook::index`].
    bodies: [BodyMeshes; 3],
    /// By [`WheelLook::index`].
    wheels: [WheelMeshes; 3],
    /// The feather's sail (issue #73, the glider's wing).
    feather: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// A body's meshes: its shell (painted), the rest; its wheels' hubs and turns, its seat.
#[derive(Clone, Debug)]
struct BodyMeshes {
    paint: Handle<Mesh>,
    trim: Handle<Mesh>,
    hubs: [Vec3; 4],
    turns: [Quat; 4],
    seat: Vec3,
}

/// A set of wheels' mesh (one for the four) and the radius of the front and back ones.
#[derive(Clone, Debug)]
struct WheelMeshes {
    mesh: Handle<Mesh>,
    radii: [f32; 2],
}

impl KartModel {
    /// The shell (painted) and the rest of the body `look`.
    pub fn body_meshes(&self, look: BodyLook) -> (&Handle<Mesh>, &Handle<Mesh>) {
        let body = &self.bodies[look.index()];
        (&body.paint, &body.trim)
    }

    /// Where the driver of the body `look` sits.
    pub fn seat(&self, look: BodyLook) -> Vec3 {
        self.bodies[look.index()].seat
    }

    /// The mesh, the radius and the scale of the wheel `index` (of `WHEEL_ANCHORS`) of the set
    /// `look`.
    pub fn wheel(&self, look: WheelLook, index: usize) -> (&Handle<Mesh>, f32, f32) {
        let wheels = &self.wheels[look.index()];
        (
            &wheels.mesh,
            wheel_radius(wheels.radii, index),
            wheel_scale(wheels.radii, index),
        )
    }

    /// The feather's sail (issue #73).
    pub fn feather(&self) -> &Handle<Mesh> {
        &self.feather
    }

    /// The material of everything but the shells: the models' shared one.
    pub fn material(&self) -> &Handle<StandardMaterial> {
        &self.material
    }
}

impl KartVisualAssets {
    /// The body's material of `color`, shared by every kart that wears it.
    pub fn paint(&self, color: KartColor) -> Handle<StandardMaterial> {
        self.paints[usize::from(color.index())].clone()
    }

    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        Self {
            body: meshes.add(Cuboid::new(1.2, 0.45, 2.0)),
            driver: meshes.add(Sphere::new(DRIVER_RADIUS)),
            ear: meshes.add(Sphere::new(0.16)),
            eye: meshes.add(Sphere::new(EYE_RADIUS)),
            long_ear: meshes.add(Capsule3d::new(0.08, 0.32)),
            pointy_ear: meshes.add(Cone::new(0.14, 0.3)),
            wheel: meshes.add(Cylinder::new(WHEEL_RADIUS, WHEEL_WIDTH)),
            // A unit cube scaled per frame; unlit so the sparks glow in the shade too.
            spark: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            driver_material: materials.add(Color::srgb(0.98, 0.92, 0.80)),
            eye_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.08, 0.06, 0.10),
                perceptual_roughness: 0.25,
                ..default()
            }),
            wheel_material: materials.add(Color::srgb(0.15, 0.15, 0.18)),
            spark_materials: SPARK_COLORS.map(|color| {
                materials.add(StandardMaterial {
                    base_color: color,
                    unlit: true,
                    ..default()
                })
            }),
            flame_material: materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.65, 0.15),
                unlit: true,
                ..default()
            }),
            smoke_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.45, 0.45, 0.48),
                unlit: true,
                ..default()
            }),
            wind_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.95, 0.97, 1.0),
                unlit: true,
                ..default()
            }),
            paints: KartColor::ALL.map(|color| materials.add(color.color())),
            environment: super::environment_visual::EnvironmentLook::new(meshes, materials),
            model: None,
        }
    }

    /// Whether the kart model is there (issue #11): the visuals are dressed with it.
    pub fn has_model(&self) -> bool {
        self.model.is_some()
    }

    /// The meshes of the kart model and its parts, once baked (issue #73).
    pub fn model(&self) -> Option<&KartModel> {
        self.model.as_ref()
    }
}

/// Asks for the kart model and the parts' (issue #73: the tractor tyres) at startup, kept for the
/// whole game (issue #11).
fn request_kart_model(
    models: Option<ResMut<Models>>,
    server: Res<AssetServer>,
    gltfs: Option<Res<Assets<bevy::gltf::Gltf>>>,
) {
    if let Some(mut models) = models {
        let server = gltfs.map(|_| &*server);
        for id in std::iter::once(KART_MODEL).chain(PART_MODELS) {
            models.request(id, Keep::Always, server);
        }
    }
}

/// Once the kart model is baked (and the parts' files are baked or missing), the meshes of every
/// part (once for every kart, issue #73). A file that is not the kart expected leaves the
/// primitives (said once); a tractor tyre missing leaves the marshmallow tyres in its place.
fn prepare_kart_model(
    models: Option<Res<Models>>,
    shared: Option<Res<ModelMaterials>>,
    mut assets: ResMut<KartVisualAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut gave_up: Local<bool>,
) {
    let (Some(models), Some(shared)) = (models, shared) else {
        return;
    };
    if *gave_up {
        return;
    }
    let Some(baked) = models.get(KART_MODEL) else {
        return;
    };
    if !PART_MODELS.iter().all(|id| models.settled(id)) {
        return;
    }
    let Some(kart) = kart_model_data(baked) else {
        warn!("model {KART_MODEL}: not the kart expected (body, four wheels); primitives stay");
        *gave_up = true;
        return;
    };
    let tractor = models.get(TRACTOR_MODEL);
    if tractor.is_none() {
        warn!("model {TRACTOR_MODEL}: missing; the tractor tyres look like marshmallow tyres");
    }
    let parts = kart_parts(&kart, tractor.map(|model| &**model));
    let bodies = parts.bodies.map(|body| BodyMeshes {
        paint: meshes.add(to_mesh(&body.paint)),
        trim: meshes.add(to_mesh(&body.trim)),
        turns: body.wheel_turns(),
        hubs: body.hubs,
        seat: body.seat,
    });
    let wheels = parts.wheels.map(|set| WheelMeshes {
        mesh: meshes.add(to_mesh(&set.mesh)),
        radii: set.radii,
    });
    assets.model = Some(KartModel {
        bodies,
        wheels,
        feather: meshes.add(to_mesh(&parts.feather)),
        material: shared.vertex_colors.clone(),
    });
}

/// Dresses the kart visuals still in primitives with the kart model (issue #11), in the parts of
/// their kart (issue #73): the body's shell (its paint material stays), the rest of the body (a
/// child), the wheels at the body's hubs with their radius, the driver in its seat, the sparks
/// behind its rear wheels. Once per visual.
fn dress_kart_visuals(
    mut commands: Commands,
    assets: Option<Res<KartVisualAssets>>,
    pivots: Query<(Entity, &KartBodyPivot, &Children), Without<KartDressed>>,
    mut bodies: Query<&mut Mesh3d, (With<BodyPaint>, Without<WheelVisual>)>,
    mut wheels: Query<(
        &mut WheelVisual,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Transform,
    )>,
    mut drivers: Query<&mut Transform, (With<DriverVisual>, Without<WheelVisual>)>,
    mut sparks: Query<
        &mut Transform,
        (
            With<DriftSpark>,
            Without<DriverVisual>,
            Without<WheelVisual>,
        ),
    >,
) {
    let Some(model) = assets.as_ref().and_then(|a| a.model.as_ref()) else {
        return;
    };
    for (pivot, kart, children) in &pivots {
        let body = &model.bodies[kart.looks.body.index()];
        let set = &model.wheels[kart.looks.wheels.index()];
        for child in children.iter() {
            if let Ok(mut mesh) = bodies.get_mut(child) {
                mesh.0 = body.paint.clone();
                commands.entity(child).with_child((
                    Mesh3d(body.trim.clone()),
                    MeshMaterial3d(model.material.clone()),
                    Transform::default(),
                ));
            } else if let Ok((mut wheel, mut mesh, mut material, mut transform)) =
                wheels.get_mut(child)
            {
                // One mesh for the set, scaled around the hub for smaller front wheels.
                mesh.0 = set.mesh.clone();
                material.0 = model.material.clone();
                wheel.radius = wheel_radius(set.radii, wheel.index);
                wheel.rest = body.turns[wheel.index];
                *transform = Transform::from_translation(body.hubs[wheel.index])
                    .with_rotation(if wheel.pivot > 0.0 {
                        pivoted_wheel(wheel.rest, WHEEL_ANCHORS[wheel.index].x, wheel.pivot)
                    } else {
                        wheel.rest
                    })
                    .with_scale(Vec3::splat(wheel_scale(set.radii, wheel.index)));
            } else if let Ok(mut transform) = drivers.get_mut(child) {
                transform.translation = body.seat;
            } else if let Ok(mut transform) = sparks.get_mut(child) {
                // Behind the rear wheel of its side, near the ground.
                let rear = body.hubs[if transform.translation.x < 0.0 { 2 } else { 3 }];
                transform.translation =
                    Vec3::new(rear.x, SPARK_OFFSET.y, rear.z + set.radii[1] + 0.1);
            }
        }
        commands.entity(pivot).insert(KartDressed);
    }
}

fn create_kart_visual_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(KartVisualAssets::new(&mut meshes, &mut materials));
}

/// Spawns the visual of the kart `body` (colored box, fluffy ball driver, 4 wheels, drift
/// sparks, boost flames, stall puffs, wind lines), all from the shared `assets`. The box wears
/// `color`, then the color of `body`'s [`KartPaint`] if it has one (and when it changes). The
/// driver is the default one (medium, cream, round ears), the parts the default ones.
pub fn spawn_kart_visual(
    commands: &mut Commands,
    assets: &KartVisualAssets,
    body: Entity,
    color: KartColor,
) -> Entity {
    spawn_kart_visual_with_driver(commands, assets, body, color, None, PartLooks::default())
}

/// Same with the driver of a character (issue #15; `None`: the default one) and the looks of its
/// kart's parts (issue #73: `characters::Roster::looks` of its loadout), shown once the kart
/// model is baked.
pub fn spawn_kart_visual_with_driver(
    commands: &mut Commands,
    assets: &KartVisualAssets,
    body: Entity,
    color: KartColor,
    driver: Option<&DriverLook>,
    looks: PartLooks,
) -> Entity {
    let body_material = assets.paint(color);
    let assets = assets.clone();
    let (character, size, driver_material, ears) = match driver {
        Some(look) => (
            Some(look.character),
            look.size,
            look.material.clone(),
            look.ears,
        ),
        None => (None, 1.0, assets.driver_material.clone(), DriverEars::Round),
    };

    commands
        .spawn((
            InterpolatedFrom(body),
            Transform::default(),
            Visibility::default(),
            super::environment_visual::WaterEffects::default(),
        ))
        .with_children(|kart| {
            for (index, at) in WIND_LINES.iter().enumerate() {
                kart.spawn((
                    WindLine { index },
                    Mesh3d(assets.spark.clone()),
                    MeshMaterial3d(assets.wind_material.clone()),
                    Transform::from_xyz(at.x, at.y, 0.0),
                    Visibility::Hidden,
                ));
            }
            kart.spawn((
                KartBodyPivot {
                    body,
                    slip: 0.0,
                    spin: TRICK_SPIN_SECONDS,
                    tricking: false,
                    hit: false,
                    gliding: false,
                    wing: false,
                    looks,
                },
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|pivot| {
                pivot.spawn((
                    BodyPaint {
                        kart: body,
                        shown: color,
                    },
                    Mesh3d(assets.body.clone()),
                    MeshMaterial3d(body_material),
                    Transform::default(),
                ));
                // The driver: its bottom on the seat, scaled with its size (ears included).
                pivot
                    .spawn((
                        DriverVisual { size, character },
                        Transform::from_translation(DRIVER_SEAT).with_scale(Vec3::splat(size)),
                        Visibility::default(),
                    ))
                    .with_children(|driver| {
                        driver.spawn((
                            DriverPrimitive,
                            Mesh3d(assets.driver.clone()),
                            MeshMaterial3d(driver_material.clone()),
                            Transform::from_xyz(0.0, DRIVER_RADIUS, 0.0),
                        ));
                        let ear = match ears {
                            DriverEars::Round => Some((assets.ear.clone(), 0.85, 0.0)),
                            DriverEars::Long => Some((assets.long_ear.clone(), 1.0, 0.12)),
                            DriverEars::Pointy => Some((assets.pointy_ear.clone(), 0.9, 0.25)),
                            DriverEars::None => None,
                        };
                        // Two eyes on the front of the ball, just out of its surface.
                        let depth = (DRIVER_RADIUS * DRIVER_RADIUS
                            - EYE_SPREAD * EYE_SPREAD
                            - (EYE_HEIGHT - DRIVER_RADIUS).powi(2))
                        .sqrt();
                        for x in [-EYE_SPREAD, EYE_SPREAD] {
                            driver.spawn((
                                DriverPrimitive,
                                Mesh3d(assets.eye.clone()),
                                MeshMaterial3d(assets.eye_material.clone()),
                                Transform::from_xyz(x, EYE_HEIGHT, -depth + EYE_RADIUS * 0.4),
                            ));
                        }
                        if let Some((mesh, height, tilt)) = ear {
                            for x in [-0.25f32, 0.25] {
                                driver.spawn((
                                    DriverPrimitive,
                                    Mesh3d(mesh.clone()),
                                    MeshMaterial3d(driver_material.clone()),
                                    Transform::from_xyz(x, height, 0.0)
                                        .with_rotation(Quat::from_rotation_z(-tilt * x.signum())),
                                ));
                            }
                        }
                    });
                for (i, anchor) in WHEEL_ANCHORS.iter().enumerate() {
                    let rest = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
                    pivot.spawn((
                        WheelVisual {
                            index: i,
                            radius: WHEEL_RADIUS,
                            rest,
                            pivot: 0.0,
                        },
                        Mesh3d(assets.wheel.clone()),
                        MeshMaterial3d(assets.wheel_material.clone()),
                        Transform::from_xyz(anchor.x.signum() * WHEEL_X, anchor.y, anchor.z)
                            .with_rotation(rest),
                    ));
                }
                for x in [-EXHAUST_X, EXHAUST_X] {
                    pivot.spawn((
                        BoostFlame { phase: x * 7.0 },
                        Mesh3d(assets.spark.clone()),
                        MeshMaterial3d(assets.flame_material.clone()),
                        Transform::from_xyz(x, EXHAUST_Y, EXHAUST_Z),
                        Visibility::Hidden,
                    ));
                    pivot.spawn((
                        StallSmoke { phase: x * 5.0 },
                        Mesh3d(assets.spark.clone()),
                        MeshMaterial3d(assets.smoke_material.clone()),
                        Transform::from_xyz(x, EXHAUST_Y + 0.1, EXHAUST_Z + 0.2),
                        Visibility::Hidden,
                    ));
                }
                for x in [-WHEEL_X, WHEEL_X] {
                    for tier in MiniTurbo::ALL {
                        pivot.spawn((
                            DriftSpark {
                                tier,
                                phase: x + tier.index() as f32 * 2.1,
                            },
                            Mesh3d(assets.spark.clone()),
                            MeshMaterial3d(assets.spark_materials[tier.index()].clone()),
                            Transform::from_translation(SPARK_OFFSET + Vec3::X * x),
                            Visibility::Hidden,
                        ));
                    }
                }
            });
        })
        .id()
}

/// Each body wears the color of its kart's [`KartPaint`] (issue #55): the shared material of that
/// color, swapped only when the color changes (a dozen lookups a frame, nothing allocated).
fn repaint_bodies(
    assets: Option<Res<KartVisualAssets>>,
    karts: Query<&KartPaint>,
    mut bodies: Query<(&mut BodyPaint, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (mut body, mut material) in &mut bodies {
        let Ok(KartPaint(color)) = karts.get(body.kart).copied() else {
            continue;
        };
        if body.shown != color {
            body.shown = color;
            material.0 = assets.paint(color);
        }
    }
}

/// Puts each wheel where its suspension ray touches the ground (fully extended in the air).
/// Issue #78: in anti-gravity, the wheels pivot flat (shown only), in
/// `EnvironmentTuning::wheel_pivot_seconds`, and back out of it; upright, they are never turned.
fn place_wheels(
    time: Res<Time>,
    tuning: Res<KartTuning>,
    karts: Query<(&KartWheels, Option<&KartEnvironment>)>,
    pivots: Query<&KartBodyPivot>,
    mut wheels: Query<(&mut WheelVisual, &ChildOf, &mut Transform)>,
) {
    let step = time.delta_secs() / tuning.environment.wheel_pivot_seconds.max(1e-3);
    for (mut wheel, child_of, mut transform) in &mut wheels {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok((state, environment)) = karts.get(pivot.body) else {
            continue;
        };
        let flat = environment.is_some_and(|e| e.anti_gravity);
        if flat || wheel.pivot > 0.0 {
            let target = if flat { 1.0 } else { 0.0 };
            let pivot = super::approach(wheel.pivot, target, step);
            if pivot != wheel.pivot {
                wheel.pivot = pivot;
                transform.rotation =
                    pivoted_wheel(wheel.rest, WHEEL_ANCHORS[wheel.index].x, wheel.pivot);
            }
        }
        let extension = tuning.suspension_rest_length - state.compression[wheel.index];
        let anchor = WHEEL_ANCHORS[wheel.index];
        let y = if state.on_ground[wheel.index] {
            anchor.y - extension + wheel.radius
        } else {
            anchor.y - tuning.suspension_rest_length + wheel.radius
        };
        if transform.translation.y != y {
            transform.translation.y = y;
        }
    }
}

/// The body's spin out or tumble after an item hit (issue #16), `t` from 0 (the hit) to 1 (the
/// end): its rotation (two turns around its vertical axis for a spin out, a forward flip for a
/// tumble) and how high it is thrown (m). Both start fast and settle.
pub fn hit_pose(reaction: HitReaction, t: f32) -> (Quat, f32) {
    let t = t.clamp(0.0, 1.0);
    let settle = 1.0 - (1.0 - t) * (1.0 - t);
    match reaction {
        HitReaction::Spin => (Quat::from_rotation_y(-2.0 * TAU * settle), 0.0),
        HitReaction::Tumble => (
            Quat::from_rotation_x(-TAU * settle),
            TUMBLE_HEIGHT * (std::f32::consts::PI * t).sin(),
        ),
    }
}

/// Whether a kart invincible after an item hit is shown at `now` (s): it blinks (GD § 8).
pub fn shown_while_invincible(invincibility: &Invincibility, now: f32) -> bool {
    !invincibility.is_active() || (now * INVINCIBLE_BLINK_HZ).fract() < 0.6
}

/// Same, with its item effects (issue #18): a super câlin or a fusée makes it invincible too,
/// but it does not blink then (it glows, `crate::items`): only the protection after a hit
/// blinks. Invisible (issue #21, a pie voleuse), it is not shown at all: a ghost stands for it
/// (`crate::items`, faint for the others).
pub fn shown_with_effects(
    invincibility: &Invincibility,
    effects: Option<&ItemEffects>,
    now: f32,
) -> bool {
    if effects.is_some_and(|e| e.hidden()) {
        return false;
    }
    effects.is_some_and(|e| e.rams()) || shown_while_invincible(invincibility, now)
}

/// Turns the body by the drift's slip angle around the front axle (the simulation's
/// `DriftMotion`, interpolated between the last two ticks like the pose); rolls the body once
/// when a trick starts; shakes it while the engine stalls; spins or flips it after an item hit
/// and makes it blink while it is invincible (issue #16).
fn pose_bodies(
    time: Res<Time>,
    fixed_time: Res<Time<Fixed>>,
    deflated_scale: Option<Res<DeflatedScale>>,
    kart_tuning: Option<Res<KartTuning>>,
    karts: Query<(
        &KartState,
        &DriftMotion,
        &Stall,
        &Invincibility,
        Option<&ItemEffects>,
        (Option<&KartEnvironment>, Option<&Velocity>),
    )>,
    mut pivots: Query<(&mut KartBodyPivot, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let alpha = fixed_time.overstep_fraction();
    let now = time.elapsed_secs();
    let small = deflated_scale.map_or(1.0, |s| s.0);
    for (mut pivot, mut transform, mut visibility) in &mut pivots {
        let Ok((state, motion, stall, invincibility, effects, (environment, velocity))) =
            karts.get(pivot.body)
        else {
            continue;
        };
        // Issue #20: gliding, the body pitches and banks (shown only).
        let glide = match (environment, velocity, kart_tuning.as_deref()) {
            (Some(environment), Some(velocity), Some(tuning)) if environment.gliding => {
                let (pitch, bank) =
                    glide_pose(&tuning.environment, velocity.linear, velocity.angular.y);
                Some(Quat::from_rotation_x(pitch) * Quat::from_rotation_z(-bank))
            }
            _ => None,
        };
        visibility.set_if_neq(if shown_with_effects(invincibility, effects, now) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        // Deflated (issue #18): the whole kart looks smaller.
        let scale = Vec3::splat(effects.map_or(1.0, |e| e.deflated_scale(small)));
        if transform.scale != scale {
            transform.scale = scale;
        }
        let hit = match *state {
            KartState::Hit {
                reaction,
                elapsed,
                duration,
            } => Some(hit_pose(
                reaction,
                (elapsed as f32 + alpha) / duration.max(1) as f32,
            )),
            _ => None,
        };
        let slip = motion.interpolated_slip(alpha);
        // Trick: a new roll when it starts; a roll in progress finishes even after landing.
        let tricking = *state == KartState::Trick;
        let mut spin = pivot.spin;
        if tricking && !pivot.tricking {
            spin = 0.0;
        } else if spin < TRICK_SPIN_SECONDS {
            spin = (spin + dt).min(TRICK_SPIN_SECONDS);
        }
        // The stalled engine coughs: the body shakes a little.
        let shake = if stall.is_active() {
            0.03 * (now * 55.0).sin()
        } else {
            0.0
        };
        let (hit_rotation, lift) = hit.unwrap_or((Quat::IDENTITY, 0.0));
        let height = shake + lift;
        if slip != pivot.slip
            || spin != pivot.spin
            || tricking != pivot.tricking
            || hit.is_some()
            || pivot.hit
            || glide.is_some()
            || pivot.gliding
        {
            pivot.slip = slip;
            pivot.spin = spin;
            pivot.tricking = tricking;
            pivot.hit = hit.is_some();
            pivot.gliding = glide.is_some();
            let t = spin / TRICK_SPIN_SECONDS;
            // Ease in and out; a full turn is the same as none.
            let roll = TAU * t * t * (3.0 - 2.0 * t);
            // The roll turns the body around its center, the slip around the front axle.
            let (yaw, offset) = slip_pose(slip);
            transform.rotation =
                glide.unwrap_or(Quat::IDENTITY) * hit_rotation * yaw * Quat::from_rotation_z(roll);
            transform.translation = offset.with_y(height);
        }
        if transform.translation.y != height {
            transform.translation.y = height;
        }
    }
}

/// Shows the sparks of the tier reached by the drift, flickering.
fn show_drift_sparks(
    time: Res<Time>,
    tuning: Res<KartTuning>,
    karts: Query<(&KartState, &KartAssists, &StatEffects)>,
    pivots: Query<&KartBodyPivot>,
    mut sparks: Query<(&DriftSpark, &ChildOf, &mut Visibility, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (spark, child_of, mut visibility, mut transform) in &mut sparks {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok((state, assists, effects)) = karts.get(pivot.body) else {
            continue;
        };
        // The tiers of this kart (its Mini-turbo stat moves the thresholds).
        let shown = matches!(state, KartState::Drifting { .. })
            && state.mini_turbo(&effects.tuning(&tuning), assists.assisted_steering)
                == Some(spark.tier);
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if shown {
            let flicker = 0.7 + 0.6 * (now * SPARK_FLICKER + spark.phase).sin().abs();
            transform.scale = Vec3::splat(SPARK_SIZES[spark.tier.index()] * flicker);
        }
    }
}

/// Exhaust flames during a boost, sized by `flame_size`, flickering.
fn show_boost_flames(
    time: Res<Time>,
    tuning: Res<KartTuning>,
    karts: Query<&Boost>,
    pivots: Query<&KartBodyPivot>,
    mut flames: Query<(&BoostFlame, &ChildOf, &mut Visibility, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (flame, child_of, mut visibility, mut transform) in &mut flames {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(boost) = karts.get(pivot.body) else {
            continue;
        };
        let size = flame_size(&tuning, boost);
        visibility.set_if_neq(if size > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if size > 0.0 {
            let flicker = 0.8 + 0.4 * (now * FLAME_FLICKER + flame.phase).sin().abs();
            let length = FLAME_LENGTH * size * flicker;
            let width = FLAME_WIDTH * (0.6 + 0.4 * size);
            transform.scale = Vec3::new(width, width, length);
            transform.translation.z = EXHAUST_Z + length / 2.0;
        }
    }
}

/// Grey puffs popping at the exhaust while the engine stalls.
fn show_stall_smoke(
    time: Res<Time>,
    karts: Query<&Stall>,
    pivots: Query<&KartBodyPivot>,
    mut puffs: Query<(&StallSmoke, &ChildOf, &mut Visibility, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (puff, child_of, mut visibility, mut transform) in &mut puffs {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(stall) = karts.get(pivot.body) else {
            continue;
        };
        let shown = stall.is_active();
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if shown {
            // Each puff grows and rises, then pops back: the engine coughing.
            let cycle = ((now * SMOKE_FLICKER + puff.phase) / TAU).fract();
            transform.scale = Vec3::splat(SMOKE_SIZE * (0.4 + cycle));
            transform.translation.y = EXHAUST_Y + 0.1 + 0.4 * cycle;
        }
    }
}

/// Wind lines streaming past the kart while its slipstream gauge charges; longer as it fills.
fn show_wind_lines(
    time: Res<Time>,
    tuning: Res<KartTuning>,
    karts: Query<&Slipstream>,
    roots: Query<&InterpolatedFrom>,
    mut lines: Query<(&WindLine, &ChildOf, &mut Visibility, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (line, child_of, mut visibility, mut transform) in &mut lines {
        let Ok(InterpolatedFrom(body)) = roots.get(child_of.parent()) else {
            continue;
        };
        let Ok(slipstream) = karts.get(*body) else {
            continue;
        };
        let shown = slipstream.charging;
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if shown {
            let fill = slipstream.fill(&tuning);
            let length = WIND_LENGTH[0] + (WIND_LENGTH[1] - WIND_LENGTH[0]) * fill;
            // From the front of the kart to behind it, each line on its own beat.
            let travel = (now * WIND_SPEED + line.index as f32 * 1.7).rem_euclid(WIND_TRAVEL);
            transform.translation.z = -1.5 + travel;
            transform.scale = Vec3::new(WIND_THICKNESS, WIND_THICKNESS, length);
        }
    }
}
