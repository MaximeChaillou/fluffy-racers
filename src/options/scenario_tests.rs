//! Headless scenarios of the options menu (issue #17): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, one fixed tick per update. The egui window is not there (no renderer): the
//! keys and the `OptionsAction`s its buttons write are.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::characters::{SelectionAction, SelectionScreen};
use crate::kart::DEBUG_RESET_KEY;
use crate::tests::{app_on, hold_keys, player};
use crate::track::TrackChoice;

/// A key pressed this frame (`just_pressed`), as the window sends it.
fn tap(app: &mut App, key_code: KeyCode) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

fn menu(app: &App) -> OptionsMenu {
    *app.world().resource::<OptionsMenu>()
}

fn options(app: &App) -> PlayerOptions {
    *app.world().resource::<PlayerOptions>()
}

fn layout(app: &App) -> KeyboardLayout {
    *app.world().resource::<KeyboardLayout>()
}

fn paused(app: &App) -> bool {
    app.world().resource::<Time<Virtual>>().is_paused()
}

fn speed(app: &mut App) -> f32 {
    let kart = player(app);
    app.world().get::<Velocity>(kart).unwrap().linear.length()
}

fn run(app: &mut App, seconds: f32) {
    for _ in 0..(seconds * 60.0).round() as usize {
        app.update();
    }
}

/// Issue #17: O opens the menu and pauses the game; right (or left, Entrée) changes the keyboard
/// at once; Échap or O closes it and the game runs again.
#[test]
fn o_opens_the_options_and_the_keyboard_changes_at_once() {
    let mut app = app_on(TrackChoice::TestGround);
    assert!(!menu(&app).open && !paused(&app));
    assert_eq!(options(&app), PlayerOptions::default());
    assert_eq!(layout(&app), KeyboardLayout::Azerty);

    tap(&mut app, OPTIONS_KEY);
    assert!(menu(&app).open);
    assert!(paused(&app), "the game waits while the menu is open");
    assert_eq!(menu(&app).row(), OptionRow::Keyboard);

    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    assert_eq!(layout(&app), KeyboardLayout::Qwerty, "applied at once");
    // D is right in both layouts; Entrée changes the line too.
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Azerty);
    tap(&mut app, KeyCode::Enter);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    // Issue #80: down goes to the second line, the names over the karts (Entrée turns them
    // off, right on again: the keyboard stays), and around back to the keyboard; up goes the
    // other way round.
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(menu(&app).row(), OptionRow::NameTags);
    tap(&mut app, KeyCode::Enter);
    assert!(!options(&app).name_tags);
    tap(&mut app, KeyCode::ArrowRight);
    assert!(options(&app).name_tags);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(menu(&app).row(), OptionRow::Keyboard);
    tap(&mut app, KeyCode::ArrowUp);
    assert_eq!(menu(&app).row(), OptionRow::NameTags);
    tap(&mut app, KeyCode::ArrowUp);
    assert_eq!(menu(&app).row(), OptionRow::Keyboard);

    tap(&mut app, KeyCode::Escape);
    assert!(!menu(&app).open);
    assert!(!paused(&app));
    assert_eq!(layout(&app), KeyboardLayout::Qwerty, "the choice stays");

    tap(&mut app, OPTIONS_KEY);
    assert!(menu(&app).open);
    tap(&mut app, OPTIONS_KEY);
    assert!(!menu(&app).open && !paused(&app));

    // The window's buttons.
    app.world_mut().write_message(OptionsAction::Toggle);
    app.update();
    app.world_mut()
        .write_message(OptionsAction::Keyboard(KeyboardLayout::Azerty));
    app.update();
    assert_eq!(layout(&app), KeyboardLayout::Azerty);
    app.world_mut()
        .write_message(OptionsAction::NameTags(false));
    app.update();
    assert!(!options(&app).name_tags);
    app.world_mut().write_message(OptionsAction::Close);
    app.update();
    assert!(!menu(&app).open && !paused(&app));
}

/// Issue #17, end to end on the keyboard: the physical W (Z on an AZERTY keyboard) drives in
/// both layouts; the position Z (what a browser that reports letters gives for an AZERTY Z)
/// drives in AZERTY only. While the menu is open, the arrows are the menu's: the kart waits.
#[test]
fn the_keyboard_layout_decides_which_keys_drive() {
    let mut app = app_on(TrackChoice::TestGround);
    // From the spawn point (R), one second with `key` held.
    let accelerates = |app: &mut App, key: KeyCode| {
        hold_keys(app, &[]);
        tap(app, DEBUG_RESET_KEY);
        assert!(speed(app) < 0.5);
        hold_keys(app, &[key]);
        run(app, 1.0);
        hold_keys(app, &[]);
        speed(app) > 10.0
    };
    // AZERTY by default.
    assert!(accelerates(&mut app, KeyCode::KeyW));
    assert!(accelerates(&mut app, KeyCode::KeyZ));
    // QWERTY: Z is left alone.
    app.world_mut().resource_mut::<PlayerOptions>().keyboard = KeyboardLayout::Qwerty;
    app.update();
    assert_eq!(layout(&app), KeyboardLayout::Qwerty);
    assert!(accelerates(&mut app, KeyCode::KeyW));
    assert!(!accelerates(&mut app, KeyCode::KeyZ));

    // Menu open: the up arrow does not drive (the game is paused).
    hold_keys(&mut app, &[]);
    tap(&mut app, DEBUG_RESET_KEY);
    tap(&mut app, OPTIONS_KEY);
    let kart = player(&mut app);
    let at = app.world().get::<Transform>(kart).unwrap().translation;
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 1.0);
    let still = app.world().get::<Transform>(kart).unwrap().translation;
    assert!(at.distance(still) < 1e-4, "{at} → {still}");
}

/// Issue #17: O opens the menu over the selection screen (to choose the keyboard before the
/// first race); its keys are then the menu's only. Échap closes the menu, then the screen; the
/// game stays paused until both are closed.
#[test]
fn the_menu_opens_over_the_selection_screen_and_takes_its_keys() {
    let mut app = app_on(TrackChoice::TestGround);
    app.world_mut().write_message(SelectionAction::Open);
    app.update();
    let screen = |app: &App| *app.world().resource::<SelectionScreen>();
    assert!(screen(&app).open && paused(&app));
    let choice = screen(&app).choice;

    tap(&mut app, OPTIONS_KEY);
    assert!(menu(&app).open && screen(&app).open);
    // Right changes the keyboard, not the character; G does not close the screen.
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(options(&app).keyboard, KeyboardLayout::Qwerty);
    assert_eq!(screen(&app).choice, choice);
    tap(&mut app, KeyCode::KeyG);
    assert!(screen(&app).open);

    tap(&mut app, KeyCode::Escape);
    assert!(!menu(&app).open);
    assert!(screen(&app).open, "Échap only closed the menu");
    assert!(paused(&app), "the screen is still open");
    // The screen now reads the QWERTY keys: D (right) changes the character.
    tap(&mut app, KeyCode::KeyD);
    assert_ne!(screen(&app).choice, choice);
    tap(&mut app, KeyCode::Escape);
    assert!(!screen(&app).open && !paused(&app));
}
