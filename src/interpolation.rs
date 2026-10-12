//! Visual interpolation between two fixed simulation ticks.
//!
//! The simulation runs in `FixedUpdate` at 60 Hz while the screen may refresh at any rate.
//! Simulated entities record their pose after each tick (`PhysicsPose`); a separate visual
//! entity (`InterpolatedFrom`) is placed between the last two poses every frame, so motion
//! stays smooth at 30, 60 or 144 fps.

use bevy::prelude::*;
use bevy::transform::TransformSystems;

/// The last two simulated poses of an entity, recorded after each fixed tick.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PhysicsPose {
    pub previous: Transform,
    pub current: Transform,
}

impl PhysicsPose {
    pub fn new(transform: Transform) -> Self {
        Self {
            previous: transform,
            current: transform,
        }
    }

    /// Forgets the previous pose: use after a teleport so the visual does not slide.
    pub fn snap(&mut self, transform: Transform) {
        *self = Self::new(transform);
    }

    /// Pose at `alpha` (0 = previous tick, 1 = current tick).
    pub fn interpolate(&self, alpha: f32) -> Transform {
        let alpha = alpha.clamp(0.0, 1.0);
        Transform {
            translation: self
                .previous
                .translation
                .lerp(self.current.translation, alpha),
            rotation: self.previous.rotation.slerp(self.current.rotation, alpha),
            scale: self.previous.scale.lerp(self.current.scale, alpha),
        }
    }
}

/// On a visual entity: follow the interpolated pose of this simulated entity.
#[derive(Component, Clone, Copy, Debug)]
pub struct InterpolatedFrom(pub Entity);

/// Runs in `PostUpdate` before transform propagation; other visual followers (camera...)
/// should run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterpolationSystems;

pub struct InterpolationPlugin;

impl Plugin for InterpolationPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            InterpolationSystems.before(TransformSystems::Propagate),
        )
        .add_systems(FixedLast, record_poses)
        .add_systems(PostUpdate, interpolate_visuals.in_set(InterpolationSystems));
    }
}

fn record_poses(mut poses: Query<(&Transform, &mut PhysicsPose)>) {
    for (transform, mut pose) in &mut poses {
        pose.previous = pose.current;
        pose.current = *transform;
    }
}

fn interpolate_visuals(
    fixed_time: Res<Time<Fixed>>,
    poses: Query<&PhysicsPose>,
    mut visuals: Query<(&InterpolatedFrom, &mut Transform), Without<PhysicsPose>>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (source, mut transform) in &mut visuals {
        if let Ok(pose) = poses.get(source.0) {
            *transform = pose.interpolate(alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_between_ticks() {
        let mut pose = PhysicsPose::new(Transform::from_xyz(0.0, 0.0, 0.0));
        pose.previous = pose.current;
        pose.current = Transform::from_xyz(2.0, 0.0, 0.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let half = pose.interpolate(0.5);
        assert!((half.translation.x - 1.0).abs() < 1e-5);
        let (_, angle) = half.rotation.to_axis_angle();
        assert!((angle - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
        assert_eq!(pose.interpolate(2.0), pose.current);
    }

    #[test]
    fn snap_removes_the_previous_pose() {
        let mut pose = PhysicsPose::new(Transform::IDENTITY);
        pose.current = Transform::from_xyz(5.0, 0.0, 0.0);
        let target = Transform::from_xyz(-3.0, 1.0, 0.0);
        pose.snap(target);
        assert_eq!(pose.interpolate(0.0), target);
    }
}
