//! Track mesh generation (pure, no ECS): road, curbs, grass shoulders, low walls and skirts,
//! as flat-shaded quads with vertex colors (one material for the whole circuit).
//!
//! Cross-section, from the centerline outward on each side:
//! road (half width) | curb | grass shoulder | wall | skirt down to the ground.

use bevy::prelude::*;

use super::data::{OffroadZone, TrackData};
use super::layout::{CenterLine, TrackSample};

/// Width of the red and white curbs (m).
pub const CURB_WIDTH: f32 = 1.0;
/// Thickness of the low walls (m).
pub const WALL_THICKNESS: f32 = 0.5;
/// Height of the surrounding grass ground. A bit below the road: no z-fighting, and the
/// spline may dip a few centimeters below its control points (Catmull-Rom overshoot).
pub const GROUND_HEIGHT: f32 = -0.25;
/// Off-road patches sit this much above the road so the wheel rays hit them first.
const PATCH_HEIGHT: f32 = 0.03;
/// Length of a curb stripe, in samples.
const CURB_STRIPE: usize = 2;

/// Colors, sRGB.
pub const ASPHALT: [f32; 4] = [0.42, 0.43, 0.48, 1.0];
pub const CURB_RED: [f32; 4] = [0.85, 0.20, 0.20, 1.0];
pub const CURB_WHITE: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
pub const SHOULDER_GRASS: [f32; 4] = [0.40, 0.70, 0.32, 1.0];
pub const SAND: [f32; 4] = [0.90, 0.80, 0.55, 1.0];
pub const WALL: [f32; 4] = [0.60, 0.55, 0.80, 1.0];
pub const WALL_TOP: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
pub const SKIRT: [f32; 4] = [0.55, 0.45, 0.35, 1.0];

/// Triangles with flat normals and vertex colors.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Adds the quad a → b → c → d (counter-clockwise seen from its front). `color` is sRGB,
    /// like `Color::srgba`; vertex colors are stored linear, as the shader expects.
    pub fn quad(&mut self, corners: [Vec3; 4], color: [f32; 4]) {
        let color = Color::srgba(color[0], color[1], color[2], color[3])
            .to_linear()
            .to_f32_array();
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or(Vec3::Y);
        let base = self.positions.len() as u32;
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(color);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Adds a strip along consecutive samples (wrapping around when `closed`): for each
    /// sample, `a` and `b` give the two edges of the strip. The front face is
    /// `(b - a) × forward`: put `a` on the left for a floor, below for a wall facing left.
    pub fn strip(
        &mut self,
        samples: &[TrackSample],
        closed: bool,
        a: impl Fn(&TrackSample) -> Vec3,
        b: impl Fn(&TrackSample) -> Vec3,
        color: impl Fn(usize) -> [f32; 4],
    ) {
        let n = samples.len();
        let segments = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            let (s0, s1) = (&samples[i], &samples[(i + 1) % n]);
            self.quad([a(s0), b(s0), b(s1), a(s1)], color(i));
        }
    }

    pub fn append(&mut self, other: &MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.colors.extend_from_slice(&other.colors);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Vertices and triangles for a Rapier trimesh collider.
    pub fn collider_data(&self) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        (
            self.positions
                .iter()
                .map(|p| Vec3::from_array(*p))
                .collect(),
            self.indices.as_chunks::<3>().0.to_vec(),
        )
    }
}

/// The generated circuit, split by surface (one collider each) plus decoration.
#[derive(Clone, Debug, Default)]
pub struct TrackMeshes {
    /// Asphalt and curbs: `Surface::Road`.
    pub road: MeshData,
    /// Grass shoulders and off-road patches: `Surface::Offroad`.
    pub offroad: MeshData,
    /// Low walls: bouncy.
    pub walls: MeshData,
    /// Skirts under raised parts: visual only.
    pub decor: MeshData,
}

impl TrackMeshes {
    /// Everything in one mesh, for a single draw call.
    pub fn combined(&self) -> MeshData {
        let mut all = self.road.clone();
        all.append(&self.offroad);
        all.append(&self.walls);
        all.append(&self.decor);
        all
    }
}

/// Lateral distance from the centerline to the inner face of the wall.
pub fn wall_offset(sample: &TrackSample) -> f32 {
    sample.half_width + CURB_WIDTH + sample.shoulder
}

pub fn generate(center: &CenterLine, data: &TrackData) -> TrackMeshes {
    let samples = center.samples();
    let mut meshes = TrackMeshes::default();
    let stripe = |i: usize| {
        if (i / CURB_STRIPE).is_multiple_of(2) {
            CURB_RED
        } else {
            CURB_WHITE
        }
    };
    let h = data.wall_height;

    // Road and curbs.
    let road = &mut meshes.road;
    road.strip(
        samples,
        true,
        |s| s.point(-s.half_width, 0.0),
        |s| s.point(s.half_width, 0.0),
        |_| ASPHALT,
    );
    road.strip(
        samples,
        true,
        |s| s.point(-s.half_width - CURB_WIDTH, 0.0),
        |s| s.point(-s.half_width, 0.0),
        stripe,
    );
    road.strip(
        samples,
        true,
        |s| s.point(s.half_width, 0.0),
        |s| s.point(s.half_width + CURB_WIDTH, 0.0),
        stripe,
    );

    // Grass shoulders.
    let offroad = &mut meshes.offroad;
    offroad.strip(
        samples,
        true,
        |s| s.point(-wall_offset(s), 0.0),
        |s| s.point(-s.half_width - CURB_WIDTH, 0.0),
        |_| SHOULDER_GRASS,
    );
    offroad.strip(
        samples,
        true,
        |s| s.point(s.half_width + CURB_WIDTH, 0.0),
        |s| s.point(wall_offset(s), 0.0),
        |_| SHOULDER_GRASS,
    );
    for zone in &data.offroad {
        offroad_patch(offroad, center, zone);
    }

    // Low walls: inner face, top, outer face on each side.
    let walls = &mut meshes.walls;
    let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
    walls.strip(
        samples,
        true,
        |s| s.point(wall_offset(s), 0.0),
        |s| s.point(wall_offset(s), h),
        |_| WALL,
    );
    walls.strip(
        samples,
        true,
        |s| s.point(wall_offset(s), h),
        |s| s.point(outer(s), h),
        |_| WALL_TOP,
    );
    walls.strip(
        samples,
        true,
        |s| s.point(outer(s), h),
        |s| s.point(outer(s), 0.0),
        |_| WALL,
    );
    walls.strip(
        samples,
        true,
        |s| s.point(-wall_offset(s), h),
        |s| s.point(-wall_offset(s), 0.0),
        |_| WALL,
    );
    walls.strip(
        samples,
        true,
        |s| s.point(-outer(s), h),
        |s| s.point(-wall_offset(s), h),
        |_| WALL_TOP,
    );
    walls.strip(
        samples,
        true,
        |s| s.point(-outer(s), 0.0),
        |s| s.point(-outer(s), h),
        |_| WALL,
    );

    // Skirts from the wall bottoms down to the ground, so raised parts do not float.
    let ground = |p: Vec3| Vec3::new(p.x, GROUND_HEIGHT, p.z);
    let decor = &mut meshes.decor;
    decor.strip(
        samples,
        true,
        |s| s.point(outer(s), 0.0),
        |s| ground(s.point(outer(s), 0.0)),
        |_| SKIRT,
    );
    decor.strip(
        samples,
        true,
        |s| ground(s.point(-outer(s), 0.0)),
        |s| s.point(-outer(s), 0.0),
        |_| SKIRT,
    );

    meshes
}

/// A sand patch on the road between two spline parameters.
fn offroad_patch(mesh: &mut MeshData, center: &CenterLine, zone: &OffroadZone) {
    let from = center.param_to_distance(zone.from);
    let mut to = center.param_to_distance(zone.to);
    if to <= from {
        to += center.length();
    }
    let step = center.length() / center.samples().len() as f32;
    let count = ((to - from) / step).ceil().max(1.0) as usize;
    let samples: Vec<TrackSample> = (0..=count)
        .map(|i| center.at(from + (to - from) * i as f32 / count as f32))
        .collect();
    mesh.strip(
        &samples,
        false,
        |s| s.point(zone.left * s.half_width, PATCH_HEIGHT),
        |s| s.point(zone.right * s.half_width, PATCH_HEIGHT),
        |_| SAND,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::data::TrackData;
    use std::collections::HashMap;

    /// Edges used by exactly one triangle, keyed by welded vertex positions.
    fn boundary_edges(mesh: &MeshData) -> Vec<([i64; 3], [i64; 3])> {
        let key = |i: u32| {
            let p = mesh.positions[i as usize];
            [
                (p[0] * 100.0).round() as i64,
                (p[1] * 100.0).round() as i64,
                (p[2] * 100.0).round() as i64,
            ]
        };
        let mut uses: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
        for t in mesh.indices.as_chunks::<3>().0 {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (ka, kb) = (key(a), key(b));
                let edge = if ka < kb { (ka, kb) } else { (kb, ka) };
                *uses.entry(edge).or_default() += 1;
            }
        }
        uses.into_iter()
            .filter(|(_, n)| *n == 1)
            .map(|(e, _)| e)
            .collect()
    }

    /// Number of closed loops formed by `edges`, or `None` if they don't form loops.
    fn count_loops(edges: &[([i64; 3], [i64; 3])]) -> Option<usize> {
        let mut neighbours: HashMap<[i64; 3], Vec<[i64; 3]>> = HashMap::new();
        for (a, b) in edges {
            neighbours.entry(*a).or_default().push(*b);
            neighbours.entry(*b).or_default().push(*a);
        }
        if neighbours.values().any(|n| n.len() != 2) {
            return None;
        }
        let mut seen = std::collections::HashSet::new();
        let mut loops = 0;
        for start in neighbours.keys() {
            if seen.insert(*start) {
                loops += 1;
                let mut stack = vec![*start];
                while let Some(v) = stack.pop() {
                    for n in &neighbours[&v] {
                        if seen.insert(*n) {
                            stack.push(*n);
                        }
                    }
                }
            }
        }
        Some(loops)
    }

    pub(crate) fn test_track() -> TrackData {
        TrackData::from_ron(include_str!("../../assets/tracks/prairie.track.ron")).unwrap()
    }

    #[test]
    fn road_ribbon_is_a_closed_loop() {
        let data = test_track();
        let center = CenterLine::new(&data.points, data.sample_step);
        let meshes = generate(&center, &data);
        // Road + curbs welded: a closed ribbon has exactly two boundary loops (outer edges of
        // the two curbs) and no other open edge: no gap, no hole, including at the start line.
        let edges = boundary_edges(&meshes.road);
        assert_eq!(
            count_loops(&edges),
            Some(2),
            "{} boundary edges",
            edges.len()
        );
        // ...and it goes all the way around: as many segments as samples per strip.
        assert_eq!(meshes.road.triangle_count(), 3 * 2 * center.samples().len());
    }

    #[test]
    fn floors_face_up_and_walls_face_the_road() {
        let data = test_track();
        let center = CenterLine::new(&data.points, data.sample_step);
        let meshes = generate(&center, &data);
        assert!(meshes.road.normals.iter().all(|n| n[1] > 0.7));
        assert!(meshes.offroad.normals.iter().all(|n| n[1] > 0.7));
        // The first wall face of each side faces the centerline.
        let s = &center.samples()[10];
        let right_inner = Vec3::from_array(meshes.walls.normals[10 * 4]);
        assert!(right_inner.dot(s.right) < -0.9, "{right_inner}");
    }
}
