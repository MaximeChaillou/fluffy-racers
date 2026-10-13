//! Kart visuals in primitive shapes, until real models arrive (issue #11).
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
//! The driver (issue #15) is the character: a fluffy ball whose size shows its weight class and
//! whose color and ears show the animal ([`DriverLook`]). Every character has one shared
//! material; only the body material is the kart's own.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use serde::Deserialize;

use super::drift::{DriftMotion, MiniTurbo, slip_pose};
use super::rocket::Stall;
use super::slipstream::Slipstream;
use super::{Boost, KartAssists, KartState, KartTuning, KartWheels, StatEffects, WHEEL_ANCHORS};
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};

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
/// The driver's ball: radius at size 1 (m), and where it sits on the body (its bottom on the
/// seat, whatever its size).
const DRIVER_RADIUS: f32 = 0.45;
const DRIVER_SEAT: Vec3 = Vec3::new(0.0, 0.15, 0.15);

/// The ears of a driver (issue #15), primitives until the real models (#11).
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DriverEars {
    /// Two balls (mouse, hamster, bear, cow).
    #[default]
    Round,
    /// Two long ones standing up (rabbit).
    Long,
    /// Two cones (cat, fox).
    Pointy,
    /// None (chick, walrus).
    None,
}

/// How a kart's driver looks (issue #15): its size (1 = medium; the weight class shows), its
/// shared material (one per character: its color) and its ears.
#[derive(Clone, Debug, PartialEq)]
pub struct DriverLook {
    pub size: f32,
    pub material: Handle<StandardMaterial>,
    pub ears: DriverEars,
}

/// The driver under a kart visual's pivot: its size (scale) shows the character's weight class.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DriverVisual {
    pub size: f32,
}

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

/// A wheel of a kart visual: index into `WHEEL_ANCHORS` / `KartWheels`.
#[derive(Component, Clone, Copy, Debug)]
pub struct WheelVisual(pub usize);

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
        app.add_systems(Startup, create_kart_visual_assets)
            .add_systems(
                PostUpdate,
                (
                    place_wheels,
                    pose_bodies,
                    show_drift_sparks,
                    show_boost_flames,
                    show_stall_smoke,
                    show_wind_lines,
                )
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// Meshes and materials shared by every kart visual: 12 karts on the grid reuse the same assets
/// (fewer GPU resources, better batching); only the body color differs per kart.
#[derive(Resource, Clone, Debug)]
pub struct KartVisualAssets {
    body: Handle<Mesh>,
    driver: Handle<Mesh>,
    ear: Handle<Mesh>,
    long_ear: Handle<Mesh>,
    pointy_ear: Handle<Mesh>,
    wheel: Handle<Mesh>,
    spark: Handle<Mesh>,
    driver_material: Handle<StandardMaterial>,
    wheel_material: Handle<StandardMaterial>,
    spark_materials: [Handle<StandardMaterial>; 3],
    flame_material: Handle<StandardMaterial>,
    smoke_material: Handle<StandardMaterial>,
    wind_material: Handle<StandardMaterial>,
}

impl KartVisualAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        Self {
            body: meshes.add(Cuboid::new(1.2, 0.45, 2.0)),
            driver: meshes.add(Sphere::new(DRIVER_RADIUS)),
            ear: meshes.add(Sphere::new(0.16)),
            long_ear: meshes.add(Capsule3d::new(0.08, 0.32)),
            pointy_ear: meshes.add(Cone::new(0.14, 0.3)),
            wheel: meshes.add(Cylinder::new(WHEEL_RADIUS, WHEEL_WIDTH)),
            // A unit cube scaled per frame; unlit so the sparks glow in the shade too.
            spark: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            driver_material: materials.add(Color::srgb(0.98, 0.92, 0.80)),
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
        }
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
/// sparks, boost flames, stall puffs, wind lines). Only the body material is new; everything
/// else comes from `assets`. The driver is the default one (medium, cream, round ears).
pub fn spawn_kart_visual(
    commands: &mut Commands,
    assets: &KartVisualAssets,
    materials: &mut Assets<StandardMaterial>,
    body: Entity,
    color: Color,
) -> Entity {
    spawn_kart_visual_with_driver(commands, assets, materials, body, color, None)
}

/// Same with the driver of a character (issue #15; `None`: the default one).
pub fn spawn_kart_visual_with_driver(
    commands: &mut Commands,
    assets: &KartVisualAssets,
    materials: &mut Assets<StandardMaterial>,
    body: Entity,
    color: Color,
    driver: Option<&DriverLook>,
) -> Entity {
    let body_material = materials.add(color);
    let assets = assets.clone();
    let (size, driver_material, ears) = match driver {
        Some(look) => (look.size, look.material.clone(), look.ears),
        None => (1.0, assets.driver_material.clone(), DriverEars::Round),
    };

    commands
        .spawn((
            InterpolatedFrom(body),
            Transform::default(),
            Visibility::default(),
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
                },
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|pivot| {
                pivot.spawn((
                    Mesh3d(assets.body.clone()),
                    MeshMaterial3d(body_material),
                    Transform::default(),
                ));
                // The driver: its bottom on the seat, scaled with its size (ears included).
                pivot
                    .spawn((
                        DriverVisual { size },
                        Transform::from_translation(DRIVER_SEAT).with_scale(Vec3::splat(size)),
                        Visibility::default(),
                    ))
                    .with_children(|driver| {
                        driver.spawn((
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
                        if let Some((mesh, height, tilt)) = ear {
                            for x in [-0.25f32, 0.25] {
                                driver.spawn((
                                    Mesh3d(mesh.clone()),
                                    MeshMaterial3d(driver_material.clone()),
                                    Transform::from_xyz(x, height, 0.0)
                                        .with_rotation(Quat::from_rotation_z(-tilt * x.signum())),
                                ));
                            }
                        }
                    });
                for (i, anchor) in WHEEL_ANCHORS.iter().enumerate() {
                    pivot.spawn((
                        WheelVisual(i),
                        Mesh3d(assets.wheel.clone()),
                        MeshMaterial3d(assets.wheel_material.clone()),
                        Transform::from_xyz(anchor.x.signum() * WHEEL_X, anchor.y, anchor.z)
                            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
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

/// Puts each wheel where its suspension ray touches the ground (fully extended in the air).
fn place_wheels(
    tuning: Res<KartTuning>,
    karts: Query<&KartWheels>,
    pivots: Query<&KartBodyPivot>,
    mut wheels: Query<(&WheelVisual, &ChildOf, &mut Transform)>,
) {
    for (wheel, child_of, mut transform) in &mut wheels {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(state) = karts.get(pivot.body) else {
            continue;
        };
        let extension = tuning.suspension_rest_length - state.compression[wheel.0];
        let anchor = WHEEL_ANCHORS[wheel.0];
        let y = if state.on_ground[wheel.0] {
            anchor.y - extension + WHEEL_RADIUS
        } else {
            anchor.y - tuning.suspension_rest_length + WHEEL_RADIUS
        };
        transform.translation.y = y;
    }
}

/// Turns the body by the drift's slip angle around the front axle (the simulation's
/// `DriftMotion`, interpolated between the last two ticks like the pose); rolls the body once
/// when a trick starts; shakes it while the engine stalls.
fn pose_bodies(
    time: Res<Time>,
    fixed_time: Res<Time<Fixed>>,
    karts: Query<(&KartState, &DriftMotion, &Stall)>,
    mut pivots: Query<(&mut KartBodyPivot, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let alpha = fixed_time.overstep_fraction();
    let now = time.elapsed_secs();
    for (mut pivot, mut transform) in &mut pivots {
        let Ok((state, motion, stall)) = karts.get(pivot.body) else {
            continue;
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
        if slip != pivot.slip || spin != pivot.spin || tricking != pivot.tricking {
            pivot.slip = slip;
            pivot.spin = spin;
            pivot.tricking = tricking;
            let t = spin / TRICK_SPIN_SECONDS;
            // Ease in and out; a full turn is the same as none.
            let roll = TAU * t * t * (3.0 - 2.0 * t);
            // The roll turns the body around its center, the slip around the front axle.
            let (yaw, offset) = slip_pose(slip);
            transform.rotation = yaw * Quat::from_rotation_z(roll);
            transform.translation = offset.with_y(shake);
        }
        if transform.translation.y != shake {
            transform.translation.y = shake;
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
