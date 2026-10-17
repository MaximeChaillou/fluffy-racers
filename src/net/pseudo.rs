//! The player's pseudo (issue #80), pure: what the lobby's field keeps of a typed text, and how
//! it is kept between sessions.
//!
//! - **Characters**: those the HUD's font can write. Issue #100: the HUD writes in Fredoka
//!   (`crate::ui::format::writable`), accents included (« Éloïse » on the results screen and over
//!   the karts, as in the lobby); the ASCII filter of #80, which kept the letters the old font
//!   could write once their accent was dropped, is gone with that font. Anything Fredoka cannot
//!   draw (an emoji, a tab, a control character) is left out as it is typed: it would be a blank
//!   box.
//! - **Length**: [`PSEUDO_MAX_CHARS`] characters at most (the wire takes up to
//!   `protocol::MAX_NAME_CHARS`, for the characters' names too).
//! - **Empty**: no pseudo, the name of the player's character instead (`net::profile_of`).
//! - **Kept** in the browser's local storage ([`PSEUDO_STORAGE_KEY`], `admin::storage`), as a
//!   RON string, like the color (#55) and the options (#17); `?pseudo=` in the address wins over
//!   it, without replacing it.

use crate::ui::format::writable;

/// A pseudo has this many characters at most: it fits the lobby's list, the results screen's
/// name column and a tag over a kart.
pub const PSEUDO_MAX_CHARS: usize = 12;

/// Local storage key (web) / file name (native) of the pseudo.
pub const PSEUDO_STORAGE_KEY: &str = "fluffy-racers/pseudo";

/// Whether a pseudo can hold `c`: a character the HUD's font (Fredoka) writes.
pub fn allowed(c: char) -> bool {
    writable(c)
}

/// What the field keeps of `text` as it is typed: the [`allowed`] characters, at most
/// [`PSEUDO_MAX_CHARS`] of them. Spaces stay where they are (« Max » then « Max L »).
pub fn typed(text: &str) -> String {
    text.chars()
        .filter(|c| allowed(*c))
        .take(PSEUDO_MAX_CHARS)
        .collect()
}

/// The pseudo a text gives: [`typed`], without the spaces at either end; `None` when nothing is
/// left (the character's name then).
pub fn clean(text: &str) -> Option<String> {
    let pseudo = typed(text.trim());
    let pseudo = pseudo.trim();
    (!pseudo.is_empty()).then(|| pseudo.to_string())
}

/// The text kept in the browser (a RON string); nothing for no pseudo (the key is removed).
pub fn to_stored(pseudo: Option<&str>) -> String {
    pseudo
        .and_then(clean)
        .map(|pseudo| ron::to_string(&pseudo).unwrap_or_default())
        .unwrap_or_default()
}

/// Reads a kept text: the pseudo, cleaned. Not a RON string, or nothing left once cleaned: the
/// reason (the game then keeps the character's name).
pub fn from_stored(text: &str) -> Result<String, String> {
    let stored: String = ron::from_str(text.trim()).map_err(|error| error.to_string())?;
    clean(&stored).ok_or_else(|| format!("no pseudo in {stored:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #80: an empty field, or one of spaces, is no pseudo; the spaces at either end go.
    #[test]
    fn an_empty_pseudo_is_none() {
        for empty in ["", " ", "   ", "\t", "\n \u{7}"] {
            assert_eq!(clean(empty), None, "{empty:?}");
        }
        assert_eq!(clean("  Max  "), Some("Max".into()));
        assert_eq!(clean("Max L"), Some("Max L".into()));
    }

    /// At most 12 characters, counted as characters (an « é » is one), cut at a character.
    #[test]
    fn a_pseudo_has_twelve_characters_at_most() {
        assert_eq!(PSEUDO_MAX_CHARS, 12);
        assert_eq!(clean("abcdefghijklmnop"), Some("abcdefghijkl".into()));
        assert_eq!(clean(&"é".repeat(20)), Some("é".repeat(12)));
        assert_eq!(typed("Éloïse la très rapide"), "Éloïse la tr");
        // Cut, then trimmed: no space left at the end.
        assert_eq!(clean("Éloïse la ttt"), Some("Éloïse la tt".into()));
        assert_eq!(clean("abcdefghijk mno"), Some("abcdefghijk".into()));
        // The spaces at the start do not count.
        assert_eq!(clean("    abcdefghijklm"), Some("abcdefghijkl".into()));
        for text in ["Éloïse la très rapide", "😀😀😀 Bob", &"x".repeat(100)] {
            let pseudo = clean(text).unwrap();
            assert!(pseudo.chars().count() <= PSEUDO_MAX_CHARS, "{pseudo}");
            assert!(
                pseudo.len() <= super::super::protocol::MAX_NAME_BYTES,
                "fits the wire"
            );
        }
    }

    /// No control character (a tab, a new line, a bell), no character the HUD cannot write (an
    /// emoji, « → »); the accented letters of French stay, and (issue #100) the HUD writes them
    /// with their accent, as « ß » or « Œ », which Fredoka has.
    #[test]
    fn a_pseudo_keeps_what_the_hud_can_write() {
        assert_eq!(clean("a\tb\nc\u{7}d\u{7f}"), Some("abcd".into()));
        assert_eq!(clean("Bob 😀✨"), Some("Bob".into()));
        assert_eq!(clean("😀"), None);
        assert_eq!(clean("Straße"), Some("Straße".into()));
        assert_eq!(clean("Éloïse ça va"), Some("Éloïse ça va".into()));
        assert_eq!(clean("Zoé-42 !?"), Some("Zoé-42 !?".into()));
        assert_eq!(clean("Œ → Ÿ"), Some("Œ  Ÿ".into()));
        // Every character kept is one the HUD's font draws.
        let pseudo = clean("Ça, c'est Noël à l'Hôtel").unwrap();
        assert_eq!(pseudo, "Ça, c'est No");
        for c in pseudo.chars() {
            assert!(allowed(c) && writable(c) && !c.is_control(), "{c:?}");
        }
        // Typing an emoji in the middle leaves the rest where it was.
        assert_eq!(typed("Ma😀x "), "Max ");
    }

    /// Issue #80: the pseudo survives a reload: what goes to the browser comes back the same,
    /// accents included; no pseudo removes the key.
    #[test]
    fn the_pseudo_survives_a_round_trip_through_the_storage_text() {
        for pseudo in ["Max", "Éloïse ça va", "Zoé \"Z\" \\", "abcdefghijkl", "a"] {
            let text = to_stored(Some(pseudo));
            assert!(!text.is_empty());
            assert_eq!(from_stored(&text), Ok(pseudo.to_string()), "{text}");
        }
        assert_eq!(to_stored(None), "");
        assert_eq!(to_stored(Some("   ")), "", "an empty pseudo is not kept");
        // What is kept is the cleaned pseudo.
        assert_eq!(
            from_stored(&to_stored(Some(" Bob 😀 "))),
            Ok("Bob".to_string())
        );
    }

    /// A kept text that is not a pseudo (not RON, not a string, nothing left once cleaned) is
    /// refused with its reason: the game keeps the character's name. A kept pseudo made by hand
    /// is cleaned like a typed one.
    #[test]
    fn a_bad_stored_pseudo_is_refused() {
        for bad in [
            "",
            "Max",
            "42",
            "(name: \"Max\")",
            "\"",
            "\"   \"",
            "\"😀\"",
        ] {
            assert!(from_stored(bad).is_err(), "{bad}");
        }
        assert_eq!(from_stored(" \"Bob\"\n"), Ok("Bob".to_string()));
        assert_eq!(
            from_stored("\"abcdefghijklmnopqrstu\""),
            Ok("abcdefghijkl".to_string())
        );
    }
}
