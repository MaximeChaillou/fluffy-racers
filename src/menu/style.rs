//! The charter of the game's screens in code (issue #56, `docs/UI_CHARTE.md`): the palette, the
//! sizes, the look of a button in each of its states, and the files of the interface (the font,
//! the icons). Pure: the tree of `screen.rs` and the tests read it, nothing here touches Bevy's
//! world.
//!
//! Sizes are given in pixels of a 720 px high window ([`REFERENCE_HEIGHT`]) and drawn in
//! percents of the window's height ([`u`], [`font`]): the screens keep their proportions at every
//! size of the window, from a small laptop to a television.

use bevy::prelude::*;
use bevy::text::FontSize;

/// The height the sizes are given for (px).
pub const REFERENCE_HEIGHT: f32 = 720.0;

/// A length of `px` pixels of the reference window, in percents of the window's height.
pub fn u(px: f32) -> Val {
    Val::Vh(px * 100.0 / REFERENCE_HEIGHT)
}

/// A font size of `px` pixels of the reference window.
pub fn font(px: f32) -> FontSize {
    FontSize::Vh(px * 100.0 / REFERENCE_HEIGHT)
}

/// A color from its `#rrggbb` code (sRGB).
const fn hex(code: u32) -> Color {
    Color::srgb(
        ((code >> 16) & 0xff) as f32 / 255.0,
        ((code >> 8) & 0xff) as f32 / 255.0,
        (code & 0xff) as f32 / 255.0,
    )
}

/// The palette (`docs/UI_CHARTE.md`, « Palette »): candy colors, a dark blueberry ink.
pub mod palette {
    use super::*;

    /// The ink: texts on light backgrounds, outlines. #3A2C6E.
    pub const MYRTILLE: Color = hex(0x3A2C6E);
    /// The panels' background, a warm cream. #FFF5E6.
    pub const GUIMAUVE: Color = hex(0xFFF5E6);
    /// A button at rest. #FFFFFF.
    pub const COTON: Color = hex(0xFFFFFF);
    /// The depth under a button at rest, the panels' outline. #CBBBEF.
    pub const LILAS: Color = hex(0xCBBBEF);
    /// The button that has the focus. #FFC83D.
    pub const MANGUE: Color = hex(0xFFC83D);
    /// Its depth. #E9932B.
    pub const ABRICOT: Color = hex(0xE9932B);
    /// The title, « En ligne ». #FF5C8D.
    pub const FRAISE: Color = hex(0xFF5C8D);
    /// « Course solo ». #3FB4F0.
    pub const LAGON: Color = hex(0x3FB4F0);
    /// « Reprendre », what goes on. #36C98B.
    pub const MENTHE: Color = hex(0x36C98B);
    /// « Options ». #9B6BE8.
    pub const PRUNE: Color = hex(0x9B6BE8);
    /// « Pilote et kart ». #F08A3C.
    pub const CAROTTE: Color = hex(0xF08A3C);
    /// A disabled button. #E6E0F0.
    pub const BRUME: Color = hex(0xE6E0F0);
    /// Its depth, and a disabled icon's badge. #D3CBE3.
    pub const BRUME_FONCEE: Color = hex(0xD3CBE3);
    /// A disabled button's texts. #8E86A8.
    pub const CENDRE: Color = hex(0x8E86A8);
    /// Texts over the veil. #FFFFFF.
    pub const NEIGE: Color = hex(0xFFFFFF);
    /// The veil over the game, behind a screen: the ink, see-through.
    pub const VOILE: Color = Color::srgba(0.141, 0.102, 0.290, 0.55);
    /// The shadow under the panels and the texts over the veil.
    pub const OMBRE: Color = Color::srgba(0.141, 0.102, 0.290, 0.45);
}

/// Sizes of the reference window (px), `docs/UI_CHARTE.md` « Marges et tailles »: a grid of 4 and
/// 8 px, round corners everywhere.
pub mod size {
    /// The title « Fluffy Racers ».
    pub const TITLE: f32 = 64.0;
    /// The line under the title.
    pub const TAGLINE: f32 = 21.0;
    /// A button's label.
    pub const LABEL: f32 = 29.0;
    /// The small line under a label (what it does, why it is off).
    pub const CAPTION: f32 = 16.0;
    /// The keys at the bottom of a screen.
    pub const HINT: f32 = 16.0;

    /// A panel: width, inner margin, corner radius, gap between its buttons, shadow's drop.
    pub const PANEL_WIDTH: f32 = 520.0;
    pub const PANEL_PADDING: f32 = 20.0;
    pub const PANEL_RADIUS: f32 = 36.0;
    pub const PANEL_GAP: f32 = 12.0;
    pub const PANEL_SHADOW: f32 = 10.0;

    /// A button: height (its depth included), a pill (radius = half the height), its depth at
    /// rest and pressed, the gap and margins inside it.
    pub const BUTTON_HEIGHT: f32 = 74.0;
    pub const BUTTON_RADIUS: f32 = BUTTON_HEIGHT / 2.0;
    pub const BUTTON_DEPTH: f32 = 7.0;
    pub const BUTTON_DEPTH_PRESSED: f32 = 2.0;
    pub const BUTTON_PADDING_LEFT: f32 = 10.0;
    pub const BUTTON_PADDING_RIGHT: f32 = 24.0;
    pub const BUTTON_GAP: f32 = 16.0;

    /// The round badge of a button's icon, and the icon in it.
    pub const BADGE: f32 = 52.0;
    pub const ICON: f32 = 40.0;
}

/// What a button shows (`docs/UI_CHARTE.md` « Boutons »). The pointer over a button gives it the
/// focus: there is only one focus, for the mouse, the keyboard and the gamepad.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonState {
    Normal,
    /// Hovered, or chosen with the keys or the gamepad.
    Focused,
    /// Held down (key, gamepad button or mouse button), until released.
    Pressed,
    /// Cannot be used now: grey, skipped by the keys, its reason written under it.
    Disabled,
}

impl ButtonState {
    pub const ALL: [Self; 4] = [
        ButtonState::Normal,
        ButtonState::Focused,
        ButtonState::Pressed,
        ButtonState::Disabled,
    ];
}

/// The colors and the depth of a button in a state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonLook {
    pub fill: Color,
    /// The depth under it (its bottom border).
    pub depth: Color,
    /// How thick the depth is (px of the reference window): pressed, the button goes down.
    pub depth_px: f32,
    pub label: Color,
    pub caption: Color,
    /// The badge of its icon: the entry's own color, or grey when disabled.
    pub badge: Color,
}

/// The look of a button in `state`, its badge `accent` (the entry's color).
pub fn button_look(state: ButtonState, accent: Color) -> ButtonLook {
    use palette::*;
    match state {
        ButtonState::Normal => ButtonLook {
            fill: COTON,
            depth: LILAS,
            depth_px: size::BUTTON_DEPTH,
            label: MYRTILLE,
            caption: MYRTILLE.with_alpha(0.7),
            badge: accent,
        },
        ButtonState::Focused => ButtonLook {
            fill: MANGUE,
            depth: ABRICOT,
            depth_px: size::BUTTON_DEPTH,
            label: MYRTILLE,
            caption: MYRTILLE.with_alpha(0.8),
            badge: accent,
        },
        ButtonState::Pressed => ButtonLook {
            fill: MANGUE,
            depth: ABRICOT,
            depth_px: size::BUTTON_DEPTH_PRESSED,
            label: MYRTILLE,
            caption: MYRTILLE.with_alpha(0.8),
            badge: accent,
        },
        ButtonState::Disabled => ButtonLook {
            fill: BRUME,
            depth: BRUME_FONCEE,
            depth_px: size::BUTTON_DEPTH_PRESSED,
            label: CENDRE,
            caption: CENDRE,
            badge: BRUME_FONCEE,
        },
    }
}

/// The font of the screens: Fredoka (SIL Open Font License), round and with every French accent.
/// Its medium weight for the small lines...
pub const FONT_MEDIUM: &str = "ui/fonts/Fredoka-Medium.ttf";
/// ...and its semi-bold for the titles and the buttons.
pub const FONT_SEMIBOLD: &str = "ui/fonts/Fredoka-SemiBold.ttf";
/// The icons of the main menu: Kenney's Game Icons (CC0), white, on a round badge.
pub const ICON_RESUME: &str = "ui/icons/right.png";
pub const ICON_SOLO: &str = "ui/icons/singleplayer.png";
pub const ICON_ONLINE: &str = "ui/icons/multiplayer.png";
pub const ICON_OPTIONS: &str = "ui/icons/gear.png";
pub const ICON_GARAGE: &str = "ui/icons/wrench.png";

/// Every file of `assets/ui/` the game uses (each pack's licence is next to them): a test checks
/// that each one is there with its credits, and that nothing else is.
pub const UI_FILES: [&str; 7] = [
    FONT_MEDIUM,
    FONT_SEMIBOLD,
    ICON_RESUME,
    ICON_SOLO,
    ICON_ONLINE,
    ICON_OPTIONS,
    ICON_GARAGE,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of an sRGB color (WCAG 2), for the contrasts of the charter.
    fn luminance(color: Color) -> f32 {
        let linear = color.to_linear();
        0.2126 * linear.red + 0.7152 * linear.green + 0.0722 * linear.blue
    }

    /// Contrast ratio between two colors (WCAG 2): 1 (none) to 21 (black on white).
    fn contrast(a: Color, b: Color) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Every state of a button looks different from the others, the depth goes down when it is
    /// pressed, and the texts are readable: at least 4.5:1 (WCAG AA) for a button that can be
    /// used, its label and its caption alike; a disabled one stays legible, under its usable
    /// neighbours.
    #[test]
    fn each_state_of_a_button_looks_its_own_and_reads_well() {
        let accent = palette::LAGON;
        let looks: Vec<ButtonLook> = ButtonState::ALL
            .iter()
            .map(|state| button_look(*state, accent))
            .collect();
        for (i, a) in looks.iter().enumerate() {
            for b in &looks[i + 1..] {
                assert_ne!(a, b, "two states look the same");
            }
        }
        let normal = button_look(ButtonState::Normal, accent);
        let focused = button_look(ButtonState::Focused, accent);
        let pressed = button_look(ButtonState::Pressed, accent);
        let disabled = button_look(ButtonState::Disabled, accent);
        assert_ne!(normal.fill, focused.fill, "the focus shows");
        assert!(pressed.depth_px < focused.depth_px, "pressed, it goes down");
        assert_ne!(disabled.badge, accent, "a disabled badge is grey");
        for look in [normal, focused, pressed] {
            assert!(contrast(look.label, look.fill) >= 7.0, "{look:?}");
            // The caption is see-through: its color over the fill.
            let caption = mix(look.fill, look.caption);
            assert!(contrast(caption, look.fill) >= 4.5, "{look:?}");
        }
        assert!(contrast(disabled.label, disabled.fill) >= 2.5);
        assert!(contrast(disabled.label, disabled.fill) < contrast(normal.label, normal.fill));
    }

    /// The panels and the texts over the veil read well too: the ink on the cream, white over
    /// the veil (on the darkest and the lightest sky behind it).
    #[test]
    fn the_panels_and_the_veil_read_well() {
        use palette::*;
        assert!(contrast(MYRTILLE, GUIMAUVE) >= 7.0);
        for sky in [Color::srgb(0.55, 0.80, 0.95), Color::WHITE] {
            let behind = mix(sky, VOILE);
            assert!(contrast(NEIGE, behind) >= 3.0, "{behind:?}");
        }
        // The badges carry a white icon, always next to its label (the label says it all).
        for badge in [LAGON, FRAISE, MENTHE, PRUNE, CAROTTE] {
            assert!(contrast(NEIGE, badge) >= 2.0, "{badge:?}");
        }
    }

    /// The sizes are given for a 720 px high window and follow its height.
    #[test]
    fn sizes_follow_the_height_of_the_window() {
        assert_eq!(u(72.0), Val::Vh(10.0));
        assert_eq!(u(REFERENCE_HEIGHT), Val::Vh(100.0));
        assert!(matches!(font(36.0), FontSize::Vh(v) if (v - 5.0).abs() < 1e-5));
        // Five buttons, the title and the keys fit in the window.
        let panel = 5.0 * size::BUTTON_HEIGHT + 4.0 * size::PANEL_GAP + 2.0 * size::PANEL_PADDING;
        let screen = size::TITLE + size::TAGLINE + panel + 3.0 * size::HINT + 60.0;
        assert!(screen < REFERENCE_HEIGHT, "{screen}");
        assert_eq!(size::BUTTON_RADIUS * 2.0, size::BUTTON_HEIGHT, "a pill");
    }

    /// Issue #56: each file of `assets/ui/` the game names is there, each pack has its licence
    /// (OFL for the font, CC0 for Kenney's icons) and a line of `assets/CREDITS.md` naming its
    /// files, its author and its page; no other file is in the folder.
    #[test]
    fn every_ui_file_is_there_with_its_licence_and_its_credits() {
        use std::collections::BTreeSet;
        const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
        let credits = std::fs::read_to_string(format!("{ASSETS}/CREDITS.md")).unwrap();
        // (folder, its licence file, words the licence has, words its credits line has)
        let packs = [
            (
                "fonts",
                "OFL.txt",
                ["SIL OPEN FONT LICENSE Version 1.1", "Fredoka"],
                ["OFL", "https://fonts.google.com/specimen/Fredoka"],
            ),
            (
                "icons",
                "License.txt",
                ["CC0", "Kenney"],
                ["CC0", "https://kenney.nl/assets/game-icons"],
            ),
        ];
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        for (folder, licence, licence_words, credit_words) in packs {
            let text = std::fs::read_to_string(format!("{ASSETS}/ui/{folder}/{licence}"))
                .unwrap_or_else(|_| panic!("assets/ui/{folder}/{licence}"));
            for word in licence_words {
                assert!(text.contains(word), "{folder}/{licence}: {word}");
            }
            wanted.insert(format!("ui/{folder}/{licence}"));
            let line = credits
                .lines()
                .find(|line| line.contains(&format!("`ui/{folder}/`")))
                .unwrap_or_else(|| panic!("no line for ui/{folder}/ in assets/CREDITS.md"));
            for word in credit_words {
                assert!(line.contains(word), "{word} not in: {line}");
            }
            for file in UI_FILES
                .iter()
                .filter(|f| f.starts_with(&format!("ui/{folder}/")))
            {
                let name = file.rsplit('/').next().unwrap();
                assert!(line.contains(name), "{name} not in: {line}");
            }
        }
        for file in UI_FILES {
            let bytes = std::fs::read(format!("{ASSETS}/{file}"))
                .unwrap_or_else(|_| panic!("assets/{file} is missing"));
            if file.ends_with(".png") {
                assert!(bytes.starts_with(b"\x89PNG"), "{file}");
            } else {
                assert!(bytes.starts_with(&[0, 1, 0, 0]), "{file}: a TrueType font");
            }
            assert!(
                packs
                    .iter()
                    .any(|(folder, ..)| file.starts_with(&format!("ui/{folder}/"))),
                "{file}: in a pack's folder"
            );
            wanted.insert(file.to_string());
        }
        let mut present: BTreeSet<String> = BTreeSet::new();
        let mut folders = vec![format!("{ASSETS}/ui")];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path.to_string_lossy().into_owned());
                } else {
                    let relative = path.strip_prefix(ASSETS).unwrap().to_string_lossy();
                    present.insert(relative.trim_start_matches('/').to_string());
                }
            }
        }
        let unused: Vec<&String> = present.difference(&wanted).collect();
        assert!(unused.is_empty(), "files nothing uses: {unused:?}");
        let missing: Vec<&String> = wanted.difference(&present).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    /// `over` (with its alpha) over `under`.
    fn mix(under: Color, over: Color) -> Color {
        let (u, o) = (under.to_srgba(), over.to_srgba());
        let a = o.alpha;
        Color::srgb(
            o.red * a + u.red * (1.0 - a),
            o.green * a + u.green * (1.0 - a),
            o.blue * a + u.blue * (1.0 - a),
        )
    }
}
