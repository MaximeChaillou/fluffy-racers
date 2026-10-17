//! Headless scenarios of the « Pilote » screen (issue #98): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, [`MainMenuPlugin`] added by hand (it brings the screen, `PilotPlugin`), and
//! what Bevy's text field needs to run without a window's renderer (its text, its focus, its
//! keys: `with_text_fields`). No picking headless: the pointer's events are triggered on the
//! entities the pointer would be over, as picking would (`click`, `hover`).

use std::time::Duration;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{NormalizedRenderTarget, RenderTarget};
use bevy::input::ButtonState;
use bevy::input::gamepad::{
    GamepadSettings, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input_focus::{FocusCause, InputDispatchPlugin, InputFocus, InputFocusPlugin};
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerOver};
use bevy::picking::pointer::{Location, PointerButton, PointerId};
use bevy::text::{EditableText, TextPlugin};
use bevy::ui_widgets::TextInputPlugin;
use bevy::window::PrimaryWindow;

use super::pilot::{self, PORTRAIT_PX};
use super::preview::{PORTRAIT_FRAMES, PREVIEW_LAYER, PREVIEW_ORIGIN};
use super::*;
use crate::admin::{AdminPanel, KeyboardCapture};
use crate::characters::{Loadout, PartKind, PlayerColor, PlayerLoadout, Roster};
use crate::kart::{
    BodyPaint, DriverVisual, KartBodyPivot, KartColor, KartEnvironment, KartInput, KartPaint,
    KartStats, Stat, Terrain, Wing, display_level,
};
use crate::net::NetSettings;
use crate::tests::{game_app_unloaded, player, wait_for_the_player};
use crate::track::TrackChoice;

/// What Bevy's `EditableText` needs headless: its text (`TextPlugin`), the focus and the keys
/// sent to the focused entity, a primary window (the keys are dispatched from it), and the few
/// resources of the UI and of picking its systems read.
fn with_text_fields(app: &mut App) {
    app.add_plugins((
        TextPlugin,
        InputFocusPlugin,
        InputDispatchPlugin,
        TextInputPlugin,
    ))
    .init_resource::<bevy::ui::UiScale>()
    .init_resource::<bevy::picking::events::PointerState>()
    .add_message::<bevy::window::Ime>()
    .add_message::<bevy::picking::events::PointerRelease>();
    app.world_mut().spawn((Window::default(), PrimaryWindow));
}

/// The game on `track` with the menu (and so the « Pilote » screen), alone on the grid, ready.
fn pilot_app(track: TrackChoice) -> App {
    let mut app = game_app_unloaded(track, 0);
    app.init_asset::<Image>();
    with_text_fields(&mut app);
    app.add_plugins(MainMenuPlugin { at_launch: false });
    wait_for_the_player(&mut app);
    app
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

fn screen(app: &App) -> SelectionScreen {
    *app.world().resource::<SelectionScreen>()
}

fn window(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap()
}

/// A key pressed (one frame) then released (the next), as the window sends it, with what it
/// types (`text`). Returns whether the game saw it pressed (`ButtonInput<KeyCode>`, what the
/// kart's input and every shortcut read).
fn key(app: &mut App, key_code: KeyCode, logical_key: Key, text: Option<&str>) -> bool {
    let window = window(app);
    let mut seen = false;
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: text
                .filter(|_| state == ButtonState::Pressed)
                .map(Into::into),
            repeat: false,
            window,
        });
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        seen |= keys.pressed(key_code) || keys.just_pressed(key_code);
    }
    seen
}

fn tap(app: &mut App, key_code: KeyCode) -> bool {
    let logical = match key_code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::ArrowUp => Key::ArrowUp,
        KeyCode::ArrowDown => Key::ArrowDown,
        KeyCode::ArrowLeft => Key::ArrowLeft,
        KeyCode::ArrowRight => Key::ArrowRight,
        KeyCode::ShiftLeft => Key::Shift,
        _ => Key::Unidentified(NativeKey::Unidentified),
    };
    key(app, key_code, logical, None)
}

/// Types `text`, a key a character, at the keys' places on an AZERTY keyboard: Z, Q, S, D are
/// where the game reads its driving keys and the screen its lines (W, A, S, D): they must type.
/// Returns whether the game saw any of them.
fn type_in(app: &mut App, text: &str) -> bool {
    let mut seen = false;
    for c in text.chars() {
        let code = match c.to_ascii_lowercase() {
            'z' => KeyCode::KeyW,
            'q' => KeyCode::KeyA,
            'a' => KeyCode::KeyQ,
            'w' => KeyCode::KeyZ,
            'd' => KeyCode::KeyD,
            's' => KeyCode::KeyS,
            'g' => KeyCode::KeyG,
            'o' => KeyCode::KeyO,
            'p' => KeyCode::KeyP,
            'l' => KeyCode::KeyL,
            'n' => KeyCode::KeyN,
            'é' | '2' => KeyCode::Digit2,
            '4' => KeyCode::Digit4,
            ' ' => KeyCode::Space,
            _ => KeyCode::KeyM,
        };
        let typed = c.to_string();
        let logical = if c == ' ' {
            Key::Space
        } else {
            Key::Character(typed.as_str().into())
        };
        seen |= key(app, code, logical, Some(&typed));
    }
    seen
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

/// The entity marked `C` that `wanted` picks.
fn find<C: Component>(app: &mut App, wanted: impl Fn(&C) -> bool) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &C)>()
        .iter(world)
        .find(|(_, c)| wanted(c))
        .map(|(entity, _)| entity)
        .expect("no such entity")
}

/// The text of the entity marked `C` that `wanted` picks.
fn text_of<C: Component>(app: &mut App, wanted: impl Fn(&C) -> bool) -> String {
    let entity = find::<C>(app, wanted);
    app.world().get::<Text>(entity).unwrap().0.clone()
}

/// The mouse, as picking describes it.
fn mouse() -> Pointer {
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 1,
                height: 1,
            },
            position: Vec2::ZERO,
        },
    )
}

/// A click on `entity`, as picking triggers it (it bubbles up to its parents).
fn click(app: &mut App, entity: Entity) {
    app.world_mut().trigger(PointerClick {
        entity,
        pointer: mouse(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: Duration::from_millis(80),
        count: 1,
    });
    run(app, 2);
}

/// The pointer coming over `entity`.
fn hover(app: &mut App, entity: Entity) {
    app.world_mut().trigger(PointerOver {
        entity,
        pointer: mouse(),
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    run(app, 2);
}

/// The preview's kart visual: its pivot (its looks).
fn preview_pivot(app: &mut App) -> (Entity, KartBodyPivot) {
    let body = app.world().resource::<PilotPreview>().body;
    let world = app.world_mut();
    let pivots: Vec<(Entity, KartBodyPivot)> = world
        .query::<(Entity, &KartBodyPivot)>()
        .iter(world)
        .filter(|(_, pivot)| pivot.body == body)
        .map(|(entity, pivot)| (entity, *pivot))
        .collect();
    assert_eq!(pivots.len(), 1, "one kart in the preview");
    pivots[0]
}

/// The driver under a kart visual's pivot.
fn driver_of(app: &mut App, pivot: Entity) -> DriverVisual {
    let world = app.world();
    world
        .get::<Children>(pivot)
        .unwrap()
        .iter()
        .find_map(|child| world.get::<DriverVisual>(child).copied())
        .expect("a driver in the kart")
}

/// The stats the screen shows: each bar's share of its track, its value written.
fn shown_stats(app: &mut App) -> Vec<(Stat, f32, String)> {
    let world = app.world_mut();
    let mut bars: Vec<(Stat, f32)> = world
        .query::<(&StatBar, &Node)>()
        .iter(world)
        .map(|(bar, node)| match node.width {
            Val::Percent(share) => (bar.stat, share / 100.0),
            other => panic!("{other:?}"),
        })
        .collect();
    bars.sort_by_key(|(stat, _)| stat.index());
    let mut values: Vec<(Stat, String)> = world
        .query::<(&StatValue, &Text)>()
        .iter(world)
        .map(|(value, text)| (value.stat, text.0.clone()))
        .collect();
    values.sort_by_key(|(stat, _)| stat.index());
    assert_eq!(bars.len(), values.len());
    bars.into_iter()
        .zip(values)
        .map(|((stat, share), (_, value))| (stat, share, value))
        .collect()
}

/// The screen shows `choice`: its bars and values `(level + 3) / 4`, its Speed and Handling on
/// each terrain, its lines' values, and the preview's kart is it (its parts, its driver).
fn assert_shows(app: &mut App, choice: &Loadout) {
    let roster = app.world().resource::<Roster>().clone();
    let stats = roster.stats(choice);
    let shown = shown_stats(app);
    assert_eq!(shown.len(), Stat::ALL.len());
    for (stat, share, value) in shown {
        let wanted = stats.display(stat);
        assert!(
            (share - pilot::bar_share(wanted)).abs() < 1e-5,
            "{stat:?}: {share}"
        );
        assert_eq!(value, pilot::french(wanted), "{stat:?}");
    }
    let world = app.world_mut();
    let terrains: Vec<(TerrainValue, String)> = world
        .query::<(&TerrainValue, &Text)>()
        .iter(world)
        .map(|(value, text)| (*value, text.0.clone()))
        .collect();
    assert_eq!(terrains.len(), 2 * Terrain::ALL.len());
    for (value, text) in terrains {
        let level = if value.speed {
            stats.speed_on(value.terrain)
        } else {
            stats.handling_on(value.terrain)
        };
        assert_eq!(text, pilot::french(display_level(level)), "{value:?}");
    }
    let (pivot, kart) = preview_pivot(app);
    assert_eq!(kart.looks, roster.looks(choice));
    let driver = driver_of(app, pivot);
    assert_eq!(driver.character, Some(choice.character));
    assert_eq!(driver.size, roster.character(choice).size);
    let character = roster.character(choice);
    assert_eq!(
        text_of::<LineValue>(app, |v| v.row == 0),
        format!("{} · {}", character.name, character.class.label())
    );
    for kind in [PartKind::Kart, PartKind::Wheels, PartKind::Wing] {
        assert_eq!(
            text_of::<LineValue>(app, |v| v.row == kind.index()),
            roster.name(kind, choice.get(kind))
        );
    }
}

/// Issue #98 acceptance: G opens the « Pilote » screen: its tree (six lines, the pseudo's
/// first, the nine animals of the grid, the twelve colors, the seven stats and the terrains),
/// its 3D preview (a camera rendering into an image on its own layer, the kart of the choice
/// with its driver, far under the game). **Each choice changes, at once, the preview's kart and
/// the bars**: the character (its driver, its size, the stats), the kart, the wheels and the
/// wing (their looks; on its line the wing opens, though the game waits, and the camera steps
/// back), the color (the body's paint), with the keys or the mouse. « Valider »: the player's
/// kart is that choice (its stats, driver, parts and color).
#[test]
fn each_choice_changes_the_preview_the_stats_and_then_the_players_kart() {
    let mut app = pilot_app(TrackChoice::TestGround);
    let roster = app.world().resource::<Roster>().clone();
    tap(&mut app, crate::characters::SELECTION_KEY);
    run(&mut app, 2);
    assert!(screen(&app).open);
    assert_eq!(screen(&app).row, 0, "the focus on the character");
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    assert_eq!(count::<With<PilotRoot>>(&mut app), 1);
    let world = app.world_mut();
    let mut rows: Vec<usize> = world
        .query::<&PilotLine>()
        .iter(world)
        .map(|line| line.row)
        .collect();
    rows.sort();
    assert_eq!(rows, (0..SelectionScreen::ROWS).collect::<Vec<_>>());
    assert_eq!(
        count::<With<PortraitCell>>(&mut app),
        roster.characters.len()
    );
    assert_eq!(count::<With<ColorSwatch>>(&mut app), KartColor::ALL.len());
    assert_eq!(count::<With<PseudoField>>(&mut app), 1);
    assert_eq!(count::<With<PilotButton>>(&mut app), 2);
    assert_eq!(count::<With<PreviewFrame>>(&mut app), 1);
    // The preview: its camera renders into an image, on its layer, far under the game.
    let preview = app.world().resource::<PilotPreview>().clone();
    let target = app.world().get::<RenderTarget>(preview.camera).unwrap();
    assert_eq!(target.as_image(), Some(&preview.image));
    assert!(
        app.world()
            .resource::<Assets<Image>>()
            .contains(&preview.image)
    );
    assert_eq!(
        app.world().get::<RenderLayers>(preview.camera),
        Some(&RenderLayers::layer(PREVIEW_LAYER))
    );
    let kart = preview.kart.expect("the kart of the choice");
    let at = app
        .world()
        .get::<GlobalTransform>(kart)
        .unwrap()
        .translation();
    assert!(at.y < -2000.0, "far under the circuits: {at}");
    // Every entity of the preview's kart is on its layer (none the game's camera would draw).
    let world = app.world_mut();
    let mut under: Vec<Entity> = vec![kart];
    let mut at = 0;
    while at < under.len() {
        if let Some(children) = world.get::<Children>(under[at]) {
            under.extend(children.iter());
        }
        at += 1;
    }
    assert!(under.len() > 10, "{under:?}");
    for entity in &under {
        assert_eq!(
            world.get::<RenderLayers>(*entity),
            Some(&RenderLayers::layer(PREVIEW_LAYER)),
            "{entity}"
        );
    }
    let mut choice = roster.default_loadout();
    assert_shows(&mut app, &choice);

    // The character, right three times: the pig (heavy). The preview has it, at once.
    for _ in 0..3 {
        tap(&mut app, KeyCode::ArrowRight);
    }
    choice.character = roster.find(PartKind::Character, "Cochon").unwrap();
    assert_eq!(screen(&app).choice, choice);
    assert_shows(&mut app, &choice);
    assert_ne!(app.world().resource::<PilotPreview>().kart, Some(kart));
    let world = app.world_mut();
    for (cell, fill) in world
        .query::<(&PortraitCell, &BackgroundColor)>()
        .iter(world)
    {
        assert_eq!(
            fill.0.alpha() > 0.5,
            cell.character == choice.character,
            "only its portrait stands out: {cell:?}"
        );
    }
    // The kart, the wheels, the wing: the mouse, on the « > » of their lines.
    for (kind, name) in [
        (PartKind::Kart, "Baignoire"),
        (PartKind::Wheels, "Pneus de tracteur"),
        (PartKind::Wing, "Plume"),
    ] {
        let arrow = find::<PilotArrow>(&mut app, |a| a.row == kind.index() && a.step == 1);
        click(&mut app, arrow);
        choice.set(kind, roster.find(kind, name).unwrap());
        assert_eq!(screen(&app).choice, choice, "{kind:?}");
        assert_eq!(screen(&app).row, kind.index());
        assert_shows(&mut app, &choice);
    }
    // On the wing's line, the wing opens (on real time: the game waits) and the camera steps
    // back.
    let body = app.world().resource::<PilotPreview>().body;
    assert!(app.world().get::<KartEnvironment>(body).unwrap().gliding);
    run(&mut app, 40);
    let (pivot, _) = preview_pivot(&mut app);
    let world = app.world_mut();
    let open: Vec<f32> = world
        .query::<(&Wing, &ChildOf)>()
        .iter(world)
        .filter(|(_, child_of)| child_of.parent() == pivot)
        .map(|(wing, _)| wing.open)
        .collect();
    assert_eq!(open, [1.0], "the wing open");
    let camera = app.world().resource::<PilotPreview>().camera;
    let away = app.world().get::<Transform>(camera).unwrap().translation - PREVIEW_ORIGIN;
    assert!(
        away.length() > pilot::preview_camera(0.0).translation.length() + 1.0,
        "{away}"
    );
    // The color: down (the wing folds), a click on red.
    tap(&mut app, KeyCode::ArrowDown);
    assert!(screen(&app).on_color_row());
    assert!(!app.world().get::<KartEnvironment>(body).unwrap().gliding);
    let red = find::<ColorSwatch>(&mut app, |s| s.color == KartColor::Red);
    click(&mut app, red);
    assert_eq!(screen(&app).color, KartColor::Red);
    assert_eq!(
        app.world().get::<KartPaint>(body),
        Some(&KartPaint(KartColor::Red))
    );
    let world = app.world_mut();
    let paints: Vec<KartColor> = world
        .query::<&BodyPaint>()
        .iter(world)
        .filter(|paint| paint.kart == body)
        .map(|paint| paint.shown)
        .collect();
    assert_eq!(paints, [KartColor::Red], "the preview's body painted red");
    assert_eq!(
        text_of::<LineValue>(&mut app, |v| v.row == SelectionScreen::COLOR_ROW),
        "Rouge"
    );
    let world = app.world_mut();
    for (swatch, ring) in world.query::<(&ColorSwatch, &BorderColor)>().iter(world) {
        assert_eq!(
            ring.top == super::style::palette::MYRTILLE,
            swatch.color == KartColor::Red,
            "the ring around red only: {swatch:?}"
        );
    }
    // A portrait clicked: its character.
    let rabbit = roster.find(PartKind::Character, "Lapin").unwrap();
    let cell = find::<PortraitCell>(&mut app, |c| c.character == rabbit);
    click(&mut app, cell);
    choice.character = rabbit;
    assert_eq!(screen(&app).row, 0);
    assert_shows(&mut app, &choice);
    // The pointer over a line gives it the focus.
    let wheels = find::<PilotLine>(&mut app, |l| l.row == PartKind::Wheels.index());
    hover(&mut app, wheels);
    assert_eq!(screen(&app).row, PartKind::Wheels.index());
    // Nothing kept yet.
    assert_eq!(
        app.world().resource::<PlayerLoadout>().0,
        roster.default_loadout()
    );
    assert_eq!(app.world().resource::<PlayerColor>().0, KartColor::Pink);

    // « Valider » (a click): the player's kart is the choice, the race starts over with it.
    let old = player(&mut app);
    let confirm = find::<PilotButton>(&mut app, |b| b.confirm);
    click(&mut app, confirm);
    assert!(!screen(&app).open);
    wait_for_the_player(&mut app);
    run(&mut app, 3);
    let kart = player(&mut app);
    assert_ne!(kart, old);
    assert_eq!(app.world().resource::<PlayerLoadout>().0, choice);
    assert_eq!(app.world().resource::<PlayerColor>().0, KartColor::Red);
    assert_eq!(
        app.world().get::<KartStats>(kart),
        Some(&roster.stats(&choice))
    );
    assert_eq!(
        app.world().get::<KartPaint>(kart),
        Some(&KartPaint(KartColor::Red))
    );
    let world = app.world_mut();
    let pivots: Vec<(Entity, KartBodyPivot)> = world
        .query::<(Entity, &KartBodyPivot)>()
        .iter(world)
        .filter(|(_, pivot)| pivot.body == kart)
        .map(|(entity, pivot)| (entity, *pivot))
        .collect();
    assert_eq!(pivots.len(), 1);
    assert_eq!(pivots[0].1.looks, roster.looks(&choice));
    assert_eq!(driver_of(&mut app, pivots[0].0).character, Some(rabbit));
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
}

/// Issue #98 acceptance: the pseudo is typed without driving the kart nor triggering anything.
/// Entrée on its line (or a click in the field: Bevy's focus on it) gives it the keyboard: the
/// game sees no key (`KeyboardCapture`). Z, Q, S, D, the arrows, Espace, G, O, P, L, N are
/// letters in it, or nothing: no line changes, no choice, no other screen, the kart does not
/// move. The field keeps 12 characters at most, those the HUD writes (an emoji is left out);
/// each change is the pseudo at once. Entrée or Échap give the keys back (the screen stays open:
/// a second Échap closes it), and so does a click elsewhere. Empty, the field shows the
/// character's name.
#[test]
fn the_pseudo_is_typed_without_driving_the_kart() {
    let mut app = pilot_app(TrackChoice::TestGround);
    assert_eq!(app.world().resource::<NetSettings>().pseudo, None);
    let kart = player(&mut app);
    let position = |app: &mut App| {
        let kart = player(app);
        app.world().get::<Transform>(kart).unwrap().translation
    };
    tap(&mut app, crate::characters::SELECTION_KEY);
    let at = position(&mut app);
    run(&mut app, 2);
    let hint = find::<PseudoHint>(&mut app, |_| true);
    assert_eq!(
        app.world().get::<Node>(hint).unwrap().display,
        Display::Flex,
        "empty: the character's name"
    );
    let name = app
        .world()
        .resource::<Roster>()
        .name(PartKind::Character, screen(&app).choice.character)
        .to_string();
    assert_eq!(app.world().get::<Text>(hint).unwrap().0, name);
    tap(&mut app, KeyCode::ArrowUp);
    assert!(screen(&app).on_pseudo_row());
    let before = screen(&app);
    tap(&mut app, KeyCode::Enter);
    run(&mut app, 1);
    let field = find::<PseudoField>(&mut app, |_| true);
    assert!(screen(&app).open && screen(&app).typing);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert!(app.world().resource::<KeyboardCapture>().0);

    let value = |app: &App| {
        app.world()
            .get::<EditableText>(field)
            .unwrap()
            .value()
            .to_string()
    };
    assert!(!type_in(&mut app, "Zoé😀 42"), "the game saw no key");
    assert_eq!(value(&app), "Zoé 42", "the emoji left out");
    assert_eq!(
        app.world().resource::<NetSettings>().pseudo.as_deref(),
        Some("Zoé 42"),
        "the pseudo at once"
    );
    assert_eq!(
        app.world().get::<Node>(hint).unwrap().display,
        Display::None
    );
    // Every key of the game types, or does nothing: no line, no choice, no screen, no kart.
    assert!(!type_in(&mut app, " qsdgo"));
    for code in [
        KeyCode::ArrowUp,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowDown,
        KeyCode::ShiftLeft,
        KeyCode::F3,
    ] {
        assert!(!tap(&mut app, code), "{code:?} reached the game");
    }
    let now = screen(&app);
    assert!(now.open && now.typing);
    assert_eq!(now.row, SelectionScreen::PSEUDO_ROW);
    assert_eq!((now.choice, now.color), (before.choice, before.color));
    assert_eq!(value(&app), "Zoé 42 qsdgo");
    assert!(!type_in(&mut app, "pln"));
    assert_eq!(value(&app), "Zoé 42 qsdgo", "12 characters at most");
    assert_eq!(
        app.world().resource::<NetSettings>().pseudo.as_deref(),
        Some("Zoé 42 qsdgo")
    );
    assert!(!app.world().resource::<AdminPanel>().open);
    assert!(!app.world().resource::<crate::options::OptionsMenu>().open);
    assert!(!app.world().resource::<crate::modes::RaceMenu>().open);
    assert!(!app.world().resource::<crate::net::LobbyWindow>().open);
    assert_eq!(
        app.world().get::<KartInput>(kart),
        Some(&KartInput::default())
    );
    assert!(
        (position(&mut app) - at).length() < 1e-4,
        "the kart did not move"
    );

    // Entrée: the keys are the game's again (the screen's: down goes to the character).
    tap(&mut app, KeyCode::Enter);
    run(&mut app, 1);
    assert!(screen(&app).open && !screen(&app).typing);
    assert!(!app.world().resource::<KeyboardCapture>().0);
    assert!(tap(&mut app, KeyCode::KeyM), "the game sees the keys again");
    assert_ne!(app.world().resource::<InputFocus>().get(), Some(field));
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(screen(&app).row, 0);

    // A click in the field (Bevy's focus on it): typing again, on its line; Échap gives the keys
    // back, and only that.
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(field, FocusCause::Pressed);
    run(&mut app, 2);
    assert!(screen(&app).typing && screen(&app).on_pseudo_row());
    assert!(app.world().resource::<KeyboardCapture>().0);
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 1);
    assert!(!screen(&app).typing);
    assert!(screen(&app).open, "Échap only stopped the typing");
    assert!(!app.world().resource::<KeyboardCapture>().0);
    // A click elsewhere: the focus leaves the field.
    tap(&mut app, KeyCode::Enter);
    run(&mut app, 1);
    assert!(screen(&app).typing);
    app.world_mut().resource_mut::<InputFocus>().clear();
    run(&mut app, 2);
    assert!(!screen(&app).typing);
    assert!(!app.world().resource::<KeyboardCapture>().0);
    tap(&mut app, KeyCode::Escape);
    assert!(!screen(&app).open, "the second Échap closes the screen");
    assert_eq!(
        app.world().resource::<NetSettings>().pseudo.as_deref(),
        Some("Zoé 42 qsdgo"),
        "the pseudo stays"
    );
    // Reopened, the field has it.
    tap(&mut app, crate::characters::SELECTION_KEY);
    run(&mut app, 2);
    let field = find::<PseudoField>(&mut app, |_| true);
    let text = app.world().get::<EditableText>(field).unwrap();
    assert_eq!(text.value().to_string(), "Zoé 42 qsdgo");
}

/// Every entity of the world (resources included) and every image.
fn census(app: &mut App) -> (usize, usize) {
    let entities = app.world_mut().query::<Entity>().iter(app.world()).count();
    let images = app.world().resource::<Assets<Image>>().len();
    (entities, images)
}

/// Issue #98 acceptance: the screen closed leaves nothing: no node, no observer, no camera of
/// its own, no light, no kart, no image of the preview. The portraits' sheet is rendered once
/// (its scene and camera a few frames, then gone) and kept: open again, nothing is rendered,
/// and closing it again leaves exactly what the first close left.
#[test]
fn the_screen_closed_leaves_nothing_and_the_portraits_are_rendered_once() {
    let mut app = pilot_app(TrackChoice::TestGround);
    run(&mut app, 5);
    let before = census(&mut app);
    let cameras = count::<With<Camera>>(&mut app);
    tap(&mut app, crate::characters::SELECTION_KEY);
    run(&mut app, 2);
    assert!(count::<With<MenuNode>>(&mut app) > 100);
    assert_eq!(
        count::<With<Camera>>(&mut app),
        cameras + 2,
        "the preview's and the portraits'"
    );
    assert!(
        count::<With<PortraitScene>>(&mut app) >= 3,
        "scene, camera, light"
    );
    // The portraits' sheet: three cells a side.
    let sheet = app.world().resource::<Portraits>().image.clone();
    let size = app
        .world()
        .resource::<Assets<Image>>()
        .get(&sheet)
        .unwrap()
        .size();
    assert_eq!(size, UVec2::splat(3 * PORTRAIT_PX));
    run(&mut app, PORTRAIT_FRAMES as usize + 2);
    assert_eq!(
        app.world().resource::<Portraits>().state,
        PortraitState::Ready
    );
    assert_eq!(
        count::<With<PortraitScene>>(&mut app),
        0,
        "rendered, then gone"
    );
    assert_eq!(
        count::<With<Camera>>(&mut app),
        cameras + 1,
        "the preview's"
    );
    // Each cell shows its part of the sheet.
    let roster = app.world().resource::<Roster>().clone();
    let world = app.world_mut();
    let cells: Vec<(usize, Option<Rect>)> = world
        .query::<(&PortraitCell, &Children)>()
        .iter(world)
        .map(|(cell, children)| {
            let rect = children
                .iter()
                .find_map(|c| world.get::<ImageNode>(c))
                .and_then(|image| image.rect);
            (cell.character, rect)
        })
        .collect();
    assert_eq!(cells.len(), 9);
    for (character, rect) in cells {
        let (line, column) = pilot::grid_cell(&roster, character);
        assert_eq!(
            rect,
            Some(pilot::portrait_rect(line, column)),
            "{character}"
        );
    }

    tap(&mut app, KeyCode::Escape);
    run(&mut app, 2);
    assert!(!screen(&app).open);
    assert_eq!(count::<With<MenuNode>>(&mut app), 0);
    assert_eq!(count::<With<PreviewScene>>(&mut app), 0);
    assert_eq!(count::<With<PreviewKart>>(&mut app), 0);
    assert_eq!(count::<With<LayerRoot>>(&mut app), 0);
    assert_eq!(count::<With<Camera>>(&mut app), cameras);
    assert!(app.world().get_resource::<PilotPreview>().is_none());
    let after = census(&mut app);
    // Kept: the screen's memory and the portraits' (two resources) and their sheet; and the
    // entity Bevy keeps for the preview's resource once removed (empty, reused the next time).
    assert_eq!(after.0, before.0 + 3, "{before:?} → {after:?}");
    assert!(app.world().get_resource::<PilotUi>().is_some());
    assert!(app.world().get_resource::<Portraits>().is_some());
    assert_eq!(
        after.1,
        before.1 + 1,
        "the sheet only: {before:?} → {after:?}"
    );

    // Again: nothing rendered, the same world once closed.
    tap(&mut app, crate::characters::SELECTION_KEY);
    run(&mut app, PORTRAIT_FRAMES as usize + 2);
    assert_eq!(
        count::<With<PortraitScene>>(&mut app),
        0,
        "not rendered again"
    );
    assert_eq!(count::<With<Camera>>(&mut app), cameras + 1);
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 2);
    assert_eq!(census(&mut app), after);
}

/// Issue #98 acceptance: the gamepad goes through the screen opened from the home screen (its
/// background races on behind it, never paused): the cross and the stick (a step per push) move
/// from line to line and change the choice, A keeps it (back to the home screen, no race
/// started), B leaves it as it was. The keys do the same (the arrows, Échap).
#[test]
fn the_gamepad_and_the_keys_go_through_the_screen_from_the_home() {
    let mut app = pilot_app(TrackChoice::TestGround);
    let roster = app.world().resource::<Roster>().clone();
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
    let stick = |app: &mut App, axis: GamepadAxis, value: f32| {
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                pad, axis, value,
            )));
        app.update();
    };
    app.world_mut().write_message(MainMenuAction::OpenAtLaunch);
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<Showcase>()
            .is_some_and(|s| s.running())
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let background = |app: &mut App| {
        let world = app.world_mut();
        let mut karts: Vec<Entity> = world
            .query_filtered::<Entity, With<ShowcaseKart>>()
            .iter(world)
            .collect();
        karts.sort();
        karts
    };
    let karts = background(&mut app);
    assert!(!karts.is_empty());
    // « Pilote », the last button of the home: up once (around), A.
    button(&mut app, GamepadButton::DPadUp);
    assert_eq!(app.world().resource::<MainMenu>().focused(), Entry::Garage);
    button(&mut app, GamepadButton::South);
    run(&mut app, 12);
    assert!(screen(&app).open);
    assert_eq!(count::<With<PilotRoot>>(&mut app), 1);
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());

    // The stick right: the next character; held, one step only.
    stick(&mut app, GamepadAxis::LeftStickX, 0.9);
    for _ in 0..10 {
        stick(&mut app, GamepadAxis::LeftStickX, 0.95);
    }
    stick(&mut app, GamepadAxis::LeftStickX, 0.0);
    let mut choice = roster.cycle(roster.default_loadout(), PartKind::Character, 1);
    assert_eq!(screen(&app).choice, choice);
    // Down twice (the cross, then the stick): the wheels; left: the previous ones.
    button(&mut app, GamepadButton::DPadDown);
    stick(&mut app, GamepadAxis::LeftStickY, -0.9);
    stick(&mut app, GamepadAxis::LeftStickY, 0.0);
    assert_eq!(screen(&app).row, PartKind::Wheels.index());
    button(&mut app, GamepadButton::DPadLeft);
    choice = roster.cycle(choice, PartKind::Wheels, -1);
    assert_eq!(screen(&app).choice, choice);
    // Up three times: around to the pseudo's line, where left and right do nothing.
    for _ in 0..3 {
        button(&mut app, GamepadButton::DPadUp);
    }
    assert!(screen(&app).on_pseudo_row());
    button(&mut app, GamepadButton::DPadRight);
    assert_eq!(screen(&app).choice, choice);
    // Up: the color; right: the next one.
    button(&mut app, GamepadButton::DPadUp);
    assert!(screen(&app).on_color_row());
    button(&mut app, GamepadButton::DPadRight);
    let color = KartColor::default().cycled(1);
    assert_eq!(screen(&app).color, color);

    // A: kept; back to the home, its background still racing (no race started), not paused.
    button(&mut app, GamepadButton::South);
    run(&mut app, 3);
    assert!(!screen(&app).open);
    assert_eq!(app.world().resource::<PlayerLoadout>().0, choice);
    assert_eq!(app.world().resource::<PlayerColor>().0, color);
    let menu = *app.world().resource::<MainMenu>();
    assert!(menu.open && !menu.in_game, "back to the home");
    assert_eq!(menu.focused(), Entry::Garage);
    assert!(app.world().resource::<Showcase>().running());
    assert_eq!(background(&mut app), karts, "the same background");
    assert_eq!(count::<With<PilotRoot>>(&mut app), 0);
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());

    // Again, with the keys: right changes the character, Échap leaves it as it was.
    button(&mut app, GamepadButton::South);
    run(&mut app, 12);
    assert!(screen(&app).open);
    tap(&mut app, KeyCode::ArrowRight);
    assert_ne!(screen(&app).choice, choice);
    tap(&mut app, KeyCode::Escape);
    assert!(!screen(&app).open);
    assert_eq!(app.world().resource::<PlayerLoadout>().0, choice);
    run(&mut app, 3);
    assert!(app.world().resource::<MainMenu>().open);
    // And B, the same.
    button(&mut app, GamepadButton::South);
    run(&mut app, 12);
    assert!(screen(&app).open);
    button(&mut app, GamepadButton::DPadRight);
    button(&mut app, GamepadButton::East);
    assert!(!screen(&app).open);
    assert_eq!(app.world().resource::<PlayerLoadout>().0, choice);
}

/// Issue #98: the screen fits the reference window (720 px high): its title, its panel (the
/// taller of its columns) and the keys, with a margin; its panel is in the charter's 720 to 900
/// px, its grid and its swatches fit in its right column.
#[test]
fn the_screen_fits_the_window() {
    use super::style::size;
    let line = |px: f32| 1.2 * px;
    let left = pilot::PREVIEW_SIZE.y
        + 10.0
        + Stat::ALL.len() as f32 * size::STAT_LINE
        + 10.0
        + 3.0 * (size::STAT_LINE - 2.0)
        + 10.0
        + 2.0
        + size::PILOT_BUTTON_HEIGHT;
    let grid = 2.0 * 6.0
        + line(size::LINE_TEXT)
        + 3.0 * (size::PORTRAIT + line(size::PORTRAIT_NAME))
        + 3.0 * size::GRID_GAP
        + size::LINE_DEPTH;
    let right = grid + 5.0 * size::LINE_HEIGHT + 5.0 * size::LINE_GAP;
    let panel = 2.0 * size::PANEL_PADDING + left.max(right);
    let screen = line(size::PILOT_TITLE) + 10.0 + panel + 2.0 * (10.0 + line(size::HINT));
    assert!(
        screen + 16.0 <= 720.0,
        "{screen} (left {left}, right {right})"
    );
    const { assert!(size::PILOT_WIDTH >= 720.0 && size::PILOT_WIDTH <= 900.0) };
    let right_width =
        size::PILOT_WIDTH - 2.0 * size::PANEL_PADDING - size::PILOT_GAP - size::PILOT_LEFT;
    let cells = 18.0 + size::CLASS_WIDTH + 3.0 * size::PORTRAIT_CELL + 12.0;
    assert!(cells <= right_width, "{cells} > {right_width}");
    let swatches =
        18.0 + size::LINE_LABEL_WIDTH + 8.0 + 12.0 * size::SWATCH + 11.0 * size::SWATCH_GAP + 8.0;
    assert!(swatches <= right_width, "{swatches} > {right_width}");
    assert_eq!(pilot::PREVIEW_SIZE.x, size::PILOT_LEFT);
    // The table of the terrains: « Anti-gravité » on one line, the three others at least 56 px.
    let terrains = size::TERRAIN_LABEL_WIDTH + 3.0 * 56.0 + size::TERRAIN_WIDE;
    assert!(
        terrains <= size::PILOT_LEFT,
        "{terrains} > {}",
        size::PILOT_LEFT
    );
}
