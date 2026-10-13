//! Kart visuals in primitive shapes, until real models arrive (issue #11).
//!
//! The visual is a separate entity that follows the simulated body with interpolation
//! (`crate::interpolation`). Under it, a pivot ([`KartBodyPivot`]) carries the body, driver and
//! wheels: it turns the kart's nose inside the curve while drifting (the slip angle is purely
//! visual, the simulated body keeps its grip and the camera keeps following the direction of
//! travel). The wheels follow the suspension, so the kart visibly hugs the ground and bounces
//! on landings. Drift sparks at the rear wheels show the mini-turbo tier (GD § 14).

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::drift::{MiniTurbo, drift_slip_angle};
use super::{KartAssists, KartInput, KartState, KartTuning, KartWheels, WHEEL_ANCHORS};
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

/// The pivot under a kart visual that carries every visible part. `slip` is the current visual
/// slip angle (radians, positive = nose to the right).
#[derive(Component, Clone, Copy, Debug)]
pub struct KartBodyPivot {
    /// The simulated kart body.
    pub body: Entity,
    pub slip: f32,
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
                (place_wheels, slip_into_drifts, show_drift_sparks)
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
    wheel: Handle<Mesh>,
    spark: Handle<Mesh>,
    driver_material: Handle<StandardMaterial>,
    wheel_material: Handle<StandardMaterial>,
    spark_materials: [Handle<StandardMaterial>; 3],
}

impl KartVisualAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        Self {
            body: meshes.add(Cuboid::new(1.2, 0.45, 2.0)),
            driver: meshes.add(Sphere::new(0.45)),
            ear: meshes.add(Sphere::new(0.16)),
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
/// sparks). Only the body material is new; everything else comes from `assets`.
pub fn spawn_kart_visual(
    commands: &mut Commands,
    assets: &KartVisualAssets,
    materials: &mut Assets<StandardMaterial>,
    body: Entity,
    color: Color,
) -> Entity {
    let body_material = materials.add(color);
    let assets = assets.clone();

    commands
        .spawn((
            InterpolatedFrom(body),
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|kart| {
            kart.spawn((
                KartBodyPivot { body, slip: 0.0 },
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|pivot| {
                pivot.spawn((
                    Mesh3d(assets.body.clone()),
                    MeshMaterial3d(body_material),
                    Transform::default(),
                ));
                pivot.spawn((
                    Mesh3d(assets.driver.clone()),
                    MeshMaterial3d(assets.driver_material.clone()),
                    Transform::from_xyz(0.0, 0.6, 0.15),
                ));
                for x in [-0.25, 0.25] {
                    pivot.spawn((
                        Mesh3d(assets.ear.clone()),
                        MeshMaterial3d(assets.driver_material.clone()),
                        Transform::from_xyz(x, 1.0, 0.15),
                    ));
                }
                for (i, anchor) in WHEEL_ANCHORS.iter().enumerate() {
                    pivot.spawn((
                        WheelVisual(i),
                        Mesh3d(assets.wheel.clone()),
                        MeshMaterial3d(assets.wheel_material.clone()),
                        Transform::from_xyz(anchor.x.signum() * WHEEL_X, anchor.y, anchor.z)
                            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
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

/// Turns the kart's nose inside the curve while drifting, smoothly (frame-rate independent).
fn slip_into_drifts(
    time: Res<Time>,
    tuning: Res<KartTuning>,
    karts: Query<(&KartState, &KartInput)>,
    mut pivots: Query<(&mut KartBodyPivot, &mut Transform)>,
) {
    let blend = 1.0 - (-tuning.drift_slip_response * time.delta_secs()).exp();
    for (mut pivot, mut transform) in &mut pivots {
        let Ok((state, input)) = karts.get(pivot.body) else {
            continue;
        };
        let target = match *state {
            KartState::Drifting { side, .. } => drift_slip_angle(&tuning, side, input.steer),
            _ => 0.0,
        };
        let mut slip = pivot.slip + (target - pivot.slip) * blend;
        if (target - slip).abs() < 1e-4 {
            slip = target;
        }
        if slip != pivot.slip {
            pivot.slip = slip;
            // Nose to the right = clockwise seen from above = negative rotation around Y.
            transform.rotation = Quat::from_rotation_y(-slip);
        }
    }
}

/// Shows the sparks of the tier reached by the drift, flickering.
fn show_drift_sparks(
    time: Res<Time>,
    tuning: Res<KartTuning>,
    karts: Query<(&KartState, &KartAssists)>,
    pivots: Query<&KartBodyPivot>,
    mut sparks: Query<(&DriftSpark, &ChildOf, &mut Visibility, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (spark, child_of, mut visibility, mut transform) in &mut sparks {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok((state, assists)) = karts.get(pivot.body) else {
            continue;
        };
        let shown = state.mini_turbo(&tuning, assists.assisted_steering) == Some(spark.tier);
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
