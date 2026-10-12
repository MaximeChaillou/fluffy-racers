//! Headless race scenarios (issue #5): the whole game (`GamePlugin`) on Prairie Pompon with
//! `MinimalPlugins`, one fixed tick per `app.update()`.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::TICK_HZ;
use crate::kart::{DriftSide, KartState};
use crate::tests::{app_on, follow_centerline, hold_keys, player};
use crate::track::{Surface, TrackChoice, spawn_static_box};
use crate::ui::{HudText, ResultsScreen};

/// The game on the test circuit, once the race exists.
fn race_app() -> (App, Entity) {
    let mut app = app_on(TrackChoice::default());
    let kart = player(&mut app);
    for _ in 0..10 {
        if app.world().get_resource::<Race>().is_some() {
            return (app, kart);
        }
        app.update();
    }
    panic!("no race on the circuit");
}

fn race(app: &App) -> &Race {
    app.world().resource::<Race>()
}

fn course(app: &App) -> Course {
    app.world().resource::<Course>().clone()
}

fn progress(app: &App, kart: Entity) -> RaceProgress {
    app.world().get::<RaceProgress>(kart).unwrap().clone()
}

fn state(app: &App, kart: Entity) -> KartState {
    *app.world().get::<KartState>(kart).unwrap()
}

fn transform(app: &App, kart: Entity) -> Transform {
    *app.world().get::<Transform>(kart).unwrap()
}

fn tow(app: &App, kart: Entity) -> Tow {
    *app.world().get::<Tow>(kart).unwrap()
}

fn hud_text(app: &mut App, field: HudText) -> String {
    let world = app.world_mut();
    let mut texts = world.query::<(&HudText, &Text)>();
    texts
        .iter(world)
        .find(|(f, _)| **f == field)
        .map(|(_, text)| text.0.clone())
        .unwrap()
}

/// A key pressed this frame (`just_pressed`), as the window would send it.
fn tap(app: &mut App, key_code: KeyCode, logical_key: Key) {
    let window = Entity::PLACEHOLDER;
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: None,
            repeat: false,
            window,
        });
        app.update();
    }
}

/// Lets the test drive the kart: the keyboard no longer does.
fn take_the_wheel(app: &mut App, kart: Entity) {
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
}

/// One tick of the test autopilot.
fn autopilot_tick(app: &mut App, kart: Entity) {
    let layout = app.world().resource::<TrackLayout>().clone();
    let world = app.world();
    let transform = *world.get::<Transform>(kart).unwrap();
    let speed = world.get::<Velocity>(kart).unwrap().linear.length();
    let input = follow_centerline(&layout, &transform, speed);
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
    app.update();
}

/// Runs until GO (the first racing tick).
fn until_go(app: &mut App) {
    for _ in 0..(10.0 * TICK_HZ) as usize {
        if race(app).started() {
            return;
        }
        app.update();
    }
    panic!("the race never started");
}

#[test]
fn the_countdown_holds_the_karts_then_go_releases_them() {
    let (mut app, kart) = race_app();
    let grid = course(&app).grid[0];
    // Full throttle from the very first tick.
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    let timings = race(&app).timings;
    let mut banners = Vec::new();
    let mut phases = Vec::new();
    while !race(&app).started() {
        // Even a boost (debug key B) does not move it.
        if race(&app).flow.tick == RaceFlow::two_tick(&timings) {
            app.world_mut()
                .get_mut::<Boost>(kart)
                .unwrap()
                .add(0.3, 2.0);
        }
        app.update();
        let tick = race(&app).flow.tick;
        assert!(tick < 10 * 60, "no GO");
        let phase = race(&app).phase();
        if phases.last() != Some(&phase) {
            phases.push(phase);
        }
        let banner = hud_text(&mut app, HudText::Banner);
        if banners.last() != Some(&banner) {
            banners.push(banner);
        }
        if !race(&app).started() {
            let position = transform(&app, kart).translation;
            let moved = (position - grid.translation).with_y(0.0).length();
            assert!(moved < 0.05, "moved {moved} m before GO (tick {tick})");
            let velocity = app.world().get::<Velocity>(kart).unwrap().linear;
            let speed = velocity.with_y(0.0).length();
            assert!(speed < 0.2, "speed {speed} before GO");
        }
    }
    assert_eq!(
        phases,
        vec![RacePhase::Grid, RacePhase::Countdown, RacePhase::Racing]
    );
    assert_eq!(banners, vec!["", "3", "2", "1", "GO !"]);
    // 1 s of grid + 3 s of countdown: GO is the 241st tick.
    assert_eq!(race(&app).flow.tick, RaceFlow::go_tick(&timings));
    assert_eq!(RaceFlow::go_tick(&timings), 241);
    // The kart moved on the GO tick itself.
    let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
    assert!(speed > 0.1, "speed {speed} on GO");
    // HUD before anything happens: pole, alone, lap 1.
    assert_eq!(hud_text(&mut app, HudText::Rank), "1er");
    assert_eq!(hud_text(&mut app, HudText::RacerCount), "/ 1");
    assert_eq!(hud_text(&mut app, HudText::Lap), "Tour 1/3");
    assert_eq!(
        hud_text(&mut app, HudText::BestLap),
        "Meilleur tour -:--.--"
    );
    // One second later the kart is well on its way.
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(hud_text(&mut app, HudText::Banner), "");
    assert_eq!(hud_text(&mut app, HudText::Chrono), "0:01.01");
    let moved = transform(&app, kart).translation.distance(grid.translation);
    assert!(moved > 5.0, "{moved}");
}

/// Issue #5 acceptance: a full race is playable solo. The autopilot of #3 drives 3 laps; the
/// laps, times and HUD are consistent; the finish banner, the results and a restart follow.
#[test]
fn a_full_three_lap_race_then_results_and_restart() {
    let (mut app, kart) = race_app();
    take_the_wheel(&mut app, kart);
    let course = course(&app);
    let length = course.length();

    let mut racing_ticks = 0;
    let mut lap_texts = Vec::new();
    let mut max_ticks = (150.0 * TICK_HZ) as usize;
    while race(&app).phase() != RacePhase::Finished {
        max_ticks -= 1;
        assert!(
            max_ticks > 0,
            "the race never finished: {:?}",
            progress(&app, kart)
        );
        autopilot_tick(&mut app, kart);
        let p = progress(&app, kart);
        if race(&app).started() && (!p.finished() || p.finish == Some(race(&app).race_ticks())) {
            racing_ticks += 1;
        }
        let lap = hud_text(&mut app, HudText::Lap);
        if lap_texts.last() != Some(&lap) {
            lap_texts.push(lap);
        }
        // Never towed: the autopilot stays on the track.
        assert!(!tow(&app, kart).is_carrying());
    }
    let p = progress(&app, kart);
    assert_eq!(lap_texts, vec!["Tour 1/3", "Tour 2/3", "Tour 3/3"]);
    assert!(p.finished());
    assert_eq!(p.passed, course.checkpoints_to_finish());
    assert_eq!(p.lap_times.len(), 3);
    // About 20 s a lap for this autopilot (#3); the first lap starts on the grid.
    for (i, lap) in p.lap_times.iter().enumerate() {
        let seconds = ticks_to_seconds(*lap);
        assert!((15.0..30.0).contains(&seconds), "lap {i}: {seconds} s");
    }
    let total = p.finish.unwrap();
    assert_eq!(p.lap_times.iter().sum::<u32>(), total);
    assert_eq!(total, racing_ticks, "the chrono counts the ticks from GO");
    assert_eq!(p.best_lap(), p.lap_times.iter().copied().min());
    // The finish is on the start line.
    let finish = course.project(p.passed - 1, transform(&app, kart).translation);
    assert!(
        (finish.distance - course.finish_distance()).abs() < 3.0,
        "finished {} m from the line",
        finish.distance - 3.0 * length
    );
    assert_eq!(p.rank, 1);
    assert_eq!(p.gap_to_leader, 0.0);
    assert_eq!(hud_text(&mut app, HudText::Banner), "FINI !");
    let mut expected = String::new();
    crate::ui::format::write_time(&mut expected, Some(crate::ui::format::hundredths(total)));
    assert_eq!(hud_text(&mut app, HudText::Chrono), expected);

    // The finished kart coasts to a stop; the clock does not move for it.
    let timings = race(&app).timings;
    for _ in 0..timings.finish {
        autopilot_tick(&mut app, kart);
        assert_eq!(app.world().get::<KartInput>(kart).unwrap().throttle, 0.0);
    }
    assert_eq!(race(&app).phase(), RacePhase::Results);
    assert_eq!(hud_text(&mut app, HudText::Chrono), expected);
    let screens = app
        .world_mut()
        .query_filtered::<(), With<ResultsScreen>>()
        .iter(app.world())
        .count();
    assert_eq!(screens, 1);
    let world = app.world_mut();
    let texts: Vec<String> = world
        .query::<&Text>()
        .iter(world)
        .map(|t| t.0.clone())
        .collect();
    assert!(texts.iter().any(|t| t == "CLASSEMENT"));
    assert!(texts.iter().any(|t| t == "Kart 1"), "{texts:?}");
    assert!(texts.contains(&expected));

    // Enter too early (still the restart delay): ignored. Then it restarts.
    tap(&mut app, KeyCode::Enter, Key::Enter);
    assert_eq!(race(&app).phase(), RacePhase::Results);
    for _ in 0..timings.restart_delay {
        app.update();
    }
    tap(&mut app, KeyCode::Enter, Key::Enter);
    assert_eq!(race(&app).phase(), RacePhase::Grid);
    let p = progress(&app, kart);
    assert_eq!((p.passed, p.finish, p.lap_times.len()), (0, None, 0));
    let position = transform(&app, kart).translation;
    assert!(
        position.distance(course.grid[0].translation) < 0.2,
        "{position}"
    );
    assert_eq!(state(&app, kart), KartState::Grounded);
    assert_eq!(hud_text(&mut app, HudText::Lap), "Tour 1/3");
    assert_eq!(hud_text(&mut app, HudText::Chrono), "0:00.00");
    let screens = app
        .world_mut()
        .query_filtered::<(), With<ResultsScreen>>()
        .iter(app.world())
        .count();
    assert_eq!(screens, 0);
    // And the countdown holds it again.
    for _ in 0..120 {
        autopilot_tick(&mut app, kart);
    }
    assert_eq!(race(&app).phase(), RacePhase::Countdown);
    assert!(transform(&app, kart).translation.distance(position) < 0.1);
}

/// Drives with the autopilot until `passed` checkpoints are validated.
fn drive_until_passed(app: &mut App, kart: Entity, passed: u32) {
    until_go(app);
    for _ in 0..(60.0 * TICK_HZ) as usize {
        if progress(app, kart).passed >= passed {
            return;
        }
        autopilot_tick(app, kart);
    }
    panic!("checkpoint {passed} never reached");
}

/// Runs ticks with `input` until the pelican drops the kart; returns the ticks spent
/// `Respawning` (the tick it was picked up included) and the highest point of the carry.
fn until_dropped(app: &mut App, kart: Entity, input: KartInput) -> (u32, f32) {
    let mut respawning = 0;
    let mut highest = f32::MIN;
    let mut pelicans_seen = 0;
    while state(app, kart) == KartState::Respawning {
        respawning += 1;
        assert!(respawning < 400, "never dropped");
        highest = highest.max(transform(app, kart).translation.y);
        let world = app.world_mut();
        pelicans_seen = pelicans_seen.max(world.query::<&PelicanVisual>().iter(world).count());
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
        app.update();
    }
    assert_eq!(pelicans_seen, 1, "the pelican was not shown");
    let world = app.world_mut();
    assert_eq!(world.query::<&PelicanVisual>().iter(world).count(), 0);
    (respawning, highest)
}

/// Checks the kart was dropped on the drop point of its last checkpoint, at a standstill.
fn assert_dropped_on_last_safe_point(app: &App, kart: Entity) {
    let course = course(app);
    let tuning = app.world().resource::<RaceTuning>().clone();
    let p = progress(app, kart);
    let expected = course.drop_transform(p.passed as i64 - 1, 0.0, tuning.tow_drop_lane);
    let t = transform(app, kart);
    assert!(
        t.translation.distance(expected.translation) < 0.05,
        "dropped at {} instead of {}",
        t.translation,
        expected.translation
    );
    assert!(t.forward().dot(*expected.forward()) > 0.999);
    let velocity = app.world().get::<Velocity>(kart).unwrap();
    assert!(velocity.linear.length() < 1e-3);
    assert_eq!(state(app, kart), KartState::Grounded);
    assert!(!app.world().get::<Boost>(kart).unwrap().is_active());
    assert!(!tow(app, kart).is_carrying());
}

#[test]
fn the_pelican_brings_back_a_kart_that_fell_and_cancels_its_drift() {
    let (mut app, kart) = race_app();
    take_the_wheel(&mut app, kart);
    drive_until_passed(&mut app, kart, 5);
    let passed = progress(&app, kart).passed;
    let drop = course(&app).drop_transform(passed as i64 - 1, 0.0, 0.4);

    // Mid-drift with a charged mini-turbo and a boost, the kart falls under the track.
    {
        let world = app.world_mut();
        *world.get_mut::<KartState>(kart).unwrap() = KartState::Drifting {
            side: DriftSide::Right,
            charge: 600.0,
        };
        world.get_mut::<Boost>(kart).unwrap().add(0.3, 2.0);
        world.get_mut::<Transform>(kart).unwrap().translation.y -= 30.0;
    }
    let held = KartInput {
        throttle: 1.0,
        jump_drift: true,
        ..default()
    };
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = held;
    app.update();
    assert_eq!(state(&app, kart), KartState::Respawning);
    assert_eq!(tow(&app, kart).carry.unwrap().reason, TowReason::Fell);

    // Releasing the drift button meanwhile fires nothing.
    let released = KartInput {
        throttle: 1.0,
        ..default()
    };
    let (respawning, highest) = until_dropped(&mut app, kart, released);
    // 2.5 s lost (150 ticks).
    assert_eq!(respawning, 150);
    assert!(highest > drop.translation.y + 5.0, "{highest}");
    assert_eq!(progress(&app, kart).passed, passed, "no checkpoint lost");
    assert_dropped_on_last_safe_point(&app, kart);

    // It drives on and validates the next checkpoints.
    for _ in 0..(4.0 * TICK_HZ) as usize {
        autopilot_tick(&mut app, kart);
    }
    assert!(progress(&app, kart).passed > passed);
    assert_eq!(state(&app, kart), KartState::Grounded);
}

#[test]
fn the_pelican_rescues_a_kart_stuck_against_a_wall() {
    let (mut app, kart) = race_app();
    take_the_wheel(&mut app, kart);
    // A high wall across the road, 3 m in front of the kart on the grid.
    let start = transform(&app, kart);
    let mut commands = app.world_mut().commands();
    spawn_static_box(
        &mut commands,
        Transform::from_translation(start.translation + *start.forward() * 4.0)
            .with_rotation(start.rotation),
        Vec3::new(12.0, 3.0, 0.5),
        Surface::Road,
        crate::track::WALL_RESTITUTION,
    );
    app.world_mut().flush();
    until_go(&mut app);

    let push = KartInput {
        throttle: 1.0,
        ..default()
    };
    let mut ticks = 0;
    while state(&app, kart) != KartState::Respawning {
        ticks += 1;
        assert!(ticks < 600, "never rescued");
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = push;
        app.update();
    }
    assert_eq!(tow(&app, kart).carry.unwrap().reason, TowReason::Stuck);
    // 3 s against the wall, plus the short run-up and the first bounces (3.43 s measured).
    let seconds = ticks_to_seconds(ticks);
    assert!((3.0..4.5).contains(&seconds), "rescued after {seconds} s");
    let (respawning, _) = until_dropped(&mut app, kart, push);
    assert_eq!(respawning, 150);
    // Not a single checkpoint yet: back behind the start line.
    assert_eq!(progress(&app, kart).passed, 0);
    assert_dropped_on_last_safe_point(&app, kart);
}

#[test]
fn a_kart_out_of_the_track_is_towed_back_after_one_second() {
    let (mut app, kart) = race_app();
    take_the_wheel(&mut app, kart);
    drive_until_passed(&mut app, kart, 3);
    let course = course(&app);
    let passed = progress(&app, kart).passed;
    // Teleported into the grass of the infield, far beyond the walls.
    let here = course.project(passed, transform(&app, kart).translation);
    let sample = course.center.at(here.distance);
    let grass = sample.position + sample.right * (here.wall + 10.0) + Vec3::Y * 0.3;
    {
        let world = app.world_mut();
        let mut t = world.get_mut::<Transform>(kart).unwrap();
        t.translation = grass;
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
    }
    let idle = KartInput::default();
    let mut ticks = 0;
    while state(&app, kart) != KartState::Respawning {
        ticks += 1;
        assert!(ticks < 200, "never towed");
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = idle;
        app.update();
    }
    assert_eq!(tow(&app, kart).carry.unwrap().reason, TowReason::OutOfTrack);
    assert_eq!(ticks, 60, "one second out of the track");
    let (respawning, _) = until_dropped(&mut app, kart, idle);
    assert_eq!(respawning, 150);
    assert_eq!(progress(&app, kart).passed, passed);
    assert_dropped_on_last_safe_point(&app, kart);
}

#[test]
fn the_r_key_calls_the_pelican_during_the_race_only() {
    let (mut app, kart) = race_app();
    // On the grid: nothing.
    tap(&mut app, KeyCode::KeyR, Key::Character("r".into()));
    assert!(!tow(&app, kart).is_carrying());
    assert!(!tow(&app, kart).requested);
    until_go(&mut app);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    for _ in 0..60 {
        app.update();
    }
    tap(&mut app, KeyCode::KeyR, Key::Character("r".into()));
    let carry = tow(&app, kart).carry.expect("the pelican did not come");
    assert_eq!(carry.reason, TowReason::Requested);
    assert_eq!(state(&app, kart), KartState::Respawning);
}

#[test]
fn the_test_ground_has_no_race() {
    let mut app = app_on(TrackChoice::TestGround);
    let kart = player(&mut app);
    for _ in 0..30 {
        app.update();
    }
    assert!(app.world().get_resource::<Race>().is_none());
    assert!(app.world().get::<Racer>(kart).is_none());
    assert!(app.world().get::<crate::kart::SpawnPoint>(kart).is_some());
    let world = app.world_mut();
    assert_eq!(world.query::<&crate::ui::HudRoot>().iter(world).count(), 0);
}
