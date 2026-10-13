//! The arcade kart (GD § 2, docs/ARCHITECTURE.md § « Le kart »).
//!
//! Rapier handles collisions and gravity; the feel comes from our own curves. Every fixed tick:
//! boost timers → top speed factors → boost pads and slipstream → 4 suspension raycasts →
//! driving state (hop, drift, trick) → engine, grip and steering written to the velocity; after
//! the physics step, the bumps between karts. The kart only reads its `KartInput`, never the
//! keyboard.
//!
//! Every boost source (mini-turbo, rocket start, trick, slipstream, boost pad, croquette, later
//! the items) goes through the one `Boost` component (GD § 2). Every top speed multiplier
//! (bots' catch-up, croquettes, later the stats) goes through `MaxSpeedFactors`.

mod bump;
mod drift;
mod input;
mod physics;
mod rocket;
mod slipstream;
mod trick;
mod tuning;
mod visual;

pub use bump::{KartBump, StepVelocity, bump_victims, closing_speed};
pub use drift::{DriftMotion, DriftSide, MiniTurbo, drift_yaw_rate, slip_pose};
pub use input::{DEBUG_RESET_KEY, KartInputPlugin, PlayerKart, SpawnPoint};
pub use physics::{Boost, KartState, max_speed, target_yaw_rate};
pub use rocket::{
    Stall, StartOutcome, StartPress, THROTTLE_PRESSED, rocket_start_duration, start_outcome,
};
pub use slipstream::{BOOST_PAD_REACH, Slipstream, fast_enough_for_slipstream, in_wake, over_pad};
pub use trick::TrickTracker;
pub use tuning::KartTuning;
pub use visual::{
    BoostFlame, DriftSpark, KartBodyPivot, KartVisualAssets, KartVisualPlugin, StallSmoke,
    WindLine, flame_size, spawn_kart_visual,
};

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::interpolation::PhysicsPose;
use crate::track::{BoostPad, Surface, TrickRamp};
use drift::{StateContext, mini_turbo_boost, step_state};
use physics::{
    approach_yaw_rate, next_forward_speed, next_lateral_speed, next_state, next_yaw_rate,
    righting_angular_velocity, suspension_accel,
};

/// What a driver (player, bot, network) asks the kart to do this tick.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct KartInput {
    /// Accelerator, 0 to 1.
    pub throttle: f32,
    /// Brake / reverse, 0 to 1 (GD § 2: one button for both).
    pub brake: f32,
    /// -1 (full left) to 1 (full right).
    pub steer: f32,
    /// Jump / drift / trick button, held: pressing it hops, holding it drifts, releasing it
    /// fires the mini-turbo (GD § 3, `drift.rs`).
    pub jump_drift: bool,
    /// Item button, held = drag the item behind (used by issue #16).
    pub item: bool,
    /// Look behind (used by the camera).
    pub look_back: bool,
}

/// A kart's simulated body. Spawn it with [`kart_body`].
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(
    KartInput,
    PreviousInput,
    KartAssists,
    KartState,
    DriftMotion,
    Boost,
    KartWheels,
    MaxSpeedScale,
    MaxSpeedFactors,
    StepVelocity,
    Stall,
    TrickTracker,
    Slipstream
)]
pub struct Kart;

/// The kart's input on the previous tick: a hop starts when the jump/drift button is
/// *pressed*, not every tick while it stays held.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PreviousInput(pub KartInput);

/// Driving aids of this kart's driver (GD § 11), set per player. Off by default; the options
/// menu comes with issue #17.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KartAssists {
    /// Assisted steering: the drift charges slower (2 and 0.8 units per tick) and the purple
    /// mini-turbo is disabled (GD § 3.3 and 3.6). Keeping the kart on the track is #17.
    pub assisted_steering: bool,
}

/// Multiplier on this kart's top speed, boosted or not (1 = unchanged): what the driving, the
/// slipstream and the bots read. It is the product of the kart's [`MaxSpeedFactors`], refreshed
/// before the karts drive whenever one of them changes: write the factors, not this.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MaxSpeedScale(pub f32);

impl Default for MaxSpeedScale {
    fn default() -> Self {
        Self(1.0)
    }
}

/// The multipliers of a kart's top speed, one per source (1 = unchanged). Each source writes
/// only its own; [`MaxSpeedScale`] is their product. The stats (issue #15) will add theirs.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MaxSpeedFactors {
    /// The bots' light catch-up (GD § 12, issue #6): within ±3 %.
    pub catch_up: f32,
    /// The croquettes held (GD § 7, issue #14): +1 % each, up to 10.
    pub croquettes: f32,
}

impl Default for MaxSpeedFactors {
    fn default() -> Self {
        Self {
            catch_up: 1.0,
            croquettes: 1.0,
        }
    }
}

impl MaxSpeedFactors {
    pub fn product(&self) -> f32 {
        self.catch_up * self.croquettes
    }
}

/// Half size of the kart's collision box: 1.4 m wide, 0.5 m high, 2 m long (GD § 2).
pub const KART_HALF_EXTENTS: Vec3 = Vec3::new(0.7, 0.25, 1.0);

/// Wheel anchors (where the suspension rays start), in the kart's local frame:
/// front left, front right, rear left, rear right.
pub const WHEEL_ANCHORS: [Vec3; 4] = [
    Vec3::new(-0.55, -0.1, -0.75),
    Vec3::new(0.55, -0.1, -0.75),
    Vec3::new(-0.55, -0.1, 0.75),
    Vec3::new(0.55, -0.1, 0.75),
];

/// Suspension and ground contact, refreshed every tick (read by visuals and tests).
#[derive(Component, Clone, Copy, Debug)]
pub struct KartWheels {
    /// Spring compression per wheel in meters, 0 when the wheel is in the air.
    pub compression: [f32; 4],
    /// Whether each wheel touches the ground.
    pub on_ground: [bool; 4],
    /// Consecutive ticks without any wheel on the ground.
    pub ticks_without_ground: u32,
    /// Share of the grounded wheels on an off-road surface (0 to 1).
    pub offroad_ratio: f32,
    /// Average ground normal under the wheels (last known one in the air).
    pub ground_normal: Vec3,
}

impl Default for KartWheels {
    fn default() -> Self {
        Self {
            compression: [0.0; 4],
            on_ground: [false; 4],
            ticks_without_ground: 0,
            offroad_ratio: 0.0,
            ground_normal: Vec3::Y,
        }
    }
}

impl KartWheels {
    pub fn wheels_on_ground(&self) -> usize {
        self.on_ground.iter().filter(|on| **on).count()
    }
}

/// Order of the kart systems inside `FixedUpdate`, all before the Rapier step.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum KartSystems {
    /// Boost and stall timers count down, before anything gives a boost this tick: a boost
    /// given later in the tick (race start, pad, mini-turbo...) keeps its full duration.
    Timers,
    /// Input sources write `KartInput` (player, bots, replays).
    Input,
    /// Top speed factors, boost pads, slipstream, suspension, state machine, engine, grip,
    /// steering.
    Drive,
    /// After the physics step: the bumps between karts of that step ([`KartBump`]).
    Bumps,
}

pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KartTuning>()
            .add_message::<KartBump>()
            .configure_sets(
                FixedUpdate,
                (
                    (KartSystems::Timers, KartSystems::Input, KartSystems::Drive)
                        .chain()
                        .before(PhysicsSet::SyncBackend),
                    KartSystems::Bumps.after(PhysicsSet::Writeback),
                ),
            )
            .add_systems(FixedUpdate, tick_timers.in_set(KartSystems::Timers))
            .add_systems(
                FixedUpdate,
                (
                    combine_max_speed_factors,
                    boost_pads,
                    charge_slipstreams,
                    drive_karts,
                    record_step_velocities,
                )
                    .chain()
                    .in_set(KartSystems::Drive),
            )
            .add_systems(FixedUpdate, detect_bumps.in_set(KartSystems::Bumps));
    }
}

/// Everything a kart body needs: physics, driving components and pose recording for the
/// visual interpolation. Add `PlayerKart` (or a bot / network driver) and a visual on top.
pub fn kart_body(tuning: &KartTuning, transform: Transform) -> impl Bundle {
    let rounding = 0.1;
    let half = KART_HALF_EXTENTS - Vec3::splat(rounding);
    (
        Kart,
        transform,
        PhysicsPose::new(transform),
        RigidBody::Dynamic,
        Collider::round_cuboid(half.x, half.y, half.z, rounding),
        ColliderMassProperties::MassProperties(MassProperties {
            local_center_of_mass: Vec3::Y * tuning.center_of_mass_height,
            mass: tuning.mass,
            principal_inertia: tuning.inertia,
            ..default()
        }),
        Velocity::default(),
        ExternalForce::default(),
        GravityScale(tuning.gravity_scale),
        // Slide along walls and the ground instead of sticking to them.
        Friction {
            coefficient: 0.0,
            combine_rule: CoefficientCombineRule::Min,
        },
        // The kart itself has no bounce; with `Max`, a contact bounces exactly as much as the
        // wall or obstacle says (`track::WALL_RESTITUTION`), and not at all on the ground.
        Restitution {
            coefficient: 0.0,
            combine_rule: CoefficientCombineRule::Max,
        },
        Ccd::enabled(),
        Sleeping::disabled(),
    )
}

fn tick_timers(time: Res<Time>, mut boosts: Query<&mut Boost>, mut stalls: Query<&mut Stall>) {
    let dt = time.delta_secs();
    for mut boost in &mut boosts {
        if boost.is_active() {
            boost.tick(dt);
        }
    }
    for mut stall in &mut stalls {
        if stall.is_active() {
            stall.tick(dt);
        }
    }
}

/// `MaxSpeedScale` = the product of the `MaxSpeedFactors`, for the karts whose factors changed
/// (their sources wrote them earlier in the tick).
fn combine_max_speed_factors(
    mut karts: Query<(&MaxSpeedFactors, &mut MaxSpeedScale), Changed<MaxSpeedFactors>>,
) {
    for (factors, mut scale) in &mut karts {
        scale.set_if_neq(MaxSpeedScale(factors.product()));
    }
}

/// The velocity each kart hands to the physics step, before its collisions (`bump.rs`).
fn record_step_velocities(mut karts: Query<(&Velocity, &mut StepVelocity), With<Kart>>) {
    for (velocity, mut step) in &mut karts {
        step.0 = velocity.linear;
    }
}

/// After the physics step: every pair of karts the solver pushed apart in that step, hitting
/// each other fast enough, is a bump (GD § 7, `bump.rs`). Read from Rapier's contact graph
/// (each kart's own contact pairs, a few per kart): no extra physics query.
fn detect_bumps(
    tuning: Res<KartTuning>,
    rapier: ReadRapierContext,
    karts: Query<(Entity, &Transform, &StepVelocity, &KartState), With<Kart>>,
    mut bumps: MessageWriter<KartBump>,
) {
    let Ok(context) = rapier.single() else {
        return;
    };
    for (a, a_transform, a_velocity, a_state) in &karts {
        if *a_state == KartState::Respawning {
            continue;
        }
        for pair in context.contact_pairs_with(a) {
            let (Some(c1), Some(c2)) = (pair.collider1(), pair.collider2()) else {
                continue;
            };
            let b = if c1 == a { c2 } else { c1 };
            // Each pair once; only karts.
            if b <= a {
                continue;
            }
            let Ok((_, b_transform, b_velocity, b_state)) = karts.get(b) else {
                continue;
            };
            if *b_state == KartState::Respawning
                || !pair.has_any_active_contact()
                || pair.total_impulse_magnitude() <= 0.0
            {
                continue;
            }
            // The contact normal, pointing from A toward B.
            let between = b_transform.translation - a_transform.translation;
            let normal = pair
                .manifolds()
                .next()
                .map(|manifold| manifold.normal())
                .unwrap_or(between);
            let normal = if normal.dot(between) < 0.0 {
                -normal
            } else {
                normal
            };
            let Some((a_bumped, b_bumped)) =
                bump_victims(&tuning, a_velocity.0, b_velocity.0, normal)
            else {
                continue;
            };
            let closing_speed = closing_speed(a_velocity.0, b_velocity.0, normal);
            if a_bumped {
                bumps.write(KartBump {
                    kart: a,
                    by: b,
                    closing_speed,
                });
            }
            if b_bumped {
                bumps.write(KartBump {
                    kart: b,
                    by: a,
                    closing_speed,
                });
            }
        }
    }
}

/// Boost pads (GD § 2): a wheel touching the ground over a pad (last tick's contacts) gets its
/// boost, refreshed every tick on it.
fn boost_pads(
    tuning: Res<KartTuning>,
    pads: Query<(&Transform, &BoostPad)>,
    mut karts: Query<(&Transform, &KartWheels, &KartState, &mut Boost), With<Kart>>,
) {
    if pads.is_empty() {
        return;
    }
    for (transform, wheels, state, mut boost) in &mut karts {
        if *state == KartState::Respawning {
            continue;
        }
        let on_pad = WHEEL_ANCHORS
            .iter()
            .zip(wheels.on_ground)
            .filter(|(_, grounded)| *grounded)
            .any(|(anchor, _)| {
                let point = transform.transform_point(*anchor);
                pads.iter()
                    .any(|(pad, size)| over_pad(pad, size.half_size, point))
            });
        if on_pad {
            boost.add(tuning.boost_pad_strength, tuning.boost_pad_duration);
        }
    }
}

/// Slipstream (GD § 6): the gauge of every kart in another kart's wake, fast enough, charges;
/// full, it fires the slipstream boost. The karts are gathered once in a buffer kept between
/// ticks (no allocation per tick).
fn charge_slipstreams(
    tuning: Res<KartTuning>,
    time: Res<Time>,
    mut leaders: Local<Vec<(Entity, Vec3, Vec3)>>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &Velocity,
            &KartState,
            &MaxSpeedScale,
            &mut Slipstream,
            &mut Boost,
        ),
        With<Kart>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    leaders.clear();
    leaders.extend(
        karts
            .iter()
            .filter(|(_, _, _, state, ..)| **state != KartState::Respawning)
            .map(|(entity, transform, ..)| (entity, transform.translation, *transform.forward())),
    );
    for (entity, transform, velocity, state, scale, mut slipstream, mut boost) in &mut karts {
        let mut gauge = *slipstream;
        let full = if *state == KartState::Respawning {
            gauge = Slipstream::default();
            false
        } else {
            let forward_speed = velocity.linear.dot(*transform.forward());
            let fast = fast_enough_for_slipstream(&tuning, forward_speed, scale.0);
            let wake = fast
                && leaders.iter().any(|(other, at, heading)| {
                    *other != entity && in_wake(&tuning, transform.translation, *at, *heading)
                });
            gauge.step(&tuning, wake, fast, dt)
        };
        slipstream.set_if_neq(gauge);
        if full {
            boost.add(
                tuning.slipstream_boost_strength,
                tuning.slipstream_boost_duration,
            );
        }
    }
}

fn drive_karts(
    tuning: Res<KartTuning>,
    time: Res<Time>,
    rapier: ReadRapierContext,
    surfaces: Query<&Surface>,
    ramps: Query<(), With<TrickRamp>>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &KartInput,
            &mut PreviousInput,
            &KartAssists,
            &MaxSpeedScale,
            &Stall,
            &mut TrickTracker,
            &mut Boost,
            &mut KartState,
            &mut DriftMotion,
            &mut KartWheels,
            &mut Velocity,
            &mut ExternalForce,
            &mut GravityScale,
        ),
        With<Kart>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let Ok(context) = rapier.single() else {
        return;
    };
    let tuning = &*tuning;
    // The trick window in ticks (GD § 5: about 0.2 s).
    let trick_window = (tuning.trick_window / dt).round() as u32;

    for (
        entity,
        transform,
        input,
        mut previous_input,
        assists,
        speed_scale,
        stall,
        mut tricks,
        mut boost,
        mut state,
        mut motion,
        mut wheels,
        mut velocity,
        mut force,
        mut gravity,
    ) in &mut karts
    {
        if *state == KartState::Respawning {
            if *motion != DriftMotion::default() {
                *motion = DriftMotion::default();
            }
            // Carried by the tow pelican: no suspension, engine or gravity; the race rules move
            // the body (issue #5).
            force.force = Vec3::ZERO;
            force.torque = Vec3::ZERO;
            if velocity.linear != Vec3::ZERO || velocity.angular != Vec3::ZERO {
                *velocity = Velocity::zero();
            }
            if gravity.0 != 0.0 {
                gravity.0 = 0.0;
            }
            *wheels = KartWheels::default();
            tricks.set_if_neq(TrickTracker::default());
            previous_input.set_if_neq(PreviousInput(*input));
            continue;
        }
        // A stalled engine (rocket start pressed too early, GD § 4) ignores the accelerator,
        // the brake and the jump button; the button edges still follow the real input.
        let driver = *input;
        let effective = if stall.is_active() {
            KartInput {
                steer: driver.steer,
                look_back: driver.look_back,
                ..default()
            }
        } else {
            driver
        };
        let input = &effective;

        let up = *transform.up();
        let forward = *transform.forward();
        let center_of_mass = transform.transform_point(Vec3::Y * tuning.center_of_mass_height);
        let filter = QueryFilter::default()
            .exclude_rigid_body(entity)
            .exclude_sensors();

        // --- Suspension: one ray per wheel, spring + damper pushing along the kart's up axis.
        let mut total_force = Vec3::ZERO;
        let mut total_torque = Vec3::ZERO;
        let mut normal_sum = Vec3::ZERO;
        let mut grounded = 0;
        let mut offroad = 0;
        let mut on_trick_ramp = false;
        for (i, anchor) in WHEEL_ANCHORS.iter().enumerate() {
            let origin = transform.transform_point(*anchor);
            let hit = context.cast_ray_and_get_normal(
                origin,
                -up,
                tuning.suspension_rest_length,
                true,
                filter,
            );
            let Some((hit_entity, hit)) =
                hit.filter(|(_, hit)| hit.normal.y >= tuning.min_ground_normal_y)
            else {
                wheels.compression[i] = 0.0;
                wheels.on_ground[i] = false;
                continue;
            };
            let compression = tuning.suspension_rest_length - hit.time_of_impact;
            let lever = origin - center_of_mass;
            let anchor_velocity = velocity.linear + velocity.angular.cross(lever);
            let wheel_force =
                up * suspension_accel(tuning, compression, anchor_velocity.dot(up)) * tuning.mass;
            total_force += wheel_force;
            total_torque += lever.cross(wheel_force);
            normal_sum += hit.normal;
            grounded += 1;
            if surfaces.get(hit_entity) == Ok(&Surface::Offroad) {
                offroad += 1;
            }
            on_trick_ramp |= ramps.contains(hit_entity);
            wheels.compression[i] = compression;
            wheels.on_ground[i] = true;
        }
        force.force = total_force;
        force.torque = total_torque;

        // --- State machine: ground contact, hop, drift (`drift::step_state`).
        if grounded > 0 {
            wheels.ticks_without_ground = 0;
            wheels.offroad_ratio = offroad as f32 / grounded as f32;
            wheels.ground_normal = normal_sum.normalize_or(Vec3::Y);
        } else {
            wheels.ticks_without_ground = wheels.ticks_without_ground.saturating_add(1);
        }
        let normal = wheels.ground_normal;
        let ahead = (forward - normal * forward.dot(normal)).normalize_or(forward);
        let right = ahead.cross(normal);
        let mut linvel = velocity.linear;
        let angvel = velocity.angular;

        // Tricks (GD § 5): takeoff from a trick ramp, and the button pressed around it.
        let pressed = input.jump_drift && !previous_input.0.jump_drift;
        let mut tracker = *tricks;
        tracker.step(on_trick_ramp, grounded, pressed);
        tricks.set_if_neq(tracker);

        let step = step_state(
            tuning,
            *state,
            &StateContext {
                wheels_on_ground: grounded,
                ticks_without_ground: wheels.ticks_without_ground,
                forward_speed: linvel.dot(ahead),
                input: *input,
                jump_was_held: previous_input.0.jump_drift,
                assisted: assists.assisted_steering,
                on_trick_ramp,
                trick_ready: tracker.trick_ready(trick_window),
            },
        );
        if *state != step.state {
            *state = step.state;
        }
        previous_input.set_if_neq(PreviousInput(driver));
        if let Some(tier) = step.boost {
            let (strength, duration) = mini_turbo_boost(tuning, tier);
            boost.add(strength, duration);
        }
        if step.trick_landed {
            boost.add(tuning.trick_boost_strength, tuning.trick_boost_duration);
        }
        if step.hop {
            // Hop: at least `hop_speed` along the ground normal (never stacks on a bump).
            linvel += normal * (tuning.hop_speed - linvel.dot(normal)).max(0.0);
        }
        if gravity.0 != tuning.gravity_scale {
            gravity.0 = tuning.gravity_scale;
        }
        // The drift's curve and slip set in and fade out progressively (issue #30), from the
        // rotation the kart has now.
        let mut drift_motion = *motion;
        drift_motion.step(tuning, &state, angvel.dot(normal), input.steer, dt);
        motion.set_if_neq(drift_motion);

        // --- Driving: rewrite the velocity from our curves. A hop keeps the ground handling
        // (full steering, engine) for its short flight; a drift in the air (longer than the
        // grace period) gets the air control.
        let on_ground = next_state(
            grounded,
            wheels.ticks_without_ground,
            tuning.airborne_grace_ticks,
        ) == KartState::Grounded;
        let ground_handling = match *state {
            KartState::Grounded | KartState::Hop { .. } => true,
            KartState::Drifting { .. } => on_ground,
            KartState::Airborne | KartState::Trick | KartState::Respawning => false,
        };
        if ground_handling {
            let forward_speed = linvel.dot(ahead);
            let top_speed =
                max_speed(tuning, wheels.offroad_ratio, &boost) * speed_scale.0.max(0.0);
            let new_forward = next_forward_speed(
                tuning,
                forward_speed,
                input,
                top_speed,
                boost.is_active(),
                dt,
            );
            let new_lateral = next_lateral_speed(tuning, linvel.dot(right), dt);

            let yaw = angvel.dot(normal);
            let tilt = angvel - normal * yaw;
            // A drift always turns toward its side; the steering only tightens or widens it.
            // Its curve sets in from the rotation the kart had on landing and fades back to the
            // normal steering after it (`DriftMotion`, issue #30).
            let steering = target_yaw_rate(tuning, input.steer, new_forward);
            let target = match drift_motion.side {
                Some(side) => drift_motion.yaw_target(
                    steering,
                    drift_yaw_rate(tuning, side, input.steer, new_forward),
                ),
                None => steering,
            };
            // In the air of a hop the kart mostly keeps its rotation: the stick acts slower.
            let response = match *state {
                KartState::Hop { .. } => tuning.hop_steer_response,
                _ => tuning.steer_response,
            };
            let new_yaw = approach_yaw_rate(yaw, target, response, dt);
            // A drift slides around the curve without losing speed: the velocity aims where
            // the heading will be at the end of this step, so the grip finds no sideways speed
            // to scrub on the next tick. The normal steering keeps the plain grip model, which
            // scrubs a little speed in tight turns. In between (the drift setting in or fading
            // out), the same blend as the curve: the direction of travel never jumps.
            let travel = Quat::from_axis_angle(normal, drift_motion.blend() * new_yaw * dt) * ahead;
            velocity.linear =
                travel * new_forward + right * new_lateral + normal * linvel.dot(normal);
            velocity.angular = normal * new_yaw + tilt * (-tuning.tilt_damping * dt).exp();
        } else {
            // A little air control, and turn back upright to land on the wheels.
            velocity.linear = linvel;
            let yaw = angvel.dot(Vec3::Y);
            let tilt = angvel - Vec3::Y * yaw;
            let target =
                target_yaw_rate(tuning, input.steer, linvel.dot(forward)) * tuning.air_steer_factor;
            let righting = righting_angular_velocity(up, Vec3::Y, tuning.righting_rate);
            let blend = 1.0 - (-tuning.air_rotation_response * dt).exp();
            velocity.angular =
                Vec3::Y * next_yaw_rate(tuning, yaw, target, dt) + tilt + (righting - tilt) * blend;
        }
    }
}

#[cfg(test)]
mod scenario_tests;
