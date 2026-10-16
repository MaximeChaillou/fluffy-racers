//! What the special environments look like on a kart (GD § 10, issue #20): primitives only,
//! shared by every kart (the same few meshes and materials), shown from the kart's
//! [`KartEnvironment`] (online, the other peers' karts too: from their states).
//!
//! - **The glider**: a puffy pillowcase on two struts above the driver (the wing of GD § 9, the
//!   « taie d'oreiller »), folded into the kart until it opens: it pops open in
//!   `KartTuning::environment.wing_open_seconds` and folds back the same way on landing.
//! - **Under water**: bubbles rise from the kart and stay behind it; going in or coming out, a
//!   splash of droplets.
//!
//! No physics: the visuals only read the kart. The wing and the bubbles are made the first time
//! a kart glides or goes under water, not with the kart: nothing more to draw or move on the
//! circuits without a glide ramp or a lake (and the karts there are exactly those of before
//! issue #20, down to the order of their entities).

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::KartTuning;
use super::environment::KartEnvironment;
use super::visual::{KartBodyPivot, KartVisualAssets};
use crate::interpolation::InterpolatedFrom;

/// Bubbles (and droplets) per kart.
const BUBBLES: usize = 8;
/// A bubble rises and drifts back for this long (s), then starts again.
const BUBBLE_SECONDS: f32 = 1.1;
/// How high it rises (m) and how far back it stays (m) in that time...
const BUBBLE_RISE: f32 = 2.2;
const BUBBLE_DRIFT: f32 = 3.0;
/// ...and how big it is (radius, m), growing as it rises.
const BUBBLE_SIZE: [f32; 2] = [0.07, 0.17];
/// A splash lasts this long (s): droplets thrown up and out around the kart, falling back.
const SPLASH_SECONDS: f32 = 0.6;
const SPLASH_SPEED: f32 = 4.0;
const SPLASH_SPREAD: f32 = 2.5;
const SPLASH_SIZE: f32 = 0.16;

/// The glider's sail: an ellipsoid (a puffy pillowcase), its center and half sizes in the kart's
/// frame (the front is -Z).
const SAIL_CENTER: Vec3 = Vec3::new(0.0, 2.05, 0.15);
const SAIL_HALF: Vec3 = Vec3::new(2.2, 0.13, 0.8);
/// Its struts, from the body's sides up to under the sail.
const STRUT_FOOT: Vec3 = Vec3::new(0.5, 0.25, 0.25);
const STRUT_TOP: Vec3 = Vec3::new(1.25, 1.95, 0.15);
const STRUT_RADIUS: f32 = 0.05;

/// The shared meshes and materials of the environments (in `KartVisualAssets`).
#[derive(Clone, Debug)]
pub struct EnvironmentLook {
    sail: Handle<Mesh>,
    strut: Handle<Mesh>,
    bubble: Handle<Mesh>,
    sail_material: Handle<StandardMaterial>,
    strut_material: Handle<StandardMaterial>,
    bubble_material: Handle<StandardMaterial>,
}

/// The glider's wing under a kart's pivot (issue #20): `open` from 0 (folded, hidden) to 1.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Wing {
    pub open: f32,
}

/// A bubble under a kart visual's root (issue #20), `index` among its [`BUBBLES`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bubble {
    pub index: usize,
}

/// On a kart visual's root (issue #20): whether its kart was under water on the last frame, and
/// when it last went in or out (s, for the splash); whether its bubbles were made (the first time
/// it went under water).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterEffects {
    pub underwater: bool,
    pub splash_at: Option<f32>,
    pub bubbles: bool,
}

impl EnvironmentLook {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        Self {
            sail: meshes.add(Sphere::new(1.0).mesh().uv(18, 9)),
            strut: meshes.add(Cylinder::new(STRUT_RADIUS, 1.0)),
            bubble: meshes.add(Sphere::new(1.0).mesh().uv(8, 5)),
            sail_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.98, 0.95, 0.88),
                perceptual_roughness: 0.9,
                ..default()
            }),
            strut_material: materials.add(Color::srgb(0.40, 0.28, 0.20)),
            bubble_material: materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.95, 1.0),
                unlit: true,
                ..default()
            }),
        }
    }

    /// The wing (folded) under a kart's pivot.
    pub fn spawn_wing(&self, pivot: &mut ChildSpawnerCommands) {
        pivot
            .spawn((
                Wing::default(),
                Transform::from_scale(Vec3::ZERO),
                Visibility::Hidden,
            ))
            .with_children(|wing| {
                wing.spawn((
                    Mesh3d(self.sail.clone()),
                    MeshMaterial3d(self.sail_material.clone()),
                    Transform::from_translation(SAIL_CENTER).with_scale(SAIL_HALF),
                ));
                for side in [-1.0f32, 1.0] {
                    let foot = STRUT_FOOT * Vec3::new(side, 1.0, 1.0);
                    let top = STRUT_TOP * Vec3::new(side, 1.0, 1.0);
                    let along = top - foot;
                    wing.spawn((
                        Mesh3d(self.strut.clone()),
                        MeshMaterial3d(self.strut_material.clone()),
                        Transform::from_translation((foot + top) / 2.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Y, along.normalize()))
                            .with_scale(Vec3::new(1.0, along.length(), 1.0)),
                        NotShadowCaster,
                    ));
                }
            });
    }

    /// The bubbles (hidden) under a kart visual's root.
    pub fn spawn_bubbles(&self, root: &mut ChildSpawnerCommands) {
        for index in 0..BUBBLES {
            root.spawn((
                Bubble { index },
                Mesh3d(self.bubble.clone()),
                MeshMaterial3d(self.bubble_material.clone()),
                Transform::from_scale(Vec3::ZERO),
                Visibility::Hidden,
                NotShadowCaster,
            ));
        }
    }
}

/// How open a wing is (0 to 1) after `dt`, toward open while gliding, folded otherwise, in
/// `seconds`.
pub fn wing_opening(open: f32, gliding: bool, dt: f32, seconds: f32) -> f32 {
    let step = if seconds > 0.0 { dt / seconds } else { 1.0 };
    let target = if gliding { 1.0 } else { 0.0 };
    open + (target - open).clamp(-step, step)
}

/// The wing's scale when `open` (0 to 1): it pops a little past its size, then settles.
pub fn wing_scale(open: f32) -> f32 {
    let t = open.clamp(0.0, 1.0);
    // Ease out with a small overshoot (a pop).
    let back = 1.7;
    let u = t - 1.0;
    1.0 + u * u * ((back + 1.0) * u + back)
}

/// Where bubble `index` is (in the kart's frame) and how big, `t` seconds into its life under
/// water: it rises and stays behind the kart, each bubble on its own beat.
pub fn bubble_pose(index: usize, t: f32) -> (Vec3, f32) {
    let phase = (t / BUBBLE_SECONDS + index as f32 * 0.37).fract();
    let x = ((index as f32 * 2.399).sin()) * 0.6;
    let position = Vec3::new(
        x,
        0.2 + BUBBLE_RISE * phase,
        0.9 + BUBBLE_DRIFT * phase * phase,
    );
    let size = BUBBLE_SIZE[0] + (BUBBLE_SIZE[1] - BUBBLE_SIZE[0]) * phase;
    (position, size)
}

/// Where droplet `index` of a splash is (in the kart's frame) and how big, `t` seconds after the
/// kart went in or out (None once the splash is over): thrown up and out in a ring, falling back.
pub fn splash_pose(index: usize, t: f32) -> Option<(Vec3, f32)> {
    if !(0.0..SPLASH_SECONDS).contains(&t) {
        return None;
    }
    let angle = index as f32 / BUBBLES as f32 * TAU;
    let out = 0.5 + SPLASH_SPREAD * t / SPLASH_SECONDS;
    let up = 0.2 + SPLASH_SPEED * t - 9.0 * t * t;
    let size = SPLASH_SIZE * (1.0 - t / SPLASH_SECONDS).max(0.2);
    Some((
        Vec3::new(angle.cos() * out * 1.2, up.max(0.0), angle.sin() * out),
        size,
    ))
}

/// Each wing opens while its kart glides and folds after (issue #20). A kart's wing is fitted the
/// first time it glides (folded: it opens from the next frame).
pub(super) fn show_wings(
    mut commands: Commands,
    time: Res<Time>,
    (tuning, assets): (Option<Res<KartTuning>>, Option<Res<KartVisualAssets>>),
    karts: Query<&KartEnvironment>,
    mut pivots: Query<(Entity, &mut KartBodyPivot)>,
    mut wings: Query<(&mut Wing, &ChildOf, &mut Transform, &mut Visibility)>,
) {
    if let Some(assets) = assets.as_deref() {
        for (entity, mut pivot) in &mut pivots {
            if !pivot.wing && karts.get(pivot.body).is_ok_and(|e| e.gliding) {
                pivot.wing = true;
                commands
                    .entity(entity)
                    .with_children(|pivot| assets.environment.spawn_wing(pivot));
            }
        }
    }
    let seconds = tuning.map_or(0.3, |t| t.environment.wing_open_seconds);
    let dt = time.delta_secs();
    for (mut wing, child_of, mut transform, mut visibility) in &mut wings {
        let gliding = pivots
            .get(child_of.parent())
            .ok()
            .and_then(|(_, pivot)| karts.get(pivot.body).ok())
            .is_some_and(|environment| environment.gliding);
        let open = wing_opening(wing.open, gliding, dt, seconds);
        let scale = Vec3::splat(wing_scale(open));
        if open == wing.open && transform.scale == scale {
            continue;
        }
        wing.open = open;
        visibility.set_if_neq(if open > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        transform.scale = scale;
    }
}

/// Under water, each kart's bubbles rise behind it; in and out, a splash (issue #20). A kart's
/// bubbles are made the first time it goes under water.
pub(super) fn show_water_effects(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<KartVisualAssets>>,
    karts: Query<&KartEnvironment>,
    mut roots: Query<(Entity, &InterpolatedFrom, &mut WaterEffects)>,
    mut bubbles: Query<(&Bubble, &ChildOf, &mut Transform, &mut Visibility)>,
) {
    let now = time.elapsed_secs();
    for (entity, InterpolatedFrom(body), mut effects) in &mut roots {
        let underwater = karts.get(*body).is_ok_and(|e| e.underwater);
        if underwater
            && !effects.bubbles
            && let Some(assets) = assets.as_deref()
        {
            effects.bubbles = true;
            commands
                .entity(entity)
                .with_children(|root| assets.environment.spawn_bubbles(root));
        }
        if effects.underwater != underwater {
            effects.underwater = underwater;
            effects.splash_at = Some(now);
        }
    }
    for (bubble, child_of, mut transform, mut visibility) in &mut bubbles {
        let Ok((_, _, effects)) = roots.get(child_of.parent()) else {
            continue;
        };
        let splash = effects
            .splash_at
            .and_then(|at| splash_pose(bubble.index, now - at));
        let pose = splash.or_else(|| effects.underwater.then(|| bubble_pose(bubble.index, now)));
        match pose {
            Some((position, size)) => {
                visibility.set_if_neq(Visibility::Inherited);
                transform.translation = position;
                transform.scale = Vec3::splat(size);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wing pops open in its time, a little past its size, and folds back the same way.
    #[test]
    fn the_wing_pops_open_and_folds() {
        let mut open = 0.0;
        let mut frames = 0;
        while open < 1.0 {
            open = wing_opening(open, true, 1.0 / 60.0, 0.3);
            frames += 1;
            assert!(frames <= 19);
        }
        assert_eq!(frames, 18);
        assert_eq!(wing_scale(0.0), 0.0);
        assert!((wing_scale(1.0) - 1.0).abs() < 1e-6);
        assert!(
            (0..100).any(|i| wing_scale(i as f32 / 100.0) > 1.05),
            "a pop"
        );
        let folded = (0..18).fold(1.0, |open, _| wing_opening(open, false, 1.0 / 60.0, 0.3));
        assert!(folded.abs() < 1e-5);
        assert_eq!(wing_opening(0.0, true, 0.1, 0.0), 1.0);
    }

    /// Bubbles rise and stay behind the kart, within a small box; a splash throws droplets up and
    /// out, then ends.
    #[test]
    fn bubbles_rise_and_splashes_end() {
        for index in 0..BUBBLES {
            for k in 0..50 {
                let (p, size) = bubble_pose(index, k as f32 * 0.05);
                assert!(p.y >= 0.2 && p.y <= 0.2 + BUBBLE_RISE + 1e-4);
                assert!(p.z >= 0.9 && p.z <= 0.9 + BUBBLE_DRIFT + 1e-4, "behind");
                assert!(p.x.abs() <= 0.6 && size > 0.0 && size <= BUBBLE_SIZE[1]);
            }
            let (start, _) = splash_pose(index, 0.0).unwrap();
            let (later, _) = splash_pose(index, 0.15).unwrap();
            assert!(later.y > start.y && later.xz().length() > start.xz().length());
            assert!(splash_pose(index, SPLASH_SECONDS).is_none());
            assert!(splash_pose(index, -0.1).is_none());
        }
    }
}
