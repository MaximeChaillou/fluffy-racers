//! Issue #101: what the lobby's screen shows (`docs/UI_CHARTE.md` § « Le salon (#101) »), in
//! pure logic (unit-tested): [`salon_view`] reads the room (the session, its lobby, its race) and
//! gives the screen its title, its lines, the host's settings, its buttons (the main action
//! first, disabled with its reason), the players and the problems of connection. The tree
//! (`screen.rs`) only draws it. The texts are those of the egui window before (issues #7, #49,
//! #63, #64, #80, #99), after the charter's tone: short, « tu », no jargon.

use std::borrow::Cow;

use crate::characters::{PartKind, Roster};
use crate::kart::KartColor;
use crate::modes::{Catalog, MenuRow};
use crate::net::championship::{self, race_setup_of, title_of};
use crate::net::{
    Diagnosis, MAX_PLAYERS, NetSession, NetStatus, OnlineRace, RaceStage, RoomFacts, RoomSetup,
    RowLink, Seated, Stage, Stuck, color_notice, settings_line,
};

/// A setting of the room, chosen by the host (issue #99). Issue #122: the mode at the top, and
/// the championship's lines: those of « Jouer solo » (`modes::MenuRow`, `modes::RaceSetup`),
/// without the time trial (`net::championship`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SalonRow {
    /// « Circuit seul » or « Championnat ».
    Mode,
    /// A championship: a cup or a custom one.
    Formula,
    Cup,
    /// A custom championship: its number of races...
    Races,
    /// ...drawn or chosen...
    Pick,
    /// ...and, chosen, the circuit of each race (0 = the first): on the right, with the program.
    Race(usize),
    /// The circuit of a race alone.
    Circuit,
    Mirror,
    Bots,
    Items,
}

impl SalonRow {
    /// The lines of `setup`, in order: the mode, what it needs, the mirror, the bots, the items,
    /// and a custom championship's races when chosen one by one (`net::championship::rows`).
    pub fn rows(setup: &RoomSetup) -> Vec<Self> {
        championship::rows(setup)
            .into_iter()
            .map(Self::of)
            .collect()
    }

    fn of(row: MenuRow) -> Self {
        match row {
            MenuRow::Mode => SalonRow::Mode,
            MenuRow::Formula => SalonRow::Formula,
            MenuRow::Cup => SalonRow::Cup,
            MenuRow::Races => SalonRow::Races,
            MenuRow::Pick => SalonRow::Pick,
            MenuRow::Race(race) => SalonRow::Race(race),
            MenuRow::Circuit => SalonRow::Circuit,
            MenuRow::Mirror => SalonRow::Mirror,
            MenuRow::Bots => SalonRow::Bots,
            MenuRow::Items => SalonRow::Items,
        }
    }

    /// The line of « Jouer solo » it is.
    pub fn menu(self) -> MenuRow {
        match self {
            SalonRow::Mode => MenuRow::Mode,
            SalonRow::Formula => MenuRow::Formula,
            SalonRow::Cup => MenuRow::Cup,
            SalonRow::Races => MenuRow::Races,
            SalonRow::Pick => MenuRow::Pick,
            SalonRow::Race(race) => MenuRow::Race(race),
            SalonRow::Circuit => MenuRow::Circuit,
            SalonRow::Mirror => MenuRow::Mirror,
            SalonRow::Bots => MenuRow::Bots,
            SalonRow::Items => MenuRow::Items,
        }
    }

    /// One of a custom championship's races (on the right, with the program).
    pub fn is_race(self) -> bool {
        matches!(self, SalonRow::Race(_))
    }

    pub fn label(self) -> Cow<'static, str> {
        self.menu().label()
    }

    /// Its value in `setup`, written.
    pub fn value(self, setup: RoomSetup, catalog: &Catalog) -> String {
        match self {
            SalonRow::Circuit => catalog
                .circuits
                .get(usize::from(setup.track))
                .map_or_else(|| "?".to_string(), |entry| entry.name.clone()),
            row => race_setup_of(&setup).value(row.menu(), catalog),
        }
    }

    /// `setup` with this setting `step` further (the circuits, the cups, the races around; the
    /// others switch); issue #122, a championship drawn at random from `seed` (the room's).
    pub fn changed(self, setup: RoomSetup, step: i32, seed: u64, catalog: &Catalog) -> RoomSetup {
        championship::changed(setup, self.menu(), step, seed, catalog)
    }

    /// What it is for (the help under the lines, for the host).
    pub fn help(self, setup: RoomSetup, catalog: &Catalog) -> String {
        match self {
            SalonRow::Mode if setup.championship => {
                "Des courses, des points, puis le podium.".to_string()
            }
            SalonRow::Mode => "Une course, sur le circuit de ton choix.".to_string(),
            SalonRow::Circuit => "La prochaine course, pour tout le monde.".to_string(),
            SalonRow::Mirror => "Le circuit à l'envers, gauche et droite.".to_string(),
            SalonRow::Bots if setup.bots => "Des bots complètent la grille de 12.".to_string(),
            SalonRow::Bots => "Seuls les joueurs courent.".to_string(),
            SalonRow::Items if setup.items => "Les boîtes à objets sur la piste.".to_string(),
            SalonRow::Items => "Aucune boîte ; les croquettes restent.".to_string(),
            SalonRow::Pick if !setup.plan.chosen => {
                "Tirés au sort par l'hôte, chacun une fois.".to_string()
            }
            row => race_setup_of(&setup).help(row.menu(), catalog),
        }
    }
}

/// Issue #122: what a championship of the room races, for the right column: its title and its
/// circuits in order (« 1 · Prairie Pompon »...).
fn program_of(setup: &RoomSetup, catalog: &Catalog) -> (String, Vec<String>) {
    if !setup.championship {
        return (String::new(), Vec::new());
    }
    let races = setup.plan.program.as_slice();
    let title = format!(
        "Au programme · {} course{}",
        races.len(),
        plural(races.len())
    );
    let lines = races
        .iter()
        .enumerate()
        .map(|(race, circuit)| format!("{} · {}", race + 1, catalog.name(usize::from(*circuit))))
        .collect();
    (title, lines)
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
    /// Issue #136: « Commandes »: the game's commands (the options' list), instead of the line of
    /// keys at the bottom of the scene.
    Controls,
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
    /// The circuits and the cups (issue #122) the host chooses from.
    pub catalog: &'a Catalog,
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
    /// Issue #122: its lines, in order (`SalonRow::rows`), their values and what they are for
    /// (the same order).
    pub rows: Vec<SalonRow>,
    pub values: Vec<String>,
    pub helps: Vec<String>,
    /// We host the room and no race runs: the settings change.
    pub editable: bool,
    /// Why they cannot be changed (a client, a race on).
    pub locked: Option<&'static str>,
    /// Issue #122: a championship's program, for the right column: its header (empty: not a
    /// championship) and its races (« 1 · Prairie Pompon »).
    pub program_title: String,
    pub program: Vec<String>,
}

impl Seat {
    /// The value of its line `row`, written (empty: not one of its lines).
    pub fn value(&self, row: SalonRow) -> &str {
        self.rows
            .iter()
            .position(|r| *r == row)
            .and_then(|index| self.values.get(index))
            .map_or("", String::as_str)
    }

    /// What its line `row` is for.
    pub fn help(&self, row: SalonRow) -> &str {
        self.rows
            .iter()
            .position(|r| *r == row)
            .and_then(|index| self.helps.get(index))
            .map_or("", String::as_str)
    }
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
            "Ton pseudo : {} · change-le dans Pilote.",
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
            .catalog
            .circuits
            .get(usize::from(track))
            .map_or("?", |entry| entry.name.as_str())
    };
    // Issue #122: a championship in progress (we watch it if we came meanwhile).
    let championship = lobby.championship();
    match (lobby.announced(), seated.race) {
        (Some(start), RaceStage::Racing | RaceStage::Watching) => {
            let race = match (championship, start.round) {
                (Some(state), round) if round > 0 => {
                    format!("Championnat, course {round}/{}", state.setup.program.len())
                }
                _ => format!("Course n° {}", start.race),
            };
            view.lines.push(format!(
                "{race} en cours sur {} : {}.",
                name(start.track),
                crate::net::race_line(start)
            ));
            if seated.race == RaceStage::Watching {
                view.lines.push(if championship.is_some() {
                    "Tu es arrivé pendant le championnat : tu seras du prochain.".to_string()
                } else {
                    "Tu es arrivé pendant la course : tu seras de la prochaine.".to_string()
                });
            }
        }
        (Some(start), _) => {
            view.lines
                .push(format!("Course n° {} terminée.", start.race));
        }
        (None, _) => {
            view.lines
                .push("En attendant, promène-toi et donne des coups de patte !".to_string());
        }
    }
    let racing = matches!(seated.race, RaceStage::Racing | RaceStage::Watching);
    let setup = lobby.setup();
    let (program_title, program) = program_of(&setup, input.catalog);
    let rows = SalonRow::rows(&setup);
    view.seat = Some(Seat {
        setup,
        values: rows.iter().map(|r| r.value(setup, input.catalog)).collect(),
        helps: rows.iter().map(|r| r.help(setup, input.catalog)).collect(),
        rows,
        program_title,
        program,
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
        // Issue #122: a championship's name and its races (its lines say the rest: the panel
        // has no room for more), or the circuit of a race alone and its settings.
        view.lines.push(if setup.championship {
            let races = setup.plan.program.len();
            format!(
                "Réglages de l'hôte : {}, {races} course{}.",
                title_of(&setup.plan, input.catalog),
                plural(races)
            )
        } else {
            format!(
                "Réglages de l'hôte : {} · {}",
                name(setup.track),
                settings_line(setup)
            )
        });
    }
    if seated.hosting {
        let blocker = seated.start_blocker();
        let ready = if setup.championship {
            format!(
                "Tout le monde est prêt : {} courses !",
                setup.plan.program.len()
            )
        } else {
            "Tout le monde est prêt !".to_string()
        };
        view.buttons.push(ButtonView {
            kind: ButtonKind::StartRace,
            label: "Lancer la course".to_string(),
            caption: blocker.clone().unwrap_or(ready),
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
    // Issue #136: the commands of the game (to walk, jump, strike here too), instead of the keys
    // written at the bottom of the scene.
    view.buttons.push(button(
        ButtonKind::Controls,
        "Commandes",
        "Marcher, sauter, donner un coup de patte.",
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
        view.notes.push(format!("{notice} Change-la dans Pilote."));
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

    /// The game's circuits and cups (issue #122: the lobby chooses among them).
    fn circuits() -> Catalog {
        Catalog::game(4)
    }

    fn input<'a>(session: Option<&'a NetSession>, catalog: &'a Catalog) -> ViewInput<'a> {
        ViewInput {
            session,
            race: None,
            roster: None,
            catalog,
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
        // The in-memory socket runs on the I/O pool (alone, these tests start it themselves).
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
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
        // Every line of every mode (issue #122: the championship's too).
        let catalog = circuits();
        let single = RoomSetup::default();
        let cup = SalonRow::Mode.changed(single, 1, 7, &catalog);
        let custom = SalonRow::Formula.changed(cup, 1, 7, &catalog);
        let chosen = SalonRow::Pick.changed(custom, 1, 7, &catalog);
        for setup in [single, cup, custom, chosen] {
            for setup in [
                setup,
                RoomSetup {
                    bots: false,
                    items: false,
                    ..setup
                },
            ] {
                for row in SalonRow::rows(&setup) {
                    assert!(row.label().chars().count() <= 16, "{row:?}");
                    let help = row.help(setup, &catalog);
                    assert!(help.chars().count() < 48, "{row:?}: {help}");
                    assert!(!row.value(setup, &catalog).is_empty(), "{row:?}");
                }
            }
        }
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
    /// « Commandes » (issue #136), « Quitter » and « Copier » follow; the players listed with
    /// their link.
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
                ButtonKind::Controls,
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
        // Issue #136: no key in its lines (« Commandes » lists them).
        for line in &view.lines {
            for key in ["(G)", "(L)", "(X ou F)", "Entrée", "Échap"] {
                assert!(!line.contains(key), "{line}");
            }
        }
        assert_short(&view);
        // « Copier » says it was.
        let copied = salon_view(&ViewInput {
            copied: true,
            ..input(Some(&session), &circuits)
        });
        // (Found by its kind: issue #136 put « Commandes » before it.)
        let copy = copied.buttons.iter().find(|b| b.kind == ButtonKind::Copy);
        assert_eq!(copy.map(|b| b.caption.as_str()), Some("Copié !"));
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
            round: 0,
            lineup: Vec::new(),
            players: vec![ANA],
        });
        let racing = salon_view(&input(Some(&session), &circuits));
        let seat = racing.seat.unwrap();
        assert!(!seat.editable && seat.locked.is_some());
        assert!(racing.lines.iter().any(|l| l.contains("en cours")));
    }

    /// The settings' lines: their values written, a step changes them (the circuits around),
    /// each says what it is for. Issue #122: the values and the steps come with the cups
    /// (`Catalog`) and the room's seed (a championship drawn at random).
    #[test]
    fn the_settings_change_around() {
        let catalog = circuits();
        let circuits = &catalog.circuits;
        let setup = RoomSetup::default();
        assert_eq!(SalonRow::Circuit.value(setup, &catalog), circuits[0].name);
        assert_eq!(SalonRow::Mirror.value(setup, &catalog), "Non");
        assert_eq!(SalonRow::Bots.value(setup, &catalog), "Oui");
        let back = SalonRow::Circuit.changed(setup, -1, 7, &catalog);
        assert_eq!(usize::from(back.track), circuits.len() - 1);
        assert_eq!(
            SalonRow::Circuit.changed(back, 1, 7, &catalog).track,
            setup.track
        );
        assert!(SalonRow::Mirror.changed(setup, 1, 7, &catalog).mirror);
        assert!(!SalonRow::Bots.changed(setup, -1, 7, &catalog).bots);
        assert!(!SalonRow::Items.changed(setup, 1, 7, &catalog).items);
        assert_ne!(
            SalonRow::Bots.help(setup, &catalog),
            SalonRow::Bots.help(
                RoomSetup {
                    bots: false,
                    ..setup
                },
                &catalog
            )
        );
    }

    /// Issue #122: the mode at the top of the host's lines: « Circuit seul » (the circuit, as
    /// before) or « Championnat » (a cup of the manifest, or a custom one: its races, drawn by the
    /// host or chosen one by one, on the right with the program); everyone sees the host's
    /// choice, the program and its name; the time trial is not offered.
    #[test]
    fn the_host_chooses_a_championship_and_everyone_sees_its_program() {
        let catalog = circuits();
        let single = RoomSetup::default();
        assert_eq!(
            SalonRow::rows(&single),
            [
                SalonRow::Mode,
                SalonRow::Circuit,
                SalonRow::Mirror,
                SalonRow::Bots,
                SalonRow::Items
            ]
        );
        assert_eq!(SalonRow::Mode.value(single, &catalog), "Circuit seul");
        let cup = SalonRow::Mode.changed(single, 1, 7, &catalog);
        assert_eq!(SalonRow::Mode.value(cup, &catalog), "Championnat");
        assert_eq!(SalonRow::Formula.value(cup, &catalog), "Une coupe");
        assert_eq!(SalonRow::Cup.value(cup, &catalog), "Coupe Doudou");
        assert!(
            !SalonRow::Mode.changed(cup, 1, 7, &catalog).championship,
            "Championnat → Circuit seul: never the time trial"
        );
        let custom = SalonRow::Formula.changed(cup, 1, 7, &catalog);
        assert_eq!(SalonRow::Pick.value(custom, &catalog), "Au hasard");
        assert!(SalonRow::Pick.help(custom, &catalog).contains("l'hôte"));
        let chosen = SalonRow::Pick.changed(custom, 1, 7, &catalog);
        let rows = SalonRow::rows(&chosen);
        let races: Vec<SalonRow> = rows.iter().copied().filter(|r| r.is_race()).collect();
        assert_eq!(races.len(), chosen.plan.program.len());
        assert_eq!(
            SalonRow::Race(0).value(chosen, &catalog),
            catalog.name(usize::from(chosen.plan.program.as_slice()[0]))
        );

        // The host's view: its lines and the program on the right.
        let mut host = session(NetStatus::InRoom, true);
        host.lobby = Some(Lobby::new(ANA, Profile::default(), 0, true));
        host.lobby.as_mut().unwrap().choose_setup(cup);
        let view = salon_view(&input(Some(&host), &catalog));
        let seat = view.seat.as_ref().unwrap();
        assert_eq!(seat.rows, SalonRow::rows(&cup));
        assert_eq!(seat.program_title, "Au programme · 4 courses");
        assert_eq!(seat.program[0], format!("1 · {}", catalog.name(0)));
        assert_eq!(seat.program.len(), 4);
        assert_short(&view);
        // A guest sees them too, and the championship in its line.
        let mut guest = session(NetStatus::InRoom, false);
        let mut lobby = Lobby::new(PeerKey(9), Profile::default(), 0, false);
        lobby.connect(ANA, 0);
        lobby.receive(ANA, crate::net::Message::Seats(vec![ANA, PeerKey(9)]), 0);
        lobby.receive(ANA, crate::net::Message::Setup(custom), 0);
        assert!(lobby.seated());
        guest.lobby = Some(lobby);
        let view = salon_view(&input(Some(&guest), &catalog));
        let seat = view.seat.as_ref().unwrap();
        assert!(!seat.editable);
        assert_eq!(seat.setup, custom);
        assert_eq!(seat.program.len(), custom.plan.program.len());
        let line = view
            .lines
            .iter()
            .find(|l| l.starts_with("Réglages de l'hôte"))
            .unwrap();
        assert!(
            line.contains(&format!(
                "Championnat, {} courses",
                custom.plan.program.len()
            )),
            "{line}"
        );
        // A race alone has no program.
        assert!(program_of(&single, &catalog).1.is_empty());
    }
}
