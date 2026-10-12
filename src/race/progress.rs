//! A racer's progress (pure, no ECS): checkpoints validated in order, laps, lap times and
//! finish, plus its live race distance for the ranking.
//!
//! Progress = (lap, last checkpoint, distance to the next one): `passed` checkpoints validated
//! and a race distance between the last one and the next one (see `course.rs`).
//!
//! - **Anti-shortcut**: only the next expected checkpoint can be validated, by crossing its gate
//!   (within the walls) while the kart is tracked on the stretch of track leading to it. A lap
//!   counts only once all its checkpoints were crossed in order.
//! - **Driving back** lowers the race distance (live ranking), down to the start of the window,
//!   but never un-validates a checkpoint, and crossing one again does not count twice.
//! - **Times** are in simulation ticks (60 Hz): deterministic.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::course::{Course, Projection};

/// What happened to a racer's progress this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressEvent {
    /// The next checkpoint was crossed.
    Checkpoint,
    /// A lap was completed (its number, from 1); not sent for the last lap.
    Lap(u32),
    /// The last lap was completed: the racer finished.
    Finished,
}

/// Race progress of a kart. On every racer (`Racer`); the race rules update it every tick.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RaceProgress {
    /// Checkpoints validated since the start, the start line included: the next expected one is
    /// global checkpoint `passed` (see `course.rs`).
    pub passed: u32,
    /// Race distance (m): along the centerline since the start line, laps included, clamped to
    /// the stretch between the last and the next checkpoint. Frozen once finished.
    pub distance: f32,
    /// Race tick (see `RaceFlow::race_ticks`) at which the current lap started.
    pub lap_start: u32,
    /// Time of each completed lap, in ticks.
    pub lap_times: Vec<u32>,
    /// Race tick of the finish: the total race time.
    pub finish: Option<u32>,
    /// Rank in the race, from 1 (written by the ranking).
    pub rank: u32,
    /// Distance to the leader along the centerline, laps included (m, 0 for the leader). The
    /// item distribution will use it (GD § 8).
    pub gap_to_leader: f32,
}

impl RaceProgress {
    /// Progress of a kart standing at `position` before the start (on the grid).
    pub fn on_grid(course: &Course, position: Vec3, laps: u32) -> Self {
        let mut progress = Self {
            lap_times: Vec::with_capacity(laps as usize),
            ..default()
        };
        progress.reset(course, position);
        progress
    }

    /// Back to the start of a race, keeping the allocated lap times.
    pub fn reset(&mut self, course: &Course, position: Vec3) {
        self.passed = 0;
        self.lap_start = 0;
        self.lap_times.clear();
        self.finish = None;
        self.distance = clamp_to_window(course, 0, course.project(0, position).distance);
    }

    pub fn finished(&self) -> bool {
        self.finish.is_some()
    }

    /// Laps completed.
    pub fn laps_completed(&self, course: &Course) -> u32 {
        (self.passed.saturating_sub(1) / course.checkpoint_count()).min(course.laps)
    }

    /// Lap being driven, from 1 (the last lap once finished): "Tour 2/3".
    pub fn current_lap(&self, course: &Course) -> u32 {
        (self.laps_completed(course) + 1).min(course.laps)
    }

    /// Fastest completed lap, in ticks.
    pub fn best_lap(&self) -> Option<u32> {
        self.lap_times.iter().copied().min()
    }

    /// The racer's own clock at race tick `race_ticks`: its total time once finished.
    pub fn time(&self, race_ticks: u32) -> u32 {
        self.finish.unwrap_or(race_ticks)
    }

    /// Updates the progress with where the kart is now (projected on its window by
    /// [`Course::project`] with the current `passed`). `race_ticks`: the race clock if the race
    /// has started (checkpoints count from GO only), `None` before. `can_validate`: false while
    /// the tow pelican carries the kart.
    pub fn update(
        &mut self,
        course: &Course,
        projection: &Projection,
        race_ticks: Option<u32>,
        can_validate: bool,
    ) -> Option<ProgressEvent> {
        if self.finished() {
            return None;
        }
        let next = course.checkpoint_distance(self.passed as i64);
        let crossed = projection.distance >= next && course.inside_gate(projection) && can_validate;
        let Some(tick) = race_ticks.filter(|_| crossed) else {
            self.distance = clamp_to_window(course, self.passed, projection.distance);
            return None;
        };

        // Validated: global checkpoint `self.passed` (one per tick at most: they are about
        // 25 m apart).
        let validated = self.passed;
        self.passed += 1;
        let n = course.checkpoint_count();
        let event = if validated > 0 && validated.is_multiple_of(n) {
            self.lap_times.push(tick - self.lap_start);
            self.lap_start = tick;
            if self.passed >= course.checkpoints_to_finish() {
                self.finish = Some(tick);
                ProgressEvent::Finished
            } else {
                ProgressEvent::Lap(validated / n)
            }
        } else {
            ProgressEvent::Checkpoint
        };
        self.distance = if self.finished() {
            projection.distance
        } else {
            clamp_to_window(course, self.passed, projection.distance)
        };
        Some(event)
    }
}

fn clamp_to_window(course: &Course, passed: u32, distance: f32) -> f32 {
    let (start, end) = course.window(passed);
    distance.clamp(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::course::tests::{at, prairie};

    /// A kart driven along the centerline by moving its position directly.
    struct Driver<'a> {
        course: &'a Course,
        progress: RaceProgress,
        tick: u32,
        /// Every event with its tick.
        log: Vec<(u32, ProgressEvent)>,
    }

    impl<'a> Driver<'a> {
        fn on_grid(course: &'a Course, slot: usize) -> Self {
            Self {
                course,
                progress: RaceProgress::on_grid(course, course.grid[slot].translation, 3),
                tick: 0,
                log: Vec::new(),
            }
        }

        /// One tick at `position`; returns the event.
        fn step(&mut self, position: Vec3) -> Option<ProgressEvent> {
            self.tick += 1;
            let projection = self.course.project(self.progress.passed, position);
            let event = self
                .progress
                .update(self.course, &projection, Some(self.tick), true);
            if let Some(event) = event {
                self.log.push((self.tick, event));
            }
            event
        }

        /// Tick of the first `event`.
        fn tick_of(&self, event: ProgressEvent) -> u32 {
            self.log.iter().find(|(_, e)| *e == event).unwrap().0
        }

        /// Drives from race distance `from` to `to` by `step` meters per tick, on the centerline
        /// shifted by `lateral`. Returns every event.
        fn drive(&mut self, from: f32, to: f32, step: f32, lateral: f32) -> Vec<ProgressEvent> {
            let ticks = ((to - from).abs() / step).ceil() as usize;
            (1..=ticks)
                .filter_map(|i| {
                    let d = from + (to - from) * i as f32 / ticks as f32;
                    self.step(at(self.course, d, lateral))
                })
                .collect()
        }
    }

    #[test]
    fn checkpoints_in_order_make_laps_then_the_finish() {
        let (_, course) = prairie();
        let n = course.checkpoint_count();
        let length = course.length();
        let mut kart = Driver::on_grid(&course, 0);
        assert_eq!(kart.progress.passed, 0);
        assert_eq!(kart.progress.current_lap(&course), 1);
        assert!((kart.progress.distance + 5.0).abs() < 0.3);

        // Crossing the start line validates checkpoint 0 (no lap yet).
        let events = kart.drive(-5.0, 10.0, 0.45, 0.0);
        assert_eq!(events, vec![ProgressEvent::Checkpoint]);
        assert_eq!(kart.progress.passed, 1);

        // One lap: every other checkpoint, then the line again.
        let events = kart.drive(10.0, length + 10.0, 0.45, 0.0);
        assert_eq!(events.len(), n as usize);
        assert_eq!(events.last(), Some(&ProgressEvent::Lap(1)));
        assert_eq!(kart.progress.laps_completed(&course), 1);
        assert_eq!(kart.progress.current_lap(&course), 2);
        assert_eq!(kart.progress.lap_times.len(), 1);
        assert!((kart.progress.distance - (length + 10.0)).abs() < 0.3);

        let events = kart.drive(length + 10.0, 2.0 * length + 10.0, 0.45, 0.0);
        assert_eq!(events.last(), Some(&ProgressEvent::Lap(2)));
        assert_eq!(kart.progress.current_lap(&course), 3);
        let events = kart.drive(2.0 * length + 10.0, 3.0 * length + 3.0, 0.45, 0.0);
        assert_eq!(events.last(), Some(&ProgressEvent::Finished));
        assert!(kart.progress.finished());
        assert_eq!(kart.progress.current_lap(&course), 3);
        assert_eq!(kart.progress.passed, course.checkpoints_to_finish());

        // Frozen after the finish.
        let finish = kart.progress.clone();
        assert!(
            kart.drive(3.0 * length + 3.0, 3.0 * length + 40.0, 0.45, 0.0)
                .is_empty()
        );
        assert_eq!(kart.progress, finish);
    }

    #[test]
    fn lap_times_best_lap_and_total_are_counted_in_ticks() {
        let (_, course) = prairie();
        let length = course.length();
        let mut kart = Driver::on_grid(&course, 0);
        // Lap 1 at 0.45 m/tick (from the grid, GO at tick 0), lap 2 faster, lap 3 slower.
        kart.drive(-5.0, length + 1.0, 0.45, 0.0);
        kart.drive(length + 1.0, 2.0 * length + 1.0, 0.5, 0.0);
        kart.drive(2.0 * length + 1.0, 3.0 * length + 1.0, 0.4, 0.0);
        let (end1, end2) = (
            kart.tick_of(ProgressEvent::Lap(1)),
            kart.tick_of(ProgressEvent::Lap(2)),
        );
        let end3 = kart.tick_of(ProgressEvent::Finished);
        let (lap1, lap2, lap3) = (end1, end2 - end1, end3 - end2);
        // About (length + 5) / 0.45, length / 0.5 and length / 0.4 ticks.
        assert!((lap1 as f32 - (length + 5.0) / 0.45).abs() < 3.0, "{lap1}");
        assert!((lap2 as f32 - length / 0.5).abs() < 3.0, "{lap2}");
        assert!((lap3 as f32 - length / 0.4).abs() < 3.0, "{lap3}");
        let p = &kart.progress;
        assert_eq!(p.lap_times, vec![lap1, lap2, lap3]);
        assert_eq!(p.finish, Some(end3));
        assert_eq!(p.time(99_999), end3, "the clock stops at the finish");
        assert_eq!(p.lap_times.iter().sum::<u32>(), p.finish.unwrap());
        assert_eq!(p.best_lap(), Some(lap2));
        assert!(lap2 < lap1 && lap2 < lap3);
    }

    #[test]
    fn nothing_counts_before_go() {
        let (_, course) = prairie();
        let mut progress = RaceProgress::on_grid(&course, course.grid[0].translation, 3);
        let past_line = course.project(0, at(&course, 2.0, 0.0));
        assert_eq!(progress.update(&course, &past_line, None, true), None);
        assert_eq!(progress.passed, 0);
        // Nor while the tow pelican carries the kart.
        assert_eq!(progress.update(&course, &past_line, Some(5), false), None);
        assert_eq!(progress.passed, 0);
        assert_eq!(
            progress.update(&course, &past_line, Some(6), true),
            Some(ProgressEvent::Checkpoint)
        );
    }

    #[test]
    fn a_shortcut_is_refused() {
        let (_, course) = prairie();
        let length = course.length();
        let mut kart = Driver::on_grid(&course, 0);
        kart.drive(-5.0, 30.0, 0.45, 0.0);
        let passed = kart.progress.passed;
        let next = course.checkpoint_distance(passed as i64);

        // Teleport (or fly over the infield) to the back straight, 150 m further: nothing is
        // validated and the race distance stays before the next checkpoint.
        for d in 0..60 {
            let event = kart.step(at(&course, 180.0 + d as f32 * 0.45, 0.0));
            assert_eq!(event, None, "a checkpoint was validated off its stretch");
            assert!(kart.progress.distance <= next);
        }
        assert_eq!(kart.progress.passed, passed);

        // Crossing the next checkpoint's plane outside the walls (cutting through the grass
        // beside it) does not count either.
        let beside = course.project(passed, at(&course, next + 1.0, 25.0));
        assert!(!course.inside_gate(&beside));
        assert_eq!(kart.step(at(&course, next + 1.0, 25.0)), None);

        // A whole "lap" that skips one stretch is not a lap: the line is not validated.
        let mut kart = Driver::on_grid(&course, 0);
        kart.drive(-5.0, 100.0, 0.45, 0.0);
        let skipped_from = kart.progress.passed;
        kart.drive(160.0, length + 5.0, 0.45, 0.0);
        assert_eq!(kart.progress.passed, skipped_from);
        assert_eq!(kart.progress.laps_completed(&course), 0);
        // Going back to the missed checkpoint and driving on from there works again.
        kart.drive(90.0, length + 5.0, 0.45, 0.0);
        assert_eq!(kart.progress.laps_completed(&course), 1);
    }

    #[test]
    fn driving_back_lowers_the_distance_but_keeps_the_checkpoints() {
        let (_, course) = prairie();
        let length = course.length();
        let mut kart = Driver::on_grid(&course, 0);
        kart.drive(-5.0, 60.0, 0.45, 0.0);
        let passed = kart.progress.passed;
        let last = course.checkpoint_distance(passed as i64 - 1);
        let before = kart.progress.distance;

        // Reverse past the last checkpoint: the distance goes down, nothing is un-validated.
        kart.drive(60.0, last - 10.0, 0.2, 0.0);
        assert!(kart.progress.distance < before - 15.0);
        assert!((kart.progress.distance - (last - 10.0)).abs() < 0.3);
        assert_eq!(kart.progress.passed, passed);
        // Further back than the window: the distance stops going down.
        let (start, _) = course.window(passed);
        kart.drive(last - 10.0, start - 20.0, 0.2, 0.0);
        assert!((kart.progress.distance - start).abs() < 1e-3);
        // Forward again across the same checkpoint: it does not count twice.
        let events = kart.drive(start - 20.0, 60.0, 0.45, 0.0);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(kart.progress.passed, passed);

        // Back and forth over the start/finish line does not count laps.
        let mut kart = Driver::on_grid(&course, 0);
        kart.drive(-5.0, length + 5.0, 0.45, 0.0);
        assert_eq!(kart.progress.laps_completed(&course), 1);
        for _ in 0..3 {
            kart.drive(length + 5.0, length - 5.0, 0.3, 0.0);
            kart.drive(length - 5.0, length + 5.0, 0.3, 0.0);
        }
        assert_eq!(kart.progress.laps_completed(&course), 1);
        assert_eq!(kart.progress.lap_times.len(), 1);
    }

    #[test]
    fn progress_serializes() {
        let (_, course) = prairie();
        let mut kart = Driver::on_grid(&course, 3);
        kart.drive(-14.0, 300.0, 0.45, 1.0);
        let text = ron::to_string(&kart.progress).unwrap();
        assert_eq!(ron::from_str::<RaceProgress>(&text).unwrap(), kart.progress);
    }
}
