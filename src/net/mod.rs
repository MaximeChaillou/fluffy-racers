//! Network, part 1 of 3 (issue #7, `docs/ARCHITECTURE.md` § « Réseau »): rooms, peer-to-peer
//! connection and the lobby. The race itself stays local for now: the karts are synchronized
//! with issue #8, the race with issue #9.
//!
//! - **Room**: `?room=CODE` in the page address, `--room CODE` natively; « Créer une partie »
//!   draws a code, hosts it (issue #63: `&host=1` in its own address, `--host` natively) and
//!   gives the invitation link (`room.rs`). The signaling server
//!   (`ws://localhost:3536` by default, `?signal=` / `--signal` for another one) only introduces
//!   the peers to each other.
//! - **Socket** (`socket.rs`): `matchbox_socket`, a reliable and an unreliable channel, polled
//!   each frame from a system, natively and in the browser. Since issue #63, a **star**: the peer
//!   that created the room hosts it, each client has a single link, with the host
//!   (`signaling::SignalFilter`), and the host relays what a client tells the others, with its
//!   origin (`Message::Relayed`), and sends each client one packet with every kart's state
//!   (`relay.rs`). The host leaving ends the room: « L'hôte a quitté la partie ».
//! - **Direct connections** (issue #49): the STUN and TURN servers (`ice.rs`: none on a local
//!   network, `?stun=`, `?turn=`), what goes through the signaling server (`signaling.rs`) and
//!   the connections being made (`links.rs`): each step in the console, and in the lobby a peer
//!   that does not connect, with the leads (README, « Dépannage »). In the browser, a small shim
//!   of the page (`index.html`, read by `browser.rs`) completes `matchbox_socket`.
//! - **Protocol** (`protocol.rs`): compact, versioned messages; **lobby** (`lobby.rs`): seats
//!   (the host's list), the host (the room's creator, issue #63), « Prêt », ping and common clock
//!   (`clock.rs`), arrivals and departures, the room full at 12. All of it is pure logic,
//!   unit-tested without Bevy.
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
//!   every screen; the strikes and grabs between karts go to the peer that decides. Online, the
//!   game never pauses.
//!
//! - **Pseudo** (issue #80, `pseudo.rs`): typed at the top of the lobby (`ui.rs`), kept in the
//!   browser, `?pseudo=` winning over it; it goes out in the `Hello`, again at each change (the
//!   host relays it), and shows in the lobby, on the results screen and, in the HUD's font, on a
//!   tag over each other player's kart (`tags.rs`, « Noms au-dessus des karts » in the options).
//!
//! **Solo is unchanged**: without a room there is no socket; only the L key and the reading of
//! the actions run each frame, the rest needs a [`NetSession`] (measured: no cost in the
//! browser's single-threaded executor, STATUS.md « Métriques »).

mod browser;
mod clock;
mod guide;
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
pub mod pseudo;
mod race;
mod relay;
mod room;
mod signaling;
mod snapshot;
mod socket;
mod sync;
pub mod tags;
mod tuning;
mod ui;

#[cfg(test)]
mod guide_tests;
#[cfg(test)]
mod item_tests;
#[cfg(test)]
mod paint_tests;
#[cfg(test)]
mod parts_tests;
#[cfg(test)]
mod pseudo_tests;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod scenario_tests;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod star_tests;
#[cfg(test)]
mod sync_tests;

pub use clock::{PeerClock, RoomClock};
pub use guide::{
    Banner, RaceStage, RoomFacts, Seated, Stage, banner_shown, race_line, race_now, settings_line,
};
pub use ice::{Candidates, IceServers, PUBLIC_STUN, StunChoice, TurnServer, is_local_signal};
pub use interp::{
    BUFFER_CAPACITY, HISTORY_TICKS, Insert, Playback, PlaybackSettings, PoseHistory, Sample,
    SampleKind, SampleLimits, SnapshotBuffer, TICK_MS, TickClock, interpolate,
};
pub use items_online::{ItemLog, OnlineItems, OnlineItemsPlugin};
pub use links::{
    CONNECTING_SHOWN_MS, Diagnosis, Link, Links, NO_CANDIDATE_MS, Role, STUCK_MS, Stuck,
};
pub use lobby::{
    Lobby, LobbyEvent, LobbyPeer, NO_HOST_MS, Outgoing, PING_INTERVAL_MS, PlayerRow, RowLink,
    SILENT_MS,
};
pub use netsim::{NetSim, NetSimTuning};
pub use online::{OnlineRace, OnlineRacePlugin, OnlineSystems, race_to_start, session_of};
pub use pickups::{Arbiter, ClaimKey, Pillow, PillowRules};
pub use protocol::{
    BlastOut, Channel, Claim, DecodeError, EFFECT_ITEMS, EffectOut, Grab, HELD_ITEMS, Honk,
    ItemBreak, ItemId, ItemOut, KartId, KartSnapshot, LooseId, MAX_BUNDLED_STATES,
    MAX_KART_STATE_BYTES, MAX_NAME_CHARS, MAX_PLAYERS, MAX_SCATTER, Message, OUT_ITEMS, OutMotion,
    OutPillow, PROTOCOL_VERSION, PeerKey, Pickup, PillowGone, Profile, Progress, RAM_ITEMS,
    RaceOver, RaceResults, RaceStart, Rammed, Retarget, RoomSetup, Scatter, Spot, Strike,
    StrikeReply, TetherBreak, Verdict,
};
pub use race::{Driver, RaceKart, SharedRace};
pub use relay::StateRelay;
pub use room::{
    CODE_ALPHABET, CODE_LENGTH, DEFAULT_SIGNAL_URL, LaunchOptions, RoomCode, SIGNAL_PORT,
    default_signal, host_query, invite_link, native_command, parse_signal, query_with_room,
    room_url,
};
pub use signaling::{
    Admit, ObservedSignallerBuilder, SignalFilter, SignalNote, WebSocketSignallerBuilder,
};
pub use snapshot::{
    HeldLook, KartLook, KartPose, LookParts, Phase, pack_rotation, unpack_rotation,
};
pub use socket::{NetSocket, SocketEvent};
pub use sync::{
    KartHistory, KartSync, KartSyncPlugin, NetKart, OnlineLevel, OnlineSlot, RemoteKart,
    RemoteKartProxy, RosterEntry, Showing, remote_body,
};
pub use tags::NameTagPlugin;
pub use tuning::NetTuning;
pub use ui::NetUiPlugin;

use bevy::prelude::*;

use crate::bots::BotTuning;
use crate::characters::{Loadout, PartKind, PlayerColor, PlayerLoadout, Roster};
use crate::kart::KartColor;
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
    /// The pseudo (issue #80, `pseudo::clean`): typed in the lobby and kept in the browser, or
    /// `?pseudo=` / `--pseudo`, which wins; without it, the name of the player's character.
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

/// Whether the lobby window is open, and the room's banner shown.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LobbyWindow {
    pub open: bool,
    /// Issue #64: the banner at the top of the screen (`guide::banner_shown`): in a room, out of
    /// a race we run. Like the window, it needs the egui context: none without a room.
    pub banner: bool,
}

/// What the player does with the network, from the keys, the window or the page address.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum NetAction {
    /// Opens the window, or closes it (L).
    Toggle,
    Close,
    /// « Créer une partie »: a new code, and host it (issue #63).
    CreateRoom,
    /// Joins this room as a client (leaving the current one, if another): the invitation link.
    Join(RoomCode),
    /// Issue #63: hosts this room (`?room=CODE&host=1`, `--host`: the creator's page, reloaded).
    Host(RoomCode),
    /// « Quitter la partie ».
    Leave,
    /// « Prêt » / « Pas prêt ».
    Ready(bool),
    /// The host chooses the room's circuit (issue #9): an index of `assets/tracks/index.ron`.
    ChooseCircuit(u8),
    /// Issue #99: the host chooses the room's settings (the circuit, the mirror, the bots, the
    /// items).
    ChooseSetup(RoomSetup),
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
    /// Issue #63: our host left the room: it is over (« L'hôte a quitté la partie »).
    HostLeft,
    /// Issue #63: we wanted to host a room that has its host already (an address with `host=1`
    /// opened twice): we left it.
    HostTaken,
}

impl NetStatus {
    /// The room can still be used (the socket is there).
    pub fn usable(&self) -> bool {
        matches!(self, NetStatus::Connecting | NetStatus::InRoom)
    }
}

/// The room we are in (or trying to join): only while there is one.
#[derive(Resource)]
pub struct NetSession {
    pub room: RoomCode,
    /// The signaling server it goes through.
    pub signal: String,
    /// The STUN and TURN servers of its connections.
    pub ice: IceServers,
    /// Issue #63: we host the room (we created it), or are a client.
    pub hosting: bool,
    pub status: NetStatus,
    /// From our id on.
    pub lobby: Option<Lobby>,
    /// The direct connections being made (issue #49).
    pub links: Links,
    socket: Option<NetSocket>,
    /// The browser's count of failed connections when we last looked (`browser::ice_failures`).
    failures_seen: u32,
    /// Issue #63, the host: the clients' kart states, until its next bundles.
    relay: StateRelay,
    /// Our karts' states of this frame (reused).
    own_states: Vec<KartSnapshot>,
}

impl NetSession {
    /// A session joining `room` through `socket`: as its host, or a client.
    pub fn new(
        room: RoomCode,
        signal: String,
        ice: IceServers,
        socket: NetSocket,
        hosting: bool,
    ) -> Self {
        Self {
            room,
            signal,
            ice,
            hosting,
            status: NetStatus::Connecting,
            lobby: None,
            links: Links::default(),
            socket: Some(socket),
            failures_seen: browser::ice_failures(),
            relay: StateRelay::default(),
            own_states: Vec::new(),
        }
    }

    /// The players of the lobby, for the screen (empty before we have an id), and, issue #63,
    /// the links with the host being made: a client's own, the host's with each newcomer.
    pub fn players(&self, now_ms: u64) -> Vec<PlayerRow> {
        let Some(lobby) = self.lobby.as_ref() else {
            return Vec::new();
        };
        let mut rows = lobby.players(now_ms);
        for link in self.links.pending() {
            if rows.iter().any(|row| row.key == link.peer) {
                continue;
            }
            rows.push(PlayerRow {
                key: link.peer,
                name: String::new(),
                loadout: [0; 4],
                chosen: None,
                color: None,
                ready: false,
                me: false,
                // A client's link being made is with the host: the only one that offers.
                host: !self.hosting,
                seated: false,
                link: match link.stuck(now_ms) {
                    Some(stuck) => RowLink::Stuck(stuck),
                    None => RowLink::Connecting,
                },
                ping_ms: None,
                silent: false,
                other_version: None,
            });
        }
        rows
    }

    /// Issue #63: a client that met no host since it joined, [`NO_HOST_MS`] ago (no offer, no
    /// connection): the room has none (« Cette partie n'existe plus »). A late offer still
    /// connects it.
    pub fn host_missing(&self, now_ms: u64) -> bool {
        !self.hosting
            && self.status == NetStatus::InRoom
            && self.links.pending().is_empty()
            && self.lobby.as_ref().is_some_and(|lobby| {
                lobby.host().is_none() && now_ms >= lobby.joined_ms() + NO_HOST_MS
            })
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
            )
            .add_systems(
                Update,
                follow_the_room
                    .after(NetSystems)
                    .after(OnlineSystems)
                    .run_if(
                        |session: Option<Res<NetSession>>, window: Res<LobbyWindow>| {
                            session.is_some() || window.banner
                        },
                    ),
            );
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            // Issue #80: the tags over the other players' karts, on screen only.
            app.add_plugins((NetUiPlugin, NameTagPlugin));
        }
    }
}

/// Issue #64: the banner shows in a room, out of a race we run; the lobby closes when our race
/// starts and opens when it ends (the host starts the next one there).
fn follow_the_room(
    session: Option<Res<NetSession>>,
    race: Option<Res<OnlineRace>>,
    mut window: ResMut<LobbyWindow>,
    mut last: Local<Option<(u16, RaceStage)>>,
) {
    let (session, race) = (session.as_deref(), race.as_deref());
    let banner = banner_shown(session, race);
    if window.banner != banner {
        window.banner = banner;
    }
    let now = race_now(session, race);
    if *last == now {
        return;
    }
    match now {
        Some((_, RaceStage::Racing)) if window.open => window.open = false,
        Some((_, RaceStage::Over)) if !window.open => window.open = true,
        _ => {}
    }
    *last = now;
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
            NetAction::CreateRoom | NetAction::Join(_) | NetAction::Host(_) => {
                let (room, hosting) = match action {
                    NetAction::Join(room) => (room.clone(), false),
                    NetAction::Host(room) => (room.clone(), true),
                    _ => (
                        RoomCode::generate(random_seed(time.elapsed().as_nanos() as u64)),
                        true,
                    ),
                };
                if let Some(current) = session.as_deref_mut() {
                    if current.room == room
                        && current.signal == settings.signal
                        && current.hosting == hosting
                        && current.status.usable()
                    {
                        continue;
                    }
                    depart(current, &mut departures, &time);
                }
                let ice = settings.ice_servers();
                browser::set_gathering_max_ms(settings.ice_gathering_max_ms);
                info!(
                    "network: joining room {room} on {} as its {}, {}",
                    settings.signal,
                    if hosting { "host" } else { "client" },
                    ice.describe()
                );
                let url = room_url(&settings.signal, &room);
                let socket = NetSocket::open(&url, settings.attempts, &ice, hosting);
                let new = NetSession::new(room, settings.signal.clone(), ice, socket, hosting);
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
            NetAction::ChooseSetup(setup) => {
                if let Some(lobby) = session.as_deref_mut().and_then(|s| s.lobby.as_mut())
                    && lobby.choose_setup(*setup)
                {
                    info!("network: we choose the room's settings: {setup:?}");
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

/// Our name, kart and color (issue #55), told to the others when they change (selection screen).
fn send_profile(
    mut session: ResMut<NetSession>,
    settings: Res<NetSettings>,
    loadout: Option<Res<PlayerLoadout>>,
    color: Option<Res<PlayerColor>>,
    roster: Option<Res<Roster>>,
) {
    // The lobby starts with the profile of its time (`poll_session`): only changes count.
    if !settings.is_changed()
        && !loadout.as_ref().is_some_and(|l| l.is_changed())
        && !color.as_ref().is_some_and(|c| c.is_changed())
    {
        return;
    }
    let (name, loadout) = profile_of(&settings, loadout.as_deref(), roster.as_deref());
    if let Some(lobby) = session.lobby.as_mut() {
        lobby.set_profile(&name, loadout, color_of(color.as_deref()));
    }
}

/// The color we ask for (issue #55): the player's choice.
fn color_of(color: Option<&PlayerColor>) -> KartColor {
    color.map(|color| color.0).unwrap_or_default()
}

/// The name and loadout we show: the pseudo, or our character's name.
fn profile_of(
    settings: &NetSettings,
    loadout: Option<&PlayerLoadout>,
    roster: Option<&Roster>,
) -> (String, [u8; 4]) {
    let loadout = loadout.map(|l| l.0).unwrap_or_default();
    let name = settings
        .pseudo
        .clone()
        .unwrap_or_else(|| character_name(Some(&loadout), roster));
    let parts = [
        loadout.character,
        loadout.kart,
        loadout.wheels,
        loadout.wing,
    ];
    (name, parts.map(|part| part.min(255) as u8))
}

/// Our name without a pseudo (issue #80: the hint of the lobby's field): our character's.
pub fn character_name(loadout: Option<&Loadout>, roster: Option<&Roster>) -> String {
    let character = loadout.map_or(0, |loadout| loadout.character);
    roster
        .map(|r| r.name(PartKind::Character, character).to_string())
        .unwrap_or_else(|| "Joueur".to_string())
}

/// The parts of the game a message is for (issue #53: the items; #8: the kart states; #9: the
/// interactions and the race), with `from` its origin: the peer that sent it, or (issue #63) the
/// client whose message our host relayed. The lobby gets every message itself, after.
#[allow(clippy::too_many_arguments)]
fn to_the_game(
    from: PeerKey,
    message: &Message,
    (host, hosting): (Option<PeerKey>, bool),
    game_ms: f64,
    sync: Option<&mut KartSync>,
    race: Option<&mut OnlineRace>,
    items: Option<&mut OnlineItems>,
    tuning: Option<&NetTuning>,
) {
    match message {
        // Issue #63: a client's message, from our host: as if it came from that client.
        Message::Relayed { origin, message } if !hosting && Some(from) == host => {
            to_the_game(
                *origin,
                message,
                (host, hosting),
                game_ms,
                sync,
                race,
                items,
                tuning,
            );
        }
        // Issue #63: our host's bundle: every kart's state, from the peer that drives it.
        Message::KartStates(states) if !hosting && Some(from) == host => {
            if let (Some(sync), Some(tuning)) = (sync, tuning) {
                for (origin, state) in states {
                    sync.receive(*origin, *state, game_ms, tuning);
                }
            }
        }
        Message::KartState(state) => {
            if let (Some(sync), Some(tuning)) = (sync, tuning) {
                sync.receive(from, *state, game_ms, tuning);
            }
        }
        // Issue #53: the items and the croquettes (#18: explosions, klaxons; #61: effects,
        // rams, oreillers; #21: the pies voleuses' thefts).
        Message::Claim(_)
        | Message::Verdict(_)
        | Message::ItemOut(_)
        | Message::ItemBreak(_)
        | Message::Scatter(_)
        | Message::Blast(_)
        | Message::Honk(_)
        | Message::Effect(_)
        | Message::Rammed(_)
        | Message::PillowGone(_)
        | Message::Retarget(_)
        | Message::Steal(_)
        | Message::Stolen(_) => {
            if let Some(items) = items {
                items.receive(from, message.clone());
            }
        }
        Message::Strike(_)
        | Message::StrikeReply(_)
        | Message::Grab(_)
        | Message::TetherBreak(_) => {
            if let Some(sync) = sync {
                sync.receive_event(from, message.clone());
            }
        }
        Message::Progress(progress) => {
            if let Some(race) = race {
                race.shared.receive(from, host, progress);
            }
        }
        Message::RaceOver(over) => {
            if let Some(race) = race
                && race.shared.receive_over(from, host, over.clone())
            {
                info!(
                    "network: the host ended race {}: {:?}",
                    over.race, over.order
                );
            }
        }
        Message::RaceResults(results) => {
            if let Some(race) = race
                && race.shared.receive_results(from, host, results.clone())
            {
                info!(
                    "network: the host closed the results of race {}: {:?}",
                    results.race, results.lines
                );
            }
        }
        _ => {}
    }
}

/// Each frame in a room: the socket's news into the lobby (the kart states into the
/// synchronization, issue #8), the lobby's messages and our karts' states out. Issue #63, the
/// star: a client sends everything to its host; the host relays a client's messages for everyone
/// to the other clients (`Lobby::relay`), and sends each client one bundle of every kart's state
/// (`relay.rs`).
#[allow(clippy::too_many_arguments)]
fn poll_session(
    mut session: ResMut<NetSession>,
    settings: Res<NetSettings>,
    (loadout, color): (Option<Res<PlayerLoadout>>, Option<Res<PlayerColor>>),
    roster: Option<Res<Roster>>,
    time: Res<Time<Real>>,
    game_time: Res<Time<Virtual>>,
    mut sync: Option<ResMut<KartSync>>,
    mut race: Option<ResMut<OnlineRace>>,
    mut items: Option<ResMut<OnlineItems>>,
    tuning: Option<Res<NetTuning>>,
    mut window: ResMut<LobbyWindow>,
) {
    let now = now_ms(&time);
    let game_ms = game_time.elapsed_secs_f64() * 1_000.0;
    let session = &mut *session;
    let Some(socket) = session.socket.as_mut() else {
        return;
    };
    let idle = session.links.pending().is_empty();
    let hosting = session.hosting;
    let lobby = &mut session.lobby;
    let status = &mut session.status;
    let links = &mut session.links;
    let relay = &mut session.relay;
    socket.poll(|event| match event {
        SocketEvent::Joined(me) => {
            let (name, loadout) = profile_of(&settings, loadout.as_deref(), roster.as_deref());
            let profile = Profile {
                name,
                loadout,
                color: color_of(color.as_deref()),
                ..Default::default()
            };
            *lobby = Some(Lobby::new(me, profile, now, hosting));
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
            relay.forget(peer);
            if let Some(lobby) = lobby.as_mut() {
                lobby.disconnect(peer);
            }
        }
        SocketEvent::Signal(note) => {
            if let Some(line) = links.note(&note, now) {
                info!("network: {line}");
            }
            if let SignalNote::OtherHost(_) = note
                && hosting
            {
                warn!("network: this room has its host already: we leave it");
                *status = NetStatus::HostTaken;
            }
        }
        SocketEvent::Packet(peer, _, bytes) => {
            if let Some(lobby) = lobby.as_mut() {
                match Message::decode(&bytes) {
                    Ok(message) => {
                        // Issue #63: a seated client's kart goes in our next bundles.
                        if let Message::KartState(state) = &message
                            && hosting
                            && lobby.seats().contains(&peer)
                        {
                            relay.push(peer, *state);
                        }
                        to_the_game(
                            peer,
                            &message,
                            (lobby.host(), hosting),
                            game_ms,
                            sync.as_deref_mut(),
                            race.as_deref_mut(),
                            items.as_deref_mut(),
                            tuning.as_deref(),
                        );
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
        // Our karts' states (issue #8), the interactions and the race's events (issue #9), the
        // items (issue #53): issue #63, to our host, or (the host) to every seated client.
        let me = lobby.me();
        let host = lobby.host().filter(|_| !lobby.host_left());
        // Who our messages for everyone go to (12 at most: nothing allocated).
        let mut targets = [me; MAX_PLAYERS];
        let mut count = 0;
        for peer in lobby.peers() {
            let target = if hosting {
                peer.direct && peer.seated
            } else {
                Some(peer.key) == host
            };
            if target && count < MAX_PLAYERS {
                targets[count] = peer.key;
                count += 1;
            }
        }
        let targets = &targets[..count];
        let to_everyone = |socket: &mut NetSocket, message: &Message| {
            for peer in targets {
                socket.send(*peer, message);
            }
        };
        if let Some(sync) = sync.as_deref_mut() {
            if hosting {
                session.own_states.clear();
                session.own_states.extend(sync.drain_outgoing());
                let interval = tuning
                    .as_deref()
                    .map_or(2, |t| t.send_interval_ticks.max(1));
                let interval_ms = (f64::from(interval) * TICK_MS).round() as u64;
                if session
                    .relay
                    .due(now, !session.own_states.is_empty(), interval_ms)
                {
                    for client in targets {
                        session
                            .relay
                            .bundles_for(*client, me, &session.own_states, |message| {
                                socket.send(*client, message)
                            });
                    }
                    session.relay.sent(now);
                }
            } else {
                for state in sync.drain_outgoing() {
                    to_everyone(socket, &Message::KartState(state));
                }
            }
            for message in sync.drain_events_out() {
                to_everyone(socket, &message);
            }
        }
        if let Some(race) = race.as_deref_mut() {
            for message in race.drain_outbox() {
                to_everyone(socket, &message);
            }
        }
        if let Some(items) = items.as_deref_mut() {
            for message in items.drain_outbox() {
                to_everyone(socket, &message);
            }
        }
        for event in lobby.drain_events() {
            info!("network: {event:?}");
            match &event {
                // Issue #64: arrived in the room, the lobby shows it.
                LobbyEvent::Seated(peer) if *peer == me => window.open = true,
                LobbyEvent::Left(peer) => {
                    // A player who leaves gives up the race (issue #9); its items out disappear
                    // (issue #53).
                    if let Some(race) = race.as_deref_mut() {
                        race.shared.leave(*peer);
                    }
                    if let Some(items) = items.as_deref_mut() {
                        items.left(*peer);
                    }
                }
                LobbyEvent::HostLeft => {
                    // Issue #63: the room is over; the game goes back to the level before.
                    warn!("network: the host left the room");
                    session.status = NetStatus::HostLeft;
                    window.open = true;
                }
                _ => {}
            }
        }
        if lobby.full() {
            session.status = NetStatus::Full;
        }
    }
    if matches!(
        session.status,
        NetStatus::Full | NetStatus::Failed(_) | NetStatus::HostLeft | NetStatus::HostTaken
    ) {
        // Refused, over, or already stopped: nothing to say, the signaling server tells the
        // others.
        if session.status == NetStatus::HostTaken {
            window.open = true;
        }
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
