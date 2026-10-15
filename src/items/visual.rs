//! Item visuals in primitive shapes (GD § 8 and 14: dangerous items are recognized at once), all
//! sharing a handful of meshes and materials, until the real models (issue #11).
//!
//! - **Box**: a translucent cube turning on itself, sky blue (gold for a double box), with a
//!   white core. Broken, it disappears; back, it pops in. Hidden when the items are switched off.
//! - **Pelure**: two yellow strips in a V, flat on the ground. **Pelote de laine**: a pink ball
//!   wrapped in a darker band, rolling. **Abeille**: a yellow and black ball with two white wings,
//!   facing where it flies, its wings beating.
//! - The items out move smoothly between two ticks (their simulated `position` and `previous`).
//! - **Phase 2** (issue #18): **bombe à confettis**, a navy ball with a yellow fuse and its spark;
//!   **oreiller géant**, a big white cushion edged in blue with four tufts; an **explosion**
//!   ([`Blast`]), a burst of confetti around a pale flash; the **klaxon-canard**, an orange ring
//!   that spreads out; a **super câlin**, a pink glow around the kart; a **fusée à confettis**,
//!   an orange shell with a white nose around the kart.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::table::ItemKind;
use super::{Blast, Honked, ItemBox, ItemObject, ItemTuning};
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::ItemEffects;
use crate::level::LevelScoped;
use crate::track::ItemBoxSpot;

/// Size of a box (m).
const BOX_SIZE: f32 = 1.0;
/// A box turns this fast (rad/s); back on the track, it pops in this long (s).
const BOX_SPIN: f32 = 1.6;
const BOX_POP_IN: f32 = 0.3;
/// An abeille's wings beat this fast (rad/s).
const WING_BEAT: f32 = 60.0;
/// Confetti pieces of an explosion, and how fast they fly out (m/s).
const CONFETTI: usize = 14;
const CONFETTI_SPEED: f32 = 9.0;
/// The klaxon-canard's ring spreads out in this long (s).
const HONK_TIME: f32 = 0.4;
/// The confetti colors (GD § 14: bright).
const CONFETTI_COLORS: [Color; 5] = [
    Color::srgb(1.0, 0.25, 0.45),
    Color::srgb(1.0, 0.85, 0.15),
    Color::srgb(0.25, 0.8, 1.0),
    Color::srgb(0.45, 0.95, 0.35),
    Color::srgb(0.75, 0.4, 1.0),
];

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
    // Issue #18.
    fuse: Handle<Mesh>,
    ring: Handle<Mesh>,
    shell: Handle<Mesh>,
    bombe_material: Handle<StandardMaterial>,
    fuse_material: Handle<StandardMaterial>,
    spark_material: Handle<StandardMaterial>,
    pillow_material: Handle<StandardMaterial>,
    pillow_edge_material: Handle<StandardMaterial>,
    flash_material: Handle<StandardMaterial>,
    confetti_materials: [Handle<StandardMaterial>; 5],
    honk_material: Handle<StandardMaterial>,
    calin_material: Handle<StandardMaterial>,
    rocket_material: Handle<StandardMaterial>,
    rocket_nose_material: Handle<StandardMaterial>,
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
            fuse: meshes.add(Capsule3d::new(0.06, 0.25)),
            ring: meshes.add(Torus::new(0.9, 1.0)),
            shell: meshes.add(Capsule3d::new(1.0, 1.6)),
            bombe_material: materials.add(Color::srgb(0.16, 0.2, 0.5)),
            fuse_material: materials.add(Color::srgb(1.0, 0.85, 0.3)),
            spark_material: materials.add(unlit(Color::srgb(1.0, 0.6, 0.1))),
            pillow_material: materials.add(Color::srgb(0.97, 0.97, 1.0)),
            pillow_edge_material: materials.add(Color::srgb(0.4, 0.6, 1.0)),
            flash_material: materials.add(StandardMaterial {
                unlit: true,
                ..translucent(Color::srgba(1.0, 0.97, 0.85, 0.35))
            }),
            confetti_materials: CONFETTI_COLORS.map(|color| materials.add(unlit(color))),
            honk_material: materials.add(StandardMaterial {
                unlit: true,
                ..translucent(Color::srgba(1.0, 0.6, 0.1, 0.6))
            }),
            calin_material: materials.add(StandardMaterial {
                unlit: true,
                ..translucent(Color::srgba(1.0, 0.45, 0.75, 0.35))
            }),
            rocket_material: materials.add(Color::srgb(1.0, 0.45, 0.1)),
            rocket_nose_material: materials.add(Color::srgb(0.98, 0.98, 0.95)),
        }
    }
}

/// An unlit material (it glows in the shade too).
fn unlit(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    }
}

/// An explosion's visual (issue #18): its age (s) and reach (m).
#[derive(Component, Clone, Copy, Debug)]
pub struct BlastLook {
    pub age: f32,
    pub radius: f32,
}

/// A piece of confetti of an explosion: its direction out.
#[derive(Component, Clone, Copy, Debug)]
pub struct Confetti {
    pub direction: Vec3,
}

/// The klaxon-canard's ring (issue #18): its age (s) and reach (m).
#[derive(Component, Clone, Copy, Debug)]
pub struct HonkLook {
    pub age: f32,
    pub radius: f32,
}

/// What an item does to a kart, seen around it (issue #18).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    /// Super câlin: a pink glow.
    Calin,
    /// Fusée à confettis: an orange shell.
    Rocket,
}

/// The visual of an item effect around `kart` (issue #18), following its interpolated pose.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectLook {
    pub kart: Entity,
    pub kind: EffectKind,
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
            .add_systems(
                Update,
                (
                    dress_boxes,
                    animate_boxes,
                    dress_items,
                    show_blasts,
                    show_effects,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                (place_items, beat_wings, animate_blasts, animate_effects)
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
    boxes: Query<&ItemBox>,
    mut looks: Query<(&ChildOf, &mut ItemBoxLook, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (child_of, mut look, mut transform, mut visibility) in &mut looks {
        let Ok(item_box) = boxes.get(child_of.parent()) else {
            continue;
        };
        let shown = tuning.enabled && item_box.available();
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
                ItemKind::Bombe => {
                    // A navy ball, a yellow fuse on top and its spark.
                    parts.spawn((
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.bombe_material.clone()),
                        Transform::from_scale(Vec3::splat(radius)),
                    ));
                    parts.spawn((
                        Mesh3d(assets.fuse.clone()),
                        MeshMaterial3d(assets.fuse_material.clone()),
                        Transform::from_xyz(0.0, radius * 1.05, 0.0)
                            .with_rotation(Quat::from_rotation_z(0.4)),
                    ));
                    parts.spawn((
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.spark_material.clone()),
                        Transform::from_xyz(radius * 0.25, radius * 1.45, 0.0)
                            .with_scale(Vec3::splat(0.09)),
                    ));
                }
                ItemKind::OreillerGeant => {
                    // A big white cushion edged in blue, four tufts at its corners.
                    parts.spawn((
                        Mesh3d(assets.cube.clone()),
                        MeshMaterial3d(assets.pillow_material.clone()),
                        Transform::from_scale(Vec3::new(2.2, 0.7, 1.5) * radius),
                    ));
                    parts.spawn((
                        Mesh3d(assets.cube.clone()),
                        MeshMaterial3d(assets.pillow_edge_material.clone()),
                        Transform::from_scale(Vec3::new(2.25, 0.25, 1.55) * radius),
                    ));
                    for (x, z) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        parts.spawn((
                            Mesh3d(assets.ball.clone()),
                            MeshMaterial3d(assets.pillow_material.clone()),
                            Transform::from_xyz(x * 1.1 * radius, 0.0, z * 0.75 * radius)
                                .with_scale(Vec3::splat(0.3 * radius)),
                        ));
                    }
                }
                _ => {}
            });
    }
}

/// An explosion (a pale flash and confetti flying out) and a klaxon-canard's ring, where they
/// happen (issue #18).
fn show_blasts(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    tuning: Res<ItemTuning>,
    mut blasts: MessageReader<Blast>,
    mut honks: MessageReader<Honked>,
) {
    for blast in blasts.read() {
        commands
            .spawn((
                Name::new("Explosion"),
                BlastLook {
                    age: 0.0,
                    radius: blast.radius,
                },
                LevelScoped,
                Transform::from_translation(blast.at),
                Visibility::default(),
            ))
            .with_children(|burst| {
                burst.spawn((
                    Mesh3d(assets.ball.clone()),
                    MeshMaterial3d(assets.flash_material.clone()),
                    Transform::from_scale(Vec3::splat(0.1)),
                    NotShadowCaster,
                ));
                for k in 0..CONFETTI {
                    // Spread around, upward (a golden angle apart).
                    let angle = k as f32 * 2.399_963;
                    let up = 0.35 + 0.6 * ((k * 7) % CONFETTI) as f32 / CONFETTI as f32;
                    let direction = Vec3::new(
                        angle.cos() * (1.0 - up * 0.5),
                        up,
                        angle.sin() * (1.0 - up * 0.5),
                    )
                    .normalize();
                    burst.spawn((
                        Confetti { direction },
                        NotShadowCaster,
                        Mesh3d(assets.cube.clone()),
                        MeshMaterial3d(assets.confetti_materials[k % 5].clone()),
                        Transform::from_scale(Vec3::new(0.22, 0.05, 0.14))
                            .with_rotation(Quat::from_rotation_y(angle)),
                    ));
                }
            });
    }
    for honk in honks.read() {
        commands.spawn((
            Name::new("Klaxon"),
            HonkLook {
                age: 0.0,
                radius: tuning.klaxon_radius,
            },
            LevelScoped,
            NotShadowCaster,
            Mesh3d(assets.ring.clone()),
            MeshMaterial3d(assets.honk_material.clone()),
            Transform::from_translation(honk.at).with_scale(Vec3::new(0.5, 0.3, 0.5)),
        ));
    }
}

/// The explosions grow and their confetti fly out and fall; the klaxon's ring spreads out. Gone
/// when they are over.
fn animate_blasts(
    time: Res<Time>,
    tuning: Res<ItemTuning>,
    mut commands: Commands,
    mut blasts: Query<(Entity, &mut BlastLook, &Children)>,
    mut parts: Query<(&mut Transform, Option<&Confetti>), Without<HonkLook>>,
    mut honks: Query<(Entity, &mut HonkLook, &mut Transform), Without<Confetti>>,
) {
    let dt = time.delta_secs();
    let show = tuning.blast_show_time.max(0.05);
    for (entity, mut blast, children) in &mut blasts {
        blast.age += dt;
        if blast.age >= show {
            commands.entity(entity).try_despawn();
            continue;
        }
        let t = blast.age / show;
        for child in children.iter() {
            let Ok((mut transform, confetti)) = parts.get_mut(child) else {
                continue;
            };
            match confetti {
                Some(confetti) => {
                    let out = confetti.direction * CONFETTI_SPEED * blast.age;
                    transform.translation = out - Vec3::Y * 6.0 * blast.age * blast.age;
                    transform.rotate_x(dt * 9.0);
                }
                None => {
                    // The flash: out to the blast's reach, then it fades (shrinks).
                    let size = blast.radius * (t * 3.0).min(1.0) * (1.0 - t * t);
                    transform.scale = Vec3::splat(size.max(0.01));
                }
            }
        }
    }
    for (entity, mut honk, mut transform) in &mut honks {
        honk.age += dt;
        if honk.age >= HONK_TIME {
            commands.entity(entity).try_despawn();
            continue;
        }
        let size = 0.5 + (honk.radius - 0.5) * honk.age / HONK_TIME;
        transform.scale = Vec3::new(size, 0.3, size);
    }
}

/// A kart in a super câlin glows pink, a kart in a fusée wears its shell (issue #18): shown
/// while it lasts, around the kart's interpolated pose.
fn show_effects(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    karts: Query<(Entity, &ItemEffects), Changed<ItemEffects>>,
    looks: Query<(Entity, &EffectLook)>,
    all: Query<&ItemEffects>,
) {
    for (look_entity, look) in &looks {
        let on = all.get(look.kart).is_ok_and(|effects| match look.kind {
            EffectKind::Calin => effects.calin > 0.0,
            EffectKind::Rocket => effects.rocket > 0.0,
        });
        if !on {
            commands.entity(look_entity).try_despawn();
        }
    }
    for (kart, effects) in &karts {
        for (kind, on) in [
            (EffectKind::Calin, effects.calin > 0.0),
            (EffectKind::Rocket, effects.rocket > 0.0),
        ] {
            if !on || looks.iter().any(|(_, l)| l.kart == kart && l.kind == kind) {
                continue;
            }
            let mut look = commands.spawn((
                Name::new("Item effect"),
                EffectLook { kart, kind },
                InterpolatedFrom(kart),
                LevelScoped,
                Transform::default(),
                Visibility::default(),
            ));
            look.with_children(|parts| match kind {
                EffectKind::Calin => {
                    parts.spawn((
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.calin_material.clone()),
                        Transform::from_xyz(0.0, 0.4, 0.0).with_scale(Vec3::splat(1.7)),
                        NotShadowCaster,
                    ));
                }
                EffectKind::Rocket => {
                    parts.spawn((
                        Mesh3d(assets.shell.clone()),
                        MeshMaterial3d(assets.rocket_material.clone()),
                        Transform::from_xyz(0.0, 0.25, 0.2)
                            .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                            .with_scale(Vec3::new(0.95, 1.0, 0.75)),
                    ));
                    parts.spawn((
                        Mesh3d(assets.ball.clone()),
                        MeshMaterial3d(assets.rocket_nose_material.clone()),
                        Transform::from_xyz(0.0, 0.25, -1.55).with_scale(Vec3::new(0.7, 0.55, 0.6)),
                    ));
                }
            });
        }
    }
}

/// The super câlin's glow pulses.
fn animate_effects(
    time: Res<Time>,
    looks: Query<(&EffectLook, &Children)>,
    mut parts: Query<&mut Transform>,
) {
    let pulse = 1.7 + 0.12 * (time.elapsed_secs() * 9.0).sin();
    for (look, children) in &looks {
        if look.kind != EffectKind::Calin {
            continue;
        }
        for child in children.iter() {
            if let Ok(mut transform) = parts.get_mut(child) {
                transform.scale = Vec3::splat(pulse);
            }
        }
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
            // The oreiller géant flies flat, its long side across its path.
            ItemKind::OreillerGeant if flat.length() > 0.1 => {
                Transform::default().looking_to(flat, Vec3::Y).rotation
                    * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
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
