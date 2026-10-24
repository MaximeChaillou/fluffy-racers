//! Issue #181: the vignette of a circuit in « Jouer solo » (#99), its outline seen from above
//! drawn as **a real line**, once, on the CPU (like the items' icons of #106, `models::raster`):
//! a smooth line of constant thickness along its centerline, closed for a circuit of laps, open
//! from its start to its finish for a long race (#119), the start line (a checker, or a bar where
//! an open race starts), a small arrow for the way the race goes, the special stretches in color
//! (anti-gravity, water, glider), the bridges read at a glance (the road on top has a white edge,
//! the one below stops under it), mirrored in the mirror mode. Pure, no ECS and no GPU: the same
//! picture in the browser, natively and in the headless tests.
//!
//! **How it is drawn**: every pixel takes its distance to the line's segments (in pixels: a
//! constant thickness, its edge smoothed over one pixel), in layers: a soft shadow, the white
//! edge (the casing), then the color of the road. Where the road passes over itself (two of its
//! segments cross in the picture, one [`BRIDGE_HEIGHT`] above the other: [`bridges`]), a pixel
//! near the crossing keeps two layers, the road below then the road on top: the road below stops
//! a white edge before the casing of the road on top. Elsewhere, two stretches side by side make
//! one line, whatever their heights. A long race, or a road that winds a lot, takes a thinner line
//! in a small vignette ([`MAX_INK`]): the same all along it. The marks (the start, the finish, the
//! arrow) are painted over it, at sixteen samples a pixel.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::math::Vec2;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::style::{self, size};
use crate::track::anti_gravity::AntiGravityZone;
use crate::track::biome::Palette;
use crate::track::layout::CenterLine;
use crate::track::{Gap, Lake, TrackData, water_depth};

/// What a stretch of the road is, colored on the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Zone {
    Road,
    /// Issue #78: the karts drive on the walls there.
    AntiGravity,
    /// Issue #20: the road under the water of a lake.
    Water,
    /// Issue #20: no road, the karts glide over the gap.
    Glide,
}

/// A point of the outline: where it is in the unit box (see [`track_map`]), its height (m), its
/// distance from the start line along the centerline (m), and its stretch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapPoint {
    pub at: Vec2,
    pub height: f32,
    pub along: f32,
    pub zone: Zone,
}

/// The outline of a circuit seen from above, in the unit box `[0, 1]²` of a box `aspect` times
/// wider than high (x right, z down the picture; mirrored, x the other way), centered, keeping its
/// proportions. Its first point is the start line; closed (a circuit of laps), its last one joins
/// the first; open (a long race), it runs from the start line to the finish line, its last point.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackMap {
    pub points: Vec<MapPoint>,
    pub closed: bool,
    /// The length of the whole centerline (m): the distances along it wrap round it.
    pub length: f32,
}

impl TrackMap {
    /// How far apart two distances along the track are (m): round the loop on a circuit of laps.
    pub fn apart(&self, a: f32, b: f32) -> f32 {
        let d = (a - b).abs();
        if self.closed {
            d.min(self.length - d)
        } else {
            d
        }
    }
}

/// The outline of circuit `data` (mirrored if `mirror`), fit in a box `aspect` times wider than
/// high: its centerline sample by sample (about a meter apart), each with its stretch.
pub fn track_map(data: &TrackData, mirror: bool, aspect: f32) -> TrackMap {
    let mirrored;
    let data = if mirror {
        mirrored = data.mirrored();
        &mirrored
    } else {
        data
    };
    let center = CenterLine::of(data);
    let gaps: Vec<Gap> = data.gaps.iter().map(|g| Gap::new(&center, g)).collect();
    let lakes: Vec<Lake> = data.lakes.iter().map(Lake::new).collect();
    let zones: Vec<AntiGravityZone> = data
        .anti_gravity
        .iter()
        .map(|z| AntiGravityZone::new(&center, z))
        .collect();
    let finish = center.open().map(|open| open.finish);
    let zone_at = |distance: f32, position| {
        if gaps.iter().any(|g| g.contains(&center, distance)) {
            Zone::Glide
        } else if water_depth(&lakes, position).is_some_and(|depth| depth > 0.0) {
            Zone::Water
        } else if zones.iter().any(|z| z.contains(&center, distance)) {
            Zone::AntiGravity
        } else {
            Zone::Road
        }
    };
    let mut points: Vec<MapPoint> = center
        .samples()
        .iter()
        .filter(|s| finish.is_none_or(|finish| s.distance < finish))
        .map(|s| MapPoint {
            at: Vec2::new(s.position.x, s.position.z),
            height: s.position.y,
            along: s.distance,
            zone: zone_at(s.distance, s.position),
        })
        .collect();
    if let Some(finish) = finish {
        let s = center.at(finish);
        points.push(MapPoint {
            at: Vec2::new(s.position.x, s.position.z),
            height: s.position.y,
            along: finish,
            zone: zone_at(finish, s.position),
        });
    }
    fit(&mut points, aspect);
    TrackMap {
        points,
        closed: finish.is_none(),
        length: center.length(),
    }
}

/// Into the unit box: the same scale on both axes in a box `aspect` times wider than high,
/// centered.
fn fit(points: &mut [MapPoint], aspect: f32) {
    let (min, max) = points.iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(min, max), p| (min.min(p.at), max.max(p.at)),
    );
    let size = (max - min).max(Vec2::splat(1e-3));
    let aspect = aspect.max(1e-3);
    // Box units: x in [0, aspect], y in [0, 1].
    let scale = (aspect / size.x).min(1.0 / size.y);
    let offset = (Vec2::new(aspect, 1.0) - size * scale) / 2.0;
    for point in points {
        let q = (point.at - min) * scale + offset;
        point.at = Vec2::new(q.x / aspect, q.y);
    }
}

/// Two stretches of a road closer than this (m, in height) are one road where they meet: only a
/// road this much above another passes over it.
pub const BRIDGE_HEIGHT: f32 = 3.0;
/// Two segments this close along the track (m) never cross each other (a tight turn); two crossings
/// this close along both roads are the same bridge.
const NEAR_ALONG: f32 = 30.0;

/// Where the road passes over itself: for each crossing of two of its segments in the picture
/// (the unit box, see [`TrackMap`]), the distances along the track of the road on top and of the
/// road below, one per bridge (the crossings of a same bridge, side by side, are one).
pub fn bridges(map: &TrackMap) -> Vec<(f32, f32)> {
    let segments = segments_of(map, |p| p.at);
    let mut found: Vec<(f32, f32)> = Vec::new();
    for (i, a) in segments.iter().enumerate() {
        for b in &segments[i + 1..] {
            let apart =
                a.a.max(a.b).cmplt(b.a.min(b.b)).any() || b.a.max(b.b).cmplt(a.a.min(a.b)).any();
            if apart || map.apart(a.along, b.along) < NEAR_ALONG || !crosses(a.a, a.b, b.a, b.b) {
                continue;
            }
            let (top, below) = if a.height >= b.height { (a, b) } else { (b, a) };
            if top.height - below.height < BRIDGE_HEIGHT {
                continue;
            }
            let same = found.iter().any(|(t, l)| {
                map.apart(*t, top.along) < NEAR_ALONG && map.apart(*l, below.along) < NEAR_ALONG
            });
            if !same {
                found.push((top.along, below.along));
            }
        }
    }
    found
}

/// Whether segments `a0 a1` and `b0 b1` cross.
fn crosses(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> bool {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(b0, b1, a0), side(b0, b1, a1));
    let (d3, d4) = (side(a0, a1, b0), side(a0, a1, b1));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// A segment of the line, between two of its points.
#[derive(Clone, Copy, Debug)]
struct Segment {
    a: Vec2,
    b: Vec2,
    height: f32,
    along: f32,
    zone: Zone,
}

/// The segments of `map`'s line, its points placed by `place`.
fn segments_of(map: &TrackMap, place: impl Fn(&MapPoint) -> Vec2) -> Vec<Segment> {
    let n = map.points.len();
    let count = if map.closed { n } else { n.saturating_sub(1) };
    (0..count)
        .map(|i| {
            let (p, q) = (&map.points[i], &map.points[(i + 1) % n]);
            Segment {
                a: place(p),
                b: place(q),
                height: (p.height + q.height) / 2.0,
                along: p.along,
                zone: p.zone,
            }
        })
        .collect()
}

/// How a vignette is drawn (px of its picture).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapStyle {
    pub width: u32,
    pub height: u32,
    /// The box of the line in the picture: its share of the width and of the height, and how much
    /// lower than the middle it is (a share of the height: the sky is at the top).
    pub inner: Vec2,
    pub lower: f32,
    /// Half the thickness of the road's color, and its white edge around it.
    pub line: f32,
    pub edge: f32,
    /// Where the shadow falls (down and to the right) and how dark it is (0 to 1).
    pub shadow: Vec2,
    pub shadow_alpha: f32,
    /// The checker of the start line: its cells across the road and along it.
    pub checker: (u32, u32),
    /// The arrow: how far after the start line (px along the line), its length and half its
    /// width.
    pub arrow_after: f32,
    pub arrow: Vec2,
}

/// The line covers this share of its box at most (with its white edge): a long race, or a circuit
/// whose road winds a lot, takes a thinner line in a small vignette (the same along the whole line).
pub const MAX_INK: f32 = 0.3;
/// The line keeps this much of its thickness at least.
const MIN_THICKNESS: f32 = 0.5;

impl MapStyle {
    /// The style for a line `length` pixels long: thinner (its edge, its shadow and its marks too)
    /// if it would cover more than [`MAX_INK`] of its box.
    pub fn fitted(&self, length: f32) -> Self {
        let area = self.width as f32 * self.inner.x * self.height as f32 * self.inner.y;
        let casing = self.line + self.edge;
        let wanted = MAX_INK * area / (2.0 * length.max(1.0));
        let k = (wanted / casing).clamp(MIN_THICKNESS, 1.0);
        Self {
            line: self.line * k,
            edge: self.edge * k,
            shadow: self.shadow * k,
            arrow: self.arrow * k,
            ..*self
        }
    }
}

/// The colors of a vignette (sRGB, 0 to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapColors {
    pub road: [f32; 3],
    pub anti_gravity: [f32; 3],
    pub water: [f32; 3],
    pub glide: [f32; 3],
    /// The white edge of the line.
    pub edge: [f32; 3],
    /// The ink: the shadow, the dark cells of the checker, the edge of the marks.
    pub ink: [f32; 3],
    /// The light cells of the checker, the arrow.
    pub light: [f32; 3],
}

impl MapColors {
    pub fn of(&self, zone: Zone) -> [f32; 3] {
        match zone {
            Zone::Road => self.road,
            Zone::AntiGravity => self.anti_gravity,
            Zone::Water => self.water,
            Zone::Glide => self.glide,
        }
    }
}

/// What a pixel of a vignette shows (for the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Empty,
    Shadow,
    /// The white edge of the line (on a bridge: what covers the road below).
    Edge,
    /// The road's color: of the road on top where it passes over another.
    Line(Zone),
    /// The road below a bridge, seen beside it.
    Below(Zone),
    /// The start line, the finish line, the arrow.
    Mark,
}

/// A vignette's picture: `width` × `height` pixels, sRGB with their opacity (straight), from the
/// top row down, and what each pixel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct MapImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
    pub kinds: Vec<Kind>,
}

impl MapImage {
    pub fn kind(&self, x: u32, y: u32) -> Kind {
        self.kinds[(y * self.width + x) as usize]
    }

    /// Its pixels as bytes, row after row (RGBA, sRGB): an image's data.
    pub fn bytes(&self) -> Vec<u8> {
        self.pixels.iter().flatten().copied().collect()
    }
}

/// Where `map`'s point `at` (the unit box) is in a picture of `style`.
pub fn to_pixels(style: &MapStyle, at: Vec2) -> Vec2 {
    let size = Vec2::new(style.width as f32, style.height as f32);
    let inner = size * style.inner;
    let corner = (size - inner) / 2.0 + Vec2::new(0.0, style.lower * size.y);
    corner + at * inner
}

/// How long `map`'s line is in a picture of `style` (px).
pub fn pixel_length(map: &TrackMap, style: &MapStyle) -> f32 {
    segments_of(map, |p| to_pixels(style, p.at))
        .iter()
        .map(|s| s.a.distance(s.b))
        .sum()
}

/// The distance from `p` to the segment `a b`.
fn distance_to(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The share of a pixel a shape `radius` thick covers, its center `distance` away (its edge
/// smoothed over a pixel).
fn coverage(distance: f32, radius: f32) -> f32 {
    (radius - distance + 0.5).clamp(0.0, 1.0)
}

/// A color painted over a pixel (premultiplied): `color` at `alpha`.
fn over(pixel: &mut [f32; 4], color: [f32; 3], alpha: f32) {
    let keep = 1.0 - alpha;
    for c in 0..3 {
        pixel[c] = color[c] * alpha + pixel[c] * keep;
    }
    pixel[3] = alpha + pixel[3] * keep;
}

/// What a pixel keeps of the line: the distances to the nearest segments of the road below and
/// of the road on top (and from the pixel where its shadow comes from), and their stretches.
#[derive(Clone, Copy)]
struct Layers {
    /// The highest segment near the pixel: its height and its index.
    top_height: f32,
    top_segment: usize,
    top: f32,
    top_shadow: f32,
    top_zone: Zone,
    below: f32,
    below_shadow: f32,
    below_zone: Zone,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            top_height: f32::NEG_INFINITY,
            top_segment: 0,
            top: f32::INFINITY,
            top_shadow: f32::INFINITY,
            top_zone: Zone::Road,
            below: f32::INFINITY,
            below_shadow: f32::INFINITY,
            below_zone: Zone::Road,
        }
    }
}

/// Draws the vignette of `map` (see the module).
pub fn render(map: &TrackMap, style: &MapStyle, colors: &MapColors) -> MapImage {
    let (w, h) = (style.width.max(1) as usize, style.height.max(1) as usize);
    // The line in pixels, a point every pixel or so (a stretch's end kept).
    let mut kept: Vec<MapPoint> = Vec::with_capacity(map.points.len());
    for (i, point) in map.points.iter().enumerate() {
        let mut point = *point;
        point.at = to_pixels(style, point.at);
        let last = i + 1 == map.points.len();
        match kept.last() {
            Some(previous)
                if !last && previous.zone == point.zone && previous.at.distance(point.at) < 1.0 => {
            }
            _ => kept.push(point),
        }
    }
    let line = TrackMap {
        points: kept,
        closed: map.closed,
        length: map.length,
    };
    let segments = segments_of(&line, |p| p.at);
    let style = &style.fitted(pixel_length(map, style));
    let reach = style.line + style.edge + style.shadow.length() + 1.0;
    let mut layers = vec![Layers::default(); w * h];
    let bounds = |s: &Segment| {
        let lo = s.a.min(s.b) - Vec2::splat(reach);
        let hi = s.a.max(s.b) + Vec2::splat(reach);
        (
            (
                lo.x.floor().max(0.0) as usize,
                lo.y.floor().max(0.0) as usize,
            ),
            (
                (hi.x.ceil().max(0.0) as usize).min(w),
                (hi.y.ceil().max(0.0) as usize).min(h),
            ),
        )
    };
    // First, the highest segment near each pixel.
    for (k, s) in segments.iter().enumerate() {
        let ((x0, y0), (x1, y1)) = bounds(s);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let layer = &mut layers[y * w + x];
                if s.height > layer.top_height && distance_to(p, s.a, s.b) <= reach {
                    layer.top_height = s.height;
                    layer.top_segment = k;
                }
            }
        }
    }
    // The bridges: the segments of each road on top and of each road below it, near where they
    // cross (one bit a bridge). Elsewhere, two stretches of road side by side make one line,
    // whatever their heights.
    let per_pixel = line.points.last().map_or(1.0, |p| p.along)
        / segments
            .iter()
            .map(|s| s.a.distance(s.b))
            .sum::<f32>()
            .max(1.0);
    let region = (6.0 * reach * per_pixel).max(NEAR_ALONG);
    let (mut tops, mut belows) = (vec![0u64; segments.len()], vec![0u64; segments.len()]);
    for (b, (top, below)) in bridges(&line).into_iter().take(64).enumerate() {
        for (k, s) in segments.iter().enumerate() {
            if line.apart(s.along, top) < region {
                tops[k] |= 1 << b;
            }
            if line.apart(s.along, below) < region {
                belows[k] |= 1 << b;
            }
        }
    }
    // Then the nearest segments on top (the highest one, and all but those of a road below it)
    // and below.
    for (k, s) in segments.iter().enumerate() {
        let ((x0, y0), (x1, y1)) = bounds(s);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let layer = &mut layers[y * w + x];
                if layer.top_height == f32::NEG_INFINITY {
                    continue;
                }
                let d = distance_to(p, s.a, s.b);
                let shadow = distance_to(p - style.shadow, s.a, s.b);
                let on_top = belows[k] & tops[layer.top_segment] == 0;
                let (nearest, nearest_shadow, zone) = if on_top {
                    (&mut layer.top, &mut layer.top_shadow, &mut layer.top_zone)
                } else {
                    (
                        &mut layer.below,
                        &mut layer.below_shadow,
                        &mut layer.below_zone,
                    )
                };
                if d < *nearest {
                    *nearest = d;
                    *zone = s.zone;
                }
                *nearest_shadow = nearest_shadow.min(shadow);
            }
        }
    }
    // The layers, from the bottom up.
    let casing = style.line + style.edge;
    let mut picture = vec![[0.0f32; 4]; w * h];
    let mut kinds = vec![Kind::Empty; w * h];
    for (i, layer) in layers.iter().enumerate() {
        let pixel = &mut picture[i];
        let mut kind = Kind::Empty;
        // The road below stops a white edge before the road on top (the ground between them).
        let moat = 1.0 - coverage(layer.top, casing + style.edge);
        for (d, shadow, zone, below) in [
            (layer.below, layer.below_shadow, layer.below_zone, true),
            (layer.top, layer.top_shadow, layer.top_zone, false),
        ] {
            let keep = if below { moat } else { 1.0 };
            let shade = coverage(shadow, casing) * style.shadow_alpha * keep;
            let (edge, fill) = (coverage(d, casing) * keep, coverage(d, style.line) * keep);
            over(pixel, colors.ink, shade);
            over(pixel, colors.edge, edge);
            over(pixel, colors.of(zone), fill);
            if fill > 0.5 {
                kind = if below {
                    Kind::Below(zone)
                } else {
                    Kind::Line(zone)
                };
            } else if edge > 0.5 {
                kind = Kind::Edge;
            } else if shade > 0.5 * style.shadow_alpha && kind == Kind::Empty {
                kind = Kind::Shadow;
            }
        }
        kinds[i] = kind;
    }
    let mut image = Marks {
        picture,
        kinds,
        width: w,
        height: h,
    };
    marks(&mut image, &line, style, colors);
    MapImage {
        width: w as u32,
        height: h as u32,
        pixels: image
            .picture
            .iter()
            .map(|p| {
                let a = p[3];
                let straight = |c: f32| {
                    if a > 1e-4 {
                        (c / a * 255.0).round().clamp(0.0, 255.0) as u8
                    } else {
                        0
                    }
                };
                [
                    straight(p[0]),
                    straight(p[1]),
                    straight(p[2]),
                    (a * 255.0).round().clamp(0.0, 255.0) as u8,
                ]
            })
            .collect(),
        kinds: image.kinds,
    }
}

/// The picture being drawn, for its marks.
struct Marks {
    picture: Vec<[f32; 4]>,
    kinds: Vec<Kind>,
    width: usize,
    height: usize,
}

impl Marks {
    /// Paints a mark over the pixels within `radius` of `center`: `shape` says the color of each
    /// sample (in the mark's frame: `along` the line, `across` it) or nothing; 4 × 4 samples a
    /// pixel.
    fn paint(
        &mut self,
        center: Vec2,
        forward: Vec2,
        radius: f32,
        shape: impl Fn(f32, f32) -> Option<[f32; 3]>,
    ) {
        const SIDE: usize = 4;
        let across = forward.perp();
        let lo = (center - Vec2::splat(radius + 1.0)).max(Vec2::ZERO);
        let hi = center + Vec2::splat(radius + 1.0);
        let (x1, y1) = (
            (hi.x.ceil().max(0.0) as usize).min(self.width),
            (hi.y.ceil().max(0.0) as usize).min(self.height),
        );
        for y in lo.y as usize..y1 {
            for x in lo.x as usize..x1 {
                let (mut sum, mut hits) = ([0.0f32; 3], 0usize);
                for sy in 0..SIDE {
                    for sx in 0..SIDE {
                        let p = Vec2::new(
                            x as f32 + (sx as f32 + 0.5) / SIDE as f32,
                            y as f32 + (sy as f32 + 0.5) / SIDE as f32,
                        ) - center;
                        if let Some(color) = shape(p.dot(forward), p.dot(across)) {
                            for c in 0..3 {
                                sum[c] += color[c];
                            }
                            hits += 1;
                        }
                    }
                }
                if hits == 0 {
                    continue;
                }
                let alpha = hits as f32 / (SIDE * SIDE) as f32;
                let color = sum.map(|c| c / hits as f32);
                let i = y * self.width + x;
                over(&mut self.picture[i], color, alpha);
                if alpha > 0.5 {
                    self.kinds[i] = Kind::Mark;
                }
            }
        }
    }
}

/// The direction of the line at its point `i` (unit, in the picture).
fn direction(line: &TrackMap, i: usize) -> Vec2 {
    let n = line.points.len();
    let (a, b) = if i + 1 < n {
        (line.points[i].at, line.points[i + 1].at)
    } else if line.closed {
        (line.points[i].at, line.points[0].at)
    } else {
        (line.points[i - 1].at, line.points[i].at)
    };
    (b - a).normalize_or(Vec2::X)
}

/// The start line (a checker; a bar where an open race starts, its finish a checker) and the
/// arrow after it.
fn marks(image: &mut Marks, line: &TrackMap, style: &MapStyle, colors: &MapColors) {
    let n = line.points.len();
    if n < 2 {
        return;
    }
    let half_across = (style.line + style.edge) * 1.3;
    let half_along = (style.line * 0.8).max(1.5);
    let rim = (style.edge * 0.6).max(0.6);
    let radius = half_across.hypot(half_along) + rim;
    let (cells_across, cells_along) = (style.checker.0.max(1), style.checker.1.max(1));
    let checker = |along: f32, across: f32| {
        let (u, v) = (along.abs(), across.abs());
        if u > half_along + rim || v > half_across + rim {
            return None;
        }
        if u > half_along || v > half_across {
            return Some(colors.ink);
        }
        let i = ((along + half_along) / (2.0 * half_along) * cells_along as f32) as u32;
        let j = ((across + half_across) / (2.0 * half_across) * cells_across as f32) as u32;
        Some(if (i + j).is_multiple_of(2) {
            colors.light
        } else {
            colors.ink
        })
    };
    let bar = |along: f32, across: f32| {
        let (u, v) = (along.abs(), across.abs());
        if u > half_along + rim || v > half_across + rim {
            None
        } else if u > half_along || v > half_across {
            Some(colors.ink)
        } else {
            Some(colors.light)
        }
    };
    // The arrow, after the start line: a little white triangle on the road.
    let mut walked = 0.0;
    for i in 1..n {
        walked += line.points[i].at.distance(line.points[i - 1].at);
        if walked >= style.arrow_after {
            let (length, half) = (style.arrow.x, style.arrow.y);
            let triangle = |along: f32, across: f32| {
                // Its tip ahead, its base behind.
                let t = (length / 2.0 - along) / length;
                (0.0..=1.0)
                    .contains(&t)
                    .then_some(())
                    .filter(|_| across.abs() <= half * t)
                    .map(|_| colors.light)
            };
            image.paint(
                line.points[i].at,
                direction(line, i),
                length.max(half),
                triangle,
            );
            break;
        }
    }
    let start = line.points[0].at;
    if line.closed {
        image.paint(start, direction(line, 0), radius, checker);
    } else {
        image.paint(start, direction(line, 0), radius, bar);
        let last = n - 1;
        image.paint(line.points[last].at, direction(line, last), radius, checker);
    }
}

/// The pictures are drawn at this many pixels per pixel of the reference window (sharp on a
/// window twice its size, or a screen of two pixels per point).
pub const RESOLUTION: f32 = 2.0;

/// The look of a vignette (`big`: a race alone's, a time trial's), from the charter's sizes
/// (`style::size`), at [`RESOLUTION`].
pub fn map_style(big: bool) -> MapStyle {
    let k = RESOLUTION;
    let (width, height, line, edge) = if big {
        (
            size::BIG_VIGNETTE_WIDTH,
            size::BIG_VIGNETTE_HEIGHT,
            size::BIG_VIGNETTE_LINE,
            size::BIG_VIGNETTE_EDGE,
        )
    } else {
        (
            size::VIGNETTE_WIDTH,
            size::VIGNETTE_HEIGHT,
            size::VIGNETTE_LINE,
            size::VIGNETTE_EDGE,
        )
    };
    let casing = line + edge;
    MapStyle {
        width: (width * k).round() as u32,
        height: (height * k).round() as u32,
        // The road in the middle 84 % × 76 % of the vignette, a little low (the sky on top).
        inner: Vec2::new(0.84, 0.76),
        lower: 0.04,
        line: line * k,
        edge: edge * k,
        shadow: Vec2::new(0.45, 0.7) * casing * k,
        shadow_alpha: 0.28,
        checker: if big { (4, 2) } else { (2, 2) },
        arrow_after: casing * k * if big { 9.0 } else { 6.0 },
        arrow: Vec2::new(2.6, 0.9) * line * k,
    }
}

/// A color of the charter (sRGB, 0 to 1).
fn srgb(color: bevy::color::Color) -> [f32; 3] {
    let c = color.to_srgba();
    [c.red, c.green, c.blue]
}

/// The colors of a vignette of a circuit of `palette`: its asphalt for the road; the charter's
/// Prune for the anti-gravity, Lagon for the water, Mangue for the glider; a white edge, the
/// Myrtille ink.
pub fn map_colors(palette: &Palette) -> MapColors {
    let (r, g, b) = palette.asphalt;
    MapColors {
        road: [r, g, b],
        anti_gravity: srgb(style::palette::PRUNE),
        water: srgb(style::palette::LAGON),
        glide: srgb(style::palette::MANGUE),
        edge: srgb(style::palette::NEIGE),
        ink: srgb(style::palette::MYRTILLE),
        light: srgb(style::palette::NEIGE),
    }
}

/// The vignette of circuit `data` (mirrored if `mirror`; `big`: a race alone's, a time trial's),
/// in the colors of its biome.
pub fn draw(data: &TrackData, mirror: bool, big: bool) -> MapImage {
    let style = map_style(big);
    let aspect = (style.width as f32 * style.inner.x) / (style.height as f32 * style.inner.y);
    let palette = data.biome().map_or_else(Palette::default, |b| b.palette);
    render(
        &track_map(data, mirror, aspect),
        &style,
        &map_colors(&palette),
    )
}

/// A vignette's picture as an image for `bevy_ui` (smooth when it is drawn smaller).
pub fn to_image(picture: &MapImage) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: picture.width,
            height: picture.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        picture.bytes(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circuit(file: &str) -> TrackData {
        let path = format!("{}/assets/tracks/{file}", env!("CARGO_MANIFEST_DIR"));
        TrackData::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A figure of eight whose second loop climbs 6 m over the first one where they cross: no
    /// circuit of the game passes over itself yet (GD « Conception des circuits », rule 3: « pas
    /// encore possible », #164), this one does.
    fn eight() -> TrackData {
        let points: Vec<String> = (0..16)
            .map(|k| {
                let t = std::f32::consts::TAU * k as f32 / 16.0 + 0.6;
                let (x, z, y) = (90.0 * t.sin(), 45.0 * (2.0 * t).sin(), 3.0 - 3.0 * t.cos());
                format!("(pos: ({x:.2}, {y:.2}, {z:.2}), width: 10.0)")
            })
            .collect();
        let text = format!("(name: \"Huit\", points: [{}])", points.join(", "));
        TrackData::from_ron(&text).unwrap()
    }

    fn aspect(style: &MapStyle) -> f32 {
        (style.width as f32 * style.inner.x) / (style.height as f32 * style.inner.y)
    }

    fn colors() -> MapColors {
        map_colors(&Palette::default())
    }

    /// The map and the picture of `data`.
    fn drawn(data: &TrackData, mirror: bool, big: bool) -> (TrackMap, MapStyle, MapImage) {
        let style = map_style(big);
        let map = track_map(data, mirror, aspect(&style));
        let image = render(&map, &style, &colors());
        let style = style.fitted(pixel_length(&map, &style));
        (map, style, image)
    }

    /// The pixels the centerline passes over in the picture, a quarter of a pixel apart, from the
    /// start line round to it (closed) or to the finish line (open), with the distance along the
    /// track of each.
    fn walk(map: &TrackMap, style: &MapStyle) -> Vec<((u32, u32), f32)> {
        let n = map.points.len();
        let count = if map.closed { n } else { n - 1 };
        let mut pixels = Vec::new();
        for i in 0..count {
            let (p, q) = (&map.points[i], &map.points[(i + 1) % n]);
            let (a, b) = (to_pixels(style, p.at), to_pixels(style, q.at));
            let steps = (a.distance(b) * 4.0).ceil().max(1.0) as usize;
            for k in 0..steps {
                let at = a.lerp(b, k as f32 / steps as f32);
                let pixel = (at.x as u32, at.y as u32);
                if pixels.last().is_none_or(|(last, _)| *last != pixel) {
                    pixels.push((pixel, p.along));
                }
            }
        }
        pixels
    }

    /// Issue #99, evolved with #181: the outline in the unit box, touching two of its opposite
    /// sides, centered on both; mirrored, the same outline flipped left to right; its first point
    /// is the start line; a point about every meter (the centerline's samples).
    #[test]
    fn the_outline_fits_its_vignette() {
        let data = circuit("prairie.track.ron");
        for aspect in [1.0, 1.8, 0.5] {
            let map = track_map(&data, false, aspect);
            let (min, max) = map.points.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(min, max), p| (min.min(p.at), max.max(p.at)),
            );
            assert!(min.x >= -1e-4 && min.y >= -1e-4, "{min}");
            assert!(max.x <= 1.0 + 1e-4 && max.y <= 1.0 + 1e-4, "{max}");
            let touches_x = min.x < 1e-3 && max.x > 1.0 - 1e-3;
            let touches_y = min.y < 1e-3 && max.y > 1.0 - 1e-3;
            assert!(touches_x || touches_y, "{aspect}: {min} {max}");
            assert!(((min.x + max.x) / 2.0 - 0.5).abs() < 1e-3, "{aspect}");
            assert!(((min.y + max.y) / 2.0 - 0.5).abs() < 1e-3, "{aspect}");
            assert!(map.closed && map.points[0].along == 0.0);
            let per_meter = map.points.len() as f32 / map.length;
            assert!((0.9..1.1).contains(&per_meter), "{per_meter}");
            let mirrored = track_map(&data, true, aspect);
            assert_eq!(mirrored.points.len(), map.points.len());
            for (a, b) in map.points.iter().zip(&mirrored.points) {
                assert!((a.at.x - (1.0 - b.at.x)).abs() < 1e-3 && (a.at.y - b.at.y).abs() < 1e-3);
                assert_eq!((a.along, a.zone), (b.along, b.zone));
            }
        }
    }

    /// Issue #181: the line has no hole: every pixel the centerline passes over is of the line
    /// (or of a mark on it), each next to the one before, on every circuit of the game, big and
    /// small, mirrored or not; closed for a circuit of laps (round from the start line back to
    /// it), open for a long race (from the start line to the finish line).
    #[test]
    fn the_line_is_continuous_closed_or_open() {
        for entry in crate::admin::tracks::circuits() {
            let data = circuit(&entry.file);
            for (mirror, big) in [(false, true), (true, true), (false, false)] {
                let (map, style, image) = drawn(&data, mirror, big);
                let name = format!("{} (mirror {mirror}, big {big})", entry.name);
                assert_eq!(map.closed, !entry.long, "{name}");
                assert!(bridges(&map).is_empty(), "{name}: no bridge yet");
                let pixels = walk(&map, &style);
                for (k, ((x, y), along)) in pixels.iter().enumerate() {
                    let kind = image.kind(*x, *y);
                    assert!(
                        matches!(kind, Kind::Line(_) | Kind::Mark),
                        "{name}: a hole at {along} m ({x}, {y}): {kind:?}"
                    );
                    let previous = pixels[if k == 0 { pixels.len() - 1 } else { k - 1 }].0;
                    let step = x.abs_diff(previous.0).max(y.abs_diff(previous.1));
                    // Round the loop, the last pixel is next to the first (closed); open, the
                    // finish is far from the start.
                    if k > 0 || map.closed {
                        assert!(step <= 1, "{name}: a jump at {along} m");
                    } else {
                        assert!(step as f32 > 8.0 * (style.line + style.edge), "{name}");
                    }
                }
                if let Some(open) = data.open.as_ref() {
                    let center = CenterLine::of(&data);
                    let finish = center.param_to_distance(open.finish);
                    assert!((map.points.last().unwrap().along - finish).abs() < 1e-3);
                    assert!(map.points.iter().all(|p| p.along <= finish));
                }
            }
        }
    }

    /// Issue #181: the line is as thick everywhere (its color twice `line`, give or take a pixel
    /// of its smoothed edge), measured across it wherever no other stretch of the road is near.
    #[test]
    fn the_line_keeps_its_thickness() {
        let data = circuit("col.track.ron");
        for big in [true, false] {
            let (map, style, image) = drawn(&data, false, big);
            let casing = style.line + style.edge;
            let pixels: Vec<Vec2> = map.points.iter().map(|p| to_pixels(&style, p.at)).collect();
            let per_meter = pixel_length(&map, &style) / map.length;
            let mut measured = 0;
            for i in (40..pixels.len() - 2).step_by(11) {
                let (at, forward) = (pixels[i], (pixels[i + 1] - pixels[i]).normalize());
                // On a straight stretch (across a tight turn, the line is wider).
                let reach = (2.0 * casing / per_meter).ceil() as usize + 1;
                let (Some(before), Some(after)) = (i.checked_sub(reach), pixels.get(i + reach))
                else {
                    continue;
                };
                let bend = (at - pixels[before])
                    .normalize()
                    .dot((*after - at).normalize());
                let alone = bend > 0.98
                    && pixels.iter().enumerate().all(|(j, p)| {
                        map.apart(map.points[j].along, map.points[i].along)
                            < 8.0 * casing / per_meter
                            || p.distance(at) > 4.0 * casing
                    });
                if !alone {
                    continue;
                }
                let across = forward.perp();
                let kinds: Vec<Kind> = (-200..=200)
                    .map(|k| at + across * (k as f32 * 0.05))
                    .map(|p| image.kind(p.x as u32, p.y as u32))
                    .collect();
                if kinds.contains(&Kind::Mark) {
                    continue;
                }
                let covered =
                    kinds.iter().filter(|k| matches!(k, Kind::Line(_))).count() as f32 * 0.05;
                assert!(
                    (covered - 2.0 * style.line).abs() <= 1.5,
                    "big {big}: {covered} px across at point {i}, not {}",
                    2.0 * style.line
                );
                measured += 1;
            }
            assert!(measured > if big { 20 } else { 5 }, "big {big}: {measured}");
        }
    }

    /// Issue #181: the mirror flips the picture left to right (the road's pixels, nearly all of
    /// them: the shadow falls on the same side).
    #[test]
    fn the_mirror_flips_the_picture() {
        for file in ["prairie.track.ron", "echappee.track.ron", "ruban.track.ron"] {
            let data = circuit(file);
            let (_, _, plain) = drawn(&data, false, true);
            let (_, _, mirrored) = drawn(&data, true, true);
            let (w, h) = (plain.width, plain.height);
            let (mut both, mut either) = (0, 0);
            for y in 0..h {
                for x in 0..w {
                    let a = matches!(plain.kind(x, y), Kind::Line(_));
                    let b = matches!(mirrored.kind(w - 1 - x, y), Kind::Line(_));
                    both += usize::from(a && b);
                    either += usize::from(a || b);
                }
            }
            let overlap = both as f32 / either as f32;
            assert!(overlap > 0.97, "{file}: {overlap}");
            assert_ne!(plain.pixels, mirrored.pixels, "{file}");
        }
    }

    /// Issue #181: where the road passes over itself, the road on top goes on (its color over the
    /// crossing, a white edge on both sides) and the road below stops under it (the edge of the
    /// road on top across it), then goes on beside it.
    #[test]
    fn the_road_below_a_bridge_stops_under_it() {
        let data = eight();
        for mirror in [false, true] {
            let (map, style, image) = drawn(&data, mirror, true);
            let found = bridges(&map);
            assert_eq!(found.len(), 1, "{found:?}");
            let (top, below) = found[0];
            let height = |along: f32| {
                let i = map.points.iter().position(|p| p.along >= along).unwrap();
                map.points[i].height
            };
            assert!(height(top) - height(below) > BRIDGE_HEIGHT);
            let casing = style.line + style.edge;
            // The road on top, in the picture.
            let on_top: Vec<Vec2> = map
                .points
                .iter()
                .filter(|p| map.apart(p.along, top) < NEAR_ALONG)
                .map(|p| to_pixels(&style, p.at))
                .collect();
            let (mut cut, mut beside) = (0, 0);
            for ((x, y), along) in &walk(&map, &style) {
                let kind = image.kind(*x, *y);
                let at = Vec2::new(*x as f32 + 0.5, *y as f32 + 0.5);
                let from_top = on_top
                    .iter()
                    .map(|p| p.distance(at))
                    .fold(f32::INFINITY, f32::min);
                if map.apart(*along, top) < NEAR_ALONG {
                    assert!(matches!(kind, Kind::Line(_)), "the road on top: {kind:?}");
                } else if map.apart(*along, below) < NEAR_ALONG {
                    // The road below: under the road on top and a white edge on each side of
                    // it, it stops; beside it, it goes on.
                    if from_top < casing + style.edge - 1.0 {
                        assert!(!matches!(kind, Kind::Below(_)), "{from_top}: {kind:?}");
                        cut += 1;
                    } else if from_top > casing + style.edge + 1.0 {
                        assert!(matches!(kind, Kind::Below(_) | Kind::Line(_)), "{kind:?}");
                        beside += usize::from(matches!(kind, Kind::Below(_)));
                    }
                } else {
                    assert!(
                        matches!(kind, Kind::Line(_) | Kind::Mark),
                        "{along}: {kind:?}"
                    );
                }
            }
            assert!(cut >= 10 && beside >= 4, "{cut} {beside}");
        }
    }

    /// Issue #181: the special stretches in color on the line: the anti-gravity (Prune), the
    /// water (Lagon), the glider (Mangue); the road in its biome's asphalt.
    #[test]
    fn the_special_stretches_are_colored() {
        let wanted = [
            ("ruban.track.ron", Zone::AntiGravity),
            ("lagon.track.ron", Zone::Water),
            ("lagon.track.ron", Zone::Glide),
            ("prairie.track.ron", Zone::Road),
        ];
        for (file, zone) in wanted {
            let data = circuit(file);
            let palette = data.biome().unwrap().palette;
            let colors = map_colors(&palette);
            let style = map_style(true);
            let map = track_map(&data, false, aspect(&style));
            assert!(
                map.points.iter().any(|p| p.zone == zone),
                "{file}: {zone:?}"
            );
            let image = render(&map, &style, &colors);
            let (w, h) = (image.width, image.height);
            // The pixels in the middle of the stretch's line (two pixels from anything else: the
            // smoothed edge of a mark on it): of its color.
            let mut seen = 0;
            for y in 2..h - 2 {
                for x in 2..w - 2 {
                    let inside = (0..25)
                        .all(|k| image.kind(x + k % 5 - 2, y + k / 5 - 2) == Kind::Line(zone));
                    if !inside {
                        continue;
                    }
                    let pixel = image.pixels[(y * w + x) as usize];
                    let want = colors.of(zone).map(|c| (c * 255.0).round());
                    for c in 0..3 {
                        assert!(
                            (pixel[c] as f32 - want[c]).abs() <= 2.0,
                            "{file}: {pixel:?}"
                        );
                    }
                    assert_eq!(pixel[3], 255);
                    seen += 1;
                }
            }
            assert!(seen > 30, "{file}: {zone:?} {seen}");
        }
        let col = map_colors(&circuit("col.track.ron").biome().unwrap().palette);
        let prairie = map_colors(&circuit("prairie.track.ron").biome().unwrap().palette);
        assert_ne!(col.road, prairie.road, "each biome its asphalt");
    }

    /// Issue #181: the start line is a checker on a circuit of laps (light and dark cells); a
    /// long race starts on a bar and finishes on a checker; the arrow after the start line points
    /// the way the race goes (a triangle: more of it behind its middle), mirrored too.
    #[test]
    fn the_start_the_finish_and_the_way() {
        let checked = |image: &MapImage, at: Vec2, radius: f32| {
            let (mut light, mut dark) = (0, 0);
            for y in (at.y - radius) as u32..=(at.y + radius) as u32 {
                for x in (at.x - radius) as u32..=(at.x + radius) as u32 {
                    if image.kind(x, y) != Kind::Mark {
                        continue;
                    }
                    let p = image.pixels[(y * image.width + x) as usize];
                    light += usize::from(p[0] > 230 && p[1] > 230 && p[2] > 230);
                    dark += usize::from(p[0] < 90 && p[2] < 140);
                }
            }
            (light, dark)
        };
        for (file, mirror) in [
            ("prairie.track.ron", false),
            ("prairie.track.ron", true),
            ("echappee.track.ron", false),
        ] {
            let data = circuit(file);
            let (map, style, image) = drawn(&data, mirror, true);
            let pixels: Vec<Vec2> = map.points.iter().map(|p| to_pixels(&style, p.at)).collect();
            let casing = style.line + style.edge;
            let (light, dark) = checked(&image, pixels[0], casing * 1.3);
            if map.closed {
                assert!(light > 15 && dark > 15, "{file}: a checker {light} {dark}");
            } else {
                assert!(
                    light > 30 && dark < light / 2,
                    "{file}: a bar {light} {dark}"
                );
                let finish = *pixels.last().unwrap();
                let (light, dark) = checked(&image, finish, casing * 1.3);
                assert!(light > 15 && dark > 15, "{file}: the finish {light} {dark}");
            }
            // The arrow: where the line has gone `arrow_after` from the start.
            let mut walked = 0.0;
            let i = (1..pixels.len())
                .find(|i| {
                    walked += pixels[*i].distance(pixels[i - 1]);
                    walked >= style.arrow_after
                })
                .unwrap();
            let forward = (pixels[i + 1] - pixels[i]).normalize();
            let (mut sum, mut count) = (Vec2::ZERO, 0.0);
            let r = style.arrow.x;
            for y in (pixels[i].y - r) as u32..=(pixels[i].y + r) as u32 {
                for x in (pixels[i].x - r) as u32..=(pixels[i].x + r) as u32 {
                    if image.kind(x, y) == Kind::Mark {
                        sum += Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        count += 1.0;
                    }
                }
            }
            assert!(count > 20.0, "{file}: the arrow {count}");
            let behind = (sum / count - pixels[i]).dot(forward);
            assert!(behind < -1.0, "{file}: the arrow points the way ({behind})");
        }
    }

    /// Issue #181: the picture is at twice the vignette's size (sharp), transparent around the
    /// line (the sky and the ground of the vignette show through).
    #[test]
    fn the_picture_is_twice_the_vignette() {
        let image = draw(&circuit("prairie.track.ron"), false, true);
        assert_eq!(
            (image.width, image.height),
            (
                (size::BIG_VIGNETTE_WIDTH * 2.0) as u32,
                (size::BIG_VIGNETTE_HEIGHT * 2.0) as u32
            )
        );
        assert_eq!(image.pixels[0], [0, 0, 0, 0]);
        assert_eq!(image.kind(0, 0), Kind::Empty);
        let small = draw(&circuit("prairie.track.ron"), false, false);
        assert_eq!(
            (small.width, small.height),
            (
                (size::VIGNETTE_WIDTH * 2.0) as u32,
                (size::VIGNETTE_HEIGHT * 2.0) as u32
            )
        );
        assert_eq!(
            to_image(&image).data.as_ref().map(Vec::len),
            Some(800 * 452 * 4)
        );
    }

    /// Writes the vignettes of every circuit (and the figure of eight) as raw RGB over their sky
    /// and ground, to look at them: `FLUFFY_VIGNETTES=/dir cargo test --lib write_the_vignettes
    /// -- --ignored`.
    #[test]
    #[ignore = "a tool: FLUFFY_VIGNETTES=/dir cargo test --lib write_the_vignettes -- --ignored"]
    fn write_the_vignettes() {
        let dir = std::env::var("FLUFFY_VIGNETTES").expect("FLUFFY_VIGNETTES=/a/folder");
        let mut all: Vec<(String, TrackData)> = crate::admin::tracks::circuits()
            .into_iter()
            .map(|entry| (entry.file.replace(".track.ron", ""), circuit(&entry.file)))
            .collect();
        all.push(("huit".into(), eight()));
        for (name, data) in all {
            for (mirror, big) in [(false, true), (true, true), (false, false)] {
                let started = std::time::Instant::now();
                let picture = draw(&data, mirror, big);
                let style = map_style(big);
                let map = track_map(&data, mirror, aspect(&style));
                let thinner = style.fitted(pixel_length(&map, &style)).line / style.line;
                println!(
                    "{name} {mirror} {big}: {:?}, line × {thinner:.2}",
                    started.elapsed()
                );
                let palette = data.biome().map_or_else(Palette::default, |b| b.palette);
                let file = format!("{dir}/{name}-{}{}", u8::from(big), u8::from(mirror));
                write_rgb(&file, &picture, &palette);
            }
        }
    }

    /// A picture over the vignette's sky and ground, as raw RGB (`<name>-<w>x<h>.rgb`).
    fn write_rgb(file: &str, picture: &MapImage, palette: &Palette) {
        let (w, h) = (picture.width as usize, picture.height as usize);
        let mut raw = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            let t = y as f32 / h as f32;
            let (sky, ground) = (palette.sky, palette.ground);
            let mix = |a: f32, b: f32| {
                if t < 0.28 {
                    a + (b - a) * t / 0.28
                } else {
                    b * (1.0 - 0.14 * (t - 0.28) / 0.72)
                }
            };
            let back = [
                mix(sky.0, ground.0),
                mix(sky.1, ground.1),
                mix(sky.2, ground.2),
            ];
            for x in 0..w {
                let p = picture.pixels[y * w + x];
                let a = p[3] as f32 / 255.0;
                for c in 0..3 {
                    raw.push((p[c] as f32 * a + back[c] * 255.0 * (1.0 - a)).round() as u8);
                }
            }
        }
        std::fs::write(format!("{file}-{w}x{h}.rgb"), raw).unwrap();
    }
}
