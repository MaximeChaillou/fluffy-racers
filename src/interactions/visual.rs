//! What the interactions look like (GD § 14 and 15, issue #34), in primitives (the struck
//! driver's mini-ragdoll is the soft characters', `characters::wobble`, issue #12). Every kart's
//! visual gets, once:
//!
//! - an **arm** ending in a **paw**, hidden at rest. It rises as soon as the Interaction button is
//!   down, swings at the kart struck (or into thin air, then windmills off balance with the driver
//!   leaning out), reaches out while grabbing, and during a tether stretches from the driver to
//!   the kart held: the elastic band. It comes back with a "boing" when it lets go. The kart held
//!   turns its driver toward the grabber, outraged. Issue #169: **the animal's own paw**
//!   (`characters::PawModel`: its shape, its colors taken on its model, a bird's wing, which
//!   leaves the bird's body while it is out), coming out of **its body** (its shoulder measured
//!   on its model, followed through its clip, its wobble and its acting each frame, at its
//!   size); a driver in primitives gets a cream paw out of its ball;
//! - a **"pof"** (a flat yellow diamond) that pops on the side of a kart struck.
//!
//! The player's camera shakes when its kart is struck (`camera::CameraShake`), and the HUD gets
//! the paw's recharge: a gauge under the item slots that fills up (GD § 14).
//!
//! One mesh per part and per animal (and one for the primitives), the models' shared material,
//! shared by every kart; the stretch is the arm's transform; nothing is spawned or despawned
//! while racing, nothing allocated. Everything is read from the simulation (`Paw`) and from where the karts are
//! shown (their visuals: interpolated, or placed by the network for another peer's kart):
//! nothing here touches the simulation.
//!
//! Issue #81: an arm is never drawn longer than the longest tether (`logic::max_arm_length`,
//! about 8 m), nor toward a kart that is not shown, nor with a NaN: whatever a paw says, it is
//! hidden then.

use std::f32::consts::PI;

use bevy::app::AnimationSystems;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::logic::{Paw, PawMotion, SwingKind, max_arm_length};
use super::tuning::InteractionTuning;
use crate::camera::CameraShake;
use crate::characters::{
    ActingSystems, AnimalPart, AnimalRoot, Animals, DriverTuning, WobbleSystems, primitive_paw,
    transform_in,
};
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{DRIVER_RADIUS, DriverVisual, KartBodyPivot, PlayerKart};
use crate::track::circuit::to_mesh;
use crate::ui::{HudRoot, SLOT_SIZES, SLOTS_TOP};

/// Where the arms start on a kart's pivot without a driver: the shoulders of a medium driver, to
/// its sides (issue #169: a driver's are its animal's, or its ball's).
const SHOULDER: Vec3 = Vec3::new(0.4, 0.62, 0.15);
/// Issue #169: where a driver in primitives' arm comes out of its ball (in the driver's frame,
/// on its right, at size 1): on its side, at its middle's height, just inside.
const BALL_SHOULDER: Vec3 = Vec3::new(DRIVER_RADIUS * 0.85, DRIVER_RADIUS, 0.0);
/// Issue #169: the palm of a hand (its pads) faces back toward the chase camera, and a little up,
/// as far as its arm lets it: where the back of the hand faces, in the pivot's frame.
const HAND_BACK: Vec3 = Vec3::new(0.0, -0.351_123_4, -0.936_329_2);
/// Where the other kart's driver is, above its body's center (m): what a paw reaches for.
const DRIVER_HEIGHT: f32 = 0.6;
/// Issue #169: a paw lands on the other driver, this far short of its middle (m): on its body,
/// not inside it.
pub(crate) const LANDS_SHORT: f32 = 0.55;
/// A swing reaches its target in this long (s), then the arm comes back in `RETURN_TIME`.
const SWING_TIME: f32 = 0.1;
const RETURN_TIME: f32 = 0.2;
/// The arm windmills this fast off balance (rad/s), this far from the shoulder (m).
const WINDMILL_SPEED: f32 = 18.0;
const WINDMILL_REACH: f32 = 0.55;
/// How far the driver leans out off balance (rad), and turns toward its grabber (rad).
const LEAN: f32 = 0.45;
const OUTRAGED_TURN: f32 = 1.0;
/// The "pof": how long it shows (s), how big it gets (m).
const POF_TIME: f32 = 0.25;
const POF_SIZE: f32 = 0.6;
/// The HUD gauge (px).
const GAUGE_SIZE: f32 = 40.0;
const GAUGE_READY: Color = Color::srgb(1.0, 0.86, 0.55);
const GAUGE_LOADING: Color = Color::srgb(0.55, 0.52, 0.6);

/// A part of a kart's arm: the arm itself or the paw at its end (its hand), under the kart
/// visual's pivot.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PawArm {
    pub body: Entity,
    /// The paw at the end, rather than the arm.
    pub paw: bool,
}

/// The "pof" of a strike received, under the kart visual's pivot.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PawPof {
    pub body: Entity,
}

/// The HUD's paw recharge gauge, and its fill.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PawGauge;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PawGaugeFill;

/// Meshes and materials shared by every kart's arm and "pof": the arm and the paw of a driver in
/// primitives (issue #169: an animal's are its own, `characters::AnimalPaw`).
#[derive(Resource, Clone, Debug)]
pub struct PawVisualAssets {
    arm: Handle<Mesh>,
    paw: Handle<Mesh>,
    /// How long the primitives' paw is (in the animals' units).
    paw_length: f32,
    pof: Handle<Mesh>,
    paw_material: Handle<StandardMaterial>,
    pof_material: Handle<StandardMaterial>,
}

pub struct InteractionVisualPlugin;

impl Plugin for InteractionVisualPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_assets)
            .add_systems(
                Update,
                (
                    attach_arms,
                    shake_the_player,
                    spawn_gauge
                        .run_if(any_with_component::<HudRoot>)
                        .run_if(not(any_with_component::<PawGauge>)),
                    fill_gauge,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    (pop_pofs, pose_drivers),
                    // Issue #169: once the driver and its animal are posed for this frame (the
                    // clips, the wobble, the acting): the arm follows its body.
                    pose_arms
                        .after(pose_drivers)
                        .after(AnimationSystems)
                        .after(WobbleSystems)
                        .after(ActingSystems),
                )
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

fn create_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let paw = primitive_paw();
    commands.insert_resource(PawVisualAssets {
        // Issue #169: the paw of a driver in primitives (cream, in its vertex colors).
        arm: meshes.add(to_mesh(&paw.arm)),
        paw: meshes.add(to_mesh(&paw.hand)),
        paw_length: paw.hand_length,
        // A flat square, turned into a diamond.
        pof: meshes.add(Cuboid::new(1.0, 1.0, 0.08)),
        paw_material: materials.add(Color::WHITE),
        pof_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.95, 0.35),
            unlit: true,
            ..default()
        }),
    });
}

/// Every new kart visual gets its arm, paw and "pof" (hidden).
fn attach_arms(
    mut commands: Commands,
    assets: Option<Res<PawVisualAssets>>,
    pivots: Query<(Entity, &KartBodyPivot), Added<KartBodyPivot>>,
    karts: Query<(), With<Paw>>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (pivot, kart) in &pivots {
        if !karts.contains(kart.body) {
            continue;
        }
        let body = kart.body;
        commands.entity(pivot).with_children(|parts| {
            parts.spawn((
                PawArm { body, paw: false },
                Mesh3d(assets.arm.clone()),
                MeshMaterial3d(assets.paw_material.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
            parts.spawn((
                PawArm { body, paw: true },
                Mesh3d(assets.paw.clone()),
                MeshMaterial3d(assets.paw_material.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
            parts.spawn((
                PawPof { body },
                Mesh3d(assets.pof.clone()),
                MeshMaterial3d(assets.pof_material.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
        });
    }
}

/// The shoulder of `shoulders` (the left one, then the right one) on `side` (-1 or 1).
fn on_side(shoulders: [Vec3; 2], side: f32) -> Vec3 {
    shoulders[usize::from(side >= 0.0)]
}

/// Where a kart's paw is, in its pivot's frame: `None` when the arm is down (hidden).
/// `shoulders`: where its arms come out, in the same frame (its left one, then its right one;
/// issue #169: its driver's, `size` big, which scales the paw raised and windmilling); `other`:
/// where the kart it goes for is (its driver), in the same frame; `now`: the clock, for the
/// wobbles; `max_length`: the longest arm (`logic::max_arm_length`).
///
/// Issue #81: the arm is hidden when it would be longer than `max_length` (whatever the paw
/// says, that tether is broken), when the kart held is not shown (`other` is `None`), or when
/// anything is not finite.
pub fn paw_tip(
    paw: &Paw,
    shoulders: [Vec3; 2],
    size: f32,
    other: Option<Vec3>,
    now: f32,
    max_length: f32,
) -> Option<(f32, Vec3)> {
    let (side, tip) = paw_tip_unchecked(paw, shoulders, size, other, now)?;
    let reach = tip.distance(on_side(shoulders, side));
    (tip.is_finite() && side.is_finite() && reach <= max_length).then_some((side, tip))
}

/// [`paw_tip`], before its checks.
fn paw_tip_unchecked(
    paw: &Paw,
    shoulders: [Vec3; 2],
    size: f32,
    other: Option<Vec3>,
    now: f32,
) -> Option<(f32, Vec3)> {
    let shoulder = |side: f32| on_side(shoulders, side);
    let seconds = |ticks: u32| ticks as f32 / crate::TICK_HZ as f32;
    // Raised (GD § 15), on its side: above its shoulder, as high as the driver is big.
    let raised = |side: f32| shoulder(side) + Vec3::new(side * 0.2, 0.5, 0.05) * size;
    match paw.motion {
        // Raised as soon as the button is down (GD § 15), on the side it will strike.
        PawMotion::Raised { .. } => {
            let side = side_of(paw.aim);
            return Some((side, raised(side)));
        }
        // Reaching out, the paw groping around.
        PawMotion::Reaching { ticks } => {
            let side = side_of(paw.aim);
            let grope = (seconds(ticks) * 20.0).sin();
            let out = Vec3::new(side * (1.1 + 0.3 * grope), 0.05, -0.3);
            return Some((side, shoulder(side) + out));
        }
        // The tether: stretched to the kart held (not shown: no arm).
        PawMotion::Holding { side, .. } => {
            return other.map(|tip| (side, tip));
        }
        _ => {}
    }
    // Off balance: the arm windmills.
    if paw.off_balance > 0 {
        let side = paw.off_balance_side;
        let angle = now * WINDMILL_SPEED;
        let circle = Vec3::new(side * angle.cos().abs(), angle.sin(), 0.0) * WINDMILL_REACH;
        return Some((
            side,
            shoulder(side) + (Vec3::X * side * 0.2 + circle) * size,
        ));
    }
    // A swing: out to its target (or into thin air), then back with a "boing".
    let swing = paw.swing?;
    let t = seconds(swing.ticks);
    let side = swing.side;
    let start = raised(side);
    let reach = match swing.kind {
        SwingKind::Strike | SwingKind::Throw | SwingKind::LetGo => {
            other.unwrap_or(shoulder(side) + Vec3::X * side * 2.0)
        }
        SwingKind::Whiff | SwingKind::GrabMiss => shoulder(side) + Vec3::new(side * 1.8, 0.0, -0.4),
    };
    let (from, out) = if swing.kind == SwingKind::LetGo {
        (reach, 1.0)
    } else {
        (start, (t / SWING_TIME).min(1.0))
    };
    if t < SWING_TIME && swing.kind != SwingKind::LetGo {
        return Some((side, from.lerp(reach, out * out * (3.0 - 2.0 * out))));
    }
    let back = (t - if swing.kind == SwingKind::LetGo {
        0.0
    } else {
        SWING_TIME
    }) / RETURN_TIME;
    if back >= 1.0 {
        return None;
    }
    // Back to the shoulder, overshooting a little: boing.
    let boing = 1.0 - (1.0 - back) * (1.0 - back) * (1.0 + 2.5 * back);
    let rest = shoulder(side);
    Some((side, reach.lerp(rest, boing)))
}

/// -1 or 1 (0 counts as the right).
fn side_of(aim: f32) -> f32 {
    if aim < 0.0 { -1.0 } else { 1.0 }
}

/// Issue #169: the arm's and the hand's transforms, from `shoulder` to `tip` on `side`: the hand
/// (its mesh centered on its middle, along +Y, `hand_length` long, its palm -Z) on `tip`, its
/// palm toward the camera ([`HAND_BACK`]) as far as the arm lets it; the arm (its mesh 1
/// long along +Y) from the shoulder to the wrist: stretched by its scale, never remade. Both
/// `scale` big (the animal's).
pub fn arm_and_hand(
    shoulder: Vec3,
    tip: Vec3,
    side: f32,
    scale: f32,
    hand_length: f32,
) -> (Transform, Transform) {
    let along = tip - shoulder;
    let toward = along.normalize_or(Vec3::X * side_of(side));
    let across = HAND_BACK - toward * HAND_BACK.dot(toward);
    let back = if across.length_squared() > 1e-4 {
        across.normalize()
    } else {
        toward.any_orthonormal_vector()
    };
    let rotation = Quat::from_mat3(&Mat3::from_cols(toward.cross(back), toward, back));
    let half = hand_length * scale / 2.0;
    let length = (along.length() - half).max(0.01);
    let arm = Transform {
        translation: shoulder,
        rotation,
        scale: Vec3::new(scale, length, scale),
    };
    let hand = Transform {
        translation: tip,
        rotation,
        scale: Vec3::splat(scale),
    };
    (arm, hand)
}

/// The "pof"'s size `t` s after a strike (0 when gone): pops up, then shrinks.
pub fn pof_size(t: f32) -> f32 {
    if !(0.0..POF_TIME).contains(&t) {
        return 0.0;
    }
    let x = t / POF_TIME;
    POF_SIZE * (PI * x.sqrt()).sin()
}

/// A kart's driver as its arms see it: where its shoulders are in its pivot's frame (its left
/// one, then its right one), how big its paw is (its animal's scale), how long its hand, which
/// meshes and material they are, and a bird's wings (its left one, then its right one).
struct ArmsOf<'a> {
    shoulders: [Vec3; 2],
    size: f32,
    scale: f32,
    hand_length: f32,
    arm: &'a Handle<Mesh>,
    hand: &'a Handle<Mesh>,
    material: &'a Handle<StandardMaterial>,
    wings: [Option<Entity>; 2],
}

/// Every kart's arm and paw (issue #169: its animal's, out of its body).
#[allow(clippy::too_many_arguments)]
fn pose_arms(
    time: Res<Time>,
    (tuning, driver_tuning): (Res<InteractionTuning>, Option<Res<DriverTuning>>),
    (assets, animals): (Option<Res<PawVisualAssets>>, Option<Res<Animals>>),
    paws: Query<&Paw>,
    roots: Query<&Transform, (With<InterpolatedFrom>, Without<PawArm>)>,
    pivots: Query<(&KartBodyPivot, &Transform, &ChildOf, &Children), Without<PawArm>>,
    drivers: Query<(&DriverVisual, &Transform, Option<&Children>), Without<PawArm>>,
    animal_roots: Query<&AnimalRoot>,
    nodes: Query<(&Transform, &ChildOf), Without<PawArm>>,
    mut wings: Query<&mut Visibility, (With<AnimalPart>, Without<PawArm>)>,
    mut arms: Query<(
        &PawArm,
        &ChildOf,
        &mut Transform,
        &mut Visibility,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(assets) = assets else {
        return;
    };
    let now = time.elapsed_secs();
    let max_length = max_arm_length(&tuning);
    let model_scale = driver_tuning.map_or(0.62, |t| t.model_scale.max(0.01));
    // Where a kart's driver is shown (issue #81): above its visual, interpolated for our karts,
    // placed by the network for another peer's (which has no `PhysicsPose`).
    let shown = |kart: Entity| {
        let (_, _, pivot_of, _) = pivots.iter().find(|(pivot, ..)| pivot.body == kart)?;
        let root = roots.get(pivot_of.parent()).ok()?;
        Some(root.translation + Vec3::Y * DRIVER_HEIGHT)
    };
    // The driver under `pivot` and its arms: its animal's paw out of its body, or the
    // primitives' out of its ball.
    let arms_of = |pivot: Entity, children: &Children| {
        let primitive = |shoulders: [Vec3; 2], size: f32| ArmsOf {
            shoulders,
            size,
            scale: model_scale * size,
            hand_length: assets.paw_length,
            arm: &assets.arm,
            hand: &assets.paw,
            material: &assets.paw_material,
            wings: [None; 2],
        };
        let Some((driver, at, below)) = children.iter().find_map(|c| drivers.get(c).ok()) else {
            return primitive([SHOULDER * Vec3::new(-1.0, 1.0, 1.0), SHOULDER], 1.0);
        };
        let size = driver.size;
        let animal = below
            .into_iter()
            .flat_map(|below| below.iter())
            .find_map(|c| animal_roots.get(c).ok())
            .and_then(|root| {
                let animal = animals.as_ref()?.get(root.character)?;
                let paw = animal.paw.as_ref()?;
                let body = transform_in(root.body?, pivot, &nodes)?;
                Some(ArmsOf {
                    shoulders: paw.shoulders(&body),
                    size,
                    scale: model_scale * size,
                    hand_length: paw.hand_length,
                    arm: &paw.arm,
                    hand: &paw.hand,
                    material: &animal.material,
                    wings: root.wings,
                })
            });
        animal.unwrap_or_else(|| {
            let ball = |x: f32| at.transform_point(BALL_SHOULDER * Vec3::new(x, 1.0, 1.0));
            primitive([ball(-1.0), ball(1.0)], size)
        })
    };
    for (arm, child_of, mut transform, mut visibility, mut mesh, mut material) in &mut arms {
        let Ok(paw) = paws.get(arm.body) else {
            continue;
        };
        let Ok((_, pivot, pivot_of, children)) = pivots.get(child_of.parent()) else {
            continue;
        };
        let of = arms_of(child_of.parent(), children);
        // The kart it goes for, in the pivot's frame.
        let target = match paw.motion {
            PawMotion::Holding { target, .. } => Some(target),
            _ => paw.swing.and_then(|s| s.target),
        };
        let other = target.and_then(shown).and_then(|world| {
            let root = roots.get(pivot_of.parent()).ok()?;
            let frame = root.mul_transform(*pivot);
            Some(frame.compute_affine().inverse().transform_point3(world))
        });
        // On the other driver's body, not inside it.
        let other = other.map(|at| {
            let from = on_side(of.shoulders, at.x);
            let toward = at - from;
            let length = toward.length();
            if length > 2.0 * LANDS_SHORT {
                at - toward * (LANDS_SHORT / length)
            } else {
                at
            }
        });
        let tip = paw_tip(paw, of.shoulders, of.size, other, now, max_length);
        // A bird's wing leaves its body while it is the arm (its arm part says so).
        if !arm.paw {
            for (i, wing) in of.wings.iter().enumerate() {
                let out = tip.is_some_and(|(side, _)| usize::from(side >= 0.0) == i);
                if let Some(mut shown) = wing.and_then(|w| wings.get_mut(w).ok()) {
                    shown.set_if_neq(if out {
                        Visibility::Hidden
                    } else {
                        Visibility::Inherited
                    });
                }
            }
        }
        let Some((side, tip)) = tip else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let wanted = if arm.paw { of.hand } else { of.arm };
        if mesh.0 != *wanted {
            mesh.0 = wanted.clone();
        }
        if material.0 != *of.material {
            material.0 = of.material.clone();
        }
        let (arm_at, hand_at) = arm_and_hand(
            on_side(of.shoulders, side),
            tip,
            side,
            of.scale,
            of.hand_length,
        );
        transform.set_if_neq(if arm.paw { hand_at } else { arm_at });
    }
}

/// The "pof" of a strike received: on the side the strike came from, at the driver's height.
fn pop_pofs(
    time: Res<Time>,
    paws: Query<&Paw>,
    roots: Query<&Transform, (With<InterpolatedFrom>, Without<PawPof>)>,
    pivots: Query<(&Transform, &ChildOf), (With<KartBodyPivot>, Without<PawPof>)>,
    mut pofs: Query<(&PawPof, &ChildOf, &mut Transform, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    for (pof, child_of, mut transform, mut visibility) in &mut pofs {
        let Ok(paw) = paws.get(pof.body) else {
            continue;
        };
        let t = paw.struck_ticks as f32 / crate::TICK_HZ as f32;
        let size = if paw.struck_count > 0 {
            pof_size(t)
        } else {
            0.0
        };
        if size <= 0.0 {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        visibility.set_if_neq(Visibility::Inherited);
        // The strike came from the other side of the push.
        let from = pivots
            .get(child_of.parent())
            .ok()
            .and_then(|(pivot, pivot_of)| {
                let root = roots.get(pivot_of.parent()).ok()?;
                let rotation = root.rotation * pivot.rotation;
                Some(rotation.inverse() * -paw.struck_side)
            })
            .unwrap_or(Vec3::X);
        transform.translation = from.with_y(0.0).normalize_or(Vec3::X) * 0.8 + Vec3::Y * 1.0;
        transform.rotation = Quat::from_rotation_y(-from.x.atan2(-from.z))
            * Quat::from_rotation_z(PI / 4.0 + now * 6.0);
        transform.scale = Vec3::splat(size);
    }
}

/// Off balance, the driver leans out of its kart; held, it turns toward its grabber (outraged).
/// Issue #130: the drivers' acting goes on top of it (`characters::ActingSystems`, after it).
pub(crate) fn pose_drivers(
    time: Res<Time>,
    paws: Query<(&Paw, &Transform)>,
    pivots: Query<&KartBodyPivot>,
    mut drivers: Query<(&ChildOf, &mut Transform), (With<DriverVisual>, Without<Paw>)>,
) {
    let now = time.elapsed_secs();
    for (child_of, mut transform) in &mut drivers {
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok((paw, body)) = paws.get(pivot.body) else {
            continue;
        };
        let lean = if paw.off_balance > 0 {
            -paw.off_balance_side * LEAN * (1.0 + 0.3 * (now * WINDMILL_SPEED).sin())
        } else {
            0.0
        };
        let turn = paw
            .held
            .and_then(|held| paws.get(held.by).ok())
            .map_or(0.0, |(_, grabber)| {
                let to = body.rotation.inverse() * (grabber.translation - body.translation);
                // Right of the kart (+X): turn right (negative yaw), at most a quarter turn.
                (-to.x.signum() * OUTRAGED_TURN).clamp(-OUTRAGED_TURN, OUTRAGED_TURN)
            });
        let rotation = Quat::from_rotation_y(turn) * Quat::from_rotation_z(lean);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
    }
}

/// The player's camera shakes when its kart is struck (GD § 14).
fn shake_the_player(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<InteractionTuning>,
    mut seen: Local<u32>,
    karts: Query<(Entity, &Paw), With<PlayerKart>>,
) {
    for (kart, paw) in &karts {
        if paw.struck_count > *seen {
            commands.entity(kart).insert(CameraShake::new(
                time.elapsed_secs(),
                tuning.struck_shake,
                tuning.struck_shake_time,
            ));
        }
        *seen = paw.struck_count;
    }
}

/// The HUD's paw gauge (GD § 14: "une patte qui se remplit"), under the item slots.
fn spawn_gauge(mut commands: Commands, hud: Query<Entity, With<HudRoot>>) {
    let Ok(hud) = hud.single() else {
        return;
    };
    commands.entity(hud).with_children(|hud| {
        hud.spawn((
            PawGauge,
            Node {
                position_type: PositionType::Absolute,
                left: px(30),
                // 12 px under the hand's slot (issue #102: bigger).
                top: px(SLOTS_TOP + SLOT_SIZES[0] + 12.0),
                width: px(GAUGE_SIZE),
                height: px(GAUGE_SIZE),
                border: UiRect::all(px(3)),
                border_radius: BorderRadius::MAX,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.06, 0.16, 0.6)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        ))
        .with_children(|gauge| {
            gauge.spawn((
                PawGaugeFill,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    bottom: px(0),
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(GAUGE_READY),
            ));
        });
    });
}

/// How full the paw gauge is: 1 when the button is ready, rising from 0 during the cooldown
/// (`cooldown` ticks left out of `full`).
pub fn gauge_fill(cooldown: u32, full: u32) -> f32 {
    if cooldown == 0 || full == 0 {
        1.0
    } else {
        1.0 - cooldown.min(full) as f32 / full as f32
    }
}

/// The gauge follows the player's paw.
fn fill_gauge(
    tuning: Res<InteractionTuning>,
    mut full: Local<u32>,
    karts: Query<&Paw, With<PlayerKart>>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<PawGaugeFill>>,
    mut gauges: Query<&mut Visibility, With<PawGauge>>,
) {
    let paw = karts.iter().next();
    for mut visibility in &mut gauges {
        visibility.set_if_neq(if paw.is_some() && tuning.enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let Some(paw) = paw else {
        return;
    };
    // The cooldown only goes down: its highest value is its length.
    if paw.cooldown == 0 {
        *full = 0;
    } else if paw.cooldown > *full {
        *full = paw.cooldown;
    }
    let fill = gauge_fill(paw.cooldown, *full);
    let color = if fill >= 1.0 {
        GAUGE_READY
    } else {
        GAUGE_LOADING
    };
    for (mut node, mut background) in &mut fills {
        let height = percent(100.0 * fill);
        if node.height != height {
            node.height = height;
        }
        background.set_if_neq(BackgroundColor(color));
    }
}

/// The arms shown, for the tests (issue #81): each one's kart and its length in the world (m),
/// NaN when its transform or its paw's is not finite.
#[cfg(test)]
pub(crate) fn shown_arms(world: &World) -> Vec<(Entity, f32)> {
    let Some(mut parts) = world.try_query::<(&PawArm, &Visibility, &GlobalTransform)>() else {
        return Vec::new();
    };
    let parts: Vec<(PawArm, GlobalTransform)> = parts
        .iter(world)
        .filter(|(_, visibility, _)| **visibility != Visibility::Hidden)
        .map(|(part, _, global)| (*part, *global))
        .collect();
    parts
        .iter()
        .filter(|(part, _)| !part.paw)
        .map(|(arm, global)| {
            let length = global.affine().transform_vector3(Vec3::Y).length();
            let finite = global.translation().is_finite()
                && parts
                    .iter()
                    .filter(|(paw, _)| paw.paw && paw.body == arm.body)
                    .all(|(_, paw)| paw.translation().is_finite());
            (arm.body, if finite { length } else { f32::NAN })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactions::Swing;

    fn tick_seconds(seconds: f32) -> u32 {
        (seconds * crate::TICK_HZ as f32).round() as u32
    }

    /// A medium driver's shoulders (its left one, then its right one).
    const SHOULDERS: [Vec3; 2] = [Vec3::new(-0.4, 0.62, 0.15), SHOULDER];

    /// [`paw_tip`] for a medium driver, with the default longest arm.
    fn tip(paw: &Paw, other: Option<Vec3>, now: f32) -> Option<(f32, Vec3)> {
        paw_tip(
            paw,
            SHOULDERS,
            1.0,
            other,
            now,
            max_arm_length(&InteractionTuning::default()),
        )
    }

    /// GD § 15: the paw shows as soon as the button is down; it swings out to the kart struck,
    /// comes back and goes down; it stretches to the kart held; it windmills off balance.
    #[test]
    fn the_paw_rises_at_once_swings_and_comes_back() {
        let mut paw = Paw::default();
        assert_eq!(tip(&paw, None, 0.0), None, "down at rest");
        paw.motion = PawMotion::Raised { ticks: 1 };
        let (_, raised) = tip(&paw, None, 0.0).unwrap();
        assert!(raised.y > SHOULDER.y + 0.3, "raised: {raised}");

        let target = Vec3::new(2.5, 0.6, 0.0);
        paw.motion = PawMotion::Idle;
        paw.swing = Some(Swing {
            kind: SwingKind::Strike,
            side: 1.0,
            target: None,
            ticks: tick_seconds(SWING_TIME),
        });
        let (side, out) = tip(&paw, Some(target), 0.0).unwrap();
        assert_eq!(side, 1.0);
        assert!(out.distance(target) < 0.05, "out at the target: {out}");
        paw.swing = Some(Swing {
            ticks: tick_seconds(SWING_TIME + RETURN_TIME + 0.02),
            ..paw.swing.unwrap()
        });
        assert_eq!(tip(&paw, Some(target), 0.0), None, "back and down");

        paw.swing = None;
        paw.motion = PawMotion::Holding {
            target: Entity::PLACEHOLDER,
            ticks: 10,
            rest: 2.0,
            side: -1.0,
        };
        let held = Vec3::new(-3.0, 0.6, 1.0);
        assert_eq!(tip(&paw, Some(held), 0.0), Some((-1.0, held)));

        paw.motion = PawMotion::Idle;
        paw.lose_balance(30, -1.0);
        let spots: Vec<Vec3> = (0..10)
            .map(|i| tip(&paw, None, i as f32 * 0.03).unwrap().1)
            .collect();
        assert!(spots.iter().all(|p| p.x < 0.0), "on the left");
        assert!(
            spots.windows(2).any(|w| w[0].distance(w[1]) > 0.1),
            "windmilling"
        );
    }

    /// Issue #81: no arm toward a kart that is not shown, none longer than the longest tether
    /// (about 8 m: what Max saw went off the screen), none with a NaN. A tether stretched to its
    /// break still shows.
    #[test]
    fn an_arm_without_a_target_in_reach_is_hidden() {
        let longest = max_arm_length(&InteractionTuning::default());
        let holding = Paw {
            motion: PawMotion::Holding {
                target: Entity::PLACEHOLDER,
                ticks: 10,
                rest: 4.0,
                side: 1.0,
            },
            ..default()
        };
        assert_eq!(tip(&holding, None, 0.0), None, "the kart held is not shown");
        let stretched = SHOULDER + Vec3::X * 6.5;
        assert_eq!(tip(&holding, Some(stretched), 0.0), Some((1.0, stretched)));
        let far = SHOULDER + Vec3::new(30.0, 0.0, -40.0);
        assert_eq!(tip(&holding, Some(far), 0.0), None, "a broken tether");
        let along = |length: f32| SHOULDER + Vec3::Z * length;
        assert!(tip(&holding, Some(along(longest * 0.999)), 0.0).is_some());
        assert_eq!(tip(&holding, Some(along(longest * 1.01)), 0.0), None);
        assert_eq!(tip(&holding, Some(Vec3::NAN), 0.0), None, "NaN");
        assert_eq!(tip(&holding, Some(Vec3::INFINITY), 0.0), None);
        // A strike or a throw at a kart far away: no arm either.
        let striking = Paw {
            swing: Some(Swing {
                kind: SwingKind::Throw,
                side: -1.0,
                target: None,
                ticks: tick_seconds(SWING_TIME * 0.5),
            }),
            ..default()
        };
        assert!(tip(&striking, Some(Vec3::new(-2.5, 0.6, 0.0)), 0.0).is_some());
        assert_eq!(tip(&striking, Some(Vec3::new(-200.0, 0.6, 0.0)), 0.0), None);
        // The arm itself and its hand: finite, even with its paw on the shoulder (or straight
        // up, along the back of the hand).
        for tip in [SHOULDER, SHOULDER + HAND_BACK] {
            for part in <[Transform; 2]>::from(arm_and_hand(SHOULDER, tip, 1.0, 0.62, 0.4)) {
                assert!(
                    part.translation.is_finite()
                        && part.rotation.is_normalized()
                        && part.scale.is_finite(),
                    "{part:?}"
                );
            }
        }
    }

    /// Issue #169: the arm goes from the shoulder to the wrist, the hand from the wrist on,
    /// centered on the tip (where the paw is: the kart held); both at the animal's scale, the arm
    /// stretched by its transform alone; the palm (the pads) toward the camera, a little up.
    #[test]
    fn the_arm_goes_from_the_shoulder_to_the_paw() {
        let (scale, hand_length) = (0.5, 0.4);
        let shoulder = Vec3::new(0.4, 0.6, 0.0);
        for (tip, side) in [
            (Vec3::new(2.4, 0.6, 0.0), 1.0),
            (Vec3::new(-3.0, 1.0, 2.0), -1.0),
            (Vec3::new(0.6, 1.2, 0.1), 1.0),
        ] {
            let (arm, hand) = arm_and_hand(shoulder, tip, side, scale, hand_length);
            let toward = (tip - shoulder).normalize();
            assert!(
                (arm.translation - shoulder).length() < 1e-5,
                "from the shoulder"
            );
            let wrist = arm.transform_point(Vec3::Y);
            let hand_wrist = hand.transform_point(Vec3::NEG_Y * hand_length / 2.0);
            assert!((wrist - hand_wrist).length() < 1e-4, "to the wrist");
            assert!(
                (hand.translation - tip).length() < 1e-5,
                "the hand on the tip"
            );
            assert!(
                (arm.scale.y - (tip.distance(shoulder) - scale * hand_length / 2.0)).abs() < 1e-4
            );
            assert_eq!((arm.scale.x, arm.scale.z), (scale, scale));
            assert_eq!(hand.scale, Vec3::splat(scale));
            assert!((arm.rotation * Vec3::Y - toward).length() < 1e-4);
            assert_eq!(arm.rotation, hand.rotation);
            let palm = hand.rotation * Vec3::NEG_Z;
            assert!(palm.dot(toward).abs() < 1e-4 && palm.z > 0.0, "{palm}");
        }
        // Sideways, the palm faces back (toward the chase camera), a little up: the camera
        // sees its pads.
        let (_, hand) = arm_and_hand(shoulder, Vec3::new(2.4, 0.6, 0.0), 1.0, scale, hand_length);
        let palm = hand.rotation * Vec3::NEG_Z;
        assert!(palm.z > 0.9 && palm.y > 0.3, "{palm}");
    }

    /// Issue #169: the paw comes out of the shoulders it is given (its driver's), on its side,
    /// and a small driver raises it and windmills less far than a big one; the reach (the kart
    /// struck, the one held) does not change.
    #[test]
    fn the_paw_leaves_the_driver_s_own_shoulders() {
        let small = [Vec3::new(-0.22, 0.33, 0.1), Vec3::new(0.22, 0.33, 0.1)];
        let max = max_arm_length(&InteractionTuning::default());
        let raised = Paw {
            motion: PawMotion::Raised { ticks: 1 },
            aim: -1.0,
            ..default()
        };
        let (side, at) = paw_tip(&raised, small, 0.75, None, 0.0, max).unwrap();
        assert_eq!(side, -1.0);
        assert!((at - small[0] - Vec3::new(-0.15, 0.375, 0.0375)).length() < 1e-5);
        let (_, big) = paw_tip(&raised, SHOULDERS, 1.3, None, 0.0, max).unwrap();
        assert!(big.y - SHOULDERS[0].y > at.y - small[0].y);
        // Back with a boing: on its own shoulder.
        let mut back = Paw {
            swing: Some(Swing {
                kind: SwingKind::Whiff,
                side: 1.0,
                target: None,
                ticks: tick_seconds(SWING_TIME + RETURN_TIME * 0.9),
            }),
            ..default()
        };
        let (_, home) = paw_tip(&back, small, 0.75, None, 0.0, max).unwrap();
        assert!(home.distance(small[1]) < 0.1, "{home}");
        // The kart held is where it is, whoever holds it.
        back.swing = None;
        back.motion = PawMotion::Holding {
            target: Entity::PLACEHOLDER,
            ticks: 10,
            rest: 2.0,
            side: 1.0,
        };
        let held = Vec3::new(3.0, 0.6, 0.5);
        assert_eq!(
            paw_tip(&back, small, 0.75, Some(held), 0.0, max),
            Some((1.0, held))
        );
    }

    #[test]
    fn the_pof_pops_then_vanishes_and_the_gauge_fills_up() {
        assert_eq!(pof_size(-0.1), 0.0);
        assert_eq!(pof_size(POF_TIME), 0.0);
        let sizes: Vec<f32> = (0..10)
            .map(|i| pof_size(i as f32 * POF_TIME / 10.0))
            .collect();
        let biggest = sizes.iter().cloned().fold(0.0, f32::max);
        assert!(
            biggest > POF_SIZE * 0.9 && sizes[9] < biggest * 0.6,
            "{sizes:?}"
        );
        assert_eq!(gauge_fill(0, 90), 1.0);
        assert_eq!(gauge_fill(90, 90), 0.0);
        assert!((gauge_fill(45, 90) - 0.5).abs() < 1e-6);
        assert_eq!(gauge_fill(10, 0), 1.0);
    }
}
