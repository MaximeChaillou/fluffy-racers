//! Track mesh generation (pure, no ECS): road, curbs, grass shoulders, low walls and skirts,
//! as flat-shaded quads with vertex colors (one material for the whole circuit).
//!
//! Cross-section, from the centerline outward on each side:
//! road (half width) | curb | grass shoulder | wall | skirt down to the ground.

use bevy::prelude::*;

use super::anti_gravity::{AntiGravityZone, anti_gravity_at};
use super::biome::{Palette, Rgb};
use super::data::{OffroadZone, ShortcutKind, TrackData};
use super::layout::{CenterLine, TrackSample};
use super::paving::{self, BORDER_TILE, PAVING_TILE};
use super::shortcut::{OpenEdge, Shortcut, walled};
use super::terrain::{Bridge, bridge_at};
use super::water::{Gap, Lake, ground_height};

/// Width of the red and white curbs (m).
pub const CURB_WIDTH: f32 = 1.0;
/// Thickness of the low walls (m).
pub const WALL_THICKNESS: f32 = 0.5;
/// Height of the low walls above the road (m), on every circuit: as drawn and as solid. Their
/// collider is the drawn mesh itself ([`TrackMeshes::walls`]; issue #191: drawn opaque, or with a
/// glass top between two legs of the track, the same faces): one constant, nothing solid that is
/// not drawn (issue #47). Issue #27: the top of a kart's body is 0.8 m above the road; with walls
/// exactly as high, a kart pushing into a wall caught its top edge, which pushed its nose down
/// until it stood on it, or let it ride up onto the wall. 0.4 m higher, a kart only ever meets
/// the wall's face. It stays below the chase camera's line of sight, which starts 1.35 m above
/// the road at the kart (`CameraTuning::look_height` above the kart's center) and rises from
/// there: the camera's anti-wall ray never meets a wall on a lap (tested on both circuits).
pub const WALL_HEIGHT: f32 = 1.2;
/// Issue #191: on the lacets, the low walls that stand between two legs of the track (where the
/// chase camera sees the other leg across them) are drawn with a **glass top**: opaque from the
/// road up to this height (m), glass from there to their top ([`TrackMeshes::glass`]). Solid as
/// before, [`WALL_HEIGHT`] high (their collider is [`TrackMeshes::walls`], unchanged): only how
/// they are drawn changes. Seen from 20 to 30 m away, a wall at 1.2 m hid the near half of the
/// other leg's road from a camera 2.4 m above the kart (3 to 5 % of the road ahead on screen, on
/// nine circuits out of twelve): the lines of sight met the walls between 0.5 m and their top
/// nine times out of ten.
pub const GLASS_FROM: f32 = 0.4;
/// Issue #191: a wall stands between two legs where a sample of the road lies beyond it (on its
/// outer side), within this distance (m) of its foot...
pub const GLASS_REACH: f32 = 50.0;
/// ...at least this far along the track from it (m), either way...
pub const GLASS_APART: f32 = 30.0;
/// ...and nearer to it than this share of how far along the track it is: the road comes back
/// toward the wall (the other leg of a lacet; the exit of a hairpin or of a tight turn, seen
/// across its inside), not the stretch the wall lines nor the rest of a wide turn. The share
/// glassed (of the walls' sides along the road): 15 to 17 % on the Ruban Réglisse and the Lagon
/// Duvet, 52 to 73 % on the circuits of lacets; 0.6 and 25 m (fewer walls) left 3 % of the road
/// ahead hidden there, 0.8 and 30 m 0.6 % at most.
pub const GLASS_RETURN: f32 = 0.8;
/// Issue #191: how opaque the glass is (its material's alpha): the road beyond it shows through,
/// the wall's shape stays plain.
pub const GLASS_ALPHA: f32 = 0.3;
/// How much of the wall's panel color the glass keeps (the rest white): a pale tint of the
/// biome's barriers.
const GLASS_TINT: f32 = 0.45;

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

    /// Adds the triangle a → b → c (counter-clockwise seen from its front), `color` sRGB like
    /// [`MeshData::quad`] (issue #146: the far decor).
    pub fn triangle(&mut self, corners: [Vec3; 3], color: [f32; 4]) {
        let normal = (corners[1] - corners[0])
            .cross(corners[2] - corners[0])
            .normalize_or(Vec3::Y);
        self.triangle_lit(corners, color, normal);
    }

    /// The same, lit as if facing `normal` (unit) rather than its own way (the far decor's
    /// clouds: lit from above, bright).
    pub fn triangle_lit(&mut self, corners: [Vec3; 3], color: [f32; 4], normal: Vec3) {
        let color = Color::srgba(color[0], color[1], color[2], color[3])
            .to_linear()
            .to_f32_array();
        let base = self.positions.len() as u32;
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(color);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
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
        self.strip_where(samples, closed, a, b, color, |_, _| true);
    }

    /// The same, only the quads between two samples that `keep` keeps (issue #78: the road of the
    /// anti-gravity zones in a mesh of its own, the rest in the usual one).
    pub fn strip_where(
        &mut self,
        samples: &[TrackSample],
        closed: bool,
        a: impl Fn(&TrackSample) -> Vec3,
        b: impl Fn(&TrackSample) -> Vec3,
        color: impl Fn(usize) -> [f32; 4],
        keep: impl Fn(&TrackSample, &TrackSample) -> bool,
    ) {
        let n = samples.len();
        let segments = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            let (s0, s1) = (&samples[i], &samples[(i + 1) % n]);
            if keep(s0, s1) {
                self.quad([a(s0), b(s0), b(s1), a(s1)], color(i));
            }
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

    /// Issue #191: the triangles added from vertex `start` on (every triangle of a mesh built
    /// quad by quad uses vertices of its own quad only), with their vertices.
    pub fn from_vertex(&self, start: usize) -> MeshData {
        let base = start as u32;
        MeshData {
            positions: self.positions[start..].to_vec(),
            normals: self.normals[start..].to_vec(),
            colors: self.colors[start..].to_vec(),
            indices: self
                .indices
                .iter()
                .filter(|&&i| i >= base)
                .map(|i| i - base)
                .collect(),
        }
    }

    /// Issue #119: the triangles split into square tiles of `size` meters on the ground plane (by
    /// their first corner), one mesh per tile that has any, in the order of the tiles (by row,
    /// then by column): each one is culled on its own (its bounding box), where one mesh for a
    /// whole long circuit would always be drawn whole. The same triangles, their vertices with
    /// them.
    pub fn tiles(&self, size: f32) -> Vec<MeshData> {
        type Tile = (MeshData, std::collections::HashMap<u32, u32>);
        let mut tiles: std::collections::BTreeMap<(i32, i32), Tile> = default();
        for triangle in self.indices.as_chunks::<3>().0 {
            let first = self.positions[triangle[0] as usize];
            let key = (
                (first[2] / size).floor() as i32,
                (first[0] / size).floor() as i32,
            );
            let (tile, remap) = tiles.entry(key).or_default();
            for &i in triangle {
                let index = *remap.entry(i).or_insert_with(|| {
                    let k = i as usize;
                    tile.positions.push(self.positions[k]);
                    tile.normals.push(self.normals[k]);
                    tile.colors.push(self.colors[k]);
                    tile.positions.len() as u32 - 1
                });
                tile.indices.push(index);
            }
        }
        tiles.into_values().map(|(tile, _)| tile).collect()
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

/// Issue #187: triangles drawn with a texture: a [`MeshData`] (flat normals, vertex colors) and
/// the UVs of its vertices. Drawn only (with a renderer): the colliders are the [`TrackMeshes`]'
/// own meshes, unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaintedMesh {
    pub mesh: MeshData,
    pub uvs: Vec<[f32; 2]>,
}

impl PaintedMesh {
    /// Adds the quad a → b → c → d, its UVs `uvs` at its corners.
    pub fn quad(&mut self, corners: [Vec3; 4], color: [f32; 4], uvs: [[f32; 2]; 4]) {
        self.mesh.quad(corners, color);
        self.uvs.extend_from_slice(&uvs);
    }

    /// A strip along consecutive samples, like [`MeshData::strip_where`]: `u(s)` gives the `u` of
    /// its two edges at a sample, `v[i]` the `v` at sample `i` (one more than the samples when
    /// `closed`: the end of the last quad), `color(i)` the color of the quad from sample `i`.
    #[allow(clippy::too_many_arguments)]
    pub fn strip_where(
        &mut self,
        samples: &[TrackSample],
        closed: bool,
        a: impl Fn(&TrackSample) -> Vec3,
        b: impl Fn(&TrackSample) -> Vec3,
        u: impl Fn(&TrackSample) -> (f32, f32),
        v: &[f32],
        color: impl Fn(usize) -> [f32; 4],
        keep: impl Fn(&TrackSample, &TrackSample) -> bool,
    ) {
        let n = samples.len();
        let segments = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segments {
            let (s0, s1) = (&samples[i], &samples[(i + 1) % n]);
            if keep(s0, s1) {
                let ((ua0, ub0), (ua1, ub1)) = (u(s0), u(s1));
                let (v0, v1) = (v[i], v[i + 1]);
                self.quad(
                    [a(s0), b(s0), b(s1), a(s1)],
                    color(i),
                    [[ua0, v0], [ub0, v0], [ub1, v1], [ua1, v1]],
                );
            }
        }
    }

    /// The same strip, its UVs the ground's own meters divided by `tile` (`x` and `z`: issue
    /// #187, a paving that lies on the ground whatever way the road goes).
    pub fn planar_strip_where(
        &mut self,
        samples: &[TrackSample],
        closed: bool,
        a: impl Fn(&TrackSample) -> Vec3,
        b: impl Fn(&TrackSample) -> Vec3,
        tile: f32,
        color: impl Fn(usize) -> [f32; 4],
        keep: impl Fn(&TrackSample, &TrackSample) -> bool,
    ) {
        let n = samples.len();
        let segments = if closed { n } else { n.saturating_sub(1) };
        let uv = |p: Vec3| [p.x / tile, p.z / tile];
        for i in 0..segments {
            let (s0, s1) = (&samples[i], &samples[(i + 1) % n]);
            if keep(s0, s1) {
                let corners = [a(s0), b(s0), b(s1), a(s1)];
                self.quad(corners, color(i), corners.map(uv));
            }
        }
    }

    pub fn append(&mut self, other: &PaintedMesh) {
        self.mesh.append(&other.mesh);
        self.uvs.extend_from_slice(&other.uvs);
    }

    pub fn is_empty(&self) -> bool {
        self.mesh.indices.is_empty()
    }

    /// In square tiles, like [`MeshData::tiles`] (each quad's vertices are its own: a triangle's
    /// UVs go with its vertices).
    pub fn tiles(&self, size: f32) -> Vec<PaintedMesh> {
        type Tile = (PaintedMesh, std::collections::HashMap<u32, u32>);
        let mut tiles: std::collections::BTreeMap<(i32, i32), Tile> = default();
        let m = &self.mesh;
        for triangle in m.indices.as_chunks::<3>().0 {
            let first = m.positions[triangle[0] as usize];
            let key = (
                (first[2] / size).floor() as i32,
                (first[0] / size).floor() as i32,
            );
            let (tile, remap) = tiles.entry(key).or_default();
            for &i in triangle {
                let index = *remap.entry(i).or_insert_with(|| {
                    let k = i as usize;
                    tile.mesh.positions.push(m.positions[k]);
                    tile.mesh.normals.push(m.normals[k]);
                    tile.mesh.colors.push(m.colors[k]);
                    tile.uvs.push(self.uvs[k]);
                    tile.mesh.positions.len() as u32 - 1
                });
                tile.mesh.indices.push(index);
            }
        }
        tiles.into_values().map(|(tile, _)| tile).collect()
    }
}

/// Issue #187: what is drawn of the circuit, with a renderer: its road's paving and its borders
/// with their textures (`super::paving`), and the rest plain (vertex colors), the same triangles
/// as [`TrackMeshes::combined`] (the anti-gravity zones' blue and white curbs in the plain one).
#[derive(Clone, Debug, Default)]
pub struct DrawnTrack {
    pub plain: MeshData,
    pub paving: PaintedMesh,
    pub borders: PaintedMesh,
}

/// The generated circuit, split by surface (one collider each) plus decoration.
#[derive(Clone, Debug, Default)]
pub struct TrackMeshes {
    /// Asphalt and curbs: `Surface::Road`.
    pub road: MeshData,
    /// Grass shoulders and off-road patches: `Surface::Offroad`.
    pub offroad: MeshData,
    /// Low walls, [`WALL_HEIGHT`] high: their collider, `Surface::Wall` (issue #47: the same
    /// triangles as drawn, [`wall_looks`](Self::wall_looks) and [`glass`](Self::glass)).
    pub walls: MeshData,
    /// Issue #191: the low walls as drawn, opaque: [`walls`](Self::walls) itself (the same
    /// triangles, in the same order) but along the walls that stand between two legs of the
    /// track ([`walls_between_legs`]), only up to [`GLASS_FROM`] there...
    pub wall_looks: MeshData,
    /// ...and their glass top, from [`GLASS_FROM`] to their top (their faces and their lid),
    /// drawn see-through (`track::circuit`). Empty on a circuit without such a wall. Every
    /// vertex of the collider is a vertex of one of the two.
    pub glass: MeshData,
    /// Skirts under raised parts: visual only. Issue #78: the blue stripes of the anti-gravity
    /// zones too.
    pub decor: MeshData,
    /// Issue #78: the asphalt and curbs of the anti-gravity zones (`Surface::Road`, and
    /// `AntiGravity`), apart from [`road`](Self::road): a wheel on it puts the kart in
    /// anti-gravity. Empty on a circuit without a zone.
    pub anti_gravity_road: MeshData,
    /// The same for their shoulders (`Surface::Offroad`, and `AntiGravity`).
    pub anti_gravity_offroad: MeshData,
    /// Issue #107: the grass paths of the shortcuts (`Surface::Offroad`; issue #193: every
    /// shortcut is grass, the roads of before are gone). Empty on a circuit without one.
    pub shortcut_grass: MeshData,
    /// Issue #180: the road and the shoulders of the trick stretches of the bumps and the drops
    /// (`CenterLine::trick_stretches`: `Surface::Road` and `Offroad`, and `TrickRamp`), apart from
    /// the rest: a kart that leaves them starts a trick on a press (GD § 5). Empty on a circuit
    /// without bump nor drop.
    pub trick_road: MeshData,
    pub trick_offroad: MeshData,
    /// Issue #180: the faces of the drops (`Surface::Wall`, drawn and solid, apart from the low
    /// walls: not a wall a kart sees the road over). Empty on a circuit without a drop.
    pub cliffs: MeshData,
    /// Issue #187: the road's surface (the road's, the anti-gravity zones', the trick stretches')
    /// as it is drawn with its paving, its UVs in meters divided by
    /// `paving::PAVING_TILE` (the ground's `x` and `z` for a paving that lies on the ground;
    /// across and along the centerline for one that follows the road, and in the anti-gravity
    /// zones), and its curbs (out of the
    /// anti-gravity zones) as drawn with their border, `u` from 0 by the road to 1 outside, `v`
    /// along their middle in meters divided by `paving::BORDER_TILE`. Drawn only: the same
    /// triangles as in the meshes above, which stay the colliders.
    pub paving: PaintedMesh,
    pub borders: PaintedMesh,
    /// The anti-gravity zones' blue and white curbs, drawn plain (their triangles in
    /// `anti_gravity_road` too).
    pub blue_curbs: MeshData,
}

impl TrackMeshes {
    /// Everything in one mesh, for a single draw call (the anti-gravity zones' road last: without
    /// a zone, exactly the mesh before issue #78).
    pub fn combined(&self) -> MeshData {
        let mut all = self.road.clone();
        all.append(&self.offroad);
        all.append(&self.wall_looks);
        // Issue #191: headless, the glass in the one mesh too (nothing draws it there).
        all.append(&self.glass);
        all.append(&self.decor);
        all.append(&self.anti_gravity_road);
        all.append(&self.anti_gravity_offroad);
        all.append(&self.shortcut_grass);
        all.append(&self.trick_road);
        all.append(&self.trick_offroad);
        all.append(&self.cliffs);
        all
    }

    /// Issue #187: what is drawn with a renderer: the road's paving and its borders apart, with
    /// their UVs, and the rest plain (every triangle of [`combined`](Self::combined) in one of
    /// the three, once; issue #191: but the walls' glass, [`glass`](Self::glass), drawn apart).
    pub fn drawn(&self) -> DrawnTrack {
        let mut plain = self.offroad.clone();
        for part in [
            &self.wall_looks,
            &self.decor,
            &self.blue_curbs,
            &self.anti_gravity_offroad,
            &self.shortcut_grass,
            &self.trick_offroad,
            &self.cliffs,
        ] {
            plain.append(part);
        }
        DrawnTrack {
            plain,
            paving: self.paving.clone(),
            borders: self.borders.clone(),
        }
    }
}

/// The curbs of the anti-gravity zones (issue #78): cyan blue and white, whatever the biome.
pub const ANTI_GRAVITY_CURBS: (Rgb, Rgb) = ((0.12, 0.72, 1.0), (0.95, 0.98, 1.0));
/// Their blue stripes along the road (sRGB), drawn only: the karts' wheels pivot there.
pub const ANTI_GRAVITY_STRIPE: Rgb = (0.1, 0.6, 1.0);
/// The stripes: this wide (m)...
const STRIPE_WIDTH: f32 = 0.35;
/// ...this far inside the road's edges (m)...
const STRIPE_INSET: f32 = 1.0;
/// ...and this much above the road (m), like the start line's band.
const STRIPE_HEIGHT: f32 = 0.02;
/// A band across the road at each end of a zone (m along the track).
const STRIPE_BAND: f32 = 1.5;

/// Issue #94: the anti-gravity zones' ribbon is this thick (m): its back, behind the road, is drawn
/// (the skirts only hang under a road that faces up).
pub const RIBBON_THICKNESS: f32 = 0.4;
/// A road faces up while its normal is no more than this below the horizontal (its `y`): a wall
/// at 90° does (its normal is horizontal, to a rounding), a road banked beyond it does not (issue
/// #94: no skirt under it; for the race, it is upside down, `race::Projection::upside_down`).
pub const FACING_UP_TOLERANCE: f32 = 0.01;

/// Lateral distance from the centerline to the inner face of the wall.
pub fn wall_offset(sample: &TrackSample) -> f32 {
    sample.half_width + CURB_WIDTH + sample.shoulder
}

/// Issue #208: a bridge's deck is this thick (m): its underside, facing down, from one wall's
/// outer face to the other's, and its two edges, in place of the skirts.
pub const BRIDGE_DECK: f32 = 1.2;
/// Issue #208: a bridge stands on bents of pillars at most this far apart (m along the
/// centerline), none at its abutments...
pub const PILLAR_SPACING: f32 = 15.0;
/// ...two per bent, this far from the middle of the road (a share of its half width)...
pub const PILLAR_SPREAD: f32 = 0.6;
/// ...each a square pillar this wide (m), from inside the deck down to the ground (the ravine's
/// slopes hide what is below them). Drawn only: the bridge is walled, a kart never falls among
/// them.
pub const PILLAR_WIDTH: f32 = 2.4;

/// Issue #208: where the bents of a bridge's pillars stand (m along the centerline): evenly
/// spaced, [`PILLAR_SPACING`] apart at most, none at its abutments.
pub fn pillar_bents(center: &CenterLine, bridge: &Bridge) -> Vec<f32> {
    let length = bridge.length(center);
    let spans = (length / PILLAR_SPACING).ceil().max(2.0) as usize;
    (1..spans)
        .map(|k| center.wrap(bridge.from + length * k as f32 / spans as f32))
        .collect()
}

/// Issue #180: the road is cut this far (m) each way around the step of a drop: the upper road
/// ends just before it, the lower one starts just after it.
pub const CLIFF_CUT: f32 = 0.01;

/// Issue #180: the walls stand higher than [`WALL_HEIGHT`] along the back of a bump and below a
/// drop, coming back down to it over this many meters after the bump's end or the drop's step: a
/// kart flying off it at an angle (steered, pushed) meets them instead of passing over them.
pub const PARAPET_RUN: f32 = 18.0;

/// Issue #180: how much higher than [`WALL_HEIGHT`] the walls stand at `distance` along the
/// centerline: from the crest of a bump on, as high as at its crest (the road falls away, the top of
/// the walls does not), then down to their own height over [`PARAPET_RUN`] meters after its end;
/// below a drop, as high as the upper road's walls at its step, then down the same way. 0 anywhere
/// else (and on a circuit without bump nor drop).
pub fn wall_lift(center: &CenterLine, distance: f32) -> f32 {
    let bumps = center.bumps().iter().map(|b| {
        let x = center.wrap(distance - b.from);
        if x < b.length / 2.0 {
            0.0
        } else if x <= b.length {
            let wave = (std::f32::consts::TAU * x / b.length).cos();
            b.height * (1.0 + wave) / 2.0
        } else {
            b.height * (1.0 - (x - b.length) / PARAPET_RUN).max(0.0)
        }
    });
    let drops = center.cliffs().iter().map(|c| {
        let x = center.wrap(distance - c.at);
        c.height * (1.0 - x / PARAPET_RUN).max(0.0)
    });
    bumps.chain(drops).fold(0.0, f32::max)
}

/// Issue #107: a shortcut's road or grass is built this much above its path, so that where it
/// lies on the circuit's road or shoulders (its two ends) the wheels find it first, like the sand
/// patches.
pub const SHORTCUT_LIFT: f32 = 0.02;

/// The circuit's meshes, in the colors of `palette` (its biome's, issue #11). Issue #20: nothing
/// in its gaps (the ends of the road there closed by a face down to the ground), and the skirts
/// down to the floor of a lake.
pub fn generate(center: &CenterLine, data: &TrackData, palette: &Palette) -> TrackMeshes {
    // Issue #119: an open circuit's way back is never built: no road there, like a gap (its two
    // ends closed down to the ground, and a barrier across the road).
    let way_back = center.open().map(|open| Gap {
        from: open.road_end,
        to: open.road_start,
        low: GROUND_HEIGHT,
    });
    let gaps: Vec<Gap> = data
        .gaps
        .iter()
        .map(|g| Gap::new(center, g))
        .chain(way_back)
        .collect();
    // Issue #107: the stretches without wall: whether the wall of side `sign` stands between two
    // samples (their middle).
    let edges: Vec<OpenEdge> = data
        .open_edges
        .iter()
        .map(|e| OpenEdge::new(center, e))
        .collect();
    let walled_side = |sign: f32, s0: &TrackSample, s1: &TrackSample| {
        let middle = s0.distance + center.wrap(s1.distance - s0.distance) / 2.0;
        walled(&edges, center, middle, sign)
    };
    let lakes: Vec<Lake> = data.lakes.iter().map(Lake::new).collect();
    let zones: Vec<AntiGravityZone> = data
        .anti_gravity
        .iter()
        .map(|z| AntiGravityZone::new(center, z))
        .collect();
    // Issue #78: whether the quad between two samples is in an anti-gravity zone (its middle).
    let zoned = |s0: &TrackSample, s1: &TrackSample| {
        let middle = s0.distance + center.wrap(s1.distance - s0.distance) / 2.0;
        anti_gravity_at(&zones, center, middle).is_some()
    };
    // Issue #180: whether the quad between two samples is on a trick stretch of a bump or a drop
    // (its middle; never in a zone: the file is refused).
    let stretches = center.trick_stretches();
    let tricky = |s0: &TrackSample, s1: &TrackSample| {
        let middle = s0.distance + center.wrap(s1.distance - s0.distance) / 2.0;
        center.on_a_trick_stretch(&stretches, middle)
    };
    let unzoned = |s0: &TrackSample, s1: &TrackSample| !zoned(s0, s1) && !tricky(s0, s1);
    // Issue #208: whether the quad between two samples is on a bridge (its middle).
    let bridges: Vec<Bridge> = data
        .bridges
        .iter()
        .map(|b| Bridge::new(center, b))
        .collect();
    let bridged = |s0: &TrackSample, s1: &TrackSample| {
        let middle = s0.distance + center.wrap(s1.distance - s0.distance) / 2.0;
        bridge_at(&bridges, center, middle).is_some()
    };
    // Issue #94: whether the road between two samples faces up (banked to 90° at most). Issue
    // #208: the skirts hang only there, and off the bridges.
    let faces_up = |s0: &TrackSample, s1: &TrackSample| {
        s0.up.y >= -FACING_UP_TOLERANCE && s1.up.y >= -FACING_UP_TOLERANCE
    };
    let skirted = |s0: &TrackSample, s1: &TrackSample| faces_up(s0, s1) && !bridged(s0, s1);
    // Issue #191: whether the wall of side `sign` between two samples stands between two legs of
    // the track (its first sample's).
    let between = walls_between_legs(center);
    let step = center.length() / center.samples().len().max(1) as f32;
    let glassed = |sign: f32, s0: &TrackSample, _: &TrackSample| {
        let i = ((s0.distance / step).round() as usize) % between.len().max(1);
        between
            .get(i)
            .is_some_and(|sides| sides[usize::from(sign > 0.0)])
    };
    let mut meshes = TrackMeshes::default();
    // Issue #187: whether the road's paving lies on the ground (its UVs the ground's meters)
    // rather than following the road (in the anti-gravity zones, it always follows it).
    let planar = !data.road_look().paving.follows_the_road();
    let (asphalt, grass, sand, skirt) = (
        rgba(palette.asphalt),
        rgba(palette.shoulder),
        rgba(palette.sand),
        rgba(palette.skirt),
    );
    let curbs = [rgba(palette.curbs.0), rgba(palette.curbs.1)];
    let stripe = |i: usize| curbs[(i / CURB_STRIPE) % 2];
    let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
    let ground = |p: Vec3| Vec3::new(p.x, ground_height(&lakes, p.xz()), p.z);

    let blue_curbs = [rgba(ANTI_GRAVITY_CURBS.0), rgba(ANTI_GRAVITY_CURBS.1)];
    let blue_stripe = |i: usize| blue_curbs[(i / CURB_STRIPE) % 2];
    // Issue #180: the road stops at the top of each drop and goes on below it: a cut of no length
    // there (`CLIFF_CUT` each way), like a gap (its face: below).
    let cuts: Vec<Gap> = gaps
        .iter()
        .copied()
        .chain(center.cliffs().iter().map(|c| Gap {
            from: c.at - CLIFF_CUT,
            to: c.at + CLIFF_CUT,
            low: GROUND_HEIGHT,
        }))
        .collect();
    for (samples, closed) in runs(center, &cuts) {
        let (samples, closed) = (&samples[..], closed);
        // Issue #187: the UVs and the tint of the paving and of the borders along this run.
        let along = Along::new(center, samples, closed);
        // Road and curbs; in the anti-gravity zones (issue #78), in their own mesh, the curbs
        // blue and white.
        for (road, keep, stripe, blue) in [
            (
                &mut meshes.road,
                &unzoned as &dyn Fn(&TrackSample, &TrackSample) -> bool,
                &stripe as &dyn Fn(usize) -> [f32; 4],
                false,
            ),
            (&mut meshes.anti_gravity_road, &zoned, &blue_stripe, true),
            (&mut meshes.trick_road, &tricky, &stripe, false),
        ] {
            // Issue #187: drawn with its paving, and its curbs with their border (the
            // anti-gravity zones' plain, blue and white): the same quads.
            along.paving(&mut meshes.paving, samples, closed, keep, planar && !blue);
            if blue {
                for side in [-1.0f32, 1.0] {
                    meshes.blue_curbs.strip_where(
                        samples,
                        closed,
                        |s| s.point(side.min(0.0) * CURB_WIDTH + side * s.half_width, 0.0),
                        |s| s.point(side.max(0.0) * CURB_WIDTH + side * s.half_width, 0.0),
                        stripe,
                        keep,
                    );
                }
            } else {
                along.borders(&mut meshes.borders, samples, closed, keep);
            }
            road.strip_where(
                samples,
                closed,
                |s| s.point(-s.half_width, 0.0),
                |s| s.point(s.half_width, 0.0),
                |_| asphalt,
                keep,
            );
            road.strip_where(
                samples,
                closed,
                |s| s.point(-s.half_width - CURB_WIDTH, 0.0),
                |s| s.point(-s.half_width, 0.0),
                stripe,
                keep,
            );
            road.strip_where(
                samples,
                closed,
                |s| s.point(s.half_width, 0.0),
                |s| s.point(s.half_width + CURB_WIDTH, 0.0),
                stripe,
                keep,
            );
        }

        // Grass shoulders. Issue #107: where a side has no wall, its shoulder runs on to the
        // wall's outer face (the skirt hangs from there: no slit between them).
        for (offroad, keep) in [
            (
                &mut meshes.offroad,
                &unzoned as &dyn Fn(&TrackSample, &TrackSample) -> bool,
            ),
            (&mut meshes.anti_gravity_offroad, &zoned),
            (&mut meshes.trick_offroad, &tricky),
        ] {
            offroad.strip_where(
                samples,
                closed,
                |s| s.point(-wall_offset(s), 0.0),
                |s| s.point(-s.half_width - CURB_WIDTH, 0.0),
                |_| grass,
                |s0, s1| keep(s0, s1) && walled_side(-1.0, s0, s1),
            );
            offroad.strip_where(
                samples,
                closed,
                |s| s.point(s.half_width + CURB_WIDTH, 0.0),
                |s| s.point(wall_offset(s), 0.0),
                |_| grass,
                |s0, s1| keep(s0, s1) && walled_side(1.0, s0, s1),
            );
            offroad.strip_where(
                samples,
                closed,
                |s| s.point(-outer(s), 0.0),
                |s| s.point(-s.half_width - CURB_WIDTH, 0.0),
                |_| grass,
                |s0, s1| keep(s0, s1) && !walled_side(-1.0, s0, s1),
            );
            offroad.strip_where(
                samples,
                closed,
                |s| s.point(s.half_width + CURB_WIDTH, 0.0),
                |s| s.point(outer(s), 0.0),
                |_| grass,
                |s0, s1| keep(s0, s1) && !walled_side(1.0, s0, s1),
            );
        }

        // The blue stripes of the anti-gravity zones (drawn only), near both edges of the road.
        for side in [-1.0f32, 1.0] {
            let inner = |s: &TrackSample| side * (s.half_width - STRIPE_INSET - STRIPE_WIDTH);
            let outer = |s: &TrackSample| side * (s.half_width - STRIPE_INSET);
            let (a, b): (&dyn Fn(&TrackSample) -> f32, &dyn Fn(&TrackSample) -> f32) = if side < 0.0
            {
                (&outer, &inner)
            } else {
                (&inner, &outer)
            };
            meshes.decor.strip_where(
                samples,
                closed,
                |s| s.point(a(s), STRIPE_HEIGHT),
                |s| s.point(b(s), STRIPE_HEIGHT),
                |_| rgba(ANTI_GRAVITY_STRIPE),
                zoned,
            );
        }

        // Low walls: inner face, top, outer face on each side (issue #107: not along the
        // stretches without wall). Issue #191: drawn with a glass top between two legs.
        wall_strips(
            WallParts {
                solid: &mut meshes.walls,
                drawn: &mut meshes.wall_looks,
                glass: &mut meshes.glass,
            },
            samples,
            closed,
            palette,
            &walled_side,
            &|s: &TrackSample| WALL_HEIGHT + wall_lift(center, s.distance),
            &glassed,
        );

        // Skirts from the wall bottoms down to the ground (or the floor of a lake), so raised
        // parts do not float. Issue #94: only under a road that faces up (to the wall, 90°, at
        // most): under a road upside down, a skirt would hang through the karts driving under it,
        // and the ribbon has its back instead (below). Out of the zones, every skirt (45° at most).
        let decor = &mut meshes.decor;
        decor.strip_where(
            samples,
            closed,
            |s| s.point(outer(s), 0.0),
            |s| ground(s.point(outer(s), 0.0)),
            |_| skirt,
            skirted,
        );
        decor.strip_where(
            samples,
            closed,
            |s| ground(s.point(-outer(s), 0.0)),
            |s| s.point(-outer(s), 0.0),
            |_| skirt,
            skirted,
        );
        // Issue #208: on a bridge, its deck instead: its underside, `BRIDGE_DECK` under the road
        // from one wall's outer face to the other's, facing down, and its two edges facing out.
        decor.strip_where(
            samples,
            closed,
            |s| s.point(outer(s), -BRIDGE_DECK),
            |s| s.point(-outer(s), -BRIDGE_DECK),
            |_| skirt,
            bridged,
        );
        decor.strip_where(
            samples,
            closed,
            |s| s.point(outer(s), 0.0),
            |s| s.point(outer(s), -BRIDGE_DECK),
            |_| skirt,
            bridged,
        );
        decor.strip_where(
            samples,
            closed,
            |s| s.point(-outer(s), -BRIDGE_DECK),
            |s| s.point(-outer(s), 0.0),
            |_| skirt,
            bridged,
        );
        // Issue #94: in the anti-gravity zones, the ribbon's back, `RIBBON_THICKNESS` behind the
        // road from one wall's outer face to the other's, and its two edges: seen from outside
        // (from below a wall, from above a ceiling), the ribbon is solid. Drawn only.
        decor.strip_where(
            samples,
            closed,
            |s| s.point(outer(s), -RIBBON_THICKNESS),
            |s| s.point(-outer(s), -RIBBON_THICKNESS),
            |_| skirt,
            zoned,
        );
        for side in [-1.0f32, 1.0] {
            let (a, b): (&dyn Fn(&TrackSample) -> Vec3, &dyn Fn(&TrackSample) -> Vec3) =
                if side > 0.0 {
                    (
                        &|s: &TrackSample| s.point(outer(s), 0.0),
                        &|s: &TrackSample| s.point(outer(s), -RIBBON_THICKNESS),
                    )
                } else {
                    (
                        &|s: &TrackSample| s.point(-outer(s), -RIBBON_THICKNESS),
                        &|s: &TrackSample| s.point(-outer(s), 0.0),
                    )
                };
            decor.strip_where(samples, closed, a, b, |_| skirt, zoned);
        }
    }
    for zone in &data.offroad {
        offroad_patch(&mut meshes.offroad, center, zone, sand);
    }
    // Issue #78: a blue band across the road where each anti-gravity zone starts and ends.
    for zone in &zones {
        for at in [zone.from, zone.to - STRIPE_BAND] {
            let samples = [center.at(at), center.at(at + STRIPE_BAND)];
            meshes.decor.strip(
                &samples,
                false,
                |s| s.point(-s.half_width, STRIPE_HEIGHT),
                |s| s.point(s.half_width, STRIPE_HEIGHT),
                |_| rgba(ANTI_GRAVITY_STRIPE),
            );
        }
    }

    // Issue #107: the shortcuts' grass paths (issue #193: all of them), their skirts down to the
    // ground.
    for shortcut in &data.shortcuts {
        let path = Shortcut::new(center, shortcut);
        let samples = path.samples();
        let half = path.half_width;
        let lift = |s: &TrackSample, lateral: f32| s.point(lateral, SHORTCUT_LIFT);
        match shortcut.kind {
            ShortcutKind::Grass => meshes.shortcut_grass.strip(
                samples,
                false,
                |s| lift(s, -half),
                |s| lift(s, half),
                |_| grass,
            ),
            ShortcutKind::Jump => continue,
        }
        meshes.decor.strip(
            samples,
            false,
            |s| lift(s, half),
            |s| ground(lift(s, half)),
            |_| skirt,
        );
        meshes.decor.strip(
            samples,
            false,
            |s| ground(lift(s, -half)),
            |s| lift(s, -half),
            |_| skirt,
        );
    }

    // Issue #208: the pillars of the bridges, two per bent, from inside the deck down to the
    // ground (the ravine's slopes hide what is below them).
    for bridge in &bridges {
        for at in pillar_bents(center, bridge) {
            let s = center.at(at);
            let f = Vec3::new(s.forward.x, 0.0, s.forward.z).normalize_or(Vec3::Z);
            let r = f.cross(Vec3::Y);
            let half = PILLAR_WIDTH / 2.0;
            for side in [-1.0f32, 1.0] {
                let top = s.point(side * PILLAR_SPREAD * s.half_width, -BRIDGE_DECK / 2.0);
                // Each face (its outward direction, the way along it that makes it face out).
                for (out, along) in [(r, f), (f, -r), (-r, -f), (-f, r)] {
                    let base = top + out * half;
                    let (a, b) = (base - along * half, base + along * half);
                    meshes.decor.quad([a, ground(a), ground(b), b], skirt);
                }
            }
        }
    }

    // Issue #191: the walls' quads from here on (their ends, the barriers) drawn opaque, as they
    // are solid.
    let strips_end = meshes.walls.positions.len();

    // Issue #20: each end of the road at a gap is closed, from the top of its walls down to the
    // ground: a cliff at the lip, a ledge at the landing.
    let panels = [rgba(palette.walls.0), rgba(palette.walls.1)];
    for gap in &gaps {
        for (distance, facing) in [(gap.from, 1.0f32), (gap.to, -1.0)] {
            let s = center.at(distance);
            let o = outer(&s);
            let (left, right) = if facing > 0.0 { (-o, o) } else { (o, -o) };
            meshes.decor.quad(
                [
                    s.point(right, 0.0),
                    ground(s.point(right, 0.0)),
                    ground(s.point(left, 0.0)),
                    s.point(left, 0.0),
                ],
                skirt,
            );
            // The ends of both walls.
            for side in [-1.0f32, 1.0] {
                let (a, b) = (side * wall_offset(&s), side * o);
                let (a, b) = if side * facing > 0.0 { (b, a) } else { (a, b) };
                meshes.walls.quad(
                    [
                        s.point(a, WALL_HEIGHT),
                        s.point(a, 0.0),
                        s.point(b, 0.0),
                        s.point(b, WALL_HEIGHT),
                    ],
                    panels[0],
                );
            }
        }
    }

    // Issue #180: the face of each drop, from the edge of its upper road down to the ground, in
    // the skirts' color, solid (its own mesh, a wall to the karts: one backing into it bumps into
    // it), and the ends of the upper road's walls.
    for cliff in center.cliffs() {
        let s = center.at(cliff.at - CLIFF_CUT);
        let o = outer(&s);
        meshes.cliffs.quad(
            [
                s.point(o, 0.0),
                ground(s.point(o, 0.0)),
                ground(s.point(-o, 0.0)),
                s.point(-o, 0.0),
            ],
            skirt,
        );
        for side in [-1.0f32, 1.0] {
            if !walled(&edges, center, cliff.at, side) {
                continue;
            }
            let (a, b) = (side * wall_offset(&s), side * o);
            let (a, b) = if side > 0.0 { (b, a) } else { (a, b) };
            meshes.walls.quad(
                [
                    s.point(a, WALL_HEIGHT),
                    s.point(a, 0.0),
                    s.point(b, 0.0),
                    s.point(b, WALL_HEIGHT),
                ],
                panels[0],
            );
            // The start of the lower road's walls (as high as the upper ones' top: `wall_lift`),
            // facing up the road.
            let below = center.at(cliff.at + CLIFF_CUT);
            let tall = WALL_HEIGHT + wall_lift(center, below.distance);
            let (a, b) = (side * wall_offset(&below), side * o);
            let (a, b) = if side > 0.0 { (a, b) } else { (b, a) };
            meshes.walls.quad(
                [
                    below.point(a, tall),
                    below.point(a, 0.0),
                    below.point(b, 0.0),
                    below.point(b, tall),
                ],
                panels[0],
            );
        }
    }

    // Issue #119: a barrier across the road at both ends of an open circuit, between its walls:
    // the run-out's end (facing the karts that coast down it) and the lead-in's start (behind
    // the grid).
    if let Some(gap) = way_back {
        let top = rgba(palette.wall_top);
        barrier(&mut meshes.walls, &center.at(gap.from), -1.0, panels, top);
        barrier(&mut meshes.walls, &center.at(gap.to), 1.0, panels, top);
    }
    let ends = meshes.walls.from_vertex(strips_end);
    meshes.wall_looks.append(&ends);

    meshes
}

/// Issue #191: for every sample of `center`, whether its wall on the left and on the right
/// (`[left, right]`) stands between two legs of the track: a sample of the road (on the road of
/// an open circuit) lies beyond it, on its outer side, within [`GLASS_REACH`] meters of its foot,
/// at least [`GLASS_APART`] meters along the track from it, either way, and nearer to it than
/// [`GLASS_RETURN`] times that.
pub fn walls_between_legs(center: &CenterLine) -> Vec<[bool; 2]> {
    let samples = center.samples();
    const CELL: f32 = 25.0;
    let cell = |p: Vec2| ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
    let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
    for (j, s) in samples.iter().enumerate() {
        if center.on_the_road(s.distance) {
            cells
                .entry(cell(s.position.xz()))
                .or_default()
                .push(j as u32);
        }
    }
    let reach = (GLASS_REACH / CELL).ceil() as i32;
    samples
        .iter()
        .map(|s| {
            [-1.0f32, 1.0].map(|sign| {
                let normal = (s.right * sign).xz().normalize_or_zero();
                let foot = s.point(sign * wall_offset(s), 0.0).xz();
                let (cx, cy) = cell(foot);
                (-reach..=reach).any(|dx| {
                    (-reach..=reach).any(|dy| {
                        cells.get(&(cx + dx, cy + dy)).is_some_and(|list| {
                            list.iter().any(|&j| {
                                let other = &samples[j as usize];
                                let to = other.position.xz() - foot;
                                let apart = center.wrap(other.distance - s.distance);
                                let apart = apart.min(center.length() - apart);
                                let far = to.length();
                                apart >= GLASS_APART
                                    && far <= GLASS_REACH
                                    && far <= GLASS_RETURN * apart
                                    && to.dot(normal) > 0.0
                            })
                        })
                    })
                })
            })
        })
        .collect()
}

/// Issue #187: the UVs of a run of samples (`v` at each sample, its end too when it is closed),
/// for the paving (meters along the centerline, so many tiles that the loop closes on a whole
/// number of them) and for each border (meters along its middle, likewise), and the paving's tint
/// of each quad ([`paving::tint_along`]).
struct Along {
    road: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    tint: Vec<[f32; 4]>,
}

impl Along {
    fn new(center: &CenterLine, samples: &[TrackSample], closed: bool) -> Self {
        let length = center.length();
        let n = samples.len();
        let ends = if closed { n + 1 } else { n };
        // Along the centerline, a meter of road is a meter of texture; in a turn, a little less
        // ([`paving_rate`]): its outer edge is stretched as much as its inner one is squeezed.
        let mut road = Vec::with_capacity(ends);
        let mut along = 0.0;
        let mut distance = samples.first().map_or(0.0, |s| s.distance);
        let mut tint = Vec::with_capacity(ends);
        for i in 0..ends {
            if i > 0 {
                let (s0, s1) = (&samples[i - 1], &samples[i % n]);
                let step = center.wrap(s1.distance - s0.distance);
                tint.push(grey(paving::tint_along(distance + step / 2.0, length)));
                distance += step;
                along += step * paving_rate(s0, s1, step);
            }
            road.push(along);
        }
        // So many tiles that a loop closes on a whole number of them; a run between gaps starts
        // where the centerline is (its meters along it).
        let start = samples.first().map_or(0.0, |s| s.distance);
        let k = if closed && along > 0.0 {
            (along / PAVING_TILE).round().max(1.0) * PAVING_TILE / along
        } else {
            1.0
        };
        let road = road.iter().map(|a| (start + a * k) / PAVING_TILE).collect();
        let side = |sign: f32| {
            arc(samples, closed, |s| {
                s.point(sign * (s.half_width + CURB_WIDTH / 2.0), 0.0)
            })
        };
        Self {
            road,
            left: side(-1.0),
            right: side(1.0),
            tint,
        }
    }

    /// The road between its edges, with its paving: its UVs along the road, or the ground's
    /// (`planar`: `Paving::follows_the_road`).
    fn paving(
        &self,
        paving: &mut PaintedMesh,
        samples: &[TrackSample],
        closed: bool,
        keep: &dyn Fn(&TrackSample, &TrackSample) -> bool,
        planar: bool,
    ) {
        if planar {
            paving.planar_strip_where(
                samples,
                closed,
                |s| s.point(-s.half_width, 0.0),
                |s| s.point(s.half_width, 0.0),
                PAVING_TILE,
                |i| self.tint[i],
                keep,
            );
            return;
        }
        paving.strip_where(
            samples,
            closed,
            |s| s.point(-s.half_width, 0.0),
            |s| s.point(s.half_width, 0.0),
            |s| (-s.half_width / PAVING_TILE, s.half_width / PAVING_TILE),
            &self.road,
            |i| self.tint[i],
            keep,
        );
    }

    /// Its two curbs, with their border (`u` 0 by the road, 1 outside).
    fn borders(
        &self,
        borders: &mut PaintedMesh,
        samples: &[TrackSample],
        closed: bool,
        keep: &dyn Fn(&TrackSample, &TrackSample) -> bool,
    ) {
        let white = |_| grey(1.0);
        borders.strip_where(
            samples,
            closed,
            |s| s.point(-s.half_width - CURB_WIDTH, 0.0),
            |s| s.point(-s.half_width, 0.0),
            |_| (1.0, 0.0),
            &self.left,
            white,
            keep,
        );
        borders.strip_where(
            samples,
            closed,
            |s| s.point(s.half_width, 0.0),
            |s| s.point(s.half_width + CURB_WIDTH, 0.0),
            |_| (0.0, 1.0),
            &self.right,
            white,
            keep,
        );
    }
}

/// Issue #187: how many meters of the paving's texture a meter of the centerline is, between two
/// samples `step` m apart: 1 on a straight; in a turn, `sqrt(1 - (w κ)²)` (`w` the half width,
/// `κ` the turn's curvature in the road's plane), so that its outer edge (`1 + w κ` meters of
/// road a meter of the centerline) is stretched along the texture as much as its inner one
/// (`1 - w κ`) is squeezed: in the tightest hairpins, the square root of what following the
/// centerline alone would give. Never below a half.
fn paving_rate(s0: &TrackSample, s1: &TrackSample, step: f32) -> f32 {
    if step <= 1e-4 {
        return 1.0;
    }
    let w = (s0.half_width + s1.half_width) / 2.0;
    let curvature = (s1.forward - s0.forward).dot(s0.right).abs() / step;
    (1.0 - (w * curvature).powi(2)).max(0.25).sqrt()
}

/// A grey vertex color (sRGB `k`, opaque): it multiplies a texture.
fn grey(k: f32) -> [f32; 4] {
    [k, k, k, 1.0]
}

/// The `v` of a border at each sample: meters along the line `point` makes through them,
/// divided by [`BORDER_TILE`]; one more for a closed run (its end), so many tiles that the loop
/// closes on a whole number of them.
fn arc(samples: &[TrackSample], closed: bool, point: impl Fn(&TrackSample) -> Vec3) -> Vec<f32> {
    let n = samples.len();
    let ends = if closed { n + 1 } else { n };
    let mut out = Vec::with_capacity(ends);
    let mut length = 0.0;
    for i in 0..ends {
        if i > 0 {
            length += point(&samples[i % n]).distance(point(&samples[i - 1]));
        }
        out.push(length);
    }
    let k = if closed && length > 0.0 {
        (length / BORDER_TILE).round().max(1.0) * BORDER_TILE / length
    } else {
        1.0
    };
    out.iter().map(|l| l * k / BORDER_TILE).collect()
}

/// Issue #119: a barrier across the road at `s`, [`WALL_HEIGHT`] high and [`WALL_THICKNESS`]
/// thick from one wall's inner face to the other's, on the side `road` of `s` (-1 behind it, 1
/// ahead): its two faces and its top, each drawn both ways (seen from the road and from the
/// way back); solid like the walls (their mesh).
fn barrier(walls: &mut MeshData, s: &TrackSample, road: f32, panels: [[f32; 4]; 2], top: [f32; 4]) {
    let w = wall_offset(s);
    let depth = s.forward * (road * WALL_THICKNESS);
    let at = |lateral: f32, height: f32, inner: bool| {
        s.point(lateral, height) + if inner { depth } else { Vec3::ZERO }
    };
    let h = WALL_HEIGHT;
    for (inner, color) in [(true, panels[0]), (false, panels[1])] {
        let face = [
            at(-w, 0.0, inner),
            at(w, 0.0, inner),
            at(w, h, inner),
            at(-w, h, inner),
        ];
        walls.quad(face, color);
        walls.quad([face[3], face[2], face[1], face[0]], color);
    }
    let lid = [
        at(-w, h, true),
        at(w, h, true),
        at(w, h, false),
        at(-w, h, false),
    ];
    walls.quad(lid, top);
    walls.quad([lid[3], lid[2], lid[1], lid[0]], top);
}

/// The stretches of the centerline the road is built on (issue #20): between the gaps, each
/// from the end of a gap exactly to the start of the next one (open). Without a gap, the whole
/// loop (closed), sample by sample.
fn runs(center: &CenterLine, gaps: &[Gap]) -> Vec<(Vec<TrackSample>, bool)> {
    let samples = center.samples();
    if gaps.is_empty() {
        return vec![(samples.to_vec(), true)];
    }
    let mut gaps = gaps.to_vec();
    gaps.sort_by(|a, b| a.from.total_cmp(&b.from));
    let step = center.length() / samples.len() as f32;
    let mut runs = Vec::with_capacity(gaps.len());
    for (i, gap) in gaps.iter().enumerate() {
        let start = gap.to;
        let end = gaps[(i + 1) % gaps.len()].from;
        let length = center.wrap(end - start);
        if length < 2.0 * step {
            continue;
        }
        let mut run = vec![center.at(start)];
        let first = (start / step).floor() as usize + 1;
        let mut last = 0.0;
        for k in 0.. {
            let s = samples[(first + k) % samples.len()];
            let along = center.wrap(s.distance - start);
            // Issue #180: or past the end, round the loop (a cut shorter than a sample step: the
            // step of a drop).
            if along >= length - 0.25 * step || along < last {
                break;
            }
            last = along;
            if along > 0.25 * step {
                run.push(s);
            }
        }
        run.push(center.at(end));
        runs.push((run, false));
    }
    runs
}

/// Issue #191: where the walls go: their collider, and how they are drawn (opaque, and their
/// glass tops).
struct WallParts<'a> {
    solid: &'a mut MeshData,
    drawn: &'a mut MeshData,
    glass: &'a mut MeshData,
}

/// The walls of both sides, [`WALL_HEIGHT`] high: inner face, flat top, outer face. The faces
/// alternate the palette's two wall colors in panels (the barriers' look, issue #11). Issue #107:
/// a side's wall only between two samples where `walled` says it stands (`walled(sign, s0, s1)`,
/// sign -1 for the left, 1 for the right); everywhere on a circuit without a stretch without wall.
/// Issue #180: `tall(s)`, their height at each sample ([`WALL_HEIGHT`], higher along a bump's back
/// and below a drop: [`wall_lift`]).
///
/// Issue #191: into `parts.solid`, the collider, whole; into `parts.drawn`, the same quads in the
/// same order, but where `glassed` says the wall stands between two legs (`glassed(sign, s0,
/// s1)`), only its faces' band up to [`GLASS_FROM`]; there, the rest of its faces and its lid
/// into `parts.glass`, in a pale tint of the panels.
fn wall_strips(
    parts: WallParts,
    samples: &[TrackSample],
    closed: bool,
    palette: &Palette,
    walled: &dyn Fn(f32, &TrackSample, &TrackSample) -> bool,
    tall: &dyn Fn(&TrackSample) -> f32,
    glassed: &dyn Fn(f32, &TrackSample, &TrackSample) -> bool,
) {
    let WallParts {
        solid,
        drawn,
        glass,
    } = parts;
    let inner = |s: &TrackSample| wall_offset(s);
    let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
    let panels = [rgba(palette.walls.0), rgba(palette.walls.1)];
    let panel = |i: usize| panels[(i / WALL_PANEL) % 2];
    let pale = |c: [f32; 4]| {
        let k = GLASS_TINT;
        [
            c[0] * k + 1.0 - k,
            c[1] * k + 1.0 - k,
            c[2] * k + 1.0 - k,
            1.0,
        ]
    };
    let pale_panel = |i: usize| pale(panel(i));
    let top = rgba(palette.wall_top);
    for sign in [1.0f32, -1.0] {
        let walled = |s0: &TrackSample, s1: &TrackSample| walled(sign, s0, s1);
        let opaque = |s0: &TrackSample, s1: &TrackSample| walled(s0, s1) && !glassed(sign, s0, s1);
        let clear = |s0: &TrackSample, s1: &TrackSample| walled(s0, s1) && glassed(sign, s0, s1);
        // A face at `lateral` from `low` to `high` (heights), its front toward the road on the
        // inner face (`toward`), away from it on the outer one; the right side's inner face
        // goes up, the left side's down (the strip's front is `(b - a) × forward`).
        let up_first = |toward: bool| (sign > 0.0) == toward;
        let face = |mesh: &mut MeshData,
                    lateral: &dyn Fn(&TrackSample) -> f32,
                    toward: bool,
                    low: &dyn Fn(&TrackSample) -> f32,
                    high: &dyn Fn(&TrackSample) -> f32,
                    color: &dyn Fn(usize) -> [f32; 4],
                    keep: &dyn Fn(&TrackSample, &TrackSample) -> bool| {
            let at = |s: &TrackSample, h: f32| s.point(sign * lateral(s), h);
            if up_first(toward) {
                mesh.strip_where(
                    samples,
                    closed,
                    |s| at(s, low(s)),
                    |s| at(s, high(s)),
                    color,
                    keep,
                );
            } else {
                mesh.strip_where(
                    samples,
                    closed,
                    |s| at(s, high(s)),
                    |s| at(s, low(s)),
                    color,
                    keep,
                );
            }
        };
        let lid = |mesh: &mut MeshData,
                   color: &dyn Fn(usize) -> [f32; 4],
                   keep: &dyn Fn(&TrackSample, &TrackSample) -> bool| {
            // From the inner edge to the outer one on the right, the other way on the left:
            // facing up both ways.
            let (a, b): (&dyn Fn(&TrackSample) -> f32, &dyn Fn(&TrackSample) -> f32) = if sign > 0.0
            {
                (&inner, &outer)
            } else {
                (&outer, &inner)
            };
            mesh.strip_where(
                samples,
                closed,
                |s| s.point(sign * a(s), tall(s)),
                |s| s.point(sign * b(s), tall(s)),
                color,
                keep,
            );
        };
        let ground = |_: &TrackSample| 0.0;
        let band = |_: &TrackSample| GLASS_FROM;
        let full = |s0: &TrackSample, s1: &TrackSample| walled(s0, s1);
        // The order of before (#47): inner face, lid, outer face on the right; on the left,
        // inner face, lid, outer face too.
        for mesh_part in 0..3 {
            match mesh_part {
                0 => {
                    face(solid, &inner, true, &ground, tall, &panel, &full);
                    face(drawn, &inner, true, &ground, tall, &panel, &opaque);
                    face(drawn, &inner, true, &ground, &band, &panel, &clear);
                    face(glass, &inner, true, &band, tall, &pale_panel, &clear);
                }
                1 => {
                    lid(solid, &|_| top, &full);
                    lid(drawn, &|_| top, &opaque);
                    lid(glass, &|_| pale(top), &clear);
                }
                _ => {
                    face(solid, &outer, false, &ground, tall, &panel, &full);
                    face(drawn, &outer, false, &ground, tall, &panel, &opaque);
                    face(drawn, &outer, false, &ground, &band, &panel, &clear);
                    face(glass, &outer, false, &band, tall, &pale_panel, &clear);
                }
            }
        }
    }
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
    use crate::track::data::{MAX_BUMP_SLOPE, TrackData};
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

    /// Issue #108: the test circuit (the layout Prairie Pompon had before #108).
    pub(crate) fn test_track() -> TrackData {
        crate::track::test_circuit()
    }

    /// Issue #187: what is drawn with a renderer (the paving, the borders and the rest plain) is
    /// exactly what was drawn before, in one mesh (`combined`): every triangle once, none more,
    /// on circuits with anti-gravity zones (their blue curbs plain), shortcuts, bumps and drops,
    /// gaps; and the paving's and borders' triangles are the road's surface and its curbs.
    #[test]
    fn the_drawn_road_is_the_same_triangles_as_before() {
        let key = |m: &MeshData, t: &[u32; 3]| {
            let mut corners: Vec<[i64; 3]> = t
                .iter()
                .map(|i| m.positions[*i as usize].map(|c| (c * 1000.0).round() as i64))
                .collect();
            corners.sort();
            corners
        };
        let triangles = |meshes: &[&MeshData]| {
            let mut all: Vec<Vec<[i64; 3]>> = meshes
                .iter()
                .flat_map(|m| m.indices.as_chunks::<3>().0.iter().map(|t| key(m, t)))
                .collect();
            all.sort();
            all
        };
        let files = [
            "prairie.track.ron",
            "ruban.track.ron",
            "corniche.track.ron",
            "lagon.track.ron",
            "echappee.track.ron",
        ];
        for file in files {
            let path = format!("{}/assets/tracks/{file}", env!("CARGO_MANIFEST_DIR"));
            let data = TrackData::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
            let center = CenterLine::of(&data);
            let meshes = generate(&center, &data, &data.palette());
            let drawn = meshes.drawn();
            // Issue #191: and the walls' glass tops, drawn apart too.
            assert_eq!(
                triangles(&[
                    &drawn.plain,
                    &drawn.paving.mesh,
                    &drawn.borders.mesh,
                    &meshes.glass
                ]),
                triangles(&[&meshes.combined()]),
                "{file}"
            );
            assert_eq!(drawn.paving.uvs.len(), drawn.paving.mesh.positions.len());
            assert_eq!(drawn.borders.uvs.len(), drawn.borders.mesh.positions.len());
            // The paving and the borders: the road's triangles (issue #193: no shortcut is a road).
            let road = triangles(&[&meshes.road, &meshes.anti_gravity_road, &meshes.trick_road]);
            for t in triangles(&[&drawn.paving.mesh, &drawn.borders.mesh]) {
                assert!(road.binary_search(&t).is_ok(), "{file}: {t:?}");
            }
            assert!(
                !drawn.paving.is_empty() && !drawn.borders.is_empty(),
                "{file}"
            );
            if !data.anti_gravity.is_empty() {
                assert!(!meshes.blue_curbs.indices.is_empty(), "{file}");
            }
        }
    }

    /// Issue #119: on an open circuit, nothing is built on the way back (no road, no shoulder, no
    /// wall: every vertex of them is by a sample of the road, the ends of the way back included);
    /// a barrier closes the road at both ends, across it from wall to wall, as high as the walls
    /// (in the walls' mesh: solid); and the road runs on from the lead-in, through the start
    /// line, to the end of the run-out.
    #[test]
    fn an_open_circuit_has_no_road_on_its_way_back_and_a_barrier_at_each_end() {
        let data = crate::track::test_open_circuit();
        let center = CenterLine::of(&data);
        let open = center.open().unwrap().clone();
        let meshes = generate(&center, &data, &Palette::default());
        let road = center.road_samples();
        let near_the_road = |p: Vec3| road.iter().any(|s| s.position.distance(p) < 15.0);
        for (name, mesh) in [
            ("road", &meshes.road),
            ("offroad", &meshes.offroad),
            ("walls", &meshes.walls),
        ] {
            for p in &mesh.positions {
                let p = Vec3::from_array(*p);
                assert!(near_the_road(p), "{name}: {p} on the way back");
            }
        }
        // The way back is not on the road.
        let back = open.road_end + center.wrap(open.road_start - open.road_end) / 2.0;
        assert!(!center.on_the_road(back));
        for (end, inward) in [(open.road_end, -1.0), (open.road_start, 1.0)] {
            let s = center.at(end);
            // Wall triangles across the road there: their middles from one wall to the other.
            let across: Vec<Vec3> = meshes
                .walls
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    t.iter()
                        .map(|i| Vec3::from_array(meshes.walls.positions[*i as usize]))
                        .sum::<Vec3>()
                        / 3.0
                })
                .filter(|m| (*m - s.position).dot(s.forward).abs() < WALL_THICKNESS + 0.01)
                .collect();
            let lateral = |m: &Vec3| (*m - s.position).dot(s.right);
            let reach = across
                .iter()
                .map(lateral)
                .fold(0.0f32, |a, l| a.max(l.abs()));
            assert!(
                across.iter().any(|m| lateral(m) < -1.0) && across.iter().any(|m| lateral(m) > 1.0),
                "a barrier across the road at {end}"
            );
            assert!(reach <= wall_offset(&s) + WALL_THICKNESS + 0.01, "{reach}");
            // On the road's side of the end.
            assert!(
                across
                    .iter()
                    .all(|m| (*m - s.position).dot(s.forward) * inward >= -0.01)
            );
            assert!(
                across
                    .iter()
                    .any(|m| (*m - s.position).dot(s.up) > WALL_HEIGHT * 0.6)
            );
        }
        // The road through the start line, from the lead-in to the run-out.
        for distance in [open.road_start + 5.0, 0.0, open.finish, open.road_end - 5.0] {
            let p = center.at(distance).position;
            assert!(
                meshes
                    .road
                    .positions
                    .iter()
                    .any(|q| Vec3::from_array(*q).distance(p) < 8.0),
                "the road at {distance}"
            );
        }
    }

    /// Issue #119: the tiles of a mesh are its triangles, each one whole in one tile (by its
    /// first corner), none lost nor doubled.
    #[test]
    fn a_mesh_in_tiles_keeps_every_triangle() {
        let data = crate::track::test_open_circuit();
        let center = CenterLine::of(&data);
        let mesh = generate(&center, &data, &Palette::default()).combined();
        let tiles = mesh.tiles(200.0);
        assert!(tiles.len() > 3, "{}", tiles.len());
        assert_eq!(
            tiles.iter().map(MeshData::triangle_count).sum::<usize>(),
            mesh.triangle_count()
        );
        let triangle = |m: &MeshData, t: &[u32; 3]| {
            t.map(|i| {
                let i = i as usize;
                (m.positions[i], m.normals[i], m.colors[i])
            })
        };
        let key = |c: [([f32; 3], [f32; 3], [f32; 4]); 3]| format!("{c:?}");
        let mut all: Vec<String> = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| key(triangle(&mesh, t)))
            .collect();
        let mut tiled: Vec<String> = tiles
            .iter()
            .flat_map(|m| {
                m.indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|t| key(triangle(m, t)))
                    .collect::<Vec<_>>()
            })
            .collect();
        all.sort();
        tiled.sort();
        assert_eq!(all, tiled);
        for tile in &tiles {
            assert!(tile.positions.len() <= 3 * tile.triangle_count());
            let first = tile.positions[tile.indices[0] as usize];
            let cell = |p: [f32; 3]| ((p[2] / 200.0).floor(), (p[0] / 200.0).floor());
            for t in tile.indices.as_chunks::<3>().0 {
                assert_eq!(cell(tile.positions[t[0] as usize]), cell(first));
            }
        }
    }

    #[test]
    fn road_ribbon_is_a_closed_loop() {
        let data = test_track();
        let center = CenterLine::of(&data);
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
            // Issue #148: the oval the Anneau Guimauve was before #148 (walls all around, no
            // bump: every wall vertex on the road or `WALL_HEIGHT` above it).
            crate::track::test_oval(),
        ] {
            let center = CenterLine::of(&data);
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
            // What is drawn is all of it: the walls are in the circuit's single mesh. Issue
            // #191: as drawn, opaque then their glass tops (on the oval, the inner walls of its
            // half circles), every vertex of the collider one of theirs.
            let all = meshes.combined();
            let start = meshes.road.positions.len() + meshes.offroad.positions.len();
            let (looks, glass) = (&meshes.wall_looks.positions, &meshes.glass.positions);
            assert_eq!(all.positions[start..start + looks.len()], looks[..]);
            let start = start + looks.len();
            assert_eq!(all.positions[start..start + glass.len()], glass[..]);
            let drawn: Vec<[i64; 3]> = looks
                .iter()
                .chain(glass)
                .map(|p| p.map(|c| (c * 1000.0).round() as i64))
                .collect();
            for p in walls.positions.iter() {
                let p = p.map(|c| (c * 1000.0).round() as i64);
                assert!(drawn.contains(&p), "{}: {p:?} not drawn", data.name);
            }
        }
    }

    /// Issue #20, on the Lagon Duvet: no road, no shoulder and no wall in its gap (each one's
    /// vertices on samples out of it, or on its two ends), both ends closed by a face down to the
    /// ground; and under the water, the skirts reach the floor of the lake.
    #[test]
    fn the_road_stops_at_the_gaps_and_the_skirts_reach_the_lake_floor() {
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/lagon.track.ron")).unwrap();
        let center = CenterLine::of(&data);
        let gap = Gap::new(&center, &data.gaps[0]);
        let lake = Lake::new(&data.lakes[0]);
        let meshes = generate(&center, &data, &Palette::default());
        let inside = |p: Vec3| {
            let s = &center.samples()[center.nearest_index(p)];
            let along = center.wrap(s.distance - gap.from);
            along > 1.5 && along < gap.length(&center) - 1.5
        };
        for (name, mesh) in [
            ("road", &meshes.road),
            ("offroad", &meshes.offroad),
            ("walls", &meshes.walls),
        ] {
            assert!(!mesh.positions.is_empty());
            for p in &mesh.positions {
                assert!(!inside(Vec3::from_array(*p)), "{name} in the gap at {p:?}");
            }
        }
        // The cliff at the lip: a face from the road's height down to the floor of the lake
        // under it, facing the gap.
        let lip = center.at(gap.from);
        let cliff = meshes
            .decor
            .normals
            .iter()
            .zip(&meshes.decor.positions)
            .filter(|(n, p)| {
                Vec3::from_array(**n).dot(lip.forward) > 0.99
                    && Vec3::from_array(**p).xz().distance(lip.position.xz()) < 20.0
            })
            .map(|(_, p)| p[1])
            .collect::<Vec<f32>>();
        assert!(!cliff.is_empty(), "the lip is closed");
        let floor = lake.floor;
        assert!(cliff.iter().any(|y| (y - floor).abs() < 1e-4), "{cliff:?}");
        assert!(
            cliff.iter().any(|y| (y - lip.position.y).abs() < 0.5),
            "{cliff:?}"
        );
        // The skirts under the water reach the floor.
        assert!(
            meshes
                .decor
                .positions
                .iter()
                .any(|p| (p[1] - floor).abs() < 1e-4 && lake.covers(Vec2::new(p[0], p[2])))
        );
    }

    /// Issue #78: on the Ruban Réglisse, the road and the shoulders of its anti-gravity zones are
    /// meshes of their own (the colliders that put a kart in anti-gravity), exactly the quads of
    /// the zones: with the rest of the road, the whole ribbon, closed, as on any circuit; its curbs
    /// blue and white, two blue stripes along it and a band at each end (drawn only), all of them
    /// facing the road's normal (up to the wall, 90°, issue #94 the ceiling, 180°, and issue #96
    /// a turn banked to 90°). On a circuit without a zone, none of them: the meshes of before #78.
    #[test]
    fn the_anti_gravity_zone_has_its_own_road_and_its_blue_stripes() {
        let empty = generate(
            &CenterLine::of(&test_track()),
            &test_track(),
            &Palette::default(),
        );
        assert!(empty.anti_gravity_road.positions.is_empty());
        assert!(empty.anti_gravity_offroad.positions.is_empty());
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/ruban.track.ron")).unwrap();
        let center = CenterLine::of(&data);
        let zones: Vec<AntiGravityZone> = data
            .anti_gravity
            .iter()
            .map(|z| AntiGravityZone::new(&center, z))
            .collect();
        // The wall, the ceiling (issue #94) and the banked turn (issue #96).
        assert_eq!(zones.len(), 3);
        let meshes = generate(&center, &data, &Palette::default());
        let samples = center.samples();
        let zone_of = |p: [f32; 3]| {
            let s = &samples[center.nearest_index(Vec3::from_array(p))];
            anti_gravity_at(&zones, &center, s.distance).copied()
        };
        let in_zone = |p: [f32; 3]| zone_of(p).is_some();
        let n = samples.len();
        let zoned = (0..n)
            .filter(|i| {
                let (a, b) = (&samples[*i], &samples[(i + 1) % n]);
                anti_gravity_at(
                    &zones,
                    &center,
                    a.distance + center.wrap(b.distance - a.distance) / 2.0,
                )
                .is_some()
            })
            .count();
        // Asphalt and two curbs per quad, in the zone or out of it.
        assert_eq!(meshes.anti_gravity_road.triangle_count(), 3 * 2 * zoned);
        assert_eq!(meshes.road.triangle_count(), 3 * 2 * (n - zoned));
        assert_eq!(meshes.anti_gravity_offroad.triangle_count(), 2 * 2 * zoned);
        let mut both = meshes.road.clone();
        both.append(&meshes.anti_gravity_road);
        assert_eq!(
            count_loops(&boundary_edges(&both)),
            Some(2),
            "one closed ribbon"
        );
        for (i, p) in meshes.anti_gravity_road.positions.iter().enumerate() {
            let s = &samples[center.nearest_index(Vec3::from_array(*p))];
            let normal = Vec3::from_array(meshes.anti_gravity_road.normals[i]);
            // The twist tilts the surface off the centerline's normal toward its edges (a
            // helicoid: atan(lateral × roll per meter), about 9° at the curbs).
            assert!(
                normal.dot(s.up) > 0.95,
                "the zone's road faces its normal: {p:?} {normal} {} at {}",
                s.up,
                s.distance
            );
        }
        let near_an_end = |p: [f32; 3], margin: f32| {
            let s = &samples[center.nearest_index(Vec3::from_array(p))];
            // Either side of an end (the nearest sample may be just beyond it).
            let near = |end: f32| center.wrap(s.distance - end + margin) < 2.0 * margin;
            zones.iter().any(|zone| near(zone.from) || near(zone.to))
        };
        assert!(meshes.road.positions.iter().all(|p| !in_zone(*p) || {
            // A vertex at the zone's ends is shared.
            near_an_end(*p, 1.5)
        }));
        // On the wall itself, the road faces the side; on the ceiling (issue #94), down.
        let wall = center.at(center.param_to_distance(9.5));
        assert!(wall.up.y.abs() < 0.01);
        assert!(
            meshes
                .anti_gravity_road
                .normals
                .iter()
                .any(|n| n[1].abs() < 0.01)
        );
        let ceiling = center.at(center.param_to_distance(39.5));
        assert!(ceiling.up.y < -0.999);
        assert!(
            meshes
                .anti_gravity_road
                .normals
                .iter()
                .any(|n| n[1] < -0.999)
        );
        // Blue and white curbs, blue stripes: only there, and in the drawn mesh.
        let blue = Color::srgba(
            ANTI_GRAVITY_STRIPE.0,
            ANTI_GRAVITY_STRIPE.1,
            ANTI_GRAVITY_STRIPE.2,
            1.0,
        )
        .to_linear()
        .to_f32_array();
        let stripes: Vec<usize> = (0..meshes.decor.colors.len())
            .filter(|i| meshes.decor.colors[*i] == blue)
            .collect();
        assert!(!stripes.is_empty());
        for i in &stripes {
            let p = meshes.decor.positions[*i];
            assert!(
                in_zone(p) || near_an_end(p, 2.0),
                "a stripe out of the zone at {p:?}"
            );
            let s = &samples[center.nearest_index(Vec3::from_array(p))];
            let height = (Vec3::from_array(p) - s.position).dot(s.up);
            assert!(
                (height - 0.02).abs() < 0.02,
                "a stripe just above the road: {height}"
            );
            assert!(Vec3::from_array(meshes.decor.normals[*i]).dot(s.up) > 0.95);
        }
        let curb = Color::srgba(
            ANTI_GRAVITY_CURBS.0.0,
            ANTI_GRAVITY_CURBS.0.1,
            ANTI_GRAVITY_CURBS.0.2,
            1.0,
        )
        .to_linear()
        .to_f32_array();
        assert!(meshes.anti_gravity_road.colors.contains(&curb));
        assert!(!meshes.road.colors.contains(&curb));
        let all = meshes.combined();
        assert_eq!(
            all.positions[all.positions.len() - meshes.anti_gravity_offroad.positions.len()..],
            meshes.anti_gravity_offroad.positions[..]
        );
    }

    /// Issue #94: on the Ruban Réglisse, nothing hangs from the road down to the ground where it
    /// faces down (its ceiling): the skirts hang only under a road that faces up (to the wall,
    /// 90°), they would hang through the karts driving under it. Each zone's ribbon has its back
    /// instead, `RIBBON_THICKNESS` behind the road and facing away from it (up, over the ceiling),
    /// and its edges. On a circuit without a zone, the skirts of before, every one, and no back.
    #[test]
    fn nothing_hangs_under_the_ceiling_and_the_ribbon_has_its_back() {
        let prairie = test_track();
        let center = CenterLine::of(&prairie);
        let meshes = generate(&center, &prairie, &Palette::default());
        // Two skirts per sample, nothing else.
        assert_eq!(
            meshes.decor.triangle_count(),
            2 * 2 * center.samples().len()
        );
        let data =
            TrackData::from_ron(include_str!("../../assets/tracks/ruban.track.ron")).unwrap();
        for data in [data.clone(), data.mirrored()] {
            let center = CenterLine::of(&data);
            let zones: Vec<AntiGravityZone> = data
                .anti_gravity
                .iter()
                .map(|z| AntiGravityZone::new(&center, z))
                .collect();
            let meshes = generate(&center, &data, &Palette::default());
            let samples = center.samples();
            let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
            let decor = &meshes.decor;
            // Every foot of a skirt (on the ground) hangs from an edge of the road right above it,
            // where the road faces up.
            let mut feet = 0;
            for v in decor.positions.iter().map(|p| Vec3::from_array(*p)) {
                if (v.y - GROUND_HEIGHT).abs() > 1e-4 {
                    continue;
                }
                feet += 1;
                let (distance, s) = samples
                    .iter()
                    .flat_map(|s| [-1.0f32, 1.0].map(|side| (s.point(side * outer(s), 0.0), s)))
                    .map(|(edge, s)| (edge.xz().distance(v.xz()), s))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .unwrap();
                assert!(distance < 1e-3, "a foot under no edge: {v}");
                assert!(
                    s.up.y > -0.02,
                    "a skirt under the road upside down at {:.0} m",
                    s.distance
                );
            }
            assert!(feet > 2 * samples.len(), "{feet}");
            // The ribbon's back, all along the zones, facing away from the road; its edges facing
            // out.
            let facing = |p: Vec3, toward: Vec3| {
                decor.positions.iter().zip(&decor.normals).any(|(q, n)| {
                    Vec3::from_array(*q).distance(p) < 1e-3
                        && Vec3::from_array(*n).dot(toward) > 0.95
                })
            };
            let mut ceilings = 0;
            for s in samples
                .iter()
                .filter(|s| anti_gravity_at(&zones, &center, s.distance + 0.5).is_some())
            {
                for side in [-1.0f32, 1.0] {
                    let corner = s.point(side * outer(s), -RIBBON_THICKNESS);
                    assert!(facing(corner, -s.up), "no back at {:.0} m", s.distance);
                    assert!(
                        facing(corner, side * s.right),
                        "no edge at {:.0} m",
                        s.distance
                    );
                }
                ceilings += usize::from(s.up.y < -0.999);
            }
            assert!(ceilings >= 25, "{ceilings}");
        }
    }

    /// Issue #208: on the Col des Moufles and its mirror, a bridge has no skirt down to the ground:
    /// it has its deck (its underside `BRIDGE_DECK` under the road, facing down, at every sample)
    /// and its pillars (two per bent, `PILLAR_SPACING` apart at most, none at its abutments), and
    /// whatever of it reaches the ground is the foot of a pillar. Its walls stand on both sides.
    #[test]
    fn a_bridge_stands_on_pillars_instead_of_skirts() {
        let data = TrackData::from_ron(include_str!("../../assets/tracks/col.track.ron")).unwrap();
        assert!(!data.bridges.is_empty());
        for data in [data.clone(), data.mirrored()] {
            let center = CenterLine::of(&data);
            let meshes = generate(&center, &data, &Palette::default());
            let decor = &meshes.decor;
            let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
            let facing = |p: Vec3, toward: Vec3| {
                decor.positions.iter().zip(&decor.normals).any(|(q, n)| {
                    Vec3::from_array(*q).distance(p) < 1e-3
                        && Vec3::from_array(*n).dot(toward) > 0.95
                })
            };
            for bridge in data.bridges.iter().map(|b| Bridge::new(&center, b)) {
                let length = bridge.length(&center);
                let bents = pillar_bents(&center, &bridge);
                assert!(bents.len() >= 2, "{} bents", bents.len());
                let mut last = bridge.from;
                for at in bents.iter().chain([&center.wrap(bridge.from + length)]) {
                    let span = center.wrap(at - last);
                    assert!(
                        span > 1.0 && span <= PILLAR_SPACING + 1e-3,
                        "a span of {span} m"
                    );
                    last = *at;
                }
                let axes: Vec<Vec3> = bents
                    .iter()
                    .flat_map(|at| {
                        let s = center.at(*at);
                        [-1.0f32, 1.0].map(|side| {
                            s.point(side * PILLAR_SPREAD * s.half_width, -BRIDGE_DECK / 2.0)
                        })
                    })
                    .collect();
                // The deck under every sample of the bridge (its quads: from the second to the
                // one before the last), facing down; no skirt hanging from its edges.
                let samples: Vec<&TrackSample> = center
                    .samples()
                    .iter()
                    .filter(|s| {
                        bridge
                            .along(&center, s.distance)
                            .is_some_and(|x| x > 1.0 && x < length - 1.0)
                    })
                    .collect();
                assert!(samples.len() as f32 >= length - 4.0, "{}", samples.len());
                for s in &samples {
                    for side in [-1.0f32, 1.0] {
                        let under = s.point(side * outer(s), -BRIDGE_DECK);
                        assert!(facing(under, -s.up), "no deck at {:.0} m", s.distance);
                        assert!(
                            facing(under, side * s.right),
                            "no edge at {:.0} m",
                            s.distance
                        );
                    }
                }
                // No skirt hangs from its edges down to the ground; the pillars stand there
                // instead, on their feet (two per face, four faces).
                let on_ground = |p: Vec3| {
                    decor.positions.iter().any(|q| {
                        let q = Vec3::from_array(*q);
                        (q.y - GROUND_HEIGHT).abs() < 1e-4 && q.xz().distance(p.xz()) < 1e-3
                    })
                };
                for s in &samples {
                    for side in [-1.0f32, 1.0] {
                        let edge = s.point(side * outer(s), 0.0);
                        assert!(
                            !on_ground(edge),
                            "a skirt under the bridge at {:.0} m",
                            s.distance
                        );
                    }
                }
                for axis in &axes {
                    let feet = decor
                        .positions
                        .iter()
                        .map(|q| Vec3::from_array(*q))
                        .filter(|q| {
                            (q.y - GROUND_HEIGHT).abs() < 1e-4
                                && q.xz().distance(axis.xz()) <= PILLAR_WIDTH * 0.71
                        })
                        .count();
                    assert_eq!(feet, 2 * 4, "the feet of the pillar at {axis}");
                }
                // Walled on both sides.
                assert!(
                    data.open_edges.iter().all(|e| {
                        let e = OpenEdge::new(&center, e);
                        bents.iter().all(|at| !e.contains(&center, *at))
                    }),
                    "a bridge without its walls"
                );
            }
        }
    }

    /// Issue #107: on the Corniche Cannelle and its mirror, a side without wall has no wall, and
    /// its shoulder runs on to the wall's outer face (no slit between it and the skirt); every
    /// other side has its wall (but over the gap); the road shortcut has its road and its curbs,
    /// the grass one its grass, the jump nothing (its ramp is a box): each as long as its path,
    /// lifted over it. A circuit without them builds none.
    #[test]
    fn the_sides_without_wall_have_none_and_the_shortcuts_their_road() {
        use crate::track::shortcut::{OpenEdge, Shortcut, walled};
        use std::collections::HashSet;
        let key = |p: Vec3| {
            [
                (p.x * 100.0).round() as i64,
                (p.y * 100.0).round() as i64,
                (p.z * 100.0).round() as i64,
            ]
        };
        let file =
            TrackData::from_ron(include_str!("../../assets/tracks/corniche.track.ron")).unwrap();
        for data in [file.clone(), file.mirrored()] {
            let center = CenterLine::of(&data);
            let meshes = generate(&center, &data, &Palette::default());
            let edges: Vec<OpenEdge> = data
                .open_edges
                .iter()
                .map(|e| OpenEdge::new(&center, e))
                .collect();
            let gaps: Vec<Gap> = data.gaps.iter().map(|g| Gap::new(&center, g)).collect();
            let walls: HashSet<[i64; 3]> = meshes
                .walls
                .positions
                .iter()
                .map(|p| key(Vec3::from_array(*p)))
                .collect();
            let shoulders: HashSet<[i64; 3]> = meshes
                .offroad
                .positions
                .iter()
                .map(|p| key(Vec3::from_array(*p)))
                .collect();
            let (mut open, mut closed) = (0, 0);
            for s in center.samples() {
                // Clear of the gap and its ends (closed by their own faces).
                if gaps.iter().any(|g| {
                    g.contains(&center, s.distance + 2.0) || g.contains(&center, s.distance - 2.0)
                }) {
                    continue;
                }
                for side in [-1.0f32, 1.0] {
                    // Clear of a stretch's ends (a sample's quads on both sides agree).
                    let all = |d: f32| {
                        [-1.5f32, 0.0, 1.5].iter().all(|o| {
                            walled(&edges, &center, d + o, side) == walled(&edges, &center, d, side)
                        })
                    };
                    if !all(s.distance) {
                        continue;
                    }
                    let top = key(s.point(side * wall_offset(s), WALL_HEIGHT));
                    let edge = key(s.point(side * (wall_offset(s) + WALL_THICKNESS), 0.0));
                    if walled(&edges, &center, s.distance, side) {
                        closed += 1;
                        assert!(
                            walls.contains(&top),
                            "{:.0} m, side {side}: no wall",
                            s.distance
                        );
                    } else {
                        open += 1;
                        assert!(
                            !walls.contains(&top),
                            "{:.0} m, side {side}: a wall",
                            s.distance
                        );
                        assert!(shoulders.contains(&edge), "{:.0} m: a slit", s.distance);
                    }
                }
            }
            assert!(open > 250 && closed > 1500, "{open} open, {closed} walled");
            let shortcuts: Vec<Shortcut> = data
                .shortcuts
                .iter()
                .map(|s| Shortcut::new(&center, s))
                .collect();
            let quads = |kind: ShortcutKind| {
                shortcuts
                    .iter()
                    .filter(|s| s.kind == kind)
                    .map(|s| s.samples().len() - 1)
                    .sum::<usize>()
            };
            assert_eq!(
                meshes.shortcut_grass.positions.len(),
                4 * quads(ShortcutKind::Grass)
            );
            // Issue #193: the hidden road and the grass bridge, both grass.
            assert_eq!(
                shortcuts
                    .iter()
                    .filter(|s| s.kind == ShortcutKind::Grass)
                    .count(),
                2
            );
            assert!(quads(ShortcutKind::Grass) > 80);
            for p in &meshes.shortcut_grass.positions {
                let p = Vec3::from_array(*p);
                // On one of the grass paths: the one it lies on (the nearest).
                let here = shortcuts
                    .iter()
                    .filter(|s| s.kind == ShortcutKind::Grass)
                    .map(|path| (path.half_width, path.locate(p)))
                    .min_by(|a, b| {
                        let off =
                            |h: &crate::track::shortcut::OnPath| h.height.abs() + h.lateral.abs();
                        off(&a.1).total_cmp(&off(&b.1))
                    })
                    .unwrap();
                assert!((here.1.height - SHORTCUT_LIFT).abs() < 0.01, "{here:?}");
                assert!(here.1.lateral.abs() <= here.0 + 0.01, "{here:?}");
            }
        }
        let prairie = test_track();
        let meshes = generate(&CenterLine::of(&prairie), &prairie, &Palette::default());
        assert!(meshes.shortcut_grass.indices.is_empty());
    }

    #[test]
    fn floors_face_up_and_walls_face_the_road() {
        let data = test_track();
        let center = CenterLine::of(&data);
        let meshes = generate(&center, &data, &Palette::default());
        assert!(meshes.road.normals.iter().all(|n| n[1] > 0.7));
        assert!(meshes.offroad.normals.iter().all(|n| n[1] > 0.7));
        // The first wall face of each side faces the centerline.
        let s = &center.samples()[10];
        let right_inner = Vec3::from_array(meshes.walls.normals[10 * 4]);
        assert!(right_inner.dot(s.right) < -0.9, "{right_inner}");
    }

    /// Issue #180, on Prairie Pompon and its mirror: the road, its shoulders, its walls and its
    /// skirts follow its bumps (each vertex on the frame of its sample: the walls `WALL_HEIGHT`
    /// above the road there up to the crest, as high as over the crest from there to the bump's
    /// end, the parapet, then back down; the skirts hanging from its edge); nothing joins the two levels of
    /// its drop (no floor quad climbs more than a bump's slope between its samples), a face closes
    /// the upper road's end from its edge down to the ground (its own mesh, solid); the trick
    /// stretches have their own road and shoulders, and nothing else does. A circuit without
    /// relief has no trick stretch.
    #[test]
    fn the_track_follows_the_bumps_and_stops_at_the_drop() {
        let text = include_str!("../../assets/tracks/prairie.track.ron");
        let prairie = TrackData::from_ron(text).unwrap();
        for data in [prairie.clone(), prairie.mirrored()] {
            let center = CenterLine::of(&data);
            let meshes = generate(&center, &data, &Palette::default());
            let step = center.length() / center.samples().len() as f32;
            let stretches = center.trick_stretches();
            // Where a vertex stands along the track, and across it.
            let locate = |p: Vec3| {
                let s = center.samples()[center.nearest_index(p)];
                let along = s.distance + (p - s.position).dot(s.forward);
                (center.at(along), along)
            };
            for (name, mesh, tricky) in [
                ("road", &meshes.road, false),
                ("offroad", &meshes.offroad, false),
                ("trick road", &meshes.trick_road, true),
                ("trick offroad", &meshes.trick_offroad, true),
            ] {
                assert!(!mesh.indices.is_empty(), "{name}");
                for quad in mesh.positions.as_chunks::<4>().0 {
                    let corners = quad.map(Vec3::from_array);
                    let (low, high) = corners
                        .iter()
                        .fold((f32::MAX, f32::MIN), |(l, h), c| (l.min(c.y), h.max(c.y)));
                    // A sand patch is one quad per sample too, and the bank tilts a quad: a
                    // bump's slope at most, plus the banking across.
                    let across = (corners[0] - corners[1]).length() * 0.2;
                    assert!(
                        high - low <= step * (MAX_BUMP_SLOPE + 0.05) + across,
                        "{name}: a quad from {low} to {high}: {corners:?}"
                    );
                    let middle = corners.iter().sum::<Vec3>() / 4.0;
                    let (_, along) = locate(middle);
                    // (Its middle a hair off its samples' middle: not right on a boundary.)
                    let boundary = stretches.iter().any(|(start, length)| {
                        [*start, start + length]
                            .iter()
                            .any(|b| center.wrap(along - b + 0.3) < 0.6)
                    });
                    if boundary {
                        continue;
                    }
                    assert_eq!(
                        center.on_a_trick_stretch(&stretches, along),
                        tricky,
                        "{name} at {along}"
                    );
                }
            }
            for bump in center.bumps() {
                let crest = center.at(bump.from + bump.length / 2.0);
                for side in [-1.0f32, 1.0] {
                    // The wall's top over the crest, and the skirt hanging from its edge.
                    let top = crest.point(side * wall_offset(&crest), WALL_HEIGHT);
                    let edge = crest.point(side * (wall_offset(&crest) + WALL_THICKNESS), 0.0);
                    let near = |mesh: &MeshData, p: Vec3| {
                        mesh.positions
                            .iter()
                            .any(|q| Vec3::from_array(*q).distance(p) < step * 0.6)
                    };
                    assert!(near(&meshes.walls, top), "the wall over {crest:?}");
                    // At its end, the walls as high as over its crest (the parapet), back to
                    // their own height `PARAPET_RUN` meters further.
                    let end = center.at(bump.from + bump.length);
                    let parapet = end.point(side * wall_offset(&end), WALL_HEIGHT + bump.height);
                    assert!(near(&meshes.walls, parapet), "the parapet at {end:?}");
                    let after = center.at(bump.from + bump.length + PARAPET_RUN + 1.0);
                    let low = after.point(side * wall_offset(&after), WALL_HEIGHT);
                    assert!(near(&meshes.walls, low), "the wall after {after:?}");
                    assert!(near(&meshes.decor, edge), "the skirt at {crest:?}");
                    let skirt_foot = Vec3::new(edge.x, GROUND_HEIGHT, edge.z);
                    assert!(near(&meshes.decor, skirt_foot), "{crest:?}");
                }
            }
            let cliff = center.cliffs()[0];
            let lip = center.at(cliff.at - CLIFF_CUT);
            let o = wall_offset(&lip) + WALL_THICKNESS;
            let face = [lip.point(o, 0.0), lip.point(-o, 0.0)];
            let feet = face.map(|p| Vec3::new(p.x, GROUND_HEIGHT, p.z));
            assert!(
                meshes
                    .cliffs
                    .positions
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|quad| {
                        let quad = quad.map(Vec3::from_array);
                        face.iter()
                            .chain(&feet)
                            .all(|p| quad.iter().any(|q| q.distance(*p) < 1e-3))
                    }),
                "the face of the drop"
            );
        }
        let plain = test_track();
        let meshes = generate(&CenterLine::of(&plain), &plain, &Palette::default());
        assert!(meshes.trick_road.indices.is_empty() && meshes.trick_offroad.indices.is_empty());
    }

    /// Issue #191, on every circuit of the game and its mirror: the walls are drawn as they are
    /// solid, a glass top where they stand between two legs of the track. Their collider is
    /// whole (every quad [`WALL_HEIGHT`] high, as before); every vertex of it is a vertex of the
    /// opaque walls or of the glass; the glass is only between [`GLASS_FROM`] and the walls'
    /// top; the walls between two legs are those of the lacets and of the hairpins (a share of
    /// the walls on the circuits with hairpins; on the oval of the tests, the inner walls of its
    /// half circles only).
    #[test]
    fn the_walls_between_two_legs_have_a_glass_top_and_stay_whole() {
        use std::collections::HashSet;
        let key = |p: [f32; 3]| {
            [
                (p[0] * 1000.0).round() as i64,
                (p[1] * 1000.0).round() as i64,
                (p[2] * 1000.0).round() as i64,
            ]
        };
        // The oval of the tests (it turns right): glass only on the inner walls of its two half
        // circles (20 to 35 m, the road coming back toward them across the infield), never on
        // its outer walls, never along its straights.
        let oval = crate::track::test_oval();
        let center = CenterLine::of(&oval);
        let between = walls_between_legs(&center);
        let turns = crate::track::decor::tight_turns(&center);
        assert!(between.iter().any(|[_, right]| *right));
        for (s, [left, right]) in center.samples().iter().zip(&between) {
            assert!(!left, "the outer wall at {}", s.distance);
            if *right {
                assert!(
                    turns.iter().any(|t| t.at.distance(s.position.xz()) < 30.0),
                    "glass along a straight at {}",
                    s.distance
                );
            }
        }

        let mut shares = Vec::new();
        for entry in crate::admin::tracks::circuits() {
            let text = std::fs::read_to_string(format!(
                "{}/assets/tracks/{}",
                env!("CARGO_MANIFEST_DIR"),
                entry.file
            ))
            .unwrap();
            let file = TrackData::from_ron(&text).unwrap();
            for data in [file.clone(), file.mirrored()] {
                let center = CenterLine::of(&data);
                let meshes = generate(&center, &data, &data.palette());
                let name = format!(
                    "{}{}",
                    data.name,
                    if data == file { "" } else { " (mirror)" }
                );
                let drawn: HashSet<[i64; 3]> = meshes
                    .wall_looks
                    .positions
                    .iter()
                    .chain(&meshes.glass.positions)
                    .map(|p| key(*p))
                    .collect();
                let undrawn = meshes
                    .walls
                    .positions
                    .iter()
                    .filter(|p| !drawn.contains(&key(**p)))
                    .count();
                assert_eq!(undrawn, 0, "{name}: vertices of the walls not drawn");
                // The glass between `GLASS_FROM` and the top, never down to the road; the
                // opaque walls there no higher than `GLASS_FROM` (they reach the top elsewhere).
                // (Each vertex is a point across the road at a sample, or at the end of a run
                // (a gap, the step of a drop): within a step of a sample along the road.)
                for p in &meshes.glass.positions {
                    let p = Vec3::from_array(*p);
                    let on = center.samples().iter().any(|s| {
                        let d = p - s.position;
                        let lateral = d.dot(s.right).abs();
                        let height = d.dot(s.up);
                        d.dot(s.forward).abs() <= 1.0
                            && (wall_offset(s) - 0.05..=wall_offset(s) + WALL_THICKNESS + 0.05)
                                .contains(&lateral)
                            && (GLASS_FROM - 0.05
                                ..=WALL_HEIGHT + wall_lift(&center, s.distance) + 0.05)
                                .contains(&height)
                    });
                    assert!(on, "{name}: glass at {p}, not on a wall's glass top");
                }
                // A share of the walls, glass where (and only where) some stand between legs.
                let between = walls_between_legs(&center);
                let road: Vec<usize> = (0..between.len())
                    .filter(|&i| center.on_the_road(center.samples()[i].distance))
                    .collect();
                let sides = 2 * road.len();
                let glassed = road.iter().flat_map(|&i| between[i]).filter(|b| *b).count();
                let share = glassed as f32 / sides as f32;
                assert!(
                    share < 0.75,
                    "{name}: {:.0} % of the walls in glass",
                    100.0 * share
                );
                assert_eq!(
                    meshes.glass.indices.is_empty(),
                    glassed == 0,
                    "{name}: glass and walls between two legs"
                );
                shares.push(format!("{name}: {:.0} %", 100.0 * share));
                // Its mirror: the same walls, side for side.
                if data != file {
                    let original = walls_between_legs(&CenterLine::of(&file));
                    let mirrored = between.iter().map(|[l, r]| [*r, *l]).collect::<Vec<_>>();
                    let differ = original
                        .iter()
                        .zip(&mirrored)
                        .filter(|(a, b)| a != b)
                        .count();
                    assert!(
                        differ * 100 <= original.len(),
                        "{name}: {differ} samples whose walls differ from the file's"
                    );
                }
            }
        }
        println!("walls between two legs: {}", shares.join(", "));
        // The circuits with lacets have some.
        assert!(
            shares.iter().filter(|s| !s.ends_with(" 0 %")).count() >= 16,
            "{shares:?}"
        );
    }
}
