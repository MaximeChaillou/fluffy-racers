//! The lobby of a room (issue #7, `docs/ARCHITECTURE.md` § « Réseau »): who is there, who is
//! ready, who hosts, the ping of each one. Pure logic: no Bevy, no socket. It reads the
//! [`Message`]s that arrive and the peers that come and go, and writes the messages to send
//! ([`Lobby::drain_outbox`]); the tests connect several lobbies to each other in memory.
//!
//! **Seats.** A room holds [`MAX_PLAYERS`] players. Every peer the signaling server puts in the
//! room connects to all the others (full mesh), so a newcomer is only a player once it has a
//! seat:
//! - the first peers of an empty room sit down by themselves, [`SETTLE_MS`] after joining, when
//!   no peer they met is seated (two friends who open the link at the same moment both do, and
//!   merge);
//! - after that, the **host** seats the newcomers in their arrival order, and sends the list of
//!   seats to everyone ([`Message::Seats`]); a newcomer beyond 12 gets [`Message::RoomFull`] and
//!   leaves, with a message.
//!
//! **Host**: the seated player with the smallest [`PeerKey`] (`PeerId`), as the architecture
//! says. If it leaves, the next one takes over at once, with nothing to exchange: every peer
//! computes the same. A newcomer never takes over before it is seated.
//!
//! **Arrivals and departures**: a peer that connects gets our [`Message::Hello`]; one that says
//! [`Message::Bye`] or that the socket reports gone leaves the lobby (and its seat). The pings
//! ([`PING_INTERVAL_MS`]) measure each one's latency and clock (`clock`); a peer silent for
//! [`SILENT_MS`] shows as such until the socket gives up on it.
//!
//! A peer of another protocol version is listed apart ([`DecodeError::OtherVersion`]): it never
//! sits, never hosts, and gets nothing more.
//!
//! The race launch ([`Message::StartRace`], issue #9) is only kept here
//! ([`Lobby::announced`]).

use super::clock::PeerClock;
use super::protocol::{DecodeError, MAX_PLAYERS, Message, PeerKey, Profile, RaceStart};

/// A ping to every peer this often (ms).
pub const PING_INTERVAL_MS: u64 = 1_000;

/// A peer alone (or among newcomers) sits down this long after joining the room (ms): enough for
/// the peers already there to connect and say they are seated.
pub const SETTLE_MS: u64 = 2_500;

/// A peer we heard nothing from for this long is shown as silent (ms).
pub const SILENT_MS: u64 = 3_000;

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
    /// A peer left (`Bye`, or gone from the socket).
    Left(PeerKey),
    HostChanged(Option<PeerKey>),
    /// The host refused us: the room is full.
    RoomFull,
    /// A peer runs another version of the protocol.
    OtherVersion(PeerKey, u8),
    /// The host starts the race (issue #9).
    RaceAnnounced(RaceStart),
}

/// Another peer of the room, as we know it.
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
    pub clock: PeerClock,
    pub last_heard_ms: u64,
    next_ping_ms: u64,
}

impl LobbyPeer {
    /// It takes part in the lobby: same version, not refused.
    fn talks(&self) -> bool {
        !self.refused && self.other_version.is_none()
    }
}

/// A line of the lobby's list, for the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerRow {
    pub key: PeerKey,
    pub name: String,
    pub loadout: [u8; 4],
    pub ready: bool,
    pub me: bool,
    pub host: bool,
    pub seated: bool,
    /// Smoothed round trip (ms); `None` for us, or before the first pong.
    pub ping_ms: Option<f32>,
    /// Nothing heard from it for [`SILENT_MS`].
    pub silent: bool,
    pub other_version: Option<u8>,
}

/// The lobby, seen from one peer (see the module's documentation).
#[derive(Clone, Debug)]
pub struct Lobby {
    me: PeerKey,
    /// Ours: name, loadout, ready, seated.
    profile: Profile,
    joined_ms: u64,
    /// The other peers, in the order they connected.
    peers: Vec<LobbyPeer>,
    /// Seated players (us included), in seat order: the host's, as last received.
    seats: Vec<PeerKey>,
    host: Option<PeerKey>,
    /// The seats changed since we, the host, last sent them.
    seats_dirty: bool,
    full: bool,
    left: bool,
    announced: Option<RaceStart>,
    outbox: Vec<Outgoing>,
    events: Vec<LobbyEvent>,
}

impl Lobby {
    /// We just joined the room as `me` (the id the signaling server gave us), at `now_ms`.
    pub fn new(me: PeerKey, mut profile: Profile, now_ms: u64) -> Self {
        profile.name = Profile::clean_name(&profile.name);
        profile.seated = false;
        Self {
            me,
            profile,
            joined_ms: now_ms,
            peers: Vec::new(),
            seats: Vec::new(),
            host: None,
            seats_dirty: false,
            full: false,
            left: false,
            announced: None,
            outbox: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn me(&self) -> PeerKey {
        self.me
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The host (see the module's documentation); `None` before anyone is seated.
    pub fn host(&self) -> Option<PeerKey> {
        self.host
    }

    pub fn is_host(&self) -> bool {
        self.host == Some(self.me)
    }

    /// We have a seat.
    pub fn seated(&self) -> bool {
        self.profile.seated
    }

    /// The host refused us: the room is full.
    pub fn full(&self) -> bool {
        self.full
    }

    /// Seated players, us included, in seat order.
    pub fn seats(&self) -> &[PeerKey] {
        &self.seats
    }

    pub fn peers(&self) -> &[LobbyPeer] {
        &self.peers
    }

    pub fn peer(&self, key: PeerKey) -> Option<&LobbyPeer> {
        self.peers.iter().find(|peer| peer.key == key)
    }

    /// The race the host announced (issue #9).
    pub fn announced(&self) -> Option<RaceStart> {
        self.announced
    }

    /// At least two players, all of them ready.
    pub fn all_ready(&self) -> bool {
        self.seats.len() >= 2
            && self.profile.ready
            && self
                .peers
                .iter()
                .filter(|peer| peer.seated && peer.talks())
                .all(|peer| peer.profile.as_ref().is_some_and(|p| p.ready))
    }

    /// The host's clock at our `now_ms` (ms since the host joined... its own start): the common
    /// clock of the room. `None` until a pong of the host arrived.
    pub fn host_time_ms(&self, now_ms: u64) -> Option<u64> {
        let host = self.host?;
        if host == self.me {
            return Some(now_ms);
        }
        self.peer(host)?.clock.their_time_ms(now_ms)
    }

    /// The messages to send, oldest first.
    pub fn drain_outbox(&mut self) -> std::vec::Drain<'_, Outgoing> {
        self.outbox.drain(..)
    }

    /// What happened since the last call.
    pub fn drain_events(&mut self) -> std::vec::Drain<'_, LobbyEvent> {
        self.events.drain(..)
    }

    /// The socket connected us to `peer`. Unknown peers only: a second report changes nothing.
    pub fn connect(&mut self, peer: PeerKey, now_ms: u64) {
        if peer == self.me || self.peer(peer).is_some() || self.stopped() {
            return;
        }
        self.peers.push(LobbyPeer {
            key: peer,
            profile: None,
            seated: false,
            refused: false,
            other_version: None,
            clock: PeerClock::default(),
            last_heard_ms: now_ms,
            next_ping_ms: now_ms,
        });
        self.events.push(LobbyEvent::Connected(peer));
        self.send(peer, Message::Hello(self.profile.clone()));
        self.refresh();
    }

    /// `peer` is gone (the socket says so, or it said `Bye`).
    pub fn disconnect(&mut self, peer: PeerKey) {
        let Some(index) = self.peers.iter().position(|p| p.key == peer) else {
            return;
        };
        self.peers.remove(index);
        if let Some(seat) = self.seats.iter().position(|key| *key == peer) {
            self.seats.remove(seat);
            self.seats_dirty = true;
        }
        self.events.push(LobbyEvent::Left(peer));
        self.refresh();
    }

    /// A message from `from` arrived at `now_ms`.
    pub fn receive(&mut self, from: PeerKey, message: Message, now_ms: u64) {
        if self.stopped() {
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
                let sits = profile.seated && !peer.seated;
                peer.profile = Some(profile);
                if sits {
                    self.sit(from);
                }
            }
            Message::Ready(ready) => {
                if let Some(profile) = &mut peer.profile {
                    profile.ready = ready;
                }
            }
            Message::Seats(list) => self.receive_seats(from, &list),
            Message::RoomFull => {
                // From a seated player (the host, as it sees it): we leave.
                if peer.seated {
                    self.full = true;
                    self.events.push(LobbyEvent::RoomFull);
                }
            }
            Message::Bye => {
                self.disconnect(from);
                return;
            }
            Message::StartRace(start) => {
                if self.host == Some(from) {
                    self.announced = Some(start);
                    self.events.push(LobbyEvent::RaceAnnounced(start));
                }
            }
            // Issue #9 routes the interactions between karts; nothing to do in the lobby.
            Message::Strike(_)
            | Message::StrikeReply(_)
            | Message::Grab(_)
            | Message::TetherBreak(_) => {}
            Message::Ping { sent_ms } => {
                let pong = Message::Pong {
                    echo_ms: sent_ms,
                    clock_ms: now_ms as u32,
                };
                self.send(from, pong);
            }
            Message::Pong { echo_ms, clock_ms } => {
                peer.clock.sample(now_ms, echo_ms, clock_ms);
            }
        }
        self.refresh();
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
            peer.seated = false;
            self.seats.retain(|key| *key != from);
            self.events.push(LobbyEvent::OtherVersion(from, version));
            self.refresh();
        }
    }

    /// Time passes: the pings, and sitting down in an empty room.
    pub fn update(&mut self, now_ms: u64) {
        if self.stopped() {
            return;
        }
        for index in 0..self.peers.len() {
            let peer = &mut self.peers[index];
            if peer.talks() && now_ms >= peer.next_ping_ms {
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
        let alone = !self.peers.iter().any(|p| p.seated && p.talks());
        let everyone_said_hello = self.peers.iter().all(|p| p.profile.is_some() || !p.talks());
        if !self.profile.seated
            && alone
            && everyone_said_hello
            && now_ms >= self.joined_ms + SETTLE_MS
        {
            self.sit(self.me);
        }
        self.refresh();
    }

    /// « Prêt » or not.
    pub fn set_ready(&mut self, ready: bool) {
        if self.profile.ready != ready && !self.stopped() {
            self.profile.ready = ready;
            self.broadcast(Message::Ready(ready));
        }
    }

    /// Our name and loadout changed (selection screen).
    pub fn set_profile(&mut self, name: &str, loadout: [u8; 4]) {
        let name = Profile::clean_name(name);
        if (self.profile.name != name || self.profile.loadout != loadout) && !self.stopped() {
            self.profile.name = name;
            self.profile.loadout = loadout;
            self.broadcast(Message::Hello(self.profile.clone()));
        }
    }

    /// We leave: `Bye` to everyone, then nothing more.
    pub fn leave(&mut self) {
        if !self.stopped() {
            self.broadcast(Message::Bye);
            self.left = true;
        }
    }

    /// The host starts the race (issue #9): only the host may.
    pub fn announce_race(&mut self, start: RaceStart) -> bool {
        if !self.is_host() || self.stopped() {
            return false;
        }
        self.announced = Some(start);
        self.broadcast(Message::StartRace(start));
        true
    }

    /// The lobby's list: seated players in seat order, then the peers waiting for a seat, then
    /// those of another version.
    pub fn players(&self, now_ms: u64) -> Vec<PlayerRow> {
        let mut rows: Vec<PlayerRow> = Vec::with_capacity(self.peers.len() + 1);
        let row_of_peer = |peer: &LobbyPeer| {
            let profile = peer.profile.clone().unwrap_or_default();
            PlayerRow {
                key: peer.key,
                name: profile.name,
                loadout: profile.loadout,
                ready: profile.ready,
                me: false,
                host: self.host == Some(peer.key),
                seated: peer.seated,
                ping_ms: peer.clock.rtt_ms(),
                silent: now_ms.saturating_sub(peer.last_heard_ms) >= SILENT_MS,
                other_version: peer.other_version,
            }
        };
        let my_row = PlayerRow {
            key: self.me,
            name: self.profile.name.clone(),
            loadout: self.profile.loadout,
            ready: self.profile.ready,
            me: true,
            host: self.is_host(),
            seated: self.profile.seated,
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

    /// Nothing more to do: refused, or gone.
    fn stopped(&self) -> bool {
        self.full || self.left
    }

    fn send(&mut self, to: PeerKey, message: Message) {
        self.outbox.push(Outgoing { to, message });
    }

    /// To every peer that takes part.
    fn broadcast(&mut self, message: Message) {
        for peer in &self.peers {
            if peer.talks() {
                self.outbox.push(Outgoing {
                    to: peer.key,
                    message: message.clone(),
                });
            }
        }
    }

    /// `key` (us or a peer) gets a seat.
    fn sit(&mut self, key: PeerKey) {
        if key == self.me {
            if self.profile.seated {
                return;
            }
            self.profile.seated = true;
            // The others learn it from our Hello (and the host's list).
            self.broadcast(Message::Hello(self.profile.clone()));
        } else {
            let Some(peer) = self.peers.iter_mut().find(|p| p.key == key) else {
                return;
            };
            if peer.seated || !peer.talks() {
                return;
            }
            peer.seated = true;
        }
        if !self.seats.contains(&key) {
            self.seats.push(key);
        }
        self.seats_dirty = true;
        self.events.push(LobbyEvent::Seated(key));
    }

    /// The seats as `from` sees them: every one of them is seated. The host's list also gives
    /// the order (seats we know of that it does not list yet stay at the end).
    fn receive_seats(&mut self, from: PeerKey, list: &[PeerKey]) {
        for key in list {
            self.sit(*key);
        }
        if self.host == Some(from) {
            let mut ordered: Vec<PeerKey> = list
                .iter()
                .copied()
                .filter(|key| self.seats.contains(key))
                .collect();
            for key in &self.seats {
                if !ordered.contains(key) {
                    ordered.push(*key);
                }
            }
            self.seats = ordered;
            // What the host sent is what everyone has now.
            self.seats_dirty = false;
        }
    }

    /// After every change: the host, and what the host does (seats newcomers, refuses those
    /// beyond 12, sends the seats).
    fn refresh(&mut self) {
        self.elect_host();
        if !self.is_host() || self.stopped() {
            return;
        }
        // Too many seated (newcomers who sat down by themselves at the same time): the last
        // ones in seat order leave.
        while self.seats.len() > MAX_PLAYERS {
            let Some(index) = self.seats.iter().rposition(|key| *key != self.me) else {
                break;
            };
            let key = self.seats.remove(index);
            self.refuse(key);
        }
        // The newcomers, in arrival order.
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
            self.broadcast(Message::Seats(self.seats.clone()));
        }
        // A newcomer we just seated may have a smaller id: the room is its now (it learns it from
        // the seats we just sent).
        self.elect_host();
    }

    /// The host: the seated player with the smallest id.
    fn elect_host(&mut self) {
        let me = self.profile.seated.then_some(self.me);
        let host = self
            .peers
            .iter()
            .filter(|p| p.seated && p.talks())
            .map(|p| p.key)
            .chain(me)
            .min();
        if host != self.host {
            self.host = host;
            self.events.push(LobbyEvent::HostChanged(host));
            // A new host sends its list.
            self.seats_dirty = true;
        }
    }

    /// We, the host, refuse `key`: the room is full.
    fn refuse(&mut self, key: PeerKey) {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.key == key) {
            peer.seated = false;
            peer.refused = true;
        }
        self.send(key, Message::RoomFull);
        self.seats_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::Channel;

    /// Several lobbies wired to each other in memory, like the full mesh of the socket. Every
    /// message goes through its bytes (`encode` then `decode`), with a delay: the order of the
    /// reliable channel is kept, the unreliable one can lose packets.
    struct Mesh {
        lobbies: Vec<Lobby>,
        /// How far ahead of the mesh's time each lobby's clock is (it counts from its own
        /// start): (peer, ms).
        skews: Vec<(PeerKey, u64)>,
        /// Who is connected to whom (both ways).
        links: Vec<(PeerKey, PeerKey)>,
        /// (arrival time, to, from, bytes).
        in_flight: Vec<(u64, PeerKey, PeerKey, Vec<u8>)>,
        now: u64,
        /// One way (ms).
        delay: u64,
        drop_unreliable: bool,
    }

    impl Mesh {
        fn new() -> Self {
            Self {
                lobbies: Vec::new(),
                skews: Vec::new(),
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

        /// A peer joins the room: the socket connects it to everyone there.
        fn join(&mut self, n: u128) -> PeerKey {
            let key = Self::key(n);
            let profile = Self::profile(&format!("Joueur {n}"));
            self.add(Lobby::new(key, profile, 0));
            let others: Vec<PeerKey> = self.lobbies.iter().map(Lobby::me).collect();
            for other in others {
                if other != key {
                    self.link(key, other);
                }
            }
            key
        }

        /// A lobby whose clock starts now (`Lobby::new` with 0): it runs `now` behind the mesh.
        fn add(&mut self, lobby: Lobby) {
            // Unsigned clocks: a lobby is ahead of the mesh by a large margin minus its start.
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

        /// A peer's tab closes: the others' sockets report it gone.
        fn close(&mut self, key: PeerKey) {
            self.lobbies.retain(|lobby| lobby.me() != key);
            self.links.retain(|(a, b)| *a != key && *b != key);
            self.in_flight
                .retain(|(_, to, from, _)| *to != key && *from != key);
            for lobby in &mut self.lobbies {
                lobby.disconnect(key);
            }
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
                self.check_hosts();
                self.flush();
            }
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
                    "{from:?} sent to {:?}, not connected",
                    out.to
                );
                if self.drop_unreliable && out.message.channel() == Channel::Unreliable {
                    continue;
                }
                let at = self.now + self.delay;
                self.in_flight
                    .push((at, out.to, from, out.message.to_bytes()));
            }
        }

        /// At every moment: a lobby that sees a seated player smaller than its host is wrong
        /// (the host is the smallest seated id, whatever the others know yet).
        fn check_hosts(&self) {
            for lobby in &self.lobbies {
                if let Some(host) = lobby.host() {
                    for seat in lobby.seats() {
                        assert!(*seat >= host, "{:?} hosts with {seat:?} seated", host);
                    }
                }
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

    /// Issue #7: two players find each other, see each other with their ping, and the smallest
    /// id hosts.
    #[test]
    fn two_peers_see_each_other_with_their_ping_and_the_smallest_hosts() {
        let mut mesh = Mesh::new();
        let b = mesh.join(7);
        mesh.run(SETTLE_MS + 100);
        assert!(mesh.view(b).seated(), "alone, it sits down");
        assert!(mesh.view(b).is_host());
        let a = mesh.join(3);
        mesh.run(2_000);
        let (host, seats) = mesh.agreed();
        assert_eq!(seats, [b, a], "arrival order");
        assert_eq!(host, Some(a), "the smallest id hosts once seated");
        for (me, other) in [(a, b), (b, a)] {
            let rows = mesh.view(me).players(mesh.local(me));
            assert_eq!(rows.len(), 2);
            let row = rows.iter().find(|r| r.key == other).unwrap();
            assert_eq!(row.name, format!("Joueur {}", other.0 >> 64));
            assert_eq!(row.loadout, [1, 2, 0, 1]);
            assert!(!row.silent);
            let ping = row.ping_ms.expect("a ping");
            assert!((ping - 40.0).abs() < 2.0, "{ping}");
            assert!(rows.iter().any(|r| r.me && r.key == me));
            assert_eq!(rows.iter().filter(|r| r.host).count(), 1);
        }
    }

    /// « Prêt » reaches the others; everyone ready shows.
    #[test]
    fn ready_goes_to_everyone() {
        let mut mesh = Mesh::new();
        let a = mesh.join(1);
        mesh.run(SETTLE_MS + 100);
        let b = mesh.join(2);
        let c = mesh.join(3);
        mesh.run(1_000);
        mesh.lobby(b).set_ready(true);
        mesh.run(100);
        for key in [a, c] {
            let rows = mesh.view(key).players(mesh.local(key));
            assert!(rows.iter().find(|r| r.key == b).unwrap().ready);
            assert!(!rows.iter().find(|r| r.key == a).unwrap().ready);
            assert!(!mesh.view(key).all_ready());
        }
        mesh.lobby(a).set_ready(true);
        mesh.lobby(c).set_ready(true);
        mesh.run(100);
        for key in [a, b, c] {
            assert!(mesh.view(key).all_ready(), "{key:?}");
        }
        mesh.lobby(b).set_ready(false);
        mesh.run(100);
        assert!(!mesh.view(a).all_ready());
        // A changed name or kart reaches them too.
        mesh.lobby(c).set_profile("  Max ", [4, 1, 1, 0]);
        mesh.run(100);
        let rows = mesh.view(a).players(mesh.local(a));
        let row = rows.iter().find(|r| r.key == c).unwrap();
        assert_eq!((row.name.as_str(), row.loadout), ("Max", [4, 1, 1, 0]));
    }

    /// A player who leaves disappears from every lobby; if it hosted, the next smallest id
    /// takes over, and its seat order is kept.
    #[test]
    fn departures_are_seen_and_the_host_hands_over() {
        let mut mesh = Mesh::new();
        let first = mesh.join(5);
        mesh.run(SETTLE_MS + 100);
        let keys: Vec<PeerKey> = [2, 9, 4].into_iter().map(|n| mesh.join(n)).collect();
        mesh.run(1_000);
        let (host, seats) = mesh.agreed();
        assert_eq!(host, Some(keys[0]));
        assert_eq!(seats, [first, keys[0], keys[1], keys[2]]);
        // The host's tab closes.
        mesh.close(keys[0]);
        mesh.run(100);
        let (host, seats) = mesh.agreed();
        assert_eq!(host, Some(keys[2]), "the next smallest");
        assert_eq!(seats, [first, keys[1], keys[2]]);
        // Another one says goodbye.
        mesh.lobby(keys[1]).leave();
        mesh.run(100);
        mesh.lobbies.retain(|l| l.me() != keys[1]);
        let (host, seats) = mesh.agreed();
        assert_eq!(host, Some(keys[2]));
        assert_eq!(seats, [first, keys[2]]);
        let events: Vec<LobbyEvent> = mesh.lobby(first).drain_events().collect();
        assert!(events.contains(&LobbyEvent::Left(keys[0])));
        assert!(events.contains(&LobbyEvent::Left(keys[1])));
        assert!(events.contains(&LobbyEvent::HostChanged(Some(keys[2]))));
        // Nothing is sent to the departed (the mesh would panic).
        mesh.run(3_000);
    }

    /// Issue #7: a 13th player is refused with a message, the 12 keep playing.
    #[test]
    fn a_full_room_refuses_the_thirteenth() {
        let mut mesh = Mesh::new();
        let first = mesh.join(500);
        mesh.run(SETTLE_MS + 100);
        for n in 1..MAX_PLAYERS as u128 {
            mesh.join(n * 10);
            mesh.run(200);
        }
        mesh.run(1_000);
        let (_, seats) = mesh.agreed();
        assert_eq!(seats.len(), MAX_PLAYERS);
        // The newcomer has the smallest id: it still cannot take over.
        let late = mesh.join(1);
        mesh.run(1_000);
        assert!(mesh.view(late).full(), "refused");
        assert!(!mesh.view(late).seated());
        assert!(
            mesh.lobby(late)
                .drain_events()
                .any(|e| e == LobbyEvent::RoomFull),
            "with a message"
        );
        // Its socket closes.
        mesh.close(late);
        mesh.run(100);
        let (host, seats) = mesh.agreed();
        assert_eq!(seats.len(), MAX_PLAYERS);
        assert!(!seats.contains(&late));
        assert_eq!(host, Some(Mesh::key(10)));
        assert!(seats.contains(&first));
        // A seat frees up: the next newcomer gets it.
        mesh.close(Mesh::key(50));
        let next = mesh.join(2);
        mesh.run(1_000);
        assert!(mesh.view(next).seated());
        assert!(
            mesh.view(next).is_host(),
            "seated last, but the smallest id: it hosts"
        );
    }

    /// Two friends open the link at the same moment, in an empty room: both sit down, and agree.
    #[test]
    fn two_newcomers_at_once_both_sit_and_agree() {
        let mut mesh = Mesh::new();
        let a = mesh.join(8);
        let b = mesh.join(6);
        mesh.run(SETTLE_MS + 500);
        let (host, seats) = mesh.agreed();
        assert_eq!(host, Some(b));
        assert_eq!(seats.len(), 2);
        assert!(mesh.view(a).seated() && mesh.view(b).seated());
        // And when they connect only after both sat down (a slow network): the same.
        let mut slow = Mesh::new();
        slow.add(Lobby::new(Mesh::key(8), Mesh::profile("A"), 0));
        slow.add(Lobby::new(Mesh::key(6), Mesh::profile("B"), 0));
        slow.run(SETTLE_MS + 100);
        assert!(slow.lobbies.iter().all(Lobby::seated));
        slow.link(Mesh::key(8), Mesh::key(6));
        slow.run(500);
        let (host, seats) = slow.agreed();
        assert_eq!(host, Some(Mesh::key(6)));
        assert_eq!(seats.len(), 2);
    }

    /// A newcomer waits for its seat: it never hosts before, even with the smallest id.
    #[test]
    fn a_newcomer_waits_for_the_host_to_seat_it() {
        let mut mesh = Mesh::new();
        let host = mesh.join(9);
        mesh.run(SETTLE_MS + 100);
        mesh.delay = 300;
        let late = mesh.join(1);
        mesh.run(100);
        assert!(!mesh.view(late).seated());
        assert_eq!(mesh.view(late).host(), None);
        mesh.run(400);
        assert_eq!(mesh.view(late).host(), Some(host), "it heard the host");
        assert!(!mesh.view(late).seated());
        mesh.run(1_000);
        assert!(mesh.view(late).seated());
        assert_eq!(mesh.agreed().0, Some(late));
    }

    /// The host that seats a newcomer with a smaller id hands it the room at once: no moment
    /// where two peers both think they host (seen with two native peers, issue #7).
    #[test]
    fn seating_a_smaller_id_hands_over_at_once() {
        let (big, small) = (PeerKey(9), PeerKey(2));
        let mut lobby = Lobby::new(big, Mesh::profile("Grand"), 0);
        lobby.connect(small, 0);
        lobby.receive(small, Message::Hello(Mesh::profile("Petit")), 10);
        assert!(!lobby.seated());
        // Alone seated: it sits, hosts, seats the newcomer, and is no longer the host.
        lobby.update(SETTLE_MS);
        assert!(lobby.seated());
        assert_eq!(lobby.seats(), [big, small]);
        assert_eq!(lobby.host(), Some(small));
        assert!(!lobby.is_host());
        // The newcomer got the seats.
        let sent: Vec<Outgoing> = lobby.drain_outbox().collect();
        assert!(sent.contains(&Outgoing {
            to: small,
            message: Message::Seats(vec![big, small]),
        }));
    }

    /// The common clock: every peer reads the host's time, within a few ms.
    #[test]
    fn every_peer_reads_the_host_clock() {
        let mut mesh = Mesh::new();
        mesh.join(4);
        mesh.run(1_234);
        mesh.join(1);
        mesh.join(7);
        mesh.run(SETTLE_MS + 3_000);
        let (host, _) = mesh.agreed();
        let host = host.unwrap();
        // Each lobby counts from its own start: their clocks differ by 1 234 ms.
        let reference = mesh.local(host);
        assert_eq!(mesh.view(host).host_time_ms(reference), Some(reference));
        let mut differ = false;
        for lobby in &mesh.lobbies {
            let local = mesh.local(lobby.me());
            differ |= local.abs_diff(reference) > 1_000;
            let read = lobby.host_time_ms(local).unwrap();
            assert!(read.abs_diff(reference) <= 2, "{read} vs {reference}");
        }
        assert!(differ);
    }

    /// A peer of another version is listed apart, never seated, never sent anything more.
    #[test]
    fn another_version_is_listed_apart() {
        let mut lobby = Lobby::new(PeerKey(5), Mesh::profile("Moi"), 0);
        lobby.update(SETTLE_MS);
        assert!(lobby.is_host());
        let stranger = PeerKey(1);
        lobby.connect(stranger, SETTLE_MS);
        lobby.drain_outbox();
        lobby.receive_error(stranger, DecodeError::OtherVersion(2), SETTLE_MS + 10);
        lobby.receive_error(stranger, DecodeError::Truncated, SETTLE_MS + 11);
        lobby.update(SETTLE_MS + 5_000);
        assert!(lobby.drain_outbox().all(|out| out.to != stranger));
        assert!(lobby.is_host());
        assert_eq!(lobby.seats(), [PeerKey(5)]);
        let rows = lobby.players(SETTLE_MS + 5_000);
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
            SETTLE_MS + 5_001,
        );
        assert!(lobby.is_host());
    }

    /// Messages that make no sense from where they come are ignored: a race started by someone
    /// else than the host, a room full sent by a newcomer, anything from an unknown peer.
    #[test]
    fn only_the_host_starts_a_race_and_strangers_are_ignored() {
        let mut mesh = Mesh::new();
        let a = mesh.join(1);
        mesh.run(SETTLE_MS + 100);
        let b = mesh.join(2);
        mesh.run(500);
        let start = RaceStart {
            seed: 42,
            track: 0,
            laps: 3,
            bots: 10,
            countdown_at_ms: 9_000,
        };
        assert!(!mesh.lobby(b).announce_race(start));
        let (now_a, now_b) = (mesh.local(a), mesh.local(b));
        mesh.lobby(b).receive(a, Message::StartRace(start), now_b);
        assert_eq!(mesh.view(b).announced(), Some(start));
        mesh.lobby(a).receive(b, Message::StartRace(start), now_a);
        assert_eq!(mesh.view(a).announced(), None);
        assert!(mesh.lobby(a).announce_race(start));
        mesh.lobby(a).receive(PeerKey(99), Message::Bye, now_a);
        mesh.lobby(a).receive(PeerKey(99), Message::RoomFull, now_a);
        assert!(!mesh.view(a).full());
        assert_eq!(mesh.view(a).seats().len(), 2);
    }

    /// Lost pings (unreliable channel) only leave the ping unknown; the lobby still works, and a
    /// peer silent for 3 s shows as such.
    #[test]
    fn lost_pings_do_not_break_the_lobby() {
        let mut mesh = Mesh::new();
        mesh.drop_unreliable = true;
        let a = mesh.join(1);
        mesh.run(SETTLE_MS + 100);
        let b = mesh.join(2);
        mesh.run(1_000);
        let rows = mesh.view(a).players(mesh.local(a));
        let row = rows.iter().find(|r| r.key == b).unwrap();
        assert!(row.seated);
        assert_eq!(row.ping_ms, None);
        mesh.run(SILENT_MS);
        let rows = mesh.view(a).players(mesh.local(a));
        assert!(rows.iter().find(|r| r.key == b).unwrap().silent);
    }
}
