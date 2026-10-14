//! Rooms (issue #7): their codes, the address of the signaling server, the invitation link, and
//! what the page address (`?room=CODE`) or the command line (`--room CODE`) asks for. Pure logic,
//! unit-tested.
//!
//! - **Code**: [`CODE_LENGTH`] characters of [`CODE_ALPHABET`], capitals and digits without the
//!   ones that look alike (0 and O, 1, I and L): easy to read out or type. A typed code is read
//!   generously ([`RoomCode::parse`]): lower case, spaces and dashes are fine.
//! - **Signaling**: the matchbox server that introduces the peers to each other (no game data goes
//!   through it). [`DEFAULT_SIGNAL_URL`] in development (`matchbox_server` on the player's
//!   machine); `?signal=wss://…` or `--signal wss://…` names another one, until the production
//!   signaling of issue #10. The room's address on it is [`room_url`].

use std::fmt;

/// The characters of a generated code: no 0, O, 1, I or L.
pub const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Length of a generated code: 31^5, about 29 million codes.
pub const CODE_LENGTH: usize = 5;

/// A typed code is at most this long.
pub const MAX_CODE_LENGTH: usize = 16;

/// The signaling server by default: `matchbox_server` on this machine (README, « Jouer à
/// plusieurs »).
pub const DEFAULT_SIGNAL_URL: &str = "ws://localhost:3536";

/// Prefix of our rooms on the signaling server: another game on the same server never meets ours.
pub const ROOM_PREFIX: &str = "fluffy-racers-";

/// A room's code: capitals and digits, 1 to [`MAX_CODE_LENGTH`] of them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RoomCode(String);

impl RoomCode {
    /// A new code, from a random number (`seed`).
    pub fn generate(seed: u64) -> Self {
        let base = CODE_ALPHABET.len() as u64;
        let mut rest = seed;
        let code = (0..CODE_LENGTH)
            .map(|_| {
                let letter = CODE_ALPHABET[(rest % base) as usize] as char;
                rest /= base;
                letter
            })
            .collect();
        Self(code)
    }

    /// A code as typed or found in an address: letters in any case and digits; spaces, dashes
    /// and underscores are dropped. Anything else, nothing, or too long: `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut code = String::new();
        for c in text.chars() {
            match c {
                ' ' | '-' | '_' => {}
                c if c.is_ascii_alphanumeric() => code.push(c.to_ascii_uppercase()),
                _ => return None,
            }
        }
        (!code.is_empty() && code.len() <= MAX_CODE_LENGTH).then_some(Self(code))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RoomCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A signaling server's address: `ws://` or `wss://`, then a host. Without a trailing slash.
pub fn parse_signal(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches('/');
    let host = text
        .strip_prefix("wss://")
        .or_else(|| text.strip_prefix("ws://"))?;
    (!host.is_empty() && !host.contains(char::is_whitespace)).then(|| text.to_string())
}

/// The room's address on the signaling server: one full-mesh room per code.
pub fn room_url(signal: &str, code: &RoomCode) -> String {
    format!("{}/{ROOM_PREFIX}{code}", signal.trim_end_matches('/'))
}

/// What the page address or the command line asks for at startup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    /// `?room=CODE`, `--room CODE`: join this room.
    pub room: Option<RoomCode>,
    /// `?signal=wss://…`, `--signal wss://…`: another signaling server.
    pub signal: Option<String>,
    /// `?pseudo=Max`, `--pseudo Max`: the name shown to the others.
    pub pseudo: Option<String>,
}

impl LaunchOptions {
    /// From a page address's query (`?room=AB3CD&pseudo=Max`, with or without the `?`). Values
    /// may be percent-encoded (`wss%3A%2F%2F…`, `+` for a space); unreadable ones are ignored.
    pub fn from_query(query: &str) -> Self {
        let mut options = Self::default();
        for pair in query.trim_start_matches('?').split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value);
            options.set(&key.to_ascii_lowercase(), &value);
        }
        options
    }

    /// From the command line's arguments (without the program's name): `--room CODE` or
    /// `--room=CODE`, and the same for `--signal` and `--pseudo`. Other arguments are ignored.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let Some(flag) = arg.strip_prefix("--") else {
                continue;
            };
            let (key, value) = match flag.split_once('=') {
                Some((key, value)) => (key.to_string(), Some(value.to_string())),
                None => (flag.to_string(), None),
            };
            if !matches!(key.as_str(), "room" | "signal" | "pseudo") {
                continue;
            }
            if let Some(value) = value.or_else(|| args.next()) {
                options.set(&key, &value);
            }
        }
        options
    }

    fn set(&mut self, key: &str, value: &str) {
        match key {
            "room" => self.room = RoomCode::parse(value),
            "signal" => self.signal = parse_signal(value),
            "pseudo" => {
                let name = crate::net::Profile::clean_name(value);
                self.pseudo = (!name.is_empty()).then_some(name);
            }
            _ => {}
        }
    }
}

/// The invitation link: the game's page (`page`: its address without query, like
/// `https://…/fluffy-racers/`) with `?room=CODE`, and `&signal=…` when the server is not the
/// default one (the friend must use the same).
pub fn invite_link(page: &str, code: &RoomCode, signal: &str) -> String {
    let mut link = format!("{page}?room={code}");
    if signal != DEFAULT_SIGNAL_URL {
        link.push_str("&signal=");
        link.push_str(&percent_encode(signal));
    }
    link
}

/// The page's query with `room=CODE` set (`None`: removed), the other parameters kept
/// (`?admin`, `signal`...). Written back in the address bar, so that a reload stays in the room.
pub fn query_with_room(query: &str, code: Option<&RoomCode>) -> String {
    let mut pairs: Vec<String> = query
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| {
            let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
            !pair.is_empty() && !key.eq_ignore_ascii_case("room")
        })
        .map(str::to_string)
        .collect();
    if let Some(code) = code {
        pairs.insert(0, format!("room={code}"));
    }
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

/// The command line that joins the room natively (`cargo run -- --room CODE`).
pub fn native_command(code: &RoomCode, signal: &str) -> String {
    let mut command = format!("cargo run -- --room {code}");
    if signal != DEFAULT_SIGNAL_URL {
        command.push_str(" --signal ");
        command.push_str(signal);
    }
    command
}

/// Decodes `%XX` and `+` (a space); a broken escape is kept as it is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                // `get` is `None` past the end, or across a character.
                let hex = text
                    .get(i + 1..i + 3)
                    .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes what a query value cannot hold as it is.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_codes_are_short_readable_and_varied() {
        let mut codes = std::collections::HashSet::new();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2_000 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let code = RoomCode::generate(seed >> 11);
            assert_eq!(code.as_str().len(), CODE_LENGTH);
            for c in code.as_str().chars() {
                assert!(CODE_ALPHABET.contains(&(c as u8)), "{code}");
                assert!(!"0O1IL".contains(c), "{code}");
            }
            assert_eq!(RoomCode::parse(code.as_str()), Some(code.clone()));
            codes.insert(code);
        }
        assert!(codes.len() > 1_990, "{} different codes", codes.len());
        assert_eq!(CODE_ALPHABET.len(), 31);
    }

    #[test]
    fn typed_codes_are_read_generously() {
        let code = |text| RoomCode::parse(text).map(|c| c.to_string());
        assert_eq!(code("ab3cd"), Some("AB3CD".into()));
        assert_eq!(code(" AB-3CD "), Some("AB3CD".into()));
        assert_eq!(code("TEST"), Some("TEST".into()));
        for bad in ["", "  ", "AB/CD", "é", "ABCDEFGHJKMNPQRST", "a?b"] {
            assert_eq!(code(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_signaling_server_and_the_room_address() {
        assert_eq!(
            parse_signal("wss://signal.example.org/"),
            Some("wss://signal.example.org".into())
        );
        assert_eq!(
            parse_signal(DEFAULT_SIGNAL_URL),
            Some(DEFAULT_SIGNAL_URL.into())
        );
        for bad in ["", "http://x", "ws://", "localhost:3536", "ws://a b"] {
            assert_eq!(parse_signal(bad), None, "{bad:?}");
        }
        let code = RoomCode::parse("AB3CD").unwrap();
        assert_eq!(
            room_url(DEFAULT_SIGNAL_URL, &code),
            "ws://localhost:3536/fluffy-racers-AB3CD"
        );
        assert_eq!(
            room_url("wss://s.example/", &code),
            "wss://s.example/fluffy-racers-AB3CD"
        );
    }

    #[test]
    fn the_page_address_names_the_room_the_server_and_the_name() {
        let options = LaunchOptions::from_query(
            "?admin&room=ab3cd&signal=wss%3A%2F%2Fs.example%2F&pseudo=Max+L",
        );
        assert_eq!(options.room, RoomCode::parse("AB3CD"));
        assert_eq!(options.signal.as_deref(), Some("wss://s.example"));
        assert_eq!(options.pseudo.as_deref(), Some("Max L"));
        assert_eq!(LaunchOptions::from_query(""), LaunchOptions::default());
        assert_eq!(
            LaunchOptions::from_query("?room="),
            LaunchOptions::default()
        );
        assert_eq!(
            LaunchOptions::from_query("?ROOM=TEST&signal=http://x&pseudo=%20"),
            LaunchOptions {
                room: RoomCode::parse("TEST"),
                ..Default::default()
            }
        );
        // A broken escape stays as it is.
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%C3%A9t%C3%A9"), "été");
    }

    #[test]
    fn the_command_line_names_the_room_the_server_and_the_name() {
        let args = |text: &str| LaunchOptions::from_args(text.split(' ').map(String::from));
        let options = args("--admin --room ab3cd --signal=ws://10.0.0.2:3536 --pseudo Max");
        assert_eq!(options.room, RoomCode::parse("AB3CD"));
        assert_eq!(options.signal.as_deref(), Some("ws://10.0.0.2:3536"));
        assert_eq!(options.pseudo.as_deref(), Some("Max"));
        assert_eq!(args("--room"), LaunchOptions::default());
        assert_eq!(args("room TEST"), LaunchOptions::default());
        assert_eq!(args("--room=TEST").room, RoomCode::parse("TEST"));
    }

    #[test]
    fn the_invitation_link_and_the_address_bar() {
        let code = RoomCode::parse("AB3CD").unwrap();
        let page = "https://maximechaillou.github.io/fluffy-racers/";
        assert_eq!(
            invite_link(page, &code, DEFAULT_SIGNAL_URL),
            "https://maximechaillou.github.io/fluffy-racers/?room=AB3CD"
        );
        let link = invite_link(page, &code, "wss://s.example");
        assert_eq!(
            link,
            "https://maximechaillou.github.io/fluffy-racers/?room=AB3CD&signal=wss%3A%2F%2Fs.example"
        );
        // The link reads back.
        let query = link.split_once('?').unwrap().1;
        let options = LaunchOptions::from_query(query);
        assert_eq!(options.room, Some(code.clone()));
        assert_eq!(options.signal.as_deref(), Some("wss://s.example"));

        assert_eq!(query_with_room("", Some(&code)), "?room=AB3CD");
        assert_eq!(
            query_with_room("?admin&room=OLD&signal=x", Some(&code)),
            "?room=AB3CD&admin&signal=x"
        );
        assert_eq!(query_with_room("?room=OLD&admin", None), "?admin");
        assert_eq!(query_with_room("?room=OLD", None), "");

        assert_eq!(
            native_command(&code, DEFAULT_SIGNAL_URL),
            "cargo run -- --room AB3CD"
        );
        assert_eq!(
            native_command(&code, "ws://10.0.0.2:3536"),
            "cargo run -- --room AB3CD --signal ws://10.0.0.2:3536"
        );
    }
}
