//! Chase camera (issue #2): behind the local player's kart, smoothed by critically damped
//! springs so it never shakes, whatever the frame rate.
//!
//! It follows the *interpolated* visual of the kart (see `crate::interpolation`), not the
//! physics body, so it is as smooth at 30 fps as at 144 fps.
//!
//! **Shake** (GD § 14, issue #34): a [`CameraShake`] on the followed kart (a paw strike
//! received) moves the view for a moment, on top of the chase.
//!
//! **Looking behind** (GD § 2 and 13, issue #17): while the kart's `look_back` button is held (C,
//! or X / Y on a gamepad), the camera cuts to the front of the kart and looks back, like MK8DX.
//! The chase rig keeps running underneath: on release the camera is back exactly where the
//! chase camera is, its springs never interrupted. The kart itself ignores the button (only an
//! item thrown at that moment goes backwards, issue #16): the view stays out of the simulation.
//!
//! **Under water** (GD § 10, issue #20): a blue tint covers the screen while the camera is under
//! the water of a lake (`underwater.rs`).
//!
//! **Anti-gravity** (GD § 10, issue #78): the camera's up follows the kart's
//! (`kart::KartEnvironment::up`, the road's normal on a wall) through a spring
//! (`CameraTuning::up_frequency`): it rolls with the road, smoothly, and is exactly the world's up
//! again once out of the zone.

mod rig;
mod underwater;

pub use rig::{CameraRig, RigInput};
pub(crate) use underwater::tint_under_water;
pub use underwater::{UNDERWATER_TINT, UnderwaterTint};

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_rapier3d::prelude::*;

use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{Boost, KartEnvironment, KartInput, KartState, KartTuning};

/// All the camera knobs, with their default values (our own starting points, à régler).
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct CameraTuning {
    /// Distance behind the kart (m).
    pub distance: f32,
    /// Height above the kart (m).
    pub height: f32,
    /// The camera looks at a point this far in front of the kart (m).
    pub look_ahead: f32,
    /// Height of the look-at point above the kart (m).
    pub look_height: f32,
    /// Anticipation of turns: the look-at point shifts toward the inside of the turn by this
    /// many meters per rad/s of yaw rate.
    pub turn_lead: f32,
    /// Stiffness of the spring that swings the camera behind the kart in turns
    /// (natural frequency, rad/s). Lower = lazier swing.
    pub swing_frequency: f32,
    /// Stiffness of the vertical spring (bumps, jumps, landings).
    pub vertical_frequency: f32,
    /// Stiffness of the spring on the look-at point.
    pub look_frequency: f32,
    /// Vertical field of view at a standstill (degrees).
    pub base_fov: f32,
    /// Field of view at max speed (degrees): it widens with speed (GD § 14).
    pub max_speed_fov: f32,
    /// Extra field of view while boosting (degrees).
    pub boost_fov: f32,
    /// How fast the field of view follows (1/s).
    pub fov_response: f32,
    /// Side shift during a drift (m), toward the outside of the turn (hook for issue #4).
    pub drift_side_offset: f32,
    /// The camera never goes closer than this to the ground or a wall (m).
    pub clearance: f32,
    /// Looking behind (issue #17): the camera sits this far in front of the kart (m)...
    pub look_back_distance: f32,
    /// ...this high above it (m)...
    pub look_back_height: f32,
    /// ...and looks at a point this far behind it (m).
    pub look_back_target: f32,
    /// Issue #78: stiffness of the spring that turns the camera's up toward the kart's in
    /// anti-gravity (natural frequency, rad/s): no jolt where the road twists, nor at the start or
    /// the end of a zone.
    pub up_frequency: f32,
}

impl Default for CameraTuning {
    fn default() -> Self {
        Self {
            distance: 6.0,
            height: 2.4,
            look_ahead: 4.0,
            look_height: 0.8,
            turn_lead: 1.2,
            swing_frequency: 10.0,
            vertical_frequency: 8.0,
            look_frequency: 12.0,
            base_fov: 60.0,
            max_speed_fov: 70.0,
            boost_fov: 6.0,
            fov_response: 4.0,
            drift_side_offset: 1.0,
            clearance: 0.4,
            look_back_distance: 4.5,
            look_back_height: 2.0,
            look_back_target: 10.0,
            up_frequency: 6.0,
        }
    }
}

/// Camera that chases a kart. `target` is the kart's visual entity (interpolated pose).
#[derive(Component, Clone, Debug)]
pub struct ChaseCamera {
    pub target: Entity,
    pub rig: CameraRig,
}

impl ChaseCamera {
    pub fn new(target: Entity) -> Self {
        Self {
            target,
            rig: CameraRig::default(),
        }
    }
}

/// A still view instead of the chase (issue #19: the podium of a Grand Prix): the camera stands at
/// `eye` and looks at `look_at` with a vertical field of view of `fov_degrees`, as long as this
/// resource is there. The chase rig waits underneath, untouched.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct FixedView {
    pub eye: Vec3,
    pub look_at: Vec3,
    pub fov_degrees: f32,
}

/// A short screen shake (GD § 14: "un léger tremblement d'écran sur les chocs"), put on a kart
/// body by what shakes it (issue #34: a paw strike or a throw received). The chase camera that
/// follows this kart moves by [`shake_offset`]; its springs never see it, so the chase goes on
/// exactly as without it once the shake is over.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraShake {
    /// When it started (s, the frames' `Time::elapsed_secs`)...
    pub started: f32,
    /// ...how far it moves the camera at first (m)...
    pub strength: f32,
    /// ...and how long it lasts (s).
    pub duration: f32,
}

impl CameraShake {
    /// A shake starting at `now`.
    pub fn new(now: f32, strength: f32, duration: f32) -> Self {
        Self {
            started: now,
            strength,
            duration,
        }
    }

    /// How far it moves the camera at `now` (m, world).
    pub fn offset(&self, now: f32) -> Vec3 {
        shake_offset(now - self.started, self.strength, self.duration)
    }
}

/// A shake `t` s after it started: a jolt of `strength` m at first, fading out over `duration`
/// s (zero before and after). Three unrelated frequencies, so it never looks like a regular
/// wobble.
pub fn shake_offset(t: f32, strength: f32, duration: f32) -> Vec3 {
    if !(0.0..duration).contains(&t) || strength <= 0.0 {
        return Vec3::ZERO;
    }
    let fade = 1.0 - t / duration;
    let wave = |hz: f32, phase: f32| (std::f32::consts::TAU * hz * t + phase).sin();
    Vec3::new(
        wave(23.0, 0.4),
        0.6 * wave(29.0, 1.3),
        0.5 * wave(17.0, 2.1),
    ) * strength
        * fade
        * fade
}

/// Side lean asked by the kart's state, from -1 (left) to 1 (right). Put it on a kart body:
/// [`lean_into_drifts`] writes it from the drift, and the camera shifts by
/// `CameraTuning::drift_side_offset` (smoothed by the rig's spring).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraLean(pub f32);

/// Lean asked by a driving state: a drift shifts the camera toward the outside of the turn
/// (left in a right drift), which shows off the kart's slip angle.
pub fn drift_lean(state: &KartState) -> f32 {
    match *state {
        KartState::Drifting { side, .. } => -side.sign(),
        _ => 0.0,
    }
}

/// Writes `CameraLean` on kart bodies from their drift.
fn lean_into_drifts(mut bodies: Query<(&KartState, &mut CameraLean)>) {
    for (state, mut lean) in &mut bodies {
        lean.set_if_neq(CameraLean(drift_lean(state)));
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraTuning>()
            .add_systems(Update, lean_into_drifts)
            .add_systems(
                PostUpdate,
                (chase, underwater::tint_under_water)
                    .chain()
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// The chase (`PostUpdate`). Issue #89: the home screen's trailer camera moves the camera after
/// it, before the water's tint (`menu`), while the home's background runs.
pub(crate) fn chase(
    time: Res<Time>,
    tuning: Res<CameraTuning>,
    fixed: Option<Res<FixedView>>,
    kart_tuning: Res<KartTuning>,
    rapier: ReadRapierContext,
    visuals: Query<(&Transform, &InterpolatedFrom), Without<ChaseCamera>>,
    bodies: Query<(
        &Velocity,
        &Boost,
        Option<&CameraLean>,
        Option<&KartInput>,
        Option<&CameraShake>,
        Option<&KartEnvironment>,
    )>,
    mut cameras: Query<(&mut ChaseCamera, &mut Transform, &mut Projection)>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let context = rapier.single().ok();
    for (mut camera, mut transform, mut projection) in &mut cameras {
        if let Some(view) = fixed.as_deref() {
            *transform = Transform::from_translation(view.eye).looking_at(view.look_at, Vec3::Y);
            if let Projection::Perspective(perspective) = &mut *projection {
                perspective.fov = view.fov_degrees.to_radians();
            }
            continue;
        }
        let Ok((target, source)) = visuals.get(camera.target) else {
            continue;
        };
        let Ok((velocity, boost, lean, kart_input, shake, environment)) = bodies.get(source.0)
        else {
            continue;
        };
        let input = RigInput {
            position: target.translation,
            heading: *target.forward(),
            yaw_rate: velocity.angular.dot(*target.up()),
            speed_ratio: velocity.linear.length() / kart_tuning.max_speed.max(0.01),
            boosting: boost.is_active(),
            lean: lean.map_or(0.0, |l| l.0),
            // Issue #78: in anti-gravity, the kart's up (the road's normal).
            up: environment.map_or(Vec3::Y, |e| e.up),
        };
        let mut pose = camera.rig.update(&tuning, &input, dt);
        // Looking behind: a cut to the front of the kart; the rig above keeps chasing.
        if kart_input.is_some_and(|kart_input| kart_input.look_back) {
            pose = camera.rig.look_back(&tuning, &input);
        }

        // Keep the camera out of the ground and walls: if something solid lies between the
        // kart and the camera, move the camera in front of it.
        let mut eye = pose.eye;
        if let Some(context) = &context {
            let from = target.translation + pose.up * tuning.look_height;
            let to_eye = eye - from;
            let length = to_eye.length();
            if length > 1e-3 {
                let dir = to_eye / length;
                let filter = QueryFilter::only_fixed();
                if let Some((_, toi)) =
                    context.cast_ray(from, dir, length + tuning.clearance, true, filter)
                {
                    eye = from + dir * (toi - tuning.clearance).max(0.0);
                }
            }
        }

        // A shake (a paw strike received) moves the whole view, on top of the chase.
        let shake = shake.map_or(Vec3::ZERO, |shake| shake.offset(now));
        transform.translation = eye + shake;
        transform.look_at(pose.look_at + shake, pose.up);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = pose.fov_degrees.to_radians();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::DriftSide;

    /// Issue #34 (GD § 14): a light shake, strongest at first, gone after its duration.
    #[test]
    fn a_shake_jolts_then_fades_out() {
        assert_eq!(shake_offset(-0.01, 0.2, 0.3), Vec3::ZERO);
        assert_eq!(shake_offset(0.3, 0.2, 0.3), Vec3::ZERO);
        assert_eq!(shake_offset(0.1, 0.0, 0.3), Vec3::ZERO);
        let biggest = |from: f32, to: f32| {
            (0..100)
                .map(|i| shake_offset(from + (to - from) * i as f32 / 100.0, 0.2, 0.3).length())
                .fold(0.0f32, f32::max)
        };
        let early = biggest(0.0, 0.1);
        let late = biggest(0.2, 0.3);
        assert!(early > 0.08 && early <= 0.2 * 1.3, "{early}");
        assert!(late < early * 0.25, "{late} vs {early}");
        let shake = CameraShake::new(5.0, 0.2, 0.3);
        assert!(shake.offset(5.05).distance(shake_offset(0.05, 0.2, 0.3)) < 1e-4);
        assert_eq!(shake.offset(6.0), Vec3::ZERO);
    }

    #[test]
    fn drifts_lean_the_camera_toward_the_outside_of_the_turn() {
        let right = KartState::Drifting {
            side: DriftSide::Right,
            charge: 0.0,
        };
        let left = KartState::Drifting {
            side: DriftSide::Left,
            charge: 300.0,
        };
        // Positive lean = camera shifts right (`RigInput::lean`).
        assert_eq!(drift_lean(&right), -1.0);
        assert_eq!(drift_lean(&left), 1.0);
        assert_eq!(drift_lean(&KartState::Grounded), 0.0);
        assert_eq!(
            drift_lean(&KartState::Hop {
                ticks: 3,
                side: Some(DriftSide::Right)
            }),
            0.0
        );
    }
}
