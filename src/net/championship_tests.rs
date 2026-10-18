//! Issue #122: headless scenarios of the championship online: two or three whole games
//! (`GamePlugin` and the lobby in 3D, `SalonPlugin`) in a room, linked by the in-memory socket of
//! the tests (`socket::memory`): every message goes through its bytes. One frame is one tick for
//! every game, in turn. The races are short (one lap, two bots, the players' karts driven by
//! bots), the waits between them too.

use super::championship::{Championship, entrant_at};
use super::race_tests::{ANA, BOB, CYD, Room, bots_drive_the_players, lobby, on_grid, showing};
use super::salon_tests::{act_on_panel, panel, salon};
use super::*;
use crate::bots::bot_loadout;
use crate::characters::Roster;
use crate::modes::{Catalog, GrandPrixScreen, PodiumScene, draw_circuits};
use crate::race::{
    GrandPrix, GrandPrixStage, GrandPrixTuning, OnPodium, Race, RaceTuning, RacerName, ResultTime,
};
use crate::salon::{SalonAction, SalonRow, SalonTuning};

fn grand_prix(app: &App) -> Option<&GrandPrix> {
    app.world().get_resource::<GrandPrix>()
}

fn state(app: &App) -> Option<ChampionshipState> {
    lobby(app).championship().cloned()
}

/// Short races and short waits: one lap; the host ends a race 3 s after the first player
/// finished and closes its results 1 s after they showed; the standings and the podium show for
/// a second.
pub(super) fn quick(room: &mut Room, host: PeerKey) {
    let app = room.app(host);
    let world = app.world_mut();
    let mut race = world.resource_mut::<RaceTuning>();
    race.laps = 1;
    race.results_close_seconds = 1.0;
    world.resource_mut::<NetTuning>().finish_grace_s = 3.0;
    let mut salon = world.resource_mut::<SalonTuning>();
    salon.standings_seconds = 1.0;
    salon.podium_seconds = 1.5;
}

/// The host chose a championship of `program` (chosen one by one) in the room's settings, and
/// starts it with `bots` bots (fewer than a grid's, for the tests' time) and the players seated:
/// its first race.
pub(super) fn start_championship(room: &mut Room, host: PeerKey, program: &[usize], bots: u8) {
    let keys: Vec<PeerKey> = room.games.iter().map(|(k, _)| *k).collect();
    for key in &keys {
        room.act(*key, NetAction::Ready(true));
    }
    room.until(2.0, "everyone ready", |app| lobby(app).all_ready());
    let setup = RoomSetup {
        championship: true,
        plan: ChampionshipSetup {
            custom: true,
            chosen: true,
            program: Program::new(program),
            ..ChampionshipSetup::default()
        },
        ..RoomSetup::default()
    };
    room.act(host, NetAction::ChooseSetup(setup));
    room.until(2.0, "everyone has the championship's settings", |app| {
        lobby(app).setup() == setup
    });
    let app = room.app(host);
    let mut session = app.world_mut().resource_mut::<NetSession>();
    let lobby = session.lobby.as_mut().unwrap();
    let players = lobby.seats().to_vec();
    assert!(lobby.start_championship(Championship::new(setup.plan, 0xc0ffee, bots, players)));
    room.act(host, NetAction::StartRace);
}

/// Every game on the grid of round `round` of the championship, the players' karts driven by
/// bots.
pub(super) fn race_round(room: &mut Room, round: u8) {
    room.until(15.0, "everyone on the grid", |app| {
        on_grid(app) && app.world().resource::<OnlineRace>().shared.start.round == round
    });
    bots_drive_the_players(room);
}

/// Every game shows the standings of `scored` races (the host's), or its podium.
fn until_stage(room: &mut Room, scored: u8, stage: GrandPrixStage) {
    room.until(150.0, "the championship's stage", |app| {
        state(app).is_some_and(|state| state.scored == scored)
            && grand_prix(app).is_some_and(|gp| gp.stage == stage)
            && app
                .world()
                .get_resource::<Race>()
                .is_some_and(|race| race.phase() == crate::race::RacePhase::Results)
    });
}

/// The points each entrant won in the race of the host's closed results `results` (their grid
/// in `start`), as GD § 1 gives them: from its place.
fn points_of(
    state: &ChampionshipState,
    start: &RaceStart,
    results: &RaceResults,
) -> Vec<(usize, u32)> {
    let table = GrandPrixTuning::default().points;
    results
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.time != ResultTime::GaveUp)
        .map(|(place, line)| {
            let entrant = entrant_at(state, start, usize::from(line.id)).unwrap();
            (entrant, table[place])
        })
        .collect()
}

/// Issue #122 acceptance: the host chooses the mode in the lobby (« Championnat »: a cup, then a
/// custom one at random of five races); the other peer sees the same settings, the same program
/// and the same draw (the host's, from its room's seed), and cannot change them.
#[test]
fn two_peers_see_the_same_championship_and_the_same_draw() {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob")]);
    for key in [ANA, BOB] {
        room.app(key).world_mut().resource_mut::<LobbyWindow>().open = true;
    }
    room.until(2.0, "the panels", |app| {
        panel(app).is_some_and(|view| view.players.len() == 2)
    });
    // « Circuit seul » to « Championnat »: a cup, its four circuits.
    act_on_panel(&mut room, ANA, SalonAction::Step(SalonRow::Mode, 1));
    room.until(2.0, "a championship for everyone", |app| {
        lobby(app).setup().championship
    });
    let cup = lobby(room.view(ANA)).setup();
    assert_eq!(cup.plan.program.as_slice(), [0, 1, 2, 3]);
    assert!(!cup.plan.custom);
    // A custom one, at random, of five races.
    act_on_panel(&mut room, ANA, SalonAction::Step(SalonRow::Formula, 1));
    act_on_panel(&mut room, ANA, SalonAction::Step(SalonRow::Races, 1));
    room.until(2.0, "the same settings everywhere", |app| {
        lobby(app).setup().plan.program.len() == 5
    });
    for _ in 0..10 {
        room.frame();
    }
    let setup = lobby(room.view(ANA)).setup();
    assert_eq!(lobby(room.view(BOB)).setup(), setup);
    assert!(setup.championship && setup.plan.custom && !setup.plan.chosen);
    let drawn = setup.plan.program.circuits();
    let mut once = drawn.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), 5, "no circuit twice: {drawn:?}");
    // The host's draw: its room's seed and the number of its draw.
    let code = room.view(ANA).world().resource::<NetSession>().room.clone();
    let circuits = crate::admin::tracks::circuits().len();
    assert_eq!(
        drawn,
        draw_circuits(room_seed(&code), setup.plan.draw, 5, circuits)
    );
    // Both panels show the same lines, values and program; the guest's cannot change.
    room.until(2.0, "the panels follow", |app| {
        panel(app).is_some_and(|view| {
            view.seat
                .as_ref()
                .is_some_and(|seat| seat.setup == setup && seat.program.len() == 5)
        })
    });
    let (host, guest) = (
        panel(room.view(ANA)).unwrap().seat.unwrap(),
        panel(room.view(BOB)).unwrap().seat.unwrap(),
    );
    assert_eq!(host.rows, guest.rows);
    assert_eq!(host.values, guest.values);
    assert_eq!(host.program, guest.program);
    assert_eq!(host.program_title, "Au programme · 5 courses");
    let catalog = Catalog::game(4);
    for (line, circuit) in guest.program.iter().zip(&drawn) {
        assert!(line.ends_with(catalog.name(*circuit)), "{line}");
    }
    assert!(host.editable && !guest.editable);
    act_on_panel(&mut room, BOB, SalonAction::Step(SalonRow::Mode, 1));
    for _ in 0..10 {
        room.frame();
    }
    assert_eq!(
        lobby(room.view(ANA)).setup(),
        setup,
        "a guest changes nothing"
    );
}

/// Issue #122 acceptance: a championship of two races, two players and two bots: after each race
/// both peers have the host's points (GD § 1, from its closed results), the same overall
/// standings on their screens (their own name « Toi », the other's pseudo); the second race's
/// grid is the reverse of the standings, the same on both, each bot keeping its number (its
/// animal and parts, its name); after the last race, the podium on both (the three first on
/// their steps), then everyone back in the lobby.
#[test]
fn a_championship_online_has_the_same_standings_grid_and_podium_everywhere() {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob")]);
    quick(&mut room, ANA);
    start_championship(&mut room, ANA, &[0, 0], 2);
    race_round(&mut room, 1);
    let first = lobby(room.view(ANA)).announced().cloned().unwrap();
    assert!(
        first.lineup.is_empty(),
        "the first race lines up as any race"
    );
    assert_eq!(lobby(room.view(BOB)).announced(), Some(&first));
    // Bob's screen: the host's bots, their parts from the championship's seed.
    let roster = room.view(BOB).world().resource::<Roster>().clone();
    let bot_parts = |app: &App, bot: u8| {
        app.world()
            .resource::<KartSync>()
            .remotes()
            .iter()
            .find(|remote| remote.bot == Some(bot) && remote.proxy.is_some())
            .map(|remote| (remote.kart, remote.loadout))
    };
    let parts = |bot: usize| {
        let p = bot_loadout(&roster, 0xc0ffee, bot);
        [p.character, p.kart, p.wheels, p.wing].map(|p| p as u8)
    };
    room.until(5.0, "Bob sees the bots", |app| {
        app.world().resource::<NetSession>().hosting || bot_parts(app, 1).is_some()
    });
    assert_eq!(bot_parts(room.view(BOB), 0), Some((0, parts(0))));
    assert_eq!(bot_parts(room.view(BOB), 1), Some((1, parts(1))));

    // The first race's standings, the same everywhere.
    until_stage(&mut room, 1, GrandPrixStage::Standings);
    let (ana, bob) = (
        state(room.view(ANA)).unwrap(),
        state(room.view(BOB)).unwrap(),
    );
    println!("after the first race: {ana:?}");
    assert_eq!(ana, bob);
    let results = room
        .view(ANA)
        .world()
        .resource::<OnlineRace>()
        .shared
        .results()
        .cloned()
        .expect("the host's table");
    for (entrant, won) in points_of(&ana, &first, &results) {
        assert_eq!(u32::from(ana.points[entrant]), won, "entrant {entrant}");
        assert_eq!(u32::from(ana.last[entrant]), won);
    }
    for key in [ANA, BOB] {
        let app = room.app(key);
        let gp = grand_prix(app).unwrap();
        assert_eq!(
            gp.standings(),
            ana.standings
                .iter()
                .map(|e| usize::from(*e))
                .collect::<Vec<_>>()
        );
        let lines = crate::modes::standings_lines(gp);
        let me = if key == ANA { 2 } else { 3 };
        let other = if key == ANA { "Bob" } else { "Ana" };
        let place = |entrant: usize| {
            ana.standings
                .iter()
                .position(|e| usize::from(*e) == entrant)
        };
        assert_eq!(lines[place(me).unwrap()].name, "Toi", "{key:?}");
        assert!(lines[place(me).unwrap()].player);
        assert_eq!(lines[place(5 - me).unwrap()].name, other);
        assert_eq!(lines[place(0).unwrap()].name, "Kart 1");
        let world = app.world_mut();
        let screens: Vec<GrandPrixScreen> = world
            .query::<&GrandPrixScreen>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(
            screens,
            [GrandPrixScreen(GrandPrixStage::Standings)],
            "{key:?}"
        );
        let results = world
            .query::<&crate::ui::ResultsScreen>()
            .iter(world)
            .count();
        assert_eq!(results, 0, "{key:?}: the standings in the results' place");
    }

    // The second race: the reverse of the standings, the same grid on both.
    race_round(&mut room, 2);
    let second = lobby(room.view(ANA)).announced().cloned().unwrap();
    println!("the second race: {second:?}");
    assert_eq!(lobby(room.view(BOB)).announced(), Some(&second));
    assert_eq!(second.seed, first.seed, "the same bots");
    let reverse: Vec<usize> = ana
        .standings
        .iter()
        .rev()
        .map(|e| usize::from(*e))
        .collect();
    let grid: Vec<usize> = (0..4)
        .map(|slot| entrant_at(&ana, &second, slot).unwrap())
        .collect();
    assert_eq!(grid, reverse, "the leader last");
    for (key, me) in [(ANA, 2), (BOB, 3)] {
        let app = room.view(key);
        let slot = reverse.iter().position(|e| *e == me).unwrap();
        assert_eq!(
            app.world().resource::<crate::net::OnlineSlot>().0,
            slot,
            "{key:?} starts where the standings put it"
        );
        let lineup = app
            .world()
            .resource::<crate::race::RaceGrid>()
            .lineup
            .unwrap();
        assert_eq!(
            lineup[..4]
                .iter()
                .map(|e| usize::from(*e))
                .collect::<Vec<_>>(),
            reverse
        );
    }
    // Each bot is still itself on Bob's screen, wherever it starts, with its name.
    room.until(5.0, "Bob sees the bots again", |app| {
        app.world().resource::<NetSession>().hosting || bot_parts(app, 1).is_some()
    });
    for bot in [0u8, 1] {
        let slot = reverse.iter().position(|e| *e == usize::from(bot)).unwrap();
        assert_eq!(
            bot_parts(room.view(BOB), bot),
            Some((slot as u8, parts(usize::from(bot))))
        );
    }
    for (key, app) in &mut room.games {
        let world = app.world_mut();
        let names: Vec<(usize, String)> = world
            .query::<(&crate::race::Racer, &RacerName)>()
            .iter(world)
            .map(|(racer, name)| (racer.grid_slot, name.0.clone()))
            .collect();
        for bot in [0usize, 1] {
            let slot = reverse.iter().position(|e| *e == bot).unwrap();
            if slot != bot {
                assert!(
                    names.contains(&(slot, format!("Kart {}", bot + 1))),
                    "{key:?}: {names:?}"
                );
            }
        }
    }

    // Its standings, the same everywhere: the points of both races.
    until_stage(&mut room, 2, GrandPrixStage::Standings);
    let (ana2, bob2) = (
        state(room.view(ANA)).unwrap(),
        state(room.view(BOB)).unwrap(),
    );
    println!("after the second race: {ana2:?}");
    assert_eq!(ana2, bob2);
    let results2 = room
        .view(ANA)
        .world()
        .resource::<OnlineRace>()
        .shared
        .results()
        .cloned()
        .unwrap();
    let mut expected: Vec<u32> = ana.points.iter().map(|p| u32::from(*p)).collect();
    for (entrant, won) in points_of(&ana2, &second, &results2) {
        expected[entrant] += won;
    }
    assert_eq!(
        ana2.points
            .iter()
            .map(|p| u32::from(*p))
            .collect::<Vec<_>>(),
        expected
    );
    // The standings by points (ties: the last race).
    for pair in ana2.standings.windows(2) {
        let (a, b) = (usize::from(pair[0]), usize::from(pair[1]));
        assert!(ana2.points[a] >= ana2.points[b], "{ana2:?}");
    }

    // The podium on both: the three first on their steps (ours by us, the others' by their
    // states).
    until_stage(&mut room, 2, GrandPrixStage::Podium);
    for _ in 0..5 {
        room.frame();
    }
    for (key, app) in &mut room.games {
        let world = app.world_mut();
        assert_eq!(
            world.query::<&PodiumScene>().iter(world).count(),
            1,
            "{key:?}"
        );
        let mut places: Vec<usize> = world
            .query::<&OnPodium>()
            .iter(world)
            .map(|p| p.0)
            .collect();
        places.sort_unstable();
        assert_eq!(places, [0, 1, 2], "{key:?}");
        let screens: Vec<GrandPrixScreen> = world
            .query::<&GrandPrixScreen>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(
            screens,
            [GrandPrixScreen(GrandPrixStage::Podium)],
            "{key:?}"
        );
    }

    // Then everyone back in the lobby: no more championship.
    room.until(10.0, "everyone back in the lobby", |app| {
        showing(app) == Some(Showing::Salon)
            && grand_prix(app).is_none()
            && lobby(app).championship().is_none()
    });
    assert!(lobby(room.view(BOB)).announced().is_none());
    assert_eq!(lobby(room.view(ANA)).setup(), lobby(room.view(BOB)).setup());
}

/// Issue #122 acceptance: a player who leaves in the middle of a championship: it goes on for
/// the others, the next race's grid closes up over the one who left, who keeps its points and
/// wins nothing more; the same standings for those still there, the podium, the lobby again.
#[test]
fn a_championship_goes_on_when_a_player_leaves() {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd")]);
    quick(&mut room, ANA);
    start_championship(&mut room, ANA, &[0, 0], 1);
    race_round(&mut room, 1);
    until_stage(&mut room, 1, GrandPrixStage::Standings);
    let before = state(room.view(ANA)).unwrap();
    assert_eq!(before.players, [ANA, BOB, CYD]);
    let cyd = before.entrant_of(CYD).unwrap();
    // Cyd leaves (its tab closes).
    room.close(CYD);
    race_round(&mut room, 2);
    let second = lobby(room.view(ANA)).announced().cloned().unwrap();
    assert_eq!(second.players, [ANA, BOB]);
    assert_eq!(second.racers(), 3, "the grid closes up");
    assert_eq!(lobby(room.view(BOB)).announced(), Some(&second));
    until_stage(&mut room, 2, GrandPrixStage::Standings);
    let (ana, bob) = (
        state(room.view(ANA)).unwrap(),
        state(room.view(BOB)).unwrap(),
    );
    assert_eq!(ana, bob);
    assert_eq!(ana.points[cyd], before.points[cyd], "Cyd keeps its points");
    assert_eq!(ana.last[cyd], 0, "and wins nothing more");
    assert_eq!(ana.players, [ANA, BOB, CYD], "still an entrant");
    for key in [ANA, BOB] {
        let gp = grand_prix(room.view(key)).unwrap();
        let lines = crate::modes::standings_lines(gp);
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().any(|line| line.name == "Cyd"), "{lines:?}");
    }
    until_stage(&mut room, 2, GrandPrixStage::Podium);
    room.until(10.0, "everyone back in the lobby", |app| {
        showing(app) == Some(Showing::Salon) && lobby(app).championship().is_none()
    });
}
