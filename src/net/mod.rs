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
//! - **Protocol** (`protocol.rs`): compact, versioned messages; **lobby** (`lobby.rs`): seats,
//!   host (the smallest `PeerId`), « Prêt », ping and common clock (`clock.rs`), arrivals and
//!   departures, the room full at 12. All of it is pure logic, unit-tested without Bevy.
//! - **Window** (`ui.rs`, only with a renderer): **L** (« en ligne ») opens and closes it (same
//!   letter at the same place in AZERTY and QWERTY, no Fn on a Mac); it opens by itself with a
//!   room in the address. It does not pause the game, and costs nothing once closed (no egui
//!   context, like the other windows).
//!
//! **Solo is unchanged**: without a room there is no socket; only the L key and the reading of
//! the actions run each frame, the rest needs a [`NetSession`] (measured: no cost in the
//! browser's single-threaded executor, STATUS.md « Métriques »).

mod clock;
pub mod interactions;
mod lobby;
mod protocol;
mod room;
mod socket;
mod ui;

#[cfg(test)]
mod scenario_tests;

pub use clock::PeerClock;
pub use lobby::{
    Lobby, LobbyEvent, LobbyPeer, Outgoing, PING_INTERVAL_MS, PlayerRow, SETTLE_MS, SILENT_MS,
};
pub use protocol::{
    Channel, DecodeError, Grab, KartId, MAX_NAME_CHARS, MAX_PLAYERS, Message, PROTOCOL_VERSION,
    PeerKey, Profile, RaceStart, Strike, StrikeReply, TetherBreak,
};
pub use room::{
    CODE_ALPHABET, CODE_LENGTH, DEFAULT_SIGNAL_URL, LaunchOptions, RoomCode, invite_link,
    native_command, parse_signal, query_with_room, room_url,
};
pub use socket::{NetSocket, SocketEvent};
pub use ui::NetUiPlugin;

use bevy::prelude::*;

use crate::characters::{PartKind, PlayerLoadout, Roster};
use crate::options::OptionsMenu;

/// Opens and closes the lobby window: « L » for « en ligne », a letter at the same place and
/// with the same name in AZERTY and QWERTY, free in the game's controls (GD § 13), no Fn on a Mac.
pub const LOBBY_KEY: KeyCode = KeyCode::KeyL;

/// A departing socket gets this long to send its `Bye` before it is dropped (s).
const DEPARTURE_SECONDS: f64 = 2.0;

/// How to reach the others: the signaling server, the name shown to them.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct NetSettings {
    /// The signaling server ([`DEFAULT_SIGNAL_URL`], `?signal=`, `--signal`).
    pub signal: String,
    /// `?pseudo=` / `--pseudo`; without it, the name of the player's character.
    pub pseudo: Option<String>,
    /// Tries to reach the signaling server before giving up, 3 s apart.
    pub attempts: u16,
}

impl Default for NetSettings {
    fn default() -> Self {
        Self {
            signal: DEFAULT_SIGNAL_URL.to_string(),
            pseudo: None,
            attempts: 3,
        }
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
    pub status: NetStatus,
    /// From our id on.
    pub lobby: Option<Lobby>,
    socket: Option<NetSocket>,
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

fn apply_net_actions(
    mut commands: Commands,
    mut actions: MessageReader<NetAction>,
    mut window: ResMut<LobbyWindow>,
    settings: Res<NetSettings>,
    mut session: Option<ResMut<NetSession>>,
    mut departures: ResMut<Departures>,
    time: Res<Time<Real>>,
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
                info!("network: joining room {room} on {}", settings.signal);
                let url = room_url(&settings.signal, &room);
                let new = NetSession {
                    room,
                    signal: settings.signal.clone(),
                    status: NetStatus::Connecting,
                    lobby: None,
                    socket: Some(NetSocket::open(&url, settings.attempts)),
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

/// Each frame in a room: the socket's news into the lobby, the lobby's messages out.
fn poll_session(
    mut session: ResMut<NetSession>,
    settings: Res<NetSettings>,
    loadout: Option<Res<PlayerLoadout>>,
    roster: Option<Res<Roster>>,
    time: Res<Time<Real>>,
) {
    let now = now_ms(&time);
    let session = &mut *session;
    let Some(socket) = session.socket.as_mut() else {
        return;
    };
    let lobby = &mut session.lobby;
    let status = &mut session.status;
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
            if let Some(lobby) = lobby.as_mut() {
                lobby.connect(peer, now);
            }
        }
        SocketEvent::Disconnected(peer) => {
            if let Some(lobby) = lobby.as_mut() {
                lobby.disconnect(peer);
            }
        }
        SocketEvent::Packet(peer, _, bytes) => {
            if let Some(lobby) = lobby.as_mut() {
                match Message::decode(&bytes) {
                    Ok(message) => lobby.receive(peer, message, now),
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
    if let Some(lobby) = session.lobby.as_mut() {
        lobby.update(now);
        for out in lobby.drain_outbox() {
            socket.send(out.to, &out.message);
        }
        for event in lobby.drain_events() {
            info!("network: {event:?}");
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
