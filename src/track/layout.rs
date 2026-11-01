//! Pure track geometry (no ECS): the closed Catmull-Rom centerline sampled every meter or so,
//! with a banked frame per sample, checkpoints, the starting grid and placements.

use bevy::prelude::*;

use super::anti_gravity::AntiGravityZone;
use super::data::{
    AntiGravityData, BUMP_TRICK_SHARE, BumpData, ControlPoint, DROP_LIP, DropData, OffroadZone,
    TrackData,
};
use super::hazard::Hazard;
use super::shortcut::{OpenEdge, Shortcut};
use super::water::{Gap, Lake};

/// One point of the sampled centerline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSample {
    /// Distance from the start line along the centerline (m).
    pub distance: f32,
    pub position: Vec3,
    /// Driving direction (unit).
    pub forward: Vec3,
    /// Toward the right edge, along the (banked) road surface (unit).
    pub right: Vec3,
    /// Road surface normal (unit).
    pub up: Vec3,
    /// Half the road width (m).
    pub half_width: f32,
    /// Grass strip between the curb and the wall (m).
    pub shoulder: f32,
}

impl TrackSample {
    /// Point on the road surface, `lateral` meters right of the centerline, `height` above it.
    pub fn point(&self, lateral: f32, height: f32) -> Vec3 {
        self.position + self.right * lateral + self.up * height
    }

    /// Transform on the road, looking down the track.
    pub fn transform(&self, lateral: f32, height: f32) -> Transform {
        Transform::from_translation(self.point(lateral, height)).looking_to(self.forward, self.up)
    }
}

/// A checkpoint: karts must cross them in order to complete a lap (issue #5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Checkpoint {
    pub index: usize,
    pub sample: TrackSample,
}

/// The closed centerline, sampled at a constant step along its length.
#[derive(Clone, Debug)]
pub struct CenterLine {
    samples: Vec<TrackSample>,
    length: f32,
    /// Distance of each control point from the start line (control point 0).
    control_distances: Vec<f32>,
    /// Issue #119: an open circuit's ends (its file's `open`); None on a circuit of laps.
    open: Option<OpenEnds>,
    /// Issue #180: its bumps and its drops (its file's `bumps` and `drops`), along it.
    bumps: Vec<BumpSpan>,
    cliffs: Vec<Cliff>,
}

/// Issue #180: a bump of the road (`TrackData::bumps`), along the centerline: from `from`,
/// `length` meters long (m), `height` meters high at its crest, in its middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BumpSpan {
    pub from: f32,
    pub length: f32,
    pub height: f32,
}

/// Issue #180: a drop (`TrackData::drops`): the road steps down `height` meters at `at` (a
/// distance along the centerline), level on both sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cliff {
    pub at: f32,
    pub height: f32,
}

/// Issue #119: where an open circuit's race and road begin and end, as distances along its
/// centerline from the start line (`TrackData::open`). The road runs from `road_start` (on the
/// lead-in, round the loop: behind the start line) to `road_end`; the rest of the loop is the way
/// back, never built nor driven.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenEnds {
    /// The finish line.
    pub finish: f32,
    /// The end of the run-out after the finish line: a barrier across the road.
    pub road_end: f32,
    /// The start of the lead-in before the start line (in `[0, length)`): a barrier across the
    /// road.
    pub road_start: f32,
    /// Where each section begins: 0 (the start line), then the file's `sections`.
    pub sections: Vec<f32>,
}

impl OpenEnds {
    /// The number of sections (« Section 1/3 »).
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Where section `k` (from 0) ends: where the next one begins, or the finish line.
    pub fn section_end(&self, k: usize) -> f32 {
        self.sections.get(k + 1).copied().unwrap_or(self.finish)
    }
}

/// Fine sub-steps per spline segment used to measure lengths.
const SUBSTEPS: usize = 64;

/// Uniform Catmull-Rom position on the segment p1 → p2.
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

/// Derivative of [`catmull_rom`] with respect to `t`.
fn catmull_rom_tangent(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    0.5 * ((p2 - p0)
        + 2.0 * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t
        + 3.0 * (3.0 * p1 - p0 - 3.0 * p2 + p3) * t2)
}

/// Smooth blend of a per-point value between two control points.
pub(crate) fn blend(a: f32, b: f32, t: f32) -> f32 {
    let s = t * t * (3.0 - 2.0 * t);
    a + (b - a) * s
}

/// Issue #78: the slope of the banking at each control point (degrees per segment). Zero
/// everywhere (the banking settles at each point: [`blend`], as before #78), but strictly inside
/// an anti-gravity zone, where the road twists in one flowing roll from point to point: the
/// slope of its neighbours (Catmull-Rom), zero at a point where the roll turns back (or stops),
/// and never more than three times the slope of either side (Fritsch-Carlson: no overshoot,
/// never beyond the banking of the points around). A kart off the centerline rides the twist like
/// a ramp: a roll that stops at every point would throw it off the wall at every point.
pub fn bank_slopes(points: &[ControlPoint], zones: &[AntiGravityData]) -> Vec<f32> {
    let n = points.len();
    (0..n)
        .map(|i| {
            let param = i as f32;
            let inside = zones.iter().any(|zone| {
                let along = (param - zone.from).rem_euclid(n as f32);
                along > 1e-4 && along < (zone.to - zone.from).rem_euclid(n as f32) - 1e-4
            });
            if !inside {
                return 0.0;
            }
            let before = points[i].bank - points[(i + n - 1) % n].bank;
            let after = points[(i + 1) % n].bank - points[i].bank;
            if before * after <= 0.0 {
                return 0.0;
            }
            let most = 3.0 * before.abs().min(after.abs());
            ((before + after) / 2.0).clamp(-most, most)
        })
        .collect()
}

/// The banking (degrees) at `t` (0 to 1) of the segment from point `a` to point `b`, whose slopes
/// are `slopes` ([`bank_slopes`]): exactly [`blend`] when both are zero, a cubic Hermite curve
/// otherwise.
pub fn bank_between(a: f32, b: f32, slopes: (f32, f32), t: f32) -> f32 {
    if slopes == (0.0, 0.0) {
        return blend(a, b, t);
    }
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * a
        + (t3 - 2.0 * t2 + t) * slopes.0
        + (-2.0 * t3 + 3.0 * t2) * b
        + (t3 - t2) * slopes.1
}

/// Issue #180: the cubic Hermite curve from `a` to `b` (slopes `ma` and `mb` per segment) at `t`
/// (0 to 1), and its slope: a height between two points whose tangent is set (level at a drop).
fn hermite(a: f32, ma: f32, b: f32, mb: f32, t: f32) -> (f32, f32) {
    let (t2, t3) = (t * t, t * t * t);
    (
        (2.0 * t3 - 3.0 * t2 + 1.0) * a
            + (t3 - 2.0 * t2 + t) * ma
            + (-2.0 * t3 + 3.0 * t2) * b
            + (t3 - t2) * mb,
        (6.0 * t2 - 6.0 * t) * a
            + (3.0 * t2 - 4.0 * t + 1.0) * ma
            + (-6.0 * t2 + 6.0 * t) * b
            + (3.0 * t2 - 2.0 * t) * mb,
    )
}

/// The cumulative length of the curve `at` at each of `params` (from 0, evenly spaced), from the
/// first one. Issue #180: a step down of the road (a parameter of `cliffs` within a sub-step) has
/// no length: only the way across it counts there.
fn measure(params: &[f32], cliffs: &[f32], at: impl Fn(f32) -> Vec3) -> Vec<f32> {
    let mut lengths = Vec::with_capacity(params.len());
    let mut total = 0.0;
    let mut previous = at(0.0);
    let mut last = 0.0;
    for &param in params {
        let position = at(param);
        total += if cliffs.iter().any(|c| *c > last && *c <= param) {
            (position - previous).xz().length()
        } else {
            position.distance(previous)
        };
        previous = position;
        last = param;
        lengths.push(total);
    }
    lengths
}

/// Issue #180: the length at spline parameter `param` of the cumulative `lengths` of
/// [`measure`] (one every 1/[`SUBSTEPS`] of a segment).
fn interpolate(lengths: &[f32], param: f32) -> f32 {
    let x = param * SUBSTEPS as f32;
    let k = (x.floor().max(0.0) as usize).min(lengths.len() - 2);
    lengths[k] + (lengths[k + 1] - lengths[k]) * (x - k as f32)
}

/// Issue #180: the spline parameter at length `distance` of the cumulative `lengths` (the
/// inverse of [`interpolate`]).
fn param_at(lengths: &[f32], distance: f32) -> f32 {
    let k = lengths
        .partition_point(|l| *l <= distance)
        .clamp(1, lengths.len() - 1)
        - 1;
    let span = (lengths[k + 1] - lengths[k]).max(1e-6);
    (k as f32 + ((distance - lengths[k]) / span).clamp(0.0, 1.0)) / SUBSTEPS as f32
}

/// Issue #180: the height the bumps `humps` ((start, length, height), along a loop of
/// `loop_length` meters) add to the road at `distance`, and its slope (per meter): half a cosine
/// wave up, half down.
fn hump(humps: &[(f32, f32, f32)], loop_length: f32, distance: f32) -> (f32, f32) {
    humps
        .iter()
        .fold((0.0, 0.0), |(height, slope), (start, length, top)| {
            let x = (distance - start).rem_euclid(loop_length);
            if x >= *length {
                return (height, slope);
            }
            let (sin, cos) = (std::f32::consts::TAU * x / length).sin_cos();
            (
                height + top * (1.0 - cos) / 2.0,
                slope + top * std::f32::consts::PI / length * sin,
            )
        })
}

impl CenterLine {
    /// Samples the closed spline through `points` roughly every `step` meters.
    pub fn new(points: &[ControlPoint], step: f32) -> Self {
        Self::with_zones(points, step, &[])
    }

    /// The centerline of a circuit's file: its points, its step, and its anti-gravity zones,
    /// where the banking flows from point to point (issue #78, [`bank_slopes`]).
    /// Issue #119: an open circuit's ends too.
    pub fn of(data: &TrackData) -> Self {
        let mut center = Self::shaped(
            &data.points,
            data.sample_step,
            &data.anti_gravity,
            &data.bumps,
            &data.drops,
        );
        center.open = data.open.as_ref().map(|open| {
            let distance = |p: f32| center.param_to_distance(p);
            OpenEnds {
                finish: distance(open.finish),
                road_end: distance(open.road_end),
                road_start: distance(open.road_start),
                sections: std::iter::once(0.0)
                    .chain(open.sections.iter().map(|p| distance(*p)))
                    .collect(),
            }
        });
        center
    }

    /// The same, the banking flowing in the anti-gravity zones `zones` ([`bank_slopes`]).
    pub fn with_zones(points: &[ControlPoint], step: f32, zones: &[AntiGravityData]) -> Self {
        Self::shaped(points, step, zones, &[], &[])
    }

    /// The same, with the bumps and the drops of issue #180 (`TrackData::bumps`, `drops`): a
    /// bump adds its hump to the height of the road (a function of the distance along the
    /// centerline without it); a drop makes its segment level with its first point up to the step
    /// and with the next one after it, both ends of the segment reached and left level (the
    /// height's tangent flat there). The step itself has no length: the distances run on through
    /// it, and [`CenterLine::at`] never blends across it. Without either, exactly the centerline
    /// before #180.
    pub fn shaped(
        points: &[ControlPoint],
        step: f32,
        zones: &[AntiGravityData],
        bumps: &[BumpData],
        drops: &[DropData],
    ) -> Self {
        let n = points.len();
        let slopes = bank_slopes(points, zones);
        assert!(n >= 4, "a track needs at least 4 control points");
        let point = |i: usize| points[i % n];
        // Issue #180: each drop's segment and where along it the road steps down (0 to 1).
        let steps: Vec<(usize, f32)> = drops
            .iter()
            .map(|d| {
                let at = d.at.rem_euclid(n as f32);
                ((at.floor() as usize).min(n - 1), at - at.floor())
            })
            .collect();
        let level = |i: usize| {
            steps
                .iter()
                .any(|(s, _)| *s == i % n || (*s + 1) % n == i % n)
        };
        let height_slope = |i: usize| {
            if level(i) {
                0.0
            } else {
                (point(i + 1).pos.1 - point(i + n - 1).pos.1) / 2.0
            }
        };
        let curve_at = |param: f32| {
            let segment = (param.floor() as usize) % n;
            let t = param - param.floor();
            let (p0, p1, p2, p3) = (
                point(segment + n - 1).position(),
                point(segment).position(),
                point(segment + 1).position(),
                point(segment + 2).position(),
            );
            let mut position = catmull_rom(p0, p1, p2, p3, t);
            let mut tangent = catmull_rom_tangent(p0, p1, p2, p3, t);
            if !steps.is_empty() {
                if let Some((_, cliff)) = steps.iter().find(|(s, _)| *s == segment) {
                    position.y = if t < *cliff { p1.y } else { p2.y };
                    tangent.y = 0.0;
                } else if level(segment) || level(segment + 1) {
                    (position.y, tangent.y) = hermite(
                        p1.y,
                        height_slope(segment),
                        p2.y,
                        height_slope(segment + 1),
                        t,
                    );
                }
            }
            (
                position,
                tangent,
                point(segment),
                point(segment + 1),
                t,
                (slopes[segment], slopes[(segment + 1) % n]),
            )
        };
        // Issue #180: where the road steps down (spline parameters, in `[0, n)`).
        let cliffs: Vec<f32> = drops.iter().map(|d| d.at.rem_euclid(n as f32)).collect();

        // Measure: cumulative length at each fine sub-step.
        let fine_count = n * SUBSTEPS;
        let fine_params: Vec<f32> = (0..=fine_count)
            .map(|k| k as f32 / SUBSTEPS as f32)
            .collect();
        let base = measure(&fine_params, &cliffs, |param| curve_at(param).0);
        // Issue #180: the bumps, along the centerline without them: (start, length, height) (m).
        let base_length = base[fine_count];
        let humps: Vec<(f32, f32, f32)> = bumps
            .iter()
            .map(|b| {
                let at = b.at.rem_euclid(n as f32);
                (interpolate(&base, at), b.length, b.height)
            })
            .collect();
        let shaped_at = |param: f32| {
            let mut curve = curve_at(param);
            if !humps.is_empty() {
                let (height, slope) = hump(&humps, base_length, interpolate(&base, param));
                curve.0.y += height;
                curve.1.y += slope * curve.1.length();
            }
            curve
        };
        let fine_lengths = if humps.is_empty() {
            base.clone()
        } else {
            measure(&fine_params, &cliffs, |param| shaped_at(param).0)
        };
        let length = fine_lengths[fine_count];
        let control_distances = (0..n).map(|i| fine_lengths[i * SUBSTEPS]).collect();

        // Resample at a constant distance step that divides the length exactly.
        let count = ((length / step).round() as usize).max(8);
        let step = length / count as f32;
        let mut samples = Vec::with_capacity(count);
        let mut fine = 0;
        for i in 0..count {
            let distance = i as f32 * step;
            while fine + 1 < fine_lengths.len() && fine_lengths[fine + 1] < distance {
                fine += 1;
            }
            let span = (fine_lengths[fine + 1] - fine_lengths[fine]).max(1e-6);
            let along = ((distance - fine_lengths[fine]) / span).clamp(0.0, 1.0);
            let param = fine_params[fine] + (fine_params[fine + 1] - fine_params[fine]) * along;
            let (position, tangent, a, b, t, slopes) = shaped_at(param);

            let forward = tangent.normalize_or(Vec3::NEG_Z);
            let flat_right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
            let flat_up = flat_right.cross(forward);
            let bank = bank_between(a.bank, b.bank, slopes, t).to_radians();
            let (sin, cos) = bank.sin_cos();
            samples.push(TrackSample {
                distance,
                position,
                forward,
                // Positive bank lowers the right edge (for right-hand turns).
                right: flat_right * cos - flat_up * sin,
                up: flat_right * sin + flat_up * cos,
                half_width: blend(a.width, b.width, t) / 2.0,
                shoulder: blend(a.shoulder, b.shoulder, t),
            });
        }

        // Issue #180: the bumps and the steps, as distances along the centerline.
        let bump_spans = humps
            .iter()
            .map(|(start, along, height)| {
                let from = interpolate(&fine_lengths, param_at(&base, *start));
                let end = (start + along).rem_euclid(base_length);
                let to = interpolate(&fine_lengths, param_at(&base, end));
                BumpSpan {
                    from,
                    length: (to - from).rem_euclid(length),
                    height: *height,
                }
            })
            .collect();
        let steps = drops
            .iter()
            .zip(&cliffs)
            .map(|(drop, param)| Cliff {
                at: interpolate(&fine_lengths, *param),
                height: drop.height,
            })
            .collect();

        Self {
            samples,
            length,
            control_distances,
            open: None,
            bumps: bump_spans,
            cliffs: steps,
        }
    }

    /// Issue #180: the bumps, as distances along the centerline.
    pub fn bumps(&self) -> &[BumpSpan] {
        &self.bumps
    }

    /// Issue #180: the drops: where the road steps down, and how far.
    pub fn cliffs(&self) -> &[Cliff] {
        &self.cliffs
    }

    /// Issue #180: the stretches of the road that are trick ramps (GD § 5), as (start, length)
    /// along the centerline: the middle of each bump ([`BUMP_TRICK_SHARE`] in from each end:
    /// around its crest, where a fast kart takes off), the last [`DROP_LIP`] meters before each
    /// drop. None on a circuit without them.
    pub fn trick_stretches(&self) -> Vec<(f32, f32)> {
        self.bumps
            .iter()
            .map(|b| {
                (
                    b.from + BUMP_TRICK_SHARE * b.length,
                    (1.0 - 2.0 * BUMP_TRICK_SHARE) * b.length,
                )
            })
            .chain(self.cliffs.iter().map(|c| (c.at - DROP_LIP, DROP_LIP)))
            .collect()
    }

    /// Issue #180: whether `distance` (wrapped) is on one of the [`trick_stretches`](Self::trick_stretches).
    pub fn on_a_trick_stretch(&self, stretches: &[(f32, f32)], distance: f32) -> bool {
        stretches
            .iter()
            .any(|(start, length)| self.wrap(distance - start) < *length)
    }

    pub fn length(&self) -> f32 {
        self.length
    }

    /// Issue #119: an open circuit's ends; None on a circuit of laps.
    pub fn open(&self) -> Option<&OpenEnds> {
        self.open.as_ref()
    }

    /// Issue #119: whether the centerline distance `distance` (wrapped) is on the road: always on
    /// a circuit of laps; on an open one, from its `road_start` round to its `road_end` (not on
    /// its way back).
    pub fn on_the_road(&self, distance: f32) -> bool {
        self.open.as_ref().is_none_or(|open| {
            self.wrap(distance - open.road_start) <= self.wrap(open.road_end - open.road_start)
        })
    }

    /// Issue #119: how much more scattered decor its road takes than a biome's (made for a lap
    /// of about [`DECOR_LAP`]): its road's length over it on an open circuit (at least 1), 1 on a
    /// circuit of laps (nothing changes there).
    pub fn decor_scale(&self) -> f32 {
        self.open.as_ref().map_or(1.0, |open| {
            let road = self.wrap(open.road_end - open.road_start);
            (road / DECOR_LAP).max(1.0)
        })
    }

    /// Issue #119: the samples on the road ([`CenterLine::on_the_road`]): all of them on a
    /// circuit of laps.
    pub fn road_samples(&self) -> Vec<TrackSample> {
        self.samples
            .iter()
            .filter(|s| self.on_the_road(s.distance))
            .copied()
            .collect()
    }

    pub fn samples(&self) -> &[TrackSample] {
        &self.samples
    }

    /// Wraps any distance into `[0, length)`.
    pub fn wrap(&self, distance: f32) -> f32 {
        distance.rem_euclid(self.length)
    }

    /// Sample at any distance (wrapped), interpolated between the two nearest samples.
    pub fn at(&self, distance: f32) -> TrackSample {
        let distance = self.wrap(distance);
        let step = self.length / self.samples.len() as f32;
        let index = ((distance / step).floor() as usize).min(self.samples.len() - 1);
        let a = self.samples[index];
        let b = self.samples[(index + 1) % self.samples.len()];
        let t = ((distance - a.distance) / step).clamp(0.0, 1.0);
        let mut position = a.position.lerp(b.position, t);
        // Issue #180: never across a step down: the road is level on both sides of it.
        for cliff in &self.cliffs {
            let into = self.wrap(cliff.at - a.distance);
            if into > 0.0 && into <= step {
                position.y = if distance - a.distance < into {
                    a.position.y
                } else {
                    b.position.y
                };
            }
        }
        TrackSample {
            distance,
            position,
            forward: a.forward.lerp(b.forward, t).normalize_or(a.forward),
            right: a.right.lerp(b.right, t).normalize_or(a.right),
            up: a.up.lerp(b.up, t).normalize_or(a.up),
            half_width: a.half_width + (b.half_width - a.half_width) * t,
            shoulder: a.shoulder + (b.shoulder - a.shoulder) * t,
        }
    }

    /// Distance along the track of a spline parameter: `3.5` = halfway between control
    /// points 3 and 4 (the unit used to place things in the track files).
    pub fn param_to_distance(&self, param: f32) -> f32 {
        let n = self.control_distances.len();
        let param = param.rem_euclid(n as f32);
        let i = (param.floor() as usize).min(n - 1);
        let start = self.control_distances[i];
        let end = if i + 1 == n {
            self.length
        } else {
            self.control_distances[i + 1]
        };
        start + (end - start) * (param - i as f32)
    }

    /// Index of the sample closest to `position` (brute force).
    pub fn nearest_index(&self, position: Vec3) -> usize {
        self.samples
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.position
                    .distance_squared(position)
                    .total_cmp(&b.position.distance_squared(position))
            })
            .map_or(0, |(i, _)| i)
    }

    /// Checkpoints every `spacing` meters, the first one on the start line. Issue #119: on an
    /// open circuit, from the start line to the finish line (not on it: like the start line at
    /// the end of a lap, the finish is the checkpoint after the last one), every `spacing` meters
    /// or a little more in each section, the first one of each section where it begins.
    pub fn checkpoints(&self, spacing: f32) -> Vec<Checkpoint> {
        if let Some(open) = &self.open {
            let mut checkpoints = Vec::new();
            for k in 0..open.section_count() {
                let (from, to) = (open.sections[k], open.section_end(k));
                let count = (((to - from) / spacing).floor() as usize).max(1);
                let spacing = (to - from) / count as f32;
                checkpoints.extend((0..count).map(|i| from + i as f32 * spacing));
            }
            return checkpoints
                .into_iter()
                .enumerate()
                .map(|(index, distance)| Checkpoint {
                    index,
                    sample: self.at(distance),
                })
                .collect();
        }
        let count = ((self.length / spacing).floor() as usize).max(2);
        let spacing = self.length / count as f32;
        (0..count)
            .map(|index| Checkpoint {
                index,
                sample: self.at(index as f32 * spacing),
            })
            .collect()
    }

    /// The 12 starting places, staggered behind the start line (GD § 1). Slot 0 is the pole, on
    /// the left of the road.
    pub fn grid(&self, slots: usize) -> Vec<Transform> {
        self.grid_from(slots, -1.0)
    }

    /// The same, the pole on `pole_side` of the road (-1 left, 1 right): on the right on a
    /// mirrored circuit (issue #19), the mirror image of the original grid.
    pub fn grid_from(&self, slots: usize, pole_side: f32) -> Vec<Transform> {
        (0..slots)
            .map(|slot| {
                let sample = self.at(-(GRID_FIRST_ROW + slot as f32 * GRID_SPACING));
                let side = if slot % 2 == 0 { pole_side } else { -pole_side };
                sample.transform(side * sample.half_width * GRID_LATERAL, GRID_HEIGHT)
            })
            .collect()
    }

    /// Turning radius of the centerline at sample `i`, on the horizontal plane (m).
    pub fn turn_radius(&self, i: usize) -> f32 {
        let n = self.samples.len();
        let step = self.length / n as f32;
        let a = self.samples[i % n].forward;
        let b = self.samples[(i + 1) % n].forward;
        let turn = Vec2::new(a.x, a.z)
            .normalize_or_zero()
            .angle_to(Vec2::new(b.x, b.z).normalize_or_zero())
            .abs();
        if turn < 1e-6 { f32::MAX } else { step / turn }
    }
}

/// Issue #119: a biome's scattered decor is counted for a lap of about this long (m): an open
/// circuit, much longer, takes more of it in proportion ([`CenterLine::decor_scale`]), as dense
/// along its road. (Not its far decor's pieces: they stand further, a few are enough.)
pub const DECOR_LAP: f32 = 1400.0;

/// Distance from the start line to the first grid row (m).
const GRID_FIRST_ROW: f32 = 5.0;
/// Distance between two consecutive (staggered) grid places (m).
const GRID_SPACING: f32 = 3.0;
/// Lateral position of the grid places, as a fraction of the half width.
const GRID_LATERAL: f32 = 0.4;
/// Kart body height above the road when spawned (m): wheels on the ground. Also where the tow
/// pelican drops karts (issue #5).
pub const GRID_HEIGHT: f32 = 0.55;

/// Everything derived from a track file that gameplay needs at runtime.
#[derive(Resource, Clone, Debug)]
pub struct TrackLayout {
    pub name: String,
    pub center: CenterLine,
    pub checkpoints: Vec<Checkpoint>,
    pub grid: Vec<Transform>,
    /// Where the tow pelican puts karts back (distances along the track).
    pub respawns: Vec<f32>,
    /// Sand patches on the road, as in the file: the bots' racing line keeps out of them.
    pub offroad: Vec<OffroadZone>,
    /// Issue #19: built from the mirror image of its file (`TrackData::mirrored`).
    pub mirrored: bool,
    /// Issue #20: the stretches with no road (the karts glide over them) and the lakes.
    pub gaps: Vec<Gap>,
    pub lakes: Vec<Lake>,
    /// Issue #78: the anti-gravity zones (the karts drive on the walls there).
    pub anti_gravity: Vec<AntiGravityZone>,
    /// Issue #107: the stretches without wall, and the shortcuts.
    pub open_edges: Vec<OpenEdge>,
    pub shortcuts: Vec<Shortcut>,
    /// Issue #144: the obstacles, the passers-by and the things to drive through
    /// (`super::hazard`), in the order of the file.
    pub hazards: Vec<Hazard>,
    /// Issue #157: the fingerprint of its file and its biome (`TrackData::fingerprint`): the
    /// time trial keeps its records with it.
    pub fingerprint: u64,
}

impl TrackLayout {
    /// Issue #119: an open circuit's ends (a long race in sections); None on a circuit of laps.
    pub fn open(&self) -> Option<&OpenEnds> {
        self.center.open()
    }
}

/// Racers on the grid (GD § 1).
pub const GRID_SLOTS: usize = 12;

/// Issue #144: the pelican drops no kart within this much of an obstacle or a passer-by (m along
/// the track, each way).
const HAZARD_RESPAWN_CLEARANCE: f32 = 9.0;

/// Issue #180: nor this far below a drop (m), where the karts off its edge land.
const DROP_LANDING: f32 = 20.0;

impl TrackLayout {
    pub fn new(data: &TrackData) -> Self {
        let center = CenterLine::of(data);
        let checkpoints = center.checkpoints(data.checkpoint_spacing);
        let respawns = if data.respawns.is_empty() {
            // One per checkpoint, except where a kart must not be dropped: on a ramp, in sand, in
            // a gap (issue #20: no road there) or just before one, in an anti-gravity zone (issue
            // #78: on a wall, maybe), along a stretch without wall (issue #107: the pelican drops
            // a kart that fell from it before it).
            let blocked: Vec<(f32, f32)> =
                data.trick_ramps
                    .iter()
                    .chain(&data.glide_ramps)
                    .map(|r| (r.at, r.length + 3.0))
                    .chain(
                        data.gaps
                            .iter()
                            .map(|g| (g.from, g.to))
                            .chain(data.anti_gravity.iter().map(|z| (z.from, z.to)))
                            .chain(data.open_edges.iter().map(|e| (e.from, e.to)))
                            .map(|(from_param, to_param)| {
                                let from = center.param_to_distance(from_param);
                                let to = center.param_to_distance(to_param);
                                (from_param, center.wrap(to - from) + 3.0)
                            }),
                    )
                    .chain(data.offroad.iter().map(|z| {
                        let from = center.param_to_distance(z.from);
                        let to = center.param_to_distance(z.to);
                        (z.from, center.wrap(to - from) + 3.0)
                    }))
                    .map(|(param, length)| (center.param_to_distance(param) - 3.0, length + 3.0))
                    // Issue #144: nor on an obstacle or a passer-by's way (some meters around it).
                    .chain(
                        data.obstacles
                            .iter()
                            .map(|o| o.at)
                            .chain(data.npcs.iter().map(|n| n.at))
                            .map(|at| {
                                let distance = center.param_to_distance(at);
                                (
                                    distance - HAZARD_RESPAWN_CLEARANCE,
                                    2.0 * HAZARD_RESPAWN_CLEARANCE,
                                )
                            }),
                    )
                    // Issue #180: nor on a bump, nor on the lip of a drop or just below it.
                    .chain(
                        center
                            .bumps()
                            .iter()
                            .map(|b| (b.from - 3.0, b.length + 6.0))
                            .chain(
                                center.cliffs().iter().map(|c| {
                                    (c.at - DROP_LIP - 3.0, DROP_LIP + 3.0 + DROP_LANDING)
                                }),
                            ),
                    )
                    .collect();
            checkpoints
                .iter()
                .map(|c| c.sample.distance)
                .filter(|d| {
                    blocked
                        .iter()
                        .all(|(start, length)| center.wrap(d - start) > *length)
                })
                .collect()
        } else {
            data.respawns
                .iter()
                .map(|p| center.param_to_distance(*p))
                .collect()
        };
        let pole_side = if data.mirrored { 1.0 } else { -1.0 };
        Self {
            name: data.name.clone(),
            fingerprint: data.fingerprint,
            grid: center.grid_from(GRID_SLOTS, pole_side),
            checkpoints,
            respawns,
            offroad: data.offroad.clone(),
            mirrored: data.mirrored,
            gaps: data.gaps.iter().map(|g| Gap::new(&center, g)).collect(),
            lakes: data.lakes.iter().map(Lake::new).collect(),
            anti_gravity: data
                .anti_gravity
                .iter()
                .map(|z| AntiGravityZone::new(&center, z))
                .collect(),
            open_edges: data
                .open_edges
                .iter()
                .map(|e| OpenEdge::new(&center, e))
                .collect(),
            shortcuts: data
                .shortcuts
                .iter()
                .map(|s| Shortcut::new(&center, s))
                .collect(),
            hazards: Hazard::all(&center, data),
            center,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::mesh::{
        CURB_WIDTH, GROUND_HEIGHT, OVERPASS_HEADROOM, WALL_HEIGHT, WALL_THICKNESS, wall_offset,
    };
    use crate::track::terrain::{Bridge, bridge_at};

    /// Issue #108: the test circuit (the layout Prairie Pompon had before #108).
    fn prairie() -> TrackData {
        crate::track::test_circuit()
    }

    /// Issue #78 (GD § 10): the Ruban Réglisse has its wall in its first anti-gravity zone, its
    /// road flat at both ends (where the karts go in and out), a wall in the middle (90°, its
    /// lower edge above the ground); its banking goes beyond 45° only in its zones (issue #94: the
    /// second one is the ceiling, `the_ribbon_has_its_ceiling_in_its_second_zone`); no pelican
    /// drops a kart in it (nor 3 m before it); the items find the wall's normal over it, and the
    /// world's up elsewhere. Its mirror image too (the banking the other way, the zone at the same
    /// place along the track).
    #[test]
    fn the_ribbon_has_its_wall_in_its_zone() {
        let data = every_circuit()
            .into_iter()
            .find(|d| d.name == "Ruban Réglisse")
            .expect("the circuit of issue #78");
        for data in [data.clone(), data.mirrored()] {
            let layout = TrackLayout::new(&data);
            let center = &layout.center;
            assert_eq!(
                layout.anti_gravity.len(),
                3,
                "the wall, the ceiling and the banked turn"
            );
            let zone = layout.anti_gravity[0];
            let flat = |s: &TrackSample| s.up.y > 0.97;
            assert!(flat(&center.at(zone.from)) && flat(&center.at(zone.to)));
            let length = zone.length(center);
            assert!((150.0..300.0).contains(&length), "{length}");
            let mut on_the_wall = 0;
            for s in center.samples() {
                let zoned = zone.contains(center, s.distance);
                let in_a_zone = layout
                    .anti_gravity
                    .iter()
                    .any(|z| z.contains(center, s.distance));
                // A right-handed road frame: right × forward is up.
                let frame = s.up.angle_between(s.right.cross(s.forward)).to_degrees();
                assert!(frame < 1.0, "{:.0} m: not a road frame", s.distance);
                let tilt = s.up.y.clamp(-1.0, 1.0).acos().to_degrees();
                if !in_a_zone {
                    assert!(tilt <= 46.0, "{:.0} m: {tilt}° out of the zone", s.distance);
                }
                if zoned {
                    assert!(s.up.y > -0.02, "{:.0} m: the wall, 90° at most", s.distance);
                }
                if s.up.y.abs() < 0.02 {
                    assert!(in_a_zone);
                    on_the_wall += u32::from(zoned);
                    // The wall's lower edge, its outer face, above the ground.
                    let low = [-1.0f32, 1.0]
                        .map(|side| s.point(side * (wall_offset(s) + WALL_THICKNESS), 0.0).y)
                        .into_iter()
                        .fold(f32::MAX, f32::min);
                    assert!(low > GROUND_HEIGHT + 0.2, "{low}");
                }
            }
            assert!(on_the_wall >= 25, "{on_the_wall} m of wall");
            for drop in &layout.respawns {
                for zone in &layout.anti_gravity {
                    assert!(
                        center.wrap(*drop - zone.from + 3.0) > zone.length(center) + 3.0,
                        "a drop point at {drop} in the zone {zone:?}"
                    );
                }
            }
            let course = crate::race::Course::new(&layout, &crate::race::RaceTuning::default());
            let wall = center.at(zone.from + length / 2.0);
            assert!(course.up_at(wall.point(2.0, 0.4)).dot(wall.up) > 0.999);
            assert_eq!(
                course.up_at(center.at(zone.from - 30.0).point(0.0, 0.4)),
                Vec3::Y
            );
        }
    }

    /// Issue #94 (GD § 10): the Ruban Réglisse's second anti-gravity zone rolls its road over onto
    /// its back: flat at both ends, past the wall (90°, its lower edge above the ground) to upside
    /// down (180°) for 25 m at least, the karts' room under it (their center 0.55 m under the
    /// road, 5 m above the ground at least), the road upside down only in that zone. The zone runs
    /// straight (seen from above); the items find the ceiling's normal (down) under it. Its mirror
    /// image too.
    #[test]
    fn the_ribbon_has_its_ceiling_in_its_second_zone() {
        let data = every_circuit()
            .into_iter()
            .find(|d| d.name == "Ruban Réglisse")
            .expect("the circuit of issue #78");
        for data in [data.clone(), data.mirrored()] {
            let layout = TrackLayout::new(&data);
            let center = &layout.center;
            let zone = layout.anti_gravity[1];
            let flat = |s: &TrackSample| s.up.y > 0.97;
            assert!(flat(&center.at(zone.from)) && flat(&center.at(zone.to)));
            let length = zone.length(center);
            assert!((300.0..420.0).contains(&length), "{length}");
            assert!(
                crate::track::data::anti_gravity_turn(center, &data.anti_gravity[1]) < 1.0,
                "straight"
            );
            let mut upside_down = 0;
            for s in center.samples() {
                let zoned = zone.contains(center, s.distance);
                // (The wall, 90°, faces the side: its normal's height is a rounding.)
                if s.up.y < -0.02 {
                    assert!(zoned, "{:.0} m: upside down out of the ceiling", s.distance);
                }
                if !zoned {
                    continue;
                }
                // The ribbon's edges (their walls' outer faces) and its karts above the ground.
                let low = [-1.0f32, 1.0]
                    .map(|side| s.point(side * (wall_offset(s) + WALL_THICKNESS), 0.0).y)
                    .into_iter()
                    .fold(f32::MAX, f32::min);
                assert!(low > GROUND_HEIGHT + 0.2, "{:.0} m: {low}", s.distance);
                if s.up.y < -0.999 {
                    upside_down += 1;
                    let kart = s.point(0.0, GRID_HEIGHT);
                    assert!(kart.y - GROUND_HEIGHT > 5.0, "{kart}");
                }
            }
            assert!(upside_down >= 25, "{upside_down} m upside down");
            let course = crate::race::Course::new(&layout, &crate::race::RaceTuning::default());
            let ceiling = center.at(zone.from + length / 2.0);
            assert!(ceiling.up.y < -0.999, "{ceiling:?}");
            assert!(course.up_at(ceiling.point(-2.0, 0.4)).dot(Vec3::NEG_Y) > 0.999);
            let under = course.project_anywhere(ceiling.point(1.0, GRID_HEIGHT));
            assert!(under.upside_down && (under.height - GRID_HEIGHT).abs() < 0.05);
        }
    }

    /// Issue #96 (GD § 10): the Ruban Réglisse's third anti-gravity zone is a turn banked like a
    /// wall: flat at both ends, it turns left by about 90° seen from above (a radius of 60 m: its
    /// ends still on the curves of the turns to the right before and after it), banked 80° and
    /// more around it for 40 m at least, 90° at its middle (its lower edge above the ground: the
    /// wall's test), its road's normal toward the inside of the turn wherever it curves (the road
    /// hardly ever curves away from the karts on it: they are pressed onto it); the items find
    /// its normal over it. Its mirror image too (a right turn).
    #[test]
    fn the_ribbon_has_its_banked_turn_in_its_third_zone() {
        let data = every_circuit()
            .into_iter()
            .find(|d| d.name == "Ruban Réglisse")
            .expect("the circuit of issue #78");
        for data in [data.clone(), data.mirrored()] {
            let layout = TrackLayout::new(&data);
            let center = &layout.center;
            let zone = layout.anti_gravity[2];
            let flat = |s: &TrackSample| s.up.y > 0.97;
            assert!(flat(&center.at(zone.from)) && flat(&center.at(zone.to)));
            let length = zone.length(center);
            assert!((150.0..250.0).contains(&length), "{length}");
            // 90° to the left, and the ends of the turns before and after it (to the right).
            let turn = crate::track::data::anti_gravity_turn(center, &data.anti_gravity[2]);
            assert!((90.0..140.0).contains(&turn), "{turn}°");
            let (into, out) = (center.at(zone.from).forward, center.at(zone.to).forward);
            let heading = into.xz().angle_to(out.xz()).to_degrees();
            let left = if data.mirrored { 1.0 } else { -1.0 };
            assert!((heading * left - 90.0).abs() < 25.0, "{heading}°");
            let (mut banked, mut tightest) = (0, f32::MAX);
            let n = center.samples().len();
            for (i, s) in center.samples().iter().enumerate() {
                if !zone.contains(center, s.distance) {
                    continue;
                }
                // Its curvature (3D): toward the road, or away from it only as a crest of 200 m
                // at least (at 50 m/s, half a kart's gravity: its ends on the tails of the turns
                // to the right, the bank only just setting in).
                let (before, after) = (
                    center.samples()[(i + n - 1) % n],
                    center.samples()[(i + 1) % n],
                );
                let curvature =
                    (after.forward - before.forward) / after.position.distance(before.position);
                assert!(
                    curvature.dot(s.up) > -1.0 / 200.0,
                    "{:.0} m: {curvature}",
                    s.distance
                );
                // Banked 80° and more.
                if s.up.y.abs() < 10f32.to_radians().sin() {
                    banked += 1;
                    tightest = tightest.min(center.turn_radius(i));
                }
            }
            assert!(banked >= 40, "{banked} m banked 80° and more");
            assert!((50.0..70.0).contains(&tightest), "radius {tightest} m");
            let course = crate::race::Course::new(&layout, &crate::race::RaceTuning::default());
            let apex = center.at(zone.from + length / 2.0);
            assert!(apex.up.y.abs() < 0.02, "{apex:?}");
            assert!(course.up_at(apex.point(2.0, 0.4)).dot(apex.up) > 0.999);
            // Its road bends up toward its normal round the turn (the karts' guide, issue #96);
            // the wall and the ceiling, level straights, do not bend, however they twist: nothing
            // changes there (but at their ends, where the road finishes climbing onto them or
            // starts coming down: a crest).
            let bend = crate::track::bend_at(&layout.anti_gravity, center, apex.point(2.0, 0.5))
                .expect("over the banked turn");
            assert!((bend.rate - 1.0 / 60.0).abs() < 0.003, "{bend:?}");
            // (Between the points whose spline segments only see level points.)
            let level = [(7.0, 14.0), (33.0, 46.0)];
            for (straight, (from, to)) in layout.anti_gravity[..2].iter().zip(level) {
                let (from, to) = (center.param_to_distance(from), center.param_to_distance(to));
                for k in 0..=50 {
                    let s = center.at(from + (to - from) * k as f32 / 50.0);
                    let bend = straight
                        .bend_near(center, s.point(0.0, 0.5))
                        .expect("over it");
                    assert!(bend.rate.abs() < 1e-3, "{:.0} m: {bend:?}", s.distance);
                }
            }
        }
    }

    /// Issue #20 (GD § 10): the Lagon Duvet has a glide ramp at the start of its gap (the lip
    /// 10 m above the landing), its gap is over its lake, its road dips under the water of the
    /// lake (deep enough for the camera too) and only there; no kart is dropped by the pelican in
    /// the gap or on the ramp. Mirrored, the lake follows.
    #[test]
    fn the_lagoon_has_its_glide_ramp_its_gap_and_its_water() {
        let data = every_circuit()
            .into_iter()
            .find(|d| d.name == "Lagon Duvet")
            .expect("the circuit of issue #20");
        for data in [data.clone(), data.mirrored()] {
            let layout = TrackLayout::new(&data);
            let center = &layout.center;
            assert_eq!(layout.gaps.len(), 1);
            assert_eq!(layout.lakes.len(), 1);
            let (gap, lake) = (layout.gaps[0], layout.lakes[0]);
            let ramp = data.glide_ramps[0];
            let top =
                center.param_to_distance(ramp.at) + ramp.length * ramp.angle.to_radians().cos();
            assert!(
                (top - gap.from).abs() < 0.2,
                "the ramp's top {top} at the gap {gap:?}"
            );
            assert!(
                ramp.width >= 2.0 * wall_offset(&center.at(top)) - 0.01,
                "wall to wall"
            );
            let lip = center.at(gap.from).position.y;
            let landing = center.at(gap.to).position.y;
            assert!(
                lip - landing > 9.0 && gap.low == landing.min(lip),
                "{lip} {landing}"
            );
            let length = gap.length(center);
            assert!((50.0..70.0).contains(&length), "{length}");
            // Under the flight, the lagoon.
            for k in 1..10 {
                let p = center.at(gap.from + length * k as f32 / 10.0).position;
                assert!(lake.covers(p.xz()), "{p}");
            }
            // The road under the water: the kart's center more than 3 m under the surface
            // (the chase camera, 2.4 m above it, under the water too).
            let deepest = center
                .samples()
                .iter()
                .filter(|s| gap_at_distance(&layout, s.distance).is_none())
                .map(|s| s.position.y)
                .fold(f32::MAX, f32::min);
            assert!(lake.level - (deepest + GRID_HEIGHT) > 3.0, "{deepest}");
            for d in &layout.respawns {
                assert!(
                    gap_at_distance(&layout, *d).is_none(),
                    "a respawn in the gap"
                );
                let from = center.param_to_distance(ramp.at) - 3.0;
                assert!(center.wrap(*d - from) > ramp.length + 3.0, "on the ramp");
            }
            assert_eq!(lake.center().x < 0.0, data.mirrored, "the lake mirrored");
        }
    }

    fn gap_at_distance(layout: &TrackLayout, distance: f32) -> Option<Gap> {
        crate::track::gap_at(&layout.gaps, &layout.center, distance).copied()
    }

    #[test]
    fn the_test_circuit_file_parses_with_all_its_features() {
        let data = prairie();
        assert_eq!(data.name, "Circuit d'essai");
        assert!(data.points.len() >= 20);
        assert!(!data.offroad.is_empty());
        assert!(!data.trick_ramps.is_empty());
        assert!(!data.boost_pads.is_empty());
        assert!(data.item_boxes.iter().any(|row| row.double));
        assert!(data.item_boxes.iter().any(|row| !row.double));
        assert!(!data.croquettes.is_empty());
        // Defaults fill what the file leaves out.
        assert_eq!(data.points[0].width, 14.0);
        assert_eq!(data.points[0].bank, 0.0);
    }

    #[test]
    fn centerline_is_closed_and_evenly_sampled() {
        let data = prairie();
        let center = CenterLine::of(&data);
        let samples = center.samples();
        let step = center.length() / samples.len() as f32;
        assert!((step - data.sample_step).abs() < 0.1, "{step}");
        for i in 0..samples.len() {
            let gap = samples[i]
                .position
                .distance(samples[(i + 1) % samples.len()].position);
            assert!((gap - step).abs() < 0.15 * step, "gap {gap} at sample {i}");
        }
        // The loop closes on the start line, which is control point 0.
        assert!(samples[0].position.distance(data.points[0].position()) < 1e-3);
        assert!(
            (500.0..700.0).contains(&center.length()),
            "{}",
            center.length()
        );
    }

    #[test]
    fn checkpoints_are_in_order_along_the_track() {
        let data = prairie();
        let layout = TrackLayout::new(&data);
        let checkpoints = &layout.checkpoints;
        let spacing = layout.center.length() / checkpoints.len() as f32;
        assert!(checkpoints.len() >= 20);
        assert_eq!(
            checkpoints[0].sample.distance, 0.0,
            "the first one is the start line"
        );
        for (i, pair) in checkpoints.windows(2).enumerate() {
            let (a, b) = (&pair[0], &pair[1]);
            assert_eq!(a.index, i);
            assert!(b.sample.distance > a.sample.distance);
            // The next checkpoint is ahead, about one spacing away.
            let to_next = b.sample.position - a.sample.position;
            assert!(
                to_next.dot(a.sample.forward) > 0.5 * spacing,
                "checkpoint {i}"
            );
            assert!(to_next.length() < 1.05 * spacing, "checkpoint {i}");
        }
        // ...and the last one leads back to the start line.
        let last = checkpoints.last().unwrap();
        assert!(
            (checkpoints[0].sample.position - last.sample.position).dot(last.sample.forward) > 0.0
        );
    }

    #[test]
    fn grid_has_12_staggered_places_behind_the_start_line() {
        let data = prairie();
        let layout = TrackLayout::new(&data);
        assert_eq!(layout.grid.len(), GRID_SLOTS);
        let start = layout.center.at(0.0);
        for (slot, place) in layout.grid.iter().enumerate() {
            let from_line = place.translation - start.position;
            assert!(
                from_line.dot(start.forward) < -4.0,
                "slot {slot} is behind the line"
            );
            let side = from_line.dot(start.right);
            assert!(
                side.abs() < start.half_width - 1.0,
                "slot {slot} is on the road"
            );
            assert_eq!(side < 0.0, slot % 2 == 0, "slot {slot} alternates sides");
            assert!(
                place.forward().dot(start.forward) > 0.99,
                "slot {slot} faces the track"
            );
        }
        for pair in layout.grid.windows(2) {
            assert!(
                pair[0].translation.distance(pair[1].translation) > 2.5,
                "no overlap"
            );
        }
    }

    #[test]
    fn default_respawns_avoid_ramps_and_sand() {
        let data = prairie();
        let layout = TrackLayout::new(&data);
        let center = &layout.center;
        assert!(layout.respawns.len() >= layout.checkpoints.len() - 3);
        for respawn in &layout.respawns {
            for ramp in &data.trick_ramps {
                let start = center.param_to_distance(ramp.at);
                let into = center.wrap(respawn - start);
                assert!(
                    into > ramp.length + 1.0,
                    "respawn {respawn} on the ramp at {start}"
                );
            }
            for zone in &data.offroad {
                let (from, to) = (
                    center.param_to_distance(zone.from),
                    center.param_to_distance(zone.to),
                );
                assert!(
                    !(from..=to).contains(respawn),
                    "respawn {respawn} in sand {from}..{to}"
                );
            }
        }
    }

    #[test]
    fn params_map_to_increasing_distances() {
        let data = prairie();
        let center = CenterLine::of(&data);
        assert_eq!(center.param_to_distance(0.0), 0.0);
        let mut previous = -1.0;
        for k in 0..(data.points.len() * 4) {
            let d = center.param_to_distance(k as f32 / 4.0);
            assert!(d > previous);
            previous = d;
        }
        // Control point 6 is the hairpin apex.
        let apex = center.at(center.param_to_distance(6.0)).position;
        assert!(apex.distance(data.points[6].position()) < 0.5);
    }

    /// Every circuit of the game (`assets/tracks/index.ron`), as written.
    fn every_circuit() -> Vec<TrackData> {
        crate::admin::tracks::circuits()
            .iter()
            .map(|entry| {
                let text = std::fs::read_to_string(format!(
                    "{}/assets/{}",
                    env!("CARGO_MANIFEST_DIR"),
                    entry.asset_path()
                ))
                .unwrap();
                let data = TrackData::from_ron(&text).unwrap();
                data.validate().unwrap();
                data
            })
            .collect()
    }

    /// Issue #19: on every circuit of the game and its mirror image, the walls never fold (no
    /// turn tighter than the inner wall's offset), the road and the walls stay above the ground,
    /// and no two parts of the track far apart along it (60 m and more) come so close that their
    /// walls would touch: a meter at least between their outer faces. Issue #20: in a lake, above
    /// its floor (and under its water where the road dips); a gap has neither road nor walls.
    /// Issue #119: on an open circuit, its road only (its way back is never built). Issue #219
    /// (GD § 16, rule 3): **unless one passes over the other**, on a bridge (its pillars, not
    /// skirts, under it), [`OVERPASS_HEADROOM`] meters above the top of the other's walls at
    /// least; one circuit does at least (the Coteau Citrouille's loop).
    #[test]
    fn every_circuit_and_its_mirror_keep_their_parts_apart() {
        let circuits = every_circuit();
        assert!(circuits.len() >= 5, "issue #20: at least 5 circuits");
        let mut overpasses = 0;
        for data in circuits.iter().flat_map(|d| [d.clone(), d.mirrored()]) {
            let name = format!(
                "{}{}",
                data.name,
                if data.mirrored { " (miroir)" } else { "" }
            );
            let layout = TrackLayout::new(&data);
            let center = CenterLine::of(&data);
            let samples = center.samples();
            for (i, s) in samples.iter().enumerate() {
                // Issue #119: an open circuit's way back is never built.
                if !center.on_the_road(s.distance) {
                    continue;
                }
                let radius = center.turn_radius(i);
                let wall = wall_offset(s) + WALL_THICKNESS;
                assert!(
                    radius > wall,
                    "{name}: radius {radius} < wall {wall} at {}",
                    s.distance
                );
                if crate::track::gap_at(&layout.gaps, &center, s.distance).is_some() {
                    continue;
                }
                for side in [-1.0, 1.0] {
                    let base = s.point(side * wall, 0.0);
                    let ground = crate::track::water::ground_height(&layout.lakes, base.xz());
                    assert!(
                        base.y > ground,
                        "{name}: under the ground at {}",
                        s.distance
                    );
                    if base.y < GROUND_HEIGHT {
                        assert!(
                            layout.lakes.iter().any(|lake| lake.covers(base.xz())),
                            "{name}: under the ground out of a lake at {}",
                            s.distance
                        );
                    }
                }
            }
            // Issue #219 (GD § 16, rule 3): a part of the track on a bridge may pass over another
            // one, [`OVERPASS_HEADROOM`] above it at least.
            let bridges: Vec<Bridge> = data
                .bridges
                .iter()
                .map(|b| Bridge::new(&center, b))
                .collect();
            let length = center.length();
            for (i, a) in samples.iter().enumerate().step_by(2) {
                if !center.on_the_road(a.distance) {
                    continue;
                }
                for b in samples[i + 1..]
                    .iter()
                    .filter(|b| center.on_the_road(b.distance))
                {
                    let along = (b.distance - a.distance).min(length - (b.distance - a.distance));
                    if along < 60.0 {
                        continue;
                    }
                    let apart = a.position.xz().distance(b.position.xz());
                    let needed = wall_offset(a) + wall_offset(b) + 2.0 * WALL_THICKNESS + 1.0;
                    if apart <= needed && passes_over(&bridges, &center, a, b) {
                        overpasses += 1;
                        continue;
                    }
                    assert!(
                        apart > needed,
                        "{name}: the track at {:.0} m and at {:.0} m is {apart:.1} m apart \
                         (walls need {needed:.1} m), and neither is on a bridge \
                         {OVERPASS_HEADROOM} m above the other",
                        a.distance,
                        b.distance
                    );
                }
            }
        }
        // Issue #219: the first circuit that passes over itself (the Coteau Citrouille's loop),
        // and its mirror.
        assert!(overpasses > 0, "no part of any circuit passes over another");
    }

    /// Issue #219 (GD § 16, rule 3: a change of level never hides the road): on every circuit
    /// that passes over itself and its mirror, the chase camera (6 m behind and 2.4 m above the
    /// kart, the kart in the middle of the road or near either edge) on the road under a bridge
    /// sees the road 3 to 45 m ahead (in its middle and near its edges) under the bridge: no
    /// sight line meets its deck, from its underside to the top of its walls.
    #[test]
    fn the_chase_camera_sees_the_road_under_a_bridge() {
        use crate::track::mesh::BRIDGE_DECK;
        use crate::track::terrain::{CHASE_DISTANCE, CHASE_HEIGHT};
        let mut checked = 0;
        for data in every_circuit()
            .iter()
            .flat_map(|d| [d.clone(), d.mirrored()])
        {
            let center = CenterLine::of(&data);
            let bridges: Vec<Bridge> = data
                .bridges
                .iter()
                .map(|b| Bridge::new(&center, b))
                .collect();
            let decks: Vec<&TrackSample> = center
                .samples()
                .iter()
                .filter(|s| bridge_at(&bridges, &center, s.distance).is_some())
                .collect();
            let length = center.length();
            let apart = |a: f32, b: f32| {
                let along = center.wrap(a - b);
                along.min(length - along)
            };
            // The road that passes under a bridge, and 60 m before it.
            let under: Vec<&TrackSample> = center
                .road_samples()
                .iter()
                .filter(|s| {
                    decks.iter().any(|d| {
                        apart(d.distance, s.distance) >= 60.0
                            && d.position.xz().distance(s.position.xz()) < 60.0
                    })
                })
                .map(|s| &center.samples()[center.nearest_index(s.position)])
                .collect();
            if under.is_empty() {
                continue;
            }
            checked += 1;
            for s in under {
                let flat = Vec3::new(s.forward.x, 0.0, s.forward.z).normalize_or_zero();
                for kart in [-0.6, 0.0, 0.6] {
                    let eye = s.point(kart * s.half_width, 0.55) - flat * CHASE_DISTANCE
                        + Vec3::Y * CHASE_HEIGHT;
                    for ahead in (1..=15).map(|k| 3.0 * k as f32) {
                        let t = center.at(s.distance + ahead);
                        for across in [-0.9, 0.0, 0.9] {
                            let target = t.point(across * t.half_width, 0.0);
                            let steps = (eye.distance(target).ceil() as usize).max(1);
                            for k in 0..=steps {
                                let p = eye.lerp(target, k as f32 / steps as f32);
                                let hit = decks.iter().any(|d| {
                                    apart(d.distance, s.distance) >= 60.0
                                        && p.xz().distance(d.position.xz())
                                            < wall_offset(d) + WALL_THICKNESS
                                        && p.y > d.position.y - BRIDGE_DECK - 0.5
                                        && p.y < d.position.y + WALL_HEIGHT + 0.5
                                });
                                assert!(
                                    !hit,
                                    "{}: from {:.0} m, the road {ahead} m ahead is hidden by the \
                                     bridge at {p}",
                                    data.name, s.distance
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(checked >= 2, "the Coteau Citrouille and its mirror");
    }

    /// Issue #219: whether one of two samples is on a bridge and passes over the other,
    /// [`OVERPASS_HEADROOM`] meters above it at least (road to road, every point across both).
    fn passes_over(
        bridges: &[Bridge],
        center: &CenterLine,
        a: &TrackSample,
        b: &TrackSample,
    ) -> bool {
        let over = |upper: &TrackSample, lower: &TrackSample| {
            bridge_at(bridges, center, upper.distance).is_some() && {
                let high = upper
                    .point(-wall_offset(upper), 0.0)
                    .y
                    .min(upper.point(wall_offset(upper), 0.0).y)
                    .min(upper.position.y);
                let low = lower
                    .point(-wall_offset(lower), WALL_HEIGHT)
                    .y
                    .max(lower.point(wall_offset(lower), WALL_HEIGHT).y)
                    .max(lower.position.y);
                high - low >= OVERPASS_HEADROOM
            }
        };
        over(a, b) || over(b, a)
    }

    /// Issue #19 (GD § 1, the mirror mode): the mirror image of every circuit is valid and is
    /// exactly the mirror image of the original across the X = 0 plane: its centerline (the same
    /// length, every sample mirrored, its right the mirror of the original's left, its banking
    /// the other way), its checkpoints, its grid (the pole on the other side), every spot of the
    /// file across the road (`offset` → `-offset`), its sand patches. Mirrored twice, the file
    /// comes back.
    #[test]
    fn a_mirrored_circuit_is_the_mirror_image_of_the_original() {
        let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        let close = |a: Vec3, b: Vec3| a.distance(b) < 2e-3;
        for data in every_circuit() {
            let mirrored = data.mirrored();
            assert!(mirrored.mirrored && !data.mirrored);
            assert_eq!(mirrored.validate(), Ok(()), "{}", data.name);
            assert_eq!(mirrored.mirrored(), data, "{}: mirrored twice", data.name);
            let (a, b) = (TrackLayout::new(&data), TrackLayout::new(&mirrored));
            assert!(b.mirrored && !a.mirrored);
            assert_eq!(a.name, b.name);
            assert!((a.center.length() - b.center.length()).abs() < 1e-3);
            assert_eq!(a.center.samples().len(), b.center.samples().len());
            for (s, m) in a.center.samples().iter().zip(b.center.samples()) {
                assert!(
                    close(m.position, flip(s.position)),
                    "{}: {s:?} / {m:?}",
                    data.name
                );
                assert!(close(m.forward, flip(s.forward)), "{}", data.name);
                assert!(close(m.right, -flip(s.right)), "{}", data.name);
                assert!(close(m.up, flip(s.up)), "{}", data.name);
                assert_eq!(m.half_width, s.half_width);
            }
            assert_eq!(a.checkpoints.len(), b.checkpoints.len());
            for (c, m) in a.checkpoints.iter().zip(&b.checkpoints) {
                assert!(close(m.sample.position, flip(c.sample.position)));
            }
            assert_eq!(a.respawns, b.respawns);
            // The grid: the mirror image of every slot, the pole on the other side.
            for (slot, (g, m)) in a.grid.iter().zip(&b.grid).enumerate() {
                assert!(
                    close(m.translation, flip(g.translation)),
                    "{}: slot {slot}",
                    data.name
                );
                assert!(close(*m.forward(), flip(*g.forward())));
            }
            let start = a.center.at(0.0);
            let side = |t: &Transform, s: &TrackSample| (t.translation - s.position).dot(s.right);
            assert!(side(&a.grid[0], &start) < 0.0, "the pole on the left");
            assert!(
                side(&b.grid[0], &b.center.at(0.0)) > 0.0,
                "on the right, mirrored"
            );
            // The spots across the road, at the same distance along it.
            let spot = |layout: &TrackLayout, at: f32, offset: f32| {
                let s = layout.center.at(layout.center.param_to_distance(at));
                s.point(offset * s.half_width, 0.0)
            };
            for (p, q) in data.boost_pads.iter().zip(&mirrored.boost_pads) {
                assert!(close(
                    spot(&b, q.at, q.offset),
                    flip(spot(&a, p.at, p.offset))
                ));
            }
            for (p, q) in data.trick_ramps.iter().zip(&mirrored.trick_ramps) {
                assert!(close(
                    spot(&b, q.at, q.offset),
                    flip(spot(&a, p.at, p.offset))
                ));
            }
            for (p, q) in data.item_boxes.iter().zip(&mirrored.item_boxes) {
                for (x, y) in p.offsets.iter().zip(&q.offsets) {
                    assert!(close(spot(&b, q.at, *y), flip(spot(&a, p.at, *x))));
                }
            }
            for (p, q) in data.croquettes.iter().zip(&mirrored.croquettes) {
                assert!(close(
                    spot(&b, q.from, q.offset),
                    flip(spot(&a, p.from, p.offset))
                ));
            }
            for (p, q) in data.offroad.iter().zip(&b.offroad) {
                assert_eq!((q.from, q.to), (p.from, p.to));
                assert!(close(
                    spot(&b, q.from, q.left),
                    flip(spot(&a, p.from, p.right))
                ));
                assert!(close(
                    spot(&b, q.from, q.right),
                    flip(spot(&a, p.from, p.left))
                ));
            }
        }
    }

    #[test]
    fn walls_never_fold_and_everything_stays_above_the_ground() {
        let data = prairie();
        let center = CenterLine::of(&data);
        // The inner wall folds over itself where a turn is tighter than its offset.
        for (i, s) in center.samples().iter().enumerate() {
            let radius = center.turn_radius(i);
            let wall = wall_offset(s) + WALL_THICKNESS;
            assert!(
                radius > wall,
                "radius {radius} < wall offset {wall} at {}",
                s.distance
            );
        }
        for s in center.samples() {
            for side in [-1.0, 1.0] {
                let wall_base = s.point(side * (wall_offset(s) + WALL_THICKNESS), 0.0);
                assert!(
                    wall_base.y > GROUND_HEIGHT,
                    "wall under the ground at {}",
                    s.distance
                );
                let curb = s.point(side * (s.half_width + CURB_WIDTH), 0.0);
                assert!(
                    curb.y > GROUND_HEIGHT,
                    "road under the ground at {}",
                    s.distance
                );
            }
        }
    }

    /// Issue #180: Prairie Pompon, its demonstration circuit (its bumps and its drop).
    fn prairie_pompon() -> TrackData {
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/prairie.track.ron")).unwrap();
        data.validate().unwrap();
        data
    }

    /// Issue #180: each bump of Prairie Pompon (and its mirror) rises and falls back on its
    /// stretch only: its crest, in its middle, as high above the road without it as it is
    /// declared; nothing changed beyond it; no sample-to-sample jump steeper than its slope; the
    /// road's frame tilted with it (its forward up its slope). Its middle half is a trick stretch.
    #[test]
    fn a_bump_rises_and_falls_back_on_its_stretch() {
        for data in [prairie_pompon(), prairie_pompon().mirrored()] {
            let center = CenterLine::of(&data);
            let flat = CenterLine::shaped(
                &data.points,
                data.sample_step,
                &data.anti_gravity,
                &[],
                &data.drops,
            );
            assert_eq!(center.bumps().len(), data.bumps.len());
            let step = center.length() / center.samples().len() as f32;
            let stretches = center.trick_stretches();
            for (bump, declared) in center.bumps().iter().zip(&data.bumps) {
                assert!((bump.length - declared.length).abs() < 1.0, "{bump:?}");
                assert_eq!(bump.height, declared.height);
                // Its start where the file puts it (the bump adds a little length on the way).
                let start = center.param_to_distance(declared.at);
                assert!((bump.from - start).abs() < 0.5, "{bump:?} {start}");
                // Compared with the road without it, the same distance from its start.
                let rise = |along: f32| {
                    center.at(bump.from + along).position.y
                        - flat
                            .at(flat.param_to_distance(declared.at) + along)
                            .position
                            .y
                };
                let crest = rise(bump.length / 2.0);
                assert!((crest - bump.height).abs() < 0.05, "{crest} {bump:?}");
                // (On a slope, the length the bump adds shifts the two a little.)
                for along in [-5.0, -1.0, bump.length + 1.0, bump.length + 5.0] {
                    assert!(rise(along).abs() < 0.05, "{along}: {}", rise(along));
                }
                let slope = std::f32::consts::PI * bump.height / bump.length;
                let mut steepest: f32 = 0.0;
                for k in 0..(bump.length / step) as usize {
                    let (a, b) = (rise(k as f32 * step), rise((k + 1) as f32 * step));
                    steepest = steepest.max((b - a).abs() / step);
                }
                assert!(steepest <= slope + 0.02, "{steepest} > {slope}");
                // Up its slope at a quarter, down it at three quarters (over the road's own).
                let tilt = |along: f32| {
                    center.at(bump.from + along).forward.y
                        - flat
                            .at(flat.param_to_distance(declared.at) + along)
                            .forward
                            .y
                };
                let (up, down) = (tilt(bump.length / 4.0), tilt(0.75 * bump.length));
                assert!(up > 0.8 * slope && down < -0.8 * slope, "{up} {down}");
                for (along, tricky) in [(0.2, false), (0.3, true), (0.7, true), (0.8, false)] {
                    let distance = bump.from + along * bump.length;
                    assert_eq!(center.on_a_trick_stretch(&stretches, distance), tricky);
                }
            }
        }
    }

    /// Issue #180: the drop of Prairie Pompon (and its mirror) is a clean step down of its
    /// height: the road level on both sides of it (`at` never blends across it, the samples
    /// around it flat), no length added by the step (the samples as far apart across it as
    /// anywhere), its last 8 m above a trick stretch; no pelican's drop point on it nor just
    /// below it.
    #[test]
    fn a_drop_is_a_clean_step_down() {
        for data in [prairie_pompon(), prairie_pompon().mirrored()] {
            let layout = TrackLayout::new(&data);
            let center = &layout.center;
            assert_eq!(center.cliffs().len(), 1);
            let cliff = center.cliffs()[0];
            assert_eq!(cliff.height, data.drops[0].height);
            assert!((cliff.at - center.param_to_distance(data.drops[0].at)).abs() < 0.5);
            let (above, below) = (center.at(cliff.at - 0.02), center.at(cliff.at + 0.02));
            assert!(
                (above.position.y - below.position.y - cliff.height).abs() < 0.01,
                "{above:?} {below:?}"
            );
            for along in [-6.0, -3.0, -0.5, 0.5, 3.0, 6.0] {
                let s = center.at(cliff.at + along);
                assert!(s.forward.y.abs() < 0.02, "{along}: {s:?}");
                let level = if along < 0.0 { above } else { below };
                assert!((s.position.y - level.position.y).abs() < 0.02, "{along}");
            }
            let step = center.length() / center.samples().len() as f32;
            let i = (cliff.at / step) as usize;
            for k in i - 3..i + 3 {
                let (a, b) = (center.samples()[k], center.samples()[k + 1]);
                let across = (b.position - a.position).xz().length();
                assert!((across - step).abs() < 0.05, "{k}: {across} for {step}");
            }
            let stretches = center.trick_stretches();
            assert!(center.on_a_trick_stretch(&stretches, cliff.at - 0.5));
            assert!(center.on_a_trick_stretch(&stretches, cliff.at - DROP_LIP + 0.5));
            assert!(!center.on_a_trick_stretch(&stretches, cliff.at + 0.5));
            assert!(!center.on_a_trick_stretch(&stretches, cliff.at - DROP_LIP - 0.5));
            for r in &layout.respawns {
                let behind = center.wrap(cliff.at - r);
                assert!(
                    behind > DROP_LIP + 3.0 && behind < center.length() - 20.0,
                    "{r}"
                );
                for bump in center.bumps() {
                    assert!(center.wrap(r - bump.from) > bump.length + 3.0, "{r}");
                }
            }
        }
    }

    /// Issue #180: a circuit without bump nor drop has none, and no trick stretch: its
    /// centerline is the one of before (`shaped` without them is `with_zones`).
    #[test]
    fn a_circuit_without_relief_has_none() {
        let data = prairie();
        assert!(data.bumps.is_empty() && data.drops.is_empty());
        let center = CenterLine::of(&data);
        assert!(center.bumps().is_empty() && center.cliffs().is_empty());
        assert!(center.trick_stretches().is_empty());
        let zoned = CenterLine::with_zones(&data.points, data.sample_step, &data.anti_gravity);
        assert_eq!(center.samples(), zoned.samples());
        assert_eq!(center.length(), zoned.length());
    }
}
