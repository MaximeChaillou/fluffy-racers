//! Headless driving scenarios (acceptance criteria of issues #1 and #4): the real
//! `SimulationPlugin` (Rapier + kart systems) at 60 Hz, one fixed tick per `app.update()`.

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::track::{Surface, WALL_RESTITUTION, ramp_transform, spawn_static_box};
use drift::{DriftSide, MiniTurbo};

const TICKS_PER_SECOND: f32 = 60.0;
const FULL_THROTTLE: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
};

/// Full throttle, jump/drift button held, steering `steer`.
fn drifting(steer: f32) -> KartInput {
    KartInput {
        steer,
        jump_drift: true,
        ..FULL_THROTTLE
    }
}

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

    fn boost(&self) -> Boost {
        *self.app.world().get::<Boost>(self.kart).unwrap()
    }

    /// Mini-turbo tier reached by the current drift, if any.
    fn tier(&self) -> Option<MiniTurbo> {
        let assists = *self.app.world().get::<KartAssists>(self.kart).unwrap();
        self.state()
            .mini_turbo(&self.tuning(), assists.assisted_steering)
    }

    /// Runs a single tick.
    fn tick(&mut self, input: KartInput) {
        self.run(1.0 / TICKS_PER_SECOND, input);
    }

    /// Presses the jump/drift button steering with `hop_steer`, then keeps it held steering
    /// with `landing_steer` until the kart lands. Returns the state after landing and the
    /// number of ticks the hop took.
    fn hop(&mut self, hop_steer: f32, landing_steer: f32, throttle: f32) -> (KartState, u32) {
        let input = |steer| KartInput {
            throttle,
            steer,
            jump_drift: true,
            ..default()
        };
        self.tick(input(hop_steer));
        assert!(
            matches!(self.state(), KartState::Hop { .. }),
            "no hop: {:?}",
            self.state()
        );
        let mut ticks = 1;
        while matches!(self.state(), KartState::Hop { .. }) {
            assert!(ticks < 60, "the hop never landed");
            self.tick(input(landing_steer));
            ticks += 1;
        }
        (self.state(), ticks)
    }

    /// Full throttle for 2.5 s, then a hop steered to the right that lands in a right drift.
    /// The landing tick (the first drifting tick) used `landing_steer`.
    fn enter_right_drift(&mut self, landing_steer: f32) {
        self.run(2.5, FULL_THROTTLE);
        let (state, _) = self.hop(1.0, landing_steer, 1.0);
        assert!(
            matches!(
                state,
                KartState::Drifting {
                    side: DriftSide::Right,
                    ..
                }
            ),
            "{state:?}"
        );
    }

    /// Keeps drifting with `steer` until `tier` is reached; returns the number of ticks spent
    /// drifting, the landing tick being the first one.
    fn drift_until(&mut self, steer: f32, tier: MiniTurbo) -> u32 {
        let mut ticks = 1;
        while self.tier() != Some(tier) {
            assert!(ticks < 600, "{tier:?} never reached");
            self.tick(drifting(steer));
            assert!(
                matches!(self.state(), KartState::Drifting { .. }),
                "the drift broke after {ticks} ticks: {:?}",
                self.state()
            );
            ticks += 1;
        }
        ticks
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

// --- Issue #4: hop, drift and mini-turbos ------------------------------------------------------

#[test]
fn hop_is_a_short_jump_that_keeps_the_speed() {
    let mut scenario = Scenario::flat(Surface::Road);
    let max = scenario.tuning().max_speed;
    scenario.run(2.5, FULL_THROTTLE);
    let rest_height = scenario.transform().translation.y;
    let mut highest = rest_height;
    let mut wheels_left_ground = false;
    let mut hop_ticks = 0;
    scenario.tick(KartInput {
        jump_drift: true,
        ..FULL_THROTTLE
    });
    while matches!(scenario.state(), KartState::Hop { .. }) {
        assert!(hop_ticks < 60, "the hop never landed");
        highest = highest.max(scenario.transform().translation.y);
        wheels_left_ground |= scenario.wheels().wheels_on_ground() == 0;
        scenario.tick(FULL_THROTTLE);
        hop_ticks += 1;
    }
    let hop_height = highest - rest_height;
    assert!(wheels_left_ground, "the wheels never left the ground");
    assert!(
        hop_height > 0.15 && hop_height < 0.6,
        "hop height {hop_height} m"
    );
    assert!(
        (10..=30).contains(&hop_ticks),
        "hop lasted {hop_ticks} ticks"
    );
    // No steering during the hop: no drift, and the kart lands at full speed on its wheels.
    assert_eq!(scenario.state(), KartState::Grounded);
    scenario.run(0.3, FULL_THROTTLE);
    assert_eq!(scenario.wheels().wheels_on_ground(), 4);
    assert!(scenario.uprightness() > 0.99);
    assert!(
        scenario.forward_speed() > 0.97 * max,
        "{}",
        scenario.forward_speed()
    );
}

#[test]
fn inward_drift_reaches_blue_orange_and_purple_at_54_114_and_174_ticks() {
    let mut scenario = Scenario::flat(Surface::Road);
    let max = scenario.tuning().max_speed;
    scenario.enter_right_drift(1.0);
    let start_heading = *scenario.transform().forward();
    let mut lowest_speed = f32::MAX;
    let mut first_tick = [None; 3];
    let mut ticks = 1;
    loop {
        if let Some(tier) = scenario.tier() {
            first_tick[tier.index()].get_or_insert(ticks);
        }
        if ticks == 200 {
            break;
        }
        scenario.tick(drifting(1.0));
        ticks += 1;
        lowest_speed = lowest_speed.min(scenario.forward_speed());
        assert!(
            matches!(scenario.state(), KartState::Drifting { .. }),
            "the drift broke after {ticks} ticks: {:?}",
            scenario.state()
        );
    }
    assert_eq!(first_tick, [Some(54), Some(114), Some(174)]);
    // A tight drift does not slow the kart down, and it really turned (to the right).
    assert!(lowest_speed > 0.95 * max, "{lowest_speed}");
    let turned = start_heading.angle_between(*scenario.transform().forward());
    assert!(turned > 1.0, "turned by {turned} rad only");
    assert!(scenario.uprightness() > 0.95, "{}", scenario.uprightness());
}

#[test]
fn outward_drift_charges_2_5_times_slower() {
    let mut scenario = Scenario::flat(Surface::Road);
    // Land with the stick back to neutral (keeps the right side), then steer outward.
    scenario.enter_right_drift(0.0);
    let outward = scenario.drift_until(-1.0, MiniTurbo::Blue);
    assert_eq!(outward, 135, "2.5 × 54 ticks");

    let mut scenario = Scenario::flat(Surface::Road);
    scenario.enter_right_drift(1.0);
    let inward = scenario.drift_until(1.0, MiniTurbo::Blue);
    assert_eq!(inward, 54);
    let ratio = outward as f32 / inward as f32;
    assert!((ratio - 2.5).abs() < 0.05, "{ratio}");
}

#[test]
fn releasing_at_each_tier_fires_a_boost_of_the_right_duration() {
    for tier in MiniTurbo::ALL {
        let mut scenario = Scenario::flat(Surface::Road);
        let tuning = scenario.tuning();
        let max = tuning.max_speed;
        scenario.enter_right_drift(1.0);
        scenario.drift_until(1.0, tier);
        assert!(!scenario.boost().is_active(), "no boost before releasing");

        scenario.tick(FULL_THROTTLE);
        let boost = scenario.boost();
        let duration = tuning.mini_turbo_durations[tier.index()];
        assert_eq!(scenario.state(), KartState::Grounded, "{tier:?}");
        assert_eq!(boost.strength, tuning.mini_turbo_strengths[tier.index()]);
        assert!(
            (boost.remaining - duration).abs() < 1e-5,
            "{tier:?}: {boost:?}"
        );

        // It lasts its full duration (to the tick), and the kart goes past its max speed.
        let mut active_ticks = 1;
        let mut top = scenario.forward_speed();
        while scenario.boost().is_active() {
            assert!(active_ticks < 300);
            scenario.tick(FULL_THROTTLE);
            top = top.max(scenario.forward_speed());
            if scenario.boost().is_active() {
                active_ticks += 1;
            }
        }
        let seconds = active_ticks as f32 / TICKS_PER_SECOND;
        assert!(
            (seconds - duration).abs() <= 1.0 / TICKS_PER_SECOND,
            "{tier:?}: boost lasted {seconds} s instead of {duration} s"
        );
        assert!(top > 1.05 * max, "{tier:?}: top speed {top} (max {max})");
    }
}

#[test]
fn releasing_before_blue_gives_nothing() {
    let mut scenario = Scenario::flat(Surface::Road);
    scenario.enter_right_drift(1.0);
    // 53 drifting ticks: one short of blue.
    scenario.run(52.0 / TICKS_PER_SECOND, drifting(1.0));
    assert!(matches!(scenario.state(), KartState::Drifting { .. }));
    assert_eq!(scenario.tier(), None);
    scenario.tick(FULL_THROTTLE);
    assert_eq!(scenario.state(), KartState::Grounded);
    assert!(!scenario.boost().is_active());
}

#[test]
fn cannot_drift_at_a_standstill_or_too_slow() {
    // Standing still: the button hops, but the kart lands without drifting.
    let mut scenario = Scenario::flat(Surface::Road);
    let (state, _) = scenario.hop(1.0, 1.0, 0.0);
    assert_eq!(state, KartState::Grounded);
    let mut ever_drifted = false;
    scenario.run_with(2.0, drifting(1.0), |s| {
        ever_drifted |= matches!(s.state(), KartState::Drifting { .. });
    });
    assert!(!ever_drifted);

    // Rolling slower than the minimum drift speed.
    let mut scenario = Scenario::flat(Surface::Road);
    let min = scenario.tuning().drift_min_speed;
    scenario.run_with(2.0, FULL_THROTTLE, |_| {});
    let brake = KartInput {
        brake: 1.0,
        ..default()
    };
    while scenario.forward_speed() > 0.6 * min {
        scenario.tick(brake);
    }
    let (state, _) = scenario.hop(1.0, 1.0, 0.0);
    assert_eq!(
        state,
        KartState::Grounded,
        "speed {}",
        scenario.forward_speed()
    );
}

#[test]
fn drift_always_turns_toward_its_side_tighter_inward_wider_outward() {
    // Settled yaw rate (rad/s) of a right drift steered with `steer`.
    let yaw_rate = |steer: f32| {
        let mut scenario = Scenario::flat(Surface::Road);
        scenario.enter_right_drift(steer.max(0.0));
        scenario.run(1.0, drifting(steer));
        assert!(matches!(
            scenario.state(),
            KartState::Drifting {
                side: DriftSide::Right,
                ..
            }
        ));
        (scenario.velocity().angular.y, scenario.forward_speed())
    };
    let (tight, tight_speed) = yaw_rate(1.0);
    let (neutral, _) = yaw_rate(0.0);
    let (wide, wide_speed) = yaw_rate(-1.0);
    // Clockwise (right) in every case, even steering full left.
    assert!(
        tight < neutral && neutral < wide && wide < 0.0,
        "{tight} {neutral} {wide}"
    );
    // Radii close to the tuning at full speed.
    let tuning = KartTuning::default();
    let tight_radius = tight_speed / -tight;
    let wide_radius = wide_speed / -wide;
    assert!(
        (tight_radius - tuning.drift_radius_tight).abs() < 1.0,
        "tight radius {tight_radius} m"
    );
    assert!(
        (wide_radius - tuning.drift_radius_wide).abs() < 3.0,
        "wide radius {wide_radius} m"
    );
}

#[test]
fn braking_hard_breaks_the_drift_and_loses_the_charge() {
    let mut scenario = Scenario::flat(Surface::Road);
    scenario.enter_right_drift(1.0);
    scenario.drift_until(1.0, MiniTurbo::Blue);
    let brake_in_drift = KartInput {
        brake: 1.0,
        steer: 1.0,
        jump_drift: true,
        ..default()
    };
    let mut ticks = 0;
    while matches!(scenario.state(), KartState::Drifting { .. }) {
        assert!(ticks < 120, "the drift never broke");
        scenario.tick(brake_in_drift);
        ticks += 1;
    }
    assert!(scenario.forward_speed() < scenario.tuning().drift_exit_speed);
    // Releasing now gives nothing.
    scenario.tick(KartInput::default());
    assert!(!scenario.boost().is_active());
}

#[test]
fn assisted_steering_charges_slower_and_never_reaches_purple() {
    let mut scenario = Scenario::flat(Surface::Road);
    scenario
        .app
        .world_mut()
        .entity_mut(scenario.kart)
        .insert(KartAssists {
            assisted_steering: true,
        });
    scenario.enter_right_drift(1.0);
    // 270 / 2 and 570 / 2 ticks.
    let blue = scenario.drift_until(1.0, MiniTurbo::Blue);
    assert_eq!(blue, 135);
    // `drift_until` counts the current tick as its first one.
    let orange = blue - 1 + scenario.drift_until(1.0, MiniTurbo::Orange);
    assert_eq!(orange, 285);
    let mut purple = false;
    scenario.run_with(5.0, drifting(1.0), |s| {
        purple |= s.tier() == Some(MiniTurbo::Purple);
    });
    assert!(matches!(scenario.state(), KartState::Drifting { .. }));
    assert!(!purple, "purple is disabled with assisted steering");
    // Releasing fires the orange mini-turbo.
    scenario.tick(FULL_THROTTLE);
    let duration = scenario.tuning().mini_turbo_durations[MiniTurbo::Orange.index()];
    assert!((scenario.boost().remaining - duration).abs() < 1e-5);
}
