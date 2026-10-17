//! Issue #101: walking in the lobby, in pure logic (unit-tested): what the keys and the stick
//! ask, how a character on foot speeds up, turns and jumps, which character its paw reaches.
//!
//! - **The keys are the race's** (GD § 13): up and down (the accelerator and the brake: the
//!   arrows, Z/S in AZERTY, W/S in QWERTY), left and right (Q/D or A/D), Espace (the jump), X or F
//!   (the Interaction button: the paw). On a gamepad: the left stick (both its axes), A and B (up
//!   and down, like the accelerator and the brake), RT (the jump), RB (the paw).
//! - **The camera does not turn** (`camera.rs`): up goes away from it (toward -Z), right to its
//!   right (+X). The character turns toward where it goes.

use bevy::prelude::*;

use super::SalonTuning;
use crate::kart::KartInput;

/// What a character on foot is asked this tick.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct FootInput {
    /// Where to go on the screen: x to the right, y up; length 1 at most.
    pub direction: Vec2,
    pub jump: bool,
    pub paw: bool,
}

/// The race's input (`kart::keyboard_input`, `kart::gamepad_input`, combined) and a stick's
/// vertical axis, as an input on foot.
pub fn foot_input(race: &KartInput, stick_y: f32) -> FootInput {
    let up = (race.throttle - race.brake + stick_y).clamp(-1.0, 1.0);
    let direction = Vec2::new(race.steer.clamp(-1.0, 1.0), up);
    FootInput {
        direction: direction.clamp_length_max(1.0),
        jump: race.jump_drift,
        paw: race.interact,
    }
}

/// The stick's vertical axis without its dead zone (as `kart::gamepad_input` does for its
/// horizontal one).
pub fn stick_axis(value: f32) -> f32 {
    const DEAD_ZONE: f32 = 0.15;
    if value.abs() < DEAD_ZONE {
        0.0
    } else {
        (value - DEAD_ZONE * value.signum()) / (1.0 - DEAD_ZONE)
    }
}

/// Where a direction on the screen goes in the world (the camera looks toward -Z).
pub fn world_direction(direction: Vec2) -> Vec3 {
    Vec3::new(direction.x, 0.0, -direction.y)
}

/// The heading (yaw, rad: 0 faces -Z) of a horizontal direction.
pub fn heading_toward(direction: Vec3) -> Option<f32> {
    let flat = Vec2::new(direction.x, direction.z);
    (flat.length_squared() > 1e-6).then(|| (-flat.x).atan2(-flat.y))
}

/// The front (-Z turned by `heading`) of a character.
pub fn front_of(heading: f32) -> Vec3 {
    Quat::from_rotation_y(heading) * Vec3::NEG_Z
}

/// `heading` turned toward `wanted` by at most `rate × dt` (the shorter way round).
pub fn turned(heading: f32, wanted: f32, rate: f32, dt: f32) -> f32 {
    let mut gap = (wanted - heading).rem_euclid(std::f32::consts::TAU);
    if gap > std::f32::consts::PI {
        gap -= std::f32::consts::TAU;
    }
    let step = (rate * dt).max(0.0);
    let next = heading + gap.clamp(-step, step);
    (next + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// The velocity of a character on foot after `dt`, asked `input` (its horizontal part speeds up
/// or slows down toward what it is asked, less in the air; its vertical part is the physics'),
/// and a jump if it is on the ground and the jump was just pressed (`jump`).
pub fn walk_velocity(
    velocity: Vec3,
    input: Vec2,
    grounded: bool,
    jump: bool,
    tuning: &SalonTuning,
    dt: f32,
) -> Vec3 {
    let wanted = world_direction(input.clamp_length_max(1.0)) * tuning.walk_speed;
    let flat = Vec3::new(velocity.x, 0.0, velocity.z);
    let control = if grounded { 1.0 } else { tuning.air_control };
    let step = tuning.acceleration * control * dt;
    let gap = wanted - flat;
    let flat = flat + gap.clamp_length_max(step.max(0.0));
    let vertical = if grounded && jump {
        tuning.jump_speed
    } else {
        velocity.y
    };
    Vec3::new(flat.x, vertical, flat.z)
}

/// A bouncer of the lobby (a giant donut): a round top `radius` m around `center` (on the
/// ground), `top` m high.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Bouncer {
    pub center: Vec2,
    pub radius: f32,
    pub top: f32,
}

impl Bouncer {
    /// Whether a character whose bottom is at `feet` (falling or still, `vertical` m/s) lands on
    /// it now: over its top, at most 0.3 m above it.
    pub fn catches(&self, feet: Vec3, vertical: f32) -> bool {
        let over = Vec2::new(feet.x, feet.z).distance(self.center) <= self.radius;
        let height = feet.y - self.top;
        over && (-0.15..=0.3).contains(&height) && vertical <= 0.5
    }
}

/// A character the paw can reach: its id, where it is (shown: what the striker sees), whether
/// it can be struck (not down nor protected, as far as the striker knows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PawCandidate<T> {
    pub id: T,
    pub position: Vec3,
    pub open: bool,
}

/// The character the paw of the one at `me` (facing `heading`) reaches: the nearest open one in
/// front of it (within `cone` rad of its front), at most `reach` m away (center to center), at
/// most `height` m higher or lower.
pub fn paw_target<T: Copy>(
    me: Vec3,
    heading: f32,
    candidates: impl IntoIterator<Item = PawCandidate<T>>,
    (reach, cone, height): (f32, f32, f32),
) -> Option<PawCandidate<T>> {
    let front = front_of(heading);
    candidates
        .into_iter()
        .filter(|c| c.open)
        .filter_map(|c| {
            let to = c.position - me;
            let flat = Vec3::new(to.x, 0.0, to.z);
            let distance = flat.length();
            if distance > reach || to.y.abs() > height {
                return None;
            }
            let angle = if distance < 1e-3 {
                0.0
            } else {
                front.angle_between(flat)
            };
            (angle <= cone).then_some((distance, c))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// On which side of the one facing `heading` a point `to` (from it) is: its right (else its
/// left).
pub fn on_the_right(heading: f32, to: Vec3) -> bool {
    let right = Quat::from_rotation_y(heading) * Vec3::X;
    right.dot(to) >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// The race's keys and stick: up and down, left and right, the jump and the paw; the stick's
    /// two axes; never longer than 1.
    #[test]
    fn the_race_keys_walk() {
        let input = KartInput {
            throttle: 1.0,
            steer: 1.0,
            jump_drift: true,
            interact: true,
            ..default()
        };
        let foot = foot_input(&input, 0.0);
        assert!((foot.direction.length() - 1.0).abs() < 1e-5);
        assert!(foot.direction.x > 0.7 && foot.direction.y > 0.7);
        assert!(foot.jump && foot.paw);
        let back = foot_input(
            &KartInput {
                brake: 1.0,
                ..default()
            },
            0.0,
        );
        assert_eq!(back.direction, Vec2::new(0.0, -1.0));
        // The stick up, half way.
        let stick = foot_input(&KartInput::default(), 0.5);
        assert_eq!(stick.direction, Vec2::new(0.0, 0.5));
        assert_eq!(stick_axis(0.1), 0.0);
        assert!((stick_axis(1.0) - 1.0).abs() < 1e-6);
        assert!(stick_axis(-0.5) < 0.0);
        // Up goes away from the camera (-Z), right to its right (+X).
        assert_eq!(world_direction(Vec2::Y), Vec3::NEG_Z);
        assert_eq!(world_direction(Vec2::X), Vec3::X);
    }

    /// From a standstill, full up: the walking speed in a fraction of a second, along -Z; let
    /// go, it stops as fast; in the air, it steers less; a jump only from the ground.
    #[test]
    fn a_character_walks_stops_and_jumps() {
        let tuning = SalonTuning::default();
        let mut v = Vec3::ZERO;
        let mut ticks = 0;
        while (v.length() - tuning.walk_speed).abs() > 1e-3 {
            v = walk_velocity(v, Vec2::Y, true, false, &tuning, DT);
            ticks += 1;
            assert!(ticks < 60, "{v}");
        }
        assert!(ticks as f32 * DT <= 0.25, "{ticks} ticks");
        assert!(v.z < 0.0 && v.x.abs() < 1e-5);
        let mut stopping = v;
        for _ in 0..ticks + 1 {
            stopping = walk_velocity(stopping, Vec2::ZERO, true, false, &tuning, DT);
        }
        assert!(stopping.length() < 1e-3);
        let air = walk_velocity(Vec3::ZERO, Vec2::X, false, false, &tuning, DT);
        let ground = walk_velocity(Vec3::ZERO, Vec2::X, true, false, &tuning, DT);
        assert!(air.x < ground.x && air.x > 0.0);
        let fall = Vec3::new(0.0, -3.0, 0.0);
        assert_eq!(
            walk_velocity(fall, Vec2::ZERO, false, true, &tuning, DT).y,
            -3.0
        );
        assert_eq!(
            walk_velocity(Vec3::ZERO, Vec2::ZERO, true, true, &tuning, DT).y,
            tuning.jump_speed
        );
    }

    /// The heading of a direction, its front, and a turn the shorter way round, never past.
    #[test]
    fn a_character_turns_toward_where_it_goes() {
        let east = heading_toward(Vec3::X).unwrap();
        assert!((front_of(east) - Vec3::X).length() < 1e-5);
        assert!((front_of(0.0) - Vec3::NEG_Z).length() < 1e-5);
        assert_eq!(heading_toward(Vec3::Y), None);
        // From facing -Z (0) toward +X (-π/2): to the right, 0.2 rad a tick at 12 rad/s.
        let next = turned(0.0, east, 12.0, DT);
        assert!((next - (-0.2)).abs() < 1e-5, "{next}");
        assert!(
            (turned(0.0, east, 1000.0, DT) - east).abs() < 1e-5,
            "never past"
        );
        // Around: from 3 rad to -3 rad is 0.28 rad, through π.
        let around = turned(3.0, -3.0, 12.0, DT);
        assert!(around > 3.0 || around < -3.0, "{around}");
    }

    /// A bouncer catches a character landing on its top, not one beside it, under it or going
    /// up.
    #[test]
    fn a_bouncer_catches_what_lands_on_it() {
        let bouncer = Bouncer {
            center: Vec2::new(4.0, 0.0),
            radius: 2.0,
            top: 1.0,
        };
        assert!(bouncer.catches(Vec3::new(4.5, 1.1, 0.5), -2.0));
        assert!(bouncer.catches(Vec3::new(4.5, 1.0, 0.5), 0.0));
        assert!(!bouncer.catches(Vec3::new(7.0, 1.0, 0.0), -2.0), "beside");
        assert!(!bouncer.catches(Vec3::new(4.0, 0.0, 0.0), -2.0), "under");
        assert!(
            !bouncer.catches(Vec3::new(4.0, 1.5, 0.0), -2.0),
            "still above"
        );
        assert!(!bouncer.catches(Vec3::new(4.0, 1.1, 0.0), 5.0), "going up");
    }

    /// The paw reaches the nearest open character in front, within reach; not one behind, too
    /// far, down, or far above.
    #[test]
    fn the_paw_reaches_the_nearest_in_front() {
        let me = Vec3::new(0.0, 0.5, 0.0);
        let at = |id: u8, x: f32, y: f32, z: f32, open: bool| PawCandidate {
            id,
            position: Vec3::new(x, y, z),
            open,
        };
        let candidates = [
            at(1, 0.0, 0.5, -1.5, true),
            at(2, 0.3, 0.5, -1.0, false),
            at(3, 0.0, 0.5, 1.0, true),
            at(4, 0.0, 3.0, -0.8, true),
            at(5, 0.0, 0.5, -4.0, true),
            at(6, -1.0, 0.5, -1.2, true),
        ];
        let cone = 75f32.to_radians();
        let target = paw_target(me, 0.0, candidates, (1.8, cone, 1.2)).unwrap();
        assert_eq!(target.id, 1, "the nearest open one in front");
        let turned_back = paw_target(me, std::f32::consts::PI, candidates, (1.8, cone, 1.2));
        assert_eq!(turned_back.map(|c| c.id), Some(3));
        assert!(
            paw_target(me, 0.0, candidates, (0.5, cone, 1.2)).is_none(),
            "too far"
        );
        assert!(on_the_right(0.0, Vec3::X));
        assert!(!on_the_right(0.0, Vec3::NEG_X));
        assert!(on_the_right(std::f32::consts::PI, Vec3::NEG_X));
    }
}
