//! Track mesh generation (pure, no ECS): road, curbs, grass shoulders, low walls and skirts,
//! as flat-shaded quads with vertex colors (one material for the whole circuit).
//!
//! Cross-section, from the centerline outward on each side:
//! road (half width) | curb | grass shoulder | wall | skirt down to the ground.

use bevy::prelude::*;

use super::biome::{Palette, Rgb};
use super::data::{OffroadZone, TrackData};
use super::layout::{CenterLine, TrackSample};

/// Width of the red and white curbs (m).
pub const CURB_WIDTH: f32 = 1.0;
/// Thickness of the low walls (m).
pub const WALL_THICKNESS: f32 = 0.5;
/// Height of the low walls above the road (m), on every circuit: as drawn and as solid. Their
/// collider is the drawn mesh itself ([`TrackMeshes::walls`]): one constant, nothing solid that is
/// not drawn (issue #47). Issue #27: the top of a kart's body is 0.8 m above the road; with walls
/// exactly as high, a kart pushing into a wall caught its top edge, which pushed its nose down
/// until it stood on it, or let it ride up onto the wall. 0.4 m higher, a kart only ever meets
/// the wall's face. It stays below the chase camera's line of sight, which starts 1.35 m above
/// the road at the kart (`CameraTuning::look_height` above the kart's center) and rises from
/// there: the camera's anti-wall ray never meets a wall on a lap (tested on both circuits).
pub const WALL_HEIGHT: f32 = 1.2;
/// Height of the surrounding grass ground. A bit below the road: no z-fighting, and the
/// spline may dip a few centimeters below its control points (Catmull-Rom overshoot).
pub const GROUND_HEIGHT: f32 = -0.25;
/// Off-road patches sit this much above the road so the wheel rays hit them first.
const PATCH_HEIGHT: f32 = 0.03;
/// Length of a curb stripe, in samples.
const CURB_STRIPE: usize = 2;

/// Length of a wall panel, in samples: the walls' faces alternate the two colors of the
/// biome's palette (issue #11).
const WALL_PANEL: usize = 3;

/// A palette color as the mesh takes it (sRGB, opaque).
fn rgba((r, g, b): Rgb) -> [f32; 4] {
    [r, g, b, 1.0]
}

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
    /// Low walls, [`WALL_HEIGHT`] high: drawn, and their collider too, `Surface::Wall` (the same
    /// triangles, issue #47).
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

/// The circuit's meshes, in the colors of `palette` (its biome's, issue #11).
pub fn generate(center: &CenterLine, data: &TrackData, palette: &Palette) -> TrackMeshes {
    let samples = center.samples();
    let mut meshes = TrackMeshes::default();
    let (asphalt, grass, sand, skirt) = (
        rgba(palette.asphalt),
        rgba(palette.shoulder),
        rgba(palette.sand),
        rgba(palette.skirt),
    );
    let curbs = [rgba(palette.curbs.0), rgba(palette.curbs.1)];
    let stripe = |i: usize| curbs[(i / CURB_STRIPE) % 2];

    // Road and curbs.
    let road = &mut meshes.road;
    road.strip(
        samples,
        true,
        |s| s.point(-s.half_width, 0.0),
        |s| s.point(s.half_width, 0.0),
        |_| asphalt,
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
        |_| grass,
    );
    offroad.strip(
        samples,
        true,
        |s| s.point(s.half_width + CURB_WIDTH, 0.0),
        |s| s.point(wall_offset(s), 0.0),
        |_| grass,
    );
    for zone in &data.offroad {
        offroad_patch(offroad, center, zone, sand);
    }

    // Low walls: inner face, top, outer face on each side.
    wall_strips(&mut meshes.walls, samples, palette);

    // Skirts from the wall bottoms down to the ground, so raised parts do not float.
    let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
    let ground = |p: Vec3| Vec3::new(p.x, GROUND_HEIGHT, p.z);
    let decor = &mut meshes.decor;
    decor.strip(
        samples,
        true,
        |s| s.point(outer(s), 0.0),
        |s| ground(s.point(outer(s), 0.0)),
        |_| skirt,
    );
    decor.strip(
        samples,
        true,
        |s| ground(s.point(-outer(s), 0.0)),
        |s| s.point(-outer(s), 0.0),
        |_| skirt,
    );

    meshes
}

/// The walls of both sides, [`WALL_HEIGHT`] high: inner face, flat top, outer face. The faces
/// alternate the palette's two wall colors in panels (the barriers' look, issue #11).
fn wall_strips(mesh: &mut MeshData, samples: &[TrackSample], palette: &Palette) {
    let height = WALL_HEIGHT;
    let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
    let panels = [rgba(palette.walls.0), rgba(palette.walls.1)];
    let panel = |i: usize| panels[(i / WALL_PANEL) % 2];
    let top = rgba(palette.wall_top);
    mesh.strip(
        samples,
        true,
        |s| s.point(wall_offset(s), 0.0),
        |s| s.point(wall_offset(s), height),
        panel,
    );
    mesh.strip(
        samples,
        true,
        |s| s.point(wall_offset(s), height),
        |s| s.point(outer(s), height),
        |_| top,
    );
    mesh.strip(
        samples,
        true,
        |s| s.point(outer(s), height),
        |s| s.point(outer(s), 0.0),
        panel,
    );
    mesh.strip(
        samples,
        true,
        |s| s.point(-wall_offset(s), height),
        |s| s.point(-wall_offset(s), 0.0),
        panel,
    );
    mesh.strip(
        samples,
        true,
        |s| s.point(-outer(s), height),
        |s| s.point(-wall_offset(s), height),
        |_| top,
    );
    mesh.strip(
        samples,
        true,
        |s| s.point(-outer(s), 0.0),
        |s| s.point(-outer(s), height),
        panel,
    );
}

/// A sand patch on the road between two spline parameters.
fn offroad_patch(mesh: &mut MeshData, center: &CenterLine, zone: &OffroadZone, sand: [f32; 4]) {
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
        |_| sand,
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
        let meshes = generate(&center, &data, &Palette::default());
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

    /// Issue #47: the walls are drawn [`WALL_HEIGHT`] high, the height they are solid at (their
    /// collider is this mesh, `circuit.rs`), and that is 0.4 m above the top of a kart's body
    /// (issue #27). Each wall vertex stands on the road or `WALL_HEIGHT` above it, on its sample:
    /// the whole flat top and the upper edge of both faces, 2 vertices in 3. Before #47 they were
    /// drawn 0.8 m high, under a collider raised to 1.2 m: 40 cm of invisible wall.
    #[test]
    fn the_walls_are_drawn_as_high_as_they_are_solid() {
        // A kart on its wheels: its body's center at `GRID_HEIGHT`, its top 0.8 m above the road.
        let kart_top = crate::track::GRID_HEIGHT + crate::kart::KART_HALF_EXTENTS.y;
        assert!(WALL_HEIGHT >= kart_top + 0.4 - 1e-4, "{WALL_HEIGHT}");
        for data in [
            test_track(),
            TrackData::from_ron(include_str!("../../assets/tracks/anneau.track.ron")).unwrap(),
        ] {
            let center = CenterLine::new(&data.points, data.sample_step);
            let meshes = generate(&center, &data, &Palette::default());
            let samples = center.samples();
            let walls = &meshes.walls;
            // 6 strips (inner face, top, outer face, on both sides), one quad per sample.
            assert_eq!(walls.positions.len(), 6 * 4 * samples.len());
            let mut on_top = 0;
            for (v, p) in walls.positions.iter().enumerate() {
                // Corners 0 and 1 of a quad come from its sample, 2 and 3 from the next one.
                let quad = v / 4 % samples.len();
                let s = &samples[(quad + usize::from(v % 4 >= 2)) % samples.len()];
                let height = (Vec3::from_array(*p) - s.position).dot(s.up);
                if (height - WALL_HEIGHT).abs() < 1e-3 {
                    on_top += 1;
                } else {
                    assert!(
                        height.abs() < 1e-3,
                        "{}: vertex {v} at {height} m",
                        data.name
                    );
                }
            }
            assert_eq!(3 * on_top, 2 * walls.positions.len(), "{}", data.name);
            // What is drawn is all of it: the walls are in the circuit's single mesh.
            let all = meshes.combined();
            let start = meshes.road.positions.len() + meshes.offroad.positions.len();
            assert_eq!(
                all.positions[start..start + walls.positions.len()],
                walls.positions[..]
            );
        }
    }

    #[test]
    fn floors_face_up_and_walls_face_the_road() {
        let data = test_track();
        let center = CenterLine::new(&data.points, data.sample_step);
        let meshes = generate(&center, &data, &Palette::default());
        assert!(meshes.road.normals.iter().all(|n| n[1] > 0.7));
        assert!(meshes.offroad.normals.iter().all(|n| n[1] > 0.7));
        // The first wall face of each side faces the centerline.
        let s = &center.samples()[10];
        let right_inner = Vec3::from_array(meshes.walls.normals[10 * 4]);
        assert!(right_inner.dot(s.right) < -0.9, "{right_inner}");
    }
}
