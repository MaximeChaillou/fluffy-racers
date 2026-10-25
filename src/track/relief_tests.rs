//! Issue #180 acceptance: headless scenarios of the bumps and the drops (the whole game,
//! `GamePlugin` with `MinimalPlugins`, one fixed tick per `app.update()`) on Prairie Pompon, their
//! demonstration circuit, and its mirror image (issue #148: and on the circuits redrawn with
//! theirs, [`RELIEF_CIRCUITS`]). A kart at full speed takes off from a bump and
//! lands on the road, upright, between its walls; a slow one rolls over it on its wheels; a trick
//! off its crest gives the trick boost; a drop is driven off at every speed, drifting too, and a
//! trick off its edge gives the boost: never the pelican, never through the road.

use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::admin::skip_countdown_now;
use crate::interpolation::PhysicsPose;
use crate::kart::{Boost, KartInput, KartState, KartTuning, KartWheels, PlayerKart, TrickTracker};
use crate::race::{Course, Race, RaceProgress, Tow};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};

/// The circuits of these scenarios, and their bumps and drops: Prairie Pompon (issue #180), the
/// Anneau Guimauve and the Crique Câline (issue #148, redrawn with theirs).
const RELIEF_CIRCUITS: [(&str, (usize, usize)); 3] = [
    ("tracks/prairie.track.ron", (2, 1)),
    ("tracks/anneau.track.ron", (2, 1)),
    ("tracks/crique.track.ron", (2, 1)),
];

/// Every circuit of [`RELIEF_CIRCUITS`] and its mirror image: (its file, mirrored).
fn relief_runs() -> impl Iterator<Item = (&'static str, bool)> {
    RELIEF_CIRCUITS
        .iter()
        .flat_map(|(path, _)| [(*path, false), (*path, true)])
}

/// The game on the circuit file `path` (its mirror image or not), the player's kart alone, its
/// race started (the test drives it).
fn game_on(path: &str, mirrored: bool) -> (App, Entity) {
    let mut app = game_app_unloaded(TrackChoice::Circuit(path.into()), 0);
    app.insert_resource(MirrorMode(mirrored));
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    for tries in 0.. {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        assert!(tries < 100, "the race never came");
        app.update();
    }
    skip_countdown_now(app.world_mut());
    for _ in 0..3 {
        app.update();
    }
    assert!(app.world().resource::<Race>().started());
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    (app, kart)
}

/// Puts the kart on the centerline at `distance`, `lateral` meters right of it, facing down the
/// track at `speed`, its race progress there (the first lap): on its way, not out of the track
/// for the pelican.
fn place(app: &mut App, kart: Entity, distance: f32, lateral: f32, speed: f32) {
    let course = app.world().resource::<Course>().clone();
    let spacing = course.checkpoints[1];
    let transform = course.center.at(distance).transform(lateral, GRID_HEIGHT);
    let world = app.world_mut();
    *world.get_mut::<Transform>(kart).unwrap() = transform;
    world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    *world.get_mut::<Velocity>(kart).unwrap() = Velocity {
        linear: *transform.forward() * speed,
        angular: Vec3::ZERO,
    };
    *world.get_mut::<KartState>(kart).unwrap() = KartState::Grounded;
    *world.get_mut::<KartWheels>(kart).unwrap() = KartWheels::default();
    *world.get_mut::<TrickTracker>(kart).unwrap() = TrickTracker::default();
    *world.get_mut::<Boost>(kart).unwrap() = Boost::default();
    let mut progress = world.get_mut::<RaceProgress>(kart).unwrap();
    progress.passed = 1 + (distance / spacing).floor() as u32;
    progress.distance = distance;
    world
        .get_mut::<Tow>(kart)
        .unwrap()
        .reset(transform.translation);
}

/// How the test drives.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pace {
    /// Full throttle.
    Full,
    /// Full throttle, a boost of +30 % going (a piment, a purple mini-turbo): the fastest a kart
    /// meets a bump or a drop.
    Boosted,
    /// Holds about this speed (m/s).
    Hold(f32),
}

/// What a run over a bump or a drop saw.
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    /// The most ticks in a row without a wheel on the ground.
    air_ticks: u32,
    /// Highest and lowest the kart's center went above the road under it (m; 0.55 at rest),
    /// and its highest vertical speed (m/s).
    highest: f32,
    lowest: f32,
    rising: f32,
    /// The lowest height above the road the race counted for it (`Course::project`: the
    /// pelican comes below `-RaceTuning::fall_depth`).
    projected: f32,
    /// The deepest a wheel's spring was pushed in (m; its rest length is 0.6).
    deepest: f32,
    /// Its lowest uprightness against the road under it, on the ground (1: wheels down).
    lowest_up: f32,
    /// Closest its center came to a wall's inner face (m; negative: past it).
    closest: f32,
    /// The pelican came for it.
    towed: bool,
    /// It drifted on the last tick on the ground before its longest time in the air.
    drifting_off: bool,
    /// It did a trick, and a boost was going as it landed from it.
    tricked: bool,
    trick_boost: bool,
    /// Where it touched the ground after its longest time in the air (m along the track).
    landing: f32,
    /// At the end: its speed (m/s), its uprightness against the road, its wheels on the ground,
    /// how far along the track (m).
    speed: f32,
    up: f32,
    wheels: usize,
    distance: f32,
}

/// Drives `kart` for `seconds` at `pace`, toward the centerline (as a player who keeps to the
/// middle), adding what `press` says on a tick (the tick, the kart's trick tracker).
fn drive(
    app: &mut App,
    kart: Entity,
    pace: Pace,
    seconds: f32,
    mut press: impl FnMut(u32, &TrickTracker) -> Option<KartInput>,
) -> Run {
    let course = app.world().resource::<Course>().clone();
    if pace == Pace::Boosted {
        *app.world_mut().get_mut::<Boost>(kart).unwrap() = Boost {
            strength: 0.3,
            remaining: seconds,
        };
    }
    let mut run = Run {
        lowest: f32::MAX,
        projected: f32::MAX,
        lowest_up: 1.0,
        closest: f32::MAX,
        ..default()
    };
    let (mut air, mut was_drifting, mut was_trick) = (0, false, false);
    let mut took_off_drifting = false;
    for tick in 0..(seconds * 60.0) as u32 {
        let world = app.world();
        let transform = *world.get::<Transform>(kart).unwrap();
        let speed = world.get::<Velocity>(kart).unwrap().linear.length();
        let tracker = *world.get::<TrickTracker>(kart).unwrap();
        let passed = world.get::<RaceProgress>(kart).unwrap().passed;
        let here = course.project(passed, transform.translation);
        let ahead = course.center.at(here.distance + 12.0).position;
        let to = ahead - transform.translation;
        let angle = to
            .dot(*transform.right())
            .atan2(to.dot(*transform.forward()));
        let mut input = KartInput {
            throttle: match pace {
                Pace::Full | Pace::Boosted => 1.0,
                Pace::Hold(target) => (target - speed).clamp(0.0, 1.0),
            },
            brake: match pace {
                Pace::Hold(target) if speed > target + 1.5 => 0.5,
                _ => 0.0,
            },
            steer: (angle * 2.5).clamp(-1.0, 1.0),
            ..default()
        };
        if let Some(pressed) = press(tick, &tracker) {
            input.jump_drift = pressed.jump_drift;
            if pressed.steer != 0.0 {
                input.steer = pressed.steer;
            }
        }
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
        app.update();

        let world = app.world();
        let transform = *world.get::<Transform>(kart).unwrap();
        let velocity = world.get::<Velocity>(kart).unwrap().linear;
        let wheels = *world.get::<KartWheels>(kart).unwrap();
        let state = *world.get::<KartState>(kart).unwrap();
        let passed = world.get::<RaceProgress>(kart).unwrap().passed;
        let here = course.project(passed, transform.translation);
        let road = course.center.at(here.distance);
        let height = (transform.translation - road.position).dot(road.up);
        run.towed |= world.get::<Tow>(kart).unwrap().is_carrying();
        run.highest = run.highest.max(height);
        run.lowest = run.lowest.min(height);
        run.projected = run.projected.min(here.height);
        run.rising = run.rising.max(velocity.y);
        run.deepest = wheels
            .compression
            .iter()
            .copied()
            .fold(run.deepest, f32::max);
        run.closest = run.closest.min(here.wall - here.lateral.abs());
        let grounded = wheels.wheels_on_ground();
        if grounded > 0 {
            run.lowest_up = run.lowest_up.min(transform.up().dot(road.up));
            if air > run.air_ticks {
                run.air_ticks = air;
                run.landing = here.distance;
                run.drifting_off = took_off_drifting;
            }
            air = 0;
            was_drifting = matches!(state, KartState::Drifting { .. });
        } else {
            if air == 0 {
                took_off_drifting = was_drifting;
            }
            air += 1;
        }
        let trick = state == KartState::Trick;
        run.tricked |= trick;
        if was_trick && !trick {
            run.trick_boost |= world.get::<Boost>(kart).unwrap().is_active();
        }
        was_trick = trick;
        run.speed = velocity.length();
        run.up = transform.up().dot(road.up);
        run.wheels = grounded;
        run.distance = here.distance;
    }
    run
}

fn no_press(_: u32, _: &TrickTracker) -> Option<KartInput> {
    None
}

/// Presses jump/drift once, on the first tick its trick window is open after a takeoff.
fn trick_press() -> impl FnMut(u32, &TrickTracker) -> Option<KartInput> {
    let mut done = false;
    move |_, tracker| {
        let window = (KartTuning::default().trick_window * 60.0).round() as u32;
        (!done && tracker.window_open(window)).then(|| {
            done = true;
            KartInput {
                jump_drift: true,
                ..default()
            }
        })
    }
}

/// The bumps and the drops of the circuit file `path` (its mirror image or not, as many as
/// [`RELIEF_CIRCUITS`] says), and the game on it.
fn relief(path: &str, mirrored: bool) -> (App, Entity, Vec<BumpSpan>, Vec<Cliff>) {
    let (app, kart) = game_on(path, mirrored);
    let center = &app.world().resource::<TrackLayout>().center;
    let (bumps, cliffs) = (center.bumps().to_vec(), center.cliffs().to_vec());
    let expected = RELIEF_CIRCUITS.iter().find(|(p, _)| *p == path).unwrap().1;
    assert_eq!((bumps.len(), cliffs.len()), expected, "{path}'s relief");
    (app, kart, bumps, cliffs)
}

/// A run that took off and landed sanely: in the air for `air` ticks at least, never thrown
/// higher than `highest` above the road nor up faster than 13 m/s, its center never closer to the
/// road than 0.15 m (never through it: 0.55 at rest), its springs never pushed all the way in,
/// upright on the ground, between the walls, never towed; at the end on its wheels, still fast.
fn assert_sane(run: &Run, what: &str, air: u32, highest: f32) {
    assert!(
        run.air_ticks >= air,
        "{what}: in the air too little: {run:?}"
    );
    assert!(
        run.highest <= highest && run.rising <= 13.0,
        "{what}: thrown too high: {run:?}"
    );
    assert!(run.lowest >= 0.15, "{what}: down to the road: {run:?}");
    assert!(
        run.deepest < 0.58,
        "{what}: its springs bottomed out: {run:?}"
    );
    assert!(run.lowest_up >= 0.95, "{what}: tipped over: {run:?}");
    assert!(run.closest > 0.5, "{what}: out of the walls: {run:?}");
    assert!(!run.towed, "{what}: the pelican came: {run:?}");
    assert!(run.wheels >= 2, "{what}: not back on its wheels: {run:?}");
}

/// Issue #180 acceptance (« un kart à pleine vitesse décolle sur une bosse et atterrit sur la
/// route »): at full speed, and boosted, a kart takes off from each bump of Prairie Pompon and its
/// mirror (issue #148: of every circuit of [`RELIEF_CIRCUITS`]) (half a second in the air at least), lands on the road a few meters after it, upright,
/// between its walls, and drives on: never thrown absurdly high (its center 5 m above the road
/// at most at full speed, 6.5 m boosted by 30 %, 37 m/s), never through the road, its springs never bottomed out.
#[test]
fn a_fast_kart_takes_off_a_bump_and_lands_on_the_road() {
    for (path, mirrored) in relief_runs() {
        let (mut app, kart, bumps, _) = relief(path, mirrored);
        for bump in &bumps {
            for pace in [Pace::Full, Pace::Boosted] {
                place(&mut app, kart, bump.from - 25.0, 0.0, 28.5);
                let run = drive(&mut app, kart, pace, 3.5, no_press);
                let what = format!(
                    "{path}: bump at {:.0} m, {pace:?}, mirror {mirrored}",
                    bump.from
                );
                println!(
                    "{what}: {run:?}, landed {:.1} m in",
                    run.landing - bump.from
                );
                let highest = if pace == Pace::Boosted { 6.5 } else { 5.0 };
                assert_sane(&run, &what, 30, highest);
                let landing = run.landing - bump.from;
                assert!(
                    landing > 0.6 * bump.length && landing < bump.length + 35.0,
                    "{what}: landed {landing:.1} m from its start: {run:?}"
                );
                assert!(run.speed > 25.0, "{what}: slowed down: {run:?}");
            }
        }
    }
}

/// Issue #180 acceptance (« un kart lent la franchit au sol »): at 12 and 16 m/s, a kart rolls
/// over each bump on its wheels (never more than two ticks without one on the ground), its
/// springs never bottomed out.
#[test]
fn a_slow_kart_rolls_over_a_bump_on_its_wheels() {
    for (path, mirrored) in relief_runs() {
        let (mut app, kart, bumps, _) = relief(path, mirrored);
        for bump in &bumps {
            for speed in [12.0, 16.0] {
                place(&mut app, kart, bump.from - 15.0, 0.0, speed);
                let run = drive(&mut app, kart, Pace::Hold(speed), 6.0, no_press);
                let what = format!(
                    "{path}: bump at {:.0} m, {speed} m/s, mirror {mirrored}",
                    bump.from
                );
                assert!(run.air_ticks <= 2, "{what}: took off: {run:?}");
                assert!(run.highest < 1.0 && run.lowest >= 0.3, "{what}: {run:?}");
                assert!(run.deepest < 0.58 && !run.towed, "{what}: {run:?}");
                assert!(
                    run.distance > bump.from + bump.length + 10.0,
                    "{what}: did not get over it: {run:?}"
                );
            }
        }
    }
}

/// Issue #180 acceptance (« une figure sur une bosse donne le boost »), GD § 5: pressing jump/drift
/// as the kart leaves the crest of a bump starts a trick, and the landing gives the trick boost;
/// off the edge of a drop too.
#[test]
fn a_trick_off_a_bump_or_a_drop_gives_the_boost() {
    for (path, mirrored) in relief_runs() {
        let (mut app, kart, bumps, cliffs) = relief(path, mirrored);
        let starts = bumps
            .iter()
            .map(|b| b.from)
            .chain(cliffs.iter().map(|c| c.at));
        for start in starts {
            place(&mut app, kart, start - 25.0, 0.0, 28.5);
            let run = drive(&mut app, kart, Pace::Full, 3.5, trick_press());
            let what = format!("{path}: at {start:.0} m, mirror {mirrored}");
            assert!(
                run.tricked && run.trick_boost,
                "{what}: no trick boost: {run:?}"
            );
            assert!(!run.towed && run.wheels >= 2, "{what}: {run:?}");
            // Without a press: no trick.
            place(&mut app, kart, start - 25.0, 0.0, 28.5);
            let run = drive(&mut app, kart, Pace::Full, 3.5, no_press);
            assert!(!run.tricked, "{what}: a trick without a press: {run:?}");
        }
    }
}

/// Issue #180 acceptance (« un décrochage se descend sans pélican, à plusieurs vitesses, en drift
/// et en miroir »): off the drop of Prairie Pompon (2.5 m; issue #148: and the 4 m of the Anneau
/// Guimauve's and the Crique Câline's drops, [`RELIEF_CIRCUITS`]) and its mirror, boosted, at full speed,
/// at 18, 8 and 3 m/s (rolling off its edge, its nose first), the kart falls to the road below and
/// drives on: never towed, never counted fallen by the race (its height above the road it is
/// counted on never below `-fall_depth`), never through the road, between the walls, on its four
/// wheels at the end, its checkpoints validated on below.
#[test]
fn a_drop_is_driven_off_at_every_speed_without_the_pelican() {
    let fall_depth = crate::race::RaceTuning::default().fall_depth;
    for (path, mirrored) in relief_runs() {
        let (mut app, kart, _, cliffs) = relief(path, mirrored);
        let cliff = cliffs[0];
        for (pace, speed, seconds) in [
            (Pace::Boosted, 28.5, 3.5),
            (Pace::Full, 28.5, 3.5),
            (Pace::Hold(18.0), 18.0, 3.5),
            (Pace::Hold(8.0), 8.0, 5.0),
            (Pace::Hold(3.0), 3.0, 12.0),
        ] {
            place(&mut app, kart, cliff.at - 20.0, 0.0, speed);
            let passed = app.world().get::<RaceProgress>(kart).unwrap().passed;
            let run = drive(&mut app, kart, pace, seconds, no_press);
            let what = format!("{path}: {pace:?}, mirror {mirrored}");
            println!(
                "{what}: {run:?}, landed {:.1} m below",
                run.landing - cliff.at
            );
            assert!(run.air_ticks >= 10, "{what}: never fell: {run:?}");
            assert!(
                run.highest <= cliff.height + 1.5,
                "{what}: thrown up: {run:?}"
            );
            assert!(run.lowest >= 0.15, "{what}: down to the road: {run:?}");
            assert!(
                run.projected > -fall_depth,
                "{what}: counted fallen: {run:?}"
            );
            assert!(run.closest > 0.5, "{what}: out of the walls: {run:?}");
            assert!(
                !run.towed && run.wheels == 4 && run.up > 0.97,
                "{what}: {run:?}"
            );
            assert!(run.distance > cliff.at + 8.0, "{what}: stuck: {run:?}");
            let now = app.world().get::<RaceProgress>(kart).unwrap().passed;
            assert!(
                now > passed,
                "{what}: no checkpoint below the drop: {run:?}"
            );
        }
    }
}

/// Issue #180 acceptance: drifting off the drop (a drift to the left or to the right started about
/// 20 m before its edge, steered down the road), the kart falls drifting and lands on the road
/// below without the pelican.
#[test]
fn a_drop_is_driven_off_drifting() {
    let fall_depth = crate::race::RaceTuning::default().fall_depth;
    for (path, mirrored) in relief_runs() {
        for side in [-1.0f32, 1.0] {
            let (mut app, kart, _, cliffs) = relief(path, mirrored);
            let cliff = cliffs[0];
            place(&mut app, kart, cliff.at - 30.0, 0.0, 26.0);
            // A hop steered to `side`, then the button held, the kart steered down the road.
            let drift = move |tick: u32, _: &TrickTracker| {
                Some(KartInput {
                    jump_drift: tick >= 15,
                    steer: if (15..20).contains(&tick) { side } else { 0.0 },
                    ..default()
                })
            };
            let run = drive(&mut app, kart, Pace::Full, 3.0, drift);
            let what = format!("{path}: drifting to {side}, mirror {mirrored}");
            assert!(
                run.drifting_off,
                "{what}: not drifting off the edge: {run:?}"
            );
            assert!(run.air_ticks >= 10 && run.lowest >= 0.15, "{what}: {run:?}");
            assert!(run.projected > -fall_depth && !run.towed, "{what}: {run:?}");
            assert!(run.closest > 0.0 && run.up > 0.9, "{what}: {run:?}");
            assert!(run.distance > cliff.at + 20.0, "{what}: {run:?}");
        }
    }
}
