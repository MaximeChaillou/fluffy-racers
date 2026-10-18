//! Headless scenarios of the lobby's banner and texts (issue #64): whole games in a room, linked
//! in memory (`race_tests`): the banner and the lobby's texts follow the room, host or client,
//! ready or not, in a race or not; the lobby opens on arrival and at the end of a race, closes
//! when our race starts.

use super::race_tests::{
    ANA, BOB, CYD, Room, bots_drive_the_players, game, lobby, on_grid, seated,
};
use super::*;
use crate::bots::BotDifficulty;
use crate::race::{Race, RacePhase};

const DAN: PeerKey = PeerKey(4 << 100);

/// The room as the lobby shows it to `app`'s player now.
fn facts(app: &App) -> RoomFacts {
    let world = app.world();
    RoomFacts::of(
        world.get_resource::<NetSession>(),
        world.get_resource::<OnlineRace>(),
        now_ms(world.resource::<Time<Real>>()),
    )
    .expect("in a room")
}

fn seated_facts(app: &App) -> Seated {
    match facts(app).stage {
        Stage::Seated(seated) => seated,
        stage => panic!("not seated: {stage:?}"),
    }
}

fn banner(app: &App) -> Option<String> {
    facts(app).banner().map(|banner| banner.to_string())
}

fn window(app: &App) -> LobbyWindow {
    *app.world().resource::<LobbyWindow>()
}

/// The host starts a race of one lap with `bots` bots, as « Lancer la course » does, once the
/// lobby lets it (the host itself not ready).
fn launch(room: &mut Room, bots: u8) -> RaceStart {
    let app = room.app(ANA);
    assert!(seated_facts(app).can_start());
    let now = now_ms(app.world().resource::<Time<Real>>());
    let mut session = app.world_mut().resource_mut::<NetSession>();
    let lobby = session.lobby.as_mut().unwrap();
    let mut start = race_to_start(
        lobby,
        now,
        0xbeef,
        1,
        BotDifficulty::Normal,
        &NetTuning::default(),
    )
    .expect("the host knows the room's clock");
    start.bots = bots;
    assert!(lobby.announce_race(start.clone()));
    start
}

/// Issue #64 acceptance (in memory, three games, then a fourth during the race): every seated
/// player has the banner and the lobby open; the host's says to choose the circuit and start,
/// and its « Lancer la course » says who it waits for (never the host itself); a client's says
/// « Prêt ! », then waits for the host by name. In the race, no banner and the lobby closed; a
/// player who comes during it is told so.
#[test]
fn the_banner_and_the_lobby_follow_the_room() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd")]);
    for (key, app) in &room.games {
        assert_eq!(
            window(app),
            LobbyWindow {
                open: true,
                banner: true
            },
            "{key:?}: seated, the lobby and the banner show"
        );
    }
    let ana = seated_facts(room.view(ANA));
    assert!(ana.hosting);
    assert_eq!(ana.players, 3);
    assert_eq!(ana.race, RaceStage::Free);
    assert_eq!(
        banner(room.view(ANA)).as_deref(),
        Some("Salon TEST · 3 joueurs · Tu es l'hôte : choisis le circuit puis lance la course")
    );
    // Its clients in its order of seats.
    let names: Vec<&str> = lobby(room.view(ANA))
        .seats()
        .iter()
        .skip(1)
        .map(|key| if *key == BOB { "Bob" } else { "Cyd" })
        .collect();
    assert_eq!(
        ana.start_blocker(),
        Some(format!("{} et {} ne sont pas prêts.", names[0], names[1]))
    );
    for key in [BOB, CYD] {
        let app = room.view(key);
        let client = seated_facts(app);
        assert!(!client.hosting);
        assert_eq!(client.host, "Ana", "{key:?}");
        assert_eq!(
            banner(app).as_deref(),
            Some("Salon TEST · 3 joueurs · Clique sur « Prêt ! »")
        );
        assert_eq!(
            client.meanwhile(),
            Some("En attendant la course, roule librement sur le circuit avec les autres.")
        );
    }

    // Bob is ready: Ana waits for Cyd; Bob for Ana.
    room.act(BOB, NetAction::Ready(true));
    room.until(2.0, "Bob ready everywhere", |app| {
        lobby(app)
            .peer(BOB)
            .and_then(|peer| peer.profile.as_ref())
            .is_none_or(|profile| profile.ready)
            && lobby(app).profile().ready == (lobby(app).me() == BOB)
    });
    assert_eq!(
        seated_facts(room.view(ANA)).start_blocker().as_deref(),
        Some("Cyd n'est pas prêt.")
    );
    assert_eq!(
        banner(room.view(BOB)).as_deref(),
        Some("Salon TEST · 3 joueurs · En attente de l'hôte (Ana)…")
    );
    assert_eq!(
        seated_facts(room.view(BOB)).client_hint(),
        "Tu es prêt ! En attente de l'hôte (Ana)…"
    );
    assert_eq!(
        banner(room.view(CYD)).as_deref(),
        Some("Salon TEST · 3 joueurs · Clique sur « Prêt ! »")
    );

    // Cyd too: the host can start, without being ready itself.
    room.act(CYD, NetAction::Ready(true));
    room.until(2.0, "the host can start", |app| {
        !lobby(app).is_host() || seated_facts(app).can_start()
    });
    assert!(!lobby(room.view(ANA)).profile().ready);
    assert!(!lobby(room.view(ANA)).all_ready(), "the host never said it");
    room.until(2.0, "Cyd ready everywhere", |app| {
        seated_facts(app).not_ready.is_empty()
    });
    assert_eq!(
        seated_facts(room.view(BOB)).client_hint(),
        "Tout le monde est prêt : l'hôte (Ana) peut lancer la course."
    );

    // The race: no banner, the lobby closed, for the three of them.
    let start = launch(&mut room, 2);
    room.until(10.0, "everyone on the grid", on_grid);
    room.until(1.0, "the banners gone", |app| !window(app).banner);
    for (key, app) in &room.games {
        assert_eq!(seated_facts(app).race, RaceStage::Racing, "{key:?}");
        assert_eq!(banner(app), None, "{key:?}");
        assert!(!window(app).open, "{key:?}: the lobby closed for the race");
    }

    // Dan comes during the race: he watches it, and the banner says so.
    let dan = game(&room.hub, DAN, "Dan", false);
    room.games.push((DAN, dan));
    room.until(15.0, "Dan seated", |app| {
        lobby(app).seats().len() == 4 && lobby(app).seated()
    });
    let app = room.view(DAN);
    assert_eq!(seated_facts(app).race, RaceStage::Watching);
    assert!(start.slot_of(DAN).is_none());
    assert_eq!(
        banner(app).as_deref(),
        Some("Salon TEST · 4 joueurs · Course en cours : tu seras de la prochaine")
    );
    room.until(1.0, "Dan's banner", |app| {
        lobby(app).me() != DAN
            || window(app)
                == LobbyWindow {
                    open: true,
                    banner: true,
                }
    });
    for key in [ANA, BOB, CYD] {
        assert!(!window(room.view(key)).banner, "{key:?}: still racing");
    }
    assert_eq!(
        seated_facts(room.view(ANA)).start_blocker().as_deref(),
        Some("Une course est en cours.")
    );
}

/// Issue #64: the lobby closes when our race starts and opens by itself at its end, with the
/// banner back: the host starts the next one from there. Leaving the room, no banner.
#[test]
fn the_lobby_opens_at_the_end_of_a_race() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    room.act(BOB, NetAction::Ready(true));
    room.until(2.0, "Bob ready", |app| {
        seated_facts(app).not_ready.is_empty()
    });
    launch(&mut room, 0);
    room.until(10.0, "everyone on the grid", on_grid);
    room.until(1.0, "the lobby closed", |app| {
        window(app)
            == LobbyWindow {
                open: false,
                banner: false,
            }
    });
    bots_drive_the_players(&mut room);
    room.until(150.0, "the results", |app| {
        app.world().resource::<Race>().phase() == RacePhase::Results
            && seated_facts(app).race == RaceStage::Over
    });
    room.until(1.0, "the lobby open", |app| {
        window(app)
            == LobbyWindow {
                open: true,
                banner: true,
            }
    });
    assert_eq!(
        banner(room.view(ANA)).as_deref(),
        Some("Salon TEST · 2 joueurs · Tu es l'hôte : choisis le circuit puis lance la course")
    );
    assert_eq!(
        banner(room.view(BOB)).as_deref(),
        Some("Salon TEST · 2 joueurs · En attente de l'hôte (Ana)…")
    );
    // Bob still ready: the next race can start.
    assert!(seated_facts(room.view(ANA)).can_start());

    // Bob leaves the room: no session, no banner.
    room.act(BOB, NetAction::Leave);
    let bob = room.app(BOB);
    for _ in 0..3 {
        bob.update();
    }
    assert!(bob.world().get_resource::<NetSession>().is_none());
    assert!(!window(bob).banner);
}
