//! The WebRTC socket (issue #7): `matchbox_socket`, used directly (`bevy_matchbox` does not
//! support Bevy 0.20). Two data channels ([`Channel`]): reliable, then unreliable.
//!
//! The socket's message loop is a future: it runs on Bevy's I/O task pool, natively (its own
//! threads) as in the browser (the page's event loop, where the pool spawns local tasks). The
//! game polls the socket from a system each frame ([`NetSocket::poll`]): peers that come and go,
//! packets received, packets to send. Nothing here waits (the lock of the signaling's notes is
//! only ever held to push one).
//!
//! Its ICE servers are chosen by the game (`ice.rs`, issue #49), and its signaling goes through
//! [`ObservedSignallerBuilder`] (`signaling.rs`): what the signaling server says comes out of
//! [`NetSocket::poll`] too ([`SocketEvent::Signal`]), before the peers it concerns are connected.
//! Since issue #63, it only lets the links of the star open: a host's with each client, a
//! client's with its host (`signaling::SignalFilter`).
//!
//! Two traps of `matchbox_socket` 0.14, avoided here:
//! - its message loop **panics** when asked to send to a peer whose data channels are not open
//!   yet: only peers it reported connected get packets;
//! - a packet can arrive before the peer is reported connected: it waits ([`EARLY_PACKETS`] at
//!   most) until it is.

use std::sync::{Arc, PoisonError};

use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use matchbox_socket::{
    ChannelConfig, PeerId, PeerState, RtcIceServerConfig, SignallerBuilder, WebRtcSocket,
    WebRtcSocketBuilder,
};

use super::ice::IceServers;
use super::protocol::{Channel, Message, PeerKey};
use super::signaling::{Notes, ObservedSignallerBuilder, SignalNote, WebSocketSignallerBuilder};

/// Packets kept from peers not reported connected yet.
pub const EARLY_PACKETS: usize = 64;

/// What the socket reported this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum SocketEvent {
    /// The signaling server gave us our id.
    Joined(PeerKey),
    Connected(PeerKey),
    Disconnected(PeerKey),
    /// A packet from a connected peer, on a channel.
    Packet(PeerKey, Channel, Box<[u8]>),
    /// The message loop stopped: the signaling server could not be reached, or dropped us.
    Closed(Result<(), String>),
    /// Something went through the signaling server (a peer announced, an offer...).
    Signal(SignalNote),
}

/// A socket in a room, and its message loop.
pub struct NetSocket {
    /// The tests' in-memory room (`memory`): everything else is unused then.
    #[cfg(test)]
    memory: Option<memory::Port>,
    socket: WebRtcSocket,
    task: Option<Task<Result<(), String>>>,
    me: Option<PeerKey>,
    /// Peers reported connected, and their ids (to send to them).
    connected: Vec<(PeerKey, PeerId)>,
    early: Vec<(PeerId, Channel, Box<[u8]>)>,
    /// Reused for every packet written.
    buffer: Vec<u8>,
    /// Written by the message loop's signaling, read by [`NetSocket::poll`].
    notes: Notes,
    /// Swapped with `notes` to read them without holding the lock.
    read: Vec<SignalNote>,
}

impl NetSocket {
    /// Joins the room at `room_url` (`room::room_url`) through the signaling server's WebSocket.
    /// `attempts`: how many times to try the signaling server before giving up (3 s apart).
    /// `ice`: the STUN and TURN servers (none at all: only the host candidates). `hosting`: we
    /// host the room (issue #63: we created it), or join it as a client.
    pub fn open(room_url: &str, attempts: u16, ice: &IceServers, hosting: bool) -> Self {
        Self::open_with(
            room_url,
            attempts,
            ice,
            Arc::new(WebSocketSignallerBuilder),
            hosting,
        )
    }

    /// The same, with another way to reach the signaling server (the tests).
    pub fn open_with(
        room_url: &str,
        attempts: u16,
        ice: &IceServers,
        signaller: Arc<dyn SignallerBuilder>,
        hosting: bool,
    ) -> Self {
        let notes = Notes::default();
        let (socket, message_loop) = WebRtcSocketBuilder::new(room_url)
            .add_channel(ChannelConfig::reliable())
            .add_channel(ChannelConfig::unreliable())
            .reconnect_attempts(Some(attempts.max(1)))
            .ice_server(RtcIceServerConfig {
                // Empty: natively, webrtc-rs takes it as no server; in the browser, the page's
                // shim drops the server entry without URL (index.html, issue #49).
                urls: ice.urls.clone(),
                username: ice.username.clone(),
                credential: ice.credential.clone(),
            })
            .signaller_builder(Arc::new(ObservedSignallerBuilder::new(
                signaller,
                notes.clone(),
                hosting,
            )))
            .build();
        let task = IoTaskPool::get()
            .spawn(async move { message_loop.await.map_err(|error| error.to_string()) });
        Self {
            #[cfg(test)]
            memory: None,
            socket,
            task: Some(task),
            me: None,
            connected: Vec::new(),
            early: Vec::new(),
            buffer: Vec::new(),
            notes,
            read: Vec::new(),
        }
    }

    /// Our id, once the signaling server gave it.
    pub fn me(&self) -> Option<PeerKey> {
        self.me
    }

    /// What happened since the last call, in order: our id, the signaling server's news, peers,
    /// packets, the end of the loop. The news about a peer always comes before it is connected.
    pub fn poll(&mut self, mut on: impl FnMut(SocketEvent)) {
        #[cfg(test)]
        if let Some(port) = self.memory.as_mut() {
            port.poll(&mut on);
            return;
        }
        if self.task.is_none() {
            return;
        }
        if self.me.is_none()
            && let Some(id) = self.socket.id()
        {
            let me = key_of(id);
            self.me = Some(me);
            on(SocketEvent::Joined(me));
        }
        // The peers that came and went, then the notes: every note written before one of these
        // changes (by the message loop, natively on another thread) is read now, and told first
        // (issue #51). The loop only holds the lock to push a note.
        let changes = self.socket.try_update_peers();
        {
            let mut notes = self.notes.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::swap(&mut *notes, &mut self.read);
        }
        for note in self.read.drain(..) {
            on(SocketEvent::Signal(note));
        }
        if let Ok(changes) = changes {
            for (id, state) in changes {
                let key = key_of(id);
                match state {
                    PeerState::Connected => {
                        if !self.connected.iter().any(|(k, _)| *k == key) {
                            self.connected.push((key, id));
                        }
                        on(SocketEvent::Connected(key));
                        // Its packets that came first.
                        let mut index = 0;
                        while index < self.early.len() {
                            if self.early[index].0 == id {
                                let (_, channel, packet) = self.early.remove(index);
                                on(SocketEvent::Packet(key, channel, packet));
                            } else {
                                index += 1;
                            }
                        }
                    }
                    PeerState::Disconnected => {
                        self.connected.retain(|(k, _)| *k != key);
                        self.early.retain(|(early, _, _)| *early != id);
                        on(SocketEvent::Disconnected(key));
                    }
                }
            }
        }
        for channel in Channel::ALL {
            let Ok(socket_channel) = self.socket.get_channel_mut(channel.index()) else {
                continue;
            };
            for (id, packet) in socket_channel.receive() {
                let key = key_of(id);
                if self.connected.iter().any(|(k, _)| *k == key) {
                    on(SocketEvent::Packet(key, channel, packet));
                } else if self.early.len() < EARLY_PACKETS {
                    self.early.push((id, channel, packet));
                }
            }
        }
        if let Some(task) = self.task.as_mut()
            && let Some(result) = check_ready(task)
        {
            self.task = None;
            self.connected.clear();
            on(SocketEvent::Closed(result));
        }
    }

    /// Whether `peer` is connected (reported so, and not gone since).
    pub fn is_connected(&self, peer: PeerKey) -> bool {
        #[cfg(test)]
        if let Some(port) = self.memory.as_ref() {
            return port.is_connected(peer);
        }
        self.connected.iter().any(|(key, _)| *key == peer)
    }

    /// Sends `message` to `to` on its channel; dropped if `to` is not connected (any more).
    pub fn send(&mut self, to: PeerKey, message: &Message) {
        #[cfg(test)]
        if let Some(port) = self.memory.as_mut() {
            port.send(to, message);
            return;
        }
        let Some(&(_, id)) = self.connected.iter().find(|(key, _)| *key == to) else {
            return;
        };
        message.encode(&mut self.buffer);
        let packet: Box<[u8]> = self.buffer.as_slice().into();
        if let Ok(channel) = self.socket.get_channel_mut(message.channel().index()) {
            // An error only means the loop already stopped: `poll` reports it.
            let _ = channel.try_send(packet, id);
        }
    }

    /// Leaves the room: what was sent on the reliable channel (the `Bye`) still goes out, then
    /// the message loop ends by itself (that channel's queue closed) and the signaling server
    /// tells the others. Keep polling until [`SocketEvent::Closed`] (dropping the socket stops
    /// the loop at once).
    pub fn close(&mut self) {
        #[cfg(test)]
        if let Some(port) = self.memory.as_mut() {
            port.close();
            return;
        }
        if let Ok(channel) = self.socket.get_channel_mut(Channel::Reliable.index()) {
            channel.close();
        }
    }

    /// The message loop stopped.
    pub fn closed(&self) -> bool {
        #[cfg(test)]
        if let Some(port) = self.memory.as_ref() {
            return port.closed();
        }
        self.task.is_none()
    }

    /// The tests' socket in an in-memory room (issue #9): the ports of `hub` connect as the
    /// signaling server and the star would connect them (issue #63: the host with each client
    /// that comes after it), the packets go through their bytes. A WebRTC socket is made too (it
    /// never reaches anything: no signaling server), only for the fields.
    #[cfg(test)]
    pub fn memory(hub: &memory::Hub, me: PeerKey, hosting: bool) -> Self {
        let mut socket = Self::open_with(
            "ws://127.0.0.1:9/memory",
            1,
            &IceServers::choose(&super::ice::StunChoice::Off, None, "ws://127.0.0.1:9"),
            Arc::new(memory::Nowhere),
            hosting,
        );
        socket.memory = Some(hub.port(me, hosting));
        socket
    }
}

/// The number of a peer's id.
pub fn key_of(id: PeerId) -> PeerKey {
    PeerKey(id.0.as_u128())
}

/// The tests' in-memory room (issue #9): a hub every port joins, like the signaling server's room.
/// Since issue #63, the links open as the star opens them: each newcomer is announced to the ports
/// already there, each one's [`SignalFilter`] says whether it offers and whether the newcomer takes
/// the offer (the host with each client that comes after it; two hosts: the second is refused).
/// A packet sent is encoded, and arrives at the next poll of its receiver, `delay` polls later
/// (both channels in order; the unreliable one can lose some). The hub counts what each port sends.
///
/// [`SignalFilter`]: super::super::signaling::SignalFilter
#[cfg(test)]
pub mod memory {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use matchbox_socket::async_trait::async_trait;
    use matchbox_socket::{
        PeerEvent, PeerRequest, PeerSignal, SignalingError, Signaller, SignallerBuilder,
    };

    use super::super::ice::Candidates;
    use super::super::protocol::{Channel, Message, PeerKey};
    use super::super::signaling::{Admit, SignalFilter, SignalNote, test_peer_id};
    use super::SocketEvent;

    /// A port of the room.
    struct Slot {
        key: PeerKey,
        filter: SignalFilter,
        /// What waits for it: (polls left, from, channel, bytes).
        inbox: VecDeque<(u32, PeerKey, Channel, Box<[u8]>)>,
        /// Notes of the signaling for it (the star's), given at its next poll.
        notes: Vec<SignalNote>,
    }

    /// What a port sent, per channel (reliable, unreliable).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Traffic {
        pub bytes: [u64; 2],
        pub packets: [u64; 2],
    }

    #[derive(Default)]
    struct State {
        ports: Vec<Slot>,
        /// The links that opened, (offerer, answerer).
        links: Vec<(PeerKey, PeerKey)>,
        /// Pairs whose link never opens (a handshake that never ends).
        cut: Vec<(PeerKey, PeerKey)>,
        /// Polls a packet takes to arrive.
        delay: u32,
        /// Every this many unreliable packets, one is lost (0: none).
        lose_every: u32,
        unreliable_sent: u32,
        /// Each port gives the signaling notes of a peer before its connection.
        signal_notes: bool,
        traffic: Vec<(PeerKey, Traffic)>,
    }

    impl State {
        fn linked(&self, a: PeerKey, b: PeerKey) -> bool {
            self.links.contains(&(a, b)) || self.links.contains(&(b, a))
        }

        fn blocked(&self, a: PeerKey, b: PeerKey) -> bool {
            self.cut.contains(&(a, b)) || self.cut.contains(&(b, a))
        }
    }

    /// The room (cloned: the same one).
    #[derive(Clone, Default)]
    pub struct Hub(Arc<Mutex<State>>);

    impl Hub {
        /// A packet takes `polls` polls of its receiver to arrive (0: the next one).
        pub fn set_delay(&self, polls: u32) {
            self.0.lock().unwrap().delay = polls;
        }

        /// One unreliable packet in `n` is lost (0: none).
        pub fn lose_every(&self, n: u32) {
            self.0.lock().unwrap().lose_every = n;
        }

        /// Each port also gives what goes through the signaling server before a peer is
        /// connected, as a WebRTC socket does (issue #51): the announcement, the offer and the
        /// answer (without candidates).
        pub fn signal_notes(&self) {
            self.0.lock().unwrap().signal_notes = true;
        }

        /// The link between `a` and `b` never opens (issue #63: a handshake that never ends);
        /// open already, it closes.
        pub fn cut(&self, a: PeerKey, b: PeerKey) {
            let mut state = self.0.lock().unwrap();
            state.cut.push((a, b));
            state
                .links
                .retain(|link| *link != (a, b) && *link != (b, a));
        }

        /// The links that opened: (offerer, answerer).
        pub fn links(&self) -> Vec<(PeerKey, PeerKey)> {
            self.0.lock().unwrap().links.clone()
        }

        /// What `key` sent so far.
        pub fn traffic(&self, key: PeerKey) -> Traffic {
            let state = self.0.lock().unwrap();
            state
                .traffic
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, traffic)| *traffic)
                .unwrap_or_default()
        }

        /// `me` joins the room, as its host or a client: the signaling server announces it to the
        /// ports there, and the star decides which links open (`signaling::SignalFilter`).
        pub fn port(&self, me: PeerKey, hosting: bool) -> Port {
            let mut state = self.0.lock().unwrap();
            let mut mine = SignalFilter::new(hosting);
            let id = |key: PeerKey| test_peer_id(key.0);
            let _ = mine.admit(&PeerEvent::IdAssigned(id(me)));
            let mut opened = Vec::new();
            let mut notes = Vec::new();
            for index in 0..state.ports.len() {
                let there = state.ports[index].key;
                match state.ports[index].filter.admit(&PeerEvent::NewPeer(id(me))) {
                    Admit::Pass => {
                        let offer = PeerEvent::Signal {
                            sender: id(there),
                            data: PeerSignal::Offer("v=0".into()),
                        };
                        match mine.admit(&offer) {
                            Admit::Pass => opened.push((there, me)),
                            Admit::Drop(note) => notes.extend(note),
                        }
                    }
                    Admit::Drop(note) => state.ports[index].notes.extend(note),
                }
            }
            for (a, b) in opened {
                if !state.blocked(a, b) {
                    state.links.push((a, b));
                }
            }
            state.ports.push(Slot {
                key: me,
                filter: mine,
                inbox: VecDeque::new(),
                notes,
            });
            drop(state);
            Port {
                hub: self.clone(),
                me,
                joined: false,
                known: Vec::new(),
                open: true,
            }
        }

        /// `key`'s tab closes without a word: the others see it gone.
        pub fn drop_port(&self, key: PeerKey) {
            let mut state = self.0.lock().unwrap();
            state.ports.retain(|slot| slot.key != key);
            state.links.retain(|(a, b)| *a != key && *b != key);
        }
    }

    /// A peer's way into the hub.
    pub struct Port {
        hub: Hub,
        me: PeerKey,
        joined: bool,
        /// The peers reported connected.
        known: Vec<PeerKey>,
        open: bool,
    }

    impl Port {
        pub fn poll(&mut self, on: &mut impl FnMut(SocketEvent)) {
            if !self.open {
                return;
            }
            if !self.joined {
                self.joined = true;
                on(SocketEvent::Joined(self.me));
            }
            let (linked, packets, signal_notes, notes) = {
                let mut state = self.hub.0.lock().unwrap();
                let signal_notes = state.signal_notes;
                let me = self.me;
                // In the order they opened: the host's links in its clients' order of arrival.
                let linked: Vec<(PeerKey, bool)> = state
                    .links
                    .iter()
                    .filter_map(|(a, b)| match (*a == me, *b == me) {
                        (true, _) => Some((*b, true)),
                        (_, true) => Some((*a, false)),
                        _ => None,
                    })
                    .collect();
                let mut packets = Vec::new();
                let mut notes = Vec::new();
                if let Some(slot) = state.ports.iter_mut().find(|slot| slot.key == me) {
                    notes = std::mem::take(&mut slot.notes);
                    let mut kept = VecDeque::new();
                    for (wait, from, channel, bytes) in slot.inbox.drain(..) {
                        if wait == 0 {
                            packets.push((from, channel, bytes));
                        } else {
                            kept.push_back((wait - 1, from, channel, bytes));
                        }
                    }
                    slot.inbox = kept;
                }
                (linked, packets, signal_notes, notes)
            };
            for note in notes {
                on(SocketEvent::Signal(note));
            }
            for &(key, offer) in &linked {
                if !self.known.contains(&key) {
                    self.known.push(key);
                    if signal_notes {
                        let (peer, candidates) = (key, Candidates::default());
                        let ours = SignalNote::SdpSent {
                            peer,
                            offer,
                            candidates,
                        };
                        let theirs = SignalNote::SdpReceived {
                            peer,
                            offer: !offer,
                            candidates,
                        };
                        if offer {
                            on(SocketEvent::Signal(SignalNote::Announced(peer)));
                            on(SocketEvent::Signal(ours));
                            on(SocketEvent::Signal(theirs));
                        } else {
                            on(SocketEvent::Signal(theirs));
                            on(SocketEvent::Signal(ours));
                        }
                    }
                    on(SocketEvent::Connected(key));
                }
            }
            let gone: Vec<PeerKey> = self
                .known
                .iter()
                .filter(|key| !linked.iter().any(|(k, _)| k == *key))
                .copied()
                .collect();
            for key in gone {
                self.known.retain(|k| *k != key);
                on(SocketEvent::Disconnected(key));
            }
            for (from, channel, bytes) in packets {
                if self.known.contains(&from) {
                    on(SocketEvent::Packet(from, channel, bytes));
                }
            }
        }

        pub fn is_connected(&self, peer: PeerKey) -> bool {
            self.open && self.known.contains(&peer)
        }

        pub fn send(&mut self, to: PeerKey, message: &Message) {
            if !self.open || !self.known.contains(&to) {
                return;
            }
            let mut state = self.hub.0.lock().unwrap();
            if !state.linked(self.me, to) {
                return;
            }
            let channel = message.channel();
            if channel == Channel::Unreliable && state.lose_every > 0 {
                state.unreliable_sent += 1;
                if state.unreliable_sent.is_multiple_of(state.lose_every) {
                    return;
                }
            }
            let bytes: Box<[u8]> = message.to_bytes().into();
            let me = self.me;
            match state.traffic.iter_mut().find(|(k, _)| *k == me) {
                Some((_, traffic)) => {
                    traffic.bytes[channel.index()] += bytes.len() as u64;
                    traffic.packets[channel.index()] += 1;
                }
                None => {
                    let mut traffic = Traffic::default();
                    traffic.bytes[channel.index()] = bytes.len() as u64;
                    traffic.packets[channel.index()] = 1;
                    state.traffic.push((me, traffic));
                }
            }
            let delay = state.delay;
            if let Some(slot) = state.ports.iter_mut().find(|slot| slot.key == to) {
                slot.inbox.push_back((delay, me, channel, bytes));
            }
        }

        /// Leaves the room: what was sent still arrives; the others see the port gone.
        pub fn close(&mut self) {
            self.open = false;
            self.hub.drop_port(self.me);
        }

        pub fn closed(&self) -> bool {
            !self.open
        }
    }

    /// A socket dropped (a session replaced): its peer leaves the room, as when a socket's
    /// connection to the signaling server closes.
    impl Drop for Port {
        fn drop(&mut self) {
            if self.open {
                self.hub.drop_port(self.me);
            }
        }
    }

    /// The signaling of a memory socket's unused WebRTC part: it never answers.
    #[derive(Debug)]
    pub struct Nowhere;

    #[async_trait]
    impl SignallerBuilder for Nowhere {
        async fn new_signaller(
            &self,
            _attempts: Option<u16>,
            _room_url: String,
        ) -> Result<Box<dyn Signaller>, SignalingError> {
            Ok(Box::new(Nowhere))
        }
    }

    #[async_trait]
    impl Signaller for Nowhere {
        async fn send(&mut self, _request: PeerRequest) -> Result<(), SignalingError> {
            Ok(())
        }

        async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
            std::future::pending().await
        }
    }
}
