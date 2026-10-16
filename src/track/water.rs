//! Lakes and gaps (GD § 10, issue #20), pure geometry (no ECS but the [`Lake`] component).
//!
//! - **A lake** is a rectangle of the ground plane, dug down to its floor and full of water up to
//!   its level, just below the ground: the road may dip into it, under the water. A kart whose
//!   center is under the water is under water (`crate::kart::KartEnvironment`). The ground
//!   around it is built in boxes ([`ground_cells`]) that leave its basin open.
//! - **A gap** is a stretch of the centerline with no road: the karts glide over it, from the lip
//!   of a glide ramp to the landing. The centerline goes on through it (checkpoints, the bots'
//!   line); the race rules know it (`crate::race::Course`).

use bevy::prelude::*;

use super::data::{GapData, LakeData};
use super::layout::CenterLine;
use super::mesh::GROUND_HEIGHT;

/// A lake of the circuit (issue #20), on the entity of its water. `min` and `max` are the corners
/// of its rectangle on the ground plane (x, z).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Lake {
    pub min: Vec2,
    pub max: Vec2,
    /// Height of the water's surface (m).
    pub level: f32,
    /// Height of the basin's floor (m).
    pub floor: f32,
}

impl Lake {
    pub fn new(data: &LakeData) -> Self {
        let center = Vec2::new(data.center.0, data.center.1);
        let half = Vec2::new(data.size.0, data.size.1) / 2.0;
        Self {
            min: center - half,
            max: center + half,
            level: data.level,
            floor: data.floor,
        }
    }

    /// Whether the lake's rectangle covers `p` (ground plane).
    pub fn covers(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// How deep `point` is under the water (m; negative above the surface), if the lake covers it.
    pub fn depth(&self, point: Vec3) -> Option<f32> {
        self.covers(point.xz()).then_some(self.level - point.y)
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) / 2.0
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }
}

/// How deep `point` is in the deepest of `lakes` that covers it (m; negative above the surface),
/// if one does.
pub fn water_depth<'a>(lakes: impl IntoIterator<Item = &'a Lake>, point: Vec3) -> Option<f32> {
    lakes
        .into_iter()
        .filter_map(|lake| lake.depth(point))
        .reduce(f32::max)
}

/// The height of the ground at `p` (ground plane): the floor of a lake there, the ground
/// elsewhere.
pub fn ground_height(lakes: &[Lake], p: Vec2) -> f32 {
    lakes
        .iter()
        .find(|lake| lake.covers(p))
        .map_or(GROUND_HEIGHT, |lake| lake.floor)
}

/// The ground between `min` and `max` (ground plane) around the lakes, as rectangles that never
/// overlap a lake nor each other and cover everything else: a grid on every edge, the cells out
/// of the lakes, merged along each row. Without a lake, the whole rectangle.
pub fn ground_cells(min: Vec2, max: Vec2, lakes: &[Lake]) -> Vec<(Vec2, Vec2)> {
    let mut xs = vec![min.x, max.x];
    let mut zs = vec![min.y, max.y];
    for lake in lakes {
        xs.extend([lake.min.x, lake.max.x].map(|x| x.clamp(min.x, max.x)));
        zs.extend([lake.min.y, lake.max.y].map(|z| z.clamp(min.y, max.y)));
    }
    let sort = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    };
    sort(&mut xs);
    sort(&mut zs);
    let mut cells = Vec::new();
    for row in zs.windows(2) {
        let mut open: Option<f32> = None;
        for column in xs.windows(2) {
            let middle = Vec2::new((column[0] + column[1]) / 2.0, (row[0] + row[1]) / 2.0);
            let dry = !lakes.iter().any(|lake| lake.covers(middle));
            match (dry, open) {
                (true, None) => open = Some(column[0]),
                (false, Some(start)) => {
                    cells.push((Vec2::new(start, row[0]), Vec2::new(column[0], row[1])));
                    open = None;
                }
                _ => {}
            }
        }
        if let Some(start) = open {
            cells.push((
                Vec2::new(start, row[0]),
                Vec2::new(*xs.last().unwrap(), row[1]),
            ));
        }
    }
    cells
}

/// A stretch with no road (issue #20, `TrackData::gaps`), along the centerline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    /// Centerline distances of its start (the lip of the glide ramp) and its end (the landing);
    /// `to` is below `from` when the gap crosses the start line.
    pub from: f32,
    pub to: f32,
    /// The lower of the road's heights at both ends (m): a kart that falls in the gap falls
    /// below it.
    pub low: f32,
}

impl Gap {
    pub fn new(center: &CenterLine, data: &GapData) -> Self {
        let from = center.wrap(center.param_to_distance(data.from));
        let to = center.wrap(center.param_to_distance(data.to));
        let low = center.at(from).position.y.min(center.at(to).position.y);
        Self { from, to, low }
    }

    /// Its length along the centerline (m).
    pub fn length(&self, center: &CenterLine) -> f32 {
        center.wrap(self.to - self.from)
    }

    /// Whether the centerline distance `distance` (any lap) is in the gap.
    pub fn contains(&self, center: &CenterLine, distance: f32) -> bool {
        center.wrap(distance - self.from) < self.length(center)
    }
}

/// The gap of `gaps` that `distance` is in, if any.
pub fn gap_at<'a>(gaps: &'a [Gap], center: &CenterLine, distance: f32) -> Option<&'a Gap> {
    gaps.iter().find(|gap| gap.contains(center, distance))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lake() -> Lake {
        Lake::new(&LakeData {
            center: (10.0, -20.0),
            size: (40.0, 20.0),
            level: -0.4,
            floor: -12.0,
        })
    }

    /// A kart is under water when its center is below the surface, over the lake only.
    #[test]
    fn a_lake_holds_its_water_over_its_rectangle() {
        let lake = lake();
        assert_eq!(lake.min, Vec2::new(-10.0, -30.0));
        assert_eq!(lake.max, Vec2::new(30.0, -10.0));
        assert_eq!(lake.center(), Vec2::new(10.0, -20.0));
        assert_eq!(lake.size(), Vec2::new(40.0, 20.0));
        assert!((lake.depth(Vec3::new(0.0, -5.4, -20.0)).unwrap() - 5.0).abs() < 1e-5);
        assert!(lake.depth(Vec3::new(0.0, 2.0, -20.0)).unwrap() < 0.0);
        assert_eq!(lake.depth(Vec3::new(31.0, -5.0, -20.0)), None);
        assert_eq!(lake.depth(Vec3::new(0.0, -5.0, -9.0)), None);
        assert_eq!(water_depth(&[lake], Vec3::new(50.0, -5.0, 0.0)), None);
        assert_eq!(water_depth(&[lake], Vec3::new(0.0, -1.4, -15.0)), Some(1.0));
        assert_eq!(ground_height(&[lake], Vec2::new(0.0, -15.0)), -12.0);
        assert_eq!(ground_height(&[lake], Vec2::new(0.0, 15.0)), GROUND_HEIGHT);
    }

    /// The ground around the lakes leaves their basins open and covers all the rest, without
    /// overlaps: a regular grid of points falls in exactly one cell or in a lake.
    #[test]
    fn the_ground_leaves_the_basins_open() {
        let (min, max) = (Vec2::new(-100.0, -80.0), Vec2::new(120.0, 60.0));
        let other = Lake::new(&LakeData {
            center: (80.0, 30.0),
            size: (30.0, 50.0),
            level: -0.4,
            floor: -6.0,
        });
        for lakes in [vec![], vec![lake()], vec![lake(), other]] {
            let cells = ground_cells(min, max, &lakes);
            assert!(!cells.is_empty());
            if lakes.is_empty() {
                assert_eq!(cells, vec![(min, max)]);
            }
            for i in 0..=43 {
                for j in 0..=27 {
                    let p = Vec2::new(-99.5 + i as f32 * 5.0, -79.5 + j as f32 * 5.0);
                    let wet = lakes.iter().any(|l| l.covers(p));
                    let inside = cells
                        .iter()
                        .filter(|(a, b)| p.x > a.x && p.x < b.x && p.y > a.y && p.y < b.y)
                        .count();
                    assert_eq!(inside, usize::from(!wet), "{p} with {} lakes", lakes.len());
                }
            }
        }
        // One lake in the middle: the rows above and below in one piece, two beside it.
        assert_eq!(ground_cells(min, max, &[lake()]).len(), 4);
    }
}
