//! The lobby of a room (issue #7, `docs/ARCHITECTURE.md` § « Réseau »): who is there, who is
//! ready, the host, the ping of each one. Pure logic: no Bevy, no socket. It reads the
//! [`Message`]s that arrive and the peers that come and go, and writes the messages to send
//! ([`Lobby::drain_outbox`]); the tests connect several lobbies to each other in memory.
//!
//! **A star around the host** (issue #63). The peer that created the room is its host, from the
//! start to the end ([`Lobby::new`] says which one we are): nothing to elect, whatever the ids.
//! Each client has a single link, with the host (the signaling keeps the others from opening,
//! `signaling::SignalFilter`); the host relays what a client tells everyone to the other clients,
//! with its origin ([`Message::Relayed`], [`Lobby::relay`]).
//!
//! **Seats.** A room holds [`MAX_PLAYERS`] players. The host is seated from the start; it seats
//! each client once its `Hello` came, in their order of arrival, and sends the list to every
//! client ([`Message::Seats`]): **one list, the host's**, that every client takes as it is. A
//! newcomer beyond 12 gets [`Message::RoomFull`] and leaves, with a message. A client seated
//! learns the others from the host: their `Hello` (relayed), and what is on in the room.
//!
//! **Arrivals and departures**: a peer that connects gets our [`Message::Hello`]. A client that
//! says [`Message::Bye`] or that the socket reports gone leaves the host's list; the host tells
//! the others (its `Bye`, relayed, and the new list). **The host leaving** ends the room for its
//! clients ([`LobbyEvent::HostLeft`]: « L'hôte a quitté la partie »); nobody takes over (a
//! possible improvement). The pings ([`PING_INTERVAL_MS`]) go between the host and each client
//! only: each one's latency and clock (`clock`); a peer silent for [`SILENT_MS`] shows as such
//! until the socket gives up on it.
//!
//! A peer of another protocol version is listed apart ([`DecodeError::OtherVersion`]): it never
//! sits and gets nothing more.
//!
//! **Colors** (issue #55): each player's `Hello` says the color it chose for its kart. Two
//! players who chose the same one: the first seat keeps it, the other wears the closest free one
//! ([`Lobby::color_of`], `kart::resolve_players`): the seats are the same on every peer, so every
//! peer paints the karts the same, whatever the order the `Hello`s arrived in.
//!
//! **The room** (issue #9): the host chooses its circuit ([`Lobby::choose_circuit`], everyone
//! drives freely on it) and starts the races ([`Lobby::announce_race`]); every client it seats
//! gets what is on. Issue #99: with its other settings ([`Lobby::choose_setup`], [`RoomSetup`]:
//! the mirror, the bots, the items), which every client sees and drives with. The room's clock ([`Lobby::room_time_ms`], `clock::RoomClock`) is the host's:
//! the race's ticks are counted on it.
//!
//! **The championship** (issue #122): the host chooses it with the room's settings (the mode,
//! and the championship's program), starts it ([`Lobby::start_championship`]), scores its races
//! from its closed results ([`Lobby::score_championship`]) and reaches its podium
//! ([`Lobby::championship_podium`]): each time, everyone is told its state
//! ([`Message::Championship`]), and so is a player seated meanwhile (it watches). The room's
//! settings again (the lobby's) end it.

use super::championship::Championship;
use super::clock::{PeerClock, RoomClock};
use super::links::Stuck;
use super::protocol::{
    ChampionshipState, DecodeError, MAX_PLAYERS, Message, PeerKey, Profile, RaceResults, RaceStart,
    RoomSetup,
};
use crate::kart::{KartColor, resolve_players};

/// A ping to every peer we are linked with this often (ms).
pub const PING_INTERVAL_MS: u64 = 1_000;

/// A peer we heard nothing from for this long is shown as silent (ms).
pub const SILENT_MS: u64 = 3_000;

/// Issue #63: a client that met no host this long after joining the room (no offer, no
/// connection): the room has no host, the lobby says so (ms). The host offers at once to a
/// newcomer; in the browser, its offer leaves once its candidates are gathered (3 s at most,
/// `NetSettings::ice_gathering_max_ms`).
pub const NO_HOST_MS: u64 = 5_000;

/// A message to send to one peer (on its channel, `Message::channel`).
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub to: PeerKey,
    pub message: Message,
}

/// What happened, for the log and the tests.
#[derive(Clone, Debug, PartialEq)]
pub enum LobbyEvent {
    /// A peer connected (not a player yet: see [`LobbyEvent::Seated`]).
    Connected(PeerKey),
    /// A player got a seat (us included).
    Seated(PeerKey),
    /// A player left (`Bye`, gone from the socket, or from the host's list).
    Left(PeerKey),
    /// Issue #63: the host left the room: it is over for its clients.
    HostLeft,
    /// The host refused us: the room is full.
    RoomFull,
    /// A peer runs another version of the protocol.
    OtherVersion(PeerKey, u8),
    /// The host starts the race (issue #9).
    RaceAnnounced(RaceStart),
    /// The host chose the room's circuit (issue #9): free driving on it. Issue #99: and its
    /// other settings.
    SetupChosen(RoomSetup),
    /// Issue #122: the host told its championship in progress (its races scored, its podium).
    ChampionshipTold(u8),
}

/// Another player of the room (or a peer waiting for its seat), as we know it.
#[derive(Clone, Debug)]
pub struct LobbyPeer {
    pub key: PeerKey,
    /// Its `Hello`: `None` until it arrives.
    pub profile: Option<Profile>,
    pub seated: bool,
    /// We (the host) refused it: the room was full.
    pub refused: bool,
    /// The version it speaks, when not ours.
    pub other_version: Option<u8>,
    /// Issue #63: we have a link with it (the host with each client, a client with its host).
    /// The other players, a client knows from its host.
    pub direct: bool,
    pub clock: PeerClock,
    pub last_heard_ms: u64,
    next_ping_ms: u64,
}

impl LobbyPeer {
    fn new(key: PeerKey, direct: bool, now_ms: u64) -> Self {
        Self {
            key,
            profile: None,
            seated: false,
            refused: false,
            other_version: None,
            direct,
            clock: PeerClock::default(),
            last_heard_ms: now_ms,
            next_ping_ms: now_ms,
        }
    }

    /// It takes part in the lobby: same version, not refused.
    fn talks(&self) -> bool {
        !self.refused && self.other_version.is_none()
    }
}

/// Issue #63: how the peer of a row is linked with us (the lobby's column « Lien »).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowLink {
    /// Our own row.
    Me,
    /// Linked with us: the host, or one of our clients.
    Direct,
    /// Another client: there through the host.
    ViaHost,
    /// The link with the host being made (`links.rs`, issue #49).
    Connecting,
    /// It does not come, and why.
    Stuck(Stuck),
}

/// A line of the lobby's list, for the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerRow {
    pub key: PeerKey,
    pub name: String,
    pub loadout: [u8; 4],
    /// The color it chose (issue #55; `None`: its `Hello` has not arrived).
    pub chosen: Option<KartColor>,
    /// The color it wears ([`Lobby::color_of`]): the one it chose, unless a player of an earlier
    /// seat chose it too.
    pub color: Option<KartColor>,
    pub ready: bool,
    pub me: bool,
    pub host: bool,
    pub seated: bool,
    /// Issue #63: how we are linked with it.
    pub link: RowLink,
    /// Smoothed round trip (ms); `None` for us, a player we are not linked with, or before the
    /// first pong.
    pub ping_ms: Option<f32>,
    /// Nothing heard from it for [`SILENT_MS`] (a peer we are linked with).
    pub silent: bool,
    pub other_version: Option<u8>,
}

/// The lobby, seen from one peer (see the module's documentation).
#[derive(Clone, Debug)]
pub struct Lobby {
    me: PeerKey,
    /// Ours: name, loadout, ready, seated.
    profile: Profile,
    /// We host the room (we created it).
    hosting: bool,
    joined_ms: u64,
    /// The other players and peers, in the order we met them.
    peers: Vec<LobbyPeer>,
    /// Seated players (us included), in seat order: the host's list.
    seats: Vec<PeerKey>,
    /// Us, or the client's host once connected.
    host: Option<PeerKey>,
    /// The seats changed since we, the host, last sent them.
    seats_dirty: bool,
    full: bool,
    left: bool,
    /// Issue #63: our host left: the room is over.
    host_left: bool,
    announced: Option<RaceStart>,
    /// The room's circuit (issue #9): the host's choice, index in `assets/tracks/index.ron`; issue
    /// #99: with its other settings.
    setup: RoomSetup,
    /// The number of the last race announced (0: none yet): the next one counts on.
    last_race: u16,
    /// Issue #122, the host: the championship in progress (its scoring with it).
    run: Option<Championship>,
    /// Issue #122, a client: the championship in progress, as the host told it.
    told: Option<ChampionshipState>,
    /// Issue #136, a client: the race whose results we left (« Retour au salon »): over for us,
    /// in the lobby already, before the host brings everyone back. Ours only (not sent).
    left_results: Option<u16>,
    /// The room's clock (issue #9): the host's.
    room: RoomClock,
    outbox: Vec<Outgoing>,
    events: Vec<LobbyEvent>,
}

impl Lobby {
    /// We just joined the room as `me` (the id the signaling server gave us), at `now_ms`: as its
    /// host (`hosting`: we created it; seated at once, the room's clock ours), or as a client.
    pub fn new(me: PeerKey, mut profile: Profile, now_ms: u64, hosting: bool) -> Self {
        profile.name = Profile::clean_name(&profile.name);
        profile.seated = hosting;
        let mut room = RoomClock::default();
        room.update(hosting, None);
        Self {
            me,
            profile,
            hosting,
            joined_ms: now_ms,
            peers: Vec::new(),
            seats: if hosting { vec![me] } else { Vec::new() },
            host: hosting.then_some(me),
            seats_dirty: false,
            full: false,
            left: false,
            host_left: false,
            announced: None,
            setup: RoomSetup::default(),
            last_race: 0,
            run: None,
            told: None,
            left_results: None,
            room,
            outbox: Vec::new(),
            events: if hosting {
                vec![LobbyEvent::Seated(me)]
            } else {
                Vec::new()
            },
        }
    }

    pub fn me(&self) -> PeerKey {
        self.me
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The host: us, or the one a client connected to (`None` before).
    pub fn host(&self) -> Option<PeerKey> {
        self.host
    }

    pub fn is_host(&self) -> bool {
        self.hosting
    }

    /// When we joined the room (ms).
    pub fn joined_ms(&self) -> u64 {
        self.joined_ms
    }

    /// We have a seat.
    pub fn seated(&self) -> bool {
        self.profile.seated
    }

    /// The host refused us: the room is full.
    pub fn full(&self) -> bool {
        self.full
    }

    /// Issue #63: our host left the room.
    pub fn host_left(&self) -> bool {
        self.host_left
    }

    /// Seated players, us included, in seat order: the host's list.
    pub fn seats(&self) -> &[PeerKey] {
        &self.seats
    }

    pub fn peers(&self) -> &[LobbyPeer] {
        &self.peers
    }

    pub fn peer(&self, key: PeerKey) -> Option<&LobbyPeer> {
        self.peers.iter().find(|peer| peer.key == key)
    }

    /// The race the host announced (issue #9), until it chooses a circuit again.
    pub fn announced(&self) -> Option<&RaceStart> {
        self.announced.as_ref()
    }

    /// Issue #136, a client: we leave the results of the race announced (« Retour au salon »):
    /// it is over for us, we go to the lobby before the host brings everyone back. Not the host
    /// (its « Retour au salon » brings everyone: `NetAction::ChooseSetup`), nor before a race.
    pub fn leave_results(&mut self) -> bool {
        let Some(race) = self.announced.as_ref().map(|start| start.race) else {
            return false;
        };
        if self.hosting {
            return false;
        }
        self.left_results = Some(race);
        true
    }

    /// Issue #136: we left the results of the race announced (`leave_results`): for us, it is
    /// over, and we are in the lobby. A new race is another one.
    pub fn left_the_results(&self) -> bool {
        self.left_results.is_some()
            && self.left_results == self.announced.as_ref().map(|start| start.race)
    }

    /// The room's circuit (issue #9): the host's choice.
    pub fn circuit(&self) -> u8 {
        self.setup.track
    }

    /// Issue #99: the room's settings: the host's choice (the race on keeps its own:
    /// [`Lobby::announced`]).
    pub fn setup(&self) -> RoomSetup {
        self.setup
    }

    /// Issue #122: the championship in progress (the host's, or as it told it), until the room's
    /// settings come again.
    pub fn championship(&self) -> Option<&ChampionshipState> {
        self.run
            .as_ref()
            .map(|run| &run.state)
            .or(self.told.as_ref())
    }

    /// Issue #122, the host: its championship in progress, with its scoring.
    pub fn championship_run(&self) -> Option<&Championship> {
        self.run.as_ref()
    }

    /// The number the next race takes: one more than the last one announced.
    pub fn next_race(&self) -> u16 {
        self.last_race.wrapping_add(1).max(1)
    }

    /// At least two players, all of them ready.
    pub fn all_ready(&self) -> bool {
        self.seats.len() >= 2
            && self.profile.ready
            && self.seats.iter().filter(|key| **key != self.me).all(|key| {
                self.peer(*key)
                    .and_then(|peer| peer.profile.as_ref())
                    .is_some_and(|profile| profile.ready)
            })
    }

    /// The host's clock at our `now_ms`: the room's clock (issue #9, [`Lobby::room_time_ms`]).
    pub fn host_time_ms(&self, now_ms: u64) -> Option<u64> {
        self.room_time_ms(now_ms)
    }

    /// The room's clock at our `now_ms` (ms): the host's (`clock::RoomClock`). `None` until a
    /// pong of the host told it a client.
    pub fn room_time_ms(&self, now_ms: u64) -> Option<u64> {
        self.room.time_ms(now_ms)
    }

    /// The messages to send, oldest first.
    pub fn drain_outbox(&mut self) -> std::vec::Drain<'_, Outgoing> {
        self.outbox.drain(..)
    }

    /// What happened since the last call.
    pub fn drain_events(&mut self) -> std::vec::Drain<'_, LobbyEvent> {
        self.events.drain(..)
    }

    /// The socket connected us to `peer`: a client of ours, or our host. A client takes only one
    /// (the signaling lets only its host's offer through); a second report changes nothing.
    pub fn connect(&mut self, peer: PeerKey, now_ms: u64) {
        if peer == self.me || self.peer(peer).is_some() || self.stopped() {
            return;
        }
        if !self.hosting {
            if self.host.is_some() {
                return;
            }
            self.host = Some(peer);
        }
        self.peers.push(LobbyPeer::new(peer, true, now_ms));
        self.events.push(LobbyEvent::Connected(peer));
        self.send(peer, Message::Hello(self.profile.clone()));
        self.refresh();
    }

    /// `peer` is gone (the socket says so, or it said `Bye`): a client of ours leaves the room;
    /// our host gone, the room is over.
    pub fn disconnect(&mut self, peer: PeerKey) {
        if !self.hosting {
            if self.host == Some(peer) && !self.host_left {
                self.host_left = true;
                self.events.push(LobbyEvent::HostLeft);
            }
            return;
        }
        let Some(index) = self.peers.iter().position(|p| p.key == peer) else {
            return;
        };
        if self.peers[index].seated {
            // The others learn it from us: its `Bye`, then the new list.
            self.relay(peer, &Message::Bye);
            self.seats.retain(|key| *key != peer);
            self.seats_dirty = true;
        }
        self.peers.remove(index);
        self.events.push(LobbyEvent::Left(peer));
        self.refresh();
    }

    /// A message from `from` arrived at `now_ms`. A client only listens to its host (the others'
    /// come relayed by it).
    pub fn receive(&mut self, from: PeerKey, message: Message, now_ms: u64) {
        if self.stopped() {
            return;
        }
        if !self.hosting && self.host != Some(from) {
            return;
        }
        let Some(index) = self.peers.iter().position(|p| p.key == from) else {
            return;
        };
        let peer = &mut self.peers[index];
        peer.last_heard_ms = now_ms;
        if !peer.talks() {
            return;
        }
        match message {
            Message::Hello(mut profile) => {
                profile.name = Profile::clean_name(&profile.name);
                let seated = peer.seated;
                peer.profile = Some(profile.clone());
                // A newcomer: seated by `refresh`; a player: the others learn its new profile.
                if self.hosting && seated {
                    self.relay(from, &Message::Hello(profile));
                }
            }
            Message::Ready(ready) => {
                if let Some(profile) = &mut peer.profile {
                    profile.ready = ready;
                }
                if self.hosting && peer.seated {
                    self.relay(from, &Message::Ready(ready));
                }
            }
            Message::Seats(list) => {
                if !self.hosting {
                    self.take_seats(list, now_ms);
                }
            }
            Message::RoomFull => {
                if !self.hosting {
                    self.full = true;
                    self.events.push(LobbyEvent::RoomFull);
                }
            }
            Message::Bye => {
                self.disconnect(from);
                return;
            }
            Message::StartRace(start) => {
                if !self.hosting {
                    // Issue #99: the race's settings are the room's (its bots: as many as it has,
                    // or as the room had; issue #153, at their level). Issue #122: the mode and
                    // the championship stay.
                    self.setup = RoomSetup {
                        track: start.track,
                        mirror: start.mirror,
                        items: start.items,
                        bots: start.bots > 0 || self.setup.bots,
                        difficulty: if start.bots > 0 {
                            start.difficulty
                        } else {
                            self.setup.difficulty
                        },
                        ..self.setup
                    };
                    self.last_race = start.race;
                    self.announced = Some(start.clone());
                    self.events.push(LobbyEvent::RaceAnnounced(start));
                }
            }
            Message::Setup(setup) => {
                if !self.hosting {
                    self.setup = setup;
                    self.announced = None;
                    // Issue #122: the lobby again: the championship is over.
                    self.told = None;
                    self.events.push(LobbyEvent::SetupChosen(setup));
                }
            }
            Message::Championship(state) => {
                if !self.hosting {
                    let scored = state.scored;
                    self.told = Some(state);
                    self.events.push(LobbyEvent::ChampionshipTold(scored));
                }
            }
            Message::Relayed { origin, message } => {
                if !self.hosting {
                    self.relayed(origin, *message, now_ms);
                }
            }
            // The race's events, the interactions between karts (issue #9), the items (issue
            // #53) and the kart states (issue #8) go to the race, the synchronization and the
            // items (`poll_session`). Issue #63: the host relays a client's to the others (not
            // the kart states: they go in its bundles; not the verdicts: only its own count).
            Message::Progress(_)
            | Message::RaceOver(_)
            | Message::RaceResults(_)
            | Message::Strike(_)
            | Message::StrikeReply(_)
            | Message::Grab(_)
            | Message::TetherBreak(_)
            | Message::Claim(_)
            | Message::Verdict(_)
            | Message::ItemOut(_)
            | Message::ItemBreak(_)
            | Message::Scatter(_)
            | Message::Blast(_)
            | Message::Honk(_)
            | Message::Effect(_)
            | Message::Rammed(_)
            | Message::PillowGone(_)
            | Message::PanelDown(_)
            | Message::Retarget(_)
            | Message::Steal(_)
            | Message::Stolen(_)
            | Message::KartState(_)
            | Message::KartStates(_)
            // Issue #101: the characters on foot of the lobby and their strikes (the host
            // relays the strikes; the states go in its bundles).
            | Message::FootState(_)
            | Message::FootStates(_)
            | Message::FootStrike(_) => {
                if self.hosting {
                    self.relay(from, &message);
                }
            }
            Message::Ping { sent_ms } => {
                let (clock_ms, room) = self.room.pong(now_ms);
                let pong = Message::Pong {
                    echo_ms: sent_ms,
                    clock_ms,
                    room,
                };
                self.send(from, pong);
            }
            Message::Pong {
                echo_ms,
                clock_ms,
                room,
            } => {
                peer.clock.sample(now_ms, echo_ms, clock_ms, room);
            }
        }
        self.refresh();
        self.update_room();
    }

    /// Issue #63: we, the host, pass what the client `origin` told everyone to the other seated
    /// clients, with its origin (`Message::Relayed`). Only what a client tells everyone
    /// ([`Message::relayable`]), and only a seated client's. A client relays nothing.
    pub fn relay(&mut self, origin: PeerKey, message: &Message) {
        if !self.hosting || !message.relayable() || !self.seats.contains(&origin) {
            return;
        }
        for peer in &self.peers {
            if peer.key != origin && peer.seated && peer.talks() {
                self.outbox.push(Outgoing {
                    to: peer.key,
                    message: Message::Relayed {
                        origin,
                        message: Box::new(message.clone()),
                    },
                });
            }
        }
    }

    /// A client: what another player told everyone, relayed by our host. Its `Hello`, « Prêt »,
    /// its `Bye`; the rest goes to the game (`poll_session`).
    fn relayed(&mut self, origin: PeerKey, message: Message, now_ms: u64) {
        if origin == self.me || Some(origin) == self.host {
            return;
        }
        match message {
            Message::Hello(mut profile) => {
                profile.name = Profile::clean_name(&profile.name);
                let seated = self.seats.contains(&origin);
                match self.peers.iter_mut().find(|p| p.key == origin) {
                    Some(peer) => peer.profile = Some(profile),
                    None => {
                        let mut peer = LobbyPeer::new(origin, false, now_ms);
                        peer.profile = Some(profile);
                        peer.seated = seated;
                        self.peers.push(peer);
                    }
                }
            }
            Message::Ready(ready) => {
                if let Some(profile) = self
                    .peers
                    .iter_mut()
                    .find(|p| p.key == origin)
                    .and_then(|p| p.profile.as_mut())
                {
                    profile.ready = ready;
                }
            }
            Message::Bye => self.forget(origin),
            _ => {}
        }
    }

    /// A client: the host's list of seats, taken as it is. The players it no longer lists left.
    fn take_seats(&mut self, list: Vec<PeerKey>, now_ms: u64) {
        let gone: Vec<PeerKey> = self
            .peers
            .iter()
            .filter(|peer| !peer.direct && !list.contains(&peer.key))
            .map(|peer| peer.key)
            .collect();
        for key in gone {
            self.forget(key);
        }
        for key in &list {
            if *key != self.me && self.peer(*key).is_none() {
                self.peers.push(LobbyPeer::new(*key, false, now_ms));
            }
        }
        for peer in &mut self.peers {
            let seated = list.contains(&peer.key);
            if seated && !peer.seated {
                self.events.push(LobbyEvent::Seated(peer.key));
            }
            peer.seated = seated;
        }
        let seated = list.contains(&self.me);
        if seated && !self.profile.seated {
            self.events.push(LobbyEvent::Seated(self.me));
        }
        self.profile.seated = seated;
        self.seats = list;
    }

    /// A client: another player left (its `Bye` relayed, or gone from the host's list).
    fn forget(&mut self, key: PeerKey) {
        let Some(index) = self.peers.iter().position(|p| p.key == key && !p.direct) else {
            return;
        };
        self.peers.remove(index);
        self.seats.retain(|seat| *seat != key);
        self.events.push(LobbyEvent::Left(key));
    }

    /// The room's clock: ours when we host, the host's for a client (`clock::RoomClock`).
    fn update_room(&mut self) {
        let host_clock = self
            .host
            .filter(|host| *host != self.me && !self.host_left)
            .and_then(|host| self.peers.iter().find(|peer| peer.key == host))
            .map(|peer| &peer.clock);
        self.room.update(self.hosting, host_clock);
    }

    /// A packet from `from` could not be read. Another version: the peer is listed apart and
    /// gets nothing more; anything else is dropped.
    pub fn receive_error(&mut self, from: PeerKey, error: DecodeError, now_ms: u64) {
        let Some(peer) = self.peers.iter_mut().find(|p| p.key == from) else {
            return;
        };
        peer.last_heard_ms = now_ms;
        if let DecodeError::OtherVersion(version) = error
            && peer.other_version.is_none()
        {
            peer.other_version = Some(version);
            let was_seated = peer.seated;
            peer.seated = false;
            if was_seated {
                self.seats.retain(|key| *key != from);
                self.seats_dirty = true;
            }
            self.events.push(LobbyEvent::OtherVersion(from, version));
            self.refresh();
        }
    }

    /// Time passes: the pings.
    pub fn update(&mut self, now_ms: u64) {
        if self.stopped() {
            return;
        }
        for index in 0..self.peers.len() {
            let peer = &mut self.peers[index];
            if peer.direct && peer.talks() && now_ms >= peer.next_ping_ms {
                peer.next_ping_ms = now_ms + PING_INTERVAL_MS;
                let key = peer.key;
                self.send(
                    key,
                    Message::Ping {
                        sent_ms: now_ms as u32,
                    },
                );
            }
        }
        self.refresh();
        self.update_room();
    }

    /// « Prêt » or not.
    pub fn set_ready(&mut self, ready: bool) {
        if self.profile.ready != ready && !self.stopped() {
            self.profile.ready = ready;
            self.broadcast(Message::Ready(ready));
        }
    }

    /// Our name, loadout or color changed (selection screen).
    pub fn set_profile(&mut self, name: &str, loadout: [u8; 4], color: KartColor) {
        let name = Profile::clean_name(name);
        let changed = self.profile.name != name
            || self.profile.loadout != loadout
            || self.profile.color != color;
        if changed && !self.stopped() {
            self.profile.name = name;
            self.profile.loadout = loadout;
            self.profile.color = color;
            self.broadcast(Message::Hello(self.profile.clone()));
        }
    }

    /// The color `key` chose (its `Hello`; ours: our profile).
    fn chosen_color(&self, key: PeerKey) -> Option<KartColor> {
        if key == self.me {
            return Some(self.profile.color);
        }
        self.peer(key)?
            .profile
            .as_ref()
            .map(|profile| profile.color)
    }

    /// The color `key`'s kart wears (issue #55): for a seated player, the one it chose unless a
    /// player of an earlier seat chose it too (then the closest free one,
    /// `kart::resolve_players`); for a peer waiting for its seat, the one it chose. `None`: an
    /// unknown peer, or its `Hello` has not arrived. Nothing allocated (12 seats at most).
    pub fn color_of(&self, key: PeerKey) -> Option<KartColor> {
        let Some(seat) = self.seats.iter().position(|seated| *seated == key) else {
            return self.chosen_color(key);
        };
        let worn = resolve_players(self.seats.iter().map(|seated| self.chosen_color(*seated)));
        worn.get(seat).copied().flatten()
    }

    /// We leave: `Bye` to the peers we are linked with, then nothing more.
    pub fn leave(&mut self) {
        if !self.stopped() {
            self.broadcast(Message::Bye);
            self.left = true;
        }
    }

    /// The host starts the race (issue #9): only the host may.
    pub fn announce_race(&mut self, start: RaceStart) -> bool {
        if !self.hosting || self.stopped() {
            return false;
        }
        self.setup.track = start.track;
        self.last_race = start.race;
        self.announced = Some(start.clone());
        self.broadcast(Message::StartRace(start));
        true
    }

    /// The host chooses the room's circuit (issue #9): everyone drives freely on it, and the
    /// race that was on (or over) is gone. Only the host may.
    pub fn choose_circuit(&mut self, track: u8) -> bool {
        let setup = RoomSetup {
            track,
            ..self.setup
        };
        self.choose_setup(setup)
    }

    /// Issue #99: the host chooses the room's settings (the circuit, the mirror, the bots, the
    /// items), like its circuit: everyone drives freely with them, the race that was on (or over)
    /// is gone, and the next race takes them. Only the host may. Issue #122: with the mode and the
    /// championship; the championship in progress (or over) is gone too.
    pub fn choose_setup(&mut self, setup: RoomSetup) -> bool {
        if !self.hosting || self.stopped() {
            return false;
        }
        self.setup = setup;
        self.announced = None;
        self.run = None;
        self.broadcast(Message::Setup(setup));
        true
    }

    /// Issue #122: the host starts a championship (its first race follows): everyone is told it.
    /// Only the host may.
    pub fn start_championship(&mut self, championship: Championship) -> bool {
        if !self.hosting || self.stopped() {
            return false;
        }
        self.broadcast(Message::Championship(championship.state.clone()));
        self.run = Some(championship);
        true
    }

    /// Issue #122, the host: the race announced closed its results (`results`, its own table):
    /// if it is the next race of the championship in progress, it is scored (`points`: those of
    /// each place) and everyone is told the standings. Returns whether it scored.
    pub fn score_championship(&mut self, results: &RaceResults, points: &[u32]) -> bool {
        if !self.hosting || self.stopped() {
            return false;
        }
        let (Some(run), Some(start)) = (self.run.as_mut(), self.announced.as_ref()) else {
            return false;
        };
        if !run.score(start, results, points) {
            return false;
        }
        let state = run.state.clone();
        self.broadcast(Message::Championship(state));
        true
    }

    /// Issue #122, the host: after the last race's standings, the podium, for everyone.
    pub fn championship_podium(&mut self) -> bool {
        if !self.hosting || self.stopped() {
            return false;
        }
        let Some(run) = self
            .run
            .as_mut()
            .filter(|run| run.finished() && !run.state.podium)
        else {
            return false;
        };
        run.podium();
        let state = run.state.clone();
        self.broadcast(Message::Championship(state));
        true
    }

    /// What is on in the room, for a client we (the host) just seated: the race, or the settings.
    /// Issue #122: the championship in progress first (it watches it).
    fn tell_room(&mut self, to: PeerKey) {
        if let Some(run) = &self.run {
            let state = run.state.clone();
            self.send(to, Message::Championship(state));
        }
        let message = match &self.announced {
            Some(start) => Message::StartRace(start.clone()),
            None => Message::Setup(self.setup),
        };
        self.send(to, message);
    }

    /// The lobby's list: seated players in seat order, then the peers waiting for a seat, then
    /// those of another version.
    pub fn players(&self, now_ms: u64) -> Vec<PlayerRow> {
        let mut rows: Vec<PlayerRow> = Vec::with_capacity(self.peers.len() + 1);
        let row_of_peer = |peer: &LobbyPeer| {
            let chosen = peer.profile.as_ref().map(|profile| profile.color);
            let profile = peer.profile.clone().unwrap_or_default();
            PlayerRow {
                key: peer.key,
                name: profile.name,
                loadout: profile.loadout,
                chosen,
                color: self.color_of(peer.key),
                ready: profile.ready,
                me: false,
                host: self.host == Some(peer.key),
                seated: peer.seated,
                link: if peer.direct {
                    RowLink::Direct
                } else {
                    RowLink::ViaHost
                },
                ping_ms: peer.clock.rtt_ms().filter(|_| peer.direct),
                silent: peer.direct && now_ms.saturating_sub(peer.last_heard_ms) >= SILENT_MS,
                other_version: peer.other_version,
            }
        };
        let my_row = PlayerRow {
            key: self.me,
            name: self.profile.name.clone(),
            loadout: self.profile.loadout,
            chosen: Some(self.profile.color),
            color: self.color_of(self.me),
            ready: self.profile.ready,
            me: true,
            host: self.hosting,
            seated: self.profile.seated,
            link: RowLink::Me,
            ping_ms: None,
            silent: false,
            other_version: None,
        };
        for key in &self.seats {
            if *key == self.me {
                rows.push(my_row.clone());
            } else if let Some(peer) = self.peer(*key) {
                rows.push(row_of_peer(peer));
            }
        }
        if !self.profile.seated {
            rows.push(my_row);
        }
        for waiting in [true, false] {
            for peer in &self.peers {
                if !peer.seated && !peer.refused && peer.other_version.is_none() == waiting {
                    rows.push(row_of_peer(peer));
                }
            }
        }
        rows
    }

    /// Nothing more to do: refused, gone, or our host gone.
    fn stopped(&self) -> bool {
        self.full || self.left || self.host_left
    }

    fn send(&mut self, to: PeerKey, message: Message) {
        self.outbox.push(Outgoing { to, message });
    }

    /// To every peer we are linked with that takes part (a client: its host).
    fn broadcast(&mut self, message: Message) {
        for peer in &self.peers {
            if peer.direct && peer.talks() {
                self.outbox.push(Outgoing {
                    to: peer.key,
                    message: message.clone(),
                });
            }
        }
    }

    /// We, the host, seat the client `key`: it learns the others (their `Hello`, relayed) and
    /// what is on in the room; the others learn it (its `Hello`, relayed); everyone gets the new
    /// list of seats (`refresh`).
    fn sit(&mut self, key: PeerKey) {
        let Some(index) = self.peers.iter().position(|p| p.key == key) else {
            return;
        };
        if self.peers[index].seated || !self.peers[index].talks() {
            return;
        }
        self.peers[index].seated = true;
        self.seats.push(key);
        self.seats_dirty = true;
        self.events.push(LobbyEvent::Seated(key));
        let others: Vec<Outgoing> = self
            .peers
            .iter()
            .filter(|peer| peer.key != key && peer.seated && peer.talks())
            .filter_map(|peer| {
                Some(Outgoing {
                    to: key,
                    message: Message::Relayed {
                        origin: peer.key,
                        message: Box::new(Message::Hello(peer.profile.clone()?)),
                    },
                })
            })
            .collect();
        self.outbox.extend(others);
        self.tell_room(key);
        if let Some(profile) = self.peers[index].profile.clone() {
            self.relay(key, &Message::Hello(profile));
        }
    }

    /// After every change, the host: it seats the newcomers in their order of arrival (refuses
    /// those beyond 12), and sends the list of seats when it changed.
    fn refresh(&mut self) {
        if !self.hosting || self.stopped() {
            return;
        }
        for index in 0..self.peers.len() {
            let peer = &self.peers[index];
            if peer.seated || !peer.talks() || peer.profile.is_none() {
                continue;
            }
            let key = peer.key;
            if self.seats.len() < MAX_PLAYERS {
                self.sit(key);
            } else {
                self.refuse(key);
            }
        }
        if self.seats_dirty {
            self.seats_dirty = false;
            let seats = Message::Seats(self.seats.clone());
            for peer in &self.peers {
                if peer.seated && peer.talks() {
                    self.outbox.push(Outgoing {
                        to: peer.key,
                        message: seats.clone(),
                    });
                }
            }
        }
    }

    /// We, the host, refuse `key`: the room is full.
    fn refuse(&mut self, key: PeerKey) {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.key == key) {
            peer.seated = false;
            peer.refused = true;
        }
        self.send(key, Message::RoomFull);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::Channel;

    /// Several lobbies of a room in memory, linked as the star links them (issue #63): the host
    /// with each client, never two clients. Every message goes through its bytes (`encode` then
    /// `decode`), with a delay: the order of the reliable channel is kept, the unreliable one can
    /// lose packets. A message sent to a peer one is not linked with fails the test.
    struct Star {
        lobbies: Vec<Lobby>,
        /// How far ahead of the star's time each lobby's clock is (it counts from its own
        /// start): (peer, ms).
        skews: Vec<(PeerKey, u64)>,
        /// The room's creator, once it opened the room.
        host: Option<PeerKey>,
        /// Who is linked with whom (both ways).
        links: Vec<(PeerKey, PeerKey)>,
        /// (arrival time, to, from, bytes).
        in_flight: Vec<(u64, PeerKey, PeerKey, Vec<u8>)>,
        now: u64,
        /// One way (ms).
        delay: u64,
        drop_unreliable: bool,
    }

    impl Star {
        fn new() -> Self {
            Self {
                lobbies: Vec::new(),
                skews: Vec::new(),
                host: None,
                links: Vec::new(),
                in_flight: Vec::new(),
                now: 0,
                delay: 20,
                drop_unreliable: false,
            }
        }

        fn key(n: u128) -> PeerKey {
            PeerKey(n << 64 | 0xabc)
        }

        fn profile(name: &str) -> Profile {
            Profile {
                name: name.into(),
                loadout: [1, 2, 0, 1],
                ..Default::default()
            }
        }

        /// « Créer une partie »: the room's creator, its host.
        fn create(&mut self, n: u128) -> PeerKey {
            let key = Self::key(n);
            let profile = Self::profile(&format!("Joueur {n}"));
            self.add(Lobby::new(key, profile, 0, true));
            self.host = Some(key);
            key
        }

        /// A player opens the invitation link: a client, linked to the host (if any).
        fn join(&mut self, n: u128) -> PeerKey {
            let key = Self::key(n);
            let profile = Self::profile(&format!("Joueur {n}"));
            self.add(Lobby::new(key, profile, 0, false));
            if let Some(host) = self.host {
                self.link(host, key);
            }
            key
        }

        /// A lobby whose clock starts now (`Lobby::new` with 0): it runs `now` behind the star.
        fn add(&mut self, lobby: Lobby) {
            // Unsigned clocks: a lobby is ahead of the star by a large margin minus its start.
            self.skews.push((lobby.me(), 1_000_000 - self.now));
            self.lobbies.push(lobby);
        }

        /// The time on `key`'s clock.
        fn local(&self, key: PeerKey) -> u64 {
            let skew = self.skews.iter().find(|(k, _)| *k == key).unwrap().1;
            self.now + skew - 1_000_000
        }

        fn link(&mut self, a: PeerKey, b: PeerKey) {
            self.links.push((a, b));
            let (now_a, now_b) = (self.local(a), self.local(b));
            self.lobby(a).connect(b, now_a);
            self.lobby(b).connect(a, now_b);
        }

        /// A peer's tab closes: the sockets linked with it report it gone.
        fn close(&mut self, key: PeerKey) {
            self.lobbies.retain(|lobby| lobby.me() != key);
            let linked: Vec<PeerKey> = self
                .links
                .iter()
                .filter_map(|(a, b)| {
                    if *a == key {
                        Some(*b)
                    } else if *b == key {
                        Some(*a)
                    } else {
                        None
                    }
                })
                .collect();
            self.links.retain(|(a, b)| *a != key && *b != key);
            self.in_flight
                .retain(|(_, to, from, _)| *to != key && *from != key);
            for other in linked {
                self.lobby(other).disconnect(key);
            }
            // What they say about it leaves now (another peer closing next does not get it).
            self.flush();
        }

        fn lobby(&mut self, key: PeerKey) -> &mut Lobby {
            self.lobbies.iter_mut().find(|l| l.me() == key).unwrap()
        }

        fn view(&self, key: PeerKey) -> &Lobby {
            self.lobbies.iter().find(|l| l.me() == key).unwrap()
        }

        fn linked(&self, a: PeerKey, b: PeerKey) -> bool {
            self.links.contains(&(a, b)) || self.links.contains(&(b, a))
        }

        /// `ms` milliseconds, one at a time.
        fn run(&mut self, ms: u64) {
            for _ in 0..ms {
                self.now += 1;
                let now = self.now;
                // Deliveries.
                let mut arrived = Vec::new();
                self.in_flight.retain(|(at, to, from, bytes)| {
                    if *at <= now {
                        arrived.push((*to, *from, bytes.clone()));
                        false
                    } else {
                        true
                    }
                });
                for (to, from, bytes) in arrived {
                    if self.lobbies.iter().all(|l| l.me() != to) {
                        continue;
                    }
                    let local = self.local(to);
                    let lobby = self.lobby(to);
                    match Message::decode(&bytes) {
                        Ok(message) => lobby.receive(from, message, local),
                        Err(error) => lobby.receive_error(from, error, local),
                    }
                }
                let keys: Vec<PeerKey> = self.lobbies.iter().map(Lobby::me).collect();
                for key in keys {
                    let local = self.local(key);
                    self.lobby(key).update(local);
                }
                self.check_host();
                self.flush();
            }
        }

        /// Until nothing is on its way, then `ms` more.
        fn settle(&mut self, ms: u64) {
            while !self.in_flight.is_empty() {
                self.run(1);
            }
            self.run(ms);
        }

        fn flush(&mut self) {
            let mut sent = Vec::new();
            for lobby in &mut self.lobbies {
                let from = lobby.me();
                for out in lobby.drain_outbox() {
                    sent.push((from, out));
                }
            }
            for (from, out) in sent {
                assert!(
                    self.linked(from, out.to),
                    "{from:?} sent to {:?}, not linked (the host: {:?}): {:?}",
                    out.to,
                    self.host,
                    out.message
                );
                if self.drop_unreliable && out.message.channel() == Channel::Unreliable {
                    continue;
                }
                let at = self.now + self.delay;
                self.in_flight
                    .push((at, out.to, from, out.message.to_bytes()));
            }
        }

        /// At every moment (issue #63): the host is the room's creator for every lobby that
        /// knows one, and it never changes.
        fn check_host(&self) {
            for lobby in &self.lobbies {
                if let Some(host) = lobby.host() {
                    assert_eq!(Some(host), self.host, "the host seen by {:?}", lobby.me());
                }
                assert_eq!(lobby.is_host(), Some(lobby.me()) == self.host);
            }
        }

        /// Every lobby agrees: the same host, the same seats.
        fn agreed(&self) -> (Option<PeerKey>, Vec<PeerKey>) {
            let first = &self.lobbies[0];
            for lobby in &self.lobbies {
                assert_eq!(lobby.host(), first.host(), "host seen by {:?}", lobby.me());
                assert_eq!(
                    lobby.seats(),
                    first.seats(),
                    "seats seen by {:?}",
                    lobby.me()
                );
            }
            (first.host(), first.seats().to_vec())
        }
    }

    /// Issue #7 (issue #63: was « the smallest id hosts »): two players find each other, see
    /// each other with their ping, and the room's creator hosts, though it has the bigger id.
    #[test]
    fn two_peers_see_each_other_with_their_ping_and_the_creator_hosts() {
        let mut star = Star::new();
        let b = star.create(7);
        star.run(10);
        assert!(star.view(b).seated(), "the creator is seated at once");
        assert!(star.view(b).is_host());
        let a = star.join(3);
        star.run(2_000);
        let (host, seats) = star.agreed();
        assert_eq!(seats, [b, a], "arrival order");
        assert_eq!(host, Some(b), "the creator hosts, whatever the ids");
        for (me, other) in [(a, b), (b, a)] {
            let rows = star.view(me).players(star.local(me));
            assert_eq!(rows.len(), 2);
            let row = rows.iter().find(|r| r.key == other).unwrap();
            assert_eq!(row.name, format!("Joueur {}", other.0 >> 64));
            assert_eq!(row.loadout, [1, 2, 0, 1]);
            assert!(!row.silent);
            assert_eq!(row.link, RowLink::Direct);
            let ping = row.ping_ms.expect("a ping");
            assert!((ping - 40.0).abs() < 2.0, "{ping}");
            assert!(rows.iter().any(|r| r.me && r.key == me));
            assert_eq!(rows.iter().filter(|r| r.host).count(), 1);
        }
    }

    /// « Prêt » reaches the others, a client's through the host (issue #63); everyone ready
    /// shows.
    #[test]
    fn ready_goes_to_everyone() {
        let mut star = Star::new();
        let a = star.create(1);
        let b = star.join(2);
        let c = star.join(3);
        star.run(1_000);
        star.lobby(b).set_ready(true);
        star.run(100);
        for key in [a, c] {
            let rows = star.view(key).players(star.local(key));
            assert!(rows.iter().find(|r| r.key == b).unwrap().ready);
            assert!(!rows.iter().find(|r| r.key == a).unwrap().ready);
            assert!(!star.view(key).all_ready());
        }
        star.lobby(a).set_ready(true);
        star.lobby(c).set_ready(true);
        star.run(100);
        for key in [a, b, c] {
            assert!(star.view(key).all_ready(), "{key:?}");
        }
        star.lobby(b).set_ready(false);
        star.run(100);
        assert!(!star.view(a).all_ready());
        assert!(!star.view(c).all_ready());
        // A changed name or kart reaches them too.
        star.lobby(c)
            .set_profile("  Max ", [4, 1, 1, 0], KartColor::Teal);
        star.run(100);
        for key in [a, b] {
            let rows = star.view(key).players(star.local(key));
            let row = rows.iter().find(|r| r.key == c).unwrap();
            assert_eq!((row.name.as_str(), row.loadout), ("Max", [4, 1, 1, 0]));
            // Issue #55: and a changed color.
            assert_eq!(row.color, Some(KartColor::Teal));
        }
        // Issue #63: Bob knows Cyd through the host only: no ping of his.
        let rows = star.view(b).players(star.local(b));
        let cyd = rows.iter().find(|r| r.key == c).unwrap();
        assert_eq!(cyd.link, RowLink::ViaHost);
        assert!(cyd.ping_ms.is_none() && !cyd.silent);
    }

    /// Issue #55: the colors the players chose reach everyone; two players of the same color:
    /// every lobby says the first seat keeps it and the other wears the closest free one, and
    /// the rows say both. A departure gives the color back.
    #[test]
    fn the_same_color_twice_is_resolved_the_same_everywhere() {
        use KartColor::*;
        let mut star = Star::new();
        let a = star.create(1);
        let b = star.join(2);
        let c = star.join(3);
        star.run(1_000);
        let (_, seats) = star.agreed();
        assert_eq!(seats, [a, b, c]);
        let colors = |star: &Star| -> Vec<Vec<Option<KartColor>>> {
            [a, b, c]
                .iter()
                .map(|key| seats.iter().map(|s| star.view(*key).color_of(*s)).collect())
                .collect()
        };
        // Nobody chose: all pink; the first seat keeps it, the others the closest free colors.
        for seen in colors(&star) {
            assert_eq!(seen, [Some(Pink), Some(Violet), Some(Orange)]);
        }
        // Issue #63: the host red, the two clients blue (the first seat leaving now ends the
        // room: its clients are the ones who choose the same).
        star.lobby(a).set_profile("Ana", [1, 2, 0, 1], Red);
        star.lobby(b).set_profile("Bob", [1, 2, 0, 1], Blue);
        star.lobby(c).set_profile("Cyd", [1, 2, 0, 1], Blue);
        star.run(200);
        for seen in colors(&star) {
            assert_eq!(seen, [Some(Red), Some(Blue), Some(Violet)]);
        }
        for key in [a, b, c] {
            let rows = star.view(key).players(star.local(key));
            let third = rows.iter().find(|r| r.key == c).unwrap();
            assert_eq!((third.chosen, third.color), (Some(Blue), Some(Violet)));
            let second = rows.iter().find(|r| r.key == b).unwrap();
            assert_eq!((second.chosen, second.color), (Some(Blue), Some(Blue)));
        }
        // Bob leaves: Cyd wears blue now, on every screen.
        star.close(b);
        star.run(1_000);
        for key in [a, c] {
            assert_eq!(star.view(key).color_of(c), Some(Blue));
            assert_eq!(star.view(key).color_of(a), Some(Red));
        }
        // An unknown peer has no color.
        assert_eq!(star.view(c).color_of(Star::key(99)), None);
    }

    /// Issue #63 (was « departures are seen and the host hands over », issue #7): a client who
    /// leaves disappears from every lobby, by the socket or by its `Bye`, and the host stays the
    /// host; the host leaving ends the room for every client (nobody takes over), and nothing is
    /// sent to the departed.
    #[test]
    fn departures_are_seen_and_the_host_never_changes() {
        let mut star = Star::new();
        let first = star.create(5);
        let keys: Vec<PeerKey> = [2, 9, 4].into_iter().map(|n| star.join(n)).collect();
        star.run(1_000);
        let (host, seats) = star.agreed();
        assert_eq!(host, Some(first));
        assert_eq!(seats, [first, keys[0], keys[1], keys[2]]);
        // A client's tab closes (the smallest id: nothing changes for the host).
        star.close(keys[0]);
        star.run(100);
        let (host, seats) = star.agreed();
        assert_eq!(host, Some(first));
        assert_eq!(seats, [first, keys[1], keys[2]]);
        // Another one says goodbye.
        star.lobby(keys[1]).leave();
        star.run(100);
        star.lobbies.retain(|l| l.me() != keys[1]);
        star.links.retain(|(_, b)| *b != keys[1]);
        let (host, seats) = star.agreed();
        assert_eq!(host, Some(first));
        assert_eq!(seats, [first, keys[2]]);
        for key in [first, keys[2]] {
            let events: Vec<LobbyEvent> = star.lobby(key).drain_events().collect();
            assert!(events.contains(&LobbyEvent::Left(keys[0])), "{key:?}");
            assert!(events.contains(&LobbyEvent::Left(keys[1])), "{key:?}");
            assert!(!events.contains(&LobbyEvent::HostLeft));
        }
        // The host's tab closes: the room is over for the client, said once.
        let late = star.join(7);
        star.run(1_000);
        star.close(first);
        star.run(100);
        for key in [keys[2], late] {
            let lobby = star.view(key);
            assert!(lobby.host_left(), "{key:?}");
            assert!(!lobby.is_host());
            assert_eq!(lobby.host(), Some(first), "nobody takes over");
            let events: Vec<LobbyEvent> = star.lobby(key).drain_events().collect();
            assert_eq!(
                events
                    .iter()
                    .filter(|e| **e == LobbyEvent::HostLeft)
                    .count(),
                1
            );
        }
        // Nothing is sent to the departed (the star would panic).
        star.run(3_000);
    }

    /// Issue #7: a 13th player is refused with a message, the 12 keep playing; issue #63: the
    /// newcomer's smallest id changes nothing, the creator hosts.
    #[test]
    fn a_full_room_refuses_the_thirteenth() {
        let mut star = Star::new();
        let first = star.create(500);
        for n in 1..MAX_PLAYERS as u128 {
            star.join(n * 10);
            star.run(200);
        }
        star.run(1_000);
        let (_, seats) = star.agreed();
        assert_eq!(seats.len(), MAX_PLAYERS);
        let late = star.join(1);
        star.run(1_000);
        assert!(star.view(late).full(), "refused");
        assert!(!star.view(late).seated());
        assert!(
            star.lobby(late)
                .drain_events()
                .any(|e| e == LobbyEvent::RoomFull),
            "with a message"
        );
        // Its socket closes.
        star.close(late);
        star.run(100);
        let (host, seats) = star.agreed();
        assert_eq!(seats.len(), MAX_PLAYERS);
        assert!(!seats.contains(&late));
        assert_eq!(host, Some(first));
        // A seat frees up: the next newcomer gets it, the last seat.
        star.close(Star::key(50));
        let next = star.join(2);
        star.run(1_000);
        assert!(star.view(next).seated());
        assert!(!star.view(next).is_host());
        let (host, seats) = star.agreed();
        assert_eq!((host, seats.last()), (Some(first), Some(&next)));
    }

    /// Issue #63 (was « two newcomers at once both sit and agree »): two friends open the
    /// invitation link at the same moment: the host seats both, in the order it met them, and
    /// every lobby has its list.
    #[test]
    fn two_clients_at_once_both_sit_in_the_hosts_order() {
        let mut star = Star::new();
        let host = star.create(9);
        star.run(500);
        let a = star.join(8);
        let b = star.join(6);
        star.run(500);
        let (seen_host, seats) = star.agreed();
        assert_eq!(seen_host, Some(host));
        assert_eq!(seats, [host, a, b]);
        assert!(star.view(a).seated() && star.view(b).seated());
    }

    /// Issue #8 (issue #63: the seat order is the host's list, as it is): the grid slot of each
    /// player is its place in the seat order, the same everywhere. A client takes the host's list
    /// whole: from the moment it is seated, its order is the host's.
    #[test]
    fn the_seat_order_is_the_hosts_list() {
        let mut star = Star::new();
        let a = star.create(8);
        assert!(star.view(a).seated(), "the creator, at once");
        let b = star.join(9);
        let c = star.join(1);
        let mut seated_at = None;
        for ms in 0..2_000 {
            star.run(1);
            for key in [b, c] {
                let view = star.view(key);
                if view.seated() {
                    seated_at.get_or_insert(ms);
                    assert_eq!(view.seats(), star.view(a).seats(), "{key:?} at {ms} ms");
                }
            }
        }
        assert!(seated_at.is_some(), "never seated");
        assert_eq!(star.view(c).seats(), &[a, b, c]);
    }

    /// A newcomer waits for the host to seat it, and never hosts (issue #63: even with the
    /// smallest id).
    #[test]
    fn a_newcomer_waits_for_the_host_to_seat_it() {
        let mut star = Star::new();
        let host = star.create(9);
        star.run(100);
        star.delay = 300;
        let late = star.join(1);
        star.run(100);
        assert!(!star.view(late).seated());
        assert_eq!(star.view(late).host(), Some(host), "linked to it");
        star.run(400);
        assert!(!star.view(late).seated(), "the host's word is on its way");
        star.run(1_000);
        assert!(star.view(late).seated());
        assert_eq!(star.agreed().0, Some(host));
    }

    /// Issue #63 (was « seating a smaller id hands over at once », issue #7): the host that seats
    /// a newcomer with a smaller id stays the host, and sends it the seats, the room's circuit
    /// and the others' `Hello`.
    #[test]
    fn seating_a_smaller_id_changes_nothing() {
        let (big, small, other) = (PeerKey(9), PeerKey(2), PeerKey(5));
        let mut lobby = Lobby::new(big, Star::profile("Grand"), 0, true);
        assert!(lobby.seated() && lobby.is_host());
        lobby.connect(other, 0);
        lobby.receive(other, Message::Hello(Star::profile("Autre")), 5);
        lobby.drain_outbox();
        lobby.connect(small, 0);
        lobby.receive(small, Message::Hello(Star::profile("Petit")), 10);
        assert_eq!(lobby.seats(), [big, other, small]);
        assert_eq!(lobby.host(), Some(big));
        assert!(lobby.is_host());
        let sent: Vec<Outgoing> = lobby.drain_outbox().collect();
        let to_small: Vec<&Message> = sent
            .iter()
            .filter(|out| out.to == small)
            .map(|out| &out.message)
            .collect();
        assert!(matches!(to_small[0], Message::Hello(profile) if profile.name == "Grand"));
        assert!(to_small.contains(&&Message::Relayed {
            origin: other,
            message: Box::new(Message::Hello(Star::profile("Autre"))),
        }));
        assert!(to_small.contains(&&Message::Setup(RoomSetup::default())));
        assert!(to_small.contains(&&Message::Seats(vec![big, other, small])));
        // The other client learns the newcomer, through us.
        assert!(sent.contains(&Outgoing {
            to: other,
            message: Message::Relayed {
                origin: small,
                message: Box::new(Message::Hello(Star::profile("Petit"))),
            },
        }));
    }

    /// The common clock: every peer reads the host's time, within a few ms.
    #[test]
    fn every_peer_reads_the_host_clock() {
        let mut star = Star::new();
        star.run(400);
        star.create(4);
        star.run(1_234);
        star.join(1);
        star.join(7);
        star.run(3_000);
        let (host, _) = star.agreed();
        let host = host.unwrap();
        // Each lobby counts from its own start: their clocks differ by 1 234 ms.
        let reference = star.local(host);
        assert_eq!(star.view(host).host_time_ms(reference), Some(reference));
        let mut differ = false;
        for lobby in &star.lobbies {
            let local = star.local(lobby.me());
            differ |= local.abs_diff(reference) > 1_000;
            let read = lobby.host_time_ms(local).unwrap();
            assert!(read.abs_diff(reference) <= 2, "{read} vs {reference}");
        }
        assert!(differ);
    }

    /// A peer of another version is listed apart, never seated, never sent anything more.
    #[test]
    fn another_version_is_listed_apart() {
        let mut lobby = Lobby::new(PeerKey(5), Star::profile("Moi"), 0, true);
        assert!(lobby.is_host());
        let stranger = PeerKey(1);
        lobby.connect(stranger, 10);
        lobby.drain_outbox();
        lobby.receive_error(stranger, DecodeError::OtherVersion(2), 20);
        lobby.receive_error(stranger, DecodeError::Truncated, 21);
        lobby.update(5_000);
        assert!(lobby.drain_outbox().all(|out| out.to != stranger));
        assert!(lobby.is_host());
        assert_eq!(lobby.seats(), [PeerKey(5)]);
        let rows = lobby.players(5_000);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].other_version, Some(2));
        assert!(rows[1].silent);
        // What it sends is ignored, even a Hello.
        lobby.receive(
            stranger,
            Message::Hello(Profile {
                seated: true,
                ..Profile::default()
            }),
            5_001,
        );
        assert!(lobby.is_host());
        assert_eq!(lobby.seats(), [PeerKey(5)]);
    }

    /// Messages that make no sense from where they come are ignored: a race started by someone
    /// else than the host, a room full sent by a client, anything from an unknown peer; issue
    /// #63: a client takes nothing from another peer than its host.
    #[test]
    fn only_the_host_starts_a_race_and_strangers_are_ignored() {
        let mut star = Star::new();
        let a = star.create(1);
        let b = star.join(2);
        star.run(500);
        let start = RaceStart {
            race: 1,
            seed: 42,
            track: 0,
            laps: 3,
            bots: 10,
            difficulty: 1,
            mirror: false,
            items: true,
            start_tick: 9_000,
            round: 0,
            lineup: Vec::new(),
            players: vec![a, b],
        };
        assert!(!star.lobby(b).announce_race(start.clone()));
        assert!(!star.lobby(b).choose_circuit(1));
        let (now_a, now_b) = (star.local(a), star.local(b));
        star.lobby(b)
            .receive(a, Message::StartRace(start.clone()), now_b);
        assert_eq!(star.view(b).announced(), Some(&start));
        star.lobby(a)
            .receive(b, Message::StartRace(start.clone()), now_a);
        let one = RoomSetup {
            track: 1,
            ..RoomSetup::default()
        };
        star.lobby(a).receive(b, Message::Setup(one), now_a);
        assert_eq!(star.view(a).announced(), None);
        assert_eq!(star.view(a).circuit(), 0);
        assert!(star.lobby(a).announce_race(start));
        star.lobby(a).receive(PeerKey(99), Message::Bye, now_a);
        star.lobby(a).receive(PeerKey(99), Message::RoomFull, now_a);
        assert!(!star.view(a).full());
        assert_eq!(star.view(a).seats().len(), 2);
        // A client: another peer's word, even its circuit or the seats, is not the host's.
        star.lobby(b)
            .receive(PeerKey(99), Message::Setup(one), now_b);
        star.lobby(b)
            .receive(PeerKey(99), Message::Seats(vec![PeerKey(99)]), now_b);
        star.lobby(b).receive(PeerKey(99), Message::RoomFull, now_b);
        assert_eq!(star.view(b).circuit(), 0);
        assert_eq!(star.view(b).seats(), [a, b]);
        assert!(!star.view(b).full());
    }

    /// Issue #9: the host chooses the circuit, then starts a race: every lobby has it, a
    /// newcomer seated later too; choosing a circuit again ends the race; the races are
    /// numbered. Issue #63: the host leaving ends the room (nobody counts on).
    #[test]
    fn the_host_chooses_the_circuit_and_starts_the_races() {
        let mut star = Star::new();
        let a = star.create(1);
        let b = star.join(2);
        star.run(1_000);
        assert!(star.lobby(a).is_host());
        assert!(star.lobby(a).choose_circuit(1));
        star.run(100);
        for key in [a, b] {
            assert_eq!(star.view(key).circuit(), 1, "{key:?}");
            assert_eq!(star.view(key).announced(), None);
        }
        assert!(star.lobby(b).drain_events().any(|event| event
            == LobbyEvent::SetupChosen(RoomSetup {
                track: 1,
                ..RoomSetup::default()
            })));
        assert_eq!(star.view(a).next_race(), 1);
        let start = RaceStart {
            race: star.view(a).next_race(),
            seed: 7,
            track: 1,
            laps: 3,
            bots: 10,
            difficulty: 1,
            mirror: false,
            items: true,
            start_tick: 12_000,
            round: 0,
            lineup: Vec::new(),
            players: vec![a, b],
        };
        assert!(star.lobby(a).announce_race(start.clone()));
        star.run(100);
        assert_eq!(star.view(b).announced(), Some(&start));
        // A newcomer seated during the race learns it.
        let c = star.join(3);
        star.run(1_000);
        assert!(star.view(c).seated());
        assert_eq!(star.view(c).announced(), Some(&start));
        assert_eq!(star.view(c).circuit(), 1);
        // A circuit again: free driving, the count goes on.
        assert!(star.lobby(a).choose_circuit(0));
        star.run(100);
        assert_eq!(star.view(c).announced(), None);
        assert_eq!(star.view(c).circuit(), 0);
        assert_eq!(
            star.view(a).next_race(),
            2,
            "the count goes on after free driving"
        );
        // A newcomer in free driving learns the circuit.
        let d = star.join(4);
        star.run(1_000);
        assert_eq!(star.view(d).circuit(), 0);
        assert_eq!(star.view(d).announced(), None);
        // The host leaves: nobody can start a race any more.
        star.close(a);
        star.run(100);
        for key in [b, c, d] {
            assert!(star.view(key).host_left());
            assert!(!star.lobby(key).choose_circuit(1));
        }
    }

    /// Issue #136: a client leaves the results of the race (« Retour au salon »): over for it
    /// alone, until the next race; not before a race, never the host (its button brings everyone
    /// back: the room's settings again).
    #[test]
    fn a_client_leaves_the_results_of_its_race_alone() {
        let mut star = Star::new();
        let a = star.create(1);
        let b = star.join(2);
        star.run(1_000);
        assert!(!star.lobby(b).leave_results(), "no race yet");
        let start = |race| RaceStart {
            race,
            seed: 7,
            track: 0,
            laps: 1,
            bots: 0,
            difficulty: 1,
            mirror: false,
            items: true,
            start_tick: 12_000,
            round: 0,
            lineup: Vec::new(),
            players: vec![a, b],
        };
        let first = star.view(a).next_race();
        assert!(star.lobby(a).announce_race(start(first)));
        star.run(100);
        assert!(!star.view(b).left_the_results());
        assert!(!star.lobby(a).leave_results(), "the host brings everyone");
        assert!(star.lobby(b).leave_results());
        assert!(star.view(b).left_the_results());
        assert!(!star.view(a).left_the_results(), "Bob's only");
        // The host brings everyone back, then starts another race: Bob is on it.
        assert!(star.lobby(a).choose_circuit(0));
        star.run(100);
        assert!(!star.view(b).left_the_results());
        let second = star.view(a).next_race();
        assert!(star.lobby(a).announce_race(start(second)));
        star.run(100);
        assert_eq!(star.view(b).announced().map(|s| s.race), Some(second));
        assert!(!star.view(b).left_the_results(), "another race");
    }

    /// Issue #99: the host's settings of the room (the circuit, the mirror, the bots, the items)
    /// reach every client, and a newcomer; a client cannot choose them; choosing the circuit
    /// alone keeps the others; a newcomer seated during a race takes them from the race.
    #[test]
    fn the_host_chooses_the_rooms_settings_and_everyone_sees_them() {
        let mut star = Star::new();
        let a = star.create(1);
        let b = star.join(2);
        star.run(1_000);
        let setup = RoomSetup {
            track: 2,
            mirror: true,
            bots: false,
            items: false,
            ..RoomSetup::default()
        };
        assert!(!star.lobby(b).choose_setup(setup));
        assert!(star.lobby(a).choose_setup(setup));
        star.run(100);
        for key in [a, b] {
            assert_eq!(star.view(key).setup(), setup, "{key:?}");
            assert_eq!(star.view(key).circuit(), 2);
        }
        assert!(
            star.lobby(b)
                .drain_events()
                .any(|event| event == LobbyEvent::SetupChosen(setup))
        );
        let c = star.join(3);
        star.run(1_000);
        assert_eq!(star.view(c).setup(), setup, "a newcomer learns them");
        assert!(star.lobby(a).choose_circuit(4));
        star.run(100);
        for key in [a, b, c] {
            assert_eq!(star.view(key).setup(), RoomSetup { track: 4, ..setup });
        }
        let start = RaceStart {
            race: star.view(a).next_race(),
            seed: 3,
            track: 4,
            laps: 3,
            bots: 0,
            difficulty: 1,
            mirror: true,
            items: false,
            start_tick: 20_000,
            round: 0,
            lineup: Vec::new(),
            players: vec![a, b, c],
        };
        assert!(star.lobby(a).announce_race(start.clone()));
        star.run(100);
        let d = star.join(4);
        star.run(1_000);
        assert_eq!(star.view(d).announced(), Some(&start));
        let seen = star.view(d).setup();
        assert_eq!((seen.track, seen.mirror, seen.items), (4, true, false));
    }

    /// Issue #63 (was « the room clock goes on when the host leaves », issue #9): the room's
    /// clock is the creator's for everyone, a newcomer with a smaller id changes nothing. Each
    /// lobby counts from its own start (clocks thousands of ms apart): at every millisecond, they
    /// all read the same room time within a few ms, and nobody's jumps; the host gone, its
    /// clients keep the clock where it was going.
    #[test]
    fn the_room_clock_is_the_creators() {
        let mut star = Star::new();
        let first = star.create(5);
        star.run(1_777);
        star.join(9);
        star.run(1_000);
        star.join(2);
        star.run(4_000);
        let read = |star: &Star| -> Vec<(PeerKey, i64)> {
            star.lobbies
                .iter()
                .map(|lobby| {
                    let time = lobby.room_time_ms(star.local(lobby.me())).expect("no room");
                    (lobby.me(), time as i64)
                })
                .collect()
        };
        let mut before = read(&star);
        let first_clock = star.local(first) as i64;
        for (key, time) in &before {
            assert!(
                (time - first_clock).abs() <= 3,
                "{key:?}: {time} vs {first_clock}"
            );
        }
        let mut spread_worst = 0;
        for ms in 0..3_000 {
            if ms == 1_000 {
                star.close(first);
            }
            star.run(1);
            let now = read(&star);
            let times: Vec<i64> = now.iter().map(|(_, t)| *t).collect();
            let spread = times.iter().max().unwrap() - times.iter().min().unwrap();
            spread_worst = spread_worst.max(spread);
            for (key, time) in &now {
                if let Some((_, then)) = before.iter().find(|(k, _)| k == key) {
                    assert!(
                        (time - then - 1).abs() <= 3,
                        "ms {ms}: {key:?} jumped {}",
                        time - then
                    );
                }
            }
            before = now;
        }
        assert!(spread_worst <= 4, "{spread_worst} ms apart");
    }

    /// Lost pings (unreliable channel) only leave the ping unknown; the lobby still works, and a
    /// peer silent for 3 s shows as such.
    #[test]
    fn lost_pings_do_not_break_the_lobby() {
        let mut star = Star::new();
        star.drop_unreliable = true;
        let a = star.create(1);
        let b = star.join(2);
        star.run(1_000);
        let rows = star.view(a).players(star.local(a));
        let row = rows.iter().find(|r| r.key == b).unwrap();
        assert!(row.seated);
        assert_eq!(row.ping_ms, None);
        star.run(SILENT_MS);
        let rows = star.view(a).players(star.local(a));
        assert!(rows.iter().find(|r| r.key == b).unwrap().silent);
    }

    /// A small generator for the orders of the tests (xorshift).
    struct Draws(u64);

    impl Draws {
        fn next(&mut self, below: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % below.max(1)
        }
    }

    /// The peers the host refused (the room was full) leave: their socket closes.
    fn close_refused(star: &mut Star, order: &mut Vec<PeerKey>) {
        let refused: Vec<PeerKey> = star
            .lobbies
            .iter()
            .filter(|lobby| lobby.full())
            .map(Lobby::me)
            .collect();
        for key in refused {
            star.close(key);
            order.retain(|k| *k != key);
        }
    }

    /// Issue #63 acceptance: a room of 12 (the host and 11 clients), with arrivals and
    /// departures in many orders (60 draws): clients that come one at a time or several in the
    /// same millisecond, leave by a closed tab or a `Bye`, come back as new peers, a 13th
    /// refused; random ids (the host's sometimes the smallest, sometimes not), random delays.
    /// At every millisecond, every lobby's host is the creator (it never changes); once the
    /// messages arrived, every lobby has the same list, in the order the host met the players,
    /// and so the same grid slot (kart id) for each.
    #[test]
    fn a_room_of_twelve_agrees_in_every_order() {
        for draw in 0..60u64 {
            let mut draws = Draws(0x9e37_79b9_7f4a_7c15 ^ (draw + 1).wrapping_mul(0x2545_f491));
            let mut star = Star::new();
            star.delay = 5 + draws.next(40);
            let host = star.create(u128::from(draws.next(1 << 40)) + 1);
            let mut order: Vec<PeerKey> = vec![host];
            let mut steps = 0;
            while steps < 40 {
                steps += 1;
                let clients = star.lobbies.len() - 1;
                match draws.next(10) {
                    // One or several come at once (beyond 12, refused: they leave).
                    0..=4 => {
                        for _ in 0..1 + draws.next(3) {
                            let key = star.join(u128::from(draws.next(1 << 40)) + 1);
                            order.push(key);
                        }
                    }
                    // A client leaves: its tab closes, or it says goodbye.
                    5..=7 if clients > 0 => {
                        let index = 1 + draws.next(clients as u64) as usize;
                        let key = star.lobbies[index].me();
                        if draws.next(2) == 0 {
                            star.close(key);
                        } else {
                            star.lobby(key).leave();
                            star.run(star.delay + 1);
                            star.close(key);
                        }
                        order.retain(|k| *k != key);
                    }
                    _ => {}
                }
                star.run(draws.next(300));
                close_refused(&mut star, &mut order);
                if draws.next(4) == 0 {
                    star.settle(50);
                    close_refused(&mut star, &mut order);
                    star.settle(50);
                    let (seen, seats) = star.agreed();
                    assert_eq!(seen, Some(host), "draw {draw}");
                    assert!(seats.len() <= MAX_PLAYERS);
                    assert_eq!(seats.first(), Some(&host));
                }
            }
            star.settle(100);
            close_refused(&mut star, &mut order);
            star.settle(100);
            let (seen, seats) = star.agreed();
            assert_eq!(seen, Some(host), "draw {draw}");
            // The host's order of arrival, the departed and the refused aside.
            assert_eq!(seats, order, "draw {draw}");
            for lobby in &star.lobbies {
                assert!(lobby.seated(), "draw {draw}: {:?} not seated", lobby.me());
                let me = lobby.me();
                let slot = |l: &Lobby| l.seats().iter().position(|k| *k == me);
                for other in &star.lobbies {
                    assert_eq!(slot(other), slot(lobby), "draw {draw}");
                }
            }
        }
    }

    /// Issue #63: the host with 11 clients, then the room is full; each client is linked with
    /// the host only (the star checks every message), knows every other player from the host
    /// (name and color), and sees the same list.
    #[test]
    fn twelve_players_know_each_other_through_the_host() {
        let mut star = Star::new();
        let host = star.create(77);
        for n in 1..MAX_PLAYERS as u128 {
            star.join(n);
        }
        star.run(1_500);
        let (seen, seats) = star.agreed();
        assert_eq!((seen, seats.len()), (Some(host), MAX_PLAYERS));
        for lobby in &star.lobbies {
            let rows = lobby.players(star.local(lobby.me()));
            assert_eq!(rows.len(), MAX_PLAYERS);
            for row in &rows {
                assert!(row.seated);
                assert_eq!(row.name, format!("Joueur {}", row.key.0 >> 64));
                assert!(row.color.is_some());
                // Linked with the host only, or (the host) with everyone.
                let expected = if row.me {
                    RowLink::Me
                } else if lobby.is_host() || row.key == host {
                    RowLink::Direct
                } else {
                    RowLink::ViaHost
                };
                assert_eq!(
                    row.link,
                    expected,
                    "{:?} in {:?}'s list",
                    row.key,
                    lobby.me()
                );
            }
            let colors: Vec<_> = seats.iter().map(|s| lobby.color_of(*s)).collect();
            assert_eq!(
                colors,
                seats
                    .iter()
                    .map(|s| star.view(host).color_of(*s))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(star.links.len(), MAX_PLAYERS - 1);
    }
}
