//! The race seen along the centerline (pure, no ECS): checkpoints, race distance, where a kart
//! is relative to its progress, and where the tow pelican drops it.
//!
//! **Race distance** = distance driven along the centerline since the start line, laps
//! included: negative on the grid, `laps × length` on the finish line. Checkpoints are counted
//! globally: checkpoint `g` is checkpoint `g mod N` of lap `g div N`, so checkpoint 0 is the
//! start line crossed at the start, checkpoint `N` the line at the end of lap 1, and checkpoint
//! `laps × N` the finish.

use bevy::prelude::*;

use super::tuning::RaceTuning;
use crate::track::mesh::{WALL_THICKNESS, wall_offset};
use crate::track::{CenterLine, GRID_HEIGHT, TrackLayout};

/// Where a kart is, relative to the stretch of track its progress allows (see
/// [`Course::project`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    /// Race distance of the kart (m). Not clamped: beyond the next checkpoint when the kart has
    /// crossed it, behind the window when it drove back further.
    pub distance: f32,
    /// Signed distance to the centerline, along the (banked) road, right positive (m).
    pub lateral: f32,
    /// Height above the road surface (m).
    pub height: f32,
    /// Distance from the centerline to the inner face of the walls there (m).
    pub wall: f32,
}

/// The circuit as the race rules see it. Built once per track from the [`TrackLayout`].
#[derive(Resource, Clone, Debug)]
pub struct Course {
    pub center: CenterLine,
    /// Centerline distance of each checkpoint; `[0]` = 0, the start/finish line.
    pub checkpoints: Vec<f32>,
    /// For each checkpoint: the tow pelican's drop point (centerline distance), the closest
    /// one at or behind it.
    pub drops: Vec<f32>,
    /// Starting places, slot 0 = pole.
    pub grid: Vec<Transform>,
    pub laps: u32,
    /// See [`RaceTuning`].
    pub backward_window: f32,
    pub gate_margin: f32,
    pub gate_height: f32,
}

impl Course {
    pub fn new(layout: &TrackLayout, tuning: &RaceTuning) -> Self {
        let center = layout.center.clone();
        let checkpoints: Vec<f32> = layout
            .checkpoints
            .iter()
            .map(|c| c.sample.distance)
            .collect();
        let drops = checkpoints
            .iter()
            .map(|cp| drop_point(&center, *cp, &layout.respawns))
            .collect();
        Self {
            center,
            checkpoints,
            drops,
            grid: layout.grid.clone(),
            laps: tuning.laps.max(1),
            backward_window: tuning.backward_window,
            gate_margin: tuning.gate_margin,
            gate_height: tuning.gate_height,
        }
    }

    pub fn length(&self) -> f32 {
        self.center.length()
    }

    /// Checkpoints per lap.
    pub fn checkpoint_count(&self) -> u32 {
        self.checkpoints.len() as u32
    }

    /// Checkpoints to validate from the grid to the finish: the start line, then `N` per lap.
    pub fn checkpoints_to_finish(&self) -> u32 {
        self.laps * self.checkpoint_count() + 1
    }

    /// Race distance of the finish line.
    pub fn finish_distance(&self) -> f32 {
        self.laps as f32 * self.length()
    }

    /// Race distance of global checkpoint `g` (`-1` = the last checkpoint before the start line,
    /// where karts on the grid come from).
    pub fn checkpoint_distance(&self, g: i64) -> f32 {
        let n = self.checkpoints.len() as i64;
        g.div_euclid(n) as f32 * self.length() + self.checkpoints[g.rem_euclid(n) as usize]
    }

    /// Drop point (race distance) for a kart whose last validated checkpoint is `g`.
    pub fn drop_distance(&self, g: i64) -> f32 {
        let k = g.rem_euclid(self.checkpoints.len() as i64) as usize;
        let behind = self.center.wrap(self.checkpoints[k] - self.drops[k]);
        self.checkpoint_distance(g) - behind
    }

    /// The stretch of track (race distances) a kart with `passed` validated checkpoints is
    /// tracked on: from `backward_window` behind its last checkpoint (or its drop point, if
    /// further back) to its next checkpoint.
    pub fn window(&self, passed: u32) -> (f32, f32) {
        let last = passed as i64 - 1;
        let last_distance = self.checkpoint_distance(last);
        let to_drop = last_distance - self.drop_distance(last);
        let back = self.backward_window.max(to_drop + 2.0);
        (
            last_distance - back,
            self.checkpoint_distance(passed as i64),
        )
    }

    /// Projects `position` on the window of a kart with `passed` validated checkpoints: the
    /// closest centerline point is searched there only, so a kart can never jump to another part
    /// of the track (shortcut, the loop passing close to itself).
    pub fn project(&self, passed: u32, position: Vec3) -> Projection {
        let (start, end) = self.window(passed);
        let samples = self.center.samples();
        let count = samples.len() as i64;
        let step = self.length() / count as f32;
        // Sample `k` (any integer) sits at race distance `k × step`.
        let first = (start / step).floor() as i64;
        let last = (end / step).ceil() as i64;
        let sample = |k: i64| &samples[k.rem_euclid(count) as usize];
        let mut best = (f32::MAX, first);
        for k in first..=last {
            let d = sample(k).position.distance_squared(position);
            if d < best.0 {
                best = (d, k);
            }
        }
        let s = sample(best.1);
        let offset = position - s.position;
        Projection {
            distance: best.1 as f32 * step + offset.dot(s.forward),
            lateral: offset.dot(s.right),
            height: offset.dot(s.up),
            wall: wall_offset(s),
        }
    }

    /// Projects `position` on the nearest point of the whole centerline, without a progress
    /// window: for a racer whose progress no longer moves (finished, but still driving: bots).
    /// `distance` is then the centerline distance in `[0, length)`. Brute force over the samples.
    pub fn project_anywhere(&self, position: Vec3) -> Projection {
        let samples = self.center.samples();
        let s = &samples[self.center.nearest_index(position)];
        let offset = position - s.position;
        Projection {
            distance: s.distance + offset.dot(s.forward),
            lateral: offset.dot(s.right),
            height: offset.dot(s.up),
            wall: wall_offset(s),
        }
    }

    /// Whether a kart there is inside the gate of its next checkpoint (laterally and in height).
    pub fn inside_gate(&self, projection: &Projection) -> bool {
        projection.lateral.abs() <= projection.wall + self.gate_margin
            && projection.height.abs() <= self.gate_height
    }

    /// Whether a kart there is out of the track: beyond the outer face of the walls by more than
    /// `margin`.
    pub fn outside_walls(&self, projection: &Projection, margin: f32) -> bool {
        projection.lateral.abs() > projection.wall + WALL_THICKNESS + margin
    }

    /// Where the tow pelican drops a kart whose last validated checkpoint is `g`: on the road at
    /// its drop point, facing the race direction. `lane` -1, 0 or 1 moves it by `lane_width`
    /// (share of the half width) so that karts dropped together do not overlap.
    pub fn drop_transform(&self, g: i64, lane: f32, lane_width: f32) -> Transform {
        let sample = self.center.at(self.drop_distance(g));
        sample.transform(lane * lane_width * sample.half_width, GRID_HEIGHT)
    }
}

/// The drop point of a checkpoint at centerline distance `checkpoint`: the respawn point of the
/// track closest to it at or behind it (the checkpoint itself if the track has none).
pub fn drop_point(center: &CenterLine, checkpoint: f32, respawns: &[f32]) -> f32 {
    respawns
        .iter()
        .copied()
        // A drop point a hair ahead (rounding) counts as on the checkpoint.
        .min_by(|a, b| {
            let behind = |r: f32| center.wrap(checkpoint - r + 0.01);
            behind(*a).total_cmp(&behind(*b))
        })
        .unwrap_or(checkpoint)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::track::TrackData;

    pub(crate) fn prairie() -> (TrackData, Course) {
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/prairie.track.ron")).unwrap();
        let course = Course::new(&TrackLayout::new(&data), &RaceTuning::default());
        (data, course)
    }

    /// A point on the road at race distance `distance`, `lateral` meters right of the
    /// centerline, at kart height.
    pub(crate) fn at(course: &Course, distance: f32, lateral: f32) -> Vec3 {
        course.center.at(distance).point(lateral, GRID_HEIGHT)
    }

    #[test]
    fn checkpoints_are_counted_across_laps() {
        let (_, course) = prairie();
        let n = course.checkpoint_count() as i64;
        let length = course.length();
        assert_eq!(n, 21);
        assert_eq!(course.checkpoint_distance(0), 0.0);
        assert!((course.checkpoint_distance(n) - length).abs() < 1e-3);
        assert!((course.checkpoint_distance(3 * n) - course.finish_distance()).abs() < 1e-2);
        assert!(course.checkpoint_distance(-1) < 0.0);
        assert!((course.checkpoint_distance(-1) + length / n as f32).abs() < 1e-2);
        assert_eq!(course.checkpoints_to_finish(), 64);
        for g in -1..(3 * n) {
            assert!(course.checkpoint_distance(g + 1) > course.checkpoint_distance(g));
        }
    }

    #[test]
    fn projection_follows_the_centerline_and_ignores_the_rest_of_the_track() {
        let (_, course) = prairie();
        // On the grid (5 m behind the line, slot 0), nothing validated yet.
        let grid = course.project(0, course.grid[0].translation);
        assert!((grid.distance + 5.0).abs() < 0.3, "{grid:?}");
        assert!(grid.lateral < -1.0, "slot 0 is on the left: {grid:?}");
        assert!(grid.height.abs() < 1.0);
        // Mid-lap 2, 3 m right of the centerline.
        let length = course.length();
        let passed = course.checkpoint_count() + 8;
        let target = length + course.checkpoints[7] + 10.0;
        let p = course.project(passed, at(&course, target, 3.0));
        assert!((p.distance - target).abs() < 0.2, "{p:?}");
        assert!((p.lateral - 3.0).abs() < 0.2);
        assert!(course.inside_gate(&p) && !course.outside_walls(&p, 0.5));
        // A point of the track 200 m further is projected on the window, not where it really
        // is: far from the window's centerline, out of the track, never inside the next gate.
        let far = course.project(passed, at(&course, target + 200.0, 0.0));
        assert!(far.lateral.abs() > 15.0, "{far:?}");
        assert!(course.outside_walls(&far, 0.5), "{far:?}");
        assert!(!course.inside_gate(&far), "{far:?}");
    }

    #[test]
    fn projecting_anywhere_finds_the_nearest_stretch_of_track() {
        let (_, course) = prairie();
        let length = course.length();
        // A finished racer's window is around the finish line; it drives on (bots) past the
        // hairpin, to the back straight 36 m beside the start straight.
        let finished = course.checkpoints_to_finish();
        let back_straight = at(&course, 3.0 * length + 220.0, -2.0);
        let windowed = course.project(finished, back_straight);
        assert!(course.outside_walls(&windowed, 0.5), "{windowed:?}");
        let anywhere = course.project_anywhere(back_straight);
        assert!((anywhere.lateral + 2.0).abs() < 0.2, "{anywhere:?}");
        assert!((anywhere.distance - 220.0).abs() < 0.5, "{anywhere:?}");
        assert!(!course.outside_walls(&anywhere, 0.5));
        assert!((anywhere.height - GRID_HEIGHT).abs() < 0.05);
    }

    #[test]
    fn drop_points_are_at_or_behind_their_checkpoint_and_off_the_ramp() {
        let (data, course) = prairie();
        let ramp = course.center.param_to_distance(data.trick_ramps[0].at);
        let mut moved = 0;
        for (k, (cp, drop)) in course.checkpoints.iter().zip(&course.drops).enumerate() {
            let behind = course.center.wrap(cp - drop + 0.01) - 0.01;
            assert!(
                (0.0..60.0).contains(&behind),
                "checkpoint {k}: drop {behind} m behind"
            );
            if behind > 0.1 {
                moved += 1;
            }
            let into_ramp = course.center.wrap(drop - ramp);
            assert!(into_ramp > data.trick_ramps[0].length, "drop on the ramp");
        }
        // The ramp pushes a checkpoint's drop point back.
        assert!(moved >= 1, "{moved}");
        // The drop point is inside the progress window of a kart that validated that
        // checkpoint, so dropping it there never looks like leaving the track.
        let n = course.checkpoint_count();
        for passed in 0..(2 * n) {
            let (start, _) = course.window(passed);
            assert!(course.drop_distance(passed as i64 - 1) > start);
        }
    }

    #[test]
    fn drop_transform_faces_the_race_on_the_road() {
        let (_, course) = prairie();
        let n = course.checkpoint_count() as i64;
        for g in -1..n {
            for lane in [-1.0, 0.0, 1.0] {
                let t = course.drop_transform(g, lane, 0.4);
                let p = course.project((g + 1) as u32, t.translation);
                assert!((p.height - GRID_HEIGHT).abs() < 0.05, "{p:?}");
                if lane == 0.0 {
                    assert!(p.lateral.abs() < 0.1, "{p:?}");
                } else {
                    assert!(p.lateral * lane > 1.5, "lane {lane}: {p:?}");
                }
                assert!(p.lateral.abs() < p.wall - 1.0);
                let forward = course.center.at(course.drop_distance(g)).forward;
                assert!(t.forward().dot(forward) > 0.99);
            }
        }
    }

    #[test]
    fn drop_point_picks_the_closest_respawn_behind() {
        let (_, course) = prairie();
        let center = &course.center;
        let length = center.length();
        assert_eq!(drop_point(center, 100.0, &[50.0, 90.0, 120.0]), 90.0);
        assert_eq!(drop_point(center, 100.0, &[100.0, 120.0]), 100.0);
        // Wraps around the start line.
        assert_eq!(
            drop_point(center, 10.0, &[200.0, length - 20.0]),
            length - 20.0
        );
        assert_eq!(drop_point(center, 42.0, &[]), 42.0);
    }
}
