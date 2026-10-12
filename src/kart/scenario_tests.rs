//! Headless driving scenarios (issue #1 acceptance criteria): the real `SimulationPlugin`
//! (Rapier + kart systems) at 60 Hz, one fixed tick per `app.update()`.

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::track::{Surface, WALL_RESTITUTION, ramp_transform, spawn_static_box};

const TICKS_PER_SECOND: f32 = 60.0;
const FULL_THROTTLE: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
};

struct Scenario {
    app: App,
    kart: Entity,
}

impl Scenario {
    /// A kart at the origin looking down -Z, on a large flat slab of `surface`.
    fn flat(surface: Surface) -> Self {
        Self::new(|commands| {
            spawn_static_box(
                commands,
                Transform::from_xyz(0.0, -0.5, 0.0),
                Vec3::new(500.0, 0.5, 500.0),
                surface,
                0.0,
            );
        })
    }

    fn new(build_arena: impl FnOnce(&mut Commands)) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, SimulationPlugin))
            .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.finish();
        app.cleanup();

        let world = app.world_mut();
        let mut commands = world.commands();
        build_arena(&mut commands);
        world.flush();
        let tuning = world.resource::<KartTuning>().clone();
        let kart = world
            .spawn(kart_body(&tuning, Transform::from_xyz(0.0, 0.55, 0.0)))
            .id();

        // First update: startup only, the fixed loop has not accumulated any time yet.
        app.update();
        let mut scenario = Self { app, kart };
        // Let the suspension settle.
        scenario.run(0.5, KartInput::default());
        scenario
    }

    fn tuning(&self) -> KartTuning {
        self.app.world().resource::<KartTuning>().clone()
    }

    /// Runs `seconds` of simulation with a constant input, calling `each_tick` after each tick.
    fn run_with(&mut self, seconds: f32, input: KartInput, mut each_tick: impl FnMut(&Self)) {
        let ticks = (seconds * TICKS_PER_SECOND).round() as usize;
        for _ in 0..ticks {
            *self
                .app
                .world_mut()
                .get_mut::<KartInput>(self.kart)
                .unwrap() = input;
            self.app.update();
            each_tick(self);
        }
    }

    fn run(&mut self, seconds: f32, input: KartInput) {
        self.run_with(seconds, input, |_| {});
    }

    fn transform(&self) -> Transform {
        *self.app.world().get::<Transform>(self.kart).unwrap()
    }

    fn velocity(&self) -> Velocity {
        *self.app.world().get::<Velocity>(self.kart).unwrap()
    }

    fn state(&self) -> KartState {
        *self.app.world().get::<KartState>(self.kart).unwrap()
    }

    fn wheels(&self) -> KartWheels {
        *self.app.world().get::<KartWheels>(self.kart).unwrap()
    }

    /// Speed along the kart's heading (negative when driving backwards).
    fn forward_speed(&self) -> f32 {
        self.velocity().linear.dot(*self.transform().forward())
    }

    /// How upright the kart is: 1 = wheels down, 0 = on its side, -1 = upside down.
    fn uprightness(&self) -> f32 {
        self.transform().up().dot(Vec3::Y)
    }

    fn add_boost(&mut self, strength: f32, duration: f32) {
        self.app
            .world_mut()
            .get_mut::<Boost>(self.kart)
            .unwrap()
            .add(strength, duration);
    }
}

#[test]
fn kart_settles_on_its_wheels_at_rest() {
    let scenario = Scenario::flat(Surface::Road);
    assert_eq!(scenario.state(), KartState::Grounded);
    assert_eq!(scenario.wheels().wheels_on_ground(), 4);
    assert!(scenario.uprightness() > 0.99);
    assert!(scenario.velocity().linear.length() < 0.5);
    // The body floats on its springs instead of lying on the ground.
    let height = scenario.transform().translation.y;
    assert!(height > 0.4 && height < 0.7, "ride height {height}");
}

#[test]
fn full_throttle_reaches_95_percent_of_max_speed_in_2_5_s() {
    let mut scenario = Scenario::flat(Surface::Road);
    let max = scenario.tuning().max_speed;
    scenario.run(2.5, FULL_THROTTLE);
    let speed = scenario.forward_speed();
    assert!(speed >= 0.95 * max, "speed {speed} < 95 % of {max}");
    assert!(speed <= 1.01 * max, "speed {speed} above max {max}");
}

#[test]
fn offroad_caps_speed_around_55_percent() {
    let mut scenario = Scenario::flat(Surface::Offroad);
    let max = scenario.tuning().max_speed;
    let mut top = 0.0f32;
    scenario.run_with(5.0, FULL_THROTTLE, |s| top = top.max(s.forward_speed()));
    let speed = scenario.forward_speed();
    assert!(
        speed > 0.50 * max && speed < 0.60 * max,
        "off-road speed {speed} (max {max})"
    );
    assert!(top < 0.60 * max, "never above the off-road cap: {top}");
}

#[test]
fn boost_exceeds_max_speed_then_settles_back() {
    let mut scenario = Scenario::flat(Surface::Road);
    let max = scenario.tuning().max_speed;
    scenario.run(3.0, FULL_THROTTLE);
    scenario.add_boost(0.3, 1.5);

    let mut top = 0.0f32;
    scenario.run_with(1.5, FULL_THROTTLE, |s| top = top.max(s.forward_speed()));
    assert!(top > 1.2 * max, "boosted top speed {top} (max {max})");

    scenario.run(2.5, FULL_THROTTLE);
    let speed = scenario.forward_speed();
    assert!(
        (speed - max).abs() < 0.02 * max,
        "back to max speed after the boost: {speed} (max {max})"
    );
}

#[test]
fn brake_stops_then_reverses() {
    let mut scenario = Scenario::flat(Surface::Road);
    scenario.run(2.0, FULL_THROTTLE);
    let brake = KartInput {
        brake: 1.0,
        ..default()
    };
    scenario.run(4.0, brake);
    let speed = scenario.forward_speed();
    let reverse = scenario.tuning().reverse_max_speed;
    assert!(
        speed < -0.9 * reverse,
        "drives backwards after stopping: {speed}"
    );
}

#[test]
fn steering_3_s_at_full_speed_does_not_flip() {
    let mut scenario = Scenario::flat(Surface::Road);
    let max = scenario.tuning().max_speed;
    scenario.run(3.0, FULL_THROTTLE);
    let start = scenario.transform().translation;

    let mut lowest_uprightness = 1.0f32;
    let mut min_wheels = 4;
    let turn = KartInput {
        steer: 1.0,
        ..FULL_THROTTLE
    };
    scenario.run_with(3.0, turn, |s| {
        lowest_uprightness = lowest_uprightness.min(s.uprightness());
        min_wheels = min_wheels.min(s.wheels().wheels_on_ground());
    });

    assert!(
        lowest_uprightness > 0.95,
        "the kart tilted too much: {lowest_uprightness}"
    );
    assert!(min_wheels >= 2, "wheels left the ground: {min_wheels}");
    // It really turned: driving in circles keeps it close to where the turn started
    // (78 m away if it had gone straight), and it kept its speed.
    let distance = start.distance(scenario.transform().translation);
    assert!(distance < 40.0, "the kart barely turned: {distance} m");
    assert!(
        scenario.forward_speed() > 0.85 * max,
        "speed {}",
        scenario.forward_speed()
    );
}

#[test]
fn steering_right_turns_clockwise() {
    let mut scenario = Scenario::flat(Surface::Road);
    scenario.run(1.5, FULL_THROTTLE);
    let turn = KartInput {
        steer: 1.0,
        ..FULL_THROTTLE
    };
    scenario.run(0.3, turn);
    // Looking down -Z, a right turn heads toward +X.
    let heading = *scenario.transform().forward();
    assert!(heading.x > 0.2, "heading {heading}");
}

#[test]
fn ramp_jump_lands_on_the_wheels() {
    let angle = 12f32.to_radians();
    let mut scenario = Scenario::new(|commands| {
        spawn_static_box(
            commands,
            Transform::from_xyz(0.0, -0.5, 0.0),
            Vec3::new(500.0, 0.5, 500.0),
            Surface::Road,
            0.0,
        );
        spawn_static_box(
            commands,
            ramp_transform(Vec3::new(0.0, 0.0, -60.0), 8.0, angle, 0.5),
            Vec3::new(4.0, 0.5, 4.0),
            Surface::Road,
            0.0,
        );
    });

    let mut was_airborne = false;
    let mut highest = 0.0f32;
    let mut landed_upright = false;
    scenario.run_with(6.0, FULL_THROTTLE, |s| {
        if s.state() == KartState::Airborne {
            was_airborne = true;
            highest = highest.max(s.transform().translation.y);
        } else if was_airborne && s.wheels().wheels_on_ground() >= 3 {
            landed_upright = true;
        }
    });

    assert!(was_airborne, "the kart never left the ground");
    assert!(highest > 1.5, "jump too low: {highest} m");
    assert!(landed_upright, "the kart did not land on its wheels");
    assert_eq!(scenario.state(), KartState::Grounded);
    assert_eq!(scenario.wheels().wheels_on_ground(), 4);
    assert!(scenario.uprightness() > 0.98, "{}", scenario.uprightness());
    assert!(
        scenario.transform().translation.z < -100.0,
        "kept driving after landing"
    );
}

#[test]
fn head_on_wall_bounces_without_tunneling_or_flipping() {
    let mut scenario = Scenario::new(|commands| {
        spawn_static_box(
            commands,
            Transform::from_xyz(0.0, -0.5, 0.0),
            Vec3::new(500.0, 0.5, 500.0),
            Surface::Road,
            0.0,
        );
        spawn_static_box(
            commands,
            Transform::from_xyz(0.0, 1.0, -60.0),
            Vec3::new(10.0, 1.0, 0.5),
            Surface::Road,
            WALL_RESTITUTION,
        );
    });

    let mut lowest_uprightness = 1.0f32;
    scenario.run_with(5.0, FULL_THROTTLE, |s| {
        lowest_uprightness = lowest_uprightness.min(s.uprightness());
    });

    let z = scenario.transform().translation.z;
    assert!(z > -59.5, "went through the wall: z = {z}");
    assert!(
        lowest_uprightness > 0.7,
        "flipped on impact: {lowest_uprightness}"
    );
    // Not stuck: backing off the wall still works.
    let brake = KartInput {
        brake: 1.0,
        ..default()
    };
    scenario.run(2.0, brake);
    assert!(scenario.forward_speed() < -2.0);
}

#[test]
fn drives_off_from_a_standstill_on_a_slight_uphill() {
    // Regression (found on the circuit's starting grid): resting on a gentle slope, the kart
    // creeps backwards and the throttle used to only brake that, forever.
    let mut scenario = Scenario::new(|commands| {
        spawn_static_box(
            commands,
            Transform::from_xyz(0.0, -0.5, 0.0)
                .with_rotation(Quat::from_rotation_x(0.5f32.to_radians())),
            Vec3::new(500.0, 0.5, 500.0),
            Surface::Road,
            0.0,
        );
    });
    scenario.run(2.0, KartInput::default());
    scenario.run(1.0, FULL_THROTTLE);
    let speed = scenario.forward_speed();
    assert!(speed > 10.0, "stuck at {speed} m/s");
}
