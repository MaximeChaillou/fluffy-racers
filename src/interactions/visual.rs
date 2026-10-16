//! What the interactions look like (GD § 14 and 15, issue #34), in primitives (the struck
//! driver's mini-ragdoll is the soft characters', `characters::wobble`, issue #12). Every kart's
//! visual gets, once:
//!
//! - an **arm** (a capsule) ending in a **paw** (a ball), hidden at rest. It rises as soon as the
//!   Interaction button is down, swings at the kart struck (or into thin air, then windmills off
//!   balance with the driver leaning out), reaches out while grabbing, and during a tether
//!   stretches from the driver to the kart held: the elastic band. It comes back with a "boing"
//!   when it lets go. The kart held turns its driver toward the grabber, outraged;
//! - a **"pof"** (a flat yellow diamond) that pops on the side of a kart struck.
//!
//! The player's camera shakes when its kart is struck (`camera::CameraShake`), and the HUD gets
//! the paw's recharge: a gauge under the item slots that fills up (GD § 14).
//!
//! One mesh and one material per part, shared by every kart; nothing is spawned or despawned
//! while racing. Everything is read from the simulation (`Paw`, interpolated poses): nothing
//! here touches it.

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::logic::{Paw, PawMotion, SwingKind};
use super::tuning::InteractionTuning;
use crate::camera::CameraShake;
use crate::interpolation::{InterpolatedFrom, InterpolationSystems, PhysicsPose};
use crate::kart::{DriverVisual, KartBodyPivot, PlayerKart};
use crate::ui::HudRoot;

/// The arm's radius and the paw's (m).
const ARM_RADIUS: f32 = 0.07;
const PAW_RADIUS: f32 = 0.13;
/// Where the arms start on a kart's pivot: the shoulders of a medium driver, to its sides.
const SHOULDER: Vec3 = Vec3::new(0.4, 0.62, 0.15);
/// Where the other kart's driver is, above its body's center (m): what a paw reaches for.
const DRIVER_HEIGHT: f32 = 0.6;
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

/// A part of a kart's arm: the arm itself (a capsule) or the paw at its end (a ball), under the
/// kart visual's pivot.
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

/// Meshes and materials shared by every kart's arm and "pof".
#[derive(Resource, Clone, Debug)]
pub struct PawVisualAssets {
    arm: Handle<Mesh>,
    paw: Handle<Mesh>,
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
                (pose_arms, pop_pofs, pose_drivers)
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
    commands.insert_resource(PawVisualAssets {
        // A capsule along Y, 1 m between its caps: scaled to the arm's length.
        arm: meshes.add(Capsule3d::new(ARM_RADIUS, 1.0)),
        paw: meshes.add(Sphere::new(PAW_RADIUS)),
        // A flat square, turned into a diamond.
        pof: meshes.add(Cuboid::new(1.0, 1.0, 0.08)),
        paw_material: materials.add(Color::srgb(0.98, 0.9, 0.78)),
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

/// Where a kart's paw is, in its pivot's frame (the shoulder is [`SHOULDER`] on the side): `None`
/// when the arm is down (hidden). `other`: where the kart it goes for is (its driver), in the
/// same frame; `t`: time since the swing (s); `now`: the clock, for the wobbles.
pub fn paw_tip(paw: &Paw, other: Option<Vec3>, now: f32) -> Option<(f32, Vec3)> {
    let shoulder = |side: f32| SHOULDER * Vec3::new(side, 1.0, 1.0);
    let seconds = |ticks: u32| ticks as f32 / crate::TICK_HZ as f32;
    match paw.motion {
        // Raised as soon as the button is down (GD § 15), on the side it will strike.
        PawMotion::Raised { .. } => {
            let side = side_of(paw.aim);
            return Some((side, shoulder(side) + Vec3::new(side * 0.2, 0.5, 0.05)));
        }
        // Reaching out, the paw groping around.
        PawMotion::Reaching { ticks } => {
            let side = side_of(paw.aim);
            let grope = (seconds(ticks) * 20.0).sin();
            let out = Vec3::new(side * (1.1 + 0.3 * grope), 0.05, -0.3);
            return Some((side, shoulder(side) + out));
        }
        // The tether: stretched to the kart held.
        PawMotion::Holding { side, .. } => {
            let tip = other.unwrap_or(shoulder(side) + Vec3::X * side * 2.0);
            return Some((side, tip));
        }
        _ => {}
    }
    // Off balance: the arm windmills.
    if paw.off_balance > 0 {
        let side = paw.off_balance_side;
        let angle = now * WINDMILL_SPEED;
        let circle = Vec3::new(side * angle.cos().abs(), angle.sin(), 0.0) * WINDMILL_REACH;
        return Some((side, shoulder(side) + Vec3::X * side * 0.2 + circle));
    }
    // A swing: out to its target (or into thin air), then back with a "boing".
    let swing = paw.swing?;
    let t = seconds(swing.ticks);
    let side = swing.side;
    let start = shoulder(side) + Vec3::new(side * 0.2, 0.5, 0.05);
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

/// The arm's transform from `shoulder` to `tip` (a capsule 1 m long along Y, scaled).
pub fn arm_transform(shoulder: Vec3, tip: Vec3) -> Transform {
    let along = tip - shoulder;
    let length = along.length().max(0.05);
    Transform::from_translation((shoulder + tip) * 0.5)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, along / length))
        .with_scale(Vec3::new(1.0, length, 1.0))
}

/// The "pof"'s size `t` s after a strike (0 when gone): pops up, then shrinks.
pub fn pof_size(t: f32) -> f32 {
    if !(0.0..POF_TIME).contains(&t) {
        return 0.0;
    }
    let x = t / POF_TIME;
    POF_SIZE * (PI * x.sqrt()).sin()
}

/// Every kart's arm and paw.
fn pose_arms(
    time: Res<Time>,
    fixed_time: Res<Time<Fixed>>,
    paws: Query<&Paw>,
    poses: Query<&PhysicsPose>,
    roots: Query<&Transform, (With<InterpolatedFrom>, Without<PawArm>)>,
    pivots: Query<(&Transform, &ChildOf), (With<KartBodyPivot>, Without<PawArm>)>,
    mut arms: Query<(&PawArm, &ChildOf, &mut Transform, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    let alpha = fixed_time.overstep_fraction();
    for (arm, child_of, mut transform, mut visibility) in &mut arms {
        let Ok(paw) = paws.get(arm.body) else {
            continue;
        };
        // The kart it goes for, in the pivot's frame.
        let target = match paw.motion {
            PawMotion::Holding { target, .. } => Some(target),
            _ => paw.swing.and_then(|s| s.target),
        };
        let other = target
            .and_then(|t| poses.get(t).ok())
            .zip(pivots.get(child_of.parent()).ok())
            .and_then(|(pose, (pivot, pivot_of))| {
                let root = roots.get(pivot_of.parent()).ok()?;
                let world = pose.interpolate(alpha).translation + Vec3::Y * DRIVER_HEIGHT;
                let frame = root.mul_transform(*pivot);
                Some(frame.compute_affine().inverse().transform_point3(world))
            });
        let Some((side, tip)) = paw_tip(paw, other, now) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let shoulder = SHOULDER * Vec3::new(side, 1.0, 1.0);
        *transform = if arm.paw {
            Transform::from_translation(tip)
        } else {
            arm_transform(shoulder, tip)
        };
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
fn pose_drivers(
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
                top: px(112),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactions::Swing;

    fn tick_seconds(seconds: f32) -> u32 {
        (seconds * crate::TICK_HZ as f32).round() as u32
    }

    /// GD § 15: the paw shows as soon as the button is down; it swings out to the kart struck,
    /// comes back and goes down; it stretches to the kart held; it windmills off balance.
    #[test]
    fn the_paw_rises_at_once_swings_and_comes_back() {
        let mut paw = Paw::default();
        assert_eq!(paw_tip(&paw, None, 0.0), None, "down at rest");
        paw.motion = PawMotion::Raised { ticks: 1 };
        let (_, raised) = paw_tip(&paw, None, 0.0).unwrap();
        assert!(raised.y > SHOULDER.y + 0.3, "raised: {raised}");

        let target = Vec3::new(2.5, 0.6, 0.0);
        paw.motion = PawMotion::Idle;
        paw.swing = Some(Swing {
            kind: SwingKind::Strike,
            side: 1.0,
            target: None,
            ticks: tick_seconds(SWING_TIME),
        });
        let (side, out) = paw_tip(&paw, Some(target), 0.0).unwrap();
        assert_eq!(side, 1.0);
        assert!(out.distance(target) < 0.05, "out at the target: {out}");
        paw.swing = Some(Swing {
            ticks: tick_seconds(SWING_TIME + RETURN_TIME + 0.02),
            ..paw.swing.unwrap()
        });
        assert_eq!(paw_tip(&paw, Some(target), 0.0), None, "back and down");

        paw.swing = None;
        paw.motion = PawMotion::Holding {
            target: Entity::PLACEHOLDER,
            ticks: 10,
            rest: 2.0,
            side: -1.0,
        };
        let held = Vec3::new(-3.0, 0.6, 1.0);
        assert_eq!(paw_tip(&paw, Some(held), 0.0), Some((-1.0, held)));

        paw.motion = PawMotion::Idle;
        paw.lose_balance(30, -1.0);
        let spots: Vec<Vec3> = (0..10)
            .map(|i| paw_tip(&paw, None, i as f32 * 0.03).unwrap().1)
            .collect();
        assert!(spots.iter().all(|p| p.x < 0.0), "on the left");
        assert!(
            spots.windows(2).any(|w| w[0].distance(w[1]) > 0.1),
            "windmilling"
        );
    }

    #[test]
    fn the_arm_goes_from_the_shoulder_to_the_paw() {
        let shoulder = Vec3::new(0.4, 0.6, 0.0);
        let tip = Vec3::new(2.4, 0.6, 0.0);
        let arm = arm_transform(shoulder, tip);
        assert!((arm.translation - Vec3::new(1.4, 0.6, 0.0)).length() < 1e-5);
        assert!((arm.scale.y - 2.0).abs() < 1e-5);
        assert!((arm.rotation * Vec3::Y - Vec3::X).length() < 1e-4);
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
