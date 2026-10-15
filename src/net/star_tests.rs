//! Headless scenarios of the star (issue #63): four whole games (`GamePlugin`) and more in a
//! room, linked by the in-memory socket of the tests (`socket::memory`), which opens the links as
//! the signaling and the star open them (`signaling::SignalFilter`: the host with each client that
//! comes after it, never two clients). Every message goes through its bytes, a client's for the
//! others through the host, relayed with its origin; the kart states in the host's bundles. The
//! helpers are those of `race_tests.rs` (the first player creates the room: it hosts).

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::race_tests::{
    ANA, BOB, CYD, Room, assert_same_progress, bots_drive_the_players, game, lobby, player_of,
    progress_by_slot, seated, standings, start_race,
};
use super::*;
use crate::interactions::{Paw, PawStats};
use crate::items::{ItemKind, ItemSlots};
use crate::kart::{KartInput, PlayerKart};
use crate::race::{Race, RacePhase};

const DAN: PeerKey = PeerKey(4 << 100);
const EVE: PeerKey = PeerKey(5 << 100);

/// The four players of the tests: Ana creates the room.
const FOUR: [(PeerKey, &str); 4] = [(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd"), (DAN, "Dan")];

/// The peers whose karts a game shows, sorted.
fn shown_peers(app: &App) -> Vec<PeerKey> {
    let mut peers: Vec<PeerKey> = app
        .world()
        .resource::<KartSync>()
        .remotes()
        .iter()
        .filter(|remote| remote.proxy.is_some())
        .map(|remote| remote.peer)
        .collect();
    peers.sort();
    peers
}

/// The states received of each kart a game shows: (its peer, how many).
fn received(app: &App) -> Vec<(PeerKey, u32)> {
    let mut all: Vec<(PeerKey, u32)> = app
        .world()
        .resource::<KartSync>()
        .remotes()
        .iter()
        .map(|remote| (remote.peer, remote.received))
        .collect();
    all.sort();
    all
}

/// Issue #63 acceptance (in memory): four games in a room. The links are the star's: the host
/// with each client, in their order of arrival, never two clients. Each game shows the three
/// other players' karts, each from the peer that drives it (the host's own states, and the
/// clients' it relays in its bundles), 30 states a second each; every lobby has the host's list.
/// A client sends its packets to one peer only: the host (its traffic says so).
#[test]
fn four_games_make_a_star_and_everyone_sees_every_kart() {
    let mut room = seated(&FOUR);
    assert_eq!(room.hub.links(), vec![(ANA, BOB), (ANA, CYD), (ANA, DAN)]);
    let keys: Vec<PeerKey> = FOUR.iter().map(|(key, _)| *key).collect();
    for (key, app) in &room.games {
        let others: Vec<PeerKey> = keys.iter().copied().filter(|k| k != key).collect();
        assert_eq!(shown_peers(app), others, "{key:?}");
        assert_eq!(
            lobby(app).seats(),
            keys.as_slice(),
            "{key:?}: the host's list"
        );
        assert_eq!(lobby(app).host(), Some(ANA));
        // A client's socket: the host only.
        let socket = app
            .world()
            .resource::<NetSession>()
            .socket
            .as_ref()
            .unwrap();
        for other in &others {
            assert_eq!(
                socket.is_connected(*other),
                *key == ANA || *other == ANA,
                "{key:?} and {other:?}"
            );
        }
    }
    let before: Vec<Vec<(PeerKey, u32)>> =
        room.games.iter().map(|(_, app)| received(app)).collect();
    let sent_before: Vec<socket::memory::Traffic> =
        keys.iter().map(|key| room.hub.traffic(*key)).collect();
    for _ in 0..120 {
        room.frame();
    }
    for (index, (key, app)) in room.games.iter().enumerate() {
        for ((peer, now), (_, then)) in received(app).iter().zip(&before[index]) {
            let fresh = now - then;
            // 30 a second (one every second tick), 2 s: 60, give or take the phase.
            assert!(
                (55..=62).contains(&fresh),
                "{key:?} got {fresh} states of {peer:?}'s kart in 2 s"
            );
        }
    }
    for (index, key) in keys.iter().enumerate() {
        let traffic = room.hub.traffic(*key);
        let packets = traffic.packets[1] - sent_before[index].packets[1];
        let bytes = traffic.bytes[1] - sent_before[index].bytes[1];
        println!("{key:?}: {packets} unreliable packets, {bytes} bytes in 2 s");
        if *key == ANA {
            // A bundle per client 30 times a second (and the pings, the pongs).
            assert!((3 * 58..=3 * 62 + 20).contains(&packets), "{packets}");
        } else {
            // Its states and its pings, to the host only: one kart, one peer.
            assert!((58..=62 + 6).contains(&packets), "{key:?}: {packets}");
        }
    }
}

/// Issue #63 acceptance (in memory): a client leaves (its tab closes): the host sees it gone,
/// and tells the others (its `Bye`, the new list): its kart disappears from every screen, the
/// player after it moves up a seat (its kart's id follows), and everyone goes on seeing the
/// others.
#[test]
fn a_client_leaving_is_seen_by_everyone_through_the_host() {
    let mut room = seated(&FOUR);
    room.close(BOB);
    room.until(5.0, "Bob gone", |app| {
        let sync = app.world().resource::<KartSync>();
        lobby(app).seats() == [ANA, CYD, DAN]
            && sync.remotes().len() == 2
            && sync.remotes().iter().all(|r| r.peer != BOB)
            && usize::from(sync.kart())
                == lobby(app)
                    .seats()
                    .iter()
                    .position(|k| *k == lobby(app).me())
                    .unwrap()
    });
    let before: Vec<Vec<(PeerKey, u32)>> =
        room.games.iter().map(|(_, app)| received(app)).collect();
    for _ in 0..60 {
        room.frame();
    }
    for (index, (key, app)) in room.games.iter().enumerate() {
        for ((peer, now), (_, then)) in received(app).iter().zip(&before[index]) {
            assert!(
                now - then >= 25,
                "{key:?}: {} states of {peer:?} in 1 s",
                now - then
            );
        }
        // Dan's kart, seat 2 now, under its new id everywhere.
        if *key != DAN {
            let sync = app.world().resource::<KartSync>();
            assert_eq!(sync.remote(DAN).map(|r| r.kart), Some(2), "{key:?}");
        }
    }
}

/// Bob's kart stands 2.4 m to the right of Cyd's, and shows there on Cyd's screen.
fn put_beside(room: &mut Room, of: PeerKey, beside: PeerKey) {
    let at = *room
        .view(of)
        .world()
        .get::<Transform>(player_of(room.view(of)))
        .unwrap();
    let place = at.translation + *at.right() * 2.4;
    {
        let app = room.app(beside);
        let kart = player_of(app);
        let world = app.world_mut();
        let mut transform = world.get_mut::<Transform>(kart).unwrap();
        *transform = at;
        transform.translation = place;
        let placed = *transform;
        world
            .get_mut::<crate::interpolation::PhysicsPose>(kart)
            .unwrap()
            .snap(placed);
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
    }
    room.until(3.0, "side by side", |app| {
        if lobby(app).me() != of {
            return true;
        }
        let sync = app.world().resource::<KartSync>();
        let proxy = sync.remote(beside).and_then(|r| r.proxy).unwrap();
        let shown = app.world().get::<Transform>(proxy).unwrap().translation;
        shown.distance(place) < 0.3
    });
}

/// Issue #63 acceptance (in memory): a paw strike from one client to another goes through the
/// host. Cyd (a client) sees Bob's kart (another client's, relayed by the host) beside his and
/// strikes: Bob's game decides (the victim, from Cyd's word relayed with its origin), pushes its
/// kart and answers; Cyd counts it landed, and every screen shows the "pof" on Bob's kart, the
/// host's and Dan's too. Bob and Cyd were never linked.
#[test]
fn a_strike_from_one_client_to_another_goes_through_the_host() {
    let mut room = seated(&FOUR);
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    for (_, app) in &mut room.games {
        let kart = player_of(app);
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    }
    put_beside(&mut room, CYD, BOB);
    for pressed in [true, true, false] {
        let app = room.app(CYD);
        let kart = player_of(app);
        app.world_mut().get_mut::<KartInput>(kart).unwrap().interact = pressed;
        room.frame();
    }
    room.until(1.5, "the answer and the pof", |app| {
        let me = lobby(app).me();
        let kart = player_of(app);
        match me {
            CYD => app
                .world()
                .get::<PawStats>(kart)
                .is_some_and(|s| s.landed == 1),
            BOB => app
                .world()
                .get::<PawStats>(kart)
                .is_some_and(|s| s.struck == 1),
            _ => {
                let sync = app.world().resource::<KartSync>();
                let proxy = sync.remote(BOB).and_then(|r| r.proxy).unwrap();
                app.world()
                    .get::<Paw>(proxy)
                    .is_some_and(|paw| paw.struck_count == 1)
            }
        }
    });
    let links = room.hub.links();
    assert!(!links.contains(&(BOB, CYD)) && !links.contains(&(CYD, BOB)));
}

/// Issue #63 acceptance (in memory): a race with items of four players (Ana hosts, her 2 bots
/// in front), all driven by bots, to the end: the same results and the same story of the items
/// on every screen (the boxes and the croquettes the host gave, the items thrown, the hits, the
/// croquettes lost), every client's events relayed by the host.
#[test]
fn a_race_with_items_in_a_star_ends_the_same_for_everyone() {
    let mut room = seated(&FOUR);
    let start = start_race(&mut room, ANA, 2, 1);
    bots_drive_the_players(&mut room);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    // A lap is short: each player starts with three abeilles (its bot brain throws them).
    for (key, _) in FOUR {
        let slot = start.slot_of(key).unwrap();
        let app = room.app(key);
        let world = app.world_mut();
        let mut karts = world.query::<(&NetKart, &mut ItemSlots)>();
        for (net, mut slots) in karts.iter_mut(world) {
            if net.0 == slot {
                slots.give(ItemKind::Abeille, 3, 0);
            }
        }
    }
    room.until(150.0, "the results", |app| {
        app.world().resource::<Race>().phase() == RacePhase::Results
    });
    for _ in 0..10 {
        room.frame();
    }
    let host_progress = progress_by_slot(room.app(ANA));
    let host_order = standings(room.app(ANA));
    assert_eq!(host_progress.len(), 6);
    for key in [BOB, CYD, DAN] {
        assert_same_progress(&host_progress, &progress_by_slot(room.app(key)));
        assert_eq!(standings(room.app(key)), host_order, "{key:?}");
        super::item_tests::assert_same_story(room.view(ANA), room.view(key));
    }
    let items = room.view(ANA).world().resource::<OnlineItems>();
    println!("{}", items.summary());
    assert!(items.log.thrown.len() >= 4, "{}", items.summary());
}

/// Issue #63: a second « host » opens the room (the host's address, with `host=1`, opened on
/// another machine): the host offers to it, it never takes an offer, so it is refused (« La
/// partie a déjà un hôte », the lobby window open) and leaves; nothing changes for the room.
/// It joins again as a client (« La rejoindre »): seated last.
#[test]
fn a_second_host_is_refused_and_changes_nothing() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    room.games.push((EVE, game(&room.hub, EVE, "Eve", true)));
    room.until(5.0, "Eve refused", |app| {
        let session = app.world().resource::<NetSession>();
        session
            .lobby
            .as_ref()
            .is_some_and(|lobby| lobby.me() != EVE)
            || session.status == NetStatus::HostTaken
    });
    for _ in 0..60 {
        room.frame();
    }
    let eve = room.view(EVE);
    assert!(eve.world().resource::<LobbyWindow>().open);
    assert!(eve.world().resource::<NetSession>().socket.is_none());
    assert!(
        eve.world().get_resource::<OnlineLevel>().is_none(),
        "never online"
    );
    for key in [ANA, BOB] {
        assert_eq!(lobby(room.view(key)).seats(), [ANA, BOB], "{key:?}");
    }
    assert_eq!(room.hub.links(), vec![(ANA, BOB)]);
    // « La rejoindre »: a client now (a new id from the signaling server).
    const EVE_AGAIN: PeerKey = PeerKey(6 << 100);
    let settings = room.view(EVE).world().resource::<NetSettings>().clone();
    let socket = NetSocket::memory(&room.hub, EVE_AGAIN, false);
    room.app(EVE).world_mut().insert_resource(NetSession::new(
        RoomCode::parse("TEST").unwrap(),
        settings.signal.clone(),
        settings.ice_servers(),
        socket,
        false,
    ));
    room.until(10.0, "Eve seated", |app| lobby(app).seats().len() == 3);
    assert_eq!(lobby(room.view(ANA)).seats(), [ANA, BOB, EVE_AGAIN]);
}

/// Issue #63: a client alone in a room without a host (its creator gone, or not there yet): once
/// `NO_HOST_MS` went by without an offer, the lobby says « Cette partie n'existe plus ». A host
/// that comes after it does not reach it (the signaling server tells the host of newcomers
/// only); joining again (« Rejoindre ») does. The host reloads its page (its tab closes, it
/// hosts the same room again): « L'hôte a quitté la partie » for the client, which joins again
/// and is seated with the reloaded host: a clean start.
#[test]
fn a_room_without_its_host_and_the_hosts_reload() {
    let hub = socket::memory::Hub::default();
    let mut room = Room {
        hub: hub.clone(),
        games: vec![(BOB, game(&hub, BOB, "Bob", false))],
    };
    let missing = |app: &App| {
        let now = now_ms(app.world().resource::<Time<Real>>());
        app.world().resource::<NetSession>().host_missing(now)
    };
    for _ in 0..60 {
        room.frame();
    }
    assert!(!missing(room.view(BOB)), "a moment for the host's offer");
    room.until(10.0, "no host", missing);
    // Ana opens the room as its host: Bob was there first, nobody connects them.
    room.games.push((ANA, game(&hub, ANA, "Ana", true)));
    for _ in 0..120 {
        room.frame();
    }
    assert!(missing(room.view(BOB)));
    assert_eq!(lobby(room.view(ANA)).seats(), [ANA]);
    // Bob joins again (a new id): seated.
    let rejoin = |room: &mut Room, key: PeerKey| {
        let settings = room.view(BOB).world().resource::<NetSettings>().clone();
        let socket = NetSocket::memory(&room.hub, key, false);
        room.app(BOB).world_mut().insert_resource(NetSession::new(
            RoomCode::parse("TEST").unwrap(),
            settings.signal.clone(),
            settings.ice_servers(),
            socket,
            false,
        ));
    };
    const BOB_AGAIN: PeerKey = PeerKey(7 << 100);
    rejoin(&mut room, BOB_AGAIN);
    room.until(10.0, "Bob seated", |app| lobby(app).seats().len() == 2);
    assert!(!missing(room.view(BOB)));
    // Ana reloads her page: her tab closes, she hosts the same room again (a new id).
    room.close(ANA);
    const ANA_AGAIN: PeerKey = PeerKey(8 << 100);
    room.games.push((ANA, game(&hub, ANA_AGAIN, "Ana", true)));
    room.until(5.0, "the host gone", |app| {
        let session = app.world().resource::<NetSession>();
        session.hosting || session.status == NetStatus::HostLeft
    });
    const BOB_THIRD: PeerKey = PeerKey(9 << 100);
    rejoin(&mut room, BOB_THIRD);
    room.until(10.0, "Bob seated with the reloaded host", |app| {
        lobby(app).seats() == [ANA_AGAIN, BOB_THIRD]
    });
}

/// The host's traffic in a room of `players` (Ana hosts), over `seconds` of free driving or of a
/// race with `bots` (the players driven by bots): bytes and packets a second, reliable and
/// unreliable, sent by the host and by each client.
fn measure(players: usize, race_bots: Option<u8>, seconds: f32) {
    let names = [
        "Ana", "Bob", "Cyd", "Dan", "Eve", "Fil", "Gus", "Hal", "Ivy", "Jo", "Kim", "Lou",
    ];
    let peers: Vec<(PeerKey, &str)> = (0..players)
        .map(|index| (PeerKey((index as u128 + 1) << 100), names[index]))
        .collect();
    let mut room = seated(&peers);
    if let Some(bots) = race_bots {
        start_race(&mut room, ANA, bots, 3);
        bots_drive_the_players(&mut room);
        room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    }
    let keys: Vec<PeerKey> = peers.iter().map(|(key, _)| *key).collect();
    let before: Vec<socket::memory::Traffic> = keys.iter().map(|k| room.hub.traffic(*k)).collect();
    let frames = (seconds * 60.0) as usize;
    for _ in 0..frames {
        room.frame();
    }
    let rate = |key: usize| {
        let (now, then) = (room.hub.traffic(keys[key]), before[key]);
        let per_s = |value: u64| value as f64 / f64::from(seconds);
        (
            per_s(now.bytes[1] - then.bytes[1]),
            per_s(now.packets[1] - then.packets[1]),
            per_s(now.bytes[0] - then.bytes[0]),
            per_s(now.packets[0] - then.packets[0]),
        )
    };
    let (host_bytes, host_packets, host_reliable, host_reliable_packets) = rate(0);
    let clients: Vec<(f64, f64, f64, f64)> = (1..players).map(rate).collect();
    let received: f64 = clients.iter().map(|c| c.0 + c.2).sum();
    let karts = players + usize::from(race_bots.unwrap_or(0));
    println!(
        "{players} players ({karts} karts{}): the host sends {:.1} kB/s unreliable in {:.0} packets/s ({:.0} bytes each), {:.2} kB/s reliable in {:.0} packets/s; it receives {:.1} kB/s; a client sends {:.2} kB/s, receives {:.1} kB/s",
        if race_bots.is_some() {
            ", a race"
        } else {
            ", free driving"
        },
        host_bytes / 1_000.0,
        host_packets,
        host_bytes / host_packets.max(1.0),
        host_reliable / 1_000.0,
        host_reliable_packets,
        received / 1_000.0,
        (clients[0].0 + clients[0].2) / 1_000.0,
        (host_bytes + host_reliable) / (players - 1) as f64 / 1_000.0,
    );
}

/// Issue #63: the host's bandwidth with 12 players (11 clients), in free driving, and with 4
/// players and 8 bots in a race (the players driven by bots, items and all). Measured by the
/// in-memory socket (every message's bytes, without the transport's own: about 50 more bytes
/// a packet for SCTP, DTLS, UDP and IP). Heavy (12 whole games): by hand,
/// `cargo test --release --lib net::star_tests::the_hosts_bandwidth -- --ignored --nocapture`.
#[test]
#[ignore = "12 whole games: by hand"]
fn the_hosts_bandwidth_with_twelve_players() {
    measure(12, None, 5.0);
    measure(4, Some(8), 5.0);
}
