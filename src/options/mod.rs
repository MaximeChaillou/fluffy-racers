//! The player's options (issue #17, GD § 13): the keyboard layout and (issue #80) the names over
//! the karts online, in a small menu kept in the browser between sessions. Made to take more
//! options later: a field of [`PlayerOptions`] (with its default) and a line of [`OptionRow`].
//!
//! - **What**: [`PlayerOptions`]: the keyboard layout (AZERTY by default: the game is in
//!   French), applied to the keyboard input (`KeyboardLayout`: the letters shown, and Z and Q
//!   read in AZERTY); the other players' pseudos over their karts online (on by default,
//!   `net::tags`). The driving aids of GD § 11 are not planned for now (Max, 2026-10-13).
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
#[derive(Resource, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct PlayerOptions {
    /// The keyboard: AZERTY or QWERTY.
    pub keyboard: KeyboardLayout,
    /// Issue #80: online, the other players' pseudos over their karts (on by default).
    pub name_tags: bool,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            keyboard: KeyboardLayout::default(),
            name_tags: true,
        }
    }
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
            OptionRow::NameTags => on_off(self.name_tags),
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
            // Two values: an odd step changes it.
            OptionRow::NameTags => self.name_tags ^= step.rem_euclid(2) == 1,
        }
        self
    }
}

/// « Oui » or « Non ».
pub fn on_off(on: bool) -> &'static str {
    if on { "Oui" } else { "Non" }
}

/// The lines of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionRow {
    Keyboard,
    /// Issue #80.
    NameTags,
}

impl OptionRow {
    pub const ALL: [Self; 2] = [OptionRow::Keyboard, OptionRow::NameTags];

    pub fn label(self) -> &'static str {
        match self {
            OptionRow::Keyboard => "Clavier",
            OptionRow::NameTags => "Noms au-dessus des karts",
        }
    }

    /// What it does, under the line.
    pub fn help(self) -> &'static str {
        match self {
            OptionRow::Keyboard => {
                "Les touches restent à la même place sous les doigts ; \
                 les lettres affichées suivent ton clavier."
            }
            OptionRow::NameTags => {
                "En ligne : le pseudo des autres joueurs au-dessus de leur kart."
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
    /// Issue #80: shows or hides the names over the karts.
    NameTags(bool),
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
            OptionsAction::NameTags(on) => {
                if options.name_tags != on {
                    options.name_tags = on;
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
pub fn controls(layout: KeyboardLayout) -> [(&'static str, String); 13] {
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
        // Issue #56.
        (
            "Menu principal (pause en solo)",
            "Échap ; manette : Start".to_string(),
        ),
        ("Pilote (pseudo, animal, kart, couleur)", "G".to_string()),
        (
            "Nouvelle course (circuit, Grand Prix, contre-la-montre, classe, miroir)",
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

    /// Issue #17: the options go to the browser and come back the same (issue #80: the names
    /// over the karts too).
    #[test]
    fn options_survive_a_round_trip_through_the_storage_text() {
        for keyboard in KeyboardLayout::ALL {
            for name_tags in [true, false] {
                let options = PlayerOptions {
                    keyboard,
                    name_tags,
                };
                let text = options.to_ron();
                assert_eq!(PlayerOptions::from_ron(&text), Ok(options), "{text}");
            }
        }
        let text = PlayerOptions {
            keyboard: KeyboardLayout::Qwerty,
            name_tags: false,
        }
        .to_ron();
        assert!(text.contains("keyboard: Qwerty"), "{text}");
        assert!(text.contains("name_tags: false"), "{text}");
    }

    /// A text from another version (a field missing, one unknown) keeps what it can read and
    /// takes the default for the rest; an unreadable one is refused with its reason (the game
    /// then keeps the defaults).
    #[test]
    fn missing_options_take_their_default_and_bad_texts_are_refused() {
        assert_eq!(PlayerOptions::from_ron("()"), Ok(PlayerOptions::default()));
        // A text from before issue #80 (no `name_tags`): the names are shown.
        assert_eq!(
            PlayerOptions::from_ron("(keyboard: Qwerty)"),
            Ok(PlayerOptions {
                keyboard: KeyboardLayout::Qwerty,
                name_tags: true,
            })
        );
        assert_eq!(
            PlayerOptions::from_ron("(name_tags: false)"),
            Ok(PlayerOptions {
                keyboard: KeyboardLayout::Azerty,
                name_tags: false,
            })
        );
        for bad in [
            "",
            "n'importe quoi",
            "(keyboard: Dvorak)",
            "(keyboard: 3)",
            "(name_tags: 3)",
            "(name_tags: oui)",
        ] {
            assert!(PlayerOptions::from_ron(bad).is_err(), "{bad}");
        }
        assert_eq!(PlayerOptions::default().keyboard, KeyboardLayout::Azerty);
        assert!(
            PlayerOptions::default().name_tags,
            "issue #80: on by default"
        );
    }

    /// Issue #80: the line of the names over the karts goes from « Oui » to « Non » and back,
    /// whatever the direction.
    #[test]
    fn the_name_tags_line_turns_them_on_and_off() {
        let options = PlayerOptions::default();
        let row = OptionRow::NameTags;
        assert_eq!(options.value(row), "Oui");
        let off = options.changed(row, 1);
        assert!(!off.name_tags);
        assert_eq!(off.value(row), "Non");
        assert_eq!(off.keyboard, options.keyboard, "only that line changes");
        assert_eq!(off.changed(row, 1), options);
        assert_eq!(options.changed(row, -1), off);
        assert_eq!(options.changed(row, 2), options);
        assert_eq!(options.changed(row, 0), options);
        assert_eq!(OptionRow::ALL, [OptionRow::Keyboard, OptionRow::NameTags]);
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
    /// of debugging (issue #56: Échap, the main menu's). The menu keys are the same letter at the
    /// same place in AZERTY and QWERTY.
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
            // Issue #56: Échap opens the main menu (and closes the other screens, as before).
            crate::menu::MAIN_MENU_KEY,
            KeyCode::Enter,
        ];
        assert_eq!(crate::menu::MAIN_MENU_KEY, KeyCode::Escape);
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
        // Issue #34: the menu shows the Interaction keys; issue #56, the main menu's.
        for layout in KeyboardLayout::ALL {
            assert!(
                controls(layout)
                    .iter()
                    .any(|(action, keys)| action.starts_with("Interaction") && keys == "X ou F")
            );
            assert!(
                controls(layout)
                    .iter()
                    .any(|(action, keys)| action.starts_with("Menu principal")
                        && keys.starts_with("Échap"))
            );
        }
    }
}
