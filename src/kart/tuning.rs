//! Kart driving tuning. Default values come from `docs/GAME_DESIGN.md` § 2 (marked "à régler"
//! there) or are our own starting points: tweak them here, never hard-code them in systems.

use bevy::prelude::*;

/// All the knobs of the arcade kart model, for the 150cc class and an average kart.
///
/// Speeds are in m/s, accelerations in m/s², rates in 1/s, angles in radians.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct KartTuning {
    // --- Engine -------------------------------------------------------------------------------
    /// Top speed on the road, 150cc. GD § 2 starts at 26 m/s; raised to 28.5 m/s (≈ 103 km/h)
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
    /// Height of the center of mass in the kart's local frame. Low = hard to flip over.
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
    /// (0.5 = 60°). Steeper faces (walls, crate sides) do not count as ground, so a kart
    /// lying against a wall falls back on its wheels instead of driving along it.
    pub min_ground_normal_y: f32,

    // --- Air ----------------------------------------------------------------------------------
    /// How fast the kart turns itself upright in the air (1/s).
    pub righting_rate: f32,
    /// How fast the pitch/roll rotation follows the righting target in the air (1/s).
    pub air_rotation_response: f32,
    /// Ticks without any wheel on the ground before switching to `Airborne` (60 ticks = 1 s).
    pub airborne_grace_ticks: u32,
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
            center_of_mass_height: -0.15,
            inertia: Vec3::new(200.0, 120.0, 200.0),
            gravity_scale: 2.2,

            suspension_rest_length: 0.6,
            suspension_stiffness: 36.0,
            suspension_damping: 3.0,

            min_ground_normal_y: 0.5,

            righting_rate: 4.0,
            air_rotation_response: 8.0,
            airborne_grace_ticks: 4,
        }
    }
}
