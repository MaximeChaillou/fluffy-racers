//! Issue #234: the gate headless, on a simulated browser display (a frame offered at each of
//! its refreshes, the clock and Bevy's time set by hand): the simulation keeps its 60 ticks per
//! second whatever the cap, the frames not drawn run nothing, and the interpolation's share of
//! the next tick stays right.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use super::*;

#[derive(Resource, Default)]
struct Counts {
    ticks: u64,
    updates: u64,
    /// The interpolation's share at each update (`Time<Fixed>::overstep_fraction`).
    shares: Vec<f32>,
}

fn app(cap: FrameCap) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(Time::<Fixed>::from_hz(crate::TICK_HZ))
        .insert_resource(FramePacing { cap, ..default() })
        .insert_resource(FrameClock::Manual(0.0))
        .init_resource::<Counts>()
        .add_systems(FixedUpdate, |mut counts: ResMut<Counts>| counts.ticks += 1)
        .add_systems(
            Update,
            |mut counts: ResMut<Counts>, fixed: Res<Time<Fixed>>| {
                counts.updates += 1;
                counts.shares.push(fixed.overstep_fraction());
            },
        );
    install_frame_gate(&mut app, PacingMode::SkipFrames);
    app
}

/// A refresh of the display at `now` (s): the clocks set, then the update.
fn refresh(app: &mut App, now: f64) {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    app.insert_resource(FrameClock::Manual(now));
    app.insert_resource(TimeUpdateStrategy::ManualInstant(
        start + Duration::from_secs_f64(now),
    ));
    app.update();
}

/// `seconds` of a display at `hz`, from 0.
fn run(app: &mut App, hz: f64, seconds: f64) {
    for frame in 0..(seconds * hz).round() as u64 {
        refresh(app, frame as f64 / hz);
    }
}

/// The simulation keeps 60 ticks per second whatever the cap and the display; the updates
/// (the frames drawn) follow the cap; the interpolation's share stays within a tick.
#[test]
fn the_simulation_keeps_sixty_ticks_whatever_the_cap() {
    let seconds = 10.0;
    for (hz, cap, drawn_per_second) in [
        (120.0, FrameCap::Sixty, 60.0),
        (120.0, FrameCap::Thirty, 30.0),
        (120.0, FrameCap::Unlimited, 120.0),
        (144.0, FrameCap::Sixty, 60.0),
        (60.0, FrameCap::Sixty, 60.0),
        (60.0, FrameCap::Thirty, 30.0),
        (50.0, FrameCap::Sixty, 50.0),
    ] {
        let mut app = app(cap);
        run(&mut app, hz, seconds);
        let counts = app.world().resource::<Counts>();
        let ticks = counts.ticks as f64;
        // The last frame drawn may be up to one image before the end.
        assert!(
            (ticks - seconds * 60.0).abs() <= 3.0,
            "{ticks} ticks at {hz} Hz, {cap:?}"
        );
        let updates = counts.updates as f64;
        assert!(
            (updates - seconds * drawn_per_second).abs() <= 2.0,
            "{updates} updates at {hz} Hz, {cap:?}"
        );
        assert!(
            counts.shares.iter().all(|s| (0.0..1.0).contains(s)),
            "{hz} Hz, {cap:?}"
        );
        let gate = app.world().resource::<FrameGate>();
        assert_eq!(gate.offered, (seconds * hz).round() as u64);
        assert_eq!(
            gate.drawn_count, counts.updates,
            "a frame not drawn runs nothing"
        );
    }
}

/// At 120 Hz and 60: each frame drawn is two refreshes after the last, the simulation one tick
/// further, the interpolation at the same share (no stutter).
#[test]
fn at_120_hz_each_image_is_one_tick_further() {
    let mut app = app(FrameCap::Sixty);
    run(&mut app, 120.0, 2.0);
    let before = app.world().resource::<Counts>().ticks;
    let share = *app.world().resource::<Counts>().shares.last().unwrap();
    let mut drawn = 0;
    for frame in 0..240 {
        let updates = app.world().resource::<Counts>().updates;
        refresh(&mut app, 2.0 + frame as f64 / 120.0);
        let counts = app.world().resource::<Counts>();
        if counts.updates > updates {
            drawn += 1;
            let latest = *counts.shares.last().unwrap();
            assert!((latest - share).abs() < 0.05, "{latest} against {share}");
        }
    }
    assert_eq!(drawn, 120);
    let ticks = app.world().resource::<Counts>().ticks - before;
    assert!((119..=121).contains(&ticks), "{ticks}");
}

/// The option changes the cap at once: from 60 to « Sans limite » on a 120 Hz display.
#[test]
fn the_cap_changes_at_once() {
    let mut app = app(FrameCap::Sixty);
    run(&mut app, 120.0, 1.0);
    let at_sixty = app.world().resource::<Counts>().updates;
    assert!((59..=61).contains(&at_sixty), "{at_sixty}");
    app.world_mut().resource_mut::<FramePacing>().cap = FrameCap::Unlimited;
    for frame in 0..120 {
        refresh(&mut app, 1.0 + frame as f64 / 120.0);
    }
    let unlimited = app.world().resource::<Counts>().updates - at_sixty;
    assert_eq!(unlimited, 120);
}
