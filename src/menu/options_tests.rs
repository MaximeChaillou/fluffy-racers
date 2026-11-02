//! Issue #100: headless scenarios of the options screen (`options_screen.rs`): the whole game
//! (`GamePlugin`) with `MinimalPlugins`, [`MainMenuPlugin`] added by hand (the game only adds it
//! with a renderer). No picking headless: the mouse's `OptionsAction`s are written as the
//! screen's observers write them.

use bevy::input::ButtonState as KeyState;
use bevy::input::gamepad::{GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::text::EditableText;
use bevy::window::PrimaryWindow;

use super::style::{palette, size};
use super::*;
use crate::admin::KeyboardCapture;
use crate::net::NetSettings;
use crate::options::{OptionRow, PlayerOptions};
use crate::tests::{game_app_unloaded, wait_for_the_player};
use crate::track::TrackChoice;

/// Issue #103: a volume line's value by default.
const VOLUME: &str = "80\u{a0}%";

fn options_app() -> App {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(MainMenuPlugin { at_launch: false });
    wait_for_the_player(&mut app);
    app
}

fn tap(app: &mut App, key_code: KeyCode) {
    for state in [KeyState::Pressed, KeyState::Released] {
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
}

fn act(app: &mut App, action: OptionsAction) {
    app.world_mut().write_message(action);
    app.update();
}

fn menu(app: &App) -> OptionsMenu {
    *app.world().resource::<OptionsMenu>()
}

fn options(app: &App) -> PlayerOptions {
    *app.world().resource::<PlayerOptions>()
}

fn paused(app: &App) -> bool {
    app.world().resource::<Time<Virtual>>().is_paused()
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

/// The values written on the lines, in their order.
fn values(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut values: Vec<(usize, String)> = world
        .query::<(&OptionsValue, &Text)>()
        .iter(world)
        .map(|(value, text)| {
            let index = OptionRow::ALL.iter().position(|r| *r == value.row).unwrap();
            (index, text.0.clone())
        })
        .collect();
    values.sort();
    values.into_iter().map(|(_, text)| text).collect()
}

fn texts_of<C: Component>(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    world
        .query_filtered::<&Text, With<C>>()
        .iter(world)
        .map(|text| text.0.clone())
        .collect()
}

/// Issue #136: the commands on the right, as written (each action, then its keys): no line of
/// keys under the panel any more, the commands follow the keyboard.
fn commands_shown(app: &mut App) -> String {
    let world = app.world_mut();
    let lines: Vec<Entity> = world
        .query_filtered::<Entity, With<ControlLine>>()
        .iter(world)
        .collect();
    world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .filter(|(_, parent)| lines.contains(&parent.parent()))
        .map(|(text, _)| text.0.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fill of the line `index`.
fn fill(app: &mut App, index: usize) -> Color {
    let world = app.world_mut();
    world
        .query::<(&OptionsLine, &BackgroundColor)>()
        .iter(world)
        .find(|(line, _)| line.index == index)
        .map(|(_, fill)| fill.0)
        .unwrap()
}

/// Issue #100 acceptance: O opens the options screen (the game waits in solo): a line per option
/// with its value, the help of the line with the focus, the commands with the keyboard's
/// letters. Right changes the keyboard (on screen and in the game, at once: the commands follow),
/// the pointer gives the focus to a line, « < » and the value change it; Échap closes it, nothing
/// of it is left; opened again, the options kept their values (and so does the text kept in the
/// browser).
#[test]
fn the_options_change_and_keep_their_value() {
    let mut app = options_app();
    assert!(
        app.world().get_resource::<OptionsUi>().is_none(),
        "nothing of the screen before it first opens"
    );
    let observers = count::<With<bevy::ecs::observer::Observer>>(&mut app);
    tap(&mut app, crate::options::OPTIONS_KEY);
    app.update();
    assert!(menu(&app).open && paused(&app));
    assert_eq!(count::<With<OptionsRoot>>(&mut app), 1);
    assert_eq!(count::<With<OptionsLine>>(&mut app), OptionRow::ALL.len());
    assert_eq!(
        count::<With<ControlLine>>(&mut app),
        crate::options::controls(KeyboardLayout::Azerty).len()
    );
    assert_eq!(values(&mut app), ["AZERTY", "Oui", VOLUME, VOLUME, VOLUME]);
    assert_eq!(
        texts_of::<OptionsHelp>(&mut app),
        [OptionRow::Keyboard.help()]
    );
    app.update();
    assert_eq!(fill(&mut app, 0), palette::MANGUE, "the focus");
    assert_eq!(fill(&mut app, 1), palette::COTON);
    // Issue #136: the keyboard's letters are those of the commands (no line of keys under the
    // panel any more).
    let commands = commands_shown(&mut app);
    assert!(commands.contains("Haut ou Z"), "{commands}");

    // Right: QWERTY, at once; the screen is built again with the new letters.
    tap(&mut app, KeyCode::ArrowRight);
    app.update();
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    assert_eq!(
        *app.world().resource::<KeyboardLayout>(),
        KeyboardLayout::Qwerty
    );
    assert_eq!(values(&mut app), ["QWERTY", "Oui", VOLUME, VOLUME, VOLUME]);
    assert_eq!(count::<With<OptionsRoot>>(&mut app), 1);
    let commands = commands_shown(&mut app);
    assert!(
        commands.contains("Haut ou W") && !commands.contains("Haut ou Z"),
        "{commands}"
    );

    // The pointer over the second line gives it the focus; « < » turns the names off; a click on
    // the value turns them on again, then off.
    act(&mut app, OptionsAction::Focus(1));
    app.update();
    assert_eq!(menu(&app).row(), OptionRow::NameTags);
    assert_eq!(fill(&mut app, 1), palette::MANGUE);
    assert_eq!(
        texts_of::<OptionsHelp>(&mut app),
        [OptionRow::NameTags.help()]
    );
    act(&mut app, OptionsAction::Change(OptionRow::NameTags, -1));
    assert_eq!(values(&mut app), ["QWERTY", "Non", VOLUME, VOLUME, VOLUME]);
    for wanted in ["Oui", "Non"] {
        act(&mut app, OptionsAction::Change(OptionRow::NameTags, 1));
        assert_eq!(values(&mut app)[1], wanted);
    }
    let changed = options(&app);
    assert!(!changed.name_tags);

    // Échap closes it: nothing is left, the game goes on.
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert!(!menu(&app).open && !paused(&app));
    assert_eq!(count::<With<MenuNode>>(&mut app), 0);
    assert_eq!(
        count::<With<bevy::ecs::observer::Observer>>(&mut app),
        observers,
        "its observers went with it"
    );
    // Its resource stays (it remembers the animations).
    let closed = app.world().entities().count_spawned();

    // Opened again: the values kept.
    tap(&mut app, crate::options::OPTIONS_KEY);
    app.update();
    assert_eq!(options(&app), changed);
    assert_eq!(values(&mut app), ["QWERTY", "Non", VOLUME, VOLUME, VOLUME]);
    // And through the text kept in the browser.
    assert_eq!(PlayerOptions::from_ron(&changed.to_ron()), Ok(changed));
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(count::<With<MenuNode>>(&mut app), 0);
    assert_eq!(
        app.world().entities().count_spawned(),
        closed,
        "nothing left, again"
    );
}

/// Issue #100: the gamepad on the options: the cross moves through the lines and changes them, A
/// changes the line too, B closes the screen.
#[test]
fn the_gamepad_goes_through_the_options() {
    let mut app = options_app();
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
    tap(&mut app, crate::options::OPTIONS_KEY);
    button(&mut app, GamepadButton::DPadRight);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    button(&mut app, GamepadButton::DPadDown);
    assert_eq!(menu(&app).row(), OptionRow::NameTags);
    button(&mut app, GamepadButton::South);
    assert!(!options(&app).name_tags, "A changes the line");
    button(&mut app, GamepadButton::DPadUp);
    button(&mut app, GamepadButton::DPadLeft);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Azerty);
    button(&mut app, GamepadButton::East);
    assert!(!menu(&app).open, "B closes it");
    app.update();
    assert!(!app.world().resource::<MainMenu>().open, "and nothing else");
}

/// Issue #100: from the home, « Options » opens the screen over its background (no pause), and
/// Échap comes back to the home, the focus on « Options »; from the pause, « Options » comes back
/// to the pause.
#[test]
fn the_options_come_back_to_the_screen_that_opened_them() {
    let mut app = options_app();
    app.world_mut().write_message(MainMenuAction::Open);
    app.update();
    let menu_now = |app: &App| *app.world().resource::<MainMenu>();
    assert!(menu_now(&app).open && menu_now(&app).in_game);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(menu_now(&app).focused(), Entry::Options);
    tap(&mut app, KeyCode::Enter);
    for _ in 0..12 {
        app.update();
    }
    assert!(menu(&app).open && !menu_now(&app).open);
    assert!(paused(&app), "in solo, the game still waits");
    assert_eq!(count::<With<OptionsRoot>>(&mut app), 1);
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert!(!menu(&app).open);
    assert!(menu_now(&app).open, "back to the pause");
    assert_eq!(menu_now(&app).focused(), Entry::Options);
    assert!(paused(&app));
}

/// Issue #100: room for the volumes of the sounds and of the music (#103, #104): six lines of
/// options, the help, « Retour », the title and the keys fit in a 720 px high window, and so do
/// the commands (six of them on two lines) on the right.
#[test]
fn the_options_screen_has_room_for_more_lines() {
    let line = |px: f32| 1.2 * px;
    let left = |lines: f32| {
        lines * size::LINE_HEIGHT
            + (lines - 1.0) * size::LINE_GAP
            + size::SOLO_HELP * 1.3
            + 2.0 * size::LINE_GAP
            + size::PILOT_BUTTON_HEIGHT
            + 4.0
    };
    let commands = crate::options::controls(KeyboardLayout::Azerty).len() as f32;
    let right = line(size::CONTROLS_HEADER)
        + 4.0
        + (commands + 6.0) * line(size::CONTROL_TEXT)
        + commands * size::CONTROL_GAP;
    let panel = left(6.0).max(right) + 2.0 * size::PANEL_PADDING;
    let screen = line(size::PILOT_TITLE) + panel + 2.0 * line(size::HINT) + 3.0 * 10.0;
    assert!(screen + 16.0 <= style::REFERENCE_HEIGHT, "{screen}");
    assert!(OptionRow::ALL.len() <= 6);
}

/// Issue #231: the game with the options screen and what Bevy's text field needs headless.
fn server_app() -> App {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    super::pilot_tests::with_text_fields(&mut app);
    app.add_plugins(MainMenuPlugin { at_launch: false });
    wait_for_the_player(&mut app);
    app
}

/// A key pressed (one frame) then released (the next), from the window, with what it types.
/// Returns whether the game saw it (`ButtonInput<KeyCode>`, what the menus and the kart read).
fn press(app: &mut App, key_code: KeyCode, logical_key: Key, text: Option<&str>) -> bool {
    let window = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap()
    };
    let mut seen = false;
    for state in [KeyState::Pressed, KeyState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: text.filter(|_| state == KeyState::Pressed).map(Into::into),
            repeat: false,
            window,
        });
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        seen |= keys.pressed(key_code) || keys.just_pressed(key_code);
    }
    seen
}

fn press_key(app: &mut App, key_code: KeyCode) -> bool {
    let logical = match key_code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        _ => Key::Unidentified(NativeKey::Unidentified),
    };
    press(app, key_code, logical, None)
}

/// Types `text`, a key a character; the letters of the menus and of the kart (W, S, D, O, Z)
/// at their places. Returns whether the game saw any key.
fn type_text(app: &mut App, text: &str) -> bool {
    let mut seen = false;
    for c in text.chars() {
        let code = match c {
            'w' => KeyCode::KeyW,
            's' => KeyCode::KeyS,
            'd' => KeyCode::KeyD,
            'o' => KeyCode::KeyO,
            'z' => KeyCode::KeyZ,
            _ => KeyCode::KeyM,
        };
        let typed = c.to_string();
        seen |= press(
            app,
            code,
            Key::Character(typed.as_str().into()),
            Some(&typed),
        );
    }
    seen
}

/// Empties the server's field, as the player would (select all, delete). Headless, the text has
/// no layout (no font), and Bevy's deleting keys need one: the field's text is emptied directly,
/// as an edit of the field (its change is seen like a key's).
fn clear_field(app: &mut App) {
    let world = app.world_mut();
    let mut fields = world.query_filtered::<&mut EditableText, With<ServerField>>();
    fields.single_mut(world).unwrap().clear();
    app.update();
}

fn server_used(app: &App) -> Option<String> {
    app.world().resource::<NetSettings>().server.clone()
}

/// Issue #231 acceptance: the « Serveur » line of the options. It is the last line (up from the
/// first); empty, its field shows « Aucun » and the line under the lines says the page hosts.
/// Entrée types in it: the game sees no key (W, S, D, O type), and nothing is used before the
/// end. Entrée checks it: a readable address is the server at once (`NetSettings::server`);
/// an unreadable one changes nothing and says why, in Fraise, without any panic; empty, no
/// server any more (the page hosts). Échap and a click elsewhere end the typing too (the screen
/// stays open). The link's server (`?server=`) is said, kept as it is when the address does not
/// change, and replaced by the player's own choice.
#[test]
fn the_server_is_typed_checked_and_used() {
    let mut app = server_app();
    assert_eq!(server_used(&app), None);
    tap(&mut app, crate::options::OPTIONS_KEY);
    app.update();
    app.update();
    let server_row = OptionRow::ALL.len() - 1;
    assert_eq!(OptionRow::ALL[server_row], OptionRow::Server);
    assert_eq!(count::<With<ServerField>>(&mut app), 1);
    let field = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<ServerField>>()
            .single(world)
            .unwrap()
    };
    let hint_shown = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&Node, With<ServerHint>>()
            .single(world)
            .unwrap()
            .display
            == Display::Flex
    };
    assert!(hint_shown(&mut app), "empty: « Aucun »");
    assert_eq!(texts_of::<ServerHint>(&mut app), [SERVER_HINT]);
    let help = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<(&Text, &TextColor), With<OptionsHelp>>()
            .single(world)
            .map(|(text, color)| (text.0.clone(), color.0))
            .unwrap()
    };
    // Up from the keyboard's line: the server's, the last.
    assert!(press_key(&mut app, KeyCode::ArrowUp));
    assert_eq!(menu(&app).row, server_row);
    app.update();
    assert_eq!(help(&mut app).0, ServerNotice::Hosting.text());
    assert_eq!(fill(&mut app, server_row), palette::MANGUE, "the focus");

    // Entrée: the field has the keys.
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(menu(&app).typing && menu(&app).open);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert!(app.world().resource::<KeyboardCapture>().0);
    let before = options(&app);
    assert!(
        !type_text(&mut app, "ws://192.168.1.30:3536"),
        "the game saw no key"
    );
    let value = |app: &App| {
        app.world()
            .get::<EditableText>(field)
            .unwrap()
            .value()
            .to_string()
    };
    assert_eq!(value(&app), "ws://192.168.1.30:3536");
    assert!(!type_text(&mut app, " "), "no space in an address");
    assert_eq!(value(&app), "ws://192.168.1.30:3536");
    assert!(!hint_shown(&mut app));
    assert_eq!(server_used(&app), None, "checked at the end only");
    assert_eq!(menu(&app).row, server_row);
    assert!(menu(&app).open, "O typed, not the options closed");
    assert_eq!(options(&app), before);

    // Entrée: checked, used.
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(!menu(&app).typing);
    assert!(!app.world().resource::<KeyboardCapture>().0);
    assert_ne!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(server_used(&app).as_deref(), Some("ws://192.168.1.30:3536"));
    assert_eq!(
        app.world().resource::<OptionsUi>().kept.as_deref(),
        Some("ws://192.168.1.30:3536")
    );
    assert_eq!(help(&mut app).0, ServerNotice::OnServer.text());

    // Unreadable: nothing changes, it says why (in Fraise).
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(menu(&app).typing);
    clear_field(&mut app);
    assert_eq!(value(&app), "");
    type_text(&mut app, "http//192.168.1.30");
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(!menu(&app).typing && menu(&app).open);
    assert_eq!(server_used(&app).as_deref(), Some("ws://192.168.1.30:3536"));
    assert_eq!(
        help(&mut app),
        (ServerNotice::Unreadable.text().to_string(), palette::FRAISE)
    );
    // The same with a click in the field, then Échap: the typing ends, the screen stays.
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(field, FocusCause::Pressed);
    app.update();
    app.update();
    assert!(menu(&app).typing);
    assert!(app.world().resource::<KeyboardCapture>().0);
    assert_eq!(
        value(&app),
        "http//192.168.1.30",
        "the text refused stays, to fix"
    );
    clear_field(&mut app);
    assert_eq!(value(&app), "");
    type_text(&mut app, "wss://fluffy.example.org/");
    press_key(&mut app, KeyCode::Escape);
    app.update();
    assert!(
        !menu(&app).typing && menu(&app).open,
        "Échap only ended the typing"
    );
    assert_eq!(
        server_used(&app).as_deref(),
        Some("wss://fluffy.example.org")
    );
    assert_eq!(help(&mut app).0, ServerNotice::OnServer.text());
    assert_ne!(help(&mut app).1, palette::FRAISE);

    // Closed, then the link's server (as `?server=` gives it at launch): the field still shows
    // the one kept, the line says the link's is used; Entrée twice without a change keeps it.
    press_key(&mut app, KeyCode::Escape);
    assert!(!menu(&app).open);
    app.world_mut().resource_mut::<NetSettings>().server =
        Some("wss://lien.example.org".to_string());
    tap(&mut app, crate::options::OPTIONS_KEY);
    app.update();
    app.update();
    let field = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<ServerField>>()
            .single(world)
            .unwrap()
    };
    assert_eq!(
        app.world()
            .get::<EditableText>(field)
            .unwrap()
            .value()
            .to_string(),
        "wss://fluffy.example.org",
        "the one kept"
    );
    press_key(&mut app, KeyCode::ArrowUp);
    app.update();
    assert_eq!(help(&mut app).0, ServerNotice::FromLink.text());
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert!(menu(&app).typing);
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(
        server_used(&app).as_deref(),
        Some("wss://lien.example.org"),
        "nothing changed: the link's stays"
    );
    // Emptied: no server, the page hosts (the player's choice wins over the link's).
    press_key(&mut app, KeyCode::Enter);
    app.update();
    clear_field(&mut app);
    press_key(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(server_used(&app), None);
    assert_eq!(app.world().resource::<OptionsUi>().kept, None);
    assert_eq!(help(&mut app).0, ServerNotice::Hosting.text());
    assert!(hint_shown(&mut app));
    press_key(&mut app, KeyCode::Escape);
    assert!(!menu(&app).open);
    assert!(!app.world().resource::<KeyboardCapture>().0);
}
