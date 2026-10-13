//! Bumps between karts (GD § 7, issue #14): pure logic, no ECS. Everything here is unit-tested.
//!
//! After each physics step, every pair of karts the solver actually pushed apart is an impact.
//! It is measured with the velocities the karts had *before* the step (`StepVelocity`): the
//! closing speed along the contact normal. From `bump_min_speed` (5 m/s) it is a bump, and the
//! kart that brought most of the closing speed (`bump_bumper_share`, 60 %) is the bumper: the
//! other one is bumped ("se faire percuter"). Head on, at about the same speed, both are.
//! Scraping along a kart or pushing it stays far below the threshold.

use bevy::prelude::*;

use super::tuning::KartTuning;

/// The linear velocity a kart handed to the last physics step, as the driving rewrote it: before
/// the collisions of that step. Impacts are measured with it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct StepVelocity(pub Vec3);

/// `kart` was bumped by the kart `by` (GD § 7): written after the physics step of the impact.
/// The croquettes (issue #14) make the bumped kart lose one; the items, the screen shake or the
/// anti-gravity spin boost (GD § 10) can listen to it too.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct KartBump {
    pub kart: Entity,
    pub by: Entity,
    /// Closing speed of the impact (m/s).
    pub closing_speed: f32,
}

/// Who got bumped when kart A (velocity `va` before the impact) and kart B (`vb`) hit each
/// other, `normal` pointing from A toward B: `(A bumped, B bumped)`, or `None` below
/// `bump_min_speed` of closing speed.
pub fn bump_victims(tuning: &KartTuning, va: Vec3, vb: Vec3, normal: Vec3) -> Option<(bool, bool)> {
    let normal = normal.normalize_or_zero();
    // How fast each kart comes at the other one.
    let a_toward_b = va.dot(normal);
    let b_toward_a = -vb.dot(normal);
    let closing = a_toward_b + b_toward_a;
    if normal == Vec3::ZERO || closing < tuning.bump_min_speed {
        return None;
    }
    let share = tuning.bump_bumper_share;
    let a_bumps = a_toward_b >= closing * share;
    let b_bumps = b_toward_a >= closing * share;
    // The bumper is not bumped; when neither clearly is (head on), both are.
    Some((!a_bumps, !b_bumps))
}

/// Closing speed of two karts along `normal` (from A toward B): positive when they come
/// together.
pub fn closing_speed(va: Vec3, vb: Vec3, normal: Vec3) -> f32 {
    (va - vb).dot(normal.normalize_or_zero())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> KartTuning {
        KartTuning::default()
    }

    #[test]
    fn rear_ending_a_slower_kart_bumps_the_one_in_front_only() {
        let t = tuning();
        // A behind at 28 m/s, B ahead at 20 m/s, both heading -Z: the normal from A to B is -Z.
        let va = Vec3::new(0.0, 0.0, -28.0);
        let vb = Vec3::new(0.0, 0.0, -20.0);
        assert_eq!(bump_victims(&t, va, vb, Vec3::NEG_Z), Some((false, true)));
        // Seen from the other side: the same impact.
        assert_eq!(bump_victims(&t, vb, va, Vec3::Z), Some((true, false)));
        assert!((closing_speed(va, vb, Vec3::NEG_Z) - 8.0).abs() < 1e-5);
    }

    #[test]
    fn a_kart_standing_still_is_bumped_by_the_one_driving_into_it() {
        let t = tuning();
        let va = Vec3::new(0.0, 0.0, -10.0);
        assert_eq!(
            bump_victims(&t, va, Vec3::ZERO, Vec3::NEG_Z),
            Some((false, true))
        );
        // Swerving into the side of a kart (6 m/s sideways).
        let swerve = Vec3::new(6.0, 0.0, -27.0);
        let straight = Vec3::new(0.0, 0.0, -27.0);
        assert_eq!(
            bump_victims(&t, swerve, straight, Vec3::X),
            Some((false, true))
        );
    }

    #[test]
    fn head_on_both_karts_are_bumped() {
        let t = tuning();
        let va = Vec3::new(0.0, 0.0, -20.0);
        let vb = Vec3::new(0.0, 0.0, 21.0);
        assert_eq!(bump_victims(&t, va, vb, Vec3::NEG_Z), Some((true, true)));
    }

    #[test]
    fn rubbing_pushing_or_separating_is_no_bump() {
        let t = tuning();
        // Side by side at the same speed, a little sideways push (2 m/s).
        let va = Vec3::new(2.0, 0.0, -28.0);
        let vb = Vec3::new(0.0, 0.0, -28.0);
        assert_eq!(bump_victims(&t, va, vb, Vec3::X), None);
        // Just below the threshold, rear end.
        let slow = Vec3::new(0.0, 0.0, -(28.0 - t.bump_min_speed + 0.01));
        assert_eq!(
            bump_victims(&t, Vec3::new(0.0, 0.0, -28.0), slow, Vec3::NEG_Z),
            None
        );
        // Moving apart.
        assert_eq!(bump_victims(&t, vb, va, Vec3::X), None);
        // No normal: nothing to measure.
        assert_eq!(bump_victims(&t, va, Vec3::ZERO, Vec3::ZERO), None);
    }
}
