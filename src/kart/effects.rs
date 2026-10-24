//! What the items of phase 2 do to a kart (GD § 8, issue #18), kept on the kart: the item rules
//! (`crate::items`) start them, the kart counts them down with its boosts, its driving and its
//! visuals read them. Pure data and maths: unit-tested.
//!
//! - **Super câlin**: invincible (the kart's `Invincibility`, which every immunity reads: items,
//!   paws), faster (`MaxSpeedFactors::items`), and the karts it runs into tumble.
//! - **Fusée à confettis**: the [`Autopilot`] drives instead of the driver: very fast along the
//!   racing line; invincible too, and the karts it runs into tumble.
//! - **Dégonflage**: smaller (a visual) and slower (`MaxSpeedFactors::items`).
//! - **Chantilly**: cream on the screen (the HUD); a bot drives less precisely.
//! - **Pie voleuse** (issue #21): invisible (a ghost: seen faintly by its own player only),
//!   invincible meanwhile (its `Invincibility`, without blinking); the items go through it.

use bevy::prelude::*;

/// The item effects on a kart: seconds left of each (0: none).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemEffects {
    /// Super câlin: invincible, faster; the karts it runs into tumble.
    pub calin: f32,
    /// Fusée à confettis: the autopilot drives; the karts it runs into tumble.
    pub rocket: f32,
    /// Dégonflage: smaller, slower...
    pub deflated: f32,
    /// ...and how long it lasts in all (its shrink and growth back, a visual).
    pub deflated_for: f32,
    /// Chantilly: cream on the screen.
    pub creamed: f32,
    /// ...and how long it lasts in all (it fades out at the end).
    pub creamed_for: f32,
    /// Pie voleuse (issue #21): invisible, the items go through it.
    pub ghost: f32,
}

impl ItemEffects {
    /// The karts it runs into tumble: super câlin, fusée.
    pub fn rams(&self) -> bool {
        self.calin > 0.0 || self.rocket > 0.0
    }

    pub fn is_active(&self) -> bool {
        self.calin > 0.0
            || self.rocket > 0.0
            || self.deflated > 0.0
            || self.creamed > 0.0
            || self.ghost > 0.0
    }

    /// Invisible (issue #21: a pie voleuse): nothing aims at it, the items go through it.
    pub fn hidden(&self) -> bool {
        self.ghost > 0.0
    }

    /// Counts every effect down by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        for left in [
            &mut self.calin,
            &mut self.rocket,
            &mut self.deflated,
            &mut self.creamed,
            &mut self.ghost,
        ] {
            *left = (*left - dt).max(0.0);
        }
    }

    /// The top speed factor of its effects (`MaxSpeedFactors::items`): faster in a super câlin
    /// (`calin_speed`: 0.15 = +15 %), slower deflated (`deflated_speed`: × 0.8).
    pub fn speed_factor(&self, calin_speed: f32, deflated_speed: f32) -> f32 {
        let mut factor = 1.0;
        if self.calin > 0.0 {
            factor *= 1.0 + calin_speed;
        }
        if self.deflated > 0.0 {
            factor *= deflated_speed;
        }
        factor
    }

    /// A deflated kart shrinks in this time (s)...
    pub const SHRINK_SECONDS: f32 = 0.25;
    /// ...and grows back in its last moments (s).
    pub const GROW_BACK_SECONDS: f32 = 0.4;

    /// How big the kart looks while deflated (1: its size): it shrinks to `scale` in 0.25 s and
    /// grows back in the last 0.4 s.
    pub fn deflated_scale(&self, scale: f32) -> f32 {
        if self.deflated <= 0.0 {
            return 1.0;
        }
        let since = (self.deflated_for - self.deflated).max(0.0);
        let shrink = (since / Self::SHRINK_SECONDS)
            .min(1.0)
            .min(self.deflated / Self::GROW_BACK_SECONDS);
        1.0 + (scale - 1.0) * shrink.clamp(0.0, 1.0)
    }

    /// How thick the cream is on the screen (0 to 1): full, fading out in its last second.
    pub fn cream(&self) -> f32 {
        if self.creamed <= 0.0 {
            0.0
        } else {
            self.creamed.min(1.0)
        }
    }
}

/// The fusée's autopilot (issue #18): while `ItemEffects::rocket` lasts, the driving follows it
/// instead of the driver: toward `target` (a point of the racing line ahead), at `speed`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Autopilot {
    pub target: Vec3,
    /// Its speed (m/s), reached at `accel` (m/s²).
    pub speed: f32,
    pub accel: f32,
    /// How fast it turns to the target (1/s), at most this fast (rad/s).
    pub turn_response: f32,
    pub max_turn_rate: f32,
    /// The racing line's sample found last tick (where to look next).
    pub sample: Option<usize>,
    /// Issue #171: the shortcut it follows (`RacingLine::branches`), from where the kart was on
    /// it to its end: never back across the void to the racing line.
    pub branch: Option<usize>,
    /// Issue #171: it follows a risky jump: over its void it flies toward `target`'s height (the
    /// path's, as everywhere in the air), and the fusée lasts until the kart is back on the
    /// ground: the kart never falls during a fusée.
    pub fly: bool,
}

/// The yaw rate (rad/s, around `up`, positive to the left as Bevy's rotations) that turns a kart
/// heading `ahead` toward `to` (a direction), at `response` per second, at most `most`.
pub fn autopilot_yaw_rate(ahead: Vec3, to: Vec3, up: Vec3, response: f32, most: f32) -> f32 {
    let flat = (to - up * to.dot(up)).normalize_or(ahead);
    let angle = ahead.cross(flat).dot(up).atan2(ahead.dot(flat));
    (angle * response).clamp(-most, most)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The effects count down to 0 and never below; the super câlin and the fusée ram.
    #[test]
    fn effects_count_down() {
        let mut effects = ItemEffects {
            calin: 0.5,
            rocket: 0.2,
            deflated: 1.0,
            deflated_for: 1.0,
            creamed: 0.1,
            creamed_for: 4.0,
            ghost: 0.25,
        };
        assert!(effects.rams() && effects.is_active() && effects.hidden());
        effects.tick(0.3);
        assert_eq!(effects.rocket, 0.0);
        assert!(!effects.hidden(), "the pie voleuse's 0.25 s are over");
        assert!((effects.calin - 0.2).abs() < 1e-6);
        assert!(effects.rams());
        effects.tick(1.0);
        assert!(!effects.rams() && !effects.is_active());
        assert_eq!(
            effects,
            ItemEffects {
                deflated_for: 1.0,
                creamed_for: 4.0,
                ..default()
            }
        );
    }

    /// Faster in a super câlin, slower deflated, both multiply; nothing: 1 exactly.
    #[test]
    fn the_speed_factor_of_the_effects() {
        assert_eq!(ItemEffects::default().speed_factor(0.15, 0.8), 1.0);
        let calin = ItemEffects {
            calin: 1.0,
            ..default()
        };
        assert!((calin.speed_factor(0.15, 0.8) - 1.15).abs() < 1e-6);
        let both = ItemEffects {
            deflated: 1.0,
            ..calin
        };
        assert!((both.speed_factor(0.15, 0.8) - 0.92).abs() < 1e-6);
    }

    /// Deflated, the kart shrinks to its scale, then grows back at the end; the cream fades out
    /// in its last second.
    #[test]
    fn a_deflated_kart_shrinks_then_grows_back() {
        let at = |left: f32| ItemEffects {
            deflated: left,
            deflated_for: 5.0,
            ..default()
        };
        assert_eq!(at(5.0).deflated_scale(0.6), 1.0);
        assert!((at(4.0).deflated_scale(0.6) - 0.6).abs() < 1e-6);
        assert!((at(0.2).deflated_scale(0.6) - 0.8).abs() < 1e-5);
        assert_eq!(at(0.0).deflated_scale(0.6), 1.0);
        let cream = |left| ItemEffects {
            creamed: left,
            creamed_for: 4.0,
            ..default()
        };
        assert_eq!(cream(3.0).cream(), 1.0);
        assert_eq!(cream(0.5).cream(), 0.5);
        assert_eq!(cream(0.0).cream(), 0.0);
    }

    /// The autopilot turns toward its target, left or right, and at most so fast.
    #[test]
    fn the_autopilot_turns_toward_its_target() {
        let ahead = Vec3::NEG_Z;
        let left = Vec3::new(-1.0, 0.0, -1.0);
        let right = Vec3::new(1.0, 0.0, -1.0);
        let yaw = |to| autopilot_yaw_rate(ahead, to, Vec3::Y, 4.0, 10.0);
        assert!((yaw(left) - std::f32::consts::FRAC_PI_4 * 4.0).abs() < 1e-5);
        assert!((yaw(right) + std::f32::consts::FRAC_PI_4 * 4.0).abs() < 1e-5);
        assert_eq!(yaw(ahead), 0.0);
        assert_eq!(
            autopilot_yaw_rate(ahead, Vec3::Z, Vec3::Y, 4.0, 3.0).abs(),
            3.0
        );
        // A positive yaw rate turns the heading to the left (Bevy's rotations).
        let turned = Quat::from_axis_angle(Vec3::Y, 0.1) * ahead;
        assert!(turned.x < 0.0);
    }
}
