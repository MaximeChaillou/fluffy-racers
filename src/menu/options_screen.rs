//! Issue #100: the options screen (O, issues #17 and #80) drawn with `bevy_ui`, after the charter
//! (`style.rs`, `anim.rs`): over a veil, its title, a wide cream panel and the keys at the bottom.
//! On the left, **a line per option** (`options::OptionRow::ALL`: its label, « < », its value,
//! « > »), what the line with the focus does, and « Retour »; on the right, the game's commands
//! with the letters of the chosen keyboard. The line with the focus is the charter's focused
//! button (mango, a little bigger). It replaces the egui window of #17.
//!
//! **Room for more lines**: the volumes (issue #103: general, effects, music) are lines of
//! `OptionRow` like the others; the panel takes six of them in a 720 px high window
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

use bevy::ecs::system::SystemParam;
use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use bevy::text::{FontSource, Justify};

use super::anim;
use super::nav::StickLatch;
use super::screen::{MenuAssets, MenuNode};
use super::solo_screen::{
    SoloButton, SoloColumn, SoloHeader, SoloHelp, SoloLine, SoloRoot, SoloValue,
};
use super::style::{self, ButtonState, font, palette, size, u};
use crate::kart::KeyboardLayout;
use crate::options::{OptionRow, OptionsAction, OptionsMenu, PlayerOptions, controls};

/// The screen's title.
pub const TITLE: &str = "Options";
/// The header of the commands, on the right.
pub const CONTROLS_TITLE: &str = "Commandes";
/// The button under the lines.
pub const BACK_LABEL: &str = "Retour";

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

/// A line of the commands (an index of `options::controls`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct ControlLine(pub usize);

/// A line of keys at the bottom.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct OptionsHint(pub usize);

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
}

/// The keys at the bottom of the screen, with the letters of the player's keyboard.
pub fn hint(layout: KeyboardLayout) -> [String; 2] {
    let [up, left, down, right] = layout.letters();
    [
        format!(
            "Haut / Bas ({up} / {down}) : la ligne  ·  Gauche / Droite ({left} / {right}), \
             Entrée : changer  ·  Échap ou O : fermer"
        ),
        "Manette : croix ou stick, A : changer, B : fermer".to_string(),
    ]
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
    menu: Res<'w, OptionsMenu>,
    ui: Option<ResMut<'w, OptionsUi>>,
    time: Res<'w, Time<Real>>,
    roots: Query<'w, 's, (Entity, &'static OptionsRoot)>,
    assets: Option<Res<'w, MenuAssets>>,
    layout: Option<Res<'w, KeyboardLayout>>,
    options: Res<'w, PlayerOptions>,
    cameras: Query<'w, 's, (), With<Camera>>,
}

impl OptionsSync<'_, '_> {
    pub fn run(&mut self) {
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
            self.commands.insert_resource(OptionsUi {
                opened_at: now,
                focus: self.menu.row,
                focused_at: now - f64::from(anim::FOCUS_TIME),
                unfocused: None,
                hovered: false,
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
        }
        let assets = self.assets.as_deref().cloned().unwrap_or_default();
        spawn_tree(
            &mut self.commands,
            &self.menu,
            &self.options,
            layout,
            &assets,
        );
    }
}

fn spawn_tree(
    commands: &mut Commands,
    menu: &OptionsMenu,
    options: &PlayerOptions,
    layout: KeyboardLayout,
    assets: &MenuAssets,
) {
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
                        spawn_lines(panel, menu, options, assets);
                        spawn_controls(panel, layout, assets);
                    });
                for (line, hint) in hint(layout).into_iter().enumerate() {
                    column.spawn((
                        OptionsHint(line),
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

/// The left column: a line per option, the help of the line with the focus, « Retour ».
fn spawn_lines(
    panel: &mut ChildSpawnerCommands,
    menu: &OptionsMenu,
    options: &PlayerOptions,
    assets: &MenuAssets,
) {
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
                spawn_line(left, assets, options, index, row, menu.row == index);
            }
            left.spawn((
                OptionsHelp,
                text(
                    menu.row().help(),
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
            spawn_back(left, assets);
        });
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
    roots: Query<'w, 's, (), Added<OptionsRoot>>,
    values:
        Query<'w, 's, (&'static OptionsValue, &'static mut Text), (NotSolo, Without<OptionsHelp>)>,
    helps: Query<'w, 's, &'static mut Text, (With<OptionsHelp>, NotSolo)>,
}

impl OptionsRefresh<'_, '_> {
    pub fn run(&mut self) {
        let menu = &self.menu;
        if !menu.open || (!menu.is_changed() && !self.options.is_changed() && self.roots.is_empty())
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
        let help = menu.row().help();
        for mut text in &mut self.helps {
            if text.0 != help {
                text.0.clear();
                text.0.push_str(help);
            }
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
        if !self.menu.open {
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

    /// The keys at the bottom write the letters of the chosen keyboard, and every text of the
    /// screen is short (the charter: a label of three words at most, a small line under 48
    /// characters).
    #[test]
    fn the_keys_and_the_texts_of_the_options() {
        let [keys, pad] = hint(KeyboardLayout::Azerty);
        assert!(
            keys.contains("(Z / S)") && keys.contains("(Q / D)"),
            "{keys}"
        );
        assert!(keys.contains("Échap ou O : fermer"), "{keys}");
        assert!(pad.contains("B : fermer"), "{pad}");
        let [keys, _] = hint(KeyboardLayout::Qwerty);
        assert!(
            keys.contains("(W / S)") && keys.contains("(A / D)"),
            "{keys}"
        );
        for row in OptionRow::ALL {
            assert!(row.label().chars().count() <= 16, "{}", row.label());
            assert!(row.help().chars().count() < 48, "{}", row.help());
        }
    }
}
