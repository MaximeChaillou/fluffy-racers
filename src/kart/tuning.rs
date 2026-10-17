//! Kart driving tuning. Default values come from `docs/GAME_DESIGN.md` § 2 (marked "à régler"
//! there) or are our own starting points: tweak them here, never hard-code them in systems.

use bevy::prelude::*;

use super::environment::EnvironmentTuning;

/// All the knobs of the arcade kart model, for an average kart.
///
/// Speeds are in m/s, accelerations in m/s², rates in 1/s, angles in radians.
///
/// Reflected (`Reflect`) so that the admin panel (issue #36) edits every field without knowing
/// them: a new field shows up there by itself.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct KartTuning {
    // --- Engine -------------------------------------------------------------------------------
    /// Top speed on the road. GD § 2 starts at 26 m/s; raised to 28.5 m/s (≈ 103 km/h)
    /// after Max's first test drive (issue #25).
    pub max_speed: f32,
    /// Time to reach `max_speed` from a standstill, full throttle (GD § 2: about 2.5 s).
    pub time_to_max_speed: f32,
    /// Shape of the engine curve. 1 = constant acceleration; 2 = punchy start that fades
    /// near top speed (95 % of `max_speed` after ~78 % of `time_to_max_speed`).
    pub engine_curve: f32,
    /// Deceleration back down to the current max speed when going faster than it
    /// (end of a boost, entering off-road, downhill).
    pub overspeed_decel: f32,
    /// Deceleration when neither throttle nor brake is pressed.
    pub coast_decel: f32,
    /// Deceleration while braking forward.
    pub brake_decel: f32,
    /// Below this forward speed, the brake button drives backwards (GD § 2: same button).
    pub reverse_threshold: f32,
    /// Top speed in reverse.
    pub reverse_max_speed: f32,
    /// Acceleration in reverse.
    pub reverse_accel: f32,
    /// Max speed multiplier on off-road surfaces (GD § 2: about 55 %). Ignored while boosting.
    pub offroad_speed_factor: f32,

    // --- Boost --------------------------------------------------------------------------------
    /// Forward acceleration while a boost is active and the kart is below the boosted top speed
    /// (GD § 2: "l'accélération est forte").
    pub boost_accel: f32,

    // --- Grip and steering --------------------------------------------------------------------
    /// How fast sideways velocity is cancelled on the ground (1/s). 30 removes ~40 % per tick.
    pub lateral_grip: f32,
    /// Max yaw rate at low and medium speed.
    pub steer_rate: f32,
    /// Max yaw rate at top speed (wider turns when fast).
    pub steer_rate_at_max_speed: f32,
    /// Below this speed the steering fades out: a kart cannot spin in place.
    pub steer_full_speed: f32,
    /// How fast the yaw rate reaches its target (1/s).
    pub steer_response: f32,
    /// Fraction of the steering available in the air.
    pub air_steer_factor: f32,
    /// Damping of pitch and roll rotation on the ground (1/s): keeps the kart calm.
    pub tilt_damping: f32,

    // --- Body ---------------------------------------------------------------------------------
    /// Kart mass in kg (the Weight stat will change it later).
    pub mass: f32,
    /// Height of the center of mass in the kart's local frame (m, 0 = the middle of its
    /// collision box). Keep it there (issue #27): a wall or another kart pushes on a side of the
    /// box, around its middle height, so the push goes through the center of mass and does not
    /// tip the kart. 0.15 m lower (the value of issues #1 to #40), every crash landed above it:
    /// a hard hit kicked the side facing the wall up (or the nose, head on) by 20 to 30°, and
    /// the kart then drove up onto the wall or the other kart. The driving rewrites the
    /// velocities and the springs push along the kart's up axis: neither depends on this height.
    pub center_of_mass_height: f32,
    /// Rotational inertia around the kart's local (x = pitch, y = yaw, z = roll) axes,
    /// in kg·m². High pitch/roll inertia keeps the kart on its wheels.
    pub inertia: Vec3,
    /// Gravity multiplier (GD § 2: reinforced gravity for short, punchy jumps).
    pub gravity_scale: f32,

    // --- Suspension (4 raycasts) --------------------------------------------------------------
    /// Ray length from the wheel anchor: the wheel loses contact beyond this distance.
    pub suspension_rest_length: f32,
    /// Spring stiffness per wheel, as an acceleration per meter of compression (scaled by mass).
    pub suspension_stiffness: f32,
    /// Damper per wheel, as an acceleration per m/s of compression speed (scaled by mass).
    /// Low = bouncy, cartoon landings.
    pub suspension_damping: f32,

    /// Steepest surface a wheel can stand on, as the minimum `normal.y` of the hit
    /// (0.5 = 60°). Steeper faces (the sides of a ramp, a steep bank) do not count as ground, so
    /// a kart lying against them falls back on its wheels instead of driving along them. Walls
    /// and obstacles are never ground, whatever their faces (`track::Surface::Wall`, issue #27).
    pub min_ground_normal_y: f32,

    // --- Air ----------------------------------------------------------------------------------
    /// How fast the kart turns itself upright in the air (1/s).
    pub righting_rate: f32,
    /// How fast the pitch/roll rotation follows the righting target in the air (1/s).
    pub air_rotation_response: f32,
    /// Ticks without any wheel on the ground before switching to `Airborne` (60 ticks = 1 s).
    pub airborne_grace_ticks: u32,

    // --- Hop and drift (GD § 3, issue #4) -------------------------------------------------------
    /// Upward speed given along the ground normal when the jump/drift button is pressed
    /// (m/s). 3.5 with the reinforced gravity ≈ a 33 cm hop, 0.33 s long (measured).
    pub hop_speed: f32,
    /// The hop lasts at least this many ticks: the kart lands on the first tick after it with
    /// a wheel on the ground.
    pub hop_min_ticks: u32,
    /// A hop still in the air after this many ticks becomes a real flight (`Airborne`, no drift):
    /// the kart hopped off a ledge.
    pub hop_max_ticks: u32,
    /// Steering past this (either way) during the hop picks the drift side.
    pub drift_steer_threshold: f32,
    /// Minimum forward speed on landing to start a drift (m/s): no drift at a standstill.
    pub drift_min_speed: f32,
    /// Below this forward speed a drift breaks, charge lost (m/s). Lower than
    /// `drift_min_speed` so that a slight slow-down does not end it.
    pub drift_exit_speed: f32,
    /// A drift survives that many ticks without ground contact (bumps, small jumps), charge
    /// frozen; longer flights break it, charge lost.
    pub drift_max_air_ticks: u32,
    /// Steering toward the drift side by at least this much counts as "inward" (GD § 3.3: 0.5).
    pub drift_inner_steer: f32,
    /// Mini-turbo charge per tick steering inward (GD § 3.3, MK8DX: 5).
    pub drift_charge_inner: f32,
    /// Mini-turbo charge per tick otherwise: neutral or outward (GD § 3.3, MK8DX: 2).
    pub drift_charge_outer: f32,
    /// Same as `drift_charge_inner` with assisted steering (GD § 3.3, MK8DX: 2).
    pub drift_charge_inner_assisted: f32,
    /// Same as `drift_charge_outer` with assisted steering (GD § 3.3, MK8DX: 0.8).
    pub drift_charge_outer_assisted: f32,
    /// Charge thresholds of the blue, orange and purple mini-turbos (GD § 3.4: 270, 570, 870;
    /// the Mini-turbo stat will scale them, issue #15).
    pub mini_turbo_thresholds: [f32; 3],
    /// Boost durations of the blue, orange and purple mini-turbos (GD § 3.4, MK8DX: 0.62 s,
    /// 1.67 s, 2.63 s).
    pub mini_turbo_durations: [f32; 3],
    /// Boost strengths (max speed increase) of the blue, orange and purple mini-turbos. Our own
    /// starting points, à régler: +20 %, +25 %, +30 % (the purple one is as strong as a chili).
    pub mini_turbo_strengths: [f32; 3],
    /// Radius of a drift steered fully inward, at `max_speed` (m). Issue #30 (Max: "sans angle
    /// serré"): 17.5 m, about as tight as the normal steering at full lock (≈ 17 m at top
    /// speed), instead of 10 m. The yaw rate is fixed by the radii, so the curve is tighter
    /// when slower: the hairpin (15 m on the racing line) wants about 25 m/s.
    pub drift_radius_tight: f32,
    /// Radius of a drift steered fully outward, at `max_speed` (m): 55 m (was 32 m). Neutral
    /// steering sits in between (≈ 26 m by default, was 15 m). The mini-turbo charges per radian
    /// of drift, so wider drifts charge more per turn: an orange fits in the big turns of
    /// Prairie Pompon (issue #30).
    pub drift_radius_wide: f32,
    /// Time for the drift's curve to set in after the landing (s, issue #30): the yaw rate goes
    /// from the one the kart had on landing to the drift's along an eased ramp (smoothstep),
    /// then follows `steer_response` like the normal steering. No jolt on landing.
    pub drift_curve_in_time: f32,
    /// Time for the drift's curve to fade back to the normal steering once the drift ends
    /// (release, with or without mini-turbo, or broken) (s): no snap on the way out either.
    pub drift_curve_out_time: f32,
    /// How fast the yaw rate follows the steering during the hop (1/s, issue #30): in the air
    /// the kart mostly keeps the rotation it had; half the ground's `steer_response`, so that
    /// hopping with the stick to the side starts the turn gently.
    pub hop_steer_response: f32,
    /// Slip angle of a drift steered fully inward: the body points this far inside the
    /// direction of travel, the rear swung out (rad, 0.3 ≈ 17°; 26° before issue #30). The
    /// simulated body keeps its grip and follows the trajectory: the angle is shown by the
    /// visual, pivoting around the front axle (the front follows the trajectory, the rear
    /// slides out).
    pub drift_slip_angle_tight: f32,
    /// Slip angle of a drift steered fully outward (rad, 0.12 ≈ 7°; 11° before issue #30).
    pub drift_slip_angle_wide: f32,
    /// The slip angle follows its target (scaled by how much of the drift's curve is set in)
    /// like a damped spring: natural frequency (rad/s)...
    pub drift_slip_frequency: f32,
    /// ...and damping ratio (1 = no overshoot; below, the rear wobbles a little, light and
    /// slippery).
    pub drift_slip_damping: f32,

    // --- Rocket start (GD § 4, issue #13) -------------------------------------------------------
    /// `t` = time from the "2" of the countdown to the first press of the accelerator. Up to
    /// this `t` the start is perfect: `rocket_start_max_duration` (GD § 4: 0.25 s).
    pub rocket_start_perfect: f32,
    /// Boost of a perfect start (s, GD § 4: 2.5 s).
    pub rocket_start_max_duration: f32,
    /// Past `rocket_start_perfect`, the boost shrinks linearly down to
    /// `rocket_start_min_duration` at this `t` (GD § 4: 1 s, when the "1" shows); later, no
    /// boost at all.
    pub rocket_start_late: f32,
    /// Boost of the latest start that still gets one (s, GD § 4: 0.5 s).
    pub rocket_start_min_duration: f32,
    /// Max speed increase of the rocket start boost. Our own starting point, à régler: +30 %,
    /// like a chili (the GD only gives the durations).
    pub rocket_start_strength: f32,
    /// Accelerating before the "2" stalls the engine for this long after GO: the kart cannot
    /// move (s, GD § 4: 1 s).
    pub rocket_start_stall: f32,

    // --- Tricks (GD § 5, issue #13) -------------------------------------------------------------
    /// Pressing jump/drift at most this long before or after the kart leaves a trick ramp
    /// starts a trick (s, GD § 5: about 0.2 s). On the ramp itself the button does not hop: it
    /// is the trick button there.
    pub trick_window: f32,
    /// Boost on landing after a trick (s, GD § 5: 0.5 to 0.8 s with the Mini-turbo stat; the
    /// average kart gets the middle until the stats of issue #15).
    pub trick_boost_duration: f32,
    /// Max speed increase of the trick boost. Our own starting point, à régler: +20 %, like a
    /// blue mini-turbo.
    pub trick_boost_strength: f32,

    // --- Slipstream (GD § 6, issue #13) ---------------------------------------------------------
    /// The slipstream gauge charges within this distance behind another kart, center to
    /// center (m, GD § 6: 5 m)...
    pub slipstream_range: f32,
    /// ...inside this cone behind it, half angle (rad, GD § 6: ±12°)...
    pub slipstream_cone: f32,
    /// ...above this share of the kart's own top speed (GD § 6: 60 %).
    pub slipstream_min_speed: f32,
    /// Charge time before the boost (s, GD § 6: 1.2 s).
    pub slipstream_charge_time: f32,
    /// Out of the wake, the gauge empties at this many seconds of charge per second (à régler:
    /// a full gauge is lost in 0.6 s). A short swerve does not lose everything.
    pub slipstream_decay: f32,
    /// Slipstream boost duration (s, GD § 6: 1 s).
    pub slipstream_boost_duration: f32,
    /// Max speed increase of the slipstream boost. Our own starting point, à régler: +25 %.
    pub slipstream_boost_strength: f32,

    // --- Boost pads (GD § 2, issue #13) ---------------------------------------------------------
    /// Boost given by a pad, refreshed while a wheel rolls on it (s, GD § 2: about 1 s).
    pub boost_pad_duration: f32,
    /// Max speed increase of a boost pad. Our own starting point, à régler: +30 %, like a chili.
    pub boost_pad_strength: f32,

    // --- Bumps between karts (GD § 7, issue #14) ------------------------------------------------
    /// Two karts hitting each other at least this fast (closing speed along the contact normal,
    /// m/s) is a bump: the bumped kart loses a croquette. Our own starting point, à régler: a
    /// boosted kart rear-ending another one, or a swerve of about 10° into its side at full
    /// speed. Rubbing along a kart or pushing it stays below.
    pub bump_min_speed: f32,
    /// The kart that brought at least this share of the closing speed is the bumper, the other
    /// one is bumped. When neither did (head on), both are bumped.
    pub bump_bumper_share: f32,

    // --- Hit by an item (GD § 8, issue #16) -----------------------------------------------------
    /// Deceleration of a kart spinning out (`KartState::Hit`, light items: pelure, abeille), the
    /// accelerator and the steering ignored (m/s², à régler): from top speed, about a quarter of
    /// it is left after the 1 s spin. How long the spin lasts is the item's
    /// (`ItemTuning::spin_time`).
    pub hit_spin_decel: f32,
    /// Same for a tumble (heavy items: pelote), which stops the kart (m/s², à régler).
    pub hit_tumble_decel: f32,

    // --- Special environments (GD § 10, issue #20) ----------------------------------------------
    /// Under water and the glider: their own knobs.
    pub environment: EnvironmentTuning,
}

impl Default for KartTuning {
    fn default() -> Self {
        Self {
            max_speed: 28.5,
            time_to_max_speed: 2.5,
            engine_curve: 2.0,
            overspeed_decel: 12.0,
            coast_decel: 6.0,
            brake_decel: 28.0,
            reverse_threshold: 0.5,
            reverse_max_speed: 8.0,
            reverse_accel: 10.0,
            offroad_speed_factor: 0.55,

            boost_accel: 40.0,

            lateral_grip: 30.0,
            steer_rate: 2.3,
            steer_rate_at_max_speed: 1.65,
            steer_full_speed: 6.0,
            steer_response: 8.0,
            air_steer_factor: 0.3,
            tilt_damping: 4.0,

            mass: 150.0,
            center_of_mass_height: 0.0,
            inertia: Vec3::new(200.0, 120.0, 200.0),
            gravity_scale: 2.2,

            suspension_rest_length: 0.6,
            suspension_stiffness: 36.0,
            suspension_damping: 3.0,

            min_ground_normal_y: 0.5,

            righting_rate: 4.0,
            air_rotation_response: 8.0,
            airborne_grace_ticks: 4,

            hop_speed: 3.5,
            hop_min_ticks: 8,
            hop_max_ticks: 30,
            drift_steer_threshold: 0.3,
            drift_min_speed: 10.0,
            drift_exit_speed: 7.0,
            drift_max_air_ticks: 30,
            drift_inner_steer: 0.5,
            drift_charge_inner: 5.0,
            drift_charge_outer: 2.0,
            drift_charge_inner_assisted: 2.0,
            drift_charge_outer_assisted: 0.8,
            mini_turbo_thresholds: [270.0, 570.0, 870.0],
            mini_turbo_durations: [0.62, 1.67, 2.63],
            mini_turbo_strengths: [0.2, 0.25, 0.3],
            drift_radius_tight: 17.5,
            drift_radius_wide: 55.0,
            drift_curve_in_time: 0.35,
            drift_curve_out_time: 0.3,
            hop_steer_response: 4.0,
            drift_slip_angle_tight: 0.3,
            drift_slip_angle_wide: 0.12,
            drift_slip_frequency: 9.0,
            drift_slip_damping: 0.7,

            rocket_start_perfect: 0.25,
            rocket_start_max_duration: 2.5,
            rocket_start_late: 1.0,
            rocket_start_min_duration: 0.5,
            rocket_start_strength: 0.3,
            rocket_start_stall: 1.0,

            trick_window: 0.2,
            trick_boost_duration: 0.65,
            trick_boost_strength: 0.2,

            slipstream_range: 5.0,
            slipstream_cone: 12f32.to_radians(),
            slipstream_min_speed: 0.6,
            slipstream_charge_time: 1.2,
            slipstream_decay: 2.0,
            slipstream_boost_duration: 1.0,
            slipstream_boost_strength: 0.25,

            boost_pad_duration: 1.0,
            boost_pad_strength: 0.3,

            bump_min_speed: 5.0,
            bump_bumper_share: 0.6,

            hit_spin_decel: 21.0,
            hit_tumble_decel: 26.0,
            environment: EnvironmentTuning::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #13: the values of GD § 2, 4, 5 and 6.
    #[test]
    fn driving_technique_defaults_follow_the_game_design() {
        let t = KartTuning::default();
        assert_eq!(t.rocket_start_perfect, 0.25);
        assert_eq!(t.rocket_start_max_duration, 2.5);
        assert_eq!(t.rocket_start_late, 1.0);
        assert_eq!(t.rocket_start_min_duration, 0.5);
        assert_eq!(t.rocket_start_stall, 1.0);
        assert!((t.trick_window - 0.2).abs() < 1e-6);
        assert!((0.5..=0.8).contains(&t.trick_boost_duration));
        assert_eq!(t.slipstream_range, 5.0);
        assert!((t.slipstream_cone.to_degrees() - 12.0).abs() < 1e-4);
        assert_eq!(t.slipstream_min_speed, 0.6);
        assert_eq!(t.slipstream_charge_time, 1.2);
        assert_eq!(t.slipstream_boost_duration, 1.0);
        assert_eq!(t.boost_pad_duration, 1.0);
        for strength in [
            t.rocket_start_strength,
            t.trick_boost_strength,
            t.slipstream_boost_strength,
            t.boost_pad_strength,
        ] {
            assert!(strength > 0.0 && strength <= 0.3, "{strength}");
        }
    }
}
