//! The arcade kart (GD § 2, docs/ARCHITECTURE.md § « Le kart »).
//!
//! Rapier handles collisions and gravity; the feel comes from our own curves. Every fixed tick:
//! 4 suspension raycasts → driving state → engine, grip and steering written to the velocity.
//! The kart only reads its `KartInput`, never the keyboard.

mod drift;
mod input;
mod physics;
mod tuning;
mod visual;

pub use drift::{DriftSide, MiniTurbo};
pub use input::{KartInputPlugin, PlayerKart, SpawnPoint};
pub use physics::{Boost, KartState};
pub use tuning::KartTuning;
pub use visual::{DriftSpark, KartBodyPivot, KartVisualPlugin, spawn_kart_visual};

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::interpolation::PhysicsPose;
use crate::track::Surface;
use drift::{StateContext, drift_yaw_rate, mini_turbo_boost, step_state};
use physics::{
    max_speed, next_forward_speed, next_lateral_speed, next_state, next_yaw_rate,
    righting_angular_velocity, suspension_accel, target_yaw_rate,
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
#[require(KartInput, PreviousInput, KartAssists, KartState, Boost, KartWheels)]
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
    /// Input sources write `KartInput` (player, bots, replays).
    Input,
    /// Suspension, state machine, engine, grip, steering.
    Drive,
}

pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KartTuning>()
            .configure_sets(
                FixedUpdate,
                (KartSystems::Input, KartSystems::Drive)
                    .chain()
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(
                FixedUpdate,
                (tick_boosts, drive_karts)
                    .chain()
                    .in_set(KartSystems::Drive),
            );
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

fn tick_boosts(time: Res<Time>, mut boosts: Query<&mut Boost>) {
    let dt = time.delta_secs();
    for mut boost in &mut boosts {
        if boost.is_active() {
            boost.tick(dt);
        }
    }
}

fn drive_karts(
    tuning: Res<KartTuning>,
    time: Res<Time>,
    rapier: ReadRapierContext,
    surfaces: Query<&Surface>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &KartInput,
            &mut PreviousInput,
            &KartAssists,
            &mut Boost,
            &mut KartState,
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

    for (
        entity,
        transform,
        input,
        mut previous_input,
        assists,
        mut boost,
        mut state,
        mut wheels,
        mut velocity,
        mut force,
        mut gravity,
    ) in &mut karts
    {
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
            },
        );
        if *state != step.state {
            *state = step.state;
        }
        previous_input.set_if_neq(PreviousInput(*input));
        if let Some(tier) = step.boost {
            let (strength, duration) = mini_turbo_boost(tuning, tier);
            boost.add(strength, duration);
        }
        if step.hop {
            // Hop: at least `hop_speed` along the ground normal (never stacks on a bump).
            linvel += normal * (tuning.hop_speed - linvel.dot(normal)).max(0.0);
        }
        if gravity.0 != tuning.gravity_scale {
            gravity.0 = tuning.gravity_scale;
        }

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
            KartState::Airborne => false,
        };
        if ground_handling {
            let forward_speed = linvel.dot(ahead);
            let top_speed = max_speed(tuning, wheels.offroad_ratio, &boost);
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
            let target = match *state {
                KartState::Drifting { side, .. } => {
                    drift_yaw_rate(tuning, side, input.steer, new_forward)
                }
                _ => target_yaw_rate(tuning, input.steer, new_forward),
            };
            let new_yaw = next_yaw_rate(tuning, yaw, target, dt);
            // A drift slides around the curve without losing speed: the velocity aims where
            // the heading will be at the end of this step, so the grip finds no sideways speed
            // to scrub on the next tick. The normal steering keeps the plain grip model, which
            // scrubs a little speed in tight turns.
            let travel = match *state {
                KartState::Drifting { .. } => Quat::from_axis_angle(normal, new_yaw * dt) * ahead,
                _ => ahead,
            };
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
