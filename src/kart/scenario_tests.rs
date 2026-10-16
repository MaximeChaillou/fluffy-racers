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
use drift::{DriftMotion, DriftSide, MiniTurbo, drift_yaw_rate, slip_pose};
use stats::{KartStats, Stat, StatEffects, StatsTuning};

const TICKS_PER_SECOND: f32 = 60.0;
const FULL_THROTTLE: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
    interact: false,
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

    /// Mini-turbo tier reached by the current drift, if any (with the kart's own thresholds:
    /// its Mini-turbo stat moves them, issue #15).
    fn tier(&self) -> Option<MiniTurbo> {
        let assists = *self.app.world().get::<KartAssists>(self.kart).unwrap();
        self.state()
            .mini_turbo(&self.kart_tuning(), assists.assisted_steering)
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
            Surface::Wall,
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

// --- Issue #30: a fluid drift ------------------------------------------------------------------
//
// Max, after playing: "le drift est beaucoup trop brusque ; il doit être fluide par rapport au
// mouvement déjà en cours, avec l'arrière beaucoup plus léger et glissant, sans prendre un angle
// serré". In numbers, below. For reference, the drift of #4 jumped on landing: its yaw rate
// target went straight to the 10 m drift's (2.85 rad/s), +0.17 rad/s in the landing tick, and the
// visual slip angle went to 26° at 10 /s, so the body's turn rate jumped by about 4 rad/s in one
// tick, on landing and again on release.

/// Wraps an angle difference into `[-π, π]`.
fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Angle of a horizontal direction, counter-clockwise from -Z like the yaw rate (rad).
fn heading_of(direction: Vec3) -> f32 {
    (-direction.x).atan2(-direction.z)
}

/// One tick of motion, as recorded by `Scenario::record`.
#[derive(Clone, Copy, Debug)]
struct Sample {
    state: KartState,
    /// Yaw rate of the simulated body (rad/s, counter-clockwise positive).
    yaw: f32,
    /// Turn rate of the direction of travel over the tick (rad/s).
    travel_rate: f32,
    /// Turn rate of the visible body (the simulated one turned by the slip angle) (rad/s).
    body_rate: f32,
    motion: DriftMotion,
    speed: f32,
}

/// Largest change from one sample to the next of `value` (rad/s per tick).
fn largest_step(samples: &[Sample], value: impl Fn(&Sample) -> f32) -> f32 {
    samples
        .windows(2)
        .map(|pair| (value(&pair[1]) - value(&pair[0])).abs())
        .fold(0.0, f32::max)
}

impl Scenario {
    fn motion(&self) -> DriftMotion {
        *self.app.world().get::<DriftMotion>(self.kart).unwrap()
    }

    /// Heading of the visible body: the simulated body turned by the slip angle (positive = nose
    /// to the right, clockwise).
    fn body_heading(&self) -> f32 {
        heading_of(*self.transform().forward()) - self.motion().slip
    }

    /// Runs `ticks` ticks with `input(tick)`, recording each one.
    fn record(&mut self, ticks: u32, input: impl Fn(u32) -> KartInput) -> Vec<Sample> {
        let mut samples = Vec::new();
        let mut travel = heading_of(self.velocity().linear);
        let mut body = self.body_heading();
        for tick in 0..ticks {
            self.tick(input(tick));
            let (new_travel, new_body) = (heading_of(self.velocity().linear), self.body_heading());
            samples.push(Sample {
                state: self.state(),
                yaw: self.velocity().angular.y,
                travel_rate: wrap(new_travel - travel) * TICKS_PER_SECOND,
                body_rate: wrap(new_body - body) * TICKS_PER_SECOND,
                motion: self.motion(),
                speed: self.forward_speed(),
            });
            (travel, body) = (new_travel, new_body);
        }
        samples
    }

    /// World positions of the front and rear axles of the visible body (`slip_pose`).
    fn visible_axles(&self) -> (Vec3, Vec3) {
        let (rotation, offset) = slip_pose(self.motion().slip);
        let body = self.transform() * Transform::from_translation(offset).with_rotation(rotation);
        let front = body.transform_point(Vec3::Z * WHEEL_ANCHORS[0].z);
        let rear = body.transform_point(Vec3::Z * WHEEL_ANCHORS[2].z);
        (front, rear)
    }
}

/// Issue #30: the hop and the drift set in from the motion in progress. Keyboard-like input at
/// top speed: jump/drift pressed with the stick fully right, held, for 1.5 s. No tick-to-tick
/// jump of the yaw rate, of the turn rate of the direction of travel or of the visible body, and
/// the drift's curve takes a fraction of a second to set in.
#[test]
fn the_drift_sets_in_progressively_from_the_motion_in_progress() {
    let mut scenario = Scenario::flat(Surface::Road);
    let tuning = scenario.tuning();
    scenario.run(2.5, FULL_THROTTLE);
    let samples = scenario.record(90, |_| drifting(1.0));
    let landing = samples
        .iter()
        .position(|s| matches!(s.state, KartState::Drifting { .. }))
        .expect("never drifted");
    assert!(
        matches!(samples[landing - 1].state, KartState::Hop { .. }),
        "{:?}",
        samples[landing - 1]
    );
    // Bounds per tick (60 Hz), hop and landing included: 0.12 rad/s is 7 rad/s². The old drift
    // jumped by 0.17 rad/s (yaw) and about 4 rad/s (visible body) on landing; the normal steering
    // at full lock and top speed changes the yaw rate by 0.2 rad/s on the first tick.
    let yaw_step = largest_step(&samples, |s| s.yaw);
    let travel_step = largest_step(&samples, |s| s.travel_rate);
    let body_step = largest_step(&samples, |s| s.body_rate);
    assert!(
        yaw_step <= 0.12,
        "yaw rate jumped by {yaw_step} rad/s in a tick"
    );
    assert!(
        travel_step <= 0.12,
        "travel turn rate jumped by {travel_step}"
    );
    assert!(body_step <= 0.15, "body turn rate jumped by {body_step}");
    // Landing: the drift starts from the rotation in progress (no step at all).
    let landing_step = (samples[landing].yaw - samples[landing - 1].yaw).abs();
    assert!(
        landing_step <= 0.01,
        "landing changed the yaw rate by {landing_step}"
    );
    // The curve sets in: on landing the kart turns clearly less than the drift will, then reaches
    // 90 % of the drift's yaw rate after 0.2 s at the earliest and 0.8 s at the latest.
    let drift = drift_yaw_rate(&tuning, DriftSide::Right, 1.0, samples[landing].speed);
    assert!(
        samples[landing].yaw.abs() < 0.85 * drift.abs(),
        "{} vs {drift}",
        samples[landing].yaw
    );
    let set_in = samples[landing..]
        .iter()
        .position(|s| s.yaw.abs() >= 0.9 * drift.abs())
        .expect("the drift's curve never set in") as f32
        / TICKS_PER_SECOND;
    assert!((0.2..=0.8).contains(&set_in), "set in after {set_in} s");
    let last = samples.last().unwrap();
    assert!((last.yaw - drift).abs() < 0.02, "{} vs {drift}", last.yaw);
    assert!(last.speed > 0.97 * tuning.max_speed, "{}", last.speed);
}

/// Issue #30: the rear breaks loose and slides out of the turn, progressively, like a damped
/// spring; the front axle stays on the trajectory.
#[test]
fn the_rear_slides_out_progressively_while_the_front_follows_the_trajectory() {
    let mut scenario = Scenario::flat(Surface::Road);
    let tuning = scenario.tuning();
    scenario.enter_right_drift(1.0);
    let target = tuning.drift_slip_angle_tight;
    let (mut front, mut rear) = scenario.visible_axles();
    let mut slips = vec![scenario.motion().slip];
    let mut settled_checks = 0;
    for _ in 0..90 {
        scenario.tick(drifting(1.0));
        let slip = scenario.motion().slip;
        slips.push(slip);
        let (new_front, new_rear) = scenario.visible_axles();
        // The front axle of the visible body is the simulated body's: on the trajectory.
        let simulated_front = scenario
            .transform()
            .transform_point(Vec3::Z * WHEEL_ANCHORS[0].z);
        assert!(new_front.distance(simulated_front) < 1e-3);
        // Sideways speed of each axle across the visible body (right positive). A right drift:
        // the outside of the turn is on the left.
        let (rotation, _) = slip_pose(slip);
        let right = scenario.transform().rotation * rotation * Vec3::X;
        let front_lateral = (new_front - front).dot(right) * TICKS_PER_SECOND;
        let rear_lateral = (new_rear - rear).dot(right) * TICKS_PER_SECOND;
        if slip > 0.5 * target {
            settled_checks += 1;
            assert!(
                rear_lateral < -1.0,
                "the rear slides out: {rear_lateral} m/s"
            );
            assert!(
                rear_lateral < front_lateral - 1.0,
                "the rear slides more than the front: {rear_lateral} vs {front_lateral}"
            );
            // The rear swung out of the simulated body's track, to the left.
            let simulated_rear = scenario
                .transform()
                .transform_point(Vec3::Z * WHEEL_ANCHORS[2].z);
            let out = (new_rear - simulated_rear).dot(*scenario.transform().left());
            assert!(out > 0.15, "rear only {out} m out");
        }
        (front, rear) = (new_front, new_rear);
    }
    assert!(settled_checks > 30, "{settled_checks}");
    // Progressive: no more than 1° per tick, under 30 % of the angle after 0.1 s, 80 % between
    // 0.25 s and 0.9 s; a light overshoot at most (damped spring), then settled.
    let largest = slips
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    assert!(
        largest < 1f32.to_radians(),
        "{} ° in a tick",
        largest.to_degrees()
    );
    assert!(slips[6] < 0.3 * target, "{}", slips[6]);
    let eighty = slips.iter().position(|s| *s >= 0.8 * target).unwrap() as f32 / TICKS_PER_SECOND;
    assert!((0.25..=0.9).contains(&eighty), "80 % after {eighty} s");
    let highest = slips.iter().copied().fold(0.0, f32::max);
    assert!(
        highest <= 1.1 * target,
        "overshoot to {}°",
        highest.to_degrees()
    );
    assert!((slips.last().unwrap() - target).abs() < 0.01);
}

/// Issue #30: no tight angle. At top speed, the drift's radius is 16 to 20 m steered fully
/// inward (about the normal steering at full lock, ≈ 17 m; #4 had 10 m), 22 to 32 m neutral
/// (15 m) and 45 to 65 m fully outward (32 m); the body slips by 10° to 20° at most (26°).
#[test]
fn drift_radii_at_top_speed_are_wide() {
    let tuning = KartTuning::default();
    let settled = |steer: f32| {
        let mut scenario = Scenario::flat(Surface::Road);
        scenario.enter_right_drift(steer.max(0.0));
        scenario.run(1.5, drifting(steer));
        assert!(matches!(scenario.state(), KartState::Drifting { .. }));
        let radius = scenario.forward_speed() / scenario.velocity().angular.y.abs();
        (radius, scenario.motion().slip, scenario.forward_speed())
    };
    let (tight, tight_slip, speed) = settled(1.0);
    let (neutral, _, _) = settled(0.0);
    let (wide, wide_slip, _) = settled(-1.0);
    assert!(speed > 0.98 * tuning.max_speed, "{speed}");
    assert!((16.0..=20.0).contains(&tight), "tight {tight} m");
    assert!((22.0..=32.0).contains(&neutral), "neutral {neutral} m");
    assert!((45.0..=65.0).contains(&wide), "wide {wide} m");
    let normal_steering = tuning.max_speed / tuning.steer_rate_at_max_speed;
    assert!(
        tight > normal_steering - 1.5,
        "{tight} vs {normal_steering}"
    );
    assert!(
        (10.0..=20.0).contains(&tight_slip.to_degrees()),
        "{}°",
        tight_slip.to_degrees()
    );
    assert!(wide_slip > 0.0 && wide_slip < tight_slip);
}

/// Issue #30: releasing the drift (here at orange, the stick back to neutral) does not snap the
/// heading: the curve fades out and the body comes back in line with the trajectory without
/// a jolt, and without swinging past it.
#[test]
fn releasing_the_drift_realigns_the_body_without_a_snap() {
    let mut scenario = Scenario::flat(Surface::Road);
    let tuning = scenario.tuning();
    scenario.enter_right_drift(1.0);
    scenario.drift_until(1.0, MiniTurbo::Orange);
    let before = scenario.record(1, |_| drifting(1.0));
    let after = scenario.record(90, |_| FULL_THROTTLE);
    assert_eq!(after[0].state, KartState::Grounded);
    assert!(scenario.boost().is_active() || after.iter().any(|s| s.speed > tuning.max_speed));
    let samples: Vec<Sample> = before.iter().chain(&after).copied().collect();
    // The entry's bounds for the yaw rate and the direction of travel, release tick included
    // (the old drift jumped by 0.36 and 2.85 rad/s on release). The visible body gets 0.25 rad/s
    // per tick: letting go of the stick on release also lowers the slip's target by 5° at once,
    // which the spring turns into about 0.2 rad/s per tick (the old drift: about 4 rad/s).
    let yaw_step = largest_step(&samples, |s| s.yaw);
    let travel_step = largest_step(&samples, |s| s.travel_rate);
    let body_step = largest_step(&samples, |s| s.body_rate);
    assert!(
        yaw_step <= 0.12,
        "yaw rate jumped by {yaw_step} rad/s in a tick"
    );
    assert!(
        travel_step <= 0.12,
        "travel turn rate jumped by {travel_step}"
    );
    assert!(body_step <= 0.25, "body turn rate jumped by {body_step}");
    // The heading keeps curving for a moment: the first tick still turns at 90 % of the drift.
    assert!(after[0].yaw.abs() > 0.9 * before[0].yaw.abs());
    // Back in line within 1.2 s, never more than 1.5° past it.
    let lowest = after.iter().map(|s| s.motion.slip).fold(f32::MAX, f32::min);
    assert!(lowest > -1.5f32.to_radians(), "{}°", lowest.to_degrees());
    let aligned = after
        .iter()
        .position(|s| s.motion.slip.abs() < 1f32.to_radians())
        .expect("the body never came back in line") as f32
        / TICKS_PER_SECOND;
    assert!(aligned <= 1.2, "{aligned} s");
    assert_eq!(after.last().unwrap().motion.side, None);
}

// --- Issue #15: character and kart statistics --------------------------------------------------

impl Scenario {
    /// A kart with `stats` at the origin looking down -Z, on a large flat slab of `surface`.
    fn flat_with_stats(surface: Surface, stats: KartStats) -> Self {
        let mut scenario = Self::flat(surface);
        let world = scenario.app.world_mut();
        world.despawn(scenario.kart);
        let tuning = world.resource::<KartTuning>().clone();
        let stats_tuning = world.resource::<StatsTuning>().clone();
        scenario.kart = world
            .spawn(kart_body_with_stats(
                &tuning,
                &stats_tuning,
                stats,
                Transform::from_xyz(0.0, 0.55, 0.0),
            ))
            .id();
        scenario.run(0.5, KartInput::default());
        scenario
    }

    /// Another kart with `stats` at `position`, looking toward `heading`.
    fn spawn_kart_with_stats(&mut self, position: Vec3, heading: Vec3, stats: KartStats) -> Entity {
        let world = self.app.world_mut();
        let tuning = world.resource::<KartTuning>().clone();
        let stats_tuning = world.resource::<StatsTuning>().clone();
        world
            .spawn(kart_body_with_stats(
                &tuning,
                &stats_tuning,
                stats,
                Transform::from_translation(position).looking_to(heading, Vec3::Y),
            ))
            .id()
    }

    /// This kart's tuning, its stats applied.
    fn kart_tuning(&self) -> KartTuning {
        let effects = *self.app.world().get::<StatEffects>(self.kart).unwrap();
        effects.apply(&self.tuning())
    }

    fn forward_speed_of(&self, kart: Entity) -> f32 {
        let world = self.app.world();
        let forward = *world.get::<Transform>(kart).unwrap().forward();
        world.get::<Velocity>(kart).unwrap().linear.dot(forward)
    }
}

/// How a kart drives in a straight line from a standstill, full throttle (issue #15).
#[derive(Clone, Copy, Debug)]
struct Launch {
    /// Speed after 1 s (m/s).
    at_one_second: f32,
    /// Time to 95 % of its own top speed (s).
    to_95_percent: f32,
    /// Top speed, after 5 s (m/s).
    top: f32,
}

fn launch(stats: KartStats) -> Launch {
    let mut scenario = Scenario::flat_with_stats(Surface::Road, stats);
    let mut speeds = Vec::new();
    scenario.run_with(5.0, FULL_THROTTLE, |s| speeds.push(s.forward_speed()));
    let top = *speeds.last().unwrap();
    let ticks = speeds.iter().position(|v| *v >= 0.95 * top).unwrap() + 1;
    Launch {
        at_one_second: speeds[TICKS_PER_SECOND as usize - 1],
        to_95_percent: ticks as f32 / TICKS_PER_SECOND,
        top,
    }
}

/// Radius of a settled right drift steered fully inward (m), and the speed it is driven at.
fn tight_drift_radius(stats: KartStats) -> (f32, f32) {
    let mut scenario = Scenario::flat_with_stats(Surface::Road, stats);
    scenario.enter_right_drift(1.0);
    scenario.run(1.0, drifting(1.0));
    assert!(matches!(scenario.state(), KartState::Drifting { .. }));
    let speed = scenario.forward_speed();
    (speed / -scenario.velocity().angular.y, speed)
}

/// The light and the heavy loadouts of the roster: a chick in a wicker basket on spools with
/// a feather wing, a pig in a bathtub on tractor tyres with the standard wing (issue #69: the
/// mouse's and the bear's stats, under the Cube Pets' names).
fn light_and_heavy() -> (KartStats, KartStats) {
    use crate::characters::{Loadout, PartKind, Roster};
    let roster = Roster::embedded();
    let pick = |names: [&str; 4]| {
        let mut loadout = Loadout::default();
        for (kind, name) in PartKind::ALL.into_iter().zip(names) {
            loadout.set(kind, roster.find(kind, name).unwrap());
        }
        roster.stats(&loadout)
    };
    (
        pick(["Poussin", "Panier en osier", "Bobines de fil", "Plume"]),
        pick([
            "Cochon",
            "Baignoire",
            "Pneus de tracteur",
            "Taie d'oreiller",
        ]),
    )
}

/// Issue #15: a kart at the middle level (the default loadout) drives exactly like a kart
/// without stats, tick for tick: the stats modulate `KartTuning`, they do not replace it.
#[test]
fn the_middle_level_drives_exactly_like_the_tuning() {
    let mut plain = Scenario::flat(Surface::Road);
    let mut middle = Scenario::flat_with_stats(Surface::Road, KartStats::default());
    assert_eq!(middle.kart_tuning(), middle.tuning());
    let mass = |s: &Scenario| match s.app.world().get::<ColliderMassProperties>(s.kart) {
        Some(ColliderMassProperties::MassProperties(m)) => m.mass,
        other => panic!("{other:?}"),
    };
    assert_eq!(mass(&middle), mass(&plain));
    assert_eq!(
        middle.app.world().get::<MaxSpeedScale>(middle.kart),
        Some(&MaxSpeedScale(1.0))
    );
    for scenario in [&mut plain, &mut middle] {
        scenario.enter_right_drift(1.0);
        scenario.drift_until(1.0, MiniTurbo::Orange);
        scenario.run(1.0, FULL_THROTTLE);
    }
    // The flat scenario settled 0.5 s more before the plain kart was replaced: compare what
    // they did, from the same start.
    let moved = |s: &Scenario| s.transform().translation - Vec3::new(0.0, 0.55, 0.0);
    assert!(
        (moved(&plain) - moved(&middle)).length() < 1e-3,
        "{} vs {}",
        moved(&plain),
        moved(&middle)
    );
    assert_eq!(plain.boost(), middle.boost());
    assert!((plain.forward_speed() - middle.forward_speed()).abs() < 1e-4);
}

/// Issue #15 acceptance: a heavy kart has a higher top speed and a lower acceleration than a
/// light one (the roster's lightest and heaviest loadouts, measured on the flat). Its drift is
/// wider too (Handling). Run with `--nocapture` for the figures.
#[test]
fn a_heavy_kart_is_faster_at_the_top_but_slower_off_the_line() {
    let (light_stats, heavy_stats) = light_and_heavy();
    let middle = launch(KartStats::default());
    let light = launch(light_stats);
    let heavy = launch(heavy_stats);
    let (light_radius, _) = tight_drift_radius(light_stats);
    let (middle_radius, _) = tight_drift_radius(KartStats::default());
    let (heavy_radius, _) = tight_drift_radius(heavy_stats);
    println!("light  {light_stats:?}: {light:?}, tight drift {light_radius:.1} m");
    println!("middle: {middle:?}, tight drift {middle_radius:.1} m");
    println!("heavy  {heavy_stats:?}: {heavy:?}, tight drift {heavy_radius:.1} m");

    assert!(heavy.top > light.top + 1.5, "{heavy:?} vs {light:?}");
    assert!(light.top < middle.top && middle.top < heavy.top);
    assert!((middle.top - 28.5).abs() < 0.1, "{middle:?}");
    // Lower acceleration: slower after 1 s, and longer to reach its own top speed.
    assert!(
        light.at_one_second > heavy.at_one_second + 1.0,
        "{light:?} vs {heavy:?}"
    );
    assert!(
        heavy.to_95_percent > light.to_95_percent + 0.3,
        "{heavy:?} vs {light:?}"
    );
    assert!(light.to_95_percent < middle.to_95_percent);
    assert!(middle.to_95_percent < heavy.to_95_percent);
    // GD § 2: about 2.5 s from 0 to the top speed for an average kart (95 % a little before).
    assert!((1.8..=2.5).contains(&middle.to_95_percent), "{middle:?}");
    // A light kart turns tighter.
    assert!(light_radius < middle_radius - 0.5 && middle_radius < heavy_radius - 0.5);
}

/// Issue #15: each stat on its own, against the middle level (Speed 0 / 10 / 20 on the top
/// speed, the others kept at 10).
#[test]
fn the_speed_stat_scales_the_top_speed() {
    let top = |level| launch(KartStats::default().with(Stat::Speed, level)).top;
    let (slow, middle, fast) = (top(0), top(10), top(20));
    assert!((slow - 28.5 * 0.92).abs() < 0.1, "{slow}");
    assert!((middle - 28.5).abs() < 0.1, "{middle}");
    assert!((fast - 28.5 * 1.08).abs() < 0.1, "{fast}");
}

/// Issue #15: the Mini-turbo stat moves the drift's tiers. The middle level keeps the 54 / 114 /
/// 174 ticks of issue #4 (270 / 570 / 870 at +5 per tick); 20 is 10 % lower, 0 is 10 % higher.
/// Its mini-turbos also last longer.
#[test]
fn the_mini_turbo_stat_moves_the_drift_tiers() {
    let first_ticks = |level: u8| {
        let mut scenario = Scenario::flat_with_stats(
            Surface::Road,
            KartStats::default().with(Stat::MiniTurbo, level),
        );
        scenario.enter_right_drift(1.0);
        let blue = scenario.drift_until(1.0, MiniTurbo::Blue);
        let orange = blue - 1 + scenario.drift_until(1.0, MiniTurbo::Orange);
        let purple = orange - 1 + scenario.drift_until(1.0, MiniTurbo::Purple);
        scenario.tick(FULL_THROTTLE);
        let boost = scenario.boost().remaining;
        ([blue, orange, purple], boost, scenario.kart_tuning())
    };
    let (middle, middle_boost, middle_tuning) = first_ticks(10);
    assert_eq!(middle, [54, 114, 174]);
    assert_eq!(middle_tuning.mini_turbo_thresholds, [270.0, 570.0, 870.0]);
    let (quick, quick_boost, quick_tuning) = first_ticks(20);
    let (slow, slow_boost, _) = first_ticks(0);
    // 243 / 513 / 783 and 297 / 627 / 957, at 5 per tick.
    assert_eq!(quick, [49, 103, 157]);
    assert_eq!(slow, [60, 126, 192]);
    assert!((quick_tuning.mini_turbo_thresholds[0] - 243.0).abs() < 0.01);
    // The purple boost: 2.63 s × 1.15 and × 0.85.
    assert!((middle_boost - 2.63).abs() < 1e-4, "{middle_boost}");
    assert!((quick_boost - 2.63 * 1.15).abs() < 1e-3, "{quick_boost}");
    assert!((slow_boost - 2.63 * 0.85).abs() < 1e-3, "{slow_boost}");
}

/// Issue #15: the Handling stat tightens the drift (and the normal steering) around the radii of
/// issue #30.
#[test]
fn the_handling_stat_changes_the_drift_radius() {
    let radius = |level| tight_drift_radius(KartStats::default().with(Stat::Handling, level));
    let ((nimble, nimble_speed), (middle, _), (clumsy, _)) = (radius(20), radius(10), radius(0));
    let tuning = KartTuning::default();
    assert!(nimble_speed > 0.95 * tuning.max_speed, "{nimble_speed}");
    assert!((middle - tuning.drift_radius_tight).abs() < 1.0, "{middle}");
    assert!(
        (nimble - 0.9 * tuning.drift_radius_tight).abs() < 1.0,
        "{nimble}"
    );
    assert!(
        (clumsy - 1.1 * tuning.drift_radius_tight).abs() < 1.0,
        "{clumsy}"
    );
    assert!(nimble < middle - 1.0 && middle < clumsy - 1.0);
    // The normal steering too: a tighter circle at full lock.
    let full_lock = |level| {
        let mut scenario = Scenario::flat_with_stats(
            Surface::Road,
            KartStats::default().with(Stat::Handling, level),
        );
        scenario.run(3.0, FULL_THROTTLE);
        scenario.run(
            1.0,
            KartInput {
                steer: 1.0,
                ..FULL_THROTTLE
            },
        );
        scenario.velocity().angular.y.abs()
    };
    assert!(full_lock(20) > full_lock(10) * 1.05);
    assert!(full_lock(0) < full_lock(10) * 0.95);
}

/// Issue #15: the Traction stat changes the off-road top speed (GD § 2: 55 % in the middle).
#[test]
fn the_traction_stat_changes_the_offroad_speed() {
    let offroad = |level| {
        let mut scenario = Scenario::flat_with_stats(
            Surface::Offroad,
            KartStats::default().with(Stat::Traction, level),
        );
        scenario.run(5.0, FULL_THROTTLE);
        scenario.forward_speed()
    };
    let (slippery, middle, grippy) = (offroad(0), offroad(10), offroad(20));
    assert!((middle - 0.55 * 28.5).abs() < 0.2, "{middle}");
    assert!((slippery - 0.55 * 0.85 * 28.5).abs() < 0.2, "{slippery}");
    assert!((grippy - 0.55 * 1.15 * 28.5).abs() < 0.2, "{grippy}");
    // On the road, nothing changes.
    let road = launch(KartStats::default().with(Stat::Traction, 20)).top;
    assert!((road - 28.5).abs() < 0.1, "{road}");
}

/// Issue #15 acceptance: in a crash between a heavy and a light kart, the light one is pushed
/// back the most. Head on, both at full throttle from 30 m apart (the roster's lightest and
/// heaviest loadouts): the light kart loses much more speed, and the heavy one pushes it back.
#[test]
fn in_a_crash_the_light_kart_is_pushed_back_more_than_the_heavy_one() {
    let (light_stats, heavy_stats) = light_and_heavy();
    let mut scenario = Scenario::flat_with_stats(Surface::Road, light_stats);
    let light = scenario.kart;
    let heavy = scenario.spawn_kart_with_stats(Vec3::new(0.0, 0.55, -30.0), Vec3::Z, heavy_stats);
    scenario.run(0.3, KartInput::default());
    let mut previous = (0.0, 0.0);
    let mut drops = (0.0f32, 0.0f32);
    let mut after = None;
    for _ in 0..(3.0 * TICKS_PER_SECOND) as usize {
        let bumps = scenario.bumps_while(
            1.0 / TICKS_PER_SECOND,
            &[(light, FULL_THROTTLE), (heavy, FULL_THROTTLE)],
        );
        let now = (
            scenario.forward_speed_of(light),
            scenario.forward_speed_of(heavy),
        );
        if !bumps.is_empty() && after.is_none() {
            drops = (previous.0 - now.0, previous.1 - now.1);
            after = Some(now);
        }
        previous = now;
    }
    let after = after.expect("they never crashed");
    println!(
        "speed lost in the crash: light {:.1} m/s, heavy {:.1} m/s; just after: {after:?}",
        drops.0, drops.1
    );
    assert!(drops.0 > 1.3 * drops.1, "{drops:?}");
    // They come out of it at the same speed along the impact (no bounce between karts): in the
    // heavy kart's direction, so the light one goes backwards.
    assert!(after.0 < 0.0 && after.1 > 0.0, "{after:?}");
}

/// Issue #15: the same kart driving into the side of a kart standing still pushes a light one
/// further aside than a heavy one (Weight 0 / 20, every other stat at the middle).
#[test]
fn a_light_kart_is_pushed_aside_further_than_a_heavy_one() {
    let shove = |target_weight: u8| {
        let mut scenario = Scenario::flat_with_stats(Surface::Road, KartStats::default());
        let striker = scenario.kart;
        let target = scenario.spawn_kart_with_stats(
            Vec3::new(0.0, 0.55, -10.0),
            Vec3::X,
            KartStats::default().with(Stat::Weight, target_weight),
        );
        scenario.run(0.3, KartInput::default());
        let start = scenario
            .app
            .world()
            .get::<Transform>(target)
            .unwrap()
            .translation;
        scenario.bumps_while(
            1.6,
            &[(striker, FULL_THROTTLE), (target, KartInput::default())],
        );
        let end = scenario
            .app
            .world()
            .get::<Transform>(target)
            .unwrap()
            .translation;
        // Pushed along the striker's way (-Z).
        start.z - end.z
    };
    let (light, heavy) = (shove(0), shove(20));
    println!("pushed aside: light {light:.2} m, heavy {heavy:.2} m");
    assert!(light > 0.5, "{light}");
    assert!(light > 1.3 * heavy, "light {light} m, heavy {heavy} m");
}

// --- Issue #27: walls and other karts are never ground, and never tip the kart -----------------

/// A wall only as high as a kart's body (0.8 m): the circuits' walls were drawn that high before
/// issue #47 (they are 1.2 m high now, `track::mesh::WALL_HEIGHT`, drawn and solid). These tests
/// keep it: it is the hard case, its top edge level with the top of the kart, and the center of
/// mass and the suspension rays of #27 must hold there even without the higher walls.
const LOW_WALL_HEIGHT: f32 = 0.8;

impl Scenario {
    /// Puts the kart at `position`, at rest, keeping its heading.
    fn place(&mut self, kart: Entity, position: Vec3) {
        let world = self.app.world_mut();
        world.get_mut::<Transform>(kart).unwrap().translation = position;
        *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
    }

    fn wheels_of(&self, kart: Entity) -> KartWheels {
        *self.app.world().get::<KartWheels>(kart).unwrap()
    }
}

/// Issue #27: the suspension rays only stand on the ground. A kart lowered onto a block as high
/// as a kart's body stands on its wheels on a block of road, but finds no ground on a wall:
/// it rests on its body there, its wheels in the air. Nor on the roof of another kart. Before the
/// fix, a ray found ground on the flat top of a wall, or on another kart's roof, and its spring
/// lifted that side of the kart further onto it.
#[test]
fn the_wheels_stand_on_the_ground_never_on_a_wall_or_another_kart() {
    for (surface, wheels) in [(Surface::Road, 4), (Surface::Wall, 0)] {
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
                Transform::from_xyz(0.0, LOW_WALL_HEIGHT / 2.0, -20.0),
                Vec3::new(3.0, LOW_WALL_HEIGHT / 2.0, 3.0),
                surface,
                WALL_RESTITUTION,
            );
        });
        let kart = scenario.kart;
        scenario.place(kart, Vec3::new(0.0, LOW_WALL_HEIGHT + 0.6, -20.0));
        scenario.run(0.5, KartInput::default());
        let height = scenario.transform().translation.y;
        assert_eq!(
            scenario.wheels().wheels_on_ground(),
            wheels,
            "on a block of {surface:?}, at {height} m"
        );
        assert!(height > LOW_WALL_HEIGHT, "fell off the block: {height}");
    }

    // On another kart's roof, at rest: no ground either, and the kart below is not pushed down
    // into the road by a spring.
    let mut scenario = Scenario::flat(Surface::Road);
    let below = scenario.spawn_other_kart(Vec3::new(0.0, 0.55, -20.0));
    let above = scenario.kart;
    scenario.run(0.3, KartInput::default());
    scenario.place(above, Vec3::new(0.0, 0.55 + 0.6, -20.0));
    for _ in 0..10 {
        scenario.tick(KartInput::default());
        assert_eq!(scenario.wheels_of(above).wheels_on_ground(), 0);
        let compression = scenario.wheels_of(below).compression;
        assert!(
            compression.iter().all(|c| *c < 0.25),
            "the kart below is pressed down: {compression:?}"
        );
    }
}

/// How a crash into a wall went (issue #27).
#[derive(Clone, Copy, Debug)]
struct WallCrash {
    /// Speed at the impact (m/s).
    speed: f32,
    /// Highest point of the kart's center (m; 0.55 at rest).
    highest: f32,
    /// Lowest uprightness (1: wheels down; 0.94: tilted by 20°).
    lowest_up: f32,
    /// Closest the kart's center came to the face of the wall (m).
    closest: f32,
    /// Fastest the kart moved away from the wall after the impact (m/s).
    bounce: f32,
}

/// Full throttle from a standstill into a low wall ([`LOW_WALL_HEIGHT`]), 40 m ahead,
/// whose face makes `angle_deg` with the kart's line (90: head on). Steering stays straight: the
/// kart keeps pushing against the wall once it has bounced.
fn crash_into_a_low_wall(stats: KartStats, angle_deg: f32) -> WallCrash {
    let normal = Quat::from_rotation_y((90.0 - angle_deg).to_radians()) * Vec3::Z;
    let face = Vec3::new(0.0, 0.0, -40.0);
    let half_thickness = 0.25;
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
            Transform::from_translation(
                face - normal * half_thickness + Vec3::Y * LOW_WALL_HEIGHT / 2.0,
            )
            .looking_to(normal, Vec3::Y),
            Vec3::new(40.0, LOW_WALL_HEIGHT / 2.0, half_thickness),
            Surface::Wall,
            WALL_RESTITUTION,
        );
    });
    let world = scenario.app.world_mut();
    world.despawn(scenario.kart);
    let tuning = world.resource::<KartTuning>().clone();
    let stats_tuning = world.resource::<StatsTuning>().clone();
    scenario.kart = world
        .spawn(kart_body_with_stats(
            &tuning,
            &stats_tuning,
            stats,
            Transform::from_xyz(0.0, 0.55, 0.0),
        ))
        .id();
    scenario.run(0.3, KartInput::default());
    let mut crash = WallCrash {
        speed: 0.0,
        highest: 0.0,
        lowest_up: 1.0,
        closest: f32::MAX,
        bounce: 0.0,
    };
    let mut previous_speed = 0.0;
    let mut hit = false;
    scenario.run_with(4.0, FULL_THROTTLE, |s| {
        let position = s.transform().translation;
        let away = s.velocity().linear.dot(normal);
        crash.highest = crash.highest.max(position.y);
        crash.lowest_up = crash.lowest_up.min(s.uprightness());
        crash.closest = crash.closest.min((position - face).dot(normal));
        if !hit && away > 0.0 {
            hit = true;
            crash.speed = previous_speed;
        }
        if hit {
            crash.bounce = crash.bounce.max(away);
        }
        previous_speed = s.velocity().linear.length();
    });
    assert!(hit, "never reached the wall");
    crash
}

/// Issue #27: a kart crashing into a low wall at full speed (27 to 28.5 m/s), at 30° to head on,
/// light, middle or heavy, bounces off it without tipping by more than 15° nor rising, and stays
/// on its side of the wall. Before the fix, the impact landed above its center of mass: 5 of
/// these 12 crashes tipped the kart by 25 to 32° (the side facing the wall up, or the nose), and
/// the light kart went over the wall at 60°.
#[test]
fn crashing_into_a_low_wall_neither_tips_nor_lifts_the_kart() {
    let (light, heavy) = light_and_heavy();
    let mut failures = Vec::new();
    for (name, stats) in [
        ("light", light),
        ("middle", KartStats::default()),
        ("heavy", heavy),
    ] {
        for angle in [30.0, 45.0, 60.0, 90.0] {
            let crash = crash_into_a_low_wall(stats, angle);
            println!("{name} kart, wall at {angle}°: {crash:?}");
            if crash.speed < 25.0
                || crash.bounce < 1.0
                || crash.highest >= 0.7
                || crash.lowest_up <= 15f32.to_radians().cos()
                || crash.closest <= 0.6
            {
                failures.push(format!("{name} kart, wall at {angle}°: {crash:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Issue #27: a kart rammed in the flank by a kart at full speed (about 26 m/s after a 25 m
/// run-up, 29 m/s after 40 m) crossing its way at 60°, 90° or 120°, whether it is light, middle
/// or heavy: neither kart rises nor tips by more than 10°; the rammer never climbs onto the other
/// kart. Before the fix, all 18 shunts broke this: the impact landed above the centers of mass,
/// the rammer's nose rose, its front wheels found ground on the other kart's roof and it drove
/// onto it (its center up to 1.67 m high), or the kart rammed was tipped (by up to 59°).
#[test]
fn ramming_a_kart_in_the_flank_never_climbs_onto_it() {
    let (light, heavy) = light_and_heavy();
    let mut failures = Vec::new();
    for (name, stats) in [
        ("light", light),
        ("middle", KartStats::default()),
        ("heavy", heavy),
    ] {
        for angle in [60.0f32, 90.0, 120.0] {
            for run_up in [25.0, 40.0] {
                let mut scenario = Scenario::flat(Surface::Road);
                let rammer = scenario.kart;
                let heading = Quat::from_rotation_y(-angle.to_radians()) * Vec3::NEG_Z;
                let rammed =
                    scenario.spawn_kart_with_stats(Vec3::new(0.0, 0.55, -run_up), heading, stats);
                scenario.run(0.3, KartInput::default());
                let mut highest = [0.0f32; 2];
                let mut lowest_up = [1.0f32; 2];
                let mut bumped = false;
                for _ in 0..(3.0 * TICKS_PER_SECOND) as usize {
                    let bumps = scenario.bumps_while(
                        1.0 / TICKS_PER_SECOND,
                        &[(rammer, FULL_THROTTLE), (rammed, KartInput::default())],
                    );
                    bumped |= !bumps.is_empty();
                    for (i, kart) in [rammer, rammed].into_iter().enumerate() {
                        let transform = scenario.app.world().get::<Transform>(kart).unwrap();
                        highest[i] = highest[i].max(transform.translation.y);
                        lowest_up[i] = lowest_up[i].min(transform.up().y);
                    }
                }
                let case = format!(
                    "{name} kart rammed at {angle}° after {run_up} m: highest {highest:?}, \
                     uprightness {lowest_up:?}, shunt {bumped}"
                );
                println!("{case}");
                if !bumped
                    || highest.iter().any(|h| *h >= 0.7)
                    || lowest_up.iter().any(|up| *up <= 10f32.to_radians().cos())
                {
                    failures.push(case);
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// --- Issue #19: the speed classes ----------------------------------------------------------------

impl Scenario {
    fn set_class(&mut self, class: SpeedClass) {
        self.app.insert_resource(RaceClass(class));
    }
}

/// Issue #19 (GD § 1): the class multiplies the top speed: 0.75, 0.87, 1.0 and 1.25 of
/// `max_speed`, reached in about the same time (the engine's curve follows the top speed). At
/// 150cc, the class's factor is exactly 1, never written: the top speed scale and the product of
/// the factors are those from before the classes, to the bit.
#[test]
fn the_classes_scale_the_top_speed_and_150cc_is_the_game() {
    let tuning = SpeedClassTuning::default();
    for class in SpeedClass::ALL {
        let mut scenario = Scenario::flat(Surface::Road);
        scenario.set_class(class);
        let max = scenario.tuning().max_speed;
        let factor = tuning.speed(class);
        scenario.run(2.5, FULL_THROTTLE);
        let at_2_5 = scenario.forward_speed();
        assert!(
            at_2_5 > 0.93 * factor * max,
            "{class:?}: {at_2_5} m/s after 2.5 s"
        );
        scenario.run(2.5, FULL_THROTTLE);
        let top = scenario.forward_speed();
        assert!(
            (top - factor * max).abs() < 0.05,
            "{class:?}: {top} vs {}",
            factor * max
        );
        let factors = *scenario
            .app
            .world()
            .get::<MaxSpeedFactors>(scenario.kart)
            .unwrap();
        assert_eq!(factors.class, factor);
    }

    // 150cc: the class's factor is the one a kart spawns with (1, never written), its top
    // speed scale exactly 1, steering and drifting: what drives the kart is what it was before
    // the classes (the kart scenarios and the bot races above check that driving to the
    // number).
    let mut classic = Scenario::flat(Surface::Road);
    classic.set_class(SpeedClass::Cc150);
    let inputs = [
        (2.5, FULL_THROTTLE),
        (
            1.0,
            KartInput {
                steer: 0.6,
                ..FULL_THROTTLE
            },
        ),
        (0.1, drifting(1.0)),
        (1.5, drifting(0.8)),
        (1.0, FULL_THROTTLE),
    ];
    for (seconds, input) in inputs {
        classic.run_with(seconds, input, |s| {
            let world = s.app.world();
            assert_eq!(
                *world.get::<MaxSpeedFactors>(s.kart).unwrap(),
                MaxSpeedFactors::default()
            );
            assert_eq!(world.get::<MaxSpeedScale>(s.kart).unwrap().0, 1.0);
        });
    }
    let product = |f: &MaxSpeedFactors| f.catch_up * f.croquettes * f.stats * f.tether * f.items;
    let factors = MaxSpeedFactors {
        catch_up: 1.03,
        croquettes: 1.07,
        stats: 0.96,
        tether: 0.9,
        items: 1.2,
        class: 1.0,
    };
    assert_eq!(
        factors.product(),
        product(&factors),
        "150cc: the product as before"
    );
}

/// GD § 3 point 7 (issue #19): in 200cc, braking in a drift slows the kart down without breaking
/// the drift, never under its floor, and tightens the curve (a smaller radius than the same drift
/// unbraked); the charge goes on. In 150cc the same brake breaks the drift (as always: too slow).
#[test]
fn braking_in_a_drift_tightens_it_in_200cc_and_breaks_it_in_150cc() {
    let class_tuning = SpeedClassTuning::default();
    let brake_in_drift = KartInput {
        throttle: 1.0,
        brake: 1.0,
        steer: 1.0,
        jump_drift: true,
        ..default()
    };
    let radius = |s: &Scenario| s.forward_speed() / s.velocity().angular.y.abs();

    let (mut braked, mut unbraked) = (Scenario::flat(Surface::Road), Scenario::flat(Surface::Road));
    for scenario in [&mut braked, &mut unbraked] {
        scenario.set_class(SpeedClass::Cc200);
        scenario.enter_right_drift(1.0);
        scenario.run(0.6, drifting(1.0));
    }
    let entry = braked.forward_speed();
    assert!(
        entry > 1.2 * braked.tuning().max_speed,
        "200cc: {entry} m/s"
    );
    let charge = |s: &Scenario| match s.state() {
        KartState::Drifting { charge, .. } => charge,
        other => panic!("not drifting: {other:?}"),
    };
    let before = charge(&braked);
    let mut lowest = f32::MAX;
    braked.run_with(2.0, brake_in_drift, |s| {
        assert!(
            matches!(s.state(), KartState::Drifting { .. }),
            "200cc: the brake broke the drift at {} m/s",
            s.forward_speed()
        );
        lowest = lowest.min(s.forward_speed());
    });
    unbraked.run(2.0, drifting(1.0));
    let speed = braked.forward_speed();
    assert!(
        speed < entry - 10.0 && lowest >= class_tuning.drift_brake_min_speed - 0.3,
        "200cc: {entry} → {speed} m/s (lowest {lowest})"
    );
    assert!(charge(&braked) > before, "the charge goes on");
    let (tight, loose) = (radius(&braked), radius(&unbraked));
    assert!(
        tight < 0.6 * loose,
        "200cc: braked radius {tight} m, unbraked {loose} m"
    );
    // Released, the drift fires its mini-turbo.
    braked.tick(FULL_THROTTLE);
    assert!(braked.boost().is_active());

    let mut classic = Scenario::flat(Surface::Road);
    classic.enter_right_drift(1.0);
    classic.run(0.6, drifting(1.0));
    let mut ticks = 0;
    while matches!(classic.state(), KartState::Drifting { .. }) {
        assert!(ticks < 120, "150cc: the brake never broke the drift");
        classic.tick(brake_in_drift);
        ticks += 1;
    }
    assert!(classic.forward_speed() < classic.tuning().drift_exit_speed);
}
