//! Issue #99: the host's settings of the room online (the circuit, the mirror, the bots, the
//! items), headless: two whole games in a room linked in memory (`race_tests`). Both peers have
//! the same settings, drive with them (free driving and the race), and get their solo ones back
//! out of the room. Issue #153: the level of the room's bots.

use super::race_tests::{ANA, BOB, lobby, on_grid, seated, showing};
use super::*;
use crate::bots::{Bot, BotDifficulty};
use crate::items::ItemBox;
use crate::race::{RaceGrid, Racer};
use crate::track::{CroquetteSpot, ItemBoxSpot, MirrorMode, NoItemBoxes, TrackLayout};

fn count<C: Component>(app: &App) -> usize {
    let mut world_query = app.world().try_query_filtered::<(), With<C>>();
    world_query
        .as_mut()
        .map_or(0, |query| query.iter(app.world()).count())
}

/// The circuit built on `app`: its name and whether it is mirrored.
fn built(app: &App) -> Option<(String, bool)> {
    app.world()
        .get_resource::<TrackLayout>()
        .map(|layout| (layout.name.clone(), layout.mirrored))
}

/// Issue #99 acceptance, online: the host chooses the room's settings (Crique Câline, mirrored,
/// without bots nor items): the guest sees the same, both drive freely on the mirrored circuit
/// without any item box (the croquettes stay); the race started takes them: the same circuit,
/// mirrored, without boxes, and only the two players on the grid (no bot); a guest cannot choose
/// them. Out of the room, the guest gets its solo settings back (not mirrored, items on).
#[test]
fn the_hosts_settings_are_everyones() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    let crique = crate::admin::tracks::circuits()
        .iter()
        .position(|entry| entry.name == "Crique Câline")
        .unwrap() as u8;
    let setup = RoomSetup {
        track: crique,
        mirror: true,
        bots: false,
        items: false,
        ..RoomSetup::default()
    };
    // A guest cannot choose.
    room.act(BOB, NetAction::ChooseSetup(setup));
    for _ in 0..30 {
        room.frame();
    }
    for (_, app) in &room.games {
        assert_eq!(lobby(app).setup(), RoomSetup::default());
    }
    room.act(ANA, NetAction::ChooseSetup(setup));
    room.until(10.0, "free driving with the host's settings", |app| {
        lobby(app).setup() == setup
            && built(app) == Some(("Crique Câline".to_string(), true))
            && count::<CroquetteSpot>(app) > 0
    });
    for (_, app) in &room.games {
        assert_eq!(count::<ItemBoxSpot>(app), 0, "no item box");
        assert_eq!(count::<ItemBox>(app), 0);
        assert!(app.world().contains_resource::<NoItemBoxes>());
        assert!(app.world().resource::<MirrorMode>().0);
        assert!(matches!(showing(app), Some(Showing::Free(shown)) if shown.track == crique));
    }

    // The race: the room's settings, the same for both.
    for key in [ANA, BOB] {
        room.act(key, NetAction::Ready(true));
    }
    room.until(2.0, "everyone ready", |app| lobby(app).all_ready());
    let app = room.app(ANA);
    let now = now_ms(app.world().resource::<Time<Real>>());
    let session = app.world().resource::<NetSession>();
    let start = race_to_start(
        session.lobby.as_ref().unwrap(),
        now,
        0xbeef,
        1,
        &NetTuning::default(),
    )
    .expect("the host knows the room's clock");
    assert_eq!(start.track, crique);
    assert_eq!(start.bots, 0, "without bots: the players only");
    assert!(start.mirror && !start.items);
    let mut session = app.world_mut().resource_mut::<NetSession>();
    assert!(session.lobby.as_mut().unwrap().announce_race(start.clone()));
    room.until(10.0, "everyone on the grid", on_grid);
    room.until(5.0, "both karts on both grids", |app| {
        let racers = count::<Racer>(app);
        racers == 2 && built(app) == Some(("Crique Câline".to_string(), true))
    });
    for (key, app) in &room.games {
        assert_eq!(lobby(app).announced(), Some(&start), "{key:?}");
        assert_eq!(
            count::<ItemBoxSpot>(app),
            0,
            "{key:?}: no item box in the race"
        );
        assert!(count::<CroquetteSpot>(app) > 0);
        assert_eq!(app.world().resource::<crate::race::RaceGrid>().bots, 0);
    }

    // Out of the room: the guest's solo settings come back.
    room.act(BOB, NetAction::Leave);
    room.until(10.0, "the guest back on its own level", |app| {
        app.world().get_resource::<OnlineLevel>().is_none() || lobby(app).seats().len() == 1
    });
    let bob = room.view(BOB);
    assert!(
        !bob.world().resource::<MirrorMode>().0,
        "the solo mirror back"
    );
    assert!(
        !bob.world().contains_resource::<NoItemBoxes>(),
        "the solo items back"
    );
}

/// Issue #153 acceptance, online: the host chooses the level of the room's bots (Difficile, then
/// Facile): the guest has the same settings (« Bots : difficile » in its line); the race started
/// carries it: both peers have the same start (its bots, their level, their seed) and the same
/// grid (its level), the host drives its 10 bots, all of that level, and the guest shows these
/// same bots (as many racers, on the same slots).
#[test]
fn the_hosts_bots_level_is_everyones() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    for key in [ANA, BOB] {
        room.act(key, NetAction::Ready(true));
    }
    room.until(2.0, "everyone ready", |app| lobby(app).all_ready());
    for (difficulty, level, name) in [
        (2, BotDifficulty::Hard, "difficile"),
        (0, BotDifficulty::Easy, "facile"),
    ] {
        let setup = RoomSetup {
            difficulty,
            ..RoomSetup::default()
        };
        room.act(ANA, NetAction::ChooseSetup(setup));
        room.until(10.0, "the host's level everywhere", |app| {
            lobby(app).setup() == setup && matches!(showing(app), Some(Showing::Free(_)))
        });
        assert!(
            settings_line(lobby(room.view(BOB)).setup()).contains(&format!("Bots : {name} ·")),
            "{}",
            settings_line(lobby(room.view(BOB)).setup())
        );
        for key in [ANA, BOB] {
            room.act(key, NetAction::Ready(true));
        }
        room.until(2.0, "everyone ready", |app| lobby(app).all_ready());
        room.act(ANA, NetAction::StartRace);
        room.until(10.0, "everyone on the grid", on_grid);
        room.until(10.0, "the 12 karts on both grids", |app| {
            count::<Racer>(app) == 12
        });
        let starts: Vec<RaceStart> = room
            .games
            .iter()
            .map(|(_, app)| lobby(app).announced().cloned().unwrap())
            .collect();
        assert_eq!(starts[0], starts[1]);
        assert_eq!((starts[0].bots, starts[0].difficulty), (10, difficulty));
        for (key, app) in &room.games {
            let grid = app.world().resource::<RaceGrid>();
            assert_eq!(grid.difficulty, level, "{key:?}");
            assert_eq!(grid.seed, starts[0].seed, "{key:?}");
            let mut bots = app.world().try_query::<&Bot>().unwrap();
            let levels: Vec<BotDifficulty> =
                bots.iter(app.world()).map(|bot| bot.difficulty).collect();
            if *key == ANA {
                assert_eq!(levels, vec![level; 10], "the host drives them");
            } else {
                assert!(levels.is_empty(), "the guest shows the host's");
            }
        }
        let slots = |app: &App| {
            let mut racers = app.world().try_query::<&Racer>().unwrap();
            let mut slots: Vec<usize> = racers.iter(app.world()).map(|r| r.grid_slot).collect();
            slots.sort_unstable();
            slots
        };
        assert_eq!(slots(room.view(ANA)), slots(room.view(BOB)));
        assert_eq!(slots(room.view(ANA)), (0..12).collect::<Vec<_>>());
        // Back to the lobby for the next level: new settings end the race.
    }
}
