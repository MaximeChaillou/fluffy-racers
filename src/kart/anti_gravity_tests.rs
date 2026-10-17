//! Headless scenarios of the anti-gravity (GD § 10, issue #78) on the Ruban Réglisse: the whole
//! game (`GamePlugin`) with `MinimalPlugins`, its race started, one fixed tick per `app.update()`.
//! The acceptance criteria: the way onto the wall and back down, never through the road, at
//! several speeds, in every class, drifting, on the mirror image too; the spin boost of two karts
//! bumping into each other there, and its pads. Issue #94: the same under the ceiling (the road
//! upside down, the Ruban's second zone), and a kart that comes off it falls.

use bevy_rapier3d::prelude::{GravityScale, Velocity};

use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::Bot;
use crate::interpolation::PhysicsPose;
use crate::race::{Course, Race, RaceProgress};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};
use crate::track::{AntiGravityZone, MirrorMode, TrackChoice, TrackLayout};

const RIBBON: &str = "tracks/ruban.track.ron";

/// The Ruban Réglisse, the player (and `others` more karts, bots no longer driven by their
/// brains), its race started; the zone its runs drive through ([`WALL`] or [`CEILING`]).
struct Ribbon {
    app: App,
    kart: Entity,
    others: Vec<Entity>,
    layout: TrackLayout,
    course: Course,
    zone: AntiGravityZone,
    /// Issue #94: which zone ([`WALL`] or [`CEILING`]).
    which: usize,
}

/// The Ruban's first zone: the wall (90°, issue #78)...
const WALL: usize = 0;
/// ...and its second: the ceiling (180°, issue #94).
const CEILING: usize = 1;

/// What a run saw, tick by tick.
#[derive(Clone, Debug, Default)]
struct Log {
    ticks: u32,
    /// Ticks at which the kart went into anti-gravity, and out of it, with its centerline
    /// distance then.
    entered: Vec<(u32, f32)>,
    left: Vec<(u32, f32)>,
    /// Lowest height of the kart's center above the road under it (m).
    lowest: f32,
    /// Its up the furthest from the road's normal there, on the ground, in anti-gravity (the
    /// cosine of the angle).
    worst_alignment: f32,
    /// Ticks on the wall itself (the road banked 80° or more), and those of them with a wheel on
    /// the road.
    on_the_wall: u32,
    wall_grounded: u32,
    /// The pelican came.
    towed: bool,
    /// The gravity scale and the top speed factor of its stats seen in anti-gravity.
    gravity_in_zone: Option<f32>,
    stats_in_zone: Option<f32>,
    /// Forward speed entering the zone and leaving it (m/s).
    speed_in: Option<f32>,
    speed_out: Option<f32>,
    /// Ticks drifting on the wall.
    drifting_on_the_wall: u32,
    /// Issue #94: ticks upside down (the road banked 170° or more), those of them with a wheel on
    /// the road, and drifting.
    upside_down: u32,
    ceiling_grounded: u32,
    drifting_upside_down: u32,
    /// Issue #94: the checkpoints the kart validated over the run (its race progress, through the
    /// gates).
    gates: u32,
}

impl Ribbon {
    fn new(class: SpeedClass, mirrored: bool, others: usize) -> Self {
        Self::at(class, mirrored, others, WALL)
    }

    /// The same, its runs through the zone `which` ([`WALL`] or [`CEILING`]).
    fn at(class: SpeedClass, mirrored: bool, others: usize, which: usize) -> Self {
        let mut app = game_app_unloaded(TrackChoice::Circuit(RIBBON.into()), others);
        app.insert_resource(RaceClass(class))
            .insert_resource(MirrorMode(mirrored));
        single_threaded(&mut app);
        wait_for_the_player(&mut app);
        let kart = player(&mut app);
        while app.world().get_resource::<Race>().is_none()
            || app
                .world_mut()
                .query_filtered::<(), With<Bot>>()
                .iter(app.world())
                .count()
                < others
        {
            app.update();
        }
        skip_countdown_now(app.world_mut());
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<Race>().started());
        // The test drives, not the keyboard nor the bots' brains.
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
        let world = app.world_mut();
        let others: Vec<Entity> = world
            .query_filtered::<Entity, With<Bot>>()
            .iter(world)
            .collect();
        for other in &others {
            app.world_mut().entity_mut(*other).remove::<Bot>();
        }
        let layout = app.world().resource::<TrackLayout>().clone();
        let course = app.world().resource::<Course>().clone();
        assert_eq!(layout.mirrored, mirrored);
        assert_eq!(
            layout.anti_gravity.len(),
            2,
            "the wall's zone and the ceiling's"
        );
        let zone = layout.anti_gravity[which];
        Self {
            app,
            kart,
            others,
            layout,
            course,
            zone,
            which,
        }
    }

    /// Puts `kart` on the road at centerline distance `distance`, `lateral` meters right of the
    /// centerline, heading down the track at `speed`, its race progress there; in anti-gravity
    /// in the zone (as if it had driven there), out of it elsewhere.
    fn place(&mut self, kart: Entity, distance: f32, lateral: f32, speed: f32) {
        let sample = self.layout.center.at(distance);
        let transform = sample.transform(lateral, crate::track::GRID_HEIGHT);
        let spacing = self.course.checkpoints[1];
        let passed = 1 + (distance / spacing).floor() as u32;
        let world = self.app.world_mut();
        *world.get_mut::<Transform>(kart).unwrap() = transform;
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity {
            linear: sample.forward * speed,
            angular: Vec3::ZERO,
        };
        world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
        *world.get_mut::<KartState>(kart).unwrap() = KartState::Grounded;
        *world.get_mut::<DriftMotion>(kart).unwrap() = DriftMotion::default();
        let zoned =
            crate::track::anti_gravity_at(&self.layout.anti_gravity, &self.layout.center, distance)
                .is_some();
        *world.get_mut::<KartEnvironment>(kart).unwrap() = KartEnvironment {
            anti_gravity: zoned,
            up: if zoned { sample.up } else { Vec3::Y },
            ..default()
        };
        *world.get_mut::<KartWheels>(kart).unwrap() = KartWheels {
            ground_normal: if zoned { sample.up } else { Vec3::Y },
            ..default()
        };
        *world.get_mut::<Boost>(kart).unwrap() = Boost::default();
        *world.get_mut::<Slipstream>(kart).unwrap() = Slipstream::default();
        world
            .get_mut::<crate::race::Tow>(kart)
            .unwrap()
            .reset(transform.translation);
        let mut progress = world.get_mut::<RaceProgress>(kart).unwrap();
        progress.passed = passed;
        progress.distance = distance;
    }

    fn environment(&self, kart: Entity) -> KartEnvironment {
        *self.app.world().get::<KartEnvironment>(kart).unwrap()
    }

    fn transform(&self, kart: Entity) -> Transform {
        *self.app.world().get::<Transform>(kart).unwrap()
    }

    fn velocity(&self, kart: Entity) -> Vec3 {
        self.app.world().get::<Velocity>(kart).unwrap().linear
    }

    fn state(&self, kart: Entity) -> KartState {
        *self.app.world().get::<KartState>(kart).unwrap()
    }

    fn boost(&self, kart: Entity) -> Boost {
        *self.app.world().get::<Boost>(kart).unwrap()
    }

    /// The centerline sample nearest `kart`, searched along the race (its progress window).
    fn sample(&self, kart: Entity) -> crate::track::TrackSample {
        let progress = self.app.world().get::<RaceProgress>(kart).unwrap();
        let projection = self
            .course
            .project(progress.passed, self.transform(kart).translation);
        self.layout.center.at(projection.distance)
    }

    /// How far right of the centerline `kart` is (m), along the road.
    fn lateral(&self, kart: Entity) -> f32 {
        let s = self.sample(kart);
        (self.transform(kart).translation - s.position).dot(s.right)
    }

    /// Height of `kart`'s center above the road under it (m).
    fn height(&self, kart: Entity) -> f32 {
        let s = self.sample(kart);
        (self.transform(kart).translation - s.position).dot(s.up)
    }

    /// The road's banking under `kart` (degrees, its absolute value).
    fn bank(&self, kart: Entity) -> f32 {
        let s = self.sample(kart);
        let flat_right = s.forward.cross(Vec3::Y).normalize_or(Vec3::X);
        s.right.angle_between(flat_right).to_degrees()
    }

    /// One tick with `input` for the player's kart (and `others` for the other karts), its events
    /// in `log`.
    fn tick(&mut self, input: KartInput, others: &[KartInput], log: &mut Log) {
        let kart = self.kart;
        let before = self.environment(kart);
        let world = self.app.world_mut();
        *world.get_mut::<KartInput>(kart).unwrap() = input;
        for (other, input) in self.others.iter().zip(others) {
            *world.get_mut::<KartInput>(*other).unwrap() = *input;
        }
        self.app.update();
        log.ticks += 1;
        let after = self.environment(kart);
        let distance = self.sample(kart).distance;
        let forward_speed = self.velocity(kart).dot(*self.transform(kart).forward());
        if after.anti_gravity && !before.anti_gravity {
            log.entered.push((log.ticks, distance));
            log.speed_in.get_or_insert(forward_speed);
        }
        if !after.anti_gravity && before.anti_gravity {
            log.left.push((log.ticks, distance));
            log.speed_out = Some(forward_speed);
        }
        let state = self.state(kart);
        log.towed |= state == KartState::Respawning;
        log.lowest = log.lowest.min(self.height(kart));
        let world = self.app.world();
        let wheels = *world.get::<KartWheels>(kart).unwrap();
        let s = self.sample(kart);
        if after.anti_gravity {
            log.gravity_in_zone = Some(world.get::<GravityScale>(kart).unwrap().0);
            log.stats_in_zone = Some(world.get::<MaxSpeedFactors>(kart).unwrap().stats);
            if wheels.wheels_on_ground() > 0 {
                let up = *self.transform(kart).up();
                log.worst_alignment = log.worst_alignment.min(up.dot(s.up));
            }
        }
        if self.bank(kart) >= 80.0 {
            log.on_the_wall += 1;
            log.wall_grounded += u32::from(wheels.wheels_on_ground() > 0);
            log.drifting_on_the_wall += u32::from(matches!(state, KartState::Drifting { .. }));
        }
        if self.bank(kart) >= 170.0 {
            log.upside_down += 1;
            log.ceiling_grounded += u32::from(wheels.wheels_on_ground() > 0);
            log.drifting_upside_down += u32::from(matches!(state, KartState::Drifting { .. }));
        }
    }

    /// The test's autopilot for `kart`: toward its lane (`lateral` meters right of the
    /// centerline) a bit ahead, in its own frame (on the wall too), at `speed` at most.
    fn autopilot(&self, kart: Entity, lateral: f32, speed: f32) -> KartInput {
        let transform = self.transform(kart);
        let forward = self.velocity(kart).dot(*transform.forward());
        let s = self.sample(kart);
        let target = self
            .layout
            .center
            .at(s.distance + 6.0 + 0.4 * forward.max(0.0))
            .point(lateral, 0.5);
        let to = target - transform.translation;
        let angle = to
            .dot(*transform.right())
            .atan2(to.dot(*transform.forward()));
        KartInput {
            throttle: if forward > speed { 0.0 } else { 1.0 },
            steer: (angle * 3.0).clamp(-1.0, 1.0),
            ..default()
        }
    }
}

fn new_log() -> Log {
    Log {
        lowest: f32::MAX,
        worst_alignment: 1.0,
        ..default()
    }
}

/// How a run drives through the zone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Driving {
    /// Throttle up to a speed, on a lane.
    Cruise { speed: f32, lateral: f32 },
    /// Full throttle, a boost from the start (a mushroom, a mini-turbo): the fastest it goes.
    Boosted,
    /// Drifting from before the zone to beyond it, left and right in turn (a drift on a straight
    /// cannot last: it always turns), a short release between two.
    Drift,
}

/// A drift of the scenarios: the button held this many ticks (the hop, then about half a second
/// of drift: on a straight, a drift turns from one edge to the other in a second)...
const DRIFT_HOLD: u32 = 50;
/// ...then released this many.
const DRIFT_RELEASE: u32 = 16;

/// One run through the zone from 40 m before it to 40 m past it: what it saw.
fn run_through(ribbon: &mut Ribbon, driving: Driving) -> Log {
    let kart = ribbon.kart;
    let center = ribbon.layout.center.clone();
    let start = ribbon.zone.from - 40.0;
    let end = ribbon.zone.from + ribbon.zone.length(&center) + 40.0;
    let (lane, speed) = match driving {
        Driving::Cruise { speed, lateral } => (lateral, speed),
        Driving::Boosted | Driving::Drift => (0.0, 100.0),
    };
    let entry = match driving {
        Driving::Cruise { speed, .. } => speed.min(25.0),
        _ => 25.0,
    };
    ribbon.place(kart, start, lane, entry);
    if driving == Driving::Boosted {
        ribbon
            .app
            .world_mut()
            .get_mut::<Boost>(kart)
            .unwrap()
            .add(0.3, 30.0);
    }
    let mut log = new_log();
    let mut ticks = 0;
    // Issue #94: the ceiling's zone is longer (the run at 12 m/s lasts about 40 s), and its drift
    // starts afresh where the road is about to be upside down.
    let (seconds, fresh_drift_bank) = match ribbon.which {
        CEILING => (50, 160.0),
        _ => (30, 70.0),
    };
    // The drifts: their side, and the ticks since the last change; whether the kart reached the
    // wall yet.
    let (mut side, mut since, mut at_the_wall) = (-1.0f32, 0u32, false);
    // How far along the run (m): a hair behind the start is the start.
    let along = |d: f32| center.wrap(d - start + 1.0) - 1.0;
    while along(ribbon.sample(kart).distance) < end - start {
        ticks += 1;
        assert!(ticks < 60 * seconds, "{driving:?}: the run never ended");
        let mut input = ribbon.autopilot(kart, lane, speed);
        if driving == Driving::Drift && ticks > 30 {
            // Hop toward `side` (the stick there for the hop), hold the drift (the autopilot's
            // stick widens or tightens it) until it is 3 m off its lane, release, then the other
            // side.
            // Reaching the wall (70°; issue #94: the ceiling, 160°), a fresh drift, whatever the
            // timing of those before it: let go for a tick (unless in the middle of a hop), then
            // hop again. It lands on the wall itself (90°; the ceiling, 180°) and drifts there, in
            // every class.
            if !at_the_wall && ribbon.bank(kart) >= fresh_drift_bank {
                at_the_wall = true;
                if !matches!(ribbon.state(kart), KartState::Hop { .. }) {
                    since = DRIFT_HOLD + DRIFT_RELEASE - 1;
                }
            }
            since += 1;
            let drifting = matches!(ribbon.state(kart), KartState::Drifting { .. });
            if drifting && ribbon.lateral(kart).abs() > 3.0 && since <= DRIFT_HOLD {
                since = DRIFT_HOLD + 1;
            }
            if since <= DRIFT_HOLD {
                input.jump_drift = true;
                if since <= 8 && !drifting {
                    input.steer = side;
                }
            } else if since >= DRIFT_HOLD + DRIFT_RELEASE {
                (side, since) = (-side, 0);
            }
        }
        let passed = |ribbon: &Ribbon| ribbon.app.world().get::<RaceProgress>(kart).unwrap().passed;
        let before = passed(ribbon);
        ribbon.tick(input, &[], &mut log);
        log.gates += passed(ribbon).saturating_sub(before);
        if log.towed {
            break;
        }
    }
    log
}

/// Issue #78 acceptance (GD § 10): onto the wall and back down. The kart goes into anti-gravity
/// once, at the start of the zone (the road still flat there), and out of it once, at its end;
/// in between, its gravity pulls it to the road (the world's gravity off), it drives with its
/// stats « anti-gravité », its up follows the road's normal on the ground (through the twist up
/// to 90° and back), its wheels stay on the wall (but for the hops of the drifts), it never goes
/// through the road (its center never below 0.2 m above it, as on the Lagon Duvet) and the
/// pelican never comes. In 50cc, 100cc, 150cc and 200cc, on
/// the mirror image too, slow, at its top speed and boosted, on three lanes, and drifting.
#[test]
fn karts_drive_onto_the_wall_and_back_down() {
    let mut failures = Vec::new();
    for (class, mirrored) in [
        (SpeedClass::Cc50, false),
        (SpeedClass::Cc100, true),
        (SpeedClass::Cc150, false),
        (SpeedClass::Cc150, true),
        (SpeedClass::Cc200, false),
        (SpeedClass::Cc200, true),
    ] {
        let mut ribbon = Ribbon::new(class, mirrored, 0);
        let center = ribbon.layout.center.clone();
        let zone = ribbon.zone;
        let gravity = ribbon.app.world().resource::<KartTuning>().gravity_scale;
        let runs = [
            Driving::Cruise {
                speed: 12.0,
                lateral: 0.0,
            },
            Driving::Cruise {
                speed: 100.0,
                lateral: -3.5,
            },
            Driving::Cruise {
                speed: 100.0,
                lateral: 0.0,
            },
            Driving::Cruise {
                speed: 100.0,
                lateral: 3.5,
            },
            Driving::Boosted,
            Driving::Drift,
        ];
        for driving in runs {
            let log = run_through(&mut ribbon, driving);
            let label = format!(
                "{class:?}{}, {driving:?}",
                if mirrored { " (mirror)" } else { "" }
            );
            println!(
                "{label}: in {:?} out {:?}, lowest {:.2} m, alignment {:.3}, wall {} ticks ({} on \
                 the ground, {} drifting), speed {:?} -> {:?}",
                log.entered,
                log.left,
                log.lowest,
                log.worst_alignment,
                log.on_the_wall,
                log.wall_grounded,
                log.drifting_on_the_wall,
                log.speed_in,
                log.speed_out,
            );
            let near = |at: f32, d: f32| center.wrap(d - at + 15.0) < 30.0;
            let mut check = |ok: bool, what: &str| {
                if !ok {
                    failures.push(format!("{label}: {what}"));
                }
            };
            check(!log.towed, "no pelican");
            check(log.entered.len() == 1, "into anti-gravity once");
            check(log.left.len() == 1, "out of it once");
            check(
                log.entered
                    .first()
                    .is_some_and(|(_, d)| near(zone.from, *d)),
                "in at the start of the zone",
            );
            check(
                log.left.first().is_some_and(|(_, d)| near(zone.to, *d)),
                "out at its end",
            );
            check(log.lowest > 0.2, "never through the road");
            check(log.on_the_wall > 20, "on the wall");
            // On the wall, its wheels on it but for the hops of the drifts.
            let grounded = if driving == Driving::Drift { 0.5 } else { 0.95 };
            check(
                log.wall_grounded as f32 >= grounded * log.on_the_wall as f32,
                "the wheels on the wall",
            );
            check(log.worst_alignment > 0.9, "its up follows the road");
            check(log.gravity_in_zone == Some(0.0), "the world's gravity off");
            check(
                ribbon
                    .app
                    .world()
                    .get::<GravityScale>(ribbon.kart)
                    .unwrap()
                    .0
                    == gravity,
                "the world's gravity back",
            );
            let effects = *ribbon.app.world().get::<StatEffects>(ribbon.kart).unwrap();
            check(
                log.stats_in_zone == Some(effects.on(Terrain::AntiGravity).top_speed),
                "its stats « anti-gravité »",
            );
            if let (Some(speed_in), Some(speed_out)) = (log.speed_in, log.speed_out)
                && matches!(driving, Driving::Cruise { speed: 100.0, .. })
            {
                check(speed_out > 0.85 * speed_in, "its speed kept on the wall");
            }
            if driving == Driving::Drift {
                check(log.drifting_on_the_wall > 20, "drifting on the wall");
            }
            let back = ribbon.environment(ribbon.kart);
            check(
                !back.anti_gravity && back.up == Vec3::Y,
                "the world's up again",
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #94 acceptance (GD § 10: « on roule sur les murs et les plafonds »): under the ceiling
/// and back. The Ruban's second zone rolls the road over onto its back (180°): the kart goes into
/// anti-gravity once, at the start of the zone, and out of it once, at its end; in between, its
/// gravity pulls it up to the road over it (the world's off), its up follows the road's normal
/// on the ground (down under the ceiling), its wheels stay on the road upside down (but for the
/// hops of the drifts), it never goes through the road nor falls off it (its center never below
/// 0.2 m from it, the pelican never comes), with its stats « anti-gravité », its speed kept. In
/// 50cc, 100cc, 150cc and 200cc, on the mirror image too, slow, at its top speed and boosted, on
/// three lanes, and drifting under the ceiling.
#[test]
fn karts_drive_under_the_ceiling_and_back() {
    let mut failures = Vec::new();
    for (class, mirrored) in [
        (SpeedClass::Cc50, false),
        (SpeedClass::Cc100, true),
        (SpeedClass::Cc150, false),
        (SpeedClass::Cc150, true),
        (SpeedClass::Cc200, false),
        (SpeedClass::Cc200, true),
    ] {
        let mut ribbon = Ribbon::at(class, mirrored, 0, CEILING);
        let center = ribbon.layout.center.clone();
        let zone = ribbon.zone;
        let gravity = ribbon.app.world().resource::<KartTuning>().gravity_scale;
        let runs = [
            Driving::Cruise {
                speed: 12.0,
                lateral: 0.0,
            },
            Driving::Cruise {
                speed: 100.0,
                lateral: -3.5,
            },
            Driving::Cruise {
                speed: 100.0,
                lateral: 0.0,
            },
            Driving::Cruise {
                speed: 100.0,
                lateral: 3.5,
            },
            Driving::Boosted,
            Driving::Drift,
        ];
        for driving in runs {
            let log = run_through(&mut ribbon, driving);
            let label = format!(
                "{class:?}{}, {driving:?}",
                if mirrored { " (mirror)" } else { "" }
            );
            println!(
                "{label}: in {:?} out {:?}, lowest {:.2} m, alignment {:.3}, upside down {} ticks \
                 ({} on the ground, {} drifting), speed {:?} -> {:?}",
                log.entered,
                log.left,
                log.lowest,
                log.worst_alignment,
                log.upside_down,
                log.ceiling_grounded,
                log.drifting_upside_down,
                log.speed_in,
                log.speed_out,
            );
            let near = |at: f32, d: f32| center.wrap(d - at + 15.0) < 30.0;
            let mut check = |ok: bool, what: &str| {
                if !ok {
                    failures.push(format!("{label}: {what}"));
                }
            };
            check(!log.towed, "no pelican");
            check(log.entered.len() == 1, "into anti-gravity once");
            check(log.left.len() == 1, "out of it once");
            check(
                log.entered
                    .first()
                    .is_some_and(|(_, d)| near(zone.from, *d)),
                "in at the start of the zone",
            );
            check(
                log.left.first().is_some_and(|(_, d)| near(zone.to, *d)),
                "out at its end",
            );
            check(log.lowest > 0.2, "never through the road");
            check(log.upside_down > 20, "upside down");
            // Through every gate on the way (the race sees the kart under the road).
            let gates = ribbon
                .course
                .checkpoints
                .iter()
                .filter(|d| center.wrap(**d - (zone.from - 40.0)) < zone.length(&center) + 80.0)
                .count() as u32;
            check(
                log.gates >= gates.saturating_sub(1) && gates > 10,
                "through the gates",
            );
            // Under the ceiling, its wheels on it but for the hops of the drifts.
            let grounded = if driving == Driving::Drift { 0.5 } else { 0.95 };
            check(
                log.ceiling_grounded as f32 >= grounded * log.upside_down as f32,
                "the wheels on the ceiling",
            );
            check(log.worst_alignment > 0.9, "its up follows the road");
            check(log.gravity_in_zone == Some(0.0), "the world's gravity off");
            check(
                ribbon
                    .app
                    .world()
                    .get::<GravityScale>(ribbon.kart)
                    .unwrap()
                    .0
                    == gravity,
                "the world's gravity back",
            );
            let effects = *ribbon.app.world().get::<StatEffects>(ribbon.kart).unwrap();
            check(
                log.stats_in_zone == Some(effects.on(Terrain::AntiGravity).top_speed),
                "its stats « anti-gravité »",
            );
            if let (Some(speed_in), Some(speed_out)) = (log.speed_in, log.speed_out)
                && matches!(driving, Driving::Cruise { speed: 100.0, .. })
            {
                check(speed_out > 0.85 * speed_in, "its speed kept upside down");
            }
            if driving == Driving::Drift {
                check(log.drifting_upside_down > 20, "drifting under the ceiling");
            }
            let back = ribbon.environment(ribbon.kart);
            check(
                !back.anti_gravity && back.up == Vec3::Y,
                "the world's up again",
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #94 (GD § 10): standing still under the ceiling, a kart stays on it (its gravity pulls
/// it up to the road, as strong as the world's: MK8DX's karts never drop off a road upside down);
/// it drives off again from there.
#[test]
fn a_kart_stopped_under_the_ceiling_stays_on_it() {
    let mut ribbon = Ribbon::at(SpeedClass::Cc150, false, 0, CEILING);
    let kart = ribbon.kart;
    let ceiling = ribbon.zone.from + ribbon.zone.length(&ribbon.layout.center) / 2.0;
    assert!(ribbon.layout.center.at(ceiling).up.y < -0.999);
    ribbon.place(kart, ceiling, 1.0, 0.0);
    let mut log = new_log();
    for _ in 0..180 {
        ribbon.tick(KartInput::default(), &[], &mut log);
    }
    let world = ribbon.app.world();
    let wheels = *world.get::<KartWheels>(kart).unwrap();
    assert_eq!(
        wheels.wheels_on_ground(),
        4,
        "on its wheels under the ceiling"
    );
    assert!(ribbon.environment(kart).anti_gravity);
    assert!(
        ribbon.velocity(kart).length() < 0.2,
        "{}",
        ribbon.velocity(kart)
    );
    assert!((ribbon.height(kart) - crate::track::GRID_HEIGHT).abs() < 0.15);
    assert!(log.lowest > 0.2 && !log.towed);
    // And off again, under the ceiling.
    for _ in 0..120 {
        let input = ribbon.autopilot(kart, 1.0, 100.0);
        ribbon.tick(input, &[], &mut log);
    }
    assert!(ribbon.velocity(kart).length() > 10.0);
    assert!(ribbon.environment(kart).anti_gravity && !log.towed && log.lowest > 0.2);
}

/// Issue #94 (GD § 10, « tombé »): a kart that comes off the ceiling (beyond its edge, where no
/// road is along its up any more) lets go: out of the anti-gravity, the world's gravity back, it
/// falls toward the world's ground, and the pelican comes for it and drops it before the zone, on
/// the road, out of the anti-gravity. A kart that fell under the ceiling, more than `fall_depth`
/// from it, is picked up at once.
#[test]
fn a_kart_off_the_ceiling_falls_and_the_pelican_comes() {
    let mut ribbon = Ribbon::at(SpeedClass::Cc150, false, 0, CEILING);
    let kart = ribbon.kart;
    let center = ribbon.layout.center.clone();
    let gravity = ribbon.app.world().resource::<KartTuning>().gravity_scale;
    let ceiling = ribbon.zone.from + ribbon.zone.length(&center) / 2.0;
    let s = center.at(ceiling);
    // Beyond the ceiling's edge (its wall's outer face at 8.5 m), in the air, its gravity still
    // toward the road.
    let lateral = crate::track::mesh::wall_offset(&s) + 3.0;
    ribbon.place(kart, ceiling, lateral, 20.0);
    let start = ribbon.transform(kart).translation;
    let mut log = new_log();
    let (mut let_go, mut lowest) = (None, f32::MAX);
    for tick in 0..240 {
        ribbon.tick(KartInput::default(), &[], &mut log);
        if let_go.is_none() && !ribbon.environment(kart).anti_gravity {
            let_go = Some(tick);
            assert_eq!(
                ribbon.app.world().get::<GravityScale>(kart).unwrap().0,
                gravity,
                "the world's gravity back"
            );
        }
        if log.towed {
            break;
        }
        lowest = lowest.min(ribbon.transform(kart).translation.y);
    }
    assert!(let_go.is_some_and(|t| t < 3), "it lets go: {let_go:?}");
    assert!(start.y - lowest > 3.0, "it falls: {} -> {lowest}", start.y);
    assert!(log.towed, "the pelican comes");
    let reason = ribbon
        .app
        .world()
        .get::<crate::race::Tow>(kart)
        .unwrap()
        .carry
        .map(|carry| carry.reason);
    assert!(
        matches!(
            reason,
            Some(crate::race::TowReason::Fell | crate::race::TowReason::OutOfTrack)
        ),
        "{reason:?}"
    );
    // Carried, then dropped before the zone, on the road, out of the anti-gravity.
    for _ in 0..200 {
        ribbon.tick(KartInput::default(), &[], &mut log);
    }
    assert_eq!(ribbon.state(kart), KartState::Grounded);
    let dropped = ribbon.sample(kart);
    assert!(!ribbon.zone.contains(&center, dropped.distance));
    assert!(!ribbon.environment(kart).anti_gravity);
    assert_eq!(ribbon.environment(kart).up, Vec3::Y);
    // Fallen under the ceiling, 6 m from it (its gravity the world's): the pelican at once.
    ribbon.place(kart, ceiling, 0.0, 0.0);
    let world = ribbon.app.world_mut();
    let fallen = s.point(0.0, 6.0);
    world.get_mut::<Transform>(kart).unwrap().translation = fallen;
    world
        .get_mut::<PhysicsPose>(kart)
        .unwrap()
        .snap(Transform::from_translation(fallen));
    *world.get_mut::<KartEnvironment>(kart).unwrap() = KartEnvironment::default();
    let mut log = new_log();
    ribbon.tick(KartInput::default(), &[], &mut log);
    assert!(log.towed, "fell from the ceiling");
    let reason = ribbon
        .app
        .world()
        .get::<crate::race::Tow>(kart)
        .unwrap()
        .carry
        .map(|carry| carry.reason);
    assert_eq!(reason, Some(crate::race::TowReason::Fell));
}

/// Two karts on `ribbon` at centerline distance `at`, on the lane `lateral`: the player's kart
/// 4 m behind the other one, faster (28 against 20 m/s), both driving straight on: it runs into
/// the other one. Returns, over 1 s, the strongest boost each one got, and whether the other one
/// was bumped (`KartBump`).
fn rear_end(ribbon: &mut Ribbon, at: f32, lateral: f32) -> (f32, f32, bool) {
    let (kart, other) = (ribbon.kart, ribbon.others[0]);
    ribbon.place(kart, at - 4.0, lateral, 28.0);
    ribbon.place(other, at, lateral, 20.0);
    let mut log = new_log();
    let (mut mine, mut theirs, mut bumped) = (0.0f32, 0.0f32, false);
    let strength = |boost: Boost| {
        if boost.is_active() {
            boost.strength
        } else {
            0.0
        }
    };
    let mut cursor = ribbon
        .app
        .world()
        .resource::<Messages<KartBump>>()
        .get_cursor_current();
    for _ in 0..60 {
        let input = KartInput {
            throttle: 1.0,
            steer: ribbon.autopilot(kart, lateral, 100.0).steer,
            ..default()
        };
        let slow = KartInput {
            throttle: 0.4,
            steer: ribbon.autopilot(other, lateral, 20.0).steer,
            ..default()
        };
        ribbon.tick(input, &[slow], &mut log);
        mine = mine.max(strength(ribbon.boost(kart)));
        theirs = theirs.max(strength(ribbon.boost(other)));
        let messages = ribbon.app.world().resource::<Messages<KartBump>>();
        bumped |= cursor.read(messages).any(|bump| bump.kart == other);
    }
    (mine, theirs, bumped)
}

/// Issue #78 acceptance (GD § 10, MK8DX: the spin boost): two karts in anti-gravity that bump
/// into each other both get a boost, the one bumped and the one bumping (`spin_boost_strength`);
/// out of the zone, the same bump gives neither a boost (the one in front is bumped as before).
/// On the wall rising to 90°, and on the mirror image.
#[test]
fn a_bump_in_anti_gravity_boosts_both_karts() {
    for mirrored in [false, true] {
        let mut ribbon = Ribbon::new(SpeedClass::Cc150, mirrored, 1);
        let tuning = ribbon
            .app
            .world()
            .resource::<KartTuning>()
            .environment
            .clone();
        // Where the road is banked 75°, rising to the wall (and before its pads).
        let wall = ribbon.layout.center.param_to_distance(8.0);
        let (mine, theirs, bumped) = rear_end(&mut ribbon, wall, 1.0);
        assert!(
            bumped,
            "mirror {mirrored}: the kart in front bumped on the wall"
        );
        assert!(
            (mine - tuning.spin_boost_strength).abs() < 1e-6
                && (theirs - tuning.spin_boost_strength).abs() < 1e-6,
            "mirror {mirrored}: both karts boosted on the wall: {mine} {theirs}"
        );
        assert!(
            ribbon.environment(ribbon.kart).anti_gravity
                && ribbon.environment(ribbon.others[0]).anti_gravity
        );
        // On the start straight: a bump, no boost.
        let (mine, theirs, bumped) = rear_end(&mut ribbon, 20.0, 1.0);
        assert!(bumped, "mirror {mirrored}: the kart in front bumped");
        assert_eq!(
            (mine, theirs),
            (0.0, 0.0),
            "mirror {mirrored}: no boost out of the zone"
        );
    }
}

/// Issue #94 (GD § 10, the spin boost): under the ceiling too, two karts that bump into each
/// other both get the boost, on the mirror image as well.
#[test]
fn a_bump_under_the_ceiling_boosts_both_karts() {
    for mirrored in [false, true] {
        let mut ribbon = Ribbon::at(SpeedClass::Cc150, mirrored, 1, CEILING);
        let strength = ribbon
            .app
            .world()
            .resource::<KartTuning>()
            .environment
            .spin_boost_strength;
        // Upside down, past its pads (their boost is another).
        let ceiling = ribbon.layout.center.param_to_distance(40.4);
        assert!(ribbon.layout.center.at(ceiling).up.y < -0.98);
        let (mine, theirs, bumped) = rear_end(&mut ribbon, ceiling, 1.0);
        assert!(bumped, "mirror {mirrored}: bumped under the ceiling");
        assert!(
            (mine - strength).abs() < 1e-6 && (theirs - strength).abs() < 1e-6,
            "mirror {mirrored}: both karts boosted under the ceiling: {mine} {theirs}"
        );
        for kart in [ribbon.kart, ribbon.others[0]] {
            let environment = ribbon.environment(kart);
            assert!(environment.anti_gravity && environment.up.y < -0.5);
        }
    }
}

/// Issue #78 (GD § 10: « des plots dédiés donnent aussi un boost »): the pads on the wall (90°)
/// give the pads' boost to a kart driving over them, on its lane, like the pads on the ground.
#[test]
fn the_pads_on_the_wall_boost() {
    pads_boost(WALL, |pad| pad.up().y.abs() < 0.1, "on the wall");
}

/// Issue #94: the same under the ceiling (180°): its two pads, upside down, boost.
#[test]
fn the_pads_on_the_ceiling_boost() {
    pads_boost(CEILING, |pad| pad.up().y < -0.99, "on the ceiling");
}

/// The two pads of the zone `which` (`mine` tells them from the others): a kart driving over each
/// one, on its lane, gets the pads' boost, in anti-gravity.
fn pads_boost(which: usize, mine: impl Fn(&Transform) -> bool, what: &str) {
    let mut ribbon = Ribbon::at(SpeedClass::Cc150, false, 0, which);
    let kart = ribbon.kart;
    let center = ribbon.layout.center.clone();
    let tuning = ribbon.app.world().resource::<KartTuning>().clone();
    let world = ribbon.app.world_mut();
    let pads: Vec<Transform> = world
        .query_filtered::<&Transform, With<crate::track::BoostPad>>()
        .iter(world)
        .copied()
        .collect();
    let on_the_wall: Vec<Transform> = pads.into_iter().filter(|pad| mine(pad)).collect();
    assert_eq!(on_the_wall.len(), 2, "two pads {what}");
    for pad in on_the_wall {
        let s = center.samples()[center.nearest_index(pad.translation)];
        let lateral = (pad.translation - s.position).dot(s.right);
        ribbon.place(kart, s.distance - 30.0, lateral, 25.0);
        let mut log = new_log();
        let mut boosted = 0.0f32;
        for _ in 0..90 {
            let input = ribbon.autopilot(kart, lateral, 25.0);
            ribbon.tick(input, &[], &mut log);
            let boost = ribbon.boost(kart);
            if boost.is_active() {
                boosted = boosted.max(boost.strength);
            }
        }
        assert!(ribbon.environment(kart).anti_gravity);
        assert!(
            (boosted - tuning.boost_pad_strength).abs() < 1e-6,
            "the pad at {lateral:.1} m {what}: {boosted}"
        );
    }
}

/// Issue #78: the items that roll or lie on the road do it on the wall too, in the road's frame:
/// a pelote thrown ahead on the wall (90°) rolls along it, its center its radius above the road,
/// never falling off; a pelure dropped behind lies on the wall, just above it.
#[test]
fn items_roll_and_lie_on_the_wall() {
    items_roll_and_lie_on(WALL, 9.4, |up| up.y.abs() < 0.01);
}

/// Issue #94: the same under the ceiling (180°): the pelote rolls along it, upside down, the
/// pelure lies on it; neither falls to the ground.
#[test]
fn items_roll_and_lie_under_the_ceiling() {
    items_roll_and_lie_on(CEILING, 39.4, |up| up.y < -0.999);
}

/// A pelote thrown ahead and a pelure dropped behind at the spline parameter `at` of the zone
/// `which` (its road's normal there as `facing` says): the pelote rolls along the road, its center
/// its radius above it, never falling off; the pelure lies on it, just above it.
fn items_roll_and_lie_on(which: usize, at: f32, facing: impl Fn(Vec3) -> bool) {
    use crate::items::{ItemKind, ItemObject, ItemSlots, Motion};
    let mut ribbon = Ribbon::at(SpeedClass::Cc150, false, 0, which);
    let kart = ribbon.kart;
    let center = ribbon.layout.center.clone();
    let wall = center.param_to_distance(at);
    assert!(facing(center.at(wall).up), "on the wall");
    // Where an item is relative to the road: along it, across it, and above it.
    let on_road = |at: Vec3| {
        let s = center.samples()[center.nearest_index(at)];
        let offset = at - s.position;
        (
            offset.dot(s.forward) + s.distance,
            offset.dot(s.right),
            offset.dot(s.up),
        )
    };
    for (item, back) in [(ItemKind::Pelote, false), (ItemKind::Pelure, true)] {
        ribbon.place(kart, wall - 30.0, 0.0, 20.0);
        {
            let world = ribbon.app.world_mut();
            world.get_mut::<ItemSlots>(kart).unwrap().clear();
            assert!(world.get_mut::<ItemSlots>(kart).unwrap().give(item, 1, 0));
        }
        let mut log = new_log();
        let drive = |ribbon: &Ribbon| ribbon.autopilot(kart, 0.0, 20.0);
        for _ in 0..60 {
            let input = drive(&ribbon);
            ribbon.tick(input, &[], &mut log);
        }
        // A press of the item button: forward for the pelote, backward (the brake held) for the
        // pelure.
        for press in [true, false] {
            let input = KartInput {
                item: press,
                brake: if back { 1.0 } else { 0.0 },
                throttle: if back { 0.0 } else { 1.0 },
                ..drive(&ribbon)
            };
            ribbon.tick(input, &[], &mut log);
        }
        let world = ribbon.app.world_mut();
        let thrown: Vec<Entity> = world
            .query_filtered::<Entity, With<ItemObject>>()
            .iter(world)
            .collect();
        assert_eq!(thrown.len(), 1, "{item:?} out");
        let object = thrown[0];
        let radius = |o: &ItemObject| match o.motion {
            Motion::Rolling { .. } => 0.4,
            _ => 0.0,
        };
        let first = *ribbon.app.world().get::<ItemObject>(object).unwrap();
        let (start, across, _) = on_road(first.position);
        let mut last = start;
        for tick in 0..50 {
            let input = drive(&ribbon);
            ribbon.tick(input, &[], &mut log);
            let Some(o) = ribbon.app.world().get::<ItemObject>(object).copied() else {
                panic!("{item:?} gone after {tick} ticks");
            };
            let (along, lateral, height) = on_road(o.position);
            assert!(
                (-0.05..1.0).contains(&height),
                "{item:?} at {height} m above the wall, tick {tick}"
            );
            // It does not slide down the wall (the world's gravity, along the road there).
            assert!(
                (lateral - across).abs() < 1.0,
                "{item:?} {lateral} m across the wall (thrown at {across}), tick {tick}"
            );
            if matches!(o.motion, Motion::Rolling { .. }) {
                assert!(along > last - 0.01, "{item:?} rolls on along the wall");
                assert!(
                    (height - radius(&o)).abs() < 0.1,
                    "{item:?} rolls on the wall: {height}"
                );
            }
            last = along;
        }
        if item == ItemKind::Pelote {
            assert!(last - start > 15.0, "the pelote rolled {} m", last - start);
        } else {
            assert!(
                (last - start).abs() < 0.5,
                "the pelure lies where it was dropped"
            );
        }
        let world = ribbon.app.world_mut();
        world.entity_mut(object).despawn();
    }
}

/// Issue #78 (GD § 10: « les roues pivotent »), shown only: a wheel pivots from upright (its rest,
/// its axle across the kart) to flat under it, its outer face down, the wheels of each side the
/// other way; half way, half way.
#[test]
fn the_wheels_pivot_flat_their_outer_face_down() {
    // The primitive wheel: a cylinder along its local Y, turned so that its axle is across.
    let rest = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    assert_eq!(pivoted_wheel(rest, -0.55, 0.0), rest);
    for (side, outer) in [(-0.55f32, Vec3::NEG_X), (0.55, Vec3::X)] {
        // Its outer face (its axle, outward) at rest, then flat: down.
        let axle = |rotation: Quat| rotation * (rest.inverse() * outer);
        assert!(axle(pivoted_wheel(rest, side, 0.0)).dot(outer) > 0.999);
        assert!(
            axle(pivoted_wheel(rest, side, 1.0)).dot(Vec3::NEG_Y) > 0.999,
            "{side}"
        );
        let half = axle(pivoted_wheel(rest, side, 0.5));
        assert!((half.angle_between(outer) - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
        assert_eq!(
            pivoted_wheel(rest, side, 3.0),
            pivoted_wheel(rest, side, 1.0)
        );
    }
}
