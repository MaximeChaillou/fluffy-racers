//! Speed classes (GD § 1, issue #19): 50cc, 100cc, 150cc (the default) and 200cc.
//!
//! - **Top speed**: the class multiplies every kart's top speed ([`SpeedClassTuning`]: 0.75 /
//!   0.87 / 1.0 / 1.25, « à régler »), through its `MaxSpeedFactors::class`, like the croquettes
//!   and the stats: the bots plan their speeds with it (`MaxSpeedScale`), the slipstream's « fast
//!   enough » follows. The acceleration follows the top speed (the engine reaches it in the same
//!   time), the steering does not: the turning circles grow with the speed, so the fast classes
//!   ask to lift or brake more before the turns, and the slow ones forgive. At 150cc the factor is
//!   exactly 1: the game is the one before the classes, to the bit.
//! - **Braking in a drift** (GD § 3 point 7): in 200cc, the brake in a drift slows the kart
//!   gently ([`SpeedClassTuning::drift_brake_decel`]), never under
//!   [`SpeedClassTuning::drift_brake_min_speed`] (above the speed that breaks a drift), and turns
//!   the drift's curve tighter ([`SpeedClassTuning::drift_brake_tighten`]): the drift goes on,
//!   tighter. In the other classes the brake breaks the drift as before (too slow).
//!
//! The class of the race is [`RaceClass`] (solo: the race menu, `crate::modes`; online, always
//! 150cc: `crate::net` puts it back in a room). It applies at once, to every kart.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A speed class (GD § 1). Issue #76: by its name in the time trial's records (`Cc150`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpeedClass {
    Cc50,
    Cc100,
    /// The default: the game's tuning as it is.
    #[default]
    Cc150,
    Cc200,
}

impl SpeedClass {
    pub const ALL: [Self; 4] = [
        SpeedClass::Cc50,
        SpeedClass::Cc100,
        SpeedClass::Cc150,
        SpeedClass::Cc200,
    ];

    /// As the menus write it.
    pub fn label(self) -> &'static str {
        match self {
            SpeedClass::Cc50 => "50cc",
            SpeedClass::Cc100 => "100cc",
            SpeedClass::Cc150 => "150cc",
            SpeedClass::Cc200 => "200cc",
        }
    }

    /// The previous (`step` < 0) or next class, around.
    pub fn cycled(self, step: i32) -> Self {
        let at = Self::ALL.iter().position(|c| *c == self).unwrap_or(2) as i32;
        Self::ALL[(at + step).rem_euclid(Self::ALL.len() as i32) as usize]
    }
}

/// The class the karts race in (issue #19): read every tick (a change applies at once).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Deref)]
pub struct RaceClass(pub SpeedClass);

/// The speed classes' knobs (GD § 1 and § 3 point 7, « à régler »). Reflected for the admin panel
/// (issue #36).
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct SpeedClassTuning {
    /// Top speed multiplier of each class (GD § 1: 0.75 / 0.87 / 1.0 / 1.25).
    pub speed_50cc: f32,
    pub speed_100cc: f32,
    pub speed_150cc: f32,
    pub speed_200cc: f32,
    /// Whether braking in a drift tightens it instead of breaking it (GD § 3 point 7: 200cc).
    pub brake_drift_50cc: bool,
    pub brake_drift_100cc: bool,
    pub brake_drift_150cc: bool,
    pub brake_drift_200cc: bool,
    /// Braking in a drift (where it is allowed): the kart slows down at this rate (m/s², at full
    /// brake; the plain brake is `KartTuning::brake_decel`, 28 m/s²)...
    pub drift_brake_decel: f32,
    /// ...never under this speed (m/s; the drift breaks under `KartTuning::drift_exit_speed`,
    /// 7 m/s)...
    pub drift_brake_min_speed: f32,
    /// ...and the drift's curve turns this much faster (1 = only the lower speed tightens it).
    pub drift_brake_tighten: f32,
}

impl Default for SpeedClassTuning {
    fn default() -> Self {
        Self {
            speed_50cc: 0.75,
            speed_100cc: 0.87,
            speed_150cc: 1.0,
            speed_200cc: 1.25,
            brake_drift_50cc: false,
            brake_drift_100cc: false,
            brake_drift_150cc: false,
            brake_drift_200cc: true,
            drift_brake_decel: 12.0,
            drift_brake_min_speed: 12.0,
            drift_brake_tighten: 1.15,
        }
    }
}

impl SpeedClassTuning {
    /// The top speed multiplier of `class`.
    pub fn speed(&self, class: SpeedClass) -> f32 {
        match class {
            SpeedClass::Cc50 => self.speed_50cc,
            SpeedClass::Cc100 => self.speed_100cc,
            SpeedClass::Cc150 => self.speed_150cc,
            SpeedClass::Cc200 => self.speed_200cc,
        }
    }

    /// Whether braking in a drift tightens it in `class`.
    pub fn brake_drift(&self, class: SpeedClass) -> bool {
        match class {
            SpeedClass::Cc50 => self.brake_drift_50cc,
            SpeedClass::Cc100 => self.brake_drift_100cc,
            SpeedClass::Cc150 => self.brake_drift_150cc,
            SpeedClass::Cc200 => self.brake_drift_200cc,
        }
    }
}

/// Forward speed after one tick of `dt` seconds of a kart braking at `brake` (0 to 1) in a drift,
/// where its class allows it: down at `decel` m/s², never under `floor` (a kart already slower
/// keeps its speed: the brake neither pushes nor breaks the drift).
pub fn drift_brake_speed(speed: f32, brake: f32, decel: f32, floor: f32, dt: f32) -> f32 {
    if speed <= floor {
        return speed;
    }
    (speed - decel.max(0.0) * brake.clamp(0.0, 1.0) * dt).max(floor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GD § 1: 50cc, 100cc, 150cc (the default, exactly 1) and 200cc, slower to faster; only
    /// 200cc brakes in a drift (GD § 3 point 7).
    #[test]
    fn the_classes_follow_the_game_design() {
        let t = SpeedClassTuning::default();
        assert_eq!(SpeedClass::default(), SpeedClass::Cc150);
        assert_eq!(t.speed(SpeedClass::Cc150), 1.0);
        let speeds: Vec<f32> = SpeedClass::ALL.iter().map(|c| t.speed(*c)).collect();
        assert_eq!(speeds, [0.75, 0.87, 1.0, 1.25]);
        let braking: Vec<bool> = SpeedClass::ALL.iter().map(|c| t.brake_drift(*c)).collect();
        assert_eq!(braking, [false, false, false, true]);
        let labels: Vec<&str> = SpeedClass::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(labels, ["50cc", "100cc", "150cc", "200cc"]);
        assert_eq!(SpeedClass::Cc200.cycled(1), SpeedClass::Cc50);
        assert_eq!(SpeedClass::Cc50.cycled(-1), SpeedClass::Cc200);
        assert_eq!(SpeedClass::Cc150.cycled(1), SpeedClass::Cc200);
        // The floor keeps a braked drift above the speed that breaks it.
        assert!(t.drift_brake_min_speed > crate::kart::KartTuning::default().drift_exit_speed);
    }

    #[test]
    fn the_drift_brake_slows_down_to_its_floor_and_never_pushes() {
        let dt = 1.0 / 60.0;
        assert!((drift_brake_speed(30.0, 1.0, 12.0, 12.0, dt) - 29.8).abs() < 1e-4);
        assert!((drift_brake_speed(30.0, 0.5, 12.0, 12.0, dt) - 29.9).abs() < 1e-4);
        assert_eq!(drift_brake_speed(12.1, 1.0, 12.0, 12.0, dt), 12.0);
        assert_eq!(drift_brake_speed(10.0, 1.0, 12.0, 12.0, dt), 10.0);
        assert_eq!(drift_brake_speed(30.0, 0.0, 12.0, 12.0, dt), 30.0);
    }
}
