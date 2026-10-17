//! Headless scenarios of the race online (issue #9): two or three whole games (`GamePlugin`) in a
//! room, linked by the in-memory socket of the tests (`socket::memory`): every message goes
//! through its bytes, the lobby, the room's clock, the kart states and the events go the way they
//! go between browsers. One frame is one tick (60 per second) for every game, in turn.
//!
//! The race with `matchbox_server` (real sockets, real time) is the ignored test at the end.

use std::time::{Duration, Instant};

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::socket::memory::Hub;
use super::*;
use crate::bots::{Bot, BotDifficulty, BotTuning, bot, bot_seed};
use crate::interactions::{
    InteractionTuning, Paw, PawArm, PawMotion, PawStats, max_arm_length, shown_arms,
};
use crate::kart::{Kart, KartInput, KartSystems, PlayerKart, Shove, SpawnPoint};
use crate::options::{OptionsAction, OptionsMenu};
use crate::race::{
    OnlineFlow, Race, RaceGrid, RaceMode, RacePhase, RaceProgress, RaceSystems, RaceTuning, Racer,
    ResultTime,
};
use crate::track::{TrackChoice, TrackLayout};

pub(super) const ANA: PeerKey = PeerKey(1 << 100);
pub(super) const BOB: PeerKey = PeerKey(2 << 100);
pub(super) const CYD: PeerKey = PeerKey(3 << 100);

/// A whole game (headless, a tick per frame) joining the room of `hub` as `key`, named `name`:
/// as its host (issue #63: it created the room), or a client. It starts on the test ground, solo.
pub(super) fn game(hub: &Hub, key: PeerKey, name: &str, hosting: bool) -> App {
    let mut app = crate::tests::game_app_unloaded(TrackChoice::TestGround, 0);
    crate::tests::single_threaded(&mut app);
    app.world_mut().resource_mut::<NetSettings>().pseudo = Some(name.into());
    crate::tests::wait_for_the_player(&mut app);
    let settings = app.world().resource::<NetSettings>().clone();
    app.world_mut().insert_resource(NetSession::new(
        RoomCode::parse("TEST").unwrap(),
        settings.signal.clone(),
        settings.ice_servers(),
        NetSocket::memory(hub, key, hosting),
        hosting,
    ));
    app
}

/// The games of a room, updated in turn.
pub(super) struct Room {
    pub(super) hub: Hub,
    pub(super) games: Vec<(PeerKey, App)>,
}

impl Room {
    /// The first player creates the room (issue #63: it hosts it), the others open its link.
    pub(super) fn new(players: &[(PeerKey, &str)]) -> Self {
        let hub = Hub::default();
        let games = players
            .iter()
            .enumerate()
            .map(|(index, (key, name))| (*key, game(&hub, *key, name, index == 0)))
            .collect();
        Self { hub, games }
    }

    pub(super) fn app(&mut self, key: PeerKey) -> &mut App {
        &mut self
            .games
            .iter_mut()
            .find(|(k, _)| *k == key)
            .expect("no such game")
            .1
    }

    pub(super) fn view(&self, key: PeerKey) -> &App {
        &self.games.iter().find(|(k, _)| *k == key).unwrap().1
    }

    /// One frame of every game.
    pub(super) fn frame(&mut self) {
        for (_, app) in &mut self.games {
            app.update();
        }
    }

    /// Frames until `done` holds for every game (the circuit files load in the background: a
    /// little real time too), `seconds` of game time at most.
    pub(super) fn until(&mut self, seconds: f32, what: &str, mut done: impl FnMut(&App) -> bool) {
        let start = Instant::now();
        for _ in 0..(seconds * 60.0) as usize {
            self.frame();
            if self.games.iter().all(|(_, app)| done(app)) {
                return;
            }
            if start.elapsed() > Duration::from_secs(120) {
                break;
            }
            std::thread::sleep(Duration::from_micros(200));
        }
        panic!("{what}: never happened");
    }

    pub(super) fn act(&mut self, key: PeerKey, action: NetAction) {
        self.app(key).world_mut().write_message(action);
    }

    /// A game leaves without a word (its tab closes): the others see it gone.
    pub(super) fn close(&mut self, key: PeerKey) {
        self.hub.drop_port(key);
        self.games.retain(|(k, _)| *k != key);
    }
}

pub(super) fn lobby(app: &App) -> &Lobby {
    app.world()
        .resource::<NetSession>()
        .lobby
        .as_ref()
        .expect("no lobby")
}

pub(super) fn showing(app: &App) -> Option<Showing> {
    app.world()
        .get_resource::<OnlineLevel>()
        .map(|level| level.showing)
}

pub(super) fn player(app: &mut App) -> Entity {
    let world = app.world_mut();
    let tagged = world
        .query_filtered::<Entity, (With<Kart>, With<NetKart>, Without<Bot>)>()
        .iter(world)
        .next();
    tagged
        .or_else(|| {
            world
                .query_filtered::<Entity, (With<Kart>, With<PlayerKart>)>()
                .iter(world)
                .next()
        })
        .expect("no player kart")
}

/// Our kart in a race: on its grid slot, the race there and the level built.
pub(super) fn on_grid(app: &App) -> bool {
    app.world().get_resource::<Race>().is_some()
        && app.world().get_resource::<OnlineRace>().is_some()
        && app
            .world()
            .iter_entities()
            .any(|e| e.contains::<PlayerKart>() && e.contains::<Racer>())
}

/// A room of `players` seated, driving freely on the room's circuit, each seeing the others.
pub(super) fn seated(players: &[(PeerKey, &str)]) -> Room {
    let mut room = Room::new(players);
    let count = players.len();
    room.until(15.0, "everyone seated, free driving", |app| {
        app.world().get_resource::<KartSync>().is_some_and(|sync| {
            sync.remotes().len() == count - 1 && sync.remotes().iter().all(|r| r.proxy.is_some())
        }) && showing(app) == Some(Showing::free(0))
            && app.world().get_resource::<TrackLayout>().is_some()
    });
    room
}

/// The host starts a race of `laps` laps with `bots` bots, everyone ready (the lobby's
/// « Lancer la course » with a smaller grid, for the tests' time).
pub(super) fn start_race(room: &mut Room, host: PeerKey, bots: u8, laps: u8) -> RaceStart {
    let keys: Vec<PeerKey> = room.games.iter().map(|(k, _)| *k).collect();
    for key in &keys {
        room.act(*key, NetAction::Ready(true));
    }
    room.until(2.0, "everyone ready", |app| lobby(app).all_ready());
    let app = room.app(host);
    let now = now_ms(app.world().resource::<Time<Real>>());
    let session = app.world().resource::<NetSession>();
    let lobby = session.lobby.as_ref().unwrap();
    let mut start = race_to_start(
        lobby,
        now,
        0xbeef,
        u32::from(laps),
        BotDifficulty::Normal,
        &NetTuning::default(),
    )
    .expect("the host knows the room's clock");
    start.bots = bots;
    let mut session = app.world_mut().resource_mut::<NetSession>();
    assert!(session.lobby.as_mut().unwrap().announce_race(start.clone()));
    room.until(10.0, "everyone on the grid", on_grid);
    start
}

/// Every game's own karts (its player's, the bots it hosts) are driven by bots: a race the test
/// can run to the end.
pub(super) fn bots_drive_the_players(room: &mut Room) {
    for (_, app) in &mut room.games {
        let kart = player(app);
        let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
        let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
        let tuning = app.world().resource::<BotTuning>().clone();
        app.world_mut()
            .entity_mut(kart)
            .remove::<PlayerKart>()
            .insert(bot(BotDifficulty::Normal, seed, &tuning));
    }
}

/// The common tick of this frame in `app` (its clock set).
pub(super) fn common_tick(app: &App) -> u32 {
    let local = (app.world().resource::<Time<Fixed>>().elapsed_secs_f64() * crate::TICK_HZ).round();
    app.world()
        .resource::<KartSync>()
        .clock
        .common(local as u32)
        .expect("clock not set")
}

/// Every racer's progress in `app`, by grid slot: (passed, lap times, finish).
pub(super) fn progress_by_slot(app: &mut App) -> Vec<(usize, u32, Vec<u32>, Option<u32>)> {
    let world = app.world_mut();
    let mut all: Vec<(usize, u32, Vec<u32>, Option<u32>)> = world
        .query::<(&Racer, &RaceProgress)>()
        .iter(world)
        .map(|(racer, p)| (racer.grid_slot, p.passed, p.lap_times.clone(), p.finish))
        .collect();
    all.sort_by_key(|(slot, ..)| *slot);
    all
}

/// Two games agree on every racer: the same laps, times and finish for those who finished; for
/// those still on their way (the bots drive on after the end), a checkpoint at most apart (one
/// may be on the way).
pub(super) fn assert_same_progress(
    a: &[(usize, u32, Vec<u32>, Option<u32>)],
    b: &[(usize, u32, Vec<u32>, Option<u32>)],
) {
    assert_eq!(a.len(), b.len(), "{a:?}\n{b:?}");
    for (x, y) in a.iter().zip(b) {
        assert_eq!(x.0, y.0);
        if x.3.is_some() || y.3.is_some() {
            assert_eq!(x, y, "slot {}", x.0);
        } else {
            assert!(x.1.abs_diff(y.1) <= 1, "slot {}: {x:?} vs {y:?}", x.0);
        }
    }
}

/// The standings (grid slots, first to last).
pub(super) fn standings(app: &mut App) -> Vec<usize> {
    let order = app.world().resource::<Race>().standings.clone();
    order
        .iter()
        .map(|entity| app.world().get::<Racer>(*entity).unwrap().grid_slot)
        .collect()
}

/// Issue #9: the host chooses the room's circuit: everyone drives freely on it; then it starts a
/// race: the same circuit, the same grid (its bots in front, the players behind in its order),
/// the same start tick, the same bots at the same places, the countdown and GO on the same tick
/// for both. The guest has no bot of its own: it shows the host's.
#[test]
fn the_host_starts_the_race_for_everyone() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    assert!(lobby(room.view(ANA)).is_host());
    // Another circuit, then back to the first one.
    room.act(ANA, NetAction::ChooseCircuit(1));
    room.until(10.0, "free driving on the second circuit", |app| {
        showing(app) == Some(Showing::free(1))
            && app
                .world()
                .get_resource::<TrackLayout>()
                .is_some_and(|layout| layout.name == "Anneau Guimauve")
    });
    // A guest cannot choose.
    room.act(BOB, NetAction::ChooseCircuit(0));
    for _ in 0..30 {
        room.frame();
    }
    for (_, app) in &room.games {
        assert_eq!(lobby(app).circuit(), 1);
    }
    room.act(ANA, NetAction::ChooseCircuit(0));
    room.until(10.0, "free driving on the first circuit", |app| {
        showing(app) == Some(Showing::free(0))
            && app
                .world()
                .get_resource::<TrackLayout>()
                .is_some_and(|layout| layout.name == "Prairie Pompon")
    });
    // Free driving: no race, no bot; online (the items and croquettes are shared since #53).
    for (_, app) in &room.games {
        assert!(app.world().get_resource::<Race>().is_none());
        assert!(app.world().get_resource::<crate::level::Online>().is_some());
    }

    let start = start_race(&mut room, ANA, 4, 1);
    assert_eq!(start.players, vec![ANA, BOB]);
    // Both on the same race: its circuit, its seed, its start; Ana on slot 4, Bob on 5.
    for key in [ANA, BOB] {
        let app = room.app(key);
        let race = app.world().resource::<OnlineRace>();
        assert_eq!(race.shared.start, start, "{key:?}");
        assert!(race.racing);
        assert_eq!(app.world().resource::<TrackLayout>().name, "Prairie Pompon");
        assert_eq!(*app.world().resource::<RaceMode>(), RaceMode::Race);
        let kart = player(app);
        let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
        assert_eq!(slot, usize::from(start.slot_of(key).unwrap()));
        let layout = app.world().resource::<TrackLayout>();
        let at = app.world().get::<Transform>(kart).unwrap().translation;
        assert!(
            at.distance(layout.grid[slot].translation) < 1.0,
            "{key:?} at {at}"
        );
    }
    // Ana hosts the 4 bots; Bob shows them (and Ana's kart).
    let bots = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<&Racer, (With<Bot>, With<Kart>, With<NetKart>)>()
            .iter(world)
            .count()
    };
    assert_eq!(bots(room.app(ANA)), 4);
    assert_eq!(bots(room.app(BOB)), 0);
    room.until(2.0, "Bob shows the bots", |app| {
        app.world()
            .resource::<KartSync>()
            .remotes()
            .iter()
            .filter(|remote| remote.proxy.is_some())
            .count()
            == if lobby(app).is_host() { 1 } else { 5 }
    });
    // The bots stand at the same places on both screens.
    {
        let ana = room.app(ANA);
        let mut ana_bots: Vec<(usize, Vec3)> = {
            let world = ana.world_mut();
            world
                .query_filtered::<(&Racer, &Transform), With<Bot>>()
                .iter(world)
                .map(|(racer, transform)| (racer.grid_slot, transform.translation))
                .collect()
        };
        ana_bots.sort_by_key(|(slot, _)| *slot);
        let bob = room.app(BOB);
        let world = bob.world_mut();
        let mut shown: Vec<(usize, Vec3)> = world
            .query::<(&RemoteKartProxy, &Transform)>()
            .iter(world)
            .filter(|(proxy, _)| proxy.kart < 4)
            .map(|(proxy, transform)| (usize::from(proxy.kart), transform.translation))
            .collect();
        shown.sort_by_key(|(slot, _)| *slot);
        assert_eq!(shown.len(), 4);
        for ((slot, at), (shown_slot, shown_at)) in ana_bots.iter().zip(&shown) {
            assert_eq!(slot, shown_slot);
            assert!(
                at.distance(*shown_at) < 0.05,
                "bot {slot}: {at} vs {shown_at}"
            );
        }
        // Bob's bots have the parts Ana's have: the same seed.
        let ana_parts = crate::bots::bot_loadout(
            room.view(ANA)
                .world()
                .resource::<crate::characters::Roster>(),
            start.seed,
            2,
        );
        let remote = room
            .view(BOB)
            .world()
            .resource::<KartSync>()
            .remote_kart(2)
            .unwrap()
            .loadout;
        assert_eq!(usize::from(remote[0]), ana_parts.character);
    }
    // GO on the same common tick for both (within a tick: each estimates the room's clock).
    let mut go: Vec<(PeerKey, u32)> = Vec::new();
    for _ in 0..(8 * 60) {
        room.frame();
        for (key, app) in &room.games {
            if !go.iter().any(|(k, _)| k == key) && app.world().resource::<Race>().started() {
                go.push((*key, common_tick(app)));
            }
        }
        if go.len() == 2 {
            break;
        }
    }
    assert_eq!(go.len(), 2, "GO never came");
    let expected = start.start_tick
        + crate::race::RaceFlow::go_tick(&room.view(ANA).world().resource::<Race>().timings)
        - 1;
    for (key, tick) in &go {
        assert!(
            tick.abs_diff(expected) <= 1,
            "{key:?}: GO at {tick}, expected {expected}"
        );
    }
}

/// Issue #9 acceptance (in memory): a race of two players and bots, run to the end: every
/// checkpoint of every racer comes out the same on both screens (laps, times, finish), the host
/// ends the race, and both show the same results, in the same order.
#[test]
fn a_race_online_ends_with_the_same_results_for_everyone() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    let start = start_race(&mut room, ANA, 4, 1);
    bots_drive_the_players(&mut room);
    room.until(150.0, "the results", |app| {
        app.world().resource::<Race>().phase() == RacePhase::Results
    });
    let (ana, bob) = (
        progress_by_slot(room.app(ANA)),
        progress_by_slot(room.app(BOB)),
    );
    println!("Ana: {ana:?}\nBob: {bob:?}");
    assert_eq!(ana.len(), usize::from(start.bots) + 2);
    assert_same_progress(&ana, &bob);
    let finished = ana.iter().filter(|(.., finish)| finish.is_some()).count();
    assert!(finished >= 2, "both players finished: {ana:?}");
    let (ana_order, bob_order) = (standings(room.app(ANA)), standings(room.app(BOB)));
    println!("results: {ana_order:?}");
    assert_eq!(ana_order, bob_order);
    let over = room
        .view(ANA)
        .world()
        .resource::<OnlineRace>()
        .shared
        .over()
        .cloned()
        .expect("the host ended it");
    assert_eq!(
        ana_order,
        over.order
            .iter()
            .map(|k| usize::from(*k))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        room.view(BOB)
            .world()
            .resource::<OnlineFlow>()
            .order
            .as_deref(),
        Some(ana_order.as_slice())
    );
}

/// The grid slot of each line of the results screen, from the top: the closed results', or the
/// standings until then.
fn results_slots(app: &App) -> Vec<usize> {
    let race = app.world().resource::<Race>();
    let slot = |entity: &Entity| app.world().get::<Racer>(*entity).unwrap().grid_slot;
    if race.results_closed() {
        race.results.iter().map(|line| slot(&line.id)).collect()
    } else {
        race.standings.iter().map(slot).collect()
    }
}

/// The bot of grid slot 0 stops 5 s after GO (it is Ana's: she hosts the bots).
fn stop_the_first_bot(race: Res<Race>, mut karts: Query<(&Racer, &mut KartInput), With<Bot>>) {
    if race.race_ticks() < 300 {
        return;
    }
    for (racer, mut input) in &mut karts {
        if racer.grid_slot == 0 {
            *input = KartInput::default();
        }
    }
}

/// Issue #70 acceptance (in memory): the results online. A race of two players and three bots,
/// one of them stopped on its way. Once the race is over, the results follow the karts that
/// finish; Bob's game does not close them on its own (its delay is 0), it waits for the host's
/// table: Ana closes them `results_close_seconds` after they appeared (2 s here), the stopped
/// bot gets its estimate, and both screens show the same lines (each one's own kart aside: its
/// name).
#[test]
fn online_the_results_close_with_the_hosts_table_for_everyone() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    for (key, seconds) in [(ANA, 2.0), (BOB, 0.0)] {
        room.app(key)
            .world_mut()
            .resource_mut::<RaceTuning>()
            .results_close_seconds = seconds;
    }
    start_race(&mut room, ANA, 3, 1);
    bots_drive_the_players(&mut room);
    room.app(ANA).add_systems(
        FixedUpdate,
        stop_the_first_bot
            .after(KartSystems::Input)
            .before(RaceSystems::Gate),
    );
    room.until(150.0, "the results", |app| {
        app.world().resource::<Race>().phase() == RacePhase::Results
    });
    // The results are live on both screens: the stopped bot is on its way.
    for _ in 0..30 {
        room.frame();
    }
    for key in [ANA, BOB] {
        let app = room.app(key);
        assert!(!app.world().resource::<Race>().results_closed());
        let slots = results_slots(app);
        let row = slots.iter().position(|slot| *slot == 0).unwrap();
        let lines = crate::ui::results_tests::results_lines(app);
        assert_eq!(&lines[row][2..], ["en course", "Tour 1/1"], "{lines:?}");
    }
    room.until(5.0, "the closed results", |app| {
        app.world().resource::<Race>().results_closed()
    });
    for _ in 0..3 {
        room.frame();
    }
    let host = room
        .view(ANA)
        .world()
        .resource::<OnlineRace>()
        .shared
        .results()
        .cloned()
        .expect("the host closed them");
    let mut screens = Vec::new();
    for key in [ANA, BOB] {
        let app = room.app(key);
        let slots = results_slots(app);
        let times: Vec<ResultTime> = app
            .world()
            .resource::<Race>()
            .results
            .iter()
            .map(|line| line.time)
            .collect();
        let lines = crate::ui::results_tests::results_lines(app);
        println!("{key:?}: {slots:?} {lines:?}");
        screens.push((slots, times, lines));
    }
    let (ana, bob) = (&screens[0], &screens[1]);
    // The host's table, on both screens.
    let table: Vec<(usize, ResultTime)> = host
        .lines
        .iter()
        .map(|line| (usize::from(line.id), line.time))
        .collect();
    for (slots, times, _) in [ana, bob] {
        let shown: Vec<(usize, ResultTime)> = slots.iter().copied().zip(times.clone()).collect();
        assert_eq!(shown, table);
    }
    assert_eq!(table.len(), 5);
    // The same lines: rank, time and detail; the name too, but for each one's own kart.
    for (row, (a, b)) in ana.2.iter().zip(&bob.2).enumerate() {
        assert_eq!([&a[0], &a[2], &a[3]], [&b[0], &b[2], &b[3]], "line {row}");
        let own = |slot: usize| slot == 3 || slot == 4;
        if !own(ana.0[row]) {
            assert_eq!(a[1], b[1], "line {row}");
        }
    }
    // The stopped bot: estimated, last of those with a time.
    let row = ana.0.iter().position(|slot| *slot == 0).unwrap();
    let ResultTime::Estimated(estimate) = ana.1[row] else {
        panic!("{:?}", ana.1);
    };
    assert!(ana.2[row][2].starts_with('~'), "{:?}", ana.2[row]);
    assert_eq!(ana.2[row][3], "Temps estimé");
    for time in &ana.1[..row] {
        assert!(time.ticks().unwrap() <= estimate);
    }
    let finished = ana
        .1
        .iter()
        .filter(|time| matches!(time, ResultTime::Finished(_)))
        .count();
    assert!(finished >= 2, "both players finished: {:?}", ana.1);
}

/// Bob's kart stands 2.4 m to the right of Ana's, and shows there on Ana's screen.
fn put_bob_beside_ana(room: &mut Room, ana_kart: Entity, bob_kart: Entity) {
    put_bob_near_ana(room, ana_kart, bob_kart, 2.4, 0.0);
}

/// Bob's kart stands `right` m to the right of Ana's and `ahead` m ahead, the same way, and shows
/// there on Ana's screen.
fn put_bob_near_ana(room: &mut Room, ana_kart: Entity, bob_kart: Entity, right: f32, ahead: f32) {
    let ana_at = *room.view(ANA).world().get::<Transform>(ana_kart).unwrap();
    let beside = ana_at.translation + *ana_at.right() * right + *ana_at.forward() * ahead;
    {
        let world = room.app(BOB).world_mut();
        let mut transform = world.get_mut::<Transform>(bob_kart).unwrap();
        *transform = ana_at;
        transform.translation = beside;
        let placed = *transform;
        world
            .get_mut::<crate::interpolation::PhysicsPose>(bob_kart)
            .unwrap()
            .snap(placed);
        *world.get_mut::<Velocity>(bob_kart).unwrap() = Velocity::zero();
    }
    room.until(3.0, "Bob beside Ana", |app| {
        if !lobby(app).is_host() {
            return true;
        }
        let sync = app.world().resource::<KartSync>();
        let proxy = sync.remote(BOB).and_then(|r| r.proxy).unwrap();
        let at = app.world().get::<Transform>(proxy).unwrap().translation;
        at.distance(beside) < 0.3
    });
}

/// Ana taps the Interaction button (her kart's input is the test's).
fn strike(room: &mut Room, ana_kart: Entity) {
    for pressed in [true, true, false] {
        room.app(ANA)
            .world_mut()
            .get_mut::<KartInput>(ana_kart)
            .unwrap()
            .interact = pressed;
        room.frame();
    }
}

/// Issue #9: a paw strike on another player's kart. Ana sees Bob's kart beside hers and
/// strikes: Bob's game decides (in reach at the tick Ana saw), pushes its own kart and answers;
/// Ana counts it landed, and shows the "pof" on Bob's kart. A strike that comes too late (0.5 s
/// on the way) is refused: Bob's kart is not struck, Ana's paw struck the air.
#[test]
fn a_strike_on_another_players_kart_is_judged_by_its_game() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    // The test drives both karts (no keyboard).
    let ana_kart = player(room.app(ANA));
    let bob_kart = player(room.app(BOB));
    room.app(ANA)
        .world_mut()
        .entity_mut(ana_kart)
        .remove::<PlayerKart>();
    room.app(BOB)
        .world_mut()
        .entity_mut(bob_kart)
        .remove::<PlayerKart>();
    put_bob_beside_ana(&mut room, ana_kart, bob_kart);
    strike(&mut room, ana_kart);
    room.until(1.0, "the answer", |app| {
        !lobby(app).is_host()
            || app
                .world()
                .get::<PawStats>(player_of(app))
                .is_some_and(|s| s.landed == 1)
    });
    let struck = room
        .view(BOB)
        .world()
        .get::<PawStats>(bob_kart)
        .unwrap()
        .struck;
    assert_eq!(struck, 1, "Bob's game applied the strike");
    assert!(
        room.view(BOB)
            .world()
            .get::<Shove>(bob_kart)
            .unwrap()
            .is_active()
            || room
                .view(BOB)
                .world()
                .get::<Velocity>(bob_kart)
                .unwrap()
                .linear
                .length()
                > 1.0,
        "Bob's kart pushed"
    );
    // The "pof" on Ana's screen: Bob's kart was struck.
    let proxy = room
        .view(ANA)
        .world()
        .resource::<KartSync>()
        .remote(BOB)
        .unwrap()
        .proxy
        .unwrap();
    assert_eq!(
        room.view(ANA)
            .world()
            .get::<Paw>(proxy)
            .unwrap()
            .struck_count,
        1
    );

    // The cooldown and the protection over, Bob beside Ana again; the strike takes 0.5 s.
    for _ in 0..(3 * 60) {
        room.frame();
    }
    put_bob_beside_ana(&mut room, ana_kart, bob_kart);
    room.hub.set_delay(30);
    strike(&mut room, ana_kart);
    for _ in 0..90 {
        room.frame();
    }
    let ana_strikes = room
        .view(ANA)
        .world()
        .get::<PawStats>(ana_kart)
        .unwrap()
        .strikes;
    assert_eq!(ana_strikes, 2, "Ana struck twice");
    let struck = room
        .view(BOB)
        .world()
        .get::<PawStats>(bob_kart)
        .unwrap()
        .struck;
    assert_eq!(struck, 1, "a strike older than 0.3 s is refused");
    let landed = room
        .view(ANA)
        .world()
        .get::<PawStats>(ana_kart)
        .unwrap()
        .landed;
    assert_eq!(landed, 1, "the refusal came back: not landed");
}

/// The player's kart of a game (bots aside).
pub(super) fn player_of(app: &App) -> Entity {
    app.world()
        .iter_entities()
        .find(|e| e.contains::<NetKart>() && e.contains::<Kart>() && !e.contains::<Bot>())
        .map(|e| e.id())
        .expect("no player kart")
}

/// Issue #9: Ana grabs Bob's kart: Bob's game accepts and holds its kart (each pulls its own
/// kart toward the other one); Ana lets go: Bob's kart is free on both screens.
#[test]
fn a_grab_holds_another_players_kart_until_released() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    let ana_kart = player(room.app(ANA));
    let bob_kart = player(room.app(BOB));
    room.app(ANA)
        .world_mut()
        .entity_mut(ana_kart)
        .remove::<PlayerKart>();
    room.app(BOB)
        .world_mut()
        .entity_mut(bob_kart)
        .remove::<PlayerKart>();
    put_bob_beside_ana(&mut room, ana_kart, bob_kart);
    room.app(ANA)
        .world_mut()
        .get_mut::<KartInput>(ana_kart)
        .unwrap()
        .interact = true;
    room.until(1.5, "Bob held", |app| {
        if lobby(app).is_host() {
            matches!(
                app.world().get::<Paw>(player_of(app)).unwrap().motion,
                PawMotion::Holding { .. }
            )
        } else {
            app.world()
                .get::<Paw>(player_of(app))
                .unwrap()
                .held
                .is_some()
        }
    });
    // Ana's paw is shown holding on Bob's screen.
    let ana_proxy = room
        .view(BOB)
        .world()
        .resource::<KartSync>()
        .remote(ANA)
        .unwrap()
        .proxy
        .unwrap();
    assert!(matches!(
        room.view(BOB).world().get::<Paw>(ana_proxy).unwrap().motion,
        PawMotion::Holding { .. }
    ));
    room.app(ANA)
        .world_mut()
        .get_mut::<KartInput>(ana_kart)
        .unwrap()
        .interact = false;
    room.until(1.0, "Bob free", |app| {
        let paw = app.world().get::<Paw>(player_of(app)).unwrap();
        paw.held.is_none() && !matches!(paw.motion, PawMotion::Holding { .. })
    });
    assert!(!matches!(
        room.view(BOB).world().get::<Paw>(ana_proxy).unwrap().motion,
        PawMotion::Holding { .. }
    ));
    let _ = InteractionTuning::default();
}

/// Issue #81: the arms shown on every screen of the room: none longer than the longest tether
/// (`max_arm_length`), nothing NaN.
fn check_arms(room: &Room, what: &str) {
    let longest = max_arm_length(&InteractionTuning::default()) + 0.01;
    for (key, app) in &room.games {
        for (body, length) in shown_arms(app.world()) {
            assert!(
                length.is_finite() && length <= longest,
                "{what}: an arm of {length} m on {body:?}, on {key:?}'s screen"
            );
        }
    }
}

/// The arms shown on `app`'s screen from the kart `body`.
fn arms_of(app: &App, body: Entity) -> usize {
    shown_arms(app.world())
        .iter()
        .filter(|(kart, _)| *kart == body)
        .count()
}

/// `frames` frames of the room, its arms checked after each one.
fn frames_checking_arms(room: &mut Room, frames: usize, what: &str) {
    for _ in 0..frames {
        room.frame();
        check_arms(room, what);
    }
}

/// Issue #81 (Max's bug: on the results of a race online, four long beige bars went from his
/// driver off the screen: the arms of the host's bots): Ana (the host) grabs Bob's kart (the
/// client's), and Bob breaks free: he hops, the tether breaks when he lands. Only Bob's game
/// decides it (the victim decides) and Ana's says nothing back, yet Ana's paw lets go on Bob's
/// screen too. Ana then drives off: no arm stays stretched from her kart toward Bob's. On both
/// screens, every frame: no arm longer than the longest tether, nothing NaN.
#[test]
fn a_kart_that_breaks_free_keeps_no_arm_stretched_toward_it() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    let ana_kart = player(room.app(ANA));
    let bob_kart = player(room.app(BOB));
    for (key, kart) in [(ANA, ana_kart), (BOB, bob_kart)] {
        room.app(key)
            .world_mut()
            .entity_mut(kart)
            .remove::<PlayerKart>();
    }
    put_bob_beside_ana(&mut room, ana_kart, bob_kart);
    let ana_proxy = room
        .view(BOB)
        .world()
        .resource::<KartSync>()
        .remote(ANA)
        .unwrap()
        .proxy
        .unwrap();
    let holding = |app: &App, kart: Entity| {
        matches!(
            app.world().get::<Paw>(kart).unwrap().motion,
            PawMotion::Holding { .. }
        )
    };
    // Ana keeps the button down to the end: no other grab.
    room.app(ANA)
        .world_mut()
        .get_mut::<KartInput>(ana_kart)
        .unwrap()
        .interact = true;
    room.until(1.5, "Bob held", |app| {
        if lobby(app).is_host() {
            holding(app, ana_kart)
        } else {
            app.world().get::<Paw>(bob_kart).unwrap().held.is_some() && holding(app, ana_proxy)
        }
    });
    frames_checking_arms(&mut room, 3, "Bob held");
    assert_eq!(arms_of(room.view(BOB), ana_proxy), 1, "the elastic band");
    // Bob hops, and breaks free when he lands.
    for jump in [true, true, false] {
        room.app(BOB)
            .world_mut()
            .get_mut::<KartInput>(bob_kart)
            .unwrap()
            .jump_drift = jump;
        room.frame();
        check_arms(&room, "Bob hops");
    }
    room.until(1.5, "Bob free", |app| {
        app.world()
            .get::<Paw>(player_of(app))
            .unwrap()
            .held
            .is_none()
    });
    assert_eq!(
        room.view(BOB)
            .world()
            .get::<PawStats>(bob_kart)
            .unwrap()
            .escapes,
        1
    );
    assert!(
        !holding(room.view(BOB), ana_proxy),
        "Ana's paw let go on Bob's screen"
    );
    // Ana drives off; Bob stays.
    room.app(ANA)
        .world_mut()
        .get_mut::<KartInput>(ana_kart)
        .unwrap()
        .throttle = 1.0;
    frames_checking_arms(&mut room, 3 * 60, "Ana drives off");
    let bob = room.view(BOB).world();
    let apart = bob
        .get::<Transform>(ana_proxy)
        .unwrap()
        .translation
        .distance(bob.get::<Transform>(bob_kart).unwrap().translation);
    assert!(
        apart > max_arm_length(&InteractionTuning::default()),
        "far apart: {apart} m"
    );
    for key in [ANA, BOB] {
        assert!(
            shown_arms(room.view(key).world()).is_empty(),
            "{key:?}: no arm left"
        );
    }
    assert!(!holding(room.view(BOB), ana_proxy));
}

/// Issue #81, the other way: Bob (a client) grabs Ana's kart (the host's), on his left and ahead.
/// On Bob's screen, his paw reaches Ana's driver where her kart is shown (another peer's kart
/// has no `PhysicsPose`: the paw went 2 m to the side, into thin air). He lets go: the arm comes
/// back and goes down.
#[test]
fn a_paw_on_another_peers_kart_reaches_where_it_is_shown() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    let ana_kart = player(room.app(ANA));
    let bob_kart = player(room.app(BOB));
    for (key, kart) in [(ANA, ana_kart), (BOB, bob_kart)] {
        room.app(key)
            .world_mut()
            .entity_mut(kart)
            .remove::<PlayerKart>();
    }
    put_bob_near_ana(&mut room, ana_kart, bob_kart, 2.4, -2.0);
    let ana_visual = room
        .view(BOB)
        .world()
        .resource::<KartSync>()
        .remote(ANA)
        .unwrap()
        .visual
        .unwrap();
    room.app(BOB)
        .world_mut()
        .get_mut::<KartInput>(bob_kart)
        .unwrap()
        .interact = true;
    room.until(1.5, "Ana held", |app| {
        let kart = player_of(app);
        let paw = app.world().get::<Paw>(kart).unwrap();
        if lobby(app).is_host() {
            paw.held.is_some()
        } else {
            matches!(paw.motion, PawMotion::Holding { .. })
        }
    });
    frames_checking_arms(&mut room, 10, "Bob holds Ana");
    let bob = room.view(BOB).world();
    let driver = bob
        .get::<GlobalTransform>(ana_visual)
        .unwrap()
        .translation()
        + Vec3::Y * 0.6;
    let mut paws = bob
        .try_query::<(&PawArm, &Visibility, &GlobalTransform)>()
        .unwrap();
    let paw = paws
        .iter(bob)
        .find(|(part, visibility, _)| {
            part.paw && part.body == bob_kart && **visibility != Visibility::Hidden
        })
        .map(|(_, _, global)| global.translation())
        .expect("Bob's paw shows");
    assert!(
        paw.distance(driver) < 0.3,
        "Bob's paw at {paw}, Ana's driver shown at {driver}"
    );
    // On Ana's screen, Bob's arm leaves from his left shoulder: she is on his left.
    let ana = room.view(ANA).world();
    let bob_proxy = ana
        .resource::<KartSync>()
        .remote(BOB)
        .unwrap()
        .proxy
        .unwrap();
    assert!(
        matches!(
            ana.get::<Paw>(bob_proxy).unwrap().motion,
            PawMotion::Holding { side, .. } if side < 0.0
        ),
        "{:?}",
        ana.get::<Paw>(bob_proxy).unwrap().motion
    );
    room.app(BOB)
        .world_mut()
        .get_mut::<KartInput>(bob_kart)
        .unwrap()
        .interact = false;
    frames_checking_arms(&mut room, 30, "Bob lets go");
    assert_eq!(arms_of(room.view(BOB), bob_kart), 0, "back and down");
    assert!(shown_arms(room.view(ANA).world()).is_empty());
}

/// Issue #63 (was « a new host takes the bots over », issue #9): the host leaves in the middle
/// of a race: the room is over for its clients. Each one sees « L'hôte a quitté la partie »
/// (`NetStatus::HostLeft`, the lobby window open), its socket closes, and the race stops: the
/// game goes back to the level it was on before the room, solo, without any remote kart. Nobody
/// takes the bots over.
#[test]
fn the_host_leaving_stops_the_race_for_everyone() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd")]);
    start_race(&mut room, ANA, 3, 1);
    bots_drive_the_players(&mut room);
    room.until(10.0, "GO", |app| app.world().resource::<Race>().started());
    for _ in 0..(2 * 60) {
        room.frame();
    }
    // Ana's tab closes.
    room.close(ANA);
    room.until(5.0, "the host gone", |app| {
        app.world().resource::<NetSession>().status == NetStatus::HostLeft
    });
    for (key, app) in &room.games {
        let session = app.world().resource::<NetSession>();
        assert!(session.socket.is_none(), "{key:?}: its socket closed");
        assert!(lobby(app).host_left());
        assert_eq!(lobby(app).host(), Some(ANA), "{key:?}: nobody takes over");
        assert!(
            app.world().resource::<LobbyWindow>().open,
            "{key:?}: the message shows"
        );
    }
    room.until(10.0, "back to the level before", |app| {
        app.world().get_resource::<OnlineLevel>().is_none()
            && app.world().get_resource::<KartSync>().is_none()
            && app.world().get_resource::<OnlineRace>().is_none()
            && *app.world().resource::<TrackChoice>() == TrackChoice::TestGround
            && app
                .world()
                .iter_entities()
                .any(|e| e.contains::<PlayerKart>())
    });
    for (key, app) in &mut room.games {
        let world = app.world_mut();
        let remotes = world
            .query_filtered::<(), With<RemoteKartProxy>>()
            .iter(world)
            .count();
        let bots = world
            .query_filtered::<(), (With<Bot>, With<NetKart>)>()
            .iter(world)
            .count();
        assert_eq!((remotes, bots), (0, 0), "{key:?}");
        assert_eq!(*world.resource::<RaceMode>(), RaceMode::Race, "solo again");
    }
}

/// Issue #9 (seen in two Chromium pages): in free driving, the karts' ids are the seats. A
/// player before another leaves (issue #63: the first client, the host's seat 0 stays): the
/// seat after it moves up, and so do its kart's id and its place on the grid (R); each one goes
/// on showing the other, its states coming under the new id.
#[test]
fn free_driving_follows_the_seats_when_a_player_leaves() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd")]);
    let first = lobby(room.view(ANA)).seats()[1];
    assert_eq!(first, BOB);
    room.close(first);
    let seat = |app: &App| {
        let lobby = lobby(app);
        lobby.seats().iter().position(|key| *key == lobby.me())
    };
    room.until(5.0, "the seats moved up", |app| {
        let sync = app.world().resource::<KartSync>();
        lobby(app).seats().len() == 2
            && seat(app).is_some_and(|seat| usize::from(sync.kart()) == seat)
            && sync.remotes().len() == 1
    });
    let received = |app: &App| app.world().resource::<KartSync>().remotes()[0].received;
    let before: Vec<u32> = room.games.iter().map(|(_, app)| received(app)).collect();
    for _ in 0..(2 * 60) {
        room.frame();
    }
    let keys: Vec<PeerKey> = room.games.iter().map(|(key, _)| *key).collect();
    for (index, key) in keys.iter().enumerate() {
        let other = keys[1 - index];
        let app = room.app(*key);
        let ours = seat(app).unwrap();
        let theirs = lobby(app).seats().iter().position(|k| *k == other).unwrap();
        assert_eq!(ours + theirs, 1, "{key:?}: seats 0 and 1");
        assert_eq!(
            ours,
            usize::from(*key == CYD),
            "{key:?}: Ana's seat stays 0"
        );
        let place = app.world().resource::<TrackLayout>().grid[ours];
        let kart = player(app);
        assert_eq!(
            usize::from(app.world().get::<NetKart>(kart).unwrap().0),
            ours
        );
        assert_eq!(app.world().get::<SpawnPoint>(kart).unwrap().0, place);
        assert_eq!(app.world().resource::<OnlineSlot>().0, ours);
        let sync = app.world().resource::<KartSync>();
        let remote = &sync.remotes()[0];
        assert_eq!((remote.peer, usize::from(remote.kart)), (other, theirs));
        assert!(remote.proxy.is_some());
        let fresh = remote.received.saturating_sub(before[index]);
        assert!(fresh >= 30, "{key:?}: {fresh} states of the other in 2 s");
    }
}

/// Issue #9 (Max's decision): online, a menu does not pause the game: the options menu and the
/// selection screen open, the game runs on, our kart coasts (its keys are the menu's) and its
/// states keep going to the others.
#[test]
fn online_the_menus_do_not_pause() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    let bob_kart = player(room.app(BOB));
    room.app(BOB)
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);
    for _ in 0..60 {
        room.frame();
    }
    room.app(BOB)
        .world_mut()
        .write_message(OptionsAction::Toggle);
    for _ in 0..10 {
        room.frame();
    }
    let bob = room.view(BOB);
    assert!(bob.world().resource::<OptionsMenu>().open);
    assert!(!bob.world().resource::<Time<Virtual>>().is_paused());
    assert_eq!(
        *bob.world().get::<KartInput>(bob_kart).unwrap(),
        KartInput::default(),
        "it coasts"
    );
    // Ana still receives Bob's states.
    let received = |room: &Room| {
        room.view(ANA)
            .world()
            .resource::<KartSync>()
            .remote(BOB)
            .unwrap()
            .received
    };
    let before = received(&room);
    for _ in 0..60 {
        room.frame();
    }
    assert!(
        received(&room) >= before + 25,
        "{} then {}",
        before,
        received(&room)
    );
    let speed = room
        .view(BOB)
        .world()
        .get::<Velocity>(bob_kart)
        .unwrap()
        .linear
        .length();
    assert!(speed > 1.0, "the kart rolls on: {speed}");
}

/// Issue #63, for the tests with `matchbox_server`: the first game creates the room and hosts
/// it; once it is in the room (the signaling server gave it its id), the others open its link
/// (a client that came first would find no host: the host offers to those who come after it).
pub(super) fn host_then_join<'a, 'b: 'a>(
    games: impl IntoIterator<Item = &'a mut (&'b str, App)>,
    code: &RoomCode,
) {
    let mut games = games.into_iter();
    let (_, host) = games.next().expect("a host");
    host.world_mut()
        .write_message(NetAction::Host(code.clone()));
    let since = Instant::now();
    while host
        .world()
        .get_resource::<NetSession>()
        .map(|s| s.status.clone())
        != Some(NetStatus::InRoom)
    {
        assert!(
            since.elapsed() < Duration::from_secs(10),
            "the host never got in"
        );
        host.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    for (_, app) in games {
        app.world_mut().write_message(NetAction::Join(code.clone()));
        app.update();
    }
}

/// Issue #9 acceptance, for real: two whole games (headless, real time) meet through
/// `matchbox_server` on this machine, get ready, and the host (issue #63: Ana, who created the
/// room) starts a race:
/// 3 laps of Prairie Pompon, the grid filled with 10 bots. Both players are driven by bots (the
/// test cannot play) until the results. Both games must agree: the same circuit, the same start
/// tick, the same bots at the same places, GO on the same tick, every racer's laps and finish,
/// and the same results. Since issue #53, with the items and the croquettes: the same boxes and
/// croquettes taken, the same items thrown, the same hits, the same croquettes lost
/// (`item_tests::assert_same_story`); since issue #18, the players start with a bombe each, a
/// klaxon-canard and a piment doré. Run it by hand, the server started:
/// `cargo test --lib net::race_tests::two_games_race -- --ignored --nocapture`
/// (`FLUFFY_NETSIM=120,20,5` puts the network simulator on Bob's side).
#[test]
#[ignore = "needs matchbox_server on localhost:3536"]
fn two_games_race_through_a_local_signaling_server() {
    let code = RoomCode::generate(getrandom::u64().unwrap());
    println!("room {code}");
    let game = |pseudo: &str| {
        let mut app = crate::tests::game_app_unloaded(TrackChoice::TestGround, 0);
        // Real time: the lobby's clock and the game's go together, as in the browser.
        app.insert_resource(TimeUpdateStrategy::Automatic);
        app.world_mut().resource_mut::<NetSettings>().pseudo = Some(pseudo.into());
        crate::tests::wait_for_the_player(&mut app);
        app
    };
    let mut games = [("Ana", game("Ana")), ("Bob", game("Bob"))];
    if let Ok(netsim) = std::env::var("FLUFFY_NETSIM") {
        let netsim = NetSimTuning::parse(&netsim).expect("FLUFFY_NETSIM: 120,20,5");
        println!("Bob's network simulator: {netsim:?}");
        games[1].1.world_mut().resource_mut::<NetTuning>().simulator = netsim;
    }
    // Issue #63: Ana creates the room (she is in it), then Bob opens its link.
    host_then_join(&mut games, &code);
    let start = Instant::now();
    let update = |games: &mut [(&str, App); 2]| {
        for (_, app) in games.iter_mut() {
            app.update();
        }
        std::thread::sleep(Duration::from_millis(3));
    };
    let wait =
        |games: &mut [(&str, App); 2], seconds: u64, what: &str, done: &dyn Fn(&App) -> bool| {
            let since = Instant::now();
            while !games.iter().all(|(_, app)| done(app)) {
                assert!(
                    since.elapsed() < Duration::from_secs(seconds),
                    "{what}: never happened"
                );
                update(games);
            }
            println!("{what} after {:.1} s", start.elapsed().as_secs_f32());
        };
    wait(&mut games, 40, "both seated, seeing each other", &|app| {
        app.world()
            .get_resource::<KartSync>()
            .is_some_and(|sync| sync.remotes().iter().any(|r| r.proxy.is_some()))
    });
    for (_, app) in &mut games {
        app.world_mut().write_message(NetAction::Ready(true));
    }
    wait(&mut games, 10, "everyone ready", &|app| {
        app.world()
            .resource::<NetSession>()
            .lobby
            .as_ref()
            .is_some_and(Lobby::all_ready)
    });
    let host = games
        .iter()
        .position(|(_, app)| lobby(app).is_host())
        .expect("a host");
    println!("{} hosts", games[host].0);
    games[host]
        .1
        .world_mut()
        .write_message(NetAction::StartRace);
    wait(&mut games, 30, "both on the grid", &on_grid);
    let starts: Vec<RaceStart> = games
        .iter()
        .map(|(_, app)| app.world().resource::<OnlineRace>().shared.start.clone())
        .collect();
    println!("the race: {:?}", starts[0]);
    assert_eq!(starts[0], starts[1]);
    assert_eq!(starts[0].bots, 10);
    for (name, app) in &games {
        let layout = app.world().resource::<TrackLayout>();
        println!("{name} races on {}", layout.name);
        assert_eq!(layout.name, "Prairie Pompon");
    }
    let mut drivers = [Entity::PLACEHOLDER; 2];
    for (index, (_, app)) in games.iter_mut().enumerate() {
        let kart = player(app);
        drivers[index] = kart;
        let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
        let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
        let tuning = app.world().resource::<BotTuning>().clone();
        app.world_mut()
            .entity_mut(kart)
            .remove::<PlayerKart>()
            .insert(bot(BotDifficulty::Normal, seed, &tuning));
    }
    // The bots on the grid: the host's and what the guest shows, before GO.
    let guest = 1 - host;
    let since = Instant::now();
    while since.elapsed() < Duration::from_secs(2) {
        update(&mut games);
    }
    let bots_of_host: Vec<(usize, Vec3)> = {
        let world = games[host].1.world_mut();
        let mut all: Vec<(usize, Vec3)> = world
            .query_filtered::<(&Racer, &Transform), With<Bot>>()
            .iter(world)
            .filter(|(racer, _)| racer.grid_slot < 10)
            .map(|(racer, t)| (racer.grid_slot, t.translation))
            .collect();
        all.sort_by_key(|(slot, _)| *slot);
        all
    };
    let bots_of_guest: Vec<(usize, Vec3)> = {
        let world = games[guest].1.world_mut();
        let mut all: Vec<(usize, Vec3)> = world
            .query::<(&RemoteKartProxy, &Transform)>()
            .iter(world)
            .filter(|(proxy, _)| proxy.kart < 10)
            .map(|(proxy, t)| (usize::from(proxy.kart), t.translation))
            .collect();
        all.sort_by_key(|(slot, _)| *slot);
        all
    };
    let worst = bots_of_host
        .iter()
        .zip(&bots_of_guest)
        .map(|((a, x), (b, y))| {
            assert_eq!(a, b);
            x.distance(*y)
        })
        .fold(0.0f32, f32::max);
    println!(
        "the 10 bots on both screens before GO: {} and {}, {worst:.3} m apart at worst",
        bots_of_host.len(),
        bots_of_guest.len()
    );
    assert_eq!(bots_of_guest.len(), 10);
    assert!(worst < 0.1);
    // GO: the common tick each game saw it on.
    let mut go: [Option<u32>; 2] = [None; 2];
    while go.iter().any(Option::is_none) {
        assert!(start.elapsed() < Duration::from_secs(120), "GO never came");
        update(&mut games);
        for (index, (_, app)) in games.iter().enumerate() {
            if go[index].is_none() && app.world().resource::<Race>().started() {
                go[index] = Some(common_tick(app));
            }
        }
    }
    println!(
        "GO at common tick {:?} (start tick {})",
        go, starts[0].start_tick
    );
    assert!(go[0].unwrap().abs_diff(go[1].unwrap()) <= 2);
    // Issue #18: the players start with the items of phase 2 (their bot brains use them); since
    // issue #61, every one of them, one after the other (each time their slots are empty): Ana
    // an oreiller géant, a chantilly, a fusée, a bombe and a klaxon-canard; Bob a dégonflage, a
    // super câlin, a bombe, an oreiller géant and a piment doré.
    use crate::items::ItemKind::*;
    let mut queues = [
        vec![OreillerGeant, Chantilly, Fusee, Bombe, KlaxonCanard],
        vec![Degonflage, SuperCalin, Bombe, OreillerGeant, PimentDore],
    ];
    let mut next_print = Instant::now();
    loop {
        for (index, (_, app)) in games.iter_mut().enumerate() {
            super::item_tests::feed(app, drivers[index], &mut queues[index]);
        }
        update(&mut games);
        if games
            .iter()
            .all(|(_, app)| app.world().resource::<Race>().phase() == RacePhase::Results)
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(400),
            "the race never ended"
        );
        if next_print.elapsed() > Duration::from_secs(10) {
            next_print = Instant::now();
            for (index, (name, app)) in games.iter_mut().enumerate() {
                let order = standings(app);
                let kart = drivers[index];
                let progress = app.world().get::<RaceProgress>(kart).unwrap().clone();
                println!(
                    "t+{:.0} s {name}: race tick {}, own kart {} checkpoints, rank {}; standings {order:?}",
                    start.elapsed().as_secs_f32(),
                    app.world().resource::<Race>().race_ticks(),
                    progress.passed,
                    progress.rank
                );
            }
        }
    }
    let progress: Vec<Vec<(usize, u32, Vec<u32>, Option<u32>)>> = games
        .iter_mut()
        .map(|(_, app)| progress_by_slot(app))
        .collect();
    let results: Vec<Vec<usize>> = games.iter_mut().map(|(_, app)| standings(app)).collect();
    for (name, app) in games.iter_mut() {
        println!("{name}'s results:");
        let order = standings(app);
        let entities = app.world().resource::<Race>().standings.clone();
        for (rank, (slot, entity)) in order.iter().zip(entities).enumerate() {
            let p = app.world().get::<RaceProgress>(entity).unwrap();
            let name = app
                .world()
                .get::<crate::race::RacerName>(entity)
                .map(|n| n.0.clone())
                .unwrap_or_else(|| {
                    if app.world().get::<NetKart>(entity).is_some()
                        && app.world().get::<Bot>(entity).is_some()
                        && *slot >= 10
                    {
                        "(this game's player)".into()
                    } else {
                        format!("Kart {}", slot + 1)
                    }
                });
            println!(
                "  {:>2}. slot {slot:>2} {name:<20} finish {:?} laps {:?}",
                rank + 1,
                p.finish,
                p.lap_times
            );
        }
    }
    assert_same_progress(&progress[0], &progress[1]);
    assert_eq!(results[0], results[1], "the same results");
    // Issue #53: the same story of the items and the croquettes (what is on its way arrives).
    let since = Instant::now();
    while since.elapsed() < Duration::from_secs(2) {
        update(&mut games);
    }
    for (name, app) in &games {
        let items = app.world().resource::<OnlineItems>();
        println!("{name}: {}", items.summary());
    }
    super::item_tests::assert_same_story(&games[0].1, &games[1].1);
    // Issue #18: the bombes went out (and exploded) on both screens. Issue #61: the chantilly
    // and the dégonflage were told, the oreillers went out.
    for (name, app) in &games {
        let items = app.world().resource::<OnlineItems>();
        let thrown = |kind| {
            items
                .log
                .thrown
                .iter()
                .filter(|(_, item, _)| *item == kind)
                .count()
        };
        println!("{name}: items left to give: {queues:?}");
        assert!(thrown(Bombe) >= 1, "{name}: {}", items.summary());
        assert!(thrown(OreillerGeant) >= 1, "{name}: {}", items.summary());
        let effects: Vec<_> = items.log.effects.iter().map(|(_, item, _)| *item).collect();
        assert!(
            effects.contains(&Chantilly) && effects.contains(&Degonflage),
            "{name}: {effects:?}"
        );
    }
    println!("the whole race took {:.0} s", start.elapsed().as_secs_f32());
}

/// Issue #76: entering a room ends a time trial and gives the player's bots back (kept for the
/// way out, like the rest of the solo level); leaving the room, the solo game has them, and no
/// time trial.
#[test]
fn a_room_ends_the_time_trial_and_gives_the_bots_back() {
    let hub = Hub::default();
    let mut app = game(&hub, ANA, "Ana", true);
    app.insert_resource(crate::modes::TimeTrial { solo_bots: 7 });
    app.world_mut().resource_mut::<RaceGrid>().bots = 0;
    let mut room = Room {
        hub,
        games: vec![(ANA, app)],
    };
    room.until(15.0, "in the room", |app| {
        app.world().get_resource::<OnlineLevel>().is_some()
    });
    let app = room.app(ANA);
    assert!(!app.world().contains_resource::<crate::modes::TimeTrial>());
    assert_eq!(app.world().resource::<OnlineLevel>().solo_grid.bots, 7);
    room.act(ANA, NetAction::Leave);
    room.until(10.0, "solo again", |app| {
        app.world().get_resource::<OnlineLevel>().is_none()
    });
    let app = room.app(ANA);
    assert_eq!(app.world().resource::<RaceGrid>().bots, 7);
    assert!(!app.world().contains_resource::<crate::modes::TimeTrial>());
}
