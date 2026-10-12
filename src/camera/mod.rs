//! Chase camera (issue #2): behind the local player's kart, smoothed by critically damped
//! springs so it never shakes, whatever the frame rate.
//!
//! It follows the *interpolated* visual of the kart (see `crate::interpolation`), not the
//! physics body, so it is as smooth at 30 fps as at 144 fps.

mod rig;

pub use rig::{CameraRig, RigInput};

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_rapier3d::prelude::*;

use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{Boost, KartState, KartTuning};

/// All the camera knobs, with their default values (our own starting points, à régler).
#[derive(Resource, Clone, Debug, PartialEq)]
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
                chase
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

fn chase(
    time: Res<Time>,
    tuning: Res<CameraTuning>,
    kart_tuning: Res<KartTuning>,
    rapier: ReadRapierContext,
    visuals: Query<(&Transform, &InterpolatedFrom), Without<ChaseCamera>>,
    bodies: Query<(&Velocity, &Boost, Option<&CameraLean>)>,
    mut cameras: Query<(&mut ChaseCamera, &mut Transform, &mut Projection)>,
) {
    let dt = time.delta_secs();
    let context = rapier.single().ok();
    for (mut camera, mut transform, mut projection) in &mut cameras {
        let Ok((target, source)) = visuals.get(camera.target) else {
            continue;
        };
        let Ok((velocity, boost, lean)) = bodies.get(source.0) else {
            continue;
        };
        let input = RigInput {
            position: target.translation,
            heading: *target.forward(),
            yaw_rate: velocity.angular.dot(*target.up()),
            speed_ratio: velocity.linear.length() / kart_tuning.max_speed.max(0.01),
            boosting: boost.is_active(),
            lean: lean.map_or(0.0, |l| l.0),
        };
        let pose = camera.rig.update(&tuning, &input, dt);

        // Keep the camera out of the ground and walls: if something solid lies between the
        // kart and the camera, move the camera in front of it.
        let mut eye = pose.eye;
        if let Some(context) = &context {
            let from = target.translation + Vec3::Y * tuning.look_height;
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

        transform.translation = eye;
        transform.look_at(pose.look_at, Vec3::Y);
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = pose.fov_degrees.to_radians();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::DriftSide;

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
