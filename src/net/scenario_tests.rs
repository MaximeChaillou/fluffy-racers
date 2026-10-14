//! Headless scenarios of the network (issue #7): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, and real sockets. The egui window is not there (no renderer): the keys and
//! the `NetAction`s its buttons write are.
//!
//! The CI has no signaling server: these tests only use one that cannot be reached. The meeting
//! of two peers through a real `matchbox_server` is the ignored test at the end (README, « Jouer
//! à plusieurs »).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use matchbox_socket::async_trait::async_trait;
use matchbox_socket::{
    PeerEvent, PeerRequest, PeerSignal, SignalingError, Signaller, SignallerBuilder,
};

use super::signaling::test_peer_id;
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

/// A signaling server in memory (issue #49): it says what the test puts in `events`, and keeps
/// what the socket sends. No other peer behind it: nobody ever answers.
#[derive(Clone, Debug, Default)]
struct FakeSignaling {
    events: Arc<Mutex<VecDeque<PeerEvent>>>,
    sent: Arc<Mutex<Vec<PeerRequest>>>,
}

#[async_trait]
impl SignallerBuilder for FakeSignaling {
    async fn new_signaller(
        &self,
        _attempts: Option<u16>,
        _room_url: String,
    ) -> Result<Box<dyn Signaller>, SignalingError> {
        Ok(Box::new(self.clone()))
    }
}

#[async_trait]
impl Signaller for FakeSignaling {
    async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
        self.sent.lock().unwrap().push(request);
        Ok(())
    }

    async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
        loop {
            let next = self.events.lock().unwrap().pop_front();
            if let Some(event) = next {
                return Ok(event);
            }
            futures_timer::Delay::new(Duration::from_millis(5)).await;
        }
    }
}

fn diagnosis(app: &App) -> Option<Diagnosis> {
    let now = now_ms(app.world().resource::<Time<Real>>());
    app.world().resource::<NetSession>().links.diagnosis(now)
}

/// Issue #49: the signaling server announces a newcomer whose connection never opens (its
/// browser never answers). We make the offer, natively with no ICE server at all (the default on a
/// local network), the lobby shows « connexion en cours » after a moment, and nothing more once
/// the server says it left.
#[test]
fn a_peer_announced_but_not_connected_shows_in_the_lobby() {
    let mut app = peer_app("Alice");
    let signaling = FakeSignaling::default();
    let (me, newcomer) = (test_peer_id(1 << 100), test_peer_id(2 << 100));
    signaling
        .events
        .lock()
        .unwrap()
        .extend([PeerEvent::IdAssigned(me), PeerEvent::NewPeer(newcomer)]);
    let settings = app.world().resource::<NetSettings>().clone();
    let ice = settings.ice_servers();
    assert!(
        ice.is_empty(),
        "no ICE server with a local signaling server"
    );
    let socket = NetSocket::open_with(
        "ws://localhost:3536/fluffy-racers-TEST",
        1,
        &ice,
        Arc::new(signaling.clone()),
    );
    app.world_mut().insert_resource(NetSession {
        room: RoomCode::parse("TEST").unwrap(),
        signal: settings.signal.clone(),
        ice,
        status: NetStatus::Connecting,
        lobby: None,
        links: Links::default(),
        socket: Some(socket),
        failures_seen: 0,
    });
    let newcomer_key = PeerKey(2 << 100);
    // Our id, then the newcomer announced and our offer sent (webrtc-rs made it without any ICE
    // server).
    assert!(
        wait_for(&mut app, 10.0, |app| {
            let session = app.world().resource::<NetSession>();
            session.status == NetStatus::InRoom
                && session
                    .links
                    .pending()
                    .first()
                    .is_some_and(|link| link.ours_ms.is_some())
        }),
        "no offer: {:?}",
        app.world().resource::<NetSession>().links.pending()
    );
    {
        let session = app.world().resource::<NetSession>();
        let link = &session.links.pending()[0];
        assert_eq!((link.peer, link.role), (newcomer_key, Role::Offerer));
        assert_eq!(link.theirs_ms, None);
        // Not a player: the lobby only lists us.
        assert_eq!(session.players(0).len(), 1);
    }
    let sent = signaling.sent.lock().unwrap();
    assert!(
        sent.iter().any(|request| matches!(
            request,
            PeerRequest::Signal { receiver, data: PeerSignal::Offer(sdp) }
                if *receiver == newcomer && sdp.starts_with("v=0")
        )),
        "{sent:?}"
    );
    drop(sent);
    // A moment later, the lobby says it.
    assert!(diagnosis(&app).is_none(), "{:?}", diagnosis(&app));
    assert!(
        wait_for(&mut app, 5.0, |app| diagnosis(app)
            == Some(Diagnosis::Connecting { peers: 1 })),
        "{:?}",
        diagnosis(&app)
    );
    // The server says it left: nothing more to show.
    signaling
        .events
        .lock()
        .unwrap()
        .push_back(PeerEvent::PeerLeft(newcomer));
    assert!(
        wait_for(&mut app, 5.0, |app| app
            .world()
            .resource::<NetSession>()
            .links
            .pending()
            .is_empty()),
        "still pending"
    );
    assert_eq!(diagnosis(&app), None);
    assert_eq!(status(&app), Some(NetStatus::InRoom));
}

/// Issue #8: seated in a room (alone here, after 2.5 s), the game switches to free driving on
/// Prairie Pompon, our kart on the grid slot of its seat, no countdown, no bot; its states go
/// out (stamped with the common clock: ours, we host) and its history fills. Leaving the room
/// brings back the level we were on, as it was.
#[test]
fn a_room_drives_freely_on_prairie_pompon_and_leaving_comes_back() {
    let mut app = app_on(TrackChoice::TestGround);
    let signaling = FakeSignaling::default();
    signaling
        .events
        .lock()
        .unwrap()
        .push_back(PeerEvent::IdAssigned(test_peer_id(1 << 100)));
    let settings = app.world().resource::<NetSettings>().clone();
    let ice = settings.ice_servers();
    let socket = NetSocket::open_with(
        "ws://localhost:3536/fluffy-racers-TEST",
        1,
        &ice,
        Arc::new(signaling.clone()),
    );
    app.world_mut().insert_resource(NetSession {
        room: RoomCode::parse("TEST").unwrap(),
        signal: settings.signal.clone(),
        ice,
        status: NetStatus::Connecting,
        lobby: None,
        links: Links::default(),
        socket: Some(socket),
        failures_seen: 0,
    });
    // Not seated yet: solo goes on.
    for _ in 0..10 {
        app.update();
    }
    assert!(app.world().get_resource::<KartSync>().is_none());
    let player_on_a_circuit = |app: &App| {
        app.world()
            .get_resource::<crate::track::TrackLayout>()
            .is_some()
            && app.world().get_resource::<OnlineSlot>().is_some()
            && app
                .world()
                .iter_entities()
                .any(|e| e.contains::<crate::kart::PlayerKart>())
    };
    assert!(
        wait_for(&mut app, 20.0, player_on_a_circuit),
        "never drove online"
    );
    assert_eq!(
        *app.world().resource::<TrackChoice>(),
        TrackChoice::default()
    );
    assert_eq!(
        *app.world().resource::<crate::race::RaceMode>(),
        crate::race::RaceMode::FreeRoam
    );
    assert_eq!(app.world().resource::<OnlineSlot>().0, 0);
    for _ in 0..30 {
        app.update();
    }
    assert!(app.world().get_resource::<crate::race::Race>().is_none());
    let mut bots = app
        .world_mut()
        .query_filtered::<(), With<crate::bots::Bot>>();
    assert_eq!(bots.iter(app.world()).count(), 0);
    let kart = crate::tests::player(&mut app);
    let pole = app.world().resource::<crate::track::TrackLayout>().grid[0];
    let at = app.world().get::<Transform>(kart).unwrap().translation;
    assert!(
        at.distance(pole.translation) < 1.0,
        "{at} vs {}",
        pole.translation
    );
    assert!(app.world().resource::<KartSync>().clock.is_set());
    assert!(app.world().get::<KartHistory>(kart).is_some());
    // Out of the room: back on the test ground, in a race, as before.
    act(&mut app, NetAction::Leave);
    assert!(
        wait_for(&mut app, 20.0, |app| {
            *app.world().resource::<TrackChoice>() == TrackChoice::TestGround
                && app.world().get_resource::<KartSync>().is_none()
                && app
                    .world()
                    .iter_entities()
                    .any(|e| e.contains::<crate::kart::PlayerKart>())
        }),
        "never came back"
    );
    assert_eq!(
        *app.world().resource::<crate::race::RaceMode>(),
        crate::race::RaceMode::Race
    );
    assert!(app.world().get_resource::<OnlineSlot>().is_none());
    assert!(app.world().get_resource::<OnlineLevel>().is_none());
}

/// An app with only the network, for the meeting test. `FLUFFY_STUN` (`none`, `auto`, a list of
/// servers) chooses the STUN servers: by default none with a local signaling server.
fn peer_app(pseudo: &str) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, NetPlugin));
    let mut settings = app.world_mut().resource_mut::<NetSettings>();
    settings.pseudo = Some(pseudo.into());
    if let Ok(stun) = std::env::var("FLUFFY_STUN") {
        settings.stun = StunChoice::parse(&stun).expect("FLUFFY_STUN");
    }
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

/// Issue #8, for real: two whole games (headless, real time) meet through `matchbox_server` on
/// this machine and drive freely on Prairie Pompon. Ana drives; Bob sees Ana's kart where she
/// drove it, compared tick by tick. `FLUFFY_NETSIM=120,20,5` puts the network simulator on Bob's
/// side. Run it by hand, the server started:
/// `cargo test --lib net::scenario_tests::two_games_drive -- --ignored --nocapture`.
#[test]
#[ignore = "needs matchbox_server on localhost:3536"]
fn two_games_drive_together_through_a_local_signaling_server() {
    use bevy::time::TimeUpdateStrategy;

    let code = RoomCode::generate(getrandom::u64().unwrap());
    println!("room {code}");
    let game = |pseudo: &str| {
        let mut app = crate::tests::game_app_unloaded(TrackChoice::TestGround, 0);
        // Real time: the lobby's clock and the game's go together, as in the browser.
        app.insert_resource(TimeUpdateStrategy::Automatic);
        app.world_mut().resource_mut::<NetSettings>().pseudo = Some(pseudo.into());
        if let Ok(stun) = std::env::var("FLUFFY_STUN") {
            app.world_mut().resource_mut::<NetSettings>().stun =
                StunChoice::parse(&stun).expect("FLUFFY_STUN");
        }
        crate::tests::wait_for_the_player(&mut app);
        app
    };
    let (mut ana, mut bob) = (game("Ana"), game("Bob"));
    if let Ok(netsim) = std::env::var("FLUFFY_NETSIM") {
        let netsim = NetSimTuning::parse(&netsim).expect("FLUFFY_NETSIM: 120,20,5");
        println!("Bob's network simulator: {netsim:?}");
        bob.world_mut().resource_mut::<NetTuning>().simulator = netsim;
    }
    for app in [&mut ana, &mut bob] {
        act(app, NetAction::Join(code.clone()));
    }
    let start = Instant::now();
    let update_both = |ana: &mut App, bob: &mut App| {
        ana.update();
        bob.update();
        std::thread::sleep(Duration::from_millis(4));
    };
    // Both seated, on Prairie Pompon, each seeing the other's kart.
    let sees_the_other = |app: &App| {
        app.world()
            .get_resource::<KartSync>()
            .is_some_and(|sync| sync.remotes().iter().any(|r| r.proxy.is_some()))
    };
    while !(sees_the_other(&ana) && sees_the_other(&bob)) {
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "they never saw each other's karts: {:?} / {:?}",
            lobby_rows(&ana),
            lobby_rows(&bob)
        );
        update_both(&mut ana, &mut bob);
    }
    println!(
        "each sees the other's kart after {:.2} s",
        start.elapsed().as_secs_f32()
    );
    let kart = crate::tests::player(&mut ana);
    let ana_key = ana.world().resource::<KartSync>().me();
    let from = ana.world().get::<Transform>(kart).unwrap().translation;
    // Each on the grid slot of its seat: not on top of each other.
    let bob_kart = crate::tests::player(&mut bob);
    let bob_from = bob.world().get::<Transform>(bob_kart).unwrap().translation;
    println!(
        "Ana starts at {from:.1} (slot {}), Bob at {bob_from:.1} (slot {})",
        ana.world().resource::<OnlineSlot>().0,
        bob.world().resource::<OnlineSlot>().0
    );
    assert!(from.distance(bob_from) > 2.0, "on top of each other");
    // Ana's kart by common tick (from its history), Bob's view of it frame by frame.
    let mut truth: std::collections::BTreeMap<u32, Vec3> = Default::default();
    let mut views: Vec<(f64, Vec3)> = Vec::new();
    // How far behind the common clock Bob shows Ana's kart (ms).
    let mut delays: Vec<f64> = Vec::new();
    let drive = Instant::now();
    let mut next_print = 0.0;
    while drive.elapsed() < Duration::from_secs(7) {
        {
            let mut keys = ana.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ArrowUp);
            let t = drive.elapsed().as_secs_f32();
            if (3.0..3.4).contains(&t) {
                keys.press(KeyCode::ArrowLeft);
            } else {
                keys.release(KeyCode::ArrowLeft);
            }
        }
        update_both(&mut ana, &mut bob);
        if let Some(history) = ana.world().get::<KartHistory>(kart)
            && let Some(newest) = history.0.newest_tick()
        {
            for tick in newest.saturating_sub(20)..=newest {
                if let Some(position) = history.0.position_at(tick) {
                    truth.entry(tick).or_insert(position);
                }
            }
        }
        let now = bob.world().resource::<Time<Virtual>>().elapsed_secs_f64() * 1_000.0
            - 1_000.0 / crate::TICK_HZ;
        let sync = bob.world().resource::<KartSync>();
        let Some(remote) = sync.remote(ana_key) else {
            continue;
        };
        if let (Some(tick), Some(visual)) = (remote.shown_tick(now), remote.visual) {
            let position = bob.world().get::<Transform>(visual).unwrap().translation;
            views.push((tick, position));
            let local = now / TICK_MS;
            if let Some(common) = sync.clock.local(0.0).map(|zero| local - zero) {
                delays.push((common - tick) * TICK_MS);
            }
        }
        let t = drive.elapsed().as_secs_f32();
        if t >= next_print {
            next_print += 0.5;
            let ana_at = ana.world().get::<Transform>(kart).unwrap().translation;
            let shown = views.last().map(|v| v.1).unwrap_or(Vec3::NAN);
            println!(
                "t+{t:.1} s  Ana drives at {ana_at:.1}  Bob shows her at {shown:.1} ({:.1} m behind)  states {}",
                ana_at.distance(shown),
                remote.received
            );
        }
    }
    let truth_at = |tick: f64| {
        let (a, b) = (tick.floor() as u32, tick.ceil() as u32);
        Some(truth.get(&a)?.lerp(*truth.get(&b)?, tick.fract() as f32))
    };
    let errors: Vec<f32> = views
        .iter()
        .filter_map(|(tick, position)| Some(truth_at(*tick)?.distance(*position)))
        .collect();
    let driven = truth.values().last().unwrap().distance(from);
    let worst = errors.iter().copied().fold(0.0f32, f32::max);
    let mean = errors.iter().sum::<f32>() / errors.len().max(1) as f32;
    let sync = bob.world().resource::<KartSync>();
    let remote = sync.remote(ana_key).unwrap();
    let delay = delays.iter().sum::<f64>() / delays.len().max(1) as f64;
    println!(
        "Ana drove {driven:.1} m; Bob compared {} frames: error mean {mean:.3} m, worst {worst:.3} m; {} states received, {:?} (received, lost) by the simulator; Bob shows Ana's kart {delay:.0} ms after her tick, on average",
        errors.len(),
        remote.received,
        sync.simulated_losses(),
    );
    assert!(driven > 50.0, "Ana barely drove: {driven} m");
    assert!(errors.len() > 100, "{} frames compared", errors.len());
    assert!(
        worst < 1.0,
        "Bob showed Ana's kart {worst} m away from where she was"
    );
}
