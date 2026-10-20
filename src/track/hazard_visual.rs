//! Issue #144: the look of the hazards of a circuit, **with a renderer only**
//! ([`HazardVisualPlugin`], added by `GamePlugin` with a renderer, by the tests by hand): the
//! headless world of the bot races has none of it.
//!
//! Every model is built once in primitives (`crate::models::shape`: soft shapes colored per
//! vertex, one mesh per kind, shared by all its instances, drawn with a single material), scaled
//! by its hazard's size:
//!
//! - a **rock** (sandstone), a fallen **trunk**, a **pillar** of layered rock;
//! - a **rolling log** (it turns as it rolls: its pose does), a **swinging stone** on its rope,
//!   hanging from a wooden gallows beside the road;
//! - the **bison**, a big round woolly creature with a dark mane, short horns and a pink nose; its
//!   four legs step as it walks, its body bobs;
//! - a **hay bale**, a **balloon** on its string, a **crate**: hidden while they are burst, they
//!   pop back in;
//! - issue #156: a shortcut's **panel**, a big sign board of weathered planks on two posts, a
//!   little askew (the discreet clue of the guide), hidden while it is broken; a **curtain of
//!   foliage**, tall grass blades and leafy tufts across a path.
//!
//! The moving ones follow their simulated pose between two ticks (`InterpolatedFrom`). The
//! bursts' straw, splinters and rubber are `crate::vfx`'s, their « pof » `crate::audio`'s.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use super::circuit::to_mesh;
use super::data::{BreakableKind, CoverKind, NpcKind, ObstacleKind};
use super::hazard::{HazardKind, ROPE_LENGTH, Shape};
use super::hazards::{Breakable, HazardBody};
use super::mesh::MeshData;
use super::{TrackLayout, TrackSample};
use crate::interpolation::InterpolatedFrom;
use crate::level::LevelScoped;
use crate::models::shape::{Detail, Ends, ellipsoid, mix, rounded_box, solid, srgb, tube};

/// A thing to drive through pops back in over this long (s).
const POP_IN: f32 = 0.35;
/// A bison's step: a stride this long (m), its legs swinging this far (rad).
const STRIDE: f32 = 1.6;
const LEG_SWING: f32 = 0.55;

/// The hazards' look (see the module's documentation).
pub struct HazardVisualPlugin;

impl Plugin for HazardVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_hazard_assets).add_systems(
            Update,
            (dress_hazards, walk_npcs, show_breakables)
                .chain()
                .run_if(resource_exists::<HazardAssets>),
        );
    }
}

/// The models' meshes (one per kind) and their material.
#[derive(Resource)]
pub struct HazardAssets {
    material: Handle<StandardMaterial>,
    meshes: Vec<(HazardKind, Handle<Mesh>)>,
    leg: Handle<Mesh>,
    rope: Handle<Mesh>,
    gallows: Handle<Mesh>,
}

impl HazardAssets {
    fn mesh(&self, kind: HazardKind) -> Handle<Mesh> {
        self.meshes
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, mesh)| mesh.clone())
            .unwrap_or_default()
    }
}

/// Every kind of hazard, for its model.
pub const ALL_KINDS: [HazardKind; 11] = [
    HazardKind::Obstacle(ObstacleKind::Rock),
    HazardKind::Obstacle(ObstacleKind::Trunk),
    HazardKind::Obstacle(ObstacleKind::Pillar),
    HazardKind::Obstacle(ObstacleKind::RollingLog),
    HazardKind::Obstacle(ObstacleKind::SwingingStone),
    HazardKind::Npc(NpcKind::Bison),
    HazardKind::Breakable(BreakableKind::HayBale),
    HazardKind::Breakable(BreakableKind::Balloon),
    HazardKind::Breakable(BreakableKind::Crate),
    HazardKind::Cover(CoverKind::Panel),
    HazardKind::Cover(CoverKind::Foliage),
];

fn create_hazard_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.8,
        ..default()
    });
    let mut add = |data: MeshData| meshes.add(to_mesh(&data));
    let built: Vec<(HazardKind, MeshData)> = ALL_KINDS.iter().map(|k| (*k, model(*k))).collect();
    let meshes_by_kind = built.into_iter().map(|(k, data)| (k, add(data))).collect();
    commands.insert_resource(HazardAssets {
        material,
        meshes: meshes_by_kind,
        leg: add(bison_leg()),
        rope: add(rope()),
        gallows: add(gallows()),
    });
}

/// The model of a hazard of `kind`, at size 1, in its entity's frame: a fixed obstacle and a
/// panel around its collider's center, a rolling log and a swinging stone around their center,
/// the others on their foot (local -Z down the road, Y up).
pub fn model(kind: HazardKind) -> MeshData {
    let mut mesh = MeshData::default();
    let shape = Shape::of(kind);
    let at = Mat4::from_translation;
    let detail = Detail::new(16, 10);
    match kind {
        HazardKind::Obstacle(ObstacleKind::Rock) => {
            let (light, dark) = (srgb(0.80, 0.52, 0.36), srgb(0.56, 0.33, 0.24));
            rounded_box(
                &mut mesh,
                Mat4::from_rotation_y(0.4),
                Vec3::new(shape.half_across, shape.height, shape.half_along),
                0.55,
                0.6,
                Detail::new(14, 10),
                |p| mix(dark, light, (p.y / shape.height + 1.0) / 2.0),
            );
        }
        HazardKind::Obstacle(ObstacleKind::Trunk | ObstacleKind::RollingLog) => {
            let (bark, wood) = if kind == HazardKind::Obstacle(ObstacleKind::Trunk) {
                (srgb(0.42, 0.28, 0.18), srgb(0.86, 0.70, 0.48))
            } else {
                (srgb(0.55, 0.36, 0.22), srgb(0.93, 0.78, 0.55))
            };
            let (half, radius) = (shape.half_length, shape.radius);
            // Its bark along Z, the pale wood of its ends.
            tube(
                &mut mesh,
                Mat4::IDENTITY,
                &[
                    (Vec3::new(0.0, 0.0, -half - 0.02), radius * 0.92),
                    (Vec3::new(0.0, 0.0, -half), radius),
                    (Vec3::new(0.0, 0.0, half), radius),
                    (Vec3::new(0.0, 0.0, half + 0.02), radius * 0.92),
                ],
                Ends::Open,
                14,
                1.0,
                Vec3::Y,
                // Stripes along it: the bark's ridges (and the roll shows).
                move |p| {
                    let ridge = ((p.x.atan2(p.y) * 3.0).sin() * 0.5 + 0.5) * 0.35;
                    mix(bark, srgb(0.25, 0.16, 0.10), ridge)
                },
            );
            for end in [-1.0, 1.0] {
                ellipsoid(
                    &mut mesh,
                    Mat4::from_translation(Vec3::Z * end * (half + 0.02))
                        * Mat4::from_rotation_x(FRAC_PI_2),
                    Vec3::new(radius * 0.92, 0.04, radius * 0.92),
                    Detail::new(14, 4),
                    solid(wood),
                );
            }
        }
        HazardKind::Obstacle(ObstacleKind::Pillar) => {
            // Layers of rock, a little narrower at the top.
            let layers = 5;
            let step = 2.0 * shape.half_length / layers as f32;
            for k in 0..layers {
                let y = -shape.half_length + step * (k as f32 + 0.5);
                let width = shape.radius * (1.05 - 0.06 * k as f32);
                let color = if k % 2 == 0 {
                    srgb(0.78, 0.48, 0.32)
                } else {
                    srgb(0.66, 0.40, 0.28)
                };
                rounded_box(
                    &mut mesh,
                    Mat4::from_translation(Vec3::Y * y) * Mat4::from_rotation_y(k as f32 * 0.7),
                    Vec3::new(width, step * 0.55, width),
                    0.35,
                    1.0,
                    detail,
                    solid(color),
                );
            }
        }
        HazardKind::Obstacle(ObstacleKind::SwingingStone) => {
            let (light, dark) = (srgb(0.62, 0.60, 0.58), srgb(0.40, 0.38, 0.37));
            ellipsoid(
                &mut mesh,
                Mat4::IDENTITY,
                Vec3::splat(shape.radius),
                Detail::new(16, 12),
                move |p| mix(dark, light, (p.y + 1.0) / 2.0),
            );
        }
        HazardKind::Npc(NpcKind::Bison) => bison(&mut mesh),
        HazardKind::Breakable(BreakableKind::HayBale) => {
            let (straw, band) = (srgb(0.95, 0.80, 0.38), srgb(0.70, 0.52, 0.22));
            rounded_box(
                &mut mesh,
                at(Vec3::Y * shape.height),
                Vec3::new(shape.half_across, shape.height, shape.half_along),
                0.3,
                0.3,
                detail,
                move |p| {
                    // Two twine bands across it.
                    let twine = (p.x.abs() - 0.45).abs() < 0.06;
                    if twine { band } else { straw }
                },
            );
        }
        HazardKind::Breakable(BreakableKind::Balloon) => {
            let red = srgb(0.95, 0.30, 0.38);
            ellipsoid(
                &mut mesh,
                at(Vec3::Y * shape.height),
                Vec3::new(0.6, 0.72, 0.6),
                Detail::new(16, 12),
                move |p| mix(red, srgb(1.0, 0.72, 0.76), (p.y - 0.2).max(0.0) * 1.2),
            );
            // Its knot and its string down to the road.
            tube(
                &mut mesh,
                Mat4::IDENTITY,
                &[
                    (Vec3::Y * (shape.height - 0.72), 0.02),
                    (Vec3::new(0.08, 0.4, 0.0), 0.02),
                    (Vec3::ZERO, 0.02),
                ],
                Ends::Poles,
                4,
                1.0,
                Vec3::X,
                solid(srgb(0.95, 0.95, 0.92)),
            );
        }
        HazardKind::Breakable(BreakableKind::Crate) => {
            let (wood, plank) = (srgb(0.80, 0.58, 0.34), srgb(0.62, 0.42, 0.22));
            let half = shape.half_across;
            rounded_box(
                &mut mesh,
                at(Vec3::Y * shape.height),
                Vec3::splat(half),
                0.15,
                0.15,
                detail,
                move |p| {
                    // Its frame: darker along its edges.
                    let edge = [p.x, p.y, p.z]
                        .iter()
                        .filter(|v| v.abs() > half * 0.8)
                        .count();
                    if edge >= 2 { plank } else { wood }
                },
            );
        }
        HazardKind::Cover(CoverKind::Panel) => panel(&mut mesh),
        HazardKind::Cover(CoverKind::Foliage) => foliage(&mut mesh),
    }
    mesh
}

/// Issue #156: a shortcut's panel around its middle: a big board of weathered planks on two
/// posts (on the path: the board goes beyond them), across the path (along X), a little askew
/// (it leans 4°: the discreet clue).
fn panel(mesh: &mut MeshData) {
    const POSTS: f32 = 2.1;
    let half = Shape::cover_box(CoverKind::Panel);
    let lean = Mat4::from_rotation_z(4f32.to_radians());
    let (post, light, dark) = (
        srgb(0.42, 0.29, 0.19),
        srgb(0.78, 0.60, 0.40),
        srgb(0.62, 0.45, 0.29),
    );
    for side in [-1.0f32, 1.0] {
        rounded_box(
            mesh,
            lean * Mat4::from_translation(Vec3::new(side * POSTS, 0.0, 0.02)),
            Vec3::new(0.16, half.y, 0.16),
            0.2,
            0.4,
            Detail::new(8, 4),
            solid(post),
        );
    }
    // Five planks, every other one darker, the top one a little shorter.
    let planks = 5;
    let height = 2.0 * (half.y - 0.25) / planks as f32;
    for k in 0..planks {
        let y = -half.y + 0.45 + height * (k as f32 + 0.5);
        let width = half.x - if k == planks - 1 { 0.3 } else { 0.0 };
        let color = if k % 2 == 0 { light } else { dark };
        rounded_box(
            mesh,
            lean * Mat4::from_translation(Vec3::new(0.0, y, -0.06)),
            Vec3::new(width, height * 0.46, half.z * 0.6),
            0.15,
            0.15,
            Detail::new(8, 4),
            solid(color),
        );
    }
    // A cross brace behind the planks.
    tube(
        mesh,
        lean,
        &[
            (Vec3::new(-POSTS, -half.y + 0.6, 0.12), 0.09),
            (Vec3::new(POSTS, half.y - 0.4, 0.12), 0.09),
        ],
        Ends::Poles,
        6,
        1.0,
        Vec3::Z,
        solid(dark),
    );
}

/// Issue #156: a curtain of foliage on its foot: leafy tufts along its width and tall grass
/// blades between them, as high as a kart and more (it hides what is behind it).
fn foliage(mesh: &mut MeshData) {
    let half = Shape::cover_box(CoverKind::Foliage);
    let (leaf, light, straw) = (
        srgb(0.33, 0.47, 0.20),
        srgb(0.55, 0.66, 0.28),
        srgb(0.78, 0.70, 0.36),
    );
    let tufts = 7;
    for k in 0..tufts {
        let x = -half.x + 0.4 + (2.0 * half.x - 0.8) * k as f32 / (tufts - 1) as f32;
        let tall = 1.5 + 0.35 * ((k as f32 * 1.7).sin() + 1.0);
        ellipsoid(
            mesh,
            Mat4::from_translation(Vec3::new(x, tall * 0.6, 0.1 * (k % 2) as f32)),
            Vec3::new(0.75, tall * 0.65, half.z),
            Detail::new(12, 8),
            move |p| mix(leaf, light, (p.y + 1.0) / 2.0),
        );
    }
    let blades = 26;
    for k in 0..blades {
        let x = -half.x + 0.1 + (2.0 * half.x - 0.2) * k as f32 / (blades - 1) as f32;
        let tall = 2.3 + 0.9 * ((k as f32 * 2.3).sin() * 0.5 + 0.5);
        let lean = 0.35 * (k as f32 * 1.3).cos();
        let z = 0.35 * (k as f32 * 0.9).sin();
        let color = if k % 3 == 0 { straw } else { light };
        tube(
            mesh,
            Mat4::IDENTITY,
            &[
                (Vec3::new(x, 0.0, z), 0.12),
                (Vec3::new(x + lean * 0.4, tall * 0.6, z), 0.08),
                (Vec3::new(x + lean, tall, z + 0.1), 0.0),
            ],
            Ends::Poles,
            4,
            1.0,
            Vec3::Z,
            solid(color),
        );
    }
}

/// The bison without its legs (they move): a round woolly body, a dark mane on its hump, its
/// head low, short pale horns, a pink nose, small eyes; local -Z forward.
fn bison(mesh: &mut MeshData) {
    let (fur, mane, horn) = (
        srgb(0.72, 0.48, 0.28),
        srgb(0.38, 0.24, 0.15),
        srgb(0.96, 0.90, 0.76),
    );
    let at = Mat4::from_translation;
    ellipsoid(
        mesh,
        at(Vec3::new(0.0, 1.3, 0.15)),
        Vec3::new(0.95, 0.82, 1.2),
        Detail::new(18, 12),
        move |p| mix(fur, srgb(0.84, 0.62, 0.40), (p.y + 0.4).max(0.0)),
    );
    // The mane over its shoulders.
    ellipsoid(
        mesh,
        at(Vec3::new(0.0, 1.62, -0.55)),
        Vec3::new(0.9, 0.78, 0.75),
        Detail::new(18, 12),
        solid(mane),
    );
    // The head, low and forward, its nose.
    ellipsoid(
        mesh,
        at(Vec3::new(0.0, 1.2, -1.35)),
        Vec3::new(0.55, 0.5, 0.5),
        Detail::new(16, 10),
        solid(mane),
    );
    ellipsoid(
        mesh,
        at(Vec3::new(0.0, 1.02, -1.72)),
        Vec3::new(0.36, 0.26, 0.22),
        Detail::new(14, 8),
        solid(srgb(0.93, 0.62, 0.62)),
    );
    for side in [-1.0f32, 1.0] {
        // Short horns, curving up and out.
        tube(
            mesh,
            Mat4::IDENTITY,
            &[
                (Vec3::new(side * 0.42, 1.48, -1.35), 0.11),
                (Vec3::new(side * 0.68, 1.55, -1.38), 0.09),
                (Vec3::new(side * 0.78, 1.78, -1.40), 0.05),
                (Vec3::new(side * 0.76, 1.9, -1.42), 0.0),
            ],
            Ends::Poles,
            8,
            1.0,
            Vec3::Z,
            solid(horn),
        );
        // Its eyes: white, a black pupil.
        ellipsoid(
            mesh,
            at(Vec3::new(side * 0.3, 1.36, -1.72)),
            Vec3::splat(0.1),
            Detail::new(10, 6),
            solid(srgb(0.98, 0.98, 0.96)),
        );
        ellipsoid(
            mesh,
            at(Vec3::new(side * 0.32, 1.37, -1.8)),
            Vec3::splat(0.05),
            Detail::new(8, 5),
            solid(srgb(0.08, 0.06, 0.05)),
        );
    }
    // A tuft of a tail.
    ellipsoid(
        mesh,
        at(Vec3::new(0.0, 1.45, 1.38)),
        Vec3::new(0.16, 0.3, 0.16),
        Detail::new(10, 6),
        solid(mane),
    );
}

/// A bison's leg, hanging from its hip (its top at the origin).
fn bison_leg() -> MeshData {
    let mut mesh = MeshData::default();
    rounded_box(
        &mut mesh,
        Mat4::from_translation(Vec3::Y * -0.36),
        Vec3::new(0.2, 0.38, 0.2),
        0.6,
        0.8,
        Detail::new(10, 8),
        |p| {
            if p.y < -0.25 {
                srgb(0.25, 0.17, 0.12)
            } else {
                srgb(0.38, 0.24, 0.15)
            }
        },
    );
    mesh
}

/// A swinging stone's rope: from its top (the origin) up the rope's length.
fn rope() -> MeshData {
    let mut mesh = MeshData::default();
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[(Vec3::ZERO, 0.07), (Vec3::Y * ROPE_LENGTH, 0.07)],
        Ends::Open,
        6,
        1.0,
        Vec3::X,
        solid(srgb(0.55, 0.42, 0.28)),
    );
    mesh
}

/// A wooden gallows of unit size: a post up X = 0 to its top (1), its beam along +X to 1 (scaled
/// by the gallows' entity).
fn gallows() -> MeshData {
    let mut mesh = MeshData::default();
    let wood = srgb(0.50, 0.33, 0.20);
    rounded_box(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)),
        Vec3::new(0.02, 0.5, 0.02),
        0.2,
        0.4,
        Detail::new(8, 4),
        solid(wood),
    );
    rounded_box(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.5, 0.98, 0.0)),
        Vec3::new(0.52, 0.02, 0.02),
        0.2,
        0.4,
        Detail::new(8, 4),
        solid(wood),
    );
    mesh
}

/// A bison's leg (front or back, its side): it swings as the bison walks.
#[derive(Component, Clone, Copy, Debug)]
struct NpcLeg {
    /// Legs in step: the front left with the back right.
    phase: f32,
}

/// A passer-by's look: how far it has walked, where it was.
#[derive(Component, Clone, Copy, Debug, Default)]
struct NpcLook {
    walked: f32,
    last: Option<Vec3>,
}

/// A thing to drive through's look: how long it has been back.
#[derive(Component, Clone, Copy, Debug)]
struct BreakableLook {
    shown_for: f32,
}

/// Dresses each hazard as it appears: its model as a child (a fixed one, a thing to drive
/// through), or an entity of its own that follows its interpolated pose (a moving one).
fn dress_hazards(
    mut commands: Commands,
    assets: Res<HazardAssets>,
    layout: Option<Res<TrackLayout>>,
    hazards: Query<(Entity, &HazardBody), Added<HazardBody>>,
) {
    let Some(layout) = layout else {
        return;
    };
    for (entity, body) in &hazards {
        let Some(hazard) = layout.hazards.get(usize::from(body.0)) else {
            continue;
        };
        let model = (
            Mesh3d(assets.mesh(hazard.kind)),
            MeshMaterial3d(assets.material.clone()),
        );
        let scale = Transform::from_scale(Vec3::splat(hazard.size));
        if hazard.kind.moves() {
            let mut look = commands.spawn((
                LevelScoped,
                Name::new("Hazard look"),
                InterpolatedFrom(entity),
                Transform::default(),
                Visibility::default(),
            ));
            look.with_children(|look| {
                look.spawn((model, scale, Visibility::default()))
                    .with_children(|model| {
                        if hazard.kind == HazardKind::Obstacle(ObstacleKind::SwingingStone) {
                            // From the stone's top up to the pivot (in the model's units:
                            // it is scaled by the size).
                            let (top, size) = (Shape::of(hazard.kind).radius * 0.9, hazard.size);
                            let length = (ROPE_LENGTH - top * size) / (size * ROPE_LENGTH);
                            model.spawn((
                                Mesh3d(assets.rope.clone()),
                                MeshMaterial3d(assets.material.clone()),
                                Transform::from_translation(Vec3::Y * top).with_scale(Vec3::new(
                                    1.0 / size,
                                    length,
                                    1.0 / size,
                                )),
                            ));
                        }
                        if matches!(hazard.kind, HazardKind::Npc(_)) {
                            for (x, z, phase) in [
                                (-0.5, -0.7, 0.0),
                                (0.5, -0.7, PI),
                                (-0.5, 0.75, PI),
                                (0.5, 0.75, 0.0),
                            ] {
                                model.spawn((
                                    NpcLeg { phase },
                                    Mesh3d(assets.leg.clone()),
                                    MeshMaterial3d(assets.material.clone()),
                                    Transform::from_xyz(x, 0.8, z),
                                ));
                            }
                        }
                    });
            });
            if matches!(hazard.kind, HazardKind::Npc(_)) {
                look.insert(NpcLook::default());
            }
            if hazard.kind == HazardKind::Obstacle(ObstacleKind::SwingingStone) {
                spawn_gallows(
                    &mut commands,
                    &assets,
                    &hazard.road,
                    hazard.lateral,
                    hazard.pivot(),
                );
            }
        } else {
            commands
                .entity(entity)
                .insert(Visibility::default())
                .with_children(|parent| {
                    let mut child = parent.spawn((model, scale, Visibility::default()));
                    if hazard.kind.breaks() {
                        child.insert(BreakableLook { shown_for: POP_IN });
                    }
                });
        }
    }
}

/// The gallows of a swinging stone: its post beside the road on the stone's side, beyond the
/// shoulder, its beam over the pivot.
fn spawn_gallows(
    commands: &mut Commands,
    assets: &HazardAssets,
    road: &TrackSample,
    lateral: f32,
    pivot: Vec3,
) {
    let side = if lateral >= 0.0 { 1.0 } else { -1.0 };
    let out = road.half_width + road.shoulder + 1.0;
    let foot = road.point(side * out, 0.0);
    let height = (pivot - foot).dot(road.up) + 0.4;
    let reach = (side * out - lateral).abs() + 0.6;
    // Its beam along local +X: toward the pivot.
    let toward = (pivot - foot - road.up * (pivot - foot).dot(road.up)).normalize_or(road.right);
    let rotation = Transform::default()
        .looking_to(road.up.cross(toward), road.up)
        .rotation;
    commands.spawn((
        LevelScoped,
        Name::new("Gallows"),
        Mesh3d(assets.gallows.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(foot)
            .with_rotation(rotation)
            .with_scale(Vec3::new(reach, height, 10.0)),
        Visibility::default(),
    ));
}

/// The passers-by walk: their legs swing with the ground they cover, their body bobs.
fn walk_npcs(
    mut looks: Query<(&mut NpcLook, &Transform, &Children)>,
    mut models: Query<(&mut Transform, &Children), Without<NpcLook>>,
    mut legs: Query<(&NpcLeg, &mut Transform), (Without<NpcLook>, Without<Children>)>,
) {
    for (mut look, transform, children) in &mut looks {
        let moved = look
            .last
            .map_or(0.0, |last| last.distance(transform.translation));
        look.last = Some(transform.translation);
        // A jump (a restart, a new circuit) is no step.
        if moved < 1.0 {
            look.walked += moved;
        }
        let stride = TAU * look.walked / STRIDE;
        for child in children.iter() {
            let Ok((mut model, legs_of)) = models.get_mut(child) else {
                continue;
            };
            model.translation.y = 0.06 * (2.0 * stride).sin().abs();
            for leg in legs_of.iter() {
                if let Ok((leg_look, mut leg_transform)) = legs.get_mut(leg) {
                    leg_transform.rotation =
                        Quat::from_rotation_x(LEG_SWING * (stride + leg_look.phase).sin());
                }
            }
        }
    }
}

/// The things to drive through: hidden while burst, popping back in.
fn show_breakables(
    time: Res<Time>,
    breakables: Query<(&Breakable, &Children)>,
    mut looks: Query<(&mut BreakableLook, &mut Visibility, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (breakable, children) in &breakables {
        for child in children.iter() {
            let Ok((mut look, mut visibility, mut transform)) = looks.get_mut(child) else {
                continue;
            };
            if !breakable.available() {
                look.shown_for = 0.0;
                visibility.set_if_neq(Visibility::Hidden);
                continue;
            }
            visibility.set_if_neq(Visibility::Inherited);
            if look.shown_for < POP_IN {
                look.shown_for += dt;
                let t = (look.shown_for / POP_IN).min(1.0);
                // A soft overshoot as it pops back.
                let pop = 1.0 + 0.25 * (t * PI).sin();
                transform.scale = Vec3::splat(t * pop);
            } else if transform.scale != Vec3::ONE {
                transform.scale = Vec3::ONE;
            }
        }
    }
}
