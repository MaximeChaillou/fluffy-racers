//! Issue #225: headless scenarios of the dedicated server: a server (the game without a player,
//! `dedicated.rs`) and whole games of players, linked by the in-memory socket of the tests
//! (`socket::memory`): every message goes through its bytes. One frame is one tick for every
//! game, in turn; each scenario waits (`Room::until`) for exactly what it checks.

use std::time::{Duration, Instant};

use bevy::time::TimeUpdateStrategy;

use super::dedicated::{DedicatedPlugin, DedicatedServer, ServerOptions, server_session};
use super::race_tests::{ANA, BOB, CYD, Room, game, lobby};
use super::socket::memory::Hub;
use super::*;
use crate::bots::{Bot, BotDifficulty, BotTuning, bot, bot_seed};
use crate::kart::{Kart, PlayerKart};
use crate::race::{Race, RaceGrid, RaceTuning, Racer};
use crate::track::TrackChoice;

const SERVER: PeerKey = PeerKey(9 << 100);
const EVE: PeerKey = PeerKey(5 << 100);

/// The dedicated server's game (as `crate::server_app` builds it, a tick per frame), in the
/// room of `hub`: short races (one lap, the end 3 s after the first finish, the results closed
/// 1 s after they showed) and short waits between them.
fn server(hub: &Hub) -> App {
    let mut app = crate::tests::game_app_unloaded(TrackChoice::Salon, 0);
    let server = DedicatedServer::new(server_room(), "ws://127.0.0.1:9".to_string());
    app.insert_resource(server.clone())
        .add_plugins(DedicatedPlugin);
    crate::tests::single_threaded(&mut app);
    let ice = IceServers::choose(&StunChoice::Off, None, "ws://127.0.0.1:9");
    app.insert_resource(server_session(
        &server,
        ice,
        NetSocket::memory(hub, SERVER, true),
    ));
    let world = app.world_mut();
    let mut race = world.resource_mut::<RaceTuning>();
    race.laps = 1;
    race.results_close_seconds = 1.0;
    let mut tuning = world.resource_mut::<NetTuning>();
    tuning.finish_grace_s = 3.0;
    tuning.server_results_s = 1.0;
    tuning.server_standings_s = 1.0;
    tuning.server_podium_s = 1.0;
    tuning.server_idle_frame_ms = 0.0;
    app
}

/// A player's game comes to the server's room: to create a game (« Créer une partie »), or to
/// join one (an invitation).
fn player_comes(room: &mut Room, key: PeerKey, name: &str, intent: Intent) {
    let mut app = game(&room.hub, key, name, false);
    let mut session = app.world_mut().resource_mut::<NetSession>();
    session.room = server_room();
    session.intent = intent;
    room.games.push((key, app));
}

/// The server, alone in its room, waiting.
fn waiting_server() -> Room {
    let hub = Hub::default();
    let app = server(&hub);
    let mut room = Room {
        hub,
        games: vec![(SERVER, app)],
    };
    room.until(5.0, "the server in its room", |app| {
        app.world()
            .resource::<NetSession>()
            .lobby
            .as_ref()
            .is_some_and(Lobby::is_dedicated)
            && app.world().get_resource::<KartSync>().is_some()
    });
    room
}

fn is_server(app: &App) -> bool {
    app.world().get_resource::<DedicatedServer>().is_some()
}

/// The race of `round` (0: a race alone) is on: the server built it, a player is on its grid.
fn racing(app: &App, round: u8) -> bool {
    let world = app.world();
    let race = world.get_resource::<OnlineRace>();
    let built = world.get_resource::<Race>().is_some()
        && race.is_some_and(|race| race.shared.start.round == round);
    if is_server(app) {
        built
    } else {
        built
            && world
                .iter_entities()
                .any(|e| e.contains::<PlayerKart>() && e.contains::<Racer>())
    }
}

/// The players' karts driven by bots (the server has none: its bots drive already).
fn bots_drive_the_players(room: &mut Room) {
    for (_, app) in &mut room.games {
        if is_server(app) {
            continue;
        }
        let world = app.world_mut();
        let kart = world
            .query_filtered::<Entity, (With<Kart>, With<PlayerKart>)>()
            .single(world)
            .expect("our kart");
        let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
        let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
        let tuning = app.world().resource::<BotTuning>().clone();
        app.world_mut()
            .entity_mut(kart)
            .remove::<PlayerKart>()
            .insert(bot(BotDifficulty::Normal, seed, &tuning));
    }
}

/// The closed results of the race on, in `app`.
fn results(app: &App) -> Option<RaceResults> {
    app.world()
        .get_resource::<OnlineRace>()
        .and_then(|race| race.shared.results().cloned())
}

fn server_lobby(room: &Room) -> &Lobby {
    lobby(room.view(SERVER))
}

/// Issue #225 acceptance: the server is on, alone. Ana comes to create a game: she is its
/// leader, and chooses a championship (two races, players only). Bob comes to create one too: the
/// game shows him the one on (a championship, one player), and he joins it. Ana starts: the
/// server runs both races and the podium, everyone sees the same results and the same
/// standings, then everyone is back in the lobby. Everyone leaves: the server forgets the game
/// and waits, nothing built.
#[test]
fn a_player_creates_a_championship_on_the_server_and_another_joins_it() {
    let mut room = waiting_server();
    // The server has no kart, no seat.
    assert!(server_lobby(&room).seats().is_empty());
    player_comes(&mut room, ANA, "Ana", Intent::Create);
    room.until(15.0, "Ana seated, leading the game", |app| {
        is_server(app) || (lobby(app).seated() && lobby(app).leads())
    });
    assert_eq!(server_lobby(&room).seats(), [ANA]);
    assert!(
        room.view(SERVER)
            .world()
            .iter_entities()
            .all(|e| !e.contains::<PlayerKart>()),
        "the server drives no kart of its own"
    );
    // Her choice: a championship of two races, without bots.
    let setup = RoomSetup {
        championship: true,
        bots: false,
        plan: ChampionshipSetup {
            custom: true,
            chosen: true,
            program: Program::new(&[0, 0]),
            ..ChampionshipSetup::default()
        },
        ..RoomSetup::default()
    };
    room.act(ANA, NetAction::ChooseSetup(setup));
    room.until(3.0, "the server has Ana's settings", |app| {
        !is_server(app) || lobby(app).setup() == setup
    });
    // Bob wants to create a game: one is on, he is told what it is.
    player_comes(&mut room, BOB, "Bob", Intent::Create);
    room.until(15.0, "Bob told of the game on", |app| {
        is_server(app)
            || lobby(app).leads()
            || matches!(
                RoomFacts::of(app.world().get_resource(), None, 0).map(|facts| facts.stage),
                Some(Stage::GameOn(info)) if info.players == 1 && info.setup == setup
            )
    });
    assert_eq!(server_lobby(&room).seats(), [ANA], "Bob has no seat yet");
    // « Rejoindre ».
    room.act(BOB, NetAction::JoinGame);
    room.until(5.0, "Bob in the game", |app| {
        lobby(app).seats() == [ANA, BOB] && (is_server(app) || lobby(app).setup() == setup)
    });
    assert!(!lobby(room.view(BOB)).leads());
    assert_eq!(lobby(room.view(BOB)).leader(), Some(ANA));
    // Bob is ready, Ana starts: the championship's first race.
    room.act(BOB, NetAction::Ready(true));
    room.until(3.0, "Bob ready", |app| {
        !is_server(app) || lobby(app).all_ready()
    });
    room.act(ANA, NetAction::StartRace);
    for round in [1, 2] {
        room.until(15.0, "everyone on the grid", |app| racing(app, round));
        bots_drive_the_players(&mut room);
        room.until(
            150.0,
            "the race scored, the same results everywhere",
            |app| {
                results(app).is_some()
                    && lobby(app)
                        .championship()
                        .is_some_and(|state| state.scored == round)
            },
        );
        let table = results(room.view(SERVER)).unwrap();
        assert_eq!(table.lines.len(), 2, "two players, no bot: {table:?}");
        for key in [ANA, BOB] {
            assert_eq!(results(room.view(key)).as_ref(), Some(&table), "{key:?}");
            assert_eq!(
                lobby(room.view(key)).championship(),
                server_lobby(&room).championship()
            );
        }
    }
    room.until(10.0, "the podium, then everyone back in the lobby", |app| {
        lobby(app).announced().is_none() && lobby(app).championship().is_none()
    });
    // Everyone leaves: the server waits, its game gone.
    room.close(ANA);
    room.close(BOB);
    room.until(10.0, "the server waits", |app| {
        let lobby = lobby(app);
        lobby.seats().is_empty()
            && !lobby.server_info().game()
            && lobby.setup() == RoomSetup::default()
            && app.world().get_resource::<OnlineRace>().is_none()
            && app.world().get_resource::<Race>().is_none()
    });
}

/// Issue #225 acceptance: Ana created the game, Bob and Cyd came from her invitation. Ana leaves:
/// Bob leads (for everyone). He chooses a race with bots and starts it: the server drives the
/// bots (10 on its grid), the players see them, and everyone ends the race with the same results;
/// then everyone is back in the lobby. Eve comes to create a game meanwhile: told of it.
#[test]
fn the_leader_leaves_and_the_next_player_leads_a_race_with_the_servers_bots() {
    let mut room = waiting_server();
    player_comes(&mut room, ANA, "Ana", Intent::Create);
    room.until(15.0, "Ana seated", |app| {
        is_server(app) || lobby(app).seated()
    });
    player_comes(&mut room, BOB, "Bob", Intent::Join);
    player_comes(&mut room, CYD, "Cyd", Intent::Join);
    room.until(15.0, "everyone seated", |app| {
        lobby(app).seats() == [ANA, BOB, CYD]
    });
    room.close(ANA);
    room.until(5.0, "Bob leads", |app| {
        lobby(app).seats() == [BOB, CYD] && lobby(app).leader() == Some(BOB)
    });
    assert!(lobby(room.view(BOB)).leads());
    assert!(!lobby(room.view(CYD)).leads());
    // Only the leader's choices count.
    let ignored = RoomSetup {
        track: 1,
        ..RoomSetup::default()
    };
    room.act(CYD, NetAction::ChooseSetup(ignored));
    let setup = RoomSetup {
        mirror: true,
        ..RoomSetup::default()
    };
    room.act(BOB, NetAction::ChooseSetup(setup));
    room.until(3.0, "Bob's settings everywhere", |app| {
        lobby(app).setup() == setup
    });
    room.act(CYD, NetAction::Ready(true));
    room.until(3.0, "Cyd ready", |app| {
        !is_server(app) || lobby(app).all_ready()
    });
    // Cyd cannot start: nothing happens.
    room.act(CYD, NetAction::StartRace);
    for _ in 0..30 {
        room.frame();
    }
    assert!(server_lobby(&room).announced().is_none());
    room.act(BOB, NetAction::StartRace);
    room.until(15.0, "everyone on the grid", |app| racing(app, 0));
    let start = server_lobby(&room).announced().cloned().unwrap();
    assert_eq!(
        (start.players.as_slice(), start.bots),
        ([BOB, CYD].as_slice(), 10)
    );
    assert!(start.mirror);
    let bots = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<(), (With<Kart>, With<Bot>)>()
            .iter(world)
            .count()
    };
    room.until(10.0, "the server's bots, seen by everyone", |app| {
        app.world().get_resource::<KartSync>().is_some_and(|sync| {
            let shown = sync.remotes().iter().filter(|r| r.proxy.is_some()).count();
            // The server shows the 2 players; a player, the other one and the 10 bots.
            shown == if is_server(app) { 2 } else { 11 }
        })
    });
    assert_eq!(bots(room.app(SERVER)), 10, "the server drives the bots");
    assert_eq!(bots(room.app(BOB)), 0, "a player does not");
    bots_drive_the_players(&mut room);
    // Eve comes to create a game meanwhile: told of the race on.
    player_comes(&mut room, EVE, "Eve", Intent::Create);
    room.until(15.0, "Eve told of the race on", |app| {
        if app
            .world()
            .resource::<NetSession>()
            .lobby
            .as_ref()
            .is_none()
        {
            return false;
        }
        let lobby = lobby(app);
        lobby.me() != EVE || lobby.deciding().is_some_and(|info| info.race == start.race)
    });
    room.close(EVE);
    room.until(150.0, "the same results everywhere", |app| {
        results(app).is_some()
    });
    let table = results(room.view(SERVER)).unwrap();
    assert_eq!(table.lines.len(), 12);
    for key in [BOB, CYD] {
        assert_eq!(results(room.view(key)).as_ref(), Some(&table), "{key:?}");
    }
    room.until(10.0, "everyone back in the lobby", |app| {
        lobby(app).announced().is_none()
    });
}

/// Issue #225: the server's command line: its signaling's port, its room, another signaling,
/// STUN; what it cannot read is refused, for a message.
#[test]
fn the_servers_command_line() {
    let args = |list: &[&str]| ServerOptions::from_args(list.iter().map(|a| a.to_string()));
    let default = args(&[]);
    assert_eq!(default.port, SIGNAL_PORT);
    assert_eq!(default.room.as_str(), SERVER_ROOM);
    assert_eq!(default.signal, None);
    assert_eq!(default.signal_url(3536), "ws://127.0.0.1:3536");
    assert!(default.refused.is_empty() && !default.help);
    let options = args(&["--port", "4000", "--room=Salon2", "--stun", "none"]);
    assert_eq!(options.port, 4000);
    assert_eq!(options.room.as_str(), "SALON2");
    assert_eq!(options.stun, StunChoice::Off);
    let options = args(&["--signal", "ws://10.0.0.2:3536/"]);
    assert_eq!(options.signal_url(3536), "ws://10.0.0.2:3536");
    assert!(args(&["--help"]).help);
    assert_eq!(args(&["--port", "x"]).refused, ["--port x"]);
    assert_eq!(args(&["--what", "1"]).refused, ["--what 1"]);
    assert_eq!(args(&["seul"]).refused, ["seul"]);
    assert_eq!(args(&["--port"]).refused, ["--port"]);
}

/// Issue #225, for real (native WebRTC on this machine): the signaling the server embeds
/// (`signal_server.rs`, a free port), a dedicated session in its room, and a player who comes to
/// create a game: they meet, the player takes the first seat and leads; a second one, from an
/// invitation, joins. Nothing else runs beside them.
#[test]
fn players_meet_the_server_through_its_own_signaling() {
    let addr =
        super::signal_server::start_signaling(([127, 0, 0, 1], 0).into()).expect("a free port");
    let signal = format!("ws://127.0.0.1:{}", addr.port());
    let peer = |name: &str| {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, NetPlugin))
            .insert_resource(TimeUpdateStrategy::Automatic);
        let mut settings = app.world_mut().resource_mut::<NetSettings>();
        settings.pseudo = Some(name.into());
        settings.signal = signal.clone();
        settings.server = Some(signal.clone());
        app
    };
    let mut server = peer("Serveur");
    let host = DedicatedServer::new(server_room(), signal.clone());
    let ice = IceServers::choose(&StunChoice::Off, None, &signal);
    let socket = NetSocket::open(&room_url(&signal, &server_room()), 3, &ice, true);
    server.insert_resource(server_session(&host, ice, socket));
    let (mut ana, mut bob) = (peer("Ana"), peer("Bob"));
    let in_room = |app: &App| {
        app.world()
            .get_resource::<NetSession>()
            .and_then(|session| session.lobby.as_ref())
            .is_some()
    };
    let start = Instant::now();
    while !in_room(&server) {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "the server never got in"
        );
        server.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    ana.world_mut().write_message(NetAction::CreateRoom);
    let seats = |app: &App| -> Vec<PeerKey> {
        app.world()
            .get_resource::<NetSession>()
            .and_then(|session| session.lobby.as_ref())
            .map_or(Vec::new(), |lobby| lobby.seats().to_vec())
    };
    let mut joined = false;
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "they never met: {:?} / {:?} / {:?}",
            seats(&server),
            seats(&ana),
            seats(&bob)
        );
        for app in [&mut server, &mut ana, &mut bob] {
            app.update();
        }
        std::thread::sleep(Duration::from_millis(5));
        let ana_in = seats(&ana).len() == 1;
        if ana_in && !joined {
            // Ana's invitation: the server's room, through its signaling.
            bob.world_mut()
                .resource_mut::<NetSettings>()
                .signal
                .clone_from(&signal);
            bob.world_mut()
                .write_message(NetAction::Join(server_room()));
            joined = true;
        }
        if joined
            && [&server, &ana, &bob]
                .iter()
                .all(|app| seats(app).len() == 2)
        {
            break;
        }
    }
    let ana_lobby = ana.world().resource::<NetSession>().lobby.as_ref().unwrap();
    assert!(ana_lobby.leads() && ana_lobby.on_server());
    let bob_lobby = bob.world().resource::<NetSession>().lobby.as_ref().unwrap();
    assert!(!bob_lobby.leads());
    assert_eq!(bob_lobby.leader(), Some(ana_lobby.me()));
    assert_eq!(seats(&server), seats(&bob));
    println!("met in {:.2} s", start.elapsed().as_secs_f32());
}

/// The resident memory of this process (kB), from `/proc` (Linux).
fn resident_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("VmRSS:"))
                .and_then(|kb| kb.trim().trim_end_matches("kB").trim().parse().ok())
        })
        .unwrap_or(0)
}

/// The server's time per frame (one tick each) over `frames` frames of `room`, in ms: the mean
/// and the worst.
fn server_cost(room: &mut Room, frames: usize) -> (f64, f64) {
    let (mut total, mut worst) = (0.0f64, 0.0f64);
    for _ in 0..frames {
        for (key, app) in &mut room.games {
            let start = Instant::now();
            app.update();
            if *key == SERVER {
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                total += ms;
                worst = worst.max(ms);
            }
        }
    }
    (total / frames as f64, worst)
}

/// Issue #225, the measures of `STATUS.md` (« Métriques »), run by hand (release-like numbers
/// need `--release`):
/// `cargo test --lib net::server_tests::measure -- --ignored --nocapture --test-threads 1`.
/// 1. The server with one player and 11 bots (its heaviest grid: nearly every kart simulated
///    there): its time per tick. 2. `FLUFFY_PLAYERS` players (4 by default) and
///    the bots that fill the grid: its time per tick, and what it sends and receives.
#[test]
#[ignore = "a measure, run by hand"]
fn measure_the_server() {
    let before = resident_kb();
    let mut room = waiting_server();
    let idle = resident_kb();
    let (idle_ms, _) = server_cost(&mut room, 600);
    println!(
        "idle: {idle_ms:.3} ms per tick; memory {} MB for the server's game",
        (idle - before) / 1024
    );
    // 1. One player (the server forgets a game without any) and 11 bots, announced by hand
    // (one player cannot start a race): every kart but one simulated by the server.
    player_comes(&mut room, ANA, "Ana", Intent::Create);
    room.until(15.0, "Ana seated", |app| lobby(app).seats() == [ANA]);
    {
        let app = room.app(SERVER);
        let now = now_ms(app.world().resource::<Time<Real>>());
        let mut session = app.world_mut().resource_mut::<NetSession>();
        let lobby = session.lobby.as_mut().unwrap();
        let mut start = race_to_start(lobby, now, 7, 1, &NetTuning::default()).unwrap();
        start.bots = 11;
        assert!(lobby.announce_race(start));
    }
    room.until(30.0, "the race built", |app| racing(app, 0));
    bots_drive_the_players(&mut room);
    let (ms, worst) = server_cost(&mut room, 60 * 30);
    println!(
        "1 player and 11 bots: {ms:.3} ms per tick (worst {worst:.2} ms), {:.1} % of a core at 60 Hz; memory {} MB (the player's game too)",
        ms * 60.0 / 10.0,
        (resident_kb() - before) / 1024
    );
    drop(room);
    // 2. Players and bots.
    let players: usize = std::env::var("FLUFFY_PLAYERS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(4);
    let mut room = waiting_server();
    let keys: Vec<PeerKey> = (0..players)
        .map(|n| PeerKey((20 + n as u128) << 100))
        .collect();
    for (n, key) in keys.iter().enumerate() {
        let intent = if n == 0 { Intent::Create } else { Intent::Join };
        player_comes(&mut room, *key, &format!("J{n}"), intent);
    }
    room.until(60.0, "everyone seated", |app| {
        lobby(app).seats().len() == players
    });
    for key in &keys[1..] {
        room.act(*key, NetAction::Ready(true));
    }
    room.until(5.0, "everyone ready", |app| {
        !is_server(app) || lobby(app).all_ready()
    });
    room.act(keys[0], NetAction::StartRace);
    room.until(30.0, "everyone on the grid", |app| racing(app, 0));
    bots_drive_the_players(&mut room);
    let sent = |room: &Room, key: PeerKey| room.hub.traffic(key);
    let (server_before, client_before) = (sent(&room, SERVER), sent(&room, keys[1]));
    let seconds = 20.0;
    let (ms, worst) = server_cost(&mut room, (60.0 * seconds) as usize);
    let (server_after, client_after) = (sent(&room, SERVER), sent(&room, keys[1]));
    let rate = |after: u64, before: u64| (after - before) as f64 / seconds / 1000.0;
    let bytes = |t: socket::memory::Traffic| t.bytes[0] + t.bytes[1];
    let packets = |t: socket::memory::Traffic| t.packets[0] + t.packets[1];
    println!(
        "{players} players and {} bots: server {ms:.3} ms per tick (worst {worst:.2} ms); it sends {:.1} kB/s in {:.0} packets/s, a player sends {:.1} kB/s",
        12 - players,
        rate(bytes(server_after), bytes(server_before)),
        (packets(server_after) - packets(server_before)) as f64 / seconds,
        rate(bytes(client_after), bytes(client_before)),
    );
}
