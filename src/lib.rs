//! Fluffy Racers: game logic lives here so it can be unit-tested without a window.

pub mod camera;
pub mod interpolation;
pub mod kart;
pub mod track;

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use camera::{CameraPlugin, FollowCamera};
use interpolation::InterpolationPlugin;
use kart::{
    KartInputPlugin, KartPlugin, KartTuning, KartVisualPlugin, PlayerKart, SpawnPoint, kart_body,
    spawn_kart_visual,
};
use track::TrackPlugin;

/// Simulation tick rate (CLAUDE.md: the simulation runs in `FixedUpdate` at 60 Hz).
pub const TICK_HZ: f64 = 60.0;

/// The headless simulation: fixed 60 Hz clock, Rapier and the kart rules. No window, no input
/// device, no rendering: tests run it with `MinimalPlugins`.
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
            // Must be inserted before the Rapier plugin, which reads it while building.
            .insert_resource(TimestepMode::Fixed {
                dt: 1.0 / TICK_HZ as f32,
                substeps: 1,
            })
            .add_plugins(RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule())
            .add_plugins((InterpolationPlugin, KartPlugin));
    }
}

/// The whole game: simulation, test ground, player kart, visuals and camera.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.55, 0.80, 0.95)))
            .add_plugins((
                SimulationPlugin,
                TrackPlugin,
                KartInputPlugin,
                KartVisualPlugin,
                CameraPlugin,
            ))
            .add_systems(Startup, (spawn_light, spawn_player));
    }
}

/// The player's starting position: the origin of the test ground, looking down -Z, wheels on
/// the ground.
pub fn player_spawn_point() -> Transform {
    Transform::from_xyz(0.0, 0.55, 0.0)
}

fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tuning: Res<KartTuning>,
) {
    let spawn = player_spawn_point();
    let body = commands
        .spawn((kart_body(&tuning, spawn), PlayerKart, SpawnPoint(spawn)))
        .id();
    let visual = spawn_kart_visual(
        &mut commands,
        &mut meshes,
        &mut materials,
        body,
        Color::srgb(0.95, 0.45, 0.65),
    );
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        FollowCamera::new(visual),
    ));
}

fn spawn_light(mut commands: Commands) {
    // One directional light with shadows (CLAUDE.md § Performance).
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            maximum_distance: 80.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(8.0, 16.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::InputPlugin;
    use bevy::time::TimeUpdateStrategy;
    use kart::{Kart, KartState, KartWheels};

    /// The whole game headless: real test ground, player kart, keyboard input. One fixed
    /// tick per update.
    fn game_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            InputPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_plugins(GamePlugin)
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.update();
        app
    }

    fn player(app: &mut App) -> Entity {
        app.world_mut()
            .query_filtered::<Entity, (With<Kart>, With<PlayerKart>)>()
            .single(app.world())
            .unwrap()
    }

    fn run(app: &mut App, seconds: f32, mut each_tick: impl FnMut(&App)) {
        for _ in 0..(seconds * TICK_HZ as f32).round() as usize {
            app.update();
            each_tick(app);
        }
    }

    #[test]
    fn game_plugin_runs_headless_with_kart_on_the_test_ground() {
        let mut app = game_app();
        run(&mut app, 0.5, |_| {});
        let kart = player(&mut app);
        let world = app.world();
        assert_eq!(world.get::<KartState>(kart), Some(&KartState::Grounded));
        assert_eq!(world.get::<KartWheels>(kart).unwrap().wheels_on_ground(), 4);
        let mut cameras = app.world_mut().query::<&FollowCamera>();
        assert_eq!(cameras.iter(app.world()).count(), 1);
    }

    /// End to end: holding the up arrow drives the player kart over the test ground's ramp,
    /// and it lands on its wheels.
    #[test]
    fn keyboard_drives_the_player_over_the_test_ramp() {
        let mut app = game_app();
        let kart = player(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowUp);

        let mut was_airborne = false;
        run(&mut app, 6.0, |app| {
            was_airborne |= app.world().get::<KartState>(kart) == Some(&KartState::Airborne);
        });

        let world = app.world();
        let transform = world.get::<Transform>(kart).unwrap();
        assert!(was_airborne, "the ramp jump did not happen");
        assert!(
            transform.translation.z < track::test_ground::RAMP_START.z - 20.0,
            "kart at {}",
            transform.translation
        );
        assert_eq!(world.get::<KartState>(kart), Some(&KartState::Grounded));
        assert!(transform.up().y > 0.98);
    }

    #[test]
    fn falling_off_the_test_ground_resets_the_kart() {
        let mut app = game_app();
        let kart = player(&mut app);
        app.world_mut()
            .get_mut::<Transform>(kart)
            .unwrap()
            .translation = Vec3::new(0.0, -50.0, 0.0);
        run(&mut app, 0.5, |_| {});
        let position = app.world().get::<Transform>(kart).unwrap().translation;
        assert!(
            position.distance(player_spawn_point().translation) < 0.5,
            "kart at {position}"
        );
    }
}
