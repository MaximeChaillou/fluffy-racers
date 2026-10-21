//! Issue #107 acceptance: headless scenarios of the shortcuts on the Corniche Cannelle and its
//! mirror (the whole game, `GamePlugin` with `MinimalPlugins`, the player's kart alone, its race
//! started, one fixed tick per `app.update()`): a risky jump made saves time, one too slow falls
//! and the pelican drops the kart before it; the grass bridge never calls the pelican, slows a
//! kart without a boost down to the off-road's top speed and pays with one; the hidden road saves
//! time; the bots take them as their level says, and the grass only with a boost. Issue #108: the
//! same on Prairie Pompon redrawn after the guide ([`CIRCUITS`]).

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

/// A circuit of the guide with a shortcut of each kind (issue #107: the Corniche Cannelle; issue
/// #108: Prairie Pompon redrawn; issue #109: the Col des Moufles redrawn; issue #119: the
/// Échappée Pissenlit, the first long race), and where the tests
/// start their bot before each shortcut (m before its start along the centerline: on the road, at
/// speed, after the turn before it).
#[derive(Clone, Copy, Debug)]
struct Circuit {
    path: &'static str,
    name: &'static str,
    jump_approach: f32,
    grass_approach: f32,
    road_approach: f32,
}

const CIRCUITS: [Circuit; 6] = [
    Circuit {
        path: "tracks/corniche.track.ron",
        name: "Corniche Cannelle",
        jump_approach: 110.0,
        grass_approach: 20.0,
        road_approach: 60.0,
    },
    // The jump from the tight left turn before it (on top of the Butte), the grass from out of
    // the last hairpin, the hidden road from the jump's landing.
    Circuit {
        path: "tracks/prairie.track.ron",
        name: "Prairie Pompon",
        jump_approach: 25.0,
        grass_approach: 20.0,
        road_approach: 20.0,
    },
    // The jump from the switchback before it (on the pass), the snow path from out of the last
    // switchback, the hidden road from the tight left turn after the summit.
    Circuit {
        path: "tracks/col.track.ron",
        name: "Col des Moufles",
        jump_approach: 25.0,
        grass_approach: 20.0,
        road_approach: 20.0,
    },
    // Issue #119, the first long race (its shortcuts are the Corniche Cannelle's pieces): the jump
    // from out of the tight corner before the cliff's edge, the grass bridge from out of the
    // bulge, the hidden road from the climb before it.
    Circuit {
        path: "tracks/echappee.track.ron",
        name: "Échappée Pissenlit",
        jump_approach: 60.0,
        grass_approach: 20.0,
        road_approach: 60.0,
    },
    // Issue #110, the Forêt Frimousse (the Corniche Cannelle's pieces, other covers): the jump
    // from the straight along the crest, the grass bridge from out of the bulge, the hidden road
    // from out of the hairpin before its climb.
    Circuit {
        path: "tracks/foret.track.ron",
        name: "Forêt Frimousse",
        jump_approach: 60.0,
        grass_approach: 20.0,
        road_approach: 60.0,
    },
    // Issue #110, the Falaises Flanelle (the Corniche Cannelle's pieces mirrored, other covers):
    // the jump from out of the hairpin before the cliff's edge, the grass bridge from out of the
    // bulge, the hidden road from the straight along the foot of the cliff.
    Circuit {
        path: "tracks/falaises.track.ron",
        name: "Falaises Flanelle",
        jump_approach: 45.0,
        grass_approach: 20.0,
        road_approach: 60.0,
    },
];

/// A circuit of [`CIRCUITS`], the player's kart alone on it, its race started.
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
    fn new(circuit: Circuit, mirrored: bool) -> Self {
        let mut app = game_app_unloaded(TrackChoice::Circuit(circuit.path.into()), 0);
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
                "{} never came (its file refused?)",
                circuit.name
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
        assert_eq!(layout.name, circuit.name);
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
/// venir le pélican »), on the Corniche Cannelle and its mirror (issue #108: and Prairie Pompon):
/// a Hard bot that goes for the risky jump from before it (110 m on the Corniche), flat out,
/// makes it (counted on the jump, never towed) and reaches 40 m past its landing at least 0.25 s
/// before the same bot by the road; a kart lined up at the foot of the jump's ramp at 13 m/s,
/// flat out, falls short: the pelican comes (it fell) and drops it before the jump.
#[test]
fn a_risky_jump_made_saves_time_and_one_too_slow_falls_to_the_pelican() {
    for (circuit, mirrored) in CIRCUITS.into_iter().flat_map(|c| [(c, false), (c, true)]) {
        let mut times = Vec::new();
        for kind in [Some(ShortcutKind::Jump), None] {
            let mut canyon = Canyon::new(circuit, mirrored);
            let jump = canyon.shortcut(ShortcutKind::Jump);
            let skill = canyon.skill_for(kind);
            canyon.becomes_a_bot(skill);
            canyon.place(jump.from - circuit.jump_approach, 26.0);
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
            "{} (mirror {mirrored}): the jump in {:.2} s, the road in {:.2} s",
            circuit.name, times[0], times[1]
        );
        assert!(
            times[0] + 0.25 <= times[1],
            "{} (mirror {mirrored}): the jump in {:.2} s, the road in {:.2} s",
            circuit.name,
            times[0],
            times[1]
        );

        // Too slow: it falls into the drop, the pelican drops it before the jump.
        let mut canyon = Canyon::new(circuit, mirrored);
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
            "{} (mirror {mirrored}): {run:?}",
            circuit.name
        );
        assert!(run.on.contains(&ShortcutKind::Jump), "{run:?}");
        let dropped = run.dropped_at.expect("dropped");
        assert!(
            dropped <= jump.from,
            "{} (mirror {mirrored}): dropped at {dropped:.0} m, the jump from {:.0} m",
            circuit.name,
            jump.from
        );
    }
}

/// Issue #107 acceptance (« l'herbe autorisée ne déclenche pas le pélican et ralentit sans
/// boost »), on the Corniche Cannelle and its mirror (issue #108: and Prairie Pompon): a kart
/// that drives the grass bridge (the grass path) flat out without a boost is never towed and goes
/// no faster there than the off-road's top speed (GD § 2: 55 %); with a boost (2 s, 20 m before
/// it: about a mini-turbo), a Hard bot takes it (it only does with one) and reaches 20 m past its
/// end at least 0.5 s before the same bot with the same boost by the road; without a boost, the
/// bot keeps to the road, and is there at least 0.3 s before a kart that drives the grass from
/// the same place, flat out. Issue #156: on the Corniche Cannelle, a panel closes the grass
/// bridge: the kart that drives it without a boost does it with the panel down (broken a moment
/// before by another kart), and a kart that drives it from its start without a boost, the panel
/// standing, never gets past the panel (it bounces off it) while the bot by the road is there.
#[test]
fn the_grass_bridge_never_calls_the_pelican_slows_without_a_boost_and_pays_with_one() {
    for (circuit, mirrored) in CIRCUITS.into_iter().flat_map(|c| [(c, false), (c, true)]) {
        let name = circuit.name;
        let mut canyon = Canyon::new(circuit, mirrored);
        let grass = canyon.shortcut(ShortcutKind::Grass);
        let tuning = canyon.app.world().resource::<KartTuning>().clone();
        let start = grass.samples()[0];
        canyon.place_at(
            start.position + Vec3::Y * GRID_HEIGHT,
            start.forward,
            12.0,
            grass.from,
        );
        if grass.closed() {
            // Issue #156: its panel down (a kart with a boost broke it a moment ago).
            let world = canyon.app.world_mut();
            let tick = world.resource::<Race>().flow.tick;
            for mut panel in world.query::<&mut crate::track::Panel>().iter_mut(world) {
                panel.down_until = tick + 20 * 60;
            }
        }
        let (path, layout, kart) = (grass.clone(), canyon.layout.clone(), canyon.kart);
        let driver = move |app: &App| follow(&path, &layout, app, kart);
        let run = canyon.run(grass.from + grass.span + 20.0, 15.0, Some(&driver));
        assert!(run.tows.is_empty(), "{name} (mirror {mirrored}): {run:?}");
        assert!(run.ticks.is_some(), "{run:?}");
        assert!(run.grass_ticks >= 60, "{run:?}");
        let cap = tuning.max_speed * tuning.offroad_speed_factor;
        assert!(
            run.grass_speed <= cap * 1.05,
            "{name} (mirror {mirrored}): {:.1} m/s on the grass, the off-road's top speed is \
             {cap:.1}",
            run.grass_speed
        );

        let mut times = Vec::new();
        for kind in [Some(ShortcutKind::Grass), None] {
            let mut canyon = Canyon::new(circuit, mirrored);
            let skill = canyon.skill_for(kind);
            canyon.becomes_a_bot(skill);
            canyon.place(grass.from - circuit.grass_approach, 26.0);
            canyon.boost(2.0);
            let run = canyon.run(grass.from + grass.span + 20.0, 15.0, None);
            assert!(
                run.tows.is_empty(),
                "{name} {kind:?} (mirror {mirrored}): {run:?}"
            );
            assert_eq!(
                run.on.contains(&ShortcutKind::Grass),
                kind.is_some(),
                "{name} {kind:?} (mirror {mirrored}): {run:?}"
            );
            times.push(run.ticks.expect("the goal is reached") as f32 / 60.0);
        }
        println!(
            "{name} (mirror {mirrored}): boosted, the grass in {:.2} s, the road in {:.2} s",
            times[0], times[1]
        );
        assert!(
            times[0] + 0.5 <= times[1],
            "{name} (mirror {mirrored}): boosted, the grass in {:.2} s, the road in {:.2} s",
            times[0],
            times[1]
        );

        // No boost: the bot that would take the grass keeps to the road, and it is right: the
        // grass is slower.
        let mut canyon = Canyon::new(circuit, mirrored);
        let skill = canyon.skill_for(Some(ShortcutKind::Grass));
        canyon.becomes_a_bot(skill);
        canyon.place(grass.from - circuit.grass_approach, 26.0);
        let run = canyon.run(grass.from + grass.span + 20.0, 15.0, None);
        assert!(run.tows.is_empty() && run.on.is_empty(), "{run:?}");
        let road = run.ticks.expect("the goal is reached") as f32 / 60.0;
        if let Some(cover) = grass.cover.filter(|c| c.closes()) {
            // Issue #156: closed by a panel: without a boost, a kart never gets past it.
            let mut canyon = Canyon::new(circuit, mirrored);
            let start = grass.samples()[0];
            canyon.place_at(
                start.position + Vec3::Y * GRID_HEIGHT,
                start.forward,
                20.0,
                grass.from,
            );
            let (path, layout, kart) = (grass.clone(), canyon.layout.clone(), canyon.kart);
            let driver = move |app: &App| follow(&path, &layout, app, kart);
            let mut furthest = f32::MIN;
            for _ in 0..(road * 60.0) as u32 + 60 {
                let input = driver(&canyon.app);
                *canyon
                    .app
                    .world_mut()
                    .get_mut::<KartInput>(canyon.kart)
                    .unwrap() = input;
                canyon.app.update();
                let world = canyon.app.world();
                let at = world.get::<Transform>(canyon.kart).unwrap().translation;
                let here = grass.locate(at);
                // Pushing against it for long, it is stuck: the pelican comes; or it slid off the
                // narrow path along the panel and fell, by the panel: the pelican too.
                if let Some(carry) = world.get::<Tow>(canyon.kart).unwrap().carry {
                    println!("{name} (mirror {mirrored}): {:?} at {here:?}", carry.reason);
                    assert!(
                        carry.reason == TowReason::Stuck
                            || (carry.reason == TowReason::Fell
                                && here.along > cover.at - 4.0
                                && here.along < cover.at + 1.0),
                        "{name} (mirror {mirrored}): {:?} at {here:?}",
                        carry.reason
                    );
                    break;
                }
                if here.lateral.abs() <= grass.half_width + 1.0 {
                    furthest = furthest.max(here.along);
                }
            }
            println!(
                "{name} (mirror {mirrored}): no boost, the road in {road:.2} s, the grass stopped \
                 {:.2} m before its panel",
                cover.at - furthest
            );
            assert!(
                furthest > cover.at - 3.0 && furthest < cover.at - 0.9,
                "{name} (mirror {mirrored}): {furthest:.1} m along the grass, its panel at {}",
                cover.at
            );
            continue;
        }
        let mut canyon = Canyon::new(circuit, mirrored);
        canyon.place(grass.from - circuit.grass_approach, 26.0);
        let (path, layout, kart) = (grass.clone(), canyon.layout.clone(), canyon.kart);
        let driver = move |app: &App| follow(&path, &layout, app, kart);
        let run = canyon.run(grass.from + grass.span + 20.0, 15.0, Some(&driver));
        assert!(
            run.tows.is_empty() && run.on == [ShortcutKind::Grass],
            "{run:?}"
        );
        let on_grass = run.ticks.expect("the goal is reached") as f32 / 60.0;
        println!(
            "{name} (mirror {mirrored}): no boost, the grass in {on_grass:.2} s, the road in \
             {road:.2} s"
        );
        assert!(
            road + 0.3 <= on_grass,
            "{name} (mirror {mirrored}): no boost, the grass in {on_grass:.2} s, the road in \
             {road:.2} s"
        );
    }
}

/// Issue #107: the hidden road saves time, and counts right: a Hard bot that takes it reaches
/// 30 m past its end before the same bot by the road, never towed, with the same checkpoints
/// crossed. Mirrored too; on Prairie Pompon too (issue #108).
#[test]
fn the_hidden_road_saves_time_and_counts_right() {
    for (circuit, mirrored) in CIRCUITS.into_iter().flat_map(|c| [(c, false), (c, true)]) {
        let mut runs = Vec::new();
        for kind in [Some(ShortcutKind::Road), None] {
            let mut canyon = Canyon::new(circuit, mirrored);
            let road = canyon.shortcut(ShortcutKind::Road);
            let skill = canyon.skill_for(kind);
            canyon.becomes_a_bot(skill);
            canyon.place(road.from - circuit.road_approach, 24.0);
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
            "{} (mirror {mirrored}): hidden road {:?}, road {:?}",
            circuit.name, runs[0], runs[1]
        );
        assert!(
            runs[0].0 + 0.4 <= runs[1].0,
            "{} (mirror {mirrored}): {runs:?}",
            circuit.name
        );
        assert_eq!(runs[0].1, runs[1].1, "the same checkpoints crossed");
    }
}

/// Whether the segment from `from` to `to` crosses the box of half extents `half` around
/// `center`, turned by `rotation` (the slabs' test, in the box's frame).
fn crosses_box(from: Vec3, to: Vec3, center: Vec3, rotation: Quat, half: Vec3) -> bool {
    let inverse = rotation.inverse();
    let (a, b) = (inverse * (from - center), inverse * (to - center));
    let d = b - a;
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for axis in 0..3 {
        let (start, way, extent) = (a[axis], d[axis], half[axis]);
        if way.abs() < 1e-6 {
            if start.abs() > extent {
                return false;
            }
            continue;
        }
        let (t1, t2) = ((-extent - start) / way, (extent - start) / way);
        low = low.max(t1.min(t2));
        high = high.min(t1.max(t2));
        if low > high {
            return false;
        }
    }
    true
}

/// Issue #156 acceptance (« un test caché » ; GD « Conception des circuits » : un raccourci est
/// étroit et caché) : on every circuit of the game and its mirror, every shortcut with a cover
/// (a panel, a curtain of foliage) is 4 to 5 m wide, and **its entrance cannot be seen from the
/// bots' racing line**: from the chase camera of a kart on the racing line (its default distance
/// behind and height above it), at every meter of the 80 m up to the shortcut's start along the
/// road, no point of its entrance (the shortcut's path from its cover to 10 m past it: its middle
/// and both edges, 0.3 m above it) is in direct view: the line of sight crosses the cover's box
/// (with the path under it, 2 m deep: one does not see through the road), or the point is out of
/// the widest field of view of the camera (its field at top speed with a boost, on a 16:9
/// screen). Honest and simple: nothing else hides it (no decor counted), and the cover must stand
/// across the path. The Corniche Cannelle has a panel and a curtain of foliage; issue #110: the
/// Forêt Frimousse a waterfall (and foliage), the Falaises Flanelle a stack of crates (and a
/// panel).
#[test]
fn the_covered_shortcuts_cannot_be_seen_from_the_racing_line() {
    use crate::camera::CameraTuning;
    use crate::track::hazard::Shape;
    use crate::track::{CoverKind, HazardKind, TrackData};
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
    let index = std::fs::read_to_string(format!("{assets}/tracks/index.ron")).unwrap();
    let manifest = crate::admin::tracks::parse_manifest(&index).unwrap();
    let camera = CameraTuning::default();
    let t = BotTuning::default();
    let vertical = (camera.max_speed_fov + camera.boost_fov).to_radians();
    let half_fov = ((vertical / 2.0).tan() * 16.0 / 9.0).atan();
    // The entrance: the path from its cover to this far past it (m); the path under the cover
    // hides this deep (m).
    const DEPTH: f32 = 10.0;
    const UNDER: f32 = 2.0;
    let mut kinds = Vec::new();
    for entry in &manifest.circuits {
        let text = std::fs::read_to_string(format!("{assets}/tracks/{}", entry.file)).unwrap();
        let file = TrackData::from_ron(&text).unwrap();
        for data in [file.clone(), file.mirrored()] {
            let layout = TrackLayout::new(&data);
            let line = RacingLine::new(
                &layout,
                t.road_margin,
                t.sand_margin,
                t.void_margin,
                t.line_margin,
                t.hazard_clearance(),
            );
            let center = &layout.center;
            for (k, shortcut) in layout.shortcuts.iter().enumerate() {
                let Some(cover) = shortcut.cover else {
                    continue;
                };
                let name = format!("{} (mirror {}), shortcut {k}", layout.name, data.mirrored);
                kinds.push((layout.name.clone(), cover.kind));
                let width = 2.0 * shortcut.half_width;
                assert!((4.0..=5.0).contains(&width), "{name}: {width} m wide");
                let hazard = layout
                    .hazards
                    .iter()
                    .find(|h| {
                        h.kind == HazardKind::Cover(cover.kind)
                            && h.road.position.distance(shortcut.at(cover.at).position) < 1e-3
                    })
                    .expect("its cover is a hazard of the circuit");
                // The cover, and its path under it (one does not see through the road).
                let cover_box = Shape::cover_box(cover.kind);
                let half = cover_box + Vec3::Y * UNDER / 2.0;
                let base = hazard.road.transform(0.0, 0.0).rotation;
                let middle = hazard.road.point(0.0, cover_box.y - UNDER / 2.0);
                assert!(
                    half.x >= shortcut.half_width + 0.25,
                    "{name}: the cover stands across the path"
                );
                let mut targets = Vec::new();
                let mut along = cover.at + half.z + 0.5;
                while along <= cover.at + DEPTH {
                    let s = shortcut.at(along);
                    for side in [-1.0, 0.0, 1.0] {
                        targets.push(s.point(side * (shortcut.half_width - 0.1), 0.3));
                    }
                    along += 1.0;
                }
                let mut seen = Vec::new();
                for back in 0..=80 {
                    let distance = center.wrap(shortcut.from - back as f32);
                    let road = center.at(distance);
                    let kart = road.point(line.offset_at(distance), 0.5);
                    let heading = road.forward;
                    let eye = kart - heading * camera.distance + road.up * camera.height;
                    let flat = Vec2::new(heading.x, heading.z).normalize();
                    for target in &targets {
                        let to = *target - eye;
                        let off = flat.angle_to(Vec2::new(to.x, to.z).normalize()).abs();
                        if off > half_fov {
                            continue;
                        }
                        if !crosses_box(eye, *target, middle, base, half) {
                            seen.push((back, *target));
                        }
                    }
                }
                assert!(
                    seen.is_empty(),
                    "{name}: {} sights of its path past its {:?} from the racing line (m before \
                     its start, point): {:?}",
                    seen.len(),
                    cover.kind,
                    &seen[..seen.len().min(8)]
                );
            }
        }
    }
    // Issue #110: and the new covers on the new circuits (a waterfall in the Forêt Frimousse, a
    // stack of crates in the Falaises Flanelle).
    for (circuit, kind) in [
        ("Corniche Cannelle", CoverKind::Panel),
        ("Corniche Cannelle", CoverKind::Foliage),
        ("Forêt Frimousse", CoverKind::Waterfall),
        ("Forêt Frimousse", CoverKind::Foliage),
        ("Falaises Flanelle", CoverKind::Crates),
        ("Falaises Flanelle", CoverKind::Panel),
    ] {
        assert!(
            kinds.contains(&(circuit.to_string(), kind)),
            "the {circuit} has a {kind:?}: {kinds:?}"
        );
    }
}
