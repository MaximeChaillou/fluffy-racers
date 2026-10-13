//! Slipstream and boost pads (GD § 6 and 2, issue #13): pure logic, no ECS. Everything here is
//! unit-tested.
//!
//! - **Slipstream**: a kart less than `slipstream_range` (5 m) behind another one, inside a
//!   cone of ±`slipstream_cone` (12°) behind it, faster than `slipstream_min_speed` (60 %) of
//!   its own top speed, charges a gauge. Full after `slipstream_charge_time` (1.2 s): a boost
//!   of `slipstream_boost_duration` (1 s), and the gauge starts again from zero. Out of the
//!   wake, the gauge empties at `slipstream_decay`.
//! - **Boost pads**: a wheel touching the ground over a pad gets its boost, refreshed every
//!   tick on it (it lasts `boost_pad_duration` after the last wheel leaves the pad).

use bevy::prelude::*;

use super::tuning::KartTuning;

/// A kart's slipstream gauge, refreshed every tick.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Slipstream {
    /// Seconds of charge, 0 to `slipstream_charge_time`.
    pub charge: f32,
    /// In a wake and fast enough this tick: the gauge is charging (wind lines).
    pub charging: bool,
}

impl Slipstream {
    /// One tick of `dt` seconds. Returns true when the gauge is full: fire the slipstream
    /// boost (the gauge restarts from zero).
    pub fn step(&mut self, tuning: &KartTuning, in_wake: bool, fast_enough: bool, dt: f32) -> bool {
        self.charging = in_wake && fast_enough;
        if !self.charging {
            self.charge = (self.charge - tuning.slipstream_decay * dt).max(0.0);
            return false;
        }
        self.charge += dt;
        // Half a tick of tolerance against the rounding of the sum of `dt`s.
        if self.charge + dt * 0.5 >= tuning.slipstream_charge_time {
            self.charge = 0.0;
            return true;
        }
        false
    }

    /// Share of the gauge filled, 0 to 1.
    pub fn fill(&self, tuning: &KartTuning) -> f32 {
        (self.charge / tuning.slipstream_charge_time.max(1e-6)).clamp(0.0, 1.0)
    }
}

/// Whether a kart at `position` is in the wake of a kart at `leader` heading `leader_forward`
/// (unit): close enough behind it, inside the cone.
pub fn in_wake(tuning: &KartTuning, position: Vec3, leader: Vec3, leader_forward: Vec3) -> bool {
    let offset = position - leader;
    let distance = offset.length();
    if distance < 1e-3 || distance > tuning.slipstream_range {
        return false;
    }
    offset.dot(-leader_forward) >= distance * tuning.slipstream_cone.cos()
}

/// Whether the kart is fast enough to charge its gauge: `forward_speed` above
/// `slipstream_min_speed` of its own top speed (`max_speed × speed_scale`).
pub fn fast_enough_for_slipstream(
    tuning: &KartTuning,
    forward_speed: f32,
    speed_scale: f32,
) -> bool {
    forward_speed >= tuning.slipstream_min_speed * tuning.max_speed * speed_scale
}

/// A wheel touching the ground counts on a pad when its anchor is over the pad's rectangle and
/// at most this high above its surface (m). Wheel anchors ride ≈ 0.45 m above the road, at
/// most the suspension length (0.6 m).
pub const BOOST_PAD_REACH: f32 = 1.0;

/// Whether `point` (a grounded wheel's anchor) is over a pad lying at `pad` (its top face, local
/// Y up, unscaled) with half size `half_size` (x across, y along its local Z).
pub fn over_pad(pad: &Transform, half_size: Vec2, point: Vec3) -> bool {
    let local = pad.rotation.inverse() * (point - pad.translation);
    local.x.abs() <= half_size.x
        && local.z.abs() <= half_size.y
        && (-0.3..=BOOST_PAD_REACH).contains(&local.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn the_wake_is_a_5_m_cone_of_12_degrees_behind_the_leader() {
        let t = KartTuning::default();
        // The leader at the origin looks down -Z: its wake is toward +Z.
        let leader = Vec3::ZERO;
        let forward = Vec3::NEG_Z;
        assert!(in_wake(&t, Vec3::new(0.0, 0.0, 4.0), leader, forward));
        assert!(in_wake(&t, Vec3::new(0.0, 0.0, 4.99), leader, forward));
        assert!(
            !in_wake(&t, Vec3::new(0.0, 0.0, 5.01), leader, forward),
            "too far"
        );
        assert!(
            !in_wake(&t, Vec3::new(0.0, 0.0, -4.0), leader, forward),
            "ahead"
        );
        // 11° and 13° off the axis, 4 m away.
        let at = |degrees: f32| {
            let a = degrees.to_radians();
            Vec3::new(4.0 * a.sin(), 0.0, 4.0 * a.cos())
        };
        assert!(in_wake(&t, at(11.0), leader, forward));
        assert!(in_wake(&t, at(-11.0), leader, forward));
        assert!(!in_wake(&t, at(13.0), leader, forward));
        assert!(
            !in_wake(&t, Vec3::new(2.0, 0.0, 0.0), leader, forward),
            "alongside"
        );
        assert!(!in_wake(&t, leader, leader, forward), "itself");
    }

    #[test]
    fn the_gauge_fires_after_1_2_s_in_the_wake_and_restarts() {
        let t = KartTuning::default();
        let mut gauge = Slipstream::default();
        let mut fired_at = Vec::new();
        for tick in 1..=150 {
            if gauge.step(&t, true, true, DT) {
                fired_at.push(tick);
            }
            assert!(gauge.charging);
        }
        // 1.2 s = 72 ticks, then 72 more.
        assert_eq!(fired_at, vec![72, 144]);
    }

    #[test]
    fn too_slow_or_out_of_the_wake_the_gauge_empties_without_firing() {
        let t = KartTuning::default();
        let mut gauge = Slipstream::default();
        for _ in 0..60 {
            assert!(!gauge.step(&t, true, true, DT));
        }
        assert!((gauge.fill(&t) - 50.0 / 60.0).abs() < 1e-3);
        // Too slow: no charge, and it empties (2 s of charge per second).
        for _ in 0..12 {
            assert!(!gauge.step(&t, true, false, DT));
            assert!(!gauge.charging);
        }
        assert!(
            (gauge.charge - (1.0 - 0.4)).abs() < 1e-3,
            "{}",
            gauge.charge
        );
        // A short swerve out of the wake costs a little; back in, it fires sooner than 1.2 s.
        let mut ticks = 0;
        while !gauge.step(&t, true, true, DT) {
            ticks += 1;
            assert!(ticks < 72);
        }
        assert_eq!(ticks + 1, 36, "0.6 s left to charge");
        // Out of the wake long enough: empty.
        for _ in 0..60 {
            gauge.step(&t, false, true, DT);
        }
        assert_eq!(gauge.charge, 0.0);
    }

    #[test]
    fn fast_enough_means_60_percent_of_its_own_top_speed() {
        let t = KartTuning::default();
        let min = 0.6 * t.max_speed;
        assert!(fast_enough_for_slipstream(&t, min + 0.01, 1.0));
        assert!(!fast_enough_for_slipstream(&t, min - 0.01, 1.0));
        // A bot with a faster top speed (catch-up) needs a bit more.
        assert!(!fast_enough_for_slipstream(&t, min + 0.01, 1.03));
    }

    #[test]
    fn a_wheel_is_over_a_pad_inside_its_rectangle_and_close_to_its_surface() {
        // A pad on a road climbing toward -Z, turned and tilted.
        let pad = Transform::from_xyz(10.0, 2.0, -30.0)
            .looking_to(Vec3::new(1.0, 0.1, -1.0).normalize(), Vec3::Y);
        let half = Vec2::new(1.5, 2.0);
        let on =
            |x: f32, y: f32, z: f32| over_pad(&pad, half, pad.transform_point(Vec3::new(x, y, z)));
        assert!(on(0.0, 0.45, 0.0));
        assert!(on(1.4, 0.45, 1.9));
        assert!(on(-1.4, 0.45, -1.9));
        assert!(!on(1.6, 0.45, 0.0), "beside it");
        assert!(!on(0.0, 0.45, 2.1), "behind it");
        assert!(!on(0.0, 1.2, 0.0), "above it");
        assert!(!on(0.0, -0.5, 0.0), "under it");
    }
}
