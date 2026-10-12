//! Fluffy Racers: game logic lives here so it can be unit-tested without a window.

use bevy::prelude::*;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.55, 0.80, 0.95)))
            .add_systems(Startup, setup_placeholder_scene)
            .add_systems(Update, spin_placeholder_kart);
    }
}

/// Marker for the temporary kart shown until issue #1 lands.
#[derive(Component)]
pub struct PlaceholderKart;

fn setup_placeholder_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.45, 0.75, 0.35))),
    ));

    // Placeholder kart: a pink box with a fluffy ball "driver"
    commands
        .spawn((
            PlaceholderKart,
            Transform::from_xyz(0.0, 0.5, 0.0),
            Visibility::default(),
        ))
        .with_children(|kart| {
            kart.spawn((
                Mesh3d(meshes.add(Cuboid::new(1.4, 0.5, 2.0))),
                MeshMaterial3d(materials.add(Color::srgb(0.95, 0.45, 0.65))),
            ));
            kart.spawn((
                Mesh3d(meshes.add(Sphere::new(0.45))),
                MeshMaterial3d(materials.add(Color::srgb(0.98, 0.92, 0.80))),
                Transform::from_xyz(0.0, 0.6, -0.1),
            ));
        });

    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    ));
}

fn spin_placeholder_kart(time: Res<Time>, mut karts: Query<&mut Transform, With<PlaceholderKart>>) {
    for mut transform in &mut karts {
        transform.rotate_y(time.delta_secs() * 0.8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: the plugin builds and runs a few frames headless.
    #[test]
    fn game_plugin_runs_headless() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .add_plugins(GamePlugin);
        for _ in 0..3 {
            app.update();
        }
        let mut karts = app
            .world_mut()
            .query_filtered::<(), With<PlaceholderKart>>();
        assert_eq!(karts.iter(app.world()).count(), 1);
    }
}
