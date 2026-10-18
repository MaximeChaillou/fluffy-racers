//! The « Pilote » screen (issue #98) drawn with `bevy_ui`, after the charter (`style.rs`,
//! `anim.rs`): over a veil, its title and a wide cream panel (issue #136: no line of keys under
//! it any more; while one types the pseudo, the help of its field: « 12 lettres au plus »). On the
//! left, the 3D preview of the kart (`preview.rs`), its seven stat bars `(level + 3) / 4` and
//! the Speed and the Handling on each terrain, « Valider » and « Retour »; on the right, its
//! lines, from the top: the pseudo (a text field), the character (a grid of every animal, a
//! portrait and a name each, one line per weight class), the kart, the wheels, the wing, the
//! color. The line with the focus is the charter's focused button (mango, a little bigger).
//!
//! Its state and its keys are `characters::SelectionScreen`'s; here: its tree, the gamepad, the
//! mouse (the observers of its lines, arrows, portraits, swatches and buttons) and the pseudo's
//! field.
//!
//! **The pseudo's field is Bevy 0.20's `EditableText`** (`bevy_ui_widgets::TextInput`), not an
//! egui field: a node of the tree like the others (the charter's font, colors and corners; it
//! pops in with the panel), no egui context for the screen, and Bevy's caret, selection, copy and
//! paste and input methods; 12 characters (`EditableText::max_characters`), those the HUD writes
//! (`EditableTextFilter`: `net::pseudo::allowed`). It has the keyboard while Bevy's focus
//! (`InputFocus`) is on it: Entrée on its line or a click in it gives it, Entrée, Échap or a click
//! elsewhere takes it back. Meanwhile the game sees no key (`admin::KeyboardCapture`): typing
//! neither drives the kart nor triggers a shortcut. Each change is the pseudo at once
//! (`NetSettings::pseudo`: the `Hello` online), kept in the browser.
//!
//! **Closed, nothing is left**: the tree, its observers and the preview go when it closes; its
//! systems only run while it is open (the portraits' sheet, rendered once, stays).

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::picking::pointer::PointerMap;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontSource, Justify, LineHeight};
use bevy::ui::widget::ViewportNode;
use bevy::ui_widgets::TextInput;

use super::anim;
use super::nav::StickLatch;
use super::pilot;
use super::preview::{self, PilotPreview, Portraits};
use super::screen::{MenuAssets, MenuNode};
use super::style::{self, ButtonState, font, palette, size, u};
use crate::admin::{KeyboardCapture, storage};
use crate::characters::{
    PartKind, PlayerLoadout, Roster, SelectionAction, SelectionInput, SelectionScreen, WeightClass,
    selection_action,
};
use crate::kart::{KartColor, Stat, Terrain, display_level};
use crate::modes::RaceMenu;
use crate::net::pseudo::{self, PSEUDO_MAX_CHARS, PSEUDO_STORAGE_KEY};
use crate::net::{NetSettings, character_name};
use crate::options::OptionsMenu;

/// The root of the screen's tree (the veil).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(MenuNode)]
pub struct PilotRoot;

/// The column of the title, the panel and the help: it pops in.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct PilotColumn;

/// A line of choices (`SelectionScreen::row`): its pill takes the focus's look.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct PilotLine {
    pub row: usize,
}

/// The value written on a line (a part's name, the color's, the character's).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct LineValue {
    pub row: usize,
}

/// « < » or « > » on a line.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct PilotArrow {
    pub row: usize,
    pub step: i32,
}

/// A character's cell of the grid: its portrait and its name.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct PortraitCell {
    pub character: usize,
}

/// A color's swatch.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct ColorSwatch {
    pub color: KartColor,
}

/// The filled part of a stat's bar.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct StatBar {
    pub stat: Stat,
}

/// A stat's value, `3,25`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct StatValue {
    pub stat: Stat,
}

/// The Speed (`speed`) or the Handling on a terrain.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct TerrainValue {
    pub terrain: Terrain,
    pub speed: bool,
}

/// The pseudo's text field.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct PseudoField;

/// What the empty field shows: the character's name (the pseudo then).
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct PseudoHint;

/// Issue #136: the help under the panel while one types the pseudo (« 12 lettres au plus »; no
/// keys), empty otherwise. It was the lines of keys.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[require(MenuNode)]
pub struct PilotHint;

/// « Valider » (`confirm`) or « Retour ».
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct PilotButton {
    pub confirm: bool,
}

/// The frame of the preview (the image of its camera).
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct PreviewFrame;

/// The pseudo is kept in the browser: in the game only (`PilotPlugin::storage`), never in the
/// tests.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PilotStorage(pub bool);

/// What the screen's animations and its field remember (real times, s). There once the screen
/// first opened.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct PilotUi {
    pub opened_at: f64,
    /// The line with the focus, since when, and the one that lost it.
    pub focus: usize,
    pub focused_at: f64,
    pub unfocused: Option<usize>,
    /// The button under the pointer.
    pub hovered: Option<bool>,
    /// The field, and whether it had the keyboard at the end of the last frame.
    pub field: Option<Entity>,
    pub was_typing: bool,
    /// The field's text as typed (its ends' spaces included).
    pub typed: String,
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
    style::button_look(state, palette::CAROTTE)
}

/// The color of a stat's bar.
fn stat_color(stat: Stat) -> Color {
    match stat {
        Stat::Speed => palette::FRAISE,
        Stat::Acceleration => palette::CAROTTE,
        Stat::Weight => palette::PRUNE,
        Stat::Handling => palette::LAGON,
        Stat::Traction => palette::MENTHE,
        Stat::MiniTurbo => palette::MANGUE,
        Stat::Invincibility => palette::LILAS,
    }
}

/// The value of a line, as written on it.
fn line_value(roster: &Roster, screen: &SelectionScreen, row: usize) -> String {
    match row {
        0 => {
            let character = roster.character(&screen.choice);
            format!("{} · {}", character.name, character.class.label())
        }
        SelectionScreen::COLOR_ROW => screen.color.title(),
        _ => match PartKind::ALL.get(row) {
            Some(kind) => roster.name(*kind, screen.choice.get(*kind)).to_string(),
            None => String::new(),
        },
    }
}

/// The screen open: its tree (and its preview, and the portraits' sheet the first time), for the
/// keyboard of the player. Closed: none of it. Not before the game has a camera (at launch, the
/// circuit is still loading): the sizes are shares of its view.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_pilot_tree(
    mut commands: Commands,
    screen: Res<SelectionScreen>,
    ui: Option<ResMut<PilotUi>>,
    time: Res<Time<Real>>,
    roots: Query<Entity, With<PilotRoot>>,
    (assets, roster, settings, loadout): (
        Option<Res<MenuAssets>>,
        Res<Roster>,
        Option<Res<NetSettings>>,
        Option<Res<PlayerLoadout>>,
    ),
    (preview, portraits): (Option<Res<PilotPreview>>, Option<Res<Portraits>>),
    mut images: Option<ResMut<Assets<Image>>>,
    cameras: Query<(), With<crate::camera::ChaseCamera>>,
    pointers: Option<Res<PointerMap>>,
) {
    if !screen.open {
        for entity in &roots {
            commands.entity(entity).despawn();
        }
        if let Some(preview) = preview.as_deref() {
            preview::despawn_preview(&mut commands, preview, images.as_deref_mut());
            commands.remove_resource::<PilotPreview>();
        }
        return;
    }
    if cameras.is_empty() {
        return;
    }
    if !roots.is_empty() {
        return;
    }
    let now = now(&time);
    let (camera, image) = match preview.as_deref() {
        Some(preview) => (preview.camera, preview.image.clone()),
        None => {
            let spawned =
                preview::spawn_preview(&mut commands, images.as_deref_mut(), screen.color, now);
            let shown = (spawned.camera, spawned.image.clone());
            commands.insert_resource(spawned);
            shown
        }
    };
    let sheet = match portraits.as_deref() {
        Some(portraits) => Some(portraits.image.clone()),
        None => preview::new_portraits(images.as_deref_mut()).map(|portraits| {
            let image = portraits.image.clone();
            commands.insert_resource(portraits);
            image
        }),
    };
    let typed = settings
        .as_deref()
        .and_then(|settings| settings.pseudo.clone())
        .unwrap_or_default();
    let hint_name = character_name(loadout.as_deref().map(|l| &l.0), Some(&roster));
    let assets = assets.map(|assets| assets.clone()).unwrap_or_default();
    let field = spawn_tree(
        &mut commands,
        Tree {
            assets: &assets,
            roster: &roster,
            screen: &screen,
            typed: &typed,
            hint_name: &hint_name,
            sheet,
            camera,
            image,
            // A viewport has a pointer of its own: picking's (the game's).
            viewport: pointers.is_some(),
        },
    );
    let fresh = PilotUi {
        opened_at: now,
        focus: screen.row,
        focused_at: now - f64::from(anim::FOCUS_TIME),
        unfocused: None,
        hovered: None,
        field: Some(field),
        was_typing: false,
        typed,
    };
    match ui {
        Some(mut ui) => *ui = fresh,
        None => commands.insert_resource(fresh),
    }
}

/// What the tree is built with.
struct Tree<'a> {
    assets: &'a MenuAssets,
    roster: &'a Roster,
    screen: &'a SelectionScreen,
    typed: &'a str,
    hint_name: &'a str,
    sheet: Option<Handle<Image>>,
    camera: Entity,
    /// The image the preview's camera renders into...
    image: Handle<Image>,
    /// ...shown by a `ViewportNode` (it follows the frame's size), or headless (no picking: a
    /// viewport needs it) by an image.
    viewport: bool,
}

/// Spawns the tree; returns the pseudo's field.
fn spawn_tree(commands: &mut Commands, tree: Tree) -> Entity {
    let mut field = Entity::PLACEHOLDER;
    let assets = tree.assets;
    commands
        .spawn((
            Name::new("Pilote"),
            PilotRoot,
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
                PilotColumn,
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
                        pilot::TITLE,
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
                        spawn_left(panel, &tree);
                        field = spawn_right(panel, &tree);
                    });
                // Issue #136: no line of keys; while one types the pseudo, its field's help.
                column.spawn((
                    PilotHint,
                    text(
                        pilot::typing_help(tree.screen),
                        &assets.medium,
                        size::HINT,
                        palette::NEIGE,
                    ),
                    TextLayout::justify(Justify::Center),
                    TextShadow {
                        offset: Vec2::new(0.0, 2.0),
                        color: palette::OMBRE,
                    },
                ));
            });
        });
    field
}

/// The left column: the preview, the stats, the buttons.
fn spawn_left(panel: &mut ChildSpawnerCommands, tree: &Tree) {
    let assets = tree.assets;
    let stats = tree.roster.stats(&tree.screen.choice);
    panel
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                width: u(size::PILOT_LEFT),
                row_gap: u(10.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|left| {
            left.spawn((
                PreviewFrame,
                Node {
                    width: u(pilot::PREVIEW_SIZE.x),
                    height: u(pilot::PREVIEW_SIZE.y),
                    border_radius: BorderRadius::all(u(size::PREVIEW_RADIUS)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::new(palette::COTON, Val::ZERO),
                    ColorStop::new(palette::LILAS, percent(100)),
                ])),
                Pickable::IGNORE,
            ))
            .with_children(|frame| {
                let mut view = frame.spawn((
                    MenuNode,
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                if tree.viewport {
                    view.insert(ViewportNode::new(tree.camera));
                } else {
                    view.insert(ImageNode::new(tree.image.clone()));
                }
            });
            // The seven stats.
            left.spawn((
                MenuNode,
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|block| {
                for stat in Stat::ALL {
                    let value = stats.display(stat);
                    block
                        .spawn((
                            MenuNode,
                            Node {
                                height: u(size::STAT_LINE),
                                align_items: AlignItems::Center,
                                column_gap: u(8.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|line| {
                            line.spawn((
                                text(
                                    stat.label(),
                                    &assets.medium,
                                    size::STAT_TEXT,
                                    palette::MYRTILLE,
                                ),
                                Node {
                                    width: u(size::STAT_LABEL_WIDTH),
                                    ..default()
                                },
                            ));
                            line.spawn((
                                MenuNode,
                                Node {
                                    flex_grow: 1.0,
                                    height: u(size::STAT_BAR),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(palette::BRUME),
                                Pickable::IGNORE,
                            ))
                            .with_children(|track| {
                                track.spawn((
                                    StatBar { stat },
                                    Node {
                                        width: percent(100.0 * pilot::bar_share(value)),
                                        height: percent(100),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(stat_color(stat)),
                                    Pickable::IGNORE,
                                ));
                            });
                            line.spawn((
                                StatValue { stat },
                                text(
                                    pilot::french(value),
                                    &assets.semibold,
                                    size::STAT_TEXT,
                                    palette::MYRTILLE,
                                ),
                                TextLayout::justify(Justify::Right),
                                Node {
                                    width: u(size::STAT_VALUE_WIDTH),
                                    ..default()
                                },
                            ));
                        });
                }
            });
            // The Speed and the Handling on each terrain.
            left.spawn((
                MenuNode,
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|table| {
                let row = |table: &mut ChildSpawnerCommands, label: &str, speed: Option<bool>| {
                    table
                        .spawn((
                            MenuNode,
                            Node {
                                height: u(size::STAT_LINE - 2.0),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|line| {
                            line.spawn((
                                text(
                                    label,
                                    &assets.medium,
                                    size::STAT_TEXT - 1.0,
                                    palette::MYRTILLE,
                                ),
                                Node {
                                    width: u(size::TERRAIN_LABEL_WIDTH),
                                    ..default()
                                },
                            ));
                            for terrain in Terrain::ALL {
                                let (content, color) = match speed {
                                    None => (terrain.label().to_string(), palette::CENDRE),
                                    Some(speed) => {
                                        let level = if speed {
                                            stats.speed_on(terrain)
                                        } else {
                                            stats.handling_on(terrain)
                                        };
                                        (pilot::french(display_level(level)), palette::MYRTILLE)
                                    }
                                };
                                let mut cell = line.spawn((
                                    text(content, &assets.medium, size::STAT_TEXT - 1.0, color),
                                    TextLayout::justify(Justify::Center),
                                    Node {
                                        flex_grow: 1.0,
                                        flex_basis: Val::ZERO,
                                        min_width: match terrain {
                                            Terrain::AntiGravity => u(size::TERRAIN_WIDE),
                                            _ => Val::ZERO,
                                        },
                                        ..default()
                                    },
                                ));
                                if let Some(speed) = speed {
                                    cell.insert(TerrainValue { terrain, speed });
                                }
                            }
                        });
                };
                row(table, "Par terrain", None);
                row(table, Stat::Speed.label(), Some(true));
                row(table, Stat::Handling.label(), Some(false));
            });
            // « Valider », « Retour ».
            left.spawn((
                MenuNode,
                Node {
                    column_gap: u(12.0),
                    margin: UiRect::top(u(2.0)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|buttons| {
                for confirm in [true, false] {
                    spawn_button(buttons, assets, confirm);
                }
            });
        });
}

fn spawn_button(buttons: &mut ChildSpawnerCommands, assets: &MenuAssets, confirm: bool) {
    let look = line_look(false);
    let (label, accent) = if confirm {
        (pilot::CONFIRM_LABEL, palette::MENTHE)
    } else {
        (pilot::BACK_LABEL, palette::MYRTILLE)
    };
    buttons
        .spawn((
            Name::new(label),
            PilotButton { confirm },
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
            UiTransform::IDENTITY,
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
        .observe(move |_: On<PointerOver>, mut ui: ResMut<PilotUi>| {
            ui.hovered = Some(confirm);
        })
        .observe(move |_: On<PointerOut>, mut ui: ResMut<PilotUi>| {
            if ui.hovered == Some(confirm) {
                ui.hovered = None;
            }
        })
        .observe(
            move |_: On<PointerClick>, mut actions: MessageWriter<SelectionAction>| {
                actions.write(if confirm {
                    SelectionAction::Confirm
                } else {
                    SelectionAction::Cancel
                });
            },
        );
}

/// A line's pill: its label, then what `content` puts in it.
fn spawn_line(
    right: &mut ChildSpawnerCommands,
    assets: &MenuAssets,
    (row, value): (usize, Option<String>),
    focused: bool,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    let look = line_look(focused);
    right
        .spawn((
            Name::new(pilot::row_label(row)),
            PilotLine { row },
            Node {
                height: u(size::LINE_HEIGHT),
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
            let label = text(
                pilot::row_label(row),
                &assets.semibold,
                size::LINE_TEXT,
                palette::MYRTILLE,
            );
            let column = Node {
                width: u(size::LINE_LABEL_WIDTH),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                ..default()
            };
            if row == SelectionScreen::COLOR_ROW {
                // The color's name under its label: the swatches take the rest of the line.
                line.spawn((MenuNode, column, Pickable::IGNORE))
                    .with_children(|label_column| {
                        label_column.spawn((label, LineHeight::RelativeToFont(1.0)));
                        label_column.spawn((
                            LineValue { row },
                            text(
                                value.unwrap_or_default(),
                                &assets.medium,
                                size::PORTRAIT_NAME,
                                palette::MYRTILLE,
                            ),
                            LineHeight::RelativeToFont(1.0),
                        ));
                    });
            } else {
                line.spawn((label, column));
            }
            content(line);
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<SelectionAction>| {
                actions.write(SelectionAction::Focus(row));
            },
        );
}

/// A round « < » or « > » of a line.
fn spawn_arrow(line: &mut ChildSpawnerCommands, assets: &MenuAssets, row: usize, step: i32) {
    line.spawn((
        PilotArrow { row, step },
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
        move |_: On<PointerClick>, mut actions: MessageWriter<SelectionAction>| {
            actions.write(match PartKind::ALL.get(row) {
                Some(kind) => SelectionAction::Change(*kind, step),
                None => SelectionAction::Color(step),
            });
        },
    );
}

/// The right column: the lines, from the top. Returns the pseudo's field.
fn spawn_right(panel: &mut ChildSpawnerCommands, tree: &Tree) -> Entity {
    let assets = tree.assets;
    let screen = tree.screen;
    let mut field = Entity::PLACEHOLDER;
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
            for row in SelectionScreen::SHOWN {
                let focused = screen.row == row;
                match row {
                    SelectionScreen::PSEUDO_ROW => {
                        spawn_line(right, assets, (row, None), focused, |line| {
                            field = spawn_field(line, assets, tree.typed, tree.hint_name);
                        });
                    }
                    0 => spawn_grid(right, tree, focused),
                    SelectionScreen::COLOR_ROW => {
                        let value = Some(line_value(tree.roster, screen, row));
                        spawn_line(right, assets, (row, value), focused, |line| {
                            spawn_swatches(line, screen);
                        });
                    }
                    _ => {
                        let value = line_value(tree.roster, screen, row);
                        spawn_line(right, assets, (row, None), focused, |line| {
                            spawn_arrow(line, assets, row, -1);
                            line.spawn((
                                LineValue { row },
                                text(value, &assets.semibold, size::LINE_TEXT, palette::MYRTILLE),
                                TextLayout::justify(Justify::Center),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            spawn_arrow(line, assets, row, 1);
                        });
                    }
                }
            }
        });
    field
}

/// The pseudo's field (see the module's documentation) and its hint; returns the field.
fn spawn_field(
    line: &mut ChildSpawnerCommands,
    assets: &MenuAssets,
    typed: &str,
    hint_name: &str,
) -> Entity {
    let mut field = Entity::PLACEHOLDER;
    line.spawn((
        MenuNode,
        Node {
            flex_grow: 1.0,
            height: u(size::LINE_HEIGHT - 14.0),
            padding: UiRect::horizontal(u(12.0)),
            align_items: AlignItems::Center,
            border: UiRect::all(u(2.0)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(palette::COTON),
        BorderColor::all(palette::LILAS),
        Pickable::IGNORE,
    ))
    .with_children(|well| {
        well.spawn((
            PseudoHint,
            text(
                hint_name,
                &assets.semibold,
                size::LINE_TEXT,
                palette::CENDRE,
            ),
            Node {
                position_type: PositionType::Absolute,
                left: u(14.0),
                display: if typed.is_empty() {
                    Display::Flex
                } else {
                    Display::None
                },
                ..default()
            },
        ));
        field = well
            .spawn((
                PseudoField,
                TextInput,
                EditableText {
                    max_characters: Some(PSEUDO_MAX_CHARS),
                    ..EditableText::new(typed)
                },
                EditableTextFilter::new(pseudo::allowed),
                TextFont {
                    font: FontSource::from(&assets.semibold),
                    font_size: font(size::LINE_TEXT),
                    ..default()
                },
                TextColor(palette::MYRTILLE),
                TabIndex(0),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            ))
            .id();
    });
    field
}

/// The character grid: a header (the character chosen), then one line per weight class.
fn spawn_grid(right: &mut ChildSpawnerCommands, tree: &Tree, focused: bool) {
    let assets = tree.assets;
    let look = line_look(focused);
    let grid = pilot::character_grid(tree.roster);
    right
        .spawn((
            Name::new("Personnage"),
            PilotLine { row: 0 },
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect {
                    left: u(18.0),
                    right: u(12.0),
                    top: u(6.0),
                    bottom: u(6.0),
                },
                row_gap: u(size::GRID_GAP),
                border: UiRect::bottom(u(size::LINE_DEPTH)),
                border_radius: BorderRadius::all(u(24.0)),
                ..default()
            },
            BackgroundColor(look.fill),
            BorderColor::all(look.depth),
        ))
        .with_children(|block| {
            block
                .spawn((
                    MenuNode,
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: u(12.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|header| {
                    // « Personnage » is wider than the other labels: its least width only.
                    header.spawn((
                        text(
                            pilot::row_label(0),
                            &assets.semibold,
                            size::LINE_TEXT,
                            palette::MYRTILLE,
                        ),
                        Node {
                            min_width: u(size::LINE_LABEL_WIDTH),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    header.spawn((
                        LineValue { row: 0 },
                        text(
                            line_value(tree.roster, tree.screen, 0),
                            &assets.semibold,
                            size::LINE_TEXT,
                            palette::MYRTILLE,
                        ),
                    ));
                });
            for (line, characters) in grid.iter().enumerate() {
                block
                    .spawn((
                        MenuNode,
                        Node {
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|cells| {
                        cells.spawn((
                            text(
                                pilot::class_label(WeightClass::ALL[line]),
                                &assets.medium,
                                size::STAT_TEXT,
                                palette::MYRTILLE.with_alpha(0.75),
                            ),
                            Node {
                                width: u(size::CLASS_WIDTH),
                                ..default()
                            },
                        ));
                        for (column, index) in characters.iter().copied().enumerate() {
                            spawn_cell(cells, tree, index, (line, column));
                        }
                    });
            }
        })
        .observe(
            |_: On<PointerOver>, mut actions: MessageWriter<SelectionAction>| {
                actions.write(SelectionAction::Focus(0));
            },
        );
}

fn spawn_cell(cells: &mut ChildSpawnerCommands, tree: &Tree, index: usize, at: (usize, usize)) {
    let assets = tree.assets;
    let character = &tree.roster.characters[index];
    let chosen = tree.screen.choice.character == index;
    cells
        .spawn((
            Name::new(character.name.clone()),
            PortraitCell { character: index },
            Node {
                width: u(size::PORTRAIT_CELL),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(u(14.0)),
                ..default()
            },
            BackgroundColor(cell_fill(chosen)),
        ))
        .with_children(|cell| {
            let portrait = Node {
                width: u(size::PORTRAIT),
                height: u(size::PORTRAIT),
                ..default()
            };
            match &tree.sheet {
                Some(sheet) => {
                    let mut image = ImageNode::new(sheet.clone());
                    image.rect = Some(pilot::portrait_rect(at.0, at.1));
                    cell.spawn((MenuNode, image, portrait, Pickable::IGNORE));
                }
                None => {
                    // No image (headless): a ball of its color.
                    let (r, g, b) = character.color;
                    cell.spawn((
                        MenuNode,
                        Node {
                            border_radius: BorderRadius::MAX,
                            ..portrait
                        },
                        BackgroundColor(Color::srgb(r, g, b)),
                        Pickable::IGNORE,
                    ));
                }
            }
            cell.spawn(text(
                character.name.clone(),
                &assets.semibold,
                size::PORTRAIT_NAME,
                palette::MYRTILLE,
            ));
        })
        .observe(
            move |_: On<PointerClick>, mut actions: MessageWriter<SelectionAction>| {
                actions.write(SelectionAction::Pick(PartKind::Character, index));
            },
        );
}

/// The background of a character's cell: the chosen one stands out.
fn cell_fill(chosen: bool) -> Color {
    if chosen {
        palette::COTON
    } else {
        palette::COTON.with_alpha(0.0)
    }
}

/// The color's line: the palette's swatches (its name is under its label).
fn spawn_swatches(line: &mut ChildSpawnerCommands, screen: &SelectionScreen) {
    line.spawn((
        MenuNode,
        Node {
            column_gap: u(size::SWATCH_GAP),
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|swatches| {
        for color in KartColor::ALL {
            swatches
                .spawn((
                    Name::new(color.title()),
                    ColorSwatch { color },
                    Node {
                        width: u(size::SWATCH),
                        height: u(size::SWATCH),
                        border: UiRect::all(u(3.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(color.color()),
                    BorderColor::all(swatch_ring(color == screen.color)),
                ))
                .observe(
                    move |_: On<PointerClick>, mut actions: MessageWriter<SelectionAction>| {
                        actions.write(SelectionAction::PickColor(color));
                    },
                );
        }
    });
}

/// The ring of a swatch: the ink around the chosen color, a light one around the others.
fn swatch_ring(chosen: bool) -> Color {
    if chosen {
        palette::MYRTILLE
    } else {
        palette::COTON
    }
}

/// The gamepad on the screen: the cross or the left stick (a step per push) go through it and
/// change the choice, A keeps it (on the pseudo's line: types), B or Start leave. Not while the
/// field has the keyboard, nor while the options menu or the race menu is over it.
pub(super) fn pilot_gamepad(
    gamepads: Query<&Gamepad>,
    screen: Res<SelectionScreen>,
    (options, race_menu): (Option<Res<OptionsMenu>>, Option<Res<RaceMenu>>),
    mut latches: Local<[StickLatch; 2]>,
    mut actions: MessageWriter<SelectionAction>,
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
    let elsewhere =
        options.is_some_and(|menu| menu.open) || race_menu.is_some_and(|menu| menu.open);
    if !screen.open || screen.typing || elsewhere {
        return;
    }
    let pressed = |button: GamepadButton| gamepads.iter().any(|g| g.just_pressed(button));
    let input = if pressed(GamepadButton::South) {
        SelectionInput::Confirm
    } else if pressed(GamepadButton::East) || pressed(GamepadButton::Start) {
        SelectionInput::Back
    } else if pressed(GamepadButton::DPadUp) || vertical < 0 {
        SelectionInput::Up
    } else if pressed(GamepadButton::DPadDown) || vertical > 0 {
        SelectionInput::Down
    } else if pressed(GamepadButton::DPadLeft) || horizontal < 0 {
        SelectionInput::Left
    } else if pressed(GamepadButton::DPadRight) || horizontal > 0 {
        SelectionInput::Right
    } else {
        return;
    };
    if let Some(action) = selection_action(&screen, input) {
        actions.write(action);
    }
}

/// The pseudo's field and the screen agree on who has the keyboard: the screen asked (Entrée on
/// its line: Bevy's focus goes to the field; the screen closed or left the line: it leaves it),
/// or the field did (a click in it, Échap; Entrée, which it lets through, takes it back here).
/// Meanwhile the game sees no key. Each change of its text is the pseudo at once (kept in the
/// browser), cleaned (`pseudo::clean`; empty: the character's name).
#[allow(clippy::too_many_arguments)]
pub(super) fn follow_the_field(
    mut screen: ResMut<SelectionScreen>,
    mut ui: ResMut<PilotUi>,
    focus: Option<ResMut<InputFocus>>,
    fields: Query<Ref<EditableText>, With<PseudoField>>,
    mut hints: Query<&mut Node, With<PseudoHint>>,
    mut keys: MessageReader<KeyboardInput>,
    mut settings: Option<ResMut<NetSettings>>,
    storage_on: Option<Res<PilotStorage>>,
    mut capture: ResMut<KeyboardCapture>,
) {
    let enter = keys.read().any(|key| {
        key.state.is_pressed()
            && (key.logical_key == Key::Enter || key.key_code == KeyCode::NumpadEnter)
    });
    let mut focus = focus;
    let field = ui.field.filter(|field| fields.contains(*field));
    if !screen.open || field.is_none() {
        if let (Some(focus), Some(old)) = (focus.as_mut(), ui.field)
            && focus.get() == Some(old)
        {
            focus.clear();
        }
        if !screen.open {
            ui.field = None;
            if screen.typing {
                screen.typing = false;
            }
        }
        if ui.was_typing {
            ui.was_typing = false;
            capture.set_if_neq(KeyboardCapture(false));
        }
        return;
    }
    let field = field.unwrap();
    let focused = focus.as_ref().is_some_and(|f| f.get() == Some(field));
    if screen.typing != ui.was_typing {
        // The screen asked.
        if let Some(focus) = focus.as_mut() {
            if screen.typing && !focused {
                focus.set(field, FocusCause::Navigated);
            } else if !screen.typing && focused {
                focus.clear();
            }
        }
    } else if screen.typing && ui.was_typing && enter {
        // Entrée: done.
        screen.typing = false;
        if let Some(focus) = focus.as_mut() {
            focus.clear();
        }
    } else if focused != screen.typing {
        // The field took the keyboard (a click), or gave it back (Échap, a click elsewhere).
        screen.typing = focused;
        if focused {
            screen.row = SelectionScreen::PSEUDO_ROW;
        }
    }
    if screen.typing != ui.was_typing {
        ui.was_typing = screen.typing;
        capture.set_if_neq(KeyboardCapture(screen.typing));
    }
    if let Ok(text) = fields.get(field)
        && text.is_changed()
    {
        let value = text.value().to_string();
        if value != ui.typed {
            let pseudo = pseudo::clean(&value);
            ui.typed = value;
            if let Some(settings) = settings.as_mut()
                && settings.pseudo != pseudo
            {
                if storage_on.is_some_and(|on| on.0) {
                    storage::save_key(PSEUDO_STORAGE_KEY, &pseudo::to_stored(pseudo.as_deref()));
                }
                settings.pseudo = pseudo;
            }
        }
    }
    // The hint while the field is empty and nobody types in it.
    let display = if ui.typed.is_empty() && !screen.typing {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut hints {
        if node.display != display {
            node.display = display;
        }
    }
}

/// What the screen shows of the choice (only when it changes: the screen or the keyboard): the
/// lines' values, the chosen portrait and color, the stats, the keys.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn refresh_pilot_tree(
    screen: Res<SelectionScreen>,
    roster: Res<Roster>,
    roots: Query<(), Added<PilotRoot>>,
    mut texts: ParamSet<(
        Query<(&LineValue, &mut Text)>,
        Query<(&StatValue, &mut Text)>,
        Query<(&TerrainValue, &mut Text)>,
        Query<&mut Text, With<PilotHint>>,
    )>,
    mut bars: Query<(&StatBar, &mut Node)>,
    mut cells: Query<(&PortraitCell, &mut BackgroundColor), Without<ColorSwatch>>,
    mut swatches: Query<(&ColorSwatch, &mut BorderColor)>,
) {
    let changed = screen.is_changed() || !roots.is_empty();
    if !changed || !screen.open {
        return;
    }
    let stats = roster.stats(&screen.choice);
    for (value, mut text) in &mut texts.p0() {
        let wanted = line_value(&roster, &screen, value.row);
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    for (value, mut text) in &mut texts.p1() {
        let wanted = pilot::french(stats.display(value.stat));
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    for (value, mut text) in &mut texts.p2() {
        let level = if value.speed {
            stats.speed_on(value.terrain)
        } else {
            stats.handling_on(value.terrain)
        };
        let wanted = pilot::french(display_level(level));
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let help = pilot::typing_help(&screen);
    for mut text in &mut texts.p3() {
        if text.0 != help {
            help.clone_into(&mut text.0);
        }
    }
    for (bar, mut node) in &mut bars {
        let width = percent(100.0 * pilot::bar_share(stats.display(bar.stat)));
        if node.width != width {
            node.width = width;
        }
    }
    for (cell, mut fill) in &mut cells {
        fill.set_if_neq(BackgroundColor(cell_fill(
            cell.character == screen.choice.character,
        )));
    }
    for (swatch, mut ring) in &mut swatches {
        ring.set_if_neq(BorderColor::all(swatch_ring(swatch.color == screen.color)));
    }
}

/// The screen open: the veil fades in, the column pops in, the line with the focus takes its
/// look and grows a little (the charter's focus; the grid only takes the color: it is tall), the
/// button under the pointer too. Only what changed is written.
#[allow(clippy::type_complexity)]
pub(super) fn animate_pilot_tree(
    screen: Res<SelectionScreen>,
    mut ui: ResMut<PilotUi>,
    time: Res<Time<Real>>,
    mut veils: Query<&mut BackgroundColor, With<PilotRoot>>,
    mut columns: Query<&mut UiTransform, (With<PilotColumn>, Without<PilotLine>)>,
    mut lines: Query<
        (
            &PilotLine,
            &mut UiTransform,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            Without<PilotRoot>,
            Without<PilotColumn>,
            Without<PilotButton>,
        ),
    >,
    mut buttons: Query<
        (&PilotButton, &mut BackgroundColor, &mut BorderColor),
        (Without<PilotRoot>, Without<PilotLine>),
    >,
) {
    let now = now(&time);
    if ui.focus != screen.row {
        ui.unfocused = Some(ui.focus);
        ui.focus = screen.row;
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
        let focused = line.row == screen.row;
        let look = line_look(focused);
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
        let scale = if line.row == 0 {
            1.0
        } else if focused {
            anim::focus_scale(true, since)
        } else if ui.unfocused == Some(line.row) {
            anim::focus_scale(false, since)
        } else {
            1.0
        };
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(scale)));
    }
    for (button, mut fill, mut depth) in &mut buttons {
        let look = line_look(ui.hovered == Some(button.confirm));
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
    }
}

/// The screen is open (its systems run).
pub(super) fn pilot_open(screen: Res<SelectionScreen>) -> bool {
    screen.open
}

/// The screen's tree must be built or removed, or its field followed.
pub(super) fn pilot_open_or_shown(
    screen: Res<SelectionScreen>,
    roots: Query<(), With<PilotRoot>>,
    preview: Option<Res<PilotPreview>>,
    ui: Option<Res<PilotUi>>,
) -> bool {
    screen.open
        || !roots.is_empty()
        || preview.is_some()
        || ui.is_some_and(|ui| ui.field.is_some() || ui.was_typing)
}
