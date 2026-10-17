//! The « Jouer solo » screen (issue #99) drawn with `bevy_ui`, after the charter (`style.rs`,
//! `anim.rs`): over a veil, its title, a wide cream panel and the keys at the bottom. On the left,
//! the lines of the choice (the mode, then what it needs: the formula and the cup or the races of
//! a custom championship, or the circuit; the mirror, the bots and the items), what the line with
//! the focus is for, « C'est parti ! » and « Retour »; on the right, the races: the cup's or the
//! custom championship's, each with its number, its vignette (the circuit seen from above, in the
//! colors of its biome) and its name, a line each when its circuits are chosen one by one; or the
//! circuit of a race alone or of a time trial, big, with the record of the time trial. The line
//! with the focus is the charter's focused button (mango, a little bigger).
//!
//! Its state and its keys are `modes::RaceMenu`'s (headless, tested); here: its tree, the gamepad
//! and the mouse (the observers of its lines, arrows and buttons). The vignettes come from the
//! circuits' files, loaded the first time the screen opens.
//!
//! **Closed, nothing is left**: the tree and its observers go when it closes; its systems only run
//! while it is open (or its tree is still there). Nothing of it exists before it first opens.

use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use bevy::text::{FontSource, Justify};

use super::anim;
use super::nav::StickLatch;
use super::screen::{MenuAssets, MenuNode};
use super::solo;
use super::style::{self, ButtonState, font, palette, size, u};
use crate::kart::KeyboardLayout;
use crate::modes::{
    Catalog, GameMode, MenuRow, RaceMenu, RaceMenuAction, TimeTrialState, TrialKey,
};
use crate::options::OptionsMenu;
use crate::race::{GrandPrixTuning, RaceGrid};
use crate::track::TrackData;
use crate::track::biome::Palette;
use crate::ui::format::{hundredths, write_time};

/// The root of the screen's tree (the veil), with what it shows: another shape, another tree.
#[derive(Component, Clone, Debug, PartialEq)]
#[require(MenuNode)]
pub struct SoloRoot {
    pub shape: SoloShape,
}

/// What the tree is built for: the keyboard, the lines, the races and what they look like.
#[derive(Clone, Debug, PartialEq)]
pub struct SoloShape {
    pub layout: KeyboardLayout,
    pub rows: Vec<MenuRow>,
    pub mode: GameMode,
    pub cup: usize,
    pub program: Vec<usize>,
    pub mirror: bool,
    /// The circuits whose vignettes could be drawn (their files loaded).
    pub drawn: Vec<bool>,
}

/// The column of the title, the panel and the keys: it pops in.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct SoloColumn;

/// A line of the choice (`index`: in `RaceMenu::rows`): its pill takes the focus's look.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SoloLine {
    pub index: usize,
    pub row: MenuRow,
}

/// The value written on a line.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SoloValue {
    pub row: MenuRow,
}

/// « < » or « > » on a line.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SoloArrow {
    pub row: MenuRow,
    pub step: i32,
}

/// What the line with the focus is for.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct SoloHelp;

/// The header of the races.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct SoloHeader;

/// A race of the program on the right (0 = the first), on its circuit.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct RaceCard {
    pub race: usize,
    pub circuit: usize,
}

/// The vignette of a circuit: its biome's colors, and its outline once its file is loaded.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct Vignette {
    pub circuit: usize,
}

/// A line of keys at the bottom.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SoloHint(pub usize);

/// « C'est parti ! » (`start`) or « Retour ».
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SoloButton {
    pub start: bool,
}

/// What the screen remembers (real times, s). There once the screen first opened.
#[derive(Resource, Debug)]
pub struct SoloUi {
    pub opened_at: f64,
    /// The line with the focus, since when, and the one that lost it.
    pub focus: usize,
    pub focused_at: f64,
    pub unfocused: Option<usize>,
    /// The button under the pointer.
    pub hovered: Option<bool>,
    /// The circuits and the cups.
    pub catalog: Catalog,
    /// The circuits' files, for their vignettes (in the manifest's order).
    pub tracks: Vec<Handle<TrackData>>,
}

/// The real time (s).
fn now(time: &Time<Real>) -> f64 {
    time.elapsed_secs_f64()
}

fn text(content: impl Into<String>, face: &Handle<Font>, px: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: FontSource::from(face),
            font_size: font(px),
            ..default()
        },
        TextColor(color),
        Pickable::IGNORE,
        MenuNode,
    )
}

/// The look of a line: the charter's button, focused or not.
fn line_look(focused: bool) -> style::ButtonLook {
    let state = if focused {
        ButtonState::Focused
    } else {
        ButtonState::Normal
    };
    style::button_look(state, palette::LAGON)
}

/// A color of a biome's palette.
fn rgb((r, g, b): (f32, f32, f32)) -> Color {
    Color::srgb(r, g, b)
}

/// A color a little darker (the bottom of a vignette's ground).
fn shade((r, g, b): (f32, f32, f32)) -> (f32, f32, f32) {
    (r * 0.86, g * 0.86, b * 0.86)
}

/// The palette of circuit `circuit`, once its file is loaded (else the one without biome).
fn palette_of(ui: &SoloUi, tracks: &Assets<TrackData>, circuit: usize) -> Palette {
    ui.tracks
        .get(circuit)
        .and_then(|handle| tracks.get(handle))
        .and_then(|data| data.biome())
        .map_or_else(Palette::default, |biome| biome.palette)
}

/// The shape of the tree for the menu now.
fn shape_of(
    menu: &RaceMenu,
    ui: &SoloUi,
    tracks: &Assets<TrackData>,
    layout: KeyboardLayout,
    seed: u64,
) -> SoloShape {
    SoloShape {
        layout,
        rows: menu.rows(),
        mode: menu.setup.mode,
        cup: menu.setup.cup,
        program: menu.setup.program(seed, &ui.catalog),
        mirror: menu.setup.mirror,
        drawn: ui.tracks.iter().map(|h| tracks.contains(h)).collect(),
    }
}

/// The screen open: its tree, for what it shows now (another choice of lines or races, another
/// keyboard, a vignette's file loaded: another tree; the values change in place,
/// [`refresh_solo_tree`]). Closed: none of it. Not before the game has a camera (at launch, the
/// circuit is still loading): the sizes are shares of its view. The circuits' files are asked
/// for the first time it opens.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_solo_tree(
    mut commands: Commands,
    menu: Res<RaceMenu>,
    ui: Option<ResMut<SoloUi>>,
    time: Res<Time<Real>>,
    roots: Query<(Entity, &SoloRoot)>,
    (assets, layout, grid, gp_tuning): (
        Option<Res<MenuAssets>>,
        Option<Res<KeyboardLayout>>,
        Res<RaceGrid>,
        Res<GrandPrixTuning>,
    ),
    (server, tracks): (Option<Res<AssetServer>>, Option<Res<Assets<TrackData>>>),
    trial: Option<Res<TimeTrialState>>,
    cameras: Query<(), With<Camera>>,
) {
    if !menu.open {
        for (entity, _) in &roots {
            commands.entity(entity).despawn();
        }
        return;
    }
    if cameras.is_empty() {
        return;
    }
    let now = now(&time);
    let Some(mut ui) = ui else {
        let catalog = Catalog::game(gp_tuning.races as usize);
        let tracks = match &server {
            Some(server) => catalog
                .circuits
                .iter()
                .map(|entry| server.load(entry.asset_path()))
                .collect(),
            None => Vec::new(),
        };
        commands.insert_resource(SoloUi {
            opened_at: now,
            focus: menu.row,
            focused_at: now - f64::from(anim::FOCUS_TIME),
            unfocused: None,
            hovered: None,
            catalog,
            tracks,
        });
        return;
    };
    let Some(tracks) = tracks.as_deref() else {
        return;
    };
    // Nothing new (the menu, the keyboard, a vignette's file loaded): the tree is the one shown.
    let drawn = ui.tracks.iter().filter(|h| tracks.contains(*h)).count();
    let new_files = roots
        .iter()
        .any(|(_, root)| root.shape.drawn.iter().filter(|d| **d).count() != drawn);
    let layout_changed = layout.as_ref().is_some_and(|layout| layout.is_changed());
    if !roots.is_empty() && !menu.is_changed() && !layout_changed && !new_files {
        return;
    }
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    let shape = shape_of(&menu, &ui, tracks, layout, grid.seed);
    let mut shown = false;
    for (entity, root) in &roots {
        if !shown && root.shape == shape {
            shown = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if shown {
        return;
    }
    if roots.is_empty() {
        // It opens now: it pops in (a tree built again while it is open does not).
        ui.opened_at = now;
        ui.focus = menu.row;
        ui.focused_at = now - f64::from(anim::FOCUS_TIME);
        ui.unfocused = None;
        ui.hovered = None;
    }
    let record = trial.as_deref().and_then(|state| {
        let entry = ui.catalog.circuits.get(menu.setup.circuit)?;
        let key = TrialKey {
            circuit: entry.asset_path(),
            mirror: menu.setup.mirror,
        };
        state.records.get(&key).map(|record| {
            let mut time = String::new();
            write_time(&mut time, Some(hundredths(record.time)));
            time
        })
    });
    let assets = assets.map(|assets| assets.clone()).unwrap_or_default();
    let program = shape.program.clone();
    spawn_tree(
        &mut commands,
        &Tree {
            menu: &menu,
            ui: &ui,
            tracks,
            assets: &assets,
            record,
            program,
        },
        shape,
    );
}

/// What the tree is built with.
struct Tree<'a> {
    menu: &'a RaceMenu,
    ui: &'a SoloUi,
    tracks: &'a Assets<TrackData>,
    assets: &'a MenuAssets,
    /// The time trial's record on its circuit, written.
    record: Option<String>,
    /// The circuits raced (a random championship's: drawn from the race's seed).
    program: Vec<usize>,
}

fn spawn_tree(commands: &mut Commands, tree: &Tree, shape: SoloShape) {
    let assets = tree.assets;
    let layout = shape.layout;
    commands
        .spawn((
            Name::new("Jouer solo"),
            SoloRoot { shape },
            GlobalZIndex(20),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(palette::VOILE.with_alpha(0.0)),
        ))
        .with_children(|root| {
            root.spawn((
                SoloColumn,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: u(10.0),
                    ..default()
                },
                UiTransform::from_scale(Vec2::splat(anim::POP_FROM)),
                Pickable::IGNORE,
            ))
            .with_children(|column| {
                column.spawn((
                    text(
                        solo::TITLE,
                        &assets.semibold,
                        size::PILOT_TITLE,
                        palette::NEIGE,
                    ),
                    TextShadow {
                        offset: Vec2::new(0.0, 4.0),
                        color: palette::FRAISE,
                    },
                ));
                column
                    .spawn((
                        MenuNode,
                        Node {
                            flex_direction: FlexDirection::Row,
                            width: u(size::PILOT_WIDTH),
                            padding: UiRect::all(u(size::PANEL_PADDING)),
                            column_gap: u(size::PILOT_GAP),
                            border_radius: BorderRadius::all(u(size::PANEL_RADIUS)),
                            ..default()
                        },
                        BackgroundColor(palette::GUIMAUVE),
                        BoxShadow::new(
                            palette::OMBRE,
                            Val::ZERO,
                            u(size::PANEL_SHADOW),
                            Val::ZERO,
                            Val::ZERO,
                        ),
                        Pickable::IGNORE,
                    ))
                    .with_children(|panel| {
                        spawn_left(panel, tree);
                        spawn_right(panel, tree);
                    });
                for (line, hint) in solo::hint(layout).into_iter().enumerate() {
                    column.spawn((
                        SoloHint(line),
                        text(hint, &assets.medium, size::HINT, palette::NEIGE),
                        TextLayout::justify(Justify::Center),
                        TextShadow {
                            offset: Vec2::new(0.0, 2.0),
                            color: palette::OMBRE,
                        },
                    ));
                }
            });
        });
}

/// The left column: the lines of the settings, the help, the buttons.
fn spawn_left(panel: &mut ChildSpawnerCommands, tree: &Tree) {
    let menu = tree.menu;
    let assets = tree.assets;
    panel
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                width: u(size::SOLO_LEFT),
                flex_shrink: 0.0,
                row_gap: u(size::LINE_GAP),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|left| {
            for (index, row) in menu.rows().into_iter().enumerate() {
                if solo::on_the_right(row) {
                    continue;
                }
                spawn_line(left, tree, index, row);
            }
            left.spawn((
                SoloHelp,
                text(
                    menu.setup.help(menu.row(), &tree.ui.catalog),
                    &assets.medium,
                    size::SOLO_HELP,
                    palette::MYRTILLE.with_alpha(0.75),
                ),
                Node {
                    margin: UiRect::left(u(18.0)),
                    min_height: u(size::SOLO_HELP * 1.3),
                    ..default()
                },
            ));
            left.spawn((
                MenuNode,
                Node {
                    column_gap: u(12.0),
                    margin: UiRect::top(u(4.0)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|buttons| {
                for start in [true, false] {
                    spawn_button(buttons, assets, start);
                }
            });
        });
}

/// A line's pill: its label, « < », its value (a cup's icon before it), « > ».
fn spawn_line(column: &mut ChildSpawnerCommands, tree: &Tree, index: usize, row: MenuRow) {
    let menu = tree.menu;
    let assets = tree.assets;
    let look = line_look(menu.row == index);
    let label = row.label();
    column
        .spawn((
            Name::new(label.to_string()),
            SoloLine { index, row },
            Node {
                height: u(size::LINE_HEIGHT),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: u(8.0),
                padding: UiRect {
                    left: u(18.0),
                    right: u(8.0),
                    ..default()
                },
                border: UiRect::bottom(u(size::LINE_DEPTH)),
                border_radius: BorderRadius::all(u(size::LINE_HEIGHT / 2.0)),
                ..default()
            },
            BackgroundColor(look.fill),
            BorderColor::all(look.depth),
            UiTransform::IDENTITY,
        ))
        .with_children(|line| {
            line.spawn((
                text(label, &assets.semibold, size::LINE_TEXT, palette::MYRTILLE),
                Node {
                    width: u(size::LINE_LABEL_WIDTH),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            spawn_arrow(line, assets, row, -1);
            line.spawn((
                MenuNode,
                Node {
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: u(8.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|value| {
                if row == MenuRow::Cup
                    && let Some(cup) = tree.ui.catalog.cups.get(menu.setup.cup)
                {
                    spawn_badge(value, assets.cup(cup.icon), palette::MANGUE);
                }
                value.spawn((
                    SoloValue { row },
                    text(
                        menu.setup.value(row, &tree.ui.catalog),
                        &assets.semibold,
                        size::LINE_TEXT,
                        palette::MYRTILLE,
                    ),
                    TextLayout::justify(Justify::Center),
                ));
            });
            spawn_arrow(line, assets, row, 1);
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<RaceMenuAction>| {
                actions.write(RaceMenuAction::Focus(index));
            },
        );
}

/// A round badge with a white icon.
fn spawn_badge(parent: &mut ChildSpawnerCommands, icon: Handle<Image>, color: Color) {
    parent
        .spawn((
            MenuNode,
            Node {
                width: u(size::CUP_BADGE),
                height: u(size::CUP_BADGE),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        ))
        .with_children(|badge| {
            badge.spawn((
                MenuNode,
                ImageNode::new(icon),
                Node {
                    width: u(size::CUP_ICON),
                    height: u(size::CUP_ICON),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

/// A round « < » or « > » of a line.
fn spawn_arrow(line: &mut ChildSpawnerCommands, assets: &MenuAssets, row: MenuRow, step: i32) {
    line.spawn((
        SoloArrow { row, step },
        Node {
            width: u(size::ARROW),
            height: u(size::ARROW),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(palette::LILAS),
    ))
    .with_children(|arrow| {
        arrow.spawn(text(
            if step < 0 { "<" } else { ">" },
            &assets.semibold,
            size::LINE_TEXT,
            palette::MYRTILLE,
        ));
    })
    .observe(
        move |_: On<PointerClick>, mut actions: MessageWriter<RaceMenuAction>| {
            actions.write(RaceMenuAction::Change(row, step));
        },
    );
}

fn spawn_button(buttons: &mut ChildSpawnerCommands, assets: &MenuAssets, start: bool) {
    let look = line_look(false);
    let (label, accent) = if start {
        (solo::START_LABEL, palette::MENTHE)
    } else {
        (solo::BACK_LABEL, palette::MYRTILLE)
    };
    buttons
        .spawn((
            Name::new(label),
            SoloButton { start },
            Node {
                flex_grow: 1.0,
                flex_basis: Val::ZERO,
                height: u(size::PILOT_BUTTON_HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: u(10.0),
                border: UiRect::bottom(u(size::BUTTON_DEPTH)),
                border_radius: BorderRadius::all(u(size::PILOT_BUTTON_HEIGHT / 2.0)),
                ..default()
            },
            BackgroundColor(look.fill),
            BorderColor::all(look.depth),
        ))
        .with_children(|button| {
            button.spawn((
                MenuNode,
                Node {
                    width: u(18.0),
                    height: u(18.0),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent),
                Pickable::IGNORE,
            ));
            button.spawn(text(
                label,
                &assets.semibold,
                size::LINE_TEXT + 3.0,
                look.label,
            ));
        })
        .observe(move |_: On<PointerOver>, mut ui: ResMut<SoloUi>| {
            ui.hovered = Some(start);
        })
        .observe(move |_: On<PointerOut>, mut ui: ResMut<SoloUi>| {
            if ui.hovered == Some(start) {
                ui.hovered = None;
            }
        })
        .observe(
            move |_: On<PointerClick>, mut actions: MessageWriter<RaceMenuAction>| {
                actions.write(if start {
                    RaceMenuAction::Confirm
                } else {
                    RaceMenuAction::Close
                });
            },
        );
}

/// The right column: the header of the races, then the races (a line each when they are chosen
/// one by one), or the one circuit, big.
fn spawn_right(panel: &mut ChildSpawnerCommands, tree: &Tree) {
    let menu = tree.menu;
    let assets = tree.assets;
    let catalog = &tree.ui.catalog;
    let rows = menu.rows();
    panel
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: u(size::LINE_GAP),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|right| {
            right.spawn((
                SoloHeader,
                text(
                    solo::header(&menu.setup, catalog),
                    &assets.semibold,
                    size::SOLO_HEADER,
                    palette::MYRTILLE,
                ),
                Node {
                    margin: UiRect::left(u(6.0)),
                    ..default()
                },
            ));
            if menu.setup.mode.picks_a_circuit() {
                let circuit = menu.setup.circuit;
                right
                    .spawn((
                        MenuNode,
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: u(10.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|big| {
                        spawn_vignette(big, tree, circuit, true);
                        if menu.setup.mode == GameMode::TimeTrial {
                            big.spawn(text(
                                solo::record_line(tree.record.as_deref()),
                                &assets.medium,
                                size::SOLO_HELP + 2.0,
                                palette::MYRTILLE,
                            ));
                        }
                    });
                return;
            }
            for (race, circuit) in tree.program.iter().copied().enumerate() {
                let index = rows.iter().position(|row| *row == MenuRow::Race(race));
                spawn_card(right, tree, race, circuit, index);
            }
        });
}

/// A race on the right: its number, its vignette, its circuit's name; a line (with its arrows,
/// `index`) when its circuit is chosen.
fn spawn_card(
    right: &mut ChildSpawnerCommands,
    tree: &Tree,
    race: usize,
    circuit: usize,
    index: Option<usize>,
) {
    let assets = tree.assets;
    let menu = tree.menu;
    let look = line_look(index == Some(menu.row));
    let mut card = right.spawn((
        Name::new(format!("Course {}", race + 1)),
        RaceCard { race, circuit },
        Node {
            height: u(size::CARD_HEIGHT),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: u(8.0),
            padding: UiRect {
                left: u(8.0),
                right: u(8.0),
                ..default()
            },
            border: UiRect::bottom(u(size::LINE_DEPTH)),
            border_radius: BorderRadius::all(u(size::CARD_HEIGHT / 2.0)),
            ..default()
        },
        BackgroundColor(look.fill),
        BorderColor::all(look.depth),
        UiTransform::IDENTITY,
    ));
    card.with_children(|card| {
        card.spawn((
            MenuNode,
            Node {
                width: u(size::CARD_BADGE),
                height: u(size::CARD_BADGE),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(palette::LAGON),
            Pickable::IGNORE,
        ))
        .with_children(|badge| {
            badge.spawn(text(
                (race + 1).to_string(),
                &assets.semibold,
                size::CARD_TEXT - 2.0,
                palette::NEIGE,
            ));
        });
        spawn_vignette(card, tree, circuit, false);
        if index.is_some() {
            let row = MenuRow::Race(race);
            spawn_arrow(card, assets, row, -1);
            card.spawn((
                SoloValue { row },
                text(
                    tree.ui.catalog.name(circuit),
                    &assets.semibold,
                    size::CARD_TEXT,
                    palette::MYRTILLE,
                ),
                TextLayout::justify(Justify::Center),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
            spawn_arrow(card, assets, row, 1);
        } else {
            card.spawn((
                text(
                    tree.ui.catalog.name(circuit),
                    &assets.semibold,
                    size::CARD_TEXT,
                    palette::MYRTILLE,
                ),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        }
    });
    if let Some(index) = index {
        card.insert(SoloLine {
            index,
            row: MenuRow::Race(race),
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<RaceMenuAction>| {
                actions.write(RaceMenuAction::Focus(index));
            },
        );
    }
}

/// The vignette of `circuit` (big: a race alone's, a time trial's): the sky and the ground of its
/// biome, and its road seen from above (dots of the asphalt's color, the start line's white),
/// mirrored in the mirror mode, once its file is loaded.
fn spawn_vignette(parent: &mut ChildSpawnerCommands, tree: &Tree, circuit: usize, big: bool) {
    let colors = palette_of(tree.ui, tree.tracks, circuit);
    let (width, height, radius, dot, count) = if big {
        (
            size::BIG_VIGNETTE_WIDTH,
            size::BIG_VIGNETTE_HEIGHT,
            size::BIG_VIGNETTE_RADIUS,
            size::BIG_VIGNETTE_DOT,
            150,
        )
    } else {
        (
            size::VIGNETTE_WIDTH,
            size::VIGNETTE_HEIGHT,
            size::VIGNETTE_RADIUS,
            size::VIGNETTE_DOT,
            48,
        )
    };
    // The road in the middle 84 % × 76 % of the vignette.
    let (inner_w, inner_h) = (0.84, 0.76);
    let outline = tree
        .ui
        .tracks
        .get(circuit)
        .and_then(|handle| tree.tracks.get(handle))
        .map(|data| {
            solo::outline(
                data,
                tree.menu.setup.mirror,
                (width * inner_w) / (height * inner_h),
                count,
            )
        })
        .unwrap_or_default();
    let ground = rgb(colors.ground);
    parent
        .spawn((
            Vignette { circuit },
            Node {
                width: u(width),
                height: u(height),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(u(radius)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(rgb(colors.sky), Val::ZERO),
                ColorStop::new(ground, percent(28)),
                ColorStop::new(rgb(shade(colors.ground)), percent(100)),
            ])),
            Pickable::IGNORE,
        ))
        .with_children(|vignette| {
            vignette
                .spawn((
                    MenuNode,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(100.0 * (1.0 - inner_w) / 2.0),
                        top: percent(100.0 * (1.0 - inner_h) / 2.0 + 4.0),
                        width: percent(100.0 * inner_w),
                        height: percent(100.0 * inner_h),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|road| {
                    let asphalt = rgb(colors.asphalt);
                    for (k, point) in outline.iter().enumerate() {
                        let (size, color) = if k == 0 {
                            (dot * 1.6, palette::NEIGE)
                        } else {
                            (dot, asphalt)
                        };
                        road.spawn((
                            MenuNode,
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(100.0 * point.x),
                                top: percent(100.0 * point.y),
                                width: u(size),
                                height: u(size),
                                margin: UiRect {
                                    left: u(-size / 2.0),
                                    top: u(-size / 2.0),
                                    ..default()
                                },
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(color),
                            Pickable::IGNORE,
                        ));
                    }
                });
        });
}

/// What the screen shows of the choice when only values change (the tree's shape is the same):
/// the lines' values, the help of the line with the focus, the header.
#[allow(clippy::type_complexity)]
pub(super) fn refresh_solo_tree(
    menu: Res<RaceMenu>,
    ui: Res<SoloUi>,
    roots: Query<(), Added<SoloRoot>>,
    mut texts: ParamSet<(
        Query<(&SoloValue, &mut Text)>,
        Query<&mut Text, With<SoloHelp>>,
        Query<&mut Text, With<SoloHeader>>,
    )>,
) {
    if !menu.is_changed() && roots.is_empty() {
        return;
    }
    let catalog = &ui.catalog;
    for (value, mut text) in &mut texts.p0() {
        let wanted = menu.setup.value(value.row, catalog);
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let help = menu.setup.help(menu.row(), catalog);
    for mut text in &mut texts.p1() {
        if text.0 != help {
            text.0.clone_from(&help);
        }
    }
    let header = solo::header(&menu.setup, catalog);
    for mut text in &mut texts.p2() {
        if text.0 != header {
            text.0.clone_from(&header);
        }
    }
}

/// The screen open: the veil fades in, the column pops in, the line with the focus takes its look
/// and grows a little (the charter's focus), the button under the pointer too. Only what changed
/// is written.
#[allow(clippy::type_complexity)]
pub(super) fn animate_solo_tree(
    menu: Res<RaceMenu>,
    mut ui: ResMut<SoloUi>,
    time: Res<Time<Real>>,
    mut veils: Query<&mut BackgroundColor, With<SoloRoot>>,
    mut columns: Query<&mut UiTransform, (With<SoloColumn>, Without<SoloLine>)>,
    mut lines: Query<
        (
            &SoloLine,
            &mut UiTransform,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (Without<SoloRoot>, Without<SoloColumn>, Without<SoloButton>),
    >,
    mut buttons: Query<
        (&SoloButton, &mut BackgroundColor, &mut BorderColor),
        (Without<SoloRoot>, Without<SoloLine>),
    >,
) {
    let now = now(&time);
    if ui.focus != menu.row {
        ui.unfocused = Some(ui.focus);
        ui.focus = menu.row;
        ui.focused_at = now;
    }
    let opened = (now - ui.opened_at) as f32;
    for mut veil in &mut veils {
        veil.set_if_neq(BackgroundColor(
            palette::VOILE.with_alpha(palette::VOILE.alpha() * anim::veil(opened)),
        ));
    }
    for mut transform in &mut columns {
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(anim::panel_pop(
            opened,
        ))));
    }
    let since = (now - ui.focused_at) as f32;
    for (line, mut transform, mut fill, mut depth) in &mut lines {
        let focused = line.index == menu.row;
        let look = line_look(focused);
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
        let scale = if focused {
            anim::focus_scale(true, since)
        } else if ui.unfocused == Some(line.index) {
            anim::focus_scale(false, since)
        } else {
            1.0
        };
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(scale)));
    }
    for (button, mut fill, mut depth) in &mut buttons {
        let look = line_look(ui.hovered == Some(button.start));
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
    }
}

/// The gamepad on the screen: the cross or the left stick (a step per push) go through its lines
/// and change them, A starts, B or Start go back. Not while the options menu is over it.
pub(super) fn solo_gamepad(
    gamepads: Query<&Gamepad>,
    menu: Res<RaceMenu>,
    options: Option<Res<OptionsMenu>>,
    mut latches: Local<[StickLatch; 2]>,
    mut actions: MessageWriter<RaceMenuAction>,
) {
    let (y, x) = gamepads.iter().fold((0.0f32, 0.0f32), |(y, x), gamepad| {
        let stick = |axis| gamepad.get(axis).unwrap_or(0.0);
        let (sy, sx) = (
            stick(GamepadAxis::LeftStickY),
            stick(GamepadAxis::LeftStickX),
        );
        (
            if sy.abs() > y.abs() { sy } else { y },
            if sx.abs() > x.abs() { sx } else { x },
        )
    });
    // The latches follow the stick even while nothing listens: no step when it comes back.
    let (vertical, horizontal) = (latches[0].step(y), -latches[1].step(x));
    if !menu.open || options.is_some_and(|options| options.open) {
        return;
    }
    let pressed = |button: GamepadButton| gamepads.iter().any(|g| g.just_pressed(button));
    let row = || menu.row();
    let action = if pressed(GamepadButton::South) {
        RaceMenuAction::Confirm
    } else if pressed(GamepadButton::East) || pressed(GamepadButton::Start) {
        RaceMenuAction::Close
    } else if pressed(GamepadButton::DPadUp) || vertical < 0 {
        RaceMenuAction::Row(-1)
    } else if pressed(GamepadButton::DPadDown) || vertical > 0 {
        RaceMenuAction::Row(1)
    } else if pressed(GamepadButton::DPadLeft) || horizontal < 0 {
        RaceMenuAction::Change(row(), -1)
    } else if pressed(GamepadButton::DPadRight) || horizontal > 0 {
        RaceMenuAction::Change(row(), 1)
    } else {
        return;
    };
    actions.write(action);
}

/// The screen is open (its systems run).
pub(super) fn solo_open(menu: Res<RaceMenu>) -> bool {
    menu.open
}

/// The screen's tree must be built, rebuilt or removed.
pub(super) fn solo_open_or_shown(menu: Res<RaceMenu>, roots: Query<(), With<SoloRoot>>) -> bool {
    menu.open || !roots.is_empty()
}
