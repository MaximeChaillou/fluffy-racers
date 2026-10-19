//! Headless scenarios of the soft characters (issue #12): the whole game (`GamePlugin`) with
//! `MinimalPlugins`, one fixed tick per update (frames of 1/60 s). No glTF loader: the drivers are
//! in primitives, which wobble as a whole (`models::tests` has the animals).

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::kart::{
    DriverPrimitive, DriverVisual, HitReaction, KartBodyPivot, KartInput, KartState, KartWheels,
    PlayerKart, TrickTracker,
};
use crate::race::Race;
use crate::tests::{app_on, follow_centerline, player, single_threaded};
use crate::track::{TrackChoice, TrackLayout};

/// Prairie Pompon, the player's kart driven by the test's autopilot (not the keyboard), every
/// schedule in its fixed order: two of them run the same.
fn ramp_app(wobble: bool) -> (App, Entity) {
    let mut app = app_on(TrackChoice::default());
    single_threaded(&mut app);
    app.world_mut().resource_mut::<WobbleTuning>().enabled = wobble;
    let kart = player(&mut app);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    (app, kart)
}

/// The driver of `kart`'s visual.
fn driver_of(app: &mut App, kart: Entity) -> Entity {
    let world = app.world_mut();
    let pivots: Vec<(Entity, KartBodyPivot)> = world
        .query::<(Entity, &KartBodyPivot)>()
        .iter(world)
        .map(|(e, p)| (e, *p))
        .collect();
    let pivot = pivots.iter().find(|(_, p)| p.body == kart).unwrap().0;
    world
        .query_filtered::<(Entity, &ChildOf), With<DriverVisual>>()
        .iter(world)
        .find(|(_, child_of)| child_of.parent() == pivot)
        .unwrap()
        .0
}

fn wobble(app: &App, driver: Entity) -> WobbleState {
    app.world().get::<DriverWobble>(driver).unwrap().0
}

/// The kart's pose and velocity after this tick (the physics, compared bit for bit).
fn physics(app: &App, kart: Entity) -> (Transform, Velocity, KartState) {
    let world = app.world();
    (
        *world.get::<Transform>(kart).unwrap(),
        *world.get::<Velocity>(kart).unwrap(),
        *world.get::<KartState>(kart).unwrap(),
    )
}

/// One tick of the autopilot (`input` instead, when given).
fn drive(app: &mut App, kart: Entity, input: Option<KartInput>) {
    let input = input.unwrap_or_else(|| {
        let layout = app.world().resource::<TrackLayout>().clone();
        let world = app.world();
        let transform = *world.get::<Transform>(kart).unwrap();
        let speed = world.get::<Velocity>(kart).unwrap().linear.length();
        follow_centerline(&layout, &transform, speed)
    });
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
    app.update();
}

/// Issue #12: the kart takes the trick ramp of Prairie Pompon. In the air its driver stretches;
/// at the landing it squashes (within its limit), then, the kart stopped, it comes back to rest.
/// Its primitives (headless) carry the pose, around the seat. And **the kart's physics is the
/// same with and without the wobble**, tick by tick, bit for bit: the same game run twice, the
/// wobble switched off in one.
#[test]
fn the_landing_after_the_ramp_squashes_the_driver_and_the_physics_does_not_change() {
    let tuning = WobbleTuning::default();
    let (mut soft, kart) = ramp_app(true);
    let (mut still, still_kart) = ramp_app(false);
    let driver = driver_of(&mut soft, kart);
    let still_driver = driver_of(&mut still, still_kart);
    // Both at GO on the same tick.
    for app in [&mut soft, &mut still] {
        let started = |app: &App| {
            app.world()
                .get_resource::<Race>()
                .is_some_and(Race::started)
        };
        for _ in 0..600 {
            if started(app) {
                break;
            }
            app.update();
        }
        assert!(started(app));
    }
    assert_eq!(physics(&soft, kart), physics(&still, still_kart));

    let mut tick = 0u32;
    let mut lockstep = |soft: &mut App, still: &mut App, input: Option<KartInput>| {
        drive(soft, kart, input);
        drive(still, still_kart, input);
        tick += 1;
        assert_eq!(
            physics(soft, kart),
            physics(still, still_kart),
            "the physics differ on tick {tick}"
        );
        // Off, the driver sits still.
        assert!(wobble(still, still_driver).at_rest(0.0001));
    };
    // To the ramp.
    let mut ticks = 0;
    while soft
        .world()
        .get::<TrickTracker>(kart)
        .unwrap()
        .ticks_since_takeoff()
        != Some(1)
    {
        ticks += 1;
        // Issue #108: the trick ramp of Prairie Pompon redrawn, 1.2 km along the lap.
        assert!(ticks < 60 * 60, "never took off from the ramp");
        lockstep(&mut soft, &mut still, None);
    }
    // In the air: stretched.
    let mut stretched = 0.0f32;
    while soft
        .world()
        .get::<KartWheels>(kart)
        .unwrap()
        .wheels_on_ground()
        == 0
    {
        ticks += 1;
        assert!(ticks < 65 * 60, "never landed");
        lockstep(&mut soft, &mut still, None);
        stretched = stretched.max(wobble(&soft, driver).pose.squash.y - 1.0);
    }
    assert!(stretched > 0.01, "stretched in the air: {stretched}");
    assert!(stretched <= tuning.max_stretch);
    // The landing: squashed within half a second, never beyond the limit; the primitives (the
    // ball, its eyes and ears) carry it, around the seat.
    let mut squashed = 0.0f32;
    let mut leaned = 0.0f32;
    let mut checked_primitives = false;
    for _ in 0..30 {
        lockstep(&mut soft, &mut still, None);
        let state = wobble(&soft, driver);
        squashed = squashed.max(1.0 - state.pose.squash.y);
        leaned = leaned.max(state.pose.lean.angle_between(Quat::IDENTITY));
        assert!(1.0 - state.pose.squash.y <= tuning.squash.max * tuning.ragdoll_max);
        if 1.0 - state.pose.squash.y > 0.03 && !checked_primitives {
            let world = soft.world_mut();
            let layers: Vec<(WobbleLayer, Transform)> = world
                .query_filtered::<(&WobbleLayer, &Transform), With<DriverPrimitive>>()
                .iter(world)
                .filter(|(layer, _)| layer.driver == driver)
                .map(|(l, t)| (*l, *t))
                .collect();
            assert!(layers.len() >= 3, "the ball and its eyes");
            for (layer, transform) in &layers {
                assert_eq!(*transform, layer.over(&state.pose));
                assert_eq!(layer.pivot, LayerPivot::Parent);
            }
            // The ball (at rest 0.45 m above the seat): lower and wider, its bottom on the seat.
            let (layer, ball) = layers
                .iter()
                .find(|(l, _)| l.base.translation.x == 0.0 && l.base.translation.y > 0.4)
                .unwrap();
            assert!(ball.scale.y < 0.97 && ball.scale.x > 1.01, "{ball:?}");
            let bottom = ball.transform_point(Vec3::new(0.0, -layer.base.translation.y, 0.0));
            assert!(bottom.length() < 0.15, "{bottom}");
            checked_primitives = true;
        }
    }
    assert!(squashed > 0.05, "squashed at the landing: {squashed}");
    assert!(checked_primitives);
    assert!(leaned < tuning.body.max * tuning.ragdoll_max + 1e-4);
    // The kart stopped: the driver comes back to rest, and stays there.
    let mut rest = None;
    for i in 0..(6 * 60) {
        let speed = soft.world().get::<Velocity>(kart).unwrap().linear.length();
        let input = KartInput {
            brake: if speed > 0.5 { 1.0 } else { 0.0 },
            ..default()
        };
        lockstep(&mut soft, &mut still, Some(input));
        if wobble(&soft, driver).at_rest(2e-3) {
            rest.get_or_insert(i);
        } else {
            rest = None;
        }
    }
    let rest = rest.expect("the driver never came back to rest");
    assert!(rest < 4 * 60, "{rest} ticks");
}

/// Issue #12: hit by an item, a kart's driver ragdolls for about a second; struck by a paw, too;
/// the others do not. Then all of them are back at rest (on the grid, a long one: no one moves).
#[test]
fn a_hit_and_a_strike_ragdoll_the_driver_for_a_second() {
    let mut app = crate::tests::game_app_unloaded(TrackChoice::default(), 2);
    app.world_mut()
        .resource_mut::<crate::race::RaceTuning>()
        .grid_seconds = 60.0;
    crate::tests::wait_for_the_player(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let karts: Vec<Entity> = world
        .query_filtered::<Entity, With<crate::kart::Kart>>()
        .iter(world)
        .collect();
    assert_eq!(karts.len(), 3);
    let drivers: Vec<Entity> = karts.iter().map(|k| driver_of(&mut app, *k)).collect();
    let world = app.world_mut();
    *world.get_mut::<KartState>(karts[0]).unwrap() = KartState::Hit {
        reaction: HitReaction::Tumble,
        elapsed: 0,
        duration: 30,
    };
    {
        let mut paw = world.get_mut::<crate::interactions::Paw>(karts[1]).unwrap();
        paw.struck_count += 1;
        paw.struck_ticks = 0;
        paw.struck_side = Vec3::X;
    }
    app.update();
    let tuning = app.world().resource::<WobbleTuning>().clone();
    let ragdolls: Vec<f32> = drivers.iter().map(|d| wobble(&app, *d).ragdoll).collect();
    assert!(ragdolls[0] > 0.9 * tuning.ragdoll_seconds, "{ragdolls:?}");
    assert!(ragdolls[1] > 0.9 * tuning.ragdoll_seconds, "{ragdolls:?}");
    assert_eq!(ragdolls[2], 0.0);
    // Thrown: the tumbled one's head back, the struck one's toward the paw.
    let mut back = 0.0f32;
    let mut toward_paw = 0.0f32;
    for _ in 0..30 {
        app.update();
        back = back.max((wobble(&app, drivers[0]).pose.lean * Vec3::Y).z);
        let rotation = app.world().get::<Transform>(karts[1]).unwrap().rotation;
        let paw = (rotation.inverse() * Vec3::NEG_X).normalize();
        toward_paw = toward_paw.max((wobble(&app, drivers[1]).pose.lean * Vec3::Y).dot(paw));
    }
    assert!(back > 0.1, "{back}");
    assert!(toward_paw > 0.1, "{toward_paw}");
    assert!(!app.world().resource::<Race>().started());
    for _ in 0..(4 * 60) {
        app.update();
    }
    for driver in &drivers {
        let state = wobble(&app, *driver);
        assert_eq!(state.ragdoll, 0.0);
        assert!(state.at_rest(2e-3), "{state:?}");
    }
}
