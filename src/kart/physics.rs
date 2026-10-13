//! Pure kart maths: no ECS, no Rapier. Everything here is unit-tested.
//!
//! Conventions: the kart looks down its local -Z (Bevy's `forward`), +Y is up,
//! `steer` > 0 turns right, forward speed < 0 means driving backwards.

use bevy::prelude::*;

use super::KartInput;
use super::drift::{DriftSide, MiniTurbo, mini_turbo_tier};
use super::tuning::KartTuning;

/// Generic boost shared by every source (mini-turbo, chili, rocket start, trick, slipstream,
/// boost pad): GD § 2 "Boosts : un seul système".
///
/// Lives on every kart; it is inactive when `remaining` is zero.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Boost {
    /// Max speed increase while active: 0.3 = +30 %.
    pub strength: f32,
    /// Seconds left.
    pub remaining: f32,
}

impl Boost {
    pub fn is_active(&self) -> bool {
        self.remaining > 0.0
    }

    /// Adds a boost. When two boosts overlap, keep the strongest intensity and the longest
    /// remaining time (GD § 2).
    pub fn add(&mut self, strength: f32, duration: f32) {
        if self.is_active() {
            self.strength = self.strength.max(strength);
            self.remaining = self.remaining.max(duration);
        } else {
            self.strength = strength;
            self.remaining = duration;
        }
    }

    /// Counts the boost down by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        if self.is_active() {
            self.remaining = (self.remaining - dt).max(0.0);
            if self.remaining == 0.0 {
                self.strength = 0.0;
            }
        }
    }
}

/// Driving state of a kart. The hit state comes with the items (issue #16)
/// (docs/ARCHITECTURE.md § « Le kart »). Transitions: `drift::step_state`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub enum KartState {
    #[default]
    Grounded,
    /// Small jump from the jump/drift button (GD § 3.1). `ticks` since the press; `side` is the
    /// last side steered during the hop, where the drift goes if the button is still held on
    /// landing.
    Hop {
        ticks: u32,
        side: Option<DriftSide>,
    },
    /// Drifting toward `side`, with the mini-turbo `charge` accumulated so far (GD § 3.3).
    Drifting {
        side: DriftSide,
        charge: f32,
    },
    Airborne,
    /// Doing a trick in the air after a trick ramp (GD § 5, issue #13, `trick.rs`): air control
    /// like `Airborne`; the landing gives the trick boost.
    Trick,
    /// Carried by the tow pelican (GD § 2, issue #5): the kart does not drive, its drift and
    /// charge are gone and the race rules move it. Only the race rules leave this state.
    Respawning,
}

impl KartState {
    /// Mini-turbo tier reached by the current drift, if any (sparks, UI).
    pub fn mini_turbo(&self, tuning: &KartTuning, assisted: bool) -> Option<MiniTurbo> {
        match *self {
            KartState::Drifting { charge, .. } => mini_turbo_tier(tuning, charge, assisted),
            _ => None,
        }
    }
}

/// Ground contact part of the state machine: `Grounded` or `Airborne`. `ticks_without_ground`
/// counts consecutive ticks with no wheel touching: a short grace period avoids flickering to
/// `Airborne` on every bump.
pub fn next_state(
    wheels_on_ground: usize,
    ticks_without_ground: u32,
    grace_ticks: u32,
) -> KartState {
    if wheels_on_ground > 0 || ticks_without_ground < grace_ticks {
        KartState::Grounded
    } else {
        KartState::Airborne
    }
}

/// Current top speed of a kart. `offroad_ratio` is the share of grounded wheels on an off-road
/// surface (0 to 1): it blends the slow-down so that crossing a border stays smooth.
/// A boost raises the top speed and ignores off-road (GD § 2).
pub fn max_speed(tuning: &KartTuning, offroad_ratio: f32, boost: &Boost) -> f32 {
    if boost.is_active() {
        tuning.max_speed * (1.0 + boost.strength)
    } else {
        let factor = 1.0 + (tuning.offroad_speed_factor - 1.0) * offroad_ratio.clamp(0.0, 1.0);
        tuning.max_speed * factor
    }
}

/// Engine acceleration at forward speed `speed` toward `top_speed`.
///
/// The speed follows `v(t) = top · (1 − (1 − t/T)^n)`, which reaches exactly `top` after
/// `T = time_to_max_speed`. Expressed as a function of the speed, so it also works when
/// starting from any speed (after a hit, a wall, off-road...).
pub fn engine_accel(tuning: &KartTuning, speed: f32, top_speed: f32) -> f32 {
    if top_speed <= 0.0 || speed >= top_speed {
        return 0.0;
    }
    let n = tuning.engine_curve.max(1.0);
    let t = tuning.time_to_max_speed.max(0.01);
    let remaining = (1.0 - speed.max(0.0) / top_speed).clamp(0.0, 1.0);
    n * top_speed / t * remaining.powf((n - 1.0) / n)
}

/// Moves `value` toward `target` by at most `max_delta`, without overshooting.
fn approach(value: f32, target: f32, max_delta: f32) -> f32 {
    if value < target {
        (value + max_delta).min(target)
    } else {
        (value - max_delta).max(target)
    }
}

/// Forward speed after one tick of `dt` seconds.
///
/// - Boost: strong acceleration up to the boosted top speed, throttle or not (not while braking).
/// - Throttle: engine curve up to `top_speed`; above it, slow down to it.
/// - Brake: brakes while moving forward; once (almost) stopped, drives backwards.
///   Throttle and brake together: just brake, no reverse.
/// - Nothing pressed: coast down.
pub fn next_forward_speed(
    tuning: &KartTuning,
    speed: f32,
    input: &KartInput,
    top_speed: f32,
    boosting: bool,
    dt: f32,
) -> f32 {
    let throttle = input.throttle.clamp(0.0, 1.0);
    let brake = input.brake.clamp(0.0, 1.0);

    // Going faster than allowed (boost ended, off-road...): ease back down, whatever the input.
    if speed > top_speed && brake == 0.0 {
        let decel = tuning
            .overspeed_decel
            .max(tuning.coast_decel * (1.0 - throttle));
        return approach(speed, top_speed, decel * dt);
    }

    if boosting && brake == 0.0 {
        // A boost pushes forward even without throttle. Braking cancels the push (the raised
        // top speed stays until the boost ends).
        return approach(speed, top_speed, tuning.boost_accel * dt);
    }

    if brake > 0.0 {
        if speed > tuning.reverse_threshold {
            return approach(speed, 0.0, tuning.brake_decel * brake * dt);
        }
        if throttle > 0.0 {
            return approach(speed, 0.0, tuning.brake_decel * dt);
        }
        // Reverse: brake if still rolling backwards too fast, otherwise accelerate backwards.
        let target = -tuning.reverse_max_speed * brake;
        let rate = if speed > target {
            tuning.reverse_accel
        } else {
            tuning.coast_decel
        };
        return approach(speed, target, rate * dt);
    }

    if throttle > 0.0 {
        if speed < -tuning.reverse_threshold {
            // Rolling backwards: the throttle first brakes.
            return approach(speed, 0.0, tuning.brake_decel * dt);
        }
        // Barely rolling back (a slight slope): drive off right away. Treating it like real
        // reverse speed would brake to 0 every tick while gravity pulls back: stuck forever.
        let speed = speed.max(0.0);
        let accel = engine_accel(tuning, speed, top_speed) * throttle;
        return (speed + accel * dt).min(top_speed);
    }

    approach(speed, 0.0, tuning.coast_decel * dt)
}

/// Sideways speed after one tick: the tyres cancel most of it (strong arcade grip).
pub fn next_lateral_speed(tuning: &KartTuning, lateral_speed: f32, dt: f32) -> f32 {
    lateral_speed * (-tuning.lateral_grip * dt).exp()
}

/// Target yaw rate (rad/s, around the kart's up axis, counter-clockwise positive like Bevy).
///
/// Steering fades out near standstill, gets a bit lazier at top speed, and is mirrored when
/// driving backwards, like a real car.
pub fn target_yaw_rate(tuning: &KartTuning, steer: f32, forward_speed: f32) -> f32 {
    let steer = steer.clamp(-1.0, 1.0);
    let abs_speed = forward_speed.abs();
    let low_speed_fade = (abs_speed / tuning.steer_full_speed.max(0.01)).clamp(0.0, 1.0);
    let speed_ratio = (abs_speed / tuning.max_speed.max(0.01)).clamp(0.0, 1.0);
    let rate =
        tuning.steer_rate + (tuning.steer_rate_at_max_speed - tuning.steer_rate) * speed_ratio;
    let direction = if forward_speed < 0.0 { -1.0 } else { 1.0 };
    // Steering right (steer > 0) is a clockwise turn seen from above: negative yaw.
    -steer * rate * low_speed_fade * direction
}

/// Moves the current yaw rate toward the target one, smoothed by `steer_response`.
pub fn next_yaw_rate(tuning: &KartTuning, current: f32, target: f32, dt: f32) -> f32 {
    approach_yaw_rate(current, target, tuning.steer_response, dt)
}

/// Moves the current yaw rate toward the target one at `response` (1/s, frame-rate independent
/// exponential smoothing).
pub fn approach_yaw_rate(current: f32, target: f32, response: f32, dt: f32) -> f32 {
    let blend = 1.0 - (-response.max(0.0) * dt).exp();
    current + (target - current) * blend
}

/// Suspension acceleration of one wheel (multiply by the mass to get a force), along the
/// kart's up axis. `compression` is how much the spring is squeezed (m); `up_speed` is the
/// speed of the wheel anchor along the kart's up axis (m/s, negative when going down).
/// A spring only pushes, it never pulls the kart to the ground.
pub fn suspension_accel(tuning: &KartTuning, compression: f32, up_speed: f32) -> f32 {
    if compression <= 0.0 {
        return 0.0;
    }
    (tuning.suspension_stiffness * compression - tuning.suspension_damping * up_speed).max(0.0)
}

/// Angular velocity correction (rad/s, world space) that rotates the kart's `up` toward
/// `target_up` at `rate` (1/s). Used in the air so the kart always lands on its wheels.
pub fn righting_angular_velocity(up: Vec3, target_up: Vec3, rate: f32) -> Vec3 {
    let axis = up.cross(target_up);
    let sin = axis.length();
    let cos = up.dot(target_up);
    let angle = sin.atan2(cos);
    if angle < 1e-4 {
        return Vec3::ZERO;
    }
    // Upside down, the cross product vanishes: any horizontal axis works.
    let axis = if sin > 1e-4 {
        axis / sin
    } else {
        up.any_orthonormal_vector()
    };
    axis * angle * rate
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn input(throttle: f32, brake: f32, steer: f32) -> KartInput {
        KartInput {
            throttle,
            brake,
            steer,
            ..default()
        }
    }

    /// Simulates `seconds` of straight driving with the pure model, returns the speed.
    fn drive(
        tuning: &KartTuning,
        mut speed: f32,
        input: &KartInput,
        top: f32,
        seconds: f32,
    ) -> f32 {
        for _ in 0..(seconds / DT).round() as usize {
            speed = next_forward_speed(tuning, speed, input, top, false, DT);
        }
        speed
    }

    #[test]
    fn engine_reaches_95_percent_within_time_to_max_speed() {
        let t = KartTuning::default();
        let speed = drive(
            &t,
            0.0,
            &input(1.0, 0.0, 0.0),
            t.max_speed,
            t.time_to_max_speed,
        );
        assert!(speed >= 0.95 * t.max_speed, "speed {speed}");
        assert!(speed <= t.max_speed);
    }

    #[test]
    fn engine_is_punchy_at_start_but_not_instant() {
        let t = KartTuning::default();
        let after_half_second = drive(&t, 0.0, &input(1.0, 0.0, 0.0), t.max_speed, 0.5);
        assert!(after_half_second > 0.3 * t.max_speed, "{after_half_second}");
        assert!(after_half_second < 0.6 * t.max_speed, "{after_half_second}");
    }

    #[test]
    fn engine_never_exceeds_top_speed() {
        let t = KartTuning::default();
        let speed = drive(&t, 0.0, &input(1.0, 0.0, 0.0), t.max_speed, 10.0);
        assert!((speed - t.max_speed).abs() < 1e-3, "{speed}");
    }

    #[test]
    fn offroad_caps_top_speed() {
        let t = KartTuning::default();
        let no_boost = Boost::default();
        assert_eq!(max_speed(&t, 0.0, &no_boost), t.max_speed);
        assert!((max_speed(&t, 1.0, &no_boost) - 0.55 * t.max_speed).abs() < 1e-4);
        let half = max_speed(&t, 0.5, &no_boost);
        assert!(half < t.max_speed && half > 0.55 * t.max_speed);
    }

    #[test]
    fn entering_offroad_at_full_speed_slows_down_to_offroad_cap() {
        let t = KartTuning::default();
        let cap = max_speed(&t, 1.0, &Boost::default());
        let speed = drive(&t, t.max_speed, &input(1.0, 0.0, 0.0), cap, 2.0);
        assert!((speed - cap).abs() < 1e-3, "{speed} vs {cap}");
    }

    #[test]
    fn boost_ignores_offroad_and_raises_top_speed() {
        let t = KartTuning::default();
        let boost = Boost {
            strength: 0.3,
            remaining: 1.0,
        };
        assert!((max_speed(&t, 1.0, &boost) - 1.3 * t.max_speed).abs() < 1e-4);
    }

    #[test]
    fn boost_accelerates_past_max_speed_even_without_throttle() {
        let t = KartTuning::default();
        let top = t.max_speed * 1.3;
        let mut speed = t.max_speed;
        for _ in 0..30 {
            speed = next_forward_speed(&t, speed, &KartInput::default(), top, true, DT);
        }
        assert!(speed > t.max_speed * 1.2, "{speed}");
    }

    #[test]
    fn braking_during_a_boost_still_brakes() {
        let t = KartTuning::default();
        let top = t.max_speed * 1.3;
        let brake = input(0.0, 1.0, 0.0);
        let slower = next_forward_speed(&t, 20.0, &brake, top, true, DT);
        assert!(slower < 20.0, "{slower}");
        // Reversing with the brake held: a boost does not throw the kart forward.
        let reversing = next_forward_speed(&t, -8.0, &brake, top, true, DT);
        assert!(reversing <= -7.9, "{reversing}");
    }

    #[test]
    fn boosts_combine_strongest_and_longest() {
        let mut boost = Boost::default();
        boost.add(0.2, 2.0);
        boost.add(0.4, 0.5);
        assert_eq!(
            boost,
            Boost {
                strength: 0.4,
                remaining: 2.0
            }
        );
        boost.tick(2.5);
        assert!(!boost.is_active());
        assert_eq!(boost.strength, 0.0);
        // A fresh boost after expiry is not mixed with the old one.
        boost.add(0.1, 0.3);
        assert_eq!(
            boost,
            Boost {
                strength: 0.1,
                remaining: 0.3
            }
        );
    }

    #[test]
    fn brake_stops_then_reverses() {
        let t = KartTuning::default();
        let brake = input(0.0, 1.0, 0.0);
        let speed = drive(&t, 20.0, &brake, t.max_speed, 0.5);
        assert!(speed < 20.0 - 0.4 * t.brake_decel, "brakes hard: {speed}");
        let speed = drive(&t, speed, &brake, t.max_speed, 4.0);
        assert!(
            (speed + t.reverse_max_speed).abs() < 1e-3,
            "then drives backwards at full reverse speed: {speed}"
        );
    }

    #[test]
    fn throttle_and_brake_together_do_not_reverse() {
        let t = KartTuning::default();
        let speed = drive(&t, 5.0, &input(1.0, 1.0, 0.0), t.max_speed, 3.0);
        assert_eq!(speed, 0.0);
    }

    #[test]
    fn throttle_while_reversing_brakes_first() {
        let t = KartTuning::default();
        let speed = next_forward_speed(&t, -5.0, &input(1.0, 0.0, 0.0), t.max_speed, false, DT);
        assert!(speed > -5.0 && speed <= 0.0);
    }

    #[test]
    fn throttle_drives_off_when_barely_rolling_back() {
        // Regression: on a slight slope the kart rolled back a millimeter per second and the
        // throttle only braked to 0, every tick, forever.
        let t = KartTuning::default();
        let speed = next_forward_speed(&t, -0.0015, &input(1.0, 0.0, 0.0), t.max_speed, false, DT);
        assert!(speed > 0.2, "{speed}");
    }

    #[test]
    fn coasting_slows_down_to_a_stop() {
        let t = KartTuning::default();
        let speed = drive(&t, 10.0, &KartInput::default(), t.max_speed, 5.0);
        assert_eq!(speed, 0.0);
    }

    #[test]
    fn lateral_grip_cancels_most_sideways_speed_quickly() {
        let t = KartTuning::default();
        let mut lateral = 10.0;
        for _ in 0..10 {
            lateral = next_lateral_speed(&t, lateral, DT);
        }
        assert!(lateral.abs() < 1.0, "{lateral}");
    }

    #[test]
    fn steering_right_is_clockwise_and_mirrored_in_reverse() {
        let t = KartTuning::default();
        assert!(target_yaw_rate(&t, 1.0, 20.0) < 0.0);
        assert!(target_yaw_rate(&t, -1.0, 20.0) > 0.0);
        assert!(target_yaw_rate(&t, 1.0, -5.0) > 0.0);
    }

    #[test]
    fn steering_fades_out_at_standstill_and_is_lazier_at_top_speed() {
        let t = KartTuning::default();
        assert_eq!(target_yaw_rate(&t, 1.0, 0.0), 0.0);
        let mid = target_yaw_rate(&t, 1.0, 10.0).abs();
        let top = target_yaw_rate(&t, 1.0, t.max_speed).abs();
        assert!(top < mid, "{top} vs {mid}");
        assert!((top - t.steer_rate_at_max_speed).abs() < 1e-4);
    }

    #[test]
    fn yaw_rate_converges_to_target() {
        let t = KartTuning::default();
        let mut yaw = 0.0;
        // Six time constants of the steering response.
        let ticks = (6.0 / t.steer_response / DT).ceil() as usize;
        for _ in 0..ticks {
            yaw = next_yaw_rate(&t, yaw, 2.0, DT);
        }
        assert!((yaw - 2.0).abs() < 0.01, "{yaw}");
    }

    #[test]
    fn suspension_holds_the_kart_at_rest_with_some_compression() {
        let t = KartTuning::default();
        let gravity = 9.81 * t.gravity_scale;
        // Compression at rest when 4 wheels share the weight.
        let rest = gravity / (4.0 * t.suspension_stiffness);
        assert!(
            rest > 0.05 && rest < t.suspension_rest_length * 0.5,
            "{rest}"
        );
        assert!((4.0 * suspension_accel(&t, rest, 0.0) - gravity).abs() < 1e-3);
    }

    #[test]
    fn suspension_only_pushes() {
        let t = KartTuning::default();
        assert_eq!(suspension_accel(&t, 0.0, -5.0), 0.0);
        assert_eq!(suspension_accel(&t, 0.01, 50.0), 0.0);
        // Landing (going down) adds damping force.
        assert!(suspension_accel(&t, 0.1, -3.0) > suspension_accel(&t, 0.1, 0.0));
    }

    #[test]
    fn state_waits_a_grace_period_before_airborne() {
        assert_eq!(next_state(2, 0, 4), KartState::Grounded);
        assert_eq!(next_state(0, 3, 4), KartState::Grounded);
        assert_eq!(next_state(0, 4, 4), KartState::Airborne);
        assert_eq!(next_state(1, 100, 4), KartState::Grounded);
    }

    #[test]
    fn righting_turns_up_vector_toward_target() {
        let up = Quat::from_rotation_z(0.5) * Vec3::Y;
        let w = righting_angular_velocity(up, Vec3::Y, 4.0);
        // Rotating `up` with w for a short time brings it closer to Y.
        let rotated = Quat::from_scaled_axis(w * 0.01) * up;
        assert!(rotated.dot(Vec3::Y) > up.dot(Vec3::Y));
        assert_eq!(righting_angular_velocity(Vec3::Y, Vec3::Y, 4.0), Vec3::ZERO);
        // Upside down still produces a correction.
        assert!(righting_angular_velocity(-Vec3::Y, Vec3::Y, 4.0).length() > 1.0);
    }
}
