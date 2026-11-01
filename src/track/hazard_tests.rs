//! Issue #144 acceptance: headless scenarios of the hazards (the whole game, `GamePlugin` with
//! `MinimalPlugins`, one fixed tick per `app.update()`): on the Corniche Cannelle, the passer-by
//! and the moving obstacles knock a kart over the same way in two games of the same clock and
//! seed; the things to drive through burst, slow down a little at most, and come back; the bots'
//! lanes keep clear of what is in the way, and a bot laps without being knocked over. A circuit
//! without hazards has nothing of them in its world.

use bevy_rapier3d::prelude::{Collider, Velocity};

use super::hazard::{Hazard, HazardKind, clock_seconds, hazard_phase, kart_capsule};
use super::hazards::{
    Breakable, HazardBody, HazardSystemsInstalled, Panel, PanelSystemsInstalled,
    install_hazard_systems, install_panel_systems,
};
use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::{BotDifficulty, BotTuning, RacingLine, bot};
use crate::croquettes::{CroquetteTuning, Croquettes};
use crate::interpolation::PhysicsPose;
use crate::kart::{Boost, HitReaction, Invincibility, KartInput, KartState, PlayerKart};
use crate::race::{Course, Race, RaceGrid, RaceProgress, RaceTuning, Tow};
use crate::tests::{game_app_unloaded, player, single_threaded, wait_for_the_player};

const CORNICHE: &str = "tracks/corniche.track.ron";
/// Issue #108: a circuit without any hazard (issue #109: the Crique Câline, the Col des Moufles
/// has some now; issue #148: the Lagon Duvet, the Crique Câline redrawn has some; issue #150: the
/// Ruban Réglisse, the Lagon Duvet redrawn has some).
const PLAIN_CIRCUIT: &str = "tracks/ruban.track.ron";

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

/// Issue #144: a circuit that declares no hazard (the Lagon Duvet; Prairie Pompon until issue
/// #108 gave it some, the Col des Moufles until #109, the Crique Câline until #148) has nothing of them: no entity, no
/// component, no resource, no system (the bot races of its world are those of before #144). The
/// Corniche Cannelle has one entity per hazard (a collider for a fixed one, a pose to interpolate
/// for a moving one, its state for a thing to drive through), and exactly three systems more,
/// added once. Issue #156: and one more for its panel (a collider, its state, the look of a thing
/// that breaks), added once too; a circuit with hazards but no panel (Prairie Pompon) has only
/// the three of #144.
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
    let (prairie, _) = game_on("tracks/prairie.track.ron", false, 0);
    assert!(
        prairie
            .world()
            .get_resource::<HazardSystemsInstalled>()
            .is_some()
    );
    assert!(
        prairie
            .world()
            .get_resource::<PanelSystemsInstalled>()
            .is_none()
    );

    let (mut canyon, _) = game_on(CORNICHE, false, 0);
    let layout = canyon.world().resource::<TrackLayout>().clone();
    assert!(
        canyon
            .world()
            .get_resource::<HazardSystemsInstalled>()
            .is_some()
    );
    assert!(
        canyon
            .world()
            .get_resource::<PanelSystemsInstalled>()
            .is_some()
    );
    // Issue #156: the panels' system, and the sync point after it (it turns its collider off and
    // on with commands).
    assert_eq!(systems(&canyon), plain_systems + 5);
    // Asked again (another circuit with hazards): still once.
    install_hazard_systems(canyon.world_mut());
    install_panel_systems(canyon.world_mut());
    canyon.update();
    assert_eq!(systems(&canyon), plain_systems + 5);
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
        let solid = hazard.kind.stands() || hazard.kind.closes();
        assert_eq!(collider.is_some(), solid, "{:?}", hazard.kind);
        assert_eq!(surface.copied(), solid.then_some(Surface::Wall));
        assert_eq!(pose.is_some(), hazard.kind.moves(), "{:?}", hazard.kind);
        assert_eq!(
            breakable.is_some(),
            hazard.kind.breaks(),
            "{:?}",
            hazard.kind
        );
    }
    assert_eq!(kinds.len(), layout.hazards.len());
    // The reference circuit has one of each: obstacles fixed and moving, a passer-by, things
    // to drive through.
    use super::data::{BreakableKind, CoverKind, NpcKind, ObstacleKind};
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
        HazardKind::Cover(CoverKind::Panel),
        HazardKind::Cover(CoverKind::Foliage),
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

/// Issue #156: the circuit's panel (the grass bridge's on the Corniche Cannelle): its entity, its
/// hazard, and the shortcut it closes.
fn panel_of(app: &mut App) -> (Entity, Hazard, Shortcut) {
    use super::data::CoverKind;
    let (entity, hazard) = hazard_of(app, HazardKind::Cover(CoverKind::Panel));
    let layout = app.world().resource::<TrackLayout>();
    let path = layout
        .shortcuts
        .iter()
        .find(|s| s.closed())
        .expect("a shortcut closed by a panel")
        .clone();
    (entity, hazard, path)
}

/// Issue #156: puts the kart on `path`, `before` meters before its panel, heading along it at
/// `speed` (its race progress at the shortcut's start).
fn on_the_path(app: &mut App, kart: Entity, path: &Shortcut, before: f32, speed: f32) {
    let cover = path.cover.unwrap();
    let s = path.at(cover.at - before);
    let transform =
        Transform::from_translation(s.point(0.0, GRID_HEIGHT)).looking_to(s.forward, s.up);
    place(app, kart, transform, speed, path.from);
}

/// Issue #156: full throttle, steering along `path` (to its point 8 m ahead of the kart).
fn along_the_path(app: &mut App, kart: Entity, path: &Shortcut) {
    let transform = *app.world().get::<Transform>(kart).unwrap();
    let here = path.locate(transform.translation);
    let to = path.at(here.along.max(0.0) + 8.0).position - transform.translation;
    let angle = to
        .dot(*transform.right())
        .atan2(to.dot(*transform.forward()));
    let mut input = app.world_mut().get_mut::<KartInput>(kart).unwrap();
    input.throttle = 1.0;
    input.steer = (angle * 3.0).clamp(-1.0, 1.0);
}

fn panel_state(app: &App, entity: Entity) -> (Panel, Breakable, bool) {
    let world = app.world();
    (
        *world.get::<Panel>(entity).unwrap(),
        *world.get::<Breakable>(entity).unwrap(),
        world
            .get::<bevy_rapier3d::prelude::ColliderDisabled>(entity)
            .is_none(),
    )
}

/// Issue #156 acceptance (« le panneau : solide sans boost, cassé avec un boost, sa
/// réapparition »), on the Corniche Cannelle and its mirror: a kart without a boost that drives
/// into the grass bridge's panel flat out (from 14 m before it at 24 m/s, steering along the
/// path) never gets past it: it
/// bounces gently (back at 5 m/s at most, a wall sends it back at about 7), stays on its wheels,
/// is not knocked over nor towed; the panel stands, solid. The same kart with a boost (a
/// mini-turbo) breaks it before touching it: it drives through at the same speed (95 % at least),
/// the panel down (no collider, burst once by it, splinters), and the panel stands again
/// `panel_respawn_seconds` later, on the tick its break says, solid.
#[test]
fn a_panel_is_solid_without_a_boost_and_breaks_with_one() {
    for mirrored in [false, true] {
        let (mut app, kart) = game_on(CORNICHE, mirrored, 0);
        let (entity, _, path) = panel_of(&mut app);
        let cover = path.cover.unwrap();
        let tuning = app.world().resource::<RaceTuning>().hazards.clone();
        assert!(tuning.panel_bounce < crate::track::WALL_RESTITUTION);
        on_the_path(&mut app, kart, &path, 14.0, 24.0);
        let (mut furthest, mut back, mut aside, mut lowest_up) = (f32::MIN, 0.0f32, 0.0f32, 1.0f32);
        for _ in 0..150 {
            along_the_path(&mut app, kart, &path);
            app.update();
            let world = app.world();
            let transform = world.get::<Transform>(kart).unwrap();
            let velocity = world.get::<Velocity>(kart).unwrap().linear;
            let here = path.locate(transform.translation);
            let s = path.at(here.along);
            furthest = furthest.max(here.along);
            back = back.max(-velocity.dot(s.forward));
            aside = aside.max(velocity.dot(s.right).abs());
            lowest_up = lowest_up.min(transform.up().dot(s.up));
            assert!(!world.get::<KartState>(kart).unwrap().is_hit());
            assert!(
                world.get::<Tow>(kart).unwrap().carry.is_none(),
                "towed: {:?} at {here:?}, {velocity}",
                world.get::<Tow>(kart).unwrap().carry
            );
        }
        let (panel, breakable, solid) = panel_state(&app, entity);
        println!(
            "mirror {mirrored}: without a boost, {:.2} m before the panel at most, back at {back:.1} \
             m/s, aside {aside:.1} m/s, up {lowest_up:.3}",
            cover.at - furthest
        );
        assert!(
            furthest < cover.at - 0.9,
            "mirror {mirrored}: past the panel ({furthest:.2} m along, the panel at {})",
            cover.at
        );
        assert!(furthest > cover.at - 2.0, "it reached the panel");
        assert!(
            back <= 5.0,
            "mirror {mirrored}: thrown back at {back:.1} m/s"
        );
        assert!(
            aside <= 3.0,
            "mirror {mirrored}: thrown aside at {aside:.1} m/s"
        );
        assert!(
            lowest_up > 0.9,
            "mirror {mirrored}: tipped ({lowest_up:.3})"
        );
        assert_eq!((panel.down_until, breakable.bursts, solid), (0, 0, true));

        // With a boost: it breaks before the kart touches it.
        on_the_path(&mut app, kart, &path, 14.0, 24.0);
        let strength = app
            .world()
            .resource::<crate::kart::KartTuning>()
            .mini_turbo_strengths[1];
        app.world_mut()
            .get_mut::<Boost>(kart)
            .unwrap()
            .add(strength, 1.5);
        let speed = |app: &App| {
            let world = app.world();
            let transform = world.get::<Transform>(kart).unwrap();
            world
                .get::<Velocity>(kart)
                .unwrap()
                .linear
                .dot(*transform.forward())
        };
        let mut broke = None;
        for _ in 0..90 {
            let before = speed(&app);
            along_the_path(&mut app, kart, &path);
            app.update();
            let (panel, breakable, solid) = panel_state(&app, entity);
            if breakable.burst_by == Some(kart) {
                assert!(broke.is_none(), "broken twice");
                broke = Some((flow_tick(&app), before));
                assert!(panel.down_until > flow_tick(&app));
                assert_eq!(breakable.bursts, 1);
            }
            if broke.is_some() {
                app.update();
                assert!(!panel_state(&app, entity).2, "no collider while it is down");
            }
            let here = path.locate(app.world().get::<Transform>(kart).unwrap().translation);
            if here.along > cover.at + 6.0 {
                break;
            }
            let _ = solid;
        }
        let (tick, before) = broke.expect("the boosted kart never broke the panel");
        let here = path.locate(app.world().get::<Transform>(kart).unwrap().translation);
        assert!(
            here.along > cover.at + 6.0,
            "mirror {mirrored}: it never got through"
        );
        assert!(
            speed(&app) >= 0.95 * before,
            "mirror {mirrored}: {:.1} m/s after, {before:.1} before",
            speed(&app)
        );
        // It stands again on the tick its break says, solid (the kart is far).
        let down_for = super::hazards::panel_down_ticks(&tuning);
        let (panel, ..) = panel_state(&app, entity);
        assert_eq!(panel.down_until, tick + down_for);
        *app.world_mut().get_mut::<KartInput>(kart).unwrap() = KartInput::default();
        while flow_tick(&app) < tick + down_for {
            assert!(!panel_state(&app, entity).0.solid);
            app.update();
        }
        app.update();
        let (panel, breakable, solid) = panel_state(&app, entity);
        assert!(panel.solid && solid && breakable.available());
        assert_eq!(breakable.bursts, 1);
    }
}

/// Issue #156: a panel that stands again with a kart in it never throws it out: its collider
/// stays off until the kart is out of it, then it is solid.
#[test]
fn a_panel_back_with_a_kart_in_it_lets_it_out_first() {
    let (mut app, kart) = game_on(CORNICHE, false, 0);
    let (entity, _, path) = panel_of(&mut app);
    // Down: a boosted kart broke it a moment ago; ours stands right in it.
    let now = flow_tick(&app);
    app.world_mut().get_mut::<Panel>(entity).unwrap().down_until = now + 10;
    on_the_path(&mut app, kart, &path, 0.0, 0.0);
    for _ in 0..40 {
        app.update();
        let velocity = app.world().get::<Velocity>(kart).unwrap().linear;
        assert!(velocity.length() < 1.5, "thrown at {velocity}");
        assert!(!panel_state(&app, entity).2, "solid with the kart in it");
    }
    // It drives out: then the panel is solid.
    let mut solid = false;
    for _ in 0..120 {
        along_the_path(&mut app, kart, &path);
        app.update();
        if panel_state(&app, entity).2 {
            solid = true;
            break;
        }
    }
    assert!(solid, "never solid again");
    let cover = path.cover.unwrap();
    let here = path.locate(app.world().get::<Transform>(kart).unwrap().translation);
    assert!(here.along > cover.at + 1.0, "out of it, past it: {here:?}");
}

/// Issue #110 acceptance (« une pile de caisses qui cache l'entrée et ralentit un peu »): the
/// Falaises Flanelle's hidden road (and its mirror) behind a stack of crates: a kart that drives
/// the path flat out bursts it on its way (no tumble, a crate's slowdown, 4 %, a little at most:
/// it never stops it), drives on through it, nothing more bursts while it is gone, and it comes
/// back `cover_respawn_seconds` later (issue #193: 1.75 s; `respawn_seconds`, 5 s, before). The stack is no wall: no collider; and it is not in the bots'
/// way (on the path, off the road).
#[test]
fn a_stack_of_crates_bursts_slows_a_little_and_comes_back() {
    use super::data::CoverKind;
    for mirrored in [false, true] {
        let (mut app, kart) = game_on("tracks/falaises.track.ron", mirrored, 0);
        let tuning = app.world().resource::<RaceTuning>().hazards.clone();
        let kind = HazardKind::Cover(CoverKind::Crates);
        let (entity, hazard) = hazard_of(&mut app, kind);
        assert!(kind.bursts() && kind.breaks() && !kind.in_the_way() && !kind.closes());
        assert!(app.world().get::<Collider>(entity).is_none());
        let path = app
            .world()
            .resource::<TrackLayout>()
            .shortcuts
            .iter()
            .find(|s| s.cover.is_some_and(|c| c.kind == CoverKind::Crates))
            .expect("a shortcut behind a stack of crates")
            .clone();
        assert!(
            hazard
                .road
                .position
                .distance(path.at(path.cover.unwrap().at).position)
                < 1e-3
        );
        on_the_path(&mut app, kart, &path, 12.0, 22.0);
        let speed = |app: &App| app.world().get::<Velocity>(kart).unwrap().linear.length();
        let mut burst = None;
        for _ in 0..60 {
            along_the_path(&mut app, kart, &path);
            let before = speed(&app);
            app.update();
            if app.world().get::<Breakable>(entity).unwrap().burst_by == Some(kart) {
                burst = Some((before, speed(&app)));
                break;
            }
        }
        let (before, after) = burst.expect("the stack of crates never burst");
        assert!(!app.world().get::<KartState>(kart).unwrap().is_hit());
        let lost = 1.0 - after / before;
        assert!(
            (lost - tuning.crate_slowdown).abs() <= 0.02,
            "mirror {mirrored}: {lost} of its speed lost ({before} → {after})"
        );
        // Through it and on along the path, while it is gone; then it comes back (issue #193:
        // sooner than the crates on the road).
        assert!(tuning.cover_respawn_seconds < tuning.respawn_seconds);
        let ticks = crate::race::seconds_to_ticks(tuning.cover_respawn_seconds);
        for tick in 1..=ticks + 1 {
            if tick < 60 {
                along_the_path(&mut app, kart, &path);
            } else {
                app.world_mut().get_mut::<KartInput>(kart).unwrap().throttle = 0.0;
            }
            app.update();
            let breakable = *app.world().get::<Breakable>(entity).unwrap();
            assert_eq!(breakable.bursts, 1, "mirror {mirrored}");
            assert_eq!(breakable.available(), tick >= ticks, "at tick {tick}");
            if tick == 30 {
                let at = app.world().get::<Transform>(kart).unwrap().translation;
                let here = path.locate(at);
                assert!(
                    here.along > path.cover.unwrap().at + 2.0,
                    "mirror {mirrored}: through the stack ({here:?})"
                );
            }
        }
    }
}
