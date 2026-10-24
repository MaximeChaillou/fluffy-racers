//! Item visuals (GD § 8 and 14: dangerous items are recognized at once). Issue #106: each item
//! is its model (`models.rs`: soft shapes built in code, or a CC0 file), one mesh per model
//! shared by every instance, drawn with one material of vertex colors; a few parts move on their
//! own.
//!
//! - **Box**: a gift box with its bow (sky blue, gold for a double box), turning slowly and
//!   bouncing softly, its lid wobbling at each bounce. Broken, it bursts (it swells, its lid
//!   pops off, then it is gone; the confetti are `vfx`'s); back, it pops in. Hidden when the
//!   items are switched off.
//! - **The items out**: the Cac's (`ItemKind::Pelure`, issue #137) lying on the ground, wobbling
//!   like jelly (dropped, it falls the last bit and squashes as it lands); the pelote rolling;
//!   the abeille facing where it flies, its wings beating; the bombe blinking red, faster and
//!   faster, its spark crackling; the oreiller géant floating, rocking; the boomerang-os spinning
//!   flat; the piñata's items around the kart, the super câlin's heart beating. They move
//!   smoothly between two ticks (their simulated `position` and `previous`).
//! - **Phase 2** (issue #18): an **explosion** ([`Blast`]), a burst of confetti around a pale
//!   flash; the **klaxon-canard**, an orange ring that spreads out; a **super câlin**, a pink
//!   glow around the kart; a **fusée à confettis**, its rocket strapped to the kart's back.
//! - **Phase 3** (issue #21): the **pie voleuse**'s ghost: the kart vanishes, a white ghost with
//!   two eyes stands for it, plain for its own player, barely there for the others.
//! - **With a renderer** ([`ItemModelPlugin`], issue #106): the models from files once baked
//!   (they take the place of the built ones, in the same meshes, and their icons too); an item
//!   used pops up over its kart (its model, half a second); the admin panel's showcase, every
//!   model side by side. The headless game (the bot races) has none of it.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::icons::{ItemIcons, icon_view};
use super::models::{ItemModel, Look, MODEL_FILES, from_file, model};
use super::table::ItemKind;
use super::{Blast, Honked, ItemBox, ItemObject, ItemStats, ItemTuning, Motion};
use crate::camera::ChaseCamera;
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{ItemEffects, PlayerKart};
use crate::level::LevelScoped;
use crate::models::{Keep, ModelSystems, Models};
use crate::race::Course;
use crate::track::ItemBoxSpot;
use crate::track::circuit::to_mesh;

/// Size of a box (its model is about 1 m wide).
const BOX_SIZE: f32 = 1.0;
/// A box turns this fast (rad/s); back on the track, it pops in this long (s).
const BOX_SPIN: f32 = 1.1;
const BOX_POP_IN: f32 = 0.3;
/// A box bounces this high (m), this often (bounces a second), squashed this much as it lands.
const BOX_BOUNCE: f32 = 0.16;
const BOX_BOUNCE_HZ: f32 = 1.1;
const BOX_SQUASH: f32 = 0.09;
/// A box bursts this long when it is taken (s): it swells by this much, its lid flies up this
/// high (m).
const BOX_BURST: f32 = 0.18;
const BOX_BURST_SWELL: f32 = 0.45;
const BOX_LID_JUMP: f32 = 0.9;
/// An abeille's wings beat this fast (rad/s), this wide (rad).
const WING_BEAT: f32 = 60.0;
const WING_SWING: f32 = 0.6;
/// Confetti pieces of an explosion, and how fast they fly out (m/s).
const CONFETTI: usize = 14;
const CONFETTI_SPEED: f32 = 9.0;
/// The klaxon-canard's ring spreads out in this long (s).
const HONK_TIME: f32 = 0.4;
/// A boomerang-os spins this fast as it flies (rad/s, issue #21).
const BOOMERANG_SPIN: f32 = 22.0;
/// The bombe blinks this many times a second when it comes out, this many more at the end of its
/// fuse (`ItemTuning::bombe_fuse`); lit this share of each blink.
const BLINK_HZ: f32 = 2.5;
const BLINK_HZ_GAIN: f32 = 7.5;
const BLINK_LIT: f32 = 0.4;
/// The Cac's (issue #137): dropped, it falls this high (in its radius) in this long (s), stretched
/// by this much; as it lands it squashes by this much, then springs back (this fast, rad/s, and
/// this damped, /s), wobbling like jelly all along (`CACS_JELLY_*`: how fast, rad/s; how much it
/// squashes and widens; how fast it calms down, /s, to this share of it, which stays).
const CACS_DROP: f32 = 0.35;
const CACS_FALL: f32 = 0.1;
const CACS_STRETCH: f32 = 0.1;
const CACS_SQUASH: f32 = 0.28;
const CACS_SPRING: f32 = 17.0;
const CACS_DAMPING: f32 = 8.0;
const CACS_JELLY_SPEED: f32 = 7.0;
const CACS_JELLY: Vec2 = Vec2::new(0.05, 0.08);
const CACS_JELLY_CALM: f32 = 0.6;
const CACS_JELLY_REST: f32 = 0.25;
/// The oreiller géant floats this much up and down (in its radius), this often (Hz).
const PILLOW_FLOAT: f32 = 0.12;
const PILLOW_FLOAT_HZ: f32 = 0.9;
/// An item used pops up over its kart (issue #106): this long (s), this high over it (m), this
/// big (m across).
const FLOURISH_TIME: f32 = 0.75;
const FLOURISH_HEIGHT: f32 = 1.9;
const FLOURISH_SIZE: f32 = 0.95;
/// The showcase (issue #106): every model this big (m), this far apart (m), this far in front of
/// the camera (m), five in a row.
const SHOWCASE_SIZE: f32 = 1.0;
const SHOWCASE_STEP: Vec2 = Vec2::new(1.6, 1.2);
const SHOWCASE_DISTANCE: f32 = 10.0;
/// Its lowest row is this high over the kart the camera follows (m).
const SHOWCASE_LIFT: f32 = 0.8;
const SHOWCASE_COLUMNS: usize = 5;
/// The confetti colors (GD § 14: bright).
const CONFETTI_COLORS: [Color; 5] = [
    Color::srgb(1.0, 0.25, 0.45),
    Color::srgb(1.0, 0.85, 0.15),
    Color::srgb(0.25, 0.8, 1.0),
    Color::srgb(0.45, 0.95, 0.35),
    Color::srgb(0.75, 0.4, 1.0),
];

/// The meshes of a model: its body, its moving parts and where they sit (in the model's space).
#[derive(Clone, Debug)]
pub struct LookMeshes {
    pub body: Handle<Mesh>,
    pub wing: Option<(Handle<Mesh>, Vec3)>,
    pub spark: Option<(Handle<Mesh>, Vec3)>,
    pub lid: Option<(Handle<Mesh>, Vec3)>,
}

impl LookMeshes {
    fn new(meshes: &mut Assets<Mesh>, model: &ItemModel) -> Self {
        let mut part = |part: &Option<(crate::track::mesh::MeshData, Vec3)>| {
            part.as_ref()
                .map(|(mesh, at)| (meshes.add(to_mesh(mesh)), *at))
        };
        let (wing, spark, lid) = (part(&model.wing), part(&model.spark), part(&model.lid));
        Self {
            body: meshes.add(to_mesh(&model.body)),
            wing,
            spark,
            lid,
        }
    }

    /// The same meshes, drawn again from `model` (a file's), in place: every entity showing them
    /// follows.
    fn replace(&mut self, meshes: &mut Assets<Mesh>, model: &ItemModel) {
        let _ = meshes.insert(self.body.id(), to_mesh(&model.body));
        for (mine, theirs) in [
            (&mut self.wing, &model.wing),
            (&mut self.spark, &model.spark),
            (&mut self.lid, &model.lid),
        ] {
            match (mine.as_mut(), theirs) {
                (Some((handle, at)), Some((mesh, new_at))) => {
                    let _ = meshes.insert(handle.id(), to_mesh(mesh));
                    *at = *new_at;
                }
                (None, Some((mesh, at))) => *mine = Some((meshes.add(to_mesh(mesh)), *at)),
                _ => {}
            }
        }
    }
}

/// Shared meshes and materials of the items.
#[derive(Resource, Clone)]
pub struct ItemVisualAssets {
    /// Every model's meshes (issue #106), built in code at startup; a file's model takes their
    /// place once baked ([`ItemModelPlugin`]).
    looks: HashMap<Look, LookMeshes>,
    /// The models' material: their vertex colors, lit, a little glossy (candy).
    model_material: Handle<StandardMaterial>,
    /// The same, glowing (the golden items).
    glow_material: Handle<StandardMaterial>,
    /// The same, glowing red (the bombe's blink).
    blink_material: Handle<StandardMaterial>,
    /// The abeille's wings: pale, see-through, both sides.
    wing_material: Handle<StandardMaterial>,
    /// The bombe's spark: unlit (it glows in the shade too).
    spark_material: Handle<StandardMaterial>,
    cube: Handle<Mesh>,
    ball: Handle<Mesh>,
    ring: Handle<Mesh>,
    shell: Handle<Mesh>,
    flash_material: Handle<StandardMaterial>,
    confetti_materials: [Handle<StandardMaterial>; 5],
    honk_material: Handle<StandardMaterial>,
    calin_material: Handle<StandardMaterial>,
    ghost_material: Handle<StandardMaterial>,
    faint_ghost_material: Handle<StandardMaterial>,
    ghost_eye_material: Handle<StandardMaterial>,
}

impl ItemVisualAssets {
    /// Issue #185: the material of an explosion's flash (a pale sphere out to its reach), which
    /// the particles hide and replace with their own (`vfx`), when there is a renderer.
    pub fn flash_material(&self) -> &Handle<StandardMaterial> {
        &self.flash_material
    }

    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let translucent = |color: Color| StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            ..default()
        };
        let candy = StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.55,
            reflectance: 0.45,
            ..default()
        };
        Self {
            looks: Look::all()
                .map(|look| (look, LookMeshes::new(meshes, &model(look))))
                .collect(),
            model_material: materials.add(candy.clone()),
            glow_material: materials.add(StandardMaterial {
                emissive: LinearRgba::rgb(0.32, 0.22, 0.02),
                ..candy.clone()
            }),
            blink_material: materials.add(StandardMaterial {
                emissive: LinearRgba::rgb(1.6, 0.12, 0.08),
                ..candy
            }),
            wing_material: materials.add(StandardMaterial {
                double_sided: true,
                cull_mode: None,
                ..translucent(Color::srgba(1.0, 1.0, 1.0, 0.6))
            }),
            spark_material: materials.add(unlit(Color::WHITE)),
            cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            ball: meshes.add(Sphere::new(1.0)),
            ring: meshes.add(Torus::new(0.9, 1.0)),
            shell: meshes.add(Capsule3d::new(1.0, 1.6)),
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
            ghost_material: materials.add(StandardMaterial {
                unlit: true,
                ..translucent(Color::srgba(0.95, 0.97, 1.0, 0.4))
            }),
            faint_ghost_material: materials.add(StandardMaterial {
                unlit: true,
                ..translucent(Color::srgba(0.95, 0.97, 1.0, 0.07))
            }),
            ghost_eye_material: materials.add(unlit(Color::srgb(0.12, 0.12, 0.2))),
        }
    }

    /// The meshes of `look`.
    pub fn look(&self, look: Look) -> &LookMeshes {
        &self.looks[&look]
    }

    /// The material `look`'s body is drawn with: the models', glowing for the golden items.
    pub fn body_material(&self, look: Look) -> Handle<StandardMaterial> {
        match look {
            Look::Item(ItemKind::PimentDore | ItemKind::CroquetteDoree) => {
                self.glow_material.clone()
            }
            _ => self.model_material.clone(),
        }
    }

    /// `look`'s model as children of `parent`, at `scale` (its unit in meters): its body (the
    /// first child), its wings, its spark.
    fn spawn_model(&self, parent: &mut ChildSpawnerCommands, look: Look, scale: f32) {
        let meshes = self.look(look);
        parent.spawn((
            Mesh3d(meshes.body.clone()),
            MeshMaterial3d(self.body_material(look)),
            Transform::from_scale(Vec3::splat(scale)),
        ));
        if let Some((wing, hinge)) = &meshes.wing {
            for side in [-1.0f32, 1.0] {
                parent.spawn((
                    AbeilleWing { side },
                    Mesh3d(wing.clone()),
                    MeshMaterial3d(self.wing_material.clone()),
                    NotShadowCaster,
                    Transform::from_translation(*hinge * Vec3::new(side, 1.0, 1.0) * scale)
                        .with_scale(Vec3::new(side, 1.0, 1.0) * scale),
                ));
            }
        }
        if let Some((spark, at)) = &meshes.spark {
            parent.spawn((
                BombeSpark,
                Mesh3d(spark.clone()),
                MeshMaterial3d(self.spark_material.clone()),
                NotShadowCaster,
                Transform::from_translation(*at * scale).with_scale(Vec3::splat(scale)),
            ));
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
    /// Fusée à confettis: its rocket on the kart's back.
    Rocket,
    /// Pie voleuse (issue #21): a ghost in the kart's place (plain for its own player, faint for
    /// the others).
    Ghost,
}

/// The visual of an item effect around `kart` (issue #18), following its interpolated pose.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectLook {
    pub kart: Entity,
    pub kind: EffectKind,
}

/// The turning part of a box (its child; its lid is its child), how long it has been back, and
/// how long ago it was taken (issue #106: it bursts).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ItemBoxLook {
    pub phase: f32,
    pub shown_for: f32,
    pub gone_for: f32,
}

/// An abeille's wing (a child of its item): it beats.
#[derive(Component, Clone, Copy, Debug)]
pub struct AbeilleWing {
    pub side: f32,
}

/// The bombe's spark (a child of its item, issue #106): it crackles, and the bombe blinks.
#[derive(Component, Clone, Copy, Debug)]
pub struct BombeSpark;

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
                (
                    (place_items, animate_items).chain(),
                    beat_wings,
                    animate_blasts,
                    animate_effects,
                )
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

/// A new box of the track: its gift box and, on it, its lid with the bow.
fn dress_boxes(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    boxes: Query<(Entity, &ItemBoxSpot, &Transform), Added<ItemBoxSpot>>,
) {
    for (entity, spot, transform) in &boxes {
        let phase = transform.translation.x * 0.37 + transform.translation.z * 0.23;
        let look = if spot.double {
            Look::DoubleGiftBox
        } else {
            Look::GiftBox
        };
        let meshes = assets.look(look);
        let lid = meshes.lid.clone();
        commands.entity(entity).with_children(|item_box| {
            item_box
                .spawn((
                    ItemBoxLook {
                        phase,
                        shown_for: BOX_POP_IN,
                        gone_for: f32::INFINITY,
                    },
                    Mesh3d(meshes.body.clone()),
                    MeshMaterial3d(assets.model_material.clone()),
                    Transform::from_scale(Vec3::splat(BOX_SIZE)),
                    Visibility::default(),
                ))
                .with_children(|gift| {
                    if let Some((lid, at)) = lid {
                        gift.spawn((
                            Mesh3d(lid),
                            MeshMaterial3d(assets.model_material.clone()),
                            Transform::from_translation(at),
                        ));
                    }
                });
        });
    }
}

/// How a box looks `t` seconds into its bounce (`phase` its own): how high (m) and its scale
/// (squashed as it lands, a little wider).
pub fn box_bounce(t: f32, phase: f32) -> (f32, Vec3) {
    let s = (PI * (t * BOX_BOUNCE_HZ + phase)).sin().abs();
    let landing = (1.0 - s).powi(4);
    let squash = BOX_SQUASH * landing;
    (
        BOX_BOUNCE * s,
        Vec3::new(1.0 + squash * 0.5, 1.0 - squash, 1.0 + squash * 0.5),
    )
}

/// Boxes turn and bounce, their lids wobble as they land; a box taken bursts (it swells, its lid
/// pops off), is hidden, then pops back in; all hidden without items.
fn animate_boxes(
    time: Res<Time>,
    tuning: Res<ItemTuning>,
    assets: Res<ItemVisualAssets>,
    boxes: Query<(&ItemBox, &ItemBoxSpot)>,
    mut looks: Query<(
        &ChildOf,
        &mut ItemBoxLook,
        &mut Transform,
        &mut Visibility,
        Option<&Children>,
    )>,
    mut lids: Query<&mut Transform, Without<ItemBoxLook>>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (child_of, mut look, mut transform, mut visibility, children) in &mut looks {
        let Ok((item_box, spot)) = boxes.get(child_of.parent()) else {
            continue;
        };
        let lid_at = assets
            .look(if spot.double {
                Look::DoubleGiftBox
            } else {
                Look::GiftBox
            })
            .lid
            .as_ref()
            .map_or(Vec3::ZERO, |(_, at)| *at);
        let shown = tuning.enabled && item_box.available();
        if tuning.enabled && !shown && look.gone_for.is_infinite() && look.shown_for > 0.0 {
            // Just taken: it bursts.
            look.gone_for = 0.0;
        }
        let bursting = !shown && look.gone_for < BOX_BURST;
        visibility.set_if_neq(if shown || bursting {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let angle = now * BOX_SPIN + look.phase;
        if bursting {
            look.gone_for += dt;
            let t = (look.gone_for / BOX_BURST).min(1.0);
            transform.scale = Vec3::splat(BOX_SIZE * (1.0 + BOX_BURST_SWELL * t));
            transform.rotation = Quat::from_rotation_y(angle);
            for child in children.into_iter().flatten() {
                if let Ok(mut lid) = lids.get_mut(*child) {
                    lid.translation = lid_at + Vec3::Y * BOX_LID_JUMP * t;
                    lid.rotation = Quat::from_rotation_z(0.9 * t) * Quat::from_rotation_x(0.5 * t);
                }
            }
            continue;
        }
        if !shown {
            look.shown_for = 0.0;
            look.gone_for = f32::INFINITY;
            continue;
        }
        look.gone_for = f32::INFINITY;
        look.shown_for += dt;
        let t = (look.shown_for / BOX_POP_IN).min(1.0);
        // Grows with a little overshoot.
        let pop = BOX_SIZE * (t + 0.25 * (PI * t).sin());
        let (height, squash) = box_bounce(now, look.phase);
        transform.rotation = Quat::from_rotation_y(angle);
        transform.translation.y = height;
        transform.scale = squash * pop.max(0.01);
        // The lid wobbles as the box lands, like jelly.
        let wobble = (1.0 - squash.y) / BOX_SQUASH;
        for child in children.into_iter().flatten() {
            if let Ok(mut lid) = lids.get_mut(*child) {
                lid.translation = lid_at;
                lid.rotation = Quat::from_rotation_z(0.12 * wobble * (now * 9.0).sin())
                    * Quat::from_rotation_x(0.08 * wobble * (now * 7.0).cos());
            }
        }
    }
}

/// A new item out: its model.
fn dress_items(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    tuning: Res<ItemTuning>,
    items: Query<(Entity, &ItemObject), Added<ItemObject>>,
) {
    for (entity, item) in &items {
        let radius = item.radius(&tuning);
        commands.entity(entity).with_children(|parts| {
            assets.spawn_model(parts, Look::Item(item.item), radius);
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

/// A kart in a super câlin glows pink, a kart in a fusée wears its rocket (issue #18): shown
/// while it lasts, around the kart's interpolated pose.
fn show_effects(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    karts: Query<(Entity, &ItemEffects, Has<PlayerKart>), Changed<ItemEffects>>,
    looks: Query<(Entity, &EffectLook)>,
    all: Query<&ItemEffects>,
) {
    for (look_entity, look) in &looks {
        let on = all.get(look.kart).is_ok_and(|effects| match look.kind {
            EffectKind::Calin => effects.calin > 0.0,
            EffectKind::Rocket => effects.rocket > 0.0,
            EffectKind::Ghost => effects.hidden(),
        });
        if !on {
            commands.entity(look_entity).try_despawn();
        }
    }
    for (kart, effects, player) in &karts {
        for (kind, on) in [
            (EffectKind::Calin, effects.calin > 0.0),
            (EffectKind::Rocket, effects.rocket > 0.0),
            (EffectKind::Ghost, effects.hidden()),
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
                    // Issue #106: the fusée's rocket strapped to the kart's back, its nose
                    // forward (−Z), its nozzle out behind.
                    parts.spawn((
                        Mesh3d(assets.look(Look::Item(ItemKind::Fusee)).body.clone()),
                        MeshMaterial3d(assets.model_material.clone()),
                        Transform::from_xyz(0.0, 0.62, 0.75)
                            .with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                            .with_scale(Vec3::splat(0.68)),
                    ));
                }
                EffectKind::Ghost => {
                    // Issue #21: a sheet ghost the size of the kart; its own player sees it
                    // plainly (with its eyes), the others barely.
                    let material = if player {
                        assets.ghost_material.clone()
                    } else {
                        assets.faint_ghost_material.clone()
                    };
                    parts.spawn((
                        Mesh3d(assets.shell.clone()),
                        MeshMaterial3d(material),
                        Transform::from_xyz(0.0, 0.2, 0.0).with_scale(Vec3::new(0.75, 0.55, 0.75)),
                        NotShadowCaster,
                    ));
                    if player {
                        for x in [-0.25f32, 0.25] {
                            parts.spawn((
                                Mesh3d(assets.ball.clone()),
                                MeshMaterial3d(assets.ghost_eye_material.clone()),
                                Transform::from_xyz(x, 0.85, -0.62).with_scale(Vec3::splat(0.12)),
                                NotShadowCaster,
                            ));
                        }
                    }
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
/// where it flies (its model's front, +Z), a pelure turns slowly on the ground. Issue #96: over
/// the road of an anti-gravity zone, the same in the road's frame (its normal as the item's up: a
/// pelure lies flat on a wall, a boomerang-os spins flat over a ceiling); only on a circuit with a
/// zone.
fn place_items(
    fixed_time: Res<Time<Fixed>>,
    tuning: Res<ItemTuning>,
    course: Option<Res<Course>>,
    mut items: Query<(&ItemObject, &mut Transform)>,
) {
    let alpha = fixed_time.overstep_fraction();
    let zoned = course.as_deref().filter(|c| !c.anti_gravity.is_empty());
    for (item, mut transform) in &mut items {
        transform.translation = item.previous.lerp(item.position, alpha);
        let up = zoned.map_or(Vec3::Y, |course| course.up_at(transform.translation));
        let to_world = if up == Vec3::Y {
            None
        } else {
            Some(Quat::from_rotation_arc(Vec3::Y, up))
        };
        let velocity = match to_world {
            Some(turn) => turn.inverse() * item.velocity,
            None => item.velocity,
        };
        let flat = velocity.with_y(0.0);
        let rotation = match item.item {
            ItemKind::Pelote if flat.length() > 0.1 => {
                // Rolled distance over the radius: its turn around the axis across its path.
                let axis = flat.normalize().cross(Vec3::Y);
                let angle = -item.age * flat.length() / item.radius(&tuning).max(0.05);
                Quat::from_axis_angle(axis, angle % TAU)
            }
            ItemKind::Abeille if velocity.length() > 0.1 => {
                // Its front (+Z) where it flies.
                Transform::default().looking_to(-velocity, Vec3::Y).rotation
            }
            // Issue #21: the boomerang-os spins flat as it flies.
            ItemKind::Boomerang => Quat::from_rotation_y(item.age * BOOMERANG_SPIN),
            // The oreiller géant flies flat, its long side across its path.
            ItemKind::OreillerGeant if flat.length() > 0.1 => {
                Transform::default().looking_to(flat, Vec3::Y).rotation
                    * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
            }
            _ => Quat::from_rotation_y(item.age * 0.5),
        };
        transform.rotation = match to_world {
            Some(turn) => turn * rotation,
            None => rotation,
        };
    }
}

/// How lit a bombe is `age` seconds after it came out (issue #106): it blinks, faster and faster
/// to the end of its fuse (`fuse` seconds).
pub fn bombe_lit(age: f32, fuse: f32) -> bool {
    let gain = BLINK_HZ_GAIN / fuse.max(0.1);
    // The blinks so far: the integral of a frequency growing from BLINK_HZ.
    let blinks = BLINK_HZ * age + 0.5 * gain * age * age;
    blinks.fract() < BLINK_LIT
}

/// The Cac's `age` s after it came out (issue #137): its body's offset (in its radius) and scale.
/// It wobbles like jelly (the pelure's motion of #106: the same tremble, then a little, for
/// good); `dropped` on the ground, it falls the last bit, stretched, squashes as it lands and
/// springs back. Its bottom stays on its ground (at −0.5) whatever its scale; held out behind a
/// kart, it only wobbles.
pub fn cacs_pose(age: f32, dropped: bool) -> (Vec3, Vec3) {
    let t = age.max(0.0);
    let jelly = (t * CACS_JELLY_SPEED).sin() * (-t * CACS_JELLY_CALM).exp().max(CACS_JELLY_REST);
    let (mut wide, mut high, mut lift) = (CACS_JELLY.x * jelly, -CACS_JELLY.y * jelly, 0.0);
    if dropped {
        if t < CACS_FALL {
            let x = t / CACS_FALL;
            lift = CACS_DROP * (1.0 - x * x);
            high += CACS_STRETCH;
            wide -= CACS_STRETCH * 0.5;
        } else {
            let since = t - CACS_FALL;
            let squash = CACS_SQUASH * (-since * CACS_DAMPING).exp() * (since * CACS_SPRING).cos();
            high -= squash;
            wide += squash * 0.5;
        }
    }
    let scale = Vec3::new(1.0 + wide, 1.0 + high, 1.0 + wide);
    // Scaled around the item's position, half its radius over its ground: moved down (or up) to
    // keep its bottom there.
    (Vec3::Y * (lift - 0.5 * (1.0 - scale.y)), scale)
}

/// The items' own motions (issue #106), on their model's parts: the Cac's wobbles like jelly,
/// the oreiller géant floats and rocks, the super câlin's heart beats, the bombe blinks and its
/// spark crackles, the abeille bobs as it flies.
fn animate_items(
    time: Res<Time>,
    tuning: Res<ItemTuning>,
    assets: Res<ItemVisualAssets>,
    items: Query<(&ItemObject, &Children)>,
    mut parts: Query<
        (
            &mut Transform,
            Option<&mut MeshMaterial3d<StandardMaterial>>,
            Has<BombeSpark>,
        ),
        Without<AbeilleWing>,
    >,
) {
    let now = time.elapsed_secs();
    for (item, children) in &items {
        let radius = item.radius(&tuning);
        let t = item.age;
        // The body's motion: its offset (in its radius), its scale and its turn.
        let (offset, scale, turn) = match item.item {
            ItemKind::Pelure => {
                let (offset, scale) = cacs_pose(t, item.motion == Motion::Ground);
                (offset, scale, Quat::IDENTITY)
            }
            ItemKind::OreillerGeant => (
                Vec3::Y * PILLOW_FLOAT * (TAU * PILLOW_FLOAT_HZ * now).sin(),
                Vec3::ONE,
                Quat::from_rotation_z(0.08 * (TAU * 0.7 * now).sin())
                    * Quat::from_rotation_x(0.06 * (TAU * 0.55 * now).cos()),
            ),
            ItemKind::SuperCalin => {
                let beat = (TAU * 1.4 * now).sin().max(0.0).powi(6);
                (Vec3::ZERO, Vec3::splat(1.0 + 0.16 * beat), Quat::IDENTITY)
            }
            ItemKind::Abeille => (
                Vec3::Y * 0.06 * (now * 18.0).sin(),
                Vec3::ONE,
                Quat::IDENTITY,
            ),
            ItemKind::Chantilly => {
                let w = (now * 5.0).sin();
                (
                    Vec3::ZERO,
                    Vec3::new(1.0 + 0.04 * w, 1.0 - 0.06 * w, 1.0 + 0.04 * w),
                    Quat::IDENTITY,
                )
            }
            _ => (Vec3::ZERO, Vec3::ONE, Quat::IDENTITY),
        };
        let blinking = item.item == ItemKind::Bombe
            && !matches!(item.motion, Motion::Held(_))
            && bombe_lit(t, tuning.bombe_fuse);
        let spark_at = assets
            .look(Look::Item(item.item))
            .spark
            .as_ref()
            .map_or(Vec3::ZERO, |(_, at)| *at);
        for (k, child) in children.iter().enumerate() {
            let Ok((mut transform, material, spark)) = parts.get_mut(child) else {
                continue;
            };
            if spark {
                let crackle = 1.0 + 0.35 * (now * 47.0).sin() * (now * 31.0).cos();
                let size = if blinking { 1.7 } else { crackle };
                transform.translation = turn * (spark_at + offset) * radius;
                transform.scale = Vec3::splat(radius * size);
            } else if k == 0 {
                // The body.
                transform.translation = offset * radius;
                transform.rotation = turn;
                transform.scale = scale * radius;
                if item.item == ItemKind::Bombe
                    && let Some(mut material) = material
                {
                    let wanted = if blinking {
                        &assets.blink_material
                    } else {
                        &assets.model_material
                    };
                    if material.0 != *wanted {
                        material.0 = wanted.clone();
                    }
                }
            }
        }
    }
}

/// The abeilles' wings beat (an item's, a used one's, the showcase's).
fn beat_wings(time: Res<Time>, mut wings: Query<(&AbeilleWing, &mut Transform)>) {
    let beat = (time.elapsed_secs() * WING_BEAT).sin() * WING_SWING;
    for (wing, mut transform) in &mut wings {
        // The left one is mirrored (its scale): turned the other way, both rise together.
        transform.rotation = Quat::from_rotation_z(wing.side * beat);
    }
}

// --- With a renderer ----------------------------------------------------------------------------

/// The items' models with a renderer (issue #106): their files, an item used popping up over its
/// kart, the admin panel's showcase. Added by `GamePlugin` only on screen: the headless game
/// keeps the models built in code, and the bot races their world.
pub struct ItemModelPlugin;

impl Plugin for ItemModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemShowcase>()
            .add_systems(Startup, request_model_files)
            .add_systems(
                Update,
                (
                    take_model_files
                        .after(ModelSystems)
                        .run_if(resource_exists::<ItemVisualAssets>),
                    show_flourishes,
                    animate_flourishes,
                    show_showcase,
                    spin_showcase,
                ),
            );
    }
}

/// Asks for the models' files (kept for the whole game), if the game can load them.
fn request_model_files(
    models: Option<ResMut<Models>>,
    server: Res<AssetServer>,
    gltfs: Option<Res<Assets<bevy::gltf::Gltf>>>,
) {
    if let Some(mut models) = models {
        let server = gltfs.map(|_| &*server);
        for (_, id) in MODEL_FILES {
            models.request(id, Keep::Always, server);
        }
    }
}

/// Once the files are baked (or missing), each one's model takes the place of the built one: in
/// its meshes, and in its icon (a file that is missing leaves the built model, said once by
/// `models`). Once.
fn take_model_files(
    models: Option<Res<Models>>,
    mut assets: ResMut<ItemVisualAssets>,
    icons: Option<Res<ItemIcons>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(models) = models else {
        *done = true;
        return;
    };
    if !MODEL_FILES.iter().all(|(_, id)| models.settled(id)) {
        return;
    }
    *done = true;
    for (look, id) in MODEL_FILES {
        let Some(made) = models
            .get(id)
            .and_then(|baked| from_file(look, baked, &model(look)))
        else {
            continue;
        };
        if let Some(meshes_of) = assets.looks.get_mut(&look) {
            meshes_of.replace(&mut meshes, &made);
        }
        if let (Look::Item(item), Some(icons), Some(images)) = (look, &icons, images.as_mut()) {
            icons.replace(images, item, &made);
        }
    }
}

/// An item used, popping up over its kart (issue #106): its age (s).
#[derive(Component, Clone, Copy, Debug)]
pub struct Flourish {
    pub age: f32,
}

/// The items shown over their kart when used (they act at once: nothing of them on the track).
fn flourishes(item: ItemKind) -> bool {
    matches!(
        item,
        ItemKind::Piment
            | ItemKind::PimentDore
            | ItemKind::CroquetteDoree
            | ItemKind::Chantilly
            | ItemKind::SuperCalin
            | ItemKind::Fusee
            | ItemKind::Degonflage
            | ItemKind::KlaxonCanard
            | ItemKind::PieVoleuse
            | ItemKind::Pinata
    )
}

/// What a kart held last frame: its items used so far, the item ready in its hand, its piñata.
#[derive(Clone, Copy, Debug, Default)]
struct Held {
    used: u32,
    hand: Option<ItemKind>,
    pinata: u8,
}

/// Each item a kart uses that acts at once pops up over it (issue #106): its model grows, turns
/// and rises, then shrinks away. Each press of a piment doré, each piment of a « x3 »; the
/// piñata on its first press only (its items then show around the kart).
fn show_flourishes(
    mut commands: Commands,
    assets: Res<ItemVisualAssets>,
    karts: Query<(Entity, &ItemStats, &super::ItemSlots)>,
    mut held: Local<HashMap<Entity, Held>>,
) {
    for (kart, stats, slots) in &karts {
        let now = Held {
            used: stats.used,
            hand: match slots.hand {
                super::Slot::Ready { item, .. } => Some(item),
                _ => None,
            },
            pinata: slots.pinata,
        };
        let Some(before) = held.insert(kart, now) else {
            continue;
        };
        let Some(item) = before.hand.filter(|item| flourishes(*item)) else {
            continue;
        };
        if now.used <= before.used || (item == ItemKind::Pinata && before.pinata != 0) {
            continue;
        }
        commands
            .spawn((
                Name::new("Item used"),
                Flourish { age: 0.0 },
                InterpolatedFrom(kart),
                LevelScoped,
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent
                    .spawn((
                        Transform::from_scale(Vec3::ZERO),
                        Visibility::default(),
                        NotShadowCaster,
                    ))
                    .with_children(|holder| {
                        assets.spawn_model(holder, Look::Item(item), FLOURISH_SIZE / 2.0);
                    });
            });
    }
    if held.len() > 64 {
        held.retain(|kart, _| karts.contains(*kart));
    }
}

/// How an item used looks `t` seconds after it popped up: its scale (it grows with an
/// overshoot, stays, then shrinks away) and how high it is over its kart.
pub fn flourish_pose(t: f32) -> (f32, f32) {
    let x = (t / FLOURISH_TIME).clamp(0.0, 1.0);
    // Eased out with an overshoot (about 15 %) over its first 30 %, gone over its last 30 %.
    let grow = (x / 0.3).min(1.0) - 1.0;
    let overshoot = 2.6;
    let pop = 1.0 + (overshoot + 1.0) * grow.powi(3) + overshoot * grow.powi(2);
    let shrink = ((1.0 - x) / 0.3).min(1.0);
    (pop * shrink, FLOURISH_HEIGHT + 0.5 * x * x)
}

fn animate_flourishes(
    mut commands: Commands,
    time: Res<Time>,
    mut flourishes: Query<(Entity, &mut Flourish, &Children)>,
    mut holders: Query<&mut Transform, Without<Flourish>>,
) {
    let dt = time.delta_secs();
    for (entity, mut flourish, children) in &mut flourishes {
        flourish.age += dt;
        if flourish.age >= FLOURISH_TIME {
            commands.entity(entity).try_despawn();
            continue;
        }
        let (scale, height) = flourish_pose(flourish.age);
        for child in children.iter() {
            if let Ok(mut holder) = holders.get_mut(child) {
                holder.translation = Vec3::Y * height;
                holder.rotation = Quat::from_rotation_y(flourish.age * 7.0);
                holder.scale = Vec3::splat(scale.max(1e-3));
            }
        }
    }
}

/// The admin panel's showcase (issue #106): every model side by side in front of the camera, to
/// look at them (`open`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemShowcase {
    pub open: bool,
}

/// A model of the showcase: its place in the row (its spin).
#[derive(Component, Clone, Copy, Debug)]
pub struct ShowcaseItem {
    pub index: usize,
    pub look: Look,
}

/// The showcase's root.
#[derive(Component, Clone, Copy, Debug)]
struct Showcase;

/// The showcase opened: every model (the sixteen items, the croquette, the two boxes) in rows in
/// front of the camera, facing it; closed, gone.
fn show_showcase(
    mut commands: Commands,
    showcase: Res<ItemShowcase>,
    assets: Res<ItemVisualAssets>,
    cameras: Query<(&GlobalTransform, &ChaseCamera)>,
    targets: Query<&GlobalTransform>,
    shown: Query<Entity, With<Showcase>>,
) {
    if !showcase.is_changed() {
        return;
    }
    for entity in &shown {
        commands.entity(entity).try_despawn();
    }
    if !showcase.open {
        return;
    }
    let Some((camera, chase)) = cameras.iter().next() else {
        return;
    };
    let camera = camera.compute_transform();
    let looks: Vec<Look> = Look::all().collect();
    let rows = looks.len().div_ceil(SHOWCASE_COLUMNS);
    // Standing upright ahead of the camera, facing it, its lowest row over the kart it follows.
    let forward = camera
        .forward()
        .as_vec3()
        .with_y(0.0)
        .normalize_or(Vec3::NEG_Z);
    let ground = targets
        .get(chase.target)
        .map_or(camera.translation.y - 2.5, |kart| kart.translation().y);
    let height = ground + SHOWCASE_LIFT + (rows - 1) as f32 / 2.0 * SHOWCASE_STEP.y;
    let center = (camera.translation + forward * SHOWCASE_DISTANCE).with_y(height);
    let root = Transform::from_translation(center).looking_to(forward, Vec3::Y);
    commands
        .spawn((
            Name::new("Showcase"),
            Showcase,
            LevelScoped,
            root,
            Visibility::default(),
        ))
        .with_children(|row| {
            for (index, look) in looks.into_iter().enumerate() {
                let (column, line) = (index % SHOWCASE_COLUMNS, index / SHOWCASE_COLUMNS);
                let x = (column as f32 - (SHOWCASE_COLUMNS - 1) as f32 / 2.0) * SHOWCASE_STEP.x;
                let y = ((rows - 1) as f32 / 2.0 - line as f32) * SHOWCASE_STEP.y;
                // The models' unit: an item's radius (about half of it across), or meters.
                let scale = match look {
                    Look::Item(_) => SHOWCASE_SIZE * 0.4,
                    Look::Croquette => SHOWCASE_SIZE * 1.4,
                    Look::GiftBox | Look::DoubleGiftBox => SHOWCASE_SIZE * 0.8,
                };
                row.spawn((
                    ShowcaseItem { index, look },
                    // Facing the camera: the root looks along `forward` (its −Z), the models'
                    // front (+Z) toward the camera.
                    Transform::from_xyz(x, y, 0.0),
                    Visibility::default(),
                ))
                .with_children(|holder| {
                    assets.spawn_model(holder, look, scale);
                    if let Some((lid, at)) = assets.look(look).lid.clone() {
                        holder.spawn((
                            Mesh3d(lid),
                            MeshMaterial3d(assets.model_material.clone()),
                            Transform::from_translation(at * scale).with_scale(Vec3::splat(scale)),
                        ));
                    }
                });
            }
        });
}

/// The showcase's models swing slowly, each a little behind the one before.
fn spin_showcase(time: Res<Time>, mut items: Query<(&ShowcaseItem, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (item, mut transform) in &mut items {
        // Seen from its icon's side (its best one), swinging a little.
        let mut view = icon_view(item.look);
        view.yaw += 0.5 * (now * 0.8 + item.index as f32 * 0.4).sin();
        transform.rotation = view.rotation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box bounces softly: up and down, never below its place, squashed only as it lands.
    #[test]
    fn a_box_bounces_softly() {
        let mut lowest_squash = f32::MAX;
        for k in 0..200 {
            let t = k as f32 / 100.0;
            let (height, scale) = box_bounce(t, 0.3);
            assert!((0.0..=BOX_BOUNCE + 1e-5).contains(&height));
            assert!(scale.y <= 1.0 && scale.x >= 1.0);
            if height > BOX_BOUNCE * 0.6 {
                assert!(scale.y > 0.995, "squashed in the air at {t}");
            }
            lowest_squash = lowest_squash.min(scale.y);
        }
        assert!((lowest_squash - (1.0 - BOX_SQUASH)).abs() < 0.01);
    }

    /// The bombe blinks faster and faster to the end of its fuse.
    #[test]
    fn the_bombe_blinks_faster_and_faster() {
        let fuse = 3.0;
        let blinks = |from: f32, to: f32| {
            let mut count = 0;
            let mut was = bombe_lit(from, fuse);
            let mut t = from;
            while t < to {
                t += 0.002;
                let lit = bombe_lit(t, fuse);
                if lit && !was {
                    count += 1;
                }
                was = lit;
            }
            count
        };
        let (first, last) = (blinks(0.0, 1.0), blinks(2.0, 3.0));
        assert!((2..=4).contains(&first), "{first}");
        assert!(last > first * 2, "{first} then {last}");
    }

    /// An item used pops up with an overshoot, stays, then shrinks away as it rises.
    #[test]
    fn an_item_used_pops_up_and_away() {
        let (start, _) = flourish_pose(0.0);
        assert!(start.abs() < 1e-6);
        let peak = (1..60)
            .map(|k| flourish_pose(FLOURISH_TIME * k as f32 / 60.0).0)
            .fold(0.0f32, f32::max);
        assert!(peak > 1.1 && peak < 1.5, "{peak}");
        let (end, high) = flourish_pose(FLOURISH_TIME);
        assert!(end.abs() < 1e-6 && high > FLOURISH_HEIGHT);
    }

    /// Issue #137: the Cac's dropped falls its last bit, stretched, lands squashed, springs back
    /// and keeps wobbling a little like jelly; its bottom is never under its ground. Held out
    /// behind a kart, it only wobbles.
    #[test]
    fn the_cacs_falls_squashes_and_wobbles_like_jelly() {
        let bottom = |(offset, scale): (Vec3, Vec3)| offset.y - 0.5 * scale.y;
        let mut last_lift = f32::MAX;
        let mut flattest = f32::MAX;
        for k in 0..=300 {
            let t = k as f32 / 100.0;
            let pose = cacs_pose(t, true);
            let (_, scale) = pose;
            assert!(
                bottom(pose) >= -0.5 - 1e-5,
                "under its ground at {t}: {pose:?}"
            );
            assert!((scale.x - scale.z).abs() < 1e-6);
            if t < CACS_FALL {
                // Falling: above its ground, lower and lower, stretched.
                let lift = bottom(pose) + 0.5;
                assert!(lift > 0.0 && lift < last_lift, "{t}: {lift}");
                last_lift = lift;
                assert!(scale.y > 1.0 && scale.x < 1.0, "{t}: {scale}");
            } else {
                assert!((bottom(pose) + 0.5).abs() < 1e-5, "on its ground at {t}");
            }
            if (CACS_FALL..CACS_FALL + 0.1).contains(&t) {
                flattest = flattest.min(scale.y);
            }
            if (CACS_FALL..CACS_FALL + 0.05).contains(&t) {
                assert!(scale.x > 1.0, "wider as it lands: {t} {scale}");
            }
            if t > 2.4 {
                // Calmed down: it only trembles a little, for good.
                assert!((scale.y - 1.0).abs() <= CACS_JELLY.y * CACS_JELLY_REST + 1e-3);
            }
        }
        assert!(last_lift < CACS_DROP && last_lift > 0.0);
        assert!(
            (flattest - (1.0 - CACS_SQUASH)).abs() < 0.08,
            "squashed to {flattest}"
        );
        let trembles = (240..300)
            .map(|k| cacs_pose(k as f32 / 100.0, true).1.y)
            .fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        assert!(
            trembles.1 - trembles.0 > CACS_JELLY.y * CACS_JELLY_REST,
            "{trembles:?}"
        );
        // Held out behind its kart: no fall, no squash, the jelly only.
        for k in 0..=300 {
            let t = k as f32 / 100.0;
            let pose = cacs_pose(t, false);
            assert!((bottom(pose) + 0.5).abs() < 1e-5);
            assert!(
                (pose.1.y - 1.0).abs() <= CACS_JELLY.y + 1e-5,
                "{t}: {pose:?}"
            );
        }
    }

    /// Every look has its meshes, the files' looks too; the golden items glow.
    #[test]
    fn every_look_has_its_meshes() {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let assets = ItemVisualAssets::new(&mut meshes, &mut materials);
        for look in Look::all() {
            let look_meshes = assets.look(look);
            assert!(meshes.get(&look_meshes.body).is_some(), "{look:?}");
        }
        assert!(assets.look(Look::Item(ItemKind::Abeille)).wing.is_some());
        assert!(assets.look(Look::Item(ItemKind::Bombe)).spark.is_some());
        assert!(assets.look(Look::GiftBox).lid.is_some());
        assert_eq!(
            assets.body_material(Look::Item(ItemKind::PimentDore)),
            assets.glow_material
        );
        assert_eq!(
            assets.body_material(Look::Item(ItemKind::Pelure)),
            assets.model_material
        );
    }
}
