//! Item tuning (GD § 8, issue #16). Values marked "GD" come from `docs/GAME_DESIGN.md` ("à régler"
//! there); the others are our own starting points. Tweak them here (or in the admin panel), never
//! hard-code them in systems. The distribution of the items by distance to the leader is data, not
//! a tuning value: `assets/data/items.ron` (`table.rs`).

use bevy::prelude::*;

/// All the knobs of the items. Distances in meters, speeds in m/s, durations in seconds, angles
/// in radians.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct ItemTuning {
    /// Items in the races. Off: no item box, nothing in the slots, nothing on the track (a race
    /// without items, like the bots' driving tests). On by default in the game.
    pub enabled: bool,

    // --- Item boxes (GD § 8) ------------------------------------------------------------------
    /// A broken box comes back this long after (GD § 8: about 2 s).
    pub box_respawn_time: f32,
    /// A kart breaks a box when the box's center is within this distance of its collision box:
    /// the box's half size (0.5 m) and a little more, so that a kart never slips between two
    /// boxes of a row (2.8 m apart on Prairie Pompon) without taking one.
    pub box_reach: f32,
    /// The roulette spins this long before the item comes out (GD § 8: about 1.5 s).
    pub roulette_time: f32,

    // --- Hits (GD § 8) --------------------------------------------------------------------------
    /// A spin out (tête-à-queue: pelure, abeille) lasts this long (GD § 8: about 1 s). How hard
    /// the kart slows down meanwhile is `KartTuning::hit_spin_decel`.
    pub spin_time: f32,
    /// A tumble (culbute: pelote) lasts this long (GD § 8: about 1.6 s;
    /// `KartTuning::hit_tumble_decel`). Then, for both, the kart is invincible for 1 to 2 s with
    /// its Invincibility stat (GD § 9, `StatsTuning`).
    pub tumble_time: f32,
    /// A kart's own pelote or pelure cannot hit it this long after it left the kart (a pelote
    /// bouncing back later can). Its own abeille never does.
    pub own_item_delay: f32,

    // --- Items held out around the kart (GD § 8) ----------------------------------------------
    /// A pelure or pelote dragged behind the kart (Objet held), this far behind its center: a
    /// shield against what comes from behind.
    pub drag_distance: f32,
    /// The three pelures of a "Pelure x3", one behind the other from `drag_distance` on, this far
    /// apart.
    pub trail_spacing: f32,
    /// The three pelotes or abeilles of an "x3" turn around the kart at this distance from its
    /// center...
    pub orbit_radius: f32,
    /// ...this fast (rad/s).
    pub orbit_speed: f32,

    // --- Pelure (GD § 8: trap on the ground, spin out) ---------------------------------------
    /// Radius of a pelure (what touches it).
    pub pelure_radius: f32,
    /// Dropped this far behind the kart's center.
    pub pelure_drop_distance: f32,
    /// A pelure on the ground vanishes after this long if nobody ran over it.
    pub pelure_lifetime: f32,

    // --- Pelote de laine (GD § 8: rolls straight, bounces off the walls, tumble) -------------
    pub pelote_radius: f32,
    /// Rolling speed, ahead of a kart at top speed even in a boost (28.5 m/s, 37 with a piment).
    pub pelote_speed: f32,
    /// Bounces off the walls; the next wall after the last one breaks it.
    pub pelote_bounces: u32,
    /// It falls apart this long after the throw.
    pub pelote_lifetime: f32,

    // --- Abeille (GD § 8: homes in on the kart just ahead, spin out) -------------------------
    pub abeille_radius: f32,
    /// Speed along the track (following the centerline), faster than the karts...
    pub abeille_speed: f32,
    /// ...this high above the road (it flies over the other karts)...
    pub abeille_height: f32,
    /// ...moving sideways toward its target's lane at most this fast...
    pub abeille_lateral_speed: f32,
    /// ...then it dives straight at its target from this close (m, center to center)...
    pub abeille_dive_distance: f32,
    /// ...this fast.
    pub abeille_dive_speed: f32,
    /// It gives up this long after the throw (thrown backwards: it rolls straight, like a
    /// pelote that breaks on the first wall).
    pub abeille_lifetime: f32,

    // --- Piment (GD § 8: a boost of about 1.2 s) ----------------------------------------------
    /// Max speed increase (GD § 2: "+30 % pour un piment").
    pub piment_strength: f32,
    /// GD § 8: about 1.2 s.
    pub piment_duration: f32,

    // --- Croquette dorée (GD § 8: +2 croquettes) -----------------------------------------------
    /// Croquettes it gives (GD § 8: 2), up to the most a kart holds (`CroquetteTuning::max_held`).
    pub golden_croquettes: u8,
}

impl Default for ItemTuning {
    fn default() -> Self {
        Self {
            enabled: true,

            box_respawn_time: 2.0,
            box_reach: 0.8,
            roulette_time: 1.5,

            spin_time: 1.0,
            tumble_time: 1.6,
            own_item_delay: 0.5,

            drag_distance: 1.7,
            trail_spacing: 0.9,
            orbit_radius: 1.9,
            orbit_speed: 6.0,

            pelure_radius: 0.45,
            pelure_drop_distance: 2.2,
            pelure_lifetime: 90.0,

            pelote_radius: 0.4,
            pelote_speed: 42.0,
            pelote_bounces: 5,
            pelote_lifetime: 8.0,

            abeille_radius: 0.35,
            abeille_speed: 50.0,
            abeille_height: 1.4,
            abeille_lateral_speed: 8.0,
            abeille_dive_distance: 12.0,
            abeille_dive_speed: 45.0,
            abeille_lifetime: 12.0,

            piment_strength: 0.3,
            piment_duration: 1.2,

            golden_croquettes: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GD § 8: the roulette spins about 1.5 s, a box comes back after about 2 s, a spin out lasts
    /// about 1 s and a tumble about 1.6 s, a piment boosts about 1.2 s, a croquette dorée gives 2.
    #[test]
    fn defaults_follow_the_game_design() {
        let t = ItemTuning::default();
        assert!(t.enabled);
        assert_eq!(t.roulette_time, 1.5);
        assert_eq!(t.box_respawn_time, 2.0);
        assert_eq!(t.spin_time, 1.0);
        assert_eq!(t.tumble_time, 1.6);
        assert_eq!(t.piment_duration, 1.2);
        assert_eq!(t.piment_strength, 0.3);
        assert_eq!(t.golden_croquettes, 2);
        // Projectiles outrun a kart at top speed, even boosted by a piment (28.5 × 1.3 m/s).
        let boosted = 28.5 * (1.0 + t.piment_strength);
        assert!(t.pelote_speed > boosted && t.abeille_speed > boosted);
        // The abeille flies over the karts (their roof is 0.8 m above the road) until it dives.
        assert!(t.abeille_height - t.abeille_radius > 0.8);
        // A dragged item sits behind the kart's body (1 m from its center to its back).
        assert!(t.drag_distance - t.pelure_radius > 1.0);
        assert!(t.drag_distance - t.pelote_radius > 1.0);
    }
}
