//! The main menu's entries and its navigation (issue #56), pure: what each entry is, which
//! entries a menu has, how the focus moves (around, over the disabled entries), and the gamepad's
//! stick turned into single steps.

use crate::kart::KeyboardLayout;

/// An entry of the main menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Entry {
    /// Back to the race (only when the menu was opened from the game).
    Resume,
    /// « Course solo »: the race menu of issue #19 (N): free race, Grand Prix or (issue #76) time
    /// trial, circuit, class.
    Solo,
    /// « En ligne »: the lobby of issue #7 (L): create or join a game.
    Online,
    /// « Options »: the options menu of issue #17 (O).
    Options,
    /// « Pilote et kart »: the selection screen of issue #15 (G).
    Garage,
}

impl Entry {
    pub fn label(self) -> &'static str {
        match self {
            Entry::Resume => "Reprendre",
            Entry::Solo => "Course solo",
            Entry::Online => "En ligne",
            Entry::Options => "Options",
            Entry::Garage => "Pilote et kart",
        }
    }

    /// The small line under the label: what it opens (`docs/UI_CHARTE.md` « Ton des textes »).
    pub fn caption(self) -> &'static str {
        match self {
            Entry::Resume => "Le kart t'attend, moteur chaud.",
            // Issue #76: and the time trial (≤ 48 characters: the test below).
            Entry::Solo => "Course libre, Grand Prix ou contre-la-montre",
            Entry::Online => "Créer ou rejoindre une partie entre amis",
            Entry::Options => "Clavier, noms au-dessus des karts",
            Entry::Garage => "Ton animal, ton kart, tes roues, ton aile",
        }
    }

    /// Why it cannot be used now, under its label instead of the caption, when it is disabled.
    pub fn why_disabled(self) -> &'static str {
        match self {
            Entry::Solo => "En ligne, c'est l'hôte qui choisit la course.",
            _ => "",
        }
    }

    /// The shortcut that opens the same screen during the game, shown in the options' list.
    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            Entry::Resume => None,
            Entry::Solo => Some("N"),
            Entry::Online => Some("L"),
            Entry::Options => Some("O"),
            Entry::Garage => Some("G"),
        }
    }
}

/// The entries of the menu at launch: what to play first.
pub const LAUNCH_ENTRIES: [Entry; 4] = [Entry::Solo, Entry::Online, Entry::Options, Entry::Garage];
/// The entries of the menu opened from the game (Échap, Start): « Reprendre » first.
pub const GAME_ENTRIES: [Entry; 5] = [
    Entry::Resume,
    Entry::Solo,
    Entry::Online,
    Entry::Options,
    Entry::Garage,
];

/// The entries of the menu, at launch or opened from the game.
pub fn entries(in_game: bool) -> &'static [Entry] {
    if in_game {
        &GAME_ENTRIES
    } else {
        &LAUNCH_ENTRIES
    }
}

/// Whether `entry` can be used: online, the host chooses the race (the race menu does nothing
/// there, `modes`).
pub fn enabled(entry: Entry, online: bool) -> bool {
    !(entry == Entry::Solo && online)
}

/// The first entry that can be used (0 if none can).
pub fn first_enabled(entries: &[Entry], online: bool) -> usize {
    entries
        .iter()
        .position(|entry| enabled(*entry, online))
        .unwrap_or(0)
}

/// The focus `step` entries away from `focus` (`step` < 0: up), around the menu, over the
/// disabled entries. Stays if no other entry can be used.
pub fn moved(entries: &[Entry], focus: usize, step: i32, online: bool) -> usize {
    let count = entries.len() as i32;
    if count == 0 || step == 0 {
        return focus;
    }
    let direction = step.signum();
    let mut at = focus as i32;
    for _ in 0..step.abs() {
        for _ in 0..count {
            at = (at + direction).rem_euclid(count);
            if enabled(entries[at as usize], online) {
                break;
            }
        }
    }
    if enabled(entries[at as usize], online) {
        at as usize
    } else {
        focus
    }
}

/// The line of keys at the bottom of the menu, with the letters of the player's keyboard.
pub fn hint(layout: KeyboardLayout, in_game: bool) -> [String; 2] {
    let [up, _, down, _] = layout.letters();
    let back = if in_game {
        "reprendre"
    } else {
        "rouler tout de suite"
    };
    [
        format!("Haut / Bas ({up} / {down}) : choisir  ·  Entrée : valider  ·  Échap : {back}"),
        format!("Manette : croix ou stick, A : valider, B ou Start : {back}"),
    ]
}

/// The gamepad's left stick as single steps up and down: a step when it is pushed past
/// [`StickLatch::PUSH`], and the next one only once it came back under [`StickLatch::RELEASE`]
/// (the stick held does not run through the menu).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StickLatch {
    held: bool,
}

impl StickLatch {
    pub const PUSH: f32 = 0.6;
    pub const RELEASE: f32 = 0.3;

    /// The step for the stick's vertical position `y` this frame (up is positive): -1 (up), 1
    /// (down) or 0.
    pub fn step(&mut self, y: f32) -> i32 {
        if self.held {
            if y.abs() < Self::RELEASE {
                self.held = false;
            }
            return 0;
        }
        if y.abs() >= Self::PUSH {
            self.held = true;
            return if y > 0.0 { -1 } else { 1 };
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_has_reprendre_only_from_the_game() {
        assert_eq!(entries(false), &LAUNCH_ENTRIES);
        assert_eq!(entries(true)[0], Entry::Resume);
        assert_eq!(&entries(true)[1..], &LAUNCH_ENTRIES);
        for entry in GAME_ENTRIES {
            assert!(!entry.label().is_empty() && !entry.caption().is_empty());
            assert!(
                entry.label().chars().count() <= 16,
                "short: {}",
                entry.label()
            );
            assert!(
                entry.caption().chars().count() <= 48,
                "short: {}",
                entry.caption()
            );
        }
        // The shortcuts are those of the screens they open.
        assert_eq!(Entry::Solo.shortcut(), Some("N"));
        assert_eq!(Entry::Online.shortcut(), Some("L"));
        assert_eq!(Entry::Options.shortcut(), Some("O"));
        assert_eq!(Entry::Garage.shortcut(), Some("G"));
    }

    /// Up and down go around the menu, and skip what cannot be used: online, « Course solo ».
    #[test]
    fn the_focus_goes_around_and_skips_the_disabled_entries() {
        let menu = entries(false);
        assert_eq!(first_enabled(menu, false), 0);
        assert_eq!(moved(menu, 0, 1, false), 1);
        assert_eq!(moved(menu, 3, 1, false), 0, "around, down");
        assert_eq!(moved(menu, 0, -1, false), 3, "around, up");
        assert_eq!(moved(menu, 1, 2, false), 3);
        assert_eq!(moved(menu, 2, 0, false), 2);

        // Online: « Course solo » is skipped.
        assert!(!enabled(Entry::Solo, true));
        assert!(enabled(Entry::Online, true) && enabled(Entry::Resume, true));
        assert_eq!(first_enabled(menu, true), 1);
        assert_eq!(moved(menu, 3, 1, true), 1);
        assert_eq!(moved(menu, 1, -1, true), 3);
        let game = entries(true);
        assert_eq!(first_enabled(game, true), 0);
        assert_eq!(moved(game, 0, 1, true), 2, "over « Course solo »");
        assert_eq!(moved(game, 2, -1, true), 0);
        assert!(!Entry::Solo.why_disabled().is_empty());

        // Nothing else to go to: it stays.
        assert_eq!(moved(&[Entry::Options], 0, 1, false), 0);
        assert_eq!(moved(&[Entry::Solo, Entry::Options], 1, 1, true), 1);
        assert_eq!(moved(&[], 0, 1, false), 0);
    }

    #[test]
    fn the_keys_line_shows_the_letters_of_the_keyboard() {
        let [keys, pad] = hint(KeyboardLayout::Azerty, true);
        assert!(keys.starts_with("Haut / Bas (Z / S)"), "{keys}");
        assert!(keys.contains("Échap : reprendre"), "{keys}");
        assert!(pad.contains("A : valider"), "{pad}");
        let [keys, _] = hint(KeyboardLayout::Qwerty, false);
        assert!(keys.starts_with("Haut / Bas (W / S)"), "{keys}");
        assert!(keys.contains("Échap : rouler tout de suite"), "{keys}");
    }

    /// The stick held does not run through the menu: one step per push.
    #[test]
    fn the_stick_moves_one_step_per_push() {
        let mut latch = StickLatch::default();
        assert_eq!(latch.step(0.0), 0);
        assert_eq!(latch.step(0.5), 0, "not far enough");
        assert_eq!(latch.step(0.8), -1, "up");
        for _ in 0..30 {
            assert_eq!(latch.step(0.9), 0, "held");
        }
        assert_eq!(latch.step(0.4), 0, "not back yet");
        assert_eq!(latch.step(0.1), 0, "back");
        assert_eq!(latch.step(-0.7), 1, "down");
        assert_eq!(latch.step(-0.2), 0);
        assert_eq!(latch.step(-0.65), 1, "down again");
    }
}
