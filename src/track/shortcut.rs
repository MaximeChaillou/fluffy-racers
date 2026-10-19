//! Issue #107 (GD « Conception des circuits »): the stretches without wall and the shortcuts of a
//! circuit, pure geometry (no ECS).
//!
//! - **A stretch without wall** ([`OpenEdge`]) is a stretch of the centerline where one side of
//!   the road (or both) has no wall: its shoulder ends on a drop. A kart that goes over falls; the
//!   tow pelican drops it back before the stretch (`TrackLayout::new` puts no drop point in it).
//! - **A shortcut** ([`Shortcut`]) is a path off the centerline, from the road at `from` to the
//!   road at `to` (centerline distances): a hidden road, a grass path (off-road: worth it with a
//!   boost), or a risky jump over a drop (a trick ramp, then nothing until the landing). Its path
//!   is an open Catmull-Rom spline through its points, sampled about every meter like the
//!   centerline ([`TrackSample`]: flat across, its up square to its slope). The race counts a
//!   kart on it along its own path, mapped onto the centerline's distances between `from` and
//!   `to` (`crate::race::Course::project`), and the bots follow it (`crate::bots`).

use bevy::prelude::*;

use super::data::{EdgeSide, OpenEdgeData, ShortcutData, ShortcutKind};
use super::layout::{CenterLine, TrackSample};

/// A shortcut's path is sampled about this often (m).
pub const PATH_STEP: f32 = 1.0;
/// Fine sub-steps per segment of a shortcut's path, to measure its length.
const SUBSTEPS: usize = 64;

/// A stretch without wall (issue #107), along the centerline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpenEdge {
    /// Centerline distances of its start and end; `to` is below `from` when it crosses the
    /// start line.
    pub from: f32,
    pub to: f32,
    pub side: EdgeSide,
}

impl OpenEdge {
    pub fn new(center: &CenterLine, data: &OpenEdgeData) -> Self {
        Self {
            from: center.wrap(center.param_to_distance(data.from)),
            to: center.wrap(center.param_to_distance(data.to)),
            side: data.side,
        }
    }

    /// Its length along the centerline (m).
    pub fn length(&self, center: &CenterLine) -> f32 {
        center.wrap(self.to - self.from)
    }

    /// Whether the centerline distance `distance` (any lap) is in it.
    pub fn contains(&self, center: &CenterLine, distance: f32) -> bool {
        center.wrap(distance - self.from) <= self.length(center)
    }
}

/// Whether the wall of side `sign` (-1 left, 1 right) stands at centerline distance `distance`:
/// everywhere but in a stretch of `edges` that opens that side.
pub fn walled(edges: &[OpenEdge], center: &CenterLine, distance: f32, sign: f32) -> bool {
    !edges
        .iter()
        .any(|edge| edge.side.opens(sign) && edge.contains(center, distance))
}

/// A risky jump's trick ramp (issue #107): a box like the trick ramps of the file, on the road,
/// climbing along `direction` (on the ground plane) from `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ramp {
    pub start: Vec3,
    /// Unit, horizontal.
    pub direction: Vec3,
    pub width: f32,
    pub length: f32,
    /// Its slope (rad).
    pub angle: f32,
}

impl Ramp {
    /// Its rotation before its slope: local -Z along `direction`, local Y up.
    pub fn base(&self) -> Quat {
        Transform::default()
            .looking_to(self.direction, Vec3::Y)
            .rotation
    }

    /// The four corners of its footprint on the ground plane (x, z).
    pub fn footprint(&self) -> [Vec2; 4] {
        let along = self.direction.xz() * self.length * self.angle.cos();
        let across = Vec2::new(-self.direction.z, self.direction.x) * self.width / 2.0;
        let start = self.start.xz();
        [
            start - across,
            start + across,
            start + along + across,
            start + along - across,
        ]
    }

    /// Where its lip is (the middle of its top end).
    pub fn lip(&self) -> Vec3 {
        self.start
            + self.direction * self.length * self.angle.cos()
            + Vec3::Y * self.length * self.angle.sin()
    }
}

/// Where a kart is relative to a shortcut's path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnPath {
    /// The nearest sample.
    pub index: usize,
    /// Distance along the path from its start (m): below 0 before it, beyond its length after.
    pub along: f32,
    /// Across it, right positive (m), and above it (m).
    pub lateral: f32,
    pub height: f32,
}

/// A shortcut of the circuit (issue #107).
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub kind: ShortcutKind,
    /// Centerline distances where it leaves the road and where it comes back to it.
    pub from: f32,
    pub to: f32,
    /// The centerline's length from `from` to `to` (m): what the path stands for in the race.
    pub span: f32,
    pub half_width: f32,
    /// The lower of its ends' heights (m): a kart that falls off a jump falls below it.
    pub low: f32,
    /// A jump's trick ramp.
    pub ramp: Option<Ramp>,
    samples: Vec<TrackSample>,
    length: f32,
}

impl Shortcut {
    pub fn new(center: &CenterLine, data: &ShortcutData) -> Self {
        let from = center.wrap(center.param_to_distance(data.from));
        let to = center.wrap(center.param_to_distance(data.to));
        let points: Vec<Vec3> = (0..data.points.len()).map(|i| data.point(i)).collect();
        let (samples, length) = open_path(&points, data.width / 2.0, PATH_STEP);
        let ramp = (data.kind == ShortcutKind::Jump).then(|| {
            let along = points[2] - points[1];
            Ramp {
                start: points[1],
                direction: Vec3::new(along.x, 0.0, along.z).normalize_or(Vec3::NEG_Z),
                width: data.ramp_width,
                length: data.ramp_length,
                angle: data.ramp_angle.to_radians(),
            }
        });
        Self {
            kind: data.kind,
            from,
            to,
            span: center.wrap(to - from),
            half_width: data.width / 2.0,
            low: points[0].y.min(points[points.len() - 1].y),
            ramp,
            samples,
            length,
        }
    }

    /// The length of its path (m).
    pub fn length(&self) -> f32 {
        self.length
    }

    pub fn samples(&self) -> &[TrackSample] {
        &self.samples
    }

    /// The path at `along` meters from its start (clamped to it), between its samples.
    pub fn at(&self, along: f32) -> TrackSample {
        let last = self.samples.len() - 1;
        let step = self.length / last as f32;
        let along = along.clamp(0.0, self.length);
        let i = ((along / step).floor() as usize).min(last - 1);
        let (a, b) = (&self.samples[i], &self.samples[i + 1]);
        let t = ((along - a.distance) / step).clamp(0.0, 1.0);
        TrackSample {
            distance: along,
            position: a.position.lerp(b.position, t),
            forward: a.forward.lerp(b.forward, t).normalize_or(a.forward),
            right: a.right.lerp(b.right, t).normalize_or(a.right),
            up: a.up.lerp(b.up, t).normalize_or(a.up),
            half_width: a.half_width,
            shoulder: 0.0,
        }
    }

    /// Where `position` is relative to its path: its nearest sample (brute force: a path has a
    /// few hundred samples at most).
    pub fn locate(&self, position: Vec3) -> OnPath {
        let index = self
            .samples
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.position
                    .distance_squared(position)
                    .total_cmp(&b.position.distance_squared(position))
            })
            .map_or(0, |(i, _)| i);
        let s = &self.samples[index];
        let offset = position - s.position;
        OnPath {
            index,
            along: s.distance + offset.dot(s.forward),
            lateral: offset.dot(s.right),
            height: offset.dot(s.up),
        }
    }

    /// The share of its path done at `along` (0 at its start, 1 at its end).
    pub fn share(&self, along: f32) -> f32 {
        (along / self.length.max(1e-3)).clamp(0.0, 1.0)
    }
}

/// The open Catmull-Rom spline through `points` (the ends extended by their mirror image), sampled
/// at a constant step close to `step` from its first point exactly to its last one: its samples
/// (`half_width` wide, flat across) and its length.
pub fn open_path(points: &[Vec3], half_width: f32, step: f32) -> (Vec<TrackSample>, f32) {
    let n = points.len();
    assert!(n >= 2, "a path needs two points");
    let point = |i: isize| -> Vec3 {
        if i < 0 {
            2.0 * points[0] - points[1]
        } else if i as usize >= n {
            2.0 * points[n - 1] - points[n - 2]
        } else {
            points[i as usize]
        }
    };
    let curve = |param: f32| -> (Vec3, Vec3) {
        let segment = (param.floor() as isize).clamp(0, n as isize - 2);
        let t = param - segment as f32;
        let (p0, p1, p2, p3) = (
            point(segment - 1),
            point(segment),
            point(segment + 1),
            point(segment + 2),
        );
        let (t2, t3) = (t * t, t * t * t);
        let position = 0.5
            * ((2.0 * p1)
                + (p2 - p0) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3);
        let tangent = 0.5
            * ((p2 - p0)
                + 2.0 * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t
                + 3.0 * (3.0 * p1 - p0 - 3.0 * p2 + p3) * t2);
        (position, tangent)
    };
    let fine = (n - 1) * SUBSTEPS;
    let mut params = Vec::with_capacity(fine + 1);
    let mut lengths = Vec::with_capacity(fine + 1);
    let mut total = 0.0;
    let mut previous = points[0];
    for k in 0..=fine {
        let param = k as f32 / SUBSTEPS as f32;
        let position = curve(param).0;
        total += position.distance(previous);
        previous = position;
        params.push(param);
        lengths.push(total);
    }
    let count = ((total / step).round() as usize).max(2);
    let step = total / count as f32;
    let mut samples = Vec::with_capacity(count + 1);
    let mut j = 0;
    for i in 0..=count {
        let distance = (i as f32 * step).min(total);
        while j + 1 < fine && lengths[j + 1] < distance {
            j += 1;
        }
        let span = (lengths[j + 1] - lengths[j]).max(1e-6);
        let t = ((distance - lengths[j]) / span).clamp(0.0, 1.0);
        let (position, tangent) = curve(params[j] + (params[j + 1] - params[j]) * t);
        let forward = tangent.normalize_or(Vec3::NEG_Z);
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        samples.push(TrackSample {
            distance,
            position,
            forward,
            right,
            up: right.cross(forward),
            half_width,
            shoulder: 0.0,
        });
    }
    (samples, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackData;

    /// A straight path, then a bend: sampled from its first point to its last, its length the
    /// polyline's for the straight one, a right turn to the right, its up square to its slope.
    #[test]
    fn a_path_runs_from_its_first_point_to_its_last() {
        let points = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, -20.0)];
        let (samples, length) = open_path(&points, 4.0, 1.0);
        assert!((length - 20.0f32.hypot(2.0)).abs() < 1e-3, "{length}");
        assert_eq!(samples.len(), 21);
        assert!(samples[0].position.distance(points[0]) < 1e-4);
        assert!(samples[20].position.distance(points[1]) < 1e-3);
        for s in &samples {
            assert!((s.forward.length() - 1.0).abs() < 1e-4);
            assert!(s.right.y.abs() < 1e-5, "flat across");
            assert!(s.up.dot(s.forward).abs() < 1e-4 && s.up.y > 0.99);
            assert_eq!(s.half_width, 4.0);
        }
        // A bend to the right (east): the path goes through its middle point.
        let bend = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -20.0),
            Vec3::new(20.0, 0.0, -40.0),
        ];
        let (samples, length) = open_path(&bend, 4.0, 1.0);
        assert!(length > 20.0 + 20.0 * 2f32.sqrt() - 1.0, "{length}");
        assert!(samples.iter().any(|s| s.position.distance(bend[1]) < 0.6));
        assert!(
            samples.last().unwrap().forward.x > 0.5,
            "heading east-north-east"
        );
    }

    /// A shortcut maps onto the centerline between its ends; a kart is located on its path; a
    /// jump has its ramp from its second point toward its third, its low end the lower one.
    #[test]
    fn a_shortcut_knows_its_path_and_its_ramp() {
        let text = "(name: \"Square\", points: [(pos: (0.0, 0.0, 0.0)), (pos: (0.0, 0.0, -50.0)), \
                    (pos: (50.0, 0.0, -50.0)), (pos: (50.0, 0.0, 0.0))], \
                    open_edges: [(from: 0.3, to: 1.7, side: Right)], \
                    shortcuts: [(from: 0.5, to: 1.5, kind: Jump, points: [(5.0, 0.0, -20.0), \
                    (7.0, 0.0, -26.0), (12.0, 0.5, -31.0), (30.0, -1.0, -43.0), \
                    (34.0, -1.0, -44.0)])])";
        let data = TrackData::from_ron(text).unwrap();
        let center = CenterLine::of(&data);
        let shortcut = Shortcut::new(&center, &data.shortcuts[0]);
        assert_eq!(shortcut.kind, ShortcutKind::Jump);
        assert!((shortcut.span - center.wrap(shortcut.to - shortcut.from)).abs() < 1e-4);
        assert_eq!(shortcut.low, -1.0);
        let ramp = shortcut.ramp.unwrap();
        assert_eq!(ramp.start, Vec3::new(7.0, 0.0, -26.0));
        assert!((ramp.direction - Vec3::new(1.0, 0.0, -1.0).normalize()).length() < 1e-4);
        assert!(ramp.lip().y > 1.9 && ramp.lip().y < 2.2, "{}", ramp.lip());
        let middle = shortcut.at(shortcut.length() / 2.0);
        let here = shortcut.locate(middle.point(1.5, 0.3));
        assert!(
            (here.along - shortcut.length() / 2.0).abs() < 0.6,
            "{here:?}"
        );
        assert!((here.lateral - 1.5).abs() < 0.05 && (here.height - 0.3).abs() < 0.05);
        assert_eq!(shortcut.share(-3.0), 0.0);
        assert_eq!(shortcut.share(1e4), 1.0);
        // The wall stands but on the right in the stretch without wall.
        let edges: Vec<OpenEdge> = data
            .open_edges
            .iter()
            .map(|e| OpenEdge::new(&center, e))
            .collect();
        let inside = center.param_to_distance(1.0);
        assert!(!walled(&edges, &center, inside, 1.0));
        assert!(walled(&edges, &center, inside, -1.0));
        assert!(walled(&edges, &center, center.param_to_distance(2.5), 1.0));
    }
}
