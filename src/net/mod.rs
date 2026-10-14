//! Network, part 1 of 3 (issue #7, `docs/ARCHITECTURE.md` § « Réseau »): rooms, peer-to-peer
//! connection and the lobby. The race itself stays local for now: the karts are synchronized
//! with issue #8, the race with issue #9.
//!
//! - **Room**: `?room=CODE` in the page address, `--room CODE` natively; « Créer une partie »
//!   draws a code and gives the invitation link (`room.rs`). The signaling server
//!   (`ws://localhost:3536` by default, `?signal=` / `--signal` for another one) only introduces
//!   the peers to each other.
//! - **Socket** (`socket.rs`): `matchbox_socket`, full mesh, a reliable and an unreliable
//!   channel, polled each frame from a system, natively and in the browser.
//! - **Direct connections** (issue #49): the STUN and TURN servers (`ice.rs`: none on a local
//!   network, `?stun=`, `?turn=`), what goes through the signaling server (`signaling.rs`) and
//!   the connections being made (`links.rs`): each step in the console, and in the lobby a peer
//!   that does not connect, with the leads (README, « Dépannage »). In the browser, a small shim
//!   of the page (`index.html`, read by `browser.rs`) completes `matchbox_socket`.
//! - **Protocol** (`protocol.rs`): compact, versioned messages; **lobby** (`lobby.rs`): seats,
//!   host (the smallest `PeerId`), « Prêt », ping and common clock (`clock.rs`), arrivals and
//!   departures, the room full at 12. All of it is pure logic, unit-tested without Bevy.
//! - **Window** (`ui.rs`, only with a renderer): **L** (« en ligne ») opens and closes it (same
//!   letter at the same place in AZERTY and QWERTY, no Fn on a Mac); it opens by itself with a
//!   room in the address. It does not pause the game, and costs nothing once closed (no egui
//!   context, like the other windows).
//!
//! - **Kart synchronization** (issue #8, `sync.rs`, [`KartSyncPlugin`] of the game): once seated,
//!   everyone drives freely on the same circuit and sees the others' karts. Each peer sends its
//!   kart's state 30 times a second ([`KartSnapshot`], `snapshot.rs`) on the unreliable channel;
//!   the others show it about 100 ms later, interpolated (`interp.rs`), as a kinematic body our
//!   kart bumps into. A network simulator (`netsim.rs`, `?netsim=120,20,5`) adds latency,
//!   jitter and losses for the debug.
//! - **The race online** (issue #9, `online.rs`, `race.rs`): the host chooses the room's circuit
//!   and starts the races; everyone races on the same grid, on the room's clock (`clock.rs`), with
//!   the host's bots; checkpoints and the end go to everyone (reliable), the same results on
//!   every screen; the strikes and grabs between karts go to the peer that decides. A new host
//!   takes the bots and the arbitration over. Online, the game never pauses.
//!
//! **Solo is unchanged**: without a room there is no socket; only the L key and the reading of
//! the actions run each frame, the rest needs a [`NetSession`] (measured: no cost in the
//! browser's single-threaded executor, STATUS.md « Métriques »).

mod browser;
mod clock;
mod ice;
pub mod interactions;
mod interp;
mod items_online;
mod links;
mod lobby;
mod netsim;
mod online;
mod pickups;
mod protocol;
mod race;
mod room;
mod signaling;
mod snapshot;
mod socket;
mod sync;
mod tuning;
mod ui;

#[cfg(test)]
mod item_tests;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod scenario_tests;
#[cfg(test)]
mod sync_tests;

pub use clock::{PeerClock, ROOM_WAIT_MS, RoomClock};
pub use ice::{Candidates, IceServers, PUBLIC_STUN, StunChoice, TurnServer, is_local_signal};
pub use interp::{
    BUFFER_CAPACITY, HISTORY_TICKS, Insert, Playback, PlaybackSettings, PoseHistory, Sample,
    SampleKind, SampleLimits, SnapshotBuffer, TICK_MS, TickClock,
};
pub use items_online::{ItemLog, OnlineItems, OnlineItemsPlugin};
pub use links::{
    CONNECTING_SHOWN_MS, Diagnosis, Link, Links, NO_CANDIDATE_MS, Role, STUCK_MS, Stuck,
};
pub use lobby::{
    Lobby, LobbyEvent, LobbyPeer, Outgoing, PING_INTERVAL_MS, PlayerRow, SETTLE_MS, SILENT_MS,
};
pub use netsim::{NetSim, NetSimTuning};
pub use online::{OnlineRace, OnlineRacePlugin, OnlineSystems, race_to_start, session_of};
pub use pickups::{Arbiter, ClaimKey};
pub use protocol::{
    Channel, Claim, DecodeError, Grab, ItemBreak, ItemId, ItemOut, KartId, KartSnapshot, LooseId,
    MAX_KART_STATE_BYTES, MAX_NAME_CHARS, MAX_PLAYERS, MAX_SCATTER, Message, OUT_ITEMS, OutMotion,
    PROTOCOL_VERSION, PeerKey, Pickup, Profile, Progress, RaceOver, RaceStart, Scatter, Spot,
    Strike, StrikeReply, TetherBreak, Verdict,
};
pub use race::{Driver, RaceKart, SharedRace};
pub use room::{
    CODE_ALPHABET, CODE_LENGTH, DEFAULT_SIGNAL_URL, LaunchOptions, RoomCode, SIGNAL_PORT,
    default_signal, invite_link, native_command, parse_signal, query_with_room, room_url,
};
pub use signaling::{ObservedSignallerBuilder, SignalNote, WebSocketSignallerBuilder};
pub use snapshot::{
    HeldLook, KartLook, KartPose, LookParts, Phase, pack_rotation, unpack_rotation,
};
pub use socket::{NetSocket, SocketEvent};
pub use sync::{
    KartHistory, KartSync, KartSyncPlugin, NetKart, OnlineLevel, OnlineSlot, RemoteKart,
    RemoteKartProxy, RosterEntry, Showing, remote_body,
};
pub use tuning::NetTuning;
pub use ui::NetUiPlugin;

use bevy::prelude::*;

use crate::bots::BotTuning;
use crate::characters::{PartKind, PlayerLoadout, Roster};
use crate::options::OptionsMenu;
use crate::race::RaceTuning;

/// Opens and closes the lobby window: « L » for « en ligne », a letter at the same place and
/// with the same name in AZERTY and QWERTY, free in the game's controls (GD § 13), no Fn on a Mac.
pub const LOBBY_KEY: KeyCode = KeyCode::KeyL;

/// A departing socket gets this long to send its `Bye` before it is dropped (s).
const DEPARTURE_SECONDS: f64 = 2.0;

/// How to reach the others: the signaling server, the name shown to them, the ICE servers.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct NetSettings {
    /// The signaling server ([`DEFAULT_SIGNAL_URL`]; in the browser, the page's own machine when
    /// it is local, [`default_signal`]; `?signal=`, `--signal`).
    pub signal: String,
    /// `?pseudo=` / `--pseudo`; without it, the name of the player's character.
    pub pseudo: Option<String>,
    /// Tries to reach the signaling server before giving up, 3 s apart.
    pub attempts: u16,
    /// STUN servers (issue #49, `?stun=` / `--stun`): by default none when the signaling server
    /// is local, the public ones otherwise ([`IceServers::choose`]).
    pub stun: StunChoice,
    /// A TURN relay (`?turn=…&turnuser=…&turnpass=…`): none by default (issue #10 brings one).
    pub turn: Option<TurnServer>,
    /// In the browser, the longest wait for the ICE candidates before an offer or an answer
    /// leaves anyway, with those found so far (ms; the next ones follow). `matchbox_socket` waits
    /// for all of them without a limit: a STUN server that UDP cannot reach holds Chrome about
    /// 40 s, and a machine without IPv6 can keep it waiting for ever. 3 s leave a STUN server
    /// several tries; on a local network, the candidates come in a few milliseconds.
    pub ice_gathering_max_ms: u32,
}

impl Default for NetSettings {
    fn default() -> Self {
        Self {
            signal: DEFAULT_SIGNAL_URL.to_string(),
            pseudo: None,
            attempts: 3,
            stun: StunChoice::Auto,
            turn: None,
            ice_gathering_max_ms: 3_000,
        }
    }
}

impl NetSettings {
    /// The ICE servers of a socket joining a room now ([`IceServers::choose`]). A page without
    /// the shim of `index.html` cannot connect without any server: the public STUN ones then.
    pub fn ice_servers(&self) -> IceServers {
        let ice = IceServers::choose(&self.stun, self.turn.as_ref(), &self.signal);
        if ice.is_empty() && !browser::accepts_no_ice_server() {
            warn!("network: this page cannot connect without an ICE server: public STUN servers");
            let public = StunChoice::Servers(PUBLIC_STUN.map(String::from).to_vec());
            return IceServers::choose(&public, self.turn.as_ref(), &self.signal);
        }
        ice
    }
}

/// Whether the lobby window is open.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LobbyWindow {
    pub open: bool,
}

/// What the player does with the network, from the keys, the window or the page address.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum NetAction {
    /// Opens the window, or closes it (L).
    Toggle,
    Close,
    /// « Créer une partie »: a new code, and join it.
    CreateRoom,
    /// Joins this room (leaving the current one, if another).
    Join(RoomCode),
    /// « Quitter la partie ».
    Leave,
    /// « Prêt » / « Pas prêt ».
    Ready(bool),
    /// The host chooses the room's circuit (issue #9): an index of `assets/tracks/index.ron`.
    ChooseCircuit(u8),
    /// The host starts a race (issue #9) on the room's circuit, with everyone seated.
    StartRace,
}

/// Where we are with the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetStatus {
    /// Waiting for the signaling server.
    Connecting,
    /// In the room: the lobby says who is there.
    InRoom,
    /// The signaling server could not be reached, or dropped us.
    Failed(String),
    /// The host refused us: 12 players already.
    Full,
}

/// The room we are in (or trying to join): only while there is one.
#[derive(Resource)]
pub struct NetSession {
    pub room: RoomCode,
    /// The signaling server it goes through.
    pub signal: String,
    /// The STUN and TURN servers of its connections.
    pub ice: IceServers,
    pub status: NetStatus,
    /// From our id on.
    pub lobby: Option<Lobby>,
    /// The direct connections being made (issue #49).
    pub links: Links,
    socket: Option<NetSocket>,
    /// The browser's count of failed connections when we last looked (`browser::ice_failures`).
    failures_seen: u32,
}

impl NetSession {
    /// The players of the lobby, for the screen (empty before we have an id).
    pub fn players(&self, now_ms: u64) -> Vec<PlayerRow> {
        self.lobby
            .as_ref()
            .map(|lobby| lobby.players(now_ms))
            .unwrap_or_default()
    }
}

/// Sockets that left their room, until their message loop ends (their `Bye` sent).
#[derive(Resource, Default)]
struct Departures(Vec<(NetSocket, f64)>);

/// The network systems, in `Update`: keys, actions, then the socket.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetSystems;

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetSettings>()
            .init_resource::<LobbyWindow>()
            .init_resource::<Departures>()
            .add_message::<NetAction>()
            .add_systems(
                Update,
                (
                    lobby_key.run_if(resource_exists::<ButtonInput<KeyCode>>),
                    apply_net_actions,
                    send_profile.run_if(resource_exists::<NetSession>),
                    poll_session.run_if(resource_exists::<NetSession>),
                    poll_departures.run_if(|d: Res<Departures>| !d.0.is_empty()),
                )
                    .chain()
                    .in_set(NetSystems),
            );
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            app.add_plugins(NetUiPlugin);
        }
    }
}

/// The lobby's clock: milliseconds of real time (it runs while the game is paused).
pub fn now_ms(time: &Time<Real>) -> u64 {
    time.elapsed().as_millis() as u64
}

/// L opens and closes the window; not while the options menu has the keys.
fn lobby_key(
    keys: Res<ButtonInput<KeyCode>>,
    options: Option<Res<OptionsMenu>>,
    mut actions: MessageWriter<NetAction>,
) {
    if keys.just_pressed(LOBBY_KEY) && !options.is_some_and(|menu| menu.open) {
        actions.write(NetAction::Toggle);
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_net_actions(
    mut commands: Commands,
    mut actions: MessageReader<NetAction>,
    mut window: ResMut<LobbyWindow>,
    settings: Res<NetSettings>,
    mut session: Option<ResMut<NetSession>>,
    mut departures: ResMut<Departures>,
    time: Res<Time<Real>>,
    (race_tuning, bot_tuning, net_tuning): (
        Option<Res<RaceTuning>>,
        Option<Res<BotTuning>>,
        Option<Res<NetTuning>>,
    ),
) {
    for action in actions.read() {
        match action {
            NetAction::Toggle => window.open = !window.open,
            NetAction::Close => window.open = false,
            NetAction::CreateRoom | NetAction::Join(_) => {
                let room = match action {
                    NetAction::Join(room) => room.clone(),
                    _ => RoomCode::generate(random_seed(time.elapsed().as_nanos() as u64)),
                };
                if let Some(current) = session.as_deref_mut() {
                    let usable = !matches!(current.status, NetStatus::Failed(_) | NetStatus::Full);
                    if current.room == room && current.signal == settings.signal && usable {
                        continue;
                    }
                    depart(current, &mut departures, &time);
                }
                let ice = settings.ice_servers();
                browser::set_gathering_max_ms(settings.ice_gathering_max_ms);
                info!(
                    "network: joining room {room} on {}, {}",
                    settings.signal,
                    ice.describe()
                );
                let url = room_url(&settings.signal, &room);
                let new = NetSession {
                    room,
                    signal: settings.signal.clone(),
                    socket: Some(NetSocket::open(&url, settings.attempts, &ice)),
                    ice,
                    status: NetStatus::Connecting,
                    lobby: None,
                    links: Links::default(),
                    failures_seen: browser::ice_failures(),
                };
                // Commands apply after this system: the next action of this frame (rare) sees
                // the old session.
                commands.insert_resource(new);
                session = None;
                window.open = true;
            }
            NetAction::Leave => {
                if let Some(current) = session.as_deref_mut() {
                    info!("network: leaving room {}", current.room);
                    depart(current, &mut departures, &time);
                    commands.remove_resource::<NetSession>();
                }
                session = None;
            }
            NetAction::Ready(ready) => {
                if let Some(lobby) = session.as_deref_mut().and_then(|s| s.lobby.as_mut()) {
                    lobby.set_ready(*ready);
                }
            }
            NetAction::ChooseCircuit(track) => {
                if let Some(lobby) = session.as_deref_mut().and_then(|s| s.lobby.as_mut())
                    && lobby.choose_circuit(*track)
                {
                    info!("network: we choose circuit {track} for the room");
                }
            }
            NetAction::StartRace => {
                let Some(lobby) = session.as_deref_mut().and_then(|s| s.lobby.as_mut()) else {
                    continue;
                };
                let seed = random_seed(time.elapsed().as_nanos() as u64);
                let difficulty = bot_tuning
                    .as_ref()
                    .map_or_else(Default::default, |t| t.grid_difficulty);
                let tuning = net_tuning.as_deref().cloned().unwrap_or_default();
                let laps = online::race_laps(race_tuning.as_deref());
                match race_to_start(lobby, now_ms(&time), seed, laps, difficulty, &tuning) {
                    Some(start) => {
                        info!(
                            "network: we start race {} on circuit {} at tick {}, {} players and {} bots",
                            start.race,
                            start.track,
                            start.start_tick,
                            start.players.len(),
                            start.bots
                        );
                        lobby.announce_race(start);
                    }
                    None => {
                        warn!("network: no race: we do not host, or the room's clock is unknown")
                    }
                }
            }
        }
    }
}

/// Our `Bye`, then the socket closes in the background.
fn depart(session: &mut NetSession, departures: &mut Departures, time: &Time<Real>) {
    if let Some(mut socket) = session.socket.take() {
        if let Some(lobby) = session.lobby.as_mut() {
            lobby.leave();
            for out in lobby.drain_outbox() {
                socket.send(out.to, &out.message);
            }
        }
        socket.close();
        departures
            .0
            .push((socket, time.elapsed_secs_f64() + DEPARTURE_SECONDS));
    }
}

/// Our name and kart, told to the others when they change (selection screen).
fn send_profile(
    mut session: ResMut<NetSession>,
    settings: Res<NetSettings>,
    loadout: Option<Res<PlayerLoadout>>,
    roster: Option<Res<Roster>>,
) {
    // The lobby starts with the profile of its time (`poll_session`): only changes count.
    if !settings.is_changed() && !loadout.as_ref().is_some_and(|l| l.is_changed()) {
        return;
    }
    let (name, loadout) = profile_of(&settings, loadout.as_deref(), roster.as_deref());
    if let Some(lobby) = session.lobby.as_mut() {
        lobby.set_profile(&name, loadout);
    }
}

/// The name and loadout we show: the pseudo, or our character's name.
fn profile_of(
    settings: &NetSettings,
    loadout: Option<&PlayerLoadout>,
    roster: Option<&Roster>,
) -> (String, [u8; 4]) {
    let loadout = loadout.map(|l| l.0).unwrap_or_default();
    let name = settings.pseudo.clone().unwrap_or_else(|| {
        roster
            .map(|r| r.name(PartKind::Character, loadout.character).to_string())
            .unwrap_or_else(|| "Joueur".to_string())
    });
    let parts = [
        loadout.character,
        loadout.kart,
        loadout.wheels,
        loadout.wing,
    ];
    (name, parts.map(|part| part.min(255) as u8))
}

/// Each frame in a room: the socket's news into the lobby (the kart states into the
/// synchronization, issue #8), the lobby's messages and our kart's states out.
#[allow(clippy::too_many_arguments)]
fn poll_session(
    mut session: ResMut<NetSession>,
    settings: Res<NetSettings>,
    loadout: Option<Res<PlayerLoadout>>,
    roster: Option<Res<Roster>>,
    time: Res<Time<Real>>,
    game_time: Res<Time<Virtual>>,
    mut sync: Option<ResMut<KartSync>>,
    mut race: Option<ResMut<OnlineRace>>,
    mut items: Option<ResMut<OnlineItems>>,
    tuning: Option<Res<NetTuning>>,
) {
    let now = now_ms(&time);
    let game_ms = game_time.elapsed_secs_f64() * 1_000.0;
    let session = &mut *session;
    let Some(socket) = session.socket.as_mut() else {
        return;
    };
    let idle = session.links.pending().is_empty();
    let lobby = &mut session.lobby;
    let status = &mut session.status;
    let links = &mut session.links;
    socket.poll(|event| match event {
        SocketEvent::Joined(me) => {
            let (name, loadout) = profile_of(&settings, loadout.as_deref(), roster.as_deref());
            let profile = Profile {
                name,
                loadout,
                ..Default::default()
            };
            *lobby = Some(Lobby::new(me, profile, now));
            *status = NetStatus::InRoom;
        }
        SocketEvent::Connected(peer) => {
            // Outside of `info!`: its arguments are not even computed when that level is off (no
            // subscriber in the headless tests, `RUST_LOG=warn`), and the link stayed (issue #51).
            let line = links.connected(peer, now);
            info!("network: {line}");
            if let Some(lobby) = lobby.as_mut() {
                lobby.connect(peer, now);
            }
        }
        SocketEvent::Disconnected(peer) => {
            links.disconnected(peer);
            if let Some(lobby) = lobby.as_mut() {
                lobby.disconnect(peer);
            }
        }
        SocketEvent::Signal(note) => {
            if let Some(line) = links.note(&note, now) {
                info!("network: {line}");
            }
        }
        SocketEvent::Packet(peer, _, bytes) => {
            if let Some(lobby) = lobby.as_mut() {
                match Message::decode(&bytes) {
                    Ok(message) => {
                        let host = lobby.host();
                        // Issue #53: the items and the croquettes.
                        if let (
                            Message::Claim(_)
                            | Message::Verdict(_)
                            | Message::ItemOut(_)
                            | Message::ItemBreak(_)
                            | Message::Scatter(_),
                            Some(items),
                        ) = (&message, items.as_deref_mut())
                        {
                            items.receive(peer, message.clone());
                        }
                        match (&message, sync.as_deref_mut(), race.as_deref_mut()) {
                            (Message::KartState(state), Some(sync), _) => {
                                if let Some(tuning) = tuning.as_deref() {
                                    sync.receive(peer, *state, game_ms, tuning);
                                }
                            }
                            (
                                Message::Strike(_)
                                | Message::StrikeReply(_)
                                | Message::Grab(_)
                                | Message::TetherBreak(_),
                                Some(sync),
                                _,
                            ) => sync.receive_event(peer, message.clone()),
                            (Message::Progress(progress), _, Some(race)) => {
                                race.shared.receive(peer, host, progress);
                            }
                            (Message::RaceOver(over), _, Some(race)) => {
                                if race.shared.receive_over(peer, host, over.clone()) {
                                    info!(
                                        "network: the host ended race {}: {:?}",
                                        over.race, over.order
                                    );
                                }
                            }
                            _ => {}
                        }
                        lobby.receive(peer, message, now);
                    }
                    Err(error) => {
                        debug!("network: packet from {peer:?} refused: {error}");
                        lobby.receive_error(peer, error, now);
                    }
                }
            }
        }
        SocketEvent::Closed(result) => {
            let reason = match result {
                Err(error) => error,
                Ok(()) => "la connexion s'est fermée".to_string(),
            };
            warn!("network: the socket stopped: {reason}");
            *status = NetStatus::Failed(reason);
        }
    });
    // While connections are being made: did the browser see one fail (the page's shim counts
    // them)? Is one stuck?
    if !session.links.pending().is_empty() {
        let failures = browser::ice_failures();
        if idle {
            // New connections: the failures before them are not theirs.
            session.failures_seen = failures;
        } else if failures > session.failures_seen {
            session.failures_seen = failures;
            session.links.failed();
        }
        for warning in session.links.newly_stuck(now) {
            warn!("network: {warning}");
        }
    }
    if let Some(lobby) = session.lobby.as_mut() {
        lobby.update(now);
        for out in lobby.drain_outbox() {
            socket.send(out.to, &out.message);
        }
        // Our karts' states (issue #8), the interactions and the race's events (issue #9), to
        // every other seated player.
        let me = lobby.me();
        let mut to_everyone = |message: &Message| {
            for peer in lobby.seats() {
                if *peer != me {
                    socket.send(*peer, message);
                }
            }
        };
        if let Some(sync) = sync.as_deref_mut() {
            for state in sync.drain_outgoing() {
                to_everyone(&Message::KartState(state));
            }
            for message in sync.drain_events_out() {
                to_everyone(&message);
            }
        }
        if let Some(race) = race.as_deref_mut() {
            for message in race.drain_outbox() {
                to_everyone(&message);
            }
        }
        if let Some(items) = items.as_deref_mut() {
            for message in items.drain_outbox() {
                to_everyone(&message);
            }
        }
        for event in lobby.drain_events() {
            info!("network: {event:?}");
            if let LobbyEvent::Left(peer) = &event {
                // A player who leaves gives up the race (issue #9); its items out disappear
                // (issue #53).
                if let Some(race) = race.as_deref_mut() {
                    race.shared.leave(*peer);
                }
                if let Some(items) = items.as_deref_mut() {
                    items.left(*peer);
                }
            }
        }
        if lobby.full() {
            session.status = NetStatus::Full;
        }
    }
    if matches!(session.status, NetStatus::Full | NetStatus::Failed(_)) {
        // Refused, or already stopped: nothing to say, the signaling server tells the others.
        session.socket = None;
    }
}

/// Departed sockets: polled until their loop ends, or dropped after a while.
fn poll_departures(mut departures: ResMut<Departures>, time: Res<Time<Real>>) {
    let now = time.elapsed_secs_f64();
    departures.0.retain_mut(|(socket, until)| {
        socket.poll(|_| {});
        !socket.closed() && now < *until
    });
}

/// A random number for a new room code (`fallback`: without any source of randomness, the time
/// since the start will do for a room code).
fn random_seed(fallback: u64) -> u64 {
    getrandom::u64().unwrap_or(fallback.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}
