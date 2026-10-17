//! Issue #101: what the lobby's screen shows (`docs/UI_CHARTE.md` § « Le salon (#101) »), in
//! pure logic (unit-tested): [`salon_view`] reads the room (the session, its lobby, its race) and
//! gives the screen its title, its lines, the host's settings, its buttons (the main action
//! first, disabled with its reason), the players and the problems of connection. The tree
//! (`screen.rs`) only draws it. The texts are those of the egui window before (issues #7, #49,
//! #63, #64, #80, #99), after the charter's tone: short, « tu », no jargon.

use crate::admin::tracks::TrackEntry;
use crate::characters::{PartKind, Roster};
use crate::kart::KartColor;
use crate::net::{
    Diagnosis, MAX_PLAYERS, NetSession, NetStatus, OnlineRace, RaceStage, RoomFacts, RoomSetup,
    RowLink, Seated, Stage, Stuck, color_notice, settings_line,
};

/// A setting of the room, chosen by the host (issue #99). Issue #122 will add the mode (a race,
/// a championship) at the top: one more line of the same kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SalonRow {
    Circuit,
    Mirror,
    Bots,
    Items,
}

impl SalonRow {
    pub const ALL: [Self; 4] = [
        SalonRow::Circuit,
        SalonRow::Mirror,
        SalonRow::Bots,
        SalonRow::Items,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SalonRow::Circuit => "Circuit",
            SalonRow::Mirror => "Miroir",
            SalonRow::Bots => "Bots",
            SalonRow::Items => "Objets",
        }
    }

    /// Its value in `setup`, written.
    pub fn value(self, setup: RoomSetup, circuits: &[TrackEntry]) -> String {
        let yes_no = |on: bool| if on { "Oui" } else { "Non" }.to_string();
        match self {
            SalonRow::Circuit => circuits
                .get(usize::from(setup.track))
                .map_or_else(|| "?".to_string(), |entry| entry.name.clone()),
            SalonRow::Mirror => yes_no(setup.mirror),
            SalonRow::Bots => yes_no(setup.bots),
            SalonRow::Items => yes_no(setup.items),
        }
    }

    /// `setup` with this setting `step` further (the circuits around; the others switch).
    pub fn changed(self, setup: RoomSetup, step: i32, circuits: usize) -> RoomSetup {
        let mut next = setup;
        match self {
            SalonRow::Circuit => {
                let count = circuits.max(1) as i32;
                next.track = (i32::from(setup.track) + step).rem_euclid(count) as u8;
            }
            SalonRow::Mirror => next.mirror = !setup.mirror,
            SalonRow::Bots => next.bots = !setup.bots,
            SalonRow::Items => next.items = !setup.items,
        }
        next
    }

    /// What it is for (the help under the lines, for the host).
    pub fn help(self, setup: RoomSetup) -> &'static str {
        match self {
            SalonRow::Circuit => "La prochaine course, pour tout le monde.",
            SalonRow::Mirror => "Le circuit à l'envers, gauche et droite.",
            SalonRow::Bots if setup.bots => "Des bots complètent la grille de 12.",
            SalonRow::Bots => "Seuls les joueurs courent.",
            SalonRow::Items if setup.items => "Les boîtes à objets sur la piste.",
            SalonRow::Items => "Aucune boîte ; les croquettes restent.",
        }
    }
}

/// What a button of the lobby does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// « Créer une partie »: a new room, hosted.
    CreateRoom,
    /// The room again (the server could not be reached, its host reloaded, our address hosted
    /// it already: as a client).
    Retry,
    JoinAgain,
    JoinAsGuest,
    Ready,
    NotReady,
    StartRace,
    /// The invitation link, copied.
    Copy,
    /// « Pilote » (issue #98).
    Pilot,
    /// Out of the room (back home).
    Leave,
    /// Back, without a room.
    Back,
}

/// A button: what it does, its label (16 signs at most), its small line (what it does, or why
/// it is off), whether it can be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonView {
    pub kind: ButtonKind,
    pub label: String,
    pub caption: String,
    pub enabled: bool,
}

fn button(kind: ButtonKind, label: &str, caption: impl Into<String>) -> ButtonView {
    ButtonView {
        kind,
        label: label.to_string(),
        caption: caption.into(),
        enabled: true,
    }
}

/// A player of the room, as its line shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerLine {
    /// Its pseudo, « (toi) » after ours.
    pub name: String,
    pub me: bool,
    pub host: bool,
    /// Its animal (empty while it arrives).
    pub animal: String,
    /// The color it wears (`None` until its `Hello`).
    pub color: Option<KartColor>,
    /// Its link: « 12 ms », « connecté », « en cours… », « échec », « autre version (14) ».
    pub link: String,
    /// The link is a problem (silent, failed, another version).
    pub trouble: bool,
    /// « prêt » (never for the host: it is ready by starting).
    pub ready: bool,
}

/// What [`salon_view`] reads.
#[derive(Clone, Copy)]
pub struct ViewInput<'a> {
    pub session: Option<&'a NetSession>,
    pub race: Option<&'a OnlineRace>,
    pub roster: Option<&'a Roster>,
    pub circuits: &'a [TrackEntry],
    /// Our pseudo, as the others see it.
    pub pseudo: &'a str,
    /// The invitation (the page's link, or natively the command line).
    pub link: Option<&'a str>,
    /// « Copier » was just clicked.
    pub copied: bool,
    /// The page is served from this machine's loopback (`localhost`): the others cannot reach it.
    pub loopback_page: bool,
    /// The signaling server we would use, and whether it is local.
    pub signal: &'a str,
    pub local_signal: bool,
    pub now_ms: u64,
}

/// What a seated player's room shows: the host's settings, and whether they can be changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub setup: RoomSetup,
    /// We host the room and no race runs: the settings change.
    pub editable: bool,
    /// Why they cannot be changed (a client, a race on).
    pub locked: Option<&'static str>,
}

/// The lobby's screen (see the module's documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SalonView {
    /// « Multijoueur » out of a room, « Salon ABCDE » in one.
    pub title: String,
    /// The left column's lines, in order.
    pub lines: Vec<String>,
    /// Seated: the room's settings.
    pub seat: Option<Seat>,
    /// The buttons, the main action first.
    pub buttons: Vec<ButtonView>,
    /// The players' header (« Joueurs · 3 sur 12 ») and lines.
    pub players_header: String,
    pub players: Vec<PlayerLine>,
    /// The invitation link (in a room).
    pub invite: Option<String>,
    /// The right column's notes: a color taken, another version, the connections being made, the
    /// leads when one does not come (#49).
    pub notes: Vec<String>,
}

/// The Chrome setting that hides the local addresses behind `….local` names.
const CHROME_MDNS_FLAG: &str = "chrome://flags/#enable-webrtc-hide-local-ips-with-mdns";

fn plural(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}

/// The lines saying where the signaling server is (and that it must run, when it is local).
fn signal_lines(lines: &mut Vec<String>, input: &ViewInput, signal: &str) {
    lines.push(format!("Mise en relation : {signal}"));
    if input.local_signal {
        lines.push("matchbox_server doit tourner (voir le README).".to_string());
    }
}

/// The lobby's screen now (see the module's documentation).
pub fn salon_view(input: &ViewInput) -> SalonView {
    let mut view = SalonView {
        title: "Multijoueur".to_string(),
        lines: vec![format!(
            "Ton pseudo : {} · change-le dans Pilote (G).",
            input.pseudo
        )],
        seat: None,
        buttons: Vec::new(),
        players_header: String::new(),
        players: Vec::new(),
        invite: None,
        notes: Vec::new(),
    };
    let leave = button(
        ButtonKind::Leave,
        "Quitter",
        "Retour à l'accueil : tu quittes la partie.",
    );
    let create = button(
        ButtonKind::CreateRoom,
        "Créer une partie",
        "Tu en es l'hôte : invite tes amis.",
    );
    let Some(session) = input.session else {
        view.lines.push(
            "Joue avec tes amis : crée une partie (tu en es l'hôte), puis envoie-leur le lien."
                .to_string(),
        );
        view.lines
            .push("Pour rejoindre une partie : ouvre le lien d'invitation.".to_string());
        signal_lines(&mut view.lines, input, input.signal);
        view.buttons.push(create);
        view.buttons
            .push(button(ButtonKind::Back, "Retour", "Retour à l'accueil."));
        return view;
    };
    let room = session.room.to_string();
    view.title = format!("Salon {room}");
    view.invite = input.link.map(str::to_string);
    if input.loopback_page {
        view.notes.push(
            "Pour inviter quelqu'un sur ton réseau, ouvre le jeu avec l'adresse IP de ton \
             ordinateur (README)."
                .to_string(),
        );
    }
    let facts = RoomFacts::of(Some(session), input.race, input.now_ms);
    let mut players = true;
    match (&session.status, facts.as_ref().map(|f| &f.stage)) {
        (NetStatus::Connecting, _) => {
            view.lines.push("Connexion à la partie…".to_string());
            signal_lines(&mut view.lines, input, &session.signal);
            view.buttons.push(leave);
            players = false;
        }
        (NetStatus::Failed(reason), _) => {
            view.lines.push(format!(
                "Impossible de joindre le serveur de mise en relation {}.",
                session.signal
            ));
            if input.local_signal {
                view.lines
                    .push("Lance matchbox_server (voir le README), puis réessaie.".to_string());
            }
            view.lines.push(reason.clone());
            view.buttons.push(button(
                ButtonKind::Retry,
                "Réessayer",
                "Rejoindre le serveur de nouveau.",
            ));
            view.buttons.push(leave);
            players = false;
        }
        (NetStatus::Full, _) => {
            view.lines.push(format!(
                "La partie {room} est pleine ({MAX_PLAYERS} joueurs)."
            ));
            view.buttons.push(ButtonView {
                caption: "Une nouvelle partie, à toi.".to_string(),
                ..create
            });
            view.buttons.push(leave);
            players = false;
        }
        (NetStatus::HostLeft, _) => {
            view.lines.push("L'hôte a quitté la partie.".to_string());
            view.lines.push(
                "« Rejoindre » : si l'hôte a rechargé sa page, la partie reprend.".to_string(),
            );
            view.buttons.push(button(
                ButtonKind::JoinAgain,
                "Rejoindre",
                "La même partie, de nouveau.",
            ));
            view.buttons.push(create);
            view.buttons.push(leave);
            players = false;
        }
        (NetStatus::HostTaken, _) => {
            view.lines.push(format!("La partie {room} a déjà un hôte."));
            view.lines.push(
                "Cette adresse est celle de l'hôte (« host=1 ») : ouvre plutôt le lien \
                 d'invitation."
                    .to_string(),
            );
            view.buttons.push(button(
                ButtonKind::JoinAsGuest,
                "La rejoindre",
                "Comme un invité, sans « host=1 ».",
            ));
            view.buttons.push(ButtonView {
                caption: "Une nouvelle partie, à toi.".to_string(),
                ..create
            });
            view.buttons.push(leave);
            players = false;
        }
        (_, Some(Stage::NoHost)) => {
            view.lines.push("Cette partie n'existe plus.".to_string());
            view.lines.push(format!(
                "Personne n'héberge la partie {room} : son hôte est parti, ou ne l'a pas encore \
                 ouverte."
            ));
            view.buttons.push(create);
            view.buttons.push(leave);
        }
        (_, Some(Stage::Seated(seated))) => {
            seated_view(&mut view, input, session, seated);
        }
        _ => {
            view.lines
                .push("Connexion à l'hôte de la partie…".to_string());
            view.buttons.push(leave);
        }
    }
    if players {
        players_view(&mut view, input, session);
    }
    connections(&mut view, session, input.now_ms);
    view
}

/// Seated in the room: who does what, the race on (or over), the settings, the main action.
fn seated_view(view: &mut SalonView, input: &ViewInput, session: &NetSession, seated: &Seated) {
    let Some(lobby) = session.lobby.as_ref() else {
        return;
    };
    view.lines.push(seated.role());
    let name = |track: u8| {
        input
            .circuits
            .get(usize::from(track))
            .map_or("?", |entry| entry.name.as_str())
    };
    match (lobby.announced(), seated.race) {
        (Some(start), RaceStage::Racing | RaceStage::Watching) => {
            view.lines.push(format!(
                "Course n° {} en cours sur {} : {}.",
                start.race,
                name(start.track),
                crate::net::race_line(start)
            ));
            if seated.race == RaceStage::Watching {
                view.lines
                    .push("Tu es arrivé pendant la course : tu seras de la prochaine.".to_string());
            }
        }
        (Some(start), _) => {
            view.lines
                .push(format!("Course n° {} terminée.", start.race));
        }
        (None, _) => {
            view.lines.push(
                "En attendant, promène-toi et donne des coups de patte (X ou F) !".to_string(),
            );
        }
    }
    let racing = matches!(seated.race, RaceStage::Racing | RaceStage::Watching);
    let setup = lobby.setup();
    view.seat = Some(Seat {
        setup,
        editable: seated.hosting && !racing,
        locked: if !seated.hosting {
            Some("En ligne, c'est l'hôte qui choisit la course.")
        } else if racing {
            Some("Une course est en cours.")
        } else {
            None
        },
    });
    if !seated.hosting {
        view.lines.push(format!(
            "Réglages de l'hôte : {} · {}",
            name(setup.track),
            settings_line(setup)
        ));
    }
    if seated.hosting {
        let blocker = seated.start_blocker();
        view.buttons.push(ButtonView {
            kind: ButtonKind::StartRace,
            label: "Lancer la course".to_string(),
            caption: blocker
                .clone()
                .unwrap_or_else(|| "Tout le monde est prêt !".to_string()),
            enabled: blocker.is_none(),
        });
    } else if seated.ready {
        view.buttons.push(button(
            ButtonKind::NotReady,
            "Pas prêt",
            seated.client_hint(),
        ));
    } else {
        view.buttons
            .push(button(ButtonKind::Ready, "Prêt !", seated.client_hint()));
    }
    view.buttons.push(button(
        ButtonKind::Pilot,
        "Pilote",
        "Ton pseudo, ton animal, ton kart.",
    ));
    view.buttons.push(button(
        ButtonKind::Leave,
        "Quitter",
        "Retour à l'accueil : tu quittes la partie.",
    ));
    if view.invite.is_some() {
        view.buttons.push(button(
            ButtonKind::Copy,
            "Copier",
            if input.copied {
                "Copié !"
            } else {
                "Le lien d'invitation, pour tes amis."
            },
        ));
    }
}

/// The players of the room (and the links with the host being made).
fn players_view(view: &mut SalonView, input: &ViewInput, session: &NetSession) {
    let rows = session.players(input.now_ms);
    let seated = rows.iter().filter(|row| row.seated).count();
    view.players_header = format!("Joueurs · {seated} sur {MAX_PLAYERS}");
    for row in &rows {
        let mut name = if row.name.is_empty() {
            if row.host { "L'hôte" } else { "…" }.to_string()
        } else {
            row.name.clone()
        };
        if row.me {
            name.push_str(" (toi)");
        }
        let animal = if row.seated || row.me {
            input
                .roster
                .map(|r| {
                    r.name(PartKind::Character, usize::from(row.loadout[0]))
                        .to_string()
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let (link, trouble) = match (row.other_version, row.link, row.silent, row.ping_ms) {
            (Some(version), ..) => (format!("autre version ({version})"), true),
            (_, RowLink::Me, ..) => (String::new(), false),
            (_, RowLink::Connecting, ..) => ("en cours…".to_string(), false),
            (_, RowLink::Stuck(_), ..) => ("échec".to_string(), true),
            (_, RowLink::ViaHost, ..) => ("connecté".to_string(), false),
            (_, _, true, _) => ("muet".to_string(), true),
            (_, _, _, Some(ms)) => (format!("{ms:.0} ms"), false),
            (_, _, _, None) => ("connecté".to_string(), false),
        };
        view.players.push(PlayerLine {
            name,
            me: row.me,
            host: row.host,
            animal,
            color: row.color,
            link,
            trouble,
            ready: row.ready && !row.host,
        });
    }
    if rows.iter().any(|row| row.other_version.is_some()) {
        view.notes
            .push("Un joueur a une autre version du jeu : rechargez tous la page.".to_string());
    }
    if let Some(notice) = color_notice(&rows) {
        view.notes
            .push(format!("{notice} Change-la dans Pilote (G)."));
    }
}

/// The connections being made (#49): « en cours » after a moment, and the leads when one does
/// not come.
fn connections(view: &mut SalonView, session: &NetSession, now_ms: u64) {
    match session.links.diagnosis(now_ms) {
        None => {}
        Some(Diagnosis::Connecting { .. }) if !session.hosting => {
            view.notes.push("Connexion à l'hôte en cours…".to_string());
        }
        Some(Diagnosis::Connecting { peers }) => {
            view.notes.push(format!(
                "Connexion en cours avec {peers} joueur{}…",
                plural(peers)
            ));
        }
        Some(Diagnosis::Stuck { peers, cause }) => {
            view.notes.push(if session.hosting {
                format!(
                    "La connexion avec {peers} joueur{} n'aboutit pas.",
                    plural(peers)
                )
            } else {
                "La connexion avec l'hôte n'aboutit pas.".to_string()
            });
            if let Some(why) = match cause {
                Stuck::NoCandidateHere => Some("Ton navigateur n'a donné aucune adresse réseau."),
                Stuck::NoCandidateThere => {
                    Some("Le navigateur de l'autre joueur n'a donné aucune adresse réseau.")
                }
                Stuck::Failed => Some("Aucun chemin trouvé entre vous deux."),
                Stuck::Slow => None,
            } {
                view.notes.push(why.to_string());
            }
            view.notes.push(
                "1. macOS : Réglages Système > Confidentialité et sécurité > Réseau local : \
                 active ton navigateur."
                    .to_string(),
            );
            view.notes
                .push(format!("2. Chrome : {CHROME_MDNS_FLAG} > Disabled."));
            view.notes
                .push("3. Réseau qui bloque l'UDP : il faudra un relais TURN (#10).".to_string());
            view.notes
                .push("Détails : la console (F12), le README (« Dépannage »).".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Lobby, PeerKey, Profile, RaceStart, session_for_tests};

    const ANA: PeerKey = PeerKey(1 << 100);

    fn circuits() -> Vec<TrackEntry> {
        crate::admin::tracks::circuits()
    }

    fn input<'a>(session: Option<&'a NetSession>, circuits: &'a [TrackEntry]) -> ViewInput<'a> {
        ViewInput {
            session,
            race: None,
            roster: None,
            circuits,
            pseudo: "Ana",
            link: Some("https://example.org/?room=ABCDE"),
            copied: false,
            loopback_page: false,
            signal: "ws://localhost:3536",
            local_signal: true,
            now_ms: 1_000,
        }
    }

    fn session(status: NetStatus, hosting: bool) -> NetSession {
        let mut session = session_for_tests("ABCDE", ANA, hosting);
        session.status = status;
        session
    }

    /// The labels stay short (16 signs at most) and the small lines under 48, whatever the
    /// state (the charter's tone).
    fn assert_short(view: &SalonView) {
        for button in &view.buttons {
            assert!(button.label.chars().count() <= 16, "{button:?}");
            assert!(
                button.caption.chars().count() < 48,
                "{}: {} signs",
                button.caption,
                button.caption.chars().count()
            );
        }
        for row in SalonRow::ALL {
            assert!(row.label().chars().count() <= 16);
            for setup in [
                RoomSetup::default(),
                RoomSetup {
                    bots: false,
                    items: false,
                    ..default()
                },
            ] {
                assert!(row.help(setup).chars().count() < 48, "{row:?}");
            }
        }
    }

    fn default<T: Default>() -> T {
        T::default()
    }

    /// Out of a room: « Multijoueur », create a game or go back, how to join, the signaling
    /// server.
    #[test]
    fn out_of_a_room_it_offers_to_create_one() {
        let circuits = circuits();
        let view = salon_view(&input(None, &circuits));
        assert_eq!(view.title, "Multijoueur");
        let kinds: Vec<ButtonKind> = view.buttons.iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [ButtonKind::CreateRoom, ButtonKind::Back]);
        assert!(view.lines[0].contains("Ana"));
        assert!(view.lines.iter().any(|l| l.contains("lien d'invitation")));
        assert!(view.lines.iter().any(|l| l.contains("matchbox_server")));
        assert!(view.seat.is_none() && view.players.is_empty());
        assert_short(&view);
    }

    /// Each state of a room has its words and its way out: connecting, the server unreachable
    /// (« Réessayer »), full, its host gone (« Rejoindre »), already hosted (« La rejoindre »);
    /// « Quitter » always.
    #[test]
    fn every_state_of_a_room_says_what_to_do() {
        let circuits = circuits();
        let cases = [
            (NetStatus::Connecting, "Connexion", vec![ButtonKind::Leave]),
            (
                NetStatus::Failed("refusé".to_string()),
                "Impossible",
                vec![ButtonKind::Retry, ButtonKind::Leave],
            ),
            (
                NetStatus::Full,
                "pleine",
                vec![ButtonKind::CreateRoom, ButtonKind::Leave],
            ),
            (
                NetStatus::HostLeft,
                "L'hôte a quitté",
                vec![
                    ButtonKind::JoinAgain,
                    ButtonKind::CreateRoom,
                    ButtonKind::Leave,
                ],
            ),
            (
                NetStatus::HostTaken,
                "déjà un hôte",
                vec![
                    ButtonKind::JoinAsGuest,
                    ButtonKind::CreateRoom,
                    ButtonKind::Leave,
                ],
            ),
        ];
        for (status, words, buttons) in cases {
            let session = session(status.clone(), false);
            let view = salon_view(&input(Some(&session), &circuits));
            assert_eq!(view.title, "Salon ABCDE", "{status:?}");
            assert!(
                view.lines.iter().any(|l| l.contains(words)),
                "{status:?}: {:?}",
                view.lines
            );
            let kinds: Vec<ButtonKind> = view.buttons.iter().map(|b| b.kind).collect();
            assert_eq!(kinds, buttons, "{status:?}");
            assert_short(&view);
        }
    }

    /// Seated: the host's main action « Lancer la course », disabled with its reason while it is
    /// alone or someone is not ready, then usable; its settings can change; « Pilote »,
    /// « Quitter » and « Copier » follow; the players listed with their link.
    #[test]
    fn the_host_starts_when_everyone_is_ready() {
        let circuits = circuits();
        let mut session = session(NetStatus::InRoom, true);
        session.lobby = Some(Lobby::new(
            ANA,
            Profile {
                name: "Ana".to_string(),
                ..Default::default()
            },
            0,
            true,
        ));
        let view = salon_view(&input(Some(&session), &circuits));
        let kinds: Vec<ButtonKind> = view.buttons.iter().map(|b| b.kind).collect();
        assert_eq!(
            kinds,
            [
                ButtonKind::StartRace,
                ButtonKind::Pilot,
                ButtonKind::Leave,
                ButtonKind::Copy
            ]
        );
        let start = &view.buttons[0];
        assert!(!start.enabled);
        assert!(start.caption.contains("Attends"), "{start:?}");
        let seat = view.seat.as_ref().unwrap();
        assert!(seat.editable && seat.locked.is_none());
        assert_eq!(view.players.len(), 1);
        assert!(view.players[0].me && view.players[0].host);
        assert_eq!(view.players[0].name, "Ana (toi)");
        assert_eq!(view.players_header, "Joueurs · 1 sur 12");
        assert_eq!(
            view.invite.as_deref(),
            Some("https://example.org/?room=ABCDE")
        );
        assert!(view.lines.iter().any(|l| l.contains("coups de patte")));
        assert_short(&view);
        // « Copier » says it was.
        let copied = salon_view(&ViewInput {
            copied: true,
            ..input(Some(&session), &circuits)
        });
        assert_eq!(copied.buttons[3].caption, "Copié !");
        // A race announced: the settings are locked, the start says why.
        let lobby = session.lobby.as_mut().unwrap();
        lobby.announce_race(RaceStart {
            race: 1,
            seed: 7,
            track: 0,
            laps: 1,
            bots: 0,
            difficulty: 0,
            mirror: false,
            items: true,
            start_tick: 100,
            players: vec![ANA],
        });
        let racing = salon_view(&input(Some(&session), &circuits));
        let seat = racing.seat.unwrap();
        assert!(!seat.editable && seat.locked.is_some());
        assert!(racing.lines.iter().any(|l| l.contains("en cours")));
    }

    /// The settings' lines: their values written, a step changes them (the circuits around),
    /// each says what it is for.
    #[test]
    fn the_settings_change_around() {
        let circuits = circuits();
        let setup = RoomSetup::default();
        assert_eq!(SalonRow::Circuit.value(setup, &circuits), circuits[0].name);
        assert_eq!(SalonRow::Mirror.value(setup, &circuits), "Non");
        assert_eq!(SalonRow::Bots.value(setup, &circuits), "Oui");
        let back = SalonRow::Circuit.changed(setup, -1, circuits.len());
        assert_eq!(usize::from(back.track), circuits.len() - 1);
        assert_eq!(
            SalonRow::Circuit.changed(back, 1, circuits.len()).track,
            setup.track
        );
        assert!(SalonRow::Mirror.changed(setup, 1, circuits.len()).mirror);
        assert!(!SalonRow::Bots.changed(setup, -1, circuits.len()).bots);
        assert!(!SalonRow::Items.changed(setup, 1, circuits.len()).items);
        assert_ne!(
            SalonRow::Bots.help(setup),
            SalonRow::Bots.help(RoomSetup {
                bots: false,
                ..setup
            })
        );
    }
}
