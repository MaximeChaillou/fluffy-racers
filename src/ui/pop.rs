//! HUD animations (pure): the croquette counter "pops" when a croquette is picked up and shakes
//! when croquettes are lost (GD § 14: "un « pop » et un son à chaque croquette").

/// The pop lasts this long (s)...
pub const POP_TIME: f32 = 0.25;
/// ...and the counter grows by this much at its peak.
pub const POP_GROWTH: f32 = 0.45;
/// The shake lasts this long (s), this wide (px) and this fast (Hz).
pub const SHAKE_TIME: f32 = 0.35;
pub const SHAKE_PX: f32 = 7.0;
pub const SHAKE_HZ: f32 = 22.0;

/// Scale of the counter `t` seconds after a pick-up: 1, up to 1 + `POP_GROWTH` and back to
/// exactly 1 (a sine bump).
pub fn pop_scale(t: f32) -> f32 {
    if !(0.0..POP_TIME).contains(&t) {
        return 1.0;
    }
    1.0 + POP_GROWTH * (std::f32::consts::PI * t / POP_TIME).sin()
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
