//! Headless scenarios of the main menu (issue #56): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, one fixed tick per update, and [`MainMenuPlugin`] added by hand (the game
//! only adds it with a renderer). No picking headless: the mouse's [`MainMenuAction`]s are
//! written as its buttons' observers write them.

use bevy::input::ButtonState as KeyState;
use bevy::input::gamepad::{
    GamepadSettings, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy_rapier3d::prelude::Velocity;

use super::screen::{MenuText, accent};
use super::style::palette;
use super::*;
use crate::kart::{KartInput, PlayerKart};
use crate::net::{NetSettings, RoomCode};
use crate::tests::{game_app_unloaded, hold_keys, player, wait_for_the_player};
use crate::track::TrackChoice;

/// The game on `track` with the main menu, alone on the grid, ready to race.
fn menu_app(track: TrackChoice, at_launch: bool) -> App {
    let mut app = game_app_unloaded(track, 0);
    app.add_plugins(MainMenuPlugin { at_launch });
    wait_for_the_player(&mut app);
    app
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

/// A key pressed (one frame) then released (the next), as the window sends it.
fn tap(app: &mut App, key_code: KeyCode) {
    press(app, key_code, KeyState::Pressed);
    press(app, key_code, KeyState::Released);
}

/// Runs `frames` updates.
fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// Entrée on the focused entry, then long enough for what it does (its stretch first).
fn choose(app: &mut App) {
    tap(app, KeyCode::Enter);
    run(app, (ACTIVATE_DELAY * 60.0).ceil() as usize + 3);
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

fn act(app: &mut App, action: MainMenuAction) {
    app.world_mut().write_message(action);
    app.update();
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

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

/// Issue #56: up and down go around the menu (the arrows, and the layout's letters: Z or W, S),
/// and each entry opens its screen; closing that screen with Échap comes back to the menu, the
/// focus on its entry. The game waits all along, but behind the lobby (it never waited there).
#[test]
fn the_keys_go_through_the_menu_and_each_entry_opens_its_screen() {
    let mut app = menu_app(TrackChoice::TestGround, false);
    assert!(!menu(&app).open, "the tests' game starts racing at once");
    act(&mut app, MainMenuAction::OpenAtLaunch);
    assert!(menu(&app).open && !menu(&app).in_game);
    assert_eq!(menu(&app).entries(), &LAUNCH_ENTRIES);
    assert_eq!(focused(&app), Entry::Solo);
    assert!(paused(&app), "the game waits in solo");

    // Down, around, and up; AZERTY: S down, Z (the key printed Z: W's place) up.
    for (key, entry) in [
        (KeyCode::ArrowDown, Entry::Online),
        (KeyCode::ArrowDown, Entry::Options),
        (KeyCode::KeyS, Entry::Garage),
        (KeyCode::ArrowDown, Entry::Solo),
        (KeyCode::ArrowUp, Entry::Garage),
        (KeyCode::KeyW, Entry::Options),
        (KeyCode::KeyZ, Entry::Online),
    ] {
        tap(&mut app, key);
        assert_eq!(focused(&app), entry, "{key:?}");
    }
    // QWERTY: W is up, Z is not (it is next to X, the Interaction key).
    app.insert_resource(crate::options::PlayerOptions {
        keyboard: KeyboardLayout::Qwerty,
        ..default()
    });
    app.update();
    tap(&mut app, KeyCode::KeyZ);
    assert_eq!(focused(&app), Entry::Online);
    tap(&mut app, KeyCode::KeyW);
    assert_eq!(focused(&app), Entry::Solo);
    app.insert_resource(crate::options::PlayerOptions::default());
    app.update();

    // « Options », then Échap: back to the menu.
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(focused(&app), Entry::Options);
    choose(&mut app);
    assert!(options_open(&app) && !menu(&app).open);
    assert!(paused(&app));
    tap(&mut app, KeyCode::Escape);
    assert!(!options_open(&app));
    assert!(menu(&app).open, "back to the menu");
    assert_eq!(focused(&app), Entry::Options);
    assert!(paused(&app), "still waiting");

    // « Pilote et kart ».
    tap(&mut app, KeyCode::ArrowDown);
    choose(&mut app);
    assert!(selection_open(&app) && !menu(&app).open);
    tap(&mut app, KeyCode::Escape);
    assert!(!selection_open(&app) && menu(&app).open);
    assert_eq!(focused(&app), Entry::Garage);

    // « Course solo »: the race menu (N).
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(focused(&app), Entry::Solo);
    choose(&mut app);
    assert!(race_menu_open(&app) && !menu(&app).open);
    assert!(paused(&app));
    tap(&mut app, KeyCode::Escape);
    assert!(!race_menu_open(&app) && menu(&app).open);
    assert_eq!(focused(&app), Entry::Solo);

    // « En ligne »: the lobby (L), which never paused the game; closing it without a room
    // comes back.
    tap(&mut app, KeyCode::ArrowDown);
    choose(&mut app);
    assert!(lobby_open(&app) && !menu(&app).open);
    run(&mut app, 2);
    assert!(!paused(&app), "the lobby does not pause");
    tap(&mut app, crate::net::LOBBY_KEY);
    assert!(!lobby_open(&app) && menu(&app).open);
    assert_eq!(focused(&app), Entry::Online);
    assert!(paused(&app));

    // Échap closes the menu: the game goes on.
    tap(&mut app, KeyCode::Escape);
    assert!(!menu(&app).open);
    run(&mut app, 2);
    assert!(!paused(&app));
}

/// Issue #56: a screen opened from the menu that starts a race (« Course ! », « Entrée » in the
/// race menu) does not bring the menu back: the race goes.
#[test]
fn a_race_started_from_a_screen_of_the_menu_goes_on_without_it() {
    let mut app = menu_app(TrackChoice::TestGround, false);
    act(&mut app, MainMenuAction::OpenAtLaunch);
    choose(&mut app);
    assert!(race_menu_open(&app));
    tap(&mut app, KeyCode::Enter);
    assert!(!race_menu_open(&app));
    wait_for_the_player(&mut app);
    run(&mut app, 5);
    assert!(!menu(&app).open, "no menu over the race");
    assert_eq!(menu(&app).child, None);
    assert!(!paused(&app));
    assert!(
        app.world()
            .get_resource::<crate::track::TrackLayout>()
            .is_some()
    );

    // « Pilote et kart » then « Course ! » (Entrée): the race starts over, no menu.
    tap(&mut app, KeyCode::Escape);
    assert!(menu(&app).open && menu(&app).in_game);
    for _ in 0..4 {
        tap(&mut app, KeyCode::ArrowDown);
    }
    assert_eq!(focused(&app), Entry::Garage);
    let kart = player(&mut app);
    choose(&mut app);
    assert!(selection_open(&app));
    tap(&mut app, KeyCode::Enter);
    wait_for_the_player(&mut app);
    run(&mut app, 5);
    assert_ne!(player(&mut app), kart, "the level was rebuilt");
    assert!(!menu(&app).open && !selection_open(&app));
    assert!(!paused(&app));
}

/// Issue #56: from the game, Échap (or the gamepad's Start) opens the menu, « Reprendre »
/// first, and pauses the game in solo; Entrée on « Reprendre » or Échap closes it. Échap that
/// closes another screen does not also open it; a screen's shortcut over the menu takes its
/// place, and Échap there comes back to it.
#[test]
fn echap_opens_the_menu_from_the_game_but_closes_the_other_screens_first() {
    let mut app = menu_app(TrackChoice::TestGround, false);
    tap(&mut app, KeyCode::Escape);
    assert!(menu(&app).open && menu(&app).in_game);
    assert_eq!(menu(&app).entries(), &GAME_ENTRIES);
    assert_eq!(focused(&app), Entry::Resume);
    assert!(paused(&app));
    choose(&mut app);
    assert!(!menu(&app).open, "« Reprendre »");
    run(&mut app, 2);
    assert!(!paused(&app));
    tap(&mut app, KeyCode::Escape);
    assert!(menu(&app).open);
    tap(&mut app, KeyCode::Escape);
    assert!(!menu(&app).open && !paused(&app));

    // Échap closes the screens opened by their shortcut: nothing more.
    let screens: [(KeyCode, fn(&App) -> bool); 3] = [
        (crate::options::OPTIONS_KEY, options_open),
        (crate::characters::SELECTION_KEY, selection_open),
        (crate::modes::RACE_MENU_KEY, race_menu_open),
    ];
    for (key, open) in screens {
        tap(&mut app, key);
        assert!(open(&app), "{key:?}");
        tap(&mut app, KeyCode::Escape);
        assert!(!open(&app), "{key:?}");
        run(&mut app, 2);
        assert!(
            !menu(&app).open,
            "{key:?}: Échap closed it, and that is all"
        );
        assert!(!paused(&app), "{key:?}");
    }

    // A shortcut over the menu: its screen takes the menu's place; Échap comes back to it.
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, crate::options::OPTIONS_KEY);
    assert!(options_open(&app) && !menu(&app).open);
    assert!(paused(&app));
    tap(&mut app, KeyCode::Escape);
    assert!(!options_open(&app) && menu(&app).open);
    assert_eq!(focused(&app), Entry::Options);
}

/// Issue #56 (Max's decision, issue #9): online, the menu does not pause the game: the kart
/// rolls on with its momentum, only its keys are the menu's; « Course solo » is disabled (the
/// host chooses), skipped by the keys, its reason under it.
#[test]
fn online_the_menu_does_not_pause_and_the_kart_coasts() {
    let mut app = menu_app(TrackChoice::TestGround, false);
    app.insert_resource(crate::level::Online);
    let kart = player(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 60);
    let speed = |app: &App| app.world().get::<Velocity>(kart).unwrap().linear.length();
    let before = speed(&app);
    assert!(before > 10.0, "{before}");

    tap(&mut app, KeyCode::Escape);
    assert!(menu(&app).open);
    run(&mut app, 20);
    assert!(!paused(&app), "online, never");
    assert_eq!(
        *app.world().get::<KartInput>(kart).unwrap(),
        KartInput::default(),
        "the keys are the menu's"
    );
    let coasting = speed(&app);
    assert!(
        coasting > 1.0 && coasting < before,
        "it rolls on: {coasting}"
    );

    // « Course solo » is skipped, and shown disabled with its reason.
    assert_eq!(focused(&app), Entry::Resume);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(focused(&app), Entry::Online, "over « Course solo »");
    act(&mut app, MainMenuAction::Hover(1));
    assert_eq!(focused(&app), Entry::Online, "the pointer cannot focus it");
    act(&mut app, MainMenuAction::Press(1));
    assert_eq!(menu(&app).pressed, None);
    let reasons: Vec<String> = {
        let world = app.world_mut();
        world
            .query::<(&MenuText, &Text)>()
            .iter(world)
            .filter(|(text, _)| text.index == 1 && text.caption)
            .map(|(_, text)| text.0.clone())
            .collect()
    };
    assert_eq!(reasons, [Entry::Solo.why_disabled().to_string()]);

    // Closed: the keys drive again.
    tap(&mut app, KeyCode::Escape);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 3);
    assert_eq!(app.world().get::<KartInput>(kart).unwrap().throttle, 1.0);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
}

/// Issue #56: the gamepad: Start opens and closes the menu, the cross and the left stick move
/// (one step per push of the stick), A chooses, B goes back.
#[test]
fn the_gamepad_goes_through_the_menu() {
    let mut app = menu_app(TrackChoice::TestGround, false);
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
    let stick = |app: &mut App, y: f32| {
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                pad,
                GamepadAxis::LeftStickY,
                y,
            )));
        app.update();
    };
    button(&mut app, MAIN_MENU_BUTTON);
    assert!(menu(&app).open && paused(&app));
    assert_eq!(focused(&app), Entry::Resume);
    button(&mut app, GamepadButton::DPadDown);
    assert_eq!(focused(&app), Entry::Solo);
    stick(&mut app, -0.9);
    assert_eq!(focused(&app), Entry::Online, "the stick down");
    for _ in 0..10 {
        stick(&mut app, -0.95);
    }
    assert_eq!(focused(&app), Entry::Online, "held: one step");
    stick(&mut app, 0.0);
    stick(&mut app, -0.9);
    assert_eq!(focused(&app), Entry::Options);
    stick(&mut app, 0.0);
    stick(&mut app, 0.8);
    assert_eq!(focused(&app), Entry::Online, "up");
    stick(&mut app, 0.0);
    button(&mut app, GamepadButton::DPadUp);
    button(&mut app, GamepadButton::DPadUp);
    assert_eq!(focused(&app), Entry::Resume);
    button(&mut app, GamepadButton::DPadUp);
    assert_eq!(focused(&app), Entry::Garage, "around");
    // B goes back.
    button(&mut app, GamepadButton::East);
    assert!(!menu(&app).open);
    // A chooses: « Reprendre ».
    button(&mut app, MAIN_MENU_BUTTON);
    assert!(menu(&app).open);
    button(&mut app, GamepadButton::South);
    run(&mut app, 10);
    assert!(!menu(&app).open);
    run(&mut app, 2);
    assert!(!paused(&app));
    // Start closes it too, and A on « Options » opens them.
    button(&mut app, MAIN_MENU_BUTTON);
    button(&mut app, MAIN_MENU_BUTTON);
    assert!(!menu(&app).open);
    button(&mut app, MAIN_MENU_BUTTON);
    for _ in 0..3 {
        button(&mut app, GamepadButton::DPadDown);
    }
    assert_eq!(focused(&app), Entry::Options);
    button(&mut app, GamepadButton::South);
    run(&mut app, 10);
    assert!(options_open(&app) && !menu(&app).open);
}

/// Issue #56: the mouse: the pointer gives the focus, a press squashes, its release over the
/// same button chooses; a press that leaves, or released over another button, does nothing.
#[test]
fn the_pointer_focuses_presses_and_chooses() {
    let mut app = menu_app(TrackChoice::TestGround, false);
    act(&mut app, MainMenuAction::OpenAtLaunch);
    act(&mut app, MainMenuAction::Hover(2));
    assert_eq!(focused(&app), Entry::Options);
    act(&mut app, MainMenuAction::Press(2));
    assert_eq!(menu(&app).pressed, Some(2));
    act(&mut app, MainMenuAction::Unpress);
    assert_eq!(menu(&app).pressed, None);
    run(&mut app, 12);
    assert!(menu(&app).open && !options_open(&app), "it left: nothing");
    act(&mut app, MainMenuAction::Press(2));
    act(&mut app, MainMenuAction::Release(3));
    run(&mut app, 12);
    assert!(menu(&app).open, "released elsewhere: nothing");
    assert!(!selection_open(&app) && !options_open(&app));
    act(&mut app, MainMenuAction::Unpress);
    act(&mut app, MainMenuAction::Hover(3));
    act(&mut app, MainMenuAction::Press(3));
    act(&mut app, MainMenuAction::Release(3));
    assert!(menu(&app).open, "its stretch is seen first");
    run(&mut app, 12);
    assert!(selection_open(&app) && !menu(&app).open);
}

/// Issue #56: the game opens on the main menu (with a renderer: the plugin's `at_launch`), and
/// waits there; with a room in the address (`?room=`: a `Join` written at startup), it goes
/// straight to the lobby without the menu, the selection screen open as before.
#[test]
fn the_menu_opens_at_launch_but_a_room_in_the_address_goes_to_the_lobby() {
    let app = menu_app(TrackChoice::TestGround, true);
    assert!(menu(&app).open && !menu(&app).in_game);
    assert_eq!(focused(&app), Entry::Solo);
    assert!(!selection_open(&app) && !lobby_open(&app));
    assert!(paused(&app));

    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(MainMenuPlugin { at_launch: true });
    {
        let mut settings = app.world_mut().resource_mut::<NetSettings>();
        // Nothing listens there: the room fails, the lobby says so.
        settings.signal = "ws://127.0.0.1:9".into();
        settings.attempts = 1;
    }
    let room = RoomCode::parse("TEST").unwrap();
    app.world_mut().write_message(NetAction::Join(room));
    wait_for_the_player(&mut app);
    run(&mut app, 5);
    assert!(!menu(&app).open, "no menu");
    assert!(lobby_open(&app), "straight to the lobby");
    assert!(selection_open(&app), "with the selection screen, as before");
    app.world_mut().write_message(NetAction::Leave);
    app.update();
}

/// Issue #56: the menu costs nothing once closed: no tree (no entity of it is left, nor its
/// observers), no egui context; open, its tree shows each state of a button.
#[test]
fn closed_the_menu_leaves_nothing_and_open_it_shows_each_state() {
    let mut app = menu_app(TrackChoice::TestGround, false);
    run(&mut app, 2);
    let entities = app.world().entities().count_spawned();
    assert_eq!(count::<With<MenuNode>>(&mut app), 0);

    tap(&mut app, KeyCode::Escape);
    assert_eq!(count::<With<MainMenuRoot>>(&mut app), 1);
    assert_eq!(count::<With<MenuButton>>(&mut app), GAME_ENTRIES.len());
    assert!(count::<With<MenuNode>>(&mut app) > 4 * GAME_ENTRIES.len());
    assert_eq!(
        count::<With<bevy_egui::PrimaryEguiContext>>(&mut app),
        0,
        "no egui for the menu"
    );
    let fill = |app: &mut App, index: usize| {
        let world = app.world_mut();
        world
            .query::<(&MenuButton, &BackgroundColor, &Node)>()
            .iter(world)
            .find(|(button, ..)| button.index == index)
            .map(|(_, fill, node)| (fill.0, node.border.bottom))
            .unwrap()
    };
    // Focused (« Reprendre »), at rest (the others), pressed, then disabled online.
    run(&mut app, 30);
    assert_eq!(fill(&mut app, 0).0, palette::MANGUE);
    assert_eq!(fill(&mut app, 1).0, palette::COTON);
    let rest_depth = fill(&mut app, 0).1;
    press(&mut app, KeyCode::Enter, KeyState::Pressed);
    assert_eq!(menu(&app).pressed, Some(0));
    app.update();
    assert_ne!(fill(&mut app, 0).1, rest_depth, "pressed, it goes down");
    let badge = {
        let world = app.world_mut();
        world
            .query::<(&screen::MenuBadge, &BackgroundColor)>()
            .iter(world)
            .find(|(badge, _)| badge.index == 1)
            .map(|(_, fill)| fill.0)
            .unwrap()
    };
    assert_eq!(badge, accent(Entry::Solo));
    act(&mut app, MainMenuAction::Unpress);
    press(&mut app, KeyCode::Enter, KeyState::Released);
    assert!(menu(&app).activating.is_none(), "let go elsewhere first");

    tap(&mut app, KeyCode::Escape);
    run(&mut app, 2);
    assert!(!menu(&app).open);
    assert_eq!(count::<With<MenuNode>>(&mut app), 0, "nothing left");
    assert_eq!(
        app.world().entities().count_spawned(),
        entities,
        "no entity left behind (observers included)"
    );

    // Online, « Course solo » is disabled: grey. The tree follows (a new one), and goes too.
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 3);
    assert_eq!(fill(&mut app, 1).0, palette::COTON);
    app.insert_resource(crate::level::Online);
    run(&mut app, 3);
    assert_eq!(fill(&mut app, 1).0, palette::BRUME, "disabled online");
    assert_eq!(count::<With<MainMenuRoot>>(&mut app), 1);
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 2);
    assert_eq!(count::<With<MenuNode>>(&mut app), 0);
}
