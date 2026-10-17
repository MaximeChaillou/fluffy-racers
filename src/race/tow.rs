//! The tow pelican (GD § 2), pure logic: when to call it, and the path it carries a kart along.
//!
//! It comes for a kart that **fell** (more than `fall_depth` below the road: at once; issue #94:
//! more than `fall_depth` away from a road upside down, under it), that is
//! **out of the track** (beyond the walls for `outside_seconds`: after a big jump over a wall,
//! or a shortcut through the infield), or that is **stuck** (trying to move, throttle or brake,
//! but staying within `stuck_radius` of the same spot for `stuck_seconds`: on its side, wedged
//! against a wall). The debug key R calls it too.
//!
//! It lifts the kart, carries it high above the track and drops it on the drop point of its
//! last validated checkpoint, facing the race, at a standstill: `tow_seconds` lost in all.

use bevy::prelude::*;

use super::tuning::{RaceTuning, seconds_to_ticks};

/// Why the pelican came.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TowReason {
    Fell,
    OutOfTrack,
    Stuck,
    /// Debug key R.
    Requested,
}

/// What the watch needs to know about a racer this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TowObservation {
    pub position: Vec3,
    /// Throttle or brake pressed.
    pub trying_to_move: bool,
    /// Beyond the walls (`Course::outside_walls`).
    pub outside: bool,
    /// Height above the road surface (m).
    pub height: f32,
    /// Issue #94: the road there is upside down (`Course::project`): a kart falling off it falls
    /// away from it, `height` growing.
    pub upside_down: bool,
}

/// The tow pelican of a racer: counters that decide when it comes, and the carry in progress.
/// On every racer (`Racer`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Tow {
    /// Where the kart was when it last moved `stuck_radius` away.
    pub anchor: Vec3,
    /// Ticks spent trying to move without leaving `anchor`.
    pub stuck_ticks: u32,
    /// Consecutive ticks out of the track.
    pub outside_ticks: u32,
    /// The debug key asked for the pelican.
    pub requested: bool,
    /// The pelican is carrying the kart (its `KartState` is `Respawning` meanwhile).
    pub carry: Option<Carry>,
}

impl Tow {
    pub fn is_carrying(&self) -> bool {
        self.carry.is_some()
    }

    /// Counts this tick; returns why the pelican must come, if it must.
    pub fn observe(&mut self, tuning: &RaceTuning, obs: &TowObservation) -> Option<TowReason> {
        if std::mem::take(&mut self.requested) {
            return Some(TowReason::Requested);
        }
        // Issue #94: off a ceiling, a kart falls toward the world's ground, away from the road.
        if obs.height < -tuning.fall_depth || (obs.upside_down && obs.height > tuning.fall_depth) {
            return Some(TowReason::Fell);
        }
        self.outside_ticks = if obs.outside {
            self.outside_ticks + 1
        } else {
            0
        };
        if self.outside_ticks >= seconds_to_ticks(tuning.outside_seconds).max(1) {
            return Some(TowReason::OutOfTrack);
        }
        if !obs.trying_to_move || obs.position.distance(self.anchor) > tuning.stuck_radius {
            self.anchor = obs.position;
            self.stuck_ticks = 0;
        } else {
            self.stuck_ticks += 1;
            if self.stuck_ticks >= seconds_to_ticks(tuning.stuck_seconds).max(1) {
                return Some(TowReason::Stuck);
            }
        }
        None
    }

    /// Forgets everything (after a tow or a restart), the kart now being at `position`.
    pub fn reset(&mut self, position: Vec3) {
        *self = Self {
            anchor: position,
            ..default()
        };
    }
}

/// A kart being carried by the pelican.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub reason: TowReason,
    /// Where it was picked up and where it is dropped.
    pub from: Transform,
    pub to: Transform,
    /// Ticks carried so far, out of `duration`.
    pub ticks: u32,
    pub duration: u32,
}

impl Carry {
    pub fn new(tuning: &RaceTuning, reason: TowReason, from: Transform, to: Transform) -> Self {
        Self {
            reason,
            from,
            to,
            ticks: 0,
            duration: seconds_to_ticks(tuning.tow_seconds).max(1),
        }
    }

    /// Advances one tick; true once the kart is dropped.
    pub fn advance(&mut self) -> bool {
        self.ticks = (self.ticks + 1).min(self.duration);
        self.ticks >= self.duration
    }

    /// Pose of the carried kart now.
    pub fn pose(&self, tuning: &RaceTuning) -> Transform {
        tow_pose(
            tuning,
            &self.from,
            &self.to,
            self.ticks as f32 / self.duration as f32,
        )
    }
}

/// Placeholder animation, at `t` from 0 (pickup) to 1 (drop): straight up by `tow_lift_height`
/// above the higher of both points, across to above the drop point while turning to face the
/// race, then straight down.
pub fn tow_pose(tuning: &RaceTuning, from: &Transform, to: &Transform, t: f32) -> Transform {
    let t = t.clamp(0.0, 1.0);
    let lift = tuning.tow_lift_share.clamp(0.0, 1.0);
    let carry = tuning.tow_carry_share.clamp(0.0, 1.0 - lift);
    let altitude = from.translation.y.max(to.translation.y) + tuning.tow_lift_height;
    let high_from = from.translation.with_y(altitude);
    let high_to = to.translation.with_y(altitude);
    let ease = |x: f32| x * x * (3.0 - 2.0 * x);
    let phase = |start: f32, length: f32| {
        if length <= 0.0 {
            1.0
        } else {
            ease(((t - start) / length).clamp(0.0, 1.0))
        }
    };
    let (up, across, down) = (
        phase(0.0, lift),
        phase(lift, carry),
        phase(lift + carry, 1.0 - lift - carry),
    );
    let translation = if t <= lift {
        from.translation.lerp(high_from, up)
    } else if t <= lift + carry {
        high_from.lerp(high_to, across)
    } else {
        high_to.lerp(to.translation, down)
    };
    Transform {
        translation,
        rotation: from.rotation.slerp(to.rotation, across),
        scale: to.scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(position: Vec3, trying: bool) -> TowObservation {
        TowObservation {
            position,
            trying_to_move: trying,
            outside: false,
            height: 0.0,
            upside_down: false,
        }
    }

    #[test]
    fn a_fall_calls_the_pelican_at_once() {
        let t = RaceTuning::default();
        let mut watch = Tow::default();
        let mut o = obs(Vec3::ZERO, false);
        o.height = -4.0;
        assert_eq!(
            watch.observe(&t, &o),
            None,
            "off the raised turn: not a fall"
        );
        o.height = -5.5;
        assert_eq!(watch.observe(&t, &o), Some(TowReason::Fell));
    }

    /// Issue #94: under a road upside down (a ceiling), a kart that came off it falls toward the
    /// world's ground, away from the road: more than `fall_depth` under it, it fell. Above a road
    /// that faces up, as high is a jump.
    #[test]
    fn a_fall_off_a_ceiling_calls_the_pelican_at_once() {
        let t = RaceTuning::default();
        let mut watch = Tow::default();
        let mut o = obs(Vec3::ZERO, true);
        o.upside_down = true;
        o.height = 0.55;
        assert_eq!(watch.observe(&t, &o), None, "driving under the ceiling");
        o.height = 4.0;
        assert_eq!(watch.observe(&t, &o), None, "a bump");
        o.height = 5.5;
        assert_eq!(watch.observe(&t, &o), Some(TowReason::Fell));
        o.height = -5.5;
        assert_eq!(watch.observe(&t, &o), Some(TowReason::Fell), "through it");
        let jump = TowObservation {
            height: 8.0,
            ..obs(Vec3::ZERO, true)
        };
        assert_eq!(
            watch.observe(&t, &jump),
            None,
            "a jump over a road facing up"
        );
    }

    #[test]
    fn out_of_the_track_for_one_second() {
        let t = RaceTuning::default();
        let mut watch = Tow::default();
        let mut o = obs(Vec3::ZERO, true);
        o.outside = true;
        for _ in 0..59 {
            assert_eq!(watch.observe(&t, &o), None);
        }
        // Back on the track for a tick: the count starts over.
        let inside = obs(Vec3::ZERO, true);
        assert_eq!(watch.observe(&t, &inside), None);
        for _ in 0..59 {
            assert_eq!(watch.observe(&t, &o), None);
        }
        assert_eq!(watch.observe(&t, &o), Some(TowReason::OutOfTrack));
    }

    #[test]
    fn stuck_after_three_seconds_trying_to_move_in_place() {
        let t = RaceTuning::default();
        let mut watch = Tow::default();
        watch.reset(Vec3::ZERO);
        // Wedged: bouncing back and forth within 2 m with the throttle held.
        for tick in 0..179 {
            let x = (tick as f32 * 0.7).sin() * 1.5;
            assert_eq!(watch.observe(&t, &obs(Vec3::X * x, true)), None);
        }
        assert_eq!(
            watch.observe(&t, &obs(Vec3::ZERO, true)),
            Some(TowReason::Stuck)
        );
    }

    #[test]
    fn not_stuck_when_moving_or_waiting() {
        let t = RaceTuning::default();
        let mut watch = Tow::default();
        // Driving slowly but really moving (0.05 m per tick = 3 m/s).
        for tick in 0..600 {
            let position = Vec3::Z * tick as f32 * 0.05;
            assert_eq!(watch.observe(&t, &obs(position, true)), None);
        }
        // Standing still without touching anything: the player waits, no pelican.
        for _ in 0..600 {
            assert_eq!(watch.observe(&t, &obs(Vec3::ONE, false)), None);
        }
        // Letting go of the throttle starts the count over.
        for _ in 0..170 {
            watch.observe(&t, &obs(Vec3::ONE, true));
        }
        watch.observe(&t, &obs(Vec3::ONE, false));
        for _ in 0..170 {
            assert_eq!(watch.observe(&t, &obs(Vec3::ONE, true)), None);
        }
    }

    #[test]
    fn the_debug_key_calls_it_once() {
        let t = RaceTuning::default();
        let mut watch = Tow {
            requested: true,
            ..default()
        };
        assert_eq!(
            watch.observe(&t, &obs(Vec3::ZERO, false)),
            Some(TowReason::Requested)
        );
        assert_eq!(watch.observe(&t, &obs(Vec3::ZERO, false)), None);
    }

    #[test]
    fn the_tow_lasts_two_and_a_half_seconds_and_ends_on_the_drop_point() {
        let t = RaceTuning::default();
        let from = Transform::from_xyz(10.0, -20.0, 5.0).with_rotation(Quat::from_rotation_z(2.0));
        let to = Transform::from_xyz(0.0, 0.55, 40.0).looking_to(Vec3::NEG_Z, Vec3::Y);
        let mut tow = Carry::new(&t, TowReason::Fell, from, to);
        assert_eq!(tow.duration, 150);
        assert_eq!(tow.pose(&t).translation, from.translation);
        assert!(tow.pose(&t).rotation.angle_between(from.rotation) < 1e-3);
        let mut highest = f32::MIN;
        let mut ticks = 0;
        while !tow.advance() {
            ticks += 1;
            highest = highest.max(tow.pose(&t).translation.y);
        }
        assert_eq!(ticks + 1, 150);
        assert_eq!(tow.pose(&t).translation, to.translation);
        assert!(tow.pose(&t).rotation.angle_between(to.rotation) < 1e-3);
        // Carried well above the track on the way.
        assert!(
            (highest - (0.55 + t.tow_lift_height)).abs() < 1e-3,
            "{highest}"
        );
        // Staying done.
        assert!(tow.advance());
    }

    #[test]
    fn the_path_goes_up_across_and_down() {
        let t = RaceTuning::default();
        let from = Transform::from_xyz(0.0, 1.0, 0.0);
        let to = Transform::from_xyz(30.0, 0.0, 0.0);
        let high = 1.0 + t.tow_lift_height;
        let at = |x: f32| tow_pose(&t, &from, &to, x).translation;
        // End of the lift: straight above the pickup.
        assert!(at(t.tow_lift_share).distance(Vec3::new(0.0, high, 0.0)) < 1e-4);
        // End of the carry: straight above the drop point.
        let carried = t.tow_lift_share + t.tow_carry_share;
        assert!(at(carried).distance(Vec3::new(30.0, high, 0.0)) < 1e-4);
        assert_eq!(at(1.0), to.translation);
        // Monotonic in each phase.
        let mut previous = at(0.0);
        for i in 1..=100 {
            let p = at(i as f32 / 100.0);
            assert!(p.x >= previous.x - 1e-5);
            previous = p;
        }
    }
}
