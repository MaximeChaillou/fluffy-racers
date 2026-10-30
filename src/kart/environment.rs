//! Special environments (GD § 10): under water and the glider (issue #20), the anti-gravity
//! (issue #78).
//!
//! - **Under water**: a kart whose center is under the water of a lake (`crate::track::Lake`,
//!   with a little hysteresis: [`next_underwater`]) drives with its stats « eau » (the Speed and
//!   the Handling of [`Terrain::Water`]) and the water's own physics ([`water_tuning`]): slower
//!   ([`EnvironmentTuning::water_top_speed`], through `MaxSpeedFactors::environment`), floatier
//!   (less gravity, its jumps and hops slowed by the water, which also brakes harder).
//! - **The glider**: leaving a glide ramp (`crate::track::GlideRamp`) opens it
//!   ([`next_gliding`]); the kart then flies with its stats « air » ([`Terrain::Air`]): Up (the
//!   accelerator) dives, faster and lower; Down (the brake) climbs, slower and higher; Left and
//!   Right turn it ([`glide_speeds`], [`glide_yaw_target`]). Its descent is slow and smooth,
//!   whatever the kart did on the ramp. Touching the ground closes it, and so do the water and
//!   the pelican. Its pitch and bank are only shown ([`glide_pose`]): the simulated body stays
//!   level (the camera and the collisions do not change).
//! - **The anti-gravity** (issue #78): a wheel on the road of an anti-gravity zone
//!   (`crate::track::AntiGravity`) puts the kart in it, a wheel on the road beyond takes it out
//!   (in the air, it stays as it was). There, its gravity pulls along the road's normal under its
//!   wheels ([`KartEnvironment::up`]; in the air, that of the zone's road below it, or the last
//!   one), as strong as the world's
//!   ([`anti_gravity_force`]); its tilt follows that normal on the ground, and turns back to it in
//!   the air; it drives with its stats « anti-gravité » ([`Terrain::AntiGravity`]); its wheels
//!   pivot (shown only); two karts in anti-gravity that bump into each other both get a boost
//!   (the spin boost, [`EnvironmentTuning::spin_boost_strength`]).
//!
//! The kart only reads its `KartInput`. Everything here is pure and unit-tested, but the two
//! systems at the end (`KartSystems::Drive`).

use bevy::prelude::*;
use bevy_rapier3d::prelude::{CollisionGroups, Group};

use super::stats::{StatEffects, Terrain};
use super::{Kart, KartInput, KartState, KartTuning, MaxSpeedFactors};
use crate::track::{Lake, water_depth};

/// The knobs of the special environments (GD § 10, issue #20). Our own starting points, à
/// régler. Speeds in m/s, rates in 1/s, angles in radians. A part of the kart's tuning
/// ([`KartTuning::environment`], edited with it in the admin panel), not a resource of its own:
/// the game's world stays exactly what it was before issue #20 on the circuits without a lake
/// or a glide ramp (a new resource is a new archetype in Bevy, which reorders the colliders in
/// Rapier, see `STATUS.md`).
#[derive(Reflect, Clone, Debug, PartialEq)]
#[reflect(Default)]
pub struct EnvironmentTuning {
    // --- Under water ----------------------------------------------------------------------------
    /// Top speed multiplier under water (GD § 10: « plus lentes »): 0.9, on top of the kart's
    /// Speed « eau » (`MaxSpeedFactors::environment`).
    pub water_top_speed: f32,
    /// Gravity multiplier under water (GD § 10: « plus flottantes »): 0.5. The hops and jumps last
    /// longer: the drift's and the hop's times in the air stretch with it.
    pub water_gravity: f32,
    /// The water brakes harder: `coast_decel` and `overspeed_decel` multiplied by this under
    /// water (1.6: entering the water at full speed, the kart slows down to its water top speed
    /// in about half a second).
    pub water_drag: f32,
    /// Off the ground under water, the vertical speed fades at this rate (1/s): floaty jumps.
    pub water_vertical_drag: f32,
    /// A kart goes under water once its center is this far below the surface, and out once it is
    /// this far above it (m): no flicker at the surface.
    pub water_margin: f32,

    // --- Glider ---------------------------------------------------------------------------------
    /// The glider opens when the kart flies within this time after a wheel last touched a glide
    /// ramp (s)...
    pub glide_open_window: f32,
    /// ...once no wheel has touched the ground for this many ticks (a bump on the ramp is not a
    /// takeoff).
    pub glide_open_ticks: u32,
    /// Forward speed of a level glide, as a share of the kart's top speed (its Speed « air »,
    /// its croquettes, a boost): 0.95.
    pub glide_speed: f32,
    /// The same diving (Up held): 1.25, faster (issue #197: 1.2 before).
    pub glide_dive_speed: f32,
    /// The same climbing (Down held): 0.7, slower.
    pub glide_climb_speed: f32,
    /// How fast the forward speed reaches the glide's (m/s²): even a kart that rolled off the
    /// ramp slowly flies on.
    pub glide_accel: f32,
    /// Slope of a level glide: meters down per meter ahead (0.1: a slow descent, the same
    /// whatever the speed).
    pub glide_slope: f32,
    /// The same diving: 0.2, twice as steep (issue #197: 0.17 before; the steepest that still
    /// reaches the landing of every glider of the game, diving from the lip, with 2 m to spare).
    pub glide_dive_slope: f32,
    /// The same climbing: 0.078 (issue #197: 0.08 before), but slower: with the rise
    /// below, a climb flies about a quarter farther than before.
    pub glide_climb_slope: f32,
    /// A glide always comes down at least this fast (m/s): a slow climb ends too (issue #197:
    /// 0.8; 1.2 before, which flattened every climb).
    pub glide_min_sink: f32,
    /// How fast the vertical speed reaches the glide's slope (1/s): the jump off the ramp fades
    /// into the glide.
    pub glide_vertical_response: f32,
    /// Issue #197: the glider's attitude (from a full dive, +1, to a full climb, -1) follows the
    /// stick at this rate (1/s): 3.3, from level to a full dive or climb in 0.3 s. The speed and
    /// the slope follow the attitude: no sudden change.
    pub glide_attitude_rate: f32,
    /// Issue #197: climbing faster than the climb's speed (off a dive, off the ramp at full
    /// speed), the glider trades its speed for height: each m/s of forward speed it loses gives it
    /// this much upward speed (m/s per m/s), times how much it climbs...
    pub glide_climb_lift: f32,
    /// ...up to this upward speed (m/s): a short rise, bounded (its speed falls to the climb's at
    /// `glide_accel`, in a second or two), then a slow descent.
    pub glide_climb_max_rise: f32,
    /// Turn rate at full lock (rad/s), times the Handling « air »'s steering factor.
    pub glide_turn_rate: f32,
    /// How fast the turn rate follows the stick (1/s), times the Handling « air »'s
    /// responsiveness.
    pub glide_turn_response: f32,
    /// The wing grips the air: sideways speed fades at this rate (1/s).
    pub glide_side_grip: f32,
    /// Shown only: the kart pitches up to this angle (rad), nose down when it sinks faster than a
    /// level glide (fully at the dive's slope), up when slower (fully at the climb's, or rising):
    /// 0.45 (issue #197: 0.3 before, against the dive's slope both ways)...
    pub glide_pitch: f32,
    /// ...and banks into its turns, up to this angle at full lock (rad).
    pub glide_bank: f32,
    /// Shown only: the wing opens (and folds) in this long (s).
    pub wing_open_seconds: f32,

    // --- Anti-gravity (issue #78) ---------------------------------------------------------------
    /// On the ground in anti-gravity, the kart's tilt turns its up toward the road's normal at
    /// this rate (1/s): the road twists under it, the kart follows (a lag of about a tenth of the
    /// road's roll rate). In the air, `KartTuning::righting_rate` turns it back to that normal.
    pub anti_gravity_align_rate: f32,
    /// In the air in anti-gravity (a hop, a jump), the kart's gravity follows the road of the
    /// zone below it, found up to this far along its up (m): the road twists away under a kart
    /// in the air, its gravity turns with it. Without road there, the normal of the last contact.
    pub anti_gravity_probe: f32,
    /// The anti-gravity holds the karts to its road: a kart in the air more than this high above
    /// it (its center, m: a hop rises to about 0.85 m)...
    pub anti_gravity_hold_height: f32,
    /// ...is pulled toward it this many times harder (on top of its gravity): where the road twists
    /// away under a kart in the air, it never flies over a wall (1.2 m high).
    pub anti_gravity_hold: f32,
    /// Issue #96: on the ground in anti-gravity, where the zone's road bends along it (a turn
    /// banked like a wall), the kart's velocity turns with it ([`anti_gravity_guide`]): the road
    /// guides it like a rail, and its springs only hold its gravity. Off, its springs alone turn
    /// it, pressed into the road by the curve (boosted to 50 m/s round the Ruban's banked turn,
    /// down to its bump stops). A straight zone, however it twists, does not bend: nothing changes there.
    pub anti_gravity_guide: bool,
    /// Two karts in anti-gravity that bump into each other this fast (closing speed, m/s) both get
    /// the spin boost (GD § 10, MK8DX)...
    pub spin_boost_min_speed: f32,
    /// ...of this strength (top speed +20 %, like a blue mini-turbo)...
    pub spin_boost_strength: f32,
    /// ...for this long (s).
    pub spin_boost_duration: f32,
    /// Shown only: the wheels pivot flat (and back) in this long (s).
    pub wheel_pivot_seconds: f32,
}

impl Default for EnvironmentTuning {
    fn default() -> Self {
        Self {
            water_top_speed: 0.9,
            water_gravity: 0.5,
            water_drag: 1.6,
            water_vertical_drag: 1.2,
            water_margin: 0.15,

            glide_open_window: 0.5,
            glide_open_ticks: 3,
            glide_speed: 0.95,
            glide_dive_speed: 1.25,
            glide_climb_speed: 0.7,
            glide_accel: 9.0,
            glide_slope: 0.1,
            glide_dive_slope: 0.2,
            glide_climb_slope: 0.078,
            glide_min_sink: 0.8,
            glide_vertical_response: 2.5,
            glide_attitude_rate: 3.3,
            glide_climb_lift: 1.0,
            glide_climb_max_rise: 2.0,
            glide_turn_rate: 1.1,
            glide_turn_response: 4.0,
            glide_side_grip: 6.0,
            glide_pitch: 0.45,
            glide_bank: 0.5,
            wing_open_seconds: 0.3,

            anti_gravity_align_rate: 12.0,
            anti_gravity_probe: 8.0,
            anti_gravity_hold_height: 1.0,
            anti_gravity_hold: 2.0,
            anti_gravity_guide: true,
            spin_boost_min_speed: 3.0,
            spin_boost_strength: 0.2,
            spin_boost_duration: 0.6,
            wheel_pivot_seconds: 0.25,
        }
    }
}

/// Where a kart is (issue #20): under water, gliding; in anti-gravity (issue #78). On every kart;
/// online, on the other peers' karts too (from their states: `crate::net`), for the visuals.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct KartEnvironment {
    /// Its center is under the water of a lake.
    pub underwater: bool,
    /// Its glider is open.
    pub gliding: bool,
    /// Ticks since a wheel last touched a glide ramp (`u32::MAX`: never).
    pub since_glide_ramp: u32,
    /// Issue #197: under its glider, its attitude ([`next_glide_attitude`]): +1 a full dive, -1
    /// a full climb, 0 level (and always 0 with the glider closed). Its own kart's only: the other
    /// peers do not need it (they see its velocity).
    pub glide_attitude: f32,
    /// Issue #78: in an anti-gravity zone (a wheel last touched its road).
    pub anti_gravity: bool,
    /// Issue #78: its up, where its gravity pulls away from: the world's up, but in anti-gravity
    /// the road's normal under its wheels (in the air, that of the zone's road below it, or the
    /// last one). What the camera, the bots and the items take as the kart's vertical.
    pub up: Vec3,
}

impl Default for KartEnvironment {
    fn default() -> Self {
        Self {
            underwater: false,
            gliding: false,
            since_glide_ramp: u32::MAX,
            glide_attitude: 0.0,
            anti_gravity: false,
            up: Vec3::Y,
        }
    }
}

impl KartEnvironment {
    /// The terrain its stats use (GD § 9): the air while gliding, the water under water, the
    /// anti-gravity in anti-gravity (issue #78).
    pub fn terrain(&self) -> Terrain {
        if self.gliding {
            Terrain::Air
        } else if self.underwater {
            Terrain::Water
        } else if self.anti_gravity {
            Terrain::AntiGravity
        } else {
            Terrain::Ground
        }
    }
}

/// The world's gravity (m/s²): Rapier's default, `RapierConfiguration::gravity`, downward.
pub const GRAVITY: f32 = 9.81;

/// Issue #78: whether the kart is in anti-gravity after this tick: with a wheel on the ground,
/// whether one is on the road of a zone (`on_zone`: how many); in the air, as it was (a hop, a
/// jump on the wall: it lands back on it). Never under its glider nor under water. Issue #171:
/// out of it, it goes in with most of its wheels on the ground on the zone's road (more than
/// half), not one: half on the zone and half on the road that follows it, a kart came out and
/// went back in from a tick to the next (its rays turn with its up), at the end of a zone.
pub fn next_anti_gravity(
    environment: &KartEnvironment,
    wheels_on_ground: usize,
    on_zone: usize,
) -> bool {
    if environment.gliding || environment.underwater {
        return false;
    }
    if wheels_on_ground == 0 {
        environment.anti_gravity
    } else if environment.anti_gravity {
        on_zone > 0
    } else {
        2 * on_zone > wheels_on_ground
    }
}

/// Issue #78: the force of the gravity of a kart of `mass` (kg) in anti-gravity, toward the road
/// under it (`up`, unit: away from it), as strong as its gravity elsewhere (`gravity_scale` times
/// the world's) (N).
pub fn anti_gravity_force(up: Vec3, gravity_scale: f32, mass: f32) -> Vec3 {
    -up * (GRAVITY * gravity_scale * mass)
}

/// Issue #96: the velocity `velocity` of a kart on the ground in anti-gravity, over the road of a
/// zone that bends (`bend`: a wall round a turn), turned with the road over this tick (`dt`, s):
/// as far as the kart goes along it, about the road's right, by the angle its normal turns (the
/// road guides it like a rail, as fast as it goes, instead of running into it). A straight zone
/// does not bend, however it twists: there, exactly the same velocity (the springs keep the kart
/// on its twist, as before). The rotation keeps its speed.
pub fn anti_gravity_guide(velocity: Vec3, bend: &crate::track::Bend, dt: f32) -> Vec3 {
    let angle = bend.rate * velocity.dot(bend.forward) * dt;
    if angle == 0.0 {
        return velocity;
    }
    Quat::from_axis_angle(bend.right, angle) * velocity
}

/// Whether a kart is under water now: `depth` is how deep its center is under the surface of the
/// lake it is over (negative above it; `None`: over no lake). It goes under once `margin` below
/// the surface and out once `margin` above it.
pub fn next_underwater(was: bool, depth: Option<f32>, margin: f32) -> bool {
    match depth {
        None => false,
        Some(depth) if was => depth > -margin,
        Some(depth) => depth > margin,
    }
}

/// The kart's tuning under water (`base`: its own, its stats applied): gravity, drag, and the
/// times allowed in the air stretched as much as the gravity is weaker (a hop lasts longer: it
/// must still become a drift, and a drift must survive it).
pub fn water_tuning(base: &KartTuning, env: &EnvironmentTuning) -> KartTuning {
    let gravity = env.water_gravity.clamp(0.05, 1.0);
    let stretch = |ticks: u32| (ticks as f32 / gravity).round() as u32;
    let drag = env.water_drag.max(0.0);
    KartTuning {
        gravity_scale: base.gravity_scale * gravity,
        coast_decel: base.coast_decel * drag,
        overspeed_decel: base.overspeed_decel * drag,
        hop_max_ticks: stretch(base.hop_max_ticks),
        drift_max_air_ticks: stretch(base.drift_max_air_ticks),
        airborne_grace_ticks: stretch(base.airborne_grace_ticks),
        ..base.clone()
    }
}

/// The collision group of the karts under their glider (issue #20).
pub const GLIDERS: Group = Group::GROUP_32;

/// A kart's collision groups: under its glider, it meets everything but the other gliders (the
/// wings are 4 m wide, the flight lasts 2 to 3 s; two gliders side by side used to lock on each
/// other's corners and spiral together, over the walls or into the lagoon); otherwise, everything.
pub fn glider_groups(gliding: bool) -> CollisionGroups {
    if gliding {
        CollisionGroups::new(GLIDERS, Group::ALL.difference(GLIDERS))
    } else {
        CollisionGroups::new(Group::ALL, Group::ALL)
    }
}

/// Whether two colliders with these groups meet (Rapier's rule, test mode « and »).
pub fn groups_meet(a: CollisionGroups, b: CollisionGroups) -> bool {
    a.memberships.intersects(b.filters) && b.memberships.intersects(a.filters)
}

/// Whether the glider is open after this tick. It opens once no wheel has touched the ground for
/// `glide_open_ticks`, within `glide_open_window` of the last glide ramp; it closes as soon as a
/// wheel touches the ground, under water, or with the pelican (`towed`).
pub fn next_gliding(
    tuning: &EnvironmentTuning,
    environment: &KartEnvironment,
    wheels_on_ground: usize,
    ticks_without_ground: u32,
    towed: bool,
    dt: f32,
) -> bool {
    if towed || environment.underwater || wheels_on_ground > 0 {
        return false;
    }
    if environment.gliding {
        return true;
    }
    let window = (tuning.glide_open_window / dt.max(1e-4)).round() as u32;
    ticks_without_ground >= tuning.glide_open_ticks && environment.since_glide_ramp <= window
}

/// The glider's pitch from the stick (GD § 10): Up (the accelerator) dives (+1), Down (the brake)
/// climbs (-1); both or neither: a level glide.
pub fn glide_pitch(input: &KartInput) -> f32 {
    input.throttle.clamp(0.0, 1.0) - input.brake.clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Issue #197: the glider's attitude after a tick of `dt`: from `attitude` toward the stick's
/// `pitch` ([`glide_pitch`]) at `glide_attitude_rate`, -1 to +1.
pub fn next_glide_attitude(tuning: &EnvironmentTuning, attitude: f32, pitch: f32, dt: f32) -> f32 {
    let step = tuning.glide_attitude_rate.max(0.0) * dt;
    let attitude = attitude.clamp(-1.0, 1.0);
    attitude + (pitch.clamp(-1.0, 1.0) - attitude).clamp(-step, step)
}

/// The glide's forward speed (a share of the kart's top speed) and slope (meters down per meter
/// ahead) at `attitude` (+1 a full dive, -1 a full climb).
pub fn glide_targets(tuning: &EnvironmentTuning, attitude: f32) -> (f32, f32) {
    let attitude = attitude.clamp(-1.0, 1.0);
    if attitude >= 0.0 {
        (
            lerp(tuning.glide_speed, tuning.glide_dive_speed, attitude),
            lerp(tuning.glide_slope, tuning.glide_dive_slope, attitude),
        )
    } else {
        (
            lerp(tuning.glide_speed, tuning.glide_climb_speed, -attitude),
            lerp(tuning.glide_slope, tuning.glide_climb_slope, -attitude),
        )
    }
}

/// One tick of `dt` of the glide at `attitude` ([`next_glide_attitude`]): the new forward
/// (level) and vertical speeds (m/s). The forward speed moves toward its share of `top_speed` at
/// `glide_accel`; the vertical speed fades toward the glide's slope times the forward speed,
/// `glide_min_sink` at least (the jump off the ramp turns into the glide's slow descent, at the
/// same angle at every speed). Issue #197: climbing faster than the climb's speed, the speed it
/// loses lifts it (`glide_climb_lift`, up to `glide_climb_max_rise`): it trades its speed for
/// height, and rises a little for a short while.
pub fn glide_speeds(
    tuning: &EnvironmentTuning,
    top_speed: f32,
    forward: f32,
    vertical: f32,
    attitude: f32,
    dt: f32,
) -> (f32, f32) {
    let (share, slope) = glide_targets(tuning, attitude);
    let target = top_speed.max(0.0) * share;
    let step = tuning.glide_accel.max(0.0) * dt;
    let slowed = forward + (target - forward).clamp(-step, step);
    let sink = (slope.max(0.0) * slowed.max(0.0)).max(tuning.glide_min_sink);
    let fade = (-tuning.glide_vertical_response.max(0.0) * dt).exp();
    let mut vertical = -sink + (vertical + sink) * fade;
    // Climbing, the speed it loses lifts it (never beyond the rise's cap).
    let climbing = (-attitude).clamp(0.0, 1.0);
    let rise = tuning.glide_climb_max_rise.max(0.0);
    if climbing > 0.0 && vertical < rise {
        let lift = tuning.glide_climb_lift.max(0.0) * (forward - slowed).max(0.0) * climbing;
        vertical = (vertical + lift).min(rise);
    }
    (slowed, vertical)
}

/// The glide's target yaw rate (rad/s, right turns negative like the ground's): the stick times
/// the turn rate, times `steer_rate`, the Handling « air »'s factor (`StatEffects::steer_rate` on
/// [`Terrain::Air`]).
pub fn glide_yaw_target(tuning: &EnvironmentTuning, steer: f32, steer_rate: f32) -> f32 {
    -steer.clamp(-1.0, 1.0) * tuning.glide_turn_rate * steer_rate
}

/// How a gliding kart is shown (rad): its pitch (positive: nose up) and its bank (positive:
/// rolled to the right), from its velocity and its yaw rate only (what the other peers know too):
/// nose down when it comes down steeper than a level glide (diving), up when flatter (climbing,
/// or still rising off the ramp); banked into its turns.
pub fn glide_pose(tuning: &EnvironmentTuning, velocity: Vec3, yaw_rate: f32) -> (f32, f32) {
    let slope = -velocity.y / velocity.xz().length().max(1.0);
    // Issue #197: a full dive shows the nose fully down, a full climb fully up (each against its
    // own slope), a rise (off the ramp, trading speed for height) fully up too.
    let below = slope - tuning.glide_slope;
    let span = if below > 0.0 {
        tuning.glide_dive_slope - tuning.glide_slope
    } else {
        tuning.glide_slope - tuning.glide_climb_slope
    };
    let pitch = (-below / span.abs().max(0.01)).clamp(-1.0, 1.0) * tuning.glide_pitch;
    let turn = tuning.glide_turn_rate.abs().max(0.1);
    // A right turn is a negative yaw rate: roll to the right.
    let bank = (-yaw_rate / turn).clamp(-1.0, 1.0) * tuning.glide_bank;
    (pitch, bank)
}

/// Every kart over a lake goes under water or out of it (`next_underwater`); under water, its
/// glider closes. Carried by the pelican, never under water.
pub(super) fn detect_water(
    tuning: Res<KartTuning>,
    lakes: Query<&Lake>,
    mut karts: Query<(&Transform, &KartState, &mut KartEnvironment), With<Kart>>,
) {
    let tuning = &tuning.environment;
    for (transform, state, mut environment) in &mut karts {
        let depth = water_depth(lakes.iter(), transform.translation);
        let under = *state != KartState::Respawning
            && next_underwater(environment.underwater, depth, tuning.water_margin);
        if under != environment.underwater {
            environment.underwater = under;
            if under {
                environment.gliding = false;
            }
        }
    }
}

/// The top speed factors of the kart's terrain: its Speed there (`MaxSpeedFactors::stats`) and
/// the water's slowness (`MaxSpeedFactors::environment`). Written only when they change: on the
/// ground, exactly what the kart was spawned with. Under its glider, the kart's collision groups
/// ([`glider_groups`]), given the first time it glides: a kart that never glides keeps none (the
/// default, everything), as before issue #20.
pub(super) fn apply_environments(
    mut commands: Commands,
    tuning: Res<KartTuning>,
    mut karts: Query<
        (
            Entity,
            &KartEnvironment,
            &StatEffects,
            &mut MaxSpeedFactors,
            Option<&mut CollisionGroups>,
        ),
        With<Kart>,
    >,
) {
    let tuning = &tuning.environment;
    for (entity, environment, effects, mut factors, groups) in &mut karts {
        let wanted = glider_groups(environment.gliding);
        match groups {
            Some(mut groups) => {
                groups.set_if_neq(wanted);
            }
            None if environment.gliding => {
                commands.entity(entity).insert(wanted);
            }
            None => {}
        }
        let stats = effects.on(environment.terrain()).top_speed;
        let water = if environment.underwater {
            tuning.water_top_speed
        } else {
            1.0
        };
        if factors.stats != stats || factors.environment != water {
            factors.stats = stats;
            factors.environment = water;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// Under the surface by more than the margin to go in, above it by more than the margin to
    /// come out: a kart bobbing at the surface does not flicker.
    #[test]
    fn a_kart_goes_under_water_and_out_with_a_margin() {
        let m = 0.15;
        assert!(!next_underwater(false, None, m));
        assert!(!next_underwater(true, None, m), "out of the lake: out");
        assert!(!next_underwater(false, Some(0.1), m));
        assert!(next_underwater(false, Some(0.2), m));
        assert!(next_underwater(true, Some(-0.1), m), "still under");
        assert!(!next_underwater(true, Some(-0.2), m));
        assert!(!next_underwater(false, Some(-3.0), m));
    }

    /// Under their glider, karts meet the track, the karts on the ground and everything else, but
    /// not the other gliders; back on the ground, everything again.
    #[test]
    fn gliders_fly_through_each_other_and_nothing_else() {
        let (glider, kart) = (glider_groups(true), glider_groups(false));
        let anything = CollisionGroups::new(Group::ALL, Group::ALL);
        assert!(!groups_meet(glider, glider), "two gliders");
        assert!(groups_meet(glider, kart) && groups_meet(kart, glider));
        assert!(groups_meet(glider, anything), "the track, an item");
        assert!(groups_meet(kart, kart) && groups_meet(kart, anything));
        assert_eq!(
            kart, anything,
            "on the ground: everything, like a collider without groups"
        );
    }

    /// Issue #78: a wheel on the road of a zone puts the kart in anti-gravity (issue #171: most
    /// of its wheels on the ground), its wheels all on any other ground take it out; in the air, it stays as it was; never under the glider or
    /// under water. Its gravity there pulls toward the road, as strong as elsewhere; its terrain
    /// is the anti-gravity's.
    #[test]
    fn the_anti_gravity_holds_from_its_road_to_the_next_ground() {
        let out = KartEnvironment::default();
        let inside = KartEnvironment {
            anti_gravity: true,
            up: Vec3::X,
            ..default()
        };
        assert!(next_anti_gravity(&out, 2, 2));
        assert!(!next_anti_gravity(&out, 2, 0));
        assert!(!next_anti_gravity(&out, 0, 0), "in the air: as it was");
        assert!(next_anti_gravity(&inside, 0, 0), "a hop on the wall");
        assert!(!next_anti_gravity(&inside, 1, 0), "back on the ground");
        assert!(next_anti_gravity(&inside, 4, 4));
        // Issue #171: in with most of its wheels on the zone, out with none.
        assert!(!next_anti_gravity(&out, 4, 2), "half of them: not yet");
        assert!(next_anti_gravity(&out, 4, 3));
        assert!(next_anti_gravity(&inside, 4, 1), "one of them still on it");
        let gliding = KartEnvironment {
            gliding: true,
            ..inside
        };
        assert!(!next_anti_gravity(&gliding, 0, 4));
        let wet = KartEnvironment {
            underwater: true,
            ..inside
        };
        assert!(!next_anti_gravity(&wet, 3, 3));
        assert_eq!(inside.terrain(), Terrain::AntiGravity);
        assert_eq!(gliding.terrain(), Terrain::Air);
        assert_eq!(out.up, Vec3::Y);
        let tuning = KartTuning::default();
        let force = anti_gravity_force(Vec3::X, tuning.gravity_scale, tuning.mass);
        assert!(force.normalize().dot(Vec3::NEG_X) > 0.9999);
        // As strong as the world's gravity on the kart (`GravityScale` times Rapier's).
        let weight = GRAVITY * tuning.gravity_scale * tuning.mass;
        assert!((force.length() - weight).abs() < 1e-3 * weight);
    }

    /// Issue #96: on the ground in anti-gravity, the road guides the kart: its velocity turns
    /// with the road's normal under it (round a wall that curves, its velocity along the road
    /// stays along it, at the same speed); with the same normal, exactly the same velocity.
    #[test]
    fn the_road_guides_a_kart_in_anti_gravity() {
        // A wall facing +X curving round about the vertical (its right, down the wall): 1/40 per
        // meter; a kart driving along it (-Z) at 40 m/s, one tick.
        let bend = crate::track::Bend {
            forward: Vec3::NEG_Z,
            right: Vec3::NEG_Y,
            rate: 1.0 / 40.0,
        };
        let dt = 1.0 / 60.0;
        let along = Vec3::new(0.0, 0.0, -40.0);
        let guided = anti_gravity_guide(along, &bend, dt);
        // Its normal turned by 1/40 × 40 m/s × dt about -Y: the velocity with it, its speed kept.
        let turned = Quat::from_axis_angle(Vec3::NEG_Y, dt);
        assert!((guided.length() - 40.0).abs() < 1e-3);
        assert!(guided.dot(turned * Vec3::X).abs() < 1e-3, "{guided}");
        // Backward, the other way; standing still or on a straight (no bend), the same velocity.
        let back = anti_gravity_guide(-along, &bend, dt);
        assert!(back.dot(turned.inverse() * Vec3::X).abs() < 1e-3, "{back}");
        assert_eq!(
            anti_gravity_guide(Vec3::new(0.0, 2.0, 0.0), &bend, dt),
            Vec3::new(0.0, 2.0, 0.0)
        );
        let straight = crate::track::Bend { rate: 0.0, ..bend };
        assert_eq!(anti_gravity_guide(along, &straight, dt), along);
        assert!(EnvironmentTuning::default().anti_gravity_guide);
    }

    /// The knobs are in the admin panel, which edits `KartTuning` by reflection: every one of
    /// them is a reflected field of `KartTuning::environment`, at its default.
    #[test]
    fn the_knobs_are_reflected_in_the_kart_tuning() {
        use bevy::reflect::ReflectRef;
        let tuning = KartTuning::default();
        let ReflectRef::Struct(kart) = tuning.reflect_ref() else {
            panic!("KartTuning is a struct");
        };
        let environment = kart.field("environment").expect("KartTuning::environment");
        let ReflectRef::Struct(environment) = environment.reflect_ref() else {
            panic!("EnvironmentTuning is a struct");
        };
        let defaults = EnvironmentTuning::default();
        let ReflectRef::Struct(expected) = defaults.reflect_ref() else {
            unreachable!()
        };
        assert_eq!(environment.field_len(), expected.field_len());
        assert!(environment.field_len() >= 20, "{}", environment.field_len());
        for name in [
            "water_top_speed",
            "water_gravity",
            "glide_slope",
            "glide_turn_rate",
            "anti_gravity_align_rate",
            "spin_boost_strength",
            "spin_boost_duration",
            "wheel_pivot_seconds",
        ] {
            assert!(environment.field(name).is_some(), "{name}");
        }
        assert_eq!(tuning.environment, defaults);
    }

    /// GD § 10: slower and floatier under water. The gravity is weaker and the times allowed in
    /// the air stretch with it; the water brakes harder; the rest is the kart's.
    #[test]
    fn the_water_is_floaty_and_brakes() {
        let base = KartTuning::default();
        let env = EnvironmentTuning::default();
        let water = water_tuning(&base, &env);
        assert!((water.gravity_scale - base.gravity_scale * 0.5).abs() < 1e-6);
        assert_eq!(water.hop_max_ticks, 60);
        assert_eq!(water.drift_max_air_ticks, 60);
        assert_eq!(water.airborne_grace_ticks, 8);
        assert!(water.coast_decel > base.coast_decel);
        assert!(water.overspeed_decel > base.overspeed_decel);
        let unchanged = KartTuning {
            gravity_scale: base.gravity_scale,
            coast_decel: base.coast_decel,
            overspeed_decel: base.overspeed_decel,
            hop_max_ticks: base.hop_max_ticks,
            drift_max_air_ticks: base.drift_max_air_ticks,
            airborne_grace_ticks: base.airborne_grace_ticks,
            ..water.clone()
        };
        assert_eq!(unchanged, base);
        assert!(env.water_top_speed < 1.0);
        // A hop under water (no drag on the way up): about twice as long, still a hop.
        let g = 9.81 * water.gravity_scale;
        let hop_ticks = 2.0 * base.hop_speed / g / DT;
        assert!(hop_ticks < water.hop_max_ticks as f32, "{hop_ticks}");
        // Silly values do not break it.
        let silly = water_tuning(
            &base,
            &EnvironmentTuning {
                water_gravity: 0.0,
                water_drag: -1.0,
                ..env
            },
        );
        assert!(silly.gravity_scale > 0.0 && silly.coast_decel == 0.0);
    }

    /// The glider opens off a glide ramp only, once in the air for a few ticks, and closes on the
    /// ground, under water or with the pelican.
    #[test]
    fn the_glider_opens_off_a_glide_ramp_and_closes_on_the_ground() {
        let t = EnvironmentTuning::default();
        let off_ramp = KartEnvironment {
            since_glide_ramp: 2,
            ..default()
        };
        assert!(!next_gliding(&t, &off_ramp, 0, 2, false, DT), "a bump");
        assert!(next_gliding(&t, &off_ramp, 0, 3, false, DT));
        assert!(!next_gliding(&t, &off_ramp, 1, 3, false, DT));
        let long_ago = KartEnvironment {
            since_glide_ramp: 31,
            ..default()
        };
        assert!(!next_gliding(&t, &long_ago, 0, 40, false, DT));
        assert!(!next_gliding(
            &t,
            &KartEnvironment::default(),
            0,
            40,
            false,
            DT
        ));
        let gliding = KartEnvironment {
            gliding: true,
            since_glide_ramp: 400,
            ..default()
        };
        assert!(
            next_gliding(&t, &gliding, 0, 400, false, DT),
            "it stays open"
        );
        assert!(!next_gliding(&t, &gliding, 2, 0, false, DT), "landed");
        assert!(!next_gliding(&t, &gliding, 0, 400, true, DT), "the pelican");
        let wet = KartEnvironment {
            underwater: true,
            ..gliding
        };
        assert!(!next_gliding(&t, &wet, 0, 400, false, DT));
        let wet = KartEnvironment {
            underwater: true,
            ..default()
        };
        assert_eq!(gliding.terrain(), Terrain::Air);
        assert_eq!(wet.terrain(), Terrain::Water);
        assert_eq!(KartEnvironment::default().terrain(), Terrain::Ground);
    }

    /// Up dives (faster, lower), Down climbs (slower, higher), neither glides level; the jump off
    /// the ramp fades into a slow descent, at the speed of the glide whatever the kart's speed
    /// on the ramp; diving lands sooner than gliding level, which lands sooner than climbing.
    #[test]
    fn the_glide_dives_climbs_and_sinks_slowly() {
        let t = EnvironmentTuning::default();
        let up = KartInput {
            throttle: 1.0,
            ..default()
        };
        let down = KartInput {
            brake: 1.0,
            ..default()
        };
        assert_eq!(glide_pitch(&up), 1.0);
        assert_eq!(glide_pitch(&down), -1.0);
        assert_eq!(glide_pitch(&KartInput::default()), 0.0);
        let (dive, dive_slope) = glide_targets(&t, 1.0);
        let (level, level_slope) = glide_targets(&t, 0.0);
        let (climb, climb_slope) = glide_targets(&t, -1.0);
        assert!(dive > level && level > climb);
        assert!(dive_slope > level_slope && level_slope > climb_slope && climb_slope > 0.0);
        // A flight from a lip 11.7 m above the landing, off a 12° ramp at `speed`, with a top
        // speed of `top` (the kart's: its Speed « air », its croquettes, a boost): how far, at
        // what speeds, and how high after 61 m.
        // Issue #197: the attitude follows the stick (from level as the glider opens).
        let flight = |pitch: f32, speed: f32, top: f32| {
            let (mut forward, mut vertical) = (speed, speed * 12f32.to_radians().sin());
            let (mut height, mut distance, mut ticks) = (11.7f32, 0.0f32, 0u32);
            let mut over_the_gap = f32::MAX;
            let mut attitude = 0.0;
            while height > 0.0 {
                attitude = next_glide_attitude(&t, attitude, pitch, DT);
                (forward, vertical) = glide_speeds(&t, top, forward, vertical, attitude, DT);
                height += vertical * DT;
                distance += forward * DT;
                if distance <= 61.0 {
                    over_the_gap = height;
                }
                ticks += 1;
                assert!(ticks < 60 * 30, "never lands");
            }
            (distance, forward, vertical, over_the_gap)
        };
        for top in [21.4, 28.5, 35.6] {
            let (dive_distance, dive_speed, dive_vertical, cleared) = flight(1.0, 20.0, top);
            let (level_distance, level_speed, level_vertical, _) = flight(0.0, top, top);
            let (climb_distance, ..) = flight(-1.0, top, top);
            assert!(
                dive_distance < level_distance && level_distance < climb_distance,
                "{top}"
            );
            // Issue #197: off the ramp at 20 m/s, a dive (steeper) lands before it reaches its
            // speed at the highest top speeds: it speeds up toward it all along; off the ramp at
            // its top speed, it reaches it.
            assert!(
                dive_speed > 20.0 + 0.6 * (top * dive - 20.0) && dive_speed <= top * dive + 0.01,
                "{dive_speed}"
            );
            let (_, full_dive_speed, full_dive_vertical, _) = flight(1.0, top, top);
            assert!(
                (full_dive_speed - top * dive).abs() < 0.5,
                "{full_dive_speed}"
            );
            assert!((full_dive_vertical + dive_slope * full_dive_speed).abs() < 0.3);
            assert!((level_speed - top * level).abs() < 0.5, "{level_speed}");
            // Still speeding up, the 20 m/s dive comes down steeper than a level glide, and no
            // steeper than its dive.
            assert!(
                dive_vertical < -level_slope * dive_speed
                    && dive_vertical > -dive_slope * dive_speed - 0.3,
                "{top}: {dive_vertical}"
            );
            let level_sink = (level_slope * level_speed).max(t.glide_min_sink);
            assert!((level_vertical + level_sink).abs() < 0.3);
            // Even a full dive off the ramp at 20 m/s, at any top speed, is still 3 m up after the
            // 61 m of the lagoon's gap; a level glide lands within 200 m, a climb within 280 m.
            assert!(cleared > 3.0, "{top}: {cleared}");
            assert!(level_distance < 200.0, "{top}: {level_distance}");
            assert!(climb_distance < 280.0, "{top}: {climb_distance}");
            // A kart that rolled off the ramp slowly still flies on.
            let (slow_distance, slow_speed, ..) = flight(0.0, 6.0, top);
            assert!(slow_distance > 61.0 && (slow_speed - top * level).abs() < 0.5);
        }
        // Issue #118: the fastest a kart can glide (the highest Speed « air » of the stats, 1.08,
        // 10 croquettes, a purple mini-turbo: 28.5 × 1.08 × 1.1 × 1.3 = 44 m/s) still clears the
        // gap diving off the ramp at 20 m/s (too short a flight to reach its diving speed), lands
        // within 200 m gliding level and within 280 m climbing.
        let top = 44.0;
        let (dive_distance, _, _, cleared) = flight(1.0, 20.0, top);
        let (level_distance, level_speed, ..) = flight(0.0, top, top);
        let (climb_distance, ..) = flight(-1.0, top, top);
        assert!(dive_distance < level_distance && level_distance < climb_distance);
        assert!((level_speed - top * level).abs() < 0.5, "{level_speed}");
        assert!(cleared > 3.0, "{top}: {cleared}");
        assert!(level_distance < 200.0, "{top}: {level_distance}");
        assert!(climb_distance < 280.0, "{top}: {climb_distance}");
        // Issue #197: the player chooses where it lands, the three well apart, at every top
        // speed: diving lands within 70 % of a level glide's way, climbing beyond 130 % of it.
        for top in [21.4, 28.5, 35.6, 44.0] {
            let (dive, ..) = flight(1.0, top, top);
            let (level, ..) = flight(0.0, top, top);
            let (climb, ..) = flight(-1.0, top, top);
            assert!(dive < 0.7 * level, "{top}: dive {dive}, level {level}");
            assert!(climb > 1.3 * level, "{top}: climb {climb}, level {level}");
        }
        // However slow, a glide comes down.
        let (_, vertical) = glide_speeds(&t, 5.0, 2.0, -0.1, -1.0, 10.0);
        assert!((vertical + t.glide_min_sink).abs() < 1e-3, "{vertical}");
        // The stick turns it, the Handling « air » more or less.
        assert!(
            glide_yaw_target(&t, 1.0, 1.0) < 0.0,
            "right: a negative yaw"
        );
        assert!(glide_yaw_target(&t, -1.0, 1.1) > glide_yaw_target(&t, -1.0, 1.0));
        assert_eq!(
            glide_yaw_target(&t, 3.0, 1.0),
            glide_yaw_target(&t, 1.0, 1.0)
        );
    }

    /// Issue #197: the attitude follows the stick in 0.3 s, from level to a full dive or climb
    /// (1 s from a full dive to a full climb at most), never past it; the speed and the slope
    /// follow it, so nothing changes at once.
    #[test]
    fn the_glider_attitude_follows_the_stick_in_a_third_of_a_second() {
        let t = EnvironmentTuning::default();
        let ticks_to = |from: f32, pitch: f32| {
            let (mut attitude, mut ticks) = (from, 0);
            while (attitude - pitch).abs() > 1e-4 {
                let next = next_glide_attitude(&t, attitude, pitch, DT);
                assert!((next - attitude).abs() <= t.glide_attitude_rate * DT + 1e-6);
                assert!((pitch - next) * (pitch - attitude) >= 0.0, "past the stick");
                attitude = next;
                ticks += 1;
                assert!(ticks < 600);
            }
            ticks
        };
        for pitch in [1.0, -1.0] {
            let ticks = ticks_to(0.0, pitch);
            assert!((15..=21).contains(&ticks), "{pitch}: {ticks} ticks");
        }
        assert!(ticks_to(1.0, -1.0) <= 60);
        assert!(ticks_to(0.5, 0.5) == 0);
        assert_eq!(next_glide_attitude(&t, 3.0, 9.0, DT), 1.0);
        // Half a tick of the stick moves the speed and the slope by a little only.
        let (s0, k0) = glide_targets(&t, 0.0);
        let (s1, k1) = glide_targets(&t, next_glide_attitude(&t, 0.0, 1.0, DT));
        assert!((s1 - s0).abs() < 0.03 && (k1 - k0).abs() < 0.01);
    }

    /// Issue #197: climbing faster than the climb's speed (off a dive), the glider trades its
    /// speed for height: once the dive's descent is stopped, it rises a little, for a short while
    /// (bounded: more than 0.5 m, less than 3 m, for less than 2 s), then comes down slowly, at the
    /// climb's slope. Climbing at the climb's speed,
    /// it never rises. Diving never rises.
    #[test]
    fn climbing_fast_trades_speed_for_a_bounded_rise() {
        let t = EnvironmentTuning::default();
        for top in [21.4f32, 28.5, 35.6, 44.0] {
            // Settled in a full dive, then Down held.
            let (dive_share, dive_slope) = glide_targets(&t, 1.0);
            let (mut forward, mut vertical) = (top * dive_share, -dive_slope * top * dive_share);
            let mut attitude = 1.0;
            let (mut height, mut lowest, mut rise, mut rising) = (0.0f32, 0.0f32, 0.0f32, 0u32);
            for _ in 0..(6 * 60) {
                attitude = next_glide_attitude(&t, attitude, -1.0, DT);
                (forward, vertical) = glide_speeds(&t, top, forward, vertical, attitude, DT);
                assert!(vertical <= t.glide_climb_max_rise + 1e-4);
                height += vertical * DT;
                lowest = lowest.min(height);
                rise = rise.max(height - lowest);
                rising += u32::from(vertical > 0.0);
            }
            assert!(rise > 0.5, "{top}: rose {rise} m");
            assert!(rise < 3.0, "{top}: rose {rise} m");
            assert!(rising < 2 * 60, "{top}: rose for {rising} ticks");
            let (climb_share, climb_slope) = glide_targets(&t, -1.0);
            assert!((forward - top * climb_share).abs() < 0.1, "{forward}");
            let sink = (climb_slope * forward).max(t.glide_min_sink);
            assert!((vertical + sink).abs() < 0.05, "{top}: {vertical}");
            // At the climb's speed, no rise; diving, no rise.
            for (attitude, share) in [(-1.0, climb_share), (1.0, dive_share)] {
                let (_, vertical) = glide_speeds(&t, top, top * share, -1.0, attitude, DT * 120.0);
                assert!(vertical < 0.0, "{top} at {attitude}: {vertical}");
            }
        }
    }

    /// Shown only: nose down diving, up climbing or rising off the ramp, level in a level glide;
    /// banked into the turns.
    #[test]
    fn the_glider_pitches_and_banks_on_screen() {
        let t = EnvironmentTuning::default();
        let (level, _) = glide_pose(&t, Vec3::new(0.0, -t.glide_slope * 27.0, -27.0), 0.0);
        assert!(level.abs() < 1e-5);
        let (dive, _) = glide_pose(&t, Vec3::new(0.0, -t.glide_dive_slope * 34.0, -34.0), 0.0);
        assert!((dive + t.glide_pitch).abs() < 1e-5, "{dive}");
        let (rise, _) = glide_pose(&t, Vec3::new(0.0, 6.0, -28.0), 0.0);
        assert_eq!(rise, t.glide_pitch);
        // Issue #197: fully up at the climb's slope (each way against its own slope), halfway
        // up halfway there.
        let (climb, _) = glide_pose(&t, Vec3::new(0.0, -t.glide_climb_slope * 20.0, -20.0), 0.0);
        assert!((climb - t.glide_pitch).abs() < 1e-5, "{climb}");
        let half = (t.glide_slope + t.glide_climb_slope) / 2.0;
        let (half, _) = glide_pose(&t, Vec3::new(0.0, -half * 20.0, -20.0), 0.0);
        assert!((half - t.glide_pitch / 2.0).abs() < 1e-4, "{half}");
        let (half, _) = glide_pose(
            &t,
            Vec3::new(
                0.0,
                -(t.glide_slope + t.glide_dive_slope) / 2.0 * 30.0,
                -30.0,
            ),
            0.0,
        );
        assert!((half + t.glide_pitch / 2.0).abs() < 1e-4, "{half}");
        let (_, right) = glide_pose(&t, Vec3::NEG_Z, -t.glide_turn_rate);
        assert_eq!(right, t.glide_bank);
        let (_, left) = glide_pose(&t, Vec3::NEG_Z, 5.0);
        assert_eq!(left, -t.glide_bank);
    }
}
