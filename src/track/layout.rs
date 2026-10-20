//! Pure track geometry (no ECS): the closed Catmull-Rom centerline sampled every meter or so,
//! with a banked frame per sample, checkpoints, the starting grid and placements.

use bevy::prelude::*;

use super::anti_gravity::AntiGravityZone;
use super::data::{AntiGravityData, ControlPoint, OffroadZone, TrackData};
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

impl CenterLine {
    /// Samples the closed spline through `points` roughly every `step` meters.
    pub fn new(points: &[ControlPoint], step: f32) -> Self {
        Self::with_zones(points, step, &[])
    }

    /// The centerline of a circuit's file: its points, its step, and its anti-gravity zones,
    /// where the banking flows from point to point (issue #78, [`bank_slopes`]).
    pub fn of(data: &TrackData) -> Self {
        Self::with_zones(&data.points, data.sample_step, &data.anti_gravity)
    }

    /// The same, the banking flowing in the anti-gravity zones `zones` ([`bank_slopes`]).
    pub fn with_zones(points: &[ControlPoint], step: f32, zones: &[AntiGravityData]) -> Self {
        let n = points.len();
        let slopes = bank_slopes(points, zones);
        assert!(n >= 4, "a track needs at least 4 control points");
        let point = |i: usize| points[i % n];
        let curve_at = |param: f32| {
            let segment = (param.floor() as usize) % n;
            let t = param - param.floor();
            let (p0, p1, p2, p3) = (
                point(segment + n - 1).position(),
                point(segment).position(),
                point(segment + 1).position(),
                point(segment + 2).position(),
            );
            (
                catmull_rom(p0, p1, p2, p3, t),
                catmull_rom_tangent(p0, p1, p2, p3, t),
                point(segment),
                point(segment + 1),
                t,
                (slopes[segment], slopes[(segment + 1) % n]),
            )
        };

        // Measure: cumulative length at each fine sub-step.
        let fine_count = n * SUBSTEPS;
        let mut fine_params = Vec::with_capacity(fine_count + 1);
        let mut fine_lengths = Vec::with_capacity(fine_count + 1);
        let mut total = 0.0;
        let mut previous = curve_at(0.0).0;
        for k in 0..=fine_count {
            let param = k as f32 / SUBSTEPS as f32;
            let position = curve_at(param).0;
            total += position.distance(previous);
            previous = position;
            fine_params.push(param);
            fine_lengths.push(total);
        }
        let length = total;
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
            let (position, tangent, a, b, t, slopes) = curve_at(param);

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

        Self {
            samples,
            length,
            control_distances,
        }
    }

    pub fn length(&self) -> f32 {
        self.length
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
        TrackSample {
            distance,
            position: a.position.lerp(b.position, t),
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

    /// Checkpoints every `spacing` meters, the first one on the start line.
    pub fn checkpoints(&self, spacing: f32) -> Vec<Checkpoint> {
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

/// Racers on the grid (GD § 1).
pub const GRID_SLOTS: usize = 12;

/// Issue #144: the pelican drops no kart within this much of an obstacle or a passer-by (m along
/// the track, each way).
const HAZARD_RESPAWN_CLEARANCE: f32 = 9.0;

impl TrackLayout {
    pub fn new(data: &TrackData) -> Self {
        let center = CenterLine::of(data);
        let checkpoints = center.checkpoints(data.checkpoint_spacing);
        let respawns = if data.respawns.is_empty() {
            // One per checkpoint, except where a kart must not be dropped: on a ramp, in sand, in
            // a gap (issue #20: no road there) or just before one, in an anti-gravity zone (issue
            // #78: on a wall, maybe), along a stretch without wall (issue #107: the pelican drops
            // a kart that fell from it before it).
            let blocked: Vec<(f32, f32)> = data
                .trick_ramps
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
    use crate::track::mesh::{CURB_WIDTH, GROUND_HEIGHT, WALL_THICKNESS, wall_offset};

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
    #[test]
    fn every_circuit_and_its_mirror_keep_their_parts_apart() {
        let circuits = every_circuit();
        assert!(circuits.len() >= 5, "issue #20: at least 5 circuits");
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
            let length = center.length();
            for (i, a) in samples.iter().enumerate().step_by(2) {
                for b in &samples[i + 1..] {
                    let along = (b.distance - a.distance).min(length - (b.distance - a.distance));
                    if along < 60.0 {
                        continue;
                    }
                    let apart = a.position.xz().distance(b.position.xz());
                    let needed = wall_offset(a) + wall_offset(b) + 2.0 * WALL_THICKNESS + 1.0;
                    assert!(
                        apart > needed,
                        "{name}: the track at {:.0} m and at {:.0} m is {apart:.1} m apart \
                         (walls need {needed:.1} m)",
                        a.distance,
                        b.distance
                    );
                }
            }
        }
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
}
