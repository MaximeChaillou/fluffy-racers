//! ICE servers and candidates (issue #49): how two players find a path to each other. Pure logic,
//! unit-tested.
//!
//! To connect directly, each browser lists its **candidates**, the addresses where the other can
//! reach it: « host » (its own address on the local network; Chrome and Safari hide it behind a
//! random `….local` name, resolved by mDNS), « srflx » (its public address, as a **STUN** server
//! sees it) and « relay » (an address on a **TURN** relay, which forwards everything when no
//! direct path exists). `matchbox_socket` 0.14 waits until the browser has gathered all its
//! candidates before sending its offer or its answer.
//!
//! - **STUN** ([`StunChoice`], `?stun=` / `--stun`): by default, none when the signaling server is
//!   local (this machine or the local network: the host candidates are enough there, and a STUN
//!   server that UDP cannot reach only delays the offer, by about 40 s in Chrome), Google's public
//!   ones otherwise ([`PUBLIC_STUN`]). `none`, or a list of servers separated by commas.
//! - **TURN** ([`TurnServer`], `?turn=…&turnuser=…&turnpass=…`, `--turn … --turnuser …
//!   --turnpass …`): none by default. The production one comes with issue #10.
//! - **Candidates** ([`Candidates`]): counted in the offers, answers and trickled candidates that
//!   go through the signaling server, for the console and the lobby: none at all means the browser
//!   found no usable address (see the README, « Dépannage »).

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// The STUN servers for players on the Internet: Google's public ones (those of
/// `matchbox_socket` by default).
pub const PUBLIC_STUN: [&str; 2] = [
    "stun:stun.l.google.com:19302",
    "stun:stun1.l.google.com:19302",
];

/// At most this many servers in a list (a typo should not make the browser try hundreds).
pub const MAX_ICE_URLS: usize = 8;

/// What `?stun=` / `--stun` asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StunChoice {
    /// None when the signaling server is local, [`PUBLIC_STUN`] otherwise.
    #[default]
    Auto,
    /// `none`: no STUN server, only the host candidates (and TURN, if given).
    Off,
    /// These `stun:` URLs.
    Servers(Vec<String>),
}

impl StunChoice {
    /// `none` (or `off`, `0`, `aucun`), `auto`, or servers separated by commas: `stun:host:port`,
    /// `stuns:host:port`, or `host:port` (read as `stun:`). Anything else: `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        match text.to_ascii_lowercase().as_str() {
            "none" | "off" | "0" | "aucun" | "false" => Some(Self::Off),
            "auto" | "default" => Some(Self::Auto),
            _ => parse_url_list(text, &["stun", "stuns"]).map(Self::Servers),
        }
    }
}

/// A TURN relay: its URLs and the credentials it asks for.
#[derive(Clone, PartialEq, Eq)]
pub struct TurnServer {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

impl fmt::Debug for TurnServer {
    /// Without the password: it ends up in the console.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TurnServer")
            .field("urls", &self.urls)
            .field("username", &self.username)
            .field("credential", &"…")
            .finish()
    }
}

impl TurnServer {
    /// URLs separated by commas (`turn:host:port`, `turns:host:port`, `?transport=udp|tcp`
    /// allowed; `host:port` is read as `turn:`), with a user name and a password: a browser
    /// refuses a TURN server without them, so all three are needed.
    pub fn parse(urls: &str, username: &str, credential: &str) -> Option<Self> {
        let urls = parse_url_list(urls, &["turn", "turns"])?;
        let (username, credential) = (username.trim(), credential.trim());
        (!username.is_empty() && !credential.is_empty()).then(|| Self {
            urls,
            username: username.to_string(),
            credential: credential.to_string(),
        })
    }
}

/// The ICE servers a socket uses: `matchbox_socket` takes a single server entry (its URLs, one
/// user name and one password, the latter only used by TURN).
#[derive(Clone, PartialEq, Eq)]
pub struct IceServers {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl fmt::Debug for IceServers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IceServers")
            .field("urls", &self.urls)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl IceServers {
    /// What a socket that meets the others through `signal` uses: STUN as chosen (by default none
    /// for a local signaling server, [`PUBLIC_STUN`] otherwise), then the TURN relay, if any.
    pub fn choose(stun: &StunChoice, turn: Option<&TurnServer>, signal: &str) -> Self {
        let mut urls: Vec<String> = match stun {
            StunChoice::Auto if is_local_signal(signal) => Vec::new(),
            StunChoice::Auto => PUBLIC_STUN.iter().map(|url| url.to_string()).collect(),
            StunChoice::Off => Vec::new(),
            StunChoice::Servers(urls) => urls.clone(),
        };
        let (mut username, mut credential) = (None, None);
        if let Some(turn) = turn {
            urls.extend(turn.urls.iter().cloned());
            username = Some(turn.username.clone());
            credential = Some(turn.credential.clone());
        }
        Self {
            urls,
            username,
            credential,
        }
    }

    /// No server at all: only the host candidates.
    pub fn is_empty(&self) -> bool {
        self.urls.is_empty()
    }

    pub fn stun(&self) -> impl Iterator<Item = &str> {
        self.urls
            .iter()
            .map(String::as_str)
            .filter(|url| url.starts_with("stun"))
    }

    pub fn turn(&self) -> impl Iterator<Item = &str> {
        self.urls
            .iter()
            .map(String::as_str)
            .filter(|url| url.starts_with("turn"))
    }

    /// For the lobby window: « sans serveur STUN », « STUN stun.l.google.com:19302, … », and the
    /// TURN relay (never its password).
    pub fn describe(&self) -> String {
        fn without_scheme(url: &str) -> &str {
            url.split_once(':').map_or(url, |(_, rest)| rest)
        }
        let stun: Vec<&str> = self.stun().map(without_scheme).collect();
        let turn: Vec<&str> = self.turn().map(without_scheme).collect();
        let mut text = if stun.is_empty() {
            "sans serveur STUN".to_string()
        } else {
            format!("STUN {}", stun.join(", "))
        };
        if !turn.is_empty() {
            text.push_str(&format!(", relais TURN {}", turn.join(", ")));
        }
        text
    }
}

/// Whether the signaling server (`ws://host:port`, `wss://…`) is on this machine or on the local
/// network: `localhost`, a loopback, private or link-local address, a `.local` name.
pub fn is_local_signal(signal: &str) -> bool {
    signal_host(signal).is_some_and(is_local_host)
}

/// The host of a signaling address (`ws://` or `wss://`), without its port (`[::1]` gives `::1`).
pub fn signal_host(signal: &str) -> Option<&str> {
    let signal = signal.trim();
    if !(signal.starts_with("ws://") || signal.starts_with("wss://")) {
        return None;
    }
    url_host(signal)
}

/// The host of an address `scheme://host:port/path`, without its port and brackets.
pub fn url_host(url: &str) -> Option<&str> {
    let (_, rest) = url.trim().split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split_once(']')?.0
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then_some(host)
}

/// `localhost`, `*.localhost`, or a loopback address: this very machine.
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `localhost` (and `*.localhost`), `*.local`, or a loopback, private (10/8, 172.16/12,
/// 192.168/16, fc00::/7), link-local (169.254/16, fe80::/10) or unspecified address.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_local_v4(ip),
        Ok(IpAddr::V6(ip)) => match ip.to_ipv4_mapped() {
            Some(v4) => is_local_v4(v4),
            None => is_local_v6(ip),
        },
        Err(_) => false,
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn is_local_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local() || ip.is_unspecified()
}

/// A list of ICE URLs separated by commas, each with one of `schemes` (the first one when it has
/// none): 1 to [`MAX_ICE_URLS`] of them, all valid, or `None`.
fn parse_url_list(text: &str, schemes: &[&str]) -> Option<Vec<String>> {
    let urls = text
        .split(',')
        .map(|url| parse_ice_url(url, schemes))
        .collect::<Option<Vec<_>>>()?;
    (!urls.is_empty() && urls.len() <= MAX_ICE_URLS).then_some(urls)
}

/// `scheme:host[:port][?transport=udp|tcp]` (`transport` for TURN only), the host a name or an
/// address (IPv6 in brackets). Without a scheme, the first of `schemes`.
fn parse_ice_url(text: &str, schemes: &[&str]) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = match text.split_once(':') {
        Some((scheme, rest)) if schemes.contains(&scheme.to_ascii_lowercase().as_str()) => {
            (scheme.to_ascii_lowercase(), rest)
        }
        _ => (schemes.first()?.to_string(), text),
    };
    let (address, query) = match rest.split_once('?') {
        Some((address, query)) => (address, Some(query.to_ascii_lowercase())),
        None => (rest, None),
    };
    if let Some(query) = &query
        && !(scheme.starts_with("turn")
            && matches!(query.as_str(), "transport=udp" | "transport=tcp"))
    {
        return None;
    }
    let (host, port) = if let Some(bracketed) = address.strip_prefix('[') {
        let (ip, after) = bracketed.split_once(']')?;
        ip.parse::<Ipv6Addr>().ok()?;
        let port = match after {
            "" => None,
            after => Some(after.strip_prefix(':')?),
        };
        (format!("[{ip}]"), port)
    } else {
        let (host, port) = match address.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (address, None),
        };
        let valid = !host.is_empty()
            && host.len() <= 253
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && !host.starts_with(['.', '-']);
        if !valid {
            return None;
        }
        (host.to_ascii_lowercase(), port)
    };
    let mut url = format!("{scheme}:{host}");
    if let Some(port) = port {
        let port: u16 = port.parse().ok().filter(|&port| port != 0)?;
        url.push_str(&format!(":{port}"));
    }
    if let Some(query) = query {
        url.push('?');
        url.push_str(&query);
    }
    Some(url)
}

/// ICE candidates, by kind: those of an offer or an answer (its `a=candidate:` lines), or of one
/// trickled candidate (its JSON).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Candidates {
    /// Host candidates with a real address (natively, or a browser that does not hide it).
    pub host: u16,
    /// Host candidates hidden behind a `….local` name (Chrome, Safari): the other side must
    /// resolve it by mDNS, on the local network.
    pub mdns: u16,
    /// Public addresses found by STUN (and peer-reflexive ones).
    pub srflx: u16,
    /// Addresses on a TURN relay.
    pub relay: u16,
}

impl Candidates {
    /// Counts the candidates of `text`: every `candidate:<foundation> <component> <transport>
    /// <priority> <address> <port> typ <type>` in it.
    pub fn count(text: &str) -> Self {
        let mut counts = Self::default();
        let mut rest = text;
        while let Some(start) = rest.find("candidate:") {
            rest = &rest[start + "candidate:".len()..];
            let line = rest.split(['\r', '\n', '"']).next().unwrap_or("");
            let fields: Vec<&str> = line.split_ascii_whitespace().collect();
            let (Some(address), Some(kind)) = (
                fields.get(4),
                fields
                    .iter()
                    .position(|field| *field == "typ")
                    .and_then(|i| fields.get(i + 1)),
            ) else {
                continue;
            };
            match *kind {
                "host" if address.to_ascii_lowercase().ends_with(".local") => counts.mdns += 1,
                "host" => counts.host += 1,
                "srflx" | "prflx" => counts.srflx += 1,
                "relay" => counts.relay += 1,
                _ => {}
            }
        }
        counts
    }

    pub fn total(&self) -> u16 {
        self.host + self.mdns + self.srflx + self.relay
    }

    pub fn add(&mut self, other: Self) {
        self.host += other.host;
        self.mdns += other.mdns;
        self.srflx += other.srflx;
        self.relay += other.relay;
    }
}

impl fmt::Display for Candidates {
    /// For the console: « 2 candidates (1 host .local, 1 srflx) », « no candidate ».
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.total();
        if total == 0 {
            return f.write_str("no candidate");
        }
        let parts: Vec<String> = [
            (self.host, "host"),
            (self.mdns, "host .local"),
            (self.srflx, "srflx"),
            (self.relay, "relay"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, kind)| format!("{n} {kind}"))
        .collect();
        let plural = if total > 1 { "s" } else { "" };
        write!(f, "{total} candidate{plural} ({})", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_signaling_servers() {
        for local in [
            "ws://localhost:3536",
            "ws://LOCALHOST:3536/",
            "ws://127.0.0.1:3536",
            "ws://127.8.9.1",
            "ws://[::1]:3536",
            "ws://10.0.0.2:3536",
            "ws://172.16.0.1:3536",
            "ws://172.31.255.254:3536",
            "ws://192.168.1.20:3536/room",
            "ws://169.254.3.4:3536",
            "ws://[fd12:3456::1]:3536",
            "ws://[fe80::1]:3536",
            "ws://[::ffff:192.168.1.2]:3536",
            "ws://mon-mac.local:3536",
            "wss://mon-mac.local.:3536",
            "ws://dev.localhost:3536",
            "ws://0.0.0.0:3536",
        ] {
            assert!(is_local_signal(local), "{local}");
        }
        for internet in [
            "wss://signal.example.org",
            "wss://signal.example.org:443/x",
            "ws://8.8.8.8:3536",
            "ws://172.32.0.1:3536",
            "ws://172.15.255.255:3536",
            "ws://192.169.0.1:3536",
            "ws://11.0.0.1:3536",
            "ws://100.64.0.1:3536",
            "ws://[2001:db8::1]:3536",
            "ws://local:3536",
            "ws://localhost.example.org:3536",
            "ws://notlocal:3536",
            // Not a signaling address at all.
            "http://localhost:3536",
            "localhost:3536",
            "",
        ] {
            assert!(!is_local_signal(internet), "{internet}");
        }
        assert_eq!(signal_host("ws://user@[::1]:3536/x"), Some("::1"));
        assert_eq!(signal_host("wss://s.example:8443?x"), Some("s.example"));
        assert_eq!(signal_host("ws://:3536"), None);
        assert_eq!(signal_host("http://localhost:3536"), None);
        assert_eq!(
            url_host("http://192.168.1.20:8080/fluffy/?room=X"),
            Some("192.168.1.20")
        );
        assert_eq!(
            url_host("https://maximechaillou.github.io/fluffy-racers/"),
            Some("maximechaillou.github.io")
        );
        assert_eq!(url_host("localhost:8080"), None);
        for here in [
            "localhost",
            "LocalHost",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
            "a.localhost",
        ] {
            assert!(is_loopback_host(here), "{here}");
        }
        for elsewhere in [
            "192.168.1.20",
            "mon-mac.local",
            "example.org",
            "10.0.0.1",
            "::2",
        ] {
            assert!(!is_loopback_host(elsewhere), "{elsewhere}");
        }
        assert!(is_local_host("[fe80::1]"));
    }

    #[test]
    fn no_stun_server_by_default_when_the_signaling_is_local() {
        let auto = StunChoice::default();
        assert_eq!(auto, StunChoice::Auto);
        let local = IceServers::choose(&auto, None, "ws://localhost:3536");
        assert!(local.is_empty());
        assert_eq!(local.username, None);
        assert_eq!(local.describe(), "sans serveur STUN");
        let lan = IceServers::choose(&auto, None, "ws://192.168.1.20:3536");
        assert!(lan.is_empty());

        let internet = IceServers::choose(&auto, None, "wss://signal.example.org");
        assert_eq!(internet.urls, PUBLIC_STUN.map(String::from).to_vec());
        assert_eq!(
            internet.describe(),
            "STUN stun.l.google.com:19302, stun1.l.google.com:19302"
        );
        // The same as matchbox_socket's default.
        let matchbox = matchbox_socket::RtcIceServerConfig::default();
        assert_eq!(internet.urls, matchbox.urls);
    }

    #[test]
    fn the_stun_parameter() {
        assert_eq!(StunChoice::parse("none"), Some(StunChoice::Off));
        assert_eq!(StunChoice::parse(" NONE "), Some(StunChoice::Off));
        assert_eq!(StunChoice::parse("off"), Some(StunChoice::Off));
        assert_eq!(StunChoice::parse("0"), Some(StunChoice::Off));
        assert_eq!(StunChoice::parse("auto"), Some(StunChoice::Auto));
        assert_eq!(
            StunChoice::parse("stun:stun.example.org:3478"),
            Some(StunChoice::Servers(vec![
                "stun:stun.example.org:3478".into()
            ]))
        );
        assert_eq!(
            StunChoice::parse("stun.example.org:3478, STUN:192.168.1.2,stuns:[2001:db8::1]:5349"),
            Some(StunChoice::Servers(vec![
                "stun:stun.example.org:3478".into(),
                "stun:192.168.1.2".into(),
                "stuns:[2001:db8::1]:5349".into(),
            ]))
        );
        for bad in [
            "",
            " ",
            "stun:",
            "stun:host:0",
            "stun:host:65536",
            "stun:host:port",
            "stun:ho st",
            "stun:host/x",
            "stun:-host",
            "stun:host?transport=udp",
            "turn:host:3478",
            "http://host",
            "stun:a,,stun:b",
            "stun:[::1",
            "stun:[not-v6]:3478",
            "stun:a,stun:b,stun:c,stun:d,stun:e,stun:f,stun:g,stun:h,stun:i",
        ] {
            assert_eq!(StunChoice::parse(bad), None, "{bad:?}");
        }

        let none = IceServers::choose(&StunChoice::Off, None, "wss://signal.example.org");
        assert!(none.is_empty());
        let custom = StunChoice::parse("stun.example.org:3478").unwrap();
        let ice = IceServers::choose(&custom, None, "ws://localhost:3536");
        assert_eq!(ice.urls, vec!["stun:stun.example.org:3478".to_string()]);
        assert_eq!(ice.describe(), "STUN stun.example.org:3478");
    }

    #[test]
    fn a_turn_relay_needs_its_credentials() {
        let turn = TurnServer::parse("turn:relay.example.org:3478", "max", "secret").unwrap();
        assert_eq!(turn.urls, vec!["turn:relay.example.org:3478".to_string()]);
        let both = TurnServer::parse(
            "relay.example.org:3478?transport=TCP,turns:relay.example.org:5349",
            " max ",
            "secret",
        )
        .unwrap();
        assert_eq!(
            both.urls,
            vec![
                "turn:relay.example.org:3478?transport=tcp".to_string(),
                "turns:relay.example.org:5349".to_string(),
            ]
        );
        assert_eq!(both.username, "max");
        for (urls, user, pass) in [
            ("turn:relay.example.org:3478", "", "secret"),
            ("turn:relay.example.org:3478", "max", " "),
            ("", "max", "secret"),
            ("stun:relay.example.org:3478", "max", "secret"),
            (
                "turn:relay.example.org:3478?transport=sctp",
                "max",
                "secret",
            ),
        ] {
            assert_eq!(TurnServer::parse(urls, user, pass), None, "{urls} {user}");
        }
        // The password never shows.
        assert!(!format!("{turn:?}").contains("secret"));

        // TURN comes after the STUN servers, with the credentials.
        let local = IceServers::choose(&StunChoice::Auto, Some(&turn), "ws://localhost:3536");
        assert_eq!(local.urls, vec!["turn:relay.example.org:3478".to_string()]);
        assert_eq!(local.username.as_deref(), Some("max"));
        assert_eq!(local.credential.as_deref(), Some("secret"));
        assert!(!local.is_empty());
        assert_eq!(
            local.describe(),
            "sans serveur STUN, relais TURN relay.example.org:3478"
        );
        assert!(!format!("{local:?}").contains("secret"));
        let internet = IceServers::choose(&StunChoice::Auto, Some(&turn), "wss://s.example.org");
        assert_eq!(internet.urls.len(), 3);
        assert_eq!(internet.turn().count(), 1);
        assert_eq!(internet.stun().count(), 2);
    }

    /// An offer from Chrome (hidden host address, then STUN), a native one (real address), a
    /// trickled candidate, and an offer with none at all.
    #[test]
    fn candidates_are_counted_by_kind() {
        let chrome = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n\
            a=candidate:1248571717 1 udp 2113937151 e3f16915-31ad-4551-a16d-4d8246d4cb58.local 45627 typ host generation 0 network-cost 999\r\n\
            a=candidate:842163049 1 udp 1677729535 203.0.113.7 45627 typ srflx raddr 0.0.0.0 rport 0 generation 0\r\n\
            a=candidate:2 1 tcp 1518280447 192.0.2.2 9 typ host tcptype active\r\n\
            a=end-of-candidates\r\n";
        let counts = Candidates::count(chrome);
        assert_eq!(
            counts,
            Candidates {
                host: 1,
                mdns: 1,
                srflx: 1,
                relay: 0
            }
        );
        assert_eq!(counts.total(), 3);
        assert_eq!(
            counts.to_string(),
            "3 candidates (1 host, 1 host .local, 1 srflx)"
        );

        let trickled = r#"{"candidate":"candidate:3 1 udp 41885439 198.51.100.4 3478 typ relay raddr 203.0.113.7 rport 45627","sdpMid":"0","sdpMLineIndex":0}"#;
        assert_eq!(
            Candidates::count(trickled),
            Candidates {
                relay: 1,
                ..Default::default()
            }
        );
        let none =
            "v=0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\nc=IN IP4 0.0.0.0\r\n";
        assert_eq!(Candidates::count(none).total(), 0);
        assert_eq!(Candidates::count(none).to_string(), "no candidate");
        assert_eq!(Candidates::count("null").total(), 0);
        // Broken lines count for nothing, without panicking.
        assert_eq!(
            Candidates::count("a=candidate:\r\na=candidate:1 2 3").total(),
            0
        );

        let mut sum = counts;
        sum.add(Candidates::count(trickled));
        assert_eq!(sum.total(), 4);
        assert_eq!(
            Candidates {
                mdns: 1,
                ..Default::default()
            }
            .to_string(),
            "1 candidate (1 host .local)"
        );
    }
}
