//! Items out on the track (GD § 8, issue #16): pure maths, no ECS. Unit-tested.
//!
//! - **Held out** around their kart ([`Hold`], [`held_offset`]): a dragged pelure or pelote
//!   behind it, the pelures of an "x3" in a row behind, the pelotes or abeilles of an "x3" turning
//!   around it. They follow the kart and are shields: anything else touching them breaks with
//!   them.
//! - **Rolling** ([`Roll`], [`roll`]): a pelote (or an abeille thrown backwards) hugs the ground
//!   and goes straight at a constant speed, bouncing off the walls a few times. No Rapier body:
//!   two rays per tick (the wall ahead, the ground below), the same on every machine.
//! - **Contacts** are swept ([`segment_sphere`], [`segment_box`]): a projectile moving 0.7 m per
//!   tick never goes through a kart or another item between two ticks.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::tuning::ItemTuning;

/// A kart's body center is this high above the ground, on its wheels (m): `GRID_HEIGHT`.
pub const KART_RIDE_HEIGHT: f32 = 0.55;

/// A ray hit on something whose normal is flatter than this (|y| below) is a wall; steeper up,
/// it is ground (the kart's own limit, `KartTuning::min_ground_normal_y`, is 0.5).
pub const WALL_NORMAL_Y: f32 = 0.6;

/// The ground probe of a rolling item starts this far above its center (it climbs slopes and
/// ramps)...
pub const GROUND_PROBE_UP: f32 = 1.2;
/// ...and finds the ground up to this far below its bottom. Further below, it falls.
pub const GROUND_PROBE_DOWN: f32 = 0.6;
/// A falling item is gone once it falls this fast (m/s): about 1.5 s of free fall.
pub const FALL_LIMIT_SPEED: f32 = 30.0;
/// Gravity of a falling item (m/s²), like the karts' (9.81 × 2.2).
pub const ITEM_GRAVITY: f32 = 21.6;

/// Where an item held out sits around its kart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// A single pelure or pelote, behind the kart while the button is held.
    Dragged,
    /// The `k`-th pelure of an "x3" (0: the closest), in a row behind.
    Trail(u8),
    /// The `k`-th pelote or abeille of an "x3", turning around the kart.
    Orbit(u8),
    /// The `k`-th item of a piñata (issue #21, `PINATA_ITEMS`), turning around the kart, wider
    /// and slower than an "x3".
    Pinata(u8),
}

/// Where a held item sits in its kart's frame (the kart looks down -Z, its back is +Z):
/// `radius` is the item's, `of` how many turn around the kart, `time` the kart's clock for the
/// rotation (s).
pub fn held_offset(hold: Hold, of: u8, radius: f32, time: f32, tuning: &ItemTuning) -> Vec3 {
    let low = radius - KART_RIDE_HEIGHT;
    match hold {
        Hold::Dragged => Vec3::new(0.0, low, tuning.drag_distance),
        Hold::Trail(k) => Vec3::new(
            0.0,
            low,
            tuning.drag_distance + k as f32 * tuning.trail_spacing,
        ),
        Hold::Orbit(k) => {
            let angle = time * tuning.orbit_speed + k as f32 * TAU / of.max(1) as f32;
            Vec3::new(
                tuning.orbit_radius * angle.sin(),
                low + 0.1,
                tuning.orbit_radius * angle.cos(),
            )
        }
        Hold::Pinata(k) => {
            let angle = pinata_angle(k, time, tuning);
            Vec3::new(
                tuning.pinata_orbit_radius * angle.sin(),
                0.35 - KART_RIDE_HEIGHT,
                tuning.pinata_orbit_radius * angle.cos(),
            )
        }
    }
}

/// Where the `k`-th item of a piñata is in its turn around the kart at `time` (issue #21): its
/// angle (rad) from the back of the kart (+Z), turning toward its left. The front is at π.
pub fn pinata_angle(k: u8, time: f32, tuning: &ItemTuning) -> f32 {
    time * tuning.pinata_orbit_speed + k as f32 * TAU / super::table::PINATA_ITEMS.len() as f32
}

/// Which of the piñata's items `left` (one bit each) is the closest to the front of the kart at
/// `time` (issue #21): the one a press uses. `None` when none is left.
pub fn pinata_in_front(left: u8, time: f32, tuning: &ItemTuning) -> Option<u8> {
    (0..super::table::PINATA_ITEMS.len() as u8)
        .filter(|k| left & (1 << k) != 0)
        .min_by(|a, b| {
            let off = |k: u8| {
                let angle = pinata_angle(k, time, tuning).rem_euclid(TAU);
                (angle - std::f32::consts::PI).abs()
            };
            off(*a).total_cmp(&off(*b))
        })
}

/// Where along the segment `a → b` (0 to 1) it first comes within `radius` of `center`, if it
/// does (0 when `a` already is).
pub fn segment_sphere(a: Vec3, b: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let d = b - a;
    let m = a - center;
    let c = m.length_squared() - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    let dd = d.length_squared();
    if dd < 1e-12 {
        return None;
    }
    let half_b = m.dot(d);
    if half_b >= 0.0 {
        // Moving away.
        return None;
    }
    let discriminant = half_b * half_b - dd * c;
    if discriminant < 0.0 {
        return None;
    }
    let t = (-half_b - discriminant.sqrt()) / dd;
    (t <= 1.0).then_some(t.max(0.0))
}

/// Where along the segment `a → b` (0 to 1) it first enters the box centered on the origin with
/// half size `half`, if it does (0 when `a` is inside). Both ends in the box's frame.
pub fn segment_box(a: Vec3, b: Vec3, half: Vec3) -> Option<f32> {
    let d = b - a;
    let (mut enter, mut exit) = (0.0f32, 1.0f32);
    for axis in 0..3 {
        let (start, delta, h) = (a[axis], d[axis], half[axis]);
        if delta.abs() < 1e-9 {
            if start.abs() > h {
                return None;
            }
            continue;
        }
        let t1 = (-h - start) / delta;
        let t2 = (h - start) / delta;
        let (near, far) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        enter = enter.max(near);
        exit = exit.min(far);
        if enter > exit {
            return None;
        }
    }
    Some(enter)
}

/// A rolling item: a pelote, or an abeille thrown backwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roll {
    /// Its center.
    pub position: Vec3,
    /// Its horizontal velocity (y ignored).
    pub velocity: Vec3,
    /// Falling speed when no ground is under it (m/s, downward positive).
    pub fall: f32,
    /// Walls it can still bounce off; the next one breaks it.
    pub bounces_left: u32,
}

/// What happened to a rolling item this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollStep {
    Rolled,
    /// It bounced off a wall.
    Bounced,
    /// It hit a wall with no bounce left: it breaks there.
    Broke,
    /// It fell off the track.
    Fell,
}

/// One tick of `dt` seconds of a rolling item of `radius`: straight on along its velocity,
/// bouncing off the walls (mirror reflection on the wall's horizontal normal), hugging the
/// ground below (slopes and ramps included), falling where there is none. `cast(origin,
/// direction, length)` is a ray against the track (walls, ground): the distance to the hit and
/// the surface normal.
pub fn roll(
    item: &mut Roll,
    radius: f32,
    dt: f32,
    cast: impl Fn(Vec3, Vec3, f32) -> Option<(f32, Vec3)>,
) -> RollStep {
    let mut outcome = RollStep::Rolled;
    let flat = item.velocity.with_y(0.0);
    let step = flat.length() * dt;
    if step > 1e-6 {
        let direction = flat / flat.length();
        match cast(item.position, direction, step + radius) {
            Some((distance, normal)) if normal.y.abs() < WALL_NORMAL_Y => {
                // Stop with the center `radius` away from the wall's face.
                let wall = normal.with_y(0.0).normalize_or(-direction);
                let facing = direction.dot(wall).abs().max(0.2);
                item.position += direction * (distance - radius / facing).max(0.0);
                if item.bounces_left == 0 {
                    return RollStep::Broke;
                }
                item.bounces_left -= 1;
                item.velocity = flat - 2.0 * flat.dot(wall) * wall;
                outcome = RollStep::Bounced;
            }
            _ => item.position += direction * step,
        }
    }
    // The ground below: snap onto it (climbing slopes), or fall when it is too far down.
    let probe = GROUND_PROBE_UP + radius + GROUND_PROBE_DOWN;
    let ground = cast(
        item.position + Vec3::Y * GROUND_PROBE_UP,
        Vec3::NEG_Y,
        probe,
    )
    .filter(|(_, normal)| normal.y >= WALL_NORMAL_Y)
    .map(|(distance, _)| item.position.y + GROUND_PROBE_UP - distance);
    let falling_to = ground.unwrap_or(f32::NEG_INFINITY) + radius;
    if item.fall > 0.0 || ground.is_none() {
        item.fall += ITEM_GRAVITY * dt;
        item.position.y -= item.fall * dt;
        if item.position.y <= falling_to {
            item.position.y = falling_to;
            item.fall = 0.0;
        } else if item.fall >= FALL_LIMIT_SPEED {
            return RollStep::Fell;
        }
    } else {
        item.position.y = falling_to;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// The ground (y = 0) and, optionally, a wall at x = `wall` facing -x.
    fn world(wall: Option<f32>) -> impl Fn(Vec3, Vec3, f32) -> Option<(f32, Vec3)> {
        move |origin: Vec3, direction: Vec3, length: f32| {
            let mut best: Option<(f32, Vec3)> = None;
            if direction.y < -1e-6 && origin.y >= 0.0 {
                let t = origin.y / -direction.y;
                if t <= length {
                    best = Some((t, Vec3::Y));
                }
            }
            if let Some(x) = wall
                && direction.x > 1e-6
                && origin.x <= x
            {
                let t = (x - origin.x) / direction.x;
                if t <= length && best.is_none_or(|(b, _)| t < b) {
                    best = Some((t, Vec3::NEG_X));
                }
            }
            best
        }
    }

    /// A pelote rolls straight at its speed, on the ground.
    #[test]
    fn a_pelote_rolls_straight_on_the_ground() {
        let mut pelote = Roll {
            position: Vec3::new(0.0, 0.4, 0.0),
            velocity: Vec3::new(0.0, 0.0, -42.0),
            fall: 0.0,
            bounces_left: 5,
        };
        for _ in 0..60 {
            assert_eq!(roll(&mut pelote, 0.4, DT, world(None)), RollStep::Rolled);
        }
        assert!(
            (pelote.position.z + 42.0).abs() < 1e-2,
            "{}",
            pelote.position
        );
        assert!((pelote.position.y - 0.4).abs() < 1e-5);
    }

    /// Acceptance criterion of #16: a pelote bounces off a wall (mirror reflection), and breaks
    /// on the wall after its last bounce.
    #[test]
    fn a_pelote_bounces_off_a_wall_then_breaks_after_its_last_bounce() {
        let speed = Vec3::new(30.0, 0.0, -30.0);
        let mut pelote = Roll {
            position: Vec3::new(0.0, 0.4, 0.0),
            velocity: speed,
            fall: 0.0,
            bounces_left: 1,
        };
        let mut bounced = false;
        for _ in 0..60 {
            if roll(&mut pelote, 0.4, DT, world(Some(5.0))) == RollStep::Bounced {
                bounced = true;
                break;
            }
        }
        assert!(bounced);
        assert!(pelote.position.x <= 4.6 + 1e-4, "{}", pelote.position);
        assert!((pelote.velocity - Vec3::new(-30.0, 0.0, -30.0)).length() < 1e-4);
        assert_eq!(pelote.bounces_left, 0);
        // Thrown at the wall again: it breaks on it.
        pelote.velocity = speed;
        let mut steps = Vec::new();
        for _ in 0..60 {
            let step = roll(&mut pelote, 0.4, DT, world(Some(5.0)));
            steps.push(step);
            if step == RollStep::Broke {
                break;
            }
        }
        assert_eq!(steps.last(), Some(&RollStep::Broke));
    }

    /// It climbs a slope (the ground found higher up), and falls off where there is no ground.
    #[test]
    fn a_pelote_hugs_the_ground_and_falls_off_the_edge() {
        let slope = |origin: Vec3, direction: Vec3, length: f32| -> Option<(f32, Vec3)> {
            // Ground at y = 0.2 x for x in 0..10, nothing beyond.
            if direction.y < -0.5 && (0.0..10.0).contains(&origin.x) {
                let ground = 0.2 * origin.x;
                let t = origin.y - ground;
                return (t >= 0.0 && t <= length).then_some((t, Vec3::Y));
            }
            None
        };
        let mut pelote = Roll {
            position: Vec3::new(0.5, 0.4, 0.0),
            velocity: Vec3::new(10.0, 0.0, 0.0),
            fall: 0.0,
            bounces_left: 0,
        };
        for _ in 0..30 {
            assert_eq!(roll(&mut pelote, 0.4, DT, slope), RollStep::Rolled);
        }
        assert!(
            (pelote.position.y - (0.2 * pelote.position.x + 0.4)).abs() < 1e-4,
            "{}",
            pelote.position
        );
        let mut fell = false;
        for _ in 0..200 {
            if roll(&mut pelote, 0.4, DT, slope) == RollStep::Fell {
                fell = true;
                break;
            }
        }
        assert!(fell);
    }

    /// Swept contacts: a fast item does not go through a sphere or a box between two ticks.
    #[test]
    fn swept_contacts_do_not_tunnel() {
        let a = Vec3::new(0.0, 0.0, 3.0);
        let b = Vec3::new(0.0, 0.0, -3.0);
        let t = segment_sphere(a, b, Vec3::ZERO, 1.0).unwrap();
        assert!((t - 2.0 / 6.0).abs() < 1e-5);
        assert_eq!(segment_sphere(a, b, Vec3::new(2.0, 0.0, 0.0), 1.0), None);
        assert_eq!(segment_sphere(Vec3::ZERO, b, Vec3::ZERO, 1.0), Some(0.0));
        assert_eq!(segment_sphere(b, b, Vec3::ZERO, 1.0), None);
        let half = Vec3::new(1.1, 0.65, 1.4);
        let t = segment_box(a, b, half).unwrap();
        assert!((t - 1.6 / 6.0).abs() < 1e-5);
        assert_eq!(segment_box(Vec3::ZERO, b, half), Some(0.0));
        assert_eq!(
            segment_box(a + Vec3::X * 1.2, b + Vec3::X * 1.2, half),
            None
        );
        assert_eq!(segment_box(a, a, half), None);
    }

    /// Held items: behind the kart (dragged, in a row for an "x3" of pelures), or turning around
    /// it at the orbit radius, a third of a turn apart.
    #[test]
    fn held_items_sit_behind_or_turn_around_the_kart() {
        let t = ItemTuning::default();
        let dragged = held_offset(Hold::Dragged, 1, 0.4, 0.0, &t);
        assert_eq!(
            dragged,
            Vec3::new(0.0, 0.4 - KART_RIDE_HEIGHT, t.drag_distance)
        );
        let third = held_offset(Hold::Trail(2), 3, 0.4, 0.0, &t);
        assert!((third.z - t.drag_distance - 2.0 * t.trail_spacing).abs() < 1e-6);
        let orbit: Vec<Vec3> = (0..3)
            .map(|k| held_offset(Hold::Orbit(k), 3, 0.4, 1.3, &t))
            .collect();
        for (i, p) in orbit.iter().enumerate() {
            assert!((p.with_y(0.0).length() - t.orbit_radius).abs() < 1e-5);
            let next = orbit[(i + 1) % 3];
            let gap = p.with_y(0.0).angle_between(next.with_y(0.0));
            assert!((gap - TAU / 3.0).abs() < 1e-4);
        }
        let later = held_offset(Hold::Orbit(0), 3, 0.4, 1.4, &t);
        assert!(later.distance(orbit[0]) > 0.5, "it turns");
    }

    /// Issue #21: a piñata's eight items turn around the kart at their radius, an eighth of a
    /// turn apart; the one a press uses is the one closest to the front of the kart (-Z), among
    /// those left.
    #[test]
    fn a_pinatas_items_turn_around_and_the_front_one_goes() {
        let t = ItemTuning::default();
        for time in [0.0, 0.37, 1.9] {
            let around: Vec<Vec3> = (0..8)
                .map(|k| held_offset(Hold::Pinata(k), 8, 0.3, time, &t))
                .collect();
            for (k, p) in around.iter().enumerate() {
                assert!((p.with_y(0.0).length() - t.pinata_orbit_radius).abs() < 1e-4);
                let next = around[(k + 1) % 8];
                let gap = p.with_y(0.0).angle_between(next.with_y(0.0));
                assert!((gap - TAU / 8.0).abs() < 1e-3);
            }
            let front = pinata_in_front(u8::MAX, time, &t).unwrap();
            let ahead = |k: u8| -around[usize::from(k)].z;
            assert!((0..8).all(|k| ahead(front) >= ahead(k) - 1e-4), "{time}");
            // Without the front one: the next closest to the front.
            let left = !(1u8 << front);
            let second = pinata_in_front(left, time, &t).unwrap();
            assert_ne!(second, front);
            assert!(
                (0..8)
                    .filter(|k| *k != front)
                    .all(|k| ahead(second) >= ahead(k) - 1e-4)
            );
        }
        assert_eq!(pinata_in_front(0, 1.0, &t), None);
        assert_eq!(pinata_in_front(0b0100_0000, 1.0, &t), Some(6));
    }
}
