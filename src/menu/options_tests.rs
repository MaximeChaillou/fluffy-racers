//! Issue #100: headless scenarios of the options screen (`options_screen.rs`): the whole game
//! (`GamePlugin`) with `MinimalPlugins`, [`MainMenuPlugin`] added by hand (the game only adds it
//! with a renderer). No picking headless: the mouse's `OptionsAction`s are written as the
//! screen's observers write them.

use bevy::input::ButtonState as KeyState;
use bevy::input::gamepad::{GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

use super::style::{palette, size};
use super::*;
use crate::options::{OptionRow, PlayerOptions};
use crate::tests::{game_app_unloaded, wait_for_the_player};
use crate::track::TrackChoice;

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
    assert_eq!(values(&mut app), ["AZERTY", "Oui"]);
    assert_eq!(
        texts_of::<OptionsHelp>(&mut app),
        [OptionRow::Keyboard.help()]
    );
    app.update();
    assert_eq!(fill(&mut app, 0), palette::MANGUE, "the focus");
    assert_eq!(fill(&mut app, 1), palette::COTON);
    let hints = texts_of::<OptionsHint>(&mut app).join("\n");
    assert!(hints.contains("(Z / S)"), "{hints}");

    // Right: QWERTY, at once; the screen is built again with the new letters.
    tap(&mut app, KeyCode::ArrowRight);
    app.update();
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    assert_eq!(
        *app.world().resource::<KeyboardLayout>(),
        KeyboardLayout::Qwerty
    );
    assert_eq!(values(&mut app), ["QWERTY", "Oui"]);
    assert_eq!(count::<With<OptionsRoot>>(&mut app), 1);
    let hints = texts_of::<OptionsHint>(&mut app).join("\n");
    assert!(hints.contains("(W / S)"), "{hints}");

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
    assert_eq!(values(&mut app), ["QWERTY", "Non"]);
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
    assert_eq!(values(&mut app), ["QWERTY", "Non"]);
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
