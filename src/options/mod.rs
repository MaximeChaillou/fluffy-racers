//! The player's options (issue #17, GD § 13): for now the keyboard layout, in a small menu kept
//! in the browser between sessions. Made to take more options later: a field of
//! [`PlayerOptions`] (with its default) and a line of [`OptionRow`].
//!
//! - **What**: [`PlayerOptions`]: the keyboard layout (AZERTY by default: the game is in
//!   French), applied to the keyboard input (`KeyboardLayout`: the letters shown, and Z and Q
//!   read in AZERTY). The driving aids of GD § 11 are not planned for now (Max, 2026-10-13).
//! - **Menu** ([`OptionsMenu`]): **O** opens and closes it (« options »: the same letter at the
//!   same place in AZERTY and QWERTY, free in the game's controls, no Fn on a Mac), from the race
//!   or from the selection screen. Up and down (arrows, or Z/S in AZERTY and W/S in QWERTY): the
//!   line; left and right (or Q/D, A/D), Entrée or Espace: change it; **Échap** or O closes it. A
//!   change applies at once, without restarting the race. The game is paused while it is open,
//!   and it lists the controls with the letters of the chosen layout.
//! - **Kept**: in the browser's local storage (`fluffy-racers/options`), or a file of the user's
//!   configuration folder natively, like the admin panel's tunings (`admin::storage`), as a small
//!   RON text ([`PlayerOptions::to_ron`]).
//!
//! This plugin is the logic, headless (the tests drive it with keys and [`OptionsAction`]s). The
//! egui window and the storage (`ui.rs`) only come with a renderer; like the admin panel and the
//! selection screen, the window costs nothing while closed (no egui context then).

mod ui;

pub use ui::OptionsUiPlugin;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::kart::KeyboardLayout;

/// Opens and closes the options menu: « O » for options, a letter at the same place and with the
/// same name in AZERTY and QWERTY, free in the game's controls (GD § 13), no Fn on a Mac.
pub const OPTIONS_KEY: KeyCode = KeyCode::KeyO;

/// Local storage key (web) / file name (native) of the options.
pub const OPTIONS_STORAGE_KEY: &str = "fluffy-racers/options";

/// Keys of the menu that are not directions: change the line, close it.
const CHANGE_KEYS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space];
const CLOSE_KEYS: [KeyCode; 2] = [KeyCode::Escape, OPTIONS_KEY];

/// The local player's options (see the module's documentation). Missing fields in a stored text
/// take their default: an option added later does not lose the others.
#[derive(Resource, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct PlayerOptions {
    /// The keyboard: AZERTY or QWERTY.
    pub keyboard: KeyboardLayout,
}

impl PlayerOptions {
    /// The text kept in the browser: RON, every field named.
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|_| String::new())
    }

    /// Reads a kept text; a missing field takes its default. An unreadable text (not RON, an
    /// unknown layout...) gives the reason.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|error| error.to_string())
    }

    /// The value of a line, as shown.
    pub fn value(&self, row: OptionRow) -> &'static str {
        match row {
            OptionRow::Keyboard => self.keyboard.label(),
        }
    }

    /// The options with the line `row` changed by `step` values (`step` < 0: backwards, around).
    pub fn changed(mut self, row: OptionRow, step: i32) -> Self {
        match row {
            OptionRow::Keyboard => {
                let all = KeyboardLayout::ALL;
                let at = all.iter().position(|l| *l == self.keyboard).unwrap_or(0) as i32;
                self.keyboard = all[(at + step).rem_euclid(all.len() as i32) as usize];
            }
        }
        self
    }
}

/// The lines of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionRow {
    Keyboard,
}

impl OptionRow {
    pub const ALL: [Self; 1] = [OptionRow::Keyboard];

    pub fn label(self) -> &'static str {
        match self {
            OptionRow::Keyboard => "Clavier",
        }
    }

    /// What it does, under the line.
    pub fn help(self) -> &'static str {
        match self {
            OptionRow::Keyboard => {
                "Les touches restent à la même place sous les doigts ; \
                 les lettres affichées suivent ton clavier."
            }
        }
    }
}

/// The options menu: whether it is open, and the line being changed.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OptionsMenu {
    pub open: bool,
    pub row: usize,
}

impl OptionsMenu {
    pub fn row(&self) -> OptionRow {
        OptionRow::ALL[self.row.min(OptionRow::ALL.len() - 1)]
    }
}

/// What the player does with the menu, from the keyboard or the egui window.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionsAction {
    /// Opens it, or closes it if it is open (O).
    Toggle,
    Close,
    /// Moves to the line above (-1) or below (+1).
    Row(i32),
    /// Changes a line by one value, backwards (-1) or forwards (+1).
    Change(OptionRow, i32),
    /// Picks a keyboard layout.
    Keyboard(KeyboardLayout),
}

/// The options' systems, in `Update`: keys, actions, then the options applied. The selection
/// screen reads its keys before them (`characters`): a key that closes the menu does not also
/// act on the screen below it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct OptionsSystems;

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerOptions>()
            .init_resource::<OptionsMenu>()
            .init_resource::<KeyboardLayout>()
            .add_message::<OptionsAction>()
            .add_systems(
                Update,
                (
                    options_keys.run_if(resource_exists::<ButtonInput<KeyCode>>),
                    apply_options_actions,
                    apply_player_options,
                )
                    .chain()
                    .in_set(OptionsSystems),
            );
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            app.add_plugins(OptionsUiPlugin);
        }
    }
}

/// The keys of the menu (see the module's documentation).
fn options_keys(
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<OptionsMenu>,
    layout: Res<KeyboardLayout>,
    mut actions: MessageWriter<OptionsAction>,
) {
    if !menu.open {
        if keys.just_pressed(OPTIONS_KEY) {
            actions.write(OptionsAction::Toggle);
        }
        return;
    }
    let pressed = |set: &[KeyCode]| keys.any_just_pressed(set.iter().copied());
    let row = menu.row();
    if pressed(&CLOSE_KEYS) {
        actions.write(OptionsAction::Close);
    } else if pressed(layout.up_keys()) {
        actions.write(OptionsAction::Row(-1));
    } else if pressed(layout.down_keys()) {
        actions.write(OptionsAction::Row(1));
    } else if pressed(layout.left_keys()) {
        actions.write(OptionsAction::Change(row, -1));
    } else if pressed(layout.right_keys()) || pressed(&CHANGE_KEYS) {
        actions.write(OptionsAction::Change(row, 1));
    }
}

fn apply_options_actions(
    mut actions: MessageReader<OptionsAction>,
    mut menu: ResMut<OptionsMenu>,
    mut options: ResMut<PlayerOptions>,
) {
    for action in actions.read() {
        match *action {
            OptionsAction::Toggle => {
                *menu = OptionsMenu {
                    open: !menu.open,
                    row: 0,
                };
            }
            OptionsAction::Close => menu.open = false,
            _ if !menu.open => {}
            OptionsAction::Row(step) => {
                let rows = OptionRow::ALL.len() as i32;
                menu.row = (menu.row as i32 + step).rem_euclid(rows) as usize;
            }
            OptionsAction::Change(row, step) => {
                options.set_if_neq(options.changed(row, step));
                if let Some(index) = OptionRow::ALL.iter().position(|r| *r == row) {
                    menu.row = index;
                }
            }
            OptionsAction::Keyboard(layout) => {
                if options.keyboard != layout {
                    options.keyboard = layout;
                }
            }
        }
    }
}

/// The options apply at once: the keyboard layout of the keyboard input.
fn apply_player_options(options: Res<PlayerOptions>, mut layout: ResMut<KeyboardLayout>) {
    layout.set_if_neq(options.keyboard);
}

/// The controls (GD § 13) with the letters of `layout`: (action, keys), for the menu.
pub fn controls(layout: KeyboardLayout) -> [(&'static str, String); 12] {
    let [up, left, down, right] = layout.letters();
    [
        ("Accélérer", format!("Haut ou {up}")),
        ("Freiner / reculer", format!("Bas ou {down}")),
        ("Diriger", format!("Gauche / Droite, ou {left} / {right}")),
        ("Saut / drift / figure", "Espace".to_string()),
        (
            "Objet (maintenir : traîner)",
            "Shift ou E ; avec Bas, S ou C : derrière".to_string(),
        ),
        (
            "Interaction (appui : coup de patte ; tenir : attraper)",
            "X ou F".to_string(),
        ),
        ("Regarder derrière", "C (tenir)".to_string()),
        ("Pilote et kart", "G".to_string()),
        (
            "Nouvelle course (circuit, Grand Prix, classe, miroir)",
            "N".to_string(),
        ),
        ("Options", "O".to_string()),
        ("En ligne (partie entre amis)", "L".to_string()),
        ("Panneau d'admin", "P".to_string()),
    ]
}

#[cfg(test)]
mod scenario_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::{DEBUG_RESET_KEY, INTERACT_KEYS, ITEM_KEYS, JUMP_KEYS, LOOK_BACK_KEYS};

    /// Issue #17: the options go to the browser and come back the same.
    #[test]
    fn options_survive_a_round_trip_through_the_storage_text() {
        for keyboard in KeyboardLayout::ALL {
            let options = PlayerOptions { keyboard };
            let text = options.to_ron();
            assert_eq!(PlayerOptions::from_ron(&text), Ok(options), "{text}");
        }
        let text = PlayerOptions {
            keyboard: KeyboardLayout::Qwerty,
        }
        .to_ron();
        assert!(text.contains("keyboard: Qwerty"), "{text}");
    }

    /// A text from another version (a field missing, one unknown) keeps what it can read and
    /// takes the default for the rest; an unreadable one is refused with its reason (the game
    /// then keeps the defaults).
    #[test]
    fn missing_options_take_their_default_and_bad_texts_are_refused() {
        assert_eq!(PlayerOptions::from_ron("()"), Ok(PlayerOptions::default()));
        assert_eq!(
            PlayerOptions::from_ron("(keyboard: Qwerty)"),
            Ok(PlayerOptions {
                keyboard: KeyboardLayout::Qwerty
            })
        );
        for bad in ["", "n'importe quoi", "(keyboard: Dvorak)", "(keyboard: 3)"] {
            assert!(PlayerOptions::from_ron(bad).is_err(), "{bad}");
        }
        assert_eq!(PlayerOptions::default().keyboard, KeyboardLayout::Azerty);
    }

    #[test]
    fn the_keyboard_line_goes_around_the_layouts() {
        let options = PlayerOptions::default();
        let row = OptionRow::Keyboard;
        let qwerty = options.changed(row, 1);
        assert_eq!(qwerty.keyboard, KeyboardLayout::Qwerty);
        assert_eq!(qwerty.value(row), "QWERTY");
        assert_eq!(options.value(row), "AZERTY");
        assert_eq!(qwerty.changed(row, 1), options);
        assert_eq!(options.changed(row, -1), qwerty);
        assert_eq!(options.changed(row, 0), options);
    }

    /// Issue #17: in both layouts, no key does two things: the driving keys, the item, look
    /// behind, the Interaction keys of GD § 15 (X, F, issue #34), and the keys of the menus and
    /// of debugging. The menu keys are the same letter at the same place in AZERTY and QWERTY.
    #[test]
    fn no_key_does_two_things_in_either_layout() {
        // Issue #34: the keys the game reads for the Interaction button, X and F (GD § 13).
        assert_eq!(INTERACT_KEYS, [KeyCode::KeyX, KeyCode::KeyF]);
        let menus = [
            OPTIONS_KEY,
            crate::characters::SELECTION_KEY,
            // Issue #19: the race menu.
            crate::modes::RACE_MENU_KEY,
            crate::admin::ADMIN_PANEL_KEY,
            // Issue #7: the lobby window.
            crate::net::LOBBY_KEY,
            crate::ui::DEBUG_OVERLAY_KEY,
            crate::kart::DEBUG_BOOST_KEY,
            DEBUG_RESET_KEY,
            // Issue #18: an item in the hand.
            crate::items::DEBUG_ITEM_KEY,
            KeyCode::Escape,
            KeyCode::Enter,
        ];
        for layout in KeyboardLayout::ALL {
            let actions: [&[KeyCode]; 9] = [
                layout.up_keys(),
                layout.down_keys(),
                layout.left_keys(),
                layout.right_keys(),
                &JUMP_KEYS,
                &ITEM_KEYS,
                &LOOK_BACK_KEYS,
                &INTERACT_KEYS,
                &menus,
            ];
            let mut all: Vec<KeyCode> = actions
                .iter()
                .flat_map(|keys| keys.iter())
                .copied()
                .collect();
            let count = all.len();
            all.sort_by_key(|key| format!("{key:?}"));
            all.dedup();
            assert_eq!(all.len(), count, "{layout:?}: a key is used twice");
        }
        assert_eq!(
            controls(KeyboardLayout::Azerty)[0].1,
            "Haut ou Z",
            "AZERTY shows Z/Q/S/D"
        );
        assert_eq!(
            controls(KeyboardLayout::Qwerty)[2].1,
            "Gauche / Droite, ou A / D"
        );
        // Issue #34: the menu shows the Interaction keys.
        for layout in KeyboardLayout::ALL {
            assert!(
                controls(layout)
                    .iter()
                    .any(|(action, keys)| action.starts_with("Interaction") && keys == "X ou F")
            );
        }
    }
}
