//! Headless scenarios of the karts' colors online (issue #55): two whole games (`GamePlugin`) in
//! a room, linked by the in-memory socket of the tests (`socket::memory`, like `race_tests.rs`):
//! each player's color goes in its `Hello`, through its bytes. Each game sees the other's color,
//! on the kart and in the lobby; two players of the same color: both games paint them the same
//! (the first seat keeps it); the race's bots avoid the players' colors, the same on both
//! screens.
//!
//! The same with `matchbox_server` (real sockets, real time) is the ignored test at the end.

use std::time::{Duration, Instant};

use bevy::time::TimeUpdateStrategy;

use super::race_tests::{ANA, BOB, Room, lobby, on_grid, showing, start_race};
use super::*;
use crate::bots::Bot;
use crate::characters::{PlayerColor, SelectionAction};
use crate::kart::{BodyPaint, Kart, KartColor, KartPaint, PlayerKart};
use crate::race::Racer;
use crate::track::{TrackChoice, TrackLayout};

/// The color our kart wears.
fn own_paint(app: &mut App) -> Option<KartColor> {
    let world = app.world_mut();
    world
        .query_filtered::<&KartPaint, (With<PlayerKart>, With<Kart>)>()
        .iter(world)
        .next()
        .map(|paint| paint.0)
}

/// The color the kart of network id `kart` of another peer wears on our screen.
fn remote_paint(app: &mut App, kart: KartId) -> Option<KartColor> {
    let world = app.world_mut();
    world
        .query::<(&RemoteKartProxy, &KartPaint)>()
        .iter(world)
        .find(|(proxy, _)| proxy.kart == kart)
        .map(|(_, paint)| paint.0)
}

/// The colors of the bots we drive, by grid slot.
fn local_bots(app: &mut App) -> Vec<(usize, KartColor)> {
    let world = app.world_mut();
    let mut bots: Vec<(usize, KartColor)> = world
        .query_filtered::<(&Racer, &KartPaint), (With<Bot>, With<Kart>)>()
        .iter(world)
        .map(|(racer, paint)| (racer.grid_slot, paint.0))
        .collect();
    bots.sort_by_key(|(slot, _)| *slot);
    bots
}

/// Every kart's visual wears its kart's color (`kart::BodyPaint`), with that color's shared
/// material; how many visuals.
fn visuals_follow_their_karts(app: &mut App) -> usize {
    let world = app.world_mut();
    let assets = world.resource::<crate::kart::KartVisualAssets>().clone();
    let bodies: Vec<(BodyPaint, Handle<StandardMaterial>)> = world
        .query::<(&BodyPaint, &MeshMaterial3d<StandardMaterial>)>()
        .iter(world)
        .map(|(paint, material)| (*paint, material.0.clone()))
        .collect();
    for (body, material) in &bodies {
        let paint = world.get::<KartPaint>(body.kart).expect("a kart's paint");
        assert_eq!(body.shown, paint.0);
        assert_eq!(*material, assets.paint(paint.0));
    }
    bodies.len()
}

/// The network id of `peer`'s kart in free driving: its seat.
fn seat(app: &App, peer: PeerKey) -> KartId {
    lobby(app)
        .seats()
        .iter()
        .position(|key| *key == peer)
        .unwrap() as KartId
}

/// The lobby's notice about our color (`ui::color_notice`).
fn notice(app: &App) -> Option<String> {
    let session = app.world().resource::<NetSession>();
    let now = now_ms(app.world().resource::<Time<Real>>());
    super::ui::color_notice(&session.players(now))
}

/// Ana and Bob, each with the color it chose before joining, seated and driving freely, each
/// seeing the other's kart.
fn seated_in(ana: KartColor, bob: KartColor) -> Room {
    let mut room = Room::new(&[(ANA, "Ana"), (BOB, "Bob")]);
    room.app(ANA).insert_resource(PlayerColor(ana));
    room.app(BOB).insert_resource(PlayerColor(bob));
    room.until(15.0, "both seated, free driving", |app| {
        app.world().get_resource::<KartSync>().is_some_and(|sync| {
            sync.remotes().len() == 1 && sync.remotes().iter().all(|r| r.proxy.is_some())
        }) && showing(app) == Some(Showing::Free(0))
            && app.world().get_resource::<TrackLayout>().is_some()
    });
    room
}

/// Issue #55 acceptance: each game sees the color the other chose, on its kart and in the lobby;
/// then both choose red: the first seat keeps it, the other wears orange (the closest free
/// color), the same on both screens, and its lobby says so. In a race, the bots avoid the
/// players' colors: the host's bots and those the guest shows wear the same colors.
#[test]
fn two_games_see_each_others_colors_and_agree_on_a_conflict() {
    use KartColor::*;
    let mut room = seated_in(Red, Green);
    let (ana_kart, bob_kart) = (seat(room.view(ANA), ANA), seat(room.view(ANA), BOB));
    for _ in 0..5 {
        room.frame();
    }
    assert_eq!(own_paint(room.app(ANA)), Some(Red));
    assert_eq!(remote_paint(room.app(ANA), bob_kart), Some(Green));
    assert_eq!(own_paint(room.app(BOB)), Some(Green));
    assert_eq!(remote_paint(room.app(BOB), ana_kart), Some(Red));
    for key in [ANA, BOB] {
        let app = room.view(key);
        assert_eq!(lobby(app).color_of(ANA), Some(Red));
        assert_eq!(lobby(app).color_of(BOB), Some(Green));
        assert_eq!(notice(app), None);
    }
    assert_eq!(visuals_follow_their_karts(room.app(ANA)), 2);
    assert_eq!(visuals_follow_their_karts(room.app(BOB)), 2);

    // Bob picks red too, on his selection screen (free driving: his level restarts).
    for action in [
        SelectionAction::Open,
        SelectionAction::PickColor(Red),
        SelectionAction::Confirm,
    ] {
        room.app(BOB).world_mut().write_message(action);
    }
    let seats = lobby(room.view(ANA)).seats().to_vec();
    assert_eq!(seats, lobby(room.view(BOB)).seats());
    let (first, second) = (seats[0], seats[1]);
    let (first_kart, second_kart) = (seat(room.view(ANA), first), seat(room.view(ANA), second));
    let agreed = |app: &mut App| {
        let me = lobby(app).me();
        let (mine, other) = if me == first {
            (Red, (second_kart, Orange))
        } else {
            (Orange, (first_kart, Red))
        };
        own_paint(app) == Some(mine) && remote_paint(app, other.0) == Some(other.1)
    };
    let since = Instant::now();
    while !(agreed(room.app(ANA)) && agreed(room.app(BOB))) {
        assert!(since.elapsed() < Duration::from_secs(60), "never agreed");
        room.frame();
    }
    for key in [ANA, BOB] {
        let app = room.view(key);
        assert_eq!(lobby(app).color_of(first), Some(Red), "{key:?}");
        assert_eq!(lobby(app).color_of(second), Some(Orange), "{key:?}");
        let rows = app.world().resource::<NetSession>().players(0);
        let row = rows.iter().find(|row| row.key == second).unwrap();
        assert_eq!((row.chosen, row.color), (Some(Red), Some(Orange)));
    }
    assert_eq!(
        notice(room.view(second)).as_deref(),
        Some("Couleur déjà prise : tu es en orange au lieu de rouge.")
    );
    assert_eq!(notice(room.view(first)), None);
    assert_eq!(room.view(BOB).world().resource::<PlayerColor>().0, Red);
    room.frame();
    for key in [ANA, BOB] {
        assert_eq!(visuals_follow_their_karts(room.app(key)), 2);
    }

    // A race with 4 bots (red, orange, yellow, lime): the red and orange ones wear the closest
    // colors nobody wears (brown, pink), on the host's screen and on the guest's.
    let start = start_race(&mut room, ANA, 4, 1);
    assert_eq!(start.players, seats);
    let bots = [Brown, Pink, Yellow, Lime];
    room.until(3.0, "the bots shown", |app| {
        let sync = app.world().resource::<KartSync>();
        app.world()
            .resource::<NetSession>()
            .lobby
            .as_ref()
            .unwrap()
            .is_host()
            || (0..4).all(|kart| sync.remote_kart(kart).is_some_and(|r| r.proxy.is_some()))
    });
    room.frame();
    let host = room.app(ANA);
    assert!(lobby(host).is_host());
    assert_eq!(
        local_bots(host),
        bots.iter().copied().enumerate().collect::<Vec<_>>()
    );
    let guest = room.app(BOB);
    for (slot, color) in bots.iter().enumerate() {
        assert_eq!(
            remote_paint(guest, slot as KartId),
            Some(*color),
            "bot {slot}"
        );
    }
    // The players keep their colors in the race, on both screens.
    for key in [ANA, BOB] {
        let app = room.app(key);
        assert!(on_grid(app));
        let me = lobby(app).me();
        let other = if me == ANA { BOB } else { ANA };
        let worn = |peer| if peer == first { Red } else { Orange };
        assert_eq!(own_paint(app), Some(worn(me)), "{key:?}");
        let other_kart = start.slot_of(other).unwrap();
        assert_eq!(remote_paint(app, other_kart), Some(worn(other)), "{key:?}");
        // 6 karts, each visual in its kart's color.
        assert_eq!(visuals_follow_their_karts(app), 6, "{key:?}");
    }
}

/// Issue #55, for real: two whole native games (headless, real time) meet through
/// `matchbox_server` on this machine, both choose red: once seated, both paint the first seat in
/// red and the other in orange; in the race, the host's 10 bots and those the guest shows wear
/// the same colors, none a player's. Run it by hand, the server started:
/// `cargo test --lib net::paint_tests::two_games_agree -- --ignored --nocapture`.
#[test]
#[ignore = "needs matchbox_server on localhost:3536"]
fn two_games_agree_on_colors_through_a_local_signaling_server() {
    use KartColor::*;
    let code = RoomCode::generate(getrandom::u64().unwrap());
    println!("room {code}");
    let game = |pseudo: &str| {
        let mut app = crate::tests::game_app_unloaded(TrackChoice::TestGround, 0);
        app.insert_resource(TimeUpdateStrategy::Automatic);
        app.world_mut().resource_mut::<NetSettings>().pseudo = Some(pseudo.into());
        app.insert_resource(PlayerColor(Red));
        crate::tests::wait_for_the_player(&mut app);
        app
    };
    let mut games = [game("Ana"), game("Bob")];
    // Issue #63: Ana creates the room (she is in it), then Bob opens its link.
    games[0]
        .world_mut()
        .write_message(NetAction::Host(code.clone()));
    let since = Instant::now();
    while games[0]
        .world()
        .get_resource::<NetSession>()
        .map(|session| session.status.clone())
        != Some(NetStatus::InRoom)
    {
        assert!(
            since.elapsed() < Duration::from_secs(10),
            "Ana never got in"
        );
        games[0].update();
        std::thread::sleep(Duration::from_millis(5));
    }
    games[1]
        .world_mut()
        .write_message(NetAction::Join(code.clone()));
    let start = Instant::now();
    let wait = |games: &mut [App; 2], seconds: u64, what: &str, done: &dyn Fn(&mut App) -> bool| {
        let since = Instant::now();
        loop {
            if games.iter_mut().all(done) {
                break;
            }
            assert!(
                since.elapsed() < Duration::from_secs(seconds),
                "{what}: never happened"
            );
            for app in games.iter_mut() {
                app.update();
            }
            std::thread::sleep(Duration::from_millis(3));
        }
        println!("{what} after {:.1} s", start.elapsed().as_secs_f32());
    };
    wait(&mut games, 40, "both seated, seeing each other", &|app| {
        app.world()
            .get_resource::<KartSync>()
            .is_some_and(|sync| sync.remotes().iter().any(|r| r.proxy.is_some()))
    });
    let seats = lobby(&games[0]).seats().to_vec();
    println!(
        "seats {:?}",
        seats.iter().map(|k| k.tag()).collect::<Vec<_>>()
    );
    let first = seats[0];
    let painted = |app: &mut App| {
        let me = lobby(app).me();
        let (mine, theirs) = if me == first {
            (Red, Orange)
        } else {
            (Orange, Red)
        };
        // The other player's kart: its seat in free driving, its grid slot in a race.
        let peer = *seats.iter().find(|peer| **peer != me).unwrap();
        let other = match app.world().get_resource::<OnlineRace>() {
            Some(race) => race.shared.start.slot_of(peer).unwrap(),
            None => seat(app, peer),
        };
        own_paint(app) == Some(mine) && remote_paint(app, other) == Some(theirs)
    };
    wait(&mut games, 10, "the same colors on both screens", &painted);
    for app in &games {
        let lobby = lobby(app);
        println!(
            "#{}: first seat {:?}, second {:?}; notice: {:?}",
            lobby.me().tag(),
            lobby.color_of(seats[0]),
            lobby.color_of(seats[1]),
            notice(app)
        );
    }
    for app in &mut games {
        app.world_mut().write_message(NetAction::Ready(true));
    }
    wait(&mut games, 10, "everyone ready", &|app| {
        lobby(app).all_ready()
    });
    let host = games.iter().position(|app| lobby(app).is_host()).unwrap();
    games[host].world_mut().write_message(NetAction::StartRace);
    wait(&mut games, 30, "both on the grid", &|app| on_grid(app));
    let shown = |app: &mut App| {
        if lobby(app).is_host() {
            return local_bots(app).len() == 10;
        }
        (0..10).all(|kart| remote_paint(app, kart).is_some())
    };
    wait(&mut games, 10, "the 10 bots shown", &shown);
    for app in &mut games {
        app.update();
    }
    let guest = 1 - host;
    let of_host: Vec<KartColor> = local_bots(&mut games[host])
        .into_iter()
        .map(|(_, color)| color)
        .collect();
    let of_guest: Vec<KartColor> = (0..10)
        .map(|kart| remote_paint(&mut games[guest], kart).unwrap())
        .collect();
    println!("bots: host {of_host:?}, guest {of_guest:?}");
    assert_eq!(of_host, of_guest);
    assert!(!of_host.contains(&Red) && !of_host.contains(&Orange));
    // Issue #63: the other player's kart in the race, once its first state came (the bots' came
    // first, above).
    wait(&mut games, 10, "the players' colors in the race", &painted);
}
