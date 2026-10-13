//! Headless driving scenarios (acceptance criteria of issues #1, #4 and #13): the real
//! `SimulationPlugin` (Rapier + kart systems) at 60 Hz, one fixed tick per `app.update()`.

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::Velocity;

use super::*;
use crate::SimulationPlugin;
use crate::track::{
    BOOST_PAD_SIZE, BoostPad, Surface, TrickRamp, WALL_RESTITUTION, ramp_transform,
    spawn_static_box,
};
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

/// Issue #6: the top speed multiplier of the bots' catch-up (`MaxSpeedScale`) scales the top
/// speed, up and down.
#[test]
fn max_speed_scale_changes_the_top_speed() {
    let mut scenario = Scenario::flat(Surface::Road);
    let max = scenario.tuning().max_speed;
    let set_scale = |scenario: &mut Scenario, scale: f32| {
        scenario
            .app
            .world_mut()
            .get_mut::<MaxSpeedScale>(scenario.kart)
            .unwrap()
            .0 = scale;
    };
    set_scale(&mut scenario, 1.03);
    scenario.run(4.0, FULL_THROTTLE);
    let fast = scenario.forward_speed();
    assert!((fast - 1.03 * max).abs() < 0.05, "{fast} vs {}", 1.03 * max);
    set_scale(&mut scenario, 0.97);
    scenario.run(2.0, FULL_THROTTLE);
    let slow = scenario.forward_speed();
    assert!((slow - 0.97 * max).abs() < 0.05, "{slow} vs {}", 0.97 * max);
}

// --- Issue #13: stall, tricks, slipstream, boost pads -------------------------------------------

impl Scenario {
    /// Another kart on the arena at `position`, with the same heading as the first one.
    fn spawn_other_kart(&mut self, position: Vec3) -> Entity {
        let tuning = self.tuning();
        let rotation = self.transform().rotation;
        self.app
            .world_mut()
            .spawn(kart_body(
                &tuning,
                Transform::from_translation(position).with_rotation(rotation),
            ))
            .id()
    }

    fn tricks(&self) -> TrickTracker {
        *self.app.world().get::<TrickTracker>(self.kart).unwrap()
    }

    fn slipstream(&self) -> Slipstream {
        *self.app.world().get::<Slipstream>(self.kart).unwrap()
    }

    /// Drives at full throttle until the first tick in the air after leaving a trick ramp;
    /// returns the number of ticks it took.
    fn drive_off_the_trick_ramp(&mut self) -> u32 {
        let mut ticks = 0;
        while self.tricks().ticks_since_takeoff() != Some(1) {
            ticks += 1;
            assert!(ticks < 400, "never took off from the trick ramp");
            self.tick(FULL_THROTTLE);
        }
        ticks
    }

    /// Ticks (with `input`) until the kart lands; returns the ticks spent in the air and
    /// whether a boost was active at any point before the landing.
    fn until_landing(&mut self, input: KartInput) -> (u32, bool) {
        let mut ticks = 0;
        let mut boosted = false;
        while self.wheels().wheels_on_ground() == 0 {
            ticks += 1;
            assert!(ticks < 200, "never landed");
            boosted |= self.boost().is_active();
            self.tick(input);
        }
        (ticks, boosted)
    }
}

/// A flat slab with a trick ramp (8 m long, 12°) 40 m ahead of the kart.
fn trick_ramp_scenario() -> Scenario {
    Scenario::new(|commands| {
        spawn_static_box(
            commands,
            Transform::from_xyz(0.0, -0.5, 0.0),
            Vec3::new(500.0, 0.5, 500.0),
            Surface::Road,
            0.0,
        );
        let ramp = spawn_static_box(
            commands,
            ramp_transform(Vec3::new(0.0, 0.0, -40.0), 8.0, 12f32.to_radians(), 0.5),
            Vec3::new(4.0, 0.5, 4.0),
            Surface::Road,
            0.0,
        );
        commands.entity(ramp).insert(TrickRamp);
    })
}

/// Issue #13 acceptance: pressing jump/drift right after leaving a trick ramp starts a trick,
/// and the landing gives the trick boost (0.5 to 0.8 s, GD § 5).
#[test]
fn a_trick_off_a_ramp_gives_a_boost_on_landing() {
    let mut scenario = trick_ramp_scenario();
    let tuning = scenario.tuning();
    scenario.drive_off_the_trick_ramp();
    scenario.tick(KartInput {
        jump_drift: true,
        ..FULL_THROTTLE
    });
    assert_eq!(scenario.state(), KartState::Trick);
    assert!(!scenario.boost().is_active());
    let (air_ticks, boosted_in_the_air) = scenario.until_landing(FULL_THROTTLE);
    assert!(air_ticks > 10, "{air_ticks} ticks in the air");
    assert!(!boosted_in_the_air, "the boost comes on landing");
    assert_eq!(scenario.state(), KartState::Grounded);
    let boost = scenario.boost();
    assert_eq!(boost.strength, tuning.trick_boost_strength);
    assert!(
        (boost.remaining - tuning.trick_boost_duration).abs() < 1e-5,
        "{boost:?}"
    );
    assert!((0.5..=0.8).contains(&tuning.trick_boost_duration));
    // It lasts its full duration, to the tick.
    let mut active_ticks = 1;
    while scenario.boost().is_active() {
        scenario.tick(FULL_THROTTLE);
        if scenario.boost().is_active() {
            active_ticks += 1;
        }
        assert!(active_ticks < 120);
    }
    let seconds = active_ticks as f32 / TICKS_PER_SECOND;
    assert!(
        (seconds - tuning.trick_boost_duration).abs() <= 1.0 / TICKS_PER_SECOND,
        "{seconds} s"
    );
    // The kart landed on its wheels.
    scenario.run(0.5, FULL_THROTTLE);
    assert!(scenario.uprightness() > 0.98);
}

/// Issue #13 acceptance: a missed trick (no press) gives no boost; pressing too late (well
/// after the window) neither, and does not hop in the air.
#[test]
fn a_missed_trick_gives_no_boost() {
    let mut scenario = trick_ramp_scenario();
    scenario.drive_off_the_trick_ramp();
    let mut tricked = false;
    let mut ticks = 0;
    while scenario.wheels().wheels_on_ground() == 0 {
        ticks += 1;
        assert!(ticks < 200);
        scenario.tick(FULL_THROTTLE);
        tricked |= scenario.state() == KartState::Trick;
    }
    assert!(!tricked);
    scenario.run(1.0, FULL_THROTTLE);
    assert!(!scenario.boost().is_active(), "no trick, no boost");

    // Too late: 14 ticks after the takeoff, the window (12 ticks) is closed.
    let mut scenario = trick_ramp_scenario();
    let window = (scenario.tuning().trick_window * TICKS_PER_SECOND).round() as u32;
    assert_eq!(window, 12);
    scenario.drive_off_the_trick_ramp();
    scenario.run(12.0 / TICKS_PER_SECOND, FULL_THROTTLE);
    assert_eq!(scenario.tricks().ticks_since_takeoff(), Some(13));
    scenario.tick(KartInput {
        jump_drift: true,
        ..FULL_THROTTLE
    });
    assert_ne!(scenario.state(), KartState::Trick);
    assert!(!matches!(scenario.state(), KartState::Hop { .. }));
    let (_, boosted) = scenario.until_landing(FULL_THROTTLE);
    assert!(!boosted);
    scenario.run(1.0, FULL_THROTTLE);
    assert!(!scenario.boost().is_active());
}

/// Issue #13: pressing just before the lip, still on the ramp, does not hop off the ramp: the
/// trick starts on the first tick in the air.
#[test]
fn pressing_on_the_lip_of_the_ramp_starts_the_trick_at_the_takeoff() {
    // Same run without pressing: when the kart takes off.
    let takeoff = trick_ramp_scenario().drive_off_the_trick_ramp();
    let mut scenario = trick_ramp_scenario();
    // Pressed 4 ticks before the first tick in the air (on the ramp), then held.
    scenario.run((takeoff - 4) as f32 / TICKS_PER_SECOND, FULL_THROTTLE);
    assert!(scenario.tricks().on_ramp(), "not on the ramp yet");
    let held = KartInput {
        jump_drift: true,
        ..FULL_THROTTLE
    };
    for _ in 0..3 {
        scenario.tick(held);
        assert_eq!(scenario.state(), KartState::Grounded, "no hop on the ramp");
    }
    scenario.tick(held);
    assert_eq!(scenario.tricks().ticks_since_takeoff(), Some(1));
    assert_eq!(scenario.state(), KartState::Trick);
    scenario.until_landing(FULL_THROTTLE);
    assert!(scenario.boost().is_active());
}

/// Issue #13 acceptance: right behind a kart at constant speed, the slipstream gauge charges
/// and gives a 1 s boost after 1.2 s (GD § 6); the kart in front gets nothing.
#[test]
fn slipstream_behind_a_kart_at_constant_speed_boosts_after_1_2_s() {
    let mut scenario = Scenario::flat(Surface::Road);
    let tuning = scenario.tuning();
    // 4 m ahead: the kart looks down -Z.
    let leader = scenario.spawn_other_kart(Vec3::new(0.0, 0.55, -4.0));
    scenario.run(0.5, KartInput::default());
    // Both at top speed, full throttle: constant speed, constant gap.
    let cruise = Vec3::NEG_Z * tuning.max_speed;
    for kart in [scenario.kart, leader] {
        let world = scenario.app.world_mut();
        world.get_mut::<Velocity>(kart).unwrap().linear = cruise;
        *world.get_mut::<KartInput>(kart).unwrap() = FULL_THROTTLE;
    }
    let mut boosted_at = None;
    for tick in 1..=90 {
        scenario.tick(FULL_THROTTLE);
        let world = scenario.app.world();
        assert!(!world.get::<Boost>(leader).unwrap().is_active());
        assert!(!world.get::<Slipstream>(leader).unwrap().charging);
        if scenario.boost().is_active() {
            boosted_at = Some(tick);
            break;
        }
        assert!(scenario.slipstream().charging, "tick {tick}");
        let gap =
            world.get::<Transform>(leader).unwrap().translation - scenario.transform().translation;
        assert!((gap.length() - 4.0).abs() < 0.1, "gap {gap}");
    }
    // 1.2 s = 72 ticks.
    assert_eq!(boosted_at, Some(72));
    let boost = scenario.boost();
    assert_eq!(boost.strength, tuning.slipstream_boost_strength);
    assert!((boost.remaining - tuning.slipstream_boost_duration).abs() < 1e-5);
    assert_eq!(scenario.slipstream().charge, 0.0, "the gauge starts again");
}

/// Issue #13: below 60 % of its top speed, a kart right behind another does not charge.
#[test]
fn no_slipstream_when_too_slow() {
    let mut scenario = Scenario::flat(Surface::Road);
    let tuning = scenario.tuning();
    let leader = scenario.spawn_other_kart(Vec3::new(0.0, 0.55, -4.0));
    scenario.run(0.5, KartInput::default());
    let slow = Vec3::NEG_Z * tuning.max_speed * 0.5;
    let coast = KartInput::default();
    for kart in [scenario.kart, leader] {
        let world = scenario.app.world_mut();
        world.get_mut::<Velocity>(kart).unwrap().linear = slow;
        *world.get_mut::<KartInput>(kart).unwrap() = coast;
    }
    // Coasting from half speed, 4 m behind: never fast enough.
    scenario.run_with(2.0, coast, |s| {
        assert!(!s.slipstream().charging);
        assert!(!s.boost().is_active());
    });
}

/// Issue #13: rolling over a boost pad gives a boost that lasts about 1 s after the pad
/// (GD § 2); driving past it, beside it, gives nothing.
/// A flat slab with a boost pad 30 m ahead of the kart, `x` meters to its right.
fn pad_arena(commands: &mut Commands, x: f32) {
    spawn_static_box(
        commands,
        Transform::from_xyz(0.0, -0.5, 0.0),
        Vec3::new(500.0, 0.5, 500.0),
        Surface::Road,
        0.0,
    );
    commands.spawn((
        Transform::from_xyz(x, 0.03, -30.0),
        BoostPad {
            half_size: BOOST_PAD_SIZE / 2.0,
        },
    ));
}

#[test]
fn a_boost_pad_gives_a_boost_of_about_one_second() {
    let mut scenario = Scenario::new(|commands| pad_arena(commands, 0.0));
    let tuning = scenario.tuning();
    let max = tuning.max_speed;
    let mut on_pad_ticks = 0;
    let mut after_pad_ticks = 0;
    let mut top = 0.0f32;
    for _ in 0..(4.0 * TICKS_PER_SECOND) as usize {
        scenario.tick(FULL_THROTTLE);
        let boost = scenario.boost();
        top = top.max(scenario.forward_speed());
        if (boost.remaining - tuning.boost_pad_duration).abs() < 1e-5 {
            // Refreshed this tick: a wheel is on the pad.
            assert_eq!(boost.strength, tuning.boost_pad_strength);
            on_pad_ticks += 1;
            after_pad_ticks = 0;
        } else if boost.is_active() {
            after_pad_ticks += 1;
        }
    }
    assert!(
        (3..20).contains(&on_pad_ticks),
        "{on_pad_ticks} ticks on the 4 m pad"
    );
    // About 1 s after the last tick on the pad (60 ticks; the last one counted on the pad).
    assert!((58..=60).contains(&after_pad_ticks), "{after_pad_ticks}");
    assert!(top > 1.1 * max, "top speed {top} (max {max})");

    // Beside the pad (4 m to the right; the pad is 3 m wide): nothing.
    let mut scenario = Scenario::new(|commands| pad_arena(commands, 4.0));
    scenario.run_with(4.0, FULL_THROTTLE, |s| assert!(!s.boost().is_active()));
    assert!(scenario.transform().translation.z < -40.0, "drove past it");
}

/// Issue #13: a stalled engine (rocket start pressed too early) keeps the kart still for its
/// duration, whatever the driver does, then it drives off.
#[test]
fn a_stalled_engine_keeps_the_kart_still_then_lets_it_go() {
    let mut scenario = Scenario::flat(Surface::Road);
    let start = scenario.transform().translation;
    let stall = scenario.tuning().rocket_start_stall;
    scenario
        .app
        .world_mut()
        .get_mut::<Stall>(scenario.kart)
        .unwrap()
        .remaining = stall;
    let pressing = KartInput {
        jump_drift: true,
        ..FULL_THROTTLE
    };
    let mut still_ticks = 0;
    while scenario
        .app
        .world()
        .get::<Stall>(scenario.kart)
        .unwrap()
        .is_active()
    {
        scenario.tick(pressing);
        still_ticks += 1;
        assert!(still_ticks <= 61);
        let moved = (scenario.transform().translation - start)
            .with_y(0.0)
            .length();
        // (The last tick may already push off: the stall ends at the start of a tick.)
        assert!(moved < 0.02, "moved {moved} m while stalled");
        assert_eq!(
            scenario.state(),
            KartState::Grounded,
            "no hop while stalled"
        );
    }
    // Set between two ticks, the stall is counted down before the first one drives: 59
    // stalled ticks here (on GO, the race sets it after the countdown: 60).
    assert_eq!(still_ticks, 60, "the 60th tick already drives");
    scenario.run(0.5, FULL_THROTTLE);
    assert!(
        scenario.forward_speed() > 8.0,
        "{}",
        scenario.forward_speed()
    );
}

// --- Issue #14: bumps between karts ---------------------------------------------------------

impl Scenario {
    /// Every `KartBump` of the next `seconds`, `inputs` held on the given karts.
    fn bumps_while(&mut self, seconds: f32, inputs: &[(Entity, KartInput)]) -> Vec<KartBump> {
        let mut cursor = self
            .app
            .world()
            .resource::<Messages<KartBump>>()
            .get_cursor_current();
        let mut bumps = Vec::new();
        for _ in 0..(seconds * TICKS_PER_SECOND).round() as usize {
            for (kart, input) in inputs {
                *self.app.world_mut().get_mut::<KartInput>(*kart).unwrap() = *input;
            }
            self.app.update();
            let messages = self.app.world().resource::<Messages<KartBump>>();
            bumps.extend(cursor.read(messages).copied());
        }
        bumps
    }
}

/// Issue #14: rear-ending a kart standing still is one bump, for the kart in front only, measured
/// at the speed of the impact; pushing it along afterwards is no new bump.
#[test]
fn rear_ending_a_kart_bumps_it_once() {
    let mut scenario = Scenario::flat(Surface::Road);
    let bumper = scenario.kart;
    let victim = scenario.spawn_other_kart(Vec3::new(0.0, 0.55, -12.0));
    scenario.run(0.3, KartInput::default());
    let bumps = scenario.bumps_while(
        2.5,
        &[(bumper, FULL_THROTTLE), (victim, KartInput::default())],
    );
    assert_eq!(bumps.len(), 1, "{bumps:?}");
    assert_eq!((bumps[0].kart, bumps[0].by), (victim, bumper));
    // About 10 m of run-up from a standstill: ≈ 17 m/s.
    assert!(
        (12.0..22.0).contains(&bumps[0].closing_speed),
        "{}",
        bumps[0].closing_speed
    );
    let pushed = scenario
        .app
        .world()
        .get::<Transform>(victim)
        .unwrap()
        .translation
        .z;
    assert!(pushed < -20.0, "pushed along: {pushed}");
}

/// Issue #14: two karts driving side by side, one leaning gently into the other: rubbing, no
/// bump.
#[test]
fn rubbing_side_by_side_is_no_bump() {
    let mut scenario = Scenario::flat(Surface::Road);
    let left = scenario.kart;
    let right = scenario.spawn_other_kart(Vec3::new(1.6, 0.55, 0.0));
    scenario.run(0.3, KartInput::default());
    let lean = KartInput {
        steer: 0.25,
        ..FULL_THROTTLE
    };
    let bumps = scenario.bumps_while(3.0, &[(left, lean), (right, FULL_THROTTLE)]);
    assert!(bumps.is_empty(), "{bumps:?}");
    // They did touch: the right one was pushed aside.
    let x = scenario
        .app
        .world()
        .get::<Transform>(right)
        .unwrap()
        .translation
        .x;
    assert!(x > 1.8, "{x}");
}

/// Issue #14: head on, both karts are bumped.
#[test]
fn a_head_on_crash_bumps_both_karts() {
    let mut scenario = Scenario::flat(Surface::Road);
    let a = scenario.kart;
    let tuning = scenario.tuning();
    let b = scenario
        .app
        .world_mut()
        .spawn(kart_body(
            &tuning,
            Transform::from_xyz(0.0, 0.55, -30.0).looking_to(Vec3::Z, Vec3::Y),
        ))
        .id();
    scenario.run(0.3, KartInput::default());
    let bumps = scenario.bumps_while(2.5, &[(a, FULL_THROTTLE), (b, FULL_THROTTLE)]);
    let mut bumped: Vec<Entity> = bumps.iter().map(|bump| bump.kart).collect();
    bumped.sort();
    bumped.dedup();
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(bumped, both, "{bumps:?}");
    assert!(bumps[0].closing_speed > 20.0, "{bumps:?}");
}
