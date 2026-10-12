//! Live ranking and distance to the leader (pure, no ECS, no allocation: it sorts a slice the
//! caller keeps between ticks).
//!
//! Finished racers come first, in order of finish; then the others by race distance (laps
//! included), so the ranking changes the moment a kart overtakes another. Ties go to the better
//! grid slot, so the order is always the same for the same inputs (deterministic: the host will
//! send it to the other peers).

/// One racer as the ranking sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RankEntry<Id> {
    pub id: Id,
    /// Grid slot (0 = pole): breaks ties.
    pub grid_slot: usize,
    /// Race distance (m), see `course.rs`.
    pub distance: f32,
    /// Race tick of the finish, if finished.
    pub finish: Option<u32>,
}

/// Sorts `entries` from first to last.
pub fn rank<Id>(entries: &mut [RankEntry<Id>]) {
    entries.sort_unstable_by(|a, b| {
        let by_finish = match (a.finish, b.finish) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        // Finished on the same tick: whoever went further crossed the line first.
        by_finish
            .then(b.distance.total_cmp(&a.distance))
            .then(a.grid_slot.cmp(&b.grid_slot))
    });
}

/// Distance to the leader along the centerline, laps included (m): what the item distribution
/// will use (GD § 8: "la distance au leader, pas la position"). 0 for the leader itself.
pub fn distance_to_leader(leader_distance: f32, distance: f32) -> f32 {
    (leader_distance - distance).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAP: f32 = 500.0;

    fn entry(id: u32, distance: f32) -> RankEntry<u32> {
        RankEntry {
            id,
            grid_slot: id as usize,
            distance,
            finish: None,
        }
    }

    fn order(entries: &[RankEntry<u32>]) -> Vec<u32> {
        entries.iter().map(|e| e.id).collect()
    }

    /// The 12 karts on the grid: pole first, 3 m apart.
    fn grid() -> Vec<RankEntry<u32>> {
        (0..12).map(|i| entry(i, -5.0 - 3.0 * i as f32)).collect()
    }

    #[test]
    fn twelve_karts_on_the_grid_are_ranked_by_slot() {
        let mut entries = grid();
        entries.reverse();
        rank(&mut entries);
        assert_eq!(order(&entries), (0..12).collect::<Vec<_>>());
        let leader = entries[0].distance;
        let gaps: Vec<f32> = entries
            .iter()
            .map(|e| distance_to_leader(leader, e.distance))
            .collect();
        assert_eq!(gaps[0], 0.0);
        assert_eq!(gaps[11], 33.0);
    }

    #[test]
    fn overtakes_change_the_ranking_at_once() {
        let mut entries = grid();
        // Kart 7 drives faster than everyone: after a while it is ahead of kart 0.
        for tick in 0..400 {
            for e in entries.iter_mut() {
                let speed = if e.id == 7 { 0.5 } else { 0.42 };
                e.distance += speed;
            }
            rank(&mut entries);
            let pos7 = entries.iter().position(|e| e.id == 7).unwrap();
            // Kart 7 started 21 m behind kart 0 and gains 0.08 m per tick.
            let expected_ahead_of_0 = (tick + 1) as f32 * 0.08 > 21.0;
            let pos0 = entries.iter().position(|e| e.id == 0).unwrap();
            assert_eq!(pos7 < pos0, expected_ahead_of_0, "tick {tick}");
        }
        assert_eq!(entries[0].id, 7);
        assert_eq!(
            order(&entries[1..]),
            vec![0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11]
        );
    }

    #[test]
    fn a_kart_driving_back_loses_places() {
        let mut entries: Vec<_> = (0..12).map(|i| entry(i, 300.0 - 4.0 * i as f32)).collect();
        // Kart 2 spins and reverses 10 m: karts 3 and 4 pass it.
        entries[2].distance -= 10.0;
        rank(&mut entries);
        assert_eq!(order(&entries[..6]), vec![0, 1, 3, 4, 2, 5]);
    }

    #[test]
    fn a_kart_that_cut_gains_nothing() {
        // The progress clamps a shortcut's distance to its next checkpoint (progress.rs): the
        // ranking only sees that distance.
        let next_checkpoint = 102.0;
        let mut entries = vec![entry(0, 110.0), entry(1, 104.0), entry(2, next_checkpoint)];
        rank(&mut entries);
        assert_eq!(order(&entries), vec![0, 1, 2]);
    }

    #[test]
    fn laps_count_in_the_distance_and_across_the_finish_line() {
        // Kart 0 just crossed the line (lap 2), kart 1 is 3 m before it (end of lap 1), kart 2
        // was lapped.
        let mut entries = vec![entry(2, 2.0), entry(1, LAP - 3.0), entry(0, LAP + 2.0)];
        rank(&mut entries);
        assert_eq!(order(&entries), vec![0, 1, 2]);
        let leader = entries[0].distance;
        assert_eq!(distance_to_leader(leader, entries[1].distance), 5.0);
        assert_eq!(distance_to_leader(leader, entries[2].distance), LAP);
        assert_eq!(distance_to_leader(leader, leader), 0.0);
        // Never negative.
        assert_eq!(distance_to_leader(10.0, 12.0), 0.0);
    }

    #[test]
    fn finishers_come_first_in_order_of_finish() {
        let finish = 3.0 * LAP;
        let mut entries = vec![
            entry(0, finish - 1.0),
            // Finished later but went further after the line: still behind kart 3.
            RankEntry {
                finish: Some(3_000),
                ..entry(1, finish + 40.0)
            },
            RankEntry {
                finish: Some(2_950),
                ..entry(3, finish + 0.2)
            },
            // Same tick as kart 1, a little further past the line: crossed first.
            RankEntry {
                finish: Some(3_000),
                ..entry(4, finish + 40.3)
            },
            entry(5, finish - 30.0),
        ];
        rank(&mut entries);
        assert_eq!(order(&entries), vec![3, 4, 1, 0, 5]);
    }

    #[test]
    fn ties_go_to_the_better_grid_slot() {
        let mut entries = vec![entry(5, 50.0), entry(2, 50.0), entry(9, 50.0)];
        rank(&mut entries);
        assert_eq!(order(&entries), vec![2, 5, 9]);
    }
}
