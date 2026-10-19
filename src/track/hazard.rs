//! Issue #144 (GD « Conception des circuits », step 2): the hazards of a circuit, pure geometry
//! and motion (no ECS).
//!
//! - **Obstacles** (`TrackData::obstacles`): fixed ones (a rock, a trunk, a pillar) are static
//!   colliders: the karts bump into them like into a wall (`crate::track::hazards` spawns them);
//!   moving ones (a rolling log, a swinging stone) have no collider: a kart that touches one is
//!   knocked over (a tumble, like an item's: the victim decides).
//! - **Passers-by** (`TrackData::npcs`): original creatures pacing back and forth across one side
//!   of the road, stopping a moment at each end, facing the karts that come: touched, they knock a
//!   kart over.
//! - **Things to drive through** (`TrackData::breakables`): hay bales, balloons, crates; a kart
//!   bursts them (a little slower, or not at all), they come back after a while.
//!
//! **Determinism**: a moving hazard's pose is a pure function of the race clock (the flow tick,
//! the same on every peer online: `crate::race::RaceFlow::follow`) and of its phase, drawn from
//! the race seed (`RaceGrid::seed`, the host's) and its number on the circuit
//! ([`hazard_phase`]). Every peer sees it at the same place on the same tick, and judges the
//! contacts of its own karts with it (docs/ARCHITECTURE.md: the victim decides).
//!
//! The shapes are simple: a capsule per moving hazard (a sphere when its ends meet), and the kart
//! as a capsule along its length ([`kart_capsule`]); the bots keep clear of a ground rectangle
//! that covers a hazard's whole motion ([`Hazard::footprint`]).

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use super::data::{BreakableKind, NpcKind, ObstacleKind, TrackData};
use super::layout::{CenterLine, TrackSample};
use crate::kart::KART_HALF_EXTENTS;

/// A swinging stone hangs this far below its pivot (m, its center).
pub const ROPE_LENGTH: f32 = 7.0;
/// At rest, the bottom of a swinging stone is this high over the road (m): it hits a kart.
pub const STONE_CLEARANCE: f32 = 0.35;
/// A passer-by stops at each end of its walk for this share of its period (half at each end).
pub const NPC_PAUSE_SHARE: f32 = 0.3;

/// What a hazard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HazardKind {
    Obstacle(ObstacleKind),
    Npc(NpcKind),
    Breakable(BreakableKind),
}

impl HazardKind {
    /// A fixed obstacle: a static collider the karts bump into.
    pub fn stands(self) -> bool {
        matches!(self, HazardKind::Obstacle(kind) if !kind.moves())
    }

    /// A moving obstacle or a passer-by: touched, it knocks a kart over.
    pub fn knocks(self) -> bool {
        match self {
            HazardKind::Obstacle(kind) => kind.moves(),
            HazardKind::Npc(_) => true,
            HazardKind::Breakable(_) => false,
        }
    }

    /// Whether it moves (its pose follows the race clock).
    pub fn moves(self) -> bool {
        self.knocks()
    }

    /// Something to drive through.
    pub fn bursts(self) -> bool {
        matches!(self, HazardKind::Breakable(_))
    }

    /// The bots keep clear of it (everything but what bursts).
    pub fn in_the_way(self) -> bool {
        !self.bursts()
    }
}

/// The size of a hazard at scale 1, in its own frame (local -Z along the road, X across it, Y
/// up): its half extents across and along the road, and its contact shape (a capsule along
/// `axis`, from `low` to `high` over the road at its foot, `radius` thick).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Half extents across (X) and along (Z), on the ground (m).
    pub half_across: f32,
    pub half_along: f32,
    /// The contact capsule: its axis (local), its center's height, its half length, its radius.
    pub axis: Vec3,
    pub height: f32,
    pub half_length: f32,
    pub radius: f32,
}

impl Shape {
    pub fn of(kind: HazardKind) -> Self {
        let shape = |half_across, half_along, axis, height, half_length, radius| Shape {
            half_across,
            half_along,
            axis,
            height,
            half_length,
            radius,
        };
        match kind {
            // A boulder: a box 2.2 m wide and 1.6 m high.
            HazardKind::Obstacle(ObstacleKind::Rock) => shape(1.1, 1.1, Vec3::Y, 0.8, 0.0, 1.1),
            // A trunk lying along the road, 5.9 m long, 0.9 m thick.
            HazardKind::Obstacle(ObstacleKind::Trunk) => shape(0.45, 2.95, Vec3::Z, 0.4, 2.5, 0.45),
            // A pillar of rock, 1.6 m wide, 6 m high.
            HazardKind::Obstacle(ObstacleKind::Pillar) => shape(0.8, 0.8, Vec3::Y, 3.0, 3.0, 0.8),
            // A log lying along the road, 6.4 m long, 1.2 m thick, rolling across it.
            HazardKind::Obstacle(ObstacleKind::RollingLog) => {
                shape(0.6, 3.2, Vec3::Z, 0.6, 2.6, 0.6)
            }
            // A stone 2 m wide, hanging from a rope.
            HazardKind::Obstacle(ObstacleKind::SwingingStone) => {
                shape(1.0, 1.0, Vec3::Y, 1.0 + STONE_CLEARANCE, 0.0, 1.0)
            }
            // A bison: a round woolly body on short legs, 2 m wide, 2.4 m high.
            HazardKind::Npc(NpcKind::Bison) => shape(1.0, 1.3, Vec3::Y, 1.25, 0.3, 1.0),
            HazardKind::Breakable(BreakableKind::HayBale) => {
                shape(0.9, 0.6, Vec3::Y, 0.6, 0.0, 0.9)
            }
            HazardKind::Breakable(BreakableKind::Balloon) => {
                shape(0.6, 0.6, Vec3::Y, 1.4, 0.0, 0.65)
            }
            HazardKind::Breakable(BreakableKind::Crate) => {
                shape(0.65, 0.65, Vec3::Y, 0.65, 0.0, 0.75)
            }
        }
    }
}

/// A segment with a thickness: a capsule (a sphere when its ends meet).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}

impl Capsule {
    /// The gap between the two surfaces (negative: they overlap), and the points of the two
    /// segments closest to each other (on `self`, on `other`).
    pub fn gap(&self, other: &Capsule) -> (f32, Vec3, Vec3) {
        let (on_self, on_other) = closest_points(self.a, self.b, other.a, other.b);
        (
            on_self.distance(on_other) - self.radius - other.radius,
            on_self,
            on_other,
        )
    }

    /// Whether the two touch.
    pub fn touches(&self, other: &Capsule) -> bool {
        self.gap(other).0 <= 0.0
    }
}

/// The closest points of the segments `p1..q1` and `p2..q2` (Ericson, « Real-Time Collision
/// Detection » § 5.1.9), degenerate segments (points) included.
pub fn closest_points(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    const EPSILON: f32 = 1e-9;
    let (s, t) = if a <= EPSILON && e <= EPSILON {
        (0.0, 0.0)
    } else if a <= EPSILON {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= EPSILON {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            let mut s = if denominator > EPSILON {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// A kart's contact shape: a capsule along its length, as wide as the kart (its half extents).
pub fn kart_capsule(kart: &Transform) -> Capsule {
    let radius = KART_HALF_EXTENTS.x;
    let half = (KART_HALF_EXTENTS.z - radius).max(0.0);
    let forward = kart.forward().as_vec3();
    Capsule {
        a: kart.translation - forward * half,
        b: kart.translation + forward * half,
        radius,
    }
}

/// The phase of hazard `index` in a race of seed `seed`: a fraction of its period, the same on
/// every peer (splitmix64 of the two).
pub fn hazard_phase(seed: u64, index: u16) -> f32 {
    let mut z = seed ^ (u64::from(index) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// The walk of a passer-by at `u` (a fraction of its period): where it is across its sweep (-1
/// to 1), and where it faces in the road's frame (radians around the road's normal: 0 down the
/// road, π/2 toward its right, π back toward the karts that come). It stops `pause` of the period
/// in all (half at each end), turning there from where it came from to face the karts, then to
/// where it goes: its facing never jumps.
pub fn patrol(u: f32, pause: f32) -> (f32, f32) {
    let u = u.rem_euclid(1.0);
    let hold = (pause / 2.0).clamp(0.0, 0.45);
    let travel = 0.5 - hold;
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let (left, right) = (3.0 * FRAC_PI_2, FRAC_PI_2);
    if u < hold {
        // Stopped on the left: from facing left, to the karts, to the right.
        (-1.0, left + (right - left) * smooth(u / hold.max(1e-6)))
    } else if u < hold + travel {
        (-1.0 + 2.0 * smooth((u - hold) / travel), right)
    } else if u < 2.0 * hold + travel {
        // Stopped on the right: from facing right, to the karts, to the left.
        let t = (u - hold - travel) / hold.max(1e-6);
        (1.0, right + (left - right) * smooth(t))
    } else {
        (1.0 - 2.0 * smooth((u - 2.0 * hold - travel) / travel), left)
    }
}

/// A hazard of the circuit.
#[derive(Clone, Debug, PartialEq)]
pub struct Hazard {
    pub kind: HazardKind,
    /// Its number on the circuit, in the order of the file (the obstacles, then the passers-by,
    /// then the things to drive through, row by row): its phase comes from it.
    pub index: u16,
    /// The road where it stands (the centerline's sample at its parameter).
    pub road: TrackSample,
    /// Across the road from the centerline (m, right positive): its middle.
    pub lateral: f32,
    /// A moving one: how far it goes on each side of `lateral` (m).
    pub sweep: f32,
    /// A moving one: a whole back and forth (s).
    pub period: f32,
    pub size: f32,
    /// A fixed one: turned this much from the road's direction (rad, positive to the right).
    pub yaw: f32,
    /// On the mirror image of its circuit (issue #19): it moves the other way across the road,
    /// the mirror image of the original's motion at the same time.
    pub mirrored: bool,
}

impl Hazard {
    /// The hazards of the circuit `data`, its centerline `center`.
    pub fn all(center: &CenterLine, data: &TrackData) -> Vec<Hazard> {
        let at = |param: f32| center.at(center.param_to_distance(param));
        let mut hazards = Vec::new();
        let mut index = 0u16;
        let mut next = || {
            index += 1;
            index - 1
        };
        for o in &data.obstacles {
            let road = at(o.at);
            hazards.push(Hazard {
                kind: HazardKind::Obstacle(o.kind),
                index: next(),
                lateral: o.offset * road.half_width,
                sweep: o.sweep * road.half_width,
                period: o.period,
                size: o.size,
                yaw: o.yaw.to_radians(),
                mirrored: data.mirrored,
                road,
            });
        }
        for npc in &data.npcs {
            let road = at(npc.at);
            hazards.push(Hazard {
                kind: HazardKind::Npc(npc.kind),
                index: next(),
                lateral: npc.offset * road.half_width,
                sweep: npc.sweep * road.half_width,
                period: npc.period,
                size: npc.size,
                yaw: 0.0,
                mirrored: data.mirrored,
                road,
            });
        }
        for row in &data.breakables {
            let road = at(row.at);
            for offset in &row.offsets {
                hazards.push(Hazard {
                    kind: HazardKind::Breakable(row.kind),
                    index: next(),
                    lateral: offset * road.half_width,
                    sweep: 0.0,
                    period: 1.0,
                    size: 1.0,
                    yaw: 0.0,
                    mirrored: data.mirrored,
                    road,
                });
            }
        }
        hazards
    }

    pub fn shape(&self) -> Shape {
        Shape::of(self.kind)
    }

    /// The road's rotation where it stands (local -Z down the road, Y its normal).
    fn base(&self) -> Quat {
        self.road.transform(0.0, 0.0).rotation
    }

    /// Where it is at `seconds` on the race clock, its phase `phase` (a fraction of its period):
    /// its foot on the road (a rolling log, a swinging stone: its center), turned as it is.
    pub fn pose(&self, seconds: f32, phase: f32) -> Transform {
        let base = self.base();
        let u = (seconds / self.period.max(1e-3) + phase).rem_euclid(1.0);
        // The mirror image goes the other way across the road (its right is the original's left).
        let sign = if self.mirrored { -1.0 } else { 1.0 };
        let shape = self.shape();
        match self.kind {
            HazardKind::Obstacle(ObstacleKind::RollingLog) => {
                let across = sign * self.sweep * (TAU * u).sin();
                let radius = shape.radius * self.size;
                // Rolling without sliding: a turn of its way across over its radius.
                let roll = -across / radius;
                Transform::from_translation(self.road.point(self.lateral + across, radius))
                    .with_rotation(base * Quat::from_rotation_z(roll))
            }
            HazardKind::Obstacle(ObstacleKind::SwingingStone) => {
                let reach = (self.sweep / ROPE_LENGTH).clamp(0.0, 0.9).asin();
                let swing = sign * reach * (TAU * u).sin();
                let rotation = base * Quat::from_rotation_z(swing);
                Transform::from_translation(
                    self.pivot() + rotation * Vec3::new(0.0, -ROPE_LENGTH, 0.0),
                )
                .with_rotation(rotation)
            }
            HazardKind::Npc(_) => {
                let (s, facing) = patrol(u, NPC_PAUSE_SHARE);
                // Facing 0 is down the road (local -Z); a positive angle turns it to the right.
                let at = self.road.point(self.lateral + sign * self.sweep * s, 0.0);
                Transform::from_translation(at)
                    .with_rotation(base * Quat::from_rotation_y(-sign * facing))
            }
            _ => Transform::from_translation(self.road.point(self.lateral, 0.0))
                .with_rotation(base * Quat::from_rotation_y(-self.yaw)),
        }
    }

    /// A swinging stone's pivot, over the middle of its swing (m).
    pub fn pivot(&self) -> Vec3 {
        let shape = self.shape();
        let height = ROPE_LENGTH + shape.radius * self.size + STONE_CLEARANCE;
        self.road.point(self.lateral, height)
    }

    /// Its contact shape at `pose` (from [`Hazard::pose`]).
    pub fn body(&self, pose: &Transform) -> Capsule {
        let shape = self.shape();
        let up = self.road.up;
        let center = match self.kind {
            // Their pose is their center (they turn around it).
            HazardKind::Obstacle(ObstacleKind::SwingingStone | ObstacleKind::RollingLog) => {
                pose.translation
            }
            _ => pose.translation + up * shape.height * self.size,
        };
        let axis = pose.rotation * shape.axis * shape.half_length * self.size;
        Capsule {
            a: center - axis,
            b: center + axis,
            radius: shape.radius * self.size,
        }
    }

    /// The rectangle of the ground (x, z) it may cover, its whole motion included: the bots keep
    /// clear of it (`crate::bots::RacingLine`). Its corners in turn around it.
    pub fn footprint(&self) -> [Vec2; 4] {
        let shape = self.shape();
        let (across, along) = (shape.half_across * self.size, shape.half_along * self.size);
        let road_forward = self.road.forward.xz().normalize_or(Vec2::NEG_Y);
        let road_right = Vec2::new(-road_forward.y, road_forward.x);
        // Its own frame on the ground: the road's, a fixed one turned by its yaw.
        let (forward, right) = if self.kind.moves() {
            (road_forward, road_right)
        } else {
            let (sin, cos) = self.yaw.sin_cos();
            (
                road_forward * cos + road_right * sin,
                road_right * cos - road_forward * sin,
            )
        };
        let middle = self.road.point(self.lateral, 0.0).xz();
        let reach = self.sweep + across;
        [
            middle - right * reach - forward * along,
            middle + right * reach - forward * along,
            middle + right * reach + forward * along,
            middle - right * reach + forward * along,
        ]
    }
}

/// Seconds on the race clock at flow tick `tick` (60 ticks a second).
pub fn clock_seconds(tick: u32) -> f32 {
    tick as f32 / crate::TICK_HZ as f32
}

/// Where a kart touched by a moving hazard is pushed: away from the hazard, along the road's
/// plane (unit; down the road when the two centers meet).
pub fn push_direction(hazard: Vec3, kart: Vec3, up: Vec3) -> Vec3 {
    let away = kart - hazard;
    let flat = away - up * away.dot(up);
    flat.normalize_or(Vec3::NEG_Z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::data::{BreakableRow, NpcData, ObstacleData};
    use std::f32::consts::PI;

    const SQUARE: &str = "(name: \"Square\", points: [(pos: (0.0, 0.0, 0.0)), (pos: (0.0, 0.0, -100.0)), (pos: (100.0, 0.0, -100.0)), (pos: (100.0, 0.0, 0.0))])";

    fn square_with(data: impl FnOnce(&mut TrackData)) -> (CenterLine, TrackData) {
        let mut track = TrackData::from_ron(SQUARE).unwrap();
        data(&mut track);
        track.validate().unwrap();
        (CenterLine::of(&track), track)
    }

    fn obstacle(kind: ObstacleKind, offset: f32, sweep: f32) -> ObstacleData {
        ObstacleData {
            kind,
            at: 0.5,
            offset,
            size: 1.0,
            yaw: 0.0,
            sweep,
            period: 4.0,
        }
    }

    #[test]
    fn closest_points_of_segments_and_capsules() {
        // Two crossing segments, one above the other.
        let (a, b) = closest_points(
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, -1.0),
            Vec3::new(0.0, 2.0, 1.0),
        );
        assert!(a.distance(Vec3::ZERO) < 1e-5 && b.distance(Vec3::Y * 2.0) < 1e-5);
        // Parallel ones, and a point against a segment, beyond its end.
        let (a, b) = closest_points(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Y + Vec3::X);
        assert!((a.distance(b) - 1.0).abs() < 1e-5);
        let (a, b) = closest_points(Vec3::ZERO, Vec3::ZERO, Vec3::X * 2.0, Vec3::X * 3.0);
        assert_eq!((a, b), (Vec3::ZERO, Vec3::X * 2.0));
        let sphere = |at: Vec3, radius: f32| Capsule {
            a: at,
            b: at,
            radius,
        };
        assert!(sphere(Vec3::ZERO, 1.0).touches(&sphere(Vec3::X * 1.9, 1.0)));
        assert!(!sphere(Vec3::ZERO, 1.0).touches(&sphere(Vec3::X * 2.1, 1.0)));
        // A kart's capsule: 2 m long, 1.4 m wide, like its box.
        let kart = kart_capsule(&Transform::from_xyz(0.0, 0.5, 0.0));
        assert!(kart.touches(&sphere(Vec3::new(0.0, 0.5, -1.9), 0.95)));
        assert!(!kart.touches(&sphere(Vec3::new(0.0, 0.5, -2.1), 0.95)));
        assert!(kart.touches(&sphere(Vec3::new(1.6, 0.5, 0.0), 0.95)));
        assert!(!kart.touches(&sphere(Vec3::new(1.7, 0.5, 0.0), 0.95)));
    }

    /// The phase: the same for the same seed and number, spread over the period otherwise.
    #[test]
    fn the_phase_comes_from_the_seed_and_the_number() {
        assert_eq!(hazard_phase(7, 3), hazard_phase(7, 3));
        assert_ne!(hazard_phase(7, 3), hazard_phase(8, 3));
        assert_ne!(hazard_phase(7, 3), hazard_phase(7, 4));
        let phases: Vec<f32> = (0..200).map(|i| hazard_phase(i, 0)).collect();
        assert!(phases.iter().all(|p| (0.0..1.0).contains(p)));
        for quarter in 0..4 {
            let low = quarter as f32 / 4.0;
            let count = phases
                .iter()
                .filter(|p| (low..low + 0.25).contains(*p))
                .count();
            assert!((30..=70).contains(&count), "quarter {quarter}: {count}");
        }
    }

    /// A passer-by's walk: from one end to the other and back, stopped a while at each end; its
    /// position and its facing never jump; it faces the way it walks, and the karts while it
    /// waits.
    #[test]
    fn a_passer_by_walks_back_and_forth_and_never_jumps() {
        let steps = 2000;
        let mut previous = patrol(0.0, NPC_PAUSE_SHARE);
        let (mut stopped, mut lowest, mut highest) = (0, 0.0f32, 0.0f32);
        for k in 1..=steps {
            let u = k as f32 / steps as f32;
            let (s, facing) = patrol(u, NPC_PAUSE_SHARE);
            assert!((-1.0..=1.0).contains(&s));
            assert!(
                (s - previous.0).abs() < 0.01,
                "at {u}: {s} after {}",
                previous.0
            );
            assert!((facing - previous.1).abs() < 0.02, "at {u}");
            if s == previous.0 {
                stopped += 1;
            } else {
                // Walking: it faces the way it goes.
                let goes_right = s > previous.0;
                let expected = if goes_right {
                    FRAC_PI_2
                } else {
                    3.0 * FRAC_PI_2
                };
                assert!((facing - expected).abs() < 1e-4, "at {u}");
            }
            (lowest, highest) = (lowest.min(s), highest.max(s));
            previous = (s, facing);
        }
        assert_eq!((lowest, highest), (-1.0, 1.0));
        let share = stopped as f32 / steps as f32;
        assert!((share - NPC_PAUSE_SHARE).abs() < 0.02, "{share}");
        // Halfway through a stop, it faces the karts that come (back up the road).
        let (_, facing) = patrol(NPC_PAUSE_SHARE / 4.0, NPC_PAUSE_SHARE);
        assert!((facing - PI).abs() < 1e-4);
    }

    /// The moving hazards: each stays on its side of the road, within its sweep and in the
    /// rectangle the bots keep clear of; the rolling log rolls (it turns as it goes), the
    /// swinging stone swings under its pivot; the same pose for the same time and phase.
    #[test]
    fn the_moving_hazards_keep_to_their_sweep_and_their_footprint() {
        let (center, data) = square_with(|d| {
            d.obstacles = vec![
                obstacle(ObstacleKind::RollingLog, 0.5, 0.4),
                obstacle(ObstacleKind::SwingingStone, -0.5, 0.4),
            ];
            d.npcs = vec![NpcData {
                kind: NpcKind::Bison,
                at: 1.5,
                offset: 0.55,
                sweep: 0.35,
                period: 5.0,
                size: 1.0,
            }];
        });
        let hazards = Hazard::all(&center, &data);
        assert_eq!(hazards.len(), 3);
        assert_eq!(
            hazards.iter().map(|h| h.index).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        for hazard in &hazards {
            assert!(hazard.kind.knocks() && hazard.kind.in_the_way());
            let footprint = hazard.footprint();
            let inside = |p: Vec2, margin: f32| {
                // Inside the rectangle (its corners in order), with a margin.
                (0..4).all(|i| {
                    let (a, b) = (footprint[i], footprint[(i + 1) % 4]);
                    let edge = (b - a).normalize();
                    let normal = Vec2::new(edge.y, -edge.x);
                    (p - a).dot(normal) <= margin
                }) || (0..4).all(|i| {
                    let (a, b) = (footprint[i], footprint[(i + 1) % 4]);
                    let edge = (b - a).normalize();
                    let normal = Vec2::new(-edge.y, edge.x);
                    (p - a).dot(normal) <= margin
                })
            };
            let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
            for k in 0..400 {
                let seconds = k as f32 * 0.05;
                let pose = hazard.pose(seconds, 0.3);
                assert_eq!(pose, hazard.pose(seconds, 0.3));
                let body = hazard.body(&pose);
                let across = (body.a - hazard.road.position).dot(hazard.road.right);
                (lowest, highest) = (lowest.min(across), highest.max(across));
                // The body's ends, and its sides, in the rectangle.
                for end in [body.a, body.b] {
                    for side in [-1.0, 1.0] {
                        let edge = end + hazard.road.right * side * body.radius;
                        assert!(inside(edge.xz(), 1e-3), "{:?} at {seconds}", hazard.kind);
                    }
                }
                // On its side of the road: the other half stays free.
                assert!(
                    across.signum() == hazard.lateral.signum(),
                    "{:?}",
                    hazard.kind
                );
            }
            assert!(
                highest - lowest > hazard.sweep * 1.8,
                "{:?}: it moves across its sweep ({lowest} to {highest})",
                hazard.kind
            );
        }
        // The log rolls: a quarter of its period, a turn of its way across over its radius.
        let log = &hazards[0];
        let (start, quarter) = (log.pose(0.0, 0.0), log.pose(1.0, 0.0));
        let turned = start.rotation.inverse() * quarter.rotation;
        let expected = Quat::from_rotation_z(-log.sweep / 0.6);
        assert!(turned.angle_between(expected) < 1e-3, "{turned}");
        // The stone hangs from its pivot, the rope's length below it.
        let stone = &hazards[1];
        for k in 0..20 {
            let pose = stone.pose(k as f32 * 0.2, 0.0);
            assert!((pose.translation.distance(stone.pivot()) - ROPE_LENGTH).abs() < 1e-3);
            let bottom = pose.translation.y - 1.0;
            assert!((STONE_CLEARANCE - 1e-3..1.2).contains(&bottom), "{bottom}");
        }
    }

    /// A fixed obstacle stands still, turned by its yaw; its footprint turns with it. The things
    /// to drive through: one per offset of their row, in the file's order after the others.
    #[test]
    fn fixed_obstacles_and_rows_of_things_to_drive_through() {
        let (center, data) = square_with(|d| {
            d.obstacles = vec![ObstacleData {
                yaw: 90.0,
                ..obstacle(ObstacleKind::Trunk, 0.3, 0.0)
            }];
            d.breakables = vec![BreakableRow {
                kind: BreakableKind::Crate,
                at: 2.5,
                offsets: vec![-0.5, 0.5],
            }];
        });
        let hazards = Hazard::all(&center, &data);
        assert_eq!(hazards.len(), 3);
        let trunk = &hazards[0];
        assert!(trunk.kind.stands() && !trunk.kind.moves());
        assert_eq!(trunk.pose(0.0, 0.0), trunk.pose(10.0, 0.7));
        // The first side heads north (-Z): turned 90° to the right, the trunk lies east-west,
        // and so does its footprint.
        let footprint = trunk.footprint();
        let xs = footprint.map(|p| p.x);
        let zs = footprint.map(|p| p.y);
        let width = |v: [f32; 4]| {
            v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(
            (width(xs) - 5.9).abs() < 0.05 && (width(zs) - 0.9).abs() < 0.05,
            "{footprint:?}"
        );
        let axis = trunk.pose(0.0, 0.0).rotation * Vec3::Z;
        assert!(axis.x.abs() > 0.99, "{axis}");
        assert!(
            hazards[1..]
                .iter()
                .all(|h| h.kind.bursts() && !h.kind.in_the_way())
        );
        assert_eq!(
            hazards[1..]
                .iter()
                .map(|h| (h.index, h.lateral.signum()))
                .collect::<Vec<_>>(),
            [(1, -1.0), (2, 1.0)]
        );
    }

    /// The mirror image of a circuit has the mirror image of its hazards: at the same time and
    /// phase, the same pose across the X = 0 plane.
    #[test]
    fn the_mirror_image_moves_its_hazards_in_mirror() {
        let (center, data) = square_with(|d| {
            d.obstacles = vec![
                obstacle(ObstacleKind::RollingLog, 0.5, 0.4),
                obstacle(ObstacleKind::SwingingStone, -0.5, 0.4),
            ];
            d.npcs = vec![NpcData {
                kind: NpcKind::Bison,
                at: 1.5,
                offset: 0.55,
                sweep: 0.35,
                period: 5.0,
                size: 1.0,
            }];
        });
        let mirrored = data.mirrored();
        let mirror_center = CenterLine::of(&mirrored);
        let (a, b) = (
            Hazard::all(&center, &data),
            Hazard::all(&mirror_center, &mirrored),
        );
        for (one, other) in a.iter().zip(&b) {
            for k in 0..30 {
                let seconds = k as f32 * 0.37;
                let (p, q) = (
                    one.body(&one.pose(seconds, 0.6)),
                    other.body(&other.pose(seconds, 0.6)),
                );
                let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
                let close = |u: Vec3, v: Vec3| u.distance(v) < 1e-2;
                assert!(
                    (close(flip(p.a), q.a) && close(flip(p.b), q.b))
                        || (close(flip(p.a), q.b) && close(flip(p.b), q.a)),
                    "{:?} at {seconds}: {p:?} / {q:?}",
                    one.kind
                );
            }
        }
    }

    #[test]
    fn a_touched_kart_is_pushed_away_along_the_road() {
        let push = push_direction(Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 0.5, 0.0), Vec3::Y);
        assert!(push.distance(Vec3::X) < 1e-5);
        assert_eq!(push_direction(Vec3::ZERO, Vec3::Y, Vec3::Y), Vec3::NEG_Z);
    }
}
