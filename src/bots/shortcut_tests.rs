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
    Autopilot, Boost, DriftMotion, Invincibility, ItemEffects, KartEnvironment, KartInput,
    KartState, KartTuning, PlayerKart,
};
use crate::race::{Course, Race, RaceProgress, Tow, TowReason};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};
use crate::track::{GRID_HEIGHT, MirrorMode, Shortcut, ShortcutKind, TrackChoice, TrackLayout};

/// A circuit of the guide with a shortcut of each kind (issue #107: the Corniche Cannelle; issue
/// #108: Prairie Pompon redrawn; issue #109: the Col des Moufles redrawn; issue #119: the
/// Échappée Pissenlit, the first long race; issue #110: the Forêt Frimousse and the Falaises
/// Flanelle; issue #111: the Coteau Citrouille and the Périple Polochon), and where the tests
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

const CIRCUITS: [Circuit; 8] = [
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
    // Issue #111, the Coteau Citrouille (the Corniche Cannelle's pieces, other covers): the jump
    // from out of the hairpin of the crest, the grass bridge from out of the bulge, the hidden
    // road from out of the last hairpin of the hillside.
    Circuit {
        path: "tracks/coteau.track.ron",
        name: "Coteau Citrouille",
        jump_approach: 50.0,
        grass_approach: 20.0,
        road_approach: 60.0,
    },
    // Issue #111, the Périple Polochon, the second long race (the Corniche Cannelle's pieces as in
    // the Falaises Flanelle and the Forêt Frimousse, other covers): the jump from out of the tight
    // corner before the cliff's edge, the grass bridge from out of the bulge, the hidden road from
    // out of the tight corner before its approach.
    Circuit {
        path: "tracks/periple.track.ron",
        name: "Périple Polochon",
        jump_approach: 50.0,
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

    /// Issue #171: a purple mini-turbo's boost (its strength, its 2.63 s): what pays on a grass
    /// path since its grass is slower (40 % of the top speed), most of the way on it.
    fn purple_boost(&mut self) {
        let seconds = self
            .app
            .world()
            .resource::<KartTuning>()
            .mini_turbo_durations[2];
        self.boost(seconds);
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
/// no faster there than the off-road's top speed (GD § 2: 40 % since #171); with a boost (issue
/// #171: a purple mini-turbo's, 2.63 s, 20 m before it; 2 s before #171, when the grass was 55 %:
/// on the bridges of 72 m, it now loses 0.1 to 0.2 s with it), a Hard bot takes it (it only does
/// with one) and reaches 20 m past its end at least 0.5 s before the same bot with the same boost
/// by the road; without a boost, the
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
            canyon.purple_boost();
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
/// panel); issue #111: the Coteau Citrouille a stack of crates and foliage, the Périple Polochon a
/// waterfall and a panel.
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
    // stack of crates in the Falaises Flanelle); issue #111: the Coteau Citrouille and the Périple
    // Polochon.
    for (circuit, kind) in [
        ("Corniche Cannelle", CoverKind::Panel),
        ("Corniche Cannelle", CoverKind::Foliage),
        ("Forêt Frimousse", CoverKind::Waterfall),
        ("Forêt Frimousse", CoverKind::Foliage),
        ("Falaises Flanelle", CoverKind::Crates),
        ("Falaises Flanelle", CoverKind::Panel),
        ("Coteau Citrouille", CoverKind::Crates),
        ("Coteau Citrouille", CoverKind::Foliage),
        ("Périple Polochon", CoverKind::Waterfall),
        ("Périple Polochon", CoverKind::Panel),
    ] {
        assert!(
            kinds.contains(&(circuit.to_string(), kind)),
            "the {circuit} has a {kind:?}: {kinds:?}"
        );
    }
}

impl Canyon {
    /// Issue #171: the kart's fusée starts now (as `items::use_items` starts it: its effect and
    /// its invincibility), after any effect of before is gone.
    fn fusee(&mut self) {
        let time = self
            .app
            .world()
            .resource::<crate::items::ItemTuning>()
            .fusee_time;
        let world = self.app.world_mut();
        *world.get_mut::<ItemEffects>(self.kart).unwrap() = ItemEffects {
            rocket: time,
            ..default()
        };
        *world.get_mut::<Autopilot>(self.kart).unwrap() = Autopilot::default();
        world
            .get_mut::<Invincibility>(self.kart)
            .unwrap()
            .start(time);
    }

    fn rocket(&self) -> f32 {
        self.app
            .world()
            .get::<ItemEffects>(self.kart)
            .unwrap()
            .rocket
    }

    /// Issue #171: runs while the fusée lasts (and `after` seconds more), `input` driving the
    /// kart (`None`: its bot): how long the fusée lasted (s), the tows meanwhile, the kinds of
    /// shortcut it was counted on, how far it went along the race (m), and the lowest it went
    /// under the shortcut's `low` while counted on it (m: negative under it).
    fn ride(&mut self, low: f32, after: f32, input: Option<&dyn Fn(&App) -> KartInput>) -> Ride {
        let start = self.progress().distance;
        let mut ride = Ride {
            lowest: f32::MAX,
            ..default()
        };
        let mut left = (after * 60.0) as u32;
        for _ in 0..(12.0 * 60.0) as u32 {
            if let Some(input) = input {
                let wanted = input(&self.app);
                *self
                    .app
                    .world_mut()
                    .get_mut::<KartInput>(self.kart)
                    .unwrap() = wanted;
            }
            let rocket = self.rocket() > 0.0;
            self.app.update();
            if rocket {
                ride.seconds += 1.0 / 60.0;
            } else if left == 0 {
                break;
            } else {
                left -= 1;
            }
            let world = self.app.world();
            let at = world.get::<Transform>(self.kart).unwrap().translation;
            let progress = world.get::<RaceProgress>(self.kart).unwrap();
            if let Some(carry) = world.get::<Tow>(self.kart).unwrap().carry
                && !ride.tows.contains(&carry.reason)
            {
                ride.tows.push(carry.reason);
                ride.towed_at = Some(at);
            }
            if let Some(k) = self.course.project(progress.passed, at).shortcut {
                let kind = self.course.shortcuts[k].kind;
                if ride.on.last() != Some(&kind) {
                    ride.on.push(kind);
                }
                // Under the shortcut while on it (beyond it, the road may go down).
                ride.lowest = ride.lowest.min(at.y - low);
            }
        }
        ride.gone = self.progress().distance - start;
        ride
    }

    fn progress(&self) -> RaceProgress {
        self.app
            .world()
            .get::<RaceProgress>(self.kart)
            .unwrap()
            .clone()
    }
}

/// What a fusée did (issue #171).
#[derive(Clone, Debug, Default)]
struct Ride {
    seconds: f32,
    tows: Vec<TowReason>,
    towed_at: Option<Vec3>,
    on: Vec<ShortcutKind>,
    gone: f32,
    lowest: f32,
}

const KINDS: [ShortcutKind; 3] = [ShortcutKind::Road, ShortcutKind::Grass, ShortcutKind::Jump];

/// Issue #171 acceptance (« la fusée sur chaque sorte de raccourci sans chute »): on every circuit
/// with shortcuts ([`CIRCUITS`]: the Corniche Cannelle, Prairie Pompon, the Col des Moufles and
/// the others) and its mirror, a fusée launched at the entrance of each shortcut (the hidden
/// road, the grass path, the risky jump), in its middle (over the void of the jump) and near its
/// exit, heading along it at 20 m/s, lasts its 6 s (a little longer if it lands from the jump
/// then), never needs the pelican, never goes more than 3 m under the shortcut's lower end while
/// on it, and carries the kart more than 150 m along the race. Before #171, it steered for the racing line
/// across the void: it fell from every one of them.
#[test]
fn a_fusee_never_falls_off_a_shortcut() {
    for (circuit, mirrored) in CIRCUITS.into_iter().flat_map(|c| [(c, false), (c, true)]) {
        let mut canyon = Canyon::new(circuit, mirrored);
        let time = canyon
            .app
            .world()
            .resource::<crate::items::ItemTuning>()
            .fusee_time;
        for kind in KINDS {
            let path = canyon.shortcut(kind);
            let length = path.length();
            for (place, along) in [
                ("entrance", 6.0),
                ("middle", length / 2.0),
                ("exit", length - 8.0),
            ] {
                let s = path.at(along);
                let ahead = s.forward.with_y(0.0).normalize();
                let distance = path.from + path.span * path.share(along);
                canyon.place_at(s.position + s.up * GRID_HEIGHT, ahead, 20.0, distance);
                canyon.fusee();
                let ride = canyon.ride(path.low, 0.0, Some(&|_: &App| KartInput::default()));
                let name = format!("{} (mirror {mirrored}), {kind:?} {place}", circuit.name);
                println!("{name}: {ride:?}");
                assert!(ride.tows.is_empty(), "{name}: {ride:?}");
                assert!(ride.seconds >= time - 0.05, "{name}: {ride:?}");
                assert!(ride.seconds <= time + 2.0, "{name}: {ride:?}");
                assert!(ride.lowest > -3.0, "{name}: {ride:?}");
                assert!(ride.gone > 150.0, "{name}: {ride:?}");
            }
        }
    }
}

/// Issue #171 (« vérifie le même cas pour les bots »): a Hard bot that takes each shortcut (the
/// grass with its boost) gets its fusée once it is on it, a third of the way (over the void of
/// the jump: in the air past its ramp): the fusée never falls, nor the bot that drives on after
/// it (2 s more), on every circuit with shortcuts and its mirror.
#[test]
fn a_bot_fusee_never_falls_off_a_shortcut() {
    for (circuit, mirrored) in CIRCUITS.into_iter().flat_map(|c| [(c, false), (c, true)]) {
        for kind in KINDS {
            let mut canyon = Canyon::new(circuit, mirrored);
            let path = canyon.shortcut(kind);
            let skill = canyon.skill_for(Some(kind));
            canyon.becomes_a_bot(skill);
            let approach = match kind {
                ShortcutKind::Road => circuit.road_approach,
                ShortcutKind::Grass => circuit.grass_approach,
                ShortcutKind::Jump => circuit.jump_approach,
            };
            canyon.place(path.from - approach, 26.0);
            if kind == ShortcutKind::Grass {
                canyon.purple_boost();
            }
            let name = format!("{} (mirror {mirrored}), {kind:?}", circuit.name);
            let mut on = false;
            for _ in 0..(20 * 60) {
                canyon.app.update();
                let world = canyon.app.world();
                let at = world.get::<Transform>(canyon.kart).unwrap().translation;
                let here = path.locate(at);
                let airborne = world
                    .get::<crate::kart::KartWheels>(canyon.kart)
                    .unwrap()
                    .ticks_without_ground
                    > 5;
                let routed = matches!(
                    world.get::<BotBrain>(canyon.kart).unwrap().route,
                    Route::On(_)
                );
                assert!(
                    world.get::<Tow>(canyon.kart).unwrap().carry.is_none(),
                    "{name}: towed before its fusée"
                );
                if routed
                    && here.along > path.length() / 3.0
                    && (kind != ShortcutKind::Jump || airborne)
                {
                    on = true;
                    break;
                }
            }
            assert!(on, "{name}: the bot never got a third of the way on it");
            canyon.fusee();
            let ride = canyon.ride(path.low, 2.0, None);
            println!("{name}: {ride:?}");
            assert!(ride.tows.is_empty(), "{name}: {ride:?}");
            assert!(ride.lowest > -3.0, "{name}: {ride:?}");
            assert!(ride.gone > 150.0, "{name}: {ride:?}");
        }
    }
}

/// Issue #171: a shortcut's road or grass path lies above the circuit's road and shoulders where
/// it runs over them (its two ends), by most of its lift (`track::mesh::SHORTCUT_LIFT`), on every
/// circuit with shortcuts: its wheels find it first. Prairie Pompon's hidden road came back over
/// the road's shoulder as high as it, for its last 12 m: the karts on it found the grass as often
/// as the road (it hardly mattered at 55 % off the road; at 40 %, the Hard bots lost 0.1 s a lap
/// there).
#[test]
fn a_shortcut_lies_over_the_road_it_runs_on() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
    for circuit in CIRCUITS {
        let text = std::fs::read_to_string(format!("{assets}/{}", circuit.path)).unwrap();
        let data = crate::track::TrackData::from_ron(&text).unwrap();
        for data in [data.clone(), data.mirrored()] {
            let layout = TrackLayout::new(&data);
            for shortcut in layout
                .shortcuts
                .iter()
                .filter(|s| s.kind != ShortcutKind::Jump)
            {
                for s in shortcut.samples() {
                    for side in [-1.0f32, 0.0, 1.0] {
                        let p = s.point(side * (shortcut.half_width - 0.3), 0.0);
                        let c = &layout.center.samples()[layout.center.nearest_index(p)];
                        let lateral = (p - c.position).dot(c.right);
                        if lateral.abs() > crate::track::mesh::wall_offset(c) {
                            continue;
                        }
                        let ground = c.point(lateral, 0.0);
                        let ground = ground + c.forward * (p - ground).dot(c.forward);
                        let above = p.y + crate::track::mesh::SHORTCUT_LIFT - ground.y;
                        assert!(
                            above >= 0.01,
                            "{} (mirror {}), {:?} at {:.1} m: {above:.3} m above the road",
                            circuit.name,
                            data.mirrored,
                            shortcut.kind,
                            s.distance
                        );
                    }
                }
            }
        }
    }
}
