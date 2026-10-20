//! Headless scenarios of the admin panel (issue #36): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, one fixed tick per update. The egui interface is not there (no renderer):
//! these tests go through the same functions as its buttons and sliders.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiInput, EguiOutput, EguiRenderOutput, PrimaryEguiContext};
use bevy_rapier3d::prelude::Velocity;

use super::tracks::circuits;
use super::tuning::Value;
use super::ui::attach_egui_context;
use super::{AdminOptions, AdminPanel, KeyboardCapture, Tunings, skip_countdown_now};
use crate::TICK_HZ;
use crate::audio::SoundEffect;
use crate::bots::Bot;
use crate::camera::ChaseCamera;
use crate::kart::{Kart, KartTuning, PlayerKart, SpawnPoint};
use crate::level::LoadLevel;
use crate::race::{Race, RaceMode, RacePhase, Racer};
use crate::tests::{app_on, app_with_bots, hold_keys, player};
use crate::track::{BoostPad, CroquetteSpot, TrackChoice, TrackLayout};

fn run(app: &mut App, seconds: f32) {
    for _ in 0..(seconds * TICK_HZ as f32).round() as usize {
        app.update();
    }
}

fn speed(app: &mut App) -> f32 {
    let kart = player(app);
    app.world().get::<Velocity>(kart).unwrap().linear.length()
}

/// Runs updates until `done` (a circuit file may have to load first).
fn update_until(app: &mut App, what: &str, done: impl Fn(&mut World) -> bool) {
    for _ in 0..2000 {
        app.update();
        if done(app.world_mut()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let world = app.world_mut();
    panic!(
        "{what} never happened: {} player kart(s), {} bot(s), race {}, mode {:?}, layout {:?}",
        count::<With<PlayerKart>>(world),
        count::<With<Bot>>(world),
        world.contains_resource::<Race>(),
        world.resource::<RaceMode>(),
        world.get_resource::<TrackLayout>().map(|l| l.name.clone()),
    );
}

/// A key pressed this frame (`just_pressed`), as the window sends it.
fn tap(app: &mut App, key_code: KeyCode, logical_key: Key) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

fn count<F: bevy::ecs::query::QueryFilter>(world: &mut World) -> usize {
    world.query_filtered::<(), F>().iter(world).count()
}

/// The level is built: the player kart, and in a race the race and its `bots` bots.
fn level_ready(world: &mut World, bots: usize) -> bool {
    let race = *world.resource::<RaceMode>() == RaceMode::Race;
    count::<With<PlayerKart>>(world) == 1
        && (!race || (world.contains_resource::<Race>() && count::<With<Bot>>(world) == bots))
}

/// Everything that could leak: entities, meshes and materials. Sounds are left out: they end on
/// their own in the game, but never in the tests (no audio output).
fn census(app: &mut App) -> (usize, usize, usize) {
    let world = app.world_mut();
    (
        world
            .query_filtered::<Entity, Without<SoundEffect>>()
            .iter(world)
            .count(),
        world.resource::<Assets<Mesh>>().len(),
        world.resource::<Assets<StandardMaterial>>().len(),
    )
}

fn load(app: &mut App, request: LoadLevel) {
    app.world_mut().write_message(request);
}

fn with_tunings<R>(app: &mut App, f: impl FnOnce(&mut World, &Tunings) -> R) -> R {
    app.world_mut()
        .resource_scope(|world, tunings: Mut<Tunings>| f(world, &tunings))
}

/// Acceptance criterion of #36: a value changed the way the panel changes it (its slider, or a
/// pasted text) changes how the kart drives: the top speed it reaches.
#[test]
fn a_tuning_edited_in_the_panel_changes_the_driving() {
    // Full throttle for 2.8 s: past the 2.5 s it takes to reach the top speed, before the ramp of
    // the test ground (70 m ahead).
    let top_speed = |edit: &dyn Fn(&mut App)| {
        let mut app = app_on(TrackChoice::TestGround);
        edit(&mut app);
        hold_keys(&mut app, &[KeyCode::ArrowUp]);
        run(&mut app, 2.8);
        speed(&mut app)
    };
    let default = top_speed(&|_| {});
    assert!((default - 28.5).abs() < 0.3, "{default}");

    let slider = top_speed(&|app| {
        with_tunings(app, |world, tunings| {
            tunings
                .set(world, "KartTuning.max_speed", Value::F32(20.0))
                .unwrap();
        });
    });
    assert!((slider - 20.0).abs() < 0.3, "{slider}");

    let pasted = top_speed(&|app| {
        let report = with_tunings(app, |world, tunings| {
            tunings
                .import(world, "{ \"KartTuning.max_speed\": 33.0 }")
                .unwrap()
        });
        assert_eq!(report.applied, 1);
    });
    assert!((pasted - 33.0).abs() < 0.3, "{pasted}");
}

/// « Relancer »: the level is built again from scratch (new karts on the grid, race back to the
/// grid phase), and nothing is left behind: same entities, meshes and materials as the first
/// time.
#[test]
fn restarting_rebuilds_the_level_without_leaking() {
    let mut app = app_with_bots(TrackChoice::default(), 3);
    update_until(&mut app, "the race", |w| level_ready(w, 3));
    run(&mut app, 0.5);
    let first = census(&mut app);
    let old_player = player(&mut app);

    // Some race first: GO, then drive.
    assert!(skip_countdown_now(app.world_mut()));
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 2.0);
    assert!(app.world().resource::<Race>().started());

    load(&mut app, LoadLevel::restart(RaceMode::Race));
    hold_keys(&mut app, &[]);
    update_until(&mut app, "the new race", |w| level_ready(w, 3));
    run(&mut app, 0.5);
    assert_eq!(census(&mut app), first);

    let kart = player(&mut app);
    assert_ne!(kart, old_player);
    assert!(app.world().get_entity(old_player).is_err());
    let world = app.world();
    let layout = world.resource::<TrackLayout>();
    let slot = world.get::<Racer>(kart).unwrap().grid_slot;
    assert_eq!(slot, 3);
    let position = world.get::<Transform>(kart).unwrap().translation;
    assert!(
        position.distance(layout.grid[slot].translation) < 0.5,
        "{position}"
    );
    assert_eq!(world.resource::<Race>().phase(), RacePhase::Grid);
    // One camera, following the new kart.
    let world = app.world_mut();
    let cameras: Vec<ChaseCamera> = world.query::<&ChaseCamera>().iter(world).cloned().collect();
    assert_eq!(cameras.len(), 1);
    assert!(world.get_entity(cameras[0].target).is_ok());
}

/// Acceptance criterion of #36: choosing another circuit builds that one (its grid, its
/// croquettes and boost pads, its race), with nothing left of the previous one; coming back
/// gives exactly the first level again.
#[test]
fn switching_the_circuit_builds_the_chosen_one() {
    let entries = circuits();
    let ring = entries
        .iter()
        .find(|e| e.name == "Anneau Guimauve")
        .unwrap()
        .clone();
    let mut app = app_with_bots(TrackChoice::default(), 2);
    update_until(&mut app, "the race", |w| level_ready(w, 2));
    run(&mut app, 0.5);
    let first = census(&mut app);
    // Issue #108: the croquettes of Prairie Pompon redrawn (22 before).
    let prairie_croquettes = count::<With<CroquetteSpot>>(app.world_mut());
    assert_eq!(prairie_croquettes, 28);

    load(&mut app, LoadLevel::on(ring.choice(), RaceMode::Race));
    update_until(&mut app, "the ring", |w| {
        w.get_resource::<TrackLayout>()
            .is_some_and(|l| l.name == "Anneau Guimauve")
            && level_ready(w, 2)
    });
    run(&mut app, 0.5);
    let world = app.world_mut();
    assert_eq!(*world.resource::<TrackChoice>(), ring.choice());
    assert_eq!(count::<With<CroquetteSpot>>(world), 10);
    assert_eq!(count::<With<BoostPad>>(world), 1);
    assert_eq!(count::<With<Kart>>(world), 3);
    let names: Vec<String> = world
        .query::<&Name>()
        .iter(world)
        .map(|n| n.to_string())
        .collect();
    assert!(names.contains(&"Track: Anneau Guimauve".to_string()));
    assert!(!names.iter().any(|n| n.contains("Prairie")), "{names:?}");
    let kart = player(&mut app);
    let world = app.world();
    let layout = world.resource::<TrackLayout>();
    let position = world.get::<Transform>(kart).unwrap().translation;
    assert!(
        position.distance(layout.grid[2].translation) < 0.5,
        "{position}"
    );
    assert_eq!(
        world.resource::<crate::race::Course>().center.length(),
        layout.center.length()
    );

    // The test ground: no race there.
    load(
        &mut app,
        LoadLevel::on(TrackChoice::TestGround, RaceMode::Race),
    );
    update_until(&mut app, "the test ground", |w| {
        !w.contains_resource::<TrackLayout>() && count::<With<PlayerKart>>(w) == 1
    });
    run(&mut app, 0.2);
    let world = app.world_mut();
    assert!(!world.contains_resource::<Race>());
    assert_eq!(count::<With<Kart>>(world), 1);
    assert_eq!(count::<With<CroquetteSpot>>(world), 0);

    // Back on Prairie Pompon: the same level as at the start.
    load(
        &mut app,
        LoadLevel::on(TrackChoice::default(), RaceMode::Race),
    );
    update_until(&mut app, "Prairie Pompon again", |w| {
        w.get_resource::<TrackLayout>()
            .is_some_and(|l| l.name == "Prairie Pompon")
            && level_ready(w, 2)
    });
    run(&mut app, 0.5);
    assert_eq!(census(&mut app), first);
}

/// « Roulage libre »: the circuit without race nor bots, the player on the pole drives at once
/// and goes back there with R; « Course » brings the race and the bots back.
#[test]
fn free_driving_has_no_race_and_no_bots() {
    let mut app = app_with_bots(TrackChoice::default(), 3);
    update_until(&mut app, "the race", |w| level_ready(w, 3));
    load(&mut app, LoadLevel::restart(RaceMode::FreeRoam));
    update_until(&mut app, "free driving", |w| {
        *w.resource::<RaceMode>() == RaceMode::FreeRoam && level_ready(w, 0)
    });
    run(&mut app, 0.2);
    let kart = player(&mut app);
    let world = app.world_mut();
    assert!(!world.contains_resource::<Race>());
    assert_eq!(count::<With<Bot>>(world), 0);
    assert!(world.get::<Racer>(kart).is_none());
    let pole = world.resource::<TrackLayout>().grid[0];
    assert_eq!(world.get::<SpawnPoint>(kart).unwrap().0, pole);

    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 1.5);
    assert!(speed(&mut app) > 15.0, "{}", speed(&mut app));
    hold_keys(&mut app, &[]);
    tap(&mut app, KeyCode::KeyR, Key::Character("r".into()));
    app.update();
    let position = app.world().get::<Transform>(kart).unwrap().translation;
    assert!(position.distance(pole.translation) < 0.5, "{position}");

    load(&mut app, LoadLevel::restart(RaceMode::Race));
    update_until(&mut app, "the race again", |w| level_ready(w, 3));
}

/// « Toujours sauter le compte à rebours »: the race starts at GO, the player drives at once
/// (no stall for an accelerator held from the start: no countdown, no rocket start judged).
#[test]
fn the_countdown_can_be_skipped() {
    let mut app = app_on(TrackChoice::default());
    app.world_mut()
        .resource_mut::<AdminOptions>()
        .skip_countdown = true;
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 1.0);
    assert!(app.world().resource::<Race>().started());
    assert!(speed(&mut app) > 12.0, "{}", speed(&mut app));

    // Without it, the karts are still waiting on the grid after 1 s.
    let mut app = app_on(TrackChoice::default());
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 1.0);
    assert!(!app.world().resource::<Race>().started());
    assert!(speed(&mut app) < 0.1);
    // « GO ! » mid-countdown.
    run(&mut app, 1.5);
    assert_eq!(app.world().resource::<Race>().phase(), RacePhase::Countdown);
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    assert!(app.world().resource::<Race>().started());
    assert!(!skip_countdown_now(app.world_mut()), "already racing");
}

/// While a text field of the panel has the keyboard, the game gets no key: the kart does not
/// move, P does not close the panel. Once released, the keys drive again.
#[test]
fn keys_typed_in_the_panel_do_not_reach_the_game() {
    let mut app = app_on(TrackChoice::TestGround);
    app.world_mut().resource_mut::<AdminPanel>().open = true;
    app.world_mut().resource_mut::<KeyboardCapture>().0 = true;
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 1.0);
    assert!(speed(&mut app) < 0.1, "{}", speed(&mut app));
    tap(&mut app, KeyCode::KeyP, Key::Character("p".into()));
    assert!(app.world().resource::<AdminPanel>().open);

    app.world_mut().resource_mut::<KeyboardCapture>().0 = false;
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 1.0);
    assert!(speed(&mut app) > 10.0, "{}", speed(&mut app));
}

#[test]
fn the_admin_key_opens_and_closes_the_panel() {
    let mut app = app_on(TrackChoice::TestGround);
    assert!(!app.world().resource::<AdminPanel>().open);
    for open in [true, false, true] {
        tap(&mut app, KeyCode::KeyP, Key::Character("p".into()));
        assert_eq!(app.world().resource::<AdminPanel>().open, open);
    }
}

/// Acceptance criterion of #36: closed, the panel costs nothing. The egui context (what egui
/// reads input for, runs its pass on and draws) only exists while the panel is open: closing it
/// removes every egui component, and the keyboard goes back to the game.
#[test]
fn closed_the_panel_has_no_egui_context() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<AdminPanel>()
        .init_resource::<KeyboardCapture>()
        .add_systems(Update, attach_egui_context);
    let camera = app
        .world_mut()
        .spawn(ChaseCamera::new(Entity::PLACEHOLDER))
        .id();
    let egui_parts = |app: &mut App| {
        let world = app.world_mut();
        count::<
            Or<(
                With<PrimaryEguiContext>,
                With<EguiContext>,
                With<EguiInput>,
                With<EguiOutput>,
                With<EguiRenderOutput>,
            )>,
        >(world)
    };
    app.update();
    assert_eq!(egui_parts(&mut app), 0);

    app.world_mut().resource_mut::<AdminPanel>().open = true;
    app.update();
    assert!(app.world().get::<EguiContext>(camera).is_some());
    app.world_mut().resource_mut::<KeyboardCapture>().0 = true;

    app.world_mut().resource_mut::<AdminPanel>().open = false;
    app.update();
    assert_eq!(egui_parts(&mut app), 0);
    assert!(app.world().get::<ChaseCamera>(camera).is_some());
    assert!(!app.world().resource::<KeyboardCapture>().0);
}

/// Issue #17: the options menu had an egui context while it was open. Issue #100: its screen is
/// `bevy_ui` now: open, it needs no egui context; the panel opening over it brings one, and
/// closing the panel takes it away while the options stay open; closed, nothing either.
#[test]
fn the_options_screen_needs_no_egui_context() {
    use crate::options::OptionsMenu;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<AdminPanel>()
        .init_resource::<KeyboardCapture>()
        .init_resource::<OptionsMenu>()
        .add_systems(Update, attach_egui_context);
    let camera = app
        .world_mut()
        .spawn(ChaseCamera::new(Entity::PLACEHOLDER))
        .id();
    let egui_parts = |app: &mut App| {
        let world = app.world_mut();
        count::<
            Or<(
                With<PrimaryEguiContext>,
                With<EguiContext>,
                With<EguiRenderOutput>,
            )>,
        >(world)
    };
    app.update();
    assert_eq!(egui_parts(&mut app), 0);
    app.world_mut().resource_mut::<OptionsMenu>().open = true;
    app.update();
    assert_eq!(egui_parts(&mut app), 0, "the options are bevy_ui");
    // The panel opens over it (a context for the panel), then closes: none is left.
    app.world_mut().resource_mut::<AdminPanel>().open = true;
    app.update();
    assert!(app.world().get::<EguiContext>(camera).is_some());
    app.world_mut().resource_mut::<AdminPanel>().open = false;
    app.update();
    assert_eq!(egui_parts(&mut app), 0);
    app.world_mut().resource_mut::<OptionsMenu>().open = false;
    app.update();
    assert_eq!(egui_parts(&mut app), 0);
}

/// Issue #7: the lobby window was egui too, a context only while it was open; issue #64: its
/// banner too, while shown. Issue #101: both are `bevy_ui` now (the lobby's screen of the
/// charter, `crate::salon`): open or shown, they need no egui context, and never pause the game;
/// the panel opening over them brings one, and closing it takes it away while the lobby stays
/// open.
#[test]
fn the_lobby_and_its_banner_need_no_egui_context() {
    use crate::net::LobbyWindow;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<AdminPanel>()
        .init_resource::<KeyboardCapture>()
        .init_resource::<LobbyWindow>()
        .add_systems(Update, attach_egui_context);
    let camera = app
        .world_mut()
        .spawn(ChaseCamera::new(Entity::PLACEHOLDER))
        .id();
    let egui_parts = |app: &mut App| {
        let world = app.world_mut();
        count::<
            Or<(
                With<PrimaryEguiContext>,
                With<EguiContext>,
                With<EguiRenderOutput>,
            )>,
        >(world)
    };
    app.update();
    assert_eq!(egui_parts(&mut app), 0);
    app.world_mut().resource_mut::<LobbyWindow>().open = true;
    app.update();
    assert_eq!(egui_parts(&mut app), 0, "the lobby's screen is bevy_ui");
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    app.world_mut().resource_mut::<LobbyWindow>().open = false;
    app.world_mut().resource_mut::<LobbyWindow>().banner = true;
    app.update();
    assert_eq!(egui_parts(&mut app), 0, "the banner is bevy_ui");
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    // The panel opens over them (a context for the panel), then closes: none is left.
    app.world_mut().resource_mut::<LobbyWindow>().open = true;
    app.world_mut().resource_mut::<AdminPanel>().open = true;
    app.update();
    assert!(app.world().get::<EguiContext>(camera).is_some());
    app.world_mut().resource_mut::<AdminPanel>().open = false;
    app.update();
    assert_eq!(egui_parts(&mut app), 0);
}

/// The tunings registered for the panel are the game's resources, and every one of them is
/// found by reflection. Issue #15 added `StatsTuning` (the bounds of the stats' effects), issue
/// #16 `ItemTuning` (the items, and the switch that takes them out of the races).
#[test]
fn every_tuning_resource_is_in_the_panel() {
    let mut app = app_on(TrackChoice::TestGround);
    let names: Vec<&str> = app
        .world()
        .resource::<Tunings>()
        .list
        .iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(
        names,
        [
            "KartTuning",
            "CroquetteTuning",
            "BotTuning",
            "CameraTuning",
            "RaceTuning",
            "StatsTuning",
            "ItemTuning",
            "InteractionTuning",
            "NetTuning",
            "DriverTuning",
            "WobbleTuning",
            "GrandPrixTuning"
        ]
    );
    with_tunings(&mut app, |world, tunings| {
        for tuning in &tunings.list {
            assert!(tuning.read(world).is_some(), "{} missing", tuning.name);
            assert!(tuning.defaults.len() >= 10, "{}", tuning.name);
        }
        assert!(tunings.changes(world).is_empty());
        // The bots' skills are among them (issue #153: their level is the grid's, not a tuning).
        tunings
            .set(
                world,
                "BotTuning.skills[2].target_tier",
                Value::Variant("Purple"),
            )
            .unwrap();
        assert!(
            tunings
                .set(world, "BotTuning.grid_difficulty", Value::Variant("Hard"))
                .is_err()
        );
        // Issue #16: the items are on by default, and the panel switches them off.
        assert!(world.resource::<crate::items::ItemTuning>().enabled);
        tunings
            .set(world, "ItemTuning.enabled", Value::Bool(false))
            .unwrap();
        // Issue #34: the interactions too.
        assert!(
            world
                .resource::<crate::interactions::InteractionTuning>()
                .enabled
        );
        tunings
            .set(world, "InteractionTuning.enabled", Value::Bool(false))
            .unwrap();
    });
    assert_eq!(
        app.world().resource::<crate::bots::BotTuning>().skills[2].target_tier,
        crate::kart::MiniTurbo::Purple
    );
    assert!(!app.world().resource::<crate::items::ItemTuning>().enabled);
    assert!(
        !app.world()
            .resource::<crate::interactions::InteractionTuning>()
            .enabled
    );
    let _ = app.world().resource::<KartTuning>();
}
