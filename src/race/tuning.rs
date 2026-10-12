//! Race tuning. Default values come from `docs/GAME_DESIGN.md` § 1 and 2 (marked "à régler"
//! there) or are our own starting points: tweak them here, never hard-code them in systems.

use bevy::prelude::*;

use crate::TICK_HZ;

/// All the knobs of a race: format, countdown, checkpoints and the tow pelican.
///
/// Durations are in seconds (converted to 60 Hz ticks with [`seconds_to_ticks`]), distances in
/// meters.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct RaceTuning {
    // --- Format (GD § 1) ----------------------------------------------------------------------
    /// Laps of a race (GD § 1: 3, like most MK8DX tracks).
    pub laps: u32,
    /// Time on the grid before the countdown starts: the karts are placed, nothing moves.
    pub grid_seconds: f32,
    /// Countdown steps: 3 = "3, 2, 1, GO" (GD § 1).
    pub countdown_steps: u32,
    /// Duration of each countdown step (GD § 1: one second).
    pub countdown_step_seconds: f32,
    /// "GO !" stays on screen this long after the start.
    pub go_display_seconds: f32,
    /// "DERNIER TOUR !" stays on screen this long at the start of the last lap.
    pub final_lap_banner_seconds: f32,
    /// Finish banner shown once the race is over, before the results screen.
    pub finish_seconds: f32,
    /// The results screen ignores the restart buttons this long (a button still held from the
    /// race must not restart it at once).
    pub restart_delay_seconds: f32,

    // --- Checkpoints (anti-shortcut) ----------------------------------------------------------
    /// A kart's progress is tracked from this far behind its last checkpoint to its next one:
    /// driving back further does not lower it more, and nothing outside this stretch of track
    /// can validate a checkpoint.
    pub backward_window: f32,
    /// A checkpoint gate reaches this far beyond the walls (m on each side of the road).
    pub gate_margin: f32,
    /// A kart more than this above or below the road does not cross a gate.
    pub gate_height: f32,

    // --- Tow pelican (GD § 2) ------------------------------------------------------------------
    /// Time lost with the tow pelican, from the pickup to the drop (GD § 2: about 2.5 s).
    pub tow_seconds: f32,
    /// Share of the tow spent lifting the kart, then carrying it; the rest drops it.
    pub tow_lift_share: f32,
    pub tow_carry_share: f32,
    /// The pelican carries the kart this high above the higher of the pickup and drop points.
    pub tow_lift_height: f32,
    /// Distance between two drop lanes, as a share of the half road width: karts dropped at the
    /// same point use the centre, left or right lane by grid slot (#6 brings 11 bots).
    pub tow_drop_lane: f32,
    /// More than this below the road surface: the kart fell, the pelican comes at once.
    pub fall_depth: f32,
    /// Beyond the outer face of the walls by this much, the kart is out of the track...
    pub outside_margin: f32,
    /// ...and after this long out of the track, the pelican brings it back.
    pub outside_seconds: f32,
    /// A kart trying to move (throttle or brake) that stays within `stuck_radius` of the same
    /// spot for `stuck_seconds` is stuck: the pelican brings it back.
    pub stuck_seconds: f32,
    pub stuck_radius: f32,
}

impl Default for RaceTuning {
    fn default() -> Self {
        Self {
            laps: 3,
            grid_seconds: 1.0,
            countdown_steps: 3,
            countdown_step_seconds: 1.0,
            go_display_seconds: 1.0,
            final_lap_banner_seconds: 2.0,
            finish_seconds: 4.0,
            restart_delay_seconds: 1.0,

            backward_window: 30.0,
            gate_margin: 1.0,
            gate_height: 10.0,

            tow_seconds: 2.5,
            tow_lift_share: 0.3,
            tow_carry_share: 0.4,
            tow_lift_height: 6.0,
            tow_drop_lane: 0.4,
            fall_depth: 5.0,
            outside_margin: 0.5,
            outside_seconds: 1.0,
            stuck_seconds: 3.0,
            stuck_radius: 2.0,
        }
    }
}

/// Seconds → simulation ticks (60 Hz), rounded.
pub fn seconds_to_ticks(seconds: f32) -> u32 {
    (seconds.max(0.0) as f64 * TICK_HZ).round() as u32
}

/// Simulation ticks → seconds.
pub fn ticks_to_seconds(ticks: u32) -> f32 {
    (ticks as f64 / TICK_HZ) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_game_design() {
        let t = RaceTuning::default();
        assert_eq!(t.laps, 3);
        assert_eq!(t.countdown_steps, 3);
        assert_eq!(seconds_to_ticks(t.countdown_step_seconds), 60);
        assert_eq!(seconds_to_ticks(t.tow_seconds), 150);
        assert!(t.tow_lift_share + t.tow_carry_share < 1.0);
    }

    #[test]
    fn ticks_and_seconds_round_trip() {
        assert_eq!(seconds_to_ticks(1.0), 60);
        assert_eq!(seconds_to_ticks(0.0), 0);
        assert_eq!(seconds_to_ticks(-3.0), 0);
        assert_eq!(seconds_to_ticks(2.51), 151);
        assert!((ticks_to_seconds(90) - 1.5).abs() < 1e-6);
    }
}
