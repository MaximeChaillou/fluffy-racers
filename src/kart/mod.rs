//! The arcade kart (GD § 2, docs/ARCHITECTURE.md § « Le kart »).
//!
//! Rapier handles collisions and gravity; the feel comes from our own curves. Every fixed tick:
//! boost timers → top speed factors → boost pads and slipstream → 4 suspension raycasts →
//! driving state (hop, drift, trick) → engine, grip and steering written to the velocity; after
//! the physics step, the turn a contact between karts gave is mostly taken back (issue #154,
//! `soften_kart_contacts`), then the bumps between karts. The kart only reads its `KartInput`,
//! never the keyboard.
//!
//! The interactions between karts (GD § 15, issue #34, `crate::interactions`) reach the driving
//! through two components (`push.rs`): the sideways push of a paw strike ([`Shove`]), kept apart
//! from the grip, and the pull of a tether ([`TetherPull`]).
//!
//! Every boost source (mini-turbo, rocket start, trick, slipstream, boost pad, croquette, piment)
//! goes through the one `Boost` component (GD § 2). Hit by an item (issue #16), the kart spins
//! out or tumbles (`KartState::Hit`: deaf to its driver, slowing down hard), then starts its
//! invincibility. Every top speed multiplier (bots' catch-up, croquettes, stats, a tether
//! holding the kart, the items of phase 2, the environment of issue #20) goes through
//! `MaxSpeedFactors`.
//!
//! The items of phase 2 (issue #18, `effects.rs`) leave their effects on the kart
//! ([`ItemEffects`]: super câlin, fusée, dégonflage, chantilly), counted down with the boosts; a
//! fusée drives the kart by its [`Autopilot`] instead of the driver.
//!
//! The stats of the kart (GD § 9, issue #15, `stats.rs`) are converted once when it spawns into
//! multipliers ([`StatEffects`]); every tick the driving applies them to `KartTuning` for this
//! kart. A kart spawned without stats is at the middle level: exactly `KartTuning`.

mod bump;
mod drift;
mod effects;
mod environment;
mod environment_visual;
mod input;
pub mod model;
mod palette;
pub mod parts;
mod physics;
mod push;
mod rocket;
mod slipstream;
mod stats;
mod trick;
mod tuning;
mod visual;

pub use bump::{KartBump, StepVelocity, bump_victims, closing_speed, soften_kart_contact};
pub use drift::{DriftMotion, DriftSide, MiniTurbo, drift_yaw_rate, slip_pose};
pub use effects::{Autopilot, ItemEffects, autopilot_yaw_rate};
pub use environment::{
    EnvironmentTuning, GLIDERS, GRAVITY, KartEnvironment, anti_gravity_force, anti_gravity_guide,
    glide_pitch, glide_pose, glide_speeds, glide_targets, glide_yaw_target, glider_groups,
    groups_meet, next_anti_gravity, next_gliding, next_underwater, water_tuning,
};
pub use environment_visual::{Bubble, WaterEffects, Wing, wing_opening};
pub use input::{
    DEBUG_BOOST_KEY, DEBUG_RESET_KEY, INTERACT_BUTTON, INTERACT_KEYS, ITEM_KEYS, JUMP_BUTTON,
    JUMP_KEYS, KartInputPlugin, KeyboardLayout, LOOK_BACK_KEYS, PlayerInputMuted, PlayerKart,
    SpawnPoint, combine, gamepad_input, keyboard_input,
};
pub use palette::{
    BOT_COLORS, ColorSet, KartColor, KartPaint, OnlinePlayerColors, PALETTE_SIZE, bot_color,
    bot_colors, resolve_players,
};
pub use parts::{BodyLook, KartParts, PartLook, PartLooks, WheelLook, WingLook};
pub use physics::{Boost, HitReaction, KartState, hit_forward_speed, max_speed, target_yaw_rate};
pub use push::{Shove, TetherPull, towed_forward_speed, towed_top_speed};
pub use rocket::{
    Stall, StartOutcome, StartPress, THROTTLE_PRESSED, rocket_start_duration, start_outcome,
};
pub use slipstream::{BOOST_PAD_REACH, Slipstream, fast_enough_for_slipstream, in_wake, over_pad};
pub use stats::{
    Invincibility, KartStats, MAX_LEVEL, MAX_PART_POINTS, MIDDLE_LEVEL, Stat, StatEffects,
    StatPoints, StatRange, StatsTuning, Terrain, TerrainEffects, TerrainRanges, display_level,
    strike_push_factor, tether_force_scale,
};
pub use trick::TrickTracker;
pub use tuning::KartTuning;
pub use visual::{
    BodyPaint, BoostFlame, DRIVER_SEAT, DeflatedScale, DriftSpark, DriverEars, DriverLook,
    DriverPrimitive, DriverVisual, KartBodyPivot, KartDressed, KartModel, KartVisualAssets,
    KartVisualPlugin, StallSmoke, WheelVisual, WindLine, flame_size, hit_pose, pivoted_wheel,
    shown_while_invincible, shown_with_effects, spawn_kart_visual, spawn_kart_visual_with_driver,
};
pub(crate) use visual::{place_wheels, show_stall_smoke};

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;
use bevy_rapier3d::rapier::geometry::Collider as RapierCollider;

use crate::interpolation::PhysicsPose;
use crate::track::{AntiGravity, BoostPad, GlideRamp, Surface, TrackLayout, TrickRamp, bend_at};
use drift::{StateContext, mini_turbo_boost, step_state};
use physics::{
    approach_yaw_rate, next_forward_speed_on, next_lateral_speed, next_state, next_yaw_rate,
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
    /// Interaction button, held (GD § 15, issue #34): a short press strikes with the paw on the
    /// release, a longer one grabs the kart alongside and holds it while the button stays down
    /// (`crate::interactions`).
    pub interact: bool,
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
    Slipstream,
    KartStats,
    StatEffects,
    Invincibility,
    Shove,
    TetherPull,
    ItemEffects,
    Autopilot,
    KartEnvironment
)]
pub struct Kart;

/// A kart another peer drives (issue #9, `crate::net`): a kinematic body shown where that peer's
/// states say, with the look of a kart, never driven here (it is not a [`Kart`]). The systems
/// that look at the other karts see it: the slipstream behind it, the bots around it, the paws
/// (strikes and grabs: its own peer decides, docs/ARCHITECTURE.md). Only the network writes it.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Remote {
    /// The common tick of the state shown: what a paw that strikes it saw (`tick_vu`).
    pub seen: u32,
    /// Where it is now, extrapolated from its newest state to the current common tick (what a
    /// tether pulls toward: not the pose shown about 100 ms in the past), and its velocity.
    pub now: Vec3,
    pub now_velocity: Vec3,
}

/// The kart's input on the previous tick: a hop starts when the jump/drift button is
/// *pressed*, not every tick while it stays held.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct PreviousInput(pub KartInput);

/// Driving aids of this kart's driver (GD § 11), off by default. Max decided on 2026-10-13
/// (issue #17) that the aids are not planned for now: nothing turns them on, no menu offers them
/// and nothing keeps the kart on the track. What remains is the drift's side of the assisted
/// steering (issue #4), kept because the drift rules, the bots' tier choice and their tests use
/// it; it is ready if the aids come back.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KartAssists {
    /// Assisted steering: the drift charges slower (2 and 0.8 units per tick) and the purple
    /// mini-turbo is disabled (GD § 3.3 and 3.6). Never on in the game (see above).
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
/// only its own; [`MaxSpeedScale`] is their product.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MaxSpeedFactors {
    /// The bots' light catch-up (GD § 12, issue #6): within ±3 %.
    pub catch_up: f32,
    /// The croquettes held (GD § 7, issue #14): +1 % each, up to 10.
    pub croquettes: f32,
    /// The Speed stat (GD § 9, issue #15): `StatEffects::top_speed`, set when the kart spawns
    /// (0.92 to 1.08 by default; 1 at the middle level).
    pub stats: f32,
    /// Held by another kart's paw (GD § 15, issue #34): braked a little while the tether lasts
    /// (0.9 at the middle Weight, with the default `InteractionTuning::tether_target_brake`).
    pub tether: f32,
    /// The items of phase 2 (issue #18, [`ItemEffects::speed_factor`]): faster in a super
    /// câlin, slower deflated.
    pub items: f32,
    /// The environment (GD § 10, issue #20, [`EnvironmentTuning::water_top_speed`]): slower under
    /// water; exactly 1 elsewhere.
    pub environment: f32,
}

impl Default for MaxSpeedFactors {
    fn default() -> Self {
        Self {
            catch_up: 1.0,
            croquettes: 1.0,
            stats: 1.0,
            tether: 1.0,
            items: 1.0,
            environment: 1.0,
        }
    }
}

impl MaxSpeedFactors {
    pub fn product(&self) -> f32 {
        // The environment last: on the ground (1), exactly the product before it.
        self.catch_up * self.croquettes * self.stats * self.tether * self.items * self.environment
    }
}

/// Moves `value` toward `target` by at most `max_delta`.
fn approach(value: f32, target: f32, max_delta: f32) -> f32 {
    if value < target {
        (value + max_delta).min(target)
    } else {
        (value - max_delta).max(target)
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
    /// After the physics step: the contacts between karts softened (issue #154), then the bumps
    /// between karts of that step ([`KartBump`]).
    Bumps,
}

pub struct KartPlugin;

impl Plugin for KartPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KartTuning>()
            .init_resource::<StatsTuning>()
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
                    environment::detect_water,
                    environment::apply_environments,
                    combine_max_speed_factors,
                    boost_pads,
                    charge_slipstreams,
                    drive_karts,
                    record_step_velocities,
                )
                    .chain()
                    .in_set(KartSystems::Drive),
            )
            .add_systems(
                FixedUpdate,
                (soften_kart_contacts, detect_bumps)
                    .chain()
                    .in_set(KartSystems::Bumps),
            );
    }
}

/// A kart body with these stats (GD § 9, issue #15), converted once here: its mass and inertia,
/// its top speed factor ([`MaxSpeedFactors::stats`]) and the multipliers the driving applies to
/// `tuning` every tick ([`StatEffects`]).
pub fn kart_body_with_stats(
    tuning: &KartTuning,
    stats_tuning: &StatsTuning,
    stats: KartStats,
    transform: Transform,
) -> impl Bundle {
    let effects = StatEffects::new(&stats, stats_tuning);
    (
        kart_body(&effects.apply(tuning), transform),
        stats,
        effects,
        MaxSpeedFactors {
            stats: effects.top_speed,
            ..default()
        },
    )
}

/// A kart's collision box ([`KART_HALF_EXTENTS`], rounded edges): ours, the bots', and the
/// kinematic bodies of the other players' karts online (issue #8).
pub fn kart_collider() -> Collider {
    let rounding = 0.1;
    let half = KART_HALF_EXTENTS - Vec3::splat(rounding);
    Collider::round_cuboid(half.x, half.y, half.z, rounding)
}

/// Everything a kart body needs: physics, driving components and pose recording for the
/// visual interpolation. Add `PlayerKart` (or a bot / network driver) and a visual on top. Its
/// stats are the middle level (exactly `tuning`); see [`kart_body_with_stats`].
pub fn kart_body(tuning: &KartTuning, transform: Transform) -> impl Bundle + use<> {
    (
        Kart,
        transform,
        PhysicsPose::new(transform),
        RigidBody::Dynamic,
        kart_collider(),
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

fn tick_timers(
    time: Res<Time>,
    mut boosts: Query<&mut Boost>,
    mut stalls: Query<&mut Stall>,
    mut invincibilities: Query<&mut Invincibility>,
    mut effects: Query<&mut ItemEffects>,
) {
    let dt = time.delta_secs();
    for mut effects in &mut effects {
        if effects.is_active() {
            effects.tick(dt);
        }
    }
    for mut invincibility in &mut invincibilities {
        if invincibility.is_active() {
            invincibility.tick(dt);
        }
    }
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
fn record_step_velocities(
    mut karts: Query<(&Velocity, &Transform, &mut StepVelocity), With<Kart>>,
) {
    for (velocity, transform, mut step) in &mut karts {
        *step = StepVelocity {
            linear: velocity.linear,
            angular: velocity.angular,
            rotation: transform.rotation,
        };
    }
}

/// Issue #154 (GD § 15): after the physics step, a kart touching another kart (ours, or another
/// peer's kinematic body online) keeps only a share of the turn that contact gave it (`bump.rs`,
/// [`soften_kart_contact`]): its rotation and its yaw rate are set back. Its push is left as it
/// is. Online, each peer softens its own kart: nothing changes in the protocol. Walls and
/// obstacles are left as they are.
fn soften_kart_contacts(
    tuning: Res<KartTuning>,
    time: Res<Time>,
    rapier: ReadRapierContext,
    mut karts: Query<
        (
            Entity,
            &mut Transform,
            &StepVelocity,
            &KartState,
            &mut Velocity,
        ),
        With<Kart>,
    >,
    others: Query<(), Or<(With<Kart>, With<Remote>)>>,
) {
    let Ok(context) = rapier.single() else {
        return;
    };
    let dt = time.delta_secs();
    for (kart, mut transform, before, state, mut velocity) in &mut karts {
        if *state == KartState::Respawning {
            continue;
        }
        let touched = context.contact_pairs_with(kart).any(|pair| {
            let (Some(c1), Some(c2)) = (pair.collider1(), pair.collider2()) else {
                return false;
            };
            let other = if c1 == kart { c2 } else { c1 };
            others.contains(other)
                && pair.has_any_active_contact()
                && pair.total_impulse_magnitude() > 0.0
        });
        if !touched {
            continue;
        }
        let after = StepVelocity {
            linear: velocity.linear,
            angular: velocity.angular,
            rotation: transform.rotation,
        };
        let softened = soften_kart_contact(&tuning, *before, after, dt);
        if softened != after {
            velocity.angular = softened.angular;
            transform.rotation = softened.rotation;
        }
    }
}

/// After the physics step: every pair of karts the solver pushed apart in that step, hitting
/// each other fast enough, is a bump (GD § 7, `bump.rs`). Read from Rapier's contact graph
/// (each kart's own contact pairs, a few per kart): no extra physics query. Issue #78 (GD § 10):
/// two karts in anti-gravity that hit each other both get the spin boost; online, each peer gives
/// it to its own kart (the other one's peer to the other).
fn detect_bumps(
    tuning: Res<KartTuning>,
    rapier: ReadRapierContext,
    karts: Query<
        (
            Entity,
            &Transform,
            &StepVelocity,
            &KartState,
            &KartEnvironment,
        ),
        With<Kart>,
    >,
    remotes: Query<
        (&Transform, &Velocity, &KartState, Option<&KartEnvironment>),
        (With<Remote>, Without<Kart>),
    >,
    mut boosts: Query<&mut Boost, With<Kart>>,
    mut spun: Local<Vec<Entity>>,
    mut bumps: MessageWriter<KartBump>,
) {
    let Ok(context) = rapier.single() else {
        return;
    };
    let environment = &tuning.environment;
    spun.clear();
    for (a, a_transform, a_velocity, a_state, a_environment) in &karts {
        if *a_state == KartState::Respawning {
            continue;
        }
        for pair in context.contact_pairs_with(a) {
            let (Some(c1), Some(c2)) = (pair.collider1(), pair.collider2()) else {
                continue;
            };
            let b = if c1 == a { c2 } else { c1 };
            // Online (issue #53): another peer's kart. Only ours is judged here (its own peer
            // judges the other one): the victim decides.
            let remote = remotes
                .get(b)
                .ok()
                .map(|(transform, velocity, state, environment)| {
                    let anti_gravity = environment.is_some_and(|e| e.anti_gravity);
                    (
                        b,
                        transform,
                        StepVelocity {
                            linear: velocity.linear,
                            angular: velocity.angular,
                            rotation: transform.rotation,
                        },
                        state,
                        anti_gravity,
                    )
                });
            // Each pair once; only karts.
            if remote.is_none() && b <= a {
                continue;
            }
            let Some((_, b_transform, b_velocity, b_state, b_anti_gravity)) =
                remote.or_else(|| {
                    karts
                        .get(b)
                        .ok()
                        .map(|(b, transform, velocity, state, environment)| {
                            (b, transform, *velocity, state, environment.anti_gravity)
                        })
                })
            else {
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
            let closing_speed = closing_speed(a_velocity.linear, b_velocity.linear, normal);
            // Issue #78: the spin boost, for both karts in anti-gravity (ours only, online).
            if a_environment.anti_gravity
                && b_anti_gravity
                && closing_speed >= environment.spin_boost_min_speed
            {
                spun.push(a);
                if remote.is_none() {
                    spun.push(b);
                }
            }
            let Some((a_bumped, b_bumped)) =
                bump_victims(&tuning, a_velocity.linear, b_velocity.linear, normal)
            else {
                continue;
            };
            if a_bumped {
                bumps.write(KartBump {
                    kart: a,
                    by: b,
                    closing_speed,
                });
            }
            if b_bumped && remote.is_none() {
                bumps.write(KartBump {
                    kart: b,
                    by: a,
                    closing_speed,
                });
            }
        }
    }
    for kart in spun.drain(..) {
        if let Ok(mut boost) = boosts.get_mut(kart) {
            boost.add(
                environment.spin_boost_strength,
                environment.spin_boost_duration,
            );
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
    remotes: Query<(Entity, &Transform, &KartState), (With<Remote>, Without<Kart>)>,
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
    // Online (issue #9): the other peers' karts leave a wake too, where they are shown.
    leaders.extend(
        remotes
            .iter()
            .filter(|(_, _, state)| **state != KartState::Respawning)
            .map(|(entity, transform, _)| (entity, transform.translation, *transform.forward())),
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
    layout: Option<Res<TrackLayout>>,
    time: Res<Time>,
    rapier: ReadRapierContext,
    surfaces: Query<&Surface>,
    (ramps, glide_ramps, zones): (
        Query<(), With<TrickRamp>>,
        Query<(), With<GlideRamp>>,
        Query<(), With<AntiGravity>>,
    ),
    mut karts: Query<
        (
            &Transform,
            &KartInput,
            &mut PreviousInput,
            &KartAssists,
            (&MaxSpeedScale, &StatEffects),
            &Stall,
            &mut TrickTracker,
            (&mut Boost, &mut Invincibility),
            &mut KartState,
            &mut DriftMotion,
            &mut KartWheels,
            &mut Velocity,
            (
                &mut Shove,
                &TetherPull,
                &ItemEffects,
                &Autopilot,
                &mut KartEnvironment,
            ),
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
    let base = &*tuning;
    // Issue #20: the special environments' knobs.
    let environment_tuning = &base.environment;
    // The trick window in ticks (GD § 5: about 0.2 s).
    let trick_window = (base.trick_window / dt).round() as u32;
    // The suspension rays only find the ground (issue #27): a static collider whose `Surface` is
    // ground. Never a wall: a kart tipped against a low wall found ground on its flat top, and
    // the spring helped lift that side over it. Never another kart: in a side shunt, the
    // rammer's front wheels found ground on the other kart's roof and drove onto it.
    let ground = |collider: Entity, _: &RapierCollider| {
        surfaces
            .get(collider)
            .is_ok_and(|surface| surface.is_ground())
    };
    let filter = QueryFilter::only_fixed()
        .exclude_sensors()
        .predicate(&ground);

    for (
        transform,
        input,
        mut previous_input,
        assists,
        (speed_scale, effects),
        stall,
        mut tricks,
        (mut boost, mut invincibility),
        mut state,
        mut motion,
        mut wheels,
        mut velocity,
        (mut shove, pull, item_effects, autopilot, mut environment),
        mut force,
        mut gravity,
    ) in &mut karts
    {
        // This kart's tuning: `KartTuning` with its stats applied (exactly `KartTuning` at the
        // middle level), those of its terrain (issue #20: in the air, under water), and under
        // water the water's physics.
        let effects = effects.on(environment.terrain());
        let mut kart_tuning = effects.tuning(base);
        if environment.underwater {
            kart_tuning = std::borrow::Cow::Owned(water_tuning(&kart_tuning, environment_tuning));
        }
        let tuning = &*kart_tuning;
        if *state == KartState::Respawning {
            // Issue #20: the pelican folds the glider; issue #78: it takes the kart out of the
            // anti-gravity.
            if environment.gliding
                || environment.since_glide_ramp != u32::MAX
                || environment.anti_gravity
                || environment.up != Vec3::Y
            {
                *environment = KartEnvironment {
                    underwater: environment.underwater,
                    ..default()
                };
            }
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
            shove.set_if_neq(Shove::default());
            tricks.set_if_neq(TrickTracker::default());
            previous_input.set_if_neq(PreviousInput(*input));
            continue;
        }
        // A stalled engine (rocket start pressed too early, GD § 4) ignores the accelerator,
        // the brake and the jump button; a kart hit by an item (GD § 8) ignores everything. The
        // button edges still follow the real input.
        let driver = *input;
        let hit = match *state {
            KartState::Hit { reaction, .. } => Some(reaction),
            _ => None,
        };
        // A fusée (issue #18) drives itself: deaf to its driver too.
        let rocket = item_effects.rocket > 0.0 && autopilot.speed > 0.0;
        let effective = if hit.is_some() || rocket {
            KartInput {
                look_back: driver.look_back,
                ..default()
            }
        } else if stall.is_active() {
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
        // Issue #78: in anti-gravity, the kart's vertical is the road's normal under its wheels
        // (last tick's); everywhere else, exactly the world's.
        let anti_gravity = environment.anti_gravity;
        let vertical = if anti_gravity {
            environment.up
        } else {
            Vec3::Y
        };

        // --- Suspension: one ray per wheel, spring + damper pushing along the kart's up axis.
        let mut total_force = Vec3::ZERO;
        let mut total_torque = Vec3::ZERO;
        let mut normal_sum = Vec3::ZERO;
        let mut grounded = 0;
        let mut offroad = 0;
        let mut on_trick_ramp = false;
        let mut on_glide_ramp = false;
        let mut on_zone = 0;
        for (i, anchor) in WHEEL_ANCHORS.iter().enumerate() {
            let origin = transform.transform_point(*anchor);
            let hit = context.cast_ray_and_get_normal(
                origin,
                -up,
                tuning.suspension_rest_length,
                true,
                filter,
            );
            // Too steep a face is no ground: steeper than `min_ground_normal_y` from the world's
            // up, or in anti-gravity (issue #78) from the road's normal the kart drives on.
            let flat_enough = |normal: Vec3| {
                if anti_gravity {
                    normal.dot(vertical) >= tuning.min_ground_normal_y
                } else {
                    normal.y >= tuning.min_ground_normal_y
                }
            };
            let Some((hit_entity, hit)) = hit.filter(|(_, hit)| flat_enough(hit.normal)) else {
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
            on_glide_ramp |= glide_ramps.contains(hit_entity);
            on_zone += usize::from(zones.contains(hit_entity));
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
        // Issue #20: the glider opens off a glide ramp and folds on the ground.
        let mut env = *environment;
        env.since_glide_ramp = if on_glide_ramp {
            0
        } else {
            env.since_glide_ramp.saturating_add(1)
        };
        env.gliding = next_gliding(
            environment_tuning,
            &env,
            grounded,
            wheels.ticks_without_ground,
            false,
            dt,
        );
        // Issue #78: in anti-gravity on the road of a zone; its vertical, the road's normal: under
        // its wheels, or in the air the normal of the zone's road below it (the road twists away
        // under a kart in the air: its gravity, and the frame it drives in, turn with it; one
        // ray, only for a kart in the air in anti-gravity).
        env.anti_gravity = next_anti_gravity(&env, grounded, on_zone);
        let mut held_down = 0.0;
        if env.anti_gravity && grounded == 0 {
            match context.cast_ray_and_get_normal(
                transform.translation,
                -vertical,
                environment_tuning.anti_gravity_probe,
                true,
                filter,
            ) {
                Some((hit, below))
                    if zones.contains(hit)
                        && below.normal.dot(vertical) >= tuning.min_ground_normal_y =>
                {
                    wheels.ground_normal = below.normal;
                    // High above the road (it twisted away under the kart): held to it harder.
                    if below.time_of_impact > environment_tuning.anti_gravity_hold_height {
                        held_down = environment_tuning.anti_gravity_hold.max(0.0);
                    }
                }
                // Another ground below it: it lands there, and leaves the anti-gravity then.
                Some(_) => {}
                // Issue #94: no ground at all along its up (thrown off a ceiling, beyond its
                // walls): it lets go, and falls with the world's gravity.
                None => env.anti_gravity = false,
            }
        }
        env.up = if env.anti_gravity {
            wheels.ground_normal
        } else {
            Vec3::Y
        };
        // Issue #96: in anti-gravity since last tick.
        let was_anti_gravity = environment.anti_gravity;
        environment.set_if_neq(env);
        let gliding = env.gliding;
        let anti_gravity = env.anti_gravity;
        let vertical = env.up;
        let normal = wheels.ground_normal;
        let ahead = (forward - normal * forward.dot(normal)).normalize_or(forward);
        let right = ahead.cross(normal);
        // A strike's push (GD § 15, `push.rs`) is kept apart: the driving works on the velocity
        // without it, and adds it back at the end. A wall that stopped it ended it.
        let carried = shove.carried(velocity.linear);
        let mut linvel = velocity.linear - shove.direction * carried;
        // Issue #96: on the road of a zone that bends (a wall round a turn), the velocity turns
        // with the road (it guides the kart like a rail). Everywhere else, nothing.
        if anti_gravity
            && was_anti_gravity
            && grounded > 0
            && environment_tuning.anti_gravity_guide
            && let Some(layout) = layout.as_deref()
            && let Some(bend) = bend_at(&layout.anti_gravity, &layout.center, transform.translation)
        {
            linvel = anti_gravity_guide(linvel, &bend, dt);
        }
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
        if step.hit_ended {
            // After the spin or tumble, protected from the items for a while (GD § 8: its
            // Invincibility stat, 1 to 2 s).
            invincibility.start(effects.invincibility);
        }
        if step.hop {
            // Hop: at least `hop_speed` along the ground normal (never stacks on a bump).
            linvel += normal * (tuning.hop_speed - linvel.dot(normal)).max(0.0);
        }
        // Gliding, the glide sets the vertical speed itself (issue #20). In anti-gravity (issue
        // #78), the world's gravity is off and the kart's own pulls it toward the road under it,
        // as strong.
        let gravity_scale = if gliding || anti_gravity {
            0.0
        } else {
            tuning.gravity_scale
        };
        if gravity.0 != gravity_scale {
            gravity.0 = gravity_scale;
        }
        if anti_gravity {
            force.force +=
                anti_gravity_force(vertical, tuning.gravity_scale, tuning.mass) * (1.0 + held_down);
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
            KartState::Drifting { .. } | KartState::Hit { .. } => on_ground,
            KartState::Airborne | KartState::Trick | KartState::Respawning => false,
        };
        if rocket {
            // The fusée's autopilot (issue #18): toward its point of the racing line, at its
            // speed, on the ground and in the air alike; no slide, no push, no tether. It hugs
            // the track: never thrown upward off a bump or a ramp. Issue #171: in the air (off a
            // bump, over the void of a risky jump), it flies level (the world's up, not the
            // ramp's it left: that one threw it up and away along the stale slope) toward its
            // point's height, never steeper down than a dive: it never falls into a void
            // (gravity off meanwhile). Gliding too, with the glider's descent (below). Not in
            // anti-gravity (its up is the road's).
            let airborne = grounded == 0 && !anti_gravity;
            let flying = airborne && !gliding;
            let (normal, ahead) = if airborne {
                let level = Vec3::new(forward.x, 0.0, forward.z).normalize_or(ahead);
                (Vec3::Y, level)
            } else {
                (normal, ahead)
            };
            let forward_speed = linvel.dot(ahead);
            let new_forward = approach(
                forward_speed,
                autopilot.speed,
                autopilot.accel.max(0.0) * dt,
            );
            let new_yaw = autopilot_yaw_rate(
                ahead,
                autopilot.target - transform.translation,
                normal,
                autopilot.turn_response,
                autopilot.max_turn_rate,
            );
            let travel = Quat::from_axis_angle(normal, new_yaw * dt) * ahead;
            let mut up_speed = linvel.dot(normal);
            if grounded == 0 {
                up_speed = up_speed.min(0.0);
            }
            if flying {
                let sink = environment_tuning.glide_dive_slope * autopilot.speed.max(0.0);
                up_speed = ((autopilot.target.y - transform.translation.y) * 4.0).clamp(-sink, 4.0);
                if gravity.0 != 0.0 {
                    gravity.0 = 0.0;
                }
            }
            if gliding {
                // Over a gap (issue #20), it flies along its line: toward the height of its
                // point, never steeper down than a dive.
                let sink = environment_tuning.glide_dive_slope * autopilot.speed.max(0.0);
                up_speed = ((autopilot.target.y - transform.translation.y) * 2.0).clamp(-sink, 2.0);
            }
            velocity.linear = travel * new_forward + normal * up_speed;
            let tilt = angvel - normal * angvel.dot(normal);
            velocity.angular = if airborne {
                // Level and upright for its landing.
                let righting = righting_angular_velocity(up, Vec3::Y, tuning.righting_rate);
                normal * new_yaw + righting
            } else {
                normal * new_yaw + tilt * (-tuning.tilt_damping * dt).exp()
            };
            shove.set_if_neq(Shove::default());
            continue;
        }
        if gliding {
            // The glider (GD § 10, issue #20): Up dives, Down climbs, Left and Right turn, with
            // the stats « air » (`effects`, this tick's terrain). The body stays level and turns
            // back upright; its pitch and bank are only shown.
            let env_tuning = environment_tuning;
            let level = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
            let forward_speed = linvel.dot(level);
            let top = max_speed(tuning, 0.0, &boost) * speed_scale.0.max(0.0);
            let (new_forward, new_up) = glide_speeds(
                env_tuning,
                top,
                forward_speed,
                linvel.y,
                glide_pitch(input),
                dt,
            );
            let side = linvel - level * forward_speed - Vec3::Y * linvel.y;
            let side = side * (-env_tuning.glide_side_grip.max(0.0) * dt).exp();
            // A glider does not spin: a bump in the air (another glider, at the takeoff) turns
            // it no faster than its own full turn (the wing), or the bump would throw it off its
            // line, over the walls or into the lagoon.
            let full_turn = glide_yaw_target(env_tuning, 1.0, effects.steer_rate).abs();
            let yaw = angvel.y.clamp(-full_turn, full_turn);
            let target = glide_yaw_target(env_tuning, input.steer, effects.steer_rate);
            let new_yaw = approach_yaw_rate(
                yaw,
                target,
                env_tuning.glide_turn_response * effects.steer_response,
                dt,
            );
            let travel = Quat::from_rotation_y(new_yaw * dt) * level;
            velocity.linear = travel * new_forward + side + Vec3::Y * new_up + pull.accel * dt;
            let tilt = angvel - Vec3::Y * angvel.y;
            let righting = righting_angular_velocity(up, Vec3::Y, tuning.righting_rate);
            let blend = 1.0 - (-tuning.air_rotation_response * dt).exp();
            velocity.angular = Vec3::Y * new_yaw + tilt + (righting - tilt) * blend;
        } else if ground_handling {
            let forward_speed = linvel.dot(ahead);
            // Towed by a tether (GD § 15), the kart neither accelerates by itself nor slows down
            // between its own top speed and its partner's: the tether pulls it there.
            let top_speed = towed_top_speed(
                max_speed(tuning, wheels.offroad_ratio, &boost) * speed_scale.0.max(0.0),
                forward_speed,
                pull.tow_speed,
            );
            // Hit by an item: it slows down hard, boost or not (GD § 8).
            let new_forward = match hit {
                Some(reaction) => hit_forward_speed(tuning, forward_speed, reaction, dt),
                // Issue #171: on the grass, it brakes down to its top speed at once.
                None => next_forward_speed_on(
                    tuning,
                    forward_speed,
                    input,
                    top_speed,
                    boost.is_active(),
                    wheels.offroad_ratio,
                    dt,
                ),
            };
            // The tether pulls along the heading (towed up to its partner's speed at most) and
            // to the side (the grip takes most of that: the kart keeps its direction).
            let new_forward =
                towed_forward_speed(new_forward, pull.accel.dot(ahead), pull.tow_speed, dt);
            let new_lateral =
                next_lateral_speed(tuning, linvel.dot(right), dt) + pull.accel.dot(right) * dt;

            let yaw = angvel.dot(normal);
            let tilt = angvel - normal * yaw;
            // A drift always turns toward its side; the steering only tightens or widens it.
            // Its curve sets in from the rotation the kart had on landing and fades back to the
            // normal steering after it (`DriftMotion`, issue #30).
            let steering = target_yaw_rate(tuning, input.steer, new_forward);
            let target = match drift_motion.side {
                Some(side) => {
                    let drift = drift_yaw_rate(tuning, side, input.steer, new_forward);
                    drift_motion.yaw_target(steering, drift)
                }
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
            // In anti-gravity (issue #78), the road may twist under the kart: its tilt follows the
            // road's normal. Elsewhere, the springs keep it on the road and its tilt calms down.
            let tilt = if anti_gravity {
                righting_angular_velocity(up, normal, environment_tuning.anti_gravity_align_rate)
            } else {
                tilt * (-tuning.tilt_damping * dt).exp()
            };
            velocity.angular = normal * new_yaw + tilt;
        } else {
            // A little air control, and turn back upright to land on the wheels: upright along the
            // world's up, or in anti-gravity (issue #78) along the road it flies over.
            velocity.linear = linvel + pull.accel * dt;
            let yaw = angvel.dot(vertical);
            let tilt = angvel - vertical * yaw;
            let target =
                target_yaw_rate(tuning, input.steer, linvel.dot(forward)) * tuning.air_steer_factor;
            let righting = righting_angular_velocity(up, vertical, tuning.righting_rate);
            let blend = 1.0 - (-tuning.air_rotation_response * dt).exp();
            velocity.angular = vertical * next_yaw_rate(tuning, yaw, target, dt)
                + tilt
                + (righting - tilt) * blend;
        }
        // Under water, off the ground, the water slows the jumps and the falls (issue #20).
        if env.underwater && grounded == 0 {
            let up_speed = velocity.linear.y;
            let fade = (-environment_tuning.water_vertical_drag.max(0.0) * dt).exp();
            velocity.linear.y = up_speed * fade;
        }
        // The push, fading by itself (`push.rs`).
        if shove.is_active() {
            let push = shove.step(carried, dt);
            velocity.linear += shove.direction * push;
        }
    }
}

#[cfg(test)]
mod anti_gravity_tests;
#[cfg(test)]
mod clash_tests;
#[cfg(test)]
mod environment_tests;
#[cfg(test)]
mod scenario_tests;
