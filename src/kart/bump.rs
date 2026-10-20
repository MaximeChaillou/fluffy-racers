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

/// The velocity a kart handed to the last physics step, as the driving rewrote it, and its
/// rotation then: before the collisions of that step. Impacts are measured with it, and a contact
/// with another kart is softened from it (issue #154, [`soften_kart_contact`]).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct StepVelocity {
    /// Linear velocity (m/s).
    pub linear: Vec3,
    /// Angular velocity (rad/s).
    pub angular: Vec3,
    /// Rotation of the body.
    pub rotation: Quat,
}

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

/// Issue #154: what a contact with another kart leaves of the turn the physics step gave a kart:
/// `before`, what the driving handed to the step; `after`, what the solver gave back (its
/// velocities and the rotation it turned the body to, in `dt` s). The result is what the kart
/// keeps: `after`, with less of the turn.
///
/// The solver pushes the two bodies apart through the contact point: a corner or a nose that hits
/// off center turns the kart around (measured: a kart rear-ended 0.7 m off center by a boosted
/// kart, both turned by 27°; head on, 0.7 m off center, by 47° and 86°), and the driving then
/// drives on along the new heading, at full speed, into the walls. Only a share of that turn is
/// kept (`bump_spin_keep`, at most `bump_spin_max` per second): of the turn the step already made
/// (by its velocity and by its push out of the other body) and of the spin it leaves for the next
/// steps. The push itself (the speed the step took or gave, the heavier kart pushing the lighter
/// one further) is left as it is, and so are the pitch and roll: the kart still feels the hit, and
/// the grip takes the sideways speed within a few ticks, as before.
pub fn soften_kart_contact(
    tuning: &KartTuning,
    before: StepVelocity,
    after: StepVelocity,
    dt: f32,
) -> StepVelocity {
    let soften = |change: f32, keep: f32, max: f32| {
        let max = max.max(0.0);
        (change * keep.clamp(0.0, 1.0)).clamp(-max, max)
    };
    let up = after.rotation * Vec3::Y;
    // The heading the kart would have without the contact, and the one it has: the turn the
    // contact gave in this step.
    let flat = |direction: Vec3| (direction - up * direction.dot(up)).normalize_or_zero();
    let expected =
        flat(Quat::from_scaled_axis(before.angular * dt) * before.rotation * Vec3::NEG_Z);
    let heading = flat(after.rotation * Vec3::NEG_Z);
    let turned = up.dot(expected.cross(heading)).atan2(expected.dot(heading));
    let kept_turn = soften(turned, tuning.bump_spin_keep, tuning.bump_spin_max * dt);
    let spin = (after.angular - before.angular).dot(up);
    let kept_spin = soften(spin, tuning.bump_spin_keep, tuning.bump_spin_max);
    StepVelocity {
        linear: after.linear,
        angular: after.angular + up * (kept_spin - spin),
        rotation: (Quat::from_axis_angle(up, kept_turn - turned) * after.rotation).normalize(),
    }
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

    /// Issue #154: a contact with another kart keeps only `bump_spin_keep` (10 %) of the turn the
    /// physics step gave, and of its spin, at most `bump_spin_max` (0.5 rad/s); the push itself,
    /// the pitch and the roll are left as the solver gave them.
    #[test]
    fn a_contact_between_karts_keeps_the_push_and_a_small_share_of_the_turn() {
        let t = tuning();
        let dt = 1.0 / 60.0;
        let rotation = Quat::from_rotation_y(0.3);
        let before = StepVelocity {
            linear: Vec3::new(1.0, 0.0, -28.0),
            angular: Vec3::new(0.0, 0.5, 0.0),
            rotation,
        };
        // Nothing but the driving's own turn: nothing to soften.
        let driven = StepVelocity {
            rotation: (Quat::from_rotation_y(0.5 * dt) * rotation).normalize(),
            ..before
        };
        let kept = soften_kart_contact(&t, before, driven, dt);
        assert!(kept.rotation.angle_between(driven.rotation) < 1e-4);
        assert!((kept.angular - driven.angular).length() < 1e-5);

        // The contact spun it by 3 rad/s more (turned 3 rad/s × dt more), pushed it 4 m/s aside
        // and tipped it a little.
        let after = StepVelocity {
            linear: Vec3::new(5.0, 0.0, -27.0),
            angular: Vec3::new(0.2, 3.5, 0.0),
            rotation: (Quat::from_rotation_y(3.5 * dt) * rotation).normalize(),
        };
        let kept = soften_kart_contact(&t, before, after, dt);
        assert_eq!(kept.linear, after.linear, "the push is the solver's");
        assert!(
            (kept.angular.x - 0.2).abs() < 1e-5,
            "the roll is the solver's"
        );
        assert!((kept.angular.y - (0.5 + 3.0 * t.bump_spin_keep)).abs() < 1e-4);
        let turned = (kept.rotation * Vec3::NEG_Z).angle_between(rotation * Vec3::NEG_Z);
        let expected = (0.5 + 3.0 * t.bump_spin_keep) * dt;
        assert!((turned - expected).abs() < 1e-4, "{turned} vs {expected}");

        // A violent hit (20 rad/s): at most `bump_spin_max`, both ways.
        for spin in [20.0f32, -20.0] {
            let after = StepVelocity {
                angular: Vec3::Y * (0.5 + spin),
                rotation: (Quat::from_rotation_y((0.5 + spin) * dt) * rotation).normalize(),
                ..before
            };
            let kept = soften_kart_contact(&t, before, after, dt);
            let added = kept.angular.y - 0.5;
            assert!(
                (added - t.bump_spin_max.copysign(spin)).abs() < 1e-4,
                "{added}"
            );
        }

        // Kept whole (the admin panel: `bump_spin_keep` = 1, no maximum): the solver's.
        let whole = KartTuning {
            bump_spin_keep: 1.0,
            bump_spin_max: f32::MAX,
            ..default()
        };
        let kept = soften_kart_contact(&whole, before, after, dt);
        assert!((kept.angular - after.angular).length() < 1e-4);
        assert!(kept.rotation.angle_between(after.rotation) < 1e-4);
    }
}
