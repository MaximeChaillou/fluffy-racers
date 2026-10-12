//! The flat test ground of issue #1: road, grass, crates, a long wall and a ramp. Handy to
//! check the kart physics; select it with `TrackChoice::TestGround`.

use bevy::prelude::*;

use super::{PlayerStart, Surface, WALL_RESTITUTION, ramp_transform, spawn_static_box};

/// Half size of the road slab. The player spawns at the origin, looking down -Z.
pub const GROUND_HALF_SIZE: f32 = 150.0;
/// Grass rectangle on the left of the start line: (min corner, max corner) on the XZ plane.
pub const GRASS_MIN: Vec2 = Vec2::new(-60.0, -120.0);
pub const GRASS_MAX: Vec2 = Vec2::new(-15.0, 30.0);
/// The ramp starts straight ahead of the start line.
pub const RAMP_START: Vec3 = Vec3::new(0.0, 0.0, -70.0);
pub const RAMP_LENGTH: f32 = 8.0;
pub const RAMP_ANGLE_DEG: f32 = 12.0;
pub const RAMP_HALF_WIDTH: f32 = 4.0;

pub(super) fn spawn_test_ground(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    build(&mut commands, &mut meshes, &mut materials);
}

pub(super) fn build(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.insert_resource(PlayerStart(Transform::from_xyz(0.0, 0.55, 0.0)));

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
        commands,
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
        commands,
        Transform::from_xyz(grass_center.x, -0.48, grass_center.y),
        Vec3::new(grass_half.x, 0.5, grass_half.y),
        Surface::Offroad,
        0.0,
        Color::srgb(0.35, 0.68, 0.30),
    );

    // Ramp straight ahead.
    let half_thickness = 0.5;
    solid(
        commands,
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
            commands,
            Transform::from_xyz(x, 0.75, z),
            Vec3::splat(0.75),
            Surface::Road,
            WALL_RESTITUTION,
            crate_color,
        );
    }
    solid(
        commands,
        Transform::from_xyz(30.0, 1.0, -40.0),
        Vec3::new(0.5, 1.0, 40.0),
        Surface::Road,
        WALL_RESTITUTION,
        Color::srgb(0.55, 0.50, 0.75),
    );
}
