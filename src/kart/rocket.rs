//! Rocket start (GD § 4, issue #13): pure logic, no ECS. Everything here is unit-tested.
//!
//! `t` is the time from the appearance of the "2" of the countdown to the **first** press of
//! the accelerator:
//! - pressed before the "2" (on the grid or during the "3"): the engine **stalls** for
//!   `rocket_start_stall` (1 s) after GO, the kart cannot move;
//! - `t ≤ rocket_start_perfect` (0.25 s): the longest boost, `rocket_start_max_duration` (2.5 s);
//! - then the boost shrinks linearly down to `rocket_start_min_duration` (0.5 s) at
//!   `t = rocket_start_late` (1 s, when the "1" shows);
//! - pressed later, or not before GO: a normal start.
//!
//! The race rules (`crate::race`) watch the accelerator during the countdown, before the karts
//! are held still, and apply the outcome on GO: a `Boost`, or a [`Stall`] that the kart obeys.

use bevy::prelude::*;

use super::tuning::KartTuning;

/// The accelerator counts as pressed above this (digital buttons give 0 or 1).
pub const THROTTLE_PRESSED: f32 = 0.5;

/// The first press of the accelerator during a race start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartPress {
    /// Not pressed yet.
    #[default]
    NotYet,
    /// First pressed before the "2": the engine will stall.
    Early,
    /// First pressed this many ticks after the "2" appeared (0 = on that very tick).
    After(u32),
}

impl StartPress {
    /// Records one tick before GO. `ticks_since_two` is `None` before the "2" (see
    /// `RaceFlow::ticks_since_two`). Only the first press counts: releasing and pressing again
    /// changes nothing.
    pub fn observe(&mut self, ticks_since_two: Option<u32>, throttle: f32) {
        if *self != StartPress::NotYet || throttle <= THROTTLE_PRESSED {
            return;
        }
        *self = match ticks_since_two {
            None => StartPress::Early,
            Some(ticks) => StartPress::After(ticks),
        };
    }
}

/// What happens on GO.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StartOutcome {
    /// A normal start.
    Normal,
    /// The engine stalls this long (s).
    Stall(f32),
    /// A boost: `(strength, duration in seconds)` for `Boost::add`.
    Boost(f32, f32),
}

/// Boost duration of a start first pressed `t` seconds after the "2", if it earns one.
pub fn rocket_start_duration(tuning: &KartTuning, t: f32) -> Option<f32> {
    if !(0.0..=tuning.rocket_start_late).contains(&t) {
        return None;
    }
    if t <= tuning.rocket_start_perfect {
        return Some(tuning.rocket_start_max_duration);
    }
    let span = (tuning.rocket_start_late - tuning.rocket_start_perfect).max(1e-6);
    let late = (t - tuning.rocket_start_perfect) / span;
    Some(
        tuning.rocket_start_max_duration
            + (tuning.rocket_start_min_duration - tuning.rocket_start_max_duration) * late,
    )
}

/// The outcome of a start, judged on GO. `tick_hz`: ticks per second of the simulation.
pub fn start_outcome(tuning: &KartTuning, press: StartPress, tick_hz: f32) -> StartOutcome {
    match press {
        StartPress::NotYet => StartOutcome::Normal,
        StartPress::Early => StartOutcome::Stall(tuning.rocket_start_stall),
        StartPress::After(ticks) => {
            match rocket_start_duration(tuning, ticks as f32 / tick_hz.max(1.0)) {
                Some(duration) => StartOutcome::Boost(tuning.rocket_start_strength, duration),
                None => StartOutcome::Normal,
            }
        }
    }
}

/// The engine stalled after a start pressed too early (GD § 4: "moteur qui crache, kart
/// immobile"). While `remaining` (s) is positive the kart ignores its accelerator, brake and
/// jump button. The race rules set it on GO; it counts down with the boosts.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Stall {
    pub remaining: f32,
}

impl Stall {
    pub fn is_active(&self) -> bool {
        self.remaining > 0.0
    }

    /// Counts the stall down by `dt` seconds. Less than half a tick left counts as over, so
    /// that a 1 s stall lasts exactly 60 ticks despite the rounding of the sum of `dt`s.
    pub fn tick(&mut self, dt: f32) {
        if self.is_active() {
            self.remaining -= dt;
            if self.remaining < dt * 0.5 {
                self.remaining = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HZ: f32 = 60.0;

    fn ticks(seconds: f32) -> u32 {
        (seconds * HZ).round() as u32
    }

    /// Outcome of a start first pressed `seconds` after the "2" (negative: before it).
    fn start_at(t: &KartTuning, seconds: f32) -> StartOutcome {
        let two = ticks(1.0);
        let mut press = StartPress::default();
        // From the grid (2 s before the "2") to GO (2 s after it).
        let first = (two as f32 + seconds * HZ).round() as i64;
        for tick in 0..(two + ticks(2.0)) as i64 {
            let since_two = (tick as u32).checked_sub(two);
            let throttle = if tick >= first { 1.0 } else { 0.0 };
            press.observe(since_two, throttle);
        }
        start_outcome(t, press, HZ)
    }

    fn boost_duration(outcome: StartOutcome) -> Option<f32> {
        match outcome {
            StartOutcome::Boost(_, duration) => Some(duration),
            _ => None,
        }
    }

    /// Issue #13 acceptance, in pure logic: pressed 0.1 s, 0.6 s and 1.2 s after the "2" → a
    /// boost of 2.5 s, of about 1.6 s, then none.
    #[test]
    fn pressing_0_1_0_6_and_1_2_s_after_the_two() {
        let t = KartTuning::default();
        assert_eq!(boost_duration(start_at(&t, 0.1)), Some(2.5));
        let d = boost_duration(start_at(&t, 0.6)).unwrap();
        // 2.5 − 2 × (0.6 − 0.25) / 0.75 = 1.567 s.
        assert!((d - 1.567).abs() < 0.01, "{d}");
        assert_eq!(start_at(&t, 1.2), StartOutcome::Normal);
        match start_at(&t, 0.1) {
            StartOutcome::Boost(strength, _) => assert_eq!(strength, t.rocket_start_strength),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pressing_before_the_two_stalls() {
        let t = KartTuning::default();
        for early in [-2.0, -1.0, -0.5, -1.0 / HZ] {
            assert_eq!(start_at(&t, early), StartOutcome::Stall(1.0), "{early} s");
        }
        // On the very tick the "2" appears: perfect.
        assert_eq!(boost_duration(start_at(&t, 0.0)), Some(2.5));
    }

    #[test]
    fn the_boost_is_perfect_up_to_0_25_s_then_shrinks_linearly_to_0_5_s_at_1_s() {
        let t = KartTuning::default();
        assert_eq!(rocket_start_duration(&t, 0.0), Some(2.5));
        assert_eq!(rocket_start_duration(&t, 0.25), Some(2.5));
        let mid = rocket_start_duration(&t, 0.625).unwrap();
        assert!((mid - 1.5).abs() < 1e-5, "{mid}");
        assert!((rocket_start_duration(&t, 1.0).unwrap() - 0.5).abs() < 1e-5);
        assert_eq!(rocket_start_duration(&t, 1.01), None);
        assert_eq!(rocket_start_duration(&t, -0.01), None);
        // Strictly decreasing in between, tick by tick.
        let mut previous = f32::MAX;
        for tick in ticks(0.25)..=ticks(1.0) {
            let d = boost_duration(start_outcome(&t, StartPress::After(tick), HZ)).unwrap();
            assert!(d <= previous, "tick {tick}");
            assert!((0.5 - 1e-5..=2.5).contains(&d));
            previous = d;
        }
        // When the "1" shows (60 ticks after the "2"): the last boost; one tick later, none.
        assert!(boost_duration(start_outcome(&t, StartPress::After(60), HZ)).is_some());
        assert_eq!(
            start_outcome(&t, StartPress::After(61), HZ),
            StartOutcome::Normal
        );
    }

    #[test]
    fn only_the_first_press_counts_and_no_press_is_a_normal_start() {
        let t = KartTuning::default();
        let mut press = StartPress::default();
        press.observe(None, 0.0);
        press.observe(Some(3), 0.4);
        assert_eq!(press, StartPress::NotYet, "half throttle is not a press");
        assert_eq!(start_outcome(&t, press, HZ), StartOutcome::Normal);
        // Pressed too early, released, pressed again perfectly: still a stall.
        press.observe(None, 1.0);
        press.observe(Some(5), 1.0);
        assert_eq!(press, StartPress::Early);
        let mut press = StartPress::default();
        press.observe(Some(10), 1.0);
        press.observe(Some(40), 1.0);
        assert_eq!(press, StartPress::After(10));
    }

    #[test]
    fn a_stall_counts_down_to_zero() {
        let mut stall = Stall { remaining: 1.0 };
        let mut ticks = 0;
        while stall.is_active() {
            stall.tick(1.0 / HZ);
            ticks += 1;
            assert!(ticks <= 61);
        }
        assert_eq!(ticks, 60);
        stall.tick(1.0);
        assert_eq!(stall.remaining, 0.0);
        let mut stall = Stall { remaining: 0.5 };
        stall.tick(1.0);
        assert_eq!(stall.remaining, 0.0);
    }
}
