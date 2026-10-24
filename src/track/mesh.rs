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
use super::shortcut::{OpenEdge, Shortcut, walled};
use super::water::{Gap, Lake, ground_height};

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
    /// Skirts under raised parts: visual only. Issue #78: the blue stripes of the anti-gravity
    /// zones too.
    pub decor: MeshData,
    /// Issue #78: the asphalt and curbs of the anti-gravity zones (`Surface::Road`, and
    /// `AntiGravity`), apart from [`road`](Self::road): a wheel on it puts the kart in
    /// anti-gravity. Empty on a circuit without a zone.
    pub anti_gravity_road: MeshData,
    /// The same for their shoulders (`Surface::Offroad`, and `AntiGravity`).
    pub anti_gravity_offroad: MeshData,
    /// Issue #107: the roads of the shortcuts (`Surface::Road`) and their grass paths
    /// (`Surface::Offroad`). Empty on a circuit without one.
    pub shortcut_road: MeshData,
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
}

impl TrackMeshes {
    /// Everything in one mesh, for a single draw call (the anti-gravity zones' road last: without
    /// a zone, exactly the mesh before issue #78).
    pub fn combined(&self) -> MeshData {
        let mut all = self.road.clone();
        all.append(&self.offroad);
        all.append(&self.walls);
        all.append(&self.decor);
        all.append(&self.anti_gravity_road);
        all.append(&self.anti_gravity_offroad);
        all.append(&self.shortcut_road);
        all.append(&self.shortcut_grass);
        all.append(&self.trick_road);
        all.append(&self.trick_offroad);
        all.append(&self.cliffs);
        all
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
    // Issue #94: whether the road between two samples faces up (banked to 90° at most).
    let faces_up = |s0: &TrackSample, s1: &TrackSample| {
        s0.up.y >= -FACING_UP_TOLERANCE && s1.up.y >= -FACING_UP_TOLERANCE
    };
    let mut meshes = TrackMeshes::default();
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
        // Road and curbs; in the anti-gravity zones (issue #78), in their own mesh, the curbs
        // blue and white.
        for (road, keep, stripe) in [
            (
                &mut meshes.road,
                &unzoned as &dyn Fn(&TrackSample, &TrackSample) -> bool,
                &stripe as &dyn Fn(usize) -> [f32; 4],
            ),
            (&mut meshes.anti_gravity_road, &zoned, &blue_stripe),
            (&mut meshes.trick_road, &tricky, &stripe),
        ] {
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
        // stretches without wall).
        wall_strips(
            &mut meshes.walls,
            samples,
            closed,
            palette,
            &walled_side,
            &|s: &TrackSample| WALL_HEIGHT + wall_lift(center, s.distance),
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
            faces_up,
        );
        decor.strip_where(
            samples,
            closed,
            |s| ground(s.point(-outer(s), 0.0)),
            |s| s.point(-outer(s), 0.0),
            |_| skirt,
            faces_up,
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

    // Issue #107: the shortcuts' roads and grass paths, their skirts down to the ground.
    for shortcut in &data.shortcuts {
        let path = Shortcut::new(center, shortcut);
        let samples = path.samples();
        let half = path.half_width;
        let lift = |s: &TrackSample, lateral: f32| s.point(lateral, SHORTCUT_LIFT);
        match shortcut.kind {
            ShortcutKind::Road => {
                let road = &mut meshes.shortcut_road;
                let inner = half - CURB_WIDTH;
                road.strip(
                    samples,
                    false,
                    |s| lift(s, -inner),
                    |s| lift(s, inner),
                    |_| asphalt,
                );
                road.strip(
                    samples,
                    false,
                    |s| lift(s, -half),
                    |s| lift(s, -inner),
                    stripe,
                );
                road.strip(
                    samples,
                    false,
                    |s| lift(s, inner),
                    |s| lift(s, half),
                    stripe,
                );
            }
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

    meshes
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

/// The walls of both sides, [`WALL_HEIGHT`] high: inner face, flat top, outer face. The faces
/// alternate the palette's two wall colors in panels (the barriers' look, issue #11). Issue #107:
/// a side's wall only between two samples where `walled` says it stands (`walled(sign, s0, s1)`,
/// sign -1 for the left, 1 for the right); everywhere on a circuit without a stretch without wall.
/// Issue #180: `tall(s)`, their height at each sample ([`WALL_HEIGHT`], higher along a bump's back
/// and below a drop: [`wall_lift`]).
fn wall_strips(
    mesh: &mut MeshData,
    samples: &[TrackSample],
    closed: bool,
    palette: &Palette,
    walled: &dyn Fn(f32, &TrackSample, &TrackSample) -> bool,
    tall: &dyn Fn(&TrackSample) -> f32,
) {
    let outer = |s: &TrackSample| wall_offset(s) + WALL_THICKNESS;
    let panels = [rgba(palette.walls.0), rgba(palette.walls.1)];
    let panel = |i: usize| panels[(i / WALL_PANEL) % 2];
    let top = rgba(palette.wall_top);
    let right = |s0: &TrackSample, s1: &TrackSample| walled(1.0, s0, s1);
    let left = |s0: &TrackSample, s1: &TrackSample| walled(-1.0, s0, s1);
    mesh.strip_where(
        samples,
        closed,
        |s| s.point(wall_offset(s), 0.0),
        |s| s.point(wall_offset(s), tall(s)),
        panel,
        right,
    );
    mesh.strip_where(
        samples,
        closed,
        |s| s.point(wall_offset(s), tall(s)),
        |s| s.point(outer(s), tall(s)),
        |_| top,
        right,
    );
    mesh.strip_where(
        samples,
        closed,
        |s| s.point(outer(s), tall(s)),
        |s| s.point(outer(s), 0.0),
        panel,
        right,
    );
    mesh.strip_where(
        samples,
        closed,
        |s| s.point(-wall_offset(s), tall(s)),
        |s| s.point(-wall_offset(s), 0.0),
        panel,
        left,
    );
    mesh.strip_where(
        samples,
        closed,
        |s| s.point(-outer(s), tall(s)),
        |s| s.point(-wall_offset(s), tall(s)),
        |_| top,
        left,
    );
    mesh.strip_where(
        samples,
        closed,
        |s| s.point(-outer(s), 0.0),
        |s| s.point(-outer(s), tall(s)),
        panel,
        left,
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
            TrackData::from_ron(include_str!("../../assets/tracks/anneau.track.ron")).unwrap(),
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
            // What is drawn is all of it: the walls are in the circuit's single mesh.
            let all = meshes.combined();
            let start = meshes.road.positions.len() + meshes.offroad.positions.len();
            assert_eq!(
                all.positions[start..start + walls.positions.len()],
                walls.positions[..]
            );
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
                meshes.shortcut_road.positions.len(),
                3 * 4 * quads(ShortcutKind::Road)
            );
            assert_eq!(
                meshes.shortcut_grass.positions.len(),
                4 * quads(ShortcutKind::Grass)
            );
            assert!(quads(ShortcutKind::Road) > 50 && quads(ShortcutKind::Grass) > 30);
            for (mesh, kind) in [
                (&meshes.shortcut_road, ShortcutKind::Road),
                (&meshes.shortcut_grass, ShortcutKind::Grass),
            ] {
                let path = shortcuts.iter().find(|s| s.kind == kind).unwrap();
                for p in &mesh.positions {
                    let here = path.locate(Vec3::from_array(*p));
                    assert!((here.height - SHORTCUT_LIFT).abs() < 0.01, "{here:?}");
                    assert!(here.lateral.abs() <= path.half_width + 0.01, "{here:?}");
                }
            }
        }
        let prairie = test_track();
        let meshes = generate(&CenterLine::of(&prairie), &prairie, &Palette::default());
        assert!(
            meshes.shortcut_road.indices.is_empty() && meshes.shortcut_grass.indices.is_empty()
        );
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
}
