//! Croquette tuning (GD § 7, issue #14). Values marked "GD" come from `docs/GAME_DESIGN.md`
//! ("à régler" there); the others are our own starting points. Tweak them here, never hard-code
//! them in systems.

use bevy::prelude::*;

/// All the knobs of the croquettes. Distances in meters, durations in seconds, angles in
/// radians.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct CroquetteTuning {
    // --- Held croquettes ------------------------------------------------------------------------
    /// Most croquettes a kart can hold (GD § 7: 10, like the coins of MK8DX). Picking one up at
    /// the maximum still takes it off the track and still gives the mini-boost.
    pub max_held: u8,
    /// Top speed bonus per croquette held (GD § 7: +1 %): 10 croquettes = +10 %.
    pub speed_per_croquette: f32,
    /// Picking a croquette up gives a mini-boost this long (GD § 7: 0.3 s)...
    pub pickup_boost_duration: f32,
    /// ...this strong (max speed increase; ours: +10 %, half a blue mini-turbo).
    pub pickup_boost_strength: f32,
    /// A kart picks a croquette up when the croquette's center is within this distance of its
    /// collision box (about the croquette's own size).
    pub pickup_reach: f32,

    // --- Croquettes of the track ----------------------------------------------------------------
    /// A croquette of the track comes back this long after being picked up (GD § 7: about
    /// 10 s).
    pub respawn_time: f32,

    // --- Losing croquettes --------------------------------------------------------------------
    /// Croquettes lost when hit by an item (GD § 7: 3).
    pub item_hit_loss: u8,
    /// Croquettes lost when bumped by another kart (GD § 7: 1). What a bump is:
    /// `KartTuning::bump_min_speed`. Walls cost nothing (GD § 2: you bounce and slow down).
    pub bump_loss: u8,
    /// After a bump, the kart cannot lose croquettes to bumps for this long: one messy
    /// collision (bounces, karts rubbing) costs one croquette.
    pub bump_grace: f32,

    // --- Scattered croquettes (lost ones) ---------------------------------------------------------
    /// Lost croquettes fly out this far from the kart (horizontally)...
    pub scatter_distance: f32,
    /// ...spread over this angle behind it (one croquette: straight behind)...
    pub scatter_angle: f32,
    /// ...arcing this high above their start...
    pub scatter_height: f32,
    /// ...and land after this long.
    pub scatter_flight_time: f32,
    /// Height of a landed croquette's center above the ground.
    pub loose_rest_height: f32,
    /// Any other kart can pick a scattered croquette up this long after it flew out (it can be
    /// caught at the end of its flight)...
    pub loose_pickup_delay: f32,
    /// ...the kart that lost it, this long after (it does not take it right back)...
    pub owner_pickup_delay: f32,
    /// ...and it vanishes this long after flying out if nobody took it (it blinks before).
    pub loose_lifetime: f32,
}

impl Default for CroquetteTuning {
    fn default() -> Self {
        Self {
            max_held: 10,
            speed_per_croquette: 0.01,
            pickup_boost_duration: 0.3,
            pickup_boost_strength: 0.1,
            pickup_reach: 0.45,

            respawn_time: 10.0,

            item_hit_loss: 3,
            bump_loss: 1,
            bump_grace: 1.0,

            scatter_distance: 2.5,
            scatter_angle: 100f32.to_radians(),
            scatter_height: 1.0,
            scatter_flight_time: 0.4,
            loose_rest_height: 0.35,
            loose_pickup_delay: 0.25,
            owner_pickup_delay: 1.0,
            loose_lifetime: 8.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GD § 7: 10 croquettes at most, +1 % each, a 0.3 s mini-boost, 3 lost to an item, 1 to a
    /// bump, back after about 10 s.
    #[test]
    fn defaults_follow_the_game_design() {
        let t = CroquetteTuning::default();
        assert_eq!(t.max_held, 10);
        assert_eq!(t.speed_per_croquette, 0.01);
        assert_eq!(t.pickup_boost_duration, 0.3);
        assert!(t.pickup_boost_strength > 0.0 && t.pickup_boost_strength <= 0.2);
        assert_eq!(t.item_hit_loss, 3);
        assert_eq!(t.bump_loss, 1);
        assert_eq!(t.respawn_time, 10.0);
        // A scattered croquette can be caught before it vanishes, by others first.
        assert!(t.loose_pickup_delay < t.owner_pickup_delay);
        assert!(t.owner_pickup_delay < t.loose_lifetime);
    }
}
