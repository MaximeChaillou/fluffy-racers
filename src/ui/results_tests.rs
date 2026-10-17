//! Headless scenario of the results screen (issue #70): the whole game (`GamePlugin`) on the
//! default circuit with `MinimalPlugins`, one fixed tick per `app.update()`.

use std::collections::BTreeMap;

use bevy::ecs::change_detection::Tick;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::format::{hundredths, write_time};
use super::{ResultsCell, ResultsColumn};
use crate::TICK_HZ;
use crate::bots::Bot;
use crate::interpolation::PhysicsPose;
use crate::kart::{KartInput, KartSystems, PlayerKart};
use crate::race::{
    Course, Race, RacePhase, RaceProgress, RaceSystems, RaceTuning, Racer, ResultTime, Standing,
    Tow, estimate_finish, seconds_to_ticks,
};
use crate::tests::{app_with_bots, follow_centerline, player};
use crate::track::{TrackChoice, TrackLayout};

/// The results screen, line by line, from the top: (rank, name, time, detail).
pub(crate) fn results_lines(app: &mut App) -> Vec<[String; 4]> {
    let world = app.world_mut();
    let cells: Vec<(ResultsCell, String)> = world
        .query::<(&ResultsCell, &Text)>()
        .iter(world)
        .map(|(cell, text)| (*cell, text.0.clone()))
        .collect();
    let rows = cells
        .iter()
        .map(|(cell, _)| cell.row + 1)
        .max()
        .unwrap_or(0);
    let mut lines = vec![<[String; 4]>::default(); rows];
    for (cell, text) in cells {
        let column = match cell.column {
            ResultsColumn::Rank => 0,
            ResultsColumn::Name => 1,
            ResultsColumn::Time => 2,
            ResultsColumn::Detail => 3,
        };
        lines[cell.row][column] = text;
    }
    lines
}

/// When the text of each cell of the results screen last changed.
fn text_ticks(app: &mut App) -> BTreeMap<Entity, Tick> {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, Ref<Text>), With<ResultsCell>>()
        .iter(world)
        .map(|(entity, text)| (entity, text.last_changed()))
        .collect()
}

/// The cells whose text was rewritten since `before`.
fn rewritten(app: &mut App, before: &BTreeMap<Entity, Tick>) -> usize {
    text_ticks(app)
        .iter()
        .filter(|(entity, tick)| before.get(*entity) != Some(*tick))
        .count()
}

/// Who drives in this scenario. The player's kart (still the player's: the race waits for it)
/// follows the centerline (the autopilot of #3); the bots stay still until `release`, and
/// `frozen` stays still after it.
#[derive(Resource, Default)]
struct Drivers {
    release: bool,
    frozen: Option<Entity>,
}

/// After the input systems, before the race's gate (which reads the inputs).
fn drive(
    drivers: Res<Drivers>,
    layout: Res<TrackLayout>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &Velocity,
            &mut KartInput,
            Has<PlayerKart>,
        ),
        With<Racer>,
    >,
) {
    for (entity, transform, velocity, mut input, player) in &mut karts {
        if player {
            *input = follow_centerline(&layout, transform, velocity.linear.length());
        } else if !drivers.release || drivers.frozen == Some(entity) {
            *input = KartInput::default();
        }
    }
}

/// Puts `kart` on grid slot `slot` (before GO).
fn move_to_slot(app: &mut App, kart: Entity, slot: usize) {
    let course = app.world().resource::<Course>().clone();
    let spawn = course.grid[slot];
    let mut entity = app.world_mut().entity_mut(kart);
    entity.get_mut::<Racer>().unwrap().grid_slot = slot;
    *entity.get_mut::<Transform>().unwrap() = spawn;
    *entity.get_mut::<Velocity>().unwrap() = Velocity::zero();
    entity.get_mut::<PhysicsPose>().unwrap().snap(spawn);
    entity
        .get_mut::<RaceProgress>()
        .unwrap()
        .reset(&course, spawn.translation);
    entity.get_mut::<Tow>().unwrap().reset(spawn.translation);
}

fn race(app: &App) -> &Race {
    app.world().resource::<Race>()
}

fn progress(app: &App, kart: Entity) -> RaceProgress {
    app.world().get::<RaceProgress>(kart).unwrap().clone()
}

/// A time as the screen writes it.
fn time_text(ticks: u32) -> String {
    let mut out = String::new();
    write_time(&mut out, Some(hundredths(ticks)));
    out
}

/// The line the screen should show, live, for `kart` (with its grid slot `slot`).
fn live_line(app: &App, kart: Entity, rank: usize, slot: usize, player: bool) -> [String; 4] {
    let p = progress(app, kart);
    let rank = if rank == 1 {
        "1er".to_string()
    } else {
        format!("{rank}e")
    };
    let name = if player {
        "Toi".to_string()
    } else {
        format!("Kart {}", slot + 1)
    };
    match p.finish {
        Some(finish) => [
            rank,
            name,
            time_text(finish),
            format!("Meilleur tour {}", time_text(p.best_lap().unwrap())),
        ],
        None => [rank, name, "en course".into(), "Tour 1/1".into()],
    }
}

/// Issue #70 acceptance (solo): the player finishes first, far ahead of the bots. The results
/// screen appears with the bots « en course » (their lap), then each bot's time and best lap
/// come in the frame it crosses the line; the lines follow the standings (a bot that stops is
/// passed: its line goes down); the screen's texts are rewritten only when they change, on the
/// same entities. Once the results close (`RaceTuning::results_close_seconds` after they
/// appeared), the bot still on track has its estimated time, marked, after every finisher; and
/// the table no longer moves, even when that bot then finishes.
#[test]
fn the_results_follow_the_bots_that_finish_then_close_with_estimates() {
    let mut app = app_with_bots(TrackChoice::default(), 3);
    let me = player(&mut app);
    for _ in 0..10 {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        app.update();
    }
    // One lap, the player on the pole (it was behind the 3 bots): nobody in its way.
    app.world_mut().resource_mut::<Course>().laps = 1;
    let mut bots: Vec<(usize, Entity)> = {
        let world = app.world_mut();
        world
            .query_filtered::<(&Racer, Entity), With<Bot>>()
            .iter(world)
            .map(|(racer, entity)| (racer.grid_slot, entity))
            .collect()
    };
    bots.sort();
    assert_eq!(bots.len(), 3);
    let slot = |app: &App, kart: Entity| app.world().get::<Racer>(kart).unwrap().grid_slot;
    let player_slot = slot(&app, me);
    move_to_slot(&mut app, bots[0].1, player_slot);
    move_to_slot(&mut app, me, 0);
    app.init_resource::<Drivers>().add_systems(
        FixedUpdate,
        drive.after(KartSystems::Input).before(RaceSystems::Gate),
    );
    // Closed by hand below, once the bots that drive on finished.
    app.world_mut()
        .resource_mut::<RaceTuning>()
        .results_close_seconds = 600.0;

    for _ in 0..(60.0 * TICK_HZ) as usize {
        if progress(&app, me).finished() {
            break;
        }
        app.update();
    }
    assert!(progress(&app, me).finished(), "{:?}", progress(&app, me));
    assert_eq!(progress(&app, me).rank, 1);
    app.world_mut().resource_mut::<Drivers>().release = true;
    while race(&app).phase() != RacePhase::Results {
        app.update();
    }

    // The screen appears: the player's time, the bots on their lap.
    let lines = results_lines(&mut app);
    let finish = progress(&app, me).finish.unwrap();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0], live_line(&app, me, 1, 0, true));
    assert_eq!(lines[0][2], time_text(finish));
    for (row, line) in lines.iter().enumerate().skip(1) {
        assert_eq!(&line[2..], ["en course", "Tour 1/1"], "{lines:?}");
        assert_eq!(line[0], format!("{}e", row + 1));
    }
    let mut ticks = text_ticks(&mut app);
    let cells: Vec<Entity> = ticks.keys().copied().collect();
    assert_eq!(cells.len(), 16);

    // The first bot of the standings stops: the two others pass it and finish, live.
    let frozen = race(&app).standings[1];
    app.world_mut().resource_mut::<Drivers>().frozen = Some(frozen);
    let mut previous = lines;
    let mut frozen_rows = vec![1];
    let mut finishes_seen = 0;
    for _ in 0..(60.0 * TICK_HZ) as usize {
        app.update();
        let lines = results_lines(&mut app);
        let standings = race(&app).standings.clone();
        for (row, kart) in standings.iter().enumerate() {
            let expected = live_line(&app, *kart, row + 1, slot(&app, *kart), *kart == me);
            assert_eq!(lines[row], expected, "line {row}: {lines:?}");
            if progress(&app, *kart).finish == Some(race(&app).race_ticks()) {
                finishes_seen += 1;
            }
        }
        let row = standings.iter().position(|k| *k == frozen).unwrap();
        if frozen_rows.last() != Some(&row) {
            frozen_rows.push(row);
        }
        // Only what changed is rewritten.
        let changed = rewritten(&mut app, &ticks);
        ticks = text_ticks(&mut app);
        if lines == previous {
            assert_eq!(changed, 0, "texts rewritten without a change");
        } else {
            assert!(changed > 0);
        }
        previous = lines;
        let others_done = bots
            .iter()
            .filter(|(_, bot)| *bot != frozen)
            .all(|(_, bot)| progress(&app, *bot).finished());
        if others_done {
            break;
        }
        assert!(!race(&app).results_closed());
    }
    println!("lines once the bots finished: {previous:?}; the stopped bot's rows {frozen_rows:?}");
    assert_eq!(finishes_seen, 2, "both bots crossed the line on the screen");
    assert!(
        frozen_rows.is_sorted() && frozen_rows.last() == Some(&3),
        "the stopped bot was passed, its line went down: {frozen_rows:?}"
    );
    assert!(!progress(&app, frozen).finished());

    // The results close `results_close_seconds` after they appeared, not before.
    let timings = race(&app).timings;
    let shown_at = race(&app).flow.over_at.unwrap() + timings.finish;
    let elapsed = race(&app).flow.tick - shown_at;
    let seconds = (elapsed as f32 / TICK_HZ as f32).ceil() + 1.0;
    app.world_mut()
        .resource_mut::<RaceTuning>()
        .results_close_seconds = seconds;
    let close_at = shown_at + seconds_to_ticks(seconds);
    while race(&app).flow.tick < close_at {
        assert!(!race(&app).results_closed());
        assert_eq!(&results_lines(&mut app)[3][2..], ["en course", "Tour 1/1"]);
        app.update();
    }
    assert!(race(&app).results_closed());

    // The bot still on track: its estimate, marked, after the finishers.
    let course = app.world().resource::<Course>().clone();
    let tuning = app.world().resource::<RaceTuning>().clone();
    let p = progress(&app, frozen);
    let standing = Standing::of(frozen, &p, &course, false);
    let now = race(&app).race_ticks();
    let estimate = estimate_finish(
        now,
        standing.covered,
        standing.remaining,
        tuning.results_min_speed,
    );
    let times: Vec<ResultTime> = race(&app).results.iter().map(|line| line.time).collect();
    assert_eq!(times[3], ResultTime::Estimated(estimate), "{times:?}");
    assert!(estimate >= now);
    for (row, kart) in race(&app).standings.clone().iter().take(3).enumerate() {
        assert_eq!(race(&app).results[row].id, *kart);
        let finish = progress(&app, *kart).finish.unwrap();
        assert_eq!(times[row], ResultTime::Finished(finish));
        assert!(finish < estimate);
    }
    let lines = results_lines(&mut app);
    println!("closed: {lines:?}");
    assert_eq!(&lines[..3], &previous[..3]);
    assert_eq!(
        lines[3],
        [
            "4e".to_string(),
            format!("Kart {}", slot(&app, frozen) + 1),
            format!("~{}", time_text(estimate)),
            "Temps estimé".to_string(),
        ]
    );

    // Final: the stopped bot drives on and finishes, the table stays as it closed, the same
    // entities, nothing rewritten.
    app.world_mut().resource_mut::<Drivers>().frozen = None;
    app.update();
    let closed = text_ticks(&mut app);
    for _ in 0..(40.0 * TICK_HZ) as usize {
        app.update();
        assert_eq!(rewritten(&mut app, &closed), 0, "the closed table moved");
        if progress(&app, frozen).finished() {
            break;
        }
    }
    assert!(progress(&app, frozen).finished(), "it drove on to the line");
    assert_eq!(results_lines(&mut app), lines);
    let after: Vec<Entity> = closed.keys().copied().collect();
    assert_eq!(after, cells, "the screen was never rebuilt");
}
