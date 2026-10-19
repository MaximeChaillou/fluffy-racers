//! Issue #144 acceptance: headless scenarios of the hazards (the whole game, `GamePlugin` with
//! `MinimalPlugins`, one fixed tick per `app.update()`): on the Corniche Cannelle, the passer-by
//! and the moving obstacles knock a kart over the same way in two games of the same clock and
//! seed; the things to drive through burst, slow down a little at most, and come back; the bots'
//! lanes keep clear of what is in the way, and a bot laps without being knocked over. A circuit
//! without hazards has nothing of them in its world.

use bevy_rapier3d::prelude::{Collider, Velocity};

use super::hazard::{Hazard, HazardKind, clock_seconds, hazard_phase, kart_capsule};
use super::hazards::{Breakable, HazardBody, HazardSystemsInstalled, install_hazard_systems};
use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::{BotDifficulty, BotTuning, RacingLine, bot};
use crate::croquettes::{CroquetteTuning, Croquettes};
use crate::interpolation::PhysicsPose;
use crate::kart::{Boost, HitReaction, Invincibility, KartInput, KartState, PlayerKart};
use crate::race::{Course, Race, RaceGrid, RaceProgress, RaceTuning, Tow};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};

const CORNICHE: &str = "tracks/corniche.track.ron";
/// Issue #108: a circuit without any hazard.
const PLAIN_CIRCUIT: &str = "tracks/col.track.ron";

/// The game on the circuit file `path` (its mirror image or not), race seed `seed`, the player's
/// kart alone, its race started (the test drives it).
fn game_on(path: &str, mirrored: bool, seed: u64) -> (App, Entity) {
    let mut app = game_app_unloaded(TrackChoice::Circuit(path.into()), 0);
    app.world_mut().resource_mut::<RaceGrid>().seed = seed;
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

fn flow_tick(app: &App) -> u32 {
    app.world().resource::<Race>().flow.tick
}

/// The circuit's hazard of `kind` (the first one) and its entity.
fn hazard_of(app: &mut App, kind: HazardKind) -> (Entity, Hazard) {
    let layout = app.world().resource::<TrackLayout>().clone();
    let hazard = layout
        .hazards
        .iter()
        .find(|h| h.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} on {}", layout.name))
        .clone();
    let world = app.world_mut();
    let entity = world
        .query::<(Entity, &HazardBody)>()
        .iter(world)
        .find(|(_, body)| body.0 == hazard.index)
        .map(|(entity, _)| entity)
        .unwrap();
    (entity, hazard)
}

/// Puts the kart at `transform`, at `speed` along it, its race progress at centerline distance
/// `distance` (the first lap): on its way, not out of the track for the pelican.
fn place(app: &mut App, kart: Entity, transform: Transform, speed: f32, distance: f32) {
    let spacing = app.world().resource::<Course>().checkpoints[1];
    let world = app.world_mut();
    *world.get_mut::<Transform>(kart).unwrap() = transform;
    world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    *world.get_mut::<Velocity>(kart).unwrap() = Velocity {
        linear: *transform.forward() * speed,
        angular: Vec3::ZERO,
    };
    *world.get_mut::<KartState>(kart).unwrap() = KartState::Grounded;
    let mut progress = world.get_mut::<RaceProgress>(kart).unwrap();
    progress.passed = 1 + (distance / spacing).floor() as u32;
    progress.distance = distance;
    world
        .get_mut::<Tow>(kart)
        .unwrap()
        .reset(transform.translation);
}

/// Where a moving hazard will be on the next tick of `app`: the kart put there, under it (its
/// center at the grid's height over the road), is touched on that tick.
fn under_it_next_tick(app: &App, hazard: &Hazard, lift: f32) -> Transform {
    let seed = app.world().resource::<RaceGrid>().seed;
    let seconds = clock_seconds(flow_tick(app) + 1);
    let body = hazard.body(&hazard.pose(seconds, hazard_phase(seed, hazard.index)));
    let middle = (body.a + body.b) / 2.0;
    let up = hazard.road.up;
    let on_road = middle - up * (middle - hazard.road.position).dot(up);
    Transform::from_translation(on_road + up * (GRID_HEIGHT + lift))
        .looking_to(hazard.road.forward, up)
}

/// Issue #144: a circuit that declares no hazard (the Col des Moufles; Prairie Pompon until
/// issue #108 gave it some) has nothing of them: no entity, no component, no resource, no system
/// (the bot races of its world are those of before #144). The Corniche Cannelle has one entity
/// per hazard (a collider for a fixed one, a pose to interpolate for a moving one, its state for
/// a thing to drive through), and exactly three systems more, added once.
#[test]
fn only_a_circuit_with_hazards_has_them_in_its_world() {
    let (mut plain, _) = game_on(PLAIN_CIRCUIT, false, 0);
    let world = plain.world_mut();
    assert_eq!(world.query::<&HazardBody>().iter(world).count(), 0);
    assert_eq!(world.query::<&Breakable>().iter(world).count(), 0);
    assert!(world.get_resource::<HazardSystemsInstalled>().is_none());
    assert!(world.resource::<TrackLayout>().hazards.is_empty());
    let systems = |app: &App| {
        app.world()
            .resource::<Schedules>()
            .get(FixedUpdate)
            .unwrap()
            .systems_len()
    };
    let plain_systems = systems(&plain);

    let (mut canyon, _) = game_on(CORNICHE, false, 0);
    let layout = canyon.world().resource::<TrackLayout>().clone();
    assert!(
        canyon
            .world()
            .get_resource::<HazardSystemsInstalled>()
            .is_some()
    );
    assert_eq!(systems(&canyon), plain_systems + 3);
    // Asked again (another circuit with hazards): still once.
    install_hazard_systems(canyon.world_mut());
    canyon.update();
    assert_eq!(systems(&canyon), plain_systems + 3);
    let world = canyon.world_mut();
    let mut kinds = Vec::new();
    for (body, collider, pose, breakable, surface) in world
        .query::<(
            &HazardBody,
            Option<&Collider>,
            Option<&PhysicsPose>,
            Option<&Breakable>,
            Option<&Surface>,
        )>()
        .iter(world)
    {
        let hazard = &layout.hazards[usize::from(body.0)];
        kinds.push(hazard.kind);
        assert_eq!(
            collider.is_some(),
            hazard.kind.stands(),
            "{:?}",
            hazard.kind
        );
        assert_eq!(
            surface.copied(),
            hazard.kind.stands().then_some(Surface::Wall)
        );
        assert_eq!(pose.is_some(), hazard.kind.moves(), "{:?}", hazard.kind);
        assert_eq!(
            breakable.is_some(),
            hazard.kind.bursts(),
            "{:?}",
            hazard.kind
        );
    }
    assert_eq!(kinds.len(), layout.hazards.len());
    // The reference circuit has one of each: obstacles fixed and moving, a passer-by, things
    // to drive through.
    use super::data::{BreakableKind, NpcKind, ObstacleKind};
    for kind in [
        HazardKind::Obstacle(ObstacleKind::Rock),
        HazardKind::Obstacle(ObstacleKind::Trunk),
        HazardKind::Obstacle(ObstacleKind::Pillar),
        HazardKind::Obstacle(ObstacleKind::RollingLog),
        HazardKind::Obstacle(ObstacleKind::SwingingStone),
        HazardKind::Npc(NpcKind::Bison),
        HazardKind::Breakable(BreakableKind::HayBale),
        HazardKind::Breakable(BreakableKind::Balloon),
        HazardKind::Breakable(BreakableKind::Crate),
    ] {
        assert!(
            kinds.contains(&kind),
            "no {kind:?} on the Corniche Cannelle"
        );
    }
}

/// Issue #144 acceptance: the bison knocks a kart over the same way in two games on the same
/// clock with the same seed (two peers: each moves the hazards from the race clock and the
/// race's seed, and judges its own karts). The bison walks the same way in both, tick for tick
/// (another seed: elsewhere); a kart standing on its way, in both games, is knocked over on the
/// same tick: a tumble, its boost gone, the same speed after, `knock_loss` croquettes lost.
#[test]
fn the_bison_knocks_a_kart_over_the_same_way_on_two_peers() {
    use super::data::NpcKind;
    let (mut ana, ana_kart) = game_on(CORNICHE, false, 7);
    let (mut bob, bob_kart) = game_on(CORNICHE, false, 7);
    let (mut cyd, _) = game_on(CORNICHE, false, 8);
    assert_eq!(flow_tick(&ana), flow_tick(&bob));
    assert_eq!(flow_tick(&ana), flow_tick(&cyd));
    let kind = HazardKind::Npc(NpcKind::Bison);
    let (ana_bison, bison) = hazard_of(&mut ana, kind);
    let (bob_bison, _) = hazard_of(&mut bob, kind);
    let (cyd_bison, _) = hazard_of(&mut cyd, kind);
    let at = |app: &App, entity: Entity| app.world().get::<Transform>(entity).unwrap().translation;
    let mut apart = 0.0f32;
    for _ in 0..120 {
        for app in [&mut ana, &mut bob, &mut cyd] {
            app.update();
        }
        assert_eq!(at(&ana, ana_bison), at(&bob, bob_bison));
        apart = apart.max(at(&ana, ana_bison).distance(at(&cyd, cyd_bison)));
    }
    assert!(
        apart > 0.5,
        "another seed, another walk ({apart} m apart at most)"
    );

    // Each kart standing in the middle of the bison's walk, the same way, 5 croquettes.
    let standing = bison.road.transform(bison.lateral, GRID_HEIGHT);
    for (app, kart) in [(&mut ana, ana_kart), (&mut bob, bob_kart)] {
        place(app, kart, standing, 0.0, bison.road.distance);
        app.world_mut().get_mut::<Croquettes>(kart).unwrap().count = 5;
    }
    let knocked = |app: &App, kart: Entity| app.world().get::<KartState>(kart).unwrap().is_hit();
    let mut ticks = Vec::new();
    for _ in 0..(8 * 60) {
        ana.update();
        bob.update();
        let (a, b) = (knocked(&ana, ana_kart), knocked(&bob, bob_kart));
        assert_eq!(a, b, "knocked over on the same tick");
        if a {
            ticks.push(flow_tick(&ana));
            break;
        }
    }
    assert_eq!(ticks.len(), 1, "the bison never came by in 8 s");
    let world = ana.world();
    assert!(matches!(
        world.get::<KartState>(ana_kart),
        Some(KartState::Hit {
            reaction: HitReaction::Tumble,
            ..
        })
    ));
    assert_eq!(world.get::<Boost>(ana_kart), Some(&Boost::default()));
    let (va, vb) = (
        world.get::<Velocity>(ana_kart).unwrap().linear,
        bob.world().get::<Velocity>(bob_kart).unwrap().linear,
    );
    assert_eq!(va, vb, "the same push in both games");
    assert!(va.length() > 1.0, "pushed away from the bison: {va}");
    // The croquettes go on the next ticks (the croquettes' rules).
    ana.update();
    let loss = CroquetteTuning::default().knock_loss;
    assert_eq!(
        ana.world().get::<Croquettes>(ana_kart).unwrap().count,
        5 - loss
    );
}

/// Issue #144: the moving obstacles knock a kart over too (the rolling log, the swinging stone),
/// the victim deciding: not a kart invincible after a hit, nor one in the air over it, nor one
/// tumbling already; a kart that touches a fixed obstacle bumps into it (a wall: no tumble).
#[test]
fn the_moving_obstacles_knock_over_a_kart_that_can_be() {
    use super::data::ObstacleKind;
    let (mut app, kart) = game_on(CORNICHE, false, 3);
    for kind in [ObstacleKind::RollingLog, ObstacleKind::SwingingStone] {
        let (_, hazard) = hazard_of(&mut app, HazardKind::Obstacle(kind));
        let knocked_now = |app: &mut App, lift: f32, invincible: bool| {
            let at = under_it_next_tick(app, &hazard, lift);
            place(app, kart, at, 0.0, hazard.road.distance);
            let world = app.world_mut();
            let mut invincibility = world.get_mut::<Invincibility>(kart).unwrap();
            *invincibility = Invincibility::default();
            if invincible {
                invincibility.start(2.0);
            }
            app.update();
            app.world().get::<KartState>(kart).unwrap().is_hit()
        };
        assert!(!knocked_now(&mut app, 0.0, true), "{kind:?}: invincible");
        assert!(
            !knocked_now(&mut app, 6.0, false),
            "{kind:?}: in the air over it"
        );
        assert!(knocked_now(&mut app, 0.0, false), "{kind:?}: knocked over");
        // Tumbling already: its tumble goes on (not knocked again from the start).
        let before = *app.world().get::<KartState>(kart).unwrap();
        app.update();
        let after = *app.world().get::<KartState>(kart).unwrap();
        match (before, after) {
            (KartState::Hit { elapsed: a, .. }, KartState::Hit { elapsed: b, .. }) => {
                assert!(b > a, "{kind:?}: knocked again")
            }
            _ => panic!("{kind:?}: {before:?} then {after:?}"),
        }
    }
    // A rock: driven into at 15 m/s, the kart bounces off (no tumble).
    let (_, rock) = hazard_of(&mut app, HazardKind::Obstacle(ObstacleKind::Rock));
    let toward = rock.road.forward;
    let start = rock.road.point(rock.lateral, GRID_HEIGHT) - toward * 8.0;
    place(
        &mut app,
        kart,
        Transform::from_translation(start).looking_to(toward, rock.road.up),
        15.0,
        rock.road.distance - 8.0,
    );
    *app.world_mut().get_mut::<Invincibility>(kart).unwrap() = Invincibility::default();
    let mut slowest = f32::MAX;
    for _ in 0..90 {
        app.update();
        assert!(!app.world().get::<KartState>(kart).unwrap().is_hit());
        let speed = app
            .world()
            .get::<Velocity>(kart)
            .unwrap()
            .linear
            .dot(toward);
        slowest = slowest.min(speed);
    }
    assert!(
        slowest < 2.0,
        "it went through the rock ({slowest} m/s at least)"
    );
    let passed = app.world().get::<Transform>(kart).unwrap().translation;
    let center = rock.road.point(rock.lateral, 0.0);
    assert!(
        (passed - center).dot(toward) < 0.0,
        "it never passed the rock"
    );
}

/// Issue #144 acceptance: a thing to drive through bursts under a kart, slows it down a little at
/// most (a hay bale: 8 %; a balloon: not at all), and comes back after `respawn_seconds`.
#[test]
fn a_thing_to_drive_through_bursts_slows_a_little_and_comes_back() {
    use super::data::BreakableKind;
    let (mut app, kart) = game_on(CORNICHE, false, 0);
    let tuning = app.world().resource::<RaceTuning>().hazards.clone();
    for (kind, slowdown) in [
        (BreakableKind::HayBale, tuning.hay_slowdown),
        (BreakableKind::Balloon, tuning.balloon_slowdown),
    ] {
        let (entity, thing) = hazard_of(&mut app, HazardKind::Breakable(kind));
        let forward = thing.road.forward;
        let start = thing.road.point(thing.lateral, GRID_HEIGHT) - forward * 10.0;
        place(
            &mut app,
            kart,
            Transform::from_translation(start).looking_to(forward, thing.road.up),
            22.0,
            thing.road.distance - 10.0,
        );
        app.world_mut().get_mut::<KartInput>(kart).unwrap().throttle = 1.0;
        let speed = |app: &App| app.world().get::<Velocity>(kart).unwrap().linear.length();
        let mut burst = None;
        for _ in 0..60 {
            let before = speed(&app);
            app.update();
            let breakable = *app.world().get::<Breakable>(entity).unwrap();
            if breakable.burst_by == Some(kart) {
                burst = Some((before, speed(&app)));
                break;
            }
        }
        let (before, after) = burst.unwrap_or_else(|| panic!("{kind:?} never burst"));
        assert!(!app.world().get::<KartState>(kart).unwrap().is_hit());
        let lost = 1.0 - after / before;
        assert!(
            lost <= slowdown + 0.02 && lost >= slowdown - 0.02,
            "{kind:?}: {lost} of its speed lost ({before} → {after})"
        );
        assert!(lost <= 0.1, "a little at most");
        let breakable = *app.world().get::<Breakable>(entity).unwrap();
        assert_eq!(breakable.bursts, 1);
        assert!(!breakable.available());
        // Gone: the kart drives on, nothing more bursts; then it comes back.
        let ticks = crate::race::seconds_to_ticks(tuning.respawn_seconds);
        for tick in 1..=ticks + 1 {
            app.update();
            let breakable = *app.world().get::<Breakable>(entity).unwrap();
            assert_eq!(breakable.bursts, 1);
            assert_eq!(
                breakable.available(),
                tick >= ticks,
                "{kind:?} at tick {tick}"
            );
        }
        app.world_mut().get_mut::<KartInput>(kart).unwrap().throttle = 0.0;
    }
}

/// Issue #144: the bots' lanes keep clear of the obstacles and the passers-by (their whole
/// motion, `BotTuning::hazard_margin` more), on the Corniche Cannelle and its mirror; the things to
/// drive through are not in their way.
#[test]
fn the_bots_lanes_keep_clear_of_what_is_in_the_way() {
    for mirrored in [false, true] {
        let (app, _) = game_on(CORNICHE, mirrored, 0);
        let layout = app.world().resource::<TrackLayout>();
        let line = app.world().resource::<RacingLine>();
        let tuning = app.world().resource::<BotTuning>();
        let center = &layout.center;
        let mut checked = 0;
        for hazard in layout.hazards.iter().filter(|h| h.kind.in_the_way()) {
            let footprint = hazard.footprint();
            for (i, s) in center.samples().iter().enumerate() {
                let forward = s.forward.xz().normalize();
                let right = Vec2::new(-forward.y, forward.x);
                let along: Vec<f32> = footprint
                    .iter()
                    .map(|c| (*c - s.position.xz()).dot(forward))
                    .collect();
                let across: Vec<f32> = footprint
                    .iter()
                    .map(|c| (*c - s.position.xz()).dot(right))
                    .collect();
                let min = |v: &[f32]| v.iter().cloned().fold(f32::MAX, f32::min);
                let max = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max);
                // Alongside it, and while the kart comes by.
                if min(&along) > 0.0 || max(&along) < -tuning.hazard_after.min(10.0) {
                    continue;
                }
                if s.position.xz().distance(hazard.road.position.xz()) > 40.0 {
                    continue;
                }
                let [low, high] = line.limits(i);
                let (left, right) = (min(&across), max(&across));
                let clear = high <= left - tuning.hazard_margin + 0.05
                    || low >= right + tuning.hazard_margin - 0.05;
                assert!(
                    clear,
                    "mirrored {mirrored}, {:?} at sample {i}: lanes {low:.2}..{high:.2}, it covers \
                     {left:.2}..{right:.2}",
                    hazard.kind
                );
                checked += 1;
            }
        }
        assert!(checked > 30, "{checked} samples checked");
    }
}

/// Issue #144: a Normal bot laps the Corniche Cannelle (and its mirror) twice, alone, without
/// being knocked over: its lanes keep clear of the bison, the log and the stone.
#[test]
fn a_bot_laps_the_corniche_without_being_knocked_over() {
    for mirrored in [false, true] {
        let (mut app, kart) = game_on(CORNICHE, mirrored, 0);
        let tuning = app.world().resource::<BotTuning>().clone();
        app.world_mut()
            .entity_mut(kart)
            .insert(bot(BotDifficulty::Normal, 11, &tuning));
        let mut knocks = 0;
        let mut was_hit = false;
        for _ in 0..(110 * 60) {
            app.update();
            let hit = app.world().get::<KartState>(kart).unwrap().is_hit();
            if hit && !was_hit {
                knocks += 1;
            }
            was_hit = hit;
            if app
                .world()
                .get::<RaceProgress>(kart)
                .is_some_and(|p| p.lap_times.len() >= 2)
            {
                break;
            }
        }
        let laps = app
            .world()
            .get::<RaceProgress>(kart)
            .unwrap()
            .lap_times
            .len();
        assert!(laps >= 2, "mirrored {mirrored}: {laps} laps in 110 s");
        assert_eq!(
            knocks, 0,
            "mirrored {mirrored}: knocked over {knocks} times"
        );
    }
}

/// The kart's contact shape is its box's (a quick check of the capsule against its extents).
#[test]
fn the_kart_s_contact_shape_is_its_size() {
    let shape = kart_capsule(&Transform::IDENTITY);
    assert!((shape.a.distance(shape.b) + 2.0 * shape.radius - 2.0).abs() < 1e-5);
    assert!((shape.radius - 0.7).abs() < 1e-5);
}
