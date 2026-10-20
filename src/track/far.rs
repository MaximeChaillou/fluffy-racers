//! Issue #146: the **far decor** of a circuit, so that one never sees the edge of the world nor an
//! empty horizon (Max: « N'oublie pas la décoration, même au loin pour permettre une meilleure
//! immersion »). Written per biome in `assets/data/biomes.ron` (`far`), with more of it per
//! circuit if its file asks (`far_decor` in `assets/tracks/*.track.ron`).
//!
//! - **The ground beyond** the circuit's (its slab reaches [`GROUND_MARGIN`] beyond the
//!   centerline's bounds): the biome's ground out to [`FAR_REACH`], or a beach and the sea
//!   ([`FarLayer::Sea`]).
//! - **Silhouettes**, procedural low-poly shapes: a closed **range** of mountains, hills, cliffs
//!   or mesas all around ([`FarLayer::Range`]: what closes the horizon), lone **peaks** (needles,
//!   mesas, islands: [`FarLayer::Peaks`]), **clouds** ([`FarLayer::Clouds`]).
//! - **Pieces at mid-distance**: the biome's models, big (trees, rocks, monuments, animals:
//!   [`FarLayer::Pieces`]), beyond the near decor.
//! - **A distance fog** (`fog`): from its start, everything fades into the sky's color, all of it
//!   at its end. The far silhouettes look far; nothing shows where the world stops.
//!
//! **Pure and deterministic** (the circuit's `decor_seed`, one generator per layer): the plan
//! ([`FarPlan`]) is triangles and pieces; `dressing.rs` draws it **in one mesh** with one material,
//! no shadows, no collider (it is out of the karts' reach), only with a renderer. Headless, nothing
//! of it exists: the bots' races do not change.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;
use serde::Deserialize;

use super::biome::{BiomeData, Palette, Rgb, Side, is_model_id};
use super::decor::{self, DecorPiece, DecorPlan};
use super::layout::CenterLine;
use super::mesh::{GROUND_HEIGHT, MeshData};
use super::shortcut::Shortcut;
use super::water::Lake;
use crate::bots::BotRng;

/// How far the circuit's ground slab reaches beyond its centerline's bounds (m).
pub const GROUND_MARGIN: f32 = 80.0;

/// How far the ground beyond reaches past the slab (m): further than any fog's end, so that from
/// anywhere on the circuit the ground fades into the sky before it stops.
pub const FAR_REACH: f32 = 2000.0;

/// How far a far layer may stand beyond the slab (m).
pub const MAX_DISTANCE: f32 = 1200.0;

/// Attempts at finding room for a piece before leaving it out.
const ATTEMPTS: usize = 12;

/// Mixed into the decor's seed: the far decor's draws are not the near decor's.
const SEED_SALT: u64 = 0x0146_FA12_DEC0_0146;

/// Segments of a quarter circle at a corner of the far ground's rings.
const CORNER_SEGMENTS: usize = 8;

/// Length of a segment along a side of the far ground's rings (m), at most.
const SIDE_SEGMENT: f32 = 60.0;

/// The far decor of a biome, or what a circuit adds to it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FarDecor {
    /// The distance fog (m from the camera): from `fog.0` everything fades into the sky's
    /// color, entirely at `fog.1`.
    pub fog: (f32, f32),
    pub layers: Vec<FarLayer>,
}

/// A layer of the far decor. Its `distance` is measured beyond the circuit's ground slab (m),
/// but a piece's ([`FarLayer::Pieces`]), measured beyond the walls like the near decor's.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum FarLayer {
    /// The ground beyond turns into the sea `from` meters beyond the slab, after a beach of
    /// `beach` meters (the palette's sand). One per circuit at most (a circuit's own replaces
    /// its biome's).
    Sea {
        color: Rgb,
        from: f32,
        #[serde(default)]
        beach: f32,
    },
    /// A closed ring of mountains (hills, cliffs) all around the circuit: its ridge `distance`
    /// meters beyond the slab, `depth` meters thick at its foot, a peak every `spacing` meters
    /// or so, each `height` high (between the two: never lower than the first). `top`: the color
    /// above a height (snow, a band of lighter rock).
    Range {
        distance: f32,
        depth: f32,
        height: (f32, f32),
        spacing: f32,
        color: Rgb,
        #[serde(default)]
        top: Option<(Rgb, f32)>,
    },
    /// Lone peaks, their foot between `distance.0` and `distance.1` meters beyond the slab: `sides`-sided
    /// (6 by default), `radius` wide at their foot, `height` high, their top `flat` times as
    /// wide as their foot (0: a point; a mesa: 0.6). `top` as for a range.
    Peaks {
        count: u32,
        distance: (f32, f32),
        height: (f32, f32),
        radius: (f32, f32),
        #[serde(default)]
        flat: f32,
        #[serde(default = "six")]
        sides: u32,
        color: Rgb,
        #[serde(default)]
        top: Option<(Rgb, f32)>,
    },
    /// Clouds, `altitude` meters up, `size` meters long, between `distance.0` and `distance.1`
    /// meters beyond the slab.
    Clouds {
        count: u32,
        distance: (f32, f32),
        altitude: (f32, f32),
        size: (f32, f32),
        color: Rgb,
    },
    /// Models at mid-distance (trees, rocks, monuments, animals), big: like the biome's scatter
    /// (`models` drawn in turn at random, `height`, `radius` their footprint, `side`), but
    /// between `distance.0` and `distance.1` meters beyond the walls, never on the sea, and
    /// without shadows.
    Pieces {
        models: Vec<String>,
        count: u32,
        height: (f32, f32),
        radius: f32,
        distance: (f32, f32),
        #[serde(default)]
        side: Side,
    },
}

fn six() -> u32 {
    6
}

impl FarDecor {
    /// Checks the values that would make the far decor fail, look broken or reach past the
    /// ground beyond.
    pub fn validate(&self) -> Result<(), String> {
        let (start, end) = self.fog;
        if !(50.0..FAR_REACH).contains(&start) || end <= start || end > FAR_REACH {
            return Err(format!(
                "fog {:?}: from 50 m, its end after its start, within {FAR_REACH} m",
                self.fog
            ));
        }
        self.layers.iter().try_for_each(FarLayer::validate)
    }

    /// Every model its pieces may use, each once, in the order of the file.
    pub fn models(&self) -> Vec<&str> {
        let mut models: Vec<&str> = Vec::new();
        for layer in &self.layers {
            if let FarLayer::Pieces { models: ids, .. } = layer {
                for id in ids {
                    if !models.contains(&id.as_str()) {
                        models.push(id);
                    }
                }
            }
        }
        models
    }

    /// Whether a layer closes the horizon all around (a range).
    pub fn has_range(&self) -> bool {
        self.layers
            .iter()
            .any(|layer| matches!(layer, FarLayer::Range { .. }))
    }
}

/// A pair within `limits`, the first not above the second.
fn pair_within(what: &str, pair: (f32, f32), limits: (f32, f32)) -> Result<(), String> {
    let (low, high) = pair;
    if low < limits.0 || high < low || high > limits.1 || !low.is_finite() {
        return Err(format!(
            "{what} {pair:?} not within {}..={}",
            limits.0, limits.1
        ));
    }
    Ok(())
}

fn color_within(color: Rgb) -> Result<(), String> {
    let (r, g, b) = color;
    if [r, g, b].iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err(format!("color {color:?} out of 0..=1"));
    }
    Ok(())
}

impl FarLayer {
    /// Checks one layer.
    pub fn validate(&self) -> Result<(), String> {
        let top = |top: &Option<(Rgb, f32)>| match top {
            Some((color, at)) if *at >= 0.0 => color_within(*color),
            Some((_, at)) => Err(format!("top at {at} m below the ground")),
            None => Ok(()),
        };
        let fail = |e: String| Err(format!("far decor: {e}"));
        let checked = match self {
            FarLayer::Sea { color, from, beach } => color_within(*color).and_then(|_| {
                pair_within("sea (beach, from)", (*beach, *from), (0.0, MAX_DISTANCE))
            }),
            FarLayer::Range {
                distance,
                depth,
                height,
                spacing,
                color,
                top: t,
            } => pair_within(
                "range distance",
                (*distance, *distance),
                (20.0, MAX_DISTANCE),
            )
            .and_then(|_| pair_within("range depth", (*depth, *depth), (10.0, 600.0)))
            .and_then(|_| {
                (*depth <= 2.0 * distance)
                    .then_some(())
                    .ok_or(format!("range: its foot ({depth} m) reaches the slab"))
            })
            .and_then(|_| pair_within("range height", *height, (5.0, 600.0)))
            .and_then(|_| pair_within("range spacing", (*spacing, *spacing), (5.0, 200.0)))
            .and_then(|_| color_within(*color))
            .and_then(|_| top(t)),
            FarLayer::Peaks {
                count,
                distance,
                height,
                radius,
                flat,
                sides,
                color,
                top: t,
            } => pair_within("peaks distance", *distance, (0.0, MAX_DISTANCE))
                .and_then(|_| pair_within("peaks height", *height, (1.0, 800.0)))
                .and_then(|_| pair_within("peaks radius", *radius, (1.0, 400.0)))
                .and_then(|_| pair_within("peaks flat", (*flat, *flat), (0.0, 0.95)))
                .and_then(|_| {
                    (*count <= 500 && (3..=12).contains(sides))
                        .then_some(())
                        .ok_or(format!(
                            "{count} peaks (500 at most), {sides} sides (3 to 12)"
                        ))
                })
                .and_then(|_| color_within(*color))
                .and_then(|_| top(t)),
            FarLayer::Clouds {
                count,
                distance,
                altitude,
                size,
                color,
            } => pair_within("clouds distance", *distance, (0.0, MAX_DISTANCE))
                .and_then(|_| pair_within("clouds altitude", *altitude, (10.0, 1000.0)))
                .and_then(|_| pair_within("clouds size", *size, (1.0, 500.0)))
                .and_then(|_| {
                    (*count <= 300)
                        .then_some(())
                        .ok_or(format!("{count} clouds (300 at most)"))
                })
                .and_then(|_| color_within(*color)),
            FarLayer::Pieces {
                models,
                count,
                height,
                radius,
                distance,
                side: _,
            } => {
                if models.is_empty() {
                    Err("pieces without a model".to_string())
                } else if let Some(bad) = models.iter().find(|id| !is_model_id(id)) {
                    Err(format!("{bad:?} is not a model (\"pack/name\")"))
                } else if *count > 2000 || !(0.1..=60.0).contains(radius) {
                    Err(format!(
                        "{models:?}: count {count} above 2000, or radius {radius} out of 0.1..=60"
                    ))
                } else {
                    pair_within("pieces height", *height, (0.1, 150.0))
                        .and_then(|_| pair_within("pieces distance", *distance, (0.0, 800.0)))
                }
            }
        };
        checked.or_else(fail)
    }
}

/// The circuit's ground slab on the ground plane: the centerline's bounds, [`GROUND_MARGIN`]
/// further on every side. Computed exactly as `circuit::spawn_ground` places its boxes (the
/// same floats): the ground beyond meets it edge to edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundRect {
    pub min: Vec2,
    pub max: Vec2,
}

impl GroundRect {
    /// The slab of a centerline (its samples).
    pub fn of(center: &CenterLine) -> Self {
        let (center, half) = slab(center);
        Self {
            min: center - half,
            max: center + half,
        }
    }

    /// How far `p` is outside the slab (0 inside).
    pub fn outside(&self, p: Vec2) -> f32 {
        (self.min - p).max(p - self.max).max(Vec2::ZERO).length()
    }

    /// The point `d` meters beyond the slab (a rounded rectangle) at `s` meters along it (from
    /// the middle of its side of lowest z, toward +x), and the way out there (unit).
    pub fn ring_at(&self, d: f32, s: f32) -> (Vec2, Vec2) {
        let (lo, hi) = (self.min, self.max);
        let middle = (lo.x + hi.x) / 2.0;
        let size = hi - lo;
        // The way around: a straight stretch (from, to, the way out) or a corner (its point,
        // the angle of the way out at its start), in turn.
        enum Piece {
            Line(Vec2, Vec2, Vec2),
            Corner(Vec2, f32),
        }
        let pieces = [
            Piece::Line(Vec2::new(middle, lo.y), Vec2::new(hi.x, lo.y), Vec2::NEG_Y),
            Piece::Corner(Vec2::new(hi.x, lo.y), -FRAC_PI_2),
            Piece::Line(Vec2::new(hi.x, lo.y), hi, Vec2::X),
            Piece::Corner(hi, 0.0),
            Piece::Line(hi, Vec2::new(lo.x, hi.y), Vec2::Y),
            Piece::Corner(Vec2::new(lo.x, hi.y), FRAC_PI_2),
            Piece::Line(Vec2::new(lo.x, hi.y), lo, Vec2::NEG_X),
            Piece::Corner(lo, PI),
            Piece::Line(lo, Vec2::new(middle, lo.y), Vec2::NEG_Y),
        ];
        let perimeter = 2.0 * (size.x + size.y) + TAU * d;
        let mut s = s.rem_euclid(perimeter);
        for piece in &pieces {
            match *piece {
                Piece::Line(from, to, out) => {
                    let length = from.distance(to);
                    if s <= length {
                        let t = if length > 0.0 { s / length } else { 0.0 };
                        return (from.lerp(to, t) + out * d, out);
                    }
                    s -= length;
                }
                Piece::Corner(at, start) => {
                    let length = FRAC_PI_2 * d;
                    if s <= length {
                        let angle = start + if d > 0.0 { s / d } else { 0.0 };
                        let out = Vec2::new(angle.cos(), angle.sin());
                        return (at + out * d, out);
                    }
                    s -= length;
                }
            }
        }
        // Rounding past the end: the start.
        (Vec2::new(middle, lo.y - d), Vec2::NEG_Y)
    }

    /// The length of the ring `d` meters beyond the slab.
    pub fn perimeter(&self, d: f32) -> f32 {
        let size = self.max - self.min;
        2.0 * (size.x + size.y) + TAU * d
    }

    /// The closed ring `d` meters beyond the slab, as points: [`CORNER_SEGMENTS`] segments at
    /// each corner (all at the corner itself for `d` = 0), each side cut in segments of
    /// [`SIDE_SEGMENT`] meters at most. The same count and order for every `d`.
    fn ring(&self, d: f32) -> Vec<(Vec2, Vec2)> {
        let size = self.max - self.min;
        let corners = [
            Vec2::new(self.max.x, self.min.y),
            self.max,
            Vec2::new(self.min.x, self.max.y),
            self.min,
        ];
        let starts = [-FRAC_PI_2, 0.0, FRAC_PI_2, PI];
        let sides = [size.y, size.x, size.y, size.x];
        let mut points = Vec::new();
        for i in 0..4 {
            for k in 0..=CORNER_SEGMENTS {
                let angle = starts[i] + FRAC_PI_2 * k as f32 / CORNER_SEGMENTS as f32;
                let out = Vec2::new(angle.cos(), angle.sin());
                points.push((corners[i] + out * d, out));
            }
            // Along the side that follows the corner, its inner points.
            let segments = side_segments(sides[i]);
            let next = corners[(i + 1) % 4];
            let end_angle = starts[i] + FRAC_PI_2;
            let out = Vec2::new(end_angle.cos(), end_angle.sin());
            for k in 1..segments {
                let t = k as f32 / segments as f32;
                points.push((corners[i].lerp(next, t) + out * d, out));
            }
        }
        points
    }

    /// The mirror image (x → -x).
    pub fn mirrored(self) -> Self {
        Self {
            min: Vec2::new(-self.max.x, self.min.y),
            max: Vec2::new(-self.min.x, self.max.y),
        }
    }
}

/// The slab's center and half size, as `circuit::spawn_ground` computes them.
pub fn slab(center: &CenterLine) -> (Vec2, Vec2) {
    let (min, max) = center.samples().iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), s| {
            let p = Vec2::new(s.position.x, s.position.z);
            (lo.min(p), hi.max(p))
        },
    );
    let half = (max - min) / 2.0 + Vec2::splat(GROUND_MARGIN);
    ((min + max) / 2.0, half)
}

fn side_segments(length: f32) -> usize {
    ((length / SIDE_SEGMENT).ceil() as usize).max(1)
}

/// What a triangle of the far decor is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarPart {
    /// The ground beyond, the beach, the sea.
    Ground,
    /// A range, a peak: what hides the horizon.
    Silhouette,
    Cloud,
}

/// A triangle of the far decor, counterclockwise seen from its front (its outside), in sRGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FarTriangle {
    pub corners: [Vec3; 3],
    pub color: [f32; 4],
    pub part: FarPart,
}

impl FarTriangle {
    /// A triangle turned to face `outward` (its corners in the order that puts its front there).
    fn facing(corners: [Vec3; 3], outward: Vec3, color: Rgb, part: FarPart) -> Self {
        let [a, b, c] = corners;
        let normal = (b - a).cross(c - a);
        let corners = if normal.dot(outward) < 0.0 {
            [a, c, b]
        } else {
            corners
        };
        Self {
            corners,
            color: [color.0, color.1, color.2, 1.0],
            part,
        }
    }

    /// The area (m²): 0 for a degenerate one.
    pub fn area(&self) -> f32 {
        let [a, b, c] = self.corners;
        (b - a).cross(c - a).length() / 2.0
    }

    /// Where a horizontal ray from `origin` toward `direction` (ground plane, unit) meets it, in
    /// meters along the ray.
    pub fn horizontal_hit(&self, origin: Vec3, direction: Vec2) -> Option<f32> {
        let y = origin.y;
        let [a, b, c] = self.corners;
        // The triangle cut by the plane at the ray's height: a segment.
        let mut ends = [Vec2::ZERO; 2];
        let mut found = 0;
        for (p, q) in [(a, b), (b, c), (c, a)] {
            if (p.y - y) * (q.y - y) <= 0.0 && (p.y - q.y).abs() > 1e-6 && found < 2 {
                let t = (y - p.y) / (q.y - p.y);
                ends[found] = p.xz().lerp(q.xz(), t);
                found += 1;
            }
        }
        if found < 2 {
            return None;
        }
        // The ray against the segment.
        let o = origin.xz();
        let e = ends[1] - ends[0];
        let denominator = direction.perp_dot(e);
        if denominator.abs() < 1e-9 {
            return None;
        }
        let to = ends[0] - o;
        let along = to.perp_dot(e) / denominator;
        let across = to.perp_dot(direction) / denominator;
        (along >= 0.0 && (0.0..=1.0).contains(&across)).then_some(along)
    }
}

/// The far decor of a circuit, placed: its triangles, its pieces and its fog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FarPlan {
    pub fog: (f32, f32),
    pub triangles: Vec<FarTriangle>,
    pub models: Vec<String>,
    pub pieces: Vec<DecorPiece>,
    /// Pieces left out for lack of room.
    pub left_out: usize,
    /// The slab it starts at.
    pub ground: Option<GroundRect>,
}

impl FarPlan {
    /// The mirror image (issue #19): every triangle and piece across the X = 0 plane, the
    /// triangles still facing out.
    pub fn mirrored(mut self) -> Self {
        for triangle in &mut self.triangles {
            for corner in &mut triangle.corners {
                corner.x = -corner.x;
            }
            triangle.corners.swap(1, 2);
        }
        for piece in &mut self.pieces {
            piece.position.x = -piece.position.x;
            piece.yaw = -piece.yaw;
        }
        self.ground = self.ground.map(GroundRect::mirrored);
        self
    }

    /// Its triangles of a part.
    pub fn of(&self, part: FarPart) -> impl Iterator<Item = &FarTriangle> {
        self.triangles.iter().filter(move |t| t.part == part)
    }

    /// Its triangles in a mesh (the models' pieces are added by `dressing.rs`).
    /// The clouds are lit as if they faced mostly up: bright, their puffs still shaded a little.
    pub fn mesh(&self) -> MeshData {
        let mut mesh = MeshData::default();
        for triangle in &self.triangles {
            let [a, b, c] = triangle.corners;
            match triangle.part {
                FarPart::Cloud => {
                    let own = (b - a).cross(c - a).normalize_or(Vec3::Y);
                    let lit = (own + Vec3::Y * 2.0).normalize_or(Vec3::Y);
                    mesh.triangle_lit(triangle.corners, triangle.color, lit);
                }
                _ => mesh.triangle(triangle.corners, triangle.color),
            }
        }
        mesh
    }

    /// The first silhouette a horizontal ray from `origin` toward `direction` meets, in
    /// meters.
    pub fn horizon_hit(&self, origin: Vec3, direction: Vec2) -> Option<f32> {
        self.of(FarPart::Silhouette)
            .filter_map(|t| t.horizontal_hit(origin, direction))
            .min_by(f32::total_cmp)
    }
}

/// The sea of a far decor: the last one (a circuit's own replaces its biome's).
fn sea_of(far: &FarDecor) -> Option<(Rgb, f32, f32)> {
    far.layers.iter().rev().find_map(|layer| match layer {
        FarLayer::Sea { color, from, beach } => Some((*color, *from, *beach)),
        _ => None,
    })
}

/// Places the far decor of a circuit (its file's centerline, not mirrored: see
/// [`FarPlan::mirrored`]): the ground beyond, its layers in turn (each with its own generator),
/// the pieces where they touch neither the circuit, its shortcuts, its lakes, the near decor
/// (`near`), the peaks nor each other, and never on the sea.
pub fn place_far_decor(
    center: &CenterLine,
    palette: &Palette,
    far: &FarDecor,
    seed: u64,
    near: &DecorPlan,
    lakes: &[Lake],
    shortcuts: &[Shortcut],
) -> FarPlan {
    let rect = GroundRect::of(center);
    let mut plan = FarPlan {
        fog: far.fog,
        ground: Some(rect),
        ..default()
    };
    let sea = sea_of(far);
    far_ground(&mut plan.triangles, &rect, palette, sea, seed);
    // Footprints already taken beyond the slab (the peaks'), for the pieces.
    let mut taken: Vec<(Vec2, f32)> = Vec::new();
    for (index, layer) in far.layers.iter().enumerate() {
        let mut rng = BotRng::new(seed ^ SEED_SALT ^ (index as u64).wrapping_mul(0x9E37_79B9));
        match layer {
            FarLayer::Sea { .. } => {}
            FarLayer::Range {
                distance,
                depth,
                height,
                spacing,
                color,
                top,
            } => range(
                &mut plan.triangles,
                &rect,
                (*distance, *depth, *height, *spacing),
                (*color, *top),
                &mut rng,
            ),
            FarLayer::Peaks {
                count,
                distance,
                height,
                radius,
                flat,
                sides,
                color,
                top,
            } => {
                for _ in 0..*count {
                    // Its foot `distance` beyond the slab: its center further by its radius
                    // (and the most its corners may stray).
                    let r = rng.range(radius.0, radius.1);
                    let d = rng.range(distance.0, distance.1) + r * 1.15;
                    let s = rng.range(0.0, rect.perimeter(d));
                    let (at, _) = rect.ring_at(d, s);
                    let h = rng.range(height.0, height.1);
                    peak(
                        &mut plan.triangles,
                        (at, r, h, *flat, *sides as usize),
                        (*color, *top),
                        &mut rng,
                    );
                    taken.push((at, r));
                }
            }
            FarLayer::Clouds {
                count,
                distance,
                altitude,
                size,
                color,
            } => {
                for _ in 0..*count {
                    let d = rng.range(distance.0, distance.1);
                    let s = rng.range(0.0, rect.perimeter(d));
                    let (at, out) = rect.ring_at(d, s);
                    let a = rng.range(altitude.0, altitude.1);
                    let length = rng.range(size.0, size.1);
                    cloud(
                        &mut plan.triangles,
                        Vec3::new(at.x, a, at.y),
                        out,
                        length,
                        *color,
                        &mut rng,
                    );
                }
            }
            FarLayer::Pieces {
                models,
                count,
                height,
                radius,
                distance,
                side,
            } => {
                let sea_line = sea.map(|(_, from, beach)| from - beach);
                let samples = center.samples();
                let inside = decor::inside_sign(center);
                let options = decor::sides(*side, inside);
                for _ in 0..*count {
                    let mut placed = false;
                    for _ in 0..ATTEMPTS {
                        let sample = center.at(rng.range(0.0, center.length()));
                        let side = options[(rng.next_u64() % options.len() as u64) as usize];
                        let gap = rng.range(distance.0, distance.1);
                        let h = rng.range(height.0, height.1);
                        let p = decor::beside(&sample, side, gap, *radius);
                        let clear = decor::clear_of_the_circuit(samples, p, *radius)
                            && shortcuts.iter().all(|shortcut| {
                                decor::clear_of_the_circuit(shortcut.samples(), p, *radius)
                            })
                            && !lakes.iter().any(|lake| {
                                p.x + radius >= lake.min.x
                                    && p.x - radius <= lake.max.x
                                    && p.y + radius >= lake.min.y
                                    && p.y - radius <= lake.max.y
                            })
                            && !near.overlaps(p, *radius)
                            && !plan
                                .pieces
                                .iter()
                                .any(|o| o.position.xz().distance(p) < o.radius + radius)
                            && !taken.iter().any(|(at, r)| at.distance(p) < r + radius)
                            && sea_line.is_none_or(|line| rect.outside(p) + radius < line);
                        if !clear {
                            continue;
                        }
                        let pick = (rng.next_u64() % models.len() as u64) as usize;
                        let id = &models[pick];
                        let model = match plan.models.iter().position(|m| m == id) {
                            Some(i) => i,
                            None => {
                                plan.models.push(id.clone());
                                plan.models.len() - 1
                            }
                        } as u16;
                        plan.pieces.push(DecorPiece {
                            model,
                            position: Vec3::new(p.x, GROUND_HEIGHT, p.y),
                            yaw: rng.range(0.0, TAU),
                            height: h,
                            radius: *radius,
                            shadows: false,
                        });
                        placed = true;
                        break;
                    }
                    if !placed {
                        plan.left_out += 1;
                    }
                }
            }
        }
    }
    plan.triangles.retain(|t| t.area() > 1e-4);
    plan
}

/// The ground beyond the slab: rings around it out to [`FAR_REACH`], the biome's ground, or a
/// beach and the sea. The first band meets the slab's four edges whole (no vertex on them: no
/// crack between the two).
fn far_ground(
    out: &mut Vec<FarTriangle>,
    rect: &GroundRect,
    palette: &Palette,
    sea: Option<(Rgb, f32, f32)>,
    seed: u64,
) {
    let mut rng = BotRng::new(seed ^ SEED_SALT.rotate_left(17));
    // The rings beyond the slab, and the color of the band inside each.
    let mut rings: Vec<(f32, Rgb)> = Vec::new();
    match sea {
        Some((color, from, beach)) => {
            if from - beach > 1.0 {
                rings.push((from - beach, palette.ground));
            }
            if beach > 0.0 {
                rings.push((from, palette.sand));
            } else if from > 1.0 && rings.is_empty() {
                rings.push((from, palette.ground));
            }
            rings.push((FAR_REACH, color));
        }
        None => rings.push((FAR_REACH, palette.ground)),
    }
    let up = Vec3::Y;
    let at = |p: Vec2| Vec3::new(p.x, GROUND_HEIGHT, p.y);
    // Each ring's points, a shore line wavy (not the last ring).
    let last = rings.len() - 1;
    let points: Vec<Vec<Vec2>> = rings
        .iter()
        .enumerate()
        .map(|(i, (d, _))| {
            rect.ring(*d)
                .into_iter()
                .map(|(p, outward)| {
                    if i == last {
                        p
                    } else {
                        p + outward * rng.range(-6.0, 6.0)
                    }
                })
                .collect()
        })
        .collect();
    // The first band: from the slab's edges to the first ring, side by side.
    let corners = [
        Vec2::new(rect.max.x, rect.min.y),
        rect.max,
        Vec2::new(rect.min.x, rect.max.y),
        rect.min,
    ];
    let first = &points[0];
    let color = rings[0].1;
    let size = rect.max - rect.min;
    let sides = [size.y, size.x, size.y, size.x];
    let mut index = 0;
    for i in 0..4 {
        let corner = corners[i];
        let next = corners[(i + 1) % 4];
        // Around the corner: a fan from it.
        for k in 0..CORNER_SEGMENTS {
            out.push(FarTriangle::facing(
                [at(corner), at(first[index + k]), at(first[index + k + 1])],
                up,
                color,
                FarPart::Ground,
            ));
        }
        index += CORNER_SEGMENTS;
        // Along the side: the ring's points from the corner's last to the next corner's first,
        // half of them fanned from each corner, the side's edge in one triangle.
        let segments = side_segments(sides[i]);
        let along: Vec<Vec2> = (0..=segments)
            .map(|k| first[(index + k) % first.len()])
            .collect();
        let middle = segments / 2;
        for k in 0..middle {
            out.push(FarTriangle::facing(
                [at(corner), at(along[k]), at(along[k + 1])],
                up,
                color,
                FarPart::Ground,
            ));
        }
        out.push(FarTriangle::facing(
            [at(corner), at(along[middle]), at(next)],
            up,
            color,
            FarPart::Ground,
        ));
        for k in middle..segments {
            out.push(FarTriangle::facing(
                [at(next), at(along[k]), at(along[k + 1])],
                up,
                color,
                FarPart::Ground,
            ));
        }
        index += segments;
    }
    // The other bands: strips between two rings.
    for band in 1..rings.len() {
        let (inner, outer) = (&points[band - 1], &points[band]);
        let color = rings[band].1;
        let n = inner.len();
        for k in 0..n {
            let (a, b) = (inner[k], inner[(k + 1) % n]);
            let (c, d) = (outer[(k + 1) % n], outer[k]);
            out.push(FarTriangle::facing(
                [at(a), at(b), at(c)],
                up,
                color,
                FarPart::Ground,
            ));
            out.push(FarTriangle::facing(
                [at(a), at(c), at(d)],
                up,
                color,
                FarPart::Ground,
            ));
        }
    }
}

/// A triangle, split by the plane at height `top.1` when it crosses it: the part above in the top
/// color.
fn split(
    out: &mut Vec<FarTriangle>,
    corners: [Vec3; 3],
    outward: Vec3,
    color: Rgb,
    top: Option<(Rgb, f32)>,
) {
    let part = FarPart::Silhouette;
    let Some((top_color, at)) = top else {
        out.push(FarTriangle::facing(corners, outward, color, part));
        return;
    };
    let above: Vec<Vec3> = corners.iter().copied().filter(|c| c.y > at).collect();
    let below: Vec<Vec3> = corners.iter().copied().filter(|c| c.y <= at).collect();
    let cut = |p: Vec3, q: Vec3| p.lerp(q, (at - p.y) / (q.y - p.y));
    match (above.len(), below.len()) {
        (0, _) => out.push(FarTriangle::facing(corners, outward, color, part)),
        (_, 0) => out.push(FarTriangle::facing(corners, outward, top_color, part)),
        (1, 2) => {
            let (a, b0, b1) = (above[0], below[0], below[1]);
            let (p0, p1) = (cut(b0, a), cut(b1, a));
            out.push(FarTriangle::facing([a, p0, p1], outward, top_color, part));
            out.push(FarTriangle::facing([p0, b0, b1], outward, color, part));
            out.push(FarTriangle::facing([p0, b1, p1], outward, color, part));
        }
        _ => {
            let (b, a0, a1) = (below[0], above[0], above[1]);
            let (p0, p1) = (cut(b, a0), cut(b, a1));
            out.push(FarTriangle::facing([b, p0, p1], outward, color, part));
            out.push(FarTriangle::facing([p0, a0, a1], outward, top_color, part));
            out.push(FarTriangle::facing([p0, a1, p1], outward, top_color, part));
        }
    }
}

/// A closed range: a ridge all around, `distance` beyond the slab, its foot `depth` wide, its
/// peaks `height` high (never below the first: a mix of each one's draw and its neighbors').
fn range(
    out: &mut Vec<FarTriangle>,
    rect: &GroundRect,
    (distance, depth, height, spacing): (f32, f32, (f32, f32), f32),
    (color, top): (Rgb, Option<(Rgb, f32)>),
    rng: &mut BotRng,
) {
    let perimeter = rect.perimeter(distance);
    let n = ((perimeter / spacing).ceil() as usize).max(8);
    let draws: Vec<f32> = (0..n).map(|_| rng.next_f32()).collect();
    let foot = GROUND_HEIGHT - 1.0;
    let points: Vec<(Vec3, Vec3, Vec3)> = (0..n)
        .map(|i| {
            let s = (i as f32 + rng.range(-0.3, 0.3)) * perimeter / n as f32;
            let (at, outward) = rect.ring_at(distance, s);
            let mix = 0.6 * draws[i] + 0.2 * (draws[(i + n - 1) % n] + draws[(i + 1) % n]);
            let h = height.0 + (height.1 - height.0) * mix;
            let ridge = at + outward * depth * rng.range(-0.15, 0.15);
            let inner = at - outward * depth / 2.0;
            let outer = at + outward * depth / 2.0;
            (
                Vec3::new(inner.x, foot, inner.y),
                Vec3::new(ridge.x, GROUND_HEIGHT + h, ridge.y),
                Vec3::new(outer.x, foot, outer.y),
            )
        })
        .collect();
    for i in 0..n {
        let (inner0, ridge0, outer0) = points[i];
        let (inner1, ridge1, outer1) = points[(i + 1) % n];
        let mid = (ridge0 + ridge1) / 2.0;
        let toward = |p: Vec3| Vec3::new(p.x - mid.x, 0.0, p.z - mid.z);
        // Its face toward the circuit, then its back.
        let front = toward((inner0 + inner1) / 2.0);
        split(out, [inner0, inner1, ridge1], front, color, top);
        split(out, [inner0, ridge1, ridge0], front, color, top);
        let back = toward((outer0 + outer1) / 2.0);
        split(out, [outer0, ridge1, outer1], back, color, top);
        split(out, [outer0, ridge0, ridge1], back, color, top);
    }
}

/// A lone peak at `at`: a pyramid of `sides` sides, a little irregular, `radius` at its foot,
/// `height` high, its top `flat` times as wide (a point at 0).
fn peak(
    out: &mut Vec<FarTriangle>,
    (at, radius, height, flat, sides): (Vec2, f32, f32, f32, usize),
    (color, top): (Rgb, Option<(Rgb, f32)>),
    rng: &mut BotRng,
) {
    let turn = rng.range(0.0, TAU);
    let foot = GROUND_HEIGHT - 1.0;
    let summit = GROUND_HEIGHT + height;
    let lean = Vec2::new(rng.range(-0.08, 0.08), rng.range(-0.08, 0.08)) * radius;
    let ring: Vec<(Vec3, Vec3)> = (0..sides)
        .map(|k| {
            let angle = turn + TAU * k as f32 / sides as f32;
            let dir = Vec2::new(angle.cos(), angle.sin());
            let r = radius * rng.range(0.8, 1.15);
            let base = at + dir * r;
            let high = at + lean + dir * r * flat;
            (
                Vec3::new(base.x, foot, base.y),
                Vec3::new(high.x, summit * rng.range(0.94, 1.0), high.y),
            )
        })
        .collect();
    let axis = Vec3::new(at.x, 0.0, at.y);
    let apex = Vec3::new(at.x + lean.x, summit, at.y + lean.y);
    for k in 0..sides {
        let (b0, h0) = ring[k];
        let (b1, h1) = ring[(k + 1) % sides];
        let middle = (b0 + b1) / 2.0;
        let outward = Vec3::new(middle.x - axis.x, 0.0, middle.z - axis.z);
        if flat > 0.0 {
            split(out, [b0, b1, h1], outward, color, top);
            split(out, [b0, h1, h0], outward, color, top);
        } else {
            split(out, [b0, b1, apex], outward, color, top);
        }
    }
    if flat > 0.0 {
        let center = ring.iter().map(|(_, h)| *h).sum::<Vec3>() / sides as f32;
        for k in 0..sides {
            split(
                out,
                [center, ring[k].1, ring[(k + 1) % sides].1],
                Vec3::Y,
                color,
                top,
            );
        }
    }
}

/// The twelve corners and twenty faces of an icosahedron (unit radius).
fn icosahedron() -> ([Vec3; 12], [[usize; 3]; 20]) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let corners = [
        Vec3::new(-1.0, t, 0.0),
        Vec3::new(1.0, t, 0.0),
        Vec3::new(-1.0, -t, 0.0),
        Vec3::new(1.0, -t, 0.0),
        Vec3::new(0.0, -1.0, t),
        Vec3::new(0.0, 1.0, t),
        Vec3::new(0.0, -1.0, -t),
        Vec3::new(0.0, 1.0, -t),
        Vec3::new(t, 0.0, -1.0),
        Vec3::new(t, 0.0, 1.0),
        Vec3::new(-t, 0.0, -1.0),
        Vec3::new(-t, 0.0, 1.0),
    ]
    .map(|c| c.normalize());
    let faces = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    (corners, faces)
}

/// A cloud: three or four flattened puffs in a row along the ring (`along`: the way out there,
/// turned a quarter), `length` meters long.
fn cloud(
    out: &mut Vec<FarTriangle>,
    at: Vec3,
    outward: Vec2,
    length: f32,
    color: Rgb,
    rng: &mut BotRng,
) {
    let (corners, faces) = icosahedron();
    let along = Vec3::new(-outward.y, 0.0, outward.x);
    let puffs = 3 + (rng.next_u64() % 2) as usize;
    for k in 0..puffs {
        let t = (k as f32 + 0.5) / puffs as f32 - 0.5;
        let size = length / puffs as f32 * rng.range(0.8, 1.2);
        let middle = at + along * t * length * 0.8 + Vec3::Y * rng.range(-0.1, 0.25) * size;
        let scale = Vec3::new(size * 0.75, size * 0.38, size * 0.6);
        let turn = Quat::from_rotation_y(rng.range(0.0, TAU));
        for face in faces {
            let points = face.map(|i| middle + turn * (corners[i] * scale));
            let centroid = (points[0] + points[1] + points[2]) / 3.0;
            out.push(FarTriangle::facing(
                points,
                centroid - middle,
                color,
                FarPart::Cloud,
            ));
        }
    }
}

/// The far decor of a biome and the circuit's own layers, put together: the biome's fog.
pub fn far_decor_of(biome: Option<&BiomeData>, extra: &[FarLayer]) -> Option<FarDecor> {
    let biome = biome?;
    let mut far = biome.far.clone();
    far.layers.extend(extra.iter().cloned());
    Some(far)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::biome::{BIOMES_RON, biomes, parse_biomes};
    use crate::track::data::TrackData;

    fn circuit(file: &str) -> TrackData {
        let path = format!("{}/assets/{file}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let data = TrackData::from_ron(&text).unwrap();
        data.validate().unwrap();
        data
    }

    /// The far plan of a circuit of the game, as `dressing.rs` places it.
    fn plan_of(data: &TrackData) -> FarPlan {
        let center = CenterLine::of(data);
        let lakes: Vec<Lake> = data.lakes.iter().map(Lake::new).collect();
        let shortcuts: Vec<Shortcut> = data
            .shortcuts
            .iter()
            .map(|s| Shortcut::new(&center, s))
            .collect();
        let near = decor::place_decor_around(
            &center,
            data.biome(),
            &data.props,
            data.decor_seed,
            &lakes,
            &shortcuts,
        );
        let far = data
            .far_decor()
            .expect("a circuit of the game has a far decor");
        place_far_decor(
            &center,
            &data.biome().unwrap().palette,
            &far,
            data.decor_seed,
            &near,
            &lakes,
            &shortcuts,
        )
    }

    fn rect() -> GroundRect {
        GroundRect {
            min: Vec2::new(-100.0, -50.0),
            max: Vec2::new(200.0, 150.0),
        }
    }

    /// The ring beyond the slab: every point `d` beyond it, its way out unit and away from it,
    /// its length the perimeter's; a ring of points the same count for every distance, the
    /// first one on the slab's corners.
    #[test]
    fn the_rings_beyond_the_slab_keep_their_distance() {
        let rect = rect();
        for d in [0.0, 30.0, 250.0] {
            let perimeter = rect.perimeter(d);
            let mut previous = rect.ring_at(d, 0.0).0;
            let mut walked = 0.0;
            for k in 1..=2000 {
                let s = perimeter * k as f32 / 2000.0;
                let (p, out) = rect.ring_at(d, s);
                assert!((rect.outside(p) - d).abs() < 0.01, "{d} {s}: {p}");
                assert!((out.length() - 1.0).abs() < 1e-4);
                if d > 0.0 {
                    assert!(rect.outside(p + out) > d + 0.99, "{d} {s}");
                }
                walked += previous.distance(p);
                previous = p;
            }
            assert!(
                (walked - perimeter).abs() < perimeter * 0.002,
                "{d}: {walked}"
            );
            let ring = rect.ring(d);
            assert_eq!(ring.len(), rect.ring(0.0).len());
            for (p, _) in &ring {
                assert!((rect.outside(*p) - d).abs() < 0.01, "{d}: {p}");
            }
        }
        assert_eq!(rect.ring(0.0)[0].0, Vec2::new(200.0, -50.0));
        assert_eq!(rect.mirrored().mirrored(), rect);
    }

    /// A horizontal ray against a triangle: hit where it crosses it, not above nor beside it.
    #[test]
    fn a_horizontal_ray_meets_a_triangle_where_it_crosses_it() {
        let t = FarTriangle::facing(
            [
                Vec3::new(100.0, 0.0, -10.0),
                Vec3::new(100.0, 0.0, 10.0),
                Vec3::new(100.0, 50.0, 0.0),
            ],
            Vec3::NEG_X,
            (0.5, 0.5, 0.5),
            FarPart::Silhouette,
        );
        let hit = t
            .horizontal_hit(Vec3::new(0.0, 10.0, 0.0), Vec2::X)
            .unwrap();
        assert!((hit - 100.0).abs() < 1e-3, "{hit}");
        assert!(
            t.horizontal_hit(Vec3::new(0.0, 60.0, 0.0), Vec2::X)
                .is_none()
        );
        assert!(
            t.horizontal_hit(Vec3::new(0.0, 10.0, 0.0), Vec2::NEG_X)
                .is_none()
        );
        assert!(
            t.horizontal_hit(Vec3::new(0.0, 45.0, 3.0), Vec2::X)
                .is_none()
        );
        // Facing out: its front toward -x.
        let [a, b, c] = t.corners;
        assert!((b - a).cross(c - a).x < 0.0);
    }

    /// Issue #146 acceptance: every biome has a far decor: a fog within the ground beyond, a
    /// range that closes its horizon, pieces at mid-distance, and more (peaks or clouds); every
    /// circuit of the game has one (its biome's, and its own layers).
    #[test]
    fn every_biome_has_a_far_decor() {
        let parsed = parse_biomes(BIOMES_RON).unwrap();
        assert_eq!(parsed.len(), 6);
        for biome in biomes() {
            let far = &biome.far;
            far.validate().unwrap();
            assert!(
                far.has_range(),
                "{}: a range closes the horizon",
                biome.name
            );
            assert!(
                !far.models().is_empty(),
                "{}: pieces at mid-distance",
                biome.name
            );
            assert!(
                far.layers
                    .iter()
                    .any(|l| matches!(l, FarLayer::Peaks { .. } | FarLayer::Clouds { .. })),
                "{}",
                biome.name
            );
            // The fog fades the far decor, but it does not hide the circuit.
            assert!(far.fog.0 >= 150.0 && far.fog.1 <= 1600.0, "{}", biome.name);
        }
        for entry in crate::admin::tracks::circuits() {
            let data = circuit(&entry.asset_path());
            assert!(data.far_decor().is_some(), "{}", entry.name);
        }
    }

    /// Issue #146 acceptance (GD § 16: « rien ne montre le bord du monde ») on every circuit of
    /// the game and its mirror: from the chase camera anywhere on the circuit (every 40 m of its
    /// centerline, 3 m and 12 m above the road: a jump, the glider), looking level in any
    /// direction (every 2°), the eye meets a silhouette (a range, a peak) before the fog has
    /// hidden 85 % of it; under it, the ground beyond reaches it; and the ground beyond goes on
    /// past the fog's end in every direction (where a silhouette would not stand, the ground
    /// would fade into the sky before it stops). The pieces left out are few.
    #[test]
    fn no_circuit_shows_the_edge_of_the_world() {
        let failures: Vec<String> = std::thread::scope(|scope| {
            let threads: Vec<_> = crate::admin::tracks::circuits()
                .into_iter()
                .map(|entry| {
                    scope.spawn(move || {
                        let data = circuit(&entry.asset_path());
                        let mut failures = Vec::new();
                        for mirror in [false, true] {
                            let plan = plan_of(&data);
                            let (plan, data) = if mirror {
                                (plan.mirrored(), data.mirrored())
                            } else {
                                (plan, data.clone())
                            };
                            failures.extend(edge_failures(&entry.name, &data, &plan, mirror));
                        }
                        failures
                    })
                })
                .collect();
            threads
                .into_iter()
                .flat_map(|t| t.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
                .collect()
        });
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// The check above catches a far decor that does not close the horizon: Prairie Pompon's
    /// without its ranges (its peaks, clouds and pieces only) shows the edge of the world.
    #[test]
    fn without_its_ranges_a_circuit_shows_the_edge_of_the_world() {
        let data = circuit("tracks/prairie.track.ron");
        let center = CenterLine::of(&data);
        let mut far = data.far_decor().unwrap();
        far.layers
            .retain(|layer| !matches!(layer, FarLayer::Range { .. }));
        assert!(!far.has_range());
        let plan = place_far_decor(
            &center,
            &data.biome().unwrap().palette,
            &far,
            data.decor_seed,
            &DecorPlan::default(),
            &[],
            &[],
        );
        let failures = edge_failures("Prairie Pompon without ranges", &data, &plan, false);
        assert!(!failures.is_empty());
    }

    fn edge_failures(name: &str, data: &TrackData, plan: &FarPlan, mirror: bool) -> Vec<String> {
        let mut failures = Vec::new();
        let center = CenterLine::of(data);
        let rect = GroundRect::of(&center);
        assert_eq!(plan.ground, Some(rect), "{name}: the slab");
        let (start, end) = plan.fog;
        let visible = start + 0.85 * (end - start);
        assert!(FAR_REACH >= end, "{name}");
        // The ground beyond is all there: its area, the ring at FAR_REACH minus the slab.
        let ground: f32 = plan.of(FarPart::Ground).map(FarTriangle::area).sum();
        let size = rect.max - rect.min;
        let ring = 2.0 * (size.x + size.y) * FAR_REACH + PI * FAR_REACH * FAR_REACH;
        if (ground - ring).abs() > ring * 0.02 {
            failures.push(format!(
                "{name}: ground beyond {ground:.0} m², {ring:.0} expected"
            ));
        }
        if plan.left_out * 5 > plan.pieces.len() {
            failures.push(format!(
                "{name}: {} pieces left out, {} placed",
                plan.left_out,
                plan.pieces.len()
            ));
        }
        let samples = center.samples();
        let step = (40.0 / (center.length() / samples.len() as f32)).max(1.0) as usize;
        let mut worst = 0.0f32;
        for s in samples.iter().step_by(step) {
            for lift in [3.0, 12.0] {
                let eye = s.position + Vec3::Y * lift;
                for degrees in (0..360).step_by(2) {
                    let angle = (degrees as f32).to_radians();
                    let direction = Vec2::new(angle.cos(), angle.sin());
                    match plan.horizon_hit(eye, direction) {
                        Some(hit) if hit <= visible => worst = worst.max(hit),
                        hit => {
                            failures.push(format!(
                                "{name}{}: from {eye} toward {degrees}°, {hit:?} (visible \
                                 within {visible:.0} m)",
                                if mirror { " (mirror)" } else { "" }
                            ));
                            if failures.len() > 20 {
                                return failures;
                            }
                        }
                    }
                }
            }
        }
        println!(
            "{name}{}: the horizon at {worst:.0} m at most (visible within {visible:.0}), {} \
             triangles, {} pieces ({} left out)",
            if mirror { " (mirror)" } else { "" },
            plan.triangles.len(),
            plan.pieces.len(),
            plan.left_out
        );
        failures
    }

    /// The far decor never comes near the circuit: its silhouettes stand beyond the slab, its
    /// clouds high above, its pieces beyond the walls (`decor::clear_of_the_circuit`) and off the sea; the
    /// same plan on every load; the mirror is the mirror image; the triangles face out (the
    /// silhouettes away from their foot's middle, the ground up).
    #[test]
    fn the_far_decor_stays_away_from_the_circuit() {
        for entry in crate::admin::tracks::circuits() {
            let data = circuit(&entry.asset_path());
            let plan = plan_of(&data);
            assert_eq!(plan, plan_of(&data), "{}: deterministic", entry.name);
            let rect = plan.ground.unwrap();
            for t in plan.of(FarPart::Silhouette) {
                for c in t.corners {
                    assert!(rect.outside(c.xz()) > 0.0, "{}: {c}", entry.name);
                }
            }
            // The clouds high above everything.
            for t in plan.of(FarPart::Cloud) {
                for c in t.corners {
                    assert!(c.y > 60.0, "{}: {c}", entry.name);
                }
            }
            for t in plan.of(FarPart::Ground) {
                let [a, b, c] = t.corners;
                assert!((b - a).cross(c - a).y > 0.0, "{}: faces up", entry.name);
                assert!(t.corners.iter().all(|c| c.y == GROUND_HEIGHT));
            }
            let center = CenterLine::of(&data);
            for piece in &plan.pieces {
                assert!(
                    decor::clear_of_the_circuit(
                        center.samples(),
                        piece.position.xz(),
                        piece.radius
                    ),
                    "{}: {piece:?}",
                    entry.name
                );
                assert!(!piece.shadows);
            }
            let mirrored = plan.clone().mirrored();
            assert_eq!(mirrored.clone().mirrored(), plan);
            for (t, m) in plan.triangles.iter().zip(&mirrored.triangles) {
                let [a, b, c] = t.corners;
                let [d, e, f] = m.corners;
                let n = (b - a).cross(c - a);
                let o = (e - d).cross(f - d);
                assert!((n.x + o.x).abs() < 1e-2 && (n.y - o.y).abs() < 1e-2);
            }
        }
    }

    /// The broken far decors are refused, with a reason.
    #[test]
    fn broken_far_decors_are_refused() {
        let good = biomes()[0].far.clone();
        assert!(good.validate().is_ok());
        let with = |change: &dyn Fn(&mut FarDecor)| {
            let mut far = good.clone();
            change(&mut far);
            far.validate()
        };
        assert!(with(&|f| f.fog = (300.0, 200.0)).is_err());
        assert!(with(&|f| f.fog = (300.0, FAR_REACH + 1.0)).is_err());
        let layer = |layer: FarLayer| with(&|f| f.layers.push(layer.clone()));
        assert!(
            layer(FarLayer::Range {
                distance: 100.0,
                depth: 60.0,
                height: (80.0, 40.0),
                spacing: 30.0,
                color: (0.5, 0.5, 0.5),
                top: None,
            })
            .is_err()
        );
        assert!(
            layer(FarLayer::Peaks {
                count: 3,
                distance: (0.0, 100.0),
                height: (10.0, 20.0),
                radius: (5.0, 8.0),
                flat: 0.0,
                sides: 2,
                color: (0.5, 0.5, 0.5),
                top: None,
            })
            .is_err()
        );
        assert!(
            layer(FarLayer::Pieces {
                models: vec!["tree".into()],
                count: 3,
                height: (10.0, 20.0),
                radius: 5.0,
                distance: (60.0, 100.0),
                side: Side::Both,
            })
            .unwrap_err()
            .contains("tree")
        );
        assert!(
            layer(FarLayer::Clouds {
                count: 3,
                distance: (0.0, 100.0),
                altitude: (60.0, 100.0),
                size: (20.0, 40.0),
                color: (1.0, 1.5, 1.0),
            })
            .is_err()
        );
        assert!(
            layer(FarLayer::Sea {
                color: (0.1, 0.4, 0.7),
                from: 20.0,
                beach: 40.0,
            })
            .is_err()
        );
    }
}

/// Preview (issue #146): every circuit's far plan (its triangles, its pieces and the near decor's,
/// its centerline), written as text for an offline renderer (shapes, scales, colors and the
/// horizon, before a build for the browser). Not a check.
/// `FLUFFY_FAR_DUMP=/path/dir cargo test --lib dump_far_decor -- --ignored`
#[cfg(test)]
mod preview {
    use std::fmt::Write;

    use super::*;
    use crate::track::data::TrackData;

    #[test]
    #[ignore]
    fn dump_far_decor() {
        let Ok(out) = std::env::var("FLUFFY_FAR_DUMP") else {
            return;
        };
        for entry in crate::admin::tracks::circuits() {
            let path = format!(
                "{}/assets/{}",
                env!("CARGO_MANIFEST_DIR"),
                entry.asset_path()
            );
            let data = TrackData::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let center = CenterLine::of(&data);
            let lakes: Vec<Lake> = data.lakes.iter().map(Lake::new).collect();
            let shortcuts: Vec<Shortcut> = data
                .shortcuts
                .iter()
                .map(|s| Shortcut::new(&center, s))
                .collect();
            let near = decor::place_decor_around(
                &center,
                data.biome(),
                &data.props,
                data.decor_seed,
                &lakes,
                &shortcuts,
            );
            let (Some(far), Some(biome)) = (data.far_decor(), data.biome()) else {
                continue;
            };
            let palette = biome.palette;
            let plan = place_far_decor(
                &center,
                &palette,
                &far,
                data.decor_seed,
                &near,
                &lakes,
                &shortcuts,
            );
            let mut s = String::new();
            let rgb = |(r, g, b): Rgb| format!("{r} {g} {b}");
            let _ = writeln!(s, "SKY {}", rgb(palette.sky));
            let _ = writeln!(s, "GROUNDC {}", rgb(palette.ground));
            let _ = writeln!(s, "ASPHALT {}", rgb(palette.asphalt));
            let _ = writeln!(s, "FOG {} {}", plan.fog.0, plan.fog.1);
            let r = GroundRect::of(&center);
            let _ = writeln!(s, "SLAB {} {} {} {}", r.min.x, r.min.y, r.max.x, r.max.y);
            for t in &plan.triangles {
                let [a, b, c] = t.corners;
                let _ = writeln!(
                    s,
                    "T {} {} {} {} {} {} {} {} {} {} {} {}",
                    a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z, t.color[0], t.color[1], t.color[2]
                );
            }
            for (key, models, pieces) in [
                ("P", &plan.models, &plan.pieces),
                ("N", &near.models, &near.pieces),
            ] {
                for p in pieces {
                    let _ = writeln!(
                        s,
                        "{key} {} {} {} {} {} {}",
                        p.position.x,
                        p.position.y,
                        p.position.z,
                        p.height,
                        p.radius,
                        models[p.model as usize]
                    );
                }
            }
            for c in center.samples().iter().step_by(2) {
                let _ = writeln!(
                    s,
                    "C {} {} {} {} {} {} {}",
                    c.position.x,
                    c.position.y,
                    c.position.z,
                    c.forward.x,
                    c.forward.z,
                    c.half_width,
                    c.distance
                );
            }
            let file = entry
                .asset_path()
                .replace("tracks/", "")
                .replace(".track.ron", ".txt");
            std::fs::write(format!("{out}/{file}"), s).unwrap();
        }
    }
}
