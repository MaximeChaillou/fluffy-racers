//! Headless scenarios of the network (issue #7): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, and real sockets. The egui window is not there (no renderer): the keys and
//! the `NetAction`s its buttons write are.
//!
//! The CI has no signaling server: these tests only use one that cannot be reached. The meeting
//! of two peers through a real `matchbox_server` is the ignored test at the end (README, « Jouer
//! à plusieurs »).

use std::time::{Duration, Instant};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

use super::*;
use crate::options::OptionsAction;
use crate::tests::app_on;
use crate::track::TrackChoice;

/// A key pressed this frame (`just_pressed`), as the window sends it.
fn tap(app: &mut App, key_code: KeyCode) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

fn window_open(app: &App) -> bool {
    app.world().resource::<LobbyWindow>().open
}

fn act(app: &mut App, action: NetAction) {
    app.world_mut().write_message(action);
    app.update();
}

/// Updates `app` until `done` (real time: sockets work in the background), for `seconds` at
/// most. Returns whether it happened.
fn wait_for(app: &mut App, seconds: f32, mut done: impl FnMut(&App) -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs_f32(seconds) {
        app.update();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn status(app: &App) -> Option<NetStatus> {
    app.world()
        .get_resource::<NetSession>()
        .map(|s| s.status.clone())
}

/// Solo is unchanged: no room, no session, no socket; L opens and closes the lobby window (not
/// while the options menu has the keys), and it does not pause the game.
#[test]
fn solo_has_no_session_and_l_opens_the_lobby() {
    let mut app = app_on(TrackChoice::TestGround);
    for _ in 0..30 {
        app.update();
    }
    assert!(app.world().get_resource::<NetSession>().is_none());
    assert!(!window_open(&app));
    tap(&mut app, LOBBY_KEY);
    assert!(window_open(&app));
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    tap(&mut app, LOBBY_KEY);
    assert!(!window_open(&app));
    // The options menu takes the keys: L does nothing there.
    app.world_mut().write_message(OptionsAction::Toggle);
    app.update();
    tap(&mut app, LOBBY_KEY);
    assert!(!window_open(&app));
    assert!(app.world().get_resource::<NetSession>().is_none());
}

/// No signaling server: the room fails with its reason, the window says so, and leaving gets
/// back to solo.
#[test]
fn an_unreachable_signaling_server_fails_cleanly() {
    let mut app = app_on(TrackChoice::TestGround);
    {
        let mut settings = app.world_mut().resource_mut::<NetSettings>();
        // Nothing listens on port 9 here: refused at once. One try only.
        settings.signal = "ws://127.0.0.1:9".into();
        settings.attempts = 1;
    }
    let room = RoomCode::parse("TEST").unwrap();
    act(&mut app, NetAction::Join(room.clone()));
    assert!(window_open(&app), "joining opens the lobby");
    {
        let session = app.world().resource::<NetSession>();
        assert_eq!(session.room, room);
        assert_eq!(session.signal, "ws://127.0.0.1:9");
    }
    assert!(
        wait_for(&mut app, 20.0, |app| matches!(
            status(app),
            Some(NetStatus::Failed(_))
        )),
        "status {:?}",
        status(&app)
    );
    let Some(NetStatus::Failed(reason)) = status(&app) else {
        unreachable!()
    };
    assert!(!reason.is_empty());
    assert!(app.world().resource::<NetSession>().lobby.is_none());
    // The game ran on meanwhile.
    for _ in 0..10 {
        app.update();
    }
    act(&mut app, NetAction::Leave);
    assert!(app.world().get_resource::<NetSession>().is_none());
}

/// « Créer une partie »: a fresh readable code, joined at once, and a different one each time.
#[test]
fn creating_a_room_draws_a_code_and_joins_it() {
    let mut app = app_on(TrackChoice::TestGround);
    {
        let mut settings = app.world_mut().resource_mut::<NetSettings>();
        settings.signal = "ws://127.0.0.1:9".into();
        settings.attempts = 1;
    }
    let mut codes = Vec::new();
    for _ in 0..3 {
        act(&mut app, NetAction::CreateRoom);
        let session = app.world().resource::<NetSession>();
        let code = session.room.to_string();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|c| CODE_ALPHABET.contains(&c)), "{code}");
        assert!(matches!(
            session.status,
            NetStatus::Connecting | NetStatus::Failed(_)
        ));
        codes.push(code);
    }
    codes.dedup();
    assert_eq!(codes.len(), 3, "{codes:?}");
    assert!(window_open(&app));
    // Joining the room we are in changes nothing.
    let room = app.world().resource::<NetSession>().room.clone();
    act(&mut app, NetAction::Leave);
    act(&mut app, NetAction::Join(room.clone()));
    act(&mut app, NetAction::Join(room.clone()));
    assert_eq!(app.world().resource::<NetSession>().room, room);
}

/// An app with only the network, for the meeting test.
fn peer_app(pseudo: &str) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, NetPlugin));
    app.world_mut().resource_mut::<NetSettings>().pseudo = Some(pseudo.into());
    app
}

fn lobby_rows(app: &App) -> Vec<PlayerRow> {
    let now = now_ms(app.world().resource::<Time<Real>>());
    app.world()
        .get_resource::<NetSession>()
        .map(|session| session.players(now))
        .unwrap_or_default()
}

/// Issue #7, for real: two native peers meet through `matchbox_server` on this machine (its
/// default port, 3536), see each other with their ping, get « Prêt » across, and see the other
/// leave. Run it by hand, the server started:
/// `cargo test --lib net::scenario_tests::two_native_peers -- --ignored --nocapture`.
#[test]
#[ignore = "needs matchbox_server on localhost:3536"]
fn two_native_peers_meet_through_a_local_signaling_server() {
    let code = RoomCode::generate(getrandom::u64().unwrap());
    println!("room {code}");
    let (mut alice, mut bob) = (peer_app("Alice"), peer_app("Bob"));
    for app in [&mut alice, &mut bob] {
        act(app, NetAction::Join(code.clone()));
    }
    let start = Instant::now();
    let update_both = |alice: &mut App, bob: &mut App| {
        alice.update();
        bob.update();
        std::thread::sleep(Duration::from_millis(5));
    };
    let both_see_two = |app: &App| {
        let rows = lobby_rows(app);
        rows.iter().filter(|r| r.seated).count() == 2
            && rows.iter().all(|r| r.me || r.ping_ms.is_some())
    };
    while !(both_see_two(&alice) && both_see_two(&bob)) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "they never met: {:?} / {:?}",
            lobby_rows(&alice),
            lobby_rows(&bob)
        );
        update_both(&mut alice, &mut bob);
    }
    println!("met after {:.2} s", start.elapsed().as_secs_f32());
    for (app, other) in [(&alice, "Bob"), (&bob, "Alice")] {
        let rows = lobby_rows(app);
        println!("{rows:#?}");
        assert!(rows.iter().any(|r| !r.me && r.name == other));
        assert_eq!(rows.iter().filter(|r| r.host).count(), 1);
    }
    let host_of = |app: &App| {
        app.world()
            .resource::<NetSession>()
            .lobby
            .as_ref()
            .unwrap()
            .host()
    };
    assert_eq!(host_of(&alice), host_of(&bob));

    act(&mut alice, NetAction::Ready(true));
    let t = Instant::now();
    while !lobby_rows(&bob).iter().any(|r| !r.me && r.ready) {
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "« Prêt » never arrived"
        );
        update_both(&mut alice, &mut bob);
    }
    println!(
        "« Prêt » arrived in {:.0} ms",
        t.elapsed().as_secs_f32() * 1000.0
    );

    act(&mut alice, NetAction::Leave);
    let t = Instant::now();
    while lobby_rows(&bob).len() != 1 {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "the departure was not seen"
        );
        update_both(&mut alice, &mut bob);
    }
    println!(
        "departure seen in {:.0} ms",
        t.elapsed().as_secs_f32() * 1000.0
    );
    let rows = lobby_rows(&bob);
    assert!(rows[0].me && rows[0].host, "Bob hosts now");

    // A third one comes, then quits without a word (a closed tab): the signaling server tells.
    let mut carol = peer_app("Carol");
    act(&mut carol, NetAction::Join(code.clone()));
    let t = Instant::now();
    while lobby_rows(&bob).iter().filter(|r| r.seated).count() != 2
        || lobby_rows(&carol).iter().filter(|r| r.seated).count() != 2
    {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "Carol never sat down"
        );
        update_both(&mut bob, &mut carol);
    }
    println!(
        "Carol seated in {:.0} ms",
        t.elapsed().as_secs_f32() * 1000.0
    );
    drop(carol);
    let t = Instant::now();
    while lobby_rows(&bob).len() != 1 {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "the closed peer was not seen leaving"
        );
        bob.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    println!(
        "closed peer seen leaving in {:.0} ms",
        t.elapsed().as_secs_f32() * 1000.0
    );
}

/// A native peer that stays in a room and prints its lobby: to meet a browser (or another
/// machine) by hand. `FLUFFY_ROOM=CODE` (and `FLUFFY_SIGNAL`, `FLUFFY_SECONDS`), the server
/// started: `FLUFFY_ROOM=TEST cargo test --lib net::scenario_tests::stay_in_a_room -- --ignored
/// --nocapture`. It says « Prêt » after 10 s.
#[test]
#[ignore = "needs matchbox_server, and someone to meet"]
fn stay_in_a_room() {
    let room = std::env::var("FLUFFY_ROOM").unwrap_or_else(|_| "TEST".into());
    let seconds: u64 = std::env::var("FLUFFY_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let mut app = peer_app("Natif");
    if let Ok(signal) = std::env::var("FLUFFY_SIGNAL") {
        app.world_mut().resource_mut::<NetSettings>().signal = signal;
    }
    act(&mut app, NetAction::Join(RoomCode::parse(&room).unwrap()));
    let start = Instant::now();
    let mut next_print = 0;
    let mut ready = false;
    while start.elapsed() < Duration::from_secs(seconds) {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = start.elapsed().as_secs();
        if !ready && elapsed >= 10 {
            ready = true;
            act(&mut app, NetAction::Ready(true));
        }
        if elapsed >= next_print {
            next_print = elapsed + 2;
            let rows: Vec<String> = lobby_rows(&app)
                .iter()
                .map(|r| {
                    format!(
                        "{}{}{}{} ping {:?}{}",
                        r.name,
                        if r.me { " (moi)" } else { "" },
                        if r.host { " hôte" } else { "" },
                        if r.ready { " prêt" } else { "" },
                        r.ping_ms.map(|p| p.round()),
                        if r.seated { "" } else { " (sans place)" },
                    )
                })
                .collect();
            println!("t+{elapsed} s {:?}: {}", status(&app), rows.join(" | "));
        }
    }
}
