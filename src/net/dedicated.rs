//! Issue #225: the **dedicated server** (`docs/ARCHITECTURE.md` § « Serveur dédié »,
//! `docs/SERVEUR.md`): a host of the star without a kart nor a seat, on a machine of its own,
//! without a window, a renderer, sounds or a GPU (the headless world of the tests, its simulation
//! in `FixedUpdate` at 60 Hz). It keeps everything the host does: the seats and their list, the
//! relay and the bundles of the kart states (`relay.rs`), the bots it drives, the arbitration of
//! the boxes, the croquettes and the race, the room's clock, the settings and the championship.
//!
//! - **Always on** ([`open_the_room`]): it sits in its room ([`SERVER_ROOM`](super::SERVER_ROOM) on its own signaling)
//!   from the start, and joins it again whenever the signaling drops it.
//! - **A game created from a client** (`lobby.rs`): « Créer une partie » goes to the server's
//!   room; the first player seated leads the game (its settings, its start), the next one when it
//!   leaves. A player who came to create while a game is on is told so, and joins it or leaves.
//! - **What it shows** ([`serve_the_room`]): nothing between the races (an empty level, the
//!   lobby's scene is the clients'); the race announced: its circuit with its bots, the players'
//!   karts as remote bodies (what the bots, the items and the arbitration see).
//! - **What a host does from its screens** ([`run_the_server`]): back to the lobby after the
//!   results, a championship's next race, its podium, then the lobby, on the same timings as the
//!   host's lobby (`NetTuning::server_*`), or at once when its leader says so; « Lancer la course »
//!   when its leader asks and everyone is ready. Everyone gone, it forgets the game and waits.

use bevy::prelude::*;

use super::championship::{after_a_championship, grid_lineup, room_seed};
use super::foot::FootSync;
use super::items_online::OnlineItems;
use super::online::{circuit, items_on, set_race_roster};
use super::sync::{KartSync, RemoteKartProxy, virtual_ms};
use super::{
    IceServers, KartId, NetSession, NetSettings, NetSocket, NetStatus, NetSystems, NetTuning,
    OnlineRace, OnlineSystems, RoomCode, StunChoice, championship_to_start, now_ms, random_seed,
    room_url, session_of, start_race_now,
};
use crate::bots::BotDifficulty;
use crate::characters::Roster;
use crate::interactions::InteractionClock;
use crate::kart::OnlinePlayerColors;
use crate::level::{LoadLevel, Online};
use crate::modes::Catalog;
use crate::race::{GrandPrixTuning, OnlineFlow, RaceGrid, RaceMode, RaceTuning};
use crate::track::{GRID_SLOTS, MirrorMode, TrackChoice};

/// Issue #225: this game is a dedicated server: its room, its signaling, where it is.
#[derive(Resource, Clone, Debug)]
pub struct DedicatedServer {
    /// Its room on its signaling.
    pub room: RoomCode,
    /// Its signaling, as it reaches it (the one it embeds: `ws://127.0.0.1:3536`).
    pub signal: String,
    /// Its STUN servers (none by default: a server with a public address needs none).
    pub stun: StunChoice,
    /// When it joins its room again (real time, s), after the signaling dropped it.
    retry_at: Option<f64>,
    /// Who it is in its room now (a new session, a new id).
    me: Option<super::PeerKey>,
    /// The race it built (`Some(0)`: the empty level between the races; `None`: nothing yet).
    showing: Option<u16>,
}

impl DedicatedServer {
    pub fn new(room: RoomCode, signal: String) -> Self {
        Self {
            room,
            signal,
            stun: StunChoice::Off,
            retry_at: None,
            me: None,
            showing: None,
        }
    }
}

/// The dedicated server's systems (see the module's documentation). The game's own player does
/// not exist here (`crate::spawn_player`), nor the online driving of a seated player
/// (`online::online_mode`).
pub struct DedicatedPlugin;

impl Plugin for DedicatedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                open_the_room.before(NetSystems),
                serve_the_room.after(NetSystems).in_set(OnlineSystems),
                run_the_server.after(OnlineSystems),
            ),
        )
        .add_systems(Last, rest_while_empty);
    }
}

/// Nobody in the room (no peer, no connection being made): the frame lasts
/// `NetTuning::server_idle_frame_ms` at least, the server waits almost without a cost.
fn rest_while_empty(session: Option<Res<NetSession>>, tuning: Res<NetTuning>) {
    let empty = session.as_deref().is_some_and(|session| {
        session.links.pending().is_empty()
            && session
                .lobby
                .as_ref()
                .is_some_and(|lobby| lobby.peers().is_empty())
    });
    let ms = tuning.server_idle_frame_ms;
    if empty && ms > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f32(
            ms.min(1_000.0) / 1_000.0,
        ));
    }
}

/// This game is a dedicated server.
pub fn dedicated(server: Option<Res<DedicatedServer>>) -> bool {
    server.is_some()
}

/// A session of the server in its room, through `socket`.
pub fn server_session(server: &DedicatedServer, ice: IceServers, socket: NetSocket) -> NetSession {
    let mut session = NetSession::new(
        server.room.clone(),
        server.signal.clone(),
        ice,
        socket,
        true,
    );
    session.dedicated = true;
    session
}

/// In its room from the start; again `NetTuning::server_retry_s` after the signaling dropped it
/// (or could not be reached: the server it embeds starts at the same time).
fn open_the_room(
    mut commands: Commands,
    mut server: ResMut<DedicatedServer>,
    session: Option<Res<NetSession>>,
    settings: Res<NetSettings>,
    tuning: Res<NetTuning>,
    time: Res<Time<Real>>,
) {
    if session
        .as_ref()
        .is_some_and(|session| session.status.usable())
    {
        if server.retry_at.is_some() {
            server.retry_at = None;
        }
        return;
    }
    let now = time.elapsed_secs_f64();
    match server.retry_at {
        None if session.is_some() => {
            warn!(
                "server: out of room {} ({:?}): again in {} s",
                server.room,
                session.as_ref().map(|session| &session.status),
                tuning.server_retry_s
            );
            server.retry_at = Some(now + f64::from(tuning.server_retry_s.max(0.1)));
            return;
        }
        Some(at) if now < at => return,
        _ => {}
    }
    server.retry_at = None;
    let ice = IceServers::choose(&server.stun, settings.turn.as_ref(), &server.signal);
    info!(
        "server: in room {} on {}, {}",
        server.room,
        server.signal,
        ice.describe()
    );
    let socket = NetSocket::open(
        &room_url(&server.signal, &server.room),
        settings.attempts,
        &ice,
        true,
    );
    commands.insert_resource(server_session(&server, ice, socket));
}

/// The server's room: the resources of a game online once it has its id (no kart of its own:
/// the bots and the players' bodies); the race announced built with its bots, an empty level
/// between the races; every frame, the room's clock, the karts it shows, no pause. Out of the
/// room (or another id): all of it gone.
#[allow(clippy::too_many_arguments)]
fn serve_the_room(
    mut commands: Commands,
    mut server: ResMut<DedicatedServer>,
    session: Option<Res<NetSession>>,
    mut sync: Option<ResMut<KartSync>>,
    race: Option<Res<OnlineRace>>,
    mut interaction_clock: Option<ResMut<InteractionClock>>,
    (mut grid, mut mirror): (ResMut<RaceGrid>, ResMut<MirrorMode>),
    roster: Option<Res<Roster>>,
    real: Res<Time<Real>>,
    mut time: ResMut<Time<Virtual>>,
    mut levels: MessageWriter<LoadLevel>,
    proxies: Query<Entity, With<RemoteKartProxy>>,
) {
    let lobby = session
        .as_deref()
        .filter(|session| session.status == NetStatus::InRoom)
        .and_then(|session| session.lobby.as_ref())
        .filter(|lobby| lobby.is_dedicated());
    let Some(lobby) = lobby.filter(|lobby| server.me.is_none_or(|me| me == lobby.me())) else {
        if server.me.take().is_some() || server.showing.is_none() {
            info!("server: out of the room, nothing built");
            for proxy in &proxies {
                commands.entity(proxy).try_despawn();
            }
            commands.remove_resource::<KartSync>();
            commands.remove_resource::<OnlineItems>();
            commands.remove_resource::<FootSync>();
            commands.remove_resource::<OnlineRace>();
            commands.remove_resource::<OnlineFlow>();
            commands.remove_resource::<Online>();
            commands.remove_resource::<OnlinePlayerColors>();
            *grid = RaceGrid {
                bots: 0,
                lineup: None,
                ..*grid
            };
            // The lobby's scene is the clients': nothing here, nothing to simulate.
            levels.write(LoadLevel::on(TrackChoice::Salon, RaceMode::FreeRoam));
            server.showing = Some(0);
        }
        return;
    };
    let me = lobby.me();
    if server.me.is_none() {
        info!("server: in the room as {me:?}");
        server.me = Some(me);
        // Its kart id is nobody's: it has no kart (the last slot, as a watcher's).
        commands.insert_resource(KartSync::new(me, (GRID_SLOTS - 1) as KartId, grid.seed));
        commands.insert_resource(OnlineItems::default());
        // Issue #101: it relays the characters on foot of the lobby.
        commands.insert_resource(FootSync::new(me));
        commands.insert_resource(Online);
        return;
    }
    let Some(sync) = sync.as_deref_mut() else {
        return;
    };
    // Online, the game never pauses nor slows down (issue #9).
    if time.is_paused() {
        time.unpause();
    }
    if time.relative_speed() != 1.0 {
        time.set_relative_speed(1.0);
    }
    if let Some(room_ms) = lobby.room_time_ms(now_ms(&real)) {
        sync.clock
            .observe((room_ms as f64 - virtual_ms(&time)) / super::interp::TICK_MS);
    }
    if let (Some(clock), Some(offset)) = (interaction_clock.as_mut(), sync.clock.offset()) {
        clock.set_if_neq(InteractionClock { offset });
    }
    let wanted = lobby.announced().map_or(0, |start| start.race);
    if server.showing != Some(wanted) {
        server.showing = Some(wanted);
        match lobby.announced() {
            Some(start) => {
                let Some(roster) = roster.as_deref() else {
                    server.showing = None;
                    return;
                };
                let mut race = OnlineRace::new(start.clone(), me, true, roster);
                // It runs the race (its bots, its end, its results) without a kart in it.
                race.racing = true;
                race.bots_here = true;
                mirror.set_if_neq(MirrorMode(start.mirror));
                items_on(&mut commands, start.items);
                let lineup = lobby
                    .championship()
                    .filter(|_| start.round > 0)
                    .map(|state| grid_lineup(state, start));
                *grid = RaceGrid {
                    bots: usize::from(start.bots),
                    seed: start.seed,
                    lineup,
                    difficulty: BotDifficulty::from_index(start.difficulty).unwrap_or_default(),
                };
                sync.set_session(session_of(start.race));
                info!(
                    "server: race {} on circuit {}, {} players and {} bots, starting at tick {}",
                    start.race,
                    start.track,
                    start.players.len(),
                    start.bots,
                    start.start_tick
                );
                commands.insert_resource(OnlineFlow::default());
                commands.insert_resource(race);
                levels.write(LoadLevel::on(circuit(start.track), RaceMode::Race));
                return;
            }
            None => {
                info!("server: the lobby, nothing built");
                commands.remove_resource::<OnlineRace>();
                commands.remove_resource::<OnlineFlow>();
                *grid = RaceGrid {
                    bots: 0,
                    lineup: None,
                    ..*grid
                };
                sync.set_session(0);
                sync.set_roster(std::iter::empty());
                levels.write(LoadLevel::on(TrackChoice::Salon, RaceMode::FreeRoam));
            }
        }
    }
    if let Some(race) = race
        .as_deref()
        .filter(|race| Some(race.shared.race()) == server.showing)
    {
        set_race_roster(sync, lobby, race);
    }
}

/// What [`run_the_server`] waits on: the race (its number), the races scored, the podium; since
/// when (real time, s); whether it moved on already.
type Waiting = ((u16, u8, bool), f64, bool);

/// What the host does from its screens, done by the server (see the module's documentation): its
/// leader's « Lancer la course » (everyone ready, no race running), back to the lobby after the
/// results, a championship's next race, its podium, the lobby again after it.
#[allow(clippy::too_many_arguments)]
fn run_the_server(
    session: Option<ResMut<NetSession>>,
    race: Option<Res<OnlineRace>>,
    tuning: Res<NetTuning>,
    race_tuning: Option<Res<RaceTuning>>,
    cups: Option<Res<GrandPrixTuning>>,
    time: Res<Time<Real>>,
    mut waiting: Local<Option<Waiting>>,
) {
    let Some(mut session) = session else {
        return;
    };
    let room = room_seed(&session.room);
    let Some(lobby) = session.lobby.as_mut().filter(|lobby| lobby.is_dedicated()) else {
        return;
    };
    let ordered = lobby.take_start_order();
    let now = time.elapsed_secs_f64();
    let catalog = || Catalog::game(cups.as_deref().map_or(4, |tuning| tuning.races as usize));
    let start_now = |lobby: &mut super::Lobby| {
        let seed = random_seed(time.elapsed().as_nanos() as u64);
        if let Some(championship) = championship_to_start(lobby, seed) {
            info!(
                "server: a championship of {} races, {} players and {} bots",
                championship.state.setup.program.len(),
                championship.state.players.len(),
                championship.state.bots
            );
            lobby.start_championship(championship);
        }
        start_race_now(lobby, now_ms(&time), seed, race_tuning.as_deref(), &tuning);
    };
    let Some(start) = lobby.announced().cloned() else {
        *waiting = None;
        // The lobby: its leader starts, once everyone is ready.
        if ordered {
            if lobby.all_ready() {
                start_now(lobby);
            } else {
                info!("server: the leader asks to start, not everyone is ready");
            }
        }
        return;
    };
    let closed = race
        .as_deref()
        .filter(|race| race.shared.race() == start.race)
        .is_some_and(|race| race.shared.results().is_some());
    let stage = match lobby.championship() {
        // A race of the championship: once scored (its standings), or its podium.
        Some(state) if start.round > 0 => {
            if state.scored < start.round && !state.podium {
                *waiting = None;
                return;
            }
            let finished = usize::from(state.scored) >= state.setup.program.len();
            Some((state.scored, state.podium, finished))
        }
        _ if closed => None,
        _ => {
            // A race running: nothing to do.
            *waiting = None;
            return;
        }
    };
    let key = (
        start.race,
        stage.map_or(0, |(scored, ..)| scored),
        stage.is_some_and(|(_, podium, _)| podium),
    );
    let (since, done) = match *waiting {
        Some((shown, since, done)) if shown == key => (since, done),
        _ => {
            *waiting = Some((key, now, false));
            (now, false)
        }
    };
    if done {
        return;
    }
    let wait = match stage {
        Some((_, true, _)) => tuning.server_podium_s,
        Some(_) => tuning.server_standings_s,
        None => tuning.server_results_s,
    };
    if !ordered && now - since < f64::from(wait.max(0.0)) {
        return;
    }
    *waiting = Some((key, since, true));
    match stage {
        Some((_, true, _)) => {
            info!("server: the championship is over, everyone back in the lobby");
            let next = after_a_championship(lobby.setup(), room, &catalog());
            lobby.choose_setup(next);
        }
        Some((_, false, true)) => {
            info!("server: the championship's podium");
            lobby.championship_podium();
        }
        Some(_) => {
            info!("server: the championship's next race");
            start_now(lobby);
        }
        // A race alone: its leader starts the next one at once (everyone ready), or everyone
        // goes back to the lobby.
        None if ordered && lobby.all_ready() => start_now(lobby),
        None => {
            info!(
                "server: race {} over, everyone back in the lobby",
                start.race
            );
            let setup = lobby.setup();
            lobby.choose_setup(setup);
        }
    }
}

/// Issue #225: what the server's command line asks for (`fluffy-racers-server --help`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerOptions {
    /// `--port 3536`: the port of the signaling it embeds (the WebSocket the players reach).
    pub port: u16,
    /// `--signal ws://…`: another signaling (a `matchbox_server` of its own): none embedded.
    pub signal: Option<String>,
    /// `--room SERVEUR`: its room ([`SERVER_ROOM`](super::SERVER_ROOM) by default: the one the game knows).
    pub room: RoomCode,
    /// `--stun …` (as the game's, `ice.rs`): a server behind a box at home needs a STUN server to
    /// be reached from outside; none by default (a server with a public address).
    pub stun: StunChoice,
    /// `--help`.
    pub help: bool,
    /// What could not be read, for a message.
    pub refused: Vec<String>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            port: super::SIGNAL_PORT,
            signal: None,
            room: super::server_room(),
            stun: StunChoice::Off,
            help: false,
            refused: Vec::new(),
        }
    }
}

/// The server's help (`--help`).
pub const SERVER_HELP: &str =
    "fluffy-racers-server : le serveur dédié de Fluffy Racers (docs/SERVEUR.md).

Options :
  --port 3536          le port de la mise en relation (WebSocket), à ouvrir en TCP
  --room SERVEUR       la partie (celle que le jeu connaît par défaut)
  --stun stun:…        un serveur STUN (un serveur derrière une box), aucun par défaut
  --signal ws://…      une mise en relation à part (matchbox_server), sans la sienne
  --help               cette aide";

impl ServerOptions {
    /// From the command line's arguments (without the program's name): `--key value` or
    /// `--key=value`. An unknown or unreadable one is kept in `refused`.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let Some(flag) = arg.strip_prefix("--") else {
                options.refused.push(arg);
                continue;
            };
            let (key, value) = match flag.split_once('=') {
                Some((key, value)) => (key.to_string(), Some(value.to_string())),
                None => (flag.to_string(), None),
            };
            if key == "help" {
                options.help = true;
                continue;
            }
            let Some(value) = value.or_else(|| args.next()) else {
                options.refused.push(arg);
                continue;
            };
            let read = match key.as_str() {
                "port" => value.parse().ok().map(|port| options.port = port),
                "signal" => super::parse_signal(&value).map(|signal| options.signal = Some(signal)),
                "room" => RoomCode::parse(&value).map(|room| options.room = room),
                "stun" => StunChoice::parse(&value).map(|stun| options.stun = stun),
                _ => None,
            };
            if read.is_none() {
                options.refused.push(format!("--{key} {value}"));
            }
        }
        options
    }

    /// The signaling the server joins its room through: its own (`ws://127.0.0.1:PORT`), or the
    /// one given.
    pub fn signal_url(&self, port: u16) -> String {
        self.signal
            .clone()
            .unwrap_or_else(|| format!("ws://127.0.0.1:{port}"))
    }
}
