//! Characters and karts (GD § 9, issue #15): who drives which kart, and the selection screen.
//!
//! - **Roster** ([`Roster`], `roster.rs`): 9 animals in 3 weight classes, 3 karts, 3 sets of
//!   wheels and 2 wings, read from the RON files of `assets/data/` (compiled into the game).
//! - **Loadouts**: the player's choice ([`PlayerLoadout`], kept for the whole session: a restart
//!   keeps it), and the bots' (`bots::bot_loadout`, from their grid slot). A kart's stats are the
//!   sum of its four parts (`kart::KartStats`), converted once when it spawns
//!   (`kart::kart_body_with_stats`).
//! - **Looks**: the driver's size shows its weight class. Its animal (issue #69): the
//!   character's Cube Pet (Kenney, CC0) sits in the kart and plays what its kart goes through
//!   (`model.rs`, `animation.rs`, `visual.rs`: idle, a shake of the head when hit, a dance or a
//!   nod at the finish; [`DriverTuning`]). Until it is there (headless: for good), primitives:
//!   one shared material per character ([`DriverLooks`]), its color and ears.
//! - **Soft** (issue #12): over its animation, the driver wobbles: damped springs on its body,
//!   tail and wings follow the kart with a delay (braking, turns, drifts), squash on landings and
//!   shocks, a mini-ragdoll of about 1 s when hit or struck (`wobble.rs`, pure; `wobble_visual.rs`;
//!   [`WobbleTuning`]). Purely visual, nothing sent.
//! - **Color** (issue #55): the player's kart's color ([`PlayerColor`]), a color of the palette
//!   (`kart::KartColor`), pink until it chooses. Kept between sessions like the options
//!   (`admin::storage`, key [`COLOR_STORAGE_KEY`]; with a renderer only, `ui.rs`). Online, two
//!   players of the same color: the first seat keeps it (`net`).
//! - **Selection screen** ([`SelectionScreen`]): the « Pilote » screen (issue #98, drawn by
//!   `menu::pilot` with `bevy_ui`): the pseudo, the character, the kart, the wheels, the wing and
//!   the color, the kart seen from 3/4 front with its stat bars `(level + 3) / 4`. It opens from
//!   the home screen and the menu opened from the game (« Pilote », issues #56 and #89), when the
//!   game starts with a room in its address (with a renderer: not in the headless tests) and
//!   with **G** (« garage »: same letter and place in AZERTY and QWERTY, no Fn on a Mac). Its
//!   logic is here, headless and tested: up and down (or the player's layout's letters: Z/S in
//!   AZERTY, W/S in QWERTY, issue #17) go through its lines, around ([`SelectionScreen::row`]:
//!   the pseudo's line is above the character's), left and right (Q/D or A/D) change the line's
//!   choice ([`selection_action`]), **Entrée** keeps the choice (on the pseudo's line: types in
//!   it), **Échap** or G closes it unchanged. Kept: from the game in solo, the race starts over
//!   with it (the level is rebuilt by `LoadLevel`); from the home screen, back to it (issue
//!   #98); online, for the next race (the others see it at once). While it is open the game is
//!   paused (as while the options menu of issue #17 is: `crate::options`, which takes the keys
//!   while open); online and on the home screen, never. Its view, its 3D preview, the mouse and
//!   the gamepad are `menu::pilot`'s (with a renderer only); the keys and the actions
//!   ([`SelectionAction`]) are headless.

mod animation;
mod model;
pub mod ragdoll;
mod roster;
mod ui;
mod visual;
mod wobble;
mod wobble_visual;

#[cfg(test)]
mod scenario_tests;
#[cfg(test)]
mod wobble_tests;

pub use animation::{AnimalAnimation, DriverFacts, DriverTuning, Finish, driver_animation};
pub use model::{AnimalModelData, AnimalNode, AnimalPart, animal_model_data};
pub use ragdoll::{
    Knock, Ragdoll, RagdollChange, RagdollPhase, RagdollPlugin, RagdollSystems, RagdollTuning,
};
pub use roster::{
    CHARACTERS_RON, CharacterData, KARTS_RON, Loadout, PartData, PartKind, Roster, WHEELS_RON,
    WINGS_RON, WeightClass, is_middle,
};
pub use ui::CharacterUiPlugin;
pub use visual::{
    AnimalRoot, AnimalSlot, AnimalVisual, AnimalVisualPlugin, Animals, AnimationNodes,
    DriverDressed, spawn_animal_on_foot, spawn_animal_still,
};
pub use wobble::{
    LayerPivot, SpringTuning, WobbleInput, WobbleLevers, WobblePose, WobbleState, WobbleTuning,
    layered,
};
/// Issue #101: the lobby's characters step their wobble before it is laid over their parts.
pub(crate) use wobble_visual::lay_wobbles;
pub use wobble_visual::{DriverWobble, WobbleLayer, WobblePlugin, WobbleSlot};

use bevy::prelude::*;

use crate::kart::{DriverLook, KartColor, KeyboardLayout, PlayerInputMuted};
use crate::level::{LoadLevel, Online};
use crate::menu::MainMenu;
use crate::modes::RaceMenu;
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

/// The selection screen (issue #98: the « Pilote » screen): whether it is open, the line being
/// changed (0 = character, 1 = kart, 2 = wheels, 3 = wing, 4 = color, 5 = pseudo: shown above the
/// character's, the lines go around), the parts and color chosen on it (kept by
/// [`SelectionAction::Confirm`]), and whether the pseudo's field has the keyboard.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionScreen {
    pub open: bool,
    pub row: usize,
    pub choice: Loadout,
    /// Issue #55.
    pub color: KartColor,
    /// Issue #98: the pseudo's field has the keyboard (Entrée on its line, a click in it): the
    /// game sees no key (`admin::KeyboardCapture`) until Entrée, Échap or a click elsewhere.
    pub typing: bool,
}

impl SelectionScreen {
    /// The color's line (issue #55), after the parts.
    pub const COLOR_ROW: usize = PartKind::ALL.len();
    /// Issue #98: the pseudo's line, last: on screen above the character's (the lines go around).
    pub const PSEUDO_ROW: usize = Self::COLOR_ROW + 1;
    /// Lines of the screen.
    pub const ROWS: usize = Self::PSEUDO_ROW + 1;
    /// Issue #98: the lines from the top of the screen down.
    pub const SHOWN: [usize; Self::ROWS] = [Self::PSEUDO_ROW, 0, 1, 2, 3, Self::COLOR_ROW];

    /// The part of the line being changed (the wing's on the color's and the pseudo's lines: see
    /// [`SelectionScreen::on_color_row`], [`SelectionScreen::on_pseudo_row`]).
    pub fn kind(&self) -> PartKind {
        PartKind::ALL[self.row.min(PartKind::ALL.len() - 1)]
    }

    /// The part of the line being changed, if it is a part's.
    pub fn part(&self) -> Option<PartKind> {
        PartKind::ALL.get(self.row).copied()
    }

    /// The line being changed is the color's.
    pub fn on_color_row(&self) -> bool {
        self.row == Self::COLOR_ROW
    }

    /// Issue #98: the line being changed is the pseudo's.
    pub fn on_pseudo_row(&self) -> bool {
        self.row == Self::PSEUDO_ROW
    }
}

/// What the player does on the selection screen, from the keyboard, the gamepad or the mouse
/// (`menu::pilot`).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionAction {
    /// Opens it on the current loadout.
    Open,
    /// Moves to the line above (-1) or below (+1).
    Row(i32),
    /// Issue #98: this line has the focus (the pointer over it).
    Focus(usize),
    /// Picks the previous (-1) or next (+1) entry of a part.
    Change(PartKind, i32),
    /// Picks this entry of a part.
    Pick(PartKind, usize),
    /// Picks the previous (-1) or next (+1) color of the palette (issue #55).
    Color(i32),
    /// Picks this color.
    PickColor(KartColor),
    /// Issue #98: the pseudo's field takes the keyboard...
    Type,
    /// ...and gives it back.
    Typed,
    /// Keeps the choice: it becomes the player's loadout and color (the race starts over with it
    /// from the game in solo).
    Confirm,
    /// Closes it, the loadout unchanged.
    Cancel,
}

/// Issue #98: what the player asks the selection screen, whatever the device (the keys, the
/// gamepad's cross and stick, A, B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionInput {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
}

/// What `input` does on `screen` (issue #98): up and down go through the lines, left and right
/// change the choice of the line (nothing on the pseudo's: it is typed), Entrée keeps the choice
/// (on the pseudo's line: types in it), Échap leaves.
pub fn selection_action(
    screen: &SelectionScreen,
    input: SelectionInput,
) -> Option<SelectionAction> {
    let change = |step| {
        if screen.on_pseudo_row() {
            None
        } else if screen.on_color_row() {
            Some(SelectionAction::Color(step))
        } else {
            Some(SelectionAction::Change(screen.kind(), step))
        }
    };
    match input {
        SelectionInput::Up => Some(SelectionAction::Row(-1)),
        SelectionInput::Down => Some(SelectionAction::Row(1)),
        SelectionInput::Left => change(-1),
        SelectionInput::Right => change(1),
        SelectionInput::Confirm if screen.on_pseudo_row() => Some(SelectionAction::Type),
        SelectionInput::Confirm => Some(SelectionAction::Confirm),
        SelectionInput::Back => Some(SelectionAction::Cancel),
    }
}

pub struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Roster::embedded())
            .init_resource::<DriverTuning>()
            .add_plugins((AnimalVisualPlugin, WobblePlugin))
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
        .enumerate()
        .map(|(index, character)| {
            let (r, g, b) = character.color;
            DriverLook {
                character: index,
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
    race_menu: Option<Res<RaceMenu>>,
    mut actions: MessageWriter<SelectionAction>,
) {
    // Issue #19: nor while the race menu (N) is open: the keys are its.
    if options.is_some_and(|menu| menu.open) || race_menu.is_some_and(|menu| menu.open) {
        return;
    }
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    if !screen.open {
        if keys.just_pressed(SELECTION_KEY) {
            actions.write(SelectionAction::Open);
        }
        return;
    }
    let pressed = |set: &[KeyCode]| keys.any_just_pressed(set.iter().copied());
    let input = if pressed(&CONFIRM_KEYS) {
        SelectionInput::Confirm
    } else if pressed(&CANCEL_KEYS) {
        SelectionInput::Back
    } else if pressed(layout.up_keys()) {
        SelectionInput::Up
    } else if pressed(layout.down_keys()) {
        SelectionInput::Down
    } else if pressed(layout.left_keys()) {
        SelectionInput::Left
    } else if pressed(layout.right_keys()) {
        SelectionInput::Right
    } else {
        return;
    };
    if let Some(action) = selection_action(&screen, input) {
        actions.write(action);
    }
}

/// Applies the actions of this frame. « Course ! » (Entrée) rebuilds the level (`LoadLevel`) with
/// the new loadout and color: every kart spawns again, the race starts over from the grid. Issue
/// #98: not from the home screen (its background races on; the screen goes back to it), nor
/// online in a race (the next one has it).
#[allow(clippy::too_many_arguments)]
pub fn apply_selection_actions(
    mut actions: MessageReader<SelectionAction>,
    roster: Res<Roster>,
    mode: Res<RaceMode>,
    mut screen: ResMut<SelectionScreen>,
    mut loadout: ResMut<PlayerLoadout>,
    mut color: ResMut<PlayerColor>,
    (online, main_menu): (Option<Res<Online>>, Option<Res<MainMenu>>),
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
                    typing: false,
                };
            }
            _ if !screen.open => {}
            SelectionAction::Row(step) => {
                let rows = SelectionScreen::ROWS as i32;
                screen.row = (screen.row as i32 + step).rem_euclid(rows) as usize;
            }
            SelectionAction::Focus(row) => {
                // The pointer passing over a line does not take the keyboard from the field.
                if row < SelectionScreen::ROWS && !screen.typing {
                    screen.row = row;
                }
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
            SelectionAction::Type => {
                screen.typing = true;
                screen.row = SelectionScreen::PSEUDO_ROW;
            }
            SelectionAction::Typed => screen.typing = false,
            SelectionAction::Confirm => {
                loadout.set_if_neq(PlayerLoadout(screen.choice));
                color.set_if_neq(PlayerColor(screen.color));
                screen.open = false;
                screen.typing = false;
                // Online (issue #9), the race goes on: the new kart comes with the next level
                // (the others already see it in the lobby), the new color at once (issue #55,
                // the same on every screen). Free driving restarts at once. Issue #98: on the home
                // screen, its background races on.
                let home = main_menu.as_ref().is_some_and(|menu| menu.on_home());
                if !home && (online.is_none() || *mode == RaceMode::FreeRoam) {
                    levels.write(LoadLevel::restart(*mode));
                }
            }
            SelectionAction::Cancel => {
                screen.open = false;
                screen.typing = false;
            }
        }
    }
}

/// The game waits while the player chooses: virtual time is paused while the screen, the
/// options menu (issue #17), the race menu (issue #19) or the main menu (issue #56, with the
/// screen it is opening) is open (the fixed 60 Hz simulation does not run), and runs again once
/// all are closed. Online (issue #9, Max's decision), no pause: the game runs on, and the
/// player's kart coasts while a menu is open (its keys are the menu's). Issue #89: never on the
/// home screen, nor over it: its background is the game, running.
#[allow(clippy::too_many_arguments)]
fn pause_in_menus(
    screen: Res<SelectionScreen>,
    options: Option<Res<OptionsMenu>>,
    race_menu: Option<Res<RaceMenu>>,
    main_menu: Option<Res<MainMenu>>,
    online: Option<Res<Online>>,
    muted: Option<ResMut<PlayerInputMuted>>,
    mut was_open: Local<bool>,
    mut time: ResMut<Time<Virtual>>,
) {
    // Issue #89: the home screen's background (its karts) runs behind it, and behind the screens
    // it opens.
    let home = main_menu.as_ref().is_some_and(|menu| menu.on_home());
    let open = !home
        && (screen.open
            || options.is_some_and(|menu| menu.open)
            || race_menu.is_some_and(|menu| menu.open)
            || main_menu.is_some_and(|menu| menu.holds_the_game()));
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
