//! Issue #101: the **ragdoll** of a character on foot: struck, it tumbles like a big soft die,
//! bounces off what it meets, then gets up on its own after a moment. Made for the lobby's
//! characters (`crate::salon`), and to be used again: by the circuits' passers-by (issue #107) and
//! by any soft body that must fall over and stand up.
//!
//! **The method: one rigid body that tumbles, with the wobble over it** (rather than a Rapier
//! ragdoll of several bodies and joints). A Cube Pet is one block (its head and its body) with a
//! tail, wings and four short legs: a ragdoll of joints would show little more than the block
//! rolling over, and would cost far more with 12 characters (about 6 bodies and 5 joints each:
//! 72 bodies in Rapier's single thread in the browser, and 6 rotations each to send online).
//! Here: the character's own body (one collider) is let free to turn, thrown along the push with
//! a spin, bounces (its restitution while it tumbles) and rolls to a stop (its friction); its
//! parts flail by the wobble's mini-ragdoll (#12: the body squashes, the tail and the wings fly),
//! and its legs paddle (its `run` clip, faster). Online, one pose per character is enough: its
//! peer simulates it, the others see its states.
//!
//! - [`Ragdoll`] (on a Rapier dynamic body): upright (its rotation locked as its walker wants,
//!   [`Ragdoll::upright_lock`]), **tumbling** (free to turn, thrown by a [`Knock`]), **getting up**
//!   (its rotation brought back upright in [`RagdollTuning::get_up_seconds`], locked meanwhile),
//!   upright again and **protected** for [`RagdollTuning::protection_seconds`] (nobody keeps a
//!   character down). The rules are pure ([`Ragdoll::knock`], [`Ragdoll::step`],
//!   [`knock_velocities`], [`get_up_rotation`]) and unit-tested.
//! - [`RagdollPlugin`]: the system that applies them to the bodies, in `FixedUpdate` before the
//!   physics. Only with what uses it (the lobby, with a renderer): nothing of it in the headless
//!   world of the bot races.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    CoefficientCombineRule, Friction, LockedAxes, PhysicsSet, Restitution, Velocity,
};

/// How a ragdoll falls and gets up (admin panel). Purely the characters' fun: nothing of the
/// race.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct RagdollTuning {
    /// A paw's knock throws the body this fast along the push (m/s)...
    pub push_speed: f32,
    /// ...and this fast up (m/s): a short flight.
    pub lift_speed: f32,
    /// It turns head over heels this fast (rad/s), around the axis across the push...
    pub spin_speed: f32,
    /// ...with this much twist around the vertical (rad/s): never twice the same fall.
    pub twist_speed: f32,
    /// It tumbles at least this long (s), then gets up once it rests...
    pub tumble_seconds: f32,
    /// ...slower than this (m/s, and turning slower than 4 times as many rad/s)...
    pub rest_speed: f32,
    /// ...or after this long anyway (s): on a slope, on a bouncer.
    pub max_tumble_seconds: f32,
    /// Getting up takes this long (s): turned back upright, its feet under it.
    pub get_up_seconds: f32,
    /// Back up, no knock reaches it for this long (s).
    pub protection_seconds: f32,
    /// While it tumbles: its bounciness (0 to 1) and its friction on what it rolls on.
    pub restitution: f32,
    pub friction: f32,
}

impl Default for RagdollTuning {
    fn default() -> Self {
        Self {
            push_speed: 6.5,
            lift_speed: 4.5,
            spin_speed: 9.0,
            twist_speed: 3.0,
            tumble_seconds: 1.4,
            rest_speed: 0.8,
            max_tumble_seconds: 3.5,
            get_up_seconds: 0.45,
            protection_seconds: 1.0,
            restitution: 0.45,
            friction: 0.7,
        }
    }
}

/// Where a ragdoll is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RagdollPhase {
    /// On its feet: its walker moves it.
    #[default]
    Upright,
    /// Thrown: the physics turns it.
    Tumbling,
    /// Turned back upright, its rotation locked.
    GettingUp,
}

/// A knock that throws a ragdoll: along `direction` (horizontal, normalized by
/// [`knock_velocities`]), `strength` times a paw's (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knock {
    pub direction: Vec3,
    pub strength: f32,
    /// Twists to the left (else to the right): drawn by who knocks, the same on every screen.
    pub twist_left: bool,
}

/// What a step of a ragdoll changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RagdollChange {
    None,
    /// A knock threw it: now tumbling.
    Thrown,
    /// It rests (or tumbled long enough): it gets up now.
    GetsUp,
    /// It stands again.
    Up,
}

/// The ragdoll of a body (see the module's documentation).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Ragdoll {
    pub phase: RagdollPhase,
    /// Seconds in this phase.
    pub elapsed: f32,
    /// Seconds since it stood again (the protection), large at first.
    pub since_up: f32,
    /// Its rotation when it started getting up.
    pub fallen: Quat,
    /// The knock waiting for the next step.
    pub pending: Option<Knock>,
    /// Knocks taken (the visuals kick their wobble on a new one).
    pub knocks: u32,
    /// The rotations locked while upright (a walker locks the tilt and turns about the
    /// vertical itself).
    pub upright_lock: LockedAxes,
    /// Its restitution and friction while upright (put back when it stands).
    pub upright_restitution: Restitution,
    pub upright_friction: Friction,
}

impl Default for Ragdoll {
    fn default() -> Self {
        Self::new(
            LockedAxes::ROTATION_LOCKED_X | LockedAxes::ROTATION_LOCKED_Z,
            Restitution::coefficient(0.0),
            Friction::coefficient(0.0),
        )
    }
}

impl Ragdoll {
    /// Upright, with these locks, restitution and friction while it stands.
    pub fn new(upright_lock: LockedAxes, restitution: Restitution, friction: Friction) -> Self {
        Self {
            phase: RagdollPhase::Upright,
            elapsed: 0.0,
            since_up: f32::MAX,
            fallen: Quat::IDENTITY,
            pending: None,
            knocks: 0,
            upright_lock,
            upright_restitution: restitution,
            upright_friction: friction,
        }
    }

    /// Upright: its walker moves it.
    pub fn upright(&self) -> bool {
        self.phase == RagdollPhase::Upright
    }

    /// No knock reaches it: down, getting up, or just up.
    pub fn protected(&self, tuning: &RagdollTuning) -> bool {
        !self.upright() || self.pending.is_some() || self.since_up < tuning.protection_seconds
    }

    /// A knock: taken (for the next step) unless it is protected. Returns whether it was.
    pub fn knock(&mut self, knock: Knock, tuning: &RagdollTuning) -> bool {
        if self.protected(tuning) {
            return false;
        }
        self.pending = Some(knock);
        true
    }

    /// `dt` s later, its body moving at `speed` m/s and turning at `turn` rad/s (rotation
    /// `rotation`): a pending knock throws it, a tumble ends once it rests (or long enough), a
    /// getting up ends after its time.
    pub fn step(
        &mut self,
        dt: f32,
        speed: f32,
        turn: f32,
        rotation: Quat,
        tuning: &RagdollTuning,
    ) -> RagdollChange {
        self.elapsed += dt;
        self.since_up = (self.since_up + dt).min(f32::MAX / 2.0);
        if self.pending.is_some() && self.upright() {
            self.phase = RagdollPhase::Tumbling;
            self.elapsed = 0.0;
            self.knocks = self.knocks.wrapping_add(1);
            return RagdollChange::Thrown;
        }
        match self.phase {
            RagdollPhase::Upright => RagdollChange::None,
            RagdollPhase::Tumbling => {
                let rests = speed < tuning.rest_speed && turn < 4.0 * tuning.rest_speed;
                let done = self.elapsed >= tuning.tumble_seconds && rests;
                if done || self.elapsed >= tuning.max_tumble_seconds {
                    self.phase = RagdollPhase::GettingUp;
                    self.elapsed = 0.0;
                    self.fallen = rotation;
                    RagdollChange::GetsUp
                } else {
                    RagdollChange::None
                }
            }
            RagdollPhase::GettingUp => {
                if self.elapsed >= tuning.get_up_seconds {
                    self.phase = RagdollPhase::Upright;
                    self.elapsed = 0.0;
                    self.since_up = 0.0;
                    RagdollChange::Up
                } else {
                    RagdollChange::None
                }
            }
        }
    }

    /// Its rotation while it gets up (`None` otherwise): from how it fell to upright.
    pub fn getting_up_rotation(&self, tuning: &RagdollTuning) -> Option<Quat> {
        (self.phase == RagdollPhase::GettingUp).then(|| {
            let t = self.elapsed / tuning.get_up_seconds.max(1e-3);
            get_up_rotation(self.fallen, t)
        })
    }
}

/// The velocities a knock gives: along its push (horizontal) and up, turning head over heels
/// around the axis across the push, with a twist about the vertical.
pub fn knock_velocities(knock: &Knock, tuning: &RagdollTuning) -> (Vec3, Vec3) {
    let flat = Vec3::new(knock.direction.x, 0.0, knock.direction.z);
    let push = flat.normalize_or(Vec3::NEG_Z);
    let strength = knock.strength.max(0.0);
    let linear = (push * tuning.push_speed + Vec3::Y * tuning.lift_speed) * strength;
    // Turning about up × push makes the top go along the push: it falls forward, head first.
    let across = Vec3::Y.cross(push);
    let twist = if knock.twist_left { 1.0 } else { -1.0 };
    let angular =
        (across * tuning.spin_speed + Vec3::Y * tuning.twist_speed * twist) * strength.sqrt();
    (linear, angular)
}

/// The heading of a body turned by `rotation` (its front, -Z, along the ground; lying on its
/// front or its back, the way its top points).
pub fn heading_of(rotation: Quat) -> f32 {
    let front = rotation * Vec3::NEG_Z;
    let flat = Vec3::new(front.x, 0.0, front.z);
    let flat = if flat.length_squared() > 0.04 {
        flat
    } else {
        let top = rotation * Vec3::Y;
        Vec3::new(top.x, 0.0, top.z)
    };
    if flat.length_squared() < 1e-8 {
        return 0.0;
    }
    // The yaw that turns -Z onto `flat`.
    (-flat.x).atan2(-flat.z)
}

/// Getting up, `t` of the way (0 to 1, eased): from the rotation it fell in to upright, facing
/// the way it lay.
pub fn get_up_rotation(fallen: Quat, t: f32) -> Quat {
    let t = t.clamp(0.0, 1.0);
    // Ease in and out: it rolls over slowly, then lands on its feet.
    let eased = t * t * (3.0 - 2.0 * t);
    let upright = Quat::from_rotation_y(heading_of(fallen));
    fallen.normalize().slerp(upright, eased).normalize()
}

/// The ragdolls' system (`FixedUpdate`, before the physics).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RagdollSystems;

/// Applies the ragdolls' rules to their bodies (see the module's documentation). Added by what
/// uses ragdolls (the lobby, with a renderer).
pub struct RagdollPlugin;

impl Plugin for RagdollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RagdollTuning>().add_systems(
            FixedUpdate,
            tumble_ragdolls
                .in_set(RagdollSystems)
                .before(PhysicsSet::SyncBackend),
        );
    }
}

/// Each ragdoll's step on its body: thrown (its rotation free, its velocities from the knock,
/// bouncy), getting up (its rotation locked and brought back upright), up (its upright locks,
/// restitution and friction back).
fn tumble_ragdolls(
    time: Res<Time>,
    tuning: Res<RagdollTuning>,
    mut bodies: Query<(
        &mut Ragdoll,
        &mut Velocity,
        &mut LockedAxes,
        &mut Transform,
        &mut Restitution,
        &mut Friction,
    )>,
) {
    let dt = time.delta_secs();
    for (mut ragdoll, mut velocity, mut locks, mut transform, mut restitution, mut friction) in
        &mut bodies
    {
        let speed = velocity.linear.length();
        let turn = velocity.angular.length();
        let pending = ragdoll.pending;
        match ragdoll.step(dt, speed, turn, transform.rotation, &tuning) {
            RagdollChange::Thrown => {
                ragdoll.pending = None;
                if let Some(knock) = pending {
                    let (linear, angular) = knock_velocities(&knock, &tuning);
                    velocity.linear = linear;
                    velocity.angular = angular;
                }
                locks.set_if_neq(LockedAxes::empty());
                *restitution = Restitution {
                    coefficient: tuning.restitution,
                    combine_rule: CoefficientCombineRule::Max,
                };
                *friction = Friction {
                    coefficient: tuning.friction,
                    combine_rule: CoefficientCombineRule::Max,
                };
            }
            RagdollChange::GetsUp => {
                locks.set_if_neq(LockedAxes::ROTATION_LOCKED);
                velocity.angular = Vec3::ZERO;
                velocity.linear.x = 0.0;
                velocity.linear.z = 0.0;
            }
            RagdollChange::Up => {
                // Exactly upright, facing the way it got up.
                transform.rotation = Quat::from_rotation_y(heading_of(transform.rotation));
                locks.set_if_neq(ragdoll.upright_lock);
                velocity.angular = Vec3::ZERO;
                *restitution = ragdoll.upright_restitution;
                *friction = ragdoll.upright_friction;
            }
            RagdollChange::None => {}
        }
        if let Some(rotation) = ragdoll.getting_up_rotation(&tuning) {
            transform.rotation = rotation;
            velocity.angular = Vec3::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn knock() -> Knock {
        Knock {
            direction: Vec3::new(3.0, 0.4, 0.0),
            strength: 1.0,
            twist_left: true,
        }
    }

    /// A knock is taken upright and unprotected; it throws the ragdoll at the next step; while
    /// down, getting up or just up, no other knock is taken.
    #[test]
    fn a_knock_throws_it_and_nothing_keeps_it_down() {
        let tuning = RagdollTuning::default();
        let mut ragdoll = Ragdoll::default();
        assert!(ragdoll.upright() && !ragdoll.protected(&tuning));
        assert!(ragdoll.knock(knock(), &tuning));
        assert!(!ragdoll.knock(knock(), &tuning), "one knock at a time");
        assert_eq!(
            ragdoll.step(DT, 0.0, 0.0, Quat::IDENTITY, &tuning),
            RagdollChange::Thrown
        );
        assert_eq!(ragdoll.phase, RagdollPhase::Tumbling);
        assert_eq!(ragdoll.knocks, 1);
        ragdoll.pending = None;
        assert!(!ragdoll.knock(knock(), &tuning), "down");
        // It rests: not before its least time.
        let mut changes = Vec::new();
        let lying = Quat::from_rotation_z(1.4);
        for _ in 0..((tuning.tumble_seconds / DT) as usize + 2) {
            changes.push(ragdoll.step(DT, 0.1, 0.1, lying, &tuning));
        }
        let got_up_at = changes
            .iter()
            .position(|c| *c == RagdollChange::GetsUp)
            .expect("it gets up once it rests");
        assert!(got_up_at as f32 * DT >= tuning.tumble_seconds - 2.0 * DT);
        assert_eq!(ragdoll.phase, RagdollPhase::GettingUp);
        assert_eq!(ragdoll.fallen, lying);
        assert!(!ragdoll.knock(knock(), &tuning), "getting up");
        let mut up = None;
        for tick in 0..60 {
            if ragdoll.step(DT, 0.0, 0.0, lying, &tuning) == RagdollChange::Up {
                up = Some(tick);
                break;
            }
        }
        let up = up.expect("it stands again") as f32 * DT;
        assert!((up - tuning.get_up_seconds).abs() < 2.0 * DT, "{up}");
        assert!(ragdoll.upright() && ragdoll.protected(&tuning), "just up");
        assert!(!ragdoll.knock(knock(), &tuning));
        for _ in 0..((tuning.protection_seconds / DT).ceil() as usize + 2) {
            ragdoll.step(DT, 0.0, 0.0, Quat::IDENTITY, &tuning);
        }
        assert!(!ragdoll.protected(&tuning));
        assert!(ragdoll.knock(knock(), &tuning));
    }

    /// Still moving, it tumbles on (it bounces, rolls), but never longer than its longest time.
    #[test]
    fn it_tumbles_while_it_moves_but_not_for_ever() {
        let tuning = RagdollTuning::default();
        let mut ragdoll = Ragdoll::default();
        ragdoll.knock(knock(), &tuning);
        ragdoll.step(DT, 0.0, 0.0, Quat::IDENTITY, &tuning);
        let mut seconds = 0.0;
        while ragdoll.phase == RagdollPhase::Tumbling {
            ragdoll.step(DT, 5.0, 8.0, Quat::IDENTITY, &tuning);
            seconds += DT;
            assert!(seconds < 10.0);
        }
        assert!(
            (seconds - tuning.max_tumble_seconds).abs() < 2.0 * DT,
            "{seconds}"
        );
        // Turning fast on the spot is not resting either.
        let mut spinning = Ragdoll::default();
        spinning.knock(knock(), &tuning);
        spinning.step(DT, 0.0, 0.0, Quat::IDENTITY, &tuning);
        for _ in 0..((tuning.tumble_seconds / DT) as usize + 5) {
            spinning.step(DT, 0.1, 20.0, Quat::IDENTITY, &tuning);
        }
        assert_eq!(spinning.phase, RagdollPhase::Tumbling);
    }

    /// A knock throws along its push and up, turning head over heels toward the push (its top
    /// goes along the push), twisting the way it says; stronger, faster.
    #[test]
    fn a_knock_throws_along_the_push_head_over_heels() {
        let tuning = RagdollTuning::default();
        let (linear, angular) = knock_velocities(&knock(), &tuning);
        assert!((linear - Vec3::new(tuning.push_speed, tuning.lift_speed, 0.0)).length() < 1e-4);
        // The top (+Y) moves along +X under this rotation.
        let top_moves = angular.cross(Vec3::Y);
        assert!(top_moves.x > 0.0, "{angular}");
        assert!(angular.y > 0.0, "a twist to the left");
        let right = Knock {
            twist_left: false,
            ..knock()
        };
        assert!(knock_velocities(&right, &tuning).1.y < 0.0);
        let strong = Knock {
            strength: 1.5,
            ..knock()
        };
        assert!(knock_velocities(&strong, &tuning).0.length() > linear.length() * 1.4);
        // No direction: forward (-Z), never NaN.
        let none = Knock {
            direction: Vec3::Y,
            ..knock()
        };
        let (linear, angular) = knock_velocities(&none, &tuning);
        assert!(linear.is_finite() && angular.is_finite());
        assert!(linear.z < 0.0);
    }

    /// Getting up: from the fall to upright, facing the way it lay (its front along the
    /// ground, or its top when it lies on its front or its back), smoothly.
    #[test]
    fn it_gets_up_facing_the_way_it_lay() {
        let lying = Quat::from_rotation_y(0.7) * Quat::from_rotation_z(1.5);
        assert!(get_up_rotation(lying, 0.0).angle_between(lying) < 1e-3);
        let up = get_up_rotation(lying, 1.0);
        assert!((up * Vec3::Y).y > 0.9999, "upright");
        assert!((heading_of(up) - 0.7).abs() < 1e-3, "{}", heading_of(up));
        // On its front: its top points the way it faced.
        let face_down =
            Quat::from_rotation_y(-1.2) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let up = get_up_rotation(face_down, 1.0);
        assert!((up * Vec3::Y).y > 0.9999);
        assert!((heading_of(up) + 1.2).abs() < 1e-2, "{}", heading_of(up));
        // Halfway: between the two, never past.
        let half = get_up_rotation(lying, 0.5);
        let total = lying.angle_between(get_up_rotation(lying, 1.0));
        assert!(half.angle_between(lying) < total);
        assert!(half.angle_between(lying) > 0.2 * total);
        assert_eq!(heading_of(Quat::IDENTITY), 0.0);
    }
}
