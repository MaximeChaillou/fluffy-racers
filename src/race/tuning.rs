//! Race tuning. Default values come from `docs/GAME_DESIGN.md` § 1 and 2 (marked "à régler"
//! there) or are our own starting points: tweak them here, never hard-code them in systems.

use bevy::prelude::*;

use crate::TICK_HZ;

/// All the knobs of a race: format, countdown, checkpoints and the tow pelican.
///
/// Durations are in seconds (converted to 60 Hz ticks with [`seconds_to_ticks`]), distances in
/// meters.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
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
    /// Issue #70: the results screen follows the racers still on track this long after it
    /// appeared (their times come in as they finish), then closes: those still on track get an
    /// estimated time, and the table is final. Read live.
    pub results_close_seconds: f32,
    /// An estimated time drives the rest of the race at the racer's average race speed, at least
    /// this one (m/s): a racer that barely moved still gets a time. Read live.
    pub results_min_speed: f32,

    // --- Checkpoints (anti-shortcut) ----------------------------------------------------------
    /// A kart's progress is tracked from this far behind its last checkpoint to its next one:
    /// driving back further does not lower it more, and nothing outside this stretch of track
    /// can validate a checkpoint.
    pub backward_window: f32,
    /// A checkpoint gate reaches this far beyond the walls (m on each side of the road).
    pub gate_margin: f32,
    /// A kart more than this above or below the road does not cross a gate.
    pub gate_height: f32,
    /// Issue #107: a kart off the road is on a shortcut while within this much of its path
    /// (beyond its half width, before its start, after its end; m)...
    pub shortcut_margin: f32,
    /// ...and back on the road where it ends within this much of its end (m, along the
    /// centerline): there it crosses the checkpoints the shortcut passed by.
    pub shortcut_exit_reach: f32,

    // --- Tow pelican (GD § 2) ------------------------------------------------------------------
    /// Time lost with the tow pelican, from the pickup to the drop (GD § 2: about 2.5 s).
    pub tow_seconds: f32,
    /// Share of the tow spent lifting the kart, then carrying it; the rest drops it.
    pub tow_lift_share: f32,
    pub tow_carry_share: f32,
    /// The pelican carries the kart this high above the higher of the pickup and drop points.
    pub tow_lift_height: f32,
    /// Issue #96: under a road that faces down (a ceiling), the pelican first takes the kart out
    /// from under it, this far beyond its edge and below its walls (m), then lifts it: never up
    /// through the road.
    pub tow_clearance: f32,
    /// Distance between two drop lanes, as a share of the half road width: karts dropped at the
    /// same point use the centre, left or right lane by grid slot (#6 brings 11 bots).
    pub tow_drop_lane: f32,
    /// More than this below the road surface: the kart fell, the pelican comes at once. Issue
    /// #94: under a road upside down (a ceiling), more than this away from it, under it.
    pub fall_depth: f32,
    /// Beyond the outer face of the walls by this much, the kart is out of the track...
    pub outside_margin: f32,
    /// ...and after this long out of the track, the pelican brings it back.
    pub outside_seconds: f32,
    /// A kart trying to move (throttle or brake) that stays within `stuck_radius` of the same
    /// spot for `stuck_seconds` is stuck: the pelican brings it back.
    pub stuck_seconds: f32,
    pub stuck_radius: f32,

    // --- Time trial (GD § 1, issue #76) --------------------------------------------------------
    /// Piments in the hand at the start of a time trial (GD § 1: 3, an « x3 »), and no other
    /// item. Read at each (re)start of the race.
    pub time_trial_chilis: u32,

    // --- Obstacles and passers-by (GD § 16, issue #144) ------------------------------------------
    /// What the hazards of a circuit do: the knock of a moving obstacle or a passer-by, the
    /// things to drive through.
    pub hazards: HazardTuning,
}

/// Issue #144 (GD « Conception des circuits »): the hazards of a circuit (`crate::track::hazard`).
/// A field of [`RaceTuning`] rather than a resource of its own: a new resource is a new archetype
/// in Bevy, which reorders the colliders in Rapier and changes every bot race (`STATUS.md`).
#[derive(Reflect, Clone, Debug, PartialEq)]
#[reflect(Default)]
pub struct HazardTuning {
    /// A moving obstacle or a passer-by knocks a kart over: a tumble this long (s; an item's
    /// tumble is 1.6 s, GD § 8), its drift and boost lost, then its invincibility (its stat).
    pub knock_seconds: f32,
    /// The kart keeps this share of its speed...
    pub knock_speed_kept: f32,
    /// ...and is pushed this fast away from what knocked it (m/s, along the road).
    pub knock_push: f32,
    /// The contact shapes count as touching this much before they meet (m; negative: they must
    /// overlap this much).
    pub contact_margin: f32,
    /// A thing to drive through comes back this long after it burst (s; an item box: 2 s).
    pub respawn_seconds: f32,
    /// What bursting one costs: the share of its speed the kart loses (GD: a little, or none).
    pub hay_slowdown: f32,
    pub crate_slowdown: f32,
    pub balloon_slowdown: f32,
    /// Issue #156 (GD « Conception des circuits »): a shortcut's panel, broken by a kart with a
    /// boost, comes back this long after (s; the same tick on every peer).
    pub panel_respawn_seconds: f32,
    /// Issue #156: how much a kart without a boost bounces off a panel (its restitution; a wall's
    /// is 0.3: a gentle bounce, the kart is not thrown back).
    pub panel_bounce: f32,
    /// Issue #156: a kart with a boost breaks a panel this far before its contact shape touches
    /// it (m; on top of the way it goes in two ticks): it never bumps into it.
    pub panel_reach: f32,
}

impl Default for HazardTuning {
    fn default() -> Self {
        Self {
            knock_seconds: 1.4,
            knock_speed_kept: 0.3,
            knock_push: 4.0,
            contact_margin: 0.0,
            respawn_seconds: 5.0,
            hay_slowdown: 0.08,
            crate_slowdown: 0.04,
            balloon_slowdown: 0.0,
            panel_respawn_seconds: 6.0,
            panel_bounce: 0.15,
            panel_reach: 0.3,
        }
    }
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
            results_close_seconds: 10.0,
            results_min_speed: 5.0,

            backward_window: 30.0,
            gate_margin: 1.0,
            gate_height: 10.0,
            shortcut_margin: 1.5,
            shortcut_exit_reach: 30.0,

            tow_seconds: 2.5,
            tow_lift_share: 0.3,
            tow_carry_share: 0.4,
            tow_lift_height: 6.0,
            tow_clearance: 1.5,
            tow_drop_lane: 0.4,
            fall_depth: 5.0,
            outside_margin: 0.5,
            outside_seconds: 1.0,
            stuck_seconds: 3.0,
            stuck_radius: 2.0,

            time_trial_chilis: 3,

            hazards: HazardTuning::default(),
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
        assert_eq!(t.time_trial_chilis, 3, "GD § 1: three piments");
        assert!(t.tow_lift_share + t.tow_carry_share < 1.0);
        // Issue #144: things to drive through slow a kart down a little, or not at all.
        let h = &t.hazards;
        for slowdown in [h.hay_slowdown, h.crate_slowdown, h.balloon_slowdown] {
            assert!((0.0..=0.1).contains(&slowdown), "{slowdown}");
        }
        assert_eq!(h.balloon_slowdown, 0.0);
        assert!(h.knock_seconds > 0.5 && h.knock_seconds <= 1.6);
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
