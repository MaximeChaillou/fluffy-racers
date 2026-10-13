//! The bots' racing line (pure, no ECS), built once per circuit from its centerline
//! (`TrackLayout`).
//!
//! - **Lane limits** per centerline sample: how far left and right a kart's center may go, on
//!   the asphalt and out of the sand patches.
//! - **Racing line**: the smoothest line within narrower limits (wide entry, apex, wide exit),
//!   found by minimizing its curvature: each point moves to where the bending of the line around
//!   it is smallest, from coarse to fine scales, clamped to the limits.
//! - **Turns**: stretches where the racing line keeps turning the same way, with their total
//!   angle, so a bot knows whether a turn is long enough to drift (GD § 3).
//! - **Locating** a kart: nearest centerline sample, searched around last tick's one.
//!
//! Lateral positions are in meters along the road surface, right positive (like
//! `TrackSample::point`); curvatures in 1/m, right turns positive.

use bevy::prelude::*;

use crate::kart::DriftSide;
use crate::track::mesh::wall_offset;
use crate::track::{CenterLine, TrackLayout};

/// A curvature below this (radius above 70 m) is a straight for the turn detection (1/m).
const TURN_MIN_CURVATURE: f32 = 1.0 / 70.0;
/// Straighter gaps up to this many samples inside a turn do not split it.
const TURN_GAP: usize = 6;
/// Half width of the curvature smoothing window (samples).
const CURVATURE_SMOOTHING: usize = 3;
/// Strides (samples) and sweeps of the coarse-to-fine smoothing of the racing line.
const LINE_STRIDES: [usize; 6] = [32, 16, 8, 4, 2, 1];
const LINE_SWEEPS: usize = 150;
/// Samples searched behind and ahead of last tick's one when locating a kart.
const SEARCH_BEHIND: isize = 6;
const SEARCH_AHEAD: isize = 12;
/// Sand patches keep the line away this far before and after them along the track (m).
const SAND_BUFFER: f32 = 4.0;

/// A turn of the racing line: samples `start..start + len` (wrapping), all turning to `side`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turn {
    pub side: DriftSide,
    /// First sample.
    pub start: usize,
    /// Number of samples.
    pub len: usize,
    /// Total heading change (rad, positive).
    pub angle: f32,
}

/// Where a kart is along the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    /// Nearest centerline sample.
    pub index: usize,
    /// Distance along the centerline from the start line, in `[0, length)` (m).
    pub distance: f32,
    /// Signed distance to the centerline along the road, right positive (m).
    pub lateral: f32,
}

/// The racing line of a circuit and what the bots need to know about its turns.
#[derive(Resource, Clone, Debug)]
pub struct RacingLine {
    center: CenterLine,
    step: f32,
    /// Lateral offset of the racing line from the centerline, per sample (m).
    offset: Vec<f32>,
    /// Lateral limits of a kart's center, per sample: `[left, right]` (m).
    limits: Vec<[f32; 2]>,
    /// Signed curvature of the racing line, per sample (1/m, right turns positive).
    curvature: Vec<f32>,
    /// Same for the centerline.
    center_curvature: Vec<f32>,
    turns: Vec<Turn>,
    /// Per sample: its turn (index into `turns`) and the angle turned since the turn's start.
    turn_of: Vec<Option<(usize, f32)>>,
}

impl RacingLine {
    /// Builds the racing line of `layout`. `road_margin`: a kart's center stays this far inside
    /// the road edge; `sand_margin`: this far from sand; `line_margin`: the racing line keeps this
    /// much more room on each side.
    pub fn new(layout: &TrackLayout, road_margin: f32, sand_margin: f32, line_margin: f32) -> Self {
        let center = layout.center.clone();
        let samples = center.samples();
        let n = samples.len();
        let step = center.length() / n as f32;

        // --- Lane limits: the road minus a margin, and out of the sand patches.
        let mut limits: Vec<[f32; 2]> = samples
            .iter()
            .map(|s| {
                let half = (s.half_width - road_margin).max(0.0);
                [-half, half]
            })
            .collect();
        for zone in &layout.offroad {
            let from = center.param_to_distance(zone.from) - SAND_BUFFER;
            let to = center.param_to_distance(zone.to) + SAND_BUFFER;
            let span = center.wrap(to - from);
            for (i, s) in samples.iter().enumerate() {
                if center.wrap(s.distance - from) > span {
                    continue;
                }
                let (left, right) = (zone.left * s.half_width, zone.right * s.half_width);
                let limit = &mut limits[i];
                let touches_right = zone.right >= 0.99;
                let touches_left = zone.left <= -0.99;
                // Keep to the side of the patch with the most room.
                let keep_left =
                    touches_right || (!touches_left && (left - limit[0]) >= (limit[1] - right));
                if keep_left {
                    limit[1] = limit[1].min(left - sand_margin);
                } else {
                    limit[0] = limit[0].max(right + sand_margin);
                }
                if limit[0] > limit[1] {
                    let middle = (limit[0] + limit[1]) / 2.0;
                    *limit = [middle, middle];
                }
            }
        }

        // --- Racing line: least curvature within the narrower limits.
        let line_limits: Vec<[f32; 2]> = limits
            .iter()
            .map(|[lo, hi]| {
                let (lo, hi) = (lo + line_margin, hi - line_margin);
                if lo <= hi {
                    [lo, hi]
                } else {
                    let middle = (lo + hi) / 2.0;
                    [middle, middle]
                }
            })
            .collect();
        let points: Vec<Vec2> = samples
            .iter()
            .map(|s| Vec2::new(s.position.x, s.position.z))
            .collect();
        let rights: Vec<Vec2> = samples
            .iter()
            .map(|s| Vec2::new(s.right.x, s.right.z).normalize_or(Vec2::X))
            .collect();
        let offset = least_curvature_offsets(&points, &rights, &line_limits);

        let line_points: Vec<Vec2> = (0..n).map(|i| points[i] + rights[i] * offset[i]).collect();
        let (angles, curvature) = turning(&line_points);
        let (_, center_curvature) = turning(&points);
        let (turns, turn_of) = find_turns(&curvature, &angles);

        Self {
            center,
            step,
            offset,
            limits,
            curvature,
            center_curvature,
            turns,
            turn_of,
        }
    }

    pub fn center(&self) -> &CenterLine {
        &self.center
    }

    pub fn length(&self) -> f32 {
        self.center.length()
    }

    /// Distance between two samples (m).
    pub fn step(&self) -> f32 {
        self.step
    }

    pub fn sample_count(&self) -> usize {
        self.offset.len()
    }

    /// Sample index `i` wrapped around the loop.
    pub fn wrap_index(&self, i: isize) -> usize {
        i.rem_euclid(self.sample_count() as isize) as usize
    }

    /// Sample just before `distance` (wrapped) and the fraction toward the next one.
    fn split(&self, distance: f32) -> (usize, usize, f32) {
        let d = self.center.wrap(distance) / self.step;
        let i = (d.floor() as usize).min(self.sample_count() - 1);
        (i, (i + 1) % self.sample_count(), d - i as f32)
    }

    /// Lateral offset of the racing line at `distance` (m).
    pub fn offset_at(&self, distance: f32) -> f32 {
        let (a, b, t) = self.split(distance);
        self.offset[a] + (self.offset[b] - self.offset[a]) * t
    }

    /// Lateral limits of a kart's center at `distance`: `[left, right]` (m).
    pub fn limits_at(&self, distance: f32) -> [f32; 2] {
        let (a, b, t) = self.split(distance);
        let (la, lb) = (self.limits[a], self.limits[b]);
        [la[0] + (lb[0] - la[0]) * t, la[1] + (lb[1] - la[1]) * t]
    }

    /// Racing line offset and lane limits at sample `i`.
    pub fn offset(&self, i: usize) -> f32 {
        self.offset[i]
    }

    pub fn limits(&self, i: usize) -> [f32; 2] {
        self.limits[i]
    }

    /// Signed curvature at sample `i` of a line that follows `fidelity` of the racing line
    /// (0 = centerline, 1 = racing line).
    pub fn curvature(&self, i: usize, fidelity: f32) -> f32 {
        self.center_curvature[i] + (self.curvature[i] - self.center_curvature[i]) * fidelity
    }

    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// The turn sample `i` belongs to, with the angle turned since its start.
    pub fn turn_at(&self, i: usize) -> Option<(usize, f32)> {
        self.turn_of[i]
    }

    /// The turn sample `i` is in, or the first one starting within `horizon` samples ahead:
    /// `(turn index, samples ahead until it starts)`.
    pub fn turn_ahead(&self, i: usize, horizon: usize) -> Option<(usize, usize)> {
        (0..=horizon).find_map(|k| {
            let j = (i + k) % self.sample_count();
            self.turn_of[j].map(|(turn, _)| (turn, k))
        })
    }

    /// Turning left in `turn` from sample `i` (rad): all of it before the turn.
    pub fn remaining_angle(&self, turn: usize, i: usize) -> f32 {
        match self.turn_of[i] {
            Some((t, done)) if t == turn => (self.turns[turn].angle - done).max(0.0),
            _ => self.turns[turn].angle,
        }
    }

    /// Finds where `position` is, searching around sample `hint` (last tick's) when there is
    /// one, the whole track otherwise or when the kart is far from there (teleported by the tow
    /// pelican or a restart). Allocation free.
    pub fn locate(&self, position: Vec3, hint: Option<usize>) -> Located {
        let samples = self.center.samples();
        let mut index = None;
        if let Some(hint) = hint {
            let mut best = (f32::MAX, hint);
            for k in -SEARCH_BEHIND..=SEARCH_AHEAD {
                let j = self.wrap_index(hint as isize + k);
                let d = samples[j].position.distance_squared(position);
                if d < best.0 {
                    best = (d, j);
                }
            }
            let reach = wall_offset(&samples[best.1]) + 4.0;
            if best.0 < reach * reach {
                index = Some(best.1);
            }
        }
        let index = index.unwrap_or_else(|| self.center.nearest_index(position));
        let s = &samples[index];
        let offset = position - s.position;
        Located {
            index,
            distance: self.center.wrap(s.distance + offset.dot(s.forward)),
            lateral: offset.dot(s.right),
        }
    }
}

/// Lateral offsets (within `limits`) of the closed line through `points` (moved along
/// `rights`) with the least total curvature: projected coordinate descent on the squared second
/// differences, from coarse to fine strides so that long turns converge quickly.
fn least_curvature_offsets(points: &[Vec2], rights: &[Vec2], limits: &[[f32; 2]]) -> Vec<f32> {
    let n = points.len();
    let mut offset: Vec<f32> = limits.iter().map(|[lo, hi]| 0f32.clamp(*lo, *hi)).collect();
    for stride in LINE_STRIDES {
        if 4 * stride >= n {
            continue;
        }
        let s = stride as isize;
        for _ in 0..LINE_SWEEPS {
            for i in 0..n {
                let at = |k: isize| {
                    let j = (i as isize + k).rem_euclid(n as isize) as usize;
                    points[j] + rights[j] * offset[j]
                };
                // Where point `i` bends the line the least, given its neighbours.
                let best = (4.0 * (at(-s) + at(s)) - (at(-2 * s) + at(2 * s))) / 6.0;
                let [lo, hi] = limits[i];
                offset[i] = (best - points[i]).dot(rights[i]).clamp(lo, hi);
            }
        }
    }
    offset
}

/// Heading change at each point of a closed polyline (rad, right turns positive) and the
/// curvature there (1/m), both smoothed over a few samples.
fn turning(points: &[Vec2]) -> (Vec<f32>, Vec<f32>) {
    let n = points.len();
    let (mut angles, mut lengths) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let before = points[i] - points[(i + n - 1) % n];
        let after = points[(i + 1) % n] - points[i];
        // In the (x, z) plane, a positive angle is a right turn (z points backwards).
        angles.push(
            before
                .normalize_or_zero()
                .angle_to(after.normalize_or_zero()),
        );
        lengths.push(0.5 * (before.length() + after.length()));
    }
    let (angles, lengths) = (smooth(&angles), smooth(&lengths));
    let curvature = angles
        .iter()
        .zip(&lengths)
        .map(|(a, l)| if *l < 1e-4 { 0.0 } else { a / l })
        .collect();
    (angles, curvature)
}

/// Box filter over `CURVATURE_SMOOTHING` samples on each side (closed loop).
fn smooth(values: &[f32]) -> Vec<f32> {
    let n = values.len() as isize;
    let w = CURVATURE_SMOOTHING as isize;
    (0..n)
        .map(|i| {
            let sum: f32 = (-w..=w)
                .map(|k| values[(i + k).rem_euclid(n) as usize])
                .sum();
            sum / (2 * w + 1) as f32
        })
        .collect()
}

/// Stretches turning the same way (see [`Turn`]), and each sample's turn with the angle turned
/// since its start. `angles`: heading change at each sample (rad, right positive).
fn find_turns(curvature: &[f32], angles: &[f32]) -> (Vec<Turn>, Vec<Option<(usize, f32)>>) {
    let n = curvature.len();
    let side_of = |k: f32| {
        if k >= TURN_MIN_CURVATURE {
            Some(DriftSide::Right)
        } else if k <= -TURN_MIN_CURVATURE {
            Some(DriftSide::Left)
        } else {
            None
        }
    };
    let mut turns = Vec::new();
    let mut turn_of = vec![None; n];
    // Start the scan on a straight, so that no turn is split by the wrap-around.
    let Some(first_straight) = (0..n).find(|&i| side_of(curvature[i]).is_none()) else {
        // Turning all the way round (a circle): one endless turn starting at sample 0.
        let side = if curvature[0] > 0.0 {
            DriftSide::Right
        } else {
            DriftSide::Left
        };
        let mut done = 0.0;
        for (i, a) in angles.iter().enumerate() {
            turn_of[i] = Some((0, done));
            done += a.abs();
        }
        turns.push(Turn {
            side,
            start: 0,
            len: n,
            angle: done,
        });
        return (turns, turn_of);
    };
    let mut k = 0;
    while k < n {
        let i = (first_straight + k) % n;
        let Some(side) = side_of(curvature[i]) else {
            k += 1;
            continue;
        };
        // Extend while the line keeps turning this way, bridging short straighter gaps.
        let mut end = k + 1;
        let mut last_turning = k;
        while end < n {
            let j = (first_straight + end) % n;
            match side_of(curvature[j]) {
                Some(s) if s == side => last_turning = end,
                Some(_) => break,
                None if end - last_turning > TURN_GAP => break,
                None => {}
            }
            end += 1;
        }
        let len = last_turning - k + 1;
        let index = turns.len();
        let mut done = 0.0;
        for m in 0..len {
            let j = (first_straight + k + m) % n;
            turn_of[j] = Some((index, done));
            // Only the turning this way counts (a bridged gap may wiggle a little).
            done += (angles[j] * side.sign()).max(0.0);
        }
        turns.push(Turn {
            side,
            start: i,
            len,
            angle: done,
        });
        k = last_turning + 1;
    }
    (turns, turn_of)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bots::BotTuning;
    use crate::track::TrackData;

    pub(crate) fn prairie_line() -> (TrackLayout, RacingLine) {
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/prairie.track.ron")).unwrap();
        let layout = TrackLayout::new(&data);
        let t = BotTuning::default();
        let line = RacingLine::new(&layout, t.road_margin, t.sand_margin, t.line_margin);
        (layout, line)
    }

    #[test]
    fn the_racing_line_stays_on_the_road_and_out_of_the_sand() {
        let (layout, line) = prairie_line();
        let t = BotTuning::default();
        let samples = layout.center.samples();
        let sand = layout.offroad[0];
        let (from, to) = (
            layout.center.param_to_distance(sand.from),
            layout.center.param_to_distance(sand.to),
        );
        let mut in_sand_zone = 0;
        for (i, s) in samples.iter().enumerate() {
            let [lo, hi] = line.limits(i);
            assert!(lo <= hi, "sample {i}");
            assert!(hi <= s.half_width - t.road_margin + 1e-4);
            assert!(lo >= -(s.half_width - t.road_margin) - 1e-4);
            let o = line.offset(i);
            assert!(
                o >= lo + t.line_margin - 1e-3 && o <= hi - t.line_margin + 1e-3,
                "sample {i}: offset {o} outside {lo}..{hi}"
            );
            if (from..=to).contains(&s.distance) {
                in_sand_zone += 1;
                // The sand covers the inside of the hairpin (right side), from 55 % of the
                // half width: the kart's center stays clear of it.
                let sand_edge = sand.left * s.half_width;
                assert!(
                    hi <= sand_edge - t.sand_margin + 1e-4,
                    "{hi} vs {sand_edge}"
                );
            }
        }
        assert!(in_sand_zone > 10);
    }

    #[test]
    fn the_racing_line_cuts_the_turns() {
        let (layout, line) = prairie_line();
        let n = line.sample_count();
        // Smoother than the centerline over the lap (less total bending).
        let energy = |fidelity: f32| -> f32 {
            (0..n)
                .map(|i| line.curvature(i, fidelity).powi(2) * line.step())
                .sum()
        };
        let (center, racing) = (energy(0.0), energy(1.0));
        assert!(racing < 0.9 * center, "racing {racing} vs center {center}");
        // Wide entry, apex, wide exit in the hairpin (a right turn): left before it, on the
        // inside (right) at the apex, as far as the sand allows, left again after it.
        let center_line = &layout.center;
        let entry = center_line.param_to_distance(3.6);
        let apex = center_line.param_to_distance(6.0);
        let exit = center_line.param_to_distance(9.0);
        assert!(line.offset_at(entry) < -2.0, "{}", line.offset_at(entry));
        assert!(line.offset_at(apex) > 0.5, "{}", line.offset_at(apex));
        assert!(line.offset_at(exit) < -1.0, "{}", line.offset_at(exit));
    }

    #[test]
    fn turns_are_found_with_their_side_and_angle() {
        let (layout, line) = prairie_line();
        let turns = line.turns();
        let big: Vec<&Turn> = turns.iter().filter(|t| t.angle > 2.5).collect();
        // The hairpin and the banked turn: two right turns of about 180°.
        assert_eq!(big.len(), 2, "{turns:?}");
        for turn in &big {
            assert_eq!(turn.side, DriftSide::Right);
            // The part curved enough to count as a turn (radius under 70 m).
            assert!(
                (130f32.to_radians()..190f32.to_radians()).contains(&turn.angle),
                "{turn:?}"
            );
        }
        let hairpin_start = big[0].start as f32 * line.step();
        assert!(
            (90.0..140.0).contains(&hairpin_start),
            "hairpin starts at {hairpin_start} m"
        );
        // The chicane has left and right bends, all much shorter.
        assert!(turns.iter().any(|t| t.side == DriftSide::Left));
        // Remaining angle: the whole turn before it, less inside, nothing past its end.
        let hairpin = turns.iter().position(|t| t == big[0]).unwrap();
        let before = line.wrap_index(big[0].start as isize - 5);
        assert_eq!(line.remaining_angle(hairpin, before), big[0].angle);
        let middle = (big[0].start + big[0].len / 2) % line.sample_count();
        let left = line.remaining_angle(hairpin, middle);
        assert!(
            left > 0.2 * big[0].angle && left < 0.8 * big[0].angle,
            "{left}"
        );
        assert_eq!(line.turn_ahead(before, 10), Some((hairpin, 5)));
        assert_eq!(line.turn_ahead(middle, 0), Some((hairpin, 0)));
        let _ = layout;
    }

    #[test]
    fn locate_follows_a_kart_and_finds_it_again_after_a_teleport() {
        let (layout, line) = prairie_line();
        let center = &layout.center;
        let mut hint = None;
        for k in 0..200 {
            let d = k as f32 * 2.7;
            let s = center.at(d);
            let p = s.point(2.0, 0.5);
            let found = line.locate(p, hint);
            assert!(
                center.wrap(found.distance - d + 1.0) < 2.0,
                "{d}: {found:?}"
            );
            assert!((found.lateral - 2.0).abs() < 0.3, "{found:?}");
            hint = Some(found.index);
        }
        // Teleported to the other side of the circuit: found by the full search.
        let far = center.at(400.0).point(-1.0, 0.5);
        let found = line.locate(far, hint);
        assert!((found.distance - 400.0).abs() < 1.0, "{found:?}");
    }
}
