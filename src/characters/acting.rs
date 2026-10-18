//! Issue #130: what the karts' bodies and their drivers act out (GD § 14 « Les animations des
//! personnages (#130) »), on top of the wobble of #12 and the clips of #69. Pure here (tested
//! without Bevy's schedules); `acting_visual.rs` feeds it each frame and lays its poses over the
//! karts' shells and their drivers, **only with a renderer**.
//!
//! - **The body** ([`BodyWobble`]): the kart's shell tilts and squashes over its wheels, as a
//!   soft box would. Its springs feel the kart's motion on screen: braking dives its nose, a turn
//!   rolls it out of the turn, a landing **squashes it then it bounces back up** (stretched in the
//!   air), a bump or a landing nose first **rocks it** (the change of the kart's pitch and roll).
//!   The wheels stay where the suspension puts them: only the shell and the driver's seat move.
//! - **The driver** ([`DriverActing`]):
//!   - **a figure** on a trick ramp (GD § 5): it pops up out of its seat, stretched, does a
//!     backflip (the next time a pirouette, and so on), and drops back in, squashed;
//!   - **it looks back**: at the kart that just overtook it (following it as it goes by), or over
//!     its shoulder at an item it threw or dropped behind;
//!   - **it sulks** over the line among the last: it slumps in its seat, turned away, sighing,
//!     until the next race (its clip shakes its head first, `animation.rs`);
//!   - **seeing stars** (« sonné », the stars of #105): its head goes round and round.
//!
//! **Inputs** come from what every peer already has: the kart's visual (where it is shown), its
//! state, its race progress, the items out, its stars. Nothing here touches the race, nothing is
//! sent.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use super::animation::DriverTuning;
use super::wobble::{
    FREQUENCY_RANGE, MAX_FRAME, Spring, SpringTuning, soft_limit, soft_limit_vec, squash_scale,
};

/// The middle of a driver, above its seat (m, at size 1): a figure turns around it.
pub const DRIVER_MIDDLE: f32 = 0.45;
/// A spring this close to rest is at rest (as the wobble's).
const SNAP: f32 = 1e-5;

/// The kart's body over its wheels (issue #130, `WobbleTuning::kart`): its tilt and its squash.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct KartWobbleTuning {
    /// The shells tilt and squash. Off: they sit still on their wheels (to compare).
    pub enabled: bool,
    /// The tilt (pitch and roll) against the acceleration, around the bottom of the body:
    /// 0.0012 rad per m/s², about 3° in a full-speed turn (47 m/s²), 2° braking (28 m/s²).
    pub tilt: SpringTuning,
    /// A change of the kart's pitch or roll rate (a bump, a landing nose first) rocks the body:
    /// it lags behind by this share of it (0: not at all, 1: as a loose box), then rocks back.
    pub rock: f32,
    /// The squash (positive: lower and wider, the volume kept) along the body: 0.0011 per m/s², a
    /// landing at 9 m/s squashes it by about 17 %, 2 % taller in the air.
    pub squash: SpringTuning,
    /// The largest stretch (0.12: 12 % taller).
    pub max_stretch: f32,
    /// The height (m, in the kart's frame) of the bottom of the body, around which it tilts and
    /// squashes: the floor of the shells.
    pub pivot_height: f32,
}

impl Default for KartWobbleTuning {
    fn default() -> Self {
        Self {
            enabled: true,
            tilt: SpringTuning {
                frequency: 3.0,
                damping: 0.32,
                per_accel: 0.0012,
                max: 0.12,
            },
            rock: 0.5,
            squash: SpringTuning {
                frequency: 4.0,
                damping: 0.3,
                per_accel: 0.0011,
                max: 0.25,
            },
            max_stretch: 0.12,
            pivot_height: -0.2,
        }
    }
}

/// What a kart's body looks like this frame, in the kart's frame: its tilt (around its bottom)
/// and its scale (x and z the width, y the height). Identity at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPose {
    pub tilt: Quat,
    pub squash: Vec3,
}

impl Default for BodyPose {
    fn default() -> Self {
        Self {
            tilt: Quat::IDENTITY,
            squash: Vec3::ONE,
        }
    }
}

impl BodyPose {
    pub fn at_rest(&self) -> bool {
        *self == Self::default()
    }
}

/// A kart body's springs, and what they remember of the kart's motion on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyWobble {
    pub tilt: Spring<Vec3>,
    pub squash: Spring<f32>,
    pub pose: BodyPose,
    position: Option<Vec3>,
    velocity: Option<Vec3>,
    rotation: Option<Quat>,
    /// Its turning rate on the last frame (rad/s, the kart's frame).
    spin: Option<Vec3>,
}

/// The kick of a spring of `tuning` by an acceleration (as the wobble's: `ω² · per_accel`).
fn gain(tuning: &SpringTuning) -> f32 {
    let omega = TAU * finite(tuning.frequency).clamp(FREQUENCY_RANGE.0, FREQUENCY_RANGE.1);
    omega * omega * finite(tuning.per_accel)
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

impl BodyWobble {
    /// Every spring at rest, within `epsilon`.
    pub fn at_rest(&self, epsilon: f32) -> bool {
        self.tilt.settled(epsilon) && self.squash.settled(epsilon)
    }

    /// Moves on by a frame of `dt` s, the kart shown at `position`, turned by `rotation`; a jump
    /// of more than `teleport` m (a pelican, a restart) or a hitch measures nothing.
    pub fn step(
        &mut self,
        dt: f32,
        position: Vec3,
        rotation: Quat,
        tuning: &KartWobbleTuning,
        teleport: f32,
    ) {
        if !tuning.enabled {
            *self = Self::default();
            return;
        }
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        let to_kart = rotation.inverse();
        let fresh = dt <= MAX_FRAME
            && self
                .position
                .is_some_and(|p| p.distance(position) <= teleport);
        let (velocity, spin) = match (fresh, self.position, self.rotation) {
            (true, Some(p), Some(r)) => {
                // The turn of this frame, the short way round, in the kart's frame.
                let mut turn = rotation * r.inverse();
                if turn.w < 0.0 {
                    turn = -turn;
                }
                (
                    Some((position - p) / dt),
                    Some(to_kart * (turn.to_scaled_axis() / dt)),
                )
            }
            _ => (None, None),
        };
        if let (Some(now), Some(before), Some(spin), Some(spun)) =
            (velocity, self.velocity, spin, self.spin)
        {
            // Inertia: the body lags behind the kart's acceleration (as the driver does)...
            let dv = to_kart * (now - before);
            self.tilt.speed += Vec3::Y.cross(-dv) * gain(&tuning.tilt);
            self.squash.speed += dv.y * gain(&tuning.squash);
            // ...and behind a change of its pitch and roll (never its heading: a turn).
            let rocked = (spin - spun) * Vec3::new(1.0, 0.0, 1.0);
            self.tilt.speed -= rocked * finite(tuning.rock).clamp(0.0, 2.0);
        }
        self.position = Some(position);
        self.rotation = Some(rotation);
        self.velocity = velocity;
        self.spin = spin;

        let tilt_max = finite(tuning.tilt.max).clamp(0.0, 0.6);
        let squash_max = finite(tuning.squash.max).clamp(0.0, 0.9);
        let stretch_max = finite(tuning.max_stretch).clamp(0.0, 0.9);
        self.tilt
            .step(tuning.tilt.frequency, tuning.tilt.damping, dt);
        self.tilt.hold(tilt_max * 2.0);
        self.tilt.snap();
        self.squash
            .step(tuning.squash.frequency, tuning.squash.damping, dt);
        self.squash.hold(stretch_max * 2.0, squash_max * 2.0);
        self.squash.snap();
        let s = self.squash.value;
        let shown = if s >= 0.0 {
            soft_limit(s, squash_max)
        } else {
            -soft_limit(-s, stretch_max)
        };
        self.pose = if self.at_rest(SNAP) && self.tilt.value == Vec3::ZERO {
            BodyPose::default()
        } else {
            BodyPose {
                tilt: Quat::from_scaled_axis(soft_limit_vec(self.tilt.value, tilt_max)),
                squash: squash_scale(shown),
            }
        };
    }
}

/// The shell's transform under its pivot for `pose`, tilted and squashed around `pivot` (the
/// bottom of the body, in the kart's frame): identity at rest.
pub fn shell_transform(pose: &BodyPose, pivot: Vec3) -> Transform {
    if pose.at_rest() {
        return Transform::IDENTITY;
    }
    Transform {
        translation: pivot - pose.tilt * (pose.squash * pivot),
        rotation: pose.tilt,
        scale: pose.squash,
    }
}

/// A trick's figure (GD § 5): each kart does them in turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Figure {
    /// A backflip, head back first.
    Backflip,
    /// A full turn on itself, to its left.
    Pirouette,
}

impl Figure {
    /// The `n`-th figure of a kart (0 first): a backflip, a pirouette, a backflip...
    pub fn nth(n: u32) -> Self {
        if n.is_multiple_of(2) {
            Figure::Backflip
        } else {
            Figure::Pirouette
        }
    }
}

/// What a driver acts out this frame, in its own frame (at size 1; its front is −Z): lifted by
/// `offset`, turned by `turn` around its seat (looking back, sulking, dizzy), by `spin` around
/// its middle (a figure), scaled by `scale` (stretched, squashed). Identity at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActingPose {
    pub offset: Vec3,
    pub turn: Quat,
    pub spin: Quat,
    pub scale: Vec3,
}

impl Default for ActingPose {
    fn default() -> Self {
        Self {
            offset: Vec3::ZERO,
            turn: Quat::IDENTITY,
            spin: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl ActingPose {
    pub fn at_rest(&self) -> bool {
        *self == Self::default()
    }
}

/// What a driver's kart is going through this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActingFacts {
    /// The frame's duration (s; 0: paused, nothing moves).
    pub dt: f32,
    /// Doing a trick (`KartState::Trick`): a new one starts a figure.
    pub tricking: bool,
    /// Over the line among the last (`animation::sulks`).
    pub sulking: bool,
    /// Seeing stars for this long still (s, `vfx::KartVfx::dizzy`; 0: not).
    pub dizzy: f32,
    /// Which way it turns away when it sulks (−1: to its right, 1: to its left).
    pub side: f32,
}

/// A driver's acting: its figure, where it looks, its sulk, its dizziness, and its pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DriverActing {
    /// The figure going on, and how long into it (s).
    pub figure: Option<(Figure, f32)>,
    /// Figures started so far (the next one is [`Figure::nth`] of it).
    pub figures: u32,
    was_tricking: bool,
    /// It looks back for this long still (s), toward this yaw (rad, in its kart's frame:
    /// positive to its left); `yaw`: its turn now, following it.
    pub look: f32,
    pub look_yaw: f32,
    pub yaw: f32,
    /// How far into its sulk (0 to 1), and for how long it has sulked (s: its sighs).
    pub sulk: f32,
    sulk_time: f32,
    /// How dizzy it looks (0 to 1), and where its head is on its round (rad).
    pub dizzy: f32,
    dizzy_phase: f32,
    pub pose: ActingPose,
}

/// `0` to `1` with an ease in and out (zero slope at both ends).
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `value` toward `target` by at most `step`.
fn approach(value: f32, target: f32, step: f32) -> f32 {
    value + (target - value).clamp(-step, step)
}

impl DriverActing {
    /// Nothing going on, and its pose at rest.
    pub fn at_rest(&self) -> bool {
        self.figure.is_none()
            && self.look <= 0.0
            && self.yaw == 0.0
            && self.sulk == 0.0
            && self.dizzy == 0.0
    }

    /// It looks back toward `yaw` (rad, its kart's frame: positive to its left; held within
    /// `DriverTuning::look_back_angle`) for `seconds` at least.
    pub fn look_back(&mut self, yaw: f32, seconds: f32, tuning: &DriverTuning) {
        if !yaw.is_finite() || !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        self.look = self.look.max(seconds);
        self.follow(yaw, tuning);
    }

    /// While it looks back, it follows `yaw` (what it looks at moved).
    pub fn follow(&mut self, yaw: f32, tuning: &DriverTuning) {
        if yaw.is_finite() {
            let angle = finite(tuning.look_back_angle).clamp(0.0, PI);
            self.look_yaw = yaw.clamp(-angle, angle);
        }
    }

    /// Moves on by one frame of `facts`, then sets `pose`.
    pub fn step(&mut self, facts: &ActingFacts, tuning: &DriverTuning) {
        if !tuning.acting {
            *self = Self::default();
            return;
        }
        let dt = facts.dt;
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        // A figure starts with each trick, in turn; it plays to its end even after the landing.
        if facts.tricking && !self.was_tricking && tuning.trick_seconds > 0.0 {
            self.figure = Some((Figure::nth(self.figures), 0.0));
            self.figures = self.figures.wrapping_add(1);
        }
        self.was_tricking = facts.tricking;
        if let Some((figure, age)) = self.figure {
            let age = age + dt;
            self.figure = (age < tuning.trick_seconds).then_some((figure, age));
        }
        // Looking back: the turn follows its target, then comes back.
        self.look = (self.look - dt).max(0.0);
        let target = if self.look > 0.0 { self.look_yaw } else { 0.0 };
        let follow = 1.0 - (-finite(tuning.look_back_speed).max(0.0) * dt).exp();
        self.yaw += (target - self.yaw) * follow;
        if self.look <= 0.0 && self.yaw.abs() < 1e-3 {
            self.yaw = 0.0;
        }
        // Sulking: in over half a second, out faster; it sighs meanwhile.
        let into = if facts.sulking { dt / 0.6 } else { -dt / 0.4 };
        self.sulk = (self.sulk + into).clamp(0.0, 1.0);
        self.sulk_time = if self.sulk > 0.0 {
            self.sulk_time + dt
        } else {
            0.0
        };
        // Seeing stars: its head goes round, fading out in their last 0.3 s.
        let dizzy = (facts.dizzy.max(0.0) / 0.3).min(1.0);
        self.dizzy = approach(self.dizzy, dizzy, dt * 6.0);
        if self.dizzy > 0.0 {
            self.dizzy_phase = (self.dizzy_phase + finite(tuning.dizzy_speed) * dt).rem_euclid(TAU);
        } else {
            self.dizzy_phase = 0.0;
        }
        self.pose = self.pose_now(facts.side, tuning);
    }

    fn pose_now(&self, side: f32, tuning: &DriverTuning) -> ActingPose {
        if self.at_rest() {
            return ActingPose::default();
        }
        let mut pose = ActingPose::default();
        if let Some((figure, age)) = self.figure {
            let t = (age / tuning.trick_seconds.max(1e-3)).clamp(0.0, 1.0);
            // Up out of its seat and back down in it...
            pose.offset.y = finite(tuning.trick_hop) * 4.0 * t * (1.0 - t);
            // ...stretched on the way, squashed as it drops back in...
            let squash = if t < 0.8 {
                -finite(tuning.trick_stretch) * (PI * t / 0.8).sin()
            } else {
                finite(tuning.trick_squash) * (PI * (t - 0.8) / 0.2).sin()
            };
            pose.scale = squash_scale(squash.clamp(-0.5, 0.5));
            // ...turning once in the air.
            let turn = TAU * ease((t - 0.05) / 0.75);
            pose.spin = match figure {
                Figure::Backflip => Quat::from_rotation_x(turn),
                Figure::Pirouette => Quat::from_rotation_y(turn),
            };
        }
        let sulk = ease(self.sulk);
        let sigh = 0.25 * (TAU * 0.35 * self.sulk_time).sin();
        pose.offset.y -= finite(tuning.sulk_sink) * sulk;
        let lean = finite(tuning.sulk_lean) * sulk * (1.0 + sigh);
        let away = finite(tuning.sulk_turn) * sulk * side.signum();
        let dizzy = if self.dizzy > 0.0 {
            let axis = Vec3::new(self.dizzy_phase.cos(), 0.0, self.dizzy_phase.sin());
            Quat::from_axis_angle(axis, finite(tuning.dizzy_sway) * ease(self.dizzy))
        } else {
            Quat::IDENTITY
        };
        pose.turn = Quat::from_rotation_y(self.yaw + away) * Quat::from_rotation_x(-lean) * dizzy;
        pose
    }
}

/// Takes in `current`, the pose another system gave the driver this frame (its seat, its size,
/// a turn of #34), field by field: what is not what `written` (the acting's last) is the new
/// `base`.
pub fn adopt(base: &mut Transform, written: &Transform, current: &Transform) {
    if current.translation != written.translation {
        base.translation = current.translation;
    }
    if current.rotation != written.rotation {
        base.rotation = current.rotation;
    }
    if current.scale != written.scale {
        base.scale = current.scale;
    }
}

/// The driver's transform under its kart's pivot: its `base` (its seat, its size, what others
/// gave it) carried by its body's `body` pose (tilted and squashed around `pivot`: the seat moves
/// with the floor), then `pose` laid over it in its own frame. `base` itself when both are at
/// rest.
pub fn acted(base: &Transform, body: &BodyPose, pivot: Vec3, pose: &ActingPose) -> Transform {
    if body.at_rest() && pose.at_rest() {
        return *base;
    }
    let (seat, rotation) = if body.at_rest() {
        (base.translation, base.rotation)
    } else {
        (
            pivot + body.tilt * (body.squash * (base.translation - pivot)),
            body.tilt * base.rotation,
        )
    };
    let middle = Vec3::Y * DRIVER_MIDDLE;
    let local = pose.offset + pose.turn * (middle - pose.spin * middle);
    Transform {
        translation: seat + rotation * (base.scale * local),
        rotation: rotation * pose.turn * pose.spin,
        scale: base.scale * pose.scale,
    }
}

/// The yaw (rad, in the frame of a kart turned by `rotation` at `from`; positive to its left)
/// that faces `toward`.
pub fn yaw_toward(from: Vec3, rotation: Quat, toward: Vec3) -> f32 {
    let to = rotation.inverse() * (toward - from);
    (-to.x).atan2(-to.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn facts() -> ActingFacts {
        ActingFacts {
            dt: DT,
            side: 1.0,
            ..default()
        }
    }

    /// Runs `driver` for `seconds` of `facts`, returning its poses.
    fn act(
        driver: &mut DriverActing,
        tuning: &DriverTuning,
        seconds: f32,
        facts: ActingFacts,
    ) -> Vec<ActingPose> {
        (0..(seconds / facts.dt).round() as usize)
            .map(|_| {
                driver.step(&facts, tuning);
                driver.pose
            })
            .collect()
    }

    /// Issue #130: a trick starts a figure (a backflip the first time, a pirouette the next),
    /// which lasts `trick_seconds` even when the kart lands before: up out of the seat,
    /// stretched, one full turn, squashed as it drops back in; then exactly at rest.
    #[test]
    fn a_trick_plays_a_figure_then_comes_back_to_rest() {
        let tuning = DriverTuning::default();
        let mut driver = DriverActing::default();
        for (n, figure) in [Figure::Backflip, Figure::Pirouette, Figure::Backflip]
            .into_iter()
            .enumerate()
        {
            // The trick lasts a third of a second: the figure goes on after it.
            let mut poses = act(
                &mut driver,
                &tuning,
                0.3,
                ActingFacts {
                    tricking: true,
                    ..facts()
                },
            );
            assert_eq!(driver.figures, n as u32 + 1);
            assert_eq!(driver.figure.map(|(f, _)| f), Some(figure));
            poses.extend(act(&mut driver, &tuning, 1.0, facts()));
            let frames = poses.iter().filter(|p| !p.at_rest()).count();
            let expected = (tuning.trick_seconds / DT).round() as usize;
            assert!(frames.abs_diff(expected) <= 1, "{frames} frames");
            let highest = poses.iter().map(|p| p.offset.y).fold(0.0, f32::max);
            assert!(
                (highest - tuning.trick_hop).abs() < 0.02,
                "up {highest} m out of its seat"
            );
            let tallest = poses.iter().map(|p| p.scale.y).fold(0.0, f32::max);
            let flattest = poses.iter().map(|p| p.scale.y).fold(2.0, f32::min);
            assert!(tallest > 1.0 + tuning.trick_stretch * 0.8, "{tallest}");
            assert!(flattest < 1.0 - tuning.trick_squash * 0.8, "{flattest}");
            // One full turn: halfway, upside down (a backflip) or facing back (a pirouette).
            let half = poses[expected / 2 - 2..expected / 2 + 3]
                .iter()
                .map(|p| p.spin.angle_between(Quat::IDENTITY))
                .fold(0.0, f32::max);
            assert!(half > 2.5, "halfway: {half} rad");
            let axis = |p: &ActingPose| p.spin.to_axis_angle().0;
            let turning = poses
                .iter()
                .find(|p| p.spin.angle_between(Quat::IDENTITY) > 0.5);
            let axis = axis(turning.unwrap());
            match figure {
                // Head back first: its top goes toward its back (+Z).
                Figure::Backflip => {
                    assert!(axis.x.abs() > 0.99, "{axis}");
                    assert!((turning.unwrap().spin * Vec3::Y).z > 0.0);
                }
                Figure::Pirouette => assert!(axis.y.abs() > 0.99, "{axis}"),
            }
            assert!(driver.at_rest());
            assert_eq!(driver.pose, ActingPose::default());
        }
        // Still tricking: no new figure.
        let mut driver = DriverActing::default();
        let tricking = ActingFacts {
            tricking: true,
            ..facts()
        };
        act(&mut driver, &tuning, 2.0, tricking);
        assert_eq!(driver.figures, 1);
        assert!(driver.at_rest());
    }

    /// Issue #130: it looks back toward what it was told for that long, held within its angle,
    /// following it as it moves; then turns back, and is exactly at rest.
    #[test]
    fn looking_back_turns_then_comes_back() {
        let tuning = DriverTuning::default();
        let mut driver = DriverActing::default();
        // Over its right shoulder (an item thrown behind, to its right).
        driver.look_back(-PI, tuning.throw_look_seconds, &tuning);
        assert_eq!(driver.look_yaw, -tuning.look_back_angle);
        let poses = act(&mut driver, &tuning, 2.0, facts());
        let yaw = |p: &ActingPose| {
            let front = p.turn * Vec3::NEG_Z;
            (-front.x).atan2(-front.z)
        };
        let most = poses.iter().map(yaw).fold(0.0, f32::min);
        assert!(most < -tuning.look_back_angle * 0.9, "{most}");
        let held = poses
            .iter()
            .filter(|p| yaw(p) < -tuning.look_back_angle * 0.5)
            .count() as f32
            * DT;
        assert!(
            (held - tuning.throw_look_seconds).abs() < 0.2,
            "looked back {held} s"
        );
        assert!(driver.at_rest());
        assert_eq!(driver.pose, ActingPose::default());
        // Overtaken on its left: it follows the kart as it goes by, ahead.
        driver.look_back(1.4, tuning.look_back_seconds, &tuning);
        act(&mut driver, &tuning, 0.4, facts());
        assert!(driver.yaw > 1.2, "{}", driver.yaw);
        driver.follow(0.3, &tuning);
        act(&mut driver, &tuning, 0.4, facts());
        assert!((driver.yaw - 0.3).abs() < 0.05, "{}", driver.yaw);
        act(&mut driver, &tuning, 2.0, facts());
        assert!(driver.at_rest());
        // Not a number: nothing.
        driver.look_back(f32::NAN, 1.0, &tuning);
        assert!(driver.at_rest());
    }

    /// Issue #130: over the line among the last, it slumps forward, sinks, turns away and sighs,
    /// as long as it sulks; then sits up again.
    #[test]
    fn a_sulk_slumps_until_it_ends() {
        let tuning = DriverTuning::default();
        let mut driver = DriverActing::default();
        let sulking = ActingFacts {
            sulking: true,
            side: -1.0,
            ..facts()
        };
        let poses = act(&mut driver, &tuning, 4.0, sulking);
        let last = poses.last().unwrap();
        let head = last.turn * Vec3::Y;
        assert!(head.z < -0.2, "slumped forward: {head}");
        assert!(last.offset.y < -tuning.sulk_sink * 0.9);
        let front = last.turn * Vec3::NEG_Z;
        assert!(front.x > 0.2, "turned away to its right: {front}");
        // It sighs: its lean goes up and down.
        let leans: Vec<f32> = poses[120..].iter().map(|p| (p.turn * Vec3::Y).z).collect();
        let (lo, hi) = leans
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), z| (lo.min(*z), hi.max(*z)));
        assert!(hi - lo > 0.05, "{lo} {hi}");
        act(&mut driver, &tuning, 1.0, facts());
        assert!(driver.at_rest());
        assert_eq!(driver.pose, ActingPose::default());
    }

    /// Issue #130: seeing stars, its head goes round (the tip of its head around its seat), as
    /// long as the stars last; then still.
    #[test]
    fn a_dizzy_head_goes_round_while_the_stars_last() {
        let tuning = DriverTuning::default();
        let mut driver = DriverActing::default();
        let mut stars = 1.2f32;
        let mut tips = Vec::new();
        for _ in 0..90 {
            driver.step(
                &ActingFacts {
                    dizzy: stars,
                    ..facts()
                },
                &tuning,
            );
            stars = (stars - DT).max(0.0);
            tips.push(driver.pose.turn * Vec3::Y);
        }
        let leaned = tips.iter().map(|t| t.xz().length()).fold(0.0, f32::max);
        assert!(leaned > tuning.dizzy_sway.sin() * 0.8, "{leaned}");
        // Round: its tip goes to the four sides.
        for side in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            assert!(
                tips.iter().any(|t| t.xz().dot(side) > 0.05),
                "never toward {side}"
            );
        }
        act(&mut driver, &tuning, 0.5, facts());
        assert!(driver.at_rest());
    }

    /// Paused, nothing moves; switched off, at rest.
    #[test]
    fn pauses_and_off_move_nothing() {
        let tuning = DriverTuning::default();
        let mut driver = DriverActing::default();
        driver.look_back(1.0, 1.0, &tuning);
        driver.step(
            &ActingFacts {
                tricking: true,
                ..facts()
            },
            &tuning,
        );
        let before = driver;
        driver.step(&ActingFacts { dt: 0.0, ..facts() }, &tuning);
        assert_eq!(driver, before);
        let off = DriverTuning {
            acting: false,
            ..default()
        };
        driver.step(&facts(), &off);
        assert!(driver.at_rest());
        assert_eq!(driver.pose, ActingPose::default());
    }

    /// The body of a kart that falls `height` m and lands, stopped by its suspension in a few
    /// frames of `dt`: its heights over time (1 at rest).
    fn landing(height: f32, dt: f32, tuning: &KartWobbleTuning) -> (Vec<f32>, f32, BodyWobble) {
        let mut body = BodyWobble::default();
        let (mut y, mut vy, mut t) = (height, 0.0f32, 0.0);
        let mut heights = Vec::new();
        let mut landed = None;
        while t < 4.0 {
            if y <= 0.0 {
                landed.get_or_insert(t);
                vy = (vy + 9.0 / 0.08 * dt).min(0.0);
            } else {
                vy -= 21.6 * dt;
            }
            y += vy * dt;
            t += dt;
            body.step(
                dt,
                Vec3::new(0.0, y, -t * 10.0),
                Quat::IDENTITY,
                tuning,
                4.0,
            );
            heights.push(body.pose.squash.y);
        }
        (heights, landed.unwrap(), body)
    }

    /// Issue #130: a landing squashes the body (within its limit), then it bounces back up
    /// (stretched past its rest), then comes back to rest; stretched in the air. About the same at
    /// any frame rate. Off, it does nothing.
    #[test]
    fn a_landing_squashes_the_body_then_it_bounces_back() {
        let tuning = KartWobbleTuning::default();
        let mut peaks = Vec::new();
        for dt in [1.0 / 30.0, 1.0 / 60.0, 1.0 / 144.0] {
            let (heights, landed, body) = landing(1.9, dt, &tuning);
            let at = (landed / dt) as usize;
            let in_air = heights[at.saturating_sub(3)];
            assert!(in_air > 1.01, "{dt}: stretched in the air {in_air}");
            let after = &heights[at..];
            let low = after.iter().copied().fold(2.0, f32::min);
            let squash = 1.0 - low;
            assert!(squash > 0.1, "{dt}: squashed {squash}");
            assert!(squash <= tuning.squash.max + 1e-4, "{dt}: {squash}");
            let lowest = after.iter().position(|h| *h == low).unwrap();
            let bounce = after[lowest..].iter().copied().fold(0.0, f32::max);
            assert!(bounce > 1.02, "{dt}: bounced back up {bounce}");
            assert!(bounce <= 1.0 + tuning.max_stretch + 1e-4);
            assert!(body.at_rest(1e-3), "{dt}: back to rest");
            assert_eq!(body.pose, BodyPose::default());
            peaks.push(squash);
        }
        let (lo, hi) = peaks
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        assert!(hi / lo < 1.3, "{peaks:?}");
        let off = KartWobbleTuning {
            enabled: false,
            ..default()
        };
        let (heights, ..) = landing(1.9, 1.0 / 60.0, &off);
        assert!(heights.iter().all(|h| *h == 1.0));
    }

    /// Issue #130: braking dives the nose, a turn rolls the body out of the turn, a change of the
    /// kart's pitch rocks it (it lags, then rocks back past it); a steady drive tilts nothing.
    #[test]
    fn the_body_tilts_against_the_acceleration_and_rocks_on_a_bump() {
        let tuning = KartWobbleTuning::default();
        // Braking at 28 m/s² from 28 m/s along −Z.
        let mut body = BodyWobble::default();
        let (mut z, mut v) = (0.0f32, 28.0f32);
        for i in 0..40 {
            if i > 10 {
                v = (v - 28.0 * DT).max(0.0);
            }
            z -= v * DT;
            body.step(DT, Vec3::new(0.0, 0.0, z), Quat::IDENTITY, &tuning, 4.0);
        }
        let nose = body.pose.tilt * Vec3::NEG_Z;
        assert!(nose.y < -0.01, "the nose dives: {nose}");
        // A left turn at 20 m/s, radius 10 m: rolled out of the turn (its top to the right).
        let mut body = BodyWobble::default();
        for i in 0..90 {
            let angle = i as f32 * DT * 2.0;
            let at = Vec3::new(angle.cos(), 0.0, -angle.sin()) * 10.0;
            body.step(DT, at, Quat::from_rotation_y(angle), &tuning, 4.0);
        }
        let top = body.pose.tilt * Vec3::Y;
        assert!(top.x > 0.02, "rolled out of the turn: {top}");
        assert!(body.pose.tilt.angle_between(Quat::IDENTITY) <= tuning.tilt.max + 1e-4);
        // A bump: the kart's nose goes up 0.15 rad in a tenth of a second, then stays.
        let mut body = BodyWobble::default();
        let mut pitches = Vec::new();
        for i in 0..120 {
            let pitch = 0.15 * ((i as f32 - 10.0) / 6.0).clamp(0.0, 1.0);
            let rotation = Quat::from_rotation_x(pitch);
            body.step(
                DT,
                Vec3::new(0.0, 0.0, -(i as f32) * 0.3),
                rotation,
                &tuning,
                4.0,
            );
            pitches.push((body.pose.tilt * Vec3::NEG_Z).y);
        }
        let lag = pitches.iter().copied().fold(0.0, f32::min);
        let back = pitches.iter().copied().fold(0.0, f32::max);
        assert!(lag < -0.005, "it lags behind the nose going up: {lag}");
        assert!(back > 0.001, "then rocks back past it: {back}");
        assert!(body.at_rest(1e-3));
        // Steady: nothing (but the rounding of the positions: below a ten-thousandth).
        let mut body = BodyWobble::default();
        for i in 0..200 {
            body.step(
                DT,
                Vec3::new(0.0, 0.0, -(i as f32) * 0.4),
                Quat::IDENTITY,
                &tuning,
                4.0,
            );
            assert!(body.pose.tilt.angle_between(Quat::IDENTITY) < 1e-4);
            assert!((body.pose.squash - Vec3::ONE).length() < 1e-4);
        }
        // Stopped: exactly at rest; a teleport measures nothing.
        for _ in 0..120 {
            body.step(DT, Vec3::new(0.0, 0.0, -80.0), Quat::IDENTITY, &tuning, 4.0);
        }
        assert_eq!(body.pose, BodyPose::default());
        body.step(DT, Vec3::new(100.0, 5.0, 0.0), Quat::IDENTITY, &tuning, 4.0);
        assert_eq!(body.pose, BodyPose::default());
    }

    /// The shell and the driver over their poses: identity gives them back exactly; a squashed
    /// body keeps its bottom (the pivot) and brings the seat down; a figure turns around the
    /// driver's middle, which stays put (but for the hop); its size scales it all.
    #[test]
    fn the_poses_are_laid_over_the_shell_and_the_seat() {
        let pivot = Vec3::new(0.0, -0.2, 0.0);
        assert_eq!(
            shell_transform(&BodyPose::default(), pivot),
            Transform::IDENTITY
        );
        let base = Transform::from_xyz(0.0, 0.15, 0.15)
            .with_rotation(Quat::from_rotation_y(0.4))
            .with_scale(Vec3::splat(1.2));
        assert_eq!(
            acted(&base, &BodyPose::default(), pivot, &ActingPose::default()),
            base
        );
        let squashed = BodyPose {
            tilt: Quat::IDENTITY,
            squash: squash_scale(0.2),
        };
        let shell = shell_transform(&squashed, pivot);
        assert!(shell.transform_point(pivot).distance(pivot) < 1e-6);
        let top = shell.transform_point(Vec3::new(0.0, 0.2, 0.0));
        assert!((top.y - (-0.2 + 0.4 * 0.8)).abs() < 1e-5, "{top}");
        let seated = acted(&base, &squashed, pivot, &ActingPose::default());
        assert!((seated.translation.y - (-0.2 + 0.35 * 0.8)).abs() < 1e-5);
        assert_eq!(
            seated.scale, base.scale,
            "the driver is not squashed by its body"
        );
        // A half backflip: the middle stays where it was, the driver upside down.
        let flip = ActingPose {
            spin: Quat::from_rotation_x(PI),
            ..default()
        };
        let flipped = acted(&base, &BodyPose::default(), pivot, &flip);
        let middle = Vec3::Y * DRIVER_MIDDLE;
        assert!(
            flipped
                .transform_point(middle)
                .distance(base.transform_point(middle))
                < 1e-5
        );
        assert!((flipped.rotation * Vec3::Y).y < -0.99);
        // A hop of 0.4 m (at size 1): 0.48 m up at size 1.2.
        let hop = ActingPose {
            offset: Vec3::Y * 0.4,
            ..default()
        };
        let up = acted(&base, &BodyPose::default(), pivot, &hop);
        assert!((up.translation.y - base.translation.y - 0.48).abs() < 1e-5);
        // Taken in: what another system wrote is the base, field by field.
        let mut kept = base;
        let written = up;
        let mut current = up;
        current.rotation = Quat::from_rotation_z(0.3);
        adopt(&mut kept, &written, &current);
        assert_eq!(kept.rotation, current.rotation);
        assert_eq!(kept.translation, base.translation);
        // Looking: its yaw toward a point.
        let yaw = yaw_toward(Vec3::ZERO, Quat::IDENTITY, Vec3::new(-1.0, 0.0, 1.0));
        assert!(
            (yaw - 3.0 * PI / 4.0).abs() < 1e-5,
            "behind on its left: {yaw}"
        );
    }
}
