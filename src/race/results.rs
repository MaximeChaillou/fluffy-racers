//! The end of the results (issue #70), pure: no ECS, unit-tested.
//!
//! The results screen follows the racers still on track once the race is over (the race ends
//! when the players finished): each one's time comes in when it crosses the line. Then, like in
//! the karting games of reference, the table closes `RaceTuning::results_close_seconds` after
//! the screen appeared ([`super::RaceFlow::results_closed`]): every racer still on track gets an
//! **estimated** time, its average race speed so far over the distance it has left
//! ([`estimate_finish`]), and the table is final ([`close_results`]). The speed is its race
//! distance (from the start line) over the race clock: at the same moment, the further a racer
//! got, the sooner it finishes, so the estimates follow the ranking.
//!
//! Online, the host closes it with the same functions and sends its table: every screen shows
//! the host's lines (`net`, `docs/ARCHITECTURE.md` § « Course en ligne »).

use super::course::Course;
use super::progress::RaceProgress;
use crate::TICK_HZ;

/// A racer's time in the closed results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultTime {
    /// Crossed the line: its race time (ticks).
    Finished(u32),
    /// Still on track when the results closed: its estimated race time (ticks).
    Estimated(u32),
    /// Gave up (online: its player left the room before the finish): no time.
    GaveUp,
}

impl ResultTime {
    /// Its race time (ticks), if it has one.
    pub fn ticks(self) -> Option<u32> {
        match self {
            Self::Finished(ticks) | Self::Estimated(ticks) => Some(ticks),
            Self::GaveUp => None,
        }
    }
}

/// A line of the closed results: who, and its time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultLine<Id> {
    pub id: Id,
    pub time: ResultTime,
}

/// A racer as the closing of the results sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing<Id> {
    pub id: Id,
    /// Race tick of its finish, if it finished.
    pub finish: Option<u32>,
    /// It gave up (online: its player left): no time.
    pub gave_up: bool,
    /// Race distance driven (m): from the start line, along the centerline (negative behind
    /// it).
    pub covered: f32,
    /// Distance left to the finish line (m).
    pub remaining: f32,
}

impl<Id> Standing<Id> {
    /// The racer `id`, with this progress.
    pub fn of(id: Id, progress: &RaceProgress, course: &Course, gave_up: bool) -> Self {
        Self {
            id,
            finish: progress.finish,
            gave_up,
            covered: progress.distance,
            remaining: course.finish_distance() - progress.distance,
        }
    }
}

/// The race tick a racer still on track would finish on, at race tick `now`, if it drove the
/// `remaining` meters left at its average race speed so far (`covered` meters in `now` ticks),
/// and at least `min_speed` (m/s: a racer that barely moved still gets a time). Never before
/// `now`.
pub fn estimate_finish(now: u32, covered: f32, remaining: f32, min_speed: f32) -> u32 {
    let remaining = f64::from(remaining.max(0.0));
    // Ticks left at the minimum speed, and at the average speed: the faster of the two.
    let at_min_speed = remaining * TICK_HZ / f64::from(min_speed.max(0.01));
    let at_average = if covered > 0.0 && now > 0 {
        remaining * f64::from(now) / f64::from(covered)
    } else {
        f64::INFINITY
    };
    // Rounded up to a whole tick (a hair of float error aside).
    let left = (at_min_speed.min(at_average) - 1e-6).ceil().max(0.0);
    (f64::from(now) + left).min(f64::from(u32::MAX)) as u32
}

/// Closes the results at race tick `now`: `standings` from first to last (the race's ranking:
/// the finishers by their finish, then the others by how far they got; online the host's order,
/// those who gave up last). The finishers keep their time, those who gave up have none, the
/// others get their estimate ([`estimate_finish`]), never before the line above: the table reads
/// in order. Written into `out` (emptied first).
pub fn close_results<Id>(
    standings: impl IntoIterator<Item = Standing<Id>>,
    now: u32,
    min_speed: f32,
    out: &mut Vec<ResultLine<Id>>,
) {
    out.clear();
    let mut floor = 0;
    for standing in standings {
        let time = match standing.finish {
            Some(finish) => ResultTime::Finished(finish),
            None if standing.gave_up => ResultTime::GaveUp,
            None => ResultTime::Estimated(
                estimate_finish(now, standing.covered, standing.remaining, min_speed).max(floor),
            ),
        };
        if let Some(ticks) = time.ticks() {
            floor = floor.max(ticks);
        }
        out.push(ResultLine {
            id: standing.id,
            time,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::course::tests::{at, prairie};

    const MIN_SPEED: f32 = 5.0;

    fn racing(id: u32, covered: f32, remaining: f32) -> Standing<u32> {
        Standing {
            id,
            finish: None,
            gave_up: false,
            covered,
            remaining,
        }
    }

    /// The rest of the race at the average speed so far: halfway after 60 s, 120 s in all; at
    /// three quarters, 80 s; at the line, now.
    #[test]
    fn the_estimate_is_the_average_speed_over_what_is_left() {
        let minute = 3_600;
        assert_eq!(estimate_finish(minute, 600.0, 600.0, MIN_SPEED), 2 * minute);
        assert_eq!(estimate_finish(minute, 900.0, 300.0, MIN_SPEED), 4_800);
        assert_eq!(estimate_finish(minute, 1200.0, 0.0, MIN_SPEED), minute);
        // Rounded up to the next tick: never before it would have finished.
        assert_eq!(estimate_finish(3, 10.0, 1.0, MIN_SPEED), 4);
        // A racer that barely moved (or is still behind the start line) drives the rest at the
        // minimum speed: 5 m/s, 600 m in 120 s.
        for covered in [0.0, -3.0, 1.0] {
            assert_eq!(
                estimate_finish(minute, covered, 600.0, MIN_SPEED),
                minute + 120 * 60
            );
        }
        assert_eq!(estimate_finish(0, 0.0, 50.0, MIN_SPEED), 600);
        // Never past what a tick count can hold.
        assert_eq!(estimate_finish(u32::MAX - 5, 1.0, 1e9, 0.0), u32::MAX);
    }

    /// The finishers keep their time, the others get their estimate in the order of the ranking
    /// (never before the line above), those who gave up have no time.
    #[test]
    fn closing_keeps_the_finishers_and_estimates_the_others() {
        let now = 4_000;
        let standings = [
            Standing {
                finish: Some(3_100),
                ..racing(7, 1500.0, 0.0)
            },
            Standing {
                finish: Some(3_700),
                ..racing(2, 1500.0, 0.0)
            },
            // 1400 m in 4000 ticks: 100 m more in 286 ticks.
            racing(4, 1400.0, 100.0),
            // Ranked behind by its checkpoints (online), but with a little more distance: its
            // estimate would be before the line above; it takes that line's time.
            racing(9, 1401.0, 99.0),
            racing(5, 700.0, 800.0),
            Standing {
                gave_up: true,
                ..racing(11, 900.0, 600.0)
            },
        ];
        let mut out = Vec::with_capacity(12);
        close_results(standings, now, MIN_SPEED, &mut out);
        let times: Vec<(u32, ResultTime)> = out.iter().map(|l| (l.id, l.time)).collect();
        let expected_4 = now + (100.0f64 * 4_000.0 / 1_400.0).ceil() as u32;
        assert_eq!(expected_4, 4_286);
        assert_eq!(
            times,
            vec![
                (7, ResultTime::Finished(3_100)),
                (2, ResultTime::Finished(3_700)),
                (4, ResultTime::Estimated(expected_4)),
                (9, ResultTime::Estimated(expected_4)),
                (5, ResultTime::Estimated(now + 4_572)),
                (11, ResultTime::GaveUp),
            ]
        );
        // Every estimate comes after now and after every finisher.
        let ticks: Vec<u32> = out.iter().filter_map(|l| l.time.ticks()).collect();
        assert!(ticks.windows(2).all(|w| w[0] <= w[1]), "{ticks:?}");
        // The buffer is reused.
        let capacity = out.capacity();
        close_results([racing(1, 10.0, 10.0)], 60, MIN_SPEED, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out.capacity(), capacity);
    }

    /// A racer's standing: its race distance (behind the line on the grid), and what is left to
    /// the finish line.
    #[test]
    fn a_standing_measures_the_race_distance_to_the_line() {
        let (_, course) = prairie();
        let mut progress = RaceProgress::on_grid(&course, course.grid[5].translation, 3);
        let standing = Standing::of(3u32, &progress, &course, false);
        assert!(standing.covered < -10.0, "{}", standing.covered);
        assert!((standing.remaining - (course.finish_distance() - standing.covered)).abs() < 1e-3);
        progress.validate(&course, 40);
        let ahead = course.project(progress.passed, at(&course, 10.0, 0.0));
        progress.follow(&course, &ahead);
        let standing = Standing::of(3u32, &progress, &course, true);
        assert!((standing.covered - 10.0).abs() < 0.3);
        assert!((standing.remaining - (course.finish_distance() - 10.0)).abs() < 0.3);
        assert!(standing.gave_up);
        assert_eq!(standing.finish, None);
    }

    /// At the same moment, the further a racer got, the sooner its estimate: racers ranked by
    /// their race distance get times in the same order, each its own (never the line above's).
    #[test]
    fn the_estimates_follow_the_race_distance() {
        let now = 3_000;
        let distances = [1180.0, 1150.0, 1149.5, 900.0, 300.0, 40.0, 2.0, -20.0];
        let standings = distances
            .iter()
            .enumerate()
            .map(|(id, d)| racing(id as u32, *d, 1200.0 - d));
        let mut out = Vec::new();
        close_results(standings, now, MIN_SPEED, &mut out);
        let ticks: Vec<u32> = out.iter().map(|line| line.time.ticks().unwrap()).collect();
        assert!(ticks.windows(2).all(|w| w[0] < w[1]), "{ticks:?}");
        for (line, d) in out.iter().zip(distances) {
            let own = estimate_finish(now, d, 1200.0 - d, MIN_SPEED);
            assert_eq!(line.time, ResultTime::Estimated(own));
        }
    }
}
