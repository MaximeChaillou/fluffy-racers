//! Anti-gravity zones (GD § 10, issue #78), pure geometry (no ECS but the [`AntiGravity`] marker).
//!
//! A zone is a stretch of the centerline (`anti_gravity` in the circuit's file, by spline
//! parameters) where the road may twist up to a wall: there, the karts' gravity follows the road's
//! normal under their wheels (`crate::kart::KartEnvironment::anti_gravity`), their wheels pivot,
//! and two karts bumping into each other both get a boost (the spin boost). The road of the zone
//! (and its shoulders) is a collider of its own, marked [`AntiGravity`]: a wheel on it puts the
//! kart in anti-gravity, a wheel on the road beyond takes it out. A zone starts and ends where the
//! road is still flat enough to drive without it (the file's validation: beyond 45° of banking,
//! the road must be in a zone).
//!
//! The items find the road's normal of a zone ([`AntiGravityZone::up_near`]) to roll on it, drop
//! on it, bounce on it; everywhere else, the world's up.

use bevy::prelude::*;

use super::data::AntiGravityData;
use super::layout::CenterLine;
use super::mesh::{WALL_THICKNESS, wall_offset};

/// On the colliders of the anti-gravity zones' road and shoulders (issue #78): a kart whose wheel
/// touches one drives with the road's normal as its up.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AntiGravity;

/// How far around the zone's road its box reaches (m): a kart or an item over the road, in the
/// air of a jump, is still in it.
const BOX_MARGIN: f32 = 6.0;

/// An anti-gravity zone of the circuit (issue #78).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AntiGravityZone {
    /// Centerline distances of its start and its end; `to` is below `from` when the zone crosses
    /// the start line.
    pub from: f32,
    pub to: f32,
    /// A box around its road (world): out of it, nothing is in the zone.
    pub min: Vec3,
    pub max: Vec3,
}

impl AntiGravityZone {
    pub fn new(center: &CenterLine, data: &AntiGravityData) -> Self {
        let from = center.wrap(center.param_to_distance(data.from));
        let to = center.wrap(center.param_to_distance(data.to));
        let mut zone = Self {
            from,
            to,
            min: Vec3::splat(f32::MAX),
            max: Vec3::splat(f32::MIN),
        };
        let length = zone.length(center);
        let step = center.length() / center.samples().len() as f32;
        let count = (length / step).ceil().max(1.0) as usize;
        for k in 0..=count {
            let s = center.at(from + length * k as f32 / count as f32);
            let reach = wall_offset(&s) + WALL_THICKNESS;
            for p in [s.point(-reach, 0.0), s.point(reach, 0.0)] {
                zone.min = zone.min.min(p);
                zone.max = zone.max.max(p);
            }
        }
        zone.min -= Vec3::splat(BOX_MARGIN);
        zone.max += Vec3::splat(BOX_MARGIN);
        zone
    }

    /// Its length along the centerline (m).
    pub fn length(&self, center: &CenterLine) -> f32 {
        center.wrap(self.to - self.from)
    }

    /// Whether the centerline distance `distance` (any lap) is in the zone.
    pub fn contains(&self, center: &CenterLine, distance: f32) -> bool {
        center.wrap(distance - self.from) < self.length(center)
    }

    /// The road's normal near `position`, if it is over this zone's road: the normal of the
    /// centerline at the closest point of the zone, within the reach of its walls (and a jump
    /// above them). A box test first: far from the zone, a few comparisons.
    pub fn up_near(&self, center: &CenterLine, position: Vec3) -> Option<Vec3> {
        if position.cmplt(self.min).any() || position.cmpgt(self.max).any() {
            return None;
        }
        let samples = center.samples();
        let count = samples.len() as i64;
        let step = center.length() / count as f32;
        let first = (self.from / step).floor() as i64;
        let last = first + (self.length(center) / step).ceil() as i64;
        let mut best = (f32::MAX, first);
        for k in first..=last {
            let d = samples[k.rem_euclid(count) as usize]
                .position
                .distance_squared(position);
            if d < best.0 {
                best = (d, k);
            }
        }
        let s = &samples[best.1.rem_euclid(count) as usize];
        let reach = wall_offset(s) + WALL_THICKNESS + BOX_MARGIN;
        let along = (position - s.position).dot(s.forward);
        let at = center.at(s.distance + along);
        (best.0 <= reach * reach && self.contains(center, at.distance)).then_some(at.up)
    }
}

/// The zone of `zones` that `distance` is in, if any.
pub fn anti_gravity_at<'a>(
    zones: &'a [AntiGravityZone],
    center: &CenterLine,
    distance: f32,
) -> Option<&'a AntiGravityZone> {
    zones.iter().find(|zone| zone.contains(center, distance))
}

/// The up of an item at `position` (issue #78): the road's normal over an anti-gravity zone,
/// exactly the world's up anywhere else (the items' motion is then the one before #78, to the bit).
pub fn up_at(zones: &[AntiGravityZone], center: &CenterLine, position: Vec3) -> Vec3 {
    zones
        .iter()
        .find_map(|zone| zone.up_near(center, position))
        .unwrap_or(Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::data::{ControlPoint, TrackData};

    /// A straight along -Z, banking up to a wall (90°) in the middle.
    pub(crate) fn wall_straight() -> TrackData {
        let point = |z: f32, y: f32, bank: f32| ControlPoint {
            pos: (0.0, y, z),
            width: 14.0,
            bank,
            shoulder: 1.0,
        };
        let mut data = TrackData::from_ron(
            "(name: \"Mur\", points: [(pos: (0.0, 0.0, 0.0)), (pos: (0.0, 0.0, -50.0)), \
             (pos: (50.0, 0.0, -50.0)), (pos: (50.0, 0.0, 0.0))])",
        )
        .unwrap();
        data.points = vec![
            point(0.0, 0.0, 0.0),
            point(-25.0, 0.0, 0.0),
            point(-50.0, 4.0, 45.0),
            point(-75.0, 9.0, 90.0),
            point(-100.0, 9.0, 90.0),
            point(-125.0, 4.0, 45.0),
            point(-150.0, 0.0, 0.0),
            point(-175.0, 0.0, 0.0),
            ControlPoint {
                pos: (80.0, 0.0, -175.0),
                ..point(0.0, 0.0, 0.0)
            },
            ControlPoint {
                pos: (80.0, 0.0, 0.0),
                ..point(0.0, 0.0, 0.0)
            },
        ];
        data.anti_gravity = vec![AntiGravityData { from: 1.0, to: 6.0 }];
        data
    }

    /// The zone covers its stretch of the centerline; over its road, the road's normal (at 90°, a
    /// wall facing the side), within the reach of the walls; elsewhere, nothing, and the world's
    /// up for an item.
    #[test]
    fn a_zone_gives_the_road_normal_over_its_road_only() {
        let data = wall_straight();
        assert_eq!(data.validate(), Ok(()));
        let center = CenterLine::of(&data);
        let zone = AntiGravityZone::new(&center, &data.anti_gravity[0]);
        let (from, to) = (center.param_to_distance(1.0), center.param_to_distance(6.0));
        assert!((zone.from - from).abs() < 1e-3, "{zone:?}");
        assert!((zone.to - to).abs() < 1e-3, "{zone:?}");
        let middle = (from + to) / 2.0;
        assert!(zone.contains(&center, middle) && !zone.contains(&center, from - 5.0));
        assert!(!zone.contains(&center, to + 5.0));
        let zones = [zone];
        assert_eq!(anti_gravity_at(&zones, &center, middle), Some(&zone));
        assert_eq!(anti_gravity_at(&zones, &center, to + 20.0), None);
        // On the wall (90°): the road faces the side, the right edge down.
        let wall = center.at(center.param_to_distance(3.5));
        assert!(wall.up.y.abs() < 0.05 && wall.right.y < -0.95, "{wall:?}");
        for lateral in [-6.0, 0.0, 6.0] {
            for height in [0.5, 3.0] {
                let p = wall.point(lateral, height);
                let up = zone.up_near(&center, p).expect("over the wall");
                assert!(up.dot(wall.up) > 0.999, "{up}");
                assert!(up_at(&zones, &center, p).dot(wall.up) > 0.999);
            }
        }
        // Before the zone, far beside it: the world's up.
        assert_eq!(
            zone.up_near(&center, center.at(from - 10.0).point(0.0, 0.5)),
            None
        );
        assert_eq!(up_at(&zones, &center, Vec3::new(200.0, 0.0, 0.0)), Vec3::Y);
        assert_eq!(up_at(&[], &center, wall.point(0.0, 0.5)), Vec3::Y);
    }

    /// Issue #78: in an anti-gravity zone, the banking flows from point to point (a roll that
    /// stopped at every point would throw a kart off the wall there): its slope at the points
    /// strictly inside the zone is that of its neighbours, zero where the roll turns back or
    /// stops and at the zone's ends; never beyond the banking of the points around (no overshoot
    /// past 90°). Out of the zones, exactly the blend of before #78.
    #[test]
    fn the_banking_flows_through_a_zone() {
        use crate::track::layout::{bank_between, bank_slopes, blend};
        let data = wall_straight();
        let slopes = bank_slopes(&data.points, &data.anti_gravity);
        // Points 2 to 5 are inside the zone (1..6): 45° rising (slope 45 per segment), the two
        // walls at 90° (a stop: zero), 45° coming back down.
        assert_eq!(
            slopes,
            vec![0.0, 0.0, 45.0, 0.0, 0.0, -45.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(bank_slopes(&data.points, &[]), vec![0.0; 10]);
        for t in [0.0, 0.1, 0.37, 0.5, 0.81, 1.0] {
            // Zero slopes: exactly the blend.
            assert_eq!(
                bank_between(10.0, 40.0, (0.0, 0.0), t),
                blend(10.0, 40.0, t)
            );
            // From 0 to 45, then 45 to 90: never beyond.
            for (a, b, m) in [(0.0, 45.0, (0.0, 45.0)), (45.0, 90.0, (45.0, 0.0))] {
                let bank = bank_between(a, b, m, t);
                assert!(bank >= a - 1e-4 && bank <= b + 1e-4, "{a} {b} {t}: {bank}");
            }
        }
        // The slope is continuous through point 2: the roll does not stop there.
        let h = 1e-3;
        let before = (bank_between(0.0, 45.0, (0.0, 45.0), 1.0)
            - bank_between(0.0, 45.0, (0.0, 45.0), 1.0 - h))
            / h;
        let after = (bank_between(45.0, 90.0, (45.0, 0.0), h)
            - bank_between(45.0, 90.0, (45.0, 0.0), 0.0))
            / h;
        assert!(
            (before - 45.0).abs() < 0.5 && (after - 45.0).abs() < 0.5,
            "{before} {after}"
        );
        // A sudden turn back (0, 60, 30) or a plateau: no slope there, no overshoot.
        let mut zigzag = data.clone();
        zigzag.points[3].bank = 30.0;
        zigzag.points[2].bank = 60.0;
        assert_eq!(bank_slopes(&zigzag.points, &zigzag.anti_gravity)[2], 0.0);
    }

    /// GD § 10: beyond 45°, the road banks only in an anti-gravity zone, 90° at most; the zones
    /// are apart from each other and from the gaps. Mirrored, a zone stays where it is (along the
    /// track) and the road banks the other way.
    #[test]
    fn steep_banking_only_in_a_zone() {
        let data = wall_straight();
        assert_eq!(data.validate(), Ok(()));
        let broken = |change: &dyn Fn(&mut TrackData)| {
            let mut data = data.clone();
            change(&mut data);
            data.validate().is_err()
        };
        assert!(broken(&|d| d.anti_gravity.clear()), "a wall without a zone");
        assert!(
            broken(&|d| d.anti_gravity[0].from = 2.5),
            "the zone starts on the wall"
        );
        assert!(
            broken(&|d| d.anti_gravity[0].to = 4.5),
            "the zone ends on the wall"
        );
        assert!(broken(&|d| d.points[3].bank = 95.0), "beyond a wall");
        assert!(broken(&|d| d.points[3].bank = f32::NAN));
        assert!(broken(&|d| d.anti_gravity[0].to = 1.2), "too short");
        assert!(
            broken(&|d| d.anti_gravity[0].to = 9.5),
            "half the circuit at most"
        );
        assert!(broken(&|d| d
            .anti_gravity
            .push(AntiGravityData { from: 5.0, to: 7.0 })));
        assert!(broken(&|d| d
            .gaps
            .push(crate::track::data::GapData { from: 5.5, to: 6.5 })));
        assert!(broken(&|d| d
            .gaps
            .push(crate::track::data::GapData { from: 0.5, to: 1.5 })));
        assert!(!broken(&|d| d
            .gaps
            .push(crate::track::data::GapData { from: 7.2, to: 7.8 })));
        // 45° at most out of a zone, as before #78.
        assert!(!broken(&|d| {
            d.anti_gravity.clear();
            for p in &mut d.points {
                p.bank = p.bank.clamp(-45.0, 45.0);
            }
        }));
        let mirrored = data.mirrored();
        assert_eq!(mirrored.anti_gravity, data.anti_gravity);
        assert_eq!(mirrored.points[3].bank, -90.0);
        assert_eq!(mirrored.validate(), Ok(()));
    }
}
