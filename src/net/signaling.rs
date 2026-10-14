//! The signaling server, seen from the game (issue #49): what goes through it, for the console and
//! the lobby. `matchbox_socket` 0.14 only reports the peers whose connection is open; the
//! signaling server tells more, and earlier: a newcomer announced ([`SignalNote::Announced`]: we
//! make the offer), the offers and answers with their ICE candidates, a peer that left. Without
//! it, a peer whose direct connection never opens would stay invisible.
//!
//! The crate lets a game bring its own [`SignallerBuilder`] (its `Signaller` extension point):
//! [`ObservedSignallerBuilder`] wraps another one and writes down what passes, untouched.
//! [`WebSocketSignallerBuilder`] is the crate's own WebSocket signaller (private there), written
//! again with the same crates: `async-tungstenite` natively, `ws_stream_wasm` in the browser,
//! nothing more compiled.

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

/// Wraps a [`SignallerBuilder`]: its signallers pass everything on, and write down what passes.
#[derive(Debug)]
pub struct ObservedSignallerBuilder {
    inner: Arc<dyn SignallerBuilder>,
    notes: Notes,
}

impl ObservedSignallerBuilder {
    pub fn new(inner: Arc<dyn SignallerBuilder>, notes: Notes) -> Self {
        Self { inner, notes }
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
        }))
    }
}

struct ObservedSignaller {
    inner: Box<dyn Signaller>,
    notes: Notes,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Signaller for ObservedSignaller {
    async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
        write(&self.notes, note_of_request(&request));
        self.inner.send(request).await
    }

    async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
        let event = self.inner.next_message().await?;
        write(&self.notes, note_of_event(&event));
        Ok(event)
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
