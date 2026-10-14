//! Item visuals in primitive shapes (GD § 8 and 14: dangerous items are recognized at once), all
//! sharing a handful of meshes and materials, until the real models (issue #11).
//!
//! - **Box**: a translucent cube turning on itself, sky blue (gold for a double box), with a
//!   white core. Broken, it disappears; back, it pops in. Hidden when the items are switched off.
//! - **Pelure**: two yellow strips in a V, flat on the ground. **Pelote de laine**: a pink ball
//!   wrapped in a darker band, rolling. **Abeille**: a yellow and black ball with two white wings,
//!   facing where it flies, its wings beating.
//! - The items out move smoothly between two ticks (their simulated `position` and `previous`).

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::table::ItemKind;
use super::{ItemBox, ItemObject, ItemTuning};
use crate::interpolation::InterpolationSystems;
use crate::track::ItemBoxSpot;

/// Size of a box (m).
const BOX_SIZE: f32 = 1.0;
/// A box turns this fast (rad/s); back on the track, it pops in this long (s).
const BOX_SPIN: f32 = 1.6;
const BOX_POP_IN: f32 = 0.3;
/// An abeille's wings beat this fast (rad/s).
const WING_BEAT: f32 = 60.0;

/// Shared meshes and materials of the items.
#[derive(Resource, Clone)]
pub struct ItemVisualAssets {
    cube: Handle<Mesh>,
    ball: Handle<Mesh>,
    band: Handle<Mesh>,
    strip: Handle<Mesh>,
    wing: Handle<Mesh>,
    box_material: Handle<StandardMaterial>,
    double_box_material: Handle<StandardMaterial>,
    core_material: Handle<StandardMaterial>,
    pelure_material: Handle<StandardMaterial>,
    pelote_material: Handle<StandardMaterial>,
    pelote_band_material: Handle<StandardMaterial>,
    abeille_material: Handle<StandardMaterial>,
    stripe_material: Handle<StandardMaterial>,
    wing_material: Handle<StandardMaterial>,
}

impl ItemVisualAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let translucent = |color: Color| StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            ..default()
        };
        Self {
            cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            ball: meshes.add(Sphere::new(1.0)),
            band: meshes.add(Torus::new(0.85, 1.05)),
            strip: meshes.add(Capsule3d::new(0.09, 0.45)),
            wing: meshes.add(Sphere::new(1.0)),
            box_material: materials.add(translucent(Color::srgba(0.4, 0.8, 1.0, 0.55))),
            double_box_material: materials.add(translucent(Color::srgba(1.0, 0.82, 0.25, 0.6))),
            core_material: materials.add(StandardMaterial {
                base_color: Color::WHITE,
                emissive: LinearRgba::rgb(0.6, 0.6, 0.6),
                ..default()
            }),
            pelure_material: materials.add(Color::srgb(1.0, 0.86, 0.15)),
            pelote_material: materials.add(Color::srgb(0.95, 0.35, 0.6)),
            pelote_band_material: materials.add(Color::srgb(0.7, 0.15, 0.4)),
            abeille_material: materials.add(Color::srgb(1.0, 0.8, 0.1)),
            stripe_material: materials.add(Color::srgb(0.1, 0.08, 0.05)),
            wing_material: materials.add(translucent(Color::srgba(0.95, 0.97, 1.0, 0.8))),
        }
    }
}

/// The turning part of a box (its child), and how long it has been back.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ItemBoxLook {
    pub phase: f32,
    pub shown_for: f32,
}

/// An abeille's wing (a child of the item): it beats.
#[derive(Component, Clone, Copy, Debug)]
pub struct AbeilleWing {
    pub side: f32,
}

pub struct ItemVisualPlugin;

impl Plugin for ItemVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_item_visual_assets)
            .add_systems(Update, (dress_boxes, animate_boxes, dress_items).chain())
            .add_systems(
                PostUpdate,
                (place_items, beat_wings)
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

fn create_item_visual_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ItemVisualAssets::new(&mut meshes, &mut materials));
}

/// A new box of the track: its turning cube and white core.
fn dress_boxes(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    boxes: Query<(Entity, &ItemBoxSpot, &Transform), Added<ItemBoxSpot>>,
) {
    for (entity, spot, transform) in &boxes {
        let phase = transform.translation.x * 0.37 + transform.translation.z * 0.23;
        let material = if spot.double {
            assets.double_box_material.clone()
        } else {
            assets.box_material.clone()
        };
        commands.entity(entity).with_children(|item_box| {
            item_box
                .spawn((
                    ItemBoxLook {
                        phase,
                        shown_for: BOX_POP_IN,
                    },
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(BOX_SIZE)),
                    Visibility::default(),
                ))
                .with_children(|cube| {
                    cube.spawn((
                        Mesh3d(assets.cube.clone()),
                        MeshMaterial3d(assets.core_material.clone()),
                        Transform::from_scale(Vec3::splat(0.35))
                            .with_rotation(Quat::from_rotation_z(0.6)),
                    ));
                });
        });
    }
}

/// Boxes turn; a broken one is hidden, then pops back in; all hidden without items.
fn animate_boxes(
    time: Res<Time>,
    tuning: Res<ItemTuning>,
    online: Option<Res<crate::level::Online>>,
    boxes: Query<&ItemBox>,
    mut looks: Query<(&ChildOf, &mut ItemBoxLook, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (child_of, mut look, mut transform, mut visibility) in &mut looks {
        let Ok(item_box) = boxes.get(child_of.parent()) else {
            continue;
        };
        let shown = tuning.enabled && online.is_none() && item_box.available();
        visibility.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if !shown {
            look.shown_for = 0.0;
            continue;
        }
        look.shown_for += dt;
        let t = (look.shown_for / BOX_POP_IN).min(1.0);
        // Grows with a little overshoot.
        let scale = BOX_SIZE * (t + 0.25 * (std::f32::consts::PI * t).sin());
        let angle = now * BOX_SPIN + look.phase;
        transform.rotation = Quat::from_rotation_y(angle) * Quat::from_rotation_x(0.6);
        transform.translation.y = 0.12 * (now * 2.0 + look.phase).sin();
        transform.scale = Vec3::splat(scale.max(0.01));
    }
}

/// A new item out: its shape.
fn dress_items(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    tuning: Res<ItemTuning>,
    items: Query<(Entity, &ItemObject), Added<ItemObject>>,
) {
    for (entity, item) in &items {
        let radius = item.radius(&tuning);
        commands
            .entity(entity)
            .with_children(|parts| match item.item {
                ItemKind::Pelure => {
                    // Two strips in a V, flat on the ground.
                    for side in [-1.0f32, 1.0] {
                        parts.spawn((
                            Mesh3d(assets.strip.clone()),
                            MeshMaterial3d(assets.pelure_material.clone()),
                            Transform::from_xyz(side * 0.15, -radius * 0.5, 0.0).with_rotation(
                                Quat::from_rotation_y(side * 0.5)
                                    * Quat::from_rotation_x(FRAC_PI_2),
                            ),
                        ));
                    }
                }
                ItemKind::Pelote => {
                    parts.spawn((
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.pelote_material.clone()),
                        Transform::from_scale(Vec3::splat(radius)),
                    ));
                    for tilt in [0.0f32, 1.1] {
                        parts.spawn((
                            Mesh3d(assets.band.clone()),
                            MeshMaterial3d(assets.pelote_band_material.clone()),
                            Transform::from_scale(Vec3::splat(radius * 0.98))
                                .with_rotation(Quat::from_rotation_z(FRAC_PI_2 + tilt)),
                        ));
                    }
                }
                ItemKind::Abeille => {
                    parts.spawn((
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.abeille_material.clone()),
                        Transform::from_scale(Vec3::new(radius, radius, radius * 1.3)),
                    ));
                    for z in [-0.12f32, 0.12] {
                        parts.spawn((
                            Mesh3d(assets.band.clone()),
                            MeshMaterial3d(assets.stripe_material.clone()),
                            Transform::from_xyz(0.0, 0.0, z * radius * 3.0)
                                .with_scale(Vec3::new(radius * 0.97, radius * 0.4, radius * 0.97))
                                .with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                        ));
                    }
                    for side in [-1.0f32, 1.0] {
                        parts.spawn((
                            AbeilleWing { side },
                            Mesh3d(assets.wing.clone()),
                            MeshMaterial3d(assets.wing_material.clone()),
                            Transform::from_xyz(side * radius * 0.9, radius * 0.8, 0.0)
                                .with_scale(Vec3::new(radius * 0.9, radius * 0.12, radius * 0.5)),
                        ));
                    }
                }
                ItemKind::Piment | ItemKind::CroquetteDoree => {}
            });
    }
}

/// Every item out, between its last two simulated positions; a pelote rolls, an abeille faces
/// where it flies, a pelure turns slowly on the ground.
fn place_items(
    fixed_time: Res<Time<Fixed>>,
    tuning: Res<ItemTuning>,
    mut items: Query<(&ItemObject, &mut Transform)>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (item, mut transform) in &mut items {
        transform.translation = item.previous.lerp(item.position, alpha);
        let flat = item.velocity.with_y(0.0);
        transform.rotation = match item.item {
            ItemKind::Pelote if flat.length() > 0.1 => {
                // Rolled distance over the radius: its turn around the axis across its path.
                let axis = flat.normalize().cross(Vec3::Y);
                let angle = -item.age * flat.length() / item.radius(&tuning).max(0.05);
                Quat::from_axis_angle(axis, angle % TAU)
            }
            ItemKind::Abeille if item.velocity.length() > 0.1 => {
                Transform::default()
                    .looking_to(item.velocity, Vec3::Y)
                    .rotation
            }
            _ => Quat::from_rotation_y(item.age * 0.5),
        };
    }
}

fn beat_wings(time: Res<Time>, mut wings: Query<(&AbeilleWing, &mut Transform)>) {
    let beat = (time.elapsed_secs() * WING_BEAT).sin() * 0.6;
    for (wing, mut transform) in &mut wings {
        transform.rotation = Quat::from_rotation_z(wing.side * beat);
    }
}
