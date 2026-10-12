//! Tracks and terrain. For now: the test ground of issue #1 (flat road, grass, obstacles, ramp).
//! The spline circuit comes with issue #3.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

/// Kind of ground under a wheel. Put it on a collider entity; colliders without it count as road.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    Road,
    /// Grass, sand...: the max speed drops (GD § 2, `KartTuning::offroad_speed_factor`).
    Offroad,
}

/// Bounciness of walls and obstacles: the kart bounces back and never stops dead (GD § 2).
/// 0.3 = a head-on hit at 26 m/s sends the kart back at about 8 m/s.
pub const WALL_RESTITUTION: f32 = 0.3;

/// Spawns a static box collider (physics only, no mesh): ground slab, wall, obstacle, ramp.
/// `restitution`: 0 for the ground, `WALL_RESTITUTION` for things the kart bumps into.
pub fn spawn_static_box(
    commands: &mut Commands,
    transform: Transform,
    half_extents: Vec3,
    surface: Surface,
    restitution: f32,
) -> Entity {
    commands
        .spawn((
            transform,
            RigidBody::Fixed,
            Collider::cuboid(half_extents.x, half_extents.y, half_extents.z),
            Restitution::coefficient(restitution),
            // No friction: scraping a wall or the ground slides instead of stopping the kart.
            Friction::coefficient(0.0),
            surface,
        ))
        .id()
}

/// Transform of a ramp box whose top face starts at ground level at `start` and climbs
/// toward -Z over `length` meters with a slope of `angle` radians.
pub fn ramp_transform(start: Vec3, length: f32, angle: f32, half_thickness: f32) -> Transform {
    let height = length * angle.sin();
    let run = length * angle.cos();
    let rotation = Quat::from_rotation_x(angle);
    // Middle of the top face, then down to the box center along the ramp's normal.
    let top_center = start + Vec3::new(0.0, height / 2.0, -run / 2.0);
    let center = top_center - rotation * Vec3::Y * half_thickness;
    Transform::from_translation(center).with_rotation(rotation)
}

pub struct TrackPlugin;

impl Plugin for TrackPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_test_ground);
    }
}

/// Layout of the test ground. The player spawns at the origin, looking down -Z.
pub mod test_ground {
    use bevy::prelude::*;

    pub const GROUND_HALF_SIZE: f32 = 150.0;
    /// Grass rectangle on the left of the start line: (min corner, max corner) on the XZ plane.
    pub const GRASS_MIN: Vec2 = Vec2::new(-60.0, -120.0);
    pub const GRASS_MAX: Vec2 = Vec2::new(-15.0, 30.0);
    /// The ramp starts straight ahead of the start line.
    pub const RAMP_START: Vec3 = Vec3::new(0.0, 0.0, -70.0);
    pub const RAMP_LENGTH: f32 = 8.0;
    pub const RAMP_ANGLE_DEG: f32 = 12.0;
    pub const RAMP_HALF_WIDTH: f32 = 4.0;
}

fn spawn_test_ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use test_ground::*;

    let mut solid = |commands: &mut Commands,
                     transform: Transform,
                     half: Vec3,
                     surface: Surface,
                     restitution: f32,
                     color: Color| {
        let entity = spawn_static_box(commands, transform, half, surface, restitution);
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Cuboid::from_size(half * 2.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.9,
                ..default()
            })),
        ));
    };

    // Road: one big slab, top face at y = 0.
    solid(
        &mut commands,
        Transform::from_xyz(0.0, -0.5, 0.0),
        Vec3::new(GROUND_HALF_SIZE, 0.5, GROUND_HALF_SIZE),
        Surface::Road,
        0.0,
        Color::srgb(0.42, 0.43, 0.48),
    );

    // Grass: a slab 2 cm above the road so the wheel rays hit it first.
    let grass_center = (GRASS_MIN + GRASS_MAX) / 2.0;
    let grass_half = (GRASS_MAX - GRASS_MIN) / 2.0;
    solid(
        &mut commands,
        Transform::from_xyz(grass_center.x, -0.48, grass_center.y),
        Vec3::new(grass_half.x, 0.5, grass_half.y),
        Surface::Offroad,
        0.0,
        Color::srgb(0.35, 0.68, 0.30),
    );

    // Ramp straight ahead.
    let half_thickness = 0.5;
    solid(
        &mut commands,
        ramp_transform(
            RAMP_START,
            RAMP_LENGTH,
            RAMP_ANGLE_DEG.to_radians(),
            half_thickness,
        ),
        Vec3::new(RAMP_HALF_WIDTH, half_thickness, RAMP_LENGTH / 2.0),
        Surface::Road,
        0.0,
        Color::srgb(0.98, 0.72, 0.25),
    );

    // Obstacles on the right: a few crates and a long wall to bounce on.
    let crate_color = Color::srgb(0.85, 0.45, 0.35);
    for (x, z) in [(12.0, -20.0), (18.0, -35.0), (10.0, -48.0), (22.0, -10.0)] {
        solid(
            &mut commands,
            Transform::from_xyz(x, 0.75, z),
            Vec3::splat(0.75),
            Surface::Road,
            WALL_RESTITUTION,
            crate_color,
        );
    }
    solid(
        &mut commands,
        Transform::from_xyz(30.0, 1.0, -40.0),
        Vec3::new(0.5, 1.0, 40.0),
        Surface::Road,
        WALL_RESTITUTION,
        Color::srgb(0.55, 0.50, 0.75),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_top_face_starts_on_the_ground_and_climbs_forward() {
        let angle = 12f32.to_radians();
        let start = Vec3::new(0.0, 0.0, -70.0);
        let t = ramp_transform(start, 8.0, angle, 0.5);
        let top_start = t.transform_point(Vec3::new(0.0, 0.5, 4.0));
        let top_end = t.transform_point(Vec3::new(0.0, 0.5, -4.0));
        assert!(top_start.distance(start) < 1e-4, "{top_start}");
        assert!((top_end.y - 8.0 * angle.sin()).abs() < 1e-4);
        assert!(top_end.z < start.z);
    }
}
