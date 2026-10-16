//! The soft characters (issue #12, `docs/ARCHITECTURE.md` § « Personnages mous »): a procedural
//! wobble, purely visual. Pure here (tested without Bevy's schedules); `wobble_visual.rs` feeds
//! it each frame and lays its pose over the drivers.
//!
//! Each driver has a few **damped springs** that follow its kart with a delay, as a soft body
//! would: the driver feels its kart's acceleration (braking, turning, drifting, landing, a wall)
//! by inertia.
//!
//! - **Lean** (the body, with the head: a Cube Pet's `body`, or the primitive ball): a rotation
//!   around its bottom, on the seat. Braking leans it forward, the engine back, a turn out of the
//!   turn.
//! - **Squash & stretch**: the vertical acceleration squashes it (landing, the bottom of a dip)
//!   or stretches it (in the air); a shock (a wall, another kart, a paw) squashes it too.
//! - **Tail** (the elephant's trunk) and **wings** of the birds: their own springs, softer, which
//!   swing a little later. The Cube Pets' ears are part of their body (no node of their own): they
//!   lean with it.
//! - **Mini-ragdoll** for about 1 s ([`WobbleTuning::ragdoll_seconds`]): hit by an item
//!   (`KartState::Hit`), struck by a paw, or a big shock. A kick, then every spring is floppier
//!   (lower frequency, less damping, wider limits), and stiffens back to normal by the end.
//!
//! **Inputs** come from what the screen shows, not from the simulation: the kart's interpolated
//! pose of each frame (its velocity and acceleration are measured from it), its state and its
//! paw. Online, the other players' karts wobble from their own interpolated motion and the states
//! that already come over the network: nothing is sent. Nothing here touches the race.
//!
//! **Stability**: each spring is integrated with implicit (backward) Euler, which never gains
//! energy whatever the step, in substeps of at most 1/240 s: from 30 to 240 fps the same springs
//! run the same substeps. The accelerations come in as velocity kicks (the velocity change of
//! the frame), so a frame rate splits them differently but gives them the same total.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::ops::{Add, Mul, Sub};

use bevy::prelude::*;

use crate::kart::HitReaction;

/// The longest substep of the springs (s): 1/240 s, the fastest frame rate we expect. A frame of
/// 1/30 s runs 8 of them, one of 1/240 s one.
pub const MAX_SUBSTEP: f32 = 1.0 / 240.0;
/// A frame longer than this (s: a hitch, a tab in the background) or a jump of more than
/// [`WobbleTuning::teleport_distance`] measures nothing: the motion starts over.
pub const MAX_FRAME: f32 = 0.1;
/// Limits of a spring's frequency (Hz) and damping ratio: whatever the panel says, every spring
/// swings back and comes to rest.
pub const FREQUENCY_RANGE: (f32, f32) = (0.2, 15.0);
pub const DAMPING_RANGE: (f32, f32) = (0.05, 4.0);
/// The spring itself never goes further than this many times its limit (what shows is softly
/// held under the limit, [`soft_limit`]).
const HARD_LIMIT: f32 = 2.0;
/// A spring this close to rest (rad, or a share of the height; ten times that a second) is put
/// exactly at rest: 0.00001 rad is 10 µm at the tip of a 1 m driver.
const SNAP: f32 = 1e-5;

/// One damped spring: how fast it swings back, how much it is damped, how far an acceleration
/// moves it and how far it may go.
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
pub struct SpringTuning {
    /// Natural frequency (Hz): higher is stiffer, lower is softer and later.
    pub frequency: f32,
    /// Damping ratio: 0 swings forever, 1 comes back without swinging past.
    pub damping: f32,
    /// How far a steady acceleration moves it, per m/s² (rad for a lean, a share of the height
    /// for the squash).
    pub per_accel: f32,
    /// How far it goes at most (rad for a lean, a share of the height for the squash).
    pub max: f32,
}

/// The wobble's knobs (issue #12). Purely visual: nothing here touches the race, nothing goes
/// over the network. Reflected (`Reflect`) so that the admin panel (issue #36) edits every field.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct WobbleTuning {
    /// The drivers wobble. Off: they sit still on top of their animation (to compare).
    pub enabled: bool,
    /// The body and its head, around its bottom on the seat. 0.005 rad per m/s²: about 13° in a
    /// full-speed turn (47 m/s²), 8° braking (28 m/s²).
    pub body: SpringTuning,
    /// Squash (positive: lower and wider, the volume kept) and stretch, along the body. `max`:
    /// the largest squash (0.3: 30 % lower). 0.0015 per m/s²: 3 % taller in the air (the
    /// reinforced gravity, 21.6 m/s²).
    pub squash: SpringTuning,
    /// The largest stretch (0.15: 15 % taller).
    pub max_stretch: f32,
    /// The tail (the elephant's trunk), around its root: softer and later than the body.
    pub tail: SpringTuning,
    /// The birds' wings, around their shoulders.
    pub wings: SpringTuning,

    // --- Shocks (a wall, another kart, a paw) ---------------------------------------------------
    /// A shock is a sudden change of the kart's horizontal velocity: the change over the last
    /// `shock_window` seconds (a steady turn stays under `shock_deadzone`)...
    pub shock_window: f32,
    /// ...beyond this (m/s)...
    pub shock_deadzone: f32,
    /// ...squashes the body by this much per m/s (at most `squash.max`).
    pub shock_squash: f32,

    // --- Mini-ragdoll (hit by an item, struck by a paw, a big shock) ----------------------------
    /// It lasts this long (s), the springs stiffening back to normal over it.
    pub ragdoll_seconds: f32,
    /// A shock of this much (m/s) sets it off too.
    pub ragdoll_shock: f32,
    /// The kick that starts it (rad/s): the head thrown aside (a spin out), back (a tumble) or
    /// toward the paw (a strike); the tail the other way, the wings up.
    pub ragdoll_kick: f32,
    /// At its start, the springs' frequencies times this...
    pub ragdoll_frequency: f32,
    /// ...their damping times this...
    pub ragdoll_damping: f32,
    /// ...and their limits times this.
    pub ragdoll_max: f32,

    /// A kart that moves more than this in one frame (m) was teleported (the pelican's drop, a
    /// restart): no acceleration measured.
    pub teleport_distance: f32,
}

impl Default for WobbleTuning {
    fn default() -> Self {
        Self {
            enabled: true,
            body: SpringTuning {
                frequency: 2.2,
                damping: 0.3,
                per_accel: 0.005,
                max: 0.45,
            },
            squash: SpringTuning {
                frequency: 3.0,
                damping: 0.3,
                per_accel: 0.0015,
                max: 0.3,
            },
            max_stretch: 0.15,
            tail: SpringTuning {
                frequency: 1.6,
                damping: 0.3,
                per_accel: 0.01,
                max: 0.7,
            },
            wings: SpringTuning {
                frequency: 2.6,
                damping: 0.2,
                per_accel: 0.012,
                max: 0.9,
            },
            shock_window: 0.05,
            shock_deadzone: 4.0,
            shock_squash: 0.02,
            ragdoll_seconds: 1.0,
            ragdoll_shock: 9.0,
            ragdoll_kick: 6.0,
            ragdoll_frequency: 0.55,
            ragdoll_damping: 0.4,
            ragdoll_max: 1.5,
            teleport_distance: 4.0,
        }
    }
}

/// A damped spring's state: where it is (`value`, 0 at rest) and how fast it moves. `T` is
/// `f32` (the squash) or `Vec3` (a lean: a rotation vector, its axis times its angle).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring<T> {
    pub value: T,
    pub speed: T,
}

impl<T> Spring<T>
where
    T: Copy + Default + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    /// Moves it on by `dt` seconds, at `frequency` (Hz) and `damping` (ratio; both kept within
    /// their ranges), in substeps of at most [`MAX_SUBSTEP`]. Implicit Euler: never unstable,
    /// never gains energy, whatever `dt`.
    pub fn step(&mut self, frequency: f32, damping: f32, dt: f32) {
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        let omega = TAU * clamp_finite(frequency, FREQUENCY_RANGE);
        let k = omega * omega;
        let c = 2.0 * clamp_finite(damping, DAMPING_RANGE) * omega;
        let dt = dt.min(MAX_FRAME);
        // A hair less than the exact count: 1/30 s is 8 substeps of 1/240 s, not 9.
        let steps = ((dt / MAX_SUBSTEP) - 1e-3).ceil().max(1.0);
        let h = dt / steps;
        let divide = 1.0 / (1.0 + h * c + h * h * k);
        for _ in 0..steps as u32 {
            // Backward Euler: v₁ = v₀ + h·(−k·x₁ − c·v₁), x₁ = x₀ + h·v₁.
            self.speed = (self.speed - self.value * (h * k)) * divide;
            self.value = self.value + self.speed * h;
        }
    }

    /// Back at rest.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Spring<Vec3> {
    /// Its energy at stiffness `k` (ω²): k·x² + v². Never grows without a kick.
    pub fn energy(&self, k: f32) -> f32 {
        k * self.value.length_squared() + self.speed.length_squared()
    }

    /// Never further than `limit` (rad): held there, its speed outward dropped.
    fn hold(&mut self, limit: f32) {
        let length = self.value.length();
        if length > limit && length > 0.0 {
            let out = self.value / length;
            self.value = out * limit;
            let outward = self.speed.dot(out);
            if outward > 0.0 {
                self.speed -= out * outward;
            }
        }
    }

    fn settled(&self, epsilon: f32) -> bool {
        self.value.length() < epsilon && self.speed.length() < epsilon * 10.0
    }

    /// Exactly at rest once nothing would show (the parts are then not written each frame).
    fn snap(&mut self) {
        if self.settled(SNAP) {
            self.reset();
        }
    }
}

impl Spring<f32> {
    pub fn energy(&self, k: f32) -> f32 {
        k * self.value * self.value + self.speed * self.speed
    }

    /// Never further than `low` below or `high` above 0, its speed outward dropped.
    fn hold(&mut self, low: f32, high: f32) {
        if self.value > high {
            self.value = high;
            self.speed = self.speed.min(0.0);
        } else if self.value < -low {
            self.value = -low;
            self.speed = self.speed.max(0.0);
        }
    }

    fn settled(&self, epsilon: f32) -> bool {
        self.value.abs() < epsilon && self.speed.abs() < epsilon * 10.0
    }

    fn snap(&mut self) {
        if self.settled(SNAP) {
            self.reset();
        }
    }
}

fn clamp_finite(value: f32, (low, high): (f32, f32)) -> f32 {
    if value.is_nan() {
        low
    } else {
        value.clamp(low, high)
    }
}

/// `value` softly held under `max`: about itself when small, never beyond `max` (a tanh).
pub fn soft_limit(value: f32, max: f32) -> f32 {
    if max.is_nan() || max <= 0.0 {
        return 0.0;
    }
    max * (value / max).tanh()
}

/// Same for a rotation vector: its angle softly held under `max`, its axis kept.
pub fn soft_limit_vec(value: Vec3, max: f32) -> Vec3 {
    let length = value.length();
    if length <= 1e-9 || !length.is_finite() {
        return Vec3::ZERO;
    }
    value * (soft_limit(length, max) / length)
}

/// Where a driver's parts hang, in its kart's frame (+Y up, -Z ahead, +X to the right): the
/// direction from each part's pivot toward its middle. An acceleration
/// across a part swings it; along it, nothing. None: the driver has no such part.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WobbleLevers {
    pub tail: Option<Vec3>,
    /// The left wing and the right one.
    pub wings: Option<[Vec3; 2]>,
}

/// What the driver looks like this frame, in its kart's frame: what `wobble_visual.rs` lays over
/// its animation. All identity at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WobblePose {
    /// The body's lean, around its bottom.
    pub lean: Quat,
    /// Its scale (x and z the width, y the height): squash below 1 in y, stretch above.
    pub squash: Vec3,
    pub tail: Quat,
    /// The left wing's and the right one's.
    pub wings: [Quat; 2],
}

impl Default for WobblePose {
    fn default() -> Self {
        Self {
            lean: Quat::IDENTITY,
            squash: Vec3::ONE,
            tail: Quat::IDENTITY,
            wings: [Quat::IDENTITY; 2],
        }
    }
}

/// The scale of a squash `s` (positive: squashed): `1 − s` high, as much wider as keeps its
/// volume.
pub fn squash_scale(s: f32) -> Vec3 {
    let height = (1.0 - s).max(0.05);
    let width = 1.0 / height.sqrt();
    Vec3::new(width, height, width)
}

/// What a driver's kart is going through this frame, as the screen shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WobbleInput {
    /// The frame's duration (s; 0: paused, nothing moves).
    pub dt: f32,
    /// The kart's pose on screen (interpolated): where it is and how it is turned (its frame).
    pub position: Vec3,
    pub rotation: Quat,
    /// Hit by an item: spinning out or tumbling (`KartState::Hit`).
    pub hit: Option<HitReaction>,
    /// Paw strikes received so far (`Paw::struck_count`), the last one pushing it toward this
    /// world direction (`Paw::struck_side`). None: no paw.
    pub struck: Option<(u32, Vec3)>,
}

impl WobbleInput {
    /// A kart standing at `position`, turned by `rotation`, for `dt` seconds.
    pub fn at(dt: f32, position: Vec3, rotation: Quat) -> Self {
        Self {
            dt,
            position,
            rotation,
            hit: None,
            struck: None,
        }
    }
}

/// A driver's wobble: its springs, and what it remembers of its kart's motion.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WobbleState {
    pub levers: WobbleLevers,
    pub lean: Spring<Vec3>,
    pub squash: Spring<f32>,
    pub tail: Spring<Vec3>,
    pub wings: [Spring<Vec3>; 2],
    /// Seconds of mini-ragdoll left.
    pub ragdoll: f32,
    /// The pose of the last step.
    pub pose: WobblePose,
    /// The kart's position on the last frame, and its velocity then (None: not measured yet).
    position: Option<Vec3>,
    velocity: Option<Vec3>,
    /// Its velocity seen through the shock window (world).
    slow: Vec3,
    /// How far the last shock went beyond the dead zone (m/s).
    shock: f32,
    was_hit: bool,
    struck: Option<u32>,
}

impl WobbleState {
    pub fn new(levers: WobbleLevers) -> Self {
        Self {
            levers,
            ..default()
        }
    }

    /// Every spring at rest: within `epsilon` (rad, or a share of the height), moving slower than
    /// ten times that a second.
    pub fn at_rest(&self, epsilon: f32) -> bool {
        self.lean.settled(epsilon)
            && self.squash.settled(epsilon)
            && self.tail.settled(epsilon)
            && self.wings.iter().all(|w| w.settled(epsilon))
    }

    /// How far the mini-ragdoll still is from its end: 1 when it starts, 0 when it is over.
    pub fn ragdoll_share(&self, tuning: &WobbleTuning) -> f32 {
        if tuning.ragdoll_seconds > 0.0 {
            (self.ragdoll / tuning.ragdoll_seconds).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Starts a mini-ragdoll, the head thrown toward `toward` (the kart's frame; zero: no kick).
    pub fn start_ragdoll(&mut self, toward: Vec3, tuning: &WobbleTuning) {
        self.ragdoll = tuning.ragdoll_seconds.max(0.0);
        let toward = toward.normalize_or_zero();
        let kick = tuning.ragdoll_kick;
        self.lean.speed += Vec3::Y.cross(toward) * kick;
        if let Some(tail) = self.levers.tail {
            // The tail whips the other way.
            self.tail.speed += tail.cross(-toward) * (kick * 1.5);
        }
        if let Some(wings) = self.levers.wings {
            // The wings fly up, flailing.
            for (spring, lever) in self.wings.iter_mut().zip(wings) {
                spring.speed += lever.cross(Vec3::Y) * (kick * 1.5);
            }
        }
    }

    /// Moves on by one frame of `input` (see the module's documentation), then sets `pose`.
    pub fn step(&mut self, input: &WobbleInput, tuning: &WobbleTuning) {
        if !tuning.enabled {
            *self = Self::new(self.levers);
            return;
        }
        let dt = input.dt;
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        let to_kart = input.rotation.inverse();
        // Measured from the motion on screen: the velocity of this frame, and its change since
        // the last one (the acceleration felt, as a kick).
        let moved = self.position.map(|p| input.position.distance(p));
        let fresh = dt <= MAX_FRAME && moved.is_some_and(|m| m <= tuning.teleport_distance);
        let velocity = match (fresh, self.position) {
            (true, Some(p)) => Some((input.position - p) / dt),
            _ => None,
        };
        let change = match (velocity, self.velocity) {
            (Some(now), Some(before)) => Some((now, to_kart * (now - before))),
            _ => None,
        };
        self.position = Some(input.position);
        self.velocity = velocity;
        if let Some((now, dv)) = change {
            self.feel(dv, tuning);
            self.shock(now, to_kart, dt, tuning);
        } else {
            // A first frame, a teleport or a hitch: the motion starts over from here.
            self.slow = velocity.unwrap_or_default();
            self.shock = 0.0;
        }

        // Hit by an item: a spin out throws the head aside (the kart turns right under it), a
        // tumble throws it back.
        let hit = input.hit;
        if let Some(reaction) = hit
            && !self.was_hit
        {
            let toward = match reaction {
                HitReaction::Spin => Vec3::new(-1.0, 0.0, 0.3),
                HitReaction::Tumble => Vec3::Z,
            };
            self.start_ragdoll(toward, tuning);
        }
        self.was_hit = hit.is_some();
        // Struck by a paw: the kart is shoved away, the head stays toward the paw.
        if let Some((count, side)) = input.struck {
            if self.struck.is_some_and(|seen| count > seen) {
                let toward = (to_kart * -side).with_y(0.0);
                self.start_ragdoll(toward, tuning);
            }
            self.struck = Some(count);
        }

        self.swing(dt, tuning);
        self.ragdoll = (self.ragdoll - dt).max(0.0);
    }

    /// The acceleration felt this frame (`dv`: the velocity change, in the kart's frame): each
    /// spring is kicked as a steady acceleration would lean it (`per_accel`).
    fn feel(&mut self, dv: Vec3, tuning: &WobbleTuning) {
        let gain = |spring: &SpringTuning| {
            let omega = TAU * clamp_finite(spring.frequency, FREQUENCY_RANGE);
            omega * omega * finite(spring.per_accel)
        };
        // Inertia: the body lags behind, as if pushed by −dv at its middle.
        self.lean.speed += Vec3::Y.cross(-dv) * gain(&tuning.body);
        self.squash.speed += dv.y * gain(&tuning.squash);
        if let Some(tail) = self.levers.tail {
            self.tail.speed += tail.cross(-dv) * gain(&tuning.tail);
        }
        if let Some(wings) = self.levers.wings {
            for (spring, lever) in self.wings.iter_mut().zip(wings) {
                spring.speed += lever.cross(-dv) * gain(&tuning.wings);
            }
        }
    }

    /// A shock: the horizontal velocity change over the shock window, beyond its dead zone,
    /// squashes the body (by what it adds to the last frame's); a big one starts a mini-ragdoll.
    fn shock(&mut self, velocity: Vec3, to_kart: Quat, dt: f32, tuning: &WobbleTuning) {
        let sudden = (to_kart * (velocity - self.slow)).with_y(0.0).length();
        let excess = (sudden - finite(tuning.shock_deadzone)).max(0.0);
        if excess > self.shock {
            let omega = TAU * clamp_finite(tuning.squash.frequency, FREQUENCY_RANGE);
            self.squash.speed += (excess - self.shock) * finite(tuning.shock_squash) * omega;
            // Only as it crosses the threshold: a shock over several frames starts one.
            let before = self.shock + finite(tuning.shock_deadzone);
            if sudden >= tuning.ragdoll_shock && before < tuning.ragdoll_shock {
                self.start_ragdoll(Vec3::ZERO, tuning);
            }
        }
        self.shock = excess;
        let window = tuning.shock_window.max(1e-3);
        self.slow += (velocity - self.slow) * (1.0 - (-dt / window).exp());
    }

    /// The springs move on (floppier during a mini-ragdoll), held within their limits; then the
    /// pose.
    fn swing(&mut self, dt: f32, tuning: &WobbleTuning) {
        let share = self.ragdoll_share(tuning);
        let blend = |normal: f32, ragdoll: f32| normal * (1.0 + (finite(ragdoll) - 1.0) * share);
        let run = |spring: &SpringTuning| {
            (
                blend(spring.frequency, tuning.ragdoll_frequency),
                blend(spring.damping, tuning.ragdoll_damping),
                blend(finite(spring.max).clamp(0.0, FRAC_PI_2), tuning.ragdoll_max)
                    .clamp(0.0, FRAC_PI_2),
            )
        };
        let (frequency, damping, body_max) = run(&tuning.body);
        self.lean.step(frequency, damping, dt);
        self.lean.hold(body_max * HARD_LIMIT);
        self.lean.snap();

        let (frequency, damping, squash_max) = run(&tuning.squash);
        let squash_max = squash_max.min(0.9);
        let stretch_max = blend(
            finite(tuning.max_stretch).clamp(0.0, 0.9),
            tuning.ragdoll_max,
        )
        .clamp(0.0, 0.9);
        self.squash.step(frequency, damping, dt);
        self.squash
            .hold(stretch_max * HARD_LIMIT, squash_max * HARD_LIMIT);
        self.squash.snap();

        let (frequency, damping, tail_max) = run(&tuning.tail);
        if self.levers.tail.is_some() {
            self.tail.step(frequency, damping, dt);
            self.tail.hold(tail_max * HARD_LIMIT);
            self.tail.snap();
        }
        let (frequency, damping, wing_max) = run(&tuning.wings);
        if self.levers.wings.is_some() {
            for spring in &mut self.wings {
                spring.step(frequency, damping, dt);
                spring.hold(wing_max * HARD_LIMIT);
                spring.snap();
            }
        }

        let s = self.squash.value;
        let shown = if s >= 0.0 {
            soft_limit(s, squash_max)
        } else {
            -soft_limit(-s, stretch_max)
        };
        let rotation = |spring: &Spring<Vec3>, max: f32| {
            Quat::from_scaled_axis(soft_limit_vec(spring.value, max))
        };
        self.pose = WobblePose {
            lean: rotation(&self.lean, body_max),
            squash: squash_scale(shown),
            tail: rotation(&self.tail, tail_max),
            wings: [
                rotation(&self.wings[0], wing_max),
                rotation(&self.wings[1], wing_max),
            ],
        };
    }
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// How a wobble is laid over a part: around its parent's origin (the driver's primitives: their
/// parent is the driver, its origin on the seat), or around its own origin (an animal's node,
/// whose `Transform` its clip writes: the body's origin is its bottom, the tail's and the
/// wings' their roots).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerPivot {
    Parent,
    Own,
}

/// `base` (the part's pose: its rest, or its clip's pose of this frame) with `rotation` and
/// `scale` laid over it, in its parent's frame. Never replaces the pose: identity gives `base`.
pub fn layered(base: &Transform, pivot: LayerPivot, rotation: Quat, scale: Vec3) -> Transform {
    match pivot {
        LayerPivot::Parent => Transform {
            translation: rotation * (scale * base.translation),
            rotation: rotation * base.rotation,
            scale: scale * base.scale,
        },
        LayerPivot::Own => Transform {
            translation: base.translation,
            rotation: rotation * base.rotation,
            scale: base.scale * scale,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: [f32; 4] = [1.0 / 30.0, 1.0 / 60.0, 1.0 / 120.0, 1.0 / 240.0];

    /// Seconds until `spring` (kicked) stays within `epsilon` of rest, its energy never growing on
    /// the way; panics if it is still moving after `limit` seconds.
    fn settle<T>(
        mut spring: Spring<T>,
        frequency: f32,
        damping: f32,
        dt: f32,
        epsilon: f32,
        limit: f32,
        energy: impl Fn(&Spring<T>) -> f32,
        size: impl Fn(&Spring<T>) -> f32,
    ) -> f32
    where
        T: Copy + Default + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
    {
        let mut t = 0.0;
        let mut last = energy(&spring);
        let mut since_rest = None;
        while t < limit {
            spring.step(frequency, damping, dt);
            t += dt;
            let now = energy(&spring);
            assert!(now.is_finite(), "diverged at {t} s");
            assert!(
                now <= last * (1.0 + 1e-5) + 1e-12,
                "energy grew: {last} → {now}"
            );
            last = now;
            if size(&spring) < epsilon {
                since_rest.get_or_insert(t);
            } else {
                since_rest = None;
            }
        }
        since_rest.unwrap_or_else(|| {
            panic!("still moving after {limit} s ({frequency} Hz, {damping}, {dt})")
        })
    }

    fn k(frequency: f32) -> f32 {
        let omega = TAU * clamp_finite(frequency, FREQUENCY_RANGE);
        omega * omega
    }

    /// The issue's criterion: after a kick, every spring of the default tuning is back at rest
    /// within 3 s, at 30, 60, 120 and 240 fps, without ever gaining energy; and the frame rate
    /// barely changes when (within 0.1 s).
    #[test]
    fn a_kicked_spring_comes_back_to_rest_at_any_frame_rate() {
        let tuning = WobbleTuning::default();
        for spring in [tuning.body, tuning.squash, tuning.tail, tuning.wings] {
            let mut times = Vec::new();
            for dt in FRAMES {
                let kicked = Spring {
                    value: Vec3::ZERO,
                    speed: Vec3::new(4.0, 0.0, -3.0),
                };
                let t = settle(
                    kicked,
                    spring.frequency,
                    spring.damping,
                    dt,
                    1e-3,
                    10.0,
                    |s| s.energy(k(spring.frequency)),
                    |s| s.value.length().max(s.speed.length() / 10.0),
                );
                assert!(t < 3.0, "{spring:?} at {dt}: {t} s");
                times.push(t);
                // The squash is a scalar spring: the same.
                let scalar = Spring {
                    value: 0.0,
                    speed: 5.0,
                };
                let t = settle(
                    scalar,
                    spring.frequency,
                    spring.damping,
                    dt,
                    1e-3,
                    10.0,
                    |s| s.energy(k(spring.frequency)),
                    |s| s.value.abs().max(s.speed.abs() / 10.0),
                );
                assert!(t < 3.0, "{spring:?} at {dt}: {t} s");
            }
            let (lo, hi) = times
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(*t), hi.max(*t)));
            assert!(hi - lo < 0.1, "{spring:?}: {times:?}");
        }
    }

    /// The extreme tunings (the stiffest, the softest, no damping, overdamped, and values out of
    /// range or not even numbers): never unstable, never gaining energy, and back at rest within
    /// the time their slowest decay allows.
    #[test]
    fn extreme_tunings_never_diverge() {
        let frequencies = [
            0.0,
            FREQUENCY_RANGE.0,
            1.0,
            FREQUENCY_RANGE.1,
            1000.0,
            f32::NAN,
        ];
        let dampings = [-1.0, DAMPING_RANGE.0, 1.0, DAMPING_RANGE.1, 50.0, f32::NAN];
        for frequency in frequencies {
            for damping in dampings {
                for dt in FRAMES {
                    let f = clamp_finite(frequency, FREQUENCY_RANGE);
                    let z = clamp_finite(damping, DAMPING_RANGE);
                    let omega = TAU * f;
                    // The slowest mode decays at ω(ζ − √(ζ² − 1)) when overdamped, ζω otherwise.
                    let rate = if z > 1.0 {
                        omega * (z - (z * z - 1.0).sqrt())
                    } else {
                        z * omega
                    };
                    let limit = (1e3f32).ln() / rate * 1.5 + 1.0;
                    let kicked = Spring {
                        value: Vec3::new(0.5, 0.0, 0.0),
                        speed: Vec3::new(0.0, 0.0, 20.0),
                    };
                    settle(
                        kicked,
                        frequency,
                        damping,
                        dt,
                        2e-3,
                        limit,
                        |s| s.energy(k(frequency)),
                        |s| s.value.length(),
                    );
                }
            }
        }
        // A frame of no time or of NaN does nothing; a frame too long is cut to `MAX_FRAME`.
        let mut spring = Spring {
            value: 1.0,
            speed: 0.0,
        };
        spring.step(2.0, 0.3, 0.0);
        spring.step(2.0, 0.3, f32::NAN);
        assert_eq!(spring.value, 1.0);
        spring.step(2.0, 0.3, 1e9);
        assert!(spring.value.is_finite() && spring.value.abs() < 1.0);
    }

    /// The driver's frame: the kart turned `yaw` around Y.
    fn kart(yaw: f32) -> Quat {
        Quat::from_rotation_y(yaw)
    }

    /// A driver whose kart moves along `positions` (one per frame of `dt`), facing `rotation`;
    /// returns the poses.
    fn ride(
        state: &mut WobbleState,
        tuning: &WobbleTuning,
        dt: f32,
        positions: impl IntoIterator<Item = (Vec3, Quat)>,
    ) -> Vec<WobblePose> {
        positions
            .into_iter()
            .map(|(position, rotation)| {
                state.step(&WobbleInput::at(dt, position, rotation), tuning);
                state.pose
            })
            .collect()
    }

    /// The head's tip (1 m above the seat) under a lean.
    fn head(pose: &WobblePose) -> Vec3 {
        pose.lean * Vec3::Y
    }

    fn birds_levers() -> WobbleLevers {
        WobbleLevers {
            tail: Some(Vec3::new(0.0, 0.6, 0.8).normalize()),
            wings: Some([Vec3::NEG_X, Vec3::X]),
        }
    }

    /// The kart's velocity along -Z (its front), per frame: `speed(t)` in m/s.
    fn straight(dt: f32, seconds: f32, speed: impl Fn(f32) -> f32) -> Vec<(Vec3, Quat)> {
        let mut z = 0.0;
        (0..(seconds / dt).round() as usize)
            .map(|i| {
                z -= speed(i as f32 * dt) * dt;
                (Vec3::new(0.0, 0.0, z), Quat::IDENTITY)
            })
            .collect()
    }

    /// The directions: braking leans the head forward (-Z), the engine back (+Z), a left turn
    /// leans it out to the right (+X), and the tail swings out too; a steady motion leans
    /// nothing. Whatever way the kart faces in the world.
    #[test]
    fn the_driver_leans_against_the_acceleration() {
        let tuning = WobbleTuning::default();
        let dt = 1.0 / 60.0;
        // Accelerating at 10 m/s².
        let mut state = WobbleState::new(birds_levers());
        let poses = ride(&mut state, &tuning, dt, straight(dt, 0.4, |t| 10.0 * t));
        assert!(head(poses.last().unwrap()).z > 0.02, "leans back");
        // Braking at 28 m/s² from 28 m/s.
        let mut state = WobbleState::new(birds_levers());
        let mut positions = straight(dt, 0.3, |_| 28.0);
        let end = positions.last().unwrap().0;
        positions.extend(
            straight(dt, 0.4, |t| 28.0 - 28.0 * t)
                .into_iter()
                .map(|(p, r)| (p + end, r)),
        );
        let poses = ride(&mut state, &tuning, dt, positions);
        let lean = head(poses.last().unwrap()).z;
        assert!(lean < -0.05, "leans forward: {lean}");
        // Steady, after a while: upright again.
        let mut state = WobbleState::new(birds_levers());
        let poses = ride(&mut state, &tuning, dt, straight(dt, 4.0, |_| 20.0));
        assert!(poses.last().unwrap().lean.angle_between(Quat::IDENTITY) < 1e-3);
        assert!(state.at_rest(1e-3));

        // A left turn at 20 m/s, radius 10 m (40 m/s² toward the left), the kart facing every
        // way: the head out of the turn, to the kart's right (+X), about per_accel × 40.
        for start in [0.0, 1.0, 2.5, -2.0] {
            let mut state = WobbleState::new(birds_levers());
            let positions = (0..120).map(|i| {
                let angle = start + i as f32 * dt * 2.0;
                // Around a center on the kart's left: moving along its -Z.
                let center = Vec3::ZERO;
                let at = center + Vec3::new(angle.cos(), 0.0, -angle.sin()) * 10.0;
                (at, kart(angle))
            });
            let poses = ride(&mut state, &tuning, dt, positions);
            let pose = poses.last().unwrap();
            let lean = head(pose);
            assert!(lean.x > 0.1, "{start}: {lean}");
            assert!(lean.x < tuning.body.max, "{start}: {lean}");
            assert!(lean.z.abs() < 0.05, "{start}: {lean}");
            // The tail (behind and up) swings out of the turn too.
            let tip = pose.tail * Vec3::new(0.0, 0.6, 0.8).normalize();
            assert!(tip.x > 0.05, "{start}: {tip}");
        }
    }

    /// A landing: the kart falls at 9 m/s (stretched in the air), then the suspension stops it in
    /// a few frames: squashed, never beyond its limit, then back to rest; about the same at any
    /// frame rate. The wings flop down then.
    #[test]
    fn a_landing_squashes_then_comes_back() {
        let tuning = WobbleTuning::default();
        let mut peaks = Vec::new();
        for dt in FRAMES {
            let mut state = WobbleState::new(birds_levers());
            let g = 21.6;
            // From 1.9 m: 9 m/s at the ground.
            let mut y = 1.9;
            let mut vy = 0.0f32;
            let mut t = 0.0;
            let mut stretched = 0.0f32;
            let mut squashed = 0.0f32;
            let mut wings_down = 0.0f32;
            let mut landed_at = None;
            let mut rest_at = None;
            while t < 5.0 {
                // The suspension stops the fall within 0.08 s.
                if y <= 0.0 {
                    landed_at.get_or_insert(t);
                    vy = (vy + 9.0 / 0.08 * dt).min(0.0);
                } else {
                    vy -= g * dt;
                }
                y += vy * dt;
                t += dt;
                state.step(
                    &WobbleInput::at(dt, Vec3::new(0.0, y, 0.0), kart(0.3)),
                    &tuning,
                );
                let height = state.pose.squash.y;
                assert!(
                    (1.0 - tuning.squash.max - 1e-4..=1.0 + tuning.max_stretch + 1e-4)
                        .contains(&height),
                    "{height}"
                );
                if landed_at.is_none() {
                    stretched = stretched.max(height - 1.0);
                } else {
                    squashed = squashed.max(1.0 - height);
                    let left_tip = state.pose.wings[0] * Vec3::NEG_X;
                    wings_down = wings_down.min(left_tip.y);
                    if state.at_rest(1e-3) {
                        rest_at.get_or_insert(t);
                    } else {
                        rest_at = None;
                    }
                }
            }
            assert!(stretched > 0.01, "stretched in the air: {stretched}");
            assert!(squashed > 0.1, "{dt}: squashed {squashed}");
            assert!(wings_down < -0.1, "{dt}: wings {wings_down}");
            let landed = landed_at.unwrap();
            let rest = rest_at.expect("never at rest");
            assert!(
                rest - landed < 3.0,
                "{dt}: at rest {} s after",
                rest - landed
            );
            peaks.push(squashed);
        }
        let (lo, hi) = peaks
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        assert!(hi / lo < 1.25, "{peaks:?}");
    }

    /// A shock: into a wall at 25 m/s, the kart stopped in two ticks. The head thrown forward,
    /// the body squashed, a mini-ragdoll; all within limits, then back to rest. Rubbing a wall
    /// (a 2 m/s nudge) is no shock.
    #[test]
    fn a_crash_squashes_and_sets_off_a_ragdoll() {
        let tuning = WobbleTuning::default();
        for dt in FRAMES {
            let mut state = WobbleState::new(birds_levers());
            let mut positions = straight(dt, 1.0, |_| 25.0);
            let end = positions.last().unwrap().0;
            positions.extend(
                straight(dt, 4.0, |t| (25.0 - t * 25.0 * 30.0).max(0.0))
                    .into_iter()
                    .map(|(p, r)| (p + end, r)),
            );
            let mut forward = 0.0f32;
            let mut squashed = 0.0f32;
            let mut ragdoll = false;
            let mut rest = None;
            for (i, (position, rotation)) in positions.into_iter().enumerate() {
                state.step(&WobbleInput::at(dt, position, rotation), &tuning);
                let t = i as f32 * dt;
                ragdoll |= state.ragdoll > 0.0;
                forward = forward.min(head(&state.pose).z);
                squashed = squashed.max(1.0 - state.pose.squash.y);
                assert!(
                    state.pose.lean.angle_between(Quat::IDENTITY)
                        <= tuning.body.max * tuning.ragdoll_max + 1e-4
                );
                assert!(1.0 - state.pose.squash.y <= tuning.squash.max * tuning.ragdoll_max + 1e-4);
                if t > 1.0 && state.at_rest(1e-3) {
                    rest.get_or_insert(t);
                } else {
                    rest = None;
                }
            }
            assert!(ragdoll, "{dt}");
            assert!(forward < -0.3, "{dt}: head forward {forward}");
            assert!(squashed > 0.1, "{dt}: squashed {squashed}");
            let rest = rest.expect("never at rest");
            assert!(rest - 1.0 < 4.0, "{dt}: {rest}");
        }
        // A nudge: no shock, no ragdoll.
        let dt = 1.0 / 60.0;
        let mut state = WobbleState::new(birds_levers());
        let mut x = 0.0;
        for i in 0..120 {
            if i == 60 {
                x += 2.0 * dt;
            }
            state.step(
                &WobbleInput::at(dt, Vec3::new(x, 0.0, -(i as f32) * 0.4), Quat::IDENTITY),
                &tuning,
            );
            assert_eq!(state.ragdoll, 0.0);
            assert!(1.0 - state.pose.squash.y < 0.01);
        }
    }

    /// Hit by an item, struck by a paw: a mini-ragdoll of about 1 s. The head thrown (aside for a
    /// spin out, back for a tumble, toward the paw for a strike), the springs floppier then
    /// stiff again, within their limits, then back to rest. Staying hit does not kick again; a
    /// strike already counted neither.
    #[test]
    fn a_hit_or_a_strike_ragdolls_for_a_second() {
        let tuning = WobbleTuning::default();
        let dt = 1.0 / 120.0;
        let still = |state: &mut WobbleState, hit, struck| {
            state.step(
                &WobbleInput {
                    hit,
                    struck,
                    ..WobbleInput::at(dt, Vec3::ZERO, kart(1.2))
                },
                &tuning,
            );
        };
        // The strike pushed the kart toward the world's +X: in the kart's frame (turned 1.2 rad)…
        let push = Vec3::X;
        let toward_paw = (kart(1.2).inverse() * -push).normalize();
        let cases: [(Option<HitReaction>, Option<(u32, Vec3)>, Vec3); 3] = [
            (
                Some(HitReaction::Spin),
                None,
                Vec3::new(-1.0, 0.0, 0.3).normalize(),
            ),
            (Some(HitReaction::Tumble), None, Vec3::Z),
            (None, Some((1, push)), toward_paw),
        ];
        for (hit, struck, toward) in cases {
            let mut state = WobbleState::new(birds_levers());
            // Before: no hit, no strike yet (the count seen first is not a strike).
            for _ in 0..30 {
                still(&mut state, None, struck.map(|(_, side)| (0, side)));
            }
            assert!(state.at_rest(1e-6));
            let mut thrown = 0.0f32;
            let mut ragdoll_end = None;
            let mut rest = None;
            for i in 0..600 {
                let t = i as f32 * dt;
                // Hit for half a second; the strike counted once.
                still(&mut state, hit.filter(|_| t < 0.5), struck);
                thrown = thrown.max(head(&state.pose).dot(toward));
                assert!(
                    state.pose.lean.angle_between(Quat::IDENTITY)
                        <= tuning.body.max * tuning.ragdoll_max + 1e-4
                );
                if state.ragdoll == 0.0 {
                    ragdoll_end.get_or_insert(t);
                }
                if state.at_rest(1e-3) {
                    rest.get_or_insert(t);
                } else {
                    rest = None;
                }
            }
            assert!(thrown > 0.15, "{hit:?} {struck:?}: {thrown}");
            let end = ragdoll_end.unwrap();
            assert!((end - tuning.ragdoll_seconds).abs() < 0.02, "{end}");
            let rest = rest.expect("never at rest");
            assert!(rest < 4.0, "{hit:?}: {rest}");
            // Then exactly at rest: the parts are no longer written.
            assert_eq!(state.pose, WobblePose::default());
        }
        // Floppier: the same kick swings longer during a ragdoll than without.
        let mut floppy = WobbleState::new(WobbleLevers::default());
        let mut stiff = floppy;
        floppy.start_ragdoll(Vec3::X, &tuning);
        stiff.lean.speed = floppy.lean.speed;
        let swing = |state: &mut WobbleState| {
            let mut first_back = None;
            for i in 0..240 {
                still(state, None, None);
                if state.lean.value.z >= 0.0 && i > 0 {
                    first_back.get_or_insert(i);
                }
            }
            first_back.unwrap()
        };
        assert!(swing(&mut floppy) > swing(&mut stiff));
    }

    /// A teleport (the pelican's drop, a restart) or a hitch measures nothing; a paused frame
    /// changes nothing; switched off, the driver sits still.
    #[test]
    fn teleports_pauses_and_off_move_nothing() {
        let tuning = WobbleTuning::default();
        let dt = 1.0 / 60.0;
        let mut state = WobbleState::new(birds_levers());
        let mut positions = straight(dt, 1.0, |_| 20.0);
        // Dropped 30 m away, standing still.
        let drop = Vec3::new(30.0, 2.0, 5.0);
        positions.extend((0..60).map(|_| (drop, Quat::IDENTITY)));
        for (position, rotation) in positions {
            state.step(&WobbleInput::at(dt, position, rotation), &tuning);
            assert!(state.at_rest(1e-3), "{state:?}");
        }
        // A frame of 0.5 s (the tab in the background), far away: nothing.
        state.step(&WobbleInput::at(0.5, Vec3::ZERO, Quat::IDENTITY), &tuning);
        state.step(
            &WobbleInput::at(dt, Vec3::new(0.0, 0.0, -0.3), Quat::IDENTITY),
            &tuning,
        );
        assert!(state.at_rest(1e-3));
        // Paused: nothing moves.
        state.lean.speed = Vec3::X;
        let before = state;
        state.step(
            &WobbleInput::at(0.0, Vec3::new(5.0, 0.0, 0.0), Quat::IDENTITY),
            &tuning,
        );
        assert_eq!(state, before);
        // Off: at rest, its levers kept.
        let off = WobbleTuning {
            enabled: false,
            ..default()
        };
        state.step(&WobbleInput::at(dt, Vec3::ZERO, Quat::IDENTITY), &off);
        assert_eq!(state, WobbleState::new(birds_levers()));
        assert_eq!(state.pose, WobblePose::default());
    }

    /// The layer over a part never replaces its pose: identity gives the pose back; around its
    /// own origin (an animal's node), the origin stays put, the rotation multiplies the clip's;
    /// around the parent's origin (the primitives), the seat stays put, and a squashed ball sits
    /// lower but still on the seat.
    #[test]
    fn the_wobble_is_laid_over_the_pose() {
        let clip = Transform::from_xyz(0.1, 0.18, 0.0)
            .with_rotation(Quat::from_rotation_z(0.2))
            .with_scale(Vec3::splat(1.1));
        for pivot in [LayerPivot::Own, LayerPivot::Parent] {
            let same = layered(&clip, pivot, Quat::IDENTITY, Vec3::ONE);
            assert!(same.translation.distance(clip.translation) < 1e-6);
            assert!(same.rotation.angle_between(clip.rotation) < 1e-6);
            assert!(same.scale.distance(clip.scale) < 1e-6);
        }
        let lean = Quat::from_rotation_x(0.3);
        let squash = squash_scale(0.2);
        let own = layered(&clip, LayerPivot::Own, lean, squash);
        assert_eq!(own.translation, clip.translation);
        assert!(own.rotation.angle_between(lean * clip.rotation) < 1e-6);
        assert!((own.scale.y - 1.1 * 0.8).abs() < 1e-5);
        // The volume is kept.
        assert!((squash.x * squash.y * squash.z - 1.0).abs() < 1e-5);
        // A ball of radius 0.45 sitting on the seat: squashed, its center lower, its bottom on it.
        let ball = Transform::from_xyz(0.0, 0.45, 0.0);
        let squashed = layered(&ball, LayerPivot::Parent, Quat::IDENTITY, squash);
        assert!((squashed.translation.y - 0.45 * 0.8).abs() < 1e-5);
        let bottom = squashed.transform_point(Vec3::new(0.0, -0.45, 0.0));
        assert!(bottom.length() < 1e-5, "{bottom}");
        // Leaning: the ball's center turns around the seat.
        let leaned = layered(&ball, LayerPivot::Parent, lean, Vec3::ONE);
        assert!((leaned.translation - lean * ball.translation).length() < 1e-6);
        // The limits: soft, never beyond.
        assert!((soft_limit(0.01, 0.5) - 0.01).abs() < 1e-4);
        assert!(soft_limit(100.0, 0.5) <= 0.5);
        assert_eq!(soft_limit(1.0, 0.0), 0.0);
        assert!(soft_limit_vec(Vec3::new(0.0, 3.0, 4.0), 0.5).length() <= 0.5);
        assert_eq!(soft_limit_vec(Vec3::splat(f32::NAN), 0.5), Vec3::ZERO);
    }
}
