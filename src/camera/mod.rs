//! Camera. For now a basic smoothed follow camera so the kart can be driven around the test
//! ground; the real chase camera (FOV with speed, look behind...) comes with issue #2.

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::interpolation::InterpolationSystems;

/// Camera that follows a (visual) entity from behind.
#[derive(Component, Clone, Copy, Debug)]
pub struct FollowCamera {
    pub target: Entity,
    /// Distance behind the target.
    pub distance: f32,
    /// Height above the target.
    pub height: f32,
    /// How fast the camera catches up (1/s).
    pub stiffness: f32,
}

impl FollowCamera {
    pub fn new(target: Entity) -> Self {
        Self {
            target,
            distance: 7.0,
            height: 3.0,
            stiffness: 6.0,
        }
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            follow_target
                .after(InterpolationSystems)
                .before(TransformSystems::Propagate),
        );
    }
}

fn follow_target(
    time: Res<Time>,
    targets: Query<&Transform, Without<FollowCamera>>,
    mut cameras: Query<(&FollowCamera, &mut Transform)>,
) {
    let blend = |stiffness: f32| 1.0 - (-stiffness * time.delta_secs()).exp();
    for (follow, mut camera) in &mut cameras {
        let Ok(target) = targets.get(follow.target) else {
            continue;
        };
        // Behind the kart on the horizontal plane, so jumps and slopes do not shake the view.
        let back = (*target.back() * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::Z);
        let wanted = target.translation + back * follow.distance + Vec3::Y * follow.height;
        camera.translation = camera.translation.lerp(wanted, blend(follow.stiffness));
        camera.look_at(target.translation + Vec3::Y * 1.0, Vec3::Y);
    }
}
