//! Headless scenarios of the « Jouer solo » screen (issue #99): the whole game (`GamePlugin`)
//! with `MinimalPlugins`, [`MainMenuPlugin`] added by hand (it brings the screen, `SoloPlugin`).
//! No picking headless: the pointer's events are triggered on the entities the pointer would be
//! over, as picking would (`click`, `hover`).

use std::time::Duration;

use bevy::camera::NormalizedRenderTarget;
use bevy::input::ButtonState as KeyState;
use bevy::input::gamepad::{GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::picking::backend::HitData;
use bevy::picking::events::{Pointer, PointerClick, PointerOver};
use bevy::picking::pointer::{Location, PointerButton, PointerId};

use super::*;
use crate::modes::{Catalog, GameMode, MenuRow, RaceMenu, RaceMenuAction};
use crate::race::{GrandPrix, RaceGrid};
use crate::tests::{game_app_unloaded, wait_for_the_player};
use crate::track::{TrackChoice, TrackLayout};

/// The game on Prairie Pompon with the menu (and so the « Jouer solo » screen), 3 bots, ready.
fn solo_app() -> App {
    let mut app = game_app_unloaded(TrackChoice::default(), 3);
    app.add_plugins(MainMenuPlugin { at_launch: false });
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

fn tap(app: &mut App, key_code: KeyCode) {
    press(app, key_code, KeyState::Pressed);
    press(app, key_code, KeyState::Released);
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

fn race_menu(app: &App) -> RaceMenu {
    app.world().resource::<RaceMenu>().clone()
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

/// Every text under `C`-marked entities that `wanted` picks.
fn texts_of<C: Component>(app: &mut App, wanted: impl Fn(&C) -> bool) -> Vec<String> {
    let world = app.world_mut();
    let roots: Vec<Entity> = world
        .query::<(Entity, &C)>()
        .iter(world)
        .filter(|(_, c)| wanted(c))
        .map(|(entity, _)| entity)
        .collect();
    let mut texts = Vec::new();
    let mut stack = roots;
    while let Some(entity) = stack.pop() {
        if let Some(text) = world.get::<Text>(entity) {
            texts.push(text.0.clone());
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    texts
}

fn text_of<C: Component>(app: &mut App) -> String {
    let entity = find::<C>(app, |_| true);
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

fn hover(app: &mut App, entity: Entity) {
    app.world_mut().trigger(PointerOver {
        entity,
        pointer: mouse(),
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    run(app, 2);
}

/// The lines of the left column, in their order: (index, row, label, value).
fn left_lines(app: &mut App) -> Vec<(usize, MenuRow)> {
    let world = app.world_mut();
    let mut lines: Vec<(usize, MenuRow)> = world
        .query_filtered::<&SoloLine, Without<RaceCard>>()
        .iter(world)
        .map(|line| (line.index, line.row))
        .collect();
    lines.sort_by_key(|(index, _)| *index);
    lines
}

/// Runs updates until every vignette's circuit file is loaded and its outline drawn.
fn wait_for_the_vignettes(app: &mut App) {
    for _ in 0..2000 {
        app.update();
        let ui = app.world().resource::<SoloUi>();
        let tracks = app.world().resource::<Assets<crate::track::TrackData>>();
        if ui.tracks.iter().all(|handle| tracks.contains(handle))
            && app.world().resource::<RaceMenu>().open
        {
            run(app, 2);
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let ui = app.world().resource::<SoloUi>();
    let server = app.world().resource::<AssetServer>();
    let states: Vec<String> = ui
        .tracks
        .iter()
        .map(|h| format!("{:?}", server.load_state(h)))
        .collect();
    panic!("the vignettes' circuits never loaded: {states:?}");
}

/// The dots of the road in each vignette (by its circuit).
fn vignette_dots(app: &mut App) -> Vec<(usize, usize)> {
    let world = app.world_mut();
    let vignettes: Vec<(Entity, usize)> = world
        .query::<(Entity, &Vignette)>()
        .iter(world)
        .map(|(entity, vignette)| (entity, vignette.circuit))
        .collect();
    vignettes
        .into_iter()
        .map(|(entity, circuit)| {
            let road = world.get::<Children>(entity).unwrap()[0];
            let dots = world.get::<Children>(road).map_or(0, |c| c.len());
            (circuit, dots)
        })
        .collect()
}

/// Issue #99 acceptance: « Jouer solo » opens its screen (N in a race; from the home screen and
/// the menu opened from the game, its button): its title, its lines on the left (the mode, the
/// circuit, the mirror, the bots, the items: no class), the help of the line with the focus, its
/// two buttons; on the right, the circuit, big, in its vignette (the colors of its biome and its
/// road seen from above, once its file is loaded). The game waits behind it (from a race, in
/// solo). Échap closes it: nothing is left of it (no node, no observer), the race goes on.
#[test]
fn jouer_solo_opens_its_screen_and_leaves_nothing_once_closed() {
    let mut app = solo_app();
    run(&mut app, 3);
    let nodes = count::<With<MenuNode>>(&mut app);
    tap(&mut app, crate::modes::RACE_MENU_KEY);
    run(&mut app, 2);
    assert!(race_menu(&app).open);
    assert!(app.world().resource::<Time<Virtual>>().is_paused());
    assert!(app.world().get_resource::<SoloUi>().is_some());
    assert_eq!(count::<With<SoloRoot>>(&mut app), 1);
    let title = texts_of::<SoloColumn>(&mut app, |_| true);
    assert!(title.iter().any(|t| t == "Jouer solo"), "{title:?}");
    use MenuRow::*;
    let lines = left_lines(&mut app);
    assert_eq!(
        lines,
        [(0, Mode), (1, Circuit), (2, Mirror), (3, Bots), (4, Items)]
    );
    let values = texts_of::<SoloLine>(&mut app, |line| line.row == Mode);
    assert!(values.contains(&"Circuit seul".to_string()), "{values:?}");
    assert!(
        texts_of::<SoloLine>(&mut app, |line| line.row == Circuit)
            .contains(&"Prairie Pompon".to_string())
    );
    assert_eq!(text_of::<SoloHeader>(&mut app), "Prairie Pompon");
    assert_eq!(
        text_of::<SoloHelp>(&mut app),
        "Une course, sur le circuit de ton choix."
    );
    assert_eq!(count::<With<SoloButton>>(&mut app), 2);
    assert_eq!(count::<With<SoloArrow>>(&mut app), 2 * 5);
    // The vignette, big, with its road once its file is loaded.
    wait_for_the_vignettes(&mut app);
    let dots = vignette_dots(&mut app);
    assert_eq!(dots, [(0, 150)], "the circuit's road, from above");
    // A key changes a value in place (the same tree); Échap closes it.
    let root = find::<SoloRoot>(&mut app, |_| true);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowRight);
    assert!(race_menu(&app).setup.mirror);
    let mirrored = find::<SoloRoot>(&mut app, |_| true);
    assert_ne!(mirrored, root, "the vignette mirrored: another tree");
    let mut mirror = texts_of::<SoloLine>(&mut app, |line| line.row == Mirror);
    mirror.sort();
    assert_eq!(mirror, ["<", ">", "Miroir", "Oui"]);
    assert_eq!(
        text_of::<SoloHelp>(&mut app),
        "Le circuit inversé gauche-droite."
    );
    tap(&mut app, KeyCode::Escape);
    run(&mut app, 2);
    assert!(!race_menu(&app).open);
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    assert_eq!(count::<With<SoloRoot>>(&mut app), 0);
    assert_eq!(count::<With<MenuNode>>(&mut app), nodes, "nothing left");
    // Nothing changed: the same race goes on, not mirrored.
    assert!(!app.world().resource::<TrackLayout>().mirrored);
}

/// Issue #99: the screen from the home screen (« Jouer solo »), its background racing behind
/// it: the championship of a cup (its icon and name on its line; its four circuits on the right,
/// each with its number, its vignette and its name), then a custom one with its circuits chosen
/// (a line per race on the right, which the keys go through); « Retour » comes back home.
#[test]
fn the_screen_shows_the_cup_and_the_custom_races() {
    let mut app = solo_app();
    app.world_mut().write_message(MainMenuAction::OpenAtLaunch);
    run(&mut app, 2);
    // The home's « Jouer solo » (its first button).
    tap(&mut app, KeyCode::Enter);
    run(&mut app, (ACTIVATE_DELAY * 60.0).ceil() as usize + 3);
    assert!(race_menu(&app).open);
    assert!(app.world().resource::<MainMenu>().on_home());
    tap(&mut app, KeyCode::ArrowRight);
    use MenuRow::*;
    let menu = race_menu(&app);
    assert_eq!(menu.setup.mode, GameMode::GrandPrix);
    assert_eq!(left_lines(&mut app).len(), 6);
    assert!(
        texts_of::<SoloLine>(&mut app, |line| line.row == Cup)
            .contains(&"Coupe Doudou".to_string())
    );
    assert_eq!(text_of::<SoloHeader>(&mut app), "Coupe Doudou");
    let circuits = crate::admin::tracks::circuits();
    let world = app.world_mut();
    let mut cards: Vec<(usize, usize)> = world
        .query::<&RaceCard>()
        .iter(world)
        .map(|card| (card.race, card.circuit))
        .collect();
    cards.sort_unstable();
    assert_eq!(cards, [(0, 0), (1, 1), (2, 2), (3, 3)]);
    let names = texts_of::<RaceCard>(&mut app, |card| card.race == 2);
    assert!(names.contains(&circuits[2].name) && names.contains(&"3".to_string()));
    assert_eq!(count::<(With<RaceCard>, With<SoloLine>)>(&mut app), 0);
    assert_eq!(count::<With<Vignette>>(&mut app), 4);

    // A custom one, its circuits chosen: a line per race on the right.
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowRight);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowRight);
    let menu = race_menu(&app);
    assert_eq!(
        (menu.setup.formula, menu.setup.pick),
        (crate::modes::Formula::Custom, crate::modes::Pick::Chosen)
    );
    assert_eq!(text_of::<SoloHeader>(&mut app), "Tes 4 courses");
    assert_eq!(count::<(With<RaceCard>, With<SoloLine>)>(&mut app), 4);
    // Down to the first race (past the mirror, the bots and the items).
    for _ in 0..4 {
        tap(&mut app, KeyCode::ArrowDown);
    }
    assert_eq!(race_menu(&app).row(), Race(0));
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(race_menu(&app).setup.chosen[0], 1);
    assert!(texts_of::<RaceCard>(&mut app, |card| card.race == 0).contains(&circuits[1].name));
    // « Retour »: home, its background still racing.
    let back = find::<SoloButton>(&mut app, |button| !button.start);
    click(&mut app, back);
    run(&mut app, 4);
    assert!(!race_menu(&app).open);
    assert!(app.world().resource::<MainMenu>().open);
    assert_eq!(count::<With<SoloRoot>>(&mut app), 0);
}

/// Issue #99: the mouse: the pointer over a line gives it the focus (its help comes), a click
/// on « > » changes it, « C'est parti ! » starts the race of the choice: here without bots, on
/// the circuit chosen.
#[test]
fn the_mouse_focuses_changes_and_starts() {
    let mut app = solo_app();
    tap(&mut app, crate::modes::RACE_MENU_KEY);
    run(&mut app, 2);
    let bots = find::<SoloLine>(&mut app, |line| line.row == MenuRow::Bots);
    hover(&mut app, bots);
    assert_eq!(race_menu(&app).row(), MenuRow::Bots);
    assert_eq!(
        text_of::<SoloHelp>(&mut app),
        "Des bots complètent la grille à 12."
    );
    let arrow = find::<SoloArrow>(&mut app, |arrow| {
        arrow.row == MenuRow::Bots && arrow.step > 0
    });
    click(&mut app, arrow);
    assert!(!race_menu(&app).setup.bots);
    assert_eq!(text_of::<SoloHelp>(&mut app), "Toi seul sur la piste.");
    let next = find::<SoloArrow>(&mut app, |arrow| {
        arrow.row == MenuRow::Circuit && arrow.step > 0
    });
    click(&mut app, next);
    assert_eq!(race_menu(&app).setup.circuit, 1);
    let start = find::<SoloButton>(&mut app, |button| button.start);
    hover(&mut app, start);
    assert_eq!(
        app.world().resource::<SoloUi>().hovered,
        Some(true),
        "the button under the pointer"
    );
    click(&mut app, start);
    assert!(!race_menu(&app).open);
    for _ in 0..3000 {
        app.update();
        let built = app
            .world()
            .get_resource::<TrackLayout>()
            .is_some_and(|layout| layout.name == "Anneau Guimauve");
        if built && count::<With<crate::kart::PlayerKart>>(&mut app) == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    run(&mut app, 3);
    assert_eq!(count::<With<crate::race::Racer>>(&mut app), 1, "no bot");
    assert_eq!(app.world().resource::<RaceGrid>().bots, 0);
    assert_eq!(race_menu(&app).kept_bots, Some(3));
}

/// The gamepad, as a pad sends it.
fn pad_button(app: &mut App, pad: Entity, button: GamepadButton, value: f32) {
    app.world_mut()
        .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
            pad, button, value,
        )));
    app.update();
}

fn pad_tap(app: &mut App, pad: Entity, button: GamepadButton) {
    pad_button(app, pad, button, 1.0);
    pad_button(app, pad, button, 0.0);
}

/// Issue #99: the gamepad goes through the screen: the cross changes the line and its value, A
/// starts (a championship of the cup), B goes back.
#[test]
fn the_gamepad_goes_through_the_screen() {
    let mut app = solo_app();
    let pad = app
        .world_mut()
        .spawn((Gamepad::default(), GamepadSettings::default()))
        .id();
    run(&mut app, 2);
    tap(&mut app, crate::modes::RACE_MENU_KEY);
    run(&mut app, 2);
    pad_tap(&mut app, pad, GamepadButton::DPadDown);
    assert_eq!(race_menu(&app).row(), MenuRow::Circuit);
    pad_tap(&mut app, pad, GamepadButton::DPadUp);
    pad_tap(&mut app, pad, GamepadButton::DPadLeft);
    assert_eq!(race_menu(&app).setup.mode, GameMode::TimeTrial);
    pad_tap(&mut app, pad, GamepadButton::DPadRight);
    pad_tap(&mut app, pad, GamepadButton::DPadRight);
    assert_eq!(race_menu(&app).setup.mode, GameMode::GrandPrix);
    pad_tap(&mut app, pad, GamepadButton::East);
    assert!(!race_menu(&app).open, "B goes back");
    tap(&mut app, crate::modes::RACE_MENU_KEY);
    run(&mut app, 2);
    pad_tap(&mut app, pad, GamepadButton::DPadRight);
    assert_eq!(race_menu(&app).setup.mode, GameMode::GrandPrix);
    pad_tap(&mut app, pad, GamepadButton::South);
    assert!(!race_menu(&app).open);
    run(&mut app, 2);
    let championship = app.world().resource::<GrandPrix>();
    assert_eq!(championship.title, "Coupe Doudou");
    let catalog = Catalog::game(4);
    assert_eq!(championship.cup, catalog.races(&catalog.cups[0].circuits));
}

/// Issue #99: the screen fits the reference window (720 px high): its title, its panel (the
/// taller of its columns: the left one with its seven lines at most, its help and its buttons;
/// the right one with its header and six races, or the big vignette and the record) and the
/// keys, with a margin; its lines and its races fit in their columns.
#[test]
fn the_screen_fits_the_window() {
    use super::style::size;
    let line = |px: f32| 1.2 * px;
    let left = 7.0 * size::LINE_HEIGHT
        + 7.0 * size::LINE_GAP
        + 1.3 * size::SOLO_HELP
        + size::LINE_GAP
        + 4.0
        + size::PILOT_BUTTON_HEIGHT;
    let races = crate::admin::tracks::circuits().len() as f32;
    let right = line(size::SOLO_HEADER)
        + (races * (size::CARD_HEIGHT + size::LINE_GAP))
            .max(size::LINE_GAP + size::BIG_VIGNETTE_HEIGHT + 10.0 + line(size::SOLO_HELP + 2.0));
    let panel = 2.0 * size::PANEL_PADDING + left.max(right);
    let screen = line(size::PILOT_TITLE) + 10.0 + panel + 2.0 * (10.0 + line(size::HINT));
    assert!(
        screen + 16.0 <= 720.0,
        "{screen} (left {left}, right {right})"
    );
    let right_width =
        size::PILOT_WIDTH - 2.0 * size::PANEL_PADDING - size::PILOT_GAP - size::SOLO_LEFT;
    assert!(size::BIG_VIGNETTE_WIDTH <= right_width, "{right_width}");
    // A race chosen: its number, its vignette, « < », its name (« Anneau Guimauve »: about 0.5
    // em a character), « > ».
    let card = 8.0
        + size::CARD_BADGE
        + size::VIGNETTE_WIDTH
        + 2.0 * size::ARROW
        + 15.0 * 0.5 * size::CARD_TEXT
        + 5.0 * 8.0
        + 8.0;
    assert!(card <= right_width, "{card} > {right_width}");
    // A line: its label, « < », « Contre-la-montre », « > ».
    let value = 16.0 * 0.5 * size::LINE_TEXT;
    let left_line = 18.0 + size::LINE_LABEL_WIDTH + 2.0 * size::ARROW + value + 3.0 * 8.0 + 8.0;
    assert!(left_line <= size::SOLO_LEFT, "{left_line}");
}

/// The screen's tree follows the menu's actions too (the keys are tested in `modes`): the
/// action of a line's focus.
#[test]
fn the_focus_follows_the_actions() {
    let mut app = solo_app();
    tap(&mut app, crate::modes::RACE_MENU_KEY);
    run(&mut app, 2);
    app.world_mut().write_message(RaceMenuAction::Focus(4));
    run(&mut app, 2);
    assert_eq!(race_menu(&app).row(), MenuRow::Items);
    assert_eq!(
        text_of::<SoloHelp>(&mut app),
        "Des boîtes à objets sur la piste."
    );
    // Beyond the lines: nothing.
    app.world_mut().write_message(RaceMenuAction::Focus(40));
    run(&mut app, 2);
    assert_eq!(race_menu(&app).row(), MenuRow::Items);
}
