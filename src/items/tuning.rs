//! Item tuning (GD § 8, issues #16, #18 and #21). Values marked "GD" come from `docs/GAME_DESIGN.md` ("à régler"
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
    /// Issue #102: on screen, the roulette scrolls the items through the slot like a slot
    /// machine, about this many a second (items/s; the HUD's reel, `ui::roulette`: it adjusts it a
    /// little so that the reel stops right on the item drawn)...
    pub roulette_scroll_speed: f32,
    /// ...then slows down to a stop on the item drawn over the end of the roulette, this long
    /// (s, at most `roulette_time`). Only the HUD: the item is ready at `roulette_time` all the
    /// same.
    pub roulette_slowdown: f32,
    /// Issue #155 (« Objets » of the admin panel's « Ressenti », à régler): a kart's distance to
    /// the leader is multiplied by this before its row of the table (`assets/data/items.ron`) is
    /// read. Above 1, the strong items come closer behind the leader (more often); below, further
    /// behind (rarer). 1: the table as it is.
    pub gap_scale: f32,

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

    // --- Piment doré (GD § 8, issue #18: boosts at will for about 7 s) -----------------------
    /// Its window opens at the first press (GD § 8: about 7 s); each press meanwhile is a
    /// piment's boost (`piment_strength`, `piment_duration`). It stays in the hand meanwhile.
    pub golden_piment_time: f32,

    // --- Bombe à confettis (GD § 8, issue #18: thrown ahead or behind, explodes: tumble) ----
    /// What touches it (a kart, an item: it explodes).
    pub bombe_radius: f32,
    /// Thrown ahead: this much faster than its thrower (m/s, along its heading)...
    pub bombe_throw_speed: f32,
    /// ...and this fast upward (m/s): a lob that lands, and explodes, about 13 m ahead of its
    /// thrower (out of its own blast), whatever its speed.
    pub bombe_throw_up: f32,
    /// Dropped behind: it rolls back this fast (m/s) and stops.
    pub bombe_drop_speed: f32,
    /// On the ground, it slows down this hard (m/s²).
    pub bombe_ground_decel: f32,
    /// Dropped behind, it explodes this long after, unless something touched it before.
    pub bombe_fuse: f32,
    /// Every kart within this distance of the explosion (m, center to center) tumbles.
    pub bombe_blast_radius: f32,
    /// How long the explosion shows (s, a visual).
    pub blast_show_time: f32,

    // --- Chantilly (GD § 8, issue #18: cream on the screen of the racers ahead) -------------
    /// The cream stays this long on a screen (s). The bots' driving meanwhile:
    /// `BotTuning::creamed_wobble`.
    pub chantilly_time: f32,

    // --- Super câlin (GD § 8, issue #18: about 7 s invincible, faster, rams) ----------------
    pub super_calin_time: f32,
    /// Top speed increase meanwhile (0.15 = +15 %), on top of the boosts.
    pub super_calin_speed: f32,

    // --- Fusée à confettis (GD § 8, issue #18: a fast autopilot along the racing line) ------
    pub fusee_time: f32,
    /// Its speed (m/s): a pelote's, half again a kart at top speed.
    pub fusee_speed: f32,
    /// It gets there this hard (m/s²).
    pub fusee_accel: f32,
    /// It aims at the racing line this far ahead (m).
    pub fusee_lookahead: f32,
    /// How fast it turns to its heading (1/s)...
    pub fusee_turn_response: f32,
    /// ...at most this fast (rad/s): tight enough for the hairpin at its speed.
    pub fusee_max_turn_rate: f32,

    // --- Dégonflage (GD § 8, issue #18: the opponents shrink, slow down, drop their items) --
    /// Their size meanwhile (a visual).
    pub degonflage_scale: f32,
    /// Their top speed meanwhile (× this).
    pub degonflage_speed: f32,
    /// How long: the leader this long...
    pub degonflage_time_first: f32,
    /// ...the last this long, in between with their rank (GD § 8: the first ones stay small
    /// longer).
    pub degonflage_time_last: f32,

    // --- Oreiller géant (GD § 8, issue #18: flies to the leader, crushes it) ----------------
    pub oreiller_radius: f32,
    /// Speed along the track (following the centerline), far above the karts...
    pub oreiller_speed: f32,
    /// ...moving sideways toward its target's lane at most this fast (m/s)...
    pub oreiller_lateral_speed: f32,
    /// ...this high above the road...
    pub oreiller_height: f32,
    /// ...then over the leader once this close (m, along the track) it hovers this long (a
    /// klaxon-canard can still destroy it)...
    pub oreiller_hover_time: f32,
    pub oreiller_reach: f32,
    /// ...and drops on it this fast (m/s).
    pub oreiller_drop_speed: f32,
    /// Every kart within this distance of where it lands (m) tumbles: a small zone.
    pub oreiller_blast_radius: f32,
    /// It gives up this long after the throw.
    pub oreiller_lifetime: f32,
    /// GD § 8: none before this long after GO (s)...
    pub oreiller_min_race_time: f32,
    /// ...and this long between two (s, from the draw of the last one). One at a time.
    pub oreiller_cooldown: f32,

    // --- Klaxon-canard (GD § 8, issue #18: a shock wave that destroys the items close by) ---
    /// The items within this distance of the kart's center are destroyed (m).
    pub klaxon_radius: f32,

    // --- Pie voleuse (GD § 8, issue #21: invisible about 3 s, steals an item) ---------------
    /// Invisible this long (s, GD § 8: about 3 s): items go through it, nothing aims at it, no
    /// paw reaches it (it is invincible meanwhile, without blinking).
    pub pie_time: f32,
    /// Who it steals from (issue #88, Max: « un kart au hasard, avec une probabilité d'autant
    /// plus forte que ce kart est devant »): a racer that holds an item to take, drawn with a
    /// weight of `(N - rank + 1)^pie_rank_power` (N racers, the leader's rank 1): 2 by default,
    /// the leader the most likely (with 12 karts, 144 against 121 for the second and 1 for the
    /// last); 1 is a gentler slope (12, 11... 1), 0 every racer alike.
    pub pie_rank_power: f32,

    // --- Piñata (GD § 8, issue #21: 8 items around the kart, used one at a time) -----------
    /// Its eight items turn around the kart at this distance from its center (m), wider than an
    /// "x3"...
    pub pinata_orbit_radius: f32,
    /// ...this fast (rad/s): slow enough to pick the one in front.
    pub pinata_orbit_speed: f32,

    // --- Boomerang-os (GD § 8, issue #21: comes back, three throws) ------------------------
    /// What touches it.
    pub boomerang_radius: f32,
    /// Its speed, out and back (m/s): a pelote's, faster than a kart at top speed in a boost.
    pub boomerang_speed: f32,
    /// It flies out this far (m), curving...
    pub boomerang_range: f32,
    /// ...as its thrower steered when it threw it (issue #88, Max: « il tourne avec le kart »):
    /// to the side it steered, this fast at full lock (rad/s), less with less steering (a
    /// radius of 28 m at 42 m/s, about 60° over its 30 m)...
    pub boomerang_steer_curve: f32,
    /// ...or, thrown straight (the steering under this, out of 1), this fast to its left (rad/s;
    /// negative: to its right, 0: straight on; issue #88: our choice, the curve of #21)...
    pub boomerang_straight_steer: f32,
    pub boomerang_curve: f32,
    /// ...then it turns back to its thrower at most this fast (rad/s), straight at it once this
    /// close (m)...
    pub boomerang_turn_rate: f32,
    pub boomerang_home_distance: f32,
    /// ...and its thrower catches it this close (m, center to center): the next throw is ready.
    pub boomerang_catch: f32,
    /// Walls it bounces off on its way back (the first wall on its way out sends it back); the
    /// next one breaks it.
    pub boomerang_bounces: u32,
    /// It falls apart this long after the throw if it never came back (s).
    pub boomerang_lifetime: f32,
}

impl Default for ItemTuning {
    fn default() -> Self {
        Self {
            enabled: true,

            box_respawn_time: 2.0,
            box_reach: 0.8,
            roulette_time: 1.5,
            roulette_scroll_speed: 12.0,
            roulette_slowdown: 0.7,
            gap_scale: 1.0,

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

            golden_piment_time: 7.0,

            bombe_radius: 0.55,
            bombe_throw_speed: 20.0,
            bombe_throw_up: 7.0,
            bombe_drop_speed: 3.0,
            bombe_ground_decel: 30.0,
            bombe_fuse: 3.0,
            bombe_blast_radius: 6.0,
            blast_show_time: 0.6,

            chantilly_time: 4.0,

            super_calin_time: 7.0,
            super_calin_speed: 0.15,

            fusee_time: 6.0,
            fusee_speed: 42.0,
            fusee_accel: 40.0,
            fusee_lookahead: 7.0,
            fusee_turn_response: 12.0,
            fusee_max_turn_rate: 6.0,

            degonflage_scale: 0.6,
            degonflage_speed: 0.8,
            degonflage_time_first: 7.0,
            degonflage_time_last: 3.0,

            oreiller_radius: 1.0,
            oreiller_speed: 60.0,
            oreiller_lateral_speed: 15.0,
            oreiller_height: 4.0,
            oreiller_hover_time: 0.8,
            oreiller_reach: 8.0,
            oreiller_drop_speed: 20.0,
            oreiller_blast_radius: 3.5,
            oreiller_lifetime: 30.0,
            oreiller_min_race_time: 20.0,
            oreiller_cooldown: 30.0,

            klaxon_radius: 9.0,

            pie_time: 3.0,
            pie_rank_power: 2.0,

            pinata_orbit_radius: 2.4,
            pinata_orbit_speed: 2.5,

            boomerang_radius: 0.45,
            boomerang_speed: 42.0,
            boomerang_range: 30.0,
            boomerang_steer_curve: 1.5,
            boomerang_straight_steer: 0.15,
            boomerang_curve: 0.6,
            boomerang_turn_rate: 6.0,
            boomerang_home_distance: 8.0,
            boomerang_catch: 2.0,
            boomerang_bounces: 2,
            boomerang_lifetime: 5.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GD § 8, issue #21: the pie voleuse about 3 s; the boomerang-os outruns a kart in a boost
    /// (it comes back to a thrower driving away) and comes back from further than its catch; the
    /// piñata turns wider than an "x3", slower.
    #[test]
    fn phase_three_defaults_follow_the_game_design() {
        let t = ItemTuning::default();
        assert_eq!(t.pie_time, 3.0);
        assert!(t.boomerang_speed > 28.5 * (1.0 + t.piment_strength));
        assert!(t.boomerang_range > 4.0 * t.boomerang_catch);
        assert!(t.boomerang_home_distance > t.boomerang_catch);
        assert!(t.pinata_orbit_radius > t.orbit_radius);
        assert!(t.pinata_orbit_speed < t.orbit_speed);
        assert!(t.boomerang_lifetime * t.boomerang_speed > 2.0 * t.boomerang_range);
    }

    /// Issue #88 (Max): the pie voleuse favours the karts ahead (a weight that grows with the
    /// place); the boomerang-os curves with the steering: at full lock more than thrown straight,
    /// and a light touch of the stick is not "straight" for long; thrown straight, it curves to
    /// its left as in #21, and never so tight that it turns back on itself before its range.
    #[test]
    fn issue_88_defaults() {
        let t = ItemTuning::default();
        assert!(t.pie_rank_power > 0.0);
        assert!(t.boomerang_steer_curve > t.boomerang_curve && t.boomerang_curve > 0.0);
        assert!((0.0..0.5).contains(&t.boomerang_straight_steer));
        let turned = t.boomerang_steer_curve * t.boomerang_range / t.boomerang_speed;
        assert!(turned < std::f32::consts::FRAC_PI_2, "{turned}");
    }

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

    /// Issue #102: the reel of the roulette on screen scrolls fast (a dozen items a second), then
    /// slows down over about half of the roulette (it is seen choosing), shorter than the
    /// roulette itself (the speed is first seen).
    #[test]
    fn issue_102_defaults() {
        let t = ItemTuning::default();
        assert_eq!(t.roulette_time, 1.5, "the bots' races count on it");
        assert!((10.0..=16.0).contains(&t.roulette_scroll_speed));
        assert!(t.roulette_slowdown > 0.3 * t.roulette_time);
        assert!(t.roulette_slowdown < 0.6 * t.roulette_time);
    }

    /// GD § 8, issue #18: the piment doré about 7 s, the super câlin about 7 s, the fusée about
    /// 6 s; the oreiller géant none before 20 s of race and 30 s between two; the fusée outruns
    /// the karts, the oreiller flies over them; the first ones stay small longer.
    #[test]
    fn phase_two_defaults_follow_the_game_design() {
        let t = ItemTuning::default();
        assert_eq!(t.golden_piment_time, 7.0);
        assert_eq!(t.super_calin_time, 7.0);
        assert_eq!(t.fusee_time, 6.0);
        assert_eq!(t.oreiller_min_race_time, 20.0);
        assert_eq!(t.oreiller_cooldown, 30.0);
        assert!(t.fusee_speed > 28.5 * (1.0 + t.piment_strength));
        assert!(t.oreiller_speed > t.abeille_speed);
        assert!(t.oreiller_height - t.oreiller_radius > 2.0);
        assert!(t.degonflage_time_first > t.degonflage_time_last);
        assert!(t.degonflage_scale < 1.0 && t.degonflage_speed < 1.0);
        // The oreiller's zone is small, the bombe's bigger.
        assert!(t.oreiller_blast_radius < t.bombe_blast_radius);
        // A thrown bombe lands (and explodes) ahead of its thrower, out of its own blast.
        let flight = 2.0 * t.bombe_throw_up / crate::items::objects::ITEM_GRAVITY;
        assert!(
            t.bombe_throw_speed * flight > t.bombe_blast_radius + 4.0,
            "{flight}"
        );
    }
}
