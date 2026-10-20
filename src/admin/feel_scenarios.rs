//! Headless driving with the « Ressenti » knobs at their ends (issue #155): the driving changes
//! the way each knob says, and stays playable (the kart drives straight, turns, drifts, and a
//! bot finishes a lap). The kart tuning is the one the panel makes (`Tunings::set_knob` over the
//! game's tunings); the kart scenarios drive it on a flat slab with the real `SimulationPlugin`,
//! the laps run the whole game.

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::Tunings;
use super::feel::{KNOB_MAX, Knob, MIDDLE};
use super::feel_tests::all_tunings;
use crate::bots::{BotDifficulty, BotTuning, bot, bot_seed};
use crate::camera::ChaseCamera;
use crate::interactions::InteractionTuning;
use crate::items::ItemTuning;
use crate::kart::{DriftSide, KartInput, KartState, KartTuning, MiniTurbo, PlayerKart, kart_body};
use crate::race::{RaceGrid, RaceProgress, Racer};
use crate::tests::{app_on, game_app_unloaded, player, single_threaded, wait_for_the_player};
use crate::track::{Surface, TrackChoice, spawn_static_box};
use crate::{SimulationPlugin, TICK_HZ};

const FULL_THROTTLE: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
    interact: false,
};

/// The kart tuning with `knobs` at their positions (the others in the middle), as the panel
/// makes it.
fn kart_tuning(knobs: &[(Knob, u8)]) -> KartTuning {
    let (mut world, mut tunings) = all_tunings();
    for (knob, position) in knobs {
        tunings.set_knob(&mut world, *knob, *position);
    }
    world.resource::<KartTuning>().clone()
}

/// A kart at the origin looking down -Z on a large flat slab of `surface`, driving with `tuning`.
struct Slab {
    app: App,
    kart: Entity,
    tuning: KartTuning,
}

impl Slab {
    fn new(tuning: KartTuning, surface: Surface) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, SimulationPlugin))
            .insert_resource(TimeUpdateStrategy::FixedTimesteps(1))
            .insert_resource(tuning.clone());
        app.finish();
        app.cleanup();
        let world = app.world_mut();
        let mut commands = world.commands();
        spawn_static_box(
            &mut commands,
            Transform::from_xyz(0.0, -0.5, 0.0),
            Vec3::new(500.0, 0.5, 500.0),
            surface,
            0.0,
        );
        world.flush();
        let kart = world
            .spawn(kart_body(&tuning, Transform::from_xyz(0.0, 0.55, 0.0)))
            .id();
        app.update();
        let mut slab = Self { app, kart, tuning };
        slab.run(0.5, KartInput::default(), |_| {});
        slab
    }

    fn run(&mut self, seconds: f32, input: KartInput, mut each_tick: impl FnMut(&Self)) {
        for _ in 0..(seconds * TICK_HZ as f32).round() as usize {
            self.tick(input);
            each_tick(self);
        }
    }

    fn tick(&mut self, input: KartInput) {
        *self
            .app
            .world_mut()
            .get_mut::<KartInput>(self.kart)
            .unwrap() = input;
        self.app.update();
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

    /// Heading of the motion on the ground (rad).
    fn heading(&self) -> f32 {
        let v = self.velocity();
        v.x.atan2(-v.z)
    }
}

fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// A drift steered fully inward: its radius once set in (m, over 1 s), and the ticks to the blue
/// mini-turbo.
struct DriftRun {
    radius: f32,
    blue_ticks: u32,
}

fn drift(position: u8) -> DriftRun {
    let mut slab = Slab::new(kart_tuning(&[(Knob::Drift, position)]), Surface::Road);
    slab.run(2.5, FULL_THROTTLE, |_| {});
    let drifting = KartInput {
        steer: 1.0,
        jump_drift: true,
        ..FULL_THROTTLE
    };
    let mut ticks = 0;
    while !matches!(slab.state(), KartState::Drifting { .. }) {
        assert!(
            ticks < 60,
            "never drifted at {position}: {:?}",
            slab.state()
        );
        slab.tick(drifting);
        ticks += 1;
    }
    assert!(matches!(
        slab.state(),
        KartState::Drifting {
            side: DriftSide::Right,
            ..
        }
    ));
    let mut blue_ticks = 0;
    let mut ticks = 1;
    let tuning = slab.tuning.clone();
    // Set in (the curve takes up to 0.5 s), then measured over 1 s.
    let (mut turned, mut travelled, mut last) = (0.0, 0.0, slab.heading());
    for tick in 0..((1.6 * TICK_HZ as f32) as u32) {
        slab.tick(drifting);
        ticks += 1;
        let state = slab.state();
        assert!(
            matches!(state, KartState::Drifting { .. }),
            "the drift broke at {position} after {ticks} ticks: {state:?}"
        );
        if blue_ticks == 0 && state.mini_turbo(&tuning, false) == Some(MiniTurbo::Blue) {
            blue_ticks = ticks;
        }
        let heading = slab.heading();
        if tick as f32 >= 0.6 * TICK_HZ as f32 {
            turned += wrap(heading - last);
            travelled += slab.velocity().length() / TICK_HZ as f32;
        }
        last = heading;
        assert!(slab.transform().up().y > 0.9, "tipped at {position}");
    }
    assert!(blue_ticks > 0, "no blue mini-turbo at {position}");
    DriftRun {
        radius: travelled / turned.abs(),
        blue_ticks,
    }
}

/// « Dérapage »: tighter and quicker at 0, wider and slower to charge at 100; the drift holds at
/// both ends.
#[test]
fn the_drift_knob_tightens_or_widens_the_drift() {
    let [tight, middle, wide] = [0, MIDDLE, KNOB_MAX].map(drift);
    eprintln!(
        "drift radius {:.1} / {:.1} / {:.1} m, blue after {} / {} / {} ticks",
        tight.radius,
        middle.radius,
        wide.radius,
        tight.blue_ticks,
        middle.blue_ticks,
        wide.blue_ticks
    );
    assert!(
        tight.radius < middle.radius * 0.9,
        "{} / {}",
        tight.radius,
        middle.radius
    );
    assert!(
        wide.radius > middle.radius * 1.1,
        "{} / {}",
        wide.radius,
        middle.radius
    );
    assert!(tight.blue_ticks < middle.blue_ticks && middle.blue_ticks < wide.blue_ticks);
    // Still a drift you can take in a turn of the game (the hairpins: about 15 m).
    assert!(tight.radius > 8.0 && wide.radius < 60.0);
}

/// The top speed after 3 s of full throttle, how long it takes to 90 % of it, and how far the
/// kart strayed sideways from its straight line (m).
fn straight(knobs: &[(Knob, u8)], surface: Surface) -> (f32, f32, f32) {
    let mut slab = Slab::new(kart_tuning(knobs), surface);
    let mut speeds = Vec::new();
    let mut stray: f32 = 0.0;
    slab.run(4.0, FULL_THROTTLE, |slab| {
        speeds.push(slab.velocity().length());
        stray = stray.max(slab.transform().translation.x.abs());
        assert!(slab.transform().up().y > 0.98);
    });
    let top = speeds[(3.0 * TICK_HZ as f32) as usize];
    let to_90 = speeds.iter().position(|v| *v >= 0.9 * top).unwrap() as f32 / TICK_HZ as f32;
    (top, to_90, stray)
}

/// « Vitesse »: slower and gentler at 0, faster at 100; the kart drives straight at both ends.
#[test]
fn the_speed_knob_changes_the_top_speed_and_keeps_the_kart_straight() {
    let [slow, middle, fast] =
        [0, MIDDLE, KNOB_MAX].map(|p| straight(&[(Knob::Speed, p)], Surface::Road));
    eprintln!("top speed {slow:?} / {middle:?} / {fast:?}");
    assert!((middle.0 - 28.5).abs() < 0.5, "{middle:?}");
    assert!(slow.0 < middle.0 * 0.85 && fast.0 > middle.0 * 1.15);
    // Faster to its own top speed at 100.
    assert!(fast.1 < slow.1, "{fast:?} / {slow:?}");
    for run in [slow, middle, fast] {
        assert!(run.2 < 0.2, "strayed {} m", run.2);
    }
}

/// Steering at full lock at top speed for 2 s: the mean sideways slide (m/s), and whether the
/// kart stayed on its wheels.
fn full_lock(position: u8) -> f32 {
    let mut slab = Slab::new(kart_tuning(&[(Knob::Grip, position)]), Surface::Road);
    slab.run(2.8, FULL_THROTTLE, |_| {});
    let mut slide = 0.0;
    let mut samples = 0.0;
    let turning = KartInput {
        steer: 1.0,
        ..FULL_THROTTLE
    };
    slab.run(2.0, turning, |slab| {
        let right = *slab.transform().right();
        slide += slab.velocity().dot(right).abs();
        samples += 1.0;
        assert!(slab.transform().up().y > 0.9, "tipped at {position}");
    });
    slide / samples
}

/// « Tenue de route »: slides more at 0, grips more and loses less off the road at 100; the kart
/// turns on its wheels and drives straight at both ends.
#[test]
fn the_grip_knob_changes_the_slide_and_the_offroad() {
    let [slippery, middle, grippy] = [0, MIDDLE, KNOB_MAX].map(full_lock);
    eprintln!("sideways slide at full lock {slippery:.3} / {middle:.3} / {grippy:.3} m/s");
    assert!(slippery > middle && middle > grippy);
    let offroad = [0, KNOB_MAX].map(|p| straight(&[(Knob::Grip, p)], Surface::Offroad));
    eprintln!("off the road {offroad:?}");
    assert!(offroad[0].0 < offroad[1].0 * 0.8, "{offroad:?}");
    for p in [0, KNOB_MAX] {
        let (_, _, stray) = straight(&[(Knob::Grip, p)], Surface::Road);
        assert!(stray < 0.2, "strayed {stray} m at {p}");
    }
}

/// « Caméra »: closer at 0, further at 100 (the chase camera of the whole game).
#[test]
fn the_camera_knob_moves_the_camera_closer_or_further() {
    let distance = |position: u8| {
        let mut app = app_on(TrackChoice::TestGround);
        app.world_mut()
            .resource_scope(|world, mut tunings: Mut<Tunings>| {
                tunings.set_knob(world, Knob::Camera, position);
            });
        for _ in 0..90 {
            app.update();
        }
        let kart = player(&mut app);
        let at = app
            .world()
            .get::<GlobalTransform>(kart)
            .unwrap()
            .translation();
        let world = app.world_mut();
        let camera = world
            .query_filtered::<&GlobalTransform, With<ChaseCamera>>()
            .single(world)
            .unwrap()
            .translation();
        camera.distance(at)
    };
    let [close, middle, far] = [0, MIDDLE, KNOB_MAX].map(distance);
    eprintln!("camera {close:.2} / {middle:.2} / {far:.2} m from the kart");
    assert!(
        close < middle * 0.9 && far > middle * 1.2,
        "{close} / {middle} / {far}"
    );
}

/// A bot of the given level drives the player's kart round one lap of the game's first circuit,
/// alone, without items, with the driving knobs and the bots' at `position`: its lap time (s),
/// if it finished one within 2 min, and how many times the pelican carried it.
fn one_lap(position: u8) -> (Option<f32>, u32) {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.world_mut().resource_mut::<ItemTuning>().enabled = false;
    app.world_mut().resource_mut::<InteractionTuning>().enabled = false;
    single_threaded(&mut app);
    app.world_mut()
        .resource_scope(|world, mut tunings: Mut<Tunings>| {
            for knob in [
                Knob::Drift,
                Knob::Speed,
                Knob::Grip,
                Knob::Boosts,
                Knob::Bumps,
                Knob::Bots,
            ] {
                tunings.set_knob(world, knob, position);
            }
        });
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    let slot = app.world().get::<Racer>(kart).unwrap().grid_slot;
    let seed = bot_seed(app.world().resource::<RaceGrid>().seed, slot);
    let tuning = app.world().resource::<BotTuning>().clone();
    app.world_mut()
        .entity_mut(kart)
        .remove::<PlayerKart>()
        .insert(bot(BotDifficulty::Normal, seed, &tuning));
    let (mut tows, mut carried) = (0, false);
    for _ in 0..(130.0 * TICK_HZ) as u32 {
        app.update();
        let world = app.world();
        let respawning = world.get::<KartState>(kart) == Some(&KartState::Respawning);
        tows += u32::from(respawning && !carried);
        carried = respawning;
        if let Some(lap) = world
            .get::<RaceProgress>(kart)
            .and_then(|p| p.lap_times.first())
        {
            return (Some(*lap as f32 / TICK_HZ as f32), tows);
        }
    }
    (None, tows)
}

/// Playable at both ends: with every driving knob (and the bots') at 0, then at 100, a bot still
/// drives a whole lap of the circuit, faster at 100 than at 0.
#[test]
fn a_bot_finishes_a_lap_with_the_knobs_at_either_end() {
    let [slow, middle, fast] = [0, MIDDLE, KNOB_MAX].map(one_lap);
    eprintln!("one lap: {slow:?} / {middle:?} / {fast:?} (s, pelicans)");
    let (Some(slow_lap), Some(middle_lap), Some(fast_lap)) = (slow.0, middle.0, fast.0) else {
        panic!("a lap was not finished: {slow:?} / {middle:?} / {fast:?}");
    };
    assert!(fast_lap < middle_lap && middle_lap < slow_lap);
    for (lap, tows) in [slow, middle, fast] {
        assert!(tows <= 1, "{tows} pelicans in a lap of {lap:?} s");
    }
}
