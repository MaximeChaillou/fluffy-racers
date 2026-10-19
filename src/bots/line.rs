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
//!
//! Issue #96: in and around the anti-gravity zones, the line is drawn in the track's own
//! coordinates (distance along it, offset across it): the road there is **developed** onto a
//! plane (`Developed`), its centerline turning only as it turns in the road's own plane (its
//! curvature measured around the road's normal). A turn banked to 90° (a wall that curves) is
//! straight there: a kart takes it without steering. Everywhere else, and on a circuit without a
//! zone, the line is drawn seen from above, exactly as before.

use bevy::prelude::*;

use crate::kart::DriftSide;
use crate::track::mesh::wall_offset;
use crate::track::{CenterLine, Shortcut, ShortcutKind, TrackLayout, TrackSample, anti_gravity_at};

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
/// Issue #96: how far the smoothing of the racing line reaches on each side of a sample (twice
/// its coarsest stride, samples). The samples this close to an anti-gravity zone are smoothed in
/// the developed road ([`Developed`]), their neighbours with them.
const STENCIL_REACH: usize = 2 * LINE_STRIDES[0];

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
    /// Issue #107: the circuit's shortcuts, as the bots follow them.
    branches: Vec<Branch>,
}

/// Issue #107: a shortcut of the circuit (`TrackLayout::shortcuts`), as a bot follows it: its
/// path, the lane to be in before it, and how its path turns.
#[derive(Clone, Debug)]
pub struct Branch {
    pub shortcut: Shortcut,
    /// Where its path starts, across the road at its start (m, right positive): the lane a bot
    /// going for it moves to.
    pub entry_lateral: f32,
    /// Signed curvature of its path, per sample (1/m, right turns positive), seen from above.
    curvature: Vec<f32>,
}

impl Branch {
    fn new(center: &CenterLine, shortcut: &Shortcut) -> Self {
        let start = center.at(shortcut.from);
        let first = shortcut.samples()[0].position;
        let samples = shortcut.samples();
        let flat = |s: &TrackSample| Vec2::new(s.forward.x, s.forward.z).normalize_or_zero();
        let curvature = (0..samples.len())
            .map(|i| {
                let (a, b) = (
                    &samples[i.saturating_sub(1)],
                    &samples[(i + 1).min(samples.len() - 1)],
                );
                let along = a.position.distance(b.position).max(1e-3);
                flat(a).angle_to(flat(b)) / along
            })
            .collect();
        Self {
            shortcut: shortcut.clone(),
            entry_lateral: (first - start.position).dot(start.right),
            curvature,
        }
    }

    pub fn kind(&self) -> ShortcutKind {
        self.shortcut.kind
    }

    /// The curvature of its path at sample `i` (1/m, right positive).
    pub fn curvature(&self, i: usize) -> f32 {
        self.curvature[i.min(self.curvature.len() - 1)]
    }
}

/// Issue #144: how the lanes keep clear of an obstacle or a passer-by (`BotTuning`): this far
/// from its footprint (its whole motion), from this far before it to this far after it along the
/// track (m). The bots aim ahead of them (`BotSkill::aim_base`, `aim_per_speed`): the lane must
/// stay clear until the kart itself has passed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HazardClearance {
    pub margin: f32,
    pub before: f32,
    pub after: f32,
}

/// Issue #107: the lane limits (`limits`, per sample of `center`) kept `margin` clear of the
/// footprint `corners` (on the ground plane) beside the centerline, on the side with the most room
/// (like a sand patch): along it, and `before` and `after` it (issue #144; [`SAND_BUFFER`] for a
/// ramp).
fn keep_clear_of(
    center: &CenterLine,
    limits: &mut [[f32; 2]],
    corners: &[Vec2],
    margin: f32,
    (before, after): (f32, f32),
) {
    for (s, limit) in center.samples().iter().zip(limits.iter_mut()) {
        let forward = s.forward.xz().normalize_or_zero();
        let right = Vec2::new(-forward.y, forward.x);
        if corners
            .iter()
            .all(|c| c.distance(s.position.xz()) > 60.0 + before.max(after))
        {
            continue;
        }
        let (mut along, mut across) = ((f32::MAX, f32::MIN), (f32::MAX, f32::MIN));
        for c in corners {
            let to = *c - s.position.xz();
            let (a, l) = (to.dot(forward), to.dot(right));
            along = (along.0.min(a), along.1.max(a));
            across = (across.0.min(l), across.1.max(l));
        }
        if along.0 > before || along.1 < -after {
            continue;
        }
        let room_left = across.0 - limit[0];
        let room_right = limit[1] - across.1;
        if room_left >= room_right {
            limit[1] = limit[1].min(across.0 - margin);
        } else {
            limit[0] = limit[0].max(across.1 + margin);
        }
        if limit[0] > limit[1] {
            let middle = (limit[0] + limit[1]) / 2.0;
            *limit = [middle, middle];
        }
    }
}

impl RacingLine {
    /// Builds the racing line of `layout`. `road_margin`: a kart's center stays this far inside
    /// the road edge; `sand_margin`: this far from sand; `void_margin`: this much farther inside a
    /// side without wall (issue #107); `line_margin`: the racing line keeps this much more room on
    /// each side; `hazard`: how the lanes keep clear of the obstacles and the passers-by (issue
    /// #144).
    pub fn new(
        layout: &TrackLayout,
        road_margin: f32,
        sand_margin: f32,
        void_margin: f32,
        line_margin: f32,
        hazard: HazardClearance,
    ) -> Self {
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

        // Issue #107: a side without wall is a drop: the lanes keep farther from its edge (a bot
        // going for a shortcut there leaves its lane for the road's edge).
        for edge in &layout.open_edges {
            for (s, limit) in samples.iter().zip(limits.iter_mut()) {
                if !edge.contains(&center, s.distance) {
                    continue;
                }
                if edge.side.opens(-1.0) {
                    limit[0] = (limit[0] + void_margin).min(limit[1]);
                }
                if edge.side.opens(1.0) {
                    limit[1] = (limit[1] - void_margin).max(limit[0]);
                }
            }
        }

        // Issue #107: the trick ramps of the risky jumps, off the centerline: the lanes keep
        // clear of them, like of the sand (only a bot going for the jump drives onto one).
        for shortcut in &layout.shortcuts {
            if let Some(ramp) = shortcut.ramp {
                keep_clear_of(
                    &center,
                    &mut limits,
                    &ramp.footprint(),
                    sand_margin,
                    (SAND_BUFFER, SAND_BUFFER),
                );
            }
        }

        // Issue #144: the obstacles and the passers-by (their whole motion), like the jumps'
        // ramps (the things to drive through are not in the way: the bots go through them).
        for obstacle in layout.hazards.iter().filter(|h| h.kind.in_the_way()) {
            keep_clear_of(
                &center,
                &mut limits,
                &obstacle.footprint(),
                hazard.margin,
                (hazard.before, hazard.after),
            );
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
        // Issue #78: in an anti-gravity zone, the road may be a wall, its right pointing down:
        // seen from above, the line moves across the track along the horizontal right of the
        // heading there (only the samples around a zone see it, and since issue #96 they draw the
        // line in the developed road instead).
        let zoned: Vec<bool> = samples
            .iter()
            .map(|s| anti_gravity_at(&layout.anti_gravity, &center, s.distance).is_some())
            .collect();
        let rights: Vec<Vec2> = samples
            .iter()
            .zip(&zoned)
            .map(|(s, zoned)| {
                if *zoned {
                    Vec2::new(-s.forward.z, s.forward.x).normalize_or(Vec2::X)
                } else {
                    Vec2::new(s.right.x, s.right.z).normalize_or(Vec2::X)
                }
            })
            .collect();
        // Issue #96: around the anti-gravity zones, the road developed onto a plane (none on a
        // circuit without a zone: everything below is then exactly as before).
        let developed = Developed::new(samples, &zoned);
        let offset = least_curvature_offsets(&points, &rights, &line_limits, developed.as_ref());

        let line_points: Vec<Vec2> = (0..n).map(|i| points[i] + rights[i] * offset[i]).collect();
        let line_chain = developed.as_ref().map(|d| d.line(&offset));
        let (angles, curvature) = turning(
            &line_points,
            line_chain
                .as_deref()
                .zip(developed.as_ref().map(|d| &d.zoned[..])),
        );
        let (_, center_curvature) = turning(
            &points,
            developed.as_ref().map(|d| (&d.points[..], &d.zoned[..])),
        );
        let (turns, turn_of) = find_turns(&curvature, &angles);
        let branches = layout
            .shortcuts
            .iter()
            .map(|shortcut| Branch::new(&center, shortcut))
            .collect();

        Self {
            center,
            step,
            offset,
            limits,
            curvature,
            center_curvature,
            turns,
            turn_of,
            branches,
        }
    }

    /// Issue #107: the shortcuts the bots may take.
    pub fn branches(&self) -> &[Branch] {
        &self.branches
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

/// Issue #96: the road around the anti-gravity zones, developed onto a plane: its centerline
/// walked sample after sample, turning by as much as it turns in the road's own plane (around the
/// road's normal), each sample with its right across the road. Seen from above, a wall that
/// curves (a turn banked to 90°) turns; developed, it runs straight. Its coordinates are those of
/// the ground plane's (`x`, `z`) seen from above (a right turn is a positive angle), but they
/// only have a meaning relative to each other, a few samples apart: the walk does not close on
/// itself.
///
/// The walk covers every sample, from [`STENCIL_REACH`] samples before the start line to as many
/// past the end of the lap: sample `i` is at `i + STENCIL_REACH`, its neighbours on both sides of
/// the line's smoothing next to it.
struct Developed {
    /// The centerline's samples, developed (len: samples + 2 × [`STENCIL_REACH`]).
    points: Vec<Vec2>,
    /// Their right, across the road.
    rights: Vec<Vec2>,
    /// Per sample: in an anti-gravity zone (its turning is measured in the road's plane).
    zoned: Vec<bool>,
    /// Per sample: close enough to a zone for the line's smoothing to see it (its neighbours are
    /// taken in the developed road).
    near: Vec<bool>,
}

impl Developed {
    /// The developed road of a centerline whose samples `zoned` are in an anti-gravity zone;
    /// `None` without any (the line is then drawn seen from above, as before issue #96).
    fn new(samples: &[TrackSample], zoned: &[bool]) -> Option<Self> {
        if !zoned.contains(&true) {
            return None;
        }
        let n = samples.len();
        let sample = |c: usize| &samples[Self::sample(c, n)];
        let count = n + 2 * STENCIL_REACH;
        let (mut points, mut rights) = (Vec::with_capacity(count), Vec::with_capacity(count));
        let (mut at, mut heading) = (Vec2::ZERO, 0.0f32);
        for c in 0..count {
            points.push(at);
            rights.push(Vec2::from_angle(heading).perp());
            let (a, b) = (sample(c), sample(c + 1));
            let next = heading + road_turn(a, b);
            at += Vec2::from_angle(0.5 * (heading + next)) * a.position.distance(b.position);
            heading = next;
        }
        // The samples whose smoothing reaches a zone's.
        let mut near = vec![false; n];
        let reach = STENCIL_REACH as isize;
        for (i, _) in zoned.iter().enumerate().filter(|(_, z)| **z) {
            for k in -reach..=reach {
                near[(i as isize + k).rem_euclid(n as isize) as usize] = true;
            }
        }
        Some(Self {
            points,
            rights,
            zoned: zoned.to_vec(),
            near,
        })
    }

    /// Where sample `i` is in the walk.
    fn index(i: usize) -> usize {
        i + STENCIL_REACH
    }

    /// The sample at `c` in the walk (of a centerline of `n` samples).
    fn sample(c: usize, n: usize) -> usize {
        (c + n - STENCIL_REACH % n) % n
    }

    /// The racing line `offset` (per sample) in the developed road, walked like it.
    fn line(&self, offset: &[f32]) -> Vec<Vec2> {
        let n = offset.len();
        (0..self.points.len())
            .map(|c| self.points[c] + self.rights[c] * offset[Self::sample(c, n)])
            .collect()
    }
}

/// Issue #96: how much the centerline turns from sample `a` to sample `b` in the road's own plane
/// (rad, toward the road's right positive): the angle between their headings around the road's
/// normal between them. On flat ground, its turn seen from above; on a wall that curves, nothing.
fn road_turn(a: &TrackSample, b: &TrackSample) -> f32 {
    let up = (a.up + b.up).normalize_or(a.up);
    let level = |v: Vec3| (v - up * v.dot(up)).normalize_or_zero();
    let (from, to) = (level(a.forward), level(b.forward));
    from.cross(to).dot(-up).atan2(from.dot(to))
}

/// Lateral offsets (within `limits`) of the closed line through `points` (moved along
/// `rights`) with the least total curvature: projected coordinate descent on the squared second
/// differences, from coarse to fine strides so that long turns converge quickly. Issue #96: the
/// samples near an anti-gravity zone bend the line the least in the `developed` road instead.
fn least_curvature_offsets(
    points: &[Vec2],
    rights: &[Vec2],
    limits: &[[f32; 2]],
    developed: Option<&Developed>,
) -> Vec<f32> {
    let n = points.len();
    let mut offset: Vec<f32> = limits.iter().map(|[lo, hi]| 0f32.clamp(*lo, *hi)).collect();
    for stride in LINE_STRIDES {
        if 4 * stride >= n {
            continue;
        }
        let s = stride as isize;
        for _ in 0..LINE_SWEEPS {
            for i in 0..n {
                let [lo, hi] = limits[i];
                if let Some(road) = developed.filter(|d| d.near[i]) {
                    // In the developed road: the same rule, its neighbours taken along it.
                    let c = Developed::index(i) as isize;
                    let at = |k: isize| {
                        let j = (i as isize + k).rem_euclid(n as isize) as usize;
                        let c = (c + k) as usize;
                        road.points[c] + road.rights[c] * offset[j]
                    };
                    let best = (4.0 * (at(-s) + at(s)) - (at(-2 * s) + at(2 * s))) / 6.0;
                    let c = c as usize;
                    offset[i] = (best - road.points[c]).dot(road.rights[c]).clamp(lo, hi);
                    continue;
                }
                let at = |k: isize| {
                    let j = (i as isize + k).rem_euclid(n as isize) as usize;
                    points[j] + rights[j] * offset[j]
                };
                // Where point `i` bends the line the least, given its neighbours.
                let best = (4.0 * (at(-s) + at(s)) - (at(-2 * s) + at(2 * s))) / 6.0;
                offset[i] = (best - points[i]).dot(rights[i]).clamp(lo, hi);
            }
        }
    }
    offset
}

/// Heading change at each point of a closed polyline (rad, right turns positive) and the
/// curvature there (1/m), both smoothed over a few samples. Issue #96: `developed`, the same
/// line walked in the developed road ([`Developed`]) and the samples in an anti-gravity zone,
/// whose turning is measured there (in the road's own plane).
fn turning(points: &[Vec2], developed: Option<(&[Vec2], &[bool])>) -> (Vec<f32>, Vec<f32>) {
    let n = points.len();
    let (mut angles, mut lengths) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for i in 0..n {
        let (before, after) = match developed {
            Some((walk, zoned)) if zoned[i] => {
                let c = Developed::index(i);
                (walk[c] - walk[c - 1], walk[c + 1] - walk[c])
            }
            _ => (
                points[i] - points[(i + n - 1) % n],
                points[(i + 1) % n] - points[i],
            ),
        };
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

    /// Issue #108: the racing line of the test circuit (the layout Prairie Pompon had before
    /// #108).
    pub(crate) fn prairie_line() -> (TrackLayout, RacingLine) {
        let data = crate::track::test_circuit();
        let layout = TrackLayout::new(&data);
        let t = BotTuning::default();
        let line = RacingLine::new(
            &layout,
            t.road_margin,
            t.sand_margin,
            t.void_margin,
            t.line_margin,
            t.hazard_clearance(),
        );
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

    /// A loop turning right, 9 m up, whose north-east corner is a turn of 90° (radius 60 m):
    /// banked like a wall around it (90° at its middle) in an anti-gravity zone, or flat without
    /// it (`banked` false).
    fn banked_loop(banked: bool) -> TrackData {
        let corners: [(f32, f32, f32); 23] = [
            (0.0, 100.0, 0.0),
            (0.0, 50.0, 0.0),
            (0.0, 0.0, 0.0),
            (8.79, -21.21, 0.0),
            (30.0, -30.0, 0.0),
            (60.0, -30.0, 0.0),
            (85.0, -30.0, 30.0),
            (110.0, -30.0, 60.0),
            (132.96, -25.43, 80.0),
            (152.43, -12.43, 90.0),
            (165.43, 7.04, 80.0),
            (170.0, 30.0, 60.0),
            (170.0, 55.0, 30.0),
            (170.0, 80.0, 0.0),
            (170.0, 120.0, 0.0),
            (170.0, 150.0, 0.0),
            (161.21, 171.21, 0.0),
            (140.0, 180.0, 0.0),
            (100.0, 180.0, 0.0),
            (60.0, 180.0, 0.0),
            (30.0, 180.0, 0.0),
            (8.79, 171.21, 0.0),
            (0.0, 150.0, 0.0),
        ];
        let points: Vec<String> = corners
            .iter()
            .map(|(x, z, bank)| {
                let bank = if banked { *bank } else { 0.0 };
                format!("(pos: ({x}, 9.0, {z}), width: 12.0, bank: {bank}, shoulder: 1.0)")
            })
            .collect();
        let zones = if banked {
            "anti_gravity: [(from: 5.0, to: 13.0)],"
        } else {
            ""
        };
        let text = format!(
            "(name: \"Virage\", points: [{}], {zones})",
            points.join(", ")
        );
        let data = TrackData::from_ron(&text).unwrap();
        assert_eq!(data.validate(), Ok(()));
        data
    }

    fn line_of(data: &TrackData) -> (TrackLayout, RacingLine) {
        let layout = TrackLayout::new(data);
        let t = BotTuning::default();
        let line = RacingLine::new(
            &layout,
            t.road_margin,
            t.sand_margin,
            t.void_margin,
            t.line_margin,
            t.hazard_clearance(),
        );
        (layout, line)
    }

    /// Issue #96 (GD § 10): in an anti-gravity zone, the racing line is drawn in the track's own
    /// coordinates, its curvature measured in the road's plane. A turn of 90° banked like a wall
    /// (a radius of 60 m seen from above) is straight there: around its middle, neither the
    /// centerline nor the racing line turns (a kart takes it without steering), no turn is found
    /// there (no braking, no drift), and the line stays on the road. Flat, the same corner is a
    /// right turn of about 90° where the line curves at about 1/60 m, as seen from above.
    #[test]
    fn a_turn_banked_like_a_wall_is_straight_for_the_bots() {
        let t = BotTuning::default();
        for banked in [true, false] {
            let data = banked_loop(banked);
            let (layout, line) = line_of(&data);
            let center = &layout.center;
            let middle = center.param_to_distance(9.0);
            let apex = center.nearest_index(center.at(middle).position);
            assert_eq!(banked, center.samples()[apex].up.y.abs() < 0.02);
            for k in -10isize..=10 {
                let i = line.wrap_index(apex as isize + k);
                let (centerline, racing) = (line.curvature(i, 0.0), line.curvature(i, 1.0));
                if banked {
                    assert!(centerline.abs() < 1.0 / 400.0, "{i}: {centerline}");
                    assert!(racing.abs() < 1.0 / 400.0, "{i}: {racing}");
                    assert!(line.turn_at(i).is_none(), "{i}");
                } else {
                    // Seen from above (the racing line cuts the corner, a little wider).
                    assert!(
                        (1.0 / 75.0..1.0 / 50.0).contains(&centerline),
                        "{i}: {centerline}"
                    );
                    assert!(racing > 1.0 / 150.0, "{i}: {racing}");
                }
            }
            // Banked, no turn at all around the corner (a turn of the corner would start before
            // its middle).
            if banked {
                let around = center.param_to_distance(5.0)..center.param_to_distance(13.0);
                for turn in line.turns() {
                    let start = turn.start as f32 * line.step();
                    assert!(
                        !around.contains(&start) || turn.angle < 30f32.to_radians(),
                        "{turn:?}"
                    );
                }
            }
            for i in 0..line.sample_count() {
                let [lo, hi] = line.limits(i);
                let o = line.offset(i);
                assert!(
                    o >= lo + t.line_margin - 1e-3 && o <= hi - t.line_margin + 1e-3,
                    "{i}: {o}"
                );
            }
        }
    }

    /// Issue #107 (GD « Conception des circuits »): on the Corniche Cannelle and its mirror, the
    /// lanes keep `void_margin` more room from a side without wall (a drop) than from a wall; and
    /// its racing line has at least five turns of 120° or more, the ones a Normal bot drifts
    /// (`BotSkill::min_drift_angle`: the guide's drifts per lap). Issue #108: the same on Prairie
    /// Pompon redrawn after the guide; issue #109: on the Col des Moufles redrawn.
    #[test]
    fn the_lanes_keep_away_from_a_drop_and_the_reference_circuit_has_turns_to_drift() {
        let t = BotTuning::default();
        let circuits = [
            include_str!("../../assets/tracks/corniche.track.ron"),
            include_str!("../../assets/tracks/prairie.track.ron"),
            include_str!("../../assets/tracks/col.track.ron"),
        ];
        for data in circuits.into_iter().flat_map(|text| {
            let data = TrackData::from_ron(text).unwrap();
            [data.clone(), data.mirrored()]
        }) {
            let (layout, line) = line_of(&data);
            let center = &layout.center;
            assert!(!layout.open_edges.is_empty());
            for edge in &layout.open_edges {
                let middle = edge.from + edge.length(center) / 2.0;
                let i = center.nearest_index(center.at(middle).position);
                let half = center.samples()[i].half_width - t.road_margin;
                let [lo, hi] = line.limits(i);
                let (open, walled) = if edge.side.opens(1.0) {
                    (hi, -lo)
                } else {
                    (-lo, hi)
                };
                assert!(
                    open <= half - t.void_margin + 1e-3,
                    "{edge:?}: {open} of {half}"
                );
                assert!(walled > half - 1e-3, "{edge:?}: {walled} of {half}");
            }
            let big = line
                .turns()
                .iter()
                .filter(|turn| turn.angle >= 120f32.to_radians())
                .count();
            assert!(big >= 5, "{}: {:?}", data.name, line.turns());
        }
    }

    /// Issue #96: the Ruban Réglisse's banked turn (its third zone) is straight for the bots too,
    /// around its middle; its mirror image too. Its other turns are still found (the hairpin
    /// where they drift, issue #94).
    #[test]
    fn the_ribbon_banked_turn_is_straight_for_the_bots() {
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/ruban.track.ron")).unwrap();
        for data in [data.clone(), data.mirrored()] {
            let (layout, line) = line_of(&data);
            let center = &layout.center;
            let zone = layout.anti_gravity[2];
            let middle = zone.from + zone.length(center) / 2.0;
            let apex = center.nearest_index(center.at(middle).position);
            let mut walled = 0;
            for k in -10isize..=10 {
                let i = line.wrap_index(apex as isize + k);
                assert!(line.turn_at(i).is_none(), "{i}");
                if center.samples()[i].up.y.abs() < 0.1 {
                    walled += 1;
                    let curvature = line.curvature(i, 1.0);
                    assert!(curvature.abs() < 1.0 / 300.0, "{i}: {curvature}");
                }
            }
            assert!(walled >= 10, "{walled}");
            assert!(
                line.turns()
                    .iter()
                    .any(|turn| turn.angle > 130f32.to_radians()),
                "the hairpin: {:?}",
                line.turns()
            );
        }
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
