//! Characters and karts (GD § 9, issue #15): who drives which kart, and the selection screen.
//!
//! - **Roster** ([`Roster`], `roster.rs`): 9 animals in 3 weight classes, 3 karts, 3 sets of
//!   wheels and 2 wings, read from the RON files of `assets/data/` (compiled into the game).
//! - **Loadouts**: the player's choice ([`PlayerLoadout`], kept for the whole session: a restart
//!   keeps it), and the bots' (`bots::bot_loadout`, from their grid slot). A kart's stats are the
//!   sum of its four parts (`kart::KartStats`), converted once when it spawns
//!   (`kart::kart_body_with_stats`).
//! - **Looks**: one shared material per character ([`DriverLooks`]); the driver's size shows its
//!   weight class, its color and ears the animal.
//! - **Color** (issue #55): the player's kart's color ([`PlayerColor`]), a color of the palette
//!   (`kart::KartColor`), pink until it chooses. Kept between sessions like the options
//!   (`admin::storage`, key [`COLOR_STORAGE_KEY`]; with a renderer only, `ui.rs`). Online, two
//!   players of the same color: the first seat keeps it (`net`).
//! - **Selection screen** ([`SelectionScreen`]): character, kart, wheels and wing, with the stat
//!   bars `(level + 3) / 4`, then the color. It opens when the game starts (with a renderer: not
//!   in the headless tests) and with **G** (« garage »: same letter and place in AZERTY and QWERTY, no Fn on a
//!   Mac). Arrows (or the player's layout's letters: Z/S and Q/D in AZERTY, W/S and A/D in
//!   QWERTY, issue #17) move through it, **Entrée** races with the choice (the level is rebuilt
//!   by `LoadLevel`), **Échap** or G closes it unchanged. While it is open the game is paused (as
//!   while the options menu of issue #17 is: `crate::options`, which takes the keys while open).
//!   Its egui interface (`ui.rs`) only exists with a renderer; the keys and the actions
//!   ([`SelectionAction`]) are headless and tested.

mod roster;
mod ui;

#[cfg(test)]
mod scenario_tests;

pub use roster::{
    CHARACTERS_RON, CharacterData, KARTS_RON, Loadout, PartData, PartKind, Roster, WHEELS_RON,
    WINGS_RON, WeightClass, is_middle,
};
pub use ui::CharacterUiPlugin;

use bevy::prelude::*;

use crate::kart::{DriverLook, KartColor, KeyboardLayout, PlayerInputMuted};
use crate::level::{LoadLevel, Online};
use crate::options::{OptionsMenu, OptionsSystems};
use crate::race::RaceMode;

/// Opens (and closes) the selection screen: « G » for garage, a letter at the same place and
/// with the same name in AZERTY and QWERTY, free in the game's controls (GD § 13), no Fn on a Mac.
pub const SELECTION_KEY: KeyCode = KeyCode::KeyG;

/// Keys of the selection screen. The directions are the driving keys of the player's keyboard
/// layout (`KeyboardLayout`, issue #17): physical positions, W/A/S/D being Z/Q/S/D on an AZERTY
/// keyboard.
const CONFIRM_KEYS: [KeyCode; 2] = [KeyCode::Enter, KeyCode::NumpadEnter];
const CANCEL_KEYS: [KeyCode; 2] = [KeyCode::Escape, SELECTION_KEY];

/// Local storage key (web) / file name (native) of the player's color (issue #55).
pub const COLOR_STORAGE_KEY: &str = "fluffy-racers/kart-color";

/// The local player's kart's color (issue #55), kept between sessions: pink until it chooses.
/// Online, it is the color it asks for: another player may have it first (`net`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Deref)]
pub struct PlayerColor(pub KartColor);

/// The local player's four parts, kept for the whole session: restarts and new levels use it.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Deref, DerefMut)]
pub struct PlayerLoadout(pub Loadout);

impl FromWorld for PlayerLoadout {
    fn from_world(world: &mut World) -> Self {
        Self(
            world
                .get_resource::<Roster>()
                .map(Roster::default_loadout)
                .unwrap_or_default(),
        )
    }
}

/// How each character's driver looks (by roster index): one shared material per character,
/// created once. Absent without a renderer's materials.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct DriverLooks(pub Vec<DriverLook>);

impl DriverLooks {
    pub fn get(&self, character: usize) -> Option<&DriverLook> {
        self.0.get(character)
    }
}

/// The selection screen: whether it is open, the line being changed (0 = character, 1 = kart,
/// 2 = wheels, 3 = wing, 4 = color) and the parts and color chosen on it (applied by
/// « Course ! »).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionScreen {
    pub open: bool,
    pub row: usize,
    pub choice: Loadout,
    /// Issue #55.
    pub color: KartColor,
}

impl SelectionScreen {
    /// The color's line (issue #55), after the parts.
    pub const COLOR_ROW: usize = PartKind::ALL.len();
    /// Lines of the screen.
    pub const ROWS: usize = Self::COLOR_ROW + 1;

    /// The part of the line being changed (the wing's on the color's line: see
    /// [`SelectionScreen::on_color_row`]).
    pub fn kind(&self) -> PartKind {
        PartKind::ALL[self.row.min(PartKind::ALL.len() - 1)]
    }

    /// The line being changed is the color's.
    pub fn on_color_row(&self) -> bool {
        self.row == Self::COLOR_ROW
    }
}

/// What the player does on the selection screen, from the keyboard or from the egui buttons.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionAction {
    /// Opens it on the current loadout.
    Open,
    /// Moves to the line above (-1) or below (+1).
    Row(i32),
    /// Picks the previous (-1) or next (+1) entry of a part.
    Change(PartKind, i32),
    /// Picks this entry of a part.
    Pick(PartKind, usize),
    /// Picks the previous (-1) or next (+1) color of the palette (issue #55).
    Color(i32),
    /// Picks this color.
    PickColor(KartColor),
    /// « Course ! »: the choice becomes the player's loadout and the race starts over with it.
    Confirm,
    /// Closes it, the loadout unchanged.
    Cancel,
}

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Roster::embedded())
            .init_resource::<PlayerLoadout>()
            .init_resource::<PlayerColor>()
            .init_resource::<SelectionScreen>()
            .add_message::<SelectionAction>()
            .add_systems(Startup, create_driver_looks)
            .add_systems(
                Update,
                (
                    // Before the options menu's keys: a key that closes the menu does not also act
                    // on this screen below it (issue #17).
                    selection_keys
                        .run_if(resource_exists::<ButtonInput<KeyCode>>)
                        .before(OptionsSystems),
                    apply_selection_actions,
                    pause_in_menus.after(OptionsSystems),
                )
                    .chain(),
            );
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            app.add_plugins(CharacterUiPlugin);
        }
    }
}

/// One material per character (its color), shared by every kart it drives.
fn create_driver_looks(
    mut commands: Commands,
    roster: Res<Roster>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let Some(mut materials) = materials else {
        return;
    };
    let looks = roster
        .characters
        .iter()
        .map(|character| {
            let (r, g, b) = character.color;
            DriverLook {
                size: character.size,
                material: materials.add(Color::srgb(r, g, b)),
                ears: character.ears,
            }
        })
        .collect();
    commands.insert_resource(DriverLooks(looks));
}

/// The keys of the selection screen (see the module's documentation). None while the options
/// menu is open over it: the keys are the menu's.
fn selection_keys(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<SelectionScreen>,
    layout: Option<Res<KeyboardLayout>>,
    options: Option<Res<OptionsMenu>>,
    mut actions: MessageWriter<SelectionAction>,
) {
    if options.is_some_and(|menu| menu.open) {
        return;
    }
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    if !screen.open {
        if keys.just_pressed(SELECTION_KEY) {
            actions.write(SelectionAction::Open);
        }
        return;
    }
    let kind = screen.kind();
    let change = |step| {
        if screen.on_color_row() {
            SelectionAction::Color(step)
        } else {
            SelectionAction::Change(kind, step)
        }
    };
    let pressed = |set: &[KeyCode]| keys.any_just_pressed(set.iter().copied());
    if pressed(&CONFIRM_KEYS) {
        actions.write(SelectionAction::Confirm);
    } else if pressed(&CANCEL_KEYS) {
        actions.write(SelectionAction::Cancel);
    } else if pressed(layout.up_keys()) {
        actions.write(SelectionAction::Row(-1));
    } else if pressed(layout.down_keys()) {
        actions.write(SelectionAction::Row(1));
    } else if pressed(layout.left_keys()) {
        actions.write(change(-1));
    } else if pressed(layout.right_keys()) {
        actions.write(change(1));
    }
}

/// Applies the actions of this frame. « Course ! » rebuilds the level (`LoadLevel`) with the new
/// loadout and color: every kart spawns again, the race starts over from the grid.
#[allow(clippy::too_many_arguments)]
fn apply_selection_actions(
    mut actions: MessageReader<SelectionAction>,
    roster: Res<Roster>,
    mode: Res<RaceMode>,
    mut screen: ResMut<SelectionScreen>,
    mut loadout: ResMut<PlayerLoadout>,
    mut color: ResMut<PlayerColor>,
    online: Option<Res<Online>>,
    mut levels: MessageWriter<LoadLevel>,
) {
    for action in actions.read() {
        match *action {
            SelectionAction::Open => {
                *screen = SelectionScreen {
                    open: true,
                    row: 0,
                    choice: loadout.0,
                    color: color.0,
                };
            }
            _ if !screen.open => {}
            SelectionAction::Row(step) => {
                let rows = SelectionScreen::ROWS as i32;
                screen.row = (screen.row as i32 + step).rem_euclid(rows) as usize;
            }
            SelectionAction::Color(step) => {
                screen.color = screen.color.cycled(step);
                screen.row = SelectionScreen::COLOR_ROW;
            }
            SelectionAction::PickColor(pick) => {
                screen.color = pick;
                screen.row = SelectionScreen::COLOR_ROW;
            }
            SelectionAction::Change(kind, step) => {
                screen.choice = roster.cycle(screen.choice, kind, step);
                screen.row = kind.index();
            }
            SelectionAction::Pick(kind, index) => {
                if index < roster.count(kind) {
                    screen.choice.set(kind, index);
                    screen.row = kind.index();
                }
            }
            SelectionAction::Confirm => {
                loadout.set_if_neq(PlayerLoadout(screen.choice));
                color.set_if_neq(PlayerColor(screen.color));
                screen.open = false;
                // Online (issue #9), the race goes on: the new kart comes with the next level
                // (the others already see it in the lobby), the new color at once (issue #55,
                // the same on every screen). Free driving restarts at once.
                if online.is_none() || *mode == RaceMode::FreeRoam {
                    levels.write(LoadLevel::restart(*mode));
                }
            }
            SelectionAction::Cancel => screen.open = false,
        }
    }
}

/// The game waits while the player chooses: virtual time is paused while the screen or the
/// options menu (issue #17) is open (the fixed 60 Hz simulation does not run), and runs again
/// once both are closed. Online (issue #9, Max's decision), no pause: the game runs on, and the
/// player's kart coasts while a menu is open (its keys are the menu's).
fn pause_in_menus(
    screen: Res<SelectionScreen>,
    options: Option<Res<OptionsMenu>>,
    online: Option<Res<Online>>,
    muted: Option<ResMut<PlayerInputMuted>>,
    mut was_open: Local<bool>,
    mut time: ResMut<Time<Virtual>>,
) {
    let open = screen.open || options.is_some_and(|menu| menu.open);
    let pause = open && online.is_none();
    if pause && !time.is_paused() {
        time.pause();
    } else if !pause && (*was_open || online.is_some()) && time.is_paused() {
        time.unpause();
    }
    if let Some(mut muted) = muted {
        muted.set_if_neq(PlayerInputMuted(open && online.is_some()));
    }
    *was_open = pause;
}
