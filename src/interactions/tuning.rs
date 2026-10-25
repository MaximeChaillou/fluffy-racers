//! Interaction tuning (GD § 15, issue #34). The values marked "GD" come from
//! `docs/GAME_DESIGN.md` § 15 ("à régler" there); the others are our own starting points. Tweak
//! them here or in the admin panel, never hard-code them in systems.

use bevy::prelude::*;

/// All the knobs of the paw strike and the grab. Distances in meters, in the frame of the kart
/// that acts (to the center of the other kart); durations in seconds; speeds in m/s.
///
/// Reflected (`Reflect`) so that the admin panel (issue #36) edits every field.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct InteractionTuning {
    /// The interactions are in the races. Off: the Interaction button does nothing, no link,
    /// no push; the bots' driving tests race without them (issue #34).
    pub enabled: bool,

    // --- Button (GD § 15) -----------------------------------------------------------------------
    /// A press released before this is a paw strike, given on the release (GD: 0.2 s); held
    /// longer, it grabs.
    pub tap_time: f32,
    /// The button does nothing for this long after a paw strike (landed or into thin air) or a
    /// grab that caught nobody (GD: 1.5 s)...
    pub strike_cooldown: f32,
    /// ...and for this long after a tether ends (GD: 3 s).
    pub tether_cooldown: f32,

    // --- Reach (GD § 15, generous: it absorbs the latency online) -------------------------------
    /// To the side (strike and grab): the target's center is between these two distances from
    /// the kart's axis (GD: 0.7 to 3.9 m, at most 2.5 m between the flanks)...
    pub side_min_lateral: f32,
    pub side_max_lateral: f32,
    /// ...at most this far behind or ahead of the kart's center (GD: 2.5 m)...
    pub side_max_longitudinal: f32,
    /// ...and at most this much higher or lower (GD: 1.5 m, also for the grab ahead).
    pub max_height_difference: f32,
    /// Just ahead (grab only): at most this much between the front of the kart and the back of
    /// the target (GD: 2.5 m)...
    pub front_max_gap: f32,
    /// ...its center at most this far to the side (GD: 1.4 m).
    pub front_max_lateral: f32,
    /// With karts on both sides, the side steered toward by at least this much wins (GD: 0.3);
    /// it is also the side of a strike into thin air (to the right in neutral).
    pub side_steer: f32,

    // --- Paw strike (GD § 15) -------------------------------------------------------------------
    /// The target is pushed sideways at this speed, in the direction of the strike (GD: 7 m/s),
    /// times the Weight factor (`kart::strike_push_factor`)...
    pub strike_push_speed: f32,
    /// ...fading out in this long (GD: 0.4 s; about 1.4 m in all).
    pub strike_push_time: f32,
    /// The target loses this share of its speed, whatever the Weights (GD: 35 % since #193, Max:
    /// « là les karts continuent de rouler à la même vitesse »; 10 % before). A throw too.
    pub strike_speed_loss: f32,
    /// Issue #193: the target swerves under the blow, seen only (its body yaws toward the push,
    /// then back, `kart::visual`): this much at most (rad)...
    pub strike_swerve: f32,
    /// ...over this long (s): it keeps its heading and its control (GD § 15, #154).
    pub strike_swerve_time: f32,
    /// Then it is protected from the paw strikes (not from the items) for this long (GD: 1 s).
    pub strike_protection: f32,
    /// A strike or a grab into thin air: the kart slides this far toward the side of the swing
    /// (GD: about 0.3 m), without losing speed...
    pub whiff_slide: f32,
    /// ...and its driver flails, off balance, for this long (visual).
    pub off_balance_time: f32,

    // --- Grab and tether (GD § 15) --------------------------------------------------------------
    /// Held past `tap_time`, the paw reaches out and grabs the first kart in reach for this long
    /// (GD: 0.5 s); nobody: a grab into thin air.
    pub grab_window: f32,
    /// A tether lasts this long at most (GD: 2 s)...
    pub tether_max_time: f32,
    /// ...and breaks when stretched this much past its rest length (the distance when it
    /// grabbed; GD: 2.5 m).
    pub tether_max_stretch: f32,
    /// The tether is a damped spring: acceleration per meter stretched past its rest length
    /// (m/s² per m)...
    pub tether_stiffness: f32,
    /// ...plus this much per m/s of the karts moving apart (1/s): the grabber catches up with
    /// its partner's speed in about 0.5 s (GD). It only pulls, it never pushes.
    pub tether_damping: f32,
    /// The kart held gets this share of the tether's force (GD: 25 %), only what holds it back or
    /// pulls it sideways, never forward. The grabber gets all of it, towed up to its partner's
    /// speed at most. Each kart's share is divided by its Weight factor
    /// (`kart::tether_force_scale`).
    pub tether_target_share: f32,
    /// The kart held is braked: its top speed is lowered by this share while the tether lasts,
    /// times its Weight factor (`kart::tether_force_scale`: a light kart held by a heavy one is
    /// braked more). GD § 15's tuning goal: at equal Weights, a full grab of 2 s costs a kart
    /// at full speed about 10 % of its speed. A quarter of the pull alone cannot: the engine
    /// (strong near the top speed) and the boosts (40 m/s²) make up for a few m/s² at once.
    pub tether_target_brake: f32,
    /// Letting go after holding at least this long (GD: 0.5 s)...
    pub throw_min_hold: f32,
    /// ...while steering at least this much (GD: 0.5) throws the target to that side...
    pub throw_steer: f32,
    /// ...a paw strike with a push this much stronger (GD: × 1.5), the same speed loss,
    /// croquette and protection.
    pub throw_push_factor: f32,
    /// Issue #193 (« même idée, plus douce, pour l'attrape quand on se fait lâcher »): let go
    /// without a throw (the button released, or the tether's time up), the kart held loses this
    /// share of its speed (GD: 10 %), whatever the Weights.
    pub release_speed_loss: f32,

    // --- Network (docs/ARCHITECTURE.md, for issues #8 and #9) -----------------------------------
    /// The kart struck or grabbed accepts the event when the other kart is in reach, plus this
    /// much (m)...
    pub reach_tolerance: f32,
    /// ...and when the event is at most this old (s).
    pub max_event_age: f32,

    // --- Feel (GD § 14) -------------------------------------------------------------------------
    /// The kart struck: its camera shakes this much (m)...
    pub struck_shake: f32,
    /// ...for this long (s).
    pub struck_shake_time: f32,
}

impl Default for InteractionTuning {
    fn default() -> Self {
        Self {
            enabled: true,

            tap_time: 0.2,
            strike_cooldown: 1.5,
            tether_cooldown: 3.0,

            side_min_lateral: 0.7,
            side_max_lateral: 3.9,
            side_max_longitudinal: 2.5,
            max_height_difference: 1.5,
            front_max_gap: 2.5,
            front_max_lateral: 1.4,
            side_steer: 0.3,

            strike_push_speed: 7.0,
            strike_push_time: 0.4,
            strike_speed_loss: 0.35,
            strike_swerve: 0.35,
            strike_swerve_time: 0.6,
            strike_protection: 1.0,
            whiff_slide: 0.3,
            off_balance_time: 0.6,

            grab_window: 0.5,
            tether_max_time: 2.0,
            tether_max_stretch: 2.5,
            tether_stiffness: 30.0,
            tether_damping: 10.0,
            tether_target_share: 0.25,
            tether_target_brake: 0.1,
            throw_min_hold: 0.5,
            throw_steer: 0.5,
            throw_push_factor: 1.5,
            release_speed_loss: 0.1,

            reach_tolerance: 0.75,
            max_event_age: 0.3,

            struck_shake: 0.15,
            struck_shake_time: 0.3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values of GD § 15.
    #[test]
    fn defaults_follow_the_game_design() {
        let t = InteractionTuning::default();
        assert!(t.enabled);
        assert_eq!(t.tap_time, 0.2);
        assert_eq!((t.strike_cooldown, t.tether_cooldown), (1.5, 3.0));
        assert_eq!((t.side_min_lateral, t.side_max_lateral), (0.7, 3.9));
        // "Au plus 2,5 m entre les flancs" of two karts 1.4 m wide.
        let flank_gap = t.side_max_lateral - 2.0 * crate::kart::KART_HALF_EXTENTS.x;
        assert!((flank_gap - 2.5).abs() < 1e-5, "{flank_gap}");
        assert_eq!(t.side_max_longitudinal, 2.5);
        assert_eq!(t.max_height_difference, 1.5);
        assert_eq!((t.front_max_gap, t.front_max_lateral), (2.5, 1.4));
        assert_eq!(t.side_steer, 0.3);
        assert_eq!((t.strike_push_speed, t.strike_push_time), (7.0, 0.4));
        // About 1.5 m sideways (a push fading out linearly).
        let moved = t.strike_push_speed * t.strike_push_time / 2.0;
        assert!((1.3..=1.6).contains(&moved), "{moved}");
        // Issue #193: 35 % (10 % before), within the issue's 30 to 40 %; a swerve of 20° seen
        // only; 10 % when let go.
        assert_eq!(t.strike_speed_loss, 0.35);
        assert!((0.3..=0.4).contains(&t.strike_speed_loss));
        assert!(t.strike_swerve > 0.0 && t.strike_swerve < 0.5 && t.strike_swerve_time > 0.0);
        assert_eq!(t.release_speed_loss, 0.1);
        assert!(t.release_speed_loss < t.strike_speed_loss);
        assert_eq!(t.strike_protection, 1.0);
        assert_eq!(t.whiff_slide, 0.3);
        assert_eq!(t.grab_window, 0.5);
        assert_eq!((t.tether_max_time, t.tether_max_stretch), (2.0, 2.5));
        assert_eq!(t.tether_target_share, 0.25);
        assert_eq!((t.throw_min_hold, t.throw_steer), (0.5, 0.5));
        assert_eq!(t.throw_push_factor, 1.5);
        assert_eq!((t.reach_tolerance, t.max_event_age), (0.75, 0.3));
    }
}
