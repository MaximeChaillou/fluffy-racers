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
//! - **Host** (issue #63): the peer that created the room hosts it. « Créer une partie » puts
//!   `&host=1` in its own address bar (a reload hosts the same room again), never in the
//!   invitation link; natively `--host`. Without it, a client.
//! - **ICE servers** (issue #49, `ice.rs`): `?stun=none` or `?stun=stun:host:port,…`, and a TURN
//!   relay with `?turn=turn:host:port&turnuser=…&turnpass=…` (the same with `--stun`, `--turn`,
//!   `--turnuser` and `--turnpass` natively). They are not part of the invitation link: each
//!   player chooses his own.

use std::fmt;

use super::ice::{StunChoice, TurnServer, is_local_host, url_host};
use super::netsim::NetSimTuning;

/// The characters of a generated code: no 0, O, 1, I or L.
pub const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Length of a generated code: 31^5, about 29 million codes.
pub const CODE_LENGTH: usize = 5;

/// A typed code is at most this long.
pub const MAX_CODE_LENGTH: usize = 16;

/// The signaling server by default: `matchbox_server` on this machine (README, « Jouer à
/// plusieurs »).
pub const DEFAULT_SIGNAL_URL: &str = "ws://localhost:3536";

/// The port of `matchbox_server`.
pub const SIGNAL_PORT: u16 = 3536;

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

/// The signaling server by default for a page served from `page_host` (issue #49): when the page
/// comes from this machine or the local network (`localhost`, `192.168.1.20`, `mon-mac.local`...),
/// `matchbox_server` on that same machine (`ws://HOST:3536`). Another device of the network that
/// opens `http://192.168.1.20:8080/?room=CODE` then meets the others without `?signal=`. Any
/// other page (GitHub Pages), or none (natively): [`DEFAULT_SIGNAL_URL`].
pub fn default_signal(page_host: Option<&str>) -> String {
    match page_host.map(|host| host.trim_start_matches('[').trim_end_matches(']')) {
        Some(host) if is_local_host(host) && host.contains(':') => {
            format!("ws://[{host}]:{SIGNAL_PORT}")
        }
        Some(host) if is_local_host(host) => format!("ws://{host}:{SIGNAL_PORT}"),
        _ => DEFAULT_SIGNAL_URL.to_string(),
    }
}

/// The room's address on the signaling server: one full-mesh room per code.
pub fn room_url(signal: &str, code: &RoomCode) -> String {
    format!("{}/{ROOM_PREFIX}{code}", signal.trim_end_matches('/'))
}

/// What the page address or the command line asks for at startup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchOptions {
    /// `?room=CODE`, `--room CODE`: join this room.
    pub room: Option<RoomCode>,
    /// Issue #63: `?host=1` (`?host`), `--host`: we host the room (we created it). Only in the
    /// creator's address, never in the invitation link.
    pub host: bool,
    /// `?signal=wss://…`, `--signal wss://…`: another signaling server.
    pub signal: Option<String>,
    /// `?pseudo=Max`, `--pseudo Max`: the name shown to the others.
    pub pseudo: Option<String>,
    /// `?stun=none`, `?stun=stun:host:port,…` (issue #49); an unreadable value is ignored.
    pub stun: Option<StunChoice>,
    /// `?turn=turn:host:port,…`, as given: see [`LaunchOptions::turn_server`].
    pub turn: Option<String>,
    /// `?turnuser=…`, `?turnpass=…`: the TURN relay's credentials.
    pub turn_user: Option<String>,
    pub turn_pass: Option<String>,
    /// An unreadable `stun` value, kept for the warning ([`LaunchOptions::unusable`]).
    pub stun_refused: Option<String>,
    /// `?netsim=120,20,5`, `--netsim 120,20,5` (issue #8, debug): latency (ms), jitter (ms) and
    /// losses (%) of the kart states received ([`NetSimTuning::parse`]).
    pub netsim: Option<NetSimTuning>,
    /// An unreadable `netsim` value, kept for the warning.
    pub netsim_refused: Option<String>,
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
    /// `--room=CODE`, and the same for `--signal`, `--pseudo`, `--stun`, `--turn`, `--turnuser`,
    /// `--turnpass` and `--netsim`; `--host` alone (issue #63). Other arguments are ignored.
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
            if key == "host" {
                options.set("host", value.as_deref().unwrap_or("1"));
                continue;
            }
            if !LAUNCH_KEYS.contains(&key.as_str()) {
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
            "host" => {
                self.host = !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no"
                )
            }
            "signal" => self.signal = parse_signal(value),
            "pseudo" => {
                let name = crate::net::Profile::clean_name(value);
                self.pseudo = (!name.is_empty()).then_some(name);
            }
            "stun" => {
                self.stun = StunChoice::parse(value);
                self.stun_refused = self.stun.is_none().then(|| value.to_string());
            }
            "netsim" => {
                self.netsim = NetSimTuning::parse(value);
                self.netsim_refused = self.netsim.is_none().then(|| value.to_string());
            }
            "turn" | "turnuser" | "turnpass" => {
                let value = value.trim();
                let value = (!value.is_empty()).then(|| value.to_string());
                match key {
                    "turn" => self.turn = value,
                    "turnuser" => self.turn_user = value,
                    _ => self.turn_pass = value,
                }
            }
            _ => {}
        }
    }

    /// The TURN relay asked for: `turn`, `turnuser` and `turnpass` all given and valid
    /// ([`TurnServer::parse`]), or none.
    pub fn turn_server(&self) -> Option<TurnServer> {
        TurnServer::parse(
            self.turn.as_deref()?,
            self.turn_user.as_deref()?,
            self.turn_pass.as_deref()?,
        )
    }

    /// What was given but cannot be used, for a warning in the console: an unreadable `stun`,
    /// a `turn` relay without its credentials or with an unreadable address.
    pub fn unusable(&self) -> Vec<String> {
        let mut unusable = Vec::new();
        if let Some(stun) = &self.stun_refused {
            unusable.push(format!("stun={stun}"));
        }
        if let Some(netsim) = &self.netsim_refused {
            unusable.push(format!("netsim={netsim} (latency,jitter,losses: 120,20,5)"));
        }
        if let Some(turn) = &self.turn
            && self.turn_server().is_none()
        {
            let missing = self.turn_user.is_none() || self.turn_pass.is_none();
            unusable.push(if missing {
                format!("turn={turn} (turnuser and turnpass are needed too)")
            } else {
                format!("turn={turn}")
            });
        }
        unusable
    }
}

/// The parameters of the page address and the command line.
const LAUNCH_KEYS: [&str; 8] = [
    "room", "signal", "pseudo", "stun", "turn", "turnuser", "turnpass", "netsim",
];

/// The invitation link: the game's page (`page`: its address without query, like
/// `https://…/fluffy-racers/`) with `?room=CODE`, and `&signal=…` when the server is not the one
/// this page uses by default ([`default_signal`]: the friend must use the same).
pub fn invite_link(page: &str, code: &RoomCode, signal: &str) -> String {
    let mut link = format!("{page}?room={code}");
    if signal != default_signal(url_host(page)) {
        link.push_str("&signal=");
        link.push_str(&percent_encode(signal));
    }
    link
}

/// The page's query with `room=CODE` set (`None`: removed), the other parameters kept
/// (`?admin`, `signal`...). Written back in the address bar, so that a reload stays in the room.
/// A client's: issue #63, without `host` (a host's: [`host_query`]).
pub fn query_with_room(query: &str, code: Option<&RoomCode>) -> String {
    room_query(query, code, false)
}

/// Issue #63: the host's address bar, `room=CODE&host=1`: a reload hosts the room again.
pub fn host_query(query: &str, code: &RoomCode) -> String {
    room_query(query, Some(code), true)
}

fn room_query(query: &str, code: Option<&RoomCode>, host: bool) -> String {
    let mut pairs: Vec<String> = query
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| {
            let key = pair.split_once('=').map_or(*pair, |(key, _)| key);
            !pair.is_empty()
                && !key.eq_ignore_ascii_case("room")
                && !key.eq_ignore_ascii_case("host")
        })
        .map(str::to_string)
        .collect();
    if let Some(code) = code {
        if host {
            pairs.insert(0, "host=1".to_string());
        }
        pairs.insert(0, format!("room={code}"));
    }
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

/// The command line that joins the room natively (`cargo run -- --room CODE`): a client's, the
/// invitation (issue #63: without `--host`).
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

    /// Issue #49: a page served on this machine or the local network meets the others through
    /// `matchbox_server` on that same machine; GitHub Pages keeps `ws://localhost:3536`.
    #[test]
    fn the_signaling_server_follows_a_local_page() {
        assert_eq!(default_signal(Some("localhost")), "ws://localhost:3536");
        assert_eq!(default_signal(Some("127.0.0.1")), "ws://127.0.0.1:3536");
        assert_eq!(
            default_signal(Some("192.168.1.20")),
            "ws://192.168.1.20:3536"
        );
        assert_eq!(default_signal(Some("10.0.0.7")), "ws://10.0.0.7:3536");
        assert_eq!(default_signal(Some("172.20.1.2")), "ws://172.20.1.2:3536");
        assert_eq!(
            default_signal(Some("mon-mac.local")),
            "ws://mon-mac.local:3536"
        );
        assert_eq!(default_signal(Some("[::1]")), "ws://[::1]:3536");
        assert_eq!(default_signal(Some("fd00::5")), "ws://[fd00::5]:3536");
        for public in [
            Some("maximechaillou.github.io"),
            Some("8.8.8.8"),
            Some("172.32.0.1"),
            Some(""),
            None,
        ] {
            assert_eq!(default_signal(public), DEFAULT_SIGNAL_URL, "{public:?}");
        }
        // Every default is a signaling address, and a local one (no STUN by default).
        for host in ["localhost", "192.168.1.20", "[::1]", "mon-mac.local"] {
            let signal = default_signal(Some(host));
            assert_eq!(parse_signal(&signal), Some(signal.clone()));
            assert!(crate::net::is_local_signal(&signal), "{signal}");
        }

        // The invitation from a page on the local network: its own address, without `signal`
        // (the friend's page finds the same); with it when another server was asked for.
        let code = RoomCode::parse("AB3CD").unwrap();
        let page = "http://192.168.1.20:8080/";
        assert_eq!(
            invite_link(page, &code, "ws://192.168.1.20:3536"),
            "http://192.168.1.20:8080/?room=AB3CD"
        );
        assert_eq!(
            invite_link(page, &code, DEFAULT_SIGNAL_URL),
            "http://192.168.1.20:8080/?room=AB3CD&signal=ws%3A%2F%2Flocalhost%3A3536"
        );
        assert_eq!(
            invite_link("http://localhost:8080/", &code, DEFAULT_SIGNAL_URL),
            "http://localhost:8080/?room=AB3CD"
        );
    }

    /// Issue #49: the ICE servers, from the page address and from the command line.
    #[test]
    fn the_page_address_and_the_command_line_name_the_ice_servers() {
        let options = LaunchOptions::from_query("?room=TEST&stun=none");
        assert_eq!(options.stun, Some(StunChoice::Off));
        assert!(options.unusable().is_empty());
        let options = LaunchOptions::from_query(
            "?stun=stun%3Astun.example.org%3A3478,stun:192.168.1.2&turn=turn:relay.example.org:3478&turnuser=max&TURNPASS=s%C3%A9same",
        );
        assert_eq!(
            options.stun,
            Some(StunChoice::Servers(vec![
                "stun:stun.example.org:3478".into(),
                "stun:192.168.1.2".into()
            ]))
        );
        let turn = options.turn_server().unwrap();
        assert_eq!(turn.urls, vec!["turn:relay.example.org:3478".to_string()]);
        assert_eq!(turn.username, "max");
        assert_eq!(turn.credential, "sésame");
        assert!(options.unusable().is_empty());

        // Unreadable values are ignored, and said.
        let options = LaunchOptions::from_query("?stun=http://x&turn=turn:relay.example.org");
        assert_eq!(options.stun, None);
        assert_eq!(options.turn_server(), None);
        assert_eq!(
            options.unusable(),
            vec![
                "stun=http://x".to_string(),
                "turn=turn:relay.example.org (turnuser and turnpass are needed too)".to_string()
            ]
        );
        let options = LaunchOptions::from_query("?turn=stun:x&turnuser=a&turnpass=b");
        assert_eq!(options.turn_server(), None);
        assert_eq!(options.unusable(), vec!["turn=stun:x".to_string()]);
        // Empty: as if not given.
        assert_eq!(
            LaunchOptions::from_query("?turn=&turnuser=&turnpass="),
            LaunchOptions::default()
        );

        let args = |text: &str| LaunchOptions::from_args(text.split(' ').map(String::from));
        let options = args(
            "--room TEST --stun none --turn turns:relay.example.org:5349 --turnuser max --turnpass=x",
        );
        assert_eq!(options.room, RoomCode::parse("TEST"));
        assert_eq!(options.stun, Some(StunChoice::Off));
        let turn = options.turn_server().unwrap();
        assert_eq!(turn.urls, vec!["turns:relay.example.org:5349".to_string()]);
        assert_eq!(
            (turn.username.as_str(), turn.credential.as_str()),
            ("max", "x")
        );
        assert_eq!(
            args("--stun=stun.example.org").stun,
            Some(StunChoice::Servers(vec!["stun:stun.example.org".into()]))
        );
        assert_eq!(
            args("--stun nope!").unusable(),
            vec!["stun=nope!".to_string()]
        );
    }

    /// Issue #8: the network simulator from the page address and from the command line.
    #[test]
    fn the_page_address_and_the_command_line_set_the_network_simulator() {
        let options = LaunchOptions::from_query("?room=TEST&netsim=120,20,5");
        let bad = NetSimTuning {
            latency_ms: 120.0,
            jitter_ms: 20.0,
            loss_percent: 5.0,
        };
        assert_eq!(options.netsim, Some(bad));
        assert!(options.unusable().is_empty());
        // Percent-encoded commas, as some browsers write them.
        assert_eq!(
            LaunchOptions::from_query("?NETSIM=120%2C20%2C5").netsim,
            Some(bad)
        );
        let args = |text: &str| LaunchOptions::from_args(text.split(' ').map(String::from));
        assert_eq!(args("--room TEST --netsim 120,20,5").netsim, Some(bad));
        assert_eq!(args("--netsim=off").netsim, Some(NetSimTuning::default()));
        let refused = LaunchOptions::from_query("?netsim=lent");
        assert_eq!(refused.netsim, None);
        assert_eq!(
            refused.unusable(),
            vec!["netsim=lent (latency,jitter,losses: 120,20,5)".to_string()]
        );
        // Not part of the invitation: each peer simulates its own way in.
        let code = RoomCode::parse("AB3CD").unwrap();
        assert_eq!(
            query_with_room("?netsim=120,20,5", Some(&code)),
            "?room=AB3CD&netsim=120,20,5"
        );
    }

    /// Issue #63: the host's address says so (`host=1`), a reload hosts the room again; the
    /// invitation link never does (a client's), nor a client's address; natively `--host`.
    #[test]
    fn only_the_hosts_address_says_it_hosts() {
        let code = RoomCode::parse("AB3CD").unwrap();
        assert_eq!(
            host_query("?admin&signal=x", &code),
            "?room=AB3CD&host=1&admin&signal=x"
        );
        let address = host_query("?room=OLD&host=1&pseudo=Max", &code);
        assert_eq!(address, "?room=AB3CD&host=1&pseudo=Max");
        let options = LaunchOptions::from_query(&address);
        assert_eq!((options.room, options.host), (Some(code.clone()), true));
        // A client's address, and leaving: no `host`.
        assert_eq!(
            query_with_room(&address, Some(&code)),
            "?room=AB3CD&pseudo=Max"
        );
        assert_eq!(query_with_room(&address, None), "?pseudo=Max");
        let link = invite_link("http://192.168.1.20:8080/", &code, "ws://192.168.1.20:3536");
        let invited = LaunchOptions::from_query(link.split_once('?').unwrap().1);
        assert!(!invited.host && invited.room == Some(code.clone()));
        for (query, host) in [
            ("?room=TEST&host", true),
            ("?room=TEST&HOST=true", true),
            ("?room=TEST&host=0", false),
            ("?room=TEST&host=false", false),
            ("?room=TEST", false),
        ] {
            assert_eq!(LaunchOptions::from_query(query).host, host, "{query}");
        }
        let args = |text: &str| LaunchOptions::from_args(text.split(' ').map(String::from));
        let options = args("--host --room ab3cd");
        assert_eq!((options.room, options.host), (Some(code.clone()), true));
        assert!(args("--room ab3cd --host").host);
        assert!(!args("--room ab3cd").host);
        assert!(!args("--host=0 --room ab3cd").host);
        assert_eq!(
            native_command(&code, DEFAULT_SIGNAL_URL),
            "cargo run -- --room AB3CD"
        );
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
