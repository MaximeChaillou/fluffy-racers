//! Tricks off trick ramps (GD § 5, issue #13): pure logic, no ECS. Everything here is
//! unit-tested.
//!
//! Trick ramps are marked in the circuit file (`trick_ramps`); their collider carries
//! `crate::track::TrickRamp`. The kart **takes off** when its last wheel leaves a trick ramp
//! without touching any other ground. Pressing the jump/drift button at most `trick_window`
//! (0.2 s) before or after the takeoff starts a **trick** (`KartState::Trick`, an animation)
//! until the landing, which gives a short boost. No press in the window: a plain jump, no boost.
//!
//! On a trick ramp the button does not hop: a hop would leave the ramp, land on it again and
//! count as a trick anywhere on it. There, the button only arms the trick for the takeoff.

use bevy::prelude::*;

/// What the trick rules remember between ticks: when the kart last stood on a trick ramp and
/// when the jump/drift button was last pressed.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrickTracker {
    /// Ticks since a wheel last touched a trick ramp: 0 on it, then counting in the air after
    /// it; `None` once the kart touches any other ground (or never touched a ramp).
    since_ramp: Option<u32>,
    /// Ticks since the button was last pressed (`None`: never).
    since_press: Option<u32>,
}

impl TrickTracker {
    /// Moves to this tick. `on_ramp`: a grounded wheel stands on a trick ramp;
    /// `wheels_on_ground`: grounded wheels, on any surface; `pressed`: the button was pressed
    /// this tick (not just held).
    pub fn step(&mut self, on_ramp: bool, wheels_on_ground: usize, pressed: bool) {
        self.since_press = if pressed {
            Some(0)
        } else {
            self.since_press.map(|t| t.saturating_add(1))
        };
        self.since_ramp = if on_ramp {
            Some(0)
        } else if wheels_on_ground > 0 {
            None
        } else {
            self.since_ramp.map(|t| t.saturating_add(1))
        };
    }

    /// Whether a wheel stood on a trick ramp this tick.
    pub fn on_ramp(&self) -> bool {
        self.since_ramp == Some(0)
    }

    /// Ticks since the takeoff from a trick ramp (1 on the first tick in the air), while the
    /// kart is still in the air after it.
    pub fn ticks_since_takeoff(&self) -> Option<u32> {
        self.since_ramp.filter(|t| *t > 0)
    }

    /// Whether the last press falls at most `window` ticks before or after the takeoff, the
    /// kart still in the air: a trick starts now (if none is going on).
    pub fn trick_ready(&self, window: u32) -> bool {
        match (self.ticks_since_takeoff(), self.since_press) {
            // Takeoff `k` ticks ago, press `s` ticks ago: they are `|k − s|` ticks apart.
            (Some(k), Some(s)) => k.abs_diff(s) <= window,
            _ => false,
        }
    }

    /// Whether a press on the next tick would start a trick (what a bot looks at).
    pub fn window_open(&self, window: u32) -> bool {
        self.ticks_since_takeoff()
            .is_some_and(|k| k.saturating_add(1) <= window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: u32 = 12;

    /// Drives `ramp_ticks` on the ramp, then flies; presses the button on tick `press` (counted
    /// from the first ramp tick). Returns the tick where a trick becomes ready, if any, within
    /// `air_ticks` in the air.
    fn takeoff(ramp_ticks: u32, air_ticks: u32, press: Option<u32>) -> Option<u32> {
        let mut tracker = TrickTracker::default();
        for tick in 0..ramp_ticks + air_ticks {
            let on_ramp = tick < ramp_ticks;
            let wheels = if on_ramp { 4 } else { 0 };
            tracker.step(on_ramp, wheels, press == Some(tick));
            if tracker.trick_ready(WINDOW) {
                return Some(tick);
            }
        }
        None
    }

    #[test]
    fn no_press_no_trick() {
        assert_eq!(takeoff(15, 40, None), None);
    }

    #[test]
    fn a_press_right_after_the_takeoff_starts_a_trick_at_once() {
        // Last ramp tick: 14; first tick in the air: 15.
        assert_eq!(takeoff(15, 40, Some(15)), Some(15));
        assert_eq!(takeoff(15, 40, Some(20)), Some(20));
        // 12 ticks (0.2 s) after the takeoff: still in time.
        assert_eq!(takeoff(15, 40, Some(14 + WINDOW)), Some(14 + WINDOW));
        // One tick later: too late.
        assert_eq!(takeoff(15, 40, Some(15 + WINDOW)), None);
    }

    #[test]
    fn a_press_just_before_the_takeoff_starts_the_trick_on_the_first_tick_in_the_air() {
        assert_eq!(takeoff(15, 40, Some(14)), Some(15));
        assert_eq!(takeoff(15, 40, Some(14 - WINDOW)), Some(15));
        // Pressed 13 ticks before leaving the ramp: too early.
        assert_eq!(takeoff(15, 40, Some(13 - WINDOW)), None);
    }

    #[test]
    fn landing_or_any_other_ground_closes_the_window() {
        let mut tracker = TrickTracker::default();
        tracker.step(true, 4, false);
        assert!(tracker.on_ramp());
        tracker.step(false, 0, false);
        assert_eq!(tracker.ticks_since_takeoff(), Some(1));
        assert!(tracker.window_open(WINDOW));
        // Back on the road: no trick even when pressing now.
        tracker.step(false, 2, true);
        assert_eq!(tracker.ticks_since_takeoff(), None);
        assert!(!tracker.trick_ready(WINDOW));
        tracker.step(false, 0, false);
        assert!(!tracker.trick_ready(WINDOW));
        // Driving off a plain ledge (no ramp): never.
        let mut tracker = TrickTracker::default();
        tracker.step(false, 4, false);
        tracker.step(false, 0, true);
        assert!(!tracker.trick_ready(WINDOW));
    }

    #[test]
    fn the_bots_see_when_the_window_is_open() {
        let mut tracker = TrickTracker::default();
        tracker.step(true, 4, false);
        assert!(!tracker.window_open(WINDOW), "still on the ramp");
        for k in 1..=WINDOW + 2 {
            tracker.step(false, 0, false);
            assert_eq!(tracker.window_open(WINDOW), k < WINDOW, "{k}");
        }
    }
}
