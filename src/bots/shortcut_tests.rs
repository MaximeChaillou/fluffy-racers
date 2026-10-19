//! Issue #107 acceptance: headless scenarios of the shortcuts on the Corniche Cannelle and its
//! mirror (the whole game, `GamePlugin` with `MinimalPlugins`, the player's kart alone, its race
//! started, one fixed tick per `app.update()`): a risky jump made saves time, one too slow falls
//! and the pelican drops the kart before it; the grass bridge never calls the pelican, slows a
//! kart without a boost down to the off-road's top speed and pays with one; the hidden road saves
//! time; the bots take them as their level says, and the grass only with a boost.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::admin::skip_countdown_now;
use crate::interpolation::PhysicsPose;
use crate::kart::{
    Boost, DriftMotion, KartEnvironment, KartInput, KartState, KartTuning, PlayerKart,
};
use crate::race::{Course, Race, RaceProgress, Tow, TowReason};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};
use crate::track::{GRID_HEIGHT, MirrorMode, Shortcut, ShortcutKind, TrackChoice, TrackLayout};

const CORNICHE: &str = "tracks/corniche.track.ron";

/// The Corniche Cannelle, the player's kart alone on it, its race started.
struct Canyon {
    app: App,
    kart: Entity,
    layout: TrackLayout,
    course: Course,
}

/// What a run saw.
#[derive(Clone, Debug, Default)]
struct Run {
    /// Ticks to reach the goal (`None`: never in time).
    ticks: Option<u32>,
    tows: Vec<TowReason>,
    /// The kinds of shortcut the kart was counted on (`Course::project`), in order.
    on: Vec<ShortcutKind>,
    /// Fastest forward speed on a grass path without a boost (m/s), and ticks spent there.
    grass_speed: f32,
    grass_ticks: u32,
    /// Its race distance right after the pelican dropped it.
    dropped_at: Option<f32>,
}

impl Canyon {
    fn new(mirrored: bool) -> Self {
        let mut app = game_app_unloaded(TrackChoice::Circuit(CORNICHE.into()), 0);
        app.insert_resource(MirrorMode(mirrored));
        single_threaded(&mut app);
        wait_for_the_player(&mut app);
        let kart = player(&mut app);
        for tries in 0.. {
            if app.world().get_resource::<Race>().is_some() {
                break;
            }
            assert!(
                tries < 100,
                "the Corniche Cannelle never came (its file refused?)"
            );
            app.update();
        }
        skip_countdown_now(app.world_mut());
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<Race>().started());
        // The test (or a bot) drives, not the keyboard.
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
        let layout = app.world().resource::<TrackLayout>().clone();
        let course = app.world().resource::<Course>().clone();
        assert_eq!(layout.mirrored, mirrored);
        assert_eq!(layout.name, "Corniche Cannelle");
        Self {
            app,
            kart,
            layout,
            course,
        }
    }

    /// The circuit's shortcut of `kind` (one of each).
    fn shortcut(&self, kind: ShortcutKind) -> Shortcut {
        self.layout
            .shortcuts
            .iter()
            .find(|s| s.kind == kind)
            .unwrap()
            .clone()
    }

    /// The kart becomes a Hard bot that drives with `skill`.
    fn becomes_a_bot(&mut self, skill: BotSkill) {
        let tuning = self.app.world().resource::<BotTuning>().clone();
        self.app
            .world_mut()
            .entity_mut(self.kart)
            .insert((bot(BotDifficulty::Hard, 7, &tuning), OwnSkill(skill)));
    }

    /// A Hard bot's skill that takes shortcuts of `kind` every time, and no other.
    fn skill_for(&self, kind: Option<ShortcutKind>) -> BotSkill {
        let tuning = self.app.world().resource::<BotTuning>();
        let mut skill = *tuning.skill(BotDifficulty::Hard);
        skill.shortcut_chance = f32::from(kind == Some(ShortcutKind::Road));
        skill.grass_chance = f32::from(kind == Some(ShortcutKind::Grass));
        skill.jump_chance = f32::from(kind == Some(ShortcutKind::Jump));
        skill
    }

    /// Puts the kart at `position` heading along `forward` at `speed`, its race progress at
    /// centerline distance `distance` (the first lap).
    fn place_at(&mut self, position: Vec3, forward: Vec3, speed: f32, distance: f32) {
        let transform = Transform::from_translation(position).looking_to(forward, Vec3::Y);
        let spacing = self.course.checkpoints[1];
        let passed = 1 + (distance / spacing).floor() as u32;
        let world = self.app.world_mut();
        *world.get_mut::<Transform>(self.kart).unwrap() = transform;
        *world.get_mut::<Velocity>(self.kart).unwrap() = Velocity {
            linear: forward * speed,
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
        let position = transform.translation;
        world.get_mut::<Tow>(self.kart).unwrap().reset(position);
    }

    /// Puts the kart on the road at centerline distance `distance`, on the centerline, heading
    /// down the track at `speed`.
    fn place(&mut self, distance: f32, speed: f32) {
        let sample = self.layout.center.at(distance);
        let position = sample.point(0.0, GRID_HEIGHT);
        self.place_at(position, sample.forward, speed, distance);
    }

    /// A boost of a purple mini-turbo's strength for `seconds`.
    fn boost(&mut self, seconds: f32) {
        let strength = self
            .app
            .world()
            .resource::<KartTuning>()
            .mini_turbo_strengths[2];
        self.app
            .world_mut()
            .get_mut::<Boost>(self.kart)
            .unwrap()
            .add(strength, seconds);
    }

    /// Runs until the kart's race distance reaches `goal` (or `seconds` went by); `input` drives
    /// it when it is not a bot (`None`: its bot drives).
    fn run(&mut self, goal: f32, seconds: f32, input: Option<&dyn Fn(&App) -> KartInput>) -> Run {
        let mut run = Run::default();
        let max_speed = self.app.world().resource::<KartTuning>().max_speed;
        let mut was_carried = false;
        for tick in 1..=(seconds * 60.0) as u32 {
            if let Some(input) = input {
                let wanted = input(&self.app);
                *self
                    .app
                    .world_mut()
                    .get_mut::<KartInput>(self.kart)
                    .unwrap() = wanted;
            }
            self.app.update();
            let world = self.app.world();
            let transform = world.get::<Transform>(self.kart).unwrap();
            let progress = world.get::<RaceProgress>(self.kart).unwrap();
            let tow = world.get::<Tow>(self.kart).unwrap();
            let carried = tow.carry.is_some();
            if let Some(carry) = tow.carry
                && !was_carried
            {
                run.tows.push(carry.reason);
            }
            if was_carried && !carried {
                run.dropped_at = Some(progress.distance);
            }
            was_carried = carried;
            let projection = self.course.project(progress.passed, transform.translation);
            if let Some(k) = projection.shortcut {
                let kind = self.course.shortcuts[k].kind;
                if run.on.last() != Some(&kind) {
                    run.on.push(kind);
                }
                let boost = world.get::<Boost>(self.kart).unwrap();
                if kind == ShortcutKind::Grass && boost.remaining <= 0.0 {
                    let velocity = world.get::<Velocity>(self.kart).unwrap().linear;
                    run.grass_speed = run.grass_speed.max(velocity.dot(*transform.forward()));
                    run.grass_ticks += 1;
                }
            }
            assert!(run.grass_speed <= max_speed * 1.5);
            if progress.distance >= goal && !carried {
                run.ticks = Some(tick);
                break;
            }
        }
        run
    }
}

/// A driver that follows a shortcut's path, flat out: it steers to its path 10 m ahead, then on
/// along the centerline past its end.
fn follow(path: &Shortcut, layout: &TrackLayout, app: &App, kart: Entity) -> KartInput {
    let transform = app.world().get::<Transform>(kart).unwrap();
    let speed = app.world().get::<Velocity>(kart).unwrap().linear.length();
    let here = path.locate(transform.translation);
    let ahead = here.along.max(0.0) + 10.0;
    let target = if ahead <= path.length() {
        path.at(ahead).position
    } else {
        layout
            .center
            .at(path.from + path.span + ahead - path.length())
            .position
    };
    let kart_tuning = app.world().resource::<KartTuning>();
    let (angle, yaw) = driver::pursuit(
        transform.translation,
        *transform.forward(),
        *transform.right(),
        target,
        speed,
    );
    KartInput {
        throttle: 1.0,
        steer: driver::steer_for_yaw(kart_tuning, yaw, speed, angle),
        ..default()
    }
}

/// Issue #107 acceptance (« un saut risqué réussi fait gagner du temps, raté il fait tomber et
/// venir le pélican »), on the Corniche Cannelle and its mirror: a Hard bot that goes for the
/// risky jump from 110 m before it, flat out, makes it (counted on the jump, never towed) and
/// reaches 40 m past its landing at least 0.25 s before the same bot by the road; a kart lined up
/// at the foot of the jump's ramp at 13 m/s, flat out, falls short: the pelican comes (it fell) and
/// drops it before the jump.
#[test]
fn a_risky_jump_made_saves_time_and_one_too_slow_falls_to_the_pelican() {
    for mirrored in [false, true] {
        let mut times = Vec::new();
        for kind in [Some(ShortcutKind::Jump), None] {
            let mut canyon = Canyon::new(mirrored);
            let jump = canyon.shortcut(ShortcutKind::Jump);
            let skill = canyon.skill_for(kind);
            canyon.becomes_a_bot(skill);
            canyon.place(jump.from - 110.0, 26.0);
            let run = canyon.run(jump.from + jump.span + 40.0, 20.0, None);
            assert!(run.tows.is_empty(), "{kind:?} (mirror {mirrored}): {run:?}");
            let jumped = run.on.contains(&ShortcutKind::Jump);
            assert_eq!(
                jumped,
                kind.is_some(),
                "{kind:?} (mirror {mirrored}): {run:?}"
            );
            times.push(run.ticks.expect("the goal is reached") as f32 / 60.0);
        }
        println!(
            "mirror {mirrored}: the jump in {:.2} s, the road in {:.2} s",
            times[0], times[1]
        );
        assert!(
            times[0] + 0.25 <= times[1],
            "mirror {mirrored}: the jump in {:.2} s, the road in {:.2} s",
            times[0],
            times[1]
        );

        // Too slow: it falls into the drop, the pelican drops it before the jump.
        let mut canyon = Canyon::new(mirrored);
        let jump = canyon.shortcut(ShortcutKind::Jump);
        let ramp = jump.ramp.unwrap();
        let start = ramp.start - ramp.direction + Vec3::Y * GRID_HEIGHT;
        canyon.place_at(start, ramp.direction, 13.0, jump.from);
        let straight = |_: &App| KartInput {
            throttle: 1.0,
            ..default()
        };
        let run = canyon.run(jump.from + jump.span + 40.0, 8.0, Some(&straight));
        assert_eq!(
            run.tows.first(),
            Some(&TowReason::Fell),
            "mirror {mirrored}: {run:?}"
        );
        assert!(run.on.contains(&ShortcutKind::Jump), "{run:?}");
        let dropped = run.dropped_at.expect("dropped");
        assert!(
            dropped <= jump.from,
            "mirror {mirrored}: dropped at {dropped:.0} m, the jump from {:.0} m",
            jump.from
        );
    }
}

/// Issue #107 acceptance (« l'herbe autorisée ne déclenche pas le pélican et ralentit sans
/// boost »), on the Corniche Cannelle and its mirror: a kart that drives the grass bridge flat
/// out without a boost is never towed and goes no faster there than the off-road's top speed
/// (GD § 2: 55 %); with a boost (2 s, 20 m before it: about a mini-turbo), a Hard bot takes it
/// (it only does with one) and reaches 20 m past its end at least 0.5 s before the same bot with
/// the same boost by the road; without a boost, the bot keeps to the road, and is there at
/// least 0.3 s before a kart that drives the grass from the same place, flat out.
#[test]
fn the_grass_bridge_never_calls_the_pelican_slows_without_a_boost_and_pays_with_one() {
    for mirrored in [false, true] {
        let mut canyon = Canyon::new(mirrored);
        let grass = canyon.shortcut(ShortcutKind::Grass);
        let tuning = canyon.app.world().resource::<KartTuning>().clone();
        let start = grass.samples()[0];
        canyon.place_at(
            start.position + Vec3::Y * GRID_HEIGHT,
            start.forward,
            12.0,
            grass.from,
        );
        let (path, layout, kart) = (grass.clone(), canyon.layout.clone(), canyon.kart);
        let driver = move |app: &App| follow(&path, &layout, app, kart);
        let run = canyon.run(grass.from + grass.span + 20.0, 15.0, Some(&driver));
        assert!(run.tows.is_empty(), "mirror {mirrored}: {run:?}");
        assert!(run.ticks.is_some(), "{run:?}");
        assert!(run.grass_ticks >= 60, "{run:?}");
        let cap = tuning.max_speed * tuning.offroad_speed_factor;
        assert!(
            run.grass_speed <= cap * 1.05,
            "mirror {mirrored}: {:.1} m/s on the grass, the off-road's top speed is {cap:.1}",
            run.grass_speed
        );

        let mut times = Vec::new();
        for kind in [Some(ShortcutKind::Grass), None] {
            let mut canyon = Canyon::new(mirrored);
            let skill = canyon.skill_for(kind);
            canyon.becomes_a_bot(skill);
            canyon.place(grass.from - 20.0, 26.0);
            canyon.boost(2.0);
            let run = canyon.run(grass.from + grass.span + 20.0, 15.0, None);
            assert!(run.tows.is_empty(), "{kind:?} (mirror {mirrored}): {run:?}");
            assert_eq!(
                run.on.contains(&ShortcutKind::Grass),
                kind.is_some(),
                "{kind:?} (mirror {mirrored}): {run:?}"
            );
            times.push(run.ticks.expect("the goal is reached") as f32 / 60.0);
        }
        println!(
            "mirror {mirrored}: boosted, the grass in {:.2} s, the road in {:.2} s",
            times[0], times[1]
        );
        assert!(
            times[0] + 0.5 <= times[1],
            "mirror {mirrored}: boosted, the grass in {:.2} s, the road in {:.2} s",
            times[0],
            times[1]
        );

        // No boost: the bot that would take the grass keeps to the road, and it is right: the
        // grass is slower.
        let mut canyon = Canyon::new(mirrored);
        let skill = canyon.skill_for(Some(ShortcutKind::Grass));
        canyon.becomes_a_bot(skill);
        canyon.place(grass.from - 20.0, 26.0);
        let run = canyon.run(grass.from + grass.span + 20.0, 15.0, None);
        assert!(run.tows.is_empty() && run.on.is_empty(), "{run:?}");
        let road = run.ticks.expect("the goal is reached") as f32 / 60.0;
        let mut canyon = Canyon::new(mirrored);
        canyon.place(grass.from - 20.0, 26.0);
        let (path, layout, kart) = (grass.clone(), canyon.layout.clone(), canyon.kart);
        let driver = move |app: &App| follow(&path, &layout, app, kart);
        let run = canyon.run(grass.from + grass.span + 20.0, 15.0, Some(&driver));
        assert!(
            run.tows.is_empty() && run.on == [ShortcutKind::Grass],
            "{run:?}"
        );
        let on_grass = run.ticks.expect("the goal is reached") as f32 / 60.0;
        println!(
            "mirror {mirrored}: no boost, the grass in {on_grass:.2} s, the road in {road:.2} s"
        );
        assert!(
            road + 0.3 <= on_grass,
            "mirror {mirrored}: no boost, the grass in {on_grass:.2} s, the road in {road:.2} s"
        );
    }
}

/// Issue #107: the hidden road saves time, and counts right: a Hard bot that takes it reaches
/// 30 m past its end before the same bot by the road, never towed, with the same checkpoints
/// crossed. Mirrored too.
#[test]
fn the_hidden_road_saves_time_and_counts_right() {
    for mirrored in [false, true] {
        let mut runs = Vec::new();
        for kind in [Some(ShortcutKind::Road), None] {
            let mut canyon = Canyon::new(mirrored);
            let road = canyon.shortcut(ShortcutKind::Road);
            let skill = canyon.skill_for(kind);
            canyon.becomes_a_bot(skill);
            canyon.place(road.from - 60.0, 24.0);
            let goal = road.from + road.span + 30.0;
            let run = canyon.run(goal, 15.0, None);
            assert!(run.tows.is_empty(), "{kind:?} (mirror {mirrored}): {run:?}");
            assert_eq!(
                run.on.contains(&ShortcutKind::Road),
                kind.is_some(),
                "{kind:?} (mirror {mirrored}): {run:?}"
            );
            let passed = canyon
                .app
                .world()
                .get::<RaceProgress>(canyon.kart)
                .unwrap()
                .passed;
            runs.push((
                run.ticks.expect("the goal is reached") as f32 / 60.0,
                passed,
            ));
        }
        println!(
            "mirror {mirrored}: hidden road {:?}, road {:?}",
            runs[0], runs[1]
        );
        assert!(runs[0].0 + 0.4 <= runs[1].0, "mirror {mirrored}: {runs:?}");
        assert_eq!(runs[0].1, runs[1].1, "the same checkpoints crossed");
    }
}
