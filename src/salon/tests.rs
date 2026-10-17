//! Headless scenarios of the lobby in 3D (issue #101), out of a room: the whole game
//! (`GamePlugin`) with `MinimalPlugins`, one fixed tick per update, [`SalonPlugin`] added by hand
//! (the game adds it with its menu, with a renderer), the lobby's level asked as `net` asks it
//! (`LoadLevel` on `TrackChoice::Salon`). The network's scenarios are in `net::salon_tests`.

use bevy_rapier3d::prelude::Velocity;

use super::walker::WalkerLook;
use super::*;
use crate::characters::{PlayerLoadout, Ragdoll, RagdollPhase, RagdollTuning, Roster};
use crate::level::LoadLevel;
use crate::net::PeerKey;
use crate::race::RaceMode;
use crate::tests::{game_app_unloaded, hold_keys, wait_for_the_player};
use crate::track::{TrackChoice, TrackLayout};

/// The game on the test ground, then the lobby's level, built, our character in it.
fn salon_app() -> App {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(SalonPlugin);
    wait_for_the_player(&mut app);
    app.world_mut()
        .write_message(LoadLevel::on(TrackChoice::Salon, RaceMode::FreeRoam));
    for _ in 0..30 {
        app.update();
        if count::<With<PlayerWalker>>(&mut app) == 1 {
            // A few ticks to stand on the lawn.
            run(&mut app, 30);
            return app;
        }
    }
    panic!("the lobby was never built");
}

fn run(app: &mut App, ticks: usize) {
    for _ in 0..ticks {
        app.update();
    }
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

fn ours(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<PlayerWalker>>()
        .single(world)
        .unwrap()
}

fn position(app: &App, entity: Entity) -> Vec3 {
    app.world().get::<Transform>(entity).unwrap().translation
}

/// Issue #101: the lobby is its own level: the clearing (its lawn, its fence, three giant
/// donuts), no kart, no race, no track; our character stands on its seat's spot, facing the
/// middle, with its visual (primitives headless: a block and its hat) and its look; nothing of
/// the lobby is in the world before.
#[test]
fn the_lobby_is_its_own_scene_with_our_character_on_foot() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(SalonPlugin);
    wait_for_the_player(&mut app);
    assert_eq!(count::<With<SalonRoot>>(&mut app), 0);
    assert_eq!(count::<With<Walker>>(&mut app), 0);
    let mut app = salon_app();
    assert_eq!(count::<With<SalonRoot>>(&mut app), 1);
    assert_eq!(count::<With<Bouncer>>(&mut app), 3);
    assert_eq!(count::<With<crate::kart::Kart>>(&mut app), 0, "no kart");
    assert!(app.world().get_resource::<crate::race::Race>().is_none());
    assert!(app.world().get_resource::<TrackLayout>().is_none());
    assert_eq!(*app.world().resource::<TrackChoice>(), TrackChoice::Salon);
    let body = ours(&mut app);
    let at = position(&app, body);
    let (spot, heading) = seat_spot(0);
    assert!(
        Vec2::new(at.x - spot.x, at.z - spot.z).length() < 0.05,
        "{at} vs {spot}"
    );
    // Standing on the lawn: its bottom on the ground.
    assert!((at.y - 0.49).abs() < 0.08, "{at}");
    let control = *app.world().get::<WalkerControl>(body).unwrap();
    assert!(control.grounded);
    assert!((control.heading - heading).abs() < 1e-4);
    let walker = app.world().get::<Walker>(body).unwrap().clone();
    assert_eq!(
        walker.character,
        app.world().resource::<PlayerLoadout>().character
    );
    assert!(
        !walker.name.is_empty(),
        "its name: the animal's without a pseudo"
    );
    assert_eq!(count::<With<WalkerVisual>>(&mut app), 1);
    assert_eq!(
        count::<With<WalkerFigure>>(&mut app),
        1,
        "dressed (primitives)"
    );
    assert_eq!(count::<With<WalkerHat>>(&mut app), 1);
}

/// Issue #101: our character walks with the race's keys (up goes away from the camera, toward
/// -Z; right toward +X), at its walking speed, turns toward where it goes, stops when let go,
/// jumps with Espace (only from the ground) and lands.
#[test]
fn our_character_walks_turns_and_jumps_with_the_race_keys() {
    let mut app = salon_app();
    let tuning = app.world().resource::<SalonTuning>().clone();
    let body = ours(&mut app);
    let start = position(&app, body);
    hold_keys(&mut app, &[KeyCode::ArrowUp]);
    run(&mut app, 60);
    let after = position(&app, body);
    let speed = app.world().get::<Velocity>(body).unwrap().linear;
    assert!(after.z < start.z - 3.5, "{start} → {after}");
    assert!((after.x - start.x).abs() < 0.1);
    assert!((speed.length() - tuning.walk_speed).abs() < 0.2, "{speed}");
    hold_keys(&mut app, &[KeyCode::ArrowRight]);
    run(&mut app, 40);
    let heading = app.world().get::<WalkerControl>(body).unwrap().heading;
    assert!(
        (heading + std::f32::consts::FRAC_PI_2).abs() < 0.05,
        "it faces +X: {heading}"
    );
    assert!(app.world().get::<Velocity>(body).unwrap().linear.x > 4.0);
    hold_keys(&mut app, &[]);
    run(&mut app, 20);
    assert!(app.world().get::<Velocity>(body).unwrap().linear.length() < 0.05);
    // The jump: up, then back on the lawn.
    let ground = position(&app, body).y;
    hold_keys(&mut app, &[KeyCode::Space]);
    let mut top = ground;
    for _ in 0..90 {
        app.update();
        top = top.max(position(&app, body).y);
    }
    hold_keys(&mut app, &[]);
    run(&mut app, 30);
    assert!(top > ground + 0.6, "a jump: {ground} → {top}");
    assert!(
        (position(&app, body).y - ground).abs() < 0.05,
        "landed (held, it jumps once)"
    );
    // AZERTY (the default): Z goes up too; Q left.
    let before = position(&app, body);
    hold_keys(&mut app, &[KeyCode::KeyZ, KeyCode::KeyQ]);
    run(&mut app, 30);
    let moved = position(&app, body) - before;
    assert!(moved.z < -0.5 && moved.x < -0.5, "{moved}");
}

/// Issue #101 (« en solo »): the paw of ours knocks over a character simulated here, in front of
/// it: it tumbles (its rotation free, thrown along the push, up), then gets up on its own,
/// upright again, protected for a moment; a paw in the air does nothing; the recharge holds the
/// next swing.
#[test]
fn a_paw_knocks_a_character_over_and_it_gets_up_on_its_own() {
    let mut app = salon_app();
    let tuning = app.world().resource::<SalonTuning>().clone();
    let ragdoll_tuning = app.world().resource::<RagdollTuning>().clone();
    let body = ours(&mut app);
    let at = position(&app, body);
    let heading = app.world().get::<WalkerControl>(body).unwrap().heading;
    let front = super::walk::front_of(heading);
    // A swing in the air: a swing, no strike.
    hold_keys(&mut app, &[KeyCode::KeyX]);
    run(&mut app, 2);
    hold_keys(&mut app, &[]);
    run(&mut app, 60);
    let control = *app.world().get::<WalkerControl>(body).unwrap();
    assert_eq!((control.swings, control.strikes), (1, 0));
    // Another character, simulated here, in front of ours.
    let size = 1.0;
    let other = app
        .world_mut()
        .spawn((
            Walker {
                peer: PeerKey(42),
                character: 1,
                color: crate::kart::KartColor::SkyBlue,
                name: "Bob".to_string(),
                size,
            },
            FootInput::default(),
            WalkerControl::default(),
            super::walker::walker_body(at + front * 1.3 - Vec3::Y * 0.49, 0.0, size, &tuning),
        ))
        .id();
    run(&mut app, 20);
    let before = position(&app, other);
    let mut phases = vec![app.world().get::<Ragdoll>(other).unwrap().phase];
    hold_keys(&mut app, &[KeyCode::KeyF]);
    run(&mut app, 2);
    hold_keys(&mut app, &[]);
    let mut highest = before.y;
    let mut farthest = 0.0f32;
    let mut tilted = 0.0f32;
    for _ in 0..(6.0 * 60.0) as usize {
        app.update();
        let ragdoll = *app.world().get::<Ragdoll>(other).unwrap();
        if phases.last() != Some(&ragdoll.phase) {
            phases.push(ragdoll.phase);
        }
        let now = position(&app, other);
        highest = highest.max(now.y);
        farthest = farthest.max(Vec2::new(now.x - before.x, now.z - before.z).length());
        let up = app.world().get::<Transform>(other).unwrap().up();
        tilted = tilted.max(up.angle_between(Vec3::Y));
    }
    assert_eq!(
        phases,
        [
            RagdollPhase::Upright,
            RagdollPhase::Tumbling,
            RagdollPhase::GettingUp,
            RagdollPhase::Upright
        ],
        "knocked over, then up on its own"
    );
    assert_eq!(app.world().get::<WalkerControl>(body).unwrap().strikes, 1);
    assert!(highest > before.y + 0.4, "thrown up: {highest}");
    assert!(farthest > 1.5, "thrown along the push: {farthest} m");
    assert!(tilted > 1.0, "it tumbled: {tilted} rad");
    let up = app.world().get::<Transform>(other).unwrap().up();
    assert!(up.y > 0.999, "upright again");
    let ragdoll = app.world().get::<Ragdoll>(other).unwrap();
    assert_eq!(ragdoll.knocks, 1);
    assert!(
        ragdoll.since_up > ragdoll_tuning.protection_seconds,
        "protected a moment"
    );
    let look = *app.world().get::<WalkerLook>(other).unwrap();
    assert_eq!((look.phase, look.knocks), (RagdollPhase::Upright, 1));
}

/// Issue #101: a giant donut sends up whoever lands on it.
#[test]
fn a_giant_donut_sends_the_character_up() {
    let mut app = salon_app();
    let body = ours(&mut app);
    let donut = {
        let world = app.world_mut();
        *world.query::<&Bouncer>().iter(world).next().unwrap()
    };
    let above = Vec3::new(donut.center.x, donut.top + 1.5, donut.center.y);
    {
        let world = app.world_mut();
        let mut transform = world.get_mut::<Transform>(body).unwrap();
        transform.translation = above;
        let snapped = *transform;
        world
            .get_mut::<crate::interpolation::PhysicsPose>(body)
            .unwrap()
            .snap(snapped);
    }
    let mut highest = above.y;
    let mut bounced = false;
    let mut low = above.y;
    for _ in 0..120 {
        app.update();
        let y = position(&app, body).y;
        low = low.min(y);
        if y > low + 0.5 {
            bounced = true;
        }
        highest = highest.max(y);
    }
    assert!(bounced, "lowest {low}, highest {highest}");
    assert!(
        highest > above.y + 1.0,
        "higher than it fell from: {highest}"
    );
}

/// Issue #101: **nothing of the lobby is left once a race's level comes**: its scene, its
/// characters, their visuals and tags, its UI; the race is built as usual.
#[test]
fn the_lobby_is_gone_when_a_race_starts() {
    let mut app = salon_app();
    assert!(count::<With<Walker>>(&mut app) > 0);
    app.world_mut()
        .write_message(LoadLevel::on(TrackChoice::default(), RaceMode::Race));
    wait_for_the_player(&mut app);
    run(&mut app, 3);
    assert_eq!(count::<With<SalonRoot>>(&mut app), 0);
    assert_eq!(count::<With<SalonDecor>>(&mut app), 0);
    assert_eq!(count::<With<Bouncer>>(&mut app), 0);
    assert_eq!(count::<With<Walker>>(&mut app), 0);
    assert_eq!(count::<With<WalkerVisual>>(&mut app), 0);
    assert_eq!(count::<With<WalkerTag>>(&mut app), 0);
    assert_eq!(count::<With<WalkerHat>>(&mut app), 0);
    assert_eq!(count::<With<SalonKeys>>(&mut app), 0);
    assert_eq!(count::<With<Ragdoll>>(&mut app), 0);
    assert!(app.world().get_resource::<TrackLayout>().is_some());
    assert_eq!(count::<With<crate::kart::PlayerKart>>(&mut app), 1);
}

/// Issue #101: the lobby's tunings are in the admin panel (with the menu: the game's).
#[test]
fn the_lobby_tunings_are_in_the_admin_panel() {
    let mut app = game_app_unloaded(TrackChoice::TestGround, 0);
    app.add_plugins(SalonPlugin);
    app.update();
    let names: Vec<&str> = app
        .world()
        .resource::<crate::admin::Tunings>()
        .list
        .iter()
        .map(|t| t.name)
        .collect();
    assert!(
        names.contains(&"SalonTuning") && names.contains(&"RagdollTuning"),
        "{names:?}"
    );
    let roster = Roster::embedded();
    assert!(roster.characters.iter().all(|c| c.size > 0.3));
}
