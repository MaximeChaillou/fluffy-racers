//! Issue #105: the particles' systems, **only with a renderer** (`VfxPlugin`).
//!
//! - **Watching** (`FixedPostUpdate`, each tick): each kart's state against the last tick's
//!   (`cues::kart_bursts`), and the messages of the rules (the bumps between karts, the blasts,
//!   the croquettes). Nothing in the rules writes for the particles.
//! - **Throwing** (`PostUpdate`, each frame, where the karts are shown): what each kart throws
//!   all along (its drift's sparks and smoke, its boost's embers and trail, its dust off the
//!   road, the anti-gravity's sparks, the glider's wind) and its bursts of the ticks gone by;
//!   the bursts of the world; the speed lines and the field of view of the camera's kart.
//! - **Showing**: the pool moves on (`pool::ParticlePool::step`) and each of its slots' entity
//!   (made once, at startup) shows its particle; the stars over the head of a kart that sees them;
//!   the color of each kart's flames by its boost's source.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::cues::{
    BoostSource, Bursts, TickEvents, VfxSnapshot, ember_paint, fov_punch, kart_bursts, spark_style,
    tier_progress,
};
use super::emit::{self, KartFrame, take};
use super::pool::{Paint, ParticlePool, Shape};
use super::tuning::{VfxColors, VfxTuning, ground_dust};
use crate::camera::{CameraShake, ChaseCamera, FixedView};
use crate::croquettes::{CroquettePickedUp, CroquettesLost};
use crate::interactions::Paw;
use crate::interpolation::InterpolatedFrom;
use crate::items::{Blast, BoxBroken, ItemSlots, Slot};
use crate::kart::{
    Boost, BoostFlame, DeflatedScale, DriftMotion, ItemEffects, Kart, KartAssists, KartBodyPivot,
    KartBump, KartEnvironment, KartState, KartTuning, KartWheels, MiniTurbo, PlayerKart, Remote,
    Slipstream, Stall, StatEffects, StepVelocity, flame_size, hit_pose, slip_pose,
};
use crate::level::LoadLevel;
use crate::menu::MainMenu;
use crate::race::{Race, RaceProgress};
use crate::track::circuit::TrackHandle;
use crate::track::{Breakable, BreakableKind, HazardBody, TrackData, TrackLayout, water_depth};

/// What a kart throws all along: one carry each (`emit::take`).
const SPARKS: usize = 0;
const SMOKE: usize = 2;
const EMBERS: usize = 4;
const TRAIL: usize = 5;
const DUST: usize = 6;
const ANTI_GRAVITY: usize = 8;
const GLIDE: usize = 9;
/// Issue #130: the wake on the water.
const WAKE: usize = 10;
const EMITTERS: usize = 11;
/// The most stars over a head (`ImpactVfx::dizzy_stars`).
pub const MAX_DIZZY_STARS: usize = 5;
/// A kart's visual that moves farther than this in a frame jumped (a pelican, a restart): its
/// velocity is not measured on that frame (m).
const TELEPORT: f32 = 15.0;

/// The meshes and materials of every particle, shared (one per `Shape` and per `Paint`).
#[derive(Resource, Clone, Debug)]
pub struct VfxAssets {
    pub meshes: [Handle<Mesh>; 4],
    pub materials: Vec<Handle<StandardMaterial>>,
    /// The colors the materials show (they follow `VfxTuning::colors`)...
    colors: VfxColors,
    /// ...but the dust off the road on a circuit (issue #130): its biome's shoulders' and sand's
    /// (`Paint::Ground`, `Paint::Sand`; `tuning::ground_dust`). None off a circuit.
    pub ground: Option<[[f32; 3]; 2]>,
}

impl VfxAssets {
    /// The color the material of `paint` shows.
    pub fn color(&self, paint: Paint) -> [f32; 3] {
        match (paint, self.ground) {
            (Paint::Ground, Some([shoulder, _])) => shoulder,
            (Paint::Sand, Some([_, sand])) => sand,
            _ => self.colors.of(paint),
        }
    }

    pub fn mesh(&self, shape: Shape) -> &Handle<Mesh> {
        &self.meshes[shape.index()]
    }

    pub fn material(&self, paint: Paint) -> &Handle<StandardMaterial> {
        &self.materials[paint.index()]
    }
}

/// The pool, and the entity of each of its slots (made once, at startup), with what each shows.
#[derive(Resource, Debug)]
pub struct Particles {
    pub pool: ParticlePool,
    pub entities: Vec<Entity>,
    shown: Vec<Option<(Shape, Paint)>>,
}

/// On each slot's entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct VfxParticle(pub usize);

/// A star over the head of a kart that sees them (« sonné »), under its visual.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DizzyStar {
    pub body: Entity,
    pub index: usize,
}

/// What the particles remember of a kart (on its body; only with a renderer).
#[derive(Component, Clone, Debug, Default)]
pub struct KartVfx {
    /// Its last tick, and whether it was seen.
    pub last: VfxSnapshot,
    seen: bool,
    /// The bursts of the ticks since the last frame.
    pub pending: Bursts,
    /// Where its boost comes from (while it lasts).
    pub source: Option<BoostSource>,
    /// The punch of the field of view asked for by a boost that started (degrees; the camera's
    /// kart only).
    pub punch: f32,
    carry: [f32; EMITTERS],
    /// Where its visual was on the last frame, and how fast it goes (m/s).
    shown_at: Option<Vec3>,
    pub velocity: Vec3,
    /// How long it still sees stars (s), and their turn.
    pub dizzy: f32,
    spin: f32,
    /// Its stars, under its visual (`root`), once it first saw them.
    stars: Option<(Entity, [Entity; MAX_DIZZY_STARS])>,
}

/// The field of view's extra of the camera's kart (degrees): while it boosts, and the punch of a
/// boost that starts.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct FovKick {
    pub extra: f32,
    pub punch: f32,
    pub punch_age: f32,
    /// What it wrote last, on top of the base the camera had.
    written: f32,
    base: f32,
    lines: f32,
}

/// The bursts of the world (where karts bumped, where a blast went off, a croquette), until the
/// next frame throws them.
#[derive(Resource, Debug)]
pub struct VfxQueue(Vec<WorldBurst>);

impl VfxQueue {
    /// At most this many bursts wait for the next frame (a stalled frame drops the others).
    pub const MAX: usize = 256;
}

impl Default for VfxQueue {
    fn default() -> Self {
        Self(Vec::with_capacity(Self::MAX))
    }
}

/// What a tick saw of the karts (buffers kept from tick to tick, never grown in a race: 12 karts,
/// a few dozen bumps).
#[derive(Debug)]
pub(super) struct Ticked {
    croquettes: Vec<Entity>,
    bumped: Vec<(Entity, f32)>,
    pairs: Vec<(Entity, Entity)>,
}

impl Default for Ticked {
    fn default() -> Self {
        Self {
            croquettes: Vec::with_capacity(64),
            bumped: Vec::with_capacity(64),
            pairs: Vec::with_capacity(64),
        }
    }
}

/// A burst at a point of the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldBurst {
    /// Two karts bumped there, this hard (m/s), `normal` away from the other kart.
    Bump {
        at: Vec3,
        normal: Vec3,
        carrier: Vec3,
        speed: f32,
    },
    /// An explosion (GD § 8), this wide.
    Blast { at: Vec3, radius: f32 },
    /// A gift box taken (issue #106), by a kart going at `carrier`.
    Gift { at: Vec3, carrier: Vec3 },
    /// Issue #144: a thing to drive through burst (a hay bale, a balloon, a crate), by a kart
    /// going at `carrier`.
    Burst {
        at: Vec3,
        carrier: Vec3,
        kind: BreakableKind,
    },
    /// A croquette picked up, or `count` lost.
    Croquette {
        at: Vec3,
        carrier: Vec3,
        count: u32,
        lost: bool,
    },
}

pub(super) fn create_vfx(
    mut commands: Commands,
    tuning: Option<Res<VfxTuning>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let tuning = tuning.map(|t| t.clone()).unwrap_or_default();
    let assets = VfxAssets {
        meshes: [
            meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            meshes.add(Sphere::new(0.5).mesh().uv(14, 8)),
            meshes.add(star_mesh()),
            meshes.add(
                Torus::new(0.9, 1.0)
                    .mesh()
                    .minor_resolution(5)
                    .major_resolution(20)
                    .build(),
            ),
        ],
        materials: Paint::ALL
            .iter()
            .map(|paint| materials.add(material(*paint, tuning.colors.of(*paint))))
            .collect(),
        colors: tuning.colors.clone(),
        ground: None,
    };
    let entities: Vec<Entity> = (0..ParticlePool::SLOTS)
        .map(|slot| {
            commands
                .spawn((
                    Name::new("Particule"),
                    VfxParticle(slot),
                    Mesh3d(assets.mesh(Shape::Cube).clone()),
                    MeshMaterial3d(assets.material(Paint::Blue).clone()),
                    Transform::from_scale(Vec3::ZERO),
                    Visibility::Hidden,
                    NotShadowCaster,
                    NotShadowReceiver,
                ))
                .id()
        })
        .collect();
    commands.insert_resource(Particles {
        pool: ParticlePool::new(tuning.max_particles),
        shown: vec![None; entities.len()],
        entities,
    });
    commands.insert_resource(assets);
}

/// A particle's material: glowing (unlit) but the puffs and the cream, lit by the sun.
fn material(paint: Paint, [r, g, b]: [f32; 3]) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(r, g, b),
        unlit: !paint.lit(),
        perceptual_roughness: 0.95,
        ..default()
    }
}

/// A five-pointed star, 1 m across, a little thick (it faces the camera), lying in the XY plane.
pub fn star_mesh() -> Mesh {
    let half = 0.09;
    let rim: Vec<Vec2> = (0..10)
        .map(|k| {
            let angle = PI / 2.0 + k as f32 * TAU / 10.0;
            let radius = if k % 2 == 0 { 0.5 } else { 0.21 };
            Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    for (z, normal) in [(half, 1.0f32), (-half, -1.0)] {
        let center = positions.len() as u16;
        positions.push([0.0, 0.0, z]);
        normals.push([0.0, 0.0, normal]);
        for point in &rim {
            positions.push([point.x, point.y, z]);
            normals.push([0.0, 0.0, normal]);
        }
        for k in 0..10u16 {
            let (a, b) = (center + 1 + k, center + 1 + (k + 1) % 10);
            if normal > 0.0 {
                indices.extend([center, a, b]);
            } else {
                indices.extend([center, b, a]);
            }
        }
    }
    for k in 0..10 {
        let (a, b) = (rim[k], rim[(k + 1) % 10]);
        let side = (b - a).perp().normalize_or_zero() * -1.0;
        let start = positions.len() as u16;
        for (point, z) in [(a, half), (b, half), (b, -half), (a, -half)] {
            positions.push([point.x, point.y, z]);
            normals.push([side.x, side.y, 0.0]);
        }
        indices.extend([start, start + 2, start + 1, start, start + 3, start + 2]);
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; count])
    .with_inserted_indices(Indices::U16(indices))
}

/// Every kart gets its memory.
pub(super) fn remember_karts(
    mut commands: Commands,
    karts: Query<Entity, (Or<(With<Kart>, With<Remote>)>, Without<KartVfx>)>,
) {
    for kart in &karts {
        commands.entity(kart).try_insert(KartVfx::default());
    }
}

type KartView = (
    Entity,
    &'static mut KartVfx,
    (
        &'static KartState,
        &'static Boost,
        &'static Stall,
        &'static Slipstream,
        &'static KartWheels,
        &'static KartEnvironment,
        &'static ItemEffects,
    ),
    (
        Option<&'static StatEffects>,
        Option<&'static KartAssists>,
        Option<&'static ItemSlots>,
        Option<&'static Paw>,
    ),
    (Option<&'static Velocity>, Option<&'static StepVelocity>),
);

/// The drift tier of a kart (its Mini-turbo stat moves the thresholds), and its charge's progress
/// toward the next one.
fn drift_tier(
    kart_tuning: &KartTuning,
    state: &KartState,
    stats: Option<&StatEffects>,
    assisted: bool,
) -> (Option<MiniTurbo>, f32) {
    let KartState::Drifting { charge, .. } = *state else {
        return (None, 0.0);
    };
    let factor = stats.map_or(1.0, |s| s.mini_turbo_thresholds);
    let thresholds = kart_tuning.mini_turbo_thresholds.map(|t| t * factor);
    let highest = MiniTurbo::highest(assisted);
    let tier = MiniTurbo::ALL
        .into_iter()
        .rev()
        .filter(|tier| *tier <= highest)
        .find(|tier| charge >= thresholds[tier.index()]);
    (tier, tier_progress(thresholds, charge, tier))
}

/// What the particles see of a kart now.
fn snapshot(
    kart_tuning: &KartTuning,
    (state, boost, stall, slipstream, wheels, environment, effects): (
        &KartState,
        &Boost,
        &Stall,
        &Slipstream,
        &KartWheels,
        &KartEnvironment,
        &ItemEffects,
    ),
    (stats, assists, slots, paw): (
        Option<&StatEffects>,
        Option<&KartAssists>,
        Option<&ItemSlots>,
        Option<&Paw>,
    ),
    (velocity, step): (Option<&Velocity>, Option<&StepVelocity>),
) -> VfxSnapshot {
    let assisted = assists.is_some_and(|a| a.assisted_steering);
    let (tier, _) = drift_tier(kart_tuning, state, stats, assisted);
    // The shock of the physics step: what it took from the speed, across the kart's up.
    let (impact, impact_dir) = match (velocity, step) {
        (Some(velocity), Some(step)) => {
            let up = environment.up;
            let lost = step.linear - velocity.linear;
            let across = lost - up * lost.dot(up);
            (across.length(), across.normalize_or_zero())
        }
        _ => (0.0, Vec3::ZERO),
    };
    VfxSnapshot {
        state: *state,
        tier,
        boost: boost.remaining,
        stalled: stall.is_active(),
        slip_charge: slipstream.charge,
        slip_charging: slipstream.charging,
        airborne_ticks: wheels.ticks_without_ground,
        gliding: environment.gliding,
        anti_gravity: environment.anti_gravity,
        hand: slots.and_then(|s| match s.hand {
            Slot::Ready { item, .. } => Some(item),
            _ => None,
        }),
        golden: slots.is_some_and(|s| s.golden > 0),
        pinata: slots.map_or(0, |s| s.pinata),
        creamed: effects.creamed > 0.0,
        impact,
        impact_dir,
        struck: paw.map_or(0, |p| p.struck_count),
        underwater: environment.underwater,
    }
}

/// Each tick: each kart's bursts (`cues::kart_bursts`), kept until the next frame throws them.
pub(super) fn watch_karts(
    time: Res<Time>,
    tuning: Res<VfxTuning>,
    kart_tuning: Res<KartTuning>,
    race: Option<Res<Race>>,
    (mut picked, mut bumps): (MessageReader<CroquettePickedUp>, MessageReader<KartBump>),
    mut karts: Query<KartView>,
    mut ticked: Local<Ticked>,
) {
    let dt = time.delta_secs().max(1e-4);
    let Ticked {
        croquettes, bumped, ..
    } = &mut *ticked;
    croquettes.clear();
    bumped.clear();
    croquettes.extend(picked.read().map(|p| p.kart));
    for bump in bumps.read() {
        for kart in [bump.kart, bump.by] {
            match bumped.iter_mut().find(|(k, _)| *k == kart) {
                Some((_, speed)) => *speed = speed.max(bump.closing_speed),
                None => bumped.push((kart, bump.closing_speed)),
            }
        }
    }
    let go = race.as_ref().is_some_and(|race| race.race_ticks() == 1);
    for (kart, mut memory, rules, items, physics) in &mut karts {
        let now = snapshot(&kart_tuning, rules, items, physics);
        let last = std::mem::replace(&mut memory.last, now);
        if !std::mem::replace(&mut memory.seen, true) {
            continue;
        }
        let events = TickEvents {
            go,
            croquette: croquettes.contains(&kart),
            bump: bumped
                .iter()
                .find(|(k, _)| *k == kart)
                .map_or(0.0, |(_, speed)| *speed),
        };
        let bursts = kart_bursts(&last, &now, events, &tuning, dt);
        match bursts.boost {
            // A croquette's mini-boost does not take over a boost already running (its flames
            // stay the color of the stronger one).
            Some(BoostSource::Croquette) if last.boost > 0.0 => {}
            Some(source) => memory.source = Some(source),
            None if now.boost <= 0.0 => memory.source = None,
            None => {}
        }
        memory.pending.merge(bursts);
    }
}

/// Each tick: the bursts of the world (where two karts bumped, the blasts, the croquettes).
pub(super) fn watch_world(
    mut queue: ResMut<VfxQueue>,
    (mut bumps, mut blasts, mut picked, mut lost, mut boxes): (
        MessageReader<KartBump>,
        MessageReader<Blast>,
        MessageReader<CroquettePickedUp>,
        MessageReader<CroquettesLost>,
        MessageReader<BoxBroken>,
    ),
    karts: Query<(&Transform, Option<&KartVfx>), Or<(With<Kart>, With<Remote>)>>,
    spots: Query<&GlobalTransform>,
    mut ticked: Local<Ticked>,
    // Issue #144: the things to drive through of the circuit (none on most circuits).
    (breakables, layout): (
        Query<(&HazardBody, &Transform, &Breakable)>,
        Option<Res<TrackLayout>>,
    ),
) {
    let pairs = &mut ticked.pairs;
    pairs.clear();
    // Never more than a few dozen a frame: what a stalled frame would pile up is dropped.
    let room = |queue: &VfxQueue| queue.0.len() < VfxQueue::MAX;
    for bump in bumps.read() {
        let pair = (bump.kart.min(bump.by), bump.kart.max(bump.by));
        if pairs.contains(&pair) || !room(&queue) {
            continue;
        }
        pairs.push(pair);
        let (Ok((a, memory)), Ok((b, _))) = (karts.get(bump.kart), karts.get(bump.by)) else {
            continue;
        };
        queue.0.push(WorldBurst::Bump {
            at: (a.translation + b.translation) * 0.5 + Vec3::Y * 0.3,
            normal: (a.translation - b.translation).normalize_or_zero(),
            carrier: memory.map_or(Vec3::ZERO, |m| m.velocity),
            speed: bump.closing_speed,
        });
    }
    for blast in blasts.read() {
        if room(&queue) {
            queue.0.push(WorldBurst::Blast {
                at: blast.at,
                radius: blast.radius,
            });
        }
    }
    // Issue #144: a thing to drive through burst: straw, rubber or splinters, and a « pof ».
    for (body, transform, breakable) in &breakables {
        let Some(kart) = breakable.burst_by else {
            continue;
        };
        let kind = layout
            .as_ref()
            .and_then(|layout| layout.hazards.get(usize::from(body.0)))
            // Issue #156: a shortcut's panel breaks into splinters, like a crate; issue #110: its
            // stack of crates too.
            .and_then(|hazard| hazard.kind.burst_kind());
        let (Some(kind), true) = (kind, room(&queue)) else {
            continue;
        };
        let carrier = karts
            .get(kart)
            .ok()
            .and_then(|(_, m)| m)
            .map_or(Vec3::ZERO, |m| m.velocity);
        queue.0.push(WorldBurst::Burst {
            at: transform.translation + Vec3::Y * 0.7,
            carrier,
            kind,
        });
    }
    // Issue #106: a gift box taken bursts into confetti.
    for taken in boxes.read() {
        let Ok(spot) = spots.get(taken.item_box) else {
            continue;
        };
        let carrier = karts
            .get(taken.kart)
            .ok()
            .and_then(|(_, m)| m)
            .map_or(Vec3::ZERO, |m| m.velocity);
        if room(&queue) {
            queue.0.push(WorldBurst::Gift {
                at: spot.translation() + Vec3::Y * 0.2,
                carrier,
            });
        }
    }
    for pick in picked.read() {
        let carrier = karts
            .get(pick.kart)
            .ok()
            .and_then(|(_, m)| m)
            .map_or(Vec3::ZERO, |m| m.velocity);
        if room(&queue) {
            queue.0.push(WorldBurst::Croquette {
                at: pick.at,
                carrier,
                count: 1,
                lost: false,
            });
        }
    }
    for loss in lost.read() {
        if let Ok((transform, memory)) = karts.get(loss.kart)
            && loss.lost > 0
            && room(&queue)
        {
            queue.0.push(WorldBurst::Croquette {
                at: transform.translation + Vec3::Y * 0.6,
                carrier: memory.map_or(Vec3::ZERO, |m| m.velocity),
                count: u32::from(loss.lost),
                lost: true,
            });
        }
    }
}

/// The pool follows the tuning (its cap, the switch), the materials its colors; another level
/// clears it.
pub(super) fn follow_tuning(
    tuning: Res<VfxTuning>,
    mut particles: ResMut<Particles>,
    mut assets: ResMut<VfxAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut loads: MessageReader<LoadLevel>,
    mut queue: ResMut<VfxQueue>,
) {
    if loads.read().count() > 0 {
        particles.pool.clear();
        queue.0.clear();
    }
    if !tuning.is_changed() {
        return;
    }
    emit::follow_the_cap(&mut particles.pool, &tuning);
    if assets.colors != tuning.colors {
        let before = Paint::ALL.map(|paint| assets.color(paint));
        assets.colors = tuning.colors.clone();
        repaint(&before, &assets, &mut materials);
    }
}

/// The materials whose color changed from `before` (by `Paint::index`) take the one of `assets`.
fn repaint(
    before: &[[f32; 3]; Paint::COUNT],
    assets: &VfxAssets,
    materials: &mut Assets<StandardMaterial>,
) {
    for paint in Paint::ALL {
        let color = assets.color(paint);
        if before[paint.index()] != color
            && let Some(mut material) = materials.get_mut(assets.material(paint))
        {
            let [r, g, b] = color;
            material.base_color = Color::srgb(r, g, b);
        }
    }
}

/// Issue #130: the dust off the road takes the colors of the circuit's biome (its shoulders, its
/// sand: `tuning::ground_dust`), the test ground's the dust's of the tuning. Only when the
/// circuit or the tuning changes.
pub(super) fn follow_the_ground(
    tuning: Res<VfxTuning>,
    layout: Option<Res<TrackLayout>>,
    (handle, tracks): (Option<Res<TrackHandle>>, Option<Res<Assets<TrackData>>>),
    mut assets: ResMut<VfxAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut had_layout: Local<bool>,
) {
    let changed = layout.as_ref().is_some_and(|layout| layout.is_changed());
    if !changed && !tuning.is_changed() && layout.is_some() == *had_layout {
        return;
    }
    *had_layout = layout.is_some();
    let palette = layout
        .and(handle)
        .zip(tracks)
        .and_then(|(handle, tracks)| tracks.get(&handle.0)?.biome())
        .map(|biome| biome.palette);
    let ground = palette.map(|palette| ground_dust(&palette, tuning.world.ground_lighten));
    if ground != assets.ground {
        let before = Paint::ALL.map(|paint| assets.color(paint));
        assets.ground = ground;
        repaint(&before, &assets, &mut materials);
    }
}

/// Issue #130: the paint of the dust off the road at `at`, a kart at `distance` along the track
/// (its race progress; None: the closest point of the centerline): the sand's on the road (a
/// sand patch), the shoulders' beside it.
fn ground_paint(layout: Option<&TrackLayout>, distance: Option<f32>, at: Vec3) -> Paint {
    let Some(layout) = layout else {
        return Paint::Ground;
    };
    let sample = match distance {
        Some(distance) => layout.center.at(distance),
        None => layout.center.samples()[layout.center.nearest_index(at)],
    };
    let lateral = (at - sample.position).dot(sample.right);
    if lateral.abs() <= sample.half_width {
        Paint::Sand
    } else {
        Paint::Ground
    }
}

type KartLook = (
    &'static mut KartVfx,
    (
        &'static KartState,
        &'static DriftMotion,
        &'static Boost,
        &'static KartWheels,
        &'static KartEnvironment,
        &'static ItemEffects,
    ),
    (
        Option<&'static StatEffects>,
        Option<&'static KartAssists>,
        Has<PlayerKart>,
        Option<&'static RaceProgress>,
    ),
);

/// Each frame, where each kart is shown: what it throws all along, and its bursts.
pub(super) fn emit_karts(
    mut commands: Commands,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<VfxTuning>,
    kart_tuning: Res<KartTuning>,
    deflated: Option<Res<DeflatedScale>>,
    layout: Option<Res<TrackLayout>>,
    mut particles: ResMut<Particles>,
    cameras: Query<&Transform, (With<ChaseCamera>, Without<InterpolatedFrom>)>,
    roots: Query<(&InterpolatedFrom, &Transform)>,
    mut karts: Query<KartLook>,
) {
    let layout = layout.as_deref();
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let alpha = fixed.overstep_fraction();
    let small = deflated.map_or(1.0, |s| s.0);
    let camera = cameras.iter().next().map(|t| t.translation);
    let pool = &mut particles.pool;
    let max = tuning.max_per_frame;
    let (drift, boost, impact, world) =
        (&tuning.drift, &tuning.boost, &tuning.impact, &tuning.world);
    for (InterpolatedFrom(body), root) in &roots {
        let Ok((mut vfx, rules, (stats, assists, player, race_progress))) = karts.get_mut(*body)
        else {
            continue;
        };
        let vfx = &mut *vfx;
        let (state, motion, kart_boost, wheels, environment, effects) = rules;
        // Its velocity, measured on its visual.
        if dt > 0.0 {
            vfx.velocity = match vfx.shown_at {
                Some(at) if at.distance(root.translation) < TELEPORT => {
                    (root.translation - at) / dt
                }
                _ => Vec3::ZERO,
            };
            vfx.shown_at = Some(root.translation);
        }
        vfx.dizzy = (vfx.dizzy - dt).max(0.0);
        let bursts = std::mem::take(&mut vfx.pending);
        if bursts.dizzy > 0.0 {
            vfx.dizzy = vfx.dizzy.max(bursts.dizzy);
        }
        // The player's camera shakes on a shock.
        if player && tuning.enabled {
            let shake = if bursts.hit.is_some() {
                impact.shake_hit
            } else if bursts.wall > 0.0 {
                impact.shake_wall * (bursts.wall / 12.0).clamp(0.5, 1.5)
            } else if bursts.bump > 0.0 {
                impact.shake_bump * (bursts.bump / 15.0).clamp(0.4, 1.4)
            } else {
                0.0
            };
            if shake > 0.0 {
                commands
                    .entity(*body)
                    .try_insert(CameraShake::new(now, shake, impact.shake_time));
            }
        }
        if let Some(source) = bursts.boost {
            vfx.punch = vfx.punch.max(fov_punch(&tuning, source));
        }
        let far = camera.is_some_and(|c| c.distance(root.translation) > tuning.emit_range);
        if !tuning.enabled
            || far
            || effects.hidden()
            || *state == KartState::Respawning
            || pool.cap() == 0
        {
            vfx.carry = [0.0; EMITTERS];
            continue;
        }
        let (slip, slip_offset) = slip_pose(motion.interpolated_slip(alpha));
        let frame = KartFrame {
            position: root.translation,
            rotation: root.rotation,
            slip,
            slip_offset,
            velocity: vfx.velocity,
            scale: effects.deflated_scale(small),
        };
        let on_ground = wheels.ticks_without_ground == 0;
        let speed = vfx.velocity.length();
        let speed_ratio = speed / kart_tuning.max_speed.max(1.0);
        // The drift: its sparks by tier, its smoke.
        let assisted = assists.is_some_and(|a| a.assisted_steering);
        let (tier, progress) = drift_tier(&kart_tuning, state, stats, assisted);
        let carry = &mut vfx.carry;
        match tier {
            Some(tier) => {
                let style = spark_style(drift, tier, progress);
                for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
                    let n = take(&mut carry[SPARKS + k], style.rate, dt, max);
                    emit::drift_sparks(pool, &frame, side, &style, drift, n, dt);
                }
            }
            None => carry[SPARKS..SPARKS + 2].fill(0.0),
        }
        let smoking = matches!(state, KartState::Drifting { .. }) && on_ground;
        for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            let rate = if smoking { drift.smoke_rate } else { 0.0 };
            let n = take(&mut carry[SMOKE + k], rate, dt, max);
            emit::drift_smoke(pool, &frame, side, drift, n, dt);
        }
        // The boost: embers that follow the flames, a mini-turbo's trail.
        let flame = flame_size(&kart_tuning, kart_boost);
        let n = take(&mut carry[EMBERS], boost.ember_rate * 2.0 * flame, dt, max);
        emit::embers(pool, &frame, ember_paint(vfx.source), flame, boost, n, dt);
        let trail = match vfx.source {
            Some(BoostSource::MiniTurbo(tier)) if kart_boost.is_active() => Some(tier),
            _ => None,
        };
        let rate = if trail.is_some() {
            drift.trail_rate
        } else {
            0.0
        };
        let n = take(&mut carry[TRAIL], rate, dt, max);
        if let Some(tier) = trail {
            emit::trail(pool, &frame, Paint::tier(tier.index()), n, dt);
        }
        // The ground: dust off the road (issue #130: the color of the ground it comes from), the
        // anti-gravity's sparks, the glider's wind.
        let dusty = on_ground && wheels.offroad_ratio > 0.0 && speed > world.dust_min_speed;
        let dust_rate = world.dust_rate * wheels.offroad_ratio * speed_ratio.min(1.2);
        let offroad = on_ground && wheels.offroad_ratio > 0.5;
        let ground = if dusty || (offroad && bursts.landing) {
            ground_paint(layout, race_progress.map(|p| p.distance), root.translation)
        } else {
            Paint::Ground
        };
        for (k, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            let n = take(
                &mut carry[DUST + k],
                if dusty { dust_rate } else { 0.0 },
                dt,
                max,
            );
            emit::dust(pool, &frame, side, ground, world, n, dt);
        }
        // Issue #130: the wake of a kart at the surface of the water.
        let water = layout.and_then(|layout| {
            water_depth(&layout.lakes, root.translation)
                .map(|depth| (depth, root.translation.y + depth))
        });
        let wake = water.filter(|(depth, _)| {
            (-world.wake_above..=world.wake_below).contains(depth)
                && speed > world.wake_min_speed
                && !environment.gliding
        });
        let n = take(
            &mut carry[WAKE],
            if wake.is_some() {
                world.wake_rate * speed_ratio.min(1.2)
            } else {
                0.0
            },
            dt,
            max,
        );
        if let Some((_, level)) = wake {
            emit::wake(pool, &frame, level, world, n, dt);
        }
        let gravity = environment.anti_gravity && on_ground && speed > 5.0;
        let rate = world.anti_gravity_rate * 4.0 * speed_ratio.min(1.2);
        let n = take(
            &mut carry[ANTI_GRAVITY],
            if gravity { rate } else { 0.0 },
            dt,
            max,
        );
        emit::anti_gravity_sparks(pool, &frame, n, dt);
        let rate = if environment.gliding {
            world.glide_wind_rate * 2.0
        } else {
            0.0
        };
        let n = take(&mut carry[GLIDE], rate, dt, max);
        emit::glide_wind(pool, &frame, n, dt);
        // The bursts of the ticks gone by.
        let landing = if offroad { ground } else { Paint::Dust };
        let surface = water.map(|(_, level)| root.translation.with_y(level));
        throw_bursts(
            pool,
            &frame,
            &bursts,
            &tuning,
            (environment.anti_gravity, landing, surface),
        );
    }
}

/// A kart's bursts, where it is shown: in anti-gravity or not, the paint of its landing's dust
/// (issue #130: the ground's off the road), and the surface of the water at it (if a lake is
/// under it).
fn throw_bursts(
    pool: &mut ParticlePool,
    frame: &KartFrame,
    bursts: &Bursts,
    tuning: &VfxTuning,
    (anti_gravity, landing, surface): (bool, Paint, Option<Vec3>),
) {
    let (drift, boost, impact, world) =
        (&tuning.drift, &tuning.boost, &tuning.impact, &tuning.world);
    let center = frame.point(Vec3::new(0.0, 0.4, 0.0));
    if let Some(tier) = bursts.tier {
        emit::tier_flash(pool, frame, tier.index(), drift);
    }
    match bursts.boost {
        Some(BoostSource::MiniTurbo(tier)) => emit::release(pool, frame, tier.index(), drift),
        Some(BoostSource::RocketStart) => {
            emit::rocket_smoke(pool, frame, boost);
            emit::spray(pool, frame, Paint::Flame, boost.pad_sparks, true);
        }
        Some(BoostSource::Trick) => emit::sparkles(
            pool,
            center,
            frame.velocity * 0.8,
            Paint::Gold,
            boost.trick_sparkles,
            world.sparkle_size * 1.3,
            world.sparkle_life,
            4.0,
        ),
        Some(BoostSource::Slipstream) => emit::wind_burst(pool, frame, boost.slipstream_streaks),
        Some(BoostSource::Piment) => {
            emit::spray(pool, frame, Paint::Piment, boost.piment_sparks, true);
        }
        Some(BoostSource::SpinBoost) => {
            emit::spray(pool, frame, Paint::Cyan, boost.spin_sparks, false)
        }
        Some(BoostSource::BoostPad) => {
            let paint = if anti_gravity {
                Paint::Cyan
            } else {
                Paint::Flame
            };
            emit::spray(pool, frame, paint, boost.pad_sparks, true);
        }
        Some(BoostSource::Croquette) | None => {}
    }
    if bursts.trick {
        emit::sparkles(
            pool,
            center,
            frame.velocity * 0.9,
            Paint::Gold,
            boost.trick_sparkles,
            world.sparkle_size * 1.3,
            world.sparkle_life,
            5.0,
        );
    }
    if bursts.hit.is_some() {
        emit::shock(
            pool,
            frame.point(Vec3::new(0.0, 1.0, 0.0)),
            Vec3::ZERO,
            frame.velocity,
            6,
            4,
            impact,
        );
    }
    if bursts.wall > 0.0 {
        let toward = bursts.wall_dir;
        emit::shock(
            pool,
            frame.position + toward * 0.9 + Vec3::Y * 0.2,
            -toward,
            frame.velocity,
            impact.wall_stars,
            impact.wall_puffs,
            impact,
        );
    }
    if bursts.struck {
        emit::shock(
            pool,
            frame.point(Vec3::new(0.0, 1.1, 0.0)),
            Vec3::ZERO,
            frame.velocity,
            impact.paw_stars,
            2,
            impact,
        );
    }
    if bursts.landing {
        emit::landing(pool, frame, landing, world);
    }
    // Issue #130: into the water or out of it, a splash on its surface.
    if let (Some(going_in), Some(surface)) = (bursts.splash, surface) {
        emit::water_splash(pool, surface, frame.velocity, going_in, world);
    }
    if bursts.creamed {
        emit::cream(pool, frame, impact);
    }
    if bursts.stall {
        emit::stall_puffs(pool, frame, boost);
    }
}

/// Each frame: the bursts of the world gone by.
pub(super) fn emit_world(
    tuning: Res<VfxTuning>,
    mut queue: ResMut<VfxQueue>,
    mut particles: ResMut<Particles>,
    cameras: Query<&Transform, With<ChaseCamera>>,
) {
    let camera = cameras.iter().next().map(|t| t.translation);
    let pool = &mut particles.pool;
    let (impact, world) = (&tuning.impact, &tuning.world);
    for burst in queue.0.drain(..) {
        if !tuning.enabled {
            continue;
        }
        let at = match burst {
            WorldBurst::Bump { at, .. }
            | WorldBurst::Blast { at, .. }
            | WorldBurst::Gift { at, .. }
            | WorldBurst::Burst { at, .. }
            | WorldBurst::Croquette { at, .. } => at,
        };
        if camera.is_some_and(|c| c.distance(at) > tuning.emit_range) {
            continue;
        }
        match burst {
            WorldBurst::Bump {
                at,
                normal,
                carrier,
                speed,
            } => {
                let stars = (speed / impact.bump_speed_per_star.max(0.1)).clamp(3.0, 8.0) as u32;
                emit::shock(pool, at, normal, carrier, stars, impact.pof_puffs, impact);
            }
            WorldBurst::Blast { at, radius } => emit::confetti(pool, at, radius, impact),
            WorldBurst::Gift { at, carrier } => emit::gift(pool, at, carrier, impact),
            WorldBurst::Burst { at, carrier, kind } => {
                emit::shock(pool, at, Vec3::Y, carrier, 2, impact.pof_puffs, impact);
                emit::debris(pool, at, carrier, emit::debris_paints(kind), impact);
            }
            WorldBurst::Croquette {
                at,
                carrier,
                count,
                lost,
            } => {
                let n = if lost {
                    world.lost_sparkles * count
                } else {
                    world.croquette_sparkles
                };
                emit::sparkles(
                    pool,
                    at,
                    carrier * 0.7,
                    Paint::Gold,
                    n.min(30),
                    world.sparkle_size,
                    world.sparkle_life,
                    if lost { 4.5 } else { 3.0 },
                );
            }
        }
    }
}

/// The camera's kart: speed lines while it boosts, and its field of view (wider while it
/// boosts, a punch when a boost starts). Not on the podium's still view, nor behind the home
/// screen.
pub(super) fn kick_the_camera(
    time: Res<Time>,
    tuning: Res<VfxTuning>,
    fixed_view: Option<Res<FixedView>>,
    menu: Option<Res<MainMenu>>,
    mut kick: ResMut<FovKick>,
    mut particles: ResMut<Particles>,
    mut cameras: Query<(&ChaseCamera, &Transform, &mut Projection)>,
    roots: Query<&InterpolatedFrom>,
    mut karts: Query<(&Boost, &mut KartVfx)>,
) {
    let dt = time.delta_secs();
    let Ok((camera, eye, mut projection)) = cameras.single_mut() else {
        return;
    };
    let Projection::Perspective(perspective) = &mut *projection else {
        return;
    };
    let still = fixed_view.is_some() || menu.is_some_and(|m| m.on_home());
    let kart = roots
        .get(camera.target)
        .ok()
        .and_then(|InterpolatedFrom(body)| karts.get_mut(*body).ok());
    let Some((kart_boost, mut vfx)) = kart.filter(|_| !still && tuning.enabled) else {
        kick.extra = 0.0;
        kick.punch = 0.0;
        kick.written = f32::NAN;
        return;
    };
    let boost = &tuning.boost;
    let strength = (kart_boost.strength / 0.3).clamp(0.6, 1.5);
    let target = if kart_boost.is_active() {
        boost.fov_extra * strength
    } else {
        0.0
    };
    kick.extra += (target - kick.extra) * (1.0 - (-boost.fov_response * dt).exp());
    if vfx.punch > 0.0 {
        kick.punch = vfx.punch;
        kick.punch_age = 0.0;
        vfx.punch = 0.0;
    }
    kick.punch_age += dt;
    let t = kick.punch_age / boost.fov_punch_time.max(0.01);
    let punch = if t >= 1.0 {
        0.0
    } else if t < 0.15 {
        kick.punch * t / 0.15
    } else {
        kick.punch * (1.0 - (t - 0.15) / 0.85).powi(2)
    };
    // On top of what the chase wrote this frame (or, if it did not, of what it wrote before).
    let base = if perspective.fov == kick.written {
        kick.base
    } else {
        perspective.fov
    };
    perspective.fov = base + (kick.extra + punch).to_radians();
    kick.base = base;
    kick.written = perspective.fov;
    // The speed lines.
    let rate = if kart_boost.is_active() {
        boost.speed_line_rate * (kart_boost.strength / 0.3).clamp(0.5, 2.0)
    } else {
        0.0
    };
    let n = take(&mut kick.lines, rate, dt, tuning.max_per_frame * 2);
    if n > 0 {
        let forward = eye.forward();
        emit::speed_lines(
            &mut particles.pool,
            eye.translation,
            *forward,
            vfx.velocity,
            boost,
            n,
        );
    }
}

/// The pool moves on, and each slot's entity shows its particle (its mesh and material swapped
/// only when its look changes; hidden when it is over).
pub(super) fn show_particles(
    time: Res<Time>,
    assets: Res<VfxAssets>,
    mut particles: ResMut<Particles>,
    cameras: Query<&Transform, (With<ChaseCamera>, Without<VfxParticle>)>,
    mut slots: Query<
        (
            &mut Transform,
            &mut Visibility,
            &mut Mesh3d,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        With<VfxParticle>,
    >,
) {
    let camera = cameras.iter().next().map_or(Vec3::ZERO, |t| t.translation);
    let Particles {
        pool,
        entities,
        shown,
    } = &mut *particles;
    pool.step(time.delta_secs());
    for ((particle, entity), shown) in pool
        .slots()
        .iter()
        .zip(entities.iter())
        .zip(shown.iter_mut())
    {
        let alive = particle.alive();
        if !alive && shown.is_none() {
            continue;
        }
        let Ok((mut transform, mut visibility, mut mesh, mut material)) = slots.get_mut(*entity)
        else {
            continue;
        };
        if !alive {
            *visibility = Visibility::Hidden;
            *shown = None;
            continue;
        }
        let look = (particle.shape, particle.paint);
        if *shown != Some(look) {
            if shown.is_none() {
                *visibility = Visibility::Inherited;
            }
            let want_mesh = assets.mesh(particle.shape);
            if mesh.0.id() != want_mesh.id() {
                mesh.0 = want_mesh.clone();
            }
            let want_material = assets.material(particle.paint);
            if material.0.id() != want_material.id() {
                material.0 = want_material.clone();
            }
            *shown = Some(look);
        }
        *transform = particle.transform(camera);
    }
}

/// The stars over the head of a kart that sees them (« sonné »): made the first time, under its
/// visual, turning around above its head.
pub(super) fn show_dizzy_stars(
    mut commands: Commands,
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<VfxTuning>,
    assets: Res<VfxAssets>,
    cameras: Query<&Transform, (With<ChaseCamera>, Without<DizzyStar>)>,
    roots: Query<(Entity, &InterpolatedFrom, &Transform), Without<DizzyStar>>,
    mut karts: Query<(&mut KartVfx, &KartState, &ItemEffects)>,
    mut stars: Query<
        (&mut Transform, &mut Visibility),
        (With<DizzyStar>, Without<InterpolatedFrom>),
    >,
) {
    let dt = time.delta_secs();
    let alpha = fixed.overstep_fraction();
    let camera = cameras.iter().next().map(|t| t.translation);
    let impact = &tuning.impact;
    let count = (impact.dizzy_stars as usize).min(MAX_DIZZY_STARS);
    for (root, InterpolatedFrom(body), root_transform) in &roots {
        let Ok((mut vfx, state, effects)) = karts.get_mut(*body) else {
            continue;
        };
        // Not over an invisible kart (a pie voleuse): they would show where it is.
        let dizzy = vfx.dizzy > 0.0 && tuning.enabled && count > 0 && !effects.hidden();
        // Its stars: made once, under its visual (again under a new visual).
        let owned = vfx.stars.filter(|(owner, _)| *owner == root);
        let Some((_, ids)) = owned.or_else(|| {
            dizzy.then(|| {
                let ids = std::array::from_fn(|index| {
                    commands
                        .spawn((
                            Name::new("Étoile"),
                            DizzyStar { body: *body, index },
                            Mesh3d(assets.mesh(Shape::Star).clone()),
                            MeshMaterial3d(assets.material(Paint::Star).clone()),
                            Transform::from_scale(Vec3::ZERO),
                            Visibility::Hidden,
                            NotShadowCaster,
                            NotShadowReceiver,
                            ChildOf(root),
                        ))
                        .id()
                });
                vfx.stars = Some((root, ids));
                (root, ids)
            })
        }) else {
            continue;
        };
        vfx.spin = (vfx.spin + impact.dizzy_spin * dt).rem_euclid(TAU);
        // Thrown up by a tumble, they go up with it.
        let lift = match *state {
            KartState::Hit {
                reaction,
                elapsed,
                duration,
            } => hit_pose(reaction, (elapsed as f32 + alpha) / duration.max(1) as f32).1,
            _ => 0.0,
        };
        // They pop in, and shrink away in their last 0.25 s.
        let size = impact.dizzy_size * (vfx.dizzy / 0.25).min(1.0);
        let to_camera = camera.map_or(Vec3::Z, |c| {
            root_transform.rotation.inverse() * (c - root_transform.translation)
        });
        for (index, id) in ids.iter().enumerate() {
            let Ok((mut transform, mut visibility)) = stars.get_mut(*id) else {
                continue;
            };
            let shown = dizzy && index < count;
            visibility.set_if_neq(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            if !shown {
                continue;
            }
            let angle = vfx.spin + index as f32 * TAU / count as f32;
            let bob = 0.06 * (angle * 2.0).sin();
            let at = Vec3::new(
                angle.cos() * impact.dizzy_radius,
                impact.dizzy_height + lift + bob,
                angle.sin() * impact.dizzy_radius,
            );
            let facing = (to_camera - at).normalize_or(Vec3::Z);
            *transform = Transform::from_translation(at)
                .with_rotation(
                    Quat::from_rotation_arc(Vec3::Z, facing) * Quat::from_rotation_z(angle * 1.5),
                )
                .with_scale(Vec3::splat(size));
        }
    }
}

/// Each kart's flames take the color of their boost's source (GD § 14: a blue, orange or purple
/// mini-turbo, a red piment...): the shared materials of the particles, swapped only when it
/// changes.
pub(super) fn tint_flames(
    assets: Res<VfxAssets>,
    pivots: Query<&KartBodyPivot>,
    karts: Query<&KartVfx>,
    mut flames: Query<(&ChildOf, &mut MeshMaterial3d<StandardMaterial>), With<BoostFlame>>,
) {
    for (child_of, mut material) in &mut flames {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(vfx) = karts.get(pivot.body) else {
            continue;
        };
        let want = assets.material(ember_paint(vfx.source));
        if material.0.id() != want.id() {
            material.0 = want.clone();
        }
    }
}
