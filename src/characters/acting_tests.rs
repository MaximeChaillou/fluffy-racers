//! Issue #130: the acting, headless: the whole game (`GamePlugin`) with `MinimalPlugins`, one
//! fixed tick per update (frames of 1/60 s), [`ActingPlugin`] added by hand (the game adds it only
//! with a renderer). No glTF loader: the drivers are in primitives, which act as a whole (the
//! acting moves the driver itself, whatever it is made of).

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::admin::skip_countdown_now;
use crate::bots::Bot;
use crate::items::{ItemKind, ItemObject, ItemSlots, Slot};
use crate::kart::{
    BodyPaint, DriverVisual, HitReaction, KartBodyPivot, KartInput, KartState, KartWheels,
    PlayerKart, TrickTracker,
};
use crate::level::LoadLevel;
use crate::race::{Race, RaceMode, RaceProgress};
use crate::tests::{follow_centerline, game_app_unloaded, player, single_threaded};
use crate::track::{TrackChoice, TrackLayout};

/// The game on `track` with `bots` bots, the acting added or not, every schedule in its fixed
/// order (two of them run the same); the player's kart spawned.
fn acting_app(track: TrackChoice, bots: usize, acting: bool) -> App {
    let mut app = game_app_unloaded(track, bots);
    if acting {
        app.add_plugins(ActingPlugin);
    }
    single_threaded(&mut app);
    crate::tests::wait_for_the_player(&mut app);
    app
}

/// Runs until the race and its bots are there, then skips the countdown: GO.
fn start(app: &mut App, bots: usize) {
    for _ in 0..600 {
        let world = app.world_mut();
        let ready = world.get_resource::<Race>().is_some()
            && world.query::<&Bot>().iter(world).count() == bots;
        if ready {
            break;
        }
        app.update();
    }
    app.update();
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    assert!(app.world().resource::<Race>().started());
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

/// The pivot of `kart`'s visual.
fn pivot_of(app: &mut App, kart: Entity) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &KartBodyPivot)>()
        .iter(world)
        .find(|(_, p)| p.body == kart)
        .unwrap()
        .0
}

/// The driver of `kart`'s visual.
fn driver_of(app: &mut App, kart: Entity) -> Entity {
    let pivot = pivot_of(app, kart);
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &ChildOf), With<DriverVisual>>()
        .iter(world)
        .find(|(_, child_of)| child_of.parent() == pivot)
        .unwrap()
        .0
}

/// The shell of `kart`'s visual.
fn shell_of(app: &mut App, kart: Entity) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &BodyPaint)>()
        .iter(world)
        .find(|(_, paint)| paint.kart == kart)
        .unwrap()
        .0
}

fn acting(app: &App, driver: Entity) -> Acting {
    *app.world().get::<Acting>(driver).unwrap()
}

fn transform(app: &App, entity: Entity) -> Transform {
    *app.world().get::<Transform>(entity).unwrap()
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

/// The autopilot of the tests along the centerline, for `kart`.
fn autopilot(app: &App, kart: Entity) -> KartInput {
    let layout = app.world().resource::<TrackLayout>().clone();
    let world = app.world();
    let transform = *world.get::<Transform>(kart).unwrap();
    let speed = world.get::<Velocity>(kart).unwrap().linear.length();
    follow_centerline(&layout, &transform, speed)
}

fn drive(app: &mut App, kart: Entity, input: KartInput) {
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
    app.update();
}

/// Issue #130: nothing of the acting in the headless game (the bot races' world): no component
/// on the drivers, the shells never moved; with the plugin, every driver of a kart acts.
#[test]
fn nothing_of_the_acting_in_the_headless_game() {
    let mut app = acting_app(TrackChoice::default(), 2, false);
    run(&mut app, 30);
    let world = app.world_mut();
    assert_eq!(world.query::<&Acting>().iter(world).count(), 0);
    assert!(
        world
            .query_filtered::<&Transform, With<BodyPaint>>()
            .iter(world)
            .all(|t| *t == Transform::IDENTITY)
    );
    let mut app = acting_app(TrackChoice::default(), 2, true);
    run(&mut app, 3);
    let world = app.world_mut();
    let actors: Vec<Acting> = world.query::<&Acting>().iter(world).copied().collect();
    assert_eq!(actors.len(), 3);
    assert!(actors.iter().all(|a| a.shell.is_some()));
}

/// Issue #130 acceptance, on the trick ramp of Prairie Pompon (the button pressed as the kart
/// takes off): **the figure** (the driver pops up out of its seat, turns once, drops back in, for
/// `trick_seconds`), **the landing** (the shell stretched in the air, squashed when the kart
/// lands, then bouncing back up), then **everything back to rest** (the shell, and the driver
/// exactly on its seat). And **the kart's physics is the same with and without the acting**,
/// tick by tick, bit for bit: the same game run twice, the acting's plugin added to one.
#[test]
fn the_trick_ramp_the_figure_the_landing_and_the_physics_does_not_change() {
    let tuning = DriverTuning::default();
    let body = WobbleTuning::default().kart;
    let mut acted = acting_app(TrackChoice::default(), 0, true);
    let mut plain = acting_app(TrackChoice::default(), 0, false);
    let kart = player(&mut acted);
    let plain_kart = player(&mut plain);
    for app in [&mut acted, &mut plain] {
        start(app, 0);
        let kart = player(app);
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    }
    assert_eq!(physics(&acted, kart), physics(&plain, plain_kart));
    let driver = driver_of(&mut acted, kart);
    let shell = shell_of(&mut acted, kart);
    let plain_shell = shell_of(&mut plain, plain_kart);
    let mut tick = 0u32;
    let mut lockstep = |acted: &mut App, plain: &mut App, input: KartInput| {
        drive(acted, kart, input);
        drive(plain, plain_kart, input);
        tick += 1;
        assert_eq!(
            physics(acted, kart),
            physics(plain, plain_kart),
            "the physics differ on tick {tick}"
        );
        assert_eq!(transform(plain, plain_shell), Transform::IDENTITY);
    };
    // To the ramp; the button pressed just after the takeoff.
    let takeoff = |app: &App| {
        app.world()
            .get::<TrickTracker>(kart)
            .unwrap()
            .ticks_since_takeoff()
    };
    let mut ticks = 0;
    while takeoff(&acted) != Some(1) {
        ticks += 1;
        // Issue #108: Prairie Pompon redrawn, its trick ramp about 1.2 km along the lap (40 s
        // before, for a ramp a few hundred meters along).
        assert!(ticks < 60 * 60, "never took off from the ramp");
        let input = autopilot(&acted, kart);
        lockstep(&mut acted, &mut plain, input);
    }
    let mut figure_frames = 0;
    let mut highest = 0.0f32;
    let mut turned = 0.0f32;
    let mut stretched = 0.0f32;
    let mut pressed = 0;
    let airborne = |app: &App| {
        app.world()
            .get::<KartWheels>(kart)
            .unwrap()
            .wheels_on_ground()
            == 0
    };
    while airborne(&acted) {
        ticks += 1;
        assert!(ticks < 45 * 60, "never landed");
        let mut input = autopilot(&acted, kart);
        input.jump_drift = pressed < 3;
        pressed += 1;
        lockstep(&mut acted, &mut plain, input);
        let now = acting(&acted, driver);
        if now.driver.figure.is_some() {
            figure_frames += 1;
        }
        let shown = transform(&acted, driver);
        let base = now.base();
        highest = highest.max(shown.translation.y - base.translation.y);
        turned = turned.max(shown.rotation.angle_between(base.rotation));
        stretched = stretched.max(transform(&acted, shell).scale.y - 1.0);
    }
    assert_eq!(
        *acted.world().get::<KartState>(kart).unwrap(),
        KartState::Grounded,
        "the trick ended at the landing"
    );
    assert!(
        stretched > 0.005,
        "the shell stretched in the air: {stretched}"
    );
    assert!(stretched <= body.max_stretch + 1e-4);
    // The landing: squashed, then back up past its rest; the figure finishes meanwhile.
    let mut squashed = 0.0f32;
    let mut bounced = 0.0f32;
    for _ in 0..60 {
        let input = autopilot(&acted, kart);
        lockstep(&mut acted, &mut plain, input);
        let now = acting(&acted, driver);
        if now.driver.figure.is_some() {
            figure_frames += 1;
        }
        let shown = transform(&acted, driver);
        highest = highest.max(shown.translation.y - now.base().translation.y);
        turned = turned.max(shown.rotation.angle_between(now.base().rotation));
        let height = transform(&acted, shell).scale.y;
        squashed = squashed.max(1.0 - height);
        if squashed > 0.03 {
            bounced = bounced.max(height - 1.0);
        }
    }
    assert!(
        squashed > 0.05,
        "the shell squashed at the landing: {squashed}"
    );
    assert!(squashed <= body.squash.max + 1e-4);
    assert!(bounced > 0.005, "then bounced back up: {bounced}");
    let expected = (tuning.trick_seconds * 60.0).round() as i32;
    assert!(
        (figure_frames - expected).abs() <= 1,
        "the figure lasted {figure_frames} frames"
    );
    let size = acted.world().get::<DriverVisual>(driver).unwrap().size;
    assert!(
        highest > tuning.trick_hop * size * 0.8,
        "up {highest} m out of its seat"
    );
    assert!(turned > 2.5, "turned {turned} rad");
    // The kart stopped: everything back to rest, and staying there.
    let mut rest = None;
    for i in 0..(5 * 60) {
        let speed = acted.world().get::<Velocity>(kart).unwrap().linear.length();
        let input = KartInput {
            brake: if speed > 0.5 { 1.0 } else { 0.0 },
            ..default()
        };
        lockstep(&mut acted, &mut plain, input);
        let now = acting(&acted, driver);
        let still = now.driver.at_rest()
            && transform(&acted, shell) == Transform::IDENTITY
            && transform(&acted, driver) == now.base();
        if still {
            rest.get_or_insert(i);
        } else {
            rest = None;
        }
    }
    let rest = rest.expect("never back to rest");
    assert!(rest < 4 * 60, "{rest} ticks");
}

/// Issue #130: overtaken, a kart's driver turns toward the kart that just passed it and follows
/// it as it goes by, for `look_back_seconds`; then faces ahead again, exactly on its seat. The
/// bot stands still on the pole, the player passes it.
#[test]
fn overtaken_it_looks_at_the_kart_going_by() {
    let tuning = DriverTuning::default();
    let mut app = acting_app(TrackChoice::default(), 1, true);
    start(&mut app, 1);
    let kart = player(&mut app);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    let world = app.world_mut();
    let bot = world
        .query_filtered::<Entity, With<Bot>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(bot)
        .remove::<Bot>()
        .insert(KartInput::default());
    let driver = driver_of(&mut app, bot);
    let rank = |app: &App, kart: Entity| app.world().get::<RaceProgress>(kart).unwrap().rank;
    assert_eq!(rank(&app, bot), 1);
    let mut frames = 0;
    while rank(&app, bot) == 1 {
        frames += 1;
        assert!(frames < 20 * 60, "never overtook the bot");
        let input = autopilot(&app, kart);
        drive(&mut app, kart, input);
        if rank(&app, bot) == 1 {
            assert!(acting(&app, driver).driver.at_rest(), "before");
        }
    }
    // It looks at the player's kart, which goes on ahead.
    let mut looked = 0.0f32;
    let mut looking = 0;
    for _ in 0..(3 * 60) {
        let input = autopilot(&app, kart);
        drive(&mut app, kart, input);
        let now = acting(&app, driver);
        if now.driver.look > 0.0 {
            looking += 1;
            assert_eq!(now.looking_at, Some(kart));
        }
        let shown = transform(&app, driver);
        looked = looked.max(shown.rotation.angle_between(now.base().rotation));
    }
    assert!(looked > 0.5, "turned {looked} rad toward it");
    let expected = (tuning.look_back_seconds * 60.0).round() as i32;
    assert!((looking - expected).abs() <= 2, "{looking} frames");
    let now = acting(&app, driver);
    assert!(now.driver.at_rest());
    assert_eq!(transform(&app, driver), now.base());
}

/// Issue #130: an item thrown behind (a pelote, the look-back button held) turns the driver
/// over its shoulder, for `throw_look_seconds`, then back; thrown ahead, nothing.
#[test]
fn an_item_thrown_behind_turns_it_over_its_shoulder() {
    let tuning = DriverTuning::default();
    let mut app = acting_app(TrackChoice::default(), 0, true);
    start(&mut app, 0);
    let kart = player(&mut app);
    app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    let driver = driver_of(&mut app, kart);
    let throttle = KartInput {
        throttle: 1.0,
        ..default()
    };
    for _ in 0..40 {
        drive(&mut app, kart, throttle);
    }
    let throw = |app: &mut App, back: bool| -> (f32, i32) {
        app.world_mut().get_mut::<ItemSlots>(kart).unwrap().hand = Slot::Ready {
            item: ItemKind::Pelote,
            count: 1,
        };
        let held = KartInput {
            item: true,
            look_back: back,
            ..throttle
        };
        for _ in 0..4 {
            drive(app, kart, held);
            assert!(acting(app, driver).driver.at_rest(), "held: no look");
        }
        let (mut turned, mut looking) = (0.0f32, 0);
        for i in 0..90 {
            let release = KartInput {
                look_back: back && i < 2,
                ..throttle
            };
            drive(app, kart, release);
            let now = acting(app, driver);
            if now.driver.look > 0.0 {
                looking += 1;
            }
            // Its own turn (its body may tilt too: the kart accelerates).
            turned = turned.max(now.driver.pose.turn.angle_between(Quat::IDENTITY));
        }
        let world = app.world_mut();
        let pelotes = world
            .query::<&ItemObject>()
            .iter(world)
            .filter(|o| o.owner == kart && o.item == ItemKind::Pelote)
            .count();
        assert!(pelotes > 0, "the pelote is out");
        (turned, looking)
    };
    let (turned, looking) = throw(&mut app, true);
    assert!(turned > PI / 2.0, "over its shoulder: {turned} rad");
    let expected = (tuning.throw_look_seconds * 60.0).round() as i32;
    assert!((looking - expected).abs() <= 2, "{looking} frames");
    // Back: the driver acts nothing more (its seat still moves with its body, which the kart
    // accelerating tilts a little).
    let now = acting(&app, driver);
    assert!(now.driver.at_rest());
    assert_eq!(now.driver.pose, ActingPose::default());
    let (turned, looking) = throw(&mut app, false);
    assert_eq!(looking, 0, "thrown ahead");
    assert_eq!(turned, 0.0);
}

/// Issue #130 acceptance: over the line, the first three keep their dance (no sulk), and the
/// last of four sulks: slumped forward and sunk in its seat, until the next race (its new
/// driver sits up).
#[test]
fn over_the_line_the_last_one_sulks_until_the_next_race() {
    let mut app = acting_app(TrackChoice::default(), 3, true);
    start(&mut app, 3);
    run(&mut app, 30);
    let standings = app.world().resource::<Race>().standings.clone();
    assert_eq!(standings.len(), 4);
    let ticks = app.world().resource::<Race>().race_ticks();
    for (i, kart) in standings.iter().enumerate() {
        app.world_mut()
            .get_mut::<RaceProgress>(*kart)
            .unwrap()
            .finish = Some(ticks + i as u32);
    }
    run(&mut app, 3);
    assert_eq!(app.world().resource::<Race>().standings, standings);
    let drivers: Vec<Entity> = standings.iter().map(|k| driver_of(&mut app, *k)).collect();
    run(&mut app, 120);
    for (place, driver) in drivers.iter().enumerate() {
        let now = acting(&app, *driver);
        if place == 3 {
            assert_eq!(now.driver.sulk, 1.0, "the last sulks");
            let shown = transform(&app, *driver);
            let base = now.base();
            let head = base.rotation.inverse() * (shown.rotation * Vec3::Y);
            assert!(head.z < -0.15, "slumped forward: {head}");
            assert!(shown.translation.y < base.translation.y - 0.03, "sunk");
        } else {
            assert_eq!(now.driver.sulk, 0.0, "place {place}");
        }
    }
    // It goes on sulking...
    run(&mut app, 120);
    assert_eq!(acting(&app, drivers[3]).driver.sulk, 1.0);
    // ...until the next race.
    app.world_mut()
        .write_message(LoadLevel::restart(RaceMode::Race));
    for _ in 0..600 {
        app.update();
        let world = app.world_mut();
        let karts = world.query::<&Acting>().iter(world).count();
        if karts == 4 && world.get_resource::<Race>().is_some() {
            break;
        }
    }
    run(&mut app, 60);
    let world = app.world_mut();
    assert!(
        world
            .query::<&Acting>()
            .iter(world)
            .all(|a| a.driver.sulk == 0.0)
    );
}

/// Issue #130: a kart that sees stars (the particles' `KartVfx::dizzy`, after a tumble) has its
/// driver's head going round as long as they last; then still, exactly on its seat.
#[test]
fn seeing_stars_its_head_goes_round() {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.add_plugins((ActingPlugin, crate::vfx::VfxPlugin));
    single_threaded(&mut app);
    crate::tests::wait_for_the_player(&mut app);
    start(&mut app, 0);
    let kart = player(&mut app);
    let driver = driver_of(&mut app, kart);
    run(&mut app, 10);
    *app.world_mut().get_mut::<KartState>(kart).unwrap() = KartState::Hit {
        reaction: HitReaction::Tumble,
        elapsed: 0,
        duration: 30,
    };
    let mut dizzy_frames = 0;
    let mut tips = Vec::new();
    for _ in 0..(4 * 60) {
        app.update();
        let stars = app.world().get::<crate::vfx::KartVfx>(kart).unwrap().dizzy;
        let now = acting(&app, driver);
        if stars > 0.0 {
            dizzy_frames += 1;
            assert!(now.driver.dizzy > 0.0 || dizzy_frames < 3);
            let shown = transform(&app, driver);
            let tip = now.base().rotation.inverse() * (shown.rotation * Vec3::Y);
            tips.push(tip);
        }
    }
    assert!(dizzy_frames > 60, "{dizzy_frames}");
    let leaned = tips.iter().map(|t| t.xz().length()).fold(0.0, f32::max);
    assert!(leaned > 0.1, "{leaned}");
    for side in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
        assert!(tips.iter().any(|t| t.xz().dot(side) > 0.05), "never {side}");
    }
    let now = acting(&app, driver);
    assert!(now.driver.at_rest());
    assert_eq!(transform(&app, driver), now.base());
}
