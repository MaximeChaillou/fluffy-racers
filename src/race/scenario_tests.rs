//! Headless race scenarios (issues #5 and #13): the whole game (`GamePlugin`) on Prairie Pompon
//! with `MinimalPlugins`, one fixed tick per `app.update()`.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::TICK_HZ;
use crate::bots::RacingLine;
use crate::kart::{
    BoostFlame, DriftSide, KartState, KartTuning, MiniTurbo, Stall, StallSmoke, TrickTracker,
    flame_size,
};
use crate::tests::{app_on, follow_centerline, hold_keys, player};
use crate::track::{
    BoostPad, Surface, TrackChoice, TrackData, TrackHandle, TrickRamp, spawn_static_box,
};
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

/// Issue #17: looking behind works on the grid too: the gate holds every other input before GO,
/// but lets the look-back button through (only the camera reads it). The kart stays put, and
/// it neither stalls nor gets a rocket start from it.
#[test]
fn looking_behind_works_during_the_countdown() {
    let (mut app, kart) = race_app();
    let grid = transform(&app, kart);
    hold_keys(&mut app, &[KeyCode::KeyC]);
    for _ in 0..(1.5 * TICK_HZ) as usize {
        app.update();
    }
    assert!(!race(&app).started());
    let input = *app.world().get::<KartInput>(kart).unwrap();
    assert_eq!(
        input,
        KartInput {
            look_back: true,
            ..default()
        }
    );
    let world = app.world_mut();
    let camera = world
        .query_filtered::<&Transform, With<crate::camera::ChaseCamera>>()
        .single(world)
        .unwrap();
    // In front of the kart, looking back over it.
    let ahead = *grid.forward();
    assert!((camera.translation - grid.translation).dot(ahead) > 2.0);
    assert!(camera.forward().dot(ahead) < -0.5);
    hold_keys(&mut app, &[]);
    until_go(&mut app);
    for _ in 0..30 {
        app.update();
    }
    assert!(transform(&app, kart).translation.distance(grid.translation) < 0.05);
    assert!(!app.world().get::<Stall>(kart).unwrap().is_active());
    assert!(!app.world().get::<Boost>(kart).unwrap().is_active());
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
    // Issue #13: the accelerator was held since the grid, before the "2": the engine stalls
    // (GD § 4), the kart stays on the grid one more second. A kart pressing after the "2" (or
    // not at all) leaves on the GO tick itself: see the rocket start tests below.
    assert!(app.world().get::<Stall>(kart).unwrap().is_active());
    let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
    assert!(speed < 0.2, "speed {speed} on GO, stalled");
    // HUD before anything happens: pole, alone, lap 1.
    assert_eq!(hud_text(&mut app, HudText::Rank), "1er");
    assert_eq!(hud_text(&mut app, HudText::RacerCount), "/ 1");
    assert_eq!(hud_text(&mut app, HudText::Lap), "Tour 1/3");
    assert_eq!(
        hud_text(&mut app, HudText::BestLap),
        "Meilleur tour -:--.--"
    );
    // One second later the stall is over: the kart did not move meanwhile.
    for _ in 0..60 {
        let moved = (transform(&app, kart).translation - grid.translation)
            .with_y(0.0)
            .length();
        assert!(moved < 0.05, "moved {moved} m while stalled");
        app.update();
    }
    assert_eq!(hud_text(&mut app, HudText::Banner), "");
    assert_eq!(hud_text(&mut app, HudText::Chrono), "0:01.01");
    assert!(!app.world().get::<Stall>(kart).unwrap().is_active());
    // One more second: the kart is well on its way.
    for _ in 0..60 {
        app.update();
    }
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

// --- Issue #13: rocket start, the circuit's boost pads and trick ramp -----------------------------

/// Runs the countdown with the accelerator first pressed `press` ticks after the "2" (negative:
/// before it; `None`: never) and held, until GO (the first racing tick has run).
fn start_pressing(app: &mut App, kart: Entity, press: Option<i64>) {
    let two = RaceFlow::two_tick(&race(app).timings) as i64;
    while !race(app).started() {
        // The tick about to run.
        let tick = race(app).flow.tick as i64 + 1;
        let throttle = if press.is_some_and(|p| tick >= two + p) {
            1.0
        } else {
            0.0
        };
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = KartInput {
            throttle,
            ..default()
        };
        app.update();
        assert!(race(app).flow.tick < 600, "no GO");
    }
}

/// Visible entities with the visual marker `T`.
fn shown<T: Component>(app: &mut App) -> usize {
    let world = app.world_mut();
    world
        .query_filtered::<&Visibility, With<T>>()
        .iter(world)
        .filter(|v| **v != Visibility::Hidden)
        .count()
}

/// Issue #13 acceptance: accelerating 0.1 s, 0.6 s and 1.2 s after the "2" gives a rocket start
/// boost of 2.5 s, of about 1.6 s, then none (GD § 4). The kart leaves on the GO tick every
/// time; the exhaust flames are bigger for a better start.
#[test]
fn rocket_start_pressed_0_1_0_6_and_1_2_s_after_the_two() {
    let tuning = KartTuning::default();
    let mut flames = Vec::new();
    let cases: [(f32, Option<f32>); 3] = [(0.1, Some(2.5)), (0.6, Some(1.567)), (1.2, None)];
    for (seconds, expected) in cases {
        let (mut app, kart) = race_app();
        take_the_wheel(&mut app, kart);
        let grid = transform(&app, kart).translation;
        let press = (seconds as f64 * TICK_HZ).round() as i64;
        start_pressing(&mut app, kart, Some(press));
        assert_eq!(race(&app).flow.tick, RaceFlow::go_tick(&race(&app).timings));
        let boost = *app.world().get::<Boost>(kart).unwrap();
        assert!(!app.world().get::<Stall>(kart).unwrap().is_active());
        match expected {
            Some(duration) => {
                assert!(
                    (boost.remaining - duration).abs() < 0.01,
                    "{seconds} s: {boost:?}"
                );
                assert_eq!(boost.strength, tuning.rocket_start_strength);
            }
            None => assert!(!boost.is_active(), "{seconds} s: {boost:?}"),
        }
        // It leaves on the GO tick itself.
        let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
        assert!(speed > 0.1, "{seconds} s: speed {speed} on GO");
        // Exhaust flames during the boost only, sized by the start.
        let flame_count = shown::<BoostFlame>(&mut app);
        assert_eq!(flame_count, if expected.is_some() { 2 } else { 0 });
        flames.push(flame_size(&tuning, &boost));
        assert_eq!(shown::<StallSmoke>(&mut app), 0);
        // The boost counts down normally and pushes past the top speed. (1.5 s only: further
        // on, the kart would reach the boost pad of the start straight.)
        let mut top = 0.0f32;
        for tick in 1..=(1.5 * TICK_HZ) as usize {
            app.update();
            let velocity = app.world().get::<Velocity>(kart).unwrap().linear;
            top = top.max(velocity.length());
            let now = *app.world().get::<Boost>(kart).unwrap();
            if let Some(duration) = expected {
                let left = duration - tick as f32 / TICK_HZ as f32;
                if left > 0.02 {
                    assert!((now.remaining - left).abs() < 0.01, "{seconds} s: {now:?}");
                }
            } else {
                assert!(!now.is_active(), "{seconds} s: {now:?}");
            }
        }
        if expected.is_some() {
            assert!(top > 1.15 * tuning.max_speed, "{seconds} s: top {top}");
        } else {
            assert!(top <= tuning.max_speed * 1.01, "{top}");
        }
        let moved = transform(&app, kart).translation.distance(grid);
        assert!(moved > 15.0, "{seconds} s: {moved} m in 1.5 s");
    }
    assert!(flames[0] > flames[1] && flames[1] > flames[2], "{flames:?}");
    assert_eq!(flames[2], 0.0);
}

/// Issue #13 acceptance: accelerating before the "2" stalls the engine: the kart stays still
/// for 1 s after GO, with grey puffs at the exhaust, then drives off. Holding the accelerator
/// from the very first tick on the grid does the same; never pressing is a normal start.
#[test]
fn accelerating_before_the_two_stalls_the_engine_for_one_second() {
    for press in [Some(-30), Some(-1000), None] {
        let (mut app, kart) = race_app();
        take_the_wheel(&mut app, kart);
        let grid = transform(&app, kart).translation;
        start_pressing(&mut app, kart, press);
        assert!(!app.world().get::<Boost>(kart).unwrap().is_active());
        if press.is_none() {
            // Never pressed: nothing special; it does not move without throttle.
            assert!(!app.world().get::<Stall>(kart).unwrap().is_active());
            assert_eq!(shown::<StallSmoke>(&mut app), 0);
            continue;
        }
        let stall = *app.world().get::<Stall>(kart).unwrap();
        assert!((stall.remaining - 1.0).abs() < 1e-5, "{press:?}: {stall:?}");
        assert_eq!(shown::<StallSmoke>(&mut app), 2);
        // Stalled ticks: GO, then every tick that still has the stall once it ran.
        let mut stalled_ticks = 1;
        loop {
            let moved = (transform(&app, kart).translation - grid)
                .with_y(0.0)
                .length();
            assert!(moved < 0.05, "{press:?}: moved {moved} m while stalled");
            app.update();
            if !app.world().get::<Stall>(kart).unwrap().is_active() {
                break;
            }
            stalled_ticks += 1;
            assert!(stalled_ticks < 100);
        }
        // GO plus 59 ticks: one second.
        assert_eq!(stalled_ticks, 60, "{press:?}");
        assert_eq!(shown::<StallSmoke>(&mut app), 0);
        for _ in 0..60 {
            app.update();
        }
        let moved = transform(&app, kart).translation.distance(grid);
        assert!(moved > 5.0, "{press:?}: {moved} m");
        assert!(!app.world().get::<Boost>(kart).unwrap().is_active());
    }
}

/// Issue #13: the boost pads and trick ramp of the circuit file are on the track and work: the
/// kart rolls over the start straight's pad and gets its boost; then the autopilot takes off
/// from the trick ramp of the back straight, a press makes a trick and the landing gives the
/// trick boost.
#[test]
fn the_circuit_boost_pads_and_trick_ramp_work() {
    let (mut app, kart) = race_app();
    take_the_wheel(&mut app, kart);
    let tuning = KartTuning::default();
    let handle = app.world().resource::<TrackHandle>().0.clone();
    let data = app
        .world()
        .resource::<Assets<TrackData>>()
        .get(&handle)
        .unwrap()
        .clone();
    let world = app.world_mut();
    let pads: Vec<Transform> = world
        .query_filtered::<&Transform, With<BoostPad>>()
        .iter(world)
        .copied()
        .collect();
    assert_eq!(pads.len(), data.boost_pads.len());
    assert!(pads.len() >= 3, "{} pads", pads.len());
    let ramps = world
        .query_filtered::<(), With<TrickRamp>>()
        .iter(world)
        .count();
    assert_eq!(ramps, data.trick_ramps.len());

    // The pad of the start straight: the closest one ahead of the grid.
    until_go(&mut app);
    let start = transform(&app, kart);
    let pad = pads
        .iter()
        .filter(|p| (p.translation - start.translation).dot(*start.forward()) > 0.0)
        .min_by(|a, b| {
            let d = |p: &Transform| p.translation.distance(start.translation);
            d(a).total_cmp(&d(b))
        })
        .copied()
        .unwrap();
    let mut on_pad = false;
    for _ in 0..(6.0 * TICK_HZ) as usize {
        let t = transform(&app, kart);
        let to_pad = pad.translation - t.translation;
        let angle = to_pad.dot(*t.right()).atan2(to_pad.dot(*t.forward()));
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = KartInput {
            throttle: 1.0,
            steer: (angle * 3.0).clamp(-1.0, 1.0),
            ..default()
        };
        app.update();
        let boost = *app.world().get::<Boost>(kart).unwrap();
        if (boost.remaining - tuning.boost_pad_duration).abs() < 1e-5 {
            assert_eq!(boost.strength, tuning.boost_pad_strength);
            on_pad = true;
            break;
        }
        assert!(!boost.is_active(), "{boost:?}");
    }
    assert!(on_pad, "never boosted by the pad");

    // The autopilot to the trick ramp; press jump/drift on the first tick in the air.
    let layout = app.world().resource::<TrackLayout>().clone();
    let mut ticks = 0;
    while app
        .world()
        .get::<TrickTracker>(kart)
        .unwrap()
        .ticks_since_takeoff()
        != Some(1)
    {
        ticks += 1;
        assert!(
            ticks < (40.0 * TICK_HZ) as u32,
            "never took off from the ramp"
        );
        autopilot_tick(&mut app, kart);
    }
    let t = transform(&app, kart);
    let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
    let mut input = follow_centerline(&layout, &t, speed);
    input.jump_drift = true;
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
    app.update();
    assert_eq!(state(&app, kart), KartState::Trick);
    while state(&app, kart) == KartState::Trick {
        ticks += 1;
        assert!(ticks < (45.0 * TICK_HZ) as u32, "never landed");
        autopilot_tick(&mut app, kart);
    }
    let boost = *app.world().get::<Boost>(kart).unwrap();
    assert_eq!(state(&app, kart), KartState::Grounded);
    assert_eq!(boost.strength, tuning.trick_boost_strength);
    assert!((boost.remaining - tuning.trick_boost_duration).abs() < 1e-5);
    assert!(!tow(&app, kart).is_carrying());
}

// --- Issue #30: an orange mini-turbo in the big turns of Prairie Pompon ----------------------------

/// A player on the keyboard: follows the racing line, hops a little before each big turn
/// (`HOP_LEAD`), and in the drift only holds the inward key or lets go of the arrows (the two
/// options of a keyboard, which charge +5 and +2), lifting off when running wide. It releases at
/// orange, or gives up past the turn.
#[derive(Default)]
struct KeyboardDrifter {
    sample: Option<usize>,
    /// The big turn it hopped for, and whether the kart got into it.
    turn: Option<usize>,
    inside: bool,
    last_jump: bool,
}

/// A drift released by the `KeyboardDrifter`: the big turn, where (m along the lap), the tier.
#[derive(Clone, Copy, Debug)]
struct Release {
    turn: usize,
    at: f32,
    tier: Option<MiniTurbo>,
}

impl KeyboardDrifter {
    /// Hops this long before a big turn (s): the drift starts on the approach, its curve setting
    /// in as the turn begins, and goes on through the whole turn.
    const HOP_LEAD: f32 = 0.7;

    fn big_turn(line: &RacingLine, sample: usize) -> Option<usize> {
        line.turn_at(sample)
            .map(|(turn, _)| turn)
            .filter(|turn| line.turns()[*turn].angle > 2.4)
    }

    fn input(
        &mut self,
        line: &RacingLine,
        tuning: &KartTuning,
        transform: &Transform,
        velocity: Vec3,
        state: KartState,
    ) -> (KartInput, Option<Release>) {
        let speed = velocity.dot(*transform.forward());
        let here = line.locate(transform.translation, self.sample);
        self.sample = Some(here.index);
        let aim = here.distance + 5.0 + 0.35 * speed.max(0.0);
        let target = line.center().at(aim).point(line.offset_at(aim), 0.0);
        let to = target - transform.translation;
        let angle = to
            .dot(*transform.right())
            .atan2(to.dot(*transform.forward()));
        let mut input = KartInput {
            throttle: 1.0,
            steer: (angle * 3.0).clamp(-1.0, 1.0),
            ..default()
        };
        let mut release = None;
        match state {
            KartState::Drifting { side, .. } => {
                self.inside |= Self::big_turn(line, here.index).is_some();
                let past = self.inside && Self::big_turn(line, here.index).is_none();
                let tier = state.mini_turbo(tuning, false);
                let inward = angle * side.sign();
                if tier >= Some(MiniTurbo::Orange) || (past && inward < -0.15) {
                    release = self.turn.map(|turn| Release {
                        turn,
                        at: here.distance,
                        tier,
                    });
                } else {
                    input.jump_drift = true;
                    input.steer = if inward > 0.04 {
                        side.sign()
                    } else if inward < -0.12 {
                        -side.sign()
                    } else {
                        0.0
                    };
                    if inward > 0.25 {
                        input.throttle = 0.0;
                    }
                }
            }
            KartState::Hop { .. } => {
                input.jump_drift = true;
                if let Some(turn) = self.turn {
                    input.steer = line.turns()[turn].side.sign();
                }
            }
            _ => {
                let lead = Self::HOP_LEAD * speed.max(0.0);
                let ahead = line.turn_ahead(here.index, (lead / line.step()).ceil() as usize);
                if let Some((turn, _)) = ahead
                    && line.turns()[turn].angle > 2.4
                    && self.turn != Some(turn)
                    && !self.last_jump
                {
                    self.turn = Some(turn);
                    self.inside = false;
                    input.jump_drift = true;
                    input.steer = line.turns()[turn].side.sign();
                }
            }
        }
        self.last_jump = input.jump_drift;
        (input, release)
    }
}

/// Issue #30 acceptance: on Prairie Pompon, the hairpin and the banked turn both give an orange
/// mini-turbo to a player, on every lap, and well before the end of the turn (the drift of #4
/// gave a blue at best: about 130 units per radian of turn, 176° turns). Driven like a keyboard
/// player: full inward or the stick released, nothing in between.
#[test]
fn a_keyboard_player_gets_an_orange_in_both_big_turns() {
    let (mut app, kart) = race_app();
    take_the_wheel(&mut app, kart);
    let line = app.world().resource::<RacingLine>().clone();
    let tuning = app.world().resource::<KartTuning>().clone();
    let big: Vec<usize> = (0..line.turns().len())
        .filter(|t| line.turns()[*t].angle > 2.4)
        .collect();
    assert_eq!(big.len(), 2, "{:?}", line.turns());
    until_go(&mut app);
    let mut driver = KeyboardDrifter::default();
    let mut releases = Vec::new();
    let mut laps = 0;
    for _ in 0..(70.0 * TICK_HZ) as usize {
        let world = app.world();
        let (input, release) = driver.input(
            &line,
            &tuning,
            world.get::<Transform>(kart).unwrap(),
            world.get::<Velocity>(kart).unwrap().linear,
            state(&app, kart),
        );
        releases.extend(release);
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
        app.update();
        assert!(!tow(&app, kart).is_carrying(), "towed: {releases:?}");
        let p = progress(&app, kart);
        if p.lap_times.len() > laps {
            laps = p.lap_times.len();
            driver.turn = None;
        }
        if p.finished() {
            break;
        }
    }
    println!(
        "keyboard player: {releases:?}, laps {:?}",
        progress(&app, kart).lap_times
    );
    assert!(progress(&app, kart).finished());
    // One drift per big turn and per lap, each released at orange, at least 15 m before the
    // turn ends.
    assert_eq!(releases.len(), 6, "{releases:?}");
    for (i, release) in releases.iter().enumerate() {
        assert_eq!(release.turn, big[i % 2], "{releases:?}");
        assert_eq!(release.tier, Some(MiniTurbo::Orange), "{release:?}");
        let turn = line.turns()[release.turn];
        let end = (turn.start + turn.len) as f32 * line.step();
        assert!(
            release.at < end - 15.0,
            "{release:?}: the turn ends at {end} m"
        );
    }
    // A lap stays around 17 s for this driver (laps 2 and 3, without the start).
    for lap in &progress(&app, kart).lap_times[1..] {
        let seconds = ticks_to_seconds(*lap);
        assert!((15.0..19.5).contains(&seconds), "{seconds} s");
    }
}
