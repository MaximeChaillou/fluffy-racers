//! HUD animations (pure): the croquette counter "pops" when a croquette is picked up and shakes
//! when croquettes are lost (GD § 14: "un « pop » et un son à chaque croquette"); the lap counter
//! pops, in gold, when the followed racer crosses the line into a new lap (issue #79).

/// The pop lasts this long (s)...
pub const POP_TIME: f32 = 0.25;
/// ...and the counter grows by this much at its peak.
pub const POP_GROWTH: f32 = 0.45;
/// The lap counter's pop: longer and bigger, a lap is rarer than a croquette (s, share).
pub const LAP_POP_TIME: f32 = 0.6;
pub const LAP_POP_GROWTH: f32 = 0.6;
/// The shake lasts this long (s), this wide (px) and this fast (Hz).
pub const SHAKE_TIME: f32 = 0.35;
pub const SHAKE_PX: f32 = 7.0;
pub const SHAKE_HZ: f32 = 22.0;

/// Scale of the counter `t` seconds after a pick-up: 1, up to 1 + `POP_GROWTH` and back to
/// exactly 1 (a sine bump).
pub fn pop_scale(t: f32) -> f32 {
    bump(t, POP_TIME, POP_GROWTH)
}

/// Scale of the lap counter `t` seconds after a new lap: the same bump, [`LAP_POP_TIME`] long.
pub fn lap_pop_scale(t: f32) -> f32 {
    bump(t, LAP_POP_TIME, LAP_POP_GROWTH)
}

/// 1, up to 1 + `growth` halfway through `time` and back to exactly 1 (a sine bump).
fn bump(t: f32, time: f32, growth: f32) -> f32 {
    if !(0.0..time).contains(&t) {
        return 1.0;
    }
    1.0 + growth * (std::f32::consts::PI * t / time).sin()
}

/// Whether the lap counter pops when the HUD goes from showing `old` to `new` (laps, from 1, out of
/// `laps`): only when the followed racer starts a new lap of the same race, never on a restart
/// (back to lap 1) nor when the race's number of laps changes.
pub fn lap_pops(old: (u32, u32), new: (u32, u32)) -> bool {
    let ((old_lap, old_laps), (lap, laps)) = (old, new);
    laps == old_laps && lap > old_lap
}

/// Horizontal offset of the counter `t` seconds after losing croquettes (px): a fading shake.
pub fn shake_offset(t: f32) -> f32 {
    if !(0.0..SHAKE_TIME).contains(&t) {
        return 0.0;
    }
    let fade = 1.0 - t / SHAKE_TIME;
    SHAKE_PX * fade * (std::f32::consts::TAU * SHAKE_HZ * t).sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_pops_then_settles() {
        assert_eq!(pop_scale(0.0), 1.0);
        assert!((pop_scale(POP_TIME / 2.0) - (1.0 + POP_GROWTH)).abs() < 1e-5);
        assert!(pop_scale(0.05) > 1.2);
        assert_eq!(pop_scale(POP_TIME), 1.0);
        assert_eq!(pop_scale(5.0), 1.0);
        assert_eq!(pop_scale(-1.0), 1.0);
        for i in 0..=50 {
            let s = pop_scale(POP_TIME * i as f32 / 50.0);
            assert!((1.0..=1.0 + POP_GROWTH + 1e-5).contains(&s), "{s}");
        }
    }

    /// Issue #79: the lap counter pops like the croquettes' one, longer and bigger, then settles
    /// at exactly 1.
    #[test]
    fn the_lap_counter_pops_then_settles() {
        assert_eq!(lap_pop_scale(0.0), 1.0);
        assert!((lap_pop_scale(LAP_POP_TIME / 2.0) - (1.0 + LAP_POP_GROWTH)).abs() < 1e-5);
        assert!(lap_pop_scale(POP_TIME) > pop_scale(POP_TIME / 2.0));
        assert_eq!(lap_pop_scale(LAP_POP_TIME), 1.0);
        assert_eq!(lap_pop_scale(-1.0), 1.0);
        for i in 0..=50 {
            let s = lap_pop_scale(LAP_POP_TIME * i as f32 / 50.0);
            assert!((1.0..=1.0 + LAP_POP_GROWTH + 1e-5).contains(&s), "{s}");
        }
    }

    /// Issue #79: "Tour 1/3" → "Tour 2/3" → "Tour 3/3" pops; the same lap, a restart (back to
    /// "Tour 1/3") or another number of laps does not.
    #[test]
    fn the_lap_counter_pops_on_a_new_lap_only() {
        assert!(lap_pops((1, 3), (2, 3)));
        assert!(lap_pops((2, 3), (3, 3)));
        assert!(!lap_pops((3, 3), (3, 3)), "finished: still 3/3");
        assert!(!lap_pops((1, 3), (1, 3)));
        assert!(!lap_pops((3, 3), (1, 3)), "a restart");
        assert!(!lap_pops((2, 3), (1, 3)));
        assert!(!lap_pops((1, 3), (2, 5)), "another race");
    }

    #[test]
    fn the_counter_shakes_then_stops() {
        assert_eq!(shake_offset(0.0), 0.0);
        let biggest = (1..100)
            .map(|i| shake_offset(SHAKE_TIME * i as f32 / 100.0).abs())
            .fold(0.0f32, f32::max);
        assert!(biggest > SHAKE_PX * 0.5 && biggest <= SHAKE_PX, "{biggest}");
        assert_eq!(shake_offset(SHAKE_TIME), 0.0);
        assert_eq!(shake_offset(3.0), 0.0);
    }
}
