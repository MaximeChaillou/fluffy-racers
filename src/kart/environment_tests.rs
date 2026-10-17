//! Headless scenarios of the special environments (GD § 10, issue #20) on the Lagon Duvet: the
//! whole game (`GamePlugin`) with `MinimalPlugins`, the player alone, its race started, one fixed
//! tick per `app.update()`. The acceptance criteria: for the glider and the water, the way in and
//! the way out, and no fall through the track, at several speeds (issue #118: up to the top speed
//! and boosted beyond it), and drifting.

use bevy_rapier3d::prelude::{GravityScale, Velocity};

use super::*;
use crate::admin::skip_countdown_now;
use crate::characters::{Loadout, PartKind, PlayerLoadout, Roster};
use crate::interpolation::PhysicsPose;
use crate::race::{Course, Race, RaceProgress};
use crate::tests::{
    follow_centerline, game_app_unloaded, player, single_threaded, wait_for_the_player,
};
use crate::track::{Gap, Lake, MirrorMode, TrackChoice, TrackLayout, gap_at};

const LAGOON: &str = "tracks/lagon.track.ron";

/// How fast the kart of a run goes. Issue #118: the game has a single speed (no more speed
/// classes); the runs that went through issue #19's classes go up to the kart's top speed, and
/// boosted beyond it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pace {
    /// Up to its top speed (`KartTuning::max_speed`: 28.5 m/s).
    Top,
    /// Boosted all along the run with the strength of a purple mini-turbo (the strongest boost of
    /// the game, a piment's too: +30 %, 37 m/s).
    Boosted,
}

/// The Lagon Duvet, the player alone on the grid, its race started.
struct Lagoon {
    app: App,
    kart: Entity,
    layout: TrackLayout,
    course: Course,
    gap: Gap,
    lake: Lake,
}

/// What the scenario saw, tick by tick.
#[derive(Clone, Debug, Default)]
struct Log {
    /// Ticks at which the glider opened, closed; went under water, came out.
    opened: Vec<u32>,
    closed: Vec<u32>,
    dived: Vec<u32>,
    surfaced: Vec<u32>,
    /// Lowest height of the kart's center above the road, out of the gap (m).
    lowest: f32,
    /// The pelican came.
    towed: bool,
    /// Highest point over the gap, above its lower end (m).
    highest_over_gap: f32,
    /// Fastest and slowest forward speeds under water, once the water's slowness set in (m/s).
    fastest_under_water: f32,
    /// The top speed factors seen while gliding and under water.
    air_stats: Option<f32>,
    water_factors: Option<(f32, f32)>,
    /// The gravity scale seen under water and while gliding.
    water_gravity: Option<f32>,
    glide_gravity: Option<f32>,
    /// Ticks so far.
    ticks: u32,
    /// The kart was drifting as it went under water, and still the tick after.
    drift_into_water: bool,
}

impl Lagoon {
    fn new(mirrored: bool, loadout: Option<Loadout>) -> Self {
        let mut app = game_app_unloaded(TrackChoice::Circuit(LAGOON.into()), 0);
        app.insert_resource(MirrorMode(mirrored));
        if let Some(loadout) = loadout {
            app.insert_resource(PlayerLoadout(loadout));
        }
        single_threaded(&mut app);
        wait_for_the_player(&mut app);
        let kart = player(&mut app);
        while app.world().get_resource::<Race>().is_none() {
            app.update();
        }
        skip_countdown_now(app.world_mut());
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<Race>().started());
        // The test drives, not the keyboard.
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
        let layout = app.world().resource::<TrackLayout>().clone();
        let course = app.world().resource::<Course>().clone();
        assert_eq!(layout.mirrored, mirrored);
        let (gap, lake) = (layout.gaps[0], layout.lakes[0]);
        Self {
            app,
            kart,
            layout,
            course,
            gap,
            lake,
        }
    }

    /// Puts the kart on the road at centerline distance `distance`, `lateral` meters right of
    /// the centerline, heading down the track at `speed`, its race progress there.
    fn place(&mut self, distance: f32, lateral: f32, speed: f32) {
        let sample = self.layout.center.at(distance);
        let transform = sample.transform(lateral, crate::track::GRID_HEIGHT);
        let spacing = self.course.checkpoints[1];
        let passed = 1 + (distance / spacing).floor() as u32;
        let world = self.app.world_mut();
        *world.get_mut::<Transform>(self.kart).unwrap() = transform;
        *world.get_mut::<Velocity>(self.kart).unwrap() = Velocity {
            linear: sample.forward * speed,
            angular: Vec3::ZERO,
        };
        world
            .get_mut::<PhysicsPose>(self.kart)
            .unwrap()
            .snap(transform);
        *world.get_mut::<KartState>(self.kart).unwrap() = KartState::Grounded;
        *world.get_mut::<DriftMotion>(self.kart).unwrap() = DriftMotion::default();
        *world.get_mut::<KartEnvironment>(self.kart).unwrap() = KartEnvironment::default();
        *world.get_mut::<Boost>(self.kart).unwrap() = Boost::default();
        let mut progress = world.get_mut::<RaceProgress>(self.kart).unwrap();
        progress.passed = passed;
        progress.distance = distance;
    }

    /// Issue #118: the boost of `pace`: none, or a purple mini-turbo's strength.
    fn boost_strength(&self, pace: Pace) -> f32 {
        match pace {
            Pace::Top => 0.0,
            Pace::Boosted => {
                self.app
                    .world()
                    .resource::<KartTuning>()
                    .mini_turbo_strengths[2]
            }
        }
    }

    /// The kart's top speed at `pace` (m/s).
    fn top_speed(&self, pace: Pace) -> f32 {
        self.app.world().resource::<KartTuning>().max_speed * (1.0 + self.boost_strength(pace))
    }

    /// The boost of `pace` for the rest of the run (a minute), once placed.
    fn boost(&mut self, pace: Pace) {
        let strength = self.boost_strength(pace);
        if strength > 0.0 {
            let world = self.app.world_mut();
            world
                .get_mut::<Boost>(self.kart)
                .unwrap()
                .add(strength, 60.0);
        }
    }

    fn environment(&self) -> KartEnvironment {
        *self.app.world().get::<KartEnvironment>(self.kart).unwrap()
    }

    fn transform(&self) -> Transform {
        *self.app.world().get::<Transform>(self.kart).unwrap()
    }

    fn velocity(&self) -> Vec3 {
        self.app.world().get::<Velocity>(self.kart).unwrap().linear
    }

    fn state(&self) -> KartState {
        *self.app.world().get::<KartState>(self.kart).unwrap()
    }

    /// How far right of the centerline the kart is (m).
    fn lateral(&self) -> f32 {
        let center = &self.layout.center;
        let position = self.transform().translation;
        let s = center.samples()[center.nearest_index(position)];
        (position - s.position).dot(s.right)
    }

    /// Centerline distance (lap 1) of where the kart is.
    fn distance(&self) -> f32 {
        let center = &self.layout.center;
        center.samples()[center.nearest_index(self.transform().translation)].distance
    }

    /// Height of the kart's center above the road under it (None over the gap).
    fn height(&self) -> Option<f32> {
        let center = &self.layout.center;
        let position = self.transform().translation;
        let s = center.samples()[center.nearest_index(position)];
        gap_at(&self.layout.gaps, center, s.distance)
            .is_none()
            .then(|| (position - s.position).dot(s.up))
    }

    /// One tick with `input`, its events in `log`.
    fn tick(&mut self, input: KartInput, log: &mut Log) {
        let before = self.environment();
        let was_drifting = matches!(self.state(), KartState::Drifting { .. });
        *self
            .app
            .world_mut()
            .get_mut::<KartInput>(self.kart)
            .unwrap() = input;
        self.app.update();
        log.ticks += 1;
        let after = self.environment();
        if after.gliding && !before.gliding {
            log.opened.push(log.ticks);
        }
        if !after.gliding && before.gliding {
            log.closed.push(log.ticks);
        }
        if after.underwater && !before.underwater {
            log.dived.push(log.ticks);
            log.drift_into_water |= was_drifting;
        }
        if !after.underwater && before.underwater {
            log.surfaced.push(log.ticks);
        }
        if before.underwater
            && after.underwater
            && log.dived.len() == 1
            && log.ticks == log.dived[0] + 1
        {
            log.drift_into_water &= matches!(self.state(), KartState::Drifting { .. });
        }
        let world = self.app.world();
        log.towed |= self.state() == KartState::Respawning;
        match self.height() {
            Some(height) => log.lowest = log.lowest.min(height),
            None => {
                let y = self.transform().translation.y - self.gap.low;
                log.highest_over_gap = log.highest_over_gap.max(y);
            }
        }
        let factors = *world.get::<MaxSpeedFactors>(self.kart).unwrap();
        let gravity = world.get::<GravityScale>(self.kart).unwrap().0;
        if after.gliding {
            log.air_stats = Some(factors.stats);
            log.glide_gravity = Some(gravity);
        }
        if after.underwater && before.underwater {
            log.water_factors = Some((factors.stats, factors.environment));
            log.water_gravity = Some(gravity);
            // Once the water slowed it down (1 s in).
            if log.ticks > log.dived[0] + 60 {
                let forward = self.velocity().dot(*self.transform().forward());
                log.fastest_under_water = log.fastest_under_water.max(forward);
            }
        }
    }

    /// The test autopilot along the centerline, at `speed` at most (lifting off above it).
    fn autopilot(&self, speed: f32) -> KartInput {
        let transform = self.transform();
        let forward = self.velocity().dot(*transform.forward());
        let mut input = follow_centerline(&self.layout, &transform, forward);
        if forward > speed {
            input.throttle = 0.0;
        }
        input
    }
}

fn new_log() -> Log {
    Log {
        lowest: f32::MAX,
        highest_over_gap: f32::MIN,
        ..default()
    }
}

/// How a scenario steers the glider.
#[derive(Clone, Copy, Debug)]
enum Pitch {
    /// Up held: it dives.
    Dive,
    /// Neither: a level glide.
    Level,
    /// Down held: it climbs.
    Climb,
}

/// Issue #20 acceptance (GD § 10): off the glide ramp at the edge of the cliff, the glider opens
/// (once, within a few ticks of the takeoff), the kart flies over the lagoon with the stats
/// « air » and without gravity (the glide sets its descent), and lands on the north beach, where
/// the glider closes (once); the pelican never comes, and the kart never goes through the road.
/// At three speeds (slow, medium, its top speed), and boosted all along ([`Pace`]), diving,
/// gliding level and climbing, and drifting into the ramp; on the mirror image too. Diving lands
/// sooner than gliding level, which lands sooner than climbing.
#[test]
fn the_glider_opens_over_the_lagoon_and_closes_on_the_beach() {
    let mut failures = Vec::new();
    for (pace, mirrored) in [
        (Pace::Top, false),
        (Pace::Boosted, false),
        (Pace::Top, true),
        (Pace::Boosted, true),
    ] {
        let mut lagoon = Lagoon::new(mirrored, None);
        let top = lagoon.top_speed(pace);
        for (speed, pitch, drift) in [
            (10.0, Pitch::Level, false),
            (20.0, Pitch::Dive, false),
            (top, Pitch::Dive, false),
            (top, Pitch::Level, false),
            (top, Pitch::Climb, false),
            (20.0, Pitch::Climb, false),
            (top, Pitch::Level, true),
        ] {
            let name = format!(
                "{pace:?}{} at {speed:.1} m/s, {pitch:?}{}",
                if mirrored { " (mirror)" } else { "" },
                if drift { ", drifting" } else { "" }
            );
            let (gap, lip) = (lagoon.gap, lagoon.gap.from);
            // The drift turns right whatever the stick: it starts from the left of the road.
            let lateral = if drift { -6.0 } else { 0.0 };
            lagoon.place(lip - 70.0, lateral, speed);
            lagoon.boost(pace);
            let mut log = new_log();
            let mut landed_at = None;
            let mut drifted = false;
            for _ in 0..(25 * 60) {
                let mut input = lagoon.autopilot(speed);
                let gliding = lagoon.environment().gliding;
                let to_lip = lip - lagoon.distance();
                if drift && to_lip >= 24.0 {
                    input.steer = 0.0;
                }
                if drift && !gliding && (4.0..24.0).contains(&to_lip) {
                    // A hop steered right 24 m before the edge, the button held, steering out: a
                    // wide drift onto the ramp, held off it.
                    let drifting = matches!(lagoon.state(), KartState::Drifting { .. });
                    input = KartInput {
                        throttle: 1.0,
                        steer: if drifting { -1.0 } else { 0.6 },
                        jump_drift: true,
                        ..default()
                    };
                    drifted |= drifting;
                }
                if gliding {
                    (input.throttle, input.brake) = match pitch {
                        Pitch::Dive => (1.0, 0.0),
                        Pitch::Level => (0.0, 0.0),
                        Pitch::Climb => (0.0, 1.0),
                    };
                }
                lagoon.tick(input, &mut log);
                if !log.closed.is_empty() && landed_at.is_none() {
                    landed_at = Some(lagoon.distance());
                }
                if landed_at.is_some() && log.ticks > log.closed[0] + 60 {
                    break;
                }
            }
            let landed = landed_at.unwrap_or(f32::NAN);
            let center = &lagoon.layout.center;
            let wheels = *lagoon.app.world().get::<KartWheels>(lagoon.kart).unwrap();
            let checks = [
                (log.opened.len() == 1, "the glider opens once"),
                (log.closed.len() == 1, "and closes once"),
                (
                    center.wrap(landed - gap.to) < 200.0 && center.wrap(landed - gap.to) > 0.0,
                    "it lands on the beach, after the gap",
                ),
                (!log.towed, "no pelican"),
                (log.lowest > 0.2, "never through the road"),
                (
                    wheels.wheels_on_ground() >= 3 && lagoon.transform().up().y > 0.95,
                    "on its wheels after the landing",
                ),
                (log.glide_gravity == Some(0.0), "no gravity in the glide"),
                (
                    log.air_stats == Some(1.0),
                    "the stats « air » (the middle level: 1)",
                ),
                (log.highest_over_gap > 0.0, "flew over the gap"),
                (!drift || drifted, "drifted into the ramp"),
            ];
            for (ok, what) in checks {
                if !ok {
                    failures.push(format!(
                        "{name}: {what} ({log:?}, landed at {landed}, gap {gap:?})"
                    ));
                }
            }
            println!(
                "{name}: opened at {:?}, closed at {:?}, landed {:.0} m after the gap, lowest {:.2} m",
                log.opened,
                log.closed,
                center.wrap(landed - gap.to),
                log.lowest
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #20 acceptance (GD § 10): down into the lagoon, the kart goes under the water (once)
/// and drives with the stats « eau » and the water's physics (slower: its top speed × 0.9; half
/// the gravity), on the road at the bottom; it comes out on the other side (once) and drives on
/// as before (on the ground: exactly the factors it was spawned with). The pelican never comes,
/// the kart never goes through the road. At three speeds (slow, medium, its top speed), and
/// boosted all along ([`Pace`]), and drifting into the water; on the mirror image too.
#[test]
fn the_kart_dives_into_the_lagoon_and_comes_out_of_it() {
    let mut failures = Vec::new();
    for (pace, mirrored) in [
        (Pace::Top, false),
        (Pace::Boosted, false),
        (Pace::Top, true),
        (Pace::Boosted, true),
    ] {
        let mut lagoon = Lagoon::new(mirrored, None);
        let top = lagoon.top_speed(pace);
        let world = lagoon.app.world();
        let water_top = world.resource::<KartTuning>().environment.water_top_speed;
        let gravity = world.resource::<KartTuning>().gravity_scale
            * world.resource::<KartTuning>().environment.water_gravity;
        // Where a kart goes under the water: the first sample of the lap where its center would
        // be under the surface by more than the margin.
        let center = lagoon.layout.center.clone();
        let margin = world.resource::<KartTuning>().environment.water_margin;
        let entry = center
            .samples()
            .iter()
            .find(|s| {
                let kart = s.position + s.up * crate::track::GRID_HEIGHT;
                lagoon.lake.depth(kart).is_some_and(|d| d > margin)
            })
            .unwrap()
            .distance;
        for (speed, drift) in [(10.0, false), (20.0, false), (top, false), (top, true)] {
            let name = format!(
                "{pace:?}{} at {speed:.1} m/s{}",
                if mirrored { " (mirror)" } else { "" },
                if drift { ", drifting" } else { "" }
            );
            // The drift turns right whatever the stick: it starts from the left of the road.
            let lateral = if drift { -6.0 } else { 0.0 };
            lagoon.place(entry - 60.0, lateral, speed);
            lagoon.boost(pace);
            let mut log = new_log();
            let mut out_at = None;
            for _ in 0..(20 * 60) {
                let mut input = lagoon.autopilot(speed);
                let to_water = center.wrap(entry - lagoon.distance());
                if drift && log.dived.is_empty() && to_water >= 25.0 {
                    input.steer = 0.0;
                }
                // The drift lasts until a sixth of a second under the water.
                let holding = log.dived.first().is_none_or(|at| log.ticks < at + 10);
                if drift && holding && (to_water < 25.0 || !log.dived.is_empty()) {
                    // A hop steered right then a wide drift held into the water, steering out.
                    let drifting = matches!(lagoon.state(), KartState::Drifting { .. });
                    input = KartInput {
                        throttle: 1.0,
                        steer: if drifting { -1.0 } else { 0.6 },
                        jump_drift: true,
                        ..default()
                    };
                }
                lagoon.tick(input, &mut log);
                if !log.surfaced.is_empty() && out_at.is_none() {
                    out_at = Some(log.ticks);
                }
                if out_at.is_some_and(|t| log.ticks > t + 90) {
                    break;
                }
            }
            let factors = *lagoon
                .app
                .world()
                .get::<MaxSpeedFactors>(lagoon.kart)
                .unwrap();
            let wheels = *lagoon.app.world().get::<KartWheels>(lagoon.kart).unwrap();
            let effects = *lagoon.app.world().get::<StatEffects>(lagoon.kart).unwrap();
            let checks = [
                (log.dived.len() == 1, "under water once"),
                (log.surfaced.len() == 1, "out once"),
                (!log.towed, "no pelican"),
                (log.lowest > 0.2, "never through the road"),
                (
                    log.water_factors == Some((1.0, water_top)),
                    "the stats « eau » and the water's slowness",
                ),
                (
                    log.water_gravity
                        .is_some_and(|g| (g - gravity).abs() < 1e-5),
                    "half the gravity",
                ),
                (
                    speed < 15.0 || log.fastest_under_water < top * water_top + 1.0,
                    "slower under water",
                ),
                (
                    factors.environment == 1.0 && factors.stats == effects.top_speed,
                    "back on the ground: the factors it was spawned with",
                ),
                (wheels.wheels_on_ground() >= 3, "on its wheels"),
                (
                    !drift || log.drift_into_water,
                    "drifting into the water, and on under it",
                ),
            ];
            for (ok, what) in checks {
                if !ok {
                    failures.push(format!("{name}: {what} ({log:?})"));
                }
            }
            println!(
                "{name}: under water at {:?}, out at {:?}, fastest under water {:.1} m/s, lowest {:.2} m",
                log.dived, log.surfaced, log.fastest_under_water, log.lowest
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #20: under water, a hop lasts longer (half the gravity, the water's drag) and still
/// lands in a drift; the stats « eau » of a kart made for the water (the bathtub) make it faster
/// there than its Speed on the ground; out of the water, the Speed it was spawned with.
#[test]
fn under_water_a_hop_floats_and_the_bathtub_swims() {
    let roster = Roster::embedded();
    let mut bathtub = roster.default_loadout();
    bathtub.set(
        PartKind::Kart,
        roster.find(PartKind::Kart, "Baignoire").unwrap(),
    );
    let mut lagoon = Lagoon::new(false, None);
    let center = lagoon.layout.center.clone();
    // The bottom of the lagoon: the deepest sample of the road.
    let bottom = center
        .samples()
        .iter()
        .filter(|s| gap_at(&lagoon.layout.gaps, &center, s.distance).is_none())
        .min_by(|a, b| a.position.y.total_cmp(&b.position.y))
        .unwrap()
        .distance;
    let hop_ticks = |lagoon: &mut Lagoon, at: f32| {
        lagoon.place(at, 0.0, 20.0);
        let mut log = new_log();
        for _ in 0..20 {
            let input = lagoon.autopilot(20.0);
            lagoon.tick(input, &mut log);
        }
        let mut ticks = 0;
        let hop = |steer| KartInput {
            throttle: 1.0,
            steer,
            jump_drift: true,
            ..default()
        };
        lagoon.tick(hop(0.6), &mut log);
        while matches!(lagoon.state(), KartState::Hop { .. }) {
            lagoon.tick(hop(0.2), &mut log);
            ticks += 1;
            assert!(ticks < 120);
        }
        (ticks, lagoon.state(), lagoon.environment().underwater)
    };
    let (dry, dry_state, wet_before) = hop_ticks(&mut lagoon, 20.0);
    assert!(!wet_before);
    let (wet, wet_state, under) = hop_ticks(&mut lagoon, bottom - 5.0);
    assert!(under, "the hop was under water");
    assert!(
        matches!(dry_state, KartState::Drifting { .. }),
        "{dry_state:?}"
    );
    assert!(
        matches!(wet_state, KartState::Drifting { .. }),
        "under water too: {wet_state:?}"
    );
    assert!(
        wet as f32 > dry as f32 * 1.3,
        "a floaty hop: {wet} ticks under water, {dry} on the ground"
    );

    // The bathtub's Speed « eau » (its stats there): faster than the default kart under water,
    // its Speed of the ground back out of it.
    let factors = |loadout: Option<Loadout>| {
        let mut lagoon = Lagoon::new(false, loadout);
        lagoon.place(bottom - 30.0, 0.0, 15.0);
        let mut log = new_log();
        for _ in 0..30 {
            let input = lagoon.autopilot(f32::MAX);
            lagoon.tick(input, &mut log);
            assert!(lagoon.environment().underwater);
        }
        let world = lagoon.app.world();
        let wet = *world.get::<MaxSpeedFactors>(lagoon.kart).unwrap();
        let effects = *world.get::<StatEffects>(lagoon.kart).unwrap();
        lagoon.place(20.0, 0.0, 15.0);
        lagoon.tick(KartInput::default(), &mut log);
        let dry = *lagoon
            .app
            .world()
            .get::<MaxSpeedFactors>(lagoon.kart)
            .unwrap();
        (wet, dry, effects)
    };
    let water = EnvironmentTuning::default().water_top_speed;
    let (wet, dry, effects) = factors(None);
    assert_eq!((wet.stats, wet.environment), (1.0, water));
    assert_eq!((dry.stats, dry.environment), (1.0, 1.0));
    assert_eq!(effects.on(Terrain::Water), effects);
    let (wet, dry, effects) = factors(Some(bathtub));
    let swims = effects.on(Terrain::Water).top_speed;
    assert!(swims > effects.top_speed, "{effects:?}");
    assert_eq!((wet.stats, wet.environment), (swims, water));
    assert_eq!((dry.stats, dry.environment), (effects.top_speed, 1.0));
}

/// Issue #20: a kart under its glider flying beyond the walls (over the beach, the lagoon) is not
/// out of the track while it flies: the pelican does not come. Once it lands there, it is (after
/// `RaceTuning::outside_seconds`, as always).
#[test]
fn a_glider_beyond_the_walls_is_out_of_the_track_only_once_landed() {
    let mut lagoon = Lagoon::new(false, None);
    let tuning = lagoon
        .app
        .world()
        .resource::<crate::race::RaceTuning>()
        .clone();
    // Over the landing straight, high above the beach beyond its left wall, gliding along it.
    let at = lagoon.gap.to + 20.0;
    let sample = lagoon.layout.center.at(at);
    let outside = crate::track::mesh::wall_offset(&sample)
        + crate::track::mesh::WALL_THICKNESS
        + tuning.outside_margin;
    lagoon.place(at, -(outside + 6.0), 25.0);
    {
        let world = lagoon.app.world_mut();
        world
            .get_mut::<Transform>(lagoon.kart)
            .unwrap()
            .translation
            .y += 12.0;
        let transform = *world.get::<Transform>(lagoon.kart).unwrap();
        world
            .get_mut::<PhysicsPose>(lagoon.kart)
            .unwrap()
            .snap(transform);
        *world.get_mut::<KartEnvironment>(lagoon.kart).unwrap() = KartEnvironment {
            gliding: true,
            since_glide_ramp: 0,
            ..default()
        };
    }
    let mut log = new_log();
    let mut flying = 0;
    while lagoon.environment().gliding {
        assert!(
            lagoon.lateral() < -outside,
            "still beyond the wall: {}",
            lagoon.lateral()
        );
        lagoon.tick(KartInput::default(), &mut log);
        assert!(!log.towed, "towed while flying, after {flying} ticks");
        flying += 1;
        assert!(flying < 20 * 60, "it never lands");
    }
    assert!(
        flying as f32 > 2.0 * tuning.outside_seconds * 60.0,
        "{flying} ticks in the air"
    );
    // Landed beyond the wall: out of the track.
    let mut ticks = 0;
    while !log.towed {
        lagoon.tick(KartInput::default(), &mut log);
        ticks += 1;
        assert!(ticks < 3 * 60, "landed out of the track, never towed");
    }
    assert!(
        ticks as f32 >= tuning.outside_seconds * 60.0 - 2.0,
        "{ticks}"
    );
}

/// Issue #20: a glider does not spin. A bump in the air (the takeoff in a pack: another glider
/// right beside it) kicks its yaw; the glider turns no faster than its own full turn, so the
/// kart stays near its line and still lands on the beach, between the walls. Before, a kick of
/// 6 rad/s faded at the glide's response only: the bots it threw off flew over the walls of the
/// landing or into the lagoon (seen in the bot races of other seeds).
#[test]
fn a_bump_in_the_air_does_not_spin_the_glider() {
    for kick in [6.0f32, -6.0] {
        let mut lagoon = Lagoon::new(false, None);
        let full_turn = glide_yaw_target(&KartTuning::default().environment, 1.0, 1.0).abs();
        // Gliding just past the lip, on the line, 12 m up, kicked.
        lagoon.place(lagoon.gap.from + 8.0, 0.0, 26.0);
        {
            let world = lagoon.app.world_mut();
            world
                .get_mut::<Transform>(lagoon.kart)
                .unwrap()
                .translation
                .y += 12.0;
            let transform = *world.get::<Transform>(lagoon.kart).unwrap();
            world
                .get_mut::<PhysicsPose>(lagoon.kart)
                .unwrap()
                .snap(transform);
            world.get_mut::<Velocity>(lagoon.kart).unwrap().angular = Vec3::Y * kick;
            *world.get_mut::<KartEnvironment>(lagoon.kart).unwrap() = KartEnvironment {
                gliding: true,
                since_glide_ramp: 0,
                ..default()
            };
        }
        let heading = |lagoon: &Lagoon| {
            let forward = lagoon.transform().forward();
            forward.x.atan2(-forward.z)
        };
        let start = heading(&lagoon);
        let mut log = new_log();
        let (mut ticks, mut widest) = (0, 0.0f32);
        while lagoon.environment().gliding {
            let input = lagoon.autopilot(40.0);
            lagoon.tick(input, &mut log);
            ticks += 1;
            let yaw = lagoon
                .app
                .world()
                .get::<Velocity>(lagoon.kart)
                .unwrap()
                .angular
                .y;
            if lagoon.environment().gliding {
                assert!(
                    yaw.abs() <= full_turn + 1e-3,
                    "kick {kick}: yaw {yaw} after {ticks} ticks"
                );
            }
            widest = widest.max((heading(&lagoon) - start).abs());
            assert!(ticks < 20 * 60, "it never lands");
        }
        assert!(
            widest < 0.35,
            "kick {kick}: turned {widest} rad off its line"
        );
        let lateral = lagoon.lateral();
        let wall = crate::track::mesh::wall_offset(&lagoon.layout.center.at(lagoon.distance()));
        assert!(
            lateral.abs() < wall - 1.0,
            "kick {kick}: landed {lateral} m from the line (walls at {wall} m)"
        );
        assert!(
            lagoon.distance() > lagoon.gap.to,
            "kick {kick}: landed before the beach"
        );
        // And drives on without the pelican.
        for _ in 0..120 {
            let input = lagoon.autopilot(40.0);
            lagoon.tick(input, &mut log);
        }
        assert!(!log.towed, "kick {kick}: towed");
    }
}
