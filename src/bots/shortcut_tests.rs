//! Issue #107 acceptance: headless scenarios of the shortcuts on the Corniche Cannelle and its
//! mirror (the whole game, `GamePlugin` with `MinimalPlugins`, the player's kart alone, its race
//! started, one fixed tick per `app.update()`): a risky jump too slow falls and the pelican drops
//! the kart before it; the grass never calls the pelican; the bots take them as their level says.
//! Issue #108 and after: the same on every circuit of the guide ([`CIRCUITS`]). Issue #193 (Max:
//! « je veux que les raccourcis soient tous en herbe, même ceux cachés, et il faut absolument un
//! piment ou équivalent pour passer ») : every shortcut is grass (a jump's ramp too), loses time
//! without a boost even taken perfectly, saves some with one, and the bots take one only with a
//! boost.

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
/// Flanelle; issue #111: the Coteau Citrouille and the Périple Polochon; issue #148: the Anneau
/// Guimauve and the Crique Câline redrawn), and where the tests
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

const CIRCUITS: [Circuit; 11] = [
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
    // Issue #148, the Anneau Guimauve redrawn (the Coteau Citrouille's pieces, other covers): the
    // jump from out of the tight corner before the top of the cake, the grass bridge from out of
    // the bulge, the hidden road from the approach of the bulge (after its bump).
    Circuit {
        path: "tracks/anneau.track.ron",
        name: "Anneau Guimauve",
        jump_approach: 50.0,
        grass_approach: 20.0,
        road_approach: 60.0,
    },
    // Issue #148, the Crique Câline redrawn (the Coteau Citrouille's pieces mirrored, other
    // covers): the jump from out of the tight corner before the top of the cape, the grass bridge
    // from out of the bulge, the hidden road from the foot of the drop before it (its run-out).
    Circuit {
        path: "tracks/crique.track.ron",
        name: "Crique Câline",
        jump_approach: 50.0,
        grass_approach: 20.0,
        road_approach: 40.0,
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
    // Issue #150, the Lagon Duvet redrawn (the Forêt Frimousse's pieces, other covers): the jump
    // from the serpent's last leg, the grass bridge from out of the bulge, the hidden road from
    // the last straight (past its bump).
    Circuit {
        path: "tracks/lagon.track.ron",
        name: "Lagon Duvet",
        jump_approach: 60.0,
        grass_approach: 20.0,
        road_approach: 40.0,
    },
];

/// The circuits of [`CIRCUITS`] the run asks for (`FLUFFY_RACE_CIRCUITS`, as the races of
/// `scenario_tests`: all of them by default).
fn wanted() -> impl Iterator<Item = Circuit> {
    CIRCUITS
        .into_iter()
        .filter(|c| super::scenario_tests::circuit_wanted(c.path))
}

impl Circuit {
    /// Where the tests start their bot before shortcut `k` of the circuit (issue #193: by its
    /// number in the file: the jump, the hidden path, the grass bridge on every circuit of the
    /// guide).
    fn approach(&self, k: usize) -> f32 {
        [self.jump_approach, self.road_approach, self.grass_approach][k]
    }
}

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
    /// The kinds of shortcut the kart was counted on (`Course::project`), in order, and their
    /// numbers.
    on: Vec<ShortcutKind>,
    on_index: Vec<usize>,
    /// Fastest forward speed on a grass path without a boost after its first half second there
    /// (m/s), and ticks spent there.
    grass_speed: f32,
    grass_ticks: u32,
    /// Issue #193: ticks on a shortcut's grass (well inside its path, or on a jump's ramp) with
    /// its wheels on the ground, and those of them with a wheel on something else than grass.
    ground_ticks: u32,
    not_grass_ticks: u32,
    /// Its race distance right after the pelican dropped it.
    dropped_at: Option<f32>,
    /// Its checkpoints crossed at the end.
    passed: u32,
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
        // Issue #193: the jump, the hidden path and the grass bridge, in this order.
        let kinds: Vec<ShortcutKind> = layout.shortcuts.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [ShortcutKind::Jump, ShortcutKind::Grass, ShortcutKind::Grass],
            "{}",
            circuit.name
        );
        Self {
            app,
            kart,
            layout,
            course,
        }
    }

    /// The circuit's first shortcut of `kind`.
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

    /// A Hard bot's skill that takes shortcuts of `kind` every time it may (with a boost: issue
    /// #193), and no other.
    fn skill_for(&self, kind: Option<ShortcutKind>) -> BotSkill {
        let tuning = self.app.world().resource::<BotTuning>();
        let mut skill = *tuning.skill(BotDifficulty::Hard);
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

    /// Issue #193: the boost the tests fire `lead` m before `path` (of a purple mini-turbo's
    /// strength): a purple mini-turbo's (2.63 s), or longer on a path that needs more for its
    /// boost to carry the kart over all of it (a jump: up to its ramp's lip): a long hidden path
    /// (60 to 100 m) needs a piment doré, or a purple fired at its very mouth; covering less of
    /// it hardly pays, the rest of the way at 35 % of the top speed costs what the boost saved.
    fn boost_for(&self, path: &Shortcut, lead: f32) -> f32 {
        let t = self.app.world().resource::<KartTuning>();
        let bots = self.app.world().resource::<BotTuning>();
        let top = t.max_speed * (1.0 + t.mini_turbo_strengths[2]);
        let run = match path.ramp {
            Some(ramp) => path.locate(ramp.lip()).along + bots.jump_boost_margin,
            None => path.length(),
        };
        t.mini_turbo_durations[2].max((lead + run) / top * 1.1 + 0.2)
    }

    /// Issue #156: every panel of the circuit down for 20 s (a kart with a boost broke it a
    /// moment ago).
    fn panels_down(&mut self) {
        let world = self.app.world_mut();
        let tick = world.resource::<Race>().flow.tick;
        for mut panel in world.query::<&mut crate::track::Panel>().iter_mut(world) {
            panel.down_until = tick + 20 * 60;
        }
    }

    /// Runs until the kart's race distance reaches `goal` (or `seconds` went by); `input` drives
    /// it when it is not a bot (`None`: its bot drives).
    fn run(&mut self, goal: f32, seconds: f32, input: Option<&dyn Fn(&App) -> KartInput>) -> Run {
        self.run_boosted(goal, seconds, input, None)
    }

    /// [`Self::run`], with a boost of a purple mini-turbo's strength, `boost.1` s long, given once
    /// the kart's race distance reaches `boost.0` (issue #193: the boost fired just before a
    /// shortcut).
    fn run_boosted(
        &mut self,
        goal: f32,
        seconds: f32,
        input: Option<&dyn Fn(&App) -> KartInput>,
        mut boost_at: Option<(f32, f32)>,
    ) -> Run {
        let mut run = Run::default();
        let max_speed = self.app.world().resource::<KartTuning>().max_speed;
        let mut was_carried = false;
        for tick in 1..=(seconds * 60.0) as u32 {
            if let Some((at, length)) = boost_at {
                let distance = self
                    .app
                    .world()
                    .get::<RaceProgress>(self.kart)
                    .unwrap()
                    .distance;
                if distance >= at {
                    self.boost(length);
                    boost_at = None;
                }
            }
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
                let path = &self.course.shortcuts[k];
                let kind = path.kind;
                if run.on_index.last() != Some(&k) {
                    run.on.push(kind);
                    run.on_index.push(k);
                }
                let boost = world.get::<Boost>(self.kart).unwrap();
                if kind == ShortcutKind::Grass && boost.remaining <= 0.0 {
                    // Once it braked down to the grass's speed (issue #193: 0.25 s from the top
                    // speed; half a second given).
                    if run.grass_ticks >= 30 {
                        let velocity = world.get::<Velocity>(self.kart).unwrap().linear;
                        run.grass_speed = run.grass_speed.max(velocity.dot(*transform.forward()));
                    }
                    run.grass_ticks += 1;
                }
            }
            // Issue #193: what its wheels stand on, well inside a grass path (counted on it), or
            // on a jump's ramp (its box).
            let wheels = world.get::<crate::kart::KartWheels>(self.kart).unwrap();
            let at = transform.translation;
            let inside =
                self.course
                    .shortcuts
                    .iter()
                    .enumerate()
                    .any(|(k, path)| match path.ramp {
                        Some(ramp) => {
                            let d = at - ramp.start;
                            let along = d.dot(ramp.direction);
                            let across = d.dot(Vec3::new(-ramp.direction.z, 0.0, ramp.direction.x));
                            // Its four wheels on it: a kart's length in from its foot.
                            (2.0..ramp.length * ramp.angle.cos() - 1.0).contains(&along)
                                && across.abs() < ramp.width / 2.0 - 1.0
                        }
                        None => {
                            let here = path.locate(at);
                            projection.shortcut == Some(k)
                                && (1.0..path.length() - 1.0).contains(&here.along)
                                && here.lateral.abs() < path.half_width - 1.0
                        }
                    });
            if inside && wheels.on_ground.iter().all(|on| *on) {
                run.ground_ticks += 1;
                if wheels.offroad_ratio < 0.999 {
                    run.not_grass_ticks += 1;
                }
            }
            assert!(run.grass_speed <= max_speed * 1.5);
            if progress.distance >= goal && !carried {
                run.ticks = Some(tick);
                break;
            }
        }
        run.passed = self
            .app
            .world()
            .get::<RaceProgress>(self.kart)
            .unwrap()
            .passed;
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

/// Issue #193: the least a boosted shortcut saves on the road **with the same boost** (s): a
/// risky jump, a hidden path, a grass bridge. Before #193, against the road without a boost, the
/// jump and the hidden road taken without one: 0.25, 0.4 and 0.5 s (a boosted kart takes the
/// road's turns faster too: the same boost on both sides is the fair measure).
const BOOSTED_GAIN: [f32; 3] = [0.15, 0.5, 0.5];
/// Issue #193: how far before a shortcut's start the boost of the tests is fired (m): « un
/// mini-turbo violet lancé 20 m avant » (#171) for a grass bridge; the risky jump's ramp and the
/// hidden path's mouth need the bot lined up first (it heads for one only once it has a boost
/// that carries it there and over most of it).
const BOOST_LEAD: [f32; 3] = [40.0, 40.0, 20.0];

/// Issue #193 acceptance (« sans boost, un raccourci fait perdre du temps à coup sûr, même à un
/// bot Difficile qui le prend parfaitement ; avec un piment, un mini-turbo violet ou un plot
/// juste avant, il en fait gagner » ; « le gain avec boost et la perte sans boost sont vérifiés
/// sur chaque raccourci de chaque circuit du guide » ; « sol d'herbe sur tout le chemin ») : on
/// every circuit of the guide with shortcuts ([`CIRCUITS`]) and its mirror, for **each** of its
/// shortcuts (the risky jump, the hidden path, the grass bridge):
///
/// - **with a boost** (a purple mini-turbo's, fired [`BOOST_LEAD`] before its start), a Hard bot
///   that may take it takes it (counted on it, never towed, the same checkpoints crossed) and
///   reaches 20 m past its end at least [`BOOSTED_GAIN`] before the same bot with the same boost
///   by the road (which never takes it);
/// - **without a boost**, the bot that would take it keeps to the road (issue #193: never on a
///   shortcut without one); and a kart that takes it anyway, flat out along its path from the same
///   place (the perfect line), is there later: at least 0.3 s on a grass path (its panel down, if
///   it has one: below, it never gets past it standing), and a risky jump made at full speed
///   falls short (its ramp is grass) and the pelican brings it back before it;
/// - **its ground is grass all along**: every tick that kart is well inside the path (a grass
///   path: 1 m from its edges and its ends; a jump: on its ramp), its four wheels on the ground,
///   all of them are on the off-road (its top speed there the off-road's, 35 %).
///
/// Before #193, the hidden roads paid without a boost (0.65 to 0.9 s), the jumps at full speed
/// (0.3 to 0.6 s).
#[test]
fn every_shortcut_is_grass_loses_time_without_a_boost_and_saves_some_with_one() {
    let mut failures = Vec::new();
    for (circuit, mirrored) in wanted().flat_map(|c| [(c, false), (c, true)]) {
        let name = circuit.name;
        for k in 0..3 {
            let canyon = Canyon::new(circuit, mirrored);
            let path = canyon.layout.shortcuts[k].clone();
            let kind = path.kind;
            let tuning = canyon.app.world().resource::<KartTuning>().clone();
            let approach = circuit.approach(k);
            let lead = BOOST_LEAD[k].min(approach);
            let seconds = canyon.boost_for(&path, lead);
            drop(canyon);
            let goal = path.from + path.span + 20.0;
            let label = format!("{name} (mirror {mirrored}), shortcut {k} ({kind:?})");

            // With a boost: by the shortcut, then by the road.
            let mut boosted = Vec::new();
            for takes in [true, false] {
                let mut canyon = Canyon::new(circuit, mirrored);
                let skill = canyon.skill_for(takes.then_some(kind));
                canyon.becomes_a_bot(skill);
                canyon.place(path.from - approach, 26.0);
                let run = canyon.run_boosted(goal, 20.0, None, Some((path.from - lead, seconds)));
                if takes {
                    println!("{label}: boosted by it, {run:?}");
                }
                assert!(
                    run.tows.is_empty(),
                    "{label} boosted, takes {takes}: {run:?}"
                );
                assert_eq!(
                    run.on_index.contains(&k),
                    takes,
                    "{label} boosted, takes {takes}: {run:?}"
                );
                boosted.push((
                    run.ticks.expect("the goal is reached") as f32 / 60.0,
                    run.passed,
                ));
            }
            println!(
                "{label}: boosted ({:.2} s, {} m before), the shortcut in {:.2} s, the road in \
                 {:.2} s",
                seconds, lead, boosted[0].0, boosted[1].0
            );
            if boosted[0].0 + BOOSTED_GAIN[k] > boosted[1].0 {
                failures.push(format!(
                    "{label}: boosted, the shortcut in {:.2} s, the road in {:.2} s",
                    boosted[0].0, boosted[1].0
                ));
            }
            assert_eq!(boosted[0].1, boosted[1].1, "{label}: the same checkpoints");

            // Without a boost: the bot that would take it keeps to the road.
            let mut canyon = Canyon::new(circuit, mirrored);
            let skill = canyon.skill_for(Some(kind));
            canyon.becomes_a_bot(skill);
            canyon.place(path.from - approach, 26.0);
            let run = canyon.run(goal, 20.0, None);
            assert!(run.tows.is_empty() && run.on.is_empty(), "{label}: {run:?}");
            let road = run.ticks.expect("the goal is reached") as f32 / 60.0;

            // A kart that takes it anyway, flat out along it from the same place, at full speed.
            let mut canyon = Canyon::new(circuit, mirrored);
            canyon.place(path.from - approach, tuning.max_speed);
            if path.closed() {
                canyon.panels_down();
            }
            let (shortcut, layout, kart) = (path.clone(), canyon.layout.clone(), canyon.kart);
            let driver = move |app: &App| follow(&shortcut, &layout, app, kart);
            let run = canyon.run(goal, 25.0, Some(&driver));
            assert!(run.on_index.contains(&k), "{label}: {run:?}");
            // Many ticks on a grass path; a few on a jump's short ramp (its four wheels on it).
            let least = if kind == ShortcutKind::Jump { 5 } else { 60 };
            assert!(run.ground_ticks >= least, "{label}: {run:?}");
            assert_eq!(
                run.not_grass_ticks, 0,
                "{label}: off the grass on it ({run:?})"
            );
            let lost = match kind {
                ShortcutKind::Grass => {
                    assert!(run.tows.is_empty(), "{label}: {run:?}");
                    let cap = tuning.max_speed * tuning.offroad_speed_factor;
                    assert!(
                        run.grass_speed <= cap * 1.05,
                        "{label}: {:.1} m/s on the grass, the off-road's top speed is {cap:.1}",
                        run.grass_speed
                    );
                    run.ticks.expect("the goal is reached") as f32 / 60.0
                }
                ShortcutKind::Jump => {
                    // Its grass ramp brakes it: it falls short, the pelican drops it before.
                    assert_eq!(run.tows.first(), Some(&TowReason::Fell), "{label}: {run:?}");
                    let dropped = run.dropped_at.expect("dropped");
                    assert!(dropped <= path.from, "{label}: dropped at {dropped:.0} m");
                    run.ticks.map_or(f32::INFINITY, |t| t as f32 / 60.0)
                }
            };
            println!("{label}: no boost, the road in {road:.2} s, the shortcut in {lost:.2} s");
            if road + 0.3 > lost {
                failures.push(format!(
                    "{label}: no boost, the road in {road:.2} s, the shortcut in {lost:.2} s"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #107 acceptance (« un saut risqué réussi fait gagner du temps, raté il fait tomber et
/// venir le pélican »), on every circuit of the guide with shortcuts and its mirror: a kart lined
/// up at the foot of the jump's ramp at 13 m/s, flat out, falls short: the pelican comes (it
/// fell) and drops it before the jump. Made (with a boost since #193): above.
#[test]
fn a_risky_jump_too_slow_falls_to_the_pelican() {
    for (circuit, mirrored) in wanted().flat_map(|c| [(c, false), (c, true)]) {
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

/// Issue #196 (« un kart touché en l'air ou sur un tremplin ne doit pas tomber dans le vide à
/// cause de cet arrêt »): a Hard bot that makes a risky jump with its boost (as in the test
/// above), hit by a pelote (a tumble, the hit that stops the most) on the jump's ramp, or just
/// after its takeoff over the void, still makes it: no pelican, it reaches the road after the
/// jump; on every circuit with shortcuts and its mirror. On the ramp, it keeps the speed of its
/// jump (not braked, its boost lost all the same).
#[test]
fn a_kart_hit_on_a_risky_jump_still_makes_it() {
    let mut failures = Vec::new();
    for (circuit, mirrored) in wanted().flat_map(|c| [(c, false), (c, true)]) {
        for on_the_ramp in [true, false] {
            let mut canyon = Canyon::new(circuit, mirrored);
            let path = canyon.shortcut(ShortcutKind::Jump);
            let approach = circuit.approach(0);
            let lead = BOOST_LEAD[0].min(approach);
            let seconds = canyon.boost_for(&path, lead);
            let skill = canyon.skill_for(Some(ShortcutKind::Jump));
            canyon.becomes_a_bot(skill);
            canyon.place(path.from - approach, 26.0);
            let kart = canyon.kart;
            let name = format!(
                "{} (mirror {mirrored}), hit {}",
                circuit.name,
                if on_the_ramp {
                    "on the ramp"
                } else {
                    "after the takeoff"
                }
            );
            let mut boost_at = Some(path.from - lead);
            let mut hit_at = None;
            let mut slowest_on_ramp = f32::MAX;
            let mut took_off = false;
            let goal = path.from + path.span + 20.0;
            let mut tows = Vec::new();
            let mut reached = false;
            for _ in 0..(20 * 60) {
                let world = canyon.app.world();
                let distance = world.get::<RaceProgress>(kart).unwrap().distance;
                let tricks = *world.get::<crate::kart::TrickTracker>(kart).unwrap();
                let speed = {
                    let transform = world.get::<Transform>(kart).unwrap();
                    world
                        .get::<Velocity>(kart)
                        .unwrap()
                        .linear
                        .dot(*transform.forward())
                };
                if boost_at.is_some_and(|at| distance >= at) {
                    canyon.boost(seconds);
                    boost_at = None;
                }
                let now = if on_the_ramp {
                    tricks.on_ramp()
                } else {
                    tricks.ticks_since_takeoff() == Some(3)
                };
                if now && hit_at.is_none() {
                    hit_at = Some(speed);
                    canyon.app.world_mut().write_message(crate::items::ItemHit {
                        kart,
                        by: Entity::PLACEHOLDER,
                        item: crate::items::ItemKind::Pelote,
                        object: None,
                    });
                }
                // On the jump's ramp, until it takes off (a trick ramp further on does not count).
                if hit_at.is_some() && tricks.on_ramp() && !took_off {
                    slowest_on_ramp = slowest_on_ramp.min(speed);
                }
                took_off |= hit_at.is_some() && tricks.ticks_since_takeoff().is_some();
                canyon.app.update();
                let world = canyon.app.world();
                if let Some(carry) = world.get::<Tow>(kart).unwrap().carry
                    && !tows.contains(&carry.reason)
                {
                    tows.push(carry.reason);
                }
                if world.get::<RaceProgress>(kart).unwrap().distance >= goal {
                    reached = true;
                    break;
                }
            }
            println!(
                "{name}: hit at {hit_at:?} m/s, slowest on the ramp {slowest_on_ramp:.1}, tows \
                 {tows:?}, reached {reached}"
            );
            let Some(speed) = hit_at else {
                failures.push(format!("{name}: never hit"));
                continue;
            };
            if !tows.is_empty() || !reached {
                failures.push(format!("{name}: tows {tows:?}, reached {reached}"));
            }
            if on_the_ramp && slowest_on_ramp < 0.8 * speed {
                failures.push(format!(
                    "{name}: braked on the ramp, {slowest_on_ramp:.1} from {speed:.1} m/s"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #107 acceptance (« l'herbe autorisée ne déclenche pas le pélican »), issue #156: a
/// shortcut closed by a panel: a kart that drives it from its start without a boost, the panel
/// standing, never gets past the panel (it bounces off it, or is stuck and towed), on every
/// circuit with one and its mirror.
#[test]
fn a_panel_stops_a_kart_without_a_boost() {
    let mut closed = 0;
    for (circuit, mirrored) in wanted().flat_map(|c| [(c, false), (c, true)]) {
        let name = circuit.name;
        let canyon = Canyon::new(circuit, mirrored);
        let Some(grass) = canyon.layout.shortcuts.iter().find(|s| s.closed()).cloned() else {
            continue;
        };
        drop(canyon);
        closed += 1;
        let cover = grass.cover.unwrap();
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
        for _ in 0..(12 * 60) {
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
            "{name} (mirror {mirrored}): no boost, the grass stopped {:.2} m before its panel",
            cover.at - furthest
        );
        assert!(
            furthest > cover.at - 3.0 && furthest < cover.at - 0.9,
            "{name} (mirror {mirrored}): {furthest:.1} m along the grass, its panel at {}",
            cover.at
        );
    }
    assert!(closed >= 8, "{closed} circuits with a panel");
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
/// waterfall and a panel; issue #148: the Anneau Guimauve a stack of crates and a panel, the
/// Crique Câline a waterfall and foliage.
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
    // Polochon; issue #148: the Anneau Guimauve and the Crique Câline redrawn; issue #150: the
    // Lagon Duvet redrawn.
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
        ("Anneau Guimauve", CoverKind::Crates),
        ("Anneau Guimauve", CoverKind::Panel),
        ("Crique Câline", CoverKind::Waterfall),
        ("Crique Câline", CoverKind::Foliage),
        ("Lagon Duvet", CoverKind::Foliage),
        ("Lagon Duvet", CoverKind::Panel),
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
    for (circuit, mirrored) in wanted().flat_map(|c| [(c, false), (c, true)]) {
        let mut canyon = Canyon::new(circuit, mirrored);
        let time = canyon
            .app
            .world()
            .resource::<crate::items::ItemTuning>()
            .fusee_time;
        for k in 0..3 {
            let path = canyon.layout.shortcuts[k].clone();
            let kind = path.kind;
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
    for (circuit, mirrored) in wanted().flat_map(|c| [(c, false), (c, true)]) {
        for (k, lead) in BOOST_LEAD.into_iter().enumerate() {
            let mut canyon = Canyon::new(circuit, mirrored);
            let path = canyon.layout.shortcuts[k].clone();
            let kind = path.kind;
            let skill = canyon.skill_for(Some(kind));
            canyon.becomes_a_bot(skill);
            canyon.place(path.from - circuit.approach(k), 26.0);
            // Issue #193: it takes one only with a boost, fired just before it.
            let lead = lead.min(circuit.approach(k));
            let mut boost_at = Some((path.from - lead, canyon.boost_for(&path, lead)));
            let name = format!("{} (mirror {mirrored}), {kind:?} {k}", circuit.name);
            let mut on = false;
            for _ in 0..(20 * 60) {
                if let Some((at, seconds)) = boost_at
                    && canyon.progress().distance >= at
                {
                    canyon.boost(seconds);
                    boost_at = None;
                }
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
            let world = canyon.app.world();
            assert!(
                on,
                "{name}: the bot never got a third of the way on it (route {:?}, boost {:?}, at \
                 {:.0} m, its path from {:.0} m)",
                world.get::<BotBrain>(canyon.kart).unwrap().route,
                world.get::<Boost>(canyon.kart).unwrap(),
                canyon.progress().distance,
                path.from
            );
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
