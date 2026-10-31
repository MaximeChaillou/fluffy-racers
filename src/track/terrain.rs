//! Issue #195: **the terrain**, real heights around every circuit (Max: « rajoute PLUS de relief
//! sur les maps, ici tout est relativement plat […] Donc rajoute des vraies hauteurs ! »). Hills,
//! vales, cliffs and mountains close to the road, written per biome in `assets/data/biomes.ron`
//! (`terrain`), and per circuit if its file wants its own (`terrain` in its file, in place of its
//! biome's).
//!
//! - **What it is**: a heightfield over the circuit's ground slab and a ring beyond it
//!   ([`extent`]), its heights an fBm of value noise (deterministic: the circuit's `decor_seed`,
//!   basic arithmetic only, the same on every peer and in every browser), shaped by the biome
//!   (`height`, `scale`, `ridges` for sharp crests, `cliffs` and `terrace` for stepped mesas and
//!   cliffs). Its foot is the ground of the circuit: it only ever rises above it.
//! - **The road is never touched**: within [`TerrainData::clear`] meters of the outer face of
//!   the walls (of the road, of every shortcut, of the lakes, and inside every tight turn), the
//!   terrain stays under the ground ([`SINK`] below it, hidden by the ground's box): the road,
//!   its shoulders, its walls, its skirts, its voids and its drops are exactly as before, and so
//!   is the decor near it. Beyond, it rises to its full height over [`TerrainData::rise`] meters.
//!   With at least [`MIN_CLEAR`] meters, the chase camera never has a slope between itself and
//!   the road it shows (tested on every circuit).
//! - **Solid where a kart can reach it**: the raised triangles within [`COLLIDER_REACH`] meters of
//!   the walls are one trimesh collider (off-road: grass), the rest is drawn only. A kart that
//!   leaves the road rolls up the slope, never through it, until the pelican comes (the rules of
//!   the race: out of the track for 1 s).
//! - **Drawn** in square tiles ([`TILE`], each culled on its own), flat
//!   shaded low-poly faces in vertex colors (the biome's ground, its rock on the steep faces, its
//!   snow high up), one material for the whole terrain, no shadow cast. Built once, when the
//!   circuit is built; nothing per frame.
//! - **The decor stands on it** (`dressing.rs`): every piece, near and far, is lifted onto the
//!   slope under its footprint.

use bevy::prelude::*;
use serde::Deserialize;

use super::biome::{Palette, Rgb};
use super::decor::tight_turns;
use super::far::{FarDecor, FarLayer, GroundRect};
use super::layout::{CenterLine, TrackSample};
use super::mesh::{GROUND_HEIGHT, MeshData, WALL_THICKNESS, wall_offset};
use super::shortcut::Shortcut;
use super::water::Lake;

/// How far below the ground the terrain stays where it is flat (m): under the ground's box, never
/// drawn over it nor in its way.
pub const SINK: f32 = 0.05;

/// The side of a cell of the heightfield (m).
pub const CELL: f32 = 6.0;

/// The terrain is drawn in square tiles this wide (m), each culled on its own: larger than the
/// circuit's (`circuit::TILE_SIZE`), as it has fewer faces per meter. A few draw calls.
pub const TILE: f32 = 400.0;

/// The least flat band a terrain may keep beyond the walls (m). A sight line of the chase camera
/// (6 m behind the kart) to the road up to 60 m ahead never passes further than about 33 m from
/// the road: at 30 m and less, the terrain is still within centimeters of the ground.
pub const MIN_CLEAR: f32 = 24.0;

/// Raised triangles closer than this to the walls (m) are solid: a kart out of the track is towed
/// back after 1 s (`RaceTuning::outside_seconds`), 30 to 40 m beyond the walls at the most.
pub const COLLIDER_REACH: f32 = 100.0;

/// How far beyond the slab the terrain reaches at most (m), and over how many meters its outer
/// rim comes back down to the ground (m, at most).
pub const MAX_EXTENT: f32 = 160.0;
const RIM: f32 = 50.0;

/// Mixed into the decor's seed: the terrain's noise is not the decor's draws.
const SEED_SALT: u64 = 0x0195_7E44_A1A0_0195;

/// The octaves of the noise.
const OCTAVES: u32 = 4;

/// The shape of a biome's terrain (`terrain` in `assets/data/biomes.ron`, or in a circuit's
/// file in place of its biome's).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TerrainData {
    /// Its full height (m): the highest hills reach it.
    pub height: f32,
    /// The size of a hill (m): the noise's first octave.
    pub scale: f32,
    /// 0: round hills; 1: sharp crests (ridged noise), mountains.
    #[serde(default)]
    pub ridges: f32,
    /// 0: smooth slopes; 1: steps of `terrace` meters with steep faces, mesas and cliffs.
    #[serde(default)]
    pub cliffs: f32,
    #[serde(default = "default_terrace")]
    pub terrace: f32,
    /// The lowest share of its height between the hills (0..1): above 0, no plain is flat.
    #[serde(default = "default_floor")]
    pub floor: f32,
    /// The flat band kept beyond the walls (m), at least [`MIN_CLEAR`].
    #[serde(default = "default_clear")]
    pub clear: f32,
    /// Beyond it, the terrain reaches its full height over this many meters.
    #[serde(default = "default_rise")]
    pub rise: f32,
    /// The color of its steep faces, and the slope (rise over run) from which a face is steep.
    pub rock: Rgb,
    #[serde(default = "default_steep")]
    pub steep: f32,
    /// Snow above a height (m): its color, the height.
    #[serde(default)]
    pub snow: Option<(Rgb, f32)>,
}

fn default_terrace() -> f32 {
    10.0
}
fn default_floor() -> f32 {
    0.2
}
fn default_clear() -> f32 {
    MIN_CLEAR
}
fn default_rise() -> f32 {
    45.0
}
fn default_steep() -> f32 {
    0.9
}

impl TerrainData {
    /// Checks the values that would make a broken terrain.
    pub fn validate(&self) -> Result<(), String> {
        let within = |what: &str, value: f32, (lo, hi): (f32, f32)| {
            if (lo..=hi).contains(&value) {
                Ok(())
            } else {
                Err(format!("terrain: {what} {value} out of {lo}..={hi}"))
            }
        };
        within("height", self.height, (0.0, 120.0))?;
        within("scale", self.scale, (40.0, 800.0))?;
        within("ridges", self.ridges, (0.0, 1.0))?;
        within("cliffs", self.cliffs, (0.0, 1.0))?;
        within("terrace", self.terrace, (2.0, 60.0))?;
        within("floor", self.floor, (0.0, 1.0))?;
        within("clear", self.clear, (MIN_CLEAR, 120.0))?;
        within("rise", self.rise, (10.0, 200.0))?;
        within("steep", self.steep, (0.1, 10.0))?;
        let colors = [Some(self.rock), self.snow.map(|(color, _)| color)];
        if colors
            .iter()
            .flatten()
            .any(|(r, g, b)| [r, g, b].iter().any(|v| !(0.0..=1.0).contains(*v)))
        {
            return Err("terrain: a color out of 0..=1".to_string());
        }
        Ok(())
    }

    /// The shape of the noise at a point (0..1), before the flat band and the rim: the fBm,
    /// ridged, raised off the floor, in terraces.
    fn shape(&self, noise: f32) -> f32 {
        let ridged = 1.0 - (2.0 * noise - 1.0).abs();
        let n = noise + (ridged - noise) * self.ridges;
        let n = self.floor + (1.0 - self.floor) * n;
        if self.cliffs <= 0.0 || self.height <= 0.0 {
            return n;
        }
        // Steps of `terrace` meters: flat on most of a step, a steep face between two.
        let t = n * self.height / self.terrace;
        let k = t.floor();
        let f = t - k;
        let step = smoothstep(0.38, 0.62, f);
        (k + f + (step - f) * self.cliffs) * self.terrace / self.height
    }
}

/// 3t² - 2t³ between `lo` and `hi` (0 below, 1 above).
fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A lattice value of the noise (0..1), from integers only.
fn lattice(seed: u64, x: i32, z: i32) -> f32 {
    let mut h = seed
        ^ (x as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise at `p` (in lattice cells), 0..1.
fn value_noise(seed: u64, p: Vec2) -> f32 {
    let (x0, z0) = (p.x.floor(), p.y.floor());
    let (fx, fz) = (p.x - x0, p.y - z0);
    let (ix, iz) = (x0 as i32, z0 as i32);
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v) = (ease(fx), ease(fz));
    let a = lattice(seed, ix, iz);
    let b = lattice(seed, ix + 1, iz);
    let c = lattice(seed, ix, iz + 1);
    let d = lattice(seed, ix + 1, iz + 1);
    let top = a + (b - a) * u;
    let bottom = c + (d - c) * u;
    top + (bottom - top) * v
}

/// The fBm at `p` (m), 0..1: [`OCTAVES`] octaves of value noise, each twice as fine and half as
/// strong as the last.
fn fbm(seed: u64, p: Vec2, scale: f32) -> f32 {
    let (mut sum, mut weight, mut total) = (0.0, 1.0, 0.0);
    let mut q = p / scale;
    for octave in 0..OCTAVES {
        sum += weight * value_noise(seed.wrapping_add(octave as u64), q);
        total += weight;
        weight *= 0.5;
        // A turn between octaves (a rotation by a rational angle: 3-4-5), so their lattices do
        // not line up.
        q = Vec2::new(0.8 * q.x - 0.6 * q.y, 0.6 * q.x + 0.8 * q.y) * 2.0 + Vec2::splat(17.0);
    }
    sum / total
}

/// How far beyond the slab the terrain of a circuit with this far decor reaches (m): to the foot
/// of its first range, or to its beach, [`MAX_EXTENT`] at most.
pub fn extent(far: Option<&FarDecor>) -> f32 {
    let Some(far) = far else {
        return 0.0;
    };
    far.layers
        .iter()
        .filter_map(|layer| match layer {
            FarLayer::Range {
                distance, depth, ..
            } => Some(distance - depth / 2.0),
            FarLayer::Sea { from, beach, .. } => Some(from - beach),
            _ => None,
        })
        .fold(MAX_EXTENT, f32::min)
        .max(0.0)
}

/// The terrain of a circuit, built: heights on a grid in the frame of its file (the mirror mode
/// reads and draws it across the X = 0 plane: the mirror is its exact image).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Terrain {
    /// The grid's first vertex (file frame, ground plane), its cells' side, its vertices across
    /// (x) and along (z).
    pub origin: Vec2,
    pub cell: f32,
    pub cols: usize,
    pub rows: usize,
    /// Height of each vertex above the ground (m; `-SINK` where flat), by row then column.
    pub heights: Vec<f32>,
    /// Distance of each vertex from the walls (m, capped): what is solid.
    pub clearance: Vec<f32>,
    /// Drawn in the mirror image.
    pub mirrored: bool,
    /// Its colors: the ground's, its rock and its steepness, its snow.
    pub ground: Rgb,
    pub rock: Rgb,
    pub steep: f32,
    pub snow: Option<(Rgb, f32)>,
}

impl Terrain {
    /// The terrain of a circuit (its file, not mirrored: `center` its centerline, `shortcuts`
    /// and `lakes` the file's), `mirrored` for the mirror mode. None without a terrain.
    pub fn build(
        terrain: &TerrainData,
        center: &CenterLine,
        shortcuts: &[Shortcut],
        lakes: &[Lake],
        far: Option<&FarDecor>,
        (palette, seed, mirrored): (&Palette, u64, bool),
    ) -> Self {
        let rect = GroundRect::of(center);
        let reach = extent(far);
        let rim = RIM.min(reach);
        let origin = rect.min - Vec2::splat(reach);
        let size = rect.max - rect.min + Vec2::splat(2.0 * reach);
        let cols = (size.x / CELL).ceil() as usize + 1;
        let rows = (size.y / CELL).ceil() as usize + 1;
        // A cell's diagonal more: a triangle with a corner in the band is flat all over.
        let start = terrain.clear + CELL * std::f32::consts::SQRT_2;
        let full = start + terrain.rise;
        let cap = full.max(COLLIDER_REACH) + CELL;
        let mut clearance = vec![cap; cols * rows];
        let at = |i: usize, j: usize| origin + Vec2::new(i as f32, j as f32) * CELL;
        // Stamps a disc of `radius` around `p`: every vertex in it at `distance(vertex)`.
        let mut stamp = |p: Vec2, radius: f32, distance: &dyn Fn(Vec2) -> f32| {
            let lo = ((p - Vec2::splat(radius) - origin) / CELL).floor();
            let hi = ((p + Vec2::splat(radius) - origin) / CELL).ceil();
            let (i0, j0) = (lo.x.max(0.0) as usize, lo.y.max(0.0) as usize);
            let (i1, j1) = (
                (hi.x.max(0.0) as usize).min(cols - 1),
                (hi.y.max(0.0) as usize).min(rows - 1),
            );
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let d = distance(at(i, j));
                    let slot = &mut clearance[j * cols + i];
                    if d < *slot {
                        *slot = d;
                    }
                }
            }
        };
        // From the walls' outer face of every sample of the road and of the shortcuts (on an open
        // circuit, its road only: nothing is built on its way back).
        let road = center.road_samples();
        let paths = shortcuts.iter().flat_map(|s| s.samples().iter());
        for sample in road.iter().chain(paths) {
            let keep = keep_out(sample);
            let p = sample.position.xz();
            stamp(p, keep + cap, &|v| v.distance(p) - keep);
        }
        for lake in lakes {
            let (lo, hi) = (lake.min, lake.max);
            stamp((lo + hi) / 2.0, (hi - lo).length() / 2.0 + cap, &|v| {
                (lo - v).max(v - hi).max(Vec2::ZERO).length()
            });
        }
        // Inside a tight turn, nothing rises: the road out of it stays in sight (GD § 16, règle
        // 3: a change of level never hides a tight turn).
        for turn in tight_turns(center).iter().step_by(3) {
            let (c, r) = (turn.center, turn.radius);
            stamp(c, r + cap, &|v| (v.distance(c) - r).max(0.0));
        }

        let seed = seed ^ SEED_SALT;
        let mut heights = vec![-SINK; cols * rows];
        for j in 0..rows {
            for i in 0..cols {
                let k = j * cols + i;
                let p = at(i, j);
                let band = smoothstep(start, start + terrain.rise, clearance[k]);
                let out = 1.0 - smoothstep(reach - rim, reach, rect.outside(p));
                let weight = band * if rim > 0.0 { out } else { 1.0 };
                if weight <= 0.0 {
                    continue;
                }
                let h = terrain.height * terrain.shape(fbm(seed, p, terrain.scale)) * weight;
                heights[k] = h - SINK;
            }
        }
        Self {
            origin,
            cell: CELL,
            cols,
            rows,
            heights,
            clearance,
            mirrored,
            ground: palette.ground,
            rock: terrain.rock,
            steep: terrain.steep,
            snow: terrain.snow,
        }
    }

    /// No terrain at all.
    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// The vertex `(i, j)` in the file's frame (m).
    fn vertex(&self, i: usize, j: usize) -> Vec3 {
        let p = self.origin + Vec2::new(i as f32, j as f32) * self.cell;
        Vec3::new(p.x, GROUND_HEIGHT + self.heights[j * self.cols + i], p.y)
    }

    /// The point in the world of a point of the file's frame (the mirror mode: x → -x).
    fn world(&self, p: Vec3) -> Vec3 {
        if self.mirrored {
            Vec3::new(-p.x, p.y, p.z)
        } else {
            p
        }
    }

    /// The height of the terrain's surface at `p` (world, ground plane), on its triangles (`None`
    /// off its grid). Under the ground ([`SINK`] below it) where it is flat.
    pub fn surface(&self, p: Vec2) -> Option<f32> {
        if self.is_empty() {
            return None;
        }
        let p = if self.mirrored {
            Vec2::new(-p.x, p.y)
        } else {
            p
        };
        let local = (p - self.origin) / self.cell;
        let (i, j) = (local.x.floor(), local.y.floor());
        if i < 0.0 || j < 0.0 || i as usize + 1 >= self.cols || j as usize + 1 >= self.rows {
            return None;
        }
        let (i, j) = (i as usize, j as usize);
        let (fu, fv) = (local.x - i as f32, local.y - j as f32);
        let h = |i: usize, j: usize| self.heights[j * self.cols + i];
        let (a, b, c, d) = (h(i, j), h(i + 1, j), h(i + 1, j + 1), h(i, j + 1));
        // The two triangles of a cell, split along a → c (see `mesh`).
        let height = if fv >= fu {
            a + fv * (d - a) + fu * (c - d)
        } else {
            a + fu * (b - a) + fv * (c - b)
        };
        Some(GROUND_HEIGHT + height)
    }

    /// Where a piece of decor of `radius` at `p` stands (its foot's height, world): on the ground,
    /// or on the slope, the lowest of its footprint (it sinks into the slope rather than floats).
    pub fn foot(&self, p: Vec2, radius: f32) -> f32 {
        let r = radius * 0.7;
        [Vec2::ZERO, Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
            .iter()
            .map(|o| self.surface(p + *o * r).unwrap_or(GROUND_HEIGHT))
            .fold(f32::MAX, f32::min)
            .max(GROUND_HEIGHT)
    }

    /// The raised cells (a vertex above the ground), `solid` those within [`COLLIDER_REACH`] of
    /// the walls, or every one.
    fn cells(&self, solid: bool) -> impl Iterator<Item = (usize, usize)> + '_ {
        let (cols, rows) = (self.cols, self.rows);
        (0..rows.saturating_sub(1))
            .flat_map(move |j| (0..cols - 1).map(move |i| (i, j)))
            .filter(move |&(i, j)| {
                let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
                let k = |(i, j): (usize, usize)| j * cols + i;
                corners.iter().any(|c| self.heights[k(*c)] > 0.0)
                    && (!solid
                        || corners
                            .iter()
                            .any(|c| self.clearance[k(*c)] < COLLIDER_REACH))
            })
    }

    /// The triangles of the raised cells (world), facing up: `solid` only, or all.
    fn triangles(&self, solid: bool) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.cells(solid).flat_map(move |(i, j)| {
            let a = self.world(self.vertex(i, j));
            let b = self.world(self.vertex(i + 1, j));
            let c = self.world(self.vertex(i + 1, j + 1));
            let d = self.world(self.vertex(i, j + 1));
            // Counter-clockwise seen from above (+Y) in the file's frame; the mirror turns them.
            if self.mirrored {
                [[a, c, d], [a, b, c]]
            } else {
                [[a, d, c], [a, c, b]]
            }
        })
    }

    /// The color of a face: snow high up (not on the steep faces), rock on the steep ones, the
    /// ground's elsewhere, a little lighter higher up.
    fn color_of(&self, corners: &[Vec3; 3]) -> Rgb {
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or(Vec3::Y);
        let flat = (1.0 - normal.y * normal.y).max(0.0).sqrt();
        let slope = flat / normal.y.max(1e-3);
        let height = (corners[0].y + corners[1].y + corners[2].y) / 3.0 - GROUND_HEIGHT;
        if let Some((snow, from)) = self.snow
            && height >= from
            && slope < self.steep * 1.5
        {
            return snow;
        }
        if slope >= self.steep {
            return self.rock;
        }
        // Up to 12 % toward the rock's color at mid-slope, and a little lighter up high: the
        // hills read as hills, not as a flat green.
        let mix = 0.12 * (slope / self.steep).min(1.0);
        let light = 1.0 + 0.10 * (height / 40.0).min(1.0);
        let (g, r) = (self.ground, self.rock);
        let blend = |a: f32, b: f32| ((a + (b - a) * mix) * light).min(1.0);
        (blend(g.0, r.0), blend(g.1, r.1), blend(g.2, r.2))
    }

    /// What is drawn: every raised face, flat-shaded, in its color.
    pub fn mesh(&self) -> MeshData {
        let mut mesh = MeshData::default();
        for corners in self.triangles(false) {
            let (r, g, b) = self.color_of(&corners);
            mesh.triangle(corners, [r, g, b, 1.0]);
        }
        mesh
    }

    /// What is solid: the raised faces within [`COLLIDER_REACH`] of the walls (a trimesh).
    pub fn collider_mesh(&self) -> MeshData {
        let mut mesh = MeshData::default();
        for corners in self.triangles(true) {
            mesh.triangle(corners, [1.0; 4]);
        }
        mesh
    }

    /// The highest point of the terrain above the ground (m).
    pub fn highest(&self) -> f32 {
        self.heights.iter().copied().fold(0.0, f32::max)
    }
}

/// How far from a sample's centerline its walls' outer face is (m).
fn keep_out(sample: &TrackSample) -> f32 {
    wall_offset(sample) + WALL_THICKNESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::tracks::circuits;
    use crate::camera::CameraTuning;
    use crate::track::TrackData;
    use crate::track::biome::biomes as all_biomes;

    fn circuit(path: &str) -> TrackData {
        let file = format!("{}/assets/{path}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e}"));
        TrackData::from_ron(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Every circuit of the game and its mirror image: (its name, its file as driven, its
    /// terrain).
    fn terrains() -> Vec<(String, TrackData, Terrain)> {
        circuits()
            .iter()
            .flat_map(|entry| {
                let file = circuit(&entry.asset_path());
                [false, true].map(|mirror| {
                    let terrain = file.terrain_built(mirror).expect("a terrain");
                    let data = if mirror {
                        file.mirrored()
                    } else {
                        file.clone()
                    };
                    let name = format!("{}{}", file.name, if mirror { " (miroir)" } else { "" });
                    (name, data, terrain)
                })
            })
            .collect()
    }

    #[test]
    fn every_biome_has_a_terrain() {
        for biome in all_biomes() {
            let terrain = biome
                .terrain
                .unwrap_or_else(|| panic!("biome {}: no terrain", biome.name));
            terrain.validate().unwrap();
            assert!(
                terrain.height >= 20.0,
                "biome {}: a terrain of {} m is no real height",
                biome.name,
                terrain.height
            );
        }
    }

    #[test]
    fn broken_terrains_are_refused() {
        let good = all_biomes()[0].terrain.unwrap();
        let cases: [(&str, fn(&mut TerrainData)); 6] = [
            ("height", |t| t.height = 500.0),
            ("clear", |t| t.clear = MIN_CLEAR - 1.0),
            ("rise", |t| t.rise = 2.0),
            ("ridges", |t| t.ridges = 1.5),
            ("scale", |t| t.scale = 5.0),
            ("color", |t| t.rock = (1.5, 0.0, 0.0)),
        ];
        for (what, change) in cases {
            let mut bad = good;
            change(&mut bad);
            let error = bad.validate().unwrap_err();
            assert!(error.contains(what), "{what}: {error}");
        }
    }

    /// The noise is deterministic, between 0 and 1, and it varies.
    #[test]
    fn the_noise_is_deterministic_and_varied() {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for k in 0..2000 {
            let p = Vec2::new(k as f32 * 7.3 - 3000.0, (k % 37) as f32 * 41.0 - 700.0);
            let n = fbm(42, p, 120.0);
            assert_eq!(n, fbm(42, p, 120.0));
            assert!((0.0..=1.0).contains(&n), "{n}");
            lo = lo.min(n);
            hi = hi.max(n);
        }
        assert!(hi - lo > 0.5, "{lo}..{hi}");
        assert_ne!(
            fbm(1, Vec2::new(10.0, 20.0), 100.0),
            fbm(2, Vec2::new(10.0, 20.0), 100.0)
        );
    }

    /// Issue #195 acceptance: on every circuit of the game and its mirror, the terrain has real
    /// heights (its highest point at least 15 m above the ground, and at least 12 m of difference
    /// within 150 m of the walls, where one sees it from the road), and it never touches the road
    /// nor its shoulders, walls and skirts: under the ground within its flat band of the walls of
    /// the road, of the shortcuts, of the lakes. The mirror is its exact image.
    #[test]
    fn every_circuit_has_real_heights_that_never_touch_the_road() {
        let mut failures = Vec::new();
        let all = terrains();
        assert!(all.len() >= 20);
        for (name, data, terrain) in &all {
            let style = data.terrain().unwrap();
            let center = CenterLine::of(data);
            let near: Vec<f32> = terrain
                .heights
                .iter()
                .zip(&terrain.clearance)
                .filter(|(_, d)| **d < 150.0)
                .map(|(h, _)| *h)
                .collect();
            let (lo, hi) = near
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
            println!(
                "{name}: highest {:.1} m, near the road {:.1} to {:.1} m, {} faces drawn in {} \
                 tiles, {} solid",
                terrain.highest(),
                lo,
                hi,
                terrain.mesh().triangle_count(),
                terrain.mesh().tiles(TILE).len(),
                terrain.collider_mesh().triangle_count(),
            );
            if terrain.highest() < 15.0 || hi - lo < 12.0 {
                failures.push(format!("{name}: no real heights ({lo}..{hi})"));
            }
            // Across the road and well beyond its walls, every meter along it: under the ground.
            let shortcuts: Vec<Shortcut> = data
                .shortcuts
                .iter()
                .map(|s| Shortcut::new(&center, s))
                .collect();
            let road = center.road_samples();
            let paths = shortcuts.iter().flat_map(|s| s.samples().iter());
            for sample in road.iter().chain(paths) {
                let reach = keep_out(sample) + style.clear - 0.5;
                let right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
                for k in -20..=20 {
                    let p = sample.position + right * reach * k as f32 / 20.0;
                    if let Some(y) = terrain.surface(p.xz())
                        && y > GROUND_HEIGHT
                    {
                        failures.push(format!(
                            "{name}: the terrain {:.2} m up at {:.1} m from the road at {:.0}",
                            y - GROUND_HEIGHT,
                            reach * k as f32 / 20.0,
                            sample.distance
                        ));
                        break;
                    }
                }
            }
            for lake in data.lakes.iter().map(Lake::new) {
                for k in 0..=20 {
                    for p in [
                        lake.min
                            .lerp(Vec2::new(lake.max.x, lake.min.y), k as f32 / 20.0),
                        lake.max
                            .lerp(Vec2::new(lake.min.x, lake.max.y), k as f32 / 20.0),
                    ] {
                        if terrain.surface(p).is_some_and(|y| y > GROUND_HEIGHT) {
                            failures.push(format!("{name}: the terrain over a lake at {p}"));
                        }
                    }
                }
            }
        }
        // The mirror: the same heights across the X = 0 plane.
        for pair in all.chunks(2) {
            let ((_, _, a), (name, _, b)) = (&pair[0], &pair[1]);
            for k in 0..400 {
                let p = a.origin
                    + Vec2::new(
                        (k * 37 % 401) as f32 / 401.0 * a.cols as f32 * a.cell,
                        (k * 91 % 397) as f32 / 397.0 * a.rows as f32 * a.cell,
                    );
                let mirrored = Vec2::new(-p.x, p.y);
                if a.surface(p) != b.surface(mirrored) {
                    failures.push(format!("{name}: not the image at {p}"));
                    break;
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Issue #195: a higher terrain never hides the road from the chase camera. On every circuit
    /// and its mirror, every 2 m of the road, the camera where the chase puts it (6 m behind and
    /// 2.4 m above the kart, the kart in the middle of the road or near either edge), the road
    /// from 3 to 60 m ahead (in its middle and near its edges): no sight line passes under the
    /// terrain (stricter than the walls' test, 45 m and 2 %).
    #[test]
    fn the_terrain_never_hides_the_road_from_the_chase_camera() {
        let tuning = CameraTuning::default();
        let mut failures = Vec::new();
        let mut views = 0usize;
        for (name, data, terrain) in terrains() {
            let center = CenterLine::of(&data);
            let mut hidden = 0usize;
            let mut example = None;
            let road = center.road_samples();
            for sample in road.iter().step_by(2) {
                let flat = Vec3::new(sample.forward.x, 0.0, sample.forward.z).normalize_or_zero();
                for kart in [-0.6, 0.0, 0.6] {
                    let at = sample.point(kart * sample.half_width, 0.55);
                    let eye = at - flat * tuning.distance + Vec3::Y * tuning.height;
                    for k in 0..20 {
                        let ahead = 3.0 + 3.0 * k as f32;
                        let distance = sample.distance + ahead;
                        if !center.on_the_road(center.wrap(distance)) {
                            continue;
                        }
                        let s = center.at(distance);
                        for across in [-0.8, 0.0, 0.8] {
                            let target = s.point(across * s.half_width, 0.05);
                            views += 1;
                            if under_the_terrain(&terrain, eye, target) {
                                hidden += 1;
                                example.get_or_insert((sample.distance, ahead));
                            }
                        }
                    }
                }
            }
            if hidden > 0 {
                failures.push(format!(
                    "{name}: {hidden} sight lines under the terrain, first at {example:?}"
                ));
            }
        }
        assert!(views > 100_000, "{views}");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Whether the segment from `eye` to `target` passes under the terrain's surface (every 0.5 m).
    fn under_the_terrain(terrain: &Terrain, eye: Vec3, target: Vec3) -> bool {
        let steps = (eye.distance(target) / 0.5).ceil() as usize;
        (1..steps).any(|k| {
            let p = eye.lerp(target, k as f32 / steps as f32);
            // Under the ground (a lake's basin, the road diving into it), the terrain is flat.
            terrain
                .surface(p.xz())
                .is_some_and(|y| y > p.y && y > GROUND_HEIGHT)
        })
    }

    /// What is solid is what is drawn, near the road: every drawn face within the reach of a
    /// kart is in the collider, and the collider is nothing else.
    #[test]
    fn the_terrain_is_solid_where_a_kart_can_reach_it() {
        for (name, _, terrain) in terrains().into_iter().step_by(5) {
            let drawn = terrain.mesh();
            let solid = terrain.collider_mesh();
            assert!(solid.triangle_count() > 100, "{name}");
            assert!(solid.triangle_count() <= drawn.triangle_count(), "{name}");
            let key = |p: &[f32; 3]| p.map(|v| (v * 1000.0).round() as i64);
            let drawn: std::collections::HashSet<_> = drawn.positions.iter().map(key).collect();
            assert!(
                solid.positions.iter().all(|p| drawn.contains(&key(p))),
                "{name}"
            );
            // Its faces face up.
            for t in solid.indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|k| Vec3::from_array(solid.positions[t[k] as usize]));
                assert!((b - a).cross(c - a).y > 0.0, "{name}");
            }
        }
    }
}
