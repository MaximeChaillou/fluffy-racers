//! Issue #100: the options screen (O, issues #17 and #80) drawn with `bevy_ui`, after the charter
//! (`style.rs`, `anim.rs`): over a veil, its title and a wide cream panel (issue #136: no line of
//! keys under it any more).
//! On the left, **a line per option** (`options::OptionRow::ALL`: its label, « < », its value,
//! « > »), what the line with the focus does, and « Retour »; on the right, the game's commands
//! with the letters of the chosen keyboard. The line with the focus is the charter's focused
//! button (mango, a little bigger). It replaces the egui window of #17.
//!
//! **Room for more lines**: the volumes (issue #103: general, effects, music) are lines of
//! `OptionRow` like the others (issue #234: the images per second, a seventh); the panel follows
//! its lines and takes seven of them, and room for one more, in a 720 px high window
//! (`options_tests`).
//!
//! Its state and its keys are `options::OptionsMenu`'s (headless, tested); here: its tree, the
//! gamepad and the mouse (the pointer over a line gives it the focus; a click on « < », « > » or
//! the value changes it). **No system of its own**: its steps ([`OptionsSync`],
//! [`OptionsRefresh`], [`OptionsAnimate`], [`OptionsGamepad`]) are parameters of the solo
//! screen's systems (`super::SoloPlugin` says why: the order of the game's systems). **Closed,
//! nothing is left**: the tree and its observers go when it closes; those systems only run while
//! one of the two screens is open (or its tree is still there). Nothing of it exists before it
//! first opens (its resource comes then): the home's background keeps its world.
//!
//! **Issue #231: the « Serveur » line** is a text field (the dedicated server's address, #225),
//! Bevy's `EditableText` like the pseudo of « Pilote » (`pilot_screen.rs`): Entrée, Espace or
//! right on its line, or a click in it, give it the keyboard (`OptionsMenu::typing`; the game
//! sees no key, `admin::KeyboardCapture`); Entrée, Échap or a click elsewhere take it back, and
//! the address is then checked ([`ServerEntry`]): readable (`ws://` or `wss://`), it is
//! `NetSettings::server` and kept in the browser (`net::SERVER_STORAGE_KEY`, in the game only:
//! [`PilotStorage`]); empty, none (the page hosts, as before); unreadable, nothing changes and the
//! line under the lines says so. That line also says when the page's link chooses another
//! server (`?server=`, which wins without replacing it) and, on a page in `https://`, that a
//! `ws://` address is blocked ([`ServerNotice`]). The field follows in the step of
//! [`OptionsSync`] (no system of its own).

use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, FontSource, Justify};
use bevy::ui_widgets::TextInput;

use super::anim;
use super::nav::StickLatch;
use super::pilot_screen::PilotStorage;
use super::screen::{MenuAssets, MenuNode};
use super::solo_screen::{
    SoloButton, SoloColumn, SoloHeader, SoloHelp, SoloLine, SoloRoot, SoloValue,
};
use super::style::{self, ButtonState, font, palette, size, u};
use crate::admin::{KeyboardCapture, storage};
use crate::kart::KeyboardLayout;
use crate::net::{
    NetSettings, SERVER_STORAGE_KEY, ServerEntry, blocked_from_secure_page, page_is_secure,
    server_from_stored, server_to_stored,
};
use crate::options::{OptionRow, OptionsAction, OptionsMenu, PlayerOptions, controls};

/// The screen's title.
pub const TITLE: &str = "Options";
/// The header of the commands, on the right.
pub const CONTROLS_TITLE: &str = "Commandes";
/// The button under the lines.
pub const BACK_LABEL: &str = "Retour";
/// Issue #231: what the empty server field shows (no server: the page hosts).
pub const SERVER_HINT: &str = "Aucun";
/// Issue #231: an address is at most this long (a domain and a port fit easily).
pub const SERVER_MAX_CHARS: usize = 120;
/// Issue #231: the label of the server's line is short: its field gets the room (px).
const SERVER_LABEL_WIDTH: f32 = 104.0;

/// Issue #231: what the line under the lines says while the « Serveur » line has the focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServerNotice {
    /// No server: the page of the one who creates the game hosts it.
    #[default]
    Hosting,
    /// « Créer une partie » goes to the server typed.
    OnServer,
    /// The page's link (`?server=`) chooses another one (or none) this time.
    FromLink,
    /// The text typed is not an address: nothing was kept.
    Unreadable,
    /// The page is in `https://` and the address in `ws://`: the browser blocks it.
    NeedsWss,
}

impl ServerNotice {
    /// What it says (a small line of the charter: fewer than 48 characters).
    pub fn text(self) -> &'static str {
        match self {
            ServerNotice::Hosting => OptionRow::Server.help(),
            ServerNotice::OnServer => "« Créer une partie » va sur ce serveur.",
            ServerNotice::FromLink => "Cette fois, le serveur du lien (?server=).",
            ServerNotice::Unreadable => "Adresse illisible : ws://… ou wss://…",
            ServerNotice::NeedsWss => "Page https : il faut wss:// (SERVEUR.md § 4)",
        }
    }

    /// A warning: written in Fraise.
    pub fn warns(self) -> bool {
        matches!(self, ServerNotice::Unreadable | ServerNotice::NeedsWss)
    }
}

/// Issue #231: what to say of the server `kept` (the options') when the game uses `used` (the
/// link's wins), on a page in `https://` (`secure`) or not.
pub fn server_notice(kept: Option<&str>, used: Option<&str>, secure: bool) -> ServerNotice {
    if secure && used.is_some_and(blocked_from_secure_page) {
        ServerNotice::NeedsWss
    } else if kept != used {
        ServerNotice::FromLink
    } else if used.is_some() {
        ServerNotice::OnServer
    } else {
        ServerNotice::Hosting
    }
}

/// The root of the screen's tree (the veil), with the keyboard it writes the letters of: another
/// keyboard, another tree.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct OptionsRoot {
    pub layout: KeyboardLayout,
}

/// The column of the title, the panel and the keys: it pops in.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct OptionsColumn;

/// A line of an option (`index`: in `OptionRow::ALL`): its pill takes the focus's look.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct OptionsLine {
    pub index: usize,
    pub row: OptionRow,
}

/// The value written on a line.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct OptionsValue {
    pub row: OptionRow,
}

/// « < » or « > » on a line.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct OptionsArrow {
    pub row: OptionRow,
    pub step: i32,
}

/// What the line with the focus does.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct OptionsHelp;

/// Issue #231: the server's text field.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct ServerField;

/// Issue #231: what the empty server field shows ([`SERVER_HINT`]).
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct ServerHint;

/// A line of the commands (an index of `options::controls`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct ControlLine(pub usize);

/// « Retour ».
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct OptionsBack;

/// What the screen remembers (real times, s). There once the screen first opened.
#[derive(Resource, Debug)]
pub struct OptionsUi {
    pub opened_at: f64,
    /// The line with the focus, since when, and the one that lost it.
    pub focus: usize,
    pub focused_at: f64,
    pub unfocused: Option<usize>,
    /// « Retour » is under the pointer.
    pub hovered: bool,
    /// Issue #231: the server's field, and whether it had the keyboard at the end of the last
    /// frame.
    pub field: Option<Entity>,
    pub was_typing: bool,
    /// The field's text as typed.
    pub typed: String,
    /// The server kept (in the browser in the game): what the field shows when the screen opens.
    pub kept: Option<String>,
    /// What the line under the lines says on the server's line.
    pub notice: ServerNotice,
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
    style::button_look(state, palette::PRUNE)
}

/// The screen's tree, step 1 (`SoloPlugin`'s `sync_solo_tree` runs it: no system of its own,
/// see `super::SoloPlugin`). Open: its tree, for the keyboard now (another keyboard: another
/// tree, its letters in the commands and the keys; the values change in place,
/// [`OptionsRefresh`]). Closed: none of it. Not before the game has a camera (at launch, the
/// circuit is still loading): the sizes are shares of its view.
#[derive(SystemParam)]
pub struct OptionsSync<'w, 's> {
    commands: Commands<'w, 's>,
    menu: ResMut<'w, OptionsMenu>,
    ui: Option<ResMut<'w, OptionsUi>>,
    time: Res<'w, Time<Real>>,
    roots: Query<'w, 's, (Entity, &'static OptionsRoot)>,
    assets: Option<Res<'w, MenuAssets>>,
    layout: Option<Res<'w, KeyboardLayout>>,
    options: Res<'w, PlayerOptions>,
    cameras: Query<'w, 's, (), With<Camera>>,
    field: ServerFieldParams<'w, 's>,
}

/// Issue #231: what the server's field reads and writes (see the module's documentation).
#[derive(SystemParam)]
pub struct ServerFieldParams<'w, 's> {
    focus: Option<ResMut<'w, InputFocus>>,
    fields: Query<'w, 's, Ref<'static, EditableText>, With<ServerField>>,
    hints: Query<'w, 's, &'static mut Node, With<ServerHint>>,
    keys: MessageReader<'w, 's, KeyboardInput>,
    settings: Option<ResMut<'w, NetSettings>>,
    storage_on: Option<Res<'w, PilotStorage>>,
    capture: Option<ResMut<'w, KeyboardCapture>>,
}

impl ServerFieldParams<'_, '_> {
    fn storage_on(&self) -> bool {
        self.storage_on.as_ref().is_some_and(|on| on.0)
    }

    /// The server kept: in the browser in the game, the one used otherwise (the tests).
    fn kept(&self) -> Option<String> {
        if self.storage_on() {
            server_from_stored(storage::load_key(SERVER_STORAGE_KEY).as_deref())
        } else {
            self.used()
        }
    }

    fn used(&self) -> Option<String> {
        self.settings
            .as_ref()
            .and_then(|settings| settings.server.clone())
    }

    fn capture(&mut self, on: bool) {
        if let Some(capture) = self.capture.as_mut() {
            capture.set_if_neq(KeyboardCapture(on));
        }
    }

    /// The typing ended: the address typed is checked, then kept (or refused, the notice says
    /// why). Unchanged, nothing is written: the link's server stays this time.
    fn commit(&mut self, ui: &mut OptionsUi) {
        let entry = ServerEntry::read(&ui.typed);
        if entry == ServerEntry::Unreadable {
            ui.notice = ServerNotice::Unreadable;
            return;
        }
        let server = entry.server().map(str::to_string);
        if server != ui.kept {
            if self.storage_on() {
                storage::save_key(SERVER_STORAGE_KEY, server_to_stored(server.as_deref()));
            }
            info!("network: dedicated server from the options: {server:?}");
            if let Some(settings) = self.settings.as_mut()
                && settings.server != server
            {
                settings.server = server.clone();
            }
            ui.kept = server;
        }
        ui.notice = server_notice(ui.kept.as_deref(), self.used().as_deref(), page_is_secure());
    }

    /// The field and the menu agree on who has the keyboard (as the pseudo's on « Pilote »): the
    /// menu asked (Entrée on the line: Bevy's focus goes to the field; the menu closed: it
    /// leaves it), or the field did (a click in it, Échap; Entrée, which it lets through, takes
    /// it back here). Meanwhile the game sees no key. The end of the typing checks the address.
    fn follow(&mut self, menu: &mut ResMut<OptionsMenu>, ui: &mut OptionsUi) {
        let enter = self.keys.read().any(|key| {
            key.state.is_pressed()
                && (key.logical_key == Key::Enter || key.key_code == KeyCode::NumpadEnter)
        });
        let field = ui.field.filter(|field| self.fields.contains(*field));
        let Some(field) = field.filter(|_| menu.open) else {
            if let (Some(focus), Some(old)) = (self.focus.as_mut(), ui.field)
                && focus.get() == Some(old)
            {
                focus.clear();
            }
            if !menu.open {
                ui.field = None;
                if menu.typing {
                    menu.typing = false;
                }
            }
            if ui.was_typing {
                ui.was_typing = false;
                self.capture(false);
                self.commit(ui);
            }
            return;
        };
        let focused = self.focus.as_ref().is_some_and(|f| f.get() == Some(field));
        if menu.typing != ui.was_typing {
            // The menu asked.
            if let Some(focus) = self.focus.as_mut() {
                if menu.typing && !focused {
                    focus.set(field, FocusCause::Navigated);
                } else if !menu.typing && focused {
                    focus.clear();
                }
            }
        } else if menu.typing && ui.was_typing && enter {
            // Entrée: done.
            menu.typing = false;
            if let Some(focus) = self.focus.as_mut() {
                focus.clear();
            }
        } else if focused != menu.typing {
            // The field took the keyboard (a click), or gave it back (Échap, a click elsewhere).
            menu.typing = focused;
            if let Some(index) = OptionRow::ALL.iter().position(|r| *r == OptionRow::Server)
                && focused
                && menu.row != index
            {
                menu.row = index;
            }
        }
        if let Ok(text) = self.fields.get(field)
            && text.is_changed()
        {
            let value = text.value().to_string();
            if value != ui.typed {
                ui.typed = value;
            }
        }
        if menu.typing != ui.was_typing {
            ui.was_typing = menu.typing;
            self.capture(menu.typing);
            if !menu.typing {
                self.commit(ui);
            }
        }
        // The hint while the field is empty and nobody types in it.
        let display = if ui.typed.is_empty() && !menu.typing {
            Display::Flex
        } else {
            Display::None
        };
        for mut node in &mut self.hints {
            if node.display != display {
                node.display = display;
            }
        }
    }
}

impl OptionsSync<'_, '_> {
    pub fn run(&mut self) {
        if let Some(ui) = self.ui.as_mut() {
            self.field.follow(&mut self.menu, ui);
        }
        if !self.menu.open {
            for (entity, _) in &self.roots {
                self.commands.entity(entity).despawn();
            }
            return;
        }
        if self.cameras.is_empty() {
            return;
        }
        let now = now(&self.time);
        let Some(ui) = self.ui.as_mut() else {
            let kept = self.field.kept();
            self.commands.insert_resource(OptionsUi {
                opened_at: now,
                focus: self.menu.row,
                focused_at: now - f64::from(anim::FOCUS_TIME),
                unfocused: None,
                hovered: false,
                field: None,
                was_typing: false,
                typed: String::new(),
                kept,
                notice: ServerNotice::default(),
            });
            return;
        };
        let layout = self
            .layout
            .as_ref()
            .map_or_else(KeyboardLayout::default, |layout| **layout);
        let mut shown = false;
        for (entity, root) in &self.roots {
            if !shown && root.layout == layout {
                shown = true;
            } else {
                self.commands.entity(entity).despawn();
            }
        }
        if shown {
            return;
        }
        if self.roots.is_empty() {
            // It opens now: it pops in (a tree built again while it is open does not).
            ui.opened_at = now;
            ui.focus = self.menu.row;
            ui.focused_at = now - f64::from(anim::FOCUS_TIME);
            ui.unfocused = None;
            ui.hovered = false;
            // Issue #231: the field shows the server kept, the line under it what it does.
            ui.typed = ui.kept.clone().unwrap_or_default();
            ui.notice = server_notice(
                ui.kept.as_deref(),
                self.field.used().as_deref(),
                page_is_secure(),
            );
        }
        let assets = self.assets.as_deref().cloned().unwrap_or_default();
        ui.field = Some(spawn_tree(
            &mut self.commands,
            &self.menu,
            &self.options,
            layout,
            &assets,
            ui,
        ));
    }
}

/// Returns the server's field (issue #231).
fn spawn_tree(
    commands: &mut Commands,
    menu: &OptionsMenu,
    options: &PlayerOptions,
    layout: KeyboardLayout,
    assets: &MenuAssets,
    ui: &OptionsUi,
) -> Entity {
    let mut field = Entity::PLACEHOLDER;
    commands
        .spawn((
            Name::new(TITLE),
            OptionsRoot { layout },
            // Over the other screens: the options open over « Pilote » (O).
            GlobalZIndex(30),
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
                OptionsColumn,
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
                    text(TITLE, &assets.semibold, size::PILOT_TITLE, palette::NEIGE),
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
                        field = spawn_lines(panel, menu, options, assets, ui);
                        spawn_controls(panel, layout, assets);
                    });
            });
        });
    field
}

/// The left column: a line per option, the help of the line with the focus, « Retour ». Returns
/// the server's field (issue #231).
fn spawn_lines(
    panel: &mut ChildSpawnerCommands,
    menu: &OptionsMenu,
    options: &PlayerOptions,
    assets: &MenuAssets,
    ui: &OptionsUi,
) -> Entity {
    let mut field = Entity::PLACEHOLDER;
    let (help, color) = help_of(menu, ui);
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
            for (index, row) in OptionRow::ALL.into_iter().enumerate() {
                if row == OptionRow::Server {
                    field = spawn_server_line(left, assets, index, &ui.typed);
                } else {
                    spawn_line(left, assets, options, index, row, menu.row == index);
                }
            }
            left.spawn((
                OptionsHelp,
                text(help, &assets.medium, size::SOLO_HELP, color),
                Node {
                    margin: UiRect::left(u(18.0)),
                    min_height: u(size::SOLO_HELP * 1.3),
                    ..default()
                },
            ));
            spawn_back(left, assets);
        });
    field
}

/// The line under the lines: what the line with the focus does; issue #231: on the server's,
/// its notice (a warning in Fraise).
fn help_of(menu: &OptionsMenu, ui: &OptionsUi) -> (&'static str, Color) {
    let normal = palette::MYRTILLE.with_alpha(0.75);
    match menu.row() {
        OptionRow::Server if ui.notice.warns() => (ui.notice.text(), palette::FRAISE),
        OptionRow::Server => (ui.notice.text(), normal),
        row => (row.help(), normal),
    }
}

/// Issue #231: the server's line: its label, then its text field (the pseudo's of « Pilote »),
/// with [`SERVER_HINT`] while it is empty. Returns the field.
fn spawn_server_line(
    column: &mut ChildSpawnerCommands,
    assets: &MenuAssets,
    index: usize,
    typed: &str,
) -> Entity {
    let row = OptionRow::Server;
    let look = line_look(false);
    let mut field = Entity::PLACEHOLDER;
    column
        .spawn((
            Name::new(row.label()),
            OptionsLine { index, row },
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
                text(
                    row.label(),
                    &assets.semibold,
                    size::LINE_TEXT,
                    palette::MYRTILLE,
                ),
                Node {
                    width: u(SERVER_LABEL_WIDTH),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            line.spawn((
                MenuNode,
                Node {
                    flex_grow: 1.0,
                    height: u(size::LINE_HEIGHT - 14.0),
                    padding: UiRect::horizontal(u(12.0)),
                    align_items: AlignItems::Center,
                    overflow: Overflow::clip(),
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
                    ServerHint,
                    text(
                        SERVER_HINT,
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
                        ServerField,
                        TextInput,
                        EditableText {
                            max_characters: Some(SERVER_MAX_CHARS),
                            ..EditableText::new(typed)
                        },
                        // An address has no space; a pasted one loses its ends'.
                        EditableTextFilter::new(|c: char| !c.is_whitespace() && !c.is_control()),
                        TextFont {
                            font: FontSource::from(&assets.medium),
                            font_size: font(size::SOLO_HELP + 2.0),
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
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<OptionsAction>| {
                actions.write(OptionsAction::Focus(index));
            },
        );
    field
}

/// A line's pill: its label, « < », its value, « > ».
fn spawn_line(
    column: &mut ChildSpawnerCommands,
    assets: &MenuAssets,
    options: &PlayerOptions,
    index: usize,
    row: OptionRow,
    focused: bool,
) {
    let look = line_look(focused);
    column
        .spawn((
            Name::new(row.label()),
            OptionsLine { index, row },
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
                text(
                    row.label(),
                    &assets.semibold,
                    size::LINE_TEXT,
                    palette::MYRTILLE,
                ),
                Node {
                    width: u(size::OPTION_LABEL_WIDTH),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            spawn_arrow(line, assets, row, -1);
            // The value: a click changes it too (forward).
            line.spawn((
                MenuNode,
                Node {
                    flex_grow: 1.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|value| {
                value.spawn((
                    OptionsValue { row },
                    text(
                        options.value(row),
                        &assets.semibold,
                        size::LINE_TEXT,
                        palette::MYRTILLE,
                    ),
                    TextLayout::justify(Justify::Center),
                ));
            })
            .observe(
                move |_: On<PointerClick>, mut actions: MessageWriter<OptionsAction>| {
                    actions.write(OptionsAction::Change(row, 1));
                },
            );
            spawn_arrow(line, assets, row, 1);
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<OptionsAction>| {
                actions.write(OptionsAction::Focus(index));
            },
        );
}

/// A round « < » or « > » of a line.
fn spawn_arrow(line: &mut ChildSpawnerCommands, assets: &MenuAssets, row: OptionRow, step: i32) {
    line.spawn((
        OptionsArrow { row, step },
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
        move |_: On<PointerClick>, mut actions: MessageWriter<OptionsAction>| {
            actions.write(OptionsAction::Change(row, step));
        },
    );
}

/// « Retour », its Myrtille dot (home, as on « Jouer solo »).
fn spawn_back(left: &mut ChildSpawnerCommands, assets: &MenuAssets) {
    let look = line_look(false);
    left.spawn((
        Name::new(BACK_LABEL),
        OptionsBack,
        Node {
            height: u(size::PILOT_BUTTON_HEIGHT),
            margin: UiRect::top(u(4.0)),
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
            BackgroundColor(palette::MYRTILLE),
            Pickable::IGNORE,
        ));
        button.spawn(text(
            BACK_LABEL,
            &assets.semibold,
            size::LINE_TEXT + 3.0,
            look.label,
        ));
    })
    .observe(|_: On<PointerOver>, mut ui: ResMut<OptionsUi>| {
        ui.hovered = true;
    })
    .observe(|_: On<PointerOut>, mut ui: ResMut<OptionsUi>| {
        ui.hovered = false;
    })
    .observe(
        |_: On<PointerClick>, mut actions: MessageWriter<OptionsAction>| {
            actions.write(OptionsAction::Close);
        },
    );
}

/// The right column: the commands, with the letters of `layout`.
fn spawn_controls(panel: &mut ChildSpawnerCommands, layout: KeyboardLayout, assets: &MenuAssets) {
    panel
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: u(size::CONTROL_GAP),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|right| {
            right.spawn((
                text(
                    CONTROLS_TITLE,
                    &assets.semibold,
                    size::CONTROLS_HEADER,
                    palette::MYRTILLE,
                ),
                Node {
                    margin: UiRect::bottom(u(4.0)),
                    ..default()
                },
            ));
            for (index, (action, keys)) in controls(layout).into_iter().enumerate() {
                right
                    .spawn((
                        ControlLine(index),
                        Node {
                            column_gap: u(10.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|line| {
                        line.spawn((
                            text(
                                action,
                                &assets.medium,
                                size::CONTROL_TEXT,
                                palette::MYRTILLE.with_alpha(0.8),
                            ),
                            Node {
                                flex_grow: 1.0,
                                flex_basis: Val::ZERO,
                                ..default()
                            },
                        ));
                        line.spawn((
                            text(
                                keys,
                                &assets.semibold,
                                size::CONTROL_TEXT,
                                palette::MYRTILLE,
                            ),
                            Node {
                                width: u(size::CONTROL_KEYS_WIDTH),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                    });
            }
        });
}

/// What the solo screen's systems may touch: the options screen's queries leave its entities out
/// (they run in the same systems, `super::SoloPlugin`).
type NotSolo = (
    Without<SoloRoot>,
    Without<SoloColumn>,
    Without<SoloLine>,
    Without<SoloButton>,
    Without<SoloValue>,
    Without<SoloHelp>,
    Without<SoloHeader>,
);

/// The screen's tree, step 2 (run by `refresh_solo_tree`): what it shows of the options when only
/// values change: the lines' values, the help of the line with the focus. Only while it is open.
#[derive(SystemParam)]
pub struct OptionsRefresh<'w, 's> {
    menu: Res<'w, OptionsMenu>,
    options: Res<'w, PlayerOptions>,
    ui: Option<Res<'w, OptionsUi>>,
    roots: Query<'w, 's, (), Added<OptionsRoot>>,
    values:
        Query<'w, 's, (&'static OptionsValue, &'static mut Text), (NotSolo, Without<OptionsHelp>)>,
    helps: Query<'w, 's, (&'static mut Text, &'static mut TextColor), (With<OptionsHelp>, NotSolo)>,
}

impl OptionsRefresh<'_, '_> {
    pub fn run(&mut self) {
        let menu = &self.menu;
        let ui_changed = self.ui.as_ref().is_some_and(|ui| ui.is_changed());
        if !menu.open
            || (!menu.is_changed()
                && !self.options.is_changed()
                && !ui_changed
                && self.roots.is_empty())
        {
            return;
        }
        for (value, mut text) in &mut self.values {
            let wanted = self.options.value(value.row);
            if text.0 != wanted {
                text.0.clear();
                text.0.push_str(wanted);
            }
        }
        let (help, color) = match self.ui.as_deref() {
            Some(ui) => help_of(menu, ui),
            None => (menu.row().help(), palette::MYRTILLE.with_alpha(0.75)),
        };
        for (mut text, mut text_color) in &mut self.helps {
            if text.0 != help {
                text.0.clear();
                text.0.push_str(help);
            }
            text_color.set_if_neq(TextColor(color));
        }
    }
}

/// The screen's tree, step 3 (run by `animate_solo_tree`), while it is open: the veil fades in,
/// the column pops in, the line with the focus takes its look and grows a little (the charter's
/// focus), « Retour » under the pointer too. Only what changed is written.
#[allow(clippy::type_complexity)]
#[derive(SystemParam)]
pub struct OptionsAnimate<'w, 's> {
    menu: Res<'w, OptionsMenu>,
    ui: Option<ResMut<'w, OptionsUi>>,
    time: Res<'w, Time<Real>>,
    veils: Query<'w, 's, &'static mut BackgroundColor, (With<OptionsRoot>, NotSolo)>,
    columns: Query<
        'w,
        's,
        &'static mut UiTransform,
        (With<OptionsColumn>, Without<OptionsLine>, NotSolo),
    >,
    lines: Query<
        'w,
        's,
        (
            &'static OptionsLine,
            &'static mut UiTransform,
            &'static mut BackgroundColor,
            &'static mut BorderColor,
        ),
        (
            Without<OptionsRoot>,
            Without<OptionsColumn>,
            Without<OptionsBack>,
            NotSolo,
        ),
    >,
    backs: Query<
        'w,
        's,
        (&'static mut BackgroundColor, &'static mut BorderColor),
        (
            With<OptionsBack>,
            Without<OptionsRoot>,
            Without<OptionsLine>,
            NotSolo,
        ),
    >,
}

impl OptionsAnimate<'_, '_> {
    pub fn run(&mut self) {
        let menu = &self.menu;
        let (Some(ui), true) = (self.ui.as_mut(), menu.open) else {
            return;
        };
        let now = now(&self.time);
        if ui.focus != menu.row {
            ui.unfocused = Some(ui.focus);
            ui.focus = menu.row;
            ui.focused_at = now;
        }
        let opened = (now - ui.opened_at) as f32;
        for mut veil in &mut self.veils {
            veil.set_if_neq(BackgroundColor(
                palette::VOILE.with_alpha(palette::VOILE.alpha() * anim::veil(opened)),
            ));
        }
        for mut transform in &mut self.columns {
            transform.set_if_neq(UiTransform::from_scale(Vec2::splat(anim::panel_pop(
                opened,
            ))));
        }
        let since = (now - ui.focused_at) as f32;
        for (line, mut transform, mut fill, mut depth) in &mut self.lines {
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
        let look = line_look(ui.hovered);
        for (mut fill, mut depth) in &mut self.backs {
            fill.set_if_neq(BackgroundColor(look.fill));
            depth.set_if_neq(BorderColor::all(look.depth));
        }
    }
}

/// The gamepad on the screen (run by `solo_gamepad`): the cross or the left stick (a step per
/// push) go through its lines and change them, A changes the line too, B or Start close it.
#[derive(SystemParam)]
pub struct OptionsGamepad<'w, 's> {
    menu: Res<'w, OptionsMenu>,
    latches: Local<'s, [StickLatch; 2]>,
    actions: MessageWriter<'w, OptionsAction>,
}

impl OptionsGamepad<'_, '_> {
    pub fn run(&mut self, gamepads: &Query<&Gamepad>) {
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
        let (vertical, horizontal) = (self.latches[0].step(y), -self.latches[1].step(x));
        // Issue #231: not while one types the server.
        if !self.menu.open || self.menu.typing {
            return;
        }
        let pressed = |button: GamepadButton| gamepads.iter().any(|g| g.just_pressed(button));
        let row = self.menu.row();
        let action = if pressed(GamepadButton::East) || pressed(GamepadButton::Start) {
            OptionsAction::Close
        } else if pressed(GamepadButton::DPadUp) || vertical < 0 {
            OptionsAction::Row(-1)
        } else if pressed(GamepadButton::DPadDown) || vertical > 0 {
            OptionsAction::Row(1)
        } else if pressed(GamepadButton::DPadLeft) || horizontal < 0 {
            OptionsAction::Change(row, -1)
        } else if pressed(GamepadButton::DPadRight)
            || horizontal > 0
            || pressed(GamepadButton::South)
        {
            OptionsAction::Change(row, 1)
        } else {
            return;
        };
        self.actions.write(action);
    }
}

/// The screen's tree must be built, rebuilt or removed (with the solo screen's: `solo_screen::
/// solo_open_or_shown`).
pub(super) fn options_open_or_shown(
    menu: Option<Res<OptionsMenu>>,
    roots: Query<(), With<OptionsRoot>>,
) -> bool {
    menu.is_some_and(|menu| menu.open) || !roots.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every text of the screen is short (the charter: a label of three words at most, a small
    /// line under 48 characters). Issue #136: no line of keys under the panel any more (it was
    /// checked here; the commands on the right still follow the keyboard: `options_tests`).
    #[test]
    fn the_texts_of_the_options_are_short() {
        for row in OptionRow::ALL {
            assert!(row.label().chars().count() <= 16, "{}", row.label());
            assert!(row.help().chars().count() < 48, "{}", row.help());
        }
        // Issue #231: and so are the server's notices.
        for notice in NOTICES {
            assert!(notice.text().chars().count() < 48, "{}", notice.text());
        }
    }

    const NOTICES: [ServerNotice; 5] = [
        ServerNotice::Hosting,
        ServerNotice::OnServer,
        ServerNotice::FromLink,
        ServerNotice::Unreadable,
        ServerNotice::NeedsWss,
    ];

    /// Issue #231: what the server's line says. No server: the page hosts; the one kept: the
    /// game goes there; another one (or none) from the link: it says so; on a page in
    /// `https://`, a `ws://` address in use (not this very machine) is blocked: `wss://`, and
    /// where to read how (`docs/SERVEUR.md`, § 4, « HTTPS »). Only refusals warn.
    #[test]
    fn the_server_line_says_what_the_address_does() {
        let lan = Some("ws://192.168.1.30:3536");
        let tls = Some("wss://fluffy.duckdns.org");
        let local = Some("ws://localhost:3536");
        assert_eq!(server_notice(None, None, false), ServerNotice::Hosting);
        assert_eq!(server_notice(None, None, true), ServerNotice::Hosting);
        assert_eq!(server_notice(lan, lan, false), ServerNotice::OnServer);
        assert_eq!(server_notice(tls, tls, true), ServerNotice::OnServer);
        assert_eq!(server_notice(local, local, true), ServerNotice::OnServer);
        assert_eq!(server_notice(lan, lan, true), ServerNotice::NeedsWss);
        assert_eq!(server_notice(None, lan, true), ServerNotice::NeedsWss);
        assert_eq!(server_notice(lan, tls, false), ServerNotice::FromLink);
        assert_eq!(server_notice(lan, tls, true), ServerNotice::FromLink);
        assert_eq!(server_notice(tls, None, true), ServerNotice::FromLink);
        assert_eq!(server_notice(None, tls, false), ServerNotice::FromLink);
        assert!(ServerNotice::NeedsWss.text().contains("wss://"));
        assert!(ServerNotice::NeedsWss.text().contains("SERVEUR.md"));
        assert!(ServerNotice::Unreadable.text().contains("ws://"));
        assert_eq!(ServerNotice::Hosting.text(), OptionRow::Server.help());
        for notice in NOTICES {
            assert_eq!(
                notice.warns(),
                matches!(notice, ServerNotice::Unreadable | ServerNotice::NeedsWss),
                "{notice:?}"
            );
        }
    }
}
