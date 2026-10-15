//! The signaling server, seen from the game (issue #49): what goes through it, for the console and
//! the lobby. `matchbox_socket` 0.14 only reports the peers whose connection is open; the
//! signaling server tells more, and earlier: a newcomer announced ([`SignalNote::Announced`]: we
//! make the offer), the offers and answers with their ICE candidates, a peer that left. Without
//! it, a peer whose direct connection never opens would stay invisible.
//!
//! The crate lets a game bring its own [`SignallerBuilder`] (its `Signaller` extension point):
//! [`ObservedSignallerBuilder`] wraps another one and writes down what passes.
//! [`WebSocketSignallerBuilder`] is the crate's own WebSocket signaller (private there), written
//! again with the same crates: `async-tungstenite` natively, `ws_stream_wasm` in the browser,
//! nothing more compiled.
//!
//! **The star** (issue #63, [`SignalFilter`]): `matchbox_server` makes a full mesh: it announces
//! every newcomer to every peer already there, which then makes it an offer. Here, only the links
//! between the host and its clients open: the observed signaller keeps from the crate's message
//! loop what would open another one.
//! - **The host** (the peer that created the room) takes every newcomer: it makes the offer.
//! - **A client** never makes an offer: it drops the announcements of the newcomers (the host
//!   connects them). It takes the **first offer** that comes, and only that peer's signals from
//!   then on: only a host makes offers, so that peer is the host. Two clients that come at once
//!   both get the host's offer only.
//! - **Two hosts** (an address with `?host=1` opened twice): the one that came second gets the
//!   first one's offer, and a host never takes an offer: it drops it and notes that the room has
//!   its host ([`SignalNote::OtherHost`]); the game leaves the room with a message. Nobody else
//!   ever connects to it.
//!
//! Dropping an event is only not passing it on: the signaller reads the next one, the crate's
//! loop goes on (an offer it never sees opens no connection; the offering peer waits for an
//! answer that never comes, until it leaves).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_timer::Delay;
use matchbox_socket::async_trait::async_trait;
use matchbox_socket::{
    PeerEvent, PeerRequest, PeerSignal, SignalingError, Signaller, SignallerBuilder,
};

use super::ice::Candidates;
use super::protocol::PeerKey;
use super::socket::key_of;

/// Between two tries to reach the signaling server (s), as `matchbox_socket` does.
pub const RETRY_SECONDS: u64 = 3;

/// What went through the signaling server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalNote {
    /// The server introduced a newcomer to us: we make the offer.
    Announced(PeerKey),
    /// Our offer (`offer`) or answer to `peer` left, with the candidates written in it (in the
    /// browser, all those gathered: `matchbox_socket` waits for them; natively, none: they follow
    /// one by one).
    SdpSent {
        peer: PeerKey,
        offer: bool,
        candidates: Candidates,
    },
    /// An offer or an answer from `peer` arrived.
    SdpReceived {
        peer: PeerKey,
        offer: bool,
        candidates: Candidates,
    },
    /// A candidate sent alone, after the offer or the answer (« trickle ICE »).
    CandidateSent {
        peer: PeerKey,
        candidates: Candidates,
    },
    CandidateReceived {
        peer: PeerKey,
        candidates: Candidates,
    },
    /// The server says `peer` left the room.
    Left(PeerKey),
    /// Issue #63: a client learns that `peer` entered the room: the host connects it, not us.
    Arrived(PeerKey),
    /// Issue #63: we wanted to host, and `peer`, in the room before us, made us an offer: the
    /// room has its host already. We take nothing more.
    OtherHost(PeerKey),
    /// Issue #63: a client got an offer from `peer`, which is not the host it connected to (a
    /// second host): dropped.
    NotOurHost(PeerKey),
}

/// What [`SignalFilter::admit`] says of an event of the signaling server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admit {
    /// The crate's message loop gets it.
    Pass,
    /// Kept from the loop, with what to note.
    Drop(Option<SignalNote>),
}

/// The star around the host (issue #63, see the module's documentation): which events of the
/// signaling server reach `matchbox_socket`'s loop. Pure logic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalFilter {
    hosting: bool,
    /// A client's host: the peer whose offer it took.
    host: Option<PeerKey>,
    /// We wanted to host a room that has its host: nothing more.
    refused: bool,
}

impl SignalFilter {
    /// The filter of a peer that hosts the room (it created it), or of a client.
    pub fn new(hosting: bool) -> Self {
        Self {
            hosting,
            host: None,
            refused: false,
        }
    }

    /// A client: the host it took the offer of, once it came.
    pub fn host(&self) -> Option<PeerKey> {
        self.host
    }

    /// Whether `event` goes on to the crate's loop, and what to note when it does not.
    pub fn admit(&mut self, event: &PeerEvent) -> Admit {
        match event {
            PeerEvent::IdAssigned(_) | PeerEvent::PeerLeft(_) => Admit::Pass,
            PeerEvent::NewPeer(id) => {
                if !self.hosting {
                    Admit::Drop(Some(SignalNote::Arrived(key_of(*id))))
                } else if self.refused {
                    Admit::Drop(None)
                } else {
                    Admit::Pass
                }
            }
            PeerEvent::Signal { sender, data } => {
                let peer = key_of(*sender);
                let offer = matches!(data, PeerSignal::Offer(_));
                if self.hosting {
                    if offer && !self.refused {
                        // Only a host makes offers, and it makes them to those who come after.
                        self.refused = true;
                        return Admit::Drop(Some(SignalNote::OtherHost(peer)));
                    }
                    return if self.refused {
                        Admit::Drop(None)
                    } else {
                        Admit::Pass
                    };
                }
                match self.host {
                    Some(host) if host == peer => Admit::Pass,
                    None if offer => {
                        self.host = Some(peer);
                        Admit::Pass
                    }
                    _ if offer => Admit::Drop(Some(SignalNote::NotOurHost(peer))),
                    _ => Admit::Drop(None),
                }
            }
        }
    }
}

/// What a message we send says, if anything worth noting (not the keep-alives).
pub fn note_of_request(request: &PeerRequest) -> Option<SignalNote> {
    let PeerRequest::Signal { receiver, data } = request else {
        return None;
    };
    let peer = key_of(*receiver);
    Some(match data {
        PeerSignal::Offer(sdp) | PeerSignal::Answer(sdp) => SignalNote::SdpSent {
            peer,
            offer: matches!(data, PeerSignal::Offer(_)),
            candidates: Candidates::count(sdp),
        },
        PeerSignal::IceCandidate(json) => SignalNote::CandidateSent {
            peer,
            candidates: Candidates::count(json),
        },
    })
}

/// What a message from the server says, if anything worth noting (not our own id).
pub fn note_of_event(event: &PeerEvent) -> Option<SignalNote> {
    Some(match event {
        PeerEvent::IdAssigned(_) => return None,
        PeerEvent::NewPeer(id) => SignalNote::Announced(key_of(*id)),
        PeerEvent::PeerLeft(id) => SignalNote::Left(key_of(*id)),
        PeerEvent::Signal { sender, data } => {
            let peer = key_of(*sender);
            match data {
                PeerSignal::Offer(sdp) | PeerSignal::Answer(sdp) => SignalNote::SdpReceived {
                    peer,
                    offer: matches!(data, PeerSignal::Offer(_)),
                    candidates: Candidates::count(sdp),
                },
                PeerSignal::IceCandidate(json) => SignalNote::CandidateReceived {
                    peer,
                    candidates: Candidates::count(json),
                },
            }
        }
    })
}

/// The notes written by the message loop, until the game reads them ([`NetSocket::poll`]).
///
/// [`NetSocket::poll`]: super::NetSocket::poll
pub type Notes = Arc<Mutex<Vec<SignalNote>>>;

fn write(notes: &Notes, note: Option<SignalNote>) {
    if let (Some(note), Ok(mut notes)) = (note, notes.lock()) {
        notes.push(note);
    }
}

/// Wraps a [`SignallerBuilder`]: its signallers pass on what makes the star around the host
/// (issue #63, [`SignalFilter`]), and write down what passes.
#[derive(Debug)]
pub struct ObservedSignallerBuilder {
    inner: Arc<dyn SignallerBuilder>,
    notes: Notes,
    hosting: bool,
}

impl ObservedSignallerBuilder {
    /// `hosting`: we host the room (we created it), or join it as a client.
    pub fn new(inner: Arc<dyn SignallerBuilder>, notes: Notes, hosting: bool) -> Self {
        Self {
            inner,
            notes,
            hosting,
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SignallerBuilder for ObservedSignallerBuilder {
    async fn new_signaller(
        &self,
        attempts: Option<u16>,
        room_url: String,
    ) -> Result<Box<dyn Signaller>, SignalingError> {
        let inner = self.inner.new_signaller(attempts, room_url).await?;
        Ok(Box::new(ObservedSignaller {
            inner,
            notes: self.notes.clone(),
            filter: SignalFilter::new(self.hosting),
        }))
    }
}

struct ObservedSignaller {
    inner: Box<dyn Signaller>,
    notes: Notes,
    filter: SignalFilter,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Signaller for ObservedSignaller {
    async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
        write(&self.notes, note_of_request(&request));
        self.inner.send(request).await
    }

    /// The next event the star lets through (issue #63). Cancelled (the crate's loop sends a
    /// request meanwhile), nothing is lost: the only wait is the inner signaller's, and an event
    /// read is given or dropped at once.
    async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
        loop {
            let event = self.inner.next_message().await?;
            match self.filter.admit(&event) {
                Admit::Pass => {
                    write(&self.notes, note_of_event(&event));
                    return Ok(event);
                }
                Admit::Drop(note) => write(&self.notes, note),
            }
        }
    }
}

/// The signaling server over a WebSocket, like `matchbox_socket`'s own: `attempts` tries,
/// [`RETRY_SECONDS`] apart (`None`: forever).
#[derive(Debug, Default)]
pub struct WebSocketSignallerBuilder;

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SignallerBuilder for WebSocketSignallerBuilder {
    async fn new_signaller(
        &self,
        mut attempts: Option<u16>,
        room_url: String,
    ) -> Result<Box<dyn Signaller>, SignalingError> {
        loop {
            match transport::connect(&room_url).await {
                Ok(signaller) => return Ok(Box::new(signaller)),
                Err(error) => {
                    if let Some(left) = attempts.as_mut() {
                        if *left <= 1 {
                            return Err(SignalingError::NegotiationFailed(Box::new(error)));
                        }
                        *left -= 1;
                    }
                    bevy::log::warn!(
                        "network: the signaling server {room_url} cannot be reached ({error}), \
                         trying again in {RETRY_SECONDS} s"
                    );
                    Delay::new(Duration::from_secs(RETRY_SECONDS)).await;
                }
            }
        }
    }
}

fn to_json(request: &PeerRequest) -> Result<String, SignalingError> {
    serde_json::to_string(request)
        .map_err(|error| SignalingError::UserImplementationError(error.to_string()))
}

fn from_json(text: &str) -> Result<PeerEvent, SignalingError> {
    serde_json::from_str(text).map_err(|_| SignalingError::UnknownFormat)
}

#[cfg(not(target_arch = "wasm32"))]
mod transport {
    use async_tungstenite::WebSocketStream;
    use async_tungstenite::async_std::{ConnectStream, connect_async};
    use async_tungstenite::tungstenite::Message;
    use futures_util::StreamExt;
    use matchbox_socket::async_trait::async_trait;
    use matchbox_socket::{PeerEvent, PeerRequest, SignalingError, Signaller};

    pub struct WebSocketSignaller {
        stream: WebSocketStream<ConnectStream>,
    }

    pub async fn connect(room_url: &str) -> Result<WebSocketSignaller, SignalingError> {
        let (stream, _) = connect_async(room_url).await?;
        Ok(WebSocketSignaller { stream })
    }

    #[async_trait]
    impl Signaller for WebSocketSignaller {
        async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
            let text = super::to_json(&request)?;
            Ok(self.stream.send(Message::Text(text.into())).await?)
        }

        async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
            match self.stream.next().await {
                Some(Ok(Message::Text(text))) => super::from_json(&text),
                Some(Ok(_)) => Err(SignalingError::UnknownFormat),
                Some(Err(error)) => Err(error.into()),
                None => Err(SignalingError::StreamExhausted),
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod transport {
    use futures_util::stream::Fuse;
    use futures_util::{SinkExt, StreamExt};
    use matchbox_socket::async_trait::async_trait;
    use matchbox_socket::{PeerEvent, PeerRequest, SignalingError, Signaller};
    use ws_stream_wasm::{WsMessage, WsMeta, WsStream};

    pub struct WebSocketSignaller {
        stream: Fuse<WsStream>,
    }

    pub async fn connect(room_url: &str) -> Result<WebSocketSignaller, SignalingError> {
        let (_, stream) = WsMeta::connect(room_url, None).await?;
        Ok(WebSocketSignaller {
            stream: stream.fuse(),
        })
    }

    #[async_trait(?Send)]
    impl Signaller for WebSocketSignaller {
        async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
            let text = super::to_json(&request)?;
            Ok(self.stream.send(WsMessage::Text(text)).await?)
        }

        async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
            match self.stream.next().await {
                Some(WsMessage::Text(text)) => super::from_json(&text),
                Some(_) => Err(SignalingError::UnknownFormat),
                None => Err(SignalingError::StreamExhausted),
            }
        }
    }
}

/// A peer id from its number, for the tests (the crate does not export its UUID type: through
/// its JSON).
#[cfg(test)]
pub(crate) fn test_peer_id(n: u128) -> matchbox_socket::PeerId {
    let hex = format!("{n:032x}");
    let text = format!(
        "\"{}-{}-{}-{}-{}\"",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    serde_json::from_str(&text).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> matchbox_socket::PeerId {
        test_peer_id(n)
    }

    const OFFER: &str = "v=0\r\na=candidate:1 1 udp 2113937151 e3f16915-31ad-4551-a16d-4d8246d4cb58.local 45627 typ host generation 0\r\n";

    #[test]
    fn what_we_send_is_noted() {
        let offer = PeerRequest::Signal {
            receiver: id(7),
            data: PeerSignal::Offer(OFFER.into()),
        };
        assert_eq!(
            note_of_request(&offer),
            Some(SignalNote::SdpSent {
                peer: PeerKey(7),
                offer: true,
                candidates: Candidates {
                    mdns: 1,
                    ..Default::default()
                }
            })
        );
        let answer = PeerRequest::Signal {
            receiver: id(7),
            data: PeerSignal::Answer("v=0\r\n".into()),
        };
        assert_eq!(
            note_of_request(&answer),
            Some(SignalNote::SdpSent {
                peer: PeerKey(7),
                offer: false,
                candidates: Candidates::default()
            })
        );
        let candidate = PeerRequest::Signal {
            receiver: id(7),
            data: PeerSignal::IceCandidate(
                r#"{"candidate":"candidate:2 1 udp 1 192.168.1.2 5000 typ host","sdpMid":"0"}"#
                    .into(),
            ),
        };
        assert_eq!(
            note_of_request(&candidate),
            Some(SignalNote::CandidateSent {
                peer: PeerKey(7),
                candidates: Candidates {
                    host: 1,
                    ..Default::default()
                }
            })
        );
        assert_eq!(note_of_request(&PeerRequest::KeepAlive), None);
    }

    #[test]
    fn what_the_server_says_is_noted() {
        assert_eq!(note_of_event(&PeerEvent::IdAssigned(id(1))), None);
        assert_eq!(
            note_of_event(&PeerEvent::NewPeer(id(9))),
            Some(SignalNote::Announced(PeerKey(9)))
        );
        assert_eq!(
            note_of_event(&PeerEvent::PeerLeft(id(9))),
            Some(SignalNote::Left(PeerKey(9)))
        );
        assert_eq!(
            note_of_event(&PeerEvent::Signal {
                sender: id(9),
                data: PeerSignal::Offer(OFFER.into())
            }),
            Some(SignalNote::SdpReceived {
                peer: PeerKey(9),
                offer: true,
                candidates: Candidates {
                    mdns: 1,
                    ..Default::default()
                }
            })
        );
        assert_eq!(
            note_of_event(&PeerEvent::Signal {
                sender: id(9),
                data: PeerSignal::IceCandidate("null".into())
            }),
            Some(SignalNote::CandidateReceived {
                peer: PeerKey(9),
                candidates: Candidates::default()
            })
        );
    }

    fn offer(from: u128) -> PeerEvent {
        PeerEvent::Signal {
            sender: id(from),
            data: PeerSignal::Offer(OFFER.into()),
        }
    }

    fn signal(from: u128, data: PeerSignal) -> PeerEvent {
        PeerEvent::Signal {
            sender: id(from),
            data,
        }
    }

    /// Issue #63: the host lets every newcomer through (the crate makes it an offer), and their
    /// answers and candidates; the departures and its id too.
    #[test]
    fn the_host_offers_to_every_newcomer() {
        let mut host = SignalFilter::new(true);
        for event in [
            PeerEvent::IdAssigned(id(1)),
            PeerEvent::NewPeer(id(2)),
            PeerEvent::NewPeer(id(3)),
            signal(2, PeerSignal::Answer("v=0".into())),
            signal(3, PeerSignal::IceCandidate("{}".into())),
            PeerEvent::PeerLeft(id(2)),
        ] {
            assert_eq!(host.admit(&event), Admit::Pass, "{event:?}");
        }
        assert_eq!(host.host(), None);
    }

    /// Issue #63: a client never makes an offer (the newcomers are the host's); it takes the first
    /// offer, then only that peer's signals; another offer (a second host) is dropped, and said.
    #[test]
    fn a_client_takes_the_first_offer_only() {
        let mut client = SignalFilter::new(false);
        assert_eq!(client.admit(&PeerEvent::IdAssigned(id(5))), Admit::Pass);
        assert_eq!(
            client.admit(&PeerEvent::NewPeer(id(9))),
            Admit::Drop(Some(SignalNote::Arrived(PeerKey(9))))
        );
        // A candidate before any offer: nobody's.
        assert_eq!(
            client.admit(&signal(9, PeerSignal::IceCandidate("{}".into()))),
            Admit::Drop(None)
        );
        assert_eq!(client.admit(&offer(1)), Admit::Pass);
        assert_eq!(client.host(), Some(PeerKey(1)));
        assert_eq!(
            client.admit(&signal(1, PeerSignal::IceCandidate("{}".into()))),
            Admit::Pass
        );
        assert_eq!(
            client.admit(&offer(7)),
            Admit::Drop(Some(SignalNote::NotOurHost(PeerKey(7))))
        );
        assert_eq!(
            client.admit(&signal(7, PeerSignal::IceCandidate("{}".into()))),
            Admit::Drop(None)
        );
        assert_eq!(client.admit(&PeerEvent::PeerLeft(id(1))), Admit::Pass);
        assert_eq!(client.host(), Some(PeerKey(1)));
    }

    /// A room on a full-mesh signaling server like `matchbox_server` (it announces each newcomer
    /// to the peers already there, which offer), with each peer's filter: the links that open, in
    /// the order the peers came. `(id, hosting)`.
    fn links_of(arrivals: &[(u128, bool)]) -> (Vec<(u128, u128)>, Vec<(u128, SignalNote)>) {
        let mut filters: Vec<(u128, SignalFilter)> = Vec::new();
        let (mut links, mut notes) = (Vec::new(), Vec::new());
        for &(newcomer, hosting) in arrivals {
            let mut mine = SignalFilter::new(hosting);
            assert_eq!(
                mine.admit(&PeerEvent::IdAssigned(id(newcomer))),
                Admit::Pass
            );
            for (there, filter) in &mut filters {
                match filter.admit(&PeerEvent::NewPeer(id(newcomer))) {
                    // It offers: the newcomer takes it or not.
                    Admit::Pass => match mine.admit(&offer(*there)) {
                        Admit::Pass => links.push((*there, newcomer)),
                        Admit::Drop(note) => notes.extend(note.map(|n| (newcomer, n))),
                    },
                    Admit::Drop(note) => notes.extend(note.map(|n| (*there, n))),
                }
            }
            filters.push((newcomer, mine));
        }
        (links, notes)
    }

    /// Issue #63 acceptance: a host and three clients, in every order of arrival (two clients at
    /// once: one right after the other, before any answer): only the links between the host and
    /// the clients that came after it open, never one between two clients. A client that came
    /// before the host has none (a room without a host: the game says so).
    #[test]
    fn only_the_host_and_its_clients_connect_in_every_order() {
        const HOST: u128 = 1 << 100;
        let peers = [HOST, 2 << 100, 3 << 100, 4 << 100];
        let mut orders = 0;
        // Every order of the 4: 24 permutations.
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        let order = [a, b, c, d];
                        if (0..4).any(|i| !order.contains(&i)) {
                            continue;
                        }
                        orders += 1;
                        let arrivals: Vec<(u128, bool)> =
                            order.iter().map(|&i| (peers[i], i == 0)).collect();
                        let (links, notes) = links_of(&arrivals);
                        let host_at = order.iter().position(|&i| i == 0).unwrap();
                        let after: Vec<u128> =
                            order[host_at + 1..].iter().map(|&i| peers[i]).collect();
                        let expected: Vec<(u128, u128)> =
                            after.iter().map(|client| (HOST, *client)).collect();
                        assert_eq!(links, expected, "{order:?}");
                        // The clients say who came; nobody else offered.
                        assert!(
                            notes
                                .iter()
                                .all(|(_, note)| matches!(note, SignalNote::Arrived(_))),
                            "{notes:?}"
                        );
                    }
                }
            }
        }
        assert_eq!(orders, 24);
    }

    /// Issue #63: two peers that say they host (an address with `?host=1` opened twice). The one
    /// that came second is refused, whatever their ids, and the clients connect to the first one
    /// only, before or after it: the same on every peer.
    #[test]
    fn of_two_hosts_the_second_is_refused() {
        // Hosts with a big and a small id, clients before and after the second host.
        let (big, small, early, late) = (9 << 100, 1 << 100, 5 << 100, 3 << 100);
        for arrivals in [
            vec![(big, true), (small, true), (late, false)],
            vec![(big, true), (early, false), (small, true), (late, false)],
            vec![(small, true), (big, true), (late, false)],
        ] {
            let (links, notes) = links_of(&arrivals);
            let (older, newer) = (
                arrivals[0].0,
                arrivals.iter().filter(|a| a.1).nth(1).unwrap().0,
            );
            let clients: Vec<u128> = arrivals.iter().filter(|a| !a.1).map(|a| a.0).collect();
            assert_eq!(
                links,
                clients.iter().map(|c| (older, *c)).collect::<Vec<_>>(),
                "{arrivals:?}"
            );
            assert!(
                notes.contains(&(newer, SignalNote::OtherHost(PeerKey(older)))),
                "{notes:?}"
            );
            // Refused, it offers to nobody.
            assert!(links.iter().all(|(a, b)| *a != newer && *b != newer));
        }
    }

    /// The messages read and written are those of `matchbox_server` (its JSON).
    #[test]
    fn the_signaling_server_speaks_json() {
        let request = PeerRequest::Signal {
            receiver: id(5),
            data: PeerSignal::Answer("v=0".into()),
        };
        let text = to_json(&request).unwrap();
        assert!(text.starts_with(r#"{"Signal":{"receiver":"#), "{text}");
        assert!(text.contains(r#""data":{"Answer":"v=0"}"#), "{text}");
        let event = from_json(r#"{"NewPeer":"00000000-0000-0000-0000-000000000009"}"#).unwrap();
        assert_eq!(event, PeerEvent::NewPeer(id(9)));
        assert!(matches!(
            from_json("not json"),
            Err(SignalingError::UnknownFormat)
        ));
    }
}
