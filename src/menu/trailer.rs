//! The home screen's trailer camera (issue #89), pure: shots around the karts behind the home
//! screen, a new one every few seconds (`HomeTuning::shot_seconds`), each one a slow move, and
//! from one to the next a smooth move around the karts (`HomeTuning::blend_seconds`): never a cut,
//! nothing sudden behind the buttons.
//!
//! - **A shot** is a [`Rig`] around its subject: how far around it the camera stands (`yaw`: 0
//!   behind, a quarter turn to its right, half a turn ahead), how far and how high, what it looks
//!   at, its field of view; each one moves a little during the shot (a dolly, a slide, an orbit).
//!   The loop ([`SHOTS`]) goes from behind, low, alongside, ahead, around, then high above, and is
//!   mirrored every other time ([`Shot::nth`]).
//! - **From one shot to the next**, the two rigs are blended (the yaw the short way round): the
//!   camera goes around the karts, never through them.
//! - **The subject** ([`Follow`]): the two karts closest to each other (where the paws fly), and
//!   the karts around them, followed by critically damped springs: the camera never jolts when a
//!   kart jostles, nor when the next shot follows other karts.
//! - **Framed on the right** ([`framed`]): the home's column stands on the left of the screen; the
//!   karts are shown on the right ([`FRAME_X`]).

use std::f32::consts::{PI, TAU};

use bevy::math::{Quat, Vec3};

/// A shot of the trailer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shot {
    /// Behind and above, moving in a little.
    Chase,
    /// Behind and to the side, at wheel height.
    Low,
    /// Alongside, the karts sliding past.
    Side,
    /// Ahead, looking back: the karts come at the camera.
    Front,
    /// From ahead, slowly around them.
    Orbit,
    /// High above.
    Aerial,
}

/// The loop of shots, in this order: each one starts not far around the karts from where the one
/// before ends.
pub const SHOTS: [Shot; 6] = [
    Shot::Chase,
    Shot::Low,
    Shot::Side,
    Shot::Front,
    Shot::Orbit,
    Shot::Aerial,
];

/// Where the karts are shown across the screen (normalized: 0 the middle, 1 the right edge): on
/// the right, the home's column covers the left.
pub const FRAME_X: f32 = 0.38;
/// The subject: the closest pair's middle, and the karts within this distance of it (m)...
pub const PACK_RADIUS: f32 = 20.0;
/// ...weighed this much against the pair (0: the pair only, 1: the karts around only).
pub const PACK_WEIGHT: f32 = 0.5;
/// The springs that follow the subject (rad/s): its place, its heading. Quick: they only smooth
/// the karts' jostling (from one pair of karts to the next, the subject itself moves smoothly,
/// [`Reel::blend_weight`]).
pub const FOCUS_FREQUENCY: f32 = 4.0;
pub const HEADING_RATE: f32 = 4.0;
/// An obstacle between the karts and the camera (a wall, a hill) brings it in front of the
/// obstacle quickly, and back out slowly (share of the distance per second).
pub const PULL_IN_RATE: f32 = 8.0;
pub const PULL_OUT_RATE: f32 = 1.0;

/// Where a camera stands around its subject, and what it looks at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    /// Around the subject (rad): 0 behind it, `PI / 2` on its right, `PI` ahead of it.
    pub yaw: f32,
    /// Distance from the subject, across the ground (m).
    pub distance: f32,
    /// Height above the subject (m).
    pub height: f32,
    /// It looks this far ahead of the subject (m)...
    pub look_ahead: f32,
    /// ...and this high above it (m).
    pub look_height: f32,
    /// Vertical field of view (degrees).
    pub fov_degrees: f32,
}

/// What a camera shows: where it stands, what it looks at, its field of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub eye: Vec3,
    pub look_at: Vec3,
    pub fov_degrees: f32,
}

impl Shot {
    /// The `index`-th shot of the trailer, and its side (1: its right, -1: its left): the loop,
    /// mirrored every other time.
    pub fn nth(index: usize) -> (Shot, f32) {
        let side = if (index / SHOTS.len()).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        (SHOTS[index % SHOTS.len()], side)
    }

    /// The rig of this shot `t` seconds into it, on its `side`.
    pub fn rig(self, t: f32, side: f32) -> Rig {
        let t = t.max(0.0);
        let (yaw, distance, height, look_ahead, look_height, fov_degrees) = match self {
            Shot::Chase => (0.08, 10.0 - (0.25 * t).min(2.5), 3.4, 3.0, 0.5, 55.0),
            Shot::Low => (0.45, 6.0, 0.7, 3.0, 0.8, 62.0),
            Shot::Side => (1.3 + (0.05 * t).min(0.5), 7.0, 1.5, 1.0, 0.5, 50.0),
            Shot::Front => (PI - 0.18, 12.0 - (0.4 * t).min(4.0), 2.0, 0.0, 0.6, 48.0),
            Shot::Orbit => (PI - 0.4 - (0.2 * t).min(2.0), 9.0, 3.0, 0.0, 0.5, 55.0),
            Shot::Aerial => (0.4, 8.0, 11.0, 5.0, 0.0, 50.0),
        };
        Rig {
            yaw: yaw * side,
            distance,
            height,
            look_ahead,
            look_height,
            fov_degrees,
        }
    }
}

/// `x` from 0 to 1, eased at both ends (and held at them).
pub fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The angle from `a` to `b` the short way round (rad, in `-PI..=PI`).
pub fn angle_between(a: f32, b: f32) -> f32 {
    let d = (b - a).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

/// `a` turned into `b` by `s` (0: `a`, 1: `b`): around the subject the short way.
pub fn blend(a: &Rig, b: &Rig, s: f32) -> Rig {
    let s = s.clamp(0.0, 1.0);
    let lerp = |x: f32, y: f32| x + (y - x) * s;
    Rig {
        yaw: a.yaw + angle_between(a.yaw, b.yaw) * s,
        distance: lerp(a.distance, b.distance),
        height: lerp(a.height, b.height),
        look_ahead: lerp(a.look_ahead, b.look_ahead),
        look_height: lerp(a.look_height, b.look_height),
        fov_degrees: lerp(a.fov_degrees, b.fov_degrees),
    }
}

/// A heading flattened on the ground (unit), or straight on (-Z) if it has none.
fn flat(heading: Vec3) -> Vec3 {
    Vec3::new(heading.x, 0.0, heading.z).normalize_or(Vec3::NEG_Z)
}

/// The view of `rig` around a subject at `focus` heading `heading`.
pub fn view(rig: &Rig, focus: Vec3, heading: Vec3) -> View {
    let forward = flat(heading);
    let right = forward.cross(Vec3::Y);
    let around = -forward * rig.yaw.cos() + right * rig.yaw.sin();
    View {
        eye: focus + around * rig.distance + Vec3::Y * rig.height,
        look_at: focus + forward * rig.look_ahead + Vec3::Y * rig.look_height,
        fov_degrees: rig.fov_degrees,
    }
}

/// `view` turned to the left about the vertical so that what it looked at shows at `share` of the
/// way to the right edge of the screen (normalized, [`FRAME_X`]), for a screen `aspect` times as
/// wide as high.
pub fn framed(view: View, aspect: f32, share: f32) -> View {
    let half_width = aspect.max(0.1) * (view.fov_degrees.to_radians() * 0.5).tan();
    let turn = (share * half_width).atan();
    let look = Quat::from_rotation_y(turn) * (view.look_at - view.eye);
    View {
        look_at: view.eye + look,
        ..view
    }
}

/// Where the trailer is in its loop: the shot (its number) and how long it has been on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reel {
    pub shot: usize,
    pub since: f32,
}

impl Reel {
    /// `dt` seconds later, with shots of `shot_seconds`: whether a new shot started.
    pub fn advance(&mut self, dt: f32, shot_seconds: f32) -> bool {
        self.since += dt.max(0.0);
        if self.since >= shot_seconds.max(0.5) {
            self.since -= shot_seconds.max(0.5);
            self.shot += 1;
            return true;
        }
        false
    }

    /// How far into its blend from the shot before (0 to 1, eased), while it lasts: the subject
    /// of the shot before is blended into this one's the same way.
    pub fn blend_weight(&self, blend_seconds: f32) -> Option<f32> {
        (self.shot > 0 && blend_seconds > 0.0 && self.since < blend_seconds)
            .then(|| smoothstep(self.since / blend_seconds))
    }

    /// The rig now: the shot's own, blended in from the one before over `blend_seconds` (the
    /// first shot of all comes at once).
    pub fn rig(&self, shot_seconds: f32, blend_seconds: f32) -> Rig {
        let (shot, side) = Shot::nth(self.shot);
        let rig = shot.rig(self.since, side);
        if self.shot == 0 || blend_seconds <= 0.0 || self.since >= blend_seconds {
            return rig;
        }
        let (before, before_side) = Shot::nth(self.shot - 1);
        let previous = before.rig(shot_seconds.max(0.5) + self.since, before_side);
        blend(&previous, &rig, smoothstep(self.since / blend_seconds))
    }
}

/// The two karts closest to each other, of those at `positions` (indices).
pub fn closest_pair(positions: &[Vec3]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, f32)> = None;
    for (i, a) in positions.iter().enumerate() {
        for (j, b) in positions.iter().enumerate().skip(i + 1) {
            let d = a.distance_squared(*b);
            if best.is_none_or(|(.., e)| d < e) {
                best = Some((i, j, d));
            }
        }
    }
    best.map(|(i, j, _)| (i, j))
}

/// Where the camera aims, for the pair `pair` of the karts at `positions`: their middle, weighed
/// with the middle of the karts within [`PACK_RADIUS`] of it.
pub fn subject(positions: &[Vec3], pair: (usize, usize)) -> Vec3 {
    let (Some(a), Some(b)) = (positions.get(pair.0), positions.get(pair.1)) else {
        return positions.first().copied().unwrap_or(Vec3::ZERO);
    };
    let middle = (*a + *b) * 0.5;
    let (sum, count) = positions
        .iter()
        .filter(|p| p.distance(middle) <= PACK_RADIUS)
        .fold((Vec3::ZERO, 0.0), |(s, n), p| (s + *p, n + 1.0));
    if count == 0.0 {
        return middle;
    }
    middle.lerp(sum / count, PACK_WEIGHT)
}

/// The subject followed by springs: the camera's focus and heading, smooth whatever the karts do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Follow {
    pub focus: Vec3,
    pub velocity: Vec3,
    pub heading: Vec3,
}

impl Follow {
    /// Starting on a subject at `focus`, moving at `velocity`, heading `heading`.
    pub fn at(focus: Vec3, velocity: Vec3, heading: Vec3) -> Self {
        Self {
            focus,
            velocity,
            heading: flat(heading),
        }
    }

    /// `dt` seconds later, toward a subject now at `focus`, moving at `speed`, heading `heading`.
    /// The spring follows the subject's move (a critically damped spring on a moving target lags
    /// behind it otherwise), exactly for a subject at a steady speed, whatever `dt` (the browser
    /// of a slow machine draws a few frames a second: the camera still keeps up).
    pub fn step(&mut self, focus: Vec3, speed: Vec3, heading: Vec3, dt: f32) {
        let dt = dt.clamp(0.0, 0.5);
        let omega = FOCUS_FREQUENCY;
        // Off the subject as it was at the start of the step (it moved `speed` × `dt` since).
        let offset = self.focus - (focus - speed * dt);
        let relative = self.velocity - speed;
        let decay = (-omega * dt).exp();
        let temp = (relative + offset * omega) * dt;
        self.focus = focus + (offset + temp) * decay;
        self.velocity = speed + (relative - temp * omega) * decay;
        let target = flat(heading);
        let share = 1.0 - (-HEADING_RATE * dt).exp();
        self.heading = flat(self.heading.lerp(target, share));
        if self.heading.dot(target) < -0.99 {
            // Turned right round (a tow): no lerp through nothing.
            self.heading = target;
        }
    }
}

/// The share of its distance the camera keeps from the subject, moving toward `wanted` (1: no
/// obstacle; less: the obstacle's distance) `dt` seconds later: in quickly, out slowly.
pub fn pull(current: f32, wanted: f32, dt: f32) -> f32 {
    let wanted = wanted.clamp(0.05, 1.0);
    let rate = if wanted < current {
        PULL_IN_RATE
    } else {
        PULL_OUT_RATE
    };
    let step = rate * dt.max(0.0);
    if wanted < current {
        (current - step).max(wanted)
    } else {
        (current + step).min(wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOT: f32 = 6.0;
    const BLEND: f32 = 2.0;

    /// Each shot stands around its subject, above it, at a few meters, and looks near it; the
    /// loop shows it from every side, low and high, near and far.
    #[test]
    fn every_shot_stands_around_the_karts_and_looks_at_them() {
        let focus = Vec3::new(30.0, 2.0, -50.0);
        let heading = Vec3::new(1.0, 0.0, 0.0);
        let mut yaws = Vec::new();
        let mut heights = Vec::new();
        for index in 0..2 * SHOTS.len() {
            let (shot, side) = Shot::nth(index);
            for t in [0.0, SHOT * 0.5, SHOT, SHOT + BLEND] {
                let rig = shot.rig(t, side);
                let v = view(&rig, focus, heading);
                let away = v.eye - focus;
                assert!(away.y >= 0.6, "{shot:?}: {away}");
                let across = Vec3::new(away.x, 0.0, away.z).length();
                assert!((5.0..=13.0).contains(&across), "{shot:?}: {across}");
                assert!(v.look_at.distance(focus) <= 9.0, "{shot:?}");
                assert!((45.0..=65.0).contains(&v.fov_degrees));
                // It looks toward the karts, never away from them.
                let to_focus = (focus - v.eye).normalize();
                let looking = (v.look_at - v.eye).normalize();
                assert!(looking.dot(to_focus) > 0.75, "{shot:?} {t}");
                yaws.push(rig.yaw);
                heights.push(rig.height);
            }
        }
        // Behind, alongside on both sides, ahead; low and high.
        assert!(yaws.iter().any(|y| y.abs() < 0.2));
        assert!(yaws.iter().any(|y| *y > 1.2 && *y < 1.9));
        assert!(yaws.iter().any(|y| *y < -1.2 && *y > -1.9));
        assert!(yaws.iter().any(|y| y.abs() > 2.8));
        assert!(heights.iter().any(|h| *h < 1.0) && heights.iter().any(|h| *h > 10.0));
        // Mirrored every other loop.
        assert_eq!(Shot::nth(2).1, 1.0);
        assert_eq!(Shot::nth(SHOTS.len() + 2).1, -1.0);
        assert_eq!(Shot::nth(SHOTS.len() + 2).0, Shot::nth(2).0);
    }

    /// The shots change every `shot_seconds`; the next one comes in smoothly over
    /// `blend_seconds`, from where the last one was.
    #[test]
    fn the_shots_follow_each_other_without_a_cut() {
        let mut reel = Reel::default();
        let mut changes = 0;
        let mut last = reel.rig(SHOT, BLEND);
        let dt = 1.0 / 60.0;
        for _ in 0..(3.5 * SHOT * 60.0) as usize {
            let before = reel;
            if reel.advance(dt, SHOT) {
                changes += 1;
                // Right after the change, the camera is still where the last shot had it.
                let previous = Shot::nth(before.shot)
                    .0
                    .rig(before.since + dt, Shot::nth(before.shot).1);
                let now = reel.rig(SHOT, BLEND);
                assert!(angle_between(previous.yaw, now.yaw).abs() < 0.01, "{now:?}");
                assert!((previous.height - now.height).abs() < 0.05);
            }
            let rig = reel.rig(SHOT, BLEND);
            // No jump from one frame to the next.
            assert!(angle_between(last.yaw, rig.yaw).abs() < 0.05, "{reel:?}");
            assert!((last.distance - rig.distance).abs() < 0.15, "{reel:?}");
            assert!((last.height - rig.height).abs() < 0.2, "{reel:?}");
            last = rig;
        }
        assert_eq!(changes, 3);
        // Halfway through the blend, halfway between the two (eased).
        let reel = Reel {
            shot: 1,
            since: BLEND / 2.0,
        };
        assert_eq!(reel.blend_weight(BLEND), Some(0.5));
        assert_eq!(
            Reel {
                shot: 1,
                since: BLEND
            }
            .blend_weight(BLEND),
            None
        );
        assert_eq!(
            Reel {
                shot: 0,
                since: 0.1
            }
            .blend_weight(BLEND),
            None
        );
        let a = Shot::Chase.rig(SHOT + BLEND / 2.0, 1.0);
        let b = Shot::Low.rig(BLEND / 2.0, 1.0);
        let mid = reel.rig(SHOT, BLEND);
        assert!((mid.height - (a.height + b.height) / 2.0).abs() < 1e-3);
        assert_eq!(
            Reel {
                shot: 1,
                since: BLEND
            }
            .rig(SHOT, BLEND),
            b
        );
    }

    /// The blend goes around the karts the short way, never through them.
    #[test]
    fn a_blend_goes_around_the_short_way() {
        assert!((angle_between(3.0, -3.0) - (TAU - 6.0)).abs() < 1e-5);
        assert!((angle_between(-3.0, 3.0) + (TAU - 6.0)).abs() < 1e-5);
        assert!((angle_between(0.2, 1.0) - 0.8).abs() < 1e-6);
        let front = Shot::Front.rig(SHOT, -1.0);
        let low = Shot::Low.rig(0.0, 1.0);
        for i in 0..=20 {
            let rig = blend(&front, &low, i as f32 / 20.0);
            let v = view(&rig, Vec3::ZERO, Vec3::NEG_Z);
            let across = Vec3::new(v.eye.x, 0.0, v.eye.z).length();
            assert!(across >= 5.5, "through the karts: {rig:?}");
        }
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert_eq!(smoothstep(2.0), 1.0);
        assert!((smoothstep(0.5) - 0.5).abs() < 1e-6);
    }

    /// Issue #89: the whole loop, filming karts that drive a circle at 25 m/s: the camera moves
    /// smoothly around them (never faster than the karts plus 15 m/s, never speeding up nor
    /// slowing down by more than 25 m/s², never turning faster than 2 rad/s: no cut, no jolt) and
    /// always shows them on the right of the screen.
    #[test]
    fn filming_karts_in_a_turn_is_smooth_and_frames_them_on_the_right() {
        let dt = 1.0 / 60.0;
        let (radius, speed) = (60.0, 25.0);
        let at = |t: f32| {
            let a = t * speed / radius;
            (
                Vec3::new(radius * a.cos(), 1.0, radius * a.sin()),
                Vec3::new(-a.sin(), 0.0, a.cos()) * speed,
            )
        };
        let (start, velocity) = at(0.0);
        let mut follow = Follow::at(start, velocity, velocity);
        let mut reel = Reel::default();
        let aspect = 16.0 / 9.0;
        let mut last: Option<View> = None;
        let mut last_relative: Option<Vec3> = None;
        let mut fastest_turn = 0.0f32;
        let mut fastest_move = 0.0f32;
        let mut hardest_change = 0.0f32;
        let mut hardest_at = 0;
        let mut widest = (1.0f32, 0.0f32);
        for frame in 0..(2.0 * SHOTS.len() as f32 * SHOT * 60.0) as usize {
            let t = frame as f32 * dt;
            let (focus, velocity) = at(t);
            follow.step(focus, velocity, velocity, dt);
            reel.advance(dt, SHOT);
            let rig = reel.rig(SHOT, BLEND);
            let v = framed(view(&rig, follow.focus, follow.heading), aspect, FRAME_X);
            if let Some(last) = last {
                // Its own move around the karts (the karts' move taken off).
                let relative = (v.eye - last.eye) / dt - velocity;
                fastest_move = fastest_move.max(relative.length());
                // (From half a second on: the first frames start the springs.)
                if let Some(before) = last_relative.filter(|_| frame > 30) {
                    let change = (relative - before).length() / dt;
                    if change > hardest_change {
                        hardest_change = change;
                        hardest_at = frame;
                    }
                }
                last_relative = Some(relative);
                let turn = (v.look_at - v.eye)
                    .normalize()
                    .angle_between((last.look_at - last.eye).normalize());
                fastest_turn = fastest_turn.max(turn / dt);
            }
            // The karts on the right of the screen: on it, never behind the home's column (the
            // left half, at 16:9).
            let x = screen_x(&v, aspect, focus);
            assert!((0.05..0.85).contains(&x), "frame {frame}: {x}");
            widest = (widest.0.min(x), widest.1.max(x));
            last = Some(v);
        }
        println!(
            "trailer: {fastest_move:.2} m/s, {hardest_change:.2} m/s² (frame {hardest_at}), {fastest_turn:.2} rad/s, on screen from {:.2} to {:.2}",
            widest.0, widest.1
        );
        assert!(fastest_move < 15.0, "{fastest_move} m/s");
        assert!(hardest_change < 25.0, "{hardest_change} m/s²");
        assert!(fastest_turn < 2.0, "{fastest_turn} rad/s");
        // The springs follow the karts closely.
        let (focus, _) = at((2.0 * SHOTS.len() as f32 * SHOT * 60.0 - 1.0) * dt);
        assert!(
            follow.focus.distance(focus) < 1.0,
            "{}",
            follow.focus.distance(focus)
        );
    }

    /// A slow machine (4 frames a second): the springs keep up with the karts all the same.
    #[test]
    fn at_a_few_frames_a_second_the_camera_keeps_up_with_the_karts() {
        for dt in [1.0 / 60.0, 1.0 / 15.0, 0.25, 0.5] {
            let velocity = Vec3::new(0.0, 0.0, -28.0);
            let mut follow = Follow::at(Vec3::ZERO, Vec3::ZERO, Vec3::NEG_Z);
            let mut worst = 0.0f32;
            for frame in 1..=(20.0 / dt) as usize {
                let t = frame as f32 * dt;
                // Off at a steady speed, then a change of pace.
                let speed = if t < 10.0 { velocity } else { velocity * 0.6 };
                let at = if t < 10.0 {
                    velocity * t
                } else {
                    velocity * 10.0 + velocity * 0.6 * (t - 10.0)
                };
                follow.step(at, speed, speed, dt);
                if t > 2.0 {
                    worst = worst.max(follow.focus.distance(at));
                }
            }
            assert!(worst < 2.0, "{dt}: {worst} m behind");
        }
    }

    /// Where `point` shows across the screen of `view` (normalized: 0 the middle, 1 the right edge).
    fn screen_x(view: &View, aspect: f32, point: Vec3) -> f32 {
        let forward = (view.look_at - view.eye).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        let up = right.cross(forward);
        let to = point - view.eye;
        let (x, z) = (to.dot(right), to.dot(forward));
        let _ = up;
        let half_width = aspect * (view.fov_degrees.to_radians() * 0.5).tan();
        x / z / half_width
    }

    /// Framed, what the shot looked at shows at `FRAME_X` across the screen, at its height.
    #[test]
    fn a_framed_view_shows_its_subject_on_the_right() {
        let v = View {
            eye: Vec3::new(0.0, 1.0, 10.0),
            look_at: Vec3::new(0.0, 1.0, 0.0),
            fov_degrees: 50.0,
        };
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0] {
            let f = framed(v, aspect, FRAME_X);
            assert_eq!(f.eye, v.eye);
            let x = screen_x(&f, aspect, v.look_at);
            assert!((x - FRAME_X).abs() < 1e-3, "{aspect}: {x}");
        }
        assert_eq!(framed(v, 1.5, 0.0).look_at, v.look_at);
    }

    /// The subject: the pair closest to each other, with the karts around them (not one far away,
    /// a kart the pelican carries); the springs follow it without a jolt when it changes.
    #[test]
    fn the_camera_follows_the_closest_pair_and_the_karts_around_them() {
        let karts = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(11.5, 0.0, 1.0),
            Vec3::new(16.0, 0.0, -2.0),
            Vec3::new(200.0, 0.0, 0.0),
        ];
        assert_eq!(closest_pair(&karts), Some((1, 2)));
        assert_eq!(closest_pair(&karts[..1]), None);
        let aim = subject(&karts, (1, 2));
        let middle = (karts[1] + karts[2]) * 0.5;
        assert!(aim.distance(middle) < 3.0, "{aim}");
        assert!(
            aim.x > middle.x - 3.0 && aim.x < 16.0,
            "not drawn far away: {aim}"
        );
        assert_eq!(subject(&karts, (7, 9)), karts[0]);

        // A subject 8 m away at once (from one shot to the next, it moves smoothly instead): the
        // focus still goes there without a jump (no step over 0.2 m a frame: 12 m/s).
        let mut follow = Follow::at(Vec3::ZERO, Vec3::ZERO, Vec3::X);
        let mut previous = follow.focus;
        for _ in 0..240 {
            follow.step(
                Vec3::new(0.0, 0.0, 8.0),
                Vec3::ZERO,
                Vec3::NEG_Z,
                1.0 / 60.0,
            );
            assert!(follow.focus.distance(previous) < 0.2);
            previous = follow.focus;
        }
        assert!(follow.focus.distance(Vec3::new(0.0, 0.0, 8.0)) < 0.05);
        assert!(follow.heading.distance(Vec3::NEG_Z) < 0.01);
        // Turned right round at once: no heading of zero length.
        follow.step(Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, Vec3::Z, 1.0 / 60.0);
        assert!((follow.heading.length() - 1.0).abs() < 1e-4);
    }

    /// An obstacle brings the camera in quickly, and it goes back out slowly.
    #[test]
    fn an_obstacle_brings_the_camera_in_quickly_and_out_slowly() {
        let dt = 1.0 / 60.0;
        let mut kept = 1.0;
        let mut frames = 0;
        while kept > 0.4 + 1e-4 {
            kept = pull(kept, 0.4, dt);
            frames += 1;
        }
        assert!(frames <= 6, "{frames}");
        let mut out = 0;
        while kept < 1.0 - 1e-4 {
            kept = pull(kept, 1.0, dt);
            out += 1;
        }
        assert!(out > 4 * frames, "{out}");
        // Never closer than a twentieth of its distance.
        let mut kept = 1.0;
        for _ in 0..120 {
            kept = pull(kept, 0.0, dt);
        }
        assert!((kept - 0.05).abs() < 1e-6, "{kept}");
    }
}
