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
//! - Issue #202: **a terrain that hugs the road** ([`TerrainData::embank`], on the circuits that
//!   ask for it): the ground comes up to the walls of a raised road, [`EMBANK_LIP`] under its
//!   surface, and falls away from them at the slope of an embankment, instead of the skirts down
//!   to a flat ground: the road climbs the side of a mountain, its switchbacks cut into it. It
//!   never rises over the road (nor steeper than 45° above it beyond its walls), and it keeps
//!   every void a kart falls into: beside a side without wall, over a gap, under a risky jump's
//!   flight, the terrain stays [`OPEN_FALL`] meters under the road at least (more than
//!   `RaceTuning::fall_depth`); flat over the lakes and around the anti-gravity zones that bank
//!   (issue #215: a level zone, a big climb, is carried like any road).
//! - Issue #204: **a road cut into the mountain** (`TrackData::cuttings`, on a terrain that
//!   hugs the road): on the side of a cutting, beyond the berm, the relief of the mountain rises
//!   over the road, at [`CUT_RISE`] at most. On a circuit with cuttings, whatever rises beyond the
//!   rules of #202 (the wall, the embankment inside the tight turns) stays under every sight line
//!   of the chase camera (`sight_bound`): the camera never loses the road behind it.

use bevy::prelude::*;
use serde::Deserialize;

use super::anti_gravity::{AntiGravityZone, anti_gravity_at, bank_cos};
use super::biome::{Palette, Rgb};
use super::data::{BridgeData, ShortcutKind};
use super::decor::tight_turns;
use super::far::{FarDecor, FarLayer, GroundRect};
use super::layout::{CenterLine, TrackSample};
use super::mesh::{GROUND_HEIGHT, MeshData, WALL_THICKNESS, wall_offset};
use super::shortcut::{OpenEdge, Shortcut, walled};
use super::water::{Gap, Lake, gap_at};

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

/// Issue #202: under the road, its shoulders and its walls, the terrain that hugs the road stays
/// this far below the road's surface (m), and comes up to there at the outer face of its walls:
/// the walls stand out of it, a kart's wheels never meet it through the road.
pub const EMBANK_LIP: f32 = 0.8;

/// Issue #202: beside a side without wall, over a gap and under a risky jump, the terrain that
/// hugs the road stays at least this far below the road (m): a kart that falls there is seen
/// fallen (`RaceTuning::fall_depth`, 5 m, and a meter more)...
pub const OPEN_FALL: f32 = 6.0;

/// ...over this many meters beyond the road's edge (or the jump's path): where a kart that falls
/// off it lands.
pub const OPEN_REACH: f32 = 16.0;

/// Issue #202: under a risky jump, from the edge of its path out to this many meters (m), the
/// same void: a kart that falls short falls along its path.
pub const JUMP_REACH: f32 = 4.0;

/// Issue #202: the steepest that what bounds the terrain (the road above it, a void beside it)
/// lets it rise away from there (rise over run).
const BOUND_RISE: f32 = 1.0;

/// Issue #204: in a cutting (`TrackData::cuttings`), the steepest the wall of rock rises above
/// the road beyond its berm (rise over run: about 68°, a wall), up to the relief of the mountain.
pub const CUT_RISE: f32 = 2.5;

/// Issue #204: over this many meters at each end of a cutting, its wall comes up from the
/// embankment to its full height (along the road): no sheer face across the road's way.
pub const CUT_FADE: f32 = 16.0;

/// Issue #208: under a bridge (`TrackData::bridges`), the ravine falls away from each abutment at
/// this slope (rise over run, the cuttings' wall: about 68°), as deep as it lets it be (down to
/// the ground at the most)...
pub const RAVINE_RISE: f32 = CUT_RISE;

/// ...and across the road, beyond its floor, its walls rise at this one (about 76°): a gorge,
/// which leaves the roads beside it (a switchback 44 m away) on their embankment.
pub const RAVINE_WALL: f32 = 4.0;

/// Issue #208: the floor of a ravine reaches this far beyond the outer face of the bridge's walls
/// (m) before its sides rise.
pub const RAVINE_REACH: f32 = 2.0;

/// Issue #204: the chase camera the cuttings keep the road in sight of (`CameraTuning`'s
/// defaults, checked by a test): this far behind the kart (m) and this high above it (m), the
/// kart this high above the road (m).
pub const CHASE_DISTANCE: f32 = 6.0;
pub const CHASE_HEIGHT: f32 = 2.4;
const KART_HEIGHT: f32 = 0.55;

/// Issue #204: where the camera looks from (the kart across the road, a share of its half width)
/// and at (the road ahead, every [`SIGHT_STEP`] meters up to [`SIGHT_REACH`], across it): wider,
/// finer and further than the camera test, so that what passes between them is in sight too.
const SIGHT_KARTS: [f32; 5] = [-0.8, -0.4, 0.0, 0.4, 0.8];
const SIGHT_TARGETS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];
const SIGHT_STEP: f32 = 2.0;
const SIGHT_REACH: f32 = 66.0;

/// Issue #204: how far under a sight line of the chase camera a cutting stays (m).
const SIGHT_MARGIN: f32 = 0.3;

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
    /// Issue #202: the terrain hugs the road, falling away from its walls at this slope (rise
    /// over run: 0.6 is an embankment of about 31°); 0 (the default): it stays flat under the
    /// ground beside the road, as in #195.
    #[serde(default)]
    pub embank: f32,
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
        if self.embank != 0.0 {
            within("embank", self.embank, (0.2, 1.5))?;
        }
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
    /// The terrain of a circuit (its file, not mirrored: `center` its centerline, `shortcuts`,
    /// `lakes` and `edges` the file's), `mirrored` for the mirror mode. None without a terrain.
    pub fn build(
        terrain: &TerrainData,
        center: &CenterLine,
        (shortcuts, lakes, edges): (&[Shortcut], &[Lake], &RoadEdges),
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
        if terrain.embank > 0.0 {
            let grid = Grid { origin, cols, rows };
            let wide = start + terrain.rise;
            let shape = (terrain.embank, wide, terrain.height);
            let hugged = hug(shape, &grid, center, (shortcuts, lakes, edges));
            for ((h, bank), bound) in heights.iter_mut().zip(&hugged.bank).zip(&hugged.bound) {
                *h = h.max(bank.min(*bound));
            }
            // Issue #204: in a cutting, the relief of the mountain right up to the berm, under
            // its bound (the wall's slope from the road, the voids, the tight turns) and under
            // every sight line of the chase camera.
            // Inside the tight turns too, the embankment then rises to its road's level (no
            // terrace at the lowest of the turn and its exit), under the same sight lines.
            if let Some(cut) = &hugged.cut {
                let sight = sight_bound(&grid, center);
                for k in 0..cols * rows {
                    let w = cut.weight[k];
                    let relief = if w > 0.0 {
                        terrain.height
                            * terrain.shape(fbm(seed, at(k % cols, k / cols), terrain.scale))
                            * w
                            - SINK
                    } else {
                        -SINK
                    };
                    let raised = relief.max(hugged.bank[k]).min(cut.bound[k]).min(sight[k]);
                    heights[k] = heights[k].max(raised);
                }
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

/// Issue #202: what the terrain that hugs the road keeps (the file's, as built along its
/// centerline): its sides without wall, its gaps, its anti-gravity zones.
#[derive(Clone, Copy, Debug, Default)]
pub struct RoadEdges<'a> {
    pub open: &'a [OpenEdge],
    pub gaps: &'a [Gap],
    pub zones: &'a [AntiGravityZone],
    /// Issue #204: its cuttings (the side cut into the hillside).
    pub cuts: &'a [OpenEdge],
    /// Issue #208: its bridges (a ravine under each).
    pub bridges: &'a [Bridge],
}

/// Issue #208: a bridge over a ravine, as built along the centerline (m).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bridge {
    pub from: f32,
    pub to: f32,
}

impl Bridge {
    pub fn new(center: &CenterLine, data: &BridgeData) -> Self {
        Self {
            from: center.wrap(center.param_to_distance(data.from)),
            to: center.wrap(center.param_to_distance(data.to)),
        }
    }

    /// Its length along the centerline (m).
    pub fn length(&self, center: &CenterLine) -> f32 {
        center.wrap(self.to - self.from)
    }

    /// How far into it the centerline distance `distance` (any lap) is (m), if it is on it.
    pub fn along(&self, center: &CenterLine, distance: f32) -> Option<f32> {
        let into = center.wrap(distance - self.from);
        (into <= self.length(center)).then_some(into)
    }

    /// How deep the ravine under it is at `distance` (m under the road; `None` off the bridge):
    /// its sides rise at [`RAVINE_RISE`] from each abutment (the ground bounds it).
    pub fn depth(&self, center: &CenterLine, distance: f32) -> Option<f32> {
        self.along(center, distance)
            .map(|into| RAVINE_RISE * into.min(self.length(center) - into))
    }
}

/// Issue #208: the bridge at centerline distance `distance`, if one is there.
pub fn bridge_at<'a>(
    bridges: &'a [Bridge],
    center: &CenterLine,
    distance: f32,
) -> Option<&'a Bridge> {
    bridges.iter().find(|b| b.along(center, distance).is_some())
}

/// Issue #202: the terrain that hugs the road, at every vertex of a grid (m above the ground).
struct Hugged {
    /// The embankment, and what bounds it.
    bank: Vec<f32>,
    bound: Vec<f32>,
    /// Issue #204: with cuttings, where they are and what bounds them.
    cut: Option<Cut>,
}

/// Issue #204: the cuttings of a road, at every vertex of a grid.
struct Cut {
    /// How much of the mountain's relief stands there (0: none, 1: all of it): beside a cutting,
    /// on its side, fading in and out at its ends ([`CUT_FADE`]).
    weight: Vec<f32>,
    /// What bounds it (m above the ground): the bound of the embankment, the road's level rising
    /// at [`CUT_RISE`] beyond the berm of a cutting on its side instead of 45°, and no terrace
    /// inside the tight turns (the chase camera's sight bounds them instead, `sight_bound`).
    bound: Vec<f32>,
}

/// The vertices of a heightfield: its first one (file frame, ground plane), how many across and
/// along, [`CELL`] apart.
struct Grid {
    origin: Vec2,
    cols: usize,
    rows: usize,
}

impl Grid {
    /// Issue #204: stamps a bound (the lowest) into the embankment's `bound` and, with cuttings,
    /// into theirs (`cut`, empty without).
    fn bound(
        &self,
        bound: &mut [f32],
        cut: &mut [f32],
        disc: (Vec2, f32),
        value: impl Fn(Vec2) -> f32,
    ) {
        self.stamp(bound, disc, true, &value);
        if !cut.is_empty() {
            self.stamp(cut, disc, true, &value);
        }
    }

    /// Issue #204: every vertex of every cell the segment `from` → `to` crosses (seen from
    /// above) gets the lowest height of the segment over that cell (m above the ground, less
    /// [`SIGHT_MARGIN`]) if it is lower than what it has.
    fn under_line(&self, values: &mut [f32], from: Vec3, to: Vec3) {
        let a = (from.xz() - self.origin) / CELL;
        let d = (to.xz() - self.origin) / CELL - a;
        let height = |t: f32| from.y + (to.y - from.y) * t - GROUND_HEIGHT - SIGHT_MARGIN;
        let (mut i, mut j) = (a.x.floor() as i64, a.y.floor() as i64);
        // Where the segment crosses the next column and the next row (its share), and how much
        // of it a cell takes.
        let first = |a: f32, d: f32, cell: i64| {
            if d > 0.0 {
                ((cell + 1) as f32 - a) / d
            } else if d < 0.0 {
                (cell as f32 - a) / d
            } else {
                f32::INFINITY
            }
        };
        let (mut next_i, mut next_j) = (first(a.x, d.x, i), first(a.y, d.y, j));
        let (delta_i, delta_j) = (1.0 / d.x.abs(), 1.0 / d.y.abs());
        let (step_i, step_j) = (
            if d.x > 0.0 { 1 } else { -1 },
            if d.y > 0.0 { 1 } else { -1 },
        );
        let mut t = 0.0;
        loop {
            let out = next_i.min(next_j).min(1.0);
            let low = height(t).min(height(out));
            for (ci, cj) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
                if ci < 0 || cj < 0 || ci as usize >= self.cols || cj as usize >= self.rows {
                    continue;
                }
                let slot = &mut values[cj as usize * self.cols + ci as usize];
                *slot = slot.min(low);
            }
            if out >= 1.0 {
                break;
            }
            t = out;
            if next_i < next_j {
                i += step_i;
                next_i += delta_i;
            } else {
                j += step_j;
                next_j += delta_j;
            }
        }
    }

    /// Every vertex within `radius` of `p` (a square around it) gets `value(vertex)` if it is
    /// lower (`lowest`) or higher (else) than what it has.
    fn stamp(
        &self,
        values: &mut [f32],
        (p, radius): (Vec2, f32),
        lowest: bool,
        value: impl Fn(Vec2) -> f32,
    ) {
        let lo = ((p - Vec2::splat(radius) - self.origin) / CELL).floor();
        let hi = ((p + Vec2::splat(radius) - self.origin) / CELL).ceil();
        let (i0, j0) = (lo.x.max(0.0) as usize, lo.y.max(0.0) as usize);
        let (i1, j1) = (
            (hi.x.max(0.0) as usize).min(self.cols - 1),
            (hi.y.max(0.0) as usize).min(self.rows - 1),
        );
        for j in j0..=j1 {
            for i in i0..=i1 {
                let v = self.origin + Vec2::new(i as f32, j as f32) * CELL;
                let slot = &mut values[j * self.cols + i];
                let x = value(v);
                if (lowest && x < *slot) || (!lowest && x > *slot) {
                    *slot = x;
                }
            }
        }
    }
}

/// The level of the terrain that hugs the road at a sample (m above the ground): [`EMBANK_LIP`]
/// under the lower of its surface at the walls and in its middle.
fn hug_level(sample: &TrackSample) -> f32 {
    let w = wall_offset(sample);
    let low = sample
        .point(-w, 0.0)
        .y
        .min(sample.point(w, 0.0).y)
        .min(sample.position.y);
    low - GROUND_HEIGHT - EMBANK_LIP
}

/// Issue #215: a road's level steeper than this between two samples (rise over horizontal run)
/// is a step (a drop), not a slope: the steepest climb of a level anti-gravity zone stays well
/// under it.
const STEP_SLOPE: f32 = 1.5;

/// Issue #215: how much lower than its measured sag the bound that follows the road stays (m):
/// the chords are measured from the samples of the road, a meter apart (a kink between two of
/// them, where the road comes out of the ground, sags a few centimeters more).
const SAG_MARGIN: f32 = 0.1;

/// Issue #215: the level of the terrain that hugs a line (the road, or a shortcut's path) along
/// it, as a bound that **follows its bumps**: a vertex takes the line's exact level where it
/// stands along it (its horizontal run), less the sag of its triangles there. A triangle of the
/// terrain spans `S` along the line, a cell's side where the line runs along the grid to its
/// diagonal at 45° to it: over a stretch that bends up (a dip, the foot of a bump) by `κ` (the
/// second derivative of the level over the run), its corners on the level stand up to
/// `κ S² / 8` above it in between, as much as the middle of a chord of `S` stands over the level
/// (measured, not supposed); over a stretch that bends down (a crest), they stand under it.
/// Within a diagonal of a step (a drop, and where the road comes out of the ground: the terrain
/// cannot sag under the ground there), the lowest level around. On a straight climb, exactly
/// the level (as the tangent of #202); over a bump, within some decimeters of it (the tangent of
/// #202 dropped meters under it, and the embankment was lost along the whole bump).
struct Profile {
    /// For each sample: its level (`hug_level`), the horizontal run to the next one (m), and
    /// whether the line goes on to it (false at its end and across a break in its samples).
    level: Vec<f32>,
    run: Vec<f32>,
    linked: Vec<bool>,
    /// For each sample: how far under the level the bound stays near it (the sag) for the
    /// triangles that span a cell's side along the line, and a cell's diagonal; or, within a
    /// diagonal of a step, the lowest level there.
    sag: Vec<(f32, f32)>,
    floor: Vec<Option<f32>>,
}

impl Profile {
    fn new(line: &[TrackSample], closed: bool, diagonal: f32) -> Self {
        let n = line.len();
        // Never under the ground's sink, as the terrain. Where the road comes out of the ground
        // (the foot of a bump in the valley), the terrain cannot follow it under the sink: that
        // kink is taken as a step.
        let level: Vec<f32> = line.iter().map(|s| hug_level(s).max(-SINK)).collect();
        let grounded: Vec<bool> = line.iter().map(|s| hug_level(s) <= -SINK).collect();
        let mut run = vec![0.0; n];
        let mut linked = vec![false; n];
        for i in 0..n {
            let j = i + 1;
            if j == n && !closed {
                continue;
            }
            let (a, b) = (&line[i], &line[j % n]);
            // A break in the samples (an open circuit's road, in the order of its samples).
            if a.position.distance(b.position) > 3.0 {
                continue;
            }
            run[i] = a.position.xz().distance(b.position.xz());
            linked[i] = true;
        }
        let mut profile = Self {
            level,
            run,
            linked,
            sag: vec![(0.0, 0.0); n],
            floor: vec![None; n],
        };
        // How far the middle of a chord across each sample (half of it on each side) stands over
        // the level there: the most a triangle's corners on the level stand over it in between
        // (`κ S² / 8` for a chord of `S` where the level bends up by `κ`, none where it bends
        // down), for a chord of a cell's side and of its diagonal.
        let rise = |half: f32| -> Vec<f32> {
            (0..profile.level.len())
                .map(
                    |i| match (profile.level_at(i, -half), profile.level_at(i, half)) {
                        (Some(back), Some(ahead)) => {
                            ((back + ahead) / 2.0 - profile.level[i]).max(0.0)
                        }
                        _ => 0.0,
                    },
                )
                .collect()
        };
        let (side, across) = (rise(CELL / 2.0), rise(diagonal / 2.0));
        let half = diagonal / 2.0;
        // A vertex's triangles span a diagonal on each side of it: their chords' middles within
        // half a diagonal (and a meter more for the vertices between two samples).
        let reach = half + 1.0;
        for i in 0..n {
            let (mut sag, mut lowest, mut step) = ((side[i], across[i]), profile.level[i], false);
            for dir in [-1i64, 1] {
                let (mut k, mut gone) = (i, 0.0);
                while gone < reach {
                    let Some(j) = profile.next(k, dir) else {
                        break;
                    };
                    let link = if dir > 0 { k } else { j };
                    let length = profile.run[link];
                    gone += length;
                    k = j;
                    sag = (sag.0.max(side[k]), sag.1.max(across[k]));
                }
                // The lowest level within a diagonal and a meter, if a step is there.
                let (mut k, mut gone) = (i, 0.0);
                while gone < diagonal + 1.0 {
                    let Some(j) = profile.next(k, dir) else {
                        break;
                    };
                    let link = if dir > 0 { k } else { j };
                    let length = profile.run[link];
                    step |= (profile.level[j] - profile.level[k]).abs()
                        > STEP_SLOPE * length.max(0.05)
                        || grounded[j] != grounded[k];
                    gone += length;
                    k = j;
                    lowest = lowest.min(profile.level[k]);
                }
            }
            // A few centimeters more: the chords are measured from the samples, a meter apart.
            profile.sag[i] = (sag.0 + SAG_MARGIN, sag.1 + SAG_MARGIN);
            profile.floor[i] = step.then_some(lowest);
        }
        profile
    }

    /// The sample after (`dir` 1) or before (-1) sample `k`, if the line goes on to it.
    fn next(&self, k: usize, dir: i64) -> Option<usize> {
        let n = self.level.len();
        if dir > 0 {
            self.linked[k].then_some((k + 1) % n)
        } else {
            let j = (k + n - 1) % n;
            (self.linked[j] && (k > 0 || self.linked[n - 1])).then_some(j)
        }
    }

    /// The line's level at `along` meters of horizontal run from sample `i` (backward if
    /// negative), between its samples; None past an end of the line.
    fn level_at(&self, i: usize, along: f32) -> Option<f32> {
        self.between(i, along)
            .map(|(k, j, t)| self.level[k] + (self.level[j] - self.level[k]) * t)
    }

    /// The two samples `along` meters of horizontal run from sample `i` is between, and how far
    /// from the first to the second; None past an end of the line.
    fn between(&self, i: usize, along: f32) -> Option<(usize, usize, f32)> {
        let dir = if along < 0.0 { -1 } else { 1 };
        let (mut k, mut gone) = (i, 0.0);
        loop {
            let j = self.next(k, dir)?;
            let length = self.run[if dir > 0 { k } else { j }];
            if gone + length >= along.abs() {
                let t = if length > 1e-4 {
                    (along.abs() - gone) / length
                } else {
                    1.0
                };
                return Some((k, j, t));
            }
            gone += length;
            k = j;
        }
    }

    /// The bound of the terrain that hugs the line at `along` meters of horizontal run from
    /// sample `i` (m above the ground), its triangles `span` meters long along it at most (a
    /// cell's side to its diagonal); None past an end of the line.
    fn at(&self, i: usize, along: f32, span: f32) -> Option<f32> {
        let (k, j, t) = self.between(i, along)?;
        let level = self.level[k] + (self.level[j] - self.level[k]) * t;
        // Between the sags of a side and of a diagonal, linearly: over them where the level bends
        // smoothly (the sag grows as the square of the span), exactly at a kink (linearly).
        let share = ((span - CELL) / (CELL * (std::f32::consts::SQRT_2 - 1.0))).clamp(0.0, 1.0);
        let sag = |(side, across): (f32, f32)| side + (across - side) * share;
        Some(match (self.floor[k], self.floor[j]) {
            (None, None) => level - sag(self.sag[k]).max(sag(self.sag[j])),
            (a, b) => level.min(a.unwrap_or(f32::MAX)).min(b.unwrap_or(f32::MAX)),
        })
    }
}

/// Issue #202: the terrain that hugs the road, at every vertex of `grid` (m above the ground):
/// the embankment (the highest of the road's levels, flat for a cell's diagonal beyond its walls
/// (a berm), then falling away at `slope`),
/// and what bounds it (the lowest of: the road's levels, rising away from its walls at
/// [`BOUND_RISE`]; inside a tight turn, the lowest of its road; [`OPEN_FALL`] under a side
/// without wall, a gap, a risky jump; the ground on a lake and around an anti-gravity zone that
/// banks: issue #215, a level zone is carried like any road).
///
/// A vertex takes the road's level from every sample less than a cell's diagonal away along the
/// road (its window), each one carried to the vertex along the road (issue #215, [`Profile`]: the
/// road's level where the vertex stands along it, less the sag of a triangle where the road bends
/// up, the lowest level near a step; #202 carried it along the road's tangent, meters under a
/// bump). The bound is the lowest of them, so that every triangle over the road (its corners
/// within a diagonal of any point of it) stays under that point's level: the terrain never comes
/// through the road, wherever the road bends up or steps down.
fn hug(
    (slope, wide, ceiling): (f32, f32, f32),
    grid: &Grid,
    center: &CenterLine,
    (shortcuts, lakes, edges): (&[Shortcut], &[Lake], &RoadEdges),
) -> Hugged {
    let n = grid.cols * grid.rows;
    let mut bank = vec![-SINK; n];
    let mut bound = vec![f32::MAX; n];
    // Issue #204: the cuttings (none: nothing of theirs is computed).
    let cutting = !edges.cuts.is_empty();
    let mut weight = vec![0.0; if cutting { n } else { 0 }];
    let mut cut_bound = vec![f32::MAX; weight.len()];
    let diagonal = CELL * std::f32::consts::SQRT_2;
    let window = diagonal + 0.5;
    let road = center.road_samples();
    let paths: Vec<&[TrackSample]> = shortcuts
        .iter()
        .filter(|s| s.kind != ShortcutKind::Jump)
        .map(|s| s.samples())
        .collect();
    let lines: Vec<&[TrackSample]> = std::iter::once(road.as_slice()).chain(paths).collect();
    let top = lines
        .iter()
        .flat_map(|line| line.iter())
        .map(hug_level)
        .fold(0.0, f32::max)
        + 1.0;
    // Issue #204: a cutting's wall rises up to the relief of the mountain, over the road: every
    // bound reaches as far as it may bound it.
    let top = if cutting { top.max(ceiling) } else { top };
    // Issue #215: a level anti-gravity zone (a big climb) is carried like any road; and a steep
    // road is not special for its slope, only for its banking.
    let special = |s: &TrackSample| {
        gap_at(edges.gaps, center, s.distance).is_some()
            || anti_gravity_at(edges.zones, center, s.distance).is_some_and(|z| !z.level)
            || bank_cos(s) < 0.9
    };
    // What rises from `base` at `p` beyond `keep` meters, at the bound's slope (a disc), as far
    // as it bounds anything.
    let rising = |p: Vec2, keep: f32, base: f32| {
        (
            (p, keep + (top - base).max(0.0) / BOUND_RISE),
            move |v: Vec2| base + BOUND_RISE * (v.distance(p) - keep).max(0.0),
        )
    };
    for (line_index, line) in lines.iter().enumerate() {
        let on_road = line_index == 0;
        // Issue #215: its level along it, as a bound that follows its bumps.
        let profile = Profile::new(line, on_road && center.open().is_none(), diagonal);
        let profile = &profile;
        for (i, s) in line.iter().enumerate() {
            let p = s.position.xz();
            let keep = keep_out(s);
            if on_road && special(s) {
                // Issue #211: over a gap, its void reaches as far as beside a side without wall
                // (`OPEN_REACH` beyond the walls): the embankment never comes up into it.
                let reach = if gap_at(edges.gaps, center, s.distance).is_some() {
                    wall_offset(s) + OPEN_REACH
                } else {
                    keep
                };
                let (disc, value) = rising(p, reach + diagonal, -SINK);
                grid.bound(&mut bound, &mut cut_bound, disc, value);
                continue;
            }
            // The road's slope (rise over horizontal run) at the sample.
            let (a, b) = (
                &line[i.saturating_sub(1)],
                &line[(i + 1).min(line.len() - 1)],
            );
            let run = a.position.xz().distance(b.position.xz());
            let gradient = if run > 1e-3 {
                (b.position.y - a.position.y) / run
            } else {
                0.0
            };
            let forward = Vec2::new(s.forward.x, s.forward.z).normalize_or(Vec2::Y);
            let across = Vec2::new(-forward.y, forward.x);
            let level = hug_level(s);
            // Issue #215: how long the grid's triangles are along the road there (their cell's
            // side across the grid, its diagonal at 45° to it).
            let span = CELL * (forward.x.abs() + forward.y.abs());
            // The vertex's place from the sample: along the road (in its window, or none), across.
            // Issue #215: the line's level where the vertex stands along it, less its sag (the
            // tangent of #202 past an end of the line).
            let place = move |v: Vec2| {
                let d = v - p;
                let along = d.dot(forward);
                (along.abs() <= window).then(|| {
                    let at = profile
                        .at(i, along, span)
                        .unwrap_or(level + gradient * along);
                    (at, d.dot(across).abs())
                })
            };
            let reach = keep + diagonal + (top - level).max(0.0) / BOUND_RISE;
            let rise = |level: f32, lateral: f32, slope: f32| {
                level.max(-SINK) + slope * (lateral - keep - diagonal).max(0.0)
            };
            grid.stamp(&mut bound, (p, reach), true, |v| match place(v) {
                Some((level, lateral)) => rise(level, lateral, BOUND_RISE),
                None => f32::MAX,
            });
            // Issue #204: on the side of a cutting, its wall rises at `CUT_RISE` beyond the berm
            // (and the mountain's relief stands there, `wide` meters beyond the walls at most).
            if cutting {
                let cut = |sign: f32| cut_weight(edges.cuts, center, s.distance, sign);
                let sides = [cut(-1.0), cut(1.0)];
                let side = move |v: Vec2| sides[usize::from((v - p).dot(across) > 0.0)];
                grid.stamp(&mut cut_bound, (p, reach), true, |v| match place(v) {
                    Some((level, lateral)) if on_road && side(v) > 0.0 => {
                        rise(level, lateral, CUT_RISE)
                    }
                    Some((level, lateral)) => rise(level, lateral, BOUND_RISE),
                    None => f32::MAX,
                });
                if on_road && sides.iter().any(|w| *w > 0.0) {
                    grid.stamp(&mut weight, (p, keep + wide), false, |v| match place(v) {
                        Some((_, lateral)) if lateral <= keep + wide => side(v),
                        _ => 0.0,
                    });
                }
            }
            // Issue #208: under a bridge, a ravine: as deep as its sides let it be from the
            // abutments (down to the ground), its floor across the deck and [`RAVINE_REACH`]
            // beyond, its walls rising at [`RAVINE_WALL`] beyond it.
            // Its depth is the bridge's at the vertex's place along the road (none beyond its
            // abutments): the sample's window never carries it past them.
            if let Some(bridge) = on_road
                .then(|| bridge_at(edges.bridges, center, s.distance))
                .flatten()
            {
                let floor = keep + RAVINE_REACH;
                let depth = |v: Vec2| {
                    let along = (v - p).dot(forward);
                    bridge.depth(center, s.distance + along).unwrap_or(0.0)
                };
                let deepest =
                    bridge.depth(center, s.distance).unwrap_or(0.0) + RAVINE_RISE * window;
                let low = (level - deepest).max(-SINK);
                let disc = (p, floor + (top - low).max(0.0) / RAVINE_WALL);
                grid.bound(&mut bound, &mut cut_bound, disc, |v: Vec2| match place(v) {
                    Some((level, lateral)) => {
                        (level - depth(v)).max(-SINK) + RAVINE_WALL * (lateral - floor).max(0.0)
                    }
                    None => f32::MAX,
                });
            }
            if level > 0.0 {
                let berm = keep + diagonal;
                grid.stamp(
                    &mut bank,
                    (p, berm + level / slope),
                    false,
                    |v| match place(v) {
                        Some((level, lateral)) => level - slope * (lateral - berm).max(0.0),
                        None => -SINK,
                    },
                );
            }
            if !on_road {
                continue;
            }
            // A side without wall: a void beside it, from the middle of the road out.
            for sign in [-1.0f32, 1.0] {
                if walled(edges.open, center, s.distance, sign) {
                    continue;
                }
                let base = level + EMBANK_LIP - OPEN_FALL;
                let keep = wall_offset(s) + OPEN_REACH + diagonal;
                let disc = (p, keep + top / BOUND_RISE);
                grid.bound(&mut bound, &mut cut_bound, disc, |v: Vec2| {
                    let d = v - p;
                    let along = d.dot(forward);
                    let lateral = d.dot(across) * sign;
                    if along.abs() > window || lateral < 0.0 {
                        return f32::MAX;
                    }
                    (base + gradient * along).max(-SINK) + BOUND_RISE * (lateral - keep).max(0.0)
                });
            }
        }
    }
    // Inside a tight turn, no higher than the lowest of its road (GD § 16, règle 3: a change of
    // level never hides a tight turn): where the road goes down out of it, the slope inside does
    // not stand in front of the road below.
    for turn in tight_turns(center).iter().step_by(3) {
        let (c, r) = (turn.center, turn.radius);
        // The level of the turn's road there and out of it (a quarter of its circle on): inside
        // it, the lowest of them.
        let Some(at) = road.iter().find(|s| s.position.xz() == turn.at) else {
            continue;
        };
        let out = r * std::f32::consts::FRAC_PI_2;
        let lowest = road
            .iter()
            .filter(|s| center.wrap(s.distance - at.distance) <= out)
            .map(hug_level)
            .fold(hug_level(at), f32::min);
        // Near the turn itself (closer to its sample than its radius: a gentle turn's circle
        // reaches far across the infield).
        let (disc, value) = rising(c, r, lowest.max(-SINK));
        let near = turn.at;
        // Issue #204: with cuttings, the chase camera's sight bounds what rises inside a tight
        // turn (`sight_bound`), not this terrace.
        grid.stamp(&mut bound, disc, true, |v: Vec2| {
            if v.distance(near) <= r {
                value(v)
            } else {
                f32::MAX
            }
        });
    }
    // Under a risky jump's flight (and its ramp): a void.
    for jump in shortcuts.iter().filter(|s| s.kind == ShortcutKind::Jump) {
        let base = (jump.low - GROUND_HEIGHT - OPEN_FALL).max(-SINK);
        // Only where it has left the road (its line-up and its landing are on it).
        let beyond = |s: &&TrackSample| {
            let near = center.samples()[center.nearest_index(s.position)];
            near.position.xz().distance(s.position.xz()) > wall_offset(&near)
        };
        for s in jump.samples().iter().filter(beyond) {
            let keep = s.half_width + JUMP_REACH + diagonal;
            let (disc, value) = rising(s.position.xz(), keep, base);
            grid.bound(&mut bound, &mut cut_bound, disc, value);
        }
    }
    for lake in lakes {
        let (lo, hi) = (lake.min, lake.max);
        let middle = (lo + hi) / 2.0;
        let radius = (hi - lo).length() / 2.0 + diagonal + top / BOUND_RISE;
        grid.bound(&mut bound, &mut cut_bound, (middle, radius), |v: Vec2| {
            let d = (lo - v).max(v - hi).max(Vec2::ZERO).length();
            -SINK + BOUND_RISE * (d - diagonal).max(0.0)
        });
    }
    let cut = cutting.then_some(Cut {
        weight,
        bound: cut_bound,
    });
    Hugged { bank, bound, cut }
}

/// Issue #204: how much of a cutting's wall stands at centerline distance `distance` on the side
/// `sign` (-1 left, 1 right): 1 in a cutting on that side, coming in and going out over
/// [`CUT_FADE`] meters at its ends; 0 elsewhere.
fn cut_weight(cuts: &[OpenEdge], center: &CenterLine, distance: f32, sign: f32) -> f32 {
    cuts.iter()
        .filter(|c| c.side.opens(sign) && c.contains(center, distance))
        .map(|c| {
            let into = center.wrap(distance - c.from);
            let left = center.wrap(c.to - distance);
            smoothstep(0.0, CUT_FADE, into.min(left))
        })
        .fold(0.0, f32::max)
}

/// Issue #204: the lowest of the sight lines of the chase camera over every vertex of `grid` (m
/// above the ground, [`SIGHT_MARGIN`] under them; `f32::MAX` where none passes): from every
/// sample of the road, the kart across it
/// ([`SIGHT_KARTS`]), to the road ahead ([`SIGHT_STEP`] to [`SIGHT_REACH`], across it). A
/// terrain whose every vertex is under it never comes over a sight line: each line bounds the four
/// corners of every cell it crosses by its lowest point over that cell.
fn sight_bound(grid: &Grid, center: &CenterLine) -> Vec<f32> {
    let mut sight = vec![f32::MAX; grid.cols * grid.rows];
    let steps = (SIGHT_REACH / SIGHT_STEP).round() as usize;
    for sample in &center.road_samples() {
        let flat = Vec3::new(sample.forward.x, 0.0, sample.forward.z).normalize_or_zero();
        let eyes = SIGHT_KARTS.map(|kart| {
            sample.point(kart * sample.half_width, KART_HEIGHT) - flat * CHASE_DISTANCE
                + Vec3::Y * CHASE_HEIGHT
        });
        for k in 1..=steps {
            let distance = sample.distance + k as f32 * SIGHT_STEP;
            if !center.on_the_road(center.wrap(distance)) {
                continue;
            }
            let s = center.at(distance);
            for across in SIGHT_TARGETS {
                let target = s.point(across * s.half_width, 0.0);
                for eye in &eyes {
                    grid.under_line(&mut sight, *eye, target);
                }
            }
        }
    }
    sight
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
    use crate::race::RaceTuning;
    use crate::track::TrackData;
    use crate::track::data::MIN_BRIDGE;

    /// Issue #202: how much less than [`EMBANK_LIP`] the terrain that hugs the road may keep under
    /// it (m): its triangles, 6 m wide, against a road that curves and dips.
    const HUG_SLACK: f32 = 0.15;
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
        let cases: [(&str, fn(&mut TerrainData)); 7] = [
            ("height", |t| t.height = 500.0),
            ("clear", |t| t.clear = MIN_CLEAR - 1.0),
            ("rise", |t| t.rise = 2.0),
            ("ridges", |t| t.ridges = 1.5),
            ("scale", |t| t.scale = 5.0),
            ("color", |t| t.rock = (1.5, 0.0, 0.0)),
            ("embank", |t| t.embank = 3.0),
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
    /// the road, of the shortcuts, of the lakes. Issue #202, where the terrain hugs the road:
    /// under the road and its walls, [`EMBANK_LIP`] under the road (to [`HUG_SLACK`]); beyond
    /// them, within the flat band, never above the road nor steeper than 45° over it; under a
    /// risky jump's flight, `fall_depth` under it. The mirror is its exact image.
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
            // Issue #202, a terrain that hugs the road: under the road, its shoulders and its
            // walls, `EMBANK_LIP` under the road's surface at least; beyond them, never above it
            // nor steeper than 45° over it; under a risky jump, a void (`fall_depth` under it).
            let fall = RaceTuning::default().fall_depth;
            let shortcuts: Vec<Shortcut> = data
                .shortcuts
                .iter()
                .map(|s| Shortcut::new(&center, s))
                .collect();
            // Issue #204: beside a cutting, on its side, its wall: never steeper than `CUT_RISE`
            // over the road.
            let cuts: Vec<OpenEdge> = data
                .cuttings
                .iter()
                .map(|e| OpenEdge::new(&center, e))
                .collect();
            let road = center.road_samples();
            let paths = shortcuts.iter().flat_map(|s| {
                let jump = (s.kind == ShortcutKind::Jump).then_some(s.low);
                s.samples().iter().map(move |sample| (sample, jump, false))
            });
            for (sample, jump, on_road) in road.iter().map(|s| (s, None, true)).chain(paths) {
                let keep = keep_out(sample);
                let reach = keep + style.clear - 0.5;
                let right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
                let surface = hug_level(sample) + GROUND_HEIGHT + EMBANK_LIP;
                for k in -20..=20 {
                    let lateral = reach * k as f32 / 20.0;
                    let limit = match jump {
                        _ if style.embank <= 0.0 => GROUND_HEIGHT,
                        Some(_) if on_the_road(&center, sample) => continue,
                        Some(low) if lateral.abs() <= sample.half_width + JUMP_REACH => {
                            GROUND_HEIGHT.max(low - fall)
                        }
                        Some(_) => continue,
                        None if lateral.abs() <= keep => {
                            GROUND_HEIGHT.max(surface - EMBANK_LIP + HUG_SLACK)
                        }
                        None => {
                            let side = lateral.signum();
                            let rise = if on_road
                                && cut_weight(&cuts, &center, sample.distance, side) > 0.0
                            {
                                CUT_RISE
                            } else {
                                1.0
                            };
                            GROUND_HEIGHT.max(surface + rise * (lateral.abs() - keep))
                        }
                    };
                    let p = sample.position + right * lateral;
                    if let Some(y) = terrain.surface(p.xz())
                        && y > limit
                    {
                        failures.push(format!(
                            "{name}: the terrain {:.2} m up at {:.1} m from the road at {:.0} \
                             (at most {:.2})",
                            y - GROUND_HEIGHT,
                            lateral,
                            sample.distance,
                            limit - GROUND_HEIGHT
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
    /// terrain (stricter than the walls' test, 45 m and 2 %). Issue #202: a sight line under the
    /// road itself (over a crest) is the road's, not the terrain's (the terrain that hugs the road
    /// is just under it); the terrain is never counted there.
    #[test]
    fn the_terrain_never_hides_the_road_from_the_chase_camera() {
        let tuning = CameraTuning::default();
        let mut failures = Vec::new();
        let mut views = 0usize;
        for (name, data, terrain) in terrains() {
            let (seen, hidden, example) = hidden_from_the_chase_camera(&tuning, &data, &terrain);
            views += seen;
            if hidden > 0 {
                failures.push(format!(
                    "{name}: {hidden} sight lines under the terrain, first at {example:?}"
                ));
            }
        }
        assert!(views > 100_000, "{views}");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The sight lines of the chase camera to the road ahead on a circuit (every 2 m of the road,
    /// the kart in the middle of the road or near either edge, the road from 3 to 60 m ahead, in
    /// its middle and near its edges): how many, how many pass under the terrain, the first of
    /// them (the kart's distance, how far ahead).
    fn hidden_from_the_chase_camera(
        tuning: &CameraTuning,
        data: &TrackData,
        terrain: &Terrain,
    ) -> (usize, usize, Option<(f32, f32)>) {
        let center = CenterLine::of(data);
        let (mut views, mut hidden, mut example) = (0usize, 0usize, None);
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
                        if under_the_terrain(terrain, &center, eye, target) {
                            hidden += 1;
                            example.get_or_insert((sample.distance, ahead));
                        }
                    }
                }
            }
        }
        (views, hidden, example)
    }

    /// Whether the segment from `eye` to `target` passes under the terrain's surface (every 0.5 m).
    /// Issue #202: not where it passes under the road itself (over a crest, the road hides what is
    /// beyond it, and the terrain that hugs it is under it): that is the road's own view, the
    /// walls' test's.
    fn under_the_terrain(terrain: &Terrain, center: &CenterLine, eye: Vec3, target: Vec3) -> bool {
        let steps = (eye.distance(target) / 0.5).ceil() as usize;
        (1..steps).any(|k| {
            let p = eye.lerp(target, k as f32 / steps as f32);
            // Under the ground (a lake's basin, the road diving into it), the terrain is flat.
            terrain
                .surface(p.xz())
                .is_some_and(|y| y > p.y && y > GROUND_HEIGHT)
                && !under_the_road(center, p)
        })
    }

    /// Whether a sample of a shortcut's path is on the road (between its walls).
    fn on_the_road(center: &CenterLine, s: &TrackSample) -> bool {
        let near = center.samples()[center.nearest_index(s.position)];
        near.position.xz().distance(s.position.xz()) <= wall_offset(&near)
    }

    /// Whether `p` is under the road's deck (between its walls, below its surface).
    fn under_the_road(center: &CenterLine, p: Vec3) -> bool {
        let near = center.samples()[center.nearest_index(p)];
        let right = near.forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let lateral = (p - near.position).dot(right);
        lateral.abs() <= wall_offset(&near) && p.y < near.point(lateral, 0.0).y
    }

    /// Issue #202: the terrain that hugs the road keeps every void a kart falls into, on every
    /// circuit and its mirror: beside a side without wall (from its edge to [`OPEN_REACH`]
    /// meters beyond, every meter along it) and across a gap, the terrain stays at least
    /// `RaceTuning::fall_depth` under the road (or under the ground, where the road is lower):
    /// a kart that falls there is seen fallen, and the pelican comes.
    #[test]
    fn the_terrain_keeps_the_voids_beside_the_open_edges_and_in_the_gaps() {
        let fall = RaceTuning::default().fall_depth;
        assert!(OPEN_FALL > fall + 0.5, "{OPEN_FALL} for {fall}");
        let mut failures = Vec::new();
        let mut checked = 0usize;
        for (name, data, terrain) in terrains() {
            let center = CenterLine::of(&data);
            let open: Vec<OpenEdge> = data
                .open_edges
                .iter()
                .map(|e| OpenEdge::new(&center, e))
                .collect();
            let gaps: Vec<Gap> = data.gaps.iter().map(|g| Gap::new(&center, g)).collect();
            for sample in center.road_samples() {
                let right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
                let gap = gap_at(&gaps, &center, sample.distance);
                for sign in [-1.0f32, 1.0] {
                    let w = wall_offset(&sample);
                    let (from, limit) = if let Some(gap) = gap {
                        (0.0, gap.low - fall)
                    } else if !walled(&open, &center, sample.distance, sign) {
                        (sample.half_width, sample.point(sign * w, 0.0).y - fall)
                    } else {
                        continue;
                    };
                    let limit = limit.max(GROUND_HEIGHT);
                    let mut lateral = from;
                    while lateral <= w + OPEN_REACH {
                        let p = sample.position + right * sign * lateral;
                        checked += 1;
                        if let Some(y) = terrain.surface(p.xz())
                            && y > limit + 0.01
                        {
                            failures.push(format!(
                                "{name}: the terrain {:.2} m up at {lateral:.1} m from the road \
                                 at {:.0}, over a void (at most {:.2})",
                                y - GROUND_HEIGHT,
                                sample.distance,
                                limit - GROUND_HEIGHT
                            ));
                            break;
                        }
                        lateral += 1.0;
                    }
                }
            }
        }
        assert!(checked > 10_000, "{checked}");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Issue #202 acceptance (GD § 16, « Notre implémentation (#202) »): on a circuit whose
    /// terrain hugs the road (the Col des Moufles, the demonstration, and its mirror), the road
    /// climbs and comes down for real (20 m at least between its lowest and its highest), and the
    /// ground carries it: beside its walls, wherever the road is raised (4 m and more above the
    /// ground) and the side is walled, 1 m beyond the walls' outer face, the terrain comes up to
    /// the walls' foot (within 0.5 m of [`EMBANK_LIP`] under the road) on half of the sides at
    /// least, and within 4 m of it on three quarters, instead of the skirts down to the ground
    /// (0 before #202). The others: inside the tight turns (no higher than the lowest of their
    /// road: a switchback's inside is a terrace at the level of its foot), beside the voids.
    /// Issue #208: not on a bridge, which stands on its pillars over its ravine instead
    /// (`the_demonstration_circuit_has_a_bridge_over_a_ravine`).
    /// Issue #211: every other circuit whose terrain hugs the road (the Anneau Guimauve, the
    /// Coteau Citrouille, the Falaises Flanelle, the Forêt Frimousse, the Crique Câline and the
    /// Corniche Cannelle, whose roads run up to 12 to 20 m above their ground) is held to the same
    /// shares; the climb of 20 m is the demonstration's (theirs are roads at a height, not
    /// mountain roads).
    #[test]
    fn the_terrain_hugs_the_raised_road_of_the_demonstration_circuit() {
        let (mut hugging, mut demonstrations) = (0, 0);
        for (name, data, terrain) in terrains() {
            if data.terrain().unwrap().embank <= 0.0 {
                continue;
            }
            hugging += 1;
            let center = CenterLine::of(&data);
            let road = center.road_samples();
            let (lo, hi) = road.iter().fold((f32::MAX, f32::MIN), |(lo, hi), s| {
                (lo.min(s.position.y), hi.max(s.position.y))
            });
            if data.name == DEMONSTRATION {
                demonstrations += 1;
                assert!(
                    hi - lo >= 20.0,
                    "{name}: the road climbs {lo} to {hi} m only"
                );
            }
            let open: Vec<OpenEdge> = data
                .open_edges
                .iter()
                .map(|e| OpenEdge::new(&center, e))
                .collect();
            // Issue #208: a bridge stands on its pillars over its ravine, not on the ground.
            let bridges: Vec<Bridge> = data
                .bridges
                .iter()
                .map(|b| Bridge::new(&center, b))
                .collect();
            let (mut raised, mut carried, mut close) = (0usize, 0usize, 0usize);
            for sample in &road {
                let right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
                for sign in [-1.0f32, 1.0] {
                    let edge = sample.point(sign * wall_offset(sample), 0.0).y;
                    if edge - GROUND_HEIGHT < 4.0
                        || !walled(&open, &center, sample.distance, sign)
                        || bridge_at(&bridges, &center, sample.distance).is_some()
                    {
                        continue;
                    }
                    raised += 1;
                    let p = sample.position + right * sign * (keep_out(sample) + 1.0);
                    let y = terrain.surface(p.xz()).unwrap_or(GROUND_HEIGHT);
                    let under = edge - EMBANK_LIP - y;
                    if under <= 0.5 {
                        carried += 1;
                    }
                    if under <= 4.0 {
                        close += 1;
                    }
                }
            }
            let share = |n: usize| n as f32 / raised.max(1) as f32;
            println!(
                "{name}: of {raised} raised walled sides, {carried} carried ({:.3}), {close} \
                 within 4 m ({:.3}); the road from {lo:.1} to {hi:.1} m",
                share(carried),
                share(close)
            );
            assert!(raised > 500, "{name}: {raised}");
            assert!(
                share(carried) >= 0.5,
                "{name}: {carried} of {raised} carried"
            );
            assert!(
                share(close) >= 0.75,
                "{name}: {close} of {raised} within 4 m"
            );
        }
        assert_eq!(demonstrations, 2, "the {DEMONSTRATION} and its mirror");
        assert!(hugging >= 14, "{hugging} circuits hug their road");
    }

    /// Issue #202: the demonstration circuit of a terrain that hugs the road.
    const DEMONSTRATION: &str = "Col des Moufles";

    /// Issue #215 (Max, 2026-10-31: « Pour les routes qui montent haut, pour ne pas ralentir, tu
    /// peux utiliser la même technique que pour les routes en spirale »): the demonstration
    /// circuit climbs far higher than before (58 m before #215), and wherever its road climbs or
    /// falls steeply, the karts drive in a level anti-gravity zone (their gravity follows the
    /// road: they do not slow down going up): every stretch of 16 m whose slope is above
    /// [`STEEP`] is in one. The zones stay level (the terrain carries them, the hug test above).
    #[test]
    fn the_demonstration_circuit_climbs_high_in_anti_gravity() {
        const HIGH: f32 = 80.0;
        const STEEP: f32 = 0.25;
        const RUN: f32 = 16.0;
        let file = format!("{}/assets/tracks/col.track.ron", env!("CARGO_MANIFEST_DIR"));
        let data = TrackData::from_ron(&std::fs::read_to_string(file).unwrap()).unwrap();
        for data in [data.clone(), data.mirrored()] {
            let center = CenterLine::of(&data);
            let zones: Vec<AntiGravityZone> = data
                .anti_gravity
                .iter()
                .map(|z| AntiGravityZone::new(&center, z))
                .collect();
            assert!(zones.iter().all(|z| z.level), "{zones:?}");
            let road = center.road_samples();
            let (lo, hi) = road.iter().fold((f32::MAX, f32::MIN), |(lo, hi), s| {
                (lo.min(s.position.y), hi.max(s.position.y))
            });
            assert!(hi - lo >= HIGH, "the road climbs {lo} to {hi} m only");
            let (mut steep, mut zoned, mut steepest) = (0usize, 0usize, 0.0f32);
            let step = center.length() / center.samples().len() as f32;
            let ahead = (RUN / step).round() as usize;
            for (i, s) in road.iter().enumerate() {
                let next = &road[(i + ahead) % road.len()];
                let run = s.position.xz().distance(next.position.xz());
                let slope = (next.position.y - s.position.y).abs() / run.max(1e-3);
                // A drop's step is no slope.
                let step = (0..ahead).any(|k| {
                    let (a, b) = (&road[(i + k) % road.len()], &road[(i + k + 1) % road.len()]);
                    (b.position.y - a.position.y).abs() > STEP_SLOPE * step
                });
                if step || slope <= STEEP {
                    continue;
                }
                steep += 1;
                steepest = steepest.max(slope);
                let middle = s.distance + RUN / 2.0;
                if anti_gravity_at(&zones, &center, middle).is_some() {
                    zoned += 1;
                } else {
                    panic!(
                        "{}: a slope of {slope:.2} out of the zones at {:.0} m",
                        data.name, s.distance
                    );
                }
            }
            println!(
                "{}: the road from {lo:.1} to {hi:.1} m, {steep} meters steeper than {STEEP} \
                 (up to {steepest:.2}), {zoned} of them in anti-gravity",
                data.name
            );
            assert!(steep > 200, "{steep}");
        }
    }

    /// Issue #215: the bound of the terrain that hugs the road follows a bump (#202 carried the
    /// road's level along its tangent, meters under a bump: the embankment was lost along the
    /// whole bump), and never lets a triangle of the terrain come over the road. A straight along
    /// the grid, 10 m up, with the bump of the Col des Moufles (2.2 m over 22 m), and its foot on
    /// the ground for a second one.
    #[test]
    fn the_bound_of_the_embankment_follows_a_bump() {
        let diagonal = CELL * std::f32::consts::SQRT_2;
        let window = diagonal + 0.5;
        for base in [10.0f32, 0.0] {
            let bump = |z: f32| {
                let t = (z - 40.0) / 22.0;
                if (0.0..=1.0).contains(&t) {
                    1.1 * (1.0 - (std::f32::consts::TAU * t).cos())
                } else {
                    0.0
                }
            };
            let height = |z: f32| base + bump(z);
            let line: Vec<TrackSample> = (0..=100)
                .map(|k| {
                    let z = k as f32;
                    let forward =
                        Vec3::new(0.0, height(z + 0.01) - height(z - 0.01), -0.02).normalize();
                    let right = forward.cross(Vec3::Y).normalize();
                    TrackSample {
                        distance: z,
                        position: Vec3::new(0.0, height(z), -z),
                        forward,
                        right,
                        up: right.cross(forward),
                        half_width: 7.0,
                        shoulder: 2.0,
                    }
                })
                .collect();
            let profile = Profile::new(&line, false, diagonal);
            let level = |z: f32| (height(z) - GROUND_HEIGHT - EMBANK_LIP).max(-SINK);
            // A vertex at `z` along the line: the lowest bound of the samples around it (as in
            // `hug`), and of #202's tangents.
            let bound = |z: f32| {
                (0..line.len())
                    .filter(|i| (z - *i as f32).abs() <= window)
                    .filter_map(|i| profile.at(i, z - i as f32, CELL))
                    .fold(f32::MAX, f32::min)
            };
            let tangent = |z: f32| {
                (1..line.len() - 1)
                    .filter(|i| (z - *i as f32).abs() <= window)
                    .map(|i| {
                        let g = (height(i as f32 + 1.0) - height(i as f32 - 1.0)) / 2.0;
                        height(i as f32) - GROUND_HEIGHT - EMBANK_LIP + g * (z - i as f32)
                    })
                    .fold(f32::MAX, f32::min)
                    .max(-SINK)
            };
            let (mut deepest, mut deepest_before) = (0.0f32, 0.0f32);
            for k in 0..=120 {
                let z = 20.0 + k as f32 * 0.5;
                let b = bound(z);
                assert!(b <= level(z) + 1e-3, "{base}: {b} over {} at {z}", level(z));
                deepest = deepest.max(level(z) - b);
                deepest_before = deepest_before.max(level(z) - tangent(z));
                // A triangle along the grid: from here to a cell on, under the road all along.
                let far = bound(z + CELL);
                for t in 1..12 {
                    let x = z + CELL * t as f32 / 12.0;
                    let chord = b + (far - b) * t as f32 / 12.0;
                    assert!(
                        chord <= level(x) + 0.02,
                        "{base}: a triangle {chord} over the road {} at {x}",
                        level(x)
                    );
                }
            }
            println!(
                "{base} m up: the bound at most {deepest:.2} m under the road over the bump \
                 ({deepest_before:.2} m along the tangent of #202)"
            );
            if base > 0.0 {
                assert!(deepest <= 0.5, "{deepest}");
                assert!(deepest_before >= 1.5, "{deepest_before}");
            }
        }
    }

    /// Issue #208 acceptance (GD § 16, « Notre implémentation (#208, étape 1) »): a bridge over a
    /// ravine. On a circuit with bridges (the Col des Moufles and its mirror), under every bridge,
    /// on the middle third of its length, the terrain under its deck (across it, from one wall to
    /// the other, every meter) stays [`BRIDGE_FALL`] meters under the road at least, and the
    /// ravine is [`BRIDGE_DEEP`] meters deep somewhere: a height that shows (Max: « des hauteurs
    /// franches »). Before #208, the terrain came up to [`EMBANK_LIP`] under the road there, or
    /// fell away from it on one side only.
    #[test]
    fn the_demonstration_circuit_has_a_bridge_over_a_ravine() {
        const BRIDGE_FALL: f32 = 20.0;
        const BRIDGE_DEEP: f32 = 40.0;
        let mut bridged = 0;
        for (name, data, terrain) in terrains() {
            if data.bridges.is_empty() {
                continue;
            }
            bridged += 1;
            let center = CenterLine::of(&data);
            for bridge in data.bridges.iter().map(|b| Bridge::new(&center, b)) {
                let length = bridge.length(&center);
                assert!(length >= MIN_BRIDGE, "{name}: a bridge of {length} m");
                let (mut checked, mut shallowest, mut deepest) = (0usize, f32::MAX, f32::MIN);
                let mut along = length / 3.0;
                while along <= 2.0 * length / 3.0 {
                    let s = center.at(bridge.from + along);
                    let w = wall_offset(&s);
                    for k in -(w as i32)..=(w as i32) {
                        let p = s.point(k as f32, 0.0);
                        let ground = terrain.surface(p.xz()).unwrap_or(GROUND_HEIGHT);
                        let under = p.y - ground;
                        checked += 1;
                        shallowest = shallowest.min(under);
                        deepest = deepest.max(under);
                    }
                    along += 1.0;
                }
                println!(
                    "{name}: a bridge of {length:.1} m, the terrain {shallowest:.1} to \
                     {deepest:.1} m under its deck on its middle third ({checked} points)"
                );
                assert!(checked > 100, "{name}: {checked}");
                assert!(
                    shallowest >= BRIDGE_FALL,
                    "{name}: the terrain {shallowest} m under the bridge only"
                );
                assert!(deepest >= BRIDGE_DEEP, "{name}: a ravine {deepest} m deep");
            }
        }
        assert!(bridged >= 2, "{bridged} circuits with a bridge");
    }

    /// Issue #204 acceptance (GD § 16, « Notre implémentation (#204) »): a road cut into the
    /// hillside. On a circuit with cuttings (the Col des Moufles and its mirror), along each of
    /// them (where its wall stands whole, every meter), on its side, the terrain rises above the
    /// road beyond its walls: within [`CUT_PROBE`] meters of their outer face, it stands at least
    /// [`CUT_WALL`] meters above the road on most of the cutting (before #204: nowhere, the
    /// terrain stayed under the road), and its wall is a wall somewhere ([`CUT_TOP`] meters up).
    /// The road is never hidden for all that (`the_terrain_never_hides_the_road_from_the_chase_camera`).
    #[test]
    fn the_demonstration_circuit_has_a_road_cut_into_the_hillside() {
        const CUT_PROBE: f32 = 30.0;
        const CUT_WALL: f32 = 4.0;
        const CUT_TOP: f32 = 10.0;
        let mut cut_circuits = 0;
        for (name, data, terrain) in terrains() {
            if data.cuttings.is_empty() {
                continue;
            }
            cut_circuits += 1;
            let center = CenterLine::of(&data);
            let cuts: Vec<OpenEdge> = data
                .cuttings
                .iter()
                .map(|e| OpenEdge::new(&center, e))
                .collect();
            let (mut whole, mut walled_up, mut top) = (0usize, 0usize, f32::MIN);
            for sample in center.road_samples() {
                let right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
                for sign in [-1.0f32, 1.0] {
                    if cut_weight(&cuts, &center, sample.distance, sign) < 1.0 {
                        continue;
                    }
                    whole += 1;
                    let edge = sample.point(sign * wall_offset(&sample), 0.0).y;
                    let highest = (0..=CUT_PROBE as usize)
                        .filter_map(|k| {
                            let lateral = keep_out(&sample) + k as f32;
                            terrain.surface((sample.position + right * sign * lateral).xz())
                        })
                        .fold(f32::MIN, f32::max)
                        - edge;
                    top = top.max(highest);
                    if highest >= CUT_WALL {
                        walled_up += 1;
                    }
                }
            }
            let share = walled_up as f32 / whole.max(1) as f32;
            println!(
                "{name}: of {whole} meters of cutting, {walled_up} with a wall {CUT_WALL} m above \
                 the road at least ({share:.3}), the highest {top:.1} m above it"
            );
            assert!(whole >= 100, "{name}: {whole} meters of cutting only");
            assert!(
                share >= 0.6,
                "{name}: a wall on {share} of the cutting only"
            );
            assert!(top >= CUT_TOP, "{name}: the wall {top} m high at most");
        }
        assert!(cut_circuits >= 2, "{cut_circuits} circuits with a cutting");
    }

    /// Issue #204: the cuttings keep the road in sight of the chase camera as the game places it.
    #[test]
    fn the_cuttings_look_from_where_the_chase_camera_is() {
        let tuning = CameraTuning::default();
        assert_eq!(CHASE_DISTANCE, tuning.distance);
        assert_eq!(CHASE_HEIGHT, tuning.height);
    }

    /// The sight bound is under every segment it is drawn from, wherever it crosses the grid.
    #[test]
    fn a_sight_line_bounds_every_cell_it_crosses() {
        let grid = Grid {
            origin: Vec2::new(-30.0, -30.0),
            cols: 11,
            rows: 11,
        };
        for (from, to) in [
            (Vec3::new(-20.0, 5.0, -17.0), Vec3::new(22.0, 1.0, 13.0)),
            (Vec3::new(10.0, 3.0, 20.0), Vec3::new(-25.0, 9.0, -1.0)),
            (Vec3::new(1.0, 2.0, -20.0), Vec3::new(1.0, 2.0, 25.0)),
            (Vec3::new(-29.0, 4.0, 0.5), Vec3::new(29.0, 0.0, 0.5)),
        ] {
            let mut values = vec![f32::MAX; 121];
            grid.under_line(&mut values, from, to);
            for k in 0..=400 {
                let p = from.lerp(to, k as f32 / 400.0);
                let c = ((p.xz() - grid.origin) / CELL).floor();
                let (i, j) = (c.x as usize, c.y as usize);
                for (ci, cj) in [(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)] {
                    let v = values[cj * 11 + ci];
                    assert!(
                        v <= p.y - GROUND_HEIGHT - SIGHT_MARGIN + 1e-4,
                        "{from} → {to}: {v} at ({ci}, {cj}) under {p}"
                    );
                }
            }
        }
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
