//! Kart visuals in primitive shapes, until real models arrive (issue #11).
//!
//! The visual is a separate entity that follows the simulated body with interpolation
//! (`crate::interpolation`). Its wheels follow the suspension, so the kart visibly hugs the
//! ground and bounces on landings.

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::{KartTuning, KartWheels, WHEEL_ANCHORS};
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};

const WHEEL_RADIUS: f32 = 0.28;
const WHEEL_WIDTH: f32 = 0.25;
/// Wheels stick out of the body on the sides.
const WHEEL_X: f32 = 0.7;

/// A wheel of a kart visual: index into `WHEEL_ANCHORS` / `KartWheels`.
#[derive(Component, Clone, Copy, Debug)]
pub struct WheelVisual(pub usize);

pub struct KartVisualPlugin;

impl Plugin for KartVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            place_wheels
                .after(InterpolationSystems)
                .before(TransformSystems::Propagate),
        );
    }
}

/// Spawns the visual of the kart `body` (pink box, fluffy ball driver, 4 wheels).
pub fn spawn_kart_visual(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    body: Entity,
    color: Color,
) -> Entity {
    let body_mesh = meshes.add(Cuboid::new(1.2, 0.45, 2.0));
    let body_material = materials.add(color);
    let driver_mesh = meshes.add(Sphere::new(0.45));
    let driver_material = materials.add(Color::srgb(0.98, 0.92, 0.80));
    let ear_mesh = meshes.add(Sphere::new(0.16));
    let wheel_mesh = meshes.add(Cylinder::new(WHEEL_RADIUS, WHEEL_WIDTH));
    let wheel_material = materials.add(Color::srgb(0.15, 0.15, 0.18));

    commands
        .spawn((
            InterpolatedFrom(body),
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|kart| {
            kart.spawn((
                Mesh3d(body_mesh),
                MeshMaterial3d(body_material),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ));
            kart.spawn((
                Mesh3d(driver_mesh),
                MeshMaterial3d(driver_material.clone()),
                Transform::from_xyz(0.0, 0.6, 0.15),
            ));
            for x in [-0.25, 0.25] {
                kart.spawn((
                    Mesh3d(ear_mesh.clone()),
                    MeshMaterial3d(driver_material.clone()),
                    Transform::from_xyz(x, 1.0, 0.15),
                ));
            }
            for (i, anchor) in WHEEL_ANCHORS.iter().enumerate() {
                kart.spawn((
                    WheelVisual(i),
                    Mesh3d(wheel_mesh.clone()),
                    MeshMaterial3d(wheel_material.clone()),
                    Transform::from_xyz(anchor.x.signum() * WHEEL_X, anchor.y, anchor.z)
                        .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
                ));
            }
        })
        .id()
}

/// Puts each wheel where its suspension ray touches the ground (fully extended in the air).
fn place_wheels(
    tuning: Res<KartTuning>,
    karts: Query<&KartWheels>,
    visuals: Query<&InterpolatedFrom>,
    mut wheels: Query<(&WheelVisual, &ChildOf, &mut Transform)>,
) {
    for (wheel, child_of, mut transform) in &mut wheels {
        let Ok(source) = visuals.get(child_of.parent()) else {
            continue;
        };
        let Ok(state) = karts.get(source.0) else {
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
