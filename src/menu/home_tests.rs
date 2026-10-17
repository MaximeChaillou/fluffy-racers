//! Headless scenarios of the home screen and its background (issue #89): the whole game
//! (`GamePlugin`) with `MinimalPlugins`, one fixed tick per update, [`MainMenuPlugin`] added by
//! hand (the game only adds it with a renderer). The background's circuit file loads from
//! `assets/`, as in the bot races.

use bevy::input::ButtonState as KeyState;
use bevy::input::gamepad::{GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy_rapier3d::prelude::Velocity;

use super::screen::MenuText;
use super::*;
use crate::admin::Value;
use crate::bots::{Bot, OwnSkill};
use crate::camera::ChaseCamera;
use crate::interactions::PawStats;
use crate::interpolation::InterpolatedFrom;
use crate::kart::{Kart, PlayerKart};
use crate::race::{Course, RaceGrid, RaceProgress, RaceTuning};
use crate::tests::{game_app_unloaded, wait_for_the_player};
use crate::track::TrackChoice;
use crate::ui::HudRoot;

/// The game with `bots` bots on the player's grid, the home screen at launch (or not), on the
/// flat test ground: the background goes to a circuit by itself.
fn home_app(bots: usize, at_launch: bool) -> App {
    let mut app = game_app_unloaded(TrackChoice::TestGround, bots);
    app.add_plugins(MainMenuPlugin { at_launch });
    wait_for_the_player(&mut app);
    app
}

/// Runs updates until the home's background races (its circuit file loads first).
fn wait_for_the_background(app: &mut App) {
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<Showcase>()
            .is_some_and(|showcase| showcase.running())
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the home's background never raced");
}

fn press(app: &mut App, key_code: KeyCode, state: KeyState) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.update();
}

fn tap(app: &mut App, key_code: KeyCode) {
    press(app, key_code, KeyState::Pressed);
    press(app, key_code, KeyState::Released);
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// What the focused entry does comes after its stretch.
fn wait_for_the_action(app: &mut App) {
    run(app, (ACTIVATE_DELAY * 60.0).ceil() as usize + 3);
}

fn choose(app: &mut App) {
    tap(app, KeyCode::Enter);
    wait_for_the_action(app);
}

fn menu(app: &App) -> MainMenu {
    *app.world().resource::<MainMenu>()
}

fn focused(app: &App) -> Entry {
    menu(app).focused()
}

fn paused(app: &App) -> bool {
    app.world().resource::<Time<Virtual>>().is_paused()
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

fn showcase(app: &App) -> Option<&Showcase> {
    app.world().get_resource::<Showcase>()
}

/// The background's karts' mean speed (m/s).
fn background_speed(app: &mut App) -> f32 {
    let world = app.world_mut();
    let speeds: Vec<f32> = world
        .query_filtered::<&Velocity, With<ShowcaseKart>>()
        .iter(world)
        .map(|v| v.linear.length())
        .collect();
    speeds.iter().sum::<f32>() / speeds.len().max(1) as f32
}

/// The background runs behind the home (or a screen of it): its karts race, the game is not
/// paused, there is no player's kart.
fn assert_the_background_runs(app: &mut App, karts: usize, context: &str) {
    assert!(
        showcase(app).is_some_and(|s| s.running()),
        "{context}: no background"
    );
    assert!(!paused(app), "{context}: paused");
    assert_eq!(count::<With<ShowcaseKart>>(app), karts, "{context}");
    assert_eq!(
        count::<With<Kart>>(app),
        karts,
        "{context}: only the background's karts"
    );
    assert_eq!(count::<With<PlayerKart>>(app), 0, "{context}");
    let before = background_speed(app);
    run(app, 30);
    let after = background_speed(app);
    assert!(
        after > 5.0,
        "{context}: they race ({before} then {after} m/s)"
    );
}

fn texts_of<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query_filtered::<&Text, F>()
        .iter(world)
        .map(|t| t.0.clone())
        .collect()
}

fn options_open(app: &App) -> bool {
    app.world().resource::<OptionsMenu>().open
}

fn selection_open(app: &App) -> bool {
    app.world().resource::<SelectionScreen>().open
}

fn race_menu_open(app: &App) -> bool {
    app.world().resource::<RaceMenu>().open
}

fn lobby_open(app: &App) -> bool {
    app.world().resource::<LobbyWindow>().open
}

/// Issue #89: the game opens on the home screen: its big title (two words), its phrase, its four
/// buttons in this order (Jouer solo, Multijoueur, Options, Pilote), over its background, which
/// races: a few karts, the player's kart one of them (a bot now), no pause.
#[test]
fn the_game_opens_on_the_home_screen_and_its_background_races() {
    let mut app = home_app(11, true);
    wait_for_the_background(&mut app);
    run(&mut app, 3);
    assert!(menu(&app).open && !menu(&app).in_game && menu(&app).on_home());
    assert_eq!(menu(&app).entries(), &LAUNCH_ENTRIES);
    assert_eq!(focused(&app), Entry::Solo);
    assert_eq!(count::<With<MainMenuRoot>>(&mut app), 1);
    assert_eq!(count::<With<MenuButton>>(&mut app), 4);
    let labels: Vec<String> = {
        let world = app.world_mut();
        let mut labels: Vec<(usize, String)> = world
            .query::<(&MenuText, &Text)>()
            .iter(world)
            .filter(|(t, _)| !t.caption)
            .map(|(t, text)| (t.index, text.0.clone()))
            .collect();
        labels.sort();
        labels.into_iter().map(|(_, l)| l).collect()
    };
    assert_eq!(labels, ["Jouer solo", "Multijoueur", "Options", "Pilote"]);
    let mut title = texts_of::<With<TitleWord>>(&mut app);
    title.sort();
    assert_eq!(title, ["Fluffy", "Racers"]);
    assert!(
        texts_of::<With<MenuNode>>(&mut app)
            .iter()
            .any(|t| t == "Des karts tout doux, des virages tout fous.")
    );
    // The home's veil, fading toward the right; its title's words bob.
    assert_eq!(
        count::<(With<MainMenuRoot>, With<BackgroundGradient>)>(&mut app),
        1
    );
    let pose = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(&TitleWord, &UiTransform)>()
            .iter(world)
            .find(|(w, _)| w.0 == 0)
            .map(|(_, t)| *t)
            .unwrap()
    };
    let a = pose(&mut app);
    run(&mut app, 20);
    assert_ne!(a, pose(&mut app), "the title is alive");

    let karts = HomeTuning::default().kart_count();
    assert_the_background_runs(&mut app, karts, "at launch");
    // The player's grid is untouched: 11 bots for the next race.
    assert_eq!(app.world().resource::<RaceGrid>().bots, 11);
    // The player's kart is one of them, driven by a bot.
    assert_eq!(count::<(With<Bot>, With<ShowcaseKart>)>(&mut app), karts);
    assert_eq!(count::<With<OwnSkill>>(&mut app), karts);
    // An endless race, started at GO, no screen of it.
    assert_eq!(
        app.world().resource::<Course>().laps,
        SHOWCASE_LAPS,
        "nobody finishes"
    );
    assert!(app.world().resource::<Race>().started());
    let world = app.world_mut();
    let huds: Vec<Display> = world
        .query_filtered::<&Node, With<HudRoot>>()
        .iter(world)
        .map(|n| n.display)
        .collect();
    assert!(huds.iter().all(|d| *d == Display::None), "{huds:?}");
    // No sound of the race: they are the player's kart's, and there is none.
    assert_eq!(count::<With<crate::audio::SoundEffect>>(&mut app), 0);
}

/// Issue #89: the keyboard goes through the home's four buttons, and each one opens its screen
/// (Jouer solo: the race menu, N; Multijoueur: the lobby, L; Options: O; Pilote: the selection,
/// G); closing it without starting anything comes back to the home, the focus on its button. The
/// background runs behind every one of them.
#[test]
fn the_keys_open_each_screen_of_the_home_and_come_back() {
    let mut app = home_app(0, true);
    wait_for_the_background(&mut app);
    let karts = HomeTuning::default().kart_count();
    let screens: [(Entry, fn(&App) -> bool, KeyCode); 4] = [
        (Entry::Solo, race_menu_open, KeyCode::Escape),
        (Entry::Online, lobby_open, crate::net::LOBBY_KEY),
        (Entry::Options, options_open, KeyCode::Escape),
        (Entry::Garage, selection_open, KeyCode::Escape),
    ];
    for (index, (entry, open, close)) in screens.into_iter().enumerate() {
        if index > 0 {
            tap(&mut app, KeyCode::ArrowDown);
        }
        assert_eq!(focused(&app), entry);
        choose(&mut app);
        assert!(open(&app) && !menu(&app).open, "{entry:?}");
        assert!(menu(&app).on_home(), "{entry:?}: over the home");
        assert_the_background_runs(&mut app, karts, entry.label());
        tap(&mut app, close);
        assert!(!open(&app), "{entry:?}");
        run(&mut app, 2);
        assert!(
            menu(&app).open && !menu(&app).in_game,
            "{entry:?}: back home"
        );
        assert_eq!(focused(&app), entry);
        assert_the_background_runs(&mut app, karts, "back home");
    }
    // Up goes around.
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(focused(&app), Entry::Solo);
}

/// Issue #89: the gamepad too: the cross moves through the home's buttons, A opens each screen.
#[test]
fn the_gamepad_opens_each_screen_of_the_home() {
    let mut app = home_app(0, true);
    wait_for_the_background(&mut app);
    let pad = app
        .world_mut()
        .spawn((Gamepad::default(), GamepadSettings::default()))
        .id();
    let button = |app: &mut App, button: GamepadButton| {
        for value in [1.0, 0.0] {
            app.world_mut().write_message(RawGamepadEvent::Button(
                RawGamepadButtonChangedEvent::new(pad, button, value),
            ));
            app.update();
        }
    };
    let screens: [(Entry, fn(&App) -> bool, KeyCode); 4] = [
        (Entry::Solo, race_menu_open, KeyCode::Escape),
        (Entry::Online, lobby_open, crate::net::LOBBY_KEY),
        (Entry::Options, options_open, KeyCode::Escape),
        (Entry::Garage, selection_open, KeyCode::Escape),
    ];
    for (index, (entry, open, close)) in screens.into_iter().enumerate() {
        for _ in 0..index {
            button(&mut app, GamepadButton::DPadDown);
        }
        assert_eq!(focused(&app), entry);
        button(&mut app, GamepadButton::South);
        wait_for_the_action(&mut app);
        assert!(open(&app) && !menu(&app).open, "{entry:?}");
        tap(&mut app, close);
        run(&mut app, 2);
        assert!(menu(&app).open, "{entry:?}: back home");
        assert_eq!(focused(&app), entry);
        // Back to the top.
        for _ in 0..index {
            button(&mut app, GamepadButton::DPadUp);
        }
    }
    // B (or Start) on the home races at once, like Échap.
    button(&mut app, GamepadButton::East);
    wait_for_the_player(&mut app);
    run(&mut app, 3);
    assert!(!menu(&app).open && showcase(&app).is_none());
    assert_eq!(count::<With<PlayerKart>>(&mut app), 1);
}

/// Issue #89: a race started from the home (Jouer solo, then Entrée in the race menu) leaves
/// nothing of it: no tree, no background (karts, resource, skills), the player's grid and its
/// laps, the HUD, the chase camera. « Quitter » in the menu opened from the race brings the home
/// back, and its background.
#[test]
fn a_race_from_the_home_leaves_nothing_of_it_and_quitting_comes_back() {
    let mut app = home_app(3, true);
    wait_for_the_background(&mut app);
    let karts = HomeTuning::default().kart_count();
    assert_ne!(karts, 4, "the race's grid is told apart");
    choose(&mut app);
    assert!(race_menu_open(&app));
    tap(&mut app, KeyCode::Enter);
    wait_for_the_player(&mut app);
    run(&mut app, 10);
    assert!(!menu(&app).open && !menu(&app).on_home());
    assert!(showcase(&app).is_none(), "the background is gone");
    assert_eq!(count::<With<MenuNode>>(&mut app), 0, "no tree");
    assert_eq!(count::<With<ShowcaseKart>>(&mut app), 0);
    assert_eq!(count::<With<OwnSkill>>(&mut app), 0);
    assert_eq!(
        count::<With<Kart>>(&mut app),
        4,
        "the player's grid: 3 bots and the player"
    );
    assert_eq!(count::<With<PlayerKart>>(&mut app), 1);
    assert_eq!(app.world().resource::<RaceGrid>().bots, 3);
    assert_eq!(
        app.world().resource::<Course>().laps,
        RaceTuning::default().laps
    );
    let world = app.world_mut();
    let huds: Vec<Display> = world
        .query_filtered::<&Node, With<HudRoot>>()
        .iter(world)
        .map(|n| n.display)
        .collect();
    assert_eq!(huds.len(), 1);
    assert_ne!(huds[0], Display::None, "the HUD is back");
    // The camera chases the player's kart again.
    let player = crate::tests::player(&mut app);
    let world = app.world_mut();
    let target = world.query::<&ChaseCamera>().single(world).unwrap().target;
    assert_eq!(
        world.get::<InterpolatedFrom>(target).map(|f| f.0),
        Some(player)
    );
    // The countdown: a race as usual.
    assert!(!app.world().resource::<Race>().started());

    // Échap, then « Quitter » (the last button: up from « Reprendre », around).
    tap(&mut app, KeyCode::Escape);
    assert!(menu(&app).open && menu(&app).in_game);
    assert_eq!(menu(&app).entries(), &GAME_ENTRIES);
    assert!(paused(&app), "the menu from the game pauses it in solo");
    tap(&mut app, KeyCode::ArrowUp);
    assert_eq!(focused(&app), Entry::Quit);
    choose(&mut app);
    assert!(menu(&app).open && !menu(&app).in_game, "home");
    assert_eq!(menu(&app).entries(), &LAUNCH_ENTRIES);
    wait_for_the_background(&mut app);
    run(&mut app, 3);
    assert_the_background_runs(&mut app, karts, "after « Quitter »");
    assert_eq!(app.world().resource::<RaceGrid>().bots, 3);

    // Échap on the home: a race at once, the player's grid.
    tap(&mut app, KeyCode::Escape);
    wait_for_the_player(&mut app);
    run(&mut app, 5);
    assert!(!menu(&app).open && showcase(&app).is_none());
    assert_eq!(count::<With<Kart>>(&mut app), 4);
    assert_eq!(count::<With<MenuNode>>(&mut app), 0);
}

/// Issue #89: the background's karts stay together and their paws fly: over 40 s, every kart
/// strikes or grabs, many strikes land and several grabs hold; the pack stays tight; the trailer
/// camera goes through its shots, near the karts. (One thread, as in the bot races: the same run
/// every time.)
#[test]
fn the_background_karts_race_together_and_strike_and_grab_each_other() {
    let mut app = home_app(0, true);
    crate::tests::single_threaded(&mut app);
    wait_for_the_background(&mut app);
    let karts = HomeTuning::default().kart_count();
    let mut widest: f32 = 0.0;
    let (mut spread, mut samples) = (0.0f32, 0.0f32);
    let mut far = 0.0f32;
    let mut cameras: Vec<Vec3> = Vec::new();
    for frame in 0..40 * 60 {
        app.update();
        let world = app.world_mut();
        let distances: Vec<f32> = world
            .query_filtered::<&RaceProgress, With<ShowcaseKart>>()
            .iter(world)
            .map(|p| p.distance)
            .collect();
        let positions: Vec<Vec3> = world
            .query_filtered::<&Transform, With<ShowcaseKart>>()
            .iter(world)
            .map(|t| t.translation)
            .collect();
        let camera = world
            .query_filtered::<&Transform, With<ChaseCamera>>()
            .single(world)
            .unwrap()
            .translation;
        if frame > 15 * 60 {
            let (low, high) = distances
                .iter()
                .fold((f32::MAX, f32::MIN), |(l, h), d| (l.min(*d), h.max(*d)));
            widest = widest.max(high - low);
            spread += high - low;
            samples += 1.0;
        }
        let middle = positions.iter().sum::<Vec3>() / positions.len() as f32;
        far = far.max(camera.distance(middle));
        if frame % 60 == 0 {
            cameras.push(camera);
        }
    }
    let world = app.world_mut();
    let stats: Vec<PawStats> = world
        .query_filtered::<&PawStats, With<ShowcaseKart>>()
        .iter(world)
        .copied()
        .collect();
    assert_eq!(stats.len(), karts);
    let landed: u32 = stats.iter().map(|s| s.landed).sum();
    let grabs: u32 = stats.iter().map(|s| s.grabs).sum();
    let busy = stats.iter().filter(|s| s.strikes + s.grabs > 0).count();
    let spread = spread / samples;
    println!(
        "background: {landed} strikes landed, {grabs} grabs, {busy} karts busy, pack {spread:.1} m wide ({widest:.1} at most), camera at most {far:.1} m from the pack"
    );
    // About one strike landed every 2 s, a grab every 10 s (measured: 25 and 6).
    assert!(landed >= 15, "{stats:?}");
    assert!(grabs >= 3, "{stats:?}");
    assert!(busy >= karts - 1, "{stats:?}");
    // Measured: 15 m on average, 33 m at most (a kart stopped by a crash waits for no one).
    assert!(spread < 25.0, "the pack stays together: {spread}");
    assert!(widest < 60.0, "the pack stays together: {widest}");
    assert!(far < 25.0, "the camera films them: {far}");
    // The shots change (6 s each).
    let reel = showcase(&app).unwrap().reel;
    assert!(reel.shot >= 5, "{reel:?}");
    // Nobody finished: an endless race.
    let world = app.world_mut();
    assert!(
        world
            .query::<&RaceProgress>()
            .iter(world)
            .all(|p| !p.finished())
    );
}

/// Issue #89: leaving a room (or being left by its host) brings the home screen and its
/// background; while in a room, no background is asked.
#[test]
fn leaving_a_room_brings_the_home() {
    let mut app = home_app(0, false);
    run(&mut app, 3);
    assert!(!menu(&app).open);
    app.insert_resource(crate::level::Online);
    run(&mut app, 3);
    assert!(!menu(&app).open && showcase(&app).is_none());
    app.world_mut().remove_resource::<crate::level::Online>();
    run(&mut app, 2);
    assert!(menu(&app).open && !menu(&app).in_game, "home");
    wait_for_the_background(&mut app);
    assert_the_background_runs(
        &mut app,
        HomeTuning::default().kart_count(),
        "out of a room",
    );
}

/// Issue #89: the background's tuning is in the admin panel (with the menu: the game's), next to
/// the others; its number of karts builds the background again, its paws' chances apply at once.
#[test]
fn the_home_tuning_is_in_the_admin_panel_and_changes_the_background() {
    let mut app = home_app(0, true);
    wait_for_the_background(&mut app);
    let names: Vec<&str> = app
        .world()
        .resource::<crate::admin::Tunings>()
        .list
        .iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names.last(), Some(&"HomeTuning"));
    let set = |app: &mut App, path: &str, value: Value| {
        app.world_mut()
            .resource_scope(|world, tunings: Mut<crate::admin::Tunings>| {
                tunings.set(world, path, value).unwrap();
            });
    };
    set(&mut app, "HomeTuning.strike_chance", Value::F32(0.5));
    run(&mut app, 2);
    let world = app.world_mut();
    let chances: Vec<f32> = world
        .query::<&OwnSkill>()
        .iter(world)
        .map(|s| s.0.strike_chance)
        .collect();
    assert!(
        !chances.is_empty() && chances.iter().all(|c| *c == 0.5),
        "{chances:?}"
    );
    set(&mut app, "HomeTuning.karts", Value::Usize(3));
    run(&mut app, 2);
    wait_for_the_background(&mut app);
    run(&mut app, 3);
    assert_the_background_runs(&mut app, 3, "three karts");
    assert_eq!(
        app.world().resource::<RaceGrid>().bots,
        0,
        "the player's grid"
    );
}
