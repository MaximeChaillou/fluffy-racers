//! What the lobby tells the player (issue #64, after Max's first test online: « Ce n'était
//! vraiment pas clair qu'on était dans un salon et qu'il fallait lancer la course »): the banner
//! at the top of the screen in a room, out of a race, and the lobby's main action (the host's
//! « Lancer la course », a client's « Prêt ! ») with what it waits for.
//!
//! Pure logic: [`RoomFacts`] is what the player's room looks like ([`RoomFacts::of`] reads it
//! from the session, the lobby and the race), and the texts follow from it, tested without Bevy
//! or egui. The window (`ui.rs`) only draws them.
//!
//! **The host is ready by starting**: « Lancer la course » waits for every other seated player's
//! « Prêt » (and at least one of them), never for the host's own (issue #64; before, the host had
//! to click « Prêt » too).

use std::fmt;

use super::lobby::Lobby;
use super::online::OnlineRace;
use super::protocol::{RaceStart, RoomSetup};
use super::{NetSession, NetStatus};

/// Issue #99: the host's settings of the room, as every player's lobby writes them: « Miroir :
/// non · Bots : oui · Objets : oui ».
pub fn settings_line(setup: RoomSetup) -> String {
    let yes_no = |on: bool| if on { "oui" } else { "non" };
    format!(
        "Miroir : {} · Bots : {} · Objets : {}",
        yes_no(setup.mirror),
        yes_no(setup.bots),
        yes_no(setup.items)
    )
}

/// Issue #99: what a race started has, after its circuit: « 3 tours, 9 bots, en miroir, sans
/// objets ».
pub fn race_line(start: &RaceStart) -> String {
    let mut line = format!(
        "{} tour{}, ",
        start.laps,
        if start.laps > 1 { "s" } else { "" }
    );
    match start.bots {
        0 => line.push_str("sans bots"),
        1 => line.push_str("1 bot"),
        bots => line.push_str(&format!("{bots} bots")),
    }
    if start.mirror {
        line.push_str(", en miroir");
    }
    if !start.items {
        line.push_str(", sans objets");
    }
    line
}

/// Where the player is with its room, as the lobby shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomFacts {
    /// The room's code.
    pub room: String,
    pub stage: Stage,
}

/// The room's state, seen from the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Reaching the server that introduces the players to each other.
    Connecting,
    /// A client that met no host (« Cette partie n'existe plus », issue #63); a late one still
    /// connects it.
    NoHost,
    /// A client in the room: its link with the host being made, or its seat coming.
    Arriving,
    /// Seated in the room.
    Seated(Seated),
    /// Nothing more to do in this room: unreachable, full, its host gone, or ours already hosted
    /// (the window says which and what to do; no banner).
    Over,
}

/// A seated player's room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seated {
    /// We host it (we created it).
    pub hosting: bool,
    /// Seated players, us included.
    pub players: usize,
    /// The host's name (empty: its `Hello` has not come yet).
    pub host: String,
    /// We said « Prêt » (a client).
    pub ready: bool,
    /// The other seated players that are not ready, in seat order, the host aside (it is ready
    /// by starting): their names (empty: unknown yet).
    pub not_ready: Vec<String>,
    pub race: RaceStage,
}

/// The room's race, for a seated player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceStage {
    /// No race: everyone drives freely on the room's circuit.
    Free,
    /// A race we are on the grid of, not over: the banner hides, the lobby closes.
    Racing,
    /// A race we are not on (we came during it): we drive freely beside it.
    Watching,
    /// The last race is over: its results; the host starts the next one from the lobby.
    Over,
}

/// The banner at the top of the screen: the room and its players, then what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    /// « Salon ABCDE · 3 joueurs ».
    pub lead: String,
    /// « Tu es l'hôte : choisis le circuit puis lance la course (L) ».
    pub action: String,
}

impl fmt::Display for Banner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.lead.is_empty() {
            f.write_str(&self.action)
        } else {
            write!(f, "{} · {}", self.lead, self.action)
        }
    }
}

impl RoomFacts {
    /// The player's room now (`now_ms`: the lobby's clock): `None` without a room.
    pub fn of(
        session: Option<&NetSession>,
        race: Option<&OnlineRace>,
        now_ms: u64,
    ) -> Option<RoomFacts> {
        let session = session?;
        let stage = match &session.status {
            NetStatus::Connecting => Stage::Connecting,
            NetStatus::InRoom if session.host_missing(now_ms) => Stage::NoHost,
            NetStatus::InRoom => match session.lobby.as_ref().filter(|lobby| lobby.seated()) {
                Some(lobby) => Stage::Seated(Seated::of(lobby, race)),
                None => Stage::Arriving,
            },
            NetStatus::Failed(_) | NetStatus::Full | NetStatus::HostLeft | NetStatus::HostTaken => {
                Stage::Over
            }
        };
        Some(RoomFacts {
            room: session.room.to_string(),
            stage,
        })
    }

    /// The banner: `None` in a race we run (and when the room is over: the window tells).
    pub fn banner(&self) -> Option<Banner> {
        let room = &self.room;
        let (lead, action) = match &self.stage {
            Stage::Connecting => (format!("Salon {room}"), "Connexion…".to_string()),
            Stage::Arriving => (format!("Salon {room}"), "Connexion à l'hôte…".to_string()),
            Stage::NoHost => (String::new(), "Cette partie n'existe plus (L)".to_string()),
            Stage::Over => return None,
            Stage::Seated(seated) => {
                if seated.race == RaceStage::Racing {
                    return None;
                }
                let lead = format!("Salon {room} · {}", players(seated.players));
                (lead, seated.banner_action())
            }
        };
        Some(Banner { lead, action })
    }
}

/// Whether the banner shows (it does unless we race, or the room is over): what
/// [`RoomFacts::banner`] says, without its texts (each frame, nothing allocated).
pub fn banner_shown(session: Option<&NetSession>, race: Option<&OnlineRace>) -> bool {
    let Some(session) = session else {
        return false;
    };
    match session.status {
        NetStatus::Connecting => true,
        NetStatus::InRoom => {
            seated_lobby(session).is_none_or(|lobby| race_stage(lobby, race) != RaceStage::Racing)
        }
        _ => false,
    }
}

/// The room's race for us: its number (0: none) and its stage; `None` unless seated in the room.
/// The lobby closes when our race starts, and opens when it ends.
pub fn race_now(
    session: Option<&NetSession>,
    race: Option<&OnlineRace>,
) -> Option<(u16, RaceStage)> {
    let lobby = seated_lobby(session?)?;
    let number = lobby.announced().map_or(0, |start| start.race);
    Some((number, race_stage(lobby, race)))
}

/// Our lobby, seated in the room.
fn seated_lobby(session: &NetSession) -> Option<&Lobby> {
    session
        .lobby
        .as_ref()
        .filter(|lobby| session.status == NetStatus::InRoom && lobby.seated())
}

/// The room's race for a seated player: the race the host announced (none: free driving), and
/// whether it is over, and we on its grid. Announced but not built yet (this frame): its grid
/// says.
fn race_stage(lobby: &Lobby, race: Option<&OnlineRace>) -> RaceStage {
    let Some(start) = lobby.announced() else {
        return RaceStage::Free;
    };
    match race.filter(|race| race.shared.race() == start.race) {
        Some(race) if race.shared.over().is_some() => RaceStage::Over,
        Some(race) if race.racing => RaceStage::Racing,
        Some(_) => RaceStage::Watching,
        None if start.slot_of(lobby.me()).is_some() => RaceStage::Racing,
        None => RaceStage::Watching,
    }
}

impl Seated {
    fn of(lobby: &Lobby, race: Option<&OnlineRace>) -> Seated {
        let host = lobby.host();
        let name_of = |key| {
            if key == lobby.me() {
                Some(lobby.profile().name.clone())
            } else {
                lobby
                    .peer(key)
                    .and_then(|peer| peer.profile.as_ref())
                    .map(|profile| profile.name.clone())
            }
        };
        let not_ready = lobby
            .seats()
            .iter()
            .filter(|key| **key != lobby.me() && Some(**key) != host)
            .filter_map(|key| {
                let profile = lobby.peer(*key).and_then(|peer| peer.profile.as_ref());
                match profile {
                    Some(profile) if profile.ready => None,
                    Some(profile) => Some(profile.name.clone()),
                    None => Some(String::new()),
                }
            })
            .collect();
        Seated {
            hosting: lobby.is_host(),
            players: lobby.seats().len(),
            host: host.and_then(name_of).unwrap_or_default(),
            ready: lobby.profile().ready,
            not_ready,
            race: race_stage(lobby, race),
        }
    }

    /// The host can start a race: at least one other player, every one of them ready, no race
    /// running.
    pub fn can_start(&self) -> bool {
        self.start_blocker().is_none()
    }

    /// Why « Lancer la course » waits (the host), written under the button: `None` when it can
    /// start.
    pub fn start_blocker(&self) -> Option<String> {
        if matches!(self.race, RaceStage::Racing | RaceStage::Watching) {
            return Some("Une course est en cours.".to_string());
        }
        if self.players < 2 {
            return Some("Attends qu'un joueur se connecte…".to_string());
        }
        not_ready(&self.not_ready)
    }

    /// What the banner asks.
    fn banner_action(&self) -> String {
        let host = self.host_named();
        match (self.hosting, self.race) {
            (_, RaceStage::Watching) => "Course en cours : tu seras de la prochaine".to_string(),
            (true, _) if self.players < 2 => {
                "Tu es l'hôte : envoie le lien d'invitation à tes amis (L)".to_string()
            }
            (true, _) => "Tu es l'hôte : choisis le circuit puis lance la course (L)".to_string(),
            (false, _) if self.ready => format!("En attente de {host}…"),
            (false, _) => "Clique sur « Prêt ! » (L)".to_string(),
        }
    }

    /// « l'hôte (Ana) », « l'hôte » before its name comes.
    fn host_named(&self) -> String {
        if self.host.is_empty() {
            "l'hôte".to_string()
        } else {
            format!("l'hôte ({})", self.host)
        }
    }

    /// Who does what, at the top of the lobby.
    pub fn role(&self) -> String {
        if self.hosting {
            if self.players < 2 {
                "Tu es l'hôte : envoie le lien d'invitation à tes amis.".to_string()
            } else {
                "Tu es l'hôte : choisis le circuit puis lance la course.".to_string()
            }
        } else {
            format!(
                "{} choisit le circuit et lance la course.",
                capitalized(&self.host_named())
            )
        }
    }

    /// What the players do meanwhile (issue #64: the free driving was not explained).
    pub fn meanwhile(&self) -> Option<&'static str> {
        match self.race {
            RaceStage::Free => {
                Some("En attendant la course, roule librement sur le circuit avec les autres.")
            }
            RaceStage::Watching => {
                Some("Tu es arrivé pendant la course : roule librement, tu seras de la prochaine.")
            }
            RaceStage::Racing | RaceStage::Over => None,
        }
    }

    /// Under a client's « Prêt ! » button.
    pub fn client_hint(&self) -> String {
        let host = self.host_named();
        if !self.ready {
            format!("Quand tout le monde est prêt, {host} lance la course.")
        } else if self.not_ready.is_empty() {
            format!("Tout le monde est prêt : {host} peut lancer la course.")
        } else {
            format!("Tu es prêt ! En attente de {host}…")
        }
    }
}

/// « 1 joueur », « 3 joueurs ».
fn players(count: usize) -> String {
    if count > 1 {
        format!("{count} joueurs")
    } else {
        format!("{count} joueur")
    }
}

/// « Bob n'est pas prêt. », « Bob et Cyd ne sont pas prêts. »…: `None` when everyone is.
fn not_ready(names: &[String]) -> Option<String> {
    let count = names.len();
    if count == 0 {
        return None;
    }
    if names.iter().any(String::is_empty) {
        return Some(if count == 1 {
            "Un joueur n'est pas prêt.".to_string()
        } else {
            format!("{count} joueurs ne sont pas prêts.")
        });
    }
    Some(match names {
        [one] => format!("{one} n'est pas prêt."),
        [one, two] => format!("{one} et {two} ne sont pas prêts."),
        [one, two, three] => format!("{one}, {two} et {three} ne sont pas prêts."),
        [one, two, ..] => format!("{one}, {two} et {} autres ne sont pas prêts.", count - 2),
        [] => unreachable!(),
    })
}

/// The text with its first letter in capitals.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(stage: Stage) -> RoomFacts {
        RoomFacts {
            room: "ABCDE".to_string(),
            stage,
        }
    }

    fn seated(hosting: bool, players: usize, ready: bool, not_ready: &[&str]) -> Seated {
        Seated {
            hosting,
            players,
            host: "Ana".to_string(),
            ready,
            not_ready: not_ready.iter().map(|name| name.to_string()).collect(),
            race: RaceStage::Free,
        }
    }

    fn banner(stage: Stage) -> Option<String> {
        facts(stage).banner().map(|banner| banner.to_string())
    }

    /// Issue #64: the banner says the room, its players, and what to do: the host chooses the
    /// circuit and starts the race; a client says « Prêt ! », then waits for the host, by name.
    #[test]
    fn the_banner_says_the_room_and_what_to_do() {
        assert_eq!(
            banner(Stage::Seated(seated(true, 3, false, &["Bob"]))).as_deref(),
            Some(
                "Salon ABCDE · 3 joueurs · Tu es l'hôte : choisis le circuit puis lance la course (L)"
            )
        );
        assert_eq!(
            banner(Stage::Seated(seated(true, 1, false, &[]))).as_deref(),
            Some(
                "Salon ABCDE · 1 joueur · Tu es l'hôte : envoie le lien d'invitation à tes amis (L)"
            )
        );
        assert_eq!(
            banner(Stage::Seated(seated(false, 3, false, &[]))).as_deref(),
            Some("Salon ABCDE · 3 joueurs · Clique sur « Prêt ! » (L)")
        );
        assert_eq!(
            banner(Stage::Seated(seated(false, 3, true, &["Cyd"]))).as_deref(),
            Some("Salon ABCDE · 3 joueurs · En attente de l'hôte (Ana)…")
        );
        // The host's name not known yet.
        let mut nameless = seated(false, 2, true, &[]);
        nameless.host.clear();
        assert_eq!(
            banner(Stage::Seated(nameless)).as_deref(),
            Some("Salon ABCDE · 2 joueurs · En attente de l'hôte…")
        );
        // Before the seat.
        assert_eq!(
            banner(Stage::Connecting).as_deref(),
            Some("Salon ABCDE · Connexion…")
        );
        assert_eq!(
            banner(Stage::Arriving).as_deref(),
            Some("Salon ABCDE · Connexion à l'hôte…")
        );
        // The words of issue #63.
        assert_eq!(
            banner(Stage::NoHost).as_deref(),
            Some("Cette partie n'existe plus (L)")
        );
        let parts = facts(Stage::Seated(seated(false, 2, true, &[])))
            .banner()
            .unwrap();
        assert_eq!(parts.lead, "Salon ABCDE · 2 joueurs");
        assert_eq!(parts.action, "En attente de l'hôte (Ana)…");
    }

    /// Issue #64: the banner hides in a race we run, comes back at its end, and says the race to
    /// a player who came during it; none once the room is over (the window says it).
    #[test]
    fn the_banner_hides_in_a_race() {
        for hosting in [true, false] {
            let mut room = seated(hosting, 2, true, &[]);
            room.race = RaceStage::Racing;
            assert_eq!(banner(Stage::Seated(room.clone())), None);
            room.race = RaceStage::Over;
            assert!(banner(Stage::Seated(room.clone())).is_some());
        }
        let mut late = seated(false, 3, false, &[]);
        late.race = RaceStage::Watching;
        assert_eq!(
            banner(Stage::Seated(late)).as_deref(),
            Some("Salon ABCDE · 3 joueurs · Course en cours : tu seras de la prochaine")
        );
        assert_eq!(banner(Stage::Over), None);
    }

    /// Issue #64: « Lancer la course » says why it waits: nobody else yet, who is not ready, a
    /// race on. The host itself never has to be ready.
    #[test]
    fn the_start_button_says_why_it_waits() {
        let alone = seated(true, 1, false, &[]);
        assert_eq!(
            alone.start_blocker().as_deref(),
            Some("Attends qu'un joueur se connecte…")
        );
        assert!(!alone.can_start());
        let cases: [(&[&str], &str); 5] = [
            (&["Bob"], "Bob n'est pas prêt."),
            (&["Bob", "Cyd"], "Bob et Cyd ne sont pas prêts."),
            (&["Bob", "Cyd", "Dan"], "Bob, Cyd et Dan ne sont pas prêts."),
            (
                &["Bob", "Cyd", "Dan", "Eve", "Fil"],
                "Bob, Cyd et 3 autres ne sont pas prêts.",
            ),
            (&["Bob", ""], "2 joueurs ne sont pas prêts."),
        ];
        for (names, reason) in cases {
            let room = seated(true, names.len() + 1, false, names);
            assert_eq!(room.start_blocker().as_deref(), Some(reason));
            assert!(!room.can_start());
        }
        assert_eq!(
            seated(true, 2, false, &[""]).start_blocker().as_deref(),
            Some("Un joueur n'est pas prêt.")
        );
        // Everyone else ready, the host not: it can start.
        let ready = seated(true, 3, false, &[]);
        assert_eq!(ready.start_blocker(), None);
        assert!(ready.can_start());
        // Again after a race; not while one runs.
        let mut after = ready.clone();
        after.race = RaceStage::Over;
        assert!(after.can_start());
        after.race = RaceStage::Racing;
        assert_eq!(
            after.start_blocker().as_deref(),
            Some("Une course est en cours.")
        );
    }

    /// Issue #64: the lobby's lines: who does what, the free driving meanwhile, a client's hint
    /// under « Prêt ! ».
    #[test]
    fn the_lobby_explains_the_room() {
        assert_eq!(
            seated(true, 2, false, &["Bob"]).role(),
            "Tu es l'hôte : choisis le circuit puis lance la course."
        );
        assert_eq!(
            seated(true, 1, false, &[]).role(),
            "Tu es l'hôte : envoie le lien d'invitation à tes amis."
        );
        assert_eq!(
            seated(false, 2, false, &[]).role(),
            "L'hôte (Ana) choisit le circuit et lance la course."
        );
        let mut room = seated(false, 2, false, &[]);
        assert_eq!(
            room.meanwhile(),
            Some("En attendant la course, roule librement sur le circuit avec les autres.")
        );
        assert_eq!(
            room.client_hint(),
            "Quand tout le monde est prêt, l'hôte (Ana) lance la course."
        );
        room.ready = true;
        assert_eq!(
            room.client_hint(),
            "Tout le monde est prêt : l'hôte (Ana) peut lancer la course."
        );
        room.not_ready = vec!["Cyd".to_string()];
        assert_eq!(
            room.client_hint(),
            "Tu es prêt ! En attente de l'hôte (Ana)…"
        );
        room.race = RaceStage::Watching;
        assert!(room.meanwhile().unwrap().contains("roule librement"));
        room.race = RaceStage::Racing;
        assert_eq!(room.meanwhile(), None);
    }

    /// Issue #64: the player's words, without the network's jargon.
    #[test]
    fn no_jargon_for_the_player() {
        let mut texts = Vec::new();
        for stage in [Stage::Connecting, Stage::Arriving, Stage::NoHost] {
            texts.extend(banner(stage));
        }
        for hosting in [true, false] {
            for ready in [true, false] {
                for players in [1, 3] {
                    for race in [
                        RaceStage::Free,
                        RaceStage::Watching,
                        RaceStage::Over,
                        RaceStage::Racing,
                    ] {
                        let mut room = seated(hosting, players, ready, &["Bob"]);
                        room.race = race;
                        texts.extend(room.start_blocker());
                        texts.push(room.role());
                        texts.push(room.client_hint());
                        texts.extend(room.meanwhile().map(str::to_string));
                        texts.extend(banner(Stage::Seated(room)));
                    }
                }
            }
        }
        for text in &texts {
            let lower = text.to_lowercase();
            for word in [
                "pair", "signal", "ice", "mdns", "peer", "stun", "turn", "webrtc",
            ] {
                assert!(
                    !lower
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|w| w == word)
                        && !lower.contains("signalisation"),
                    "{word:?} in {text:?}"
                );
            }
        }
    }

    /// Issue #99: the lobby writes the host's settings, and a race's: its laps, its bots (or none),
    /// the mirror and the items when they are not the default.
    #[test]
    fn the_lobby_writes_the_hosts_settings() {
        assert_eq!(
            settings_line(RoomSetup::default()),
            "Miroir : non · Bots : oui · Objets : oui"
        );
        let all_off = RoomSetup {
            track: 2,
            mirror: true,
            bots: false,
            items: false,
        };
        assert_eq!(
            settings_line(all_off),
            "Miroir : oui · Bots : non · Objets : non"
        );
        let mut start = RaceStart {
            race: 1,
            seed: 0,
            track: 0,
            laps: 3,
            bots: 9,
            difficulty: 1,
            mirror: false,
            items: true,
            start_tick: 0,
            players: Vec::new(),
        };
        assert_eq!(race_line(&start), "3 tours, 9 bots");
        start.bots = 0;
        start.mirror = true;
        start.items = false;
        start.laps = 1;
        assert_eq!(
            race_line(&start),
            "1 tour, sans bots, en miroir, sans objets"
        );
        start.bots = 1;
        assert!(race_line(&start).contains("1 bot,"));
    }
}
