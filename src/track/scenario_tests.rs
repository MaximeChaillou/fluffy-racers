//! Headless crashes into the walls of Prairie Pompon (issue #27): the whole game (`GamePlugin`)
//! with `MinimalPlugins`, one fixed tick per `app.update()`.
//!
//! A kart that hits a wall bounces off it and stays on the track, whatever the angle, its speed,
//! a drift, a boost or its weight. Before issue #27 it could climb the 0.8 m walls and fall out:
//! the impact landed above its center of mass (0.15 m below the middle of its collision box),
//! which kicked the side facing the wall up, or its nose, by 20 to 30°; it then drove up onto the
//! top edge of the wall, and its suspension rays found ground on the flat top of the wall.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::TICK_HZ;
use crate::admin::skip_countdown_now;
use crate::characters::{Loadout, PartKind, PlayerLoadout, Roster};
use crate::kart::{Boost, KartInput, KartState, PlayerKart};
use crate::race::{Course, Race, RaceProgress};
use crate::tests::{
    follow_centerline, game_app_unloaded, hold_keys, player, single_threaded, wait_for_the_player,
};
use crate::track::{TrackChoice, TrackLayout};

/// The kart's parts: the default ones (every stat at the middle), or the roster's lightest and
/// heaviest loadouts (issue #15).
#[derive(Clone, Copy, Debug)]
enum Build {
    Middle,
    Light,
    Heavy,
}

impl Build {
    fn loadout(self) -> Option<Loadout> {
        let names = match self {
            Self::Middle => return None,
            Self::Light => ["Souris", "Panier en osier", "Bobines de fil", "Plume"],
            Self::Heavy => ["Ours", "Baignoire", "Pneus de tracteur", "Taie d'oreiller"],
        };
        let roster = Roster::embedded();
        let mut loadout = Loadout::default();
        for (kind, name) in PartKind::ALL.into_iter().zip(names) {
            loadout.set(kind, roster.find(kind, name).unwrap());
        }
        Some(loadout)
    }
}

/// What the driver does on top of steering into the wall.
#[derive(Clone, Copy, Debug)]
enum Extra {
    Nothing,
    /// A hop steered toward the wall, the button held: a drift into it.
    Drift,
    /// A +30 % boost of 2 s (a piment, an orange or purple mini-turbo) as it turns.
    Boost,
}

/// A crash into a wall.
#[derive(Clone, Copy, Debug)]
struct Crash {
    build: Build,
    /// The test autopilot first drives this far along the lap from the pole (m; 0: not at all).
    autopilot: f32,
    /// Then full throttle, straight on, for this long (s): the speed of the crash.
    straight: f32,
    /// Then full throttle steering this way (-1: left, 1: right) for [`CRASH_SECONDS`].
    steer: f32,
    extra: Extra,
}

/// How long a crash steers into the wall: it hits it within 1.2 s, and a kart that climbed was
/// over the wall less than 1 s later. Shorter than the 3 s after which the pelican comes for a
/// kart that keeps pushing against a wall without getting anywhere.
const CRASH_SECONDS: f32 = 2.5;

/// What happened during a crash.
#[derive(Clone, Copy, Debug)]
struct Outcome {
    /// Speed as the steering started (m/s).
    speed: f32,
    /// Highest point of the kart's center above the road (m; 0.55 at rest, at least 1.05 on top
    /// of a 0.8 m wall).
    highest: f32,
    /// Closest the kart's center came to the inner face of a wall (m; negative: past it).
    closest: f32,
    /// Fastest the kart moved away from the wall after touching it, across the road (m/s).
    bounce: f32,
    /// Lowest uprightness of the kart against the road (1: wheels down, 0.94: tilted by 20°).
    lowest_up: f32,
    /// The pelican came for it.
    towed: bool,
}

/// The game on Prairie Pompon, the player alone on the pole with `build`, once the race exists.
fn crash_app(build: Build) -> (App, Entity) {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    if let Some(loadout) = build.loadout() {
        app.insert_resource(PlayerLoadout(loadout));
    }
    single_threaded(&mut app);
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    while app.world().get_resource::<Race>().is_none() {
        app.update();
    }
    (app, kart)
}

/// Follows a crash tick by tick.
struct Watch {
    kart: Entity,
    course: Course,
    outcome: Outcome,
    /// Side of the wall touched (sign of the lateral offset), once the kart touched one.
    wall_side: Option<f32>,
}

impl Watch {
    fn new(app: &App, kart: Entity) -> Self {
        Self {
            kart,
            course: app.world().resource::<Course>().clone(),
            outcome: Outcome {
                speed: 0.0,
                highest: 0.0,
                closest: f32::MAX,
                bounce: 0.0,
                lowest_up: 1.0,
                towed: false,
            },
            wall_side: None,
        }
    }

    fn observe(&mut self, app: &App) {
        let world = app.world();
        if *world.get::<KartState>(self.kart).unwrap() == KartState::Respawning {
            self.outcome.towed = true;
            return;
        }
        let transform = world.get::<Transform>(self.kart).unwrap();
        let passed = world.get::<RaceProgress>(self.kart).unwrap().passed;
        let p = self.course.project(passed, transform.translation);
        let sample = self.course.center.at(p.distance);
        let to_wall = p.wall - p.lateral.abs();
        let outcome = &mut self.outcome;
        outcome.highest = outcome.highest.max(p.height);
        outcome.closest = outcome.closest.min(to_wall);
        outcome.lowest_up = outcome.lowest_up.min(transform.up().dot(sample.up));
        // Touching a wall: the kart's center less than its half width (0.7 m) and a margin
        // from it.
        if to_wall < 1.25 {
            self.wall_side.get_or_insert(p.lateral.signum());
        }
        if let Some(side) = self.wall_side {
            let across = world
                .get::<Velocity>(self.kart)
                .unwrap()
                .linear
                .dot(sample.right);
            outcome.bounce = outcome.bounce.max(-side * across);
        }
    }
}

fn drive(app: &mut App, kart: Entity, input: KartInput) {
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
    app.update();
}

/// One tick of the test autopilot.
fn autopilot(app: &mut App, kart: Entity, layout: &TrackLayout) {
    let world = app.world();
    let transform = *world.get::<Transform>(kart).unwrap();
    let speed = world.get::<Velocity>(kart).unwrap().linear.length();
    drive(app, kart, follow_centerline(layout, &transform, speed));
}

fn crash(c: Crash) -> Outcome {
    let (mut app, kart) = crash_app(c.build);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    skip_countdown_now(app.world_mut());
    let layout = app.world().resource::<TrackLayout>().clone();
    let center = &layout.center;
    let along = |app: &App| {
        let position = app.world().get::<Transform>(kart).unwrap().translation;
        center.samples()[center.nearest_index(position)].distance
    };
    let start = along(&app);
    let mut ticks = 0;
    while c.autopilot > 0.0 && center.wrap(along(&app) - start) < c.autopilot {
        ticks += 1;
        assert!(ticks < (30.0 * TICK_HZ) as u32, "{c:?}: never got there");
        autopilot(&mut app, kart, &layout);
    }
    let mut watch = Watch::new(&app, kart);
    let throttle = KartInput {
        throttle: 1.0,
        ..default()
    };
    for _ in 0..(c.straight * TICK_HZ as f32).round() as usize {
        drive(&mut app, kart, throttle);
        watch.observe(&app);
    }
    let world = app.world();
    watch.outcome.speed = world
        .get::<Velocity>(kart)
        .unwrap()
        .linear
        .dot(*world.get::<Transform>(kart).unwrap().forward());
    if matches!(c.extra, Extra::Boost) {
        let mut boost = app.world_mut().get_mut::<Boost>(kart).unwrap();
        boost.add(0.3, 2.0);
    }
    let steer = KartInput {
        steer: c.steer,
        jump_drift: matches!(c.extra, Extra::Drift),
        ..throttle
    };
    for _ in 0..(CRASH_SECONDS * TICK_HZ as f32).round() as usize {
        drive(&mut app, kart, steer);
        watch.observe(&app);
    }
    watch.outcome
}

/// Runs the crashes side by side, a few threads at a time; the outcomes come back in order.
fn crash_all(crashes: &[Crash]) -> Vec<Outcome> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let mut outcomes = Vec::with_capacity(crashes.len());
    for batch in crashes.chunks(threads) {
        std::thread::scope(|scope| {
            let runs: Vec<_> = batch
                .iter()
                .map(|c| scope.spawn(move || crash(*c)))
                .collect();
            outcomes.extend(runs.into_iter().map(|run| {
                run.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            }));
        });
    }
    outcomes
}

/// Issue #27 acceptance, the crash of the issue: on the start straight, the up arrow for 1.75 s
/// after GO, then the left arrow too. The kart hits the left wall at about 21 m/s across the road
/// (60° off its line), bounces off it and stays on the track, wheels down (tilted by 11° at
/// most). Then the down arrow backs it off the wall, and it drives on. Before the fix, it rose
/// onto the wall (its center 0.97 m above the road within 1.5 s, 1.5 m a little later) and fell
/// out of the track.
#[test]
fn the_crash_of_the_issue_bounces_off_the_wall() {
    let (mut app, kart) = crash_app(Build::Middle);
    while !app.world().resource::<Race>().started() {
        app.update();
    }
    let mut watch = Watch::new(&app, kart);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    for _ in 0..(1.75 * TICK_HZ) as usize {
        app.update();
    }
    hold_keys(&mut app, &[KeyCode::ArrowUp, KeyCode::ArrowLeft]);
    for _ in 0..(1.5 * TICK_HZ) as usize {
        app.update();
        watch.observe(&app);
    }
    let crash = watch.outcome;
    println!("the crash of the issue: {crash:?}");
    assert_eq!(watch.wall_side, Some(-1.0), "never reached the left wall");
    assert!(crash.bounce > 2.0, "no bounce: {crash:?}");
    assert!(crash.highest < 0.75, "lifted: {crash:?}");
    assert!(crash.closest > 0.6, "into the wall: {crash:?}");
    assert!(crash.lowest_up > 0.94, "tipped up: {crash:?}");
    assert!(!crash.towed, "{crash:?}");

    // Nose against the wall: backing off it (the down arrow, 1 s) gets it out, and the test
    // autopilot takes it on, through the next checkpoints, without the pelican.
    hold_keys(&mut app, &[KeyCode::ArrowDown]);
    for _ in 0..TICK_HZ as usize {
        app.update();
        watch.observe(&app);
    }
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    hold_keys(&mut app, &[]);
    let passed = app.world().get::<RaceProgress>(kart).unwrap().passed;
    let layout = app.world().resource::<TrackLayout>().clone();
    for _ in 0..(4.0 * TICK_HZ) as usize {
        autopilot(&mut app, kart, &layout);
        watch.observe(&app);
    }
    assert!(!watch.outcome.towed);
    let now = app.world().get::<RaceProgress>(kart).unwrap().passed;
    assert!(now >= passed + 2, "did not drive on: {passed} → {now}");
}

/// Issue #27, the crashes that sent a kart over a wall before the fix, and more: the walls of the
/// start straight at 3 speeds (14 to 26 m/s), steering fully or half into the left wall, or half
/// into the right one, straight in, in a drift or with a +30 % boost, with light, middle and
/// heavy karts; and the walls after the hairpin, at 25 to 28 m/s. The kart never rises onto a
/// wall (its center stays below 1 m: a hop takes it to 0.89 m, on top of a 0.8 m wall it would be
/// at 1.05 m at least), never comes closer to a wall than its half width, never tips by more than
/// 30° and is never towed. Before the fix, 37 of these 87 crashes broke one of these rules, and
/// 30 ended with the kart's center past the wall's face.
#[test]
fn no_crash_sends_a_kart_over_a_wall() {
    let mut crashes = Vec::new();
    for build in [Build::Middle, Build::Light, Build::Heavy] {
        for straight in [0.75, 1.0, 1.75] {
            for steer in [-1.0, -0.5, 0.5] {
                for extra in [Extra::Nothing, Extra::Drift, Extra::Boost] {
                    crashes.push(Crash {
                        build,
                        autopilot: 0.0,
                        straight,
                        steer,
                        extra,
                    });
                }
            }
        }
    }
    // Out of the hairpin (it ends about 125 m along the lap): into its outer wall on the left, or
    // back into the right one.
    for build in [Build::Middle, Build::Heavy] {
        for (autopilot, steer, extra) in [
            (130.0, -1.0, Extra::Boost),
            (130.0, 1.0, Extra::Nothing),
            (140.0, 1.0, Extra::Drift),
        ] {
            crashes.push(Crash {
                build,
                autopilot,
                straight: 0.0,
                steer,
                extra,
            });
        }
    }
    let outcomes = crash_all(&crashes);
    let mut failures = Vec::new();
    for (c, o) in crashes.iter().zip(&outcomes) {
        println!(
            "{:?} {:>3.0} m {:.2} s steer {:+.1} {:?}: {:.1} m/s, highest {:.2} m, closest {:.2} m, \
             bounce {:.1} m/s, up {:.2}{}",
            c.build,
            c.autopilot,
            c.straight,
            c.steer,
            c.extra,
            o.speed,
            o.highest,
            o.closest,
            o.bounce,
            o.lowest_up,
            if o.towed { ", towed" } else { "" }
        );
        if o.highest >= 1.0 || o.closest < 0.6 || o.lowest_up < 0.85 || o.towed {
            failures.push(format!("{c:?}: {o:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} crashes out of {} over the line:\n{}",
        failures.len(),
        crashes.len(),
        failures.join("\n")
    );
}
