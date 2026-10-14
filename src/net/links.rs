//! The direct connections being made (issue #49): a peer the signaling server announced, or whose
//! offer arrived, until its connection opens or it leaves. Pure logic, unit-tested.
//!
//! Fed with what went through the signaling server ([`SignalNote`]) and with the peers the socket
//! reports connected, it says what the console and the lobby show:
//! - each step, with the time since the peer was announced (`Links::note`, `Links::connected`);
//! - a peer not connected after [`CONNECTING_SHOWN_MS`]: « connexion en cours »
//!   ([`Diagnosis::Connecting`]);
//! - a connection that does not come ([`Diagnosis::Stuck`]): no candidate on one side
//!   [`NO_CANDIDATE_MS`] after the offer and the answer crossed (the browser gave no address it
//!   can be reached at), the browser saw it fail (`Links::failed`), or still nothing after
//!   [`STUCK_MS`].

use super::ice::Candidates;
use super::protocol::PeerKey;
use super::signaling::SignalNote;

/// A peer still not connected after this long shows in the lobby (ms): a connection usually opens
/// well before (in less than half a second on a local network).
pub const CONNECTING_SHOWN_MS: u64 = 2_000;

/// No candidate on one side this long after the offer and the answer crossed (ms): that side has
/// none (natively, the candidates follow the answer by a few milliseconds).
pub const NO_CANDIDATE_MS: u64 = 2_000;

/// Still not connected after this long (ms): the lobby gives the leads.
pub const STUCK_MS: u64 = 10_000;

/// Who makes the offer: the peer that was in the room first (the server announces the newcomer to
/// it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Offerer,
    Answerer,
}

/// A connection being made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub peer: PeerKey,
    pub role: Role,
    /// Announced (or its offer arrived) at this time (ms).
    pub since_ms: u64,
    /// When our offer or answer left, and when theirs arrived.
    pub ours_ms: Option<u64>,
    pub theirs_ms: Option<u64>,
    /// The candidates each side gave: in its offer or answer, then one by one.
    pub ours: Candidates,
    pub theirs: Candidates,
    /// The browser saw a direct connection fail meanwhile.
    pub failed: bool,
    /// Already said in the console that it is stuck.
    pub reported: bool,
}

impl Link {
    fn new(peer: PeerKey, role: Role, now_ms: u64) -> Self {
        Self {
            peer,
            role,
            since_ms: now_ms,
            ours_ms: None,
            theirs_ms: None,
            ours: Candidates::default(),
            theirs: Candidates::default(),
            failed: false,
            reported: false,
        }
    }

    /// Why it does not come, if we can tell yet.
    pub fn stuck(&self, now_ms: u64) -> Option<Stuck> {
        let crossed = self.ours_ms.zip(self.theirs_ms).map(|(a, b)| a.max(b));
        let quiet = crossed.is_some_and(|at| now_ms >= at + NO_CANDIDATE_MS);
        if quiet && self.ours.total() == 0 {
            Some(Stuck::NoCandidateHere)
        } else if quiet && self.theirs.total() == 0 {
            Some(Stuck::NoCandidateThere)
        } else if self.failed {
            Some(Stuck::Failed)
        } else if now_ms >= self.since_ms + STUCK_MS {
            Some(Stuck::Slow)
        } else {
            None
        }
    }
}

/// Why a direct connection does not come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stuck {
    /// Our browser gave no candidate: no address the other can reach us at.
    NoCandidateHere,
    /// The other's browser gave none.
    NoCandidateThere,
    /// The browser tried every pair of addresses: none answered (ICE « failed »).
    Failed,
    /// Nothing yet, after [`STUCK_MS`].
    Slow,
}

impl Stuck {
    /// For the console.
    pub fn explain(self) -> &'static str {
        match self {
            Self::NoCandidateHere => {
                "this browser gave no ICE candidate (no address the other can reach)"
            }
            Self::NoCandidateThere => "the other browser gave no ICE candidate",
            Self::Failed => "the browser found no working path (ICE failed)",
            Self::Slow => "still not connected",
        }
    }
}

/// What the lobby shows about the connections being made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diagnosis {
    /// `peers` connections take a moment.
    Connecting { peers: usize },
    /// `peers` connections do not come; `cause`: that of the first one.
    Stuck { peers: usize, cause: Stuck },
}

/// The connections being made, in their order of arrival.
#[derive(Clone, Debug, Default)]
pub struct Links {
    links: Vec<Link>,
}

impl Links {
    pub fn pending(&self) -> &[Link] {
        &self.links
    }

    fn get(&mut self, peer: PeerKey, role: Role, now_ms: u64) -> &mut Link {
        let index = match self.links.iter().position(|link| link.peer == peer) {
            Some(index) => index,
            None => {
                self.links.push(Link::new(peer, role, now_ms));
                self.links.len() - 1
            }
        };
        &mut self.links[index]
    }

    /// Something went through the signaling server: what the console says about it, if anything.
    pub fn note(&mut self, note: &SignalNote, now_ms: u64) -> Option<String> {
        let seconds = |since: u64| (now_ms.saturating_sub(since)) as f32 / 1000.0;
        match *note {
            SignalNote::Announced(peer) => {
                // Announced again (it came back): start over.
                self.links.retain(|link| link.peer != peer);
                self.get(peer, Role::Offerer, now_ms);
                Some(format!(
                    "#{} announced by the signaling server: we make the offer",
                    peer.tag()
                ))
            }
            SignalNote::SdpSent {
                peer,
                offer,
                candidates,
            } => {
                let role = if offer { Role::Offerer } else { Role::Answerer };
                let link = self.get(peer, role, now_ms);
                link.ours_ms = Some(now_ms);
                link.ours.add(candidates);
                let what = if offer { "offer" } else { "answer" };
                Some(format!(
                    "{what} sent to #{} after {:.2} s: {candidates}",
                    peer.tag(),
                    seconds(link.since_ms)
                ))
            }
            SignalNote::SdpReceived {
                peer,
                offer,
                candidates,
            } => {
                let role = if offer { Role::Answerer } else { Role::Offerer };
                let link = self.get(peer, role, now_ms);
                link.theirs_ms = Some(now_ms);
                link.theirs.add(candidates);
                let what = if offer { "offer" } else { "answer" };
                Some(format!(
                    "{what} received from #{} after {:.2} s: {candidates}",
                    peer.tag(),
                    seconds(link.since_ms)
                ))
            }
            SignalNote::CandidateSent { peer, candidates } => {
                if let Some(link) = self.links.iter_mut().find(|link| link.peer == peer) {
                    link.ours.add(candidates);
                }
                None
            }
            SignalNote::CandidateReceived { peer, candidates } => {
                if let Some(link) = self.links.iter_mut().find(|link| link.peer == peer) {
                    link.theirs.add(candidates);
                }
                None
            }
            SignalNote::Left(peer) => {
                let index = self.links.iter().position(|link| link.peer == peer)?;
                let link = self.links.remove(index);
                Some(format!(
                    "#{} left the room after {:.2} s, before the direct connection opened",
                    peer.tag(),
                    seconds(link.since_ms)
                ))
            }
        }
    }

    /// The socket reports `peer` connected: what the console says.
    pub fn connected(&mut self, peer: PeerKey, now_ms: u64) -> String {
        match self.links.iter().position(|link| link.peer == peer) {
            Some(index) => {
                let link = self.links.remove(index);
                format!(
                    "#{} connected, {:.2} s after it was announced ({} here, {} there)",
                    peer.tag(),
                    now_ms.saturating_sub(link.since_ms) as f32 / 1000.0,
                    link.ours,
                    link.theirs
                )
            }
            None => format!("#{} connected", peer.tag()),
        }
    }

    /// The socket reports `peer` gone.
    pub fn disconnected(&mut self, peer: PeerKey) {
        self.links.retain(|link| link.peer != peer);
    }

    /// The browser saw a direct connection fail: the pending ones are stuck.
    pub fn failed(&mut self) {
        for link in &mut self.links {
            link.failed = true;
        }
    }

    /// The links newly stuck, for a warning in the console (once each).
    pub fn newly_stuck(&mut self, now_ms: u64) -> Vec<String> {
        let mut warnings = Vec::new();
        for link in &mut self.links {
            if link.reported {
                continue;
            }
            if let Some(stuck) = link.stuck(now_ms) {
                link.reported = true;
                let side = |at: Option<u64>, candidates: Candidates, missing: &str| match at {
                    Some(_) => candidates.to_string(),
                    None => missing.to_string(),
                };
                let (ours_missing, theirs_missing) = match link.role {
                    Role::Offerer => ("offer not sent yet", "no answer yet"),
                    Role::Answerer => ("answer not sent yet", "no offer yet"),
                };
                warnings.push(format!(
                    "#{} not connected after {:.1} s: {} (ours: {}, theirs: {}; README, « Dépannage »)",
                    link.peer.tag(),
                    now_ms.saturating_sub(link.since_ms) as f32 / 1000.0,
                    stuck.explain(),
                    side(link.ours_ms, link.ours, ours_missing),
                    side(link.theirs_ms, link.theirs, theirs_missing)
                ));
            }
        }
        warnings
    }

    /// What the lobby shows: nothing while every connection is young.
    pub fn diagnosis(&self, now_ms: u64) -> Option<Diagnosis> {
        let stuck: Vec<Stuck> = self
            .links
            .iter()
            .filter_map(|link| link.stuck(now_ms))
            .collect();
        if let Some(&cause) = stuck.first() {
            return Some(Diagnosis::Stuck {
                peers: stuck.len(),
                cause,
            });
        }
        let waiting = self
            .links
            .iter()
            .filter(|link| now_ms >= link.since_ms + CONNECTING_SHOWN_MS)
            .count();
        (waiting > 0).then_some(Diagnosis::Connecting { peers: waiting })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PeerKey = PeerKey(0xf411_8f53 << 96);
    const B: PeerKey = PeerKey(0xf599_073b << 96);

    fn mdns(n: u16) -> Candidates {
        Candidates {
            mdns: n,
            ..Default::default()
        }
    }

    fn sent(peer: PeerKey, offer: bool, candidates: Candidates) -> SignalNote {
        SignalNote::SdpSent {
            peer,
            offer,
            candidates,
        }
    }

    fn received(peer: PeerKey, offer: bool, candidates: Candidates) -> SignalNote {
        SignalNote::SdpReceived {
            peer,
            offer,
            candidates,
        }
    }

    /// The usual case: announced, offer, answer, connected in a moment. Nothing in the lobby.
    #[test]
    fn a_connection_that_opens_at_once_shows_nothing() {
        let mut links = Links::default();
        let log = links.note(&SignalNote::Announced(B), 1_000).unwrap();
        assert_eq!(
            log,
            "#f599 announced by the signaling server: we make the offer"
        );
        assert_eq!(links.pending()[0].role, Role::Offerer);
        let log = links.note(&sent(B, true, mdns(1)), 1_130).unwrap();
        assert_eq!(
            log,
            "offer sent to #f599 after 0.13 s: 1 candidate (1 host .local)"
        );
        let log = links.note(&received(B, false, mdns(2)), 1_300).unwrap();
        assert_eq!(
            log,
            "answer received from #f599 after 0.30 s: 2 candidates (2 host .local)"
        );
        assert_eq!(links.diagnosis(1_400), None);
        let log = links.connected(B, 1_350);
        assert_eq!(
            log,
            "#f599 connected, 0.35 s after it was announced (1 candidate (1 host .local) here, 2 candidates (2 host .local) there)"
        );
        assert!(links.pending().is_empty());
        assert_eq!(links.diagnosis(60_000), None);
        assert!(links.newly_stuck(60_000).is_empty());
        // A peer connected without passing here (the socket's word is enough).
        assert_eq!(links.connected(A, 2_000), "#f411 connected");
    }

    /// The answering side learns of the peer from its offer.
    #[test]
    fn the_answering_side_starts_with_the_offer() {
        let mut links = Links::default();
        links.note(&received(A, true, mdns(1)), 500);
        let link = &links.pending()[0];
        assert_eq!(
            (link.peer, link.role, link.since_ms),
            (A, Role::Answerer, 500)
        );
        // Natively, the answer has no candidate: they follow one by one.
        links.note(&sent(A, false, Candidates::default()), 520);
        links.note(
            &SignalNote::CandidateSent {
                peer: A,
                candidates: Candidates {
                    host: 1,
                    ..Default::default()
                },
            },
            525,
        );
        assert_eq!(links.pending()[0].ours.total(), 1);
        assert_eq!(links.pending()[0].stuck(5_000), None);
        // Trickled candidates of an unknown peer are ignored.
        assert_eq!(
            links.note(
                &SignalNote::CandidateReceived {
                    peer: B,
                    candidates: mdns(1)
                },
                600
            ),
            None
        );
        assert_eq!(links.pending().len(), 1);
    }

    /// Max's case (Chrome on a Mac, mDNS addresses refused, STUN unreachable): offer and answer
    /// cross without any candidate. The lobby says so 2 s later, before the 10 s.
    #[test]
    fn no_candidate_here_is_seen_soon_after_the_answer() {
        let mut links = Links::default();
        links.note(&SignalNote::Announced(B), 0);
        links.note(&sent(B, true, Candidates::default()), 100);
        assert_eq!(links.diagnosis(1_000), None);
        assert_eq!(
            links.diagnosis(CONNECTING_SHOWN_MS),
            Some(Diagnosis::Connecting { peers: 1 })
        );
        links.note(&received(B, false, Candidates::default()), 3_000);
        assert_eq!(
            links.diagnosis(4_999),
            Some(Diagnosis::Connecting { peers: 1 })
        );
        assert_eq!(
            links.diagnosis(5_000),
            Some(Diagnosis::Stuck {
                peers: 1,
                cause: Stuck::NoCandidateHere
            })
        );
        let warnings = links.newly_stuck(5_000);
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0]
                .starts_with("#f599 not connected after 5.0 s: this browser gave no ICE candidate"),
            "{}",
            warnings[0]
        );
        // Once.
        assert!(links.newly_stuck(9_000).is_empty());
        // A candidate that comes late changes the diagnosis.
        links.note(
            &SignalNote::CandidateSent {
                peer: B,
                candidates: mdns(1),
            },
            6_000,
        );
        assert_eq!(
            links.diagnosis(6_000),
            Some(Diagnosis::Stuck {
                peers: 1,
                cause: Stuck::NoCandidateThere
            })
        );
    }

    /// The offer never leaves (the browser still gathering), or the browser saw the connection
    /// fail: leads after 10 s, or at once.
    #[test]
    fn slow_and_failed_connections() {
        let mut links = Links::default();
        links.note(&SignalNote::Announced(B), 1_000);
        assert_eq!(
            links.diagnosis(10_999),
            Some(Diagnosis::Connecting { peers: 1 })
        );
        assert_eq!(
            links.diagnosis(11_000),
            Some(Diagnosis::Stuck {
                peers: 1,
                cause: Stuck::Slow
            })
        );
        // The browser still gathering its candidates: our offer has not left.
        assert_eq!(
            links.newly_stuck(11_000),
            vec![
                "#f599 not connected after 10.0 s: still not connected (ours: offer not sent yet, \
                 theirs: no answer yet; README, « Dépannage »)"
                    .to_string()
            ]
        );
        links.note(&received(A, true, mdns(1)), 2_000);
        links.note(&sent(A, false, mdns(1)), 2_100);
        assert_eq!(links.pending()[1].stuck(2_200), None);
        // The browser saw a connection fail: every pending one may be it.
        links.failed();
        assert_eq!(links.pending()[1].stuck(2_200), Some(Stuck::Failed));
        assert_eq!(
            links.diagnosis(2_200),
            Some(Diagnosis::Stuck {
                peers: 2,
                cause: Stuck::Failed
            })
        );
        // Gone: one before its connection opened (the server says), one from the socket.
        let log = links.note(&SignalNote::Left(B), 12_000).unwrap();
        assert_eq!(
            log,
            "#f599 left the room after 11.00 s, before the direct connection opened"
        );
        assert_eq!(links.note(&SignalNote::Left(B), 12_000), None);
        links.disconnected(A);
        assert!(links.pending().is_empty());
        assert_eq!(links.diagnosis(20_000), None);
    }

    /// A peer announced again (it came back) starts over.
    #[test]
    fn announced_again_starts_over() {
        let mut links = Links::default();
        links.note(&SignalNote::Announced(B), 0);
        links.failed();
        links.note(&SignalNote::Announced(B), 30_000);
        assert_eq!(links.pending().len(), 1);
        assert_eq!(links.pending()[0].since_ms, 30_000);
        assert_eq!(links.diagnosis(30_500), None);
    }
}
