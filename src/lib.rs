//! Fluffy Racers: game logic lives here so it can be unit-tested without a window.

pub mod admin;
pub mod audio;
pub mod bots;
pub mod camera;
pub mod characters;
pub mod croquettes;
pub mod interactions;
pub mod interpolation;
pub mod items;
pub mod kart;
pub mod level;
pub mod menu;
pub mod models;
pub mod modes;
pub mod net;
pub mod options;
pub mod race;
pub mod salon;
pub mod track;
pub mod ui;
pub mod vfx;

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use admin::AdminPlugin;
use audio::SoundPlugin;
use bots::BotPlugin;
use camera::{CameraLean, CameraPlugin, CameraTuning, ChaseCamera};
use characters::{CharacterPlugin, DriverLooks, PlayerColor, PlayerLoadout, Roster};
use croquettes::{CroquettePlugin, CroquetteVisualPlugin};
use interactions::{InteractionPlugin, InteractionVisualPlugin};
use interpolation::InterpolationPlugin;
use items::{ItemPlugin, ItemVisualPlugin};
use kart::{
    KartInputPlugin, KartPaint, KartPlugin, KartTuning, KartVisualAssets, KartVisualPlugin,
    PlayerKart, SpawnPoint, StatsTuning, kart_body_with_stats, spawn_kart_visual_with_driver,
};
use level::LevelPlugin;
use menu::MainMenuPlugin;
use models::ModelPlugin;
use modes::{ModesPlugin, TimeTrialPlugin};
use net::{KartId, KartSyncPlugin, NetKart, NetPlugin, OnlineSlot};
use options::OptionsPlugin;
use race::{PelicanPlugin, RaceGrid, RaceMode, RacePlugin, Racer};
use track::{PlayerStart, TrackLayout, TrackPlugin};
use ui::{DebugOverlayPlugin, HudPlugin};

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

/// The whole game: simulation, track, race rules, croquettes, items, the interactions between
/// karts (issue #34), characters and their selection screen, player kart, bots, visuals, sounds,
/// camera, HUD, debug overlay, the player's options (issue #17), admin panel and the network's
/// rooms and lobby (issue #7; the egui parts only with a renderer), the other players' karts
/// online (issue #8, `net::KartSyncPlugin`: nothing runs without a room), and the game modes
/// (issue #19, `modes::ModesPlugin`: the race menu, the Grand Prix), and with a renderer the home
/// screen and the menu opened from the game (issues #56 and #89, `menu::MainMenuPlugin`, with the
/// home's background), the time trial (issue #76, `modes::TimeTrialPlugin`), the reel of the
/// item slots' roulette (issue #102, `ui::RoulettePlugin`), the sound design (issue #103,
/// `audio::SoundDesignPlugin`), the music (issue #104, `audio::MusicPlugin`) and the particles
/// (issue #105, `vfx::VfxPlugin`). `LevelPlugin`
/// restarts the level or switches tracks (issue #36).
///
/// The race (`RacePlugin`) is not part of `SimulationPlugin`: the kart scenarios drive freely,
/// without a countdown. It only starts on a circuit, not on the flat test ground. On a circuit,
/// the bots (`BotPlugin`) fill the grid: 11 of them by default (`RaceGrid`), the player behind.
/// The croquettes (`CroquettePlugin`), the items (`ItemPlugin`, issue #16) and the interactions
/// between karts (`InteractionPlugin`, issue #34) are not part of it either: a kart scenario has
/// none.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.55, 0.80, 0.95)))
            .add_plugins((
                SimulationPlugin,
                ModelPlugin,
                TrackPlugin,
                RacePlugin,
                (
                    CroquettePlugin,
                    CharacterPlugin,
                    ItemPlugin,
                    InteractionPlugin,
                ),
                KartInputPlugin,
                KartVisualPlugin,
                (
                    CroquetteVisualPlugin,
                    ItemVisualPlugin,
                    InteractionVisualPlugin,
                ),
                BotPlugin,
                PelicanPlugin,
                CameraPlugin,
                HudPlugin,
                DebugOverlayPlugin,
                SoundPlugin,
                (
                    LevelPlugin,
                    OptionsPlugin,
                    AdminPlugin,
                    NetPlugin,
                    KartSyncPlugin,
                    ModesPlugin,
                ),
            ))
            .add_systems(Startup, spawn_light)
            .add_systems(Update, spawn_player.run_if(resource_added::<PlayerStart>));
        // Issue #56: the main menu, with Échap; issue #89: the home screen at launch, and its
        // background. Only on screen: the headless game (the tests, the bot races) keeps the
        // world it was measured with (`menu`).
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            // Issue #98: the « Pilote » screen, its pseudo kept in the browser.
            app.add_plugins(menu::PilotPlugin { storage: true });
            app.add_plugins(MainMenuPlugin { at_launch: true });
            // Issue #76: the time trial, its records kept in the browser. Only on screen, like
            // the main menu: the bot races' world has none of it (`modes::time_trial`).
            app.add_plugins(TimeTrialPlugin { storage: true });
            // Issue #102: the roulette of the item slots as a slot machine's reel. Only on screen:
            // the headless HUD keeps its single icon, the bot races their world.
            app.add_plugins(ui::RoulettePlugin);
            // Issue #103: the sound design (engines, drift, boosts, items, hits, cries, screens).
            // Only on screen: nothing audio in the bot races' world.
            app.add_plugins(audio::SoundDesignPlugin);
            // Issue #104: the music (a piece per circuit, the home's, the lobby's, the finish's
            // and the podium's). Only on screen too.
            app.add_plugins(audio::MusicPlugin);
            // Issue #105: the particles (the drift's sparks, the boosts, the shocks, the dust).
            // Only on screen: nothing of them in the bot races' world.
            app.add_plugins(vfx::VfxPlugin);
        }
    }
}

/// Spawns the player's kart, when the track says where to start, and the camera the first time
/// (a reloaded level, issue #36, gives the same camera the new kart to follow). In a race the
/// player is a racer on its `RaceGrid` slot, behind the bots (alone on the pole without bots);
/// on the test ground, or driving freely on a circuit (`RaceMode::FreeRoam`), it just drives,
/// and falls back to its spawn point. Its character and parts are the player's loadout
/// (issue #15): their stats, and the driver on the kart; its color, the player's (issue #55;
/// online, `net` paints it the color it wears in the room). Online (issue #8, `net::OnlineSlot`),
/// it starts from the grid slot of its seat, so that the players do not start on top of each
/// other; in a race online (issue #9), from its place on the race's grid (behind the bots), and
/// that slot is its network id (`net::NetKart`).
fn spawn_player(
    mut commands: Commands,
    visuals: Res<KartVisualAssets>,
    tuning: Res<KartTuning>,
    stats_tuning: Res<StatsTuning>,
    roster: Res<Roster>,
    (loadout, color): (Res<PlayerLoadout>, Option<Res<PlayerColor>>),
    looks: Option<Res<DriverLooks>>,
    camera_tuning: Res<CameraTuning>,
    start: Res<PlayerStart>,
    circuit: Option<Res<TrackLayout>>,
    grid: Res<RaceGrid>,
    mode: Res<RaceMode>,
    online: Option<Res<OnlineSlot>>,
    mut cameras: Query<(&mut ChaseCamera, &mut Projection)>,
) {
    let slot = circuit
        .as_ref()
        .filter(|_| *mode == RaceMode::Race)
        .map(|layout| {
            let slot = online
                .as_ref()
                .map_or(grid.player_slot(), |online| online.0);
            slot.min(layout.grid.len() - 1)
        });
    let spawn = match (&circuit, slot) {
        (Some(layout), Some(slot)) => layout.grid[slot],
        _ => online
            .as_ref()
            .zip(circuit.as_ref())
            .and_then(|(online, layout)| layout.grid.get(online.0).copied())
            .unwrap_or(start.0),
    };
    let stats = roster.stats(&loadout);
    let color = color.map(|color| color.0).unwrap_or_default();
    let mut kart = commands.spawn((
        kart_body_with_stats(&tuning, &stats_tuning, stats, spawn),
        PlayerKart,
        CameraLean::default(),
        KartPaint(color),
    ));
    match slot {
        Some(slot) => kart.insert(Racer::new(slot)),
        None => kart.insert(SpawnPoint(spawn)),
    };
    if let Some(online) = &online {
        kart.insert(NetKart(online.0.min(track::GRID_SLOTS - 1) as KartId));
    }
    let body = kart.id();
    let driver = looks
        .as_ref()
        .and_then(|looks| looks.get(loadout.character));
    // Issue #73: its parts on screen.
    let looks = roster.looks(&loadout);
    let visual = spawn_kart_visual_with_driver(&mut commands, &visuals, body, color, driver, looks);
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: camera_tuning.base_fov.to_radians(),
        ..default()
    });
    if let Ok((mut camera, mut current)) = cameras.single_mut() {
        // Its springs start over behind the new kart.
        *camera = ChaseCamera::new(visual);
        *current = projection;
        return;
    }
    commands.spawn((
        Camera3d::default(),
        projection,
        Transform::from_xyz(0.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        ChaseCamera::new(visual),
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
    use bevy::ecs::schedule::SingleThreadedExecutor;
    use bevy::input::InputPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_rapier3d::prelude::Velocity;
    use kart::{
        Boost, DriftSide, DriftSpark, Kart, KartBodyPivot, KartState, KartWheels, MiniTurbo,
    };
    use track::TrackChoice;

    /// The whole game headless on `track`, the player alone on the grid (the tests of #1 to #5):
    /// player kart, keyboard input, one fixed tick per update. Returns once the track is built
    /// and the player kart spawned.
    pub(crate) fn app_on(track: TrackChoice) -> App {
        app_with_bots(track, 0)
    }

    /// Same with `bots` bots on the grid (issue #6; the game has 11).
    pub(crate) fn app_with_bots(track: TrackChoice, bots: usize) -> App {
        let mut app = game_app_unloaded(track, bots);
        wait_for_the_player(&mut app);
        app
    }

    /// The whole game headless on `track` with `bots` bots, before its first update.
    pub(crate) fn game_app_unloaded(track: TrackChoice, bots: usize) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            InputPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .insert_resource(track)
        .insert_resource(RaceGrid { bots, ..default() })
        .add_plugins(GamePlugin)
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app
    }

    /// Every schedule of `app` runs its systems one after the other, in the fixed order of the
    /// schedule: the executor of the browser (wasm has a single thread). The bot races (issue
    /// #40) and the crashes into walls (issue #27) run side by side, one App per thread, so they
    /// gain nothing from Bevy's multi-threaded executor, which costs several times more per tick
    /// here; and a fixed order of the systems gives the same run on every machine, whatever the
    /// load.
    pub(crate) fn single_threaded(app: &mut App) {
        for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
            schedule.set_executor(SingleThreadedExecutor::new());
        }
    }

    /// Runs updates until the track is built and the player kart spawned (circuit files load
    /// asynchronously).
    pub(crate) fn wait_for_the_player(app: &mut App) {
        for _ in 0..2000 {
            app.update();
            let world = app.world_mut();
            if world
                .query_filtered::<(), With<PlayerKart>>()
                .iter(world)
                .next()
                .is_some()
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("the track never finished loading");
    }

    /// The flat test ground of issue #1.
    fn game_app() -> App {
        app_on(TrackChoice::TestGround)
    }

    pub(crate) fn player(app: &mut App) -> Entity {
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
        let mut cameras = app.world_mut().query::<&ChaseCamera>();
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

    pub(crate) fn hold_keys(app: &mut App, keys: &[KeyCode]) {
        let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        input.release_all();
        for key in keys {
            input.press(*key);
        }
    }

    /// Issue #4 end to end on the keyboard: Space + left arrow at speed → hop, then a left
    /// drift with blue sparks, the nose turned inside and the camera leaning out; releasing
    /// Space fires the blue mini-turbo.
    #[test]
    fn keyboard_drift_shows_sparks_leans_the_camera_and_fires_a_mini_turbo() {
        let mut app = game_app();
        let kart = player(&mut app);
        hold_keys(&mut app, &[KeyCode::ArrowUp]);
        run(&mut app, 1.5, |_| {});
        hold_keys(
            &mut app,
            &[KeyCode::ArrowUp, KeyCode::Space, KeyCode::ArrowLeft],
        );
        let mut drifting_ticks = 0;
        for _ in 0..(3.0 * TICK_HZ) as usize {
            app.update();
            if matches!(
                app.world().get::<KartState>(kart),
                Some(KartState::Drifting { .. })
            ) {
                drifting_ticks += 1;
            }
            if drifting_ticks == 60 {
                break;
            }
        }
        assert_eq!(drifting_ticks, 60, "the kart never drifted");

        let world = app.world_mut();
        assert!(matches!(
            world.get::<KartState>(kart),
            Some(KartState::Drifting {
                side: DriftSide::Left,
                ..
            })
        ));
        // Left drift: the camera shifts right (outside of the turn), the nose points left.
        assert_eq!(world.get::<CameraLean>(kart), Some(&CameraLean(1.0)));
        let slip = world.query::<&KartBodyPivot>().single(world).unwrap().slip;
        assert!(slip < -0.2, "slip angle {slip}");
        // 60 ticks steering inward: blue sparks, at both rear wheels.
        let visible_sparks = |world: &mut World| {
            let mut sparks = world.query::<(&DriftSpark, &Visibility)>();
            let mut shown = [0; 3];
            for (spark, visibility) in sparks.iter(world) {
                if *visibility != Visibility::Hidden {
                    shown[spark.tier.index()] += 1;
                }
            }
            shown
        };
        assert_eq!(visible_sparks(world), [2, 0, 0]);

        hold_keys(&mut app, &[KeyCode::ArrowUp]);
        app.update();
        let world = app.world_mut();
        let boost = *world.get::<Boost>(kart).unwrap();
        let duration = KartTuning::default().mini_turbo_durations[MiniTurbo::Blue.index()];
        assert!((boost.remaining - duration).abs() < 1e-5, "{boost:?}");
        assert!(!matches!(
            world.get::<KartState>(kart),
            Some(KartState::Drifting { .. })
        ));
        assert_eq!(visible_sparks(world), [0, 0, 0]);
        run(&mut app, 0.1, |_| {});
        assert_eq!(app.world().get::<CameraLean>(kart), Some(&CameraLean(0.0)));
    }

    fn camera_and_kart_visual(app: &mut App) -> (Transform, Transform) {
        let world = app.world_mut();
        let (camera, target) = world
            .query::<(&Transform, &ChaseCamera)>()
            .single(world)
            .map(|(t, c)| (*t, c.target))
            .unwrap();
        (camera, *world.get::<Transform>(target).unwrap())
    }

    /// Issue #2: the camera stays within a sane distance of the kart while driving.
    #[test]
    fn chase_camera_stays_close_while_driving() {
        let mut app = game_app();
        let mut distances = Vec::new();
        let legs = [
            (None, 2.0),
            (Some(KeyCode::ArrowLeft), 1.5),
            (Some(KeyCode::ArrowRight), 1.5),
        ];
        for (steer, seconds) in legs {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release_all();
            keys.press(KeyCode::ArrowUp);
            if let Some(key) = steer {
                keys.press(key);
            }
            for _ in 0..(seconds * TICK_HZ as f32) as usize {
                app.update();
                let (camera, kart) = camera_and_kart_visual(&mut app);
                distances.push(camera.translation.distance(kart.translation));
            }
        }
        let tuning = CameraTuning::default();
        let rest = (tuning.distance.powi(2) + tuning.height.powi(2)).sqrt();
        let (min, max) = distances
            .iter()
            .fold((f32::MAX, 0.0f32), |(lo, hi), d| (lo.min(*d), hi.max(*d)));
        assert!(
            min > rest - 1.5 && max < rest + 1.5,
            "camera distance in [{min}, {max}], rest {rest}"
        );
        // It looks at the kart: the kart is in front of the camera.
        let (camera, kart) = camera_and_kart_visual(&mut app);
        assert!((kart.translation - camera.translation).dot(*camera.forward()) > 0.0);
    }

    /// Issue #2: a wall between the kart and the camera pulls the camera in front of it.
    #[test]
    fn chase_camera_does_not_go_through_walls() {
        let mut app = game_app();
        run(&mut app, 0.5, |_| {});
        // The kart looks down -Z from the origin, so the camera sits around z = +6.
        let (camera, _) = camera_and_kart_visual(&mut app);
        assert!(camera.translation.z > 4.0, "{}", camera.translation);
        let mut commands = app.world_mut().commands();
        track::spawn_static_box(
            &mut commands,
            Transform::from_xyz(0.0, 2.0, 3.0),
            Vec3::new(10.0, 4.0, 0.25),
            track::Surface::Wall,
            0.0,
        );
        app.world_mut().flush();
        run(&mut app, 0.2, |_| {});
        let (camera, _) = camera_and_kart_visual(&mut app);
        assert!(
            camera.translation.z < 2.75,
            "camera behind the wall: {}",
            camera.translation
        );
    }

    /// Issue #17: holding C looks behind. The camera cuts to the front of the kart and looks
    /// back over it, for as long as C is held; released, it is exactly where the chase camera of
    /// a game that never looked back is (its springs ran on underneath). The kart does not care.
    #[test]
    fn holding_c_looks_behind_then_the_chase_camera_is_back() {
        let (mut looking, mut chasing) = (game_app(), game_app());
        // Accelerating, then a left turn while looking back, then straight on.
        for app in [&mut looking, &mut chasing] {
            hold_keys(app, &[KeyCode::ArrowUp]);
            run(app, 1.0, |_| {});
        }
        hold_keys(&mut chasing, &[KeyCode::ArrowUp, KeyCode::ArrowLeft]);
        hold_keys(
            &mut looking,
            &[KeyCode::ArrowUp, KeyCode::ArrowLeft, KeyCode::KeyC],
        );
        let tuning = CameraTuning::default();
        for _ in 0..(0.75 * TICK_HZ) as usize {
            looking.update();
            chasing.update();
            let (camera, kart) = camera_and_kart_visual(&mut looking);
            let ahead = kart.forward().with_y(0.0).normalize();
            let eye = camera.translation - kart.translation;
            // In front of the kart, above it, looking back.
            assert!(
                (eye.dot(ahead) - tuning.look_back_distance).abs() < 0.3,
                "{eye}"
            );
            assert!(eye.y > 1.0, "{eye}");
            assert!(camera.forward().dot(ahead) < -0.5);
            let (chase, _) = camera_and_kart_visual(&mut chasing);
            assert!(chase.translation.distance(camera.translation) > 8.0);
        }
        for app in [&mut looking, &mut chasing] {
            hold_keys(app, &[KeyCode::ArrowUp]);
        }
        for _ in 0..(0.5 * TICK_HZ) as usize {
            looking.update();
            chasing.update();
            let (camera, kart) = camera_and_kart_visual(&mut looking);
            let (chase, chase_kart) = camera_and_kart_visual(&mut chasing);
            assert!(
                kart.translation.distance(chase_kart.translation) < 1e-3,
                "the kart ignores the look-back button"
            );
            assert!(
                camera.translation.distance(chase.translation) < 1e-3
                    && camera.forward().distance(*chase.forward()) < 1e-4,
                "back to the chase camera at once: {} vs {}",
                camera.translation,
                chase.translation
            );
            assert!((kart.translation - camera.translation).dot(*camera.forward()) > 0.0);
        }
    }

    /// Issue #3: the circuit file loads, the track is built and the player starts on the grid
    /// with its wheels on the road.
    #[test]
    fn circuit_loads_and_the_player_starts_on_the_grid() {
        let mut app = app_on(TrackChoice::default());
        run(&mut app, 0.5, |_| {});
        let kart = player(&mut app);
        let world = app.world();
        let layout = world.resource::<track::TrackLayout>();
        assert_eq!(layout.name, "Prairie Pompon");
        let position = world.get::<Transform>(kart).unwrap().translation;
        assert!(
            position.distance(layout.grid[0].translation) < 0.5,
            "kart at {position}"
        );
        assert_eq!(world.get::<KartState>(kart), Some(&KartState::Grounded));
        assert_eq!(world.get::<KartWheels>(kart).unwrap().wheels_on_ground(), 4);
        assert_eq!(world.get::<KartWheels>(kart).unwrap().offroad_ratio, 0.0);
    }

    /// Simple test driver: aims at a point of the centerline ahead and lifts off in tight turns.
    pub(crate) fn follow_centerline(
        layout: &track::TrackLayout,
        transform: &Transform,
        speed: f32,
    ) -> kart::KartInput {
        let center = &layout.center;
        let here = center.samples()[center.nearest_index(transform.translation)].distance;
        let target = center.at(here + 6.0 + 0.4 * speed).position;
        let to_target = target - transform.translation;
        let angle = to_target
            .dot(*transform.right())
            .atan2(to_target.dot(*transform.forward()));
        kart::KartInput {
            throttle: if angle.abs() > 0.25 && speed > 16.0 {
                0.0
            } else {
                1.0
            },
            brake: if angle.abs() > 0.5 && speed > 12.0 {
                1.0
            } else {
                0.0
            },
            steer: (angle * 3.0).clamp(-1.0, 1.0),
            ..default()
        }
    }

    /// Issue #3 acceptance: the kart of #1 can drive a full lap, through every checkpoint in
    /// order (hairpin, jump, chicane, banked turn), without leaving the track.
    #[test]
    fn kart_drives_a_full_lap_of_the_circuit() {
        let mut app = app_on(TrackChoice::default());
        let kart = player(&mut app);
        // The test drives, not the keyboard.
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
        let layout = app.world().resource::<track::TrackLayout>().clone();
        let length = layout.center.length();

        let center = &layout.center;
        let checkpoints = &layout.checkpoints;
        let handle = app.world().resource::<track::TrackHandle>().0.clone();
        let data = app
            .world()
            .resource::<Assets<track::TrackData>>()
            .get(&handle)
            .unwrap()
            .clone();
        let ramp = data.trick_ramps[0];
        let ramp_start = center.param_to_distance(ramp.at);

        // Distance to drive from the grid to reach checkpoint `k` (the last one closes the lap:
        // back on the start line).
        let start = center.samples()[center.nearest_index(layout.grid[0].translation)].distance;
        let to_reach = |k: usize| {
            if k == checkpoints.len() {
                center.wrap(-start) + length
            } else {
                center.wrap(checkpoints[k].sample.distance - start)
            }
        };

        let step = length / center.samples().len() as f32;
        let n = center.samples().len() as i64;
        let mut last = center.nearest_index(layout.grid[0].translation);
        let mut progress = 0.0;
        let mut next_checkpoint = 1;
        let mut airborne_ticks = 0;
        let mut ticks = 0;
        while next_checkpoint <= checkpoints.len() && ticks < (90.0 * TICK_HZ) as usize {
            ticks += 1;
            let world = app.world();
            let transform = *world.get::<Transform>(kart).unwrap();
            let speed = world.get::<Velocity>(kart).unwrap().linear.length();
            let input = follow_centerline(&layout, &transform, speed);
            *app.world_mut().get_mut::<kart::KartInput>(kart).unwrap() = input;
            app.update();

            let world = app.world();
            let position = world.get::<Transform>(kart).unwrap().translation;
            let index = center.nearest_index(position);
            let sample = center.samples()[index];
            // Always between the walls...
            let lateral = (position - sample.position).dot(sample.right).abs();
            assert!(
                lateral < track::mesh::wall_offset(&sample),
                "left the track at {position}"
            );
            // ...and only in the air after the trick ramp.
            if world.get::<KartState>(kart) == Some(&KartState::Airborne) {
                airborne_ticks += 1;
                let after_ramp = center.wrap(sample.distance - ramp_start);
                assert!(
                    after_ramp < ramp.length + 40.0,
                    "airborne away from the ramp at {position}"
                );
            }
            // Progress along the centerline (wrapping around the start line), never backwards.
            let delta = (index as i64 - last as i64 + n + n / 2).rem_euclid(n) - n / 2;
            assert!(delta > -5, "the kart went backwards at {position}");
            progress += delta as f32 * step;
            last = index;
            while next_checkpoint <= checkpoints.len() && progress >= to_reach(next_checkpoint) {
                next_checkpoint += 1;
            }
        }
        assert!(
            next_checkpoint > checkpoints.len(),
            "stuck before checkpoint {next_checkpoint} of {} after {:.0} s (progress {progress:.0} m)",
            checkpoints.len(),
            ticks as f32 / TICK_HZ as f32
        );
        assert!(
            airborne_ticks > 10,
            "the jump was not taken ({airborne_ticks} ticks in the air)"
        );
    }

    /// A missing or broken circuit file must not leave a blank screen.
    #[test]
    fn a_missing_circuit_file_falls_back_to_the_test_ground() {
        let mut app = app_on(TrackChoice::Circuit("tracks/missing.track.ron".into()));
        let kart = player(&mut app);
        let position = app.world().get::<Transform>(kart).unwrap().translation;
        assert!(
            position.distance(Vec3::new(0.0, 0.55, 0.0)) < 0.1,
            "kart at {position}"
        );
        assert!(app.world().get_resource::<track::TrackLayout>().is_none());
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
        let spawn = app.world().resource::<PlayerStart>().0.translation;
        assert!(position.distance(spawn) < 0.5, "kart at {position}");
    }
}
