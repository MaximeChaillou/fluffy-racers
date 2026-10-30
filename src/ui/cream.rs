//! Issue #194: **the chantilly's cream on the screen**, drawn (GD § 8 « Chantilly », § 14 « La
//! crème sur l'écran (#194) »). Only with a renderer ([`CreamPlugin`], added by `GamePlugin` when
//! there is a `RenderPlugin`; the tests add it by hand): the headless HUD keeps its seven plain
//! blobs (`ui::CreamOverlay`), the bot races every archetype they were measured with.
//!
//! - **Splats of whipped cream**, drawn by the game at startup (pure, unit-tested: no file, no
//!   crate) in **one texture** shared by every piece (one material, one batch): three splats with
//!   a wavy, lumpy outline, a piped swirl and its peak in their middle, a few splashed arms, drips
//!   hanging under them; lit like a soft relief (a warm shade, glossy crests, small bubbles, an
//!   edge a little see-through) over a soft drop shadow; a cherry, two droplets and the smear a
//!   splat leaves when it slides.
//! - **The arrival**: each splat hits the screen one after the other (a few hundredths apart),
//!   squashed wide, then bounces back to its size (a damped spring); its droplets fly out of it
//!   and stick; the cherry falls on the biggest one and bounces on it. The squelch is the
//!   sound's (`SoundEffect::Creamed`, #103).
//! - **Its life**: the splats slide slowly down the screen, faster and faster, leaving their
//!   smear above them, and stretch a little as they fade out in the last second (the GD's rule:
//!   4 s, fading in the last one, `ItemEffects::cream`).
//! - **The view stays**: the splats sit around the edges and the sky, only two small ones in the
//!   driving window; a test bounds what they hide of it, all along ([`opacity_at`]).
//! - **Light**: they move with `UiTransform` and their tint only (nothing laid out again while
//!   they move), in a fixed array (nothing allocated per frame); nothing runs without cream.

use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::{CreamOverlay, followed};
use crate::admin::Tunings;
use crate::kart::{ItemEffects, PlayerKart};
use crate::race::{Race, RaceProgress, Racer};

/// The cream drawn on the screen, with a renderer.
pub struct CreamPlugin;

impl Plugin for CreamPlugin {
    fn build(&self, app: &mut App) {
        // The texture is an image (the headless tests have no renderer's storage of images: made
        // here).
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        app.init_resource::<CreamTuning>()
            .add_systems(Startup, create_cream)
            .add_systems(
                Update,
                (dress_cream, animate_cream)
                    .chain()
                    .after(super::show_cream)
                    .run_if(resource_exists::<Race>)
                    .run_if(resource_exists::<CreamAssets>),
            );
        if let Some(mut tunings) = app.world_mut().get_resource_mut::<Tunings>() {
            tunings.register::<CreamTuning>();
        }
    }
}

/// How the cream moves on the screen (admin panel, « à régler »). Purely visual: the rule (4 s,
/// fading in the last second) is `ItemTuning::chantilly_time`'s and `ItemEffects::cream`'s.
/// Times in s, distances in % of the screen's height (`vh`).
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct CreamTuning {
    /// The splats' opacity at full thickness (0 to 1).
    pub opacity: f32,
    /// A splat's size when it hits the screen (× its size), before it bounces back.
    pub impact_scale: f32,
    /// How wide (and flat) a splat is squashed when it hits (× its size, more and less).
    pub squash: f32,
    /// Its bounce: how fast it wobbles (rad/s) and how fast that dies down (1/s).
    pub bounce_frequency: f32,
    pub bounce_damping: f32,
    /// The time between two splats hitting the screen.
    pub stagger: f32,
    /// When the splats start sliding (after their hit).
    pub slide_after: f32,
    /// How far they slide (× each splat's own distance, 4 to 9 vh).
    pub slide: f32,
    /// How much they stretch down as they fade out (× their height).
    pub droop: f32,
    /// The smear they leave above them (opacity, 0 to 1).
    pub trail_opacity: f32,
    /// How far they fall more in their last second, as they fade (vh).
    pub end_fall: f32,
    /// How far their droplets fly (× each droplet's own distance).
    pub droplet_reach: f32,
    /// How high the cherry falls from (vh) before it bounces on its splat.
    pub cherry_drop: f32,
}

impl Default for CreamTuning {
    fn default() -> Self {
        Self {
            opacity: 0.97,
            impact_scale: 0.35,
            squash: 0.3,
            bounce_frequency: 17.0,
            bounce_damping: 8.0,
            stagger: 0.045,
            slide_after: 0.45,
            slide: 1.0,
            droop: 0.1,
            trail_opacity: 0.75,
            end_fall: 6.0,
            droplet_reach: 1.0,
            cherry_drop: 11.0,
        }
    }
}

// --- The texture --------------------------------------------------------------------------------

/// A splat's cell (px, square).
pub const SPLAT_CELL: u32 = 384;
/// The small cells under the splats (px, square): the cherry, two droplets, the smear.
pub const SMALL_CELL: u32 = 128;
/// The number of splat drawings.
pub const SPLAT_VARIANTS: u8 = 3;
/// The texture's size (px): the three splats side by side, the small cells under them.
pub const ATLAS: (u32, u32) = (SPLAT_CELL * 3, SPLAT_CELL + SMALL_CELL);

/// A drawing of the texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Splat(u8),
    Cherry,
    Droplet(u8),
    Trail,
}

impl Cell {
    /// Its rectangle in the texture (px: x, y, width, height).
    pub fn rect(self) -> (u32, u32, u32, u32) {
        match self {
            Cell::Splat(v) => (
                u32::from(v % SPLAT_VARIANTS) * SPLAT_CELL,
                0,
                SPLAT_CELL,
                SPLAT_CELL,
            ),
            Cell::Cherry => (0, SPLAT_CELL, SMALL_CELL, SMALL_CELL),
            Cell::Droplet(v) => (
                (1 + u32::from(v % 2)) * SMALL_CELL,
                SPLAT_CELL,
                SMALL_CELL,
                SMALL_CELL,
            ),
            Cell::Trail => (3 * SMALL_CELL, SPLAT_CELL, SMALL_CELL, SMALL_CELL),
        }
    }
}

/// A number from 0 to 1 for the corner (`x`, `y`) of the noise's grid.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_215.0
}

/// A number from 0 to 1, for the `n`-th pick of `seed`.
fn pick(seed: u32, n: u32) -> f32 {
    hash(n as i32, 7, seed)
}

/// Value noise (0 to 1), smooth.
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (s(x - xi), s(y - yi));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    let top = a + (b - a) * fx;
    let bottom = c + (d - c) * fx;
    top + (bottom - top) * fy
}

/// Fractal noise (0 to 1), `octaves` of it.
fn fbm(mut x: f32, mut y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += amplitude * noise(x, y, seed.wrapping_add(octave * 131));
        total += amplitude;
        amplitude *= 0.5;
        x = x * 2.07 + 13.1;
        y = y * 2.07 + 5.3;
    }
    sum / total
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The smooth union of two distances (`k`: how round the join is).
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

/// The distance to the segment `a`-`b`, and where along it (0 to 1).
fn segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let (pa, ba) = (p - a, b - a);
    let h = (pa.dot(ba) / ba.length_squared()).clamp(0.0, 1.0);
    ((pa - ba * h).length(), h)
}

/// The shapes of a splat drawing (cell coordinates, -1 to 1, y down): its main dollop, the lumps
/// around it, the tongues of cream splashed out of it, the specks flung further, its drips, its
/// bubbles. Picked from its seed once.
struct SplatShape {
    seed: u32,
    center: Vec2,
    radius: f32,
    lumps: [(Vec2, f32); 11],
    /// From, to, width at the root.
    tongues: [(Vec2, Vec2, f32); 5],
    specks: [(Vec2, f32); 5],
    /// x, from y, to y, width.
    drips: [(f32, f32, f32, f32); 3],
    bubbles: [(Vec2, f32); 12],
    /// The swirl's turn (how its ridges wind).
    twist: f32,
}

impl SplatShape {
    fn new(variant: u8) -> Self {
        let seed = 0xC4EA + u32::from(variant) * 7_919;
        let r = |n: u32| pick(seed, n);
        let center = Vec2::new(0.06 * (r(1) - 0.5), -0.1 + 0.05 * (r(2) - 0.5));
        let radius = 0.44 + 0.05 * r(3);
        let mut lumps = [(Vec2::ZERO, 0.0); 11];
        let start = r(4) * TAU;
        for (i, lump) in lumps.iter_mut().enumerate() {
            let i = i as u32;
            let angle = start + i as f32 * TAU / 11.0 + 0.4 * (r(10 + i) - 0.5);
            let ring = radius * (0.78 + 0.2 * r(20 + i));
            let size = 0.1 + 0.1 * r(30 + i);
            *lump = (
                center + Vec2::new(angle.cos(), angle.sin() * 0.9) * ring,
                size,
            );
        }
        // Tongues of cream splashed out all around but straight down (the drips hang there):
        // thick at the root, tapering, a teardrop at the end.
        let mut tongues = [(Vec2::ZERO, Vec2::ZERO, 0.0); 5];
        for (i, tongue) in tongues.iter_mut().enumerate() {
            let i = i as u32;
            let angle = -PI * 1.15 + PI * 1.3 * ((i as f32 + 0.2 + 0.6 * r(40 + i)) / 5.0);
            let dir = Vec2::new(angle.cos(), angle.sin());
            let reach = radius + 0.12 + 0.2 * r(50 + i);
            *tongue = (
                center + dir * radius * 0.6,
                center + dir * reach,
                0.07 + 0.05 * r(60 + i),
            );
        }
        // Specks flung beyond them, apart.
        let mut specks = [(Vec2::ZERO, 0.0); 5];
        for (i, speck) in specks.iter_mut().enumerate() {
            let i = i as u32;
            let angle = -PI * 1.1 + PI * 1.2 * ((i as f32 + r(150 + i)) / 5.0);
            let at = (radius + 0.22 + 0.1 * r(160 + i)).min(0.78);
            *speck = (
                center + Vec2::new(angle.cos(), angle.sin()) * at,
                0.025 + 0.025 * r(170 + i),
            );
        }
        let mut drips = [(0.0, 0.0, 0.0, 0.0); 3];
        for (i, drip) in drips.iter_mut().enumerate() {
            let i = i as u32;
            let x = center.x + radius * (-0.55 + 0.55 * i as f32 + 0.25 * (r(70 + i) - 0.5));
            let from = center.y + radius * 0.5;
            let to = center.y + radius + 0.12 + 0.28 * r(80 + i);
            *drip = (x, from, to.min(0.74), 0.055 + 0.03 * r(90 + i));
        }
        let mut bubbles = [(Vec2::ZERO, 0.0); 12];
        for (i, bubble) in bubbles.iter_mut().enumerate() {
            let i = i as u32;
            let angle = r(100 + i) * TAU;
            let at = radius * (0.3 + 0.75 * r(120 + i));
            *bubble = (
                center + Vec2::new(angle.cos(), angle.sin()) * at,
                0.008 + 0.014 * r(140 + i),
            );
        }
        Self {
            seed,
            center,
            radius,
            lumps,
            tongues,
            specks,
            drips,
            bubbles,
            twist: 7.0 + 4.0 * r(5),
        }
    }

    /// The signed distance to its edge (negative inside).
    fn distance(&self, p: Vec2) -> f32 {
        let wobble = 0.04 * (fbm(p.x * 3.0 + 4.0, p.y * 3.0 + 9.0, self.seed, 3) - 0.5);
        let mut d = (p - self.center).length() - self.radius;
        for (at, size) in self.lumps {
            d = smin(d, (p - at).length() - size, 0.08);
        }
        for (from, to, width) in self.tongues {
            let (dist, h) = segment(p, from, to);
            // Tapering, then a teardrop at its end.
            let tongue = dist - width * (1.0 - 0.55 * h);
            let drop = (p - to).length() - width * 0.75;
            d = smin(d, smin(tongue, drop, 0.05), 0.09);
        }
        for (x, from, to, width) in self.drips {
            let (dist, h) = segment(p, Vec2::new(x, from), Vec2::new(x, to));
            let drip = dist - width * (1.0 - 0.3 * h);
            let drop = (p - Vec2::new(x, to)).length() - width * 1.3;
            d = smin(d, smin(drip, drop, 0.06), 0.1);
        }
        for (at, size) in self.specks {
            d = d.min((p - at).length() - size);
        }
        d + wobble
    }

    /// Its height above the screen at `p`, `depth` inside its edge: a rounded edge, a soft top,
    /// the piped swirl and its peak on the main dollop, small bumps; and how far from a crease
    /// (0 in one, 1 on a crest).
    fn height(&self, p: Vec2, depth: f32) -> (f32, f32) {
        let body = 1.0 - (1.0 - (depth / 0.2).clamp(0.0, 1.0)).powi(2);
        let q = p - self.center;
        let r = q.length() / self.radius;
        let angle = q.y.atan2(q.x) + 1.6 * (fbm(p.x * 2.0, p.y * 2.0, self.seed ^ 0x99, 2) - 0.5);
        let ridge = 0.5 + 0.5 * (3.0 * angle + self.twist * r).cos();
        let on_swirl = 1.0 - smoothstep(0.3, 0.95, r);
        let peak = (1.0 - smoothstep(0.0, 0.45, r)).powi(2);
        let bumps = fbm(p.x * 6.0, p.y * 6.0, self.seed ^ 0x55, 3);
        let height =
            0.34 * body + body * (0.09 * on_swirl * ridge + 0.2 * peak) + 0.05 * body * bumps;
        (height, on_swirl * ridge + (1.0 - on_swirl))
    }
}

/// The light on the cream: from the top left, toward the viewer.
fn light() -> Vec3 {
    Vec3::new(-0.5, -0.72, 0.9).normalize()
}

/// The cream's colors (sRGB): lit, in its shade (warm), the shadow it casts.
const CREAM_LIT: Vec3 = Vec3::new(1.0, 0.995, 0.975);
const CREAM_SHADE: Vec3 = Vec3::new(0.87, 0.8, 0.72);
const CREAM_EDGE: Vec3 = Vec3::new(1.0, 0.95, 0.86);
const DROP_SHADOW: Vec3 = Vec3::new(0.22, 0.15, 0.1);

/// The cream lit at a point of `normal`, `crease` (0 in a crease, 1 on a crest) and `depth` inside
/// its edge (sRGB, 0 to 1).
fn shade_cream(normal: Vec3, crease: f32, depth: f32) -> Vec3 {
    let l = light();
    let wrap = ((normal.dot(l) + 0.35) / 1.35).clamp(0.0, 1.0);
    let half = (l + Vec3::Z).normalize();
    let nh = normal.dot(half).max(0.0);
    let gloss = 0.75 * nh.powi(48) + 0.1 * nh.powi(8);
    let occlusion = 0.86 + 0.14 * crease;
    let mut color = CREAM_SHADE.lerp(CREAM_LIT, wrap.powf(0.8)) * occlusion;
    // Thin at its edge: a little warmer, the light through it.
    color = color.lerp(CREAM_EDGE, 0.35 * (1.0 - smoothstep(0.0, 0.05, depth)));
    (color + Vec3::splat(gloss)).min(Vec3::ONE)
}

/// "Over" for straight alpha: `top` over `under`.
fn over(top: (Vec3, f32), under: (Vec3, f32)) -> (Vec3, f32) {
    let alpha = top.1 + under.1 * (1.0 - top.1);
    if alpha <= 1e-4 {
        // Nothing: the cream's shade (no dark fringe when the texture is filtered).
        return (CREAM_SHADE, 0.0);
    }
    let color = (top.0 * top.1 + under.0 * under.1 * (1.0 - top.1)) / alpha;
    (color, alpha)
}

/// The drop shadow's offset (cell units: down and right, away from the light), its blur, its
/// opacity.
const SHADOW_OFFSET: Vec2 = Vec2::new(0.035, 0.055);
const SHADOW_BLUR: f32 = 0.06;
const SHADOW_ALPHA: f32 = 0.3;

/// A splat drawing: its pixels (sRGB, straight alpha) into `out`, a `SPLAT_CELL` square.
fn draw_splat(variant: u8, out: &mut [[f32; 4]]) {
    let shape = SplatShape::new(variant);
    let n = SPLAT_CELL as usize;
    let texel = 2.0 / n as f32;
    let point = |x: usize, y: usize| {
        Vec2::new(
            (x as f32 + 0.5) * texel - 1.0,
            (y as f32 + 0.5) * texel - 1.0,
        )
    };
    // The distances and heights first (one evaluation a texel), the normals from them.
    let mut distance = vec![0.0f32; n * n];
    let mut height = vec![0.0f32; n * n];
    let mut crease = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let p = point(x, y);
            let d = shape.distance(p);
            distance[y * n + x] = d;
            if d < 0.0 {
                let (h, c) = shape.height(p, -d);
                height[y * n + x] = h;
                crease[y * n + x] = c;
            }
        }
    }
    for y in 0..n {
        for x in 0..n {
            let i = y * n + x;
            let p = point(x, y);
            let d = distance[i];
            let depth = -d;
            let at = |dx: isize, dy: isize| {
                let (xx, yy) = (
                    (x as isize + dx).clamp(0, n as isize - 1) as usize,
                    (y as isize + dy).clamp(0, n as isize - 1) as usize,
                );
                height[yy * n + xx]
            };
            let gx = (at(1, 0) - at(-1, 0)) / (2.0 * texel);
            let gy = (at(0, 1) - at(0, -1)) / (2.0 * texel);
            let normal = Vec3::new(-gx, -gy, 1.0).normalize();
            let mut color = shade_cream(normal, crease[i], depth);
            // Small bubbles: a darker hollow, a glint on their far rim.
            for (center, radius) in shape.bubbles {
                let q = (p - center) / radius;
                let r = q.length();
                if r < 1.25 && depth > 0.03 {
                    let inside = 1.0 - smoothstep(0.8, 1.0, r);
                    color *= 1.0 - 0.08 * inside;
                    let rim = smoothstep(0.55, 0.85, r) * (1.0 - smoothstep(0.95, 1.2, r));
                    let lit = smoothstep(0.0, 0.7, q.normalize_or_zero().dot(Vec2::new(0.6, 0.8)));
                    color += Vec3::splat(0.35 * rim * lit);
                    let glint = 1.0 - smoothstep(0.0, 0.28, (q - Vec2::new(-0.35, -0.35)).length());
                    color += Vec3::splat(0.5 * glint);
                }
            }
            // Its edge, smooth over a texel and a half, a little see-through where it is thin.
            let alpha = smoothstep(-texel * 0.75, texel * 0.75, depth)
                * (0.86 + 0.14 * smoothstep(0.0, 0.05, depth));
            let shadow_d = shape.distance(p - SHADOW_OFFSET);
            let shadow = SHADOW_ALPHA * (1.0 - smoothstep(-SHADOW_BLUR, SHADOW_BLUR, shadow_d));
            // Nothing near its cell's edge.
            let fade = 1.0 - smoothstep(0.93, 0.995, p.x.abs().max(p.y.abs()));
            let (c, a) = over((color.min(Vec3::ONE), alpha), (DROP_SHADOW, shadow));
            out[i] = [c.x, c.y, c.z, a * fade];
        }
    }
}

/// The cherry: a glossy red ball, its dimple, its stem, its soft shadow (a `SMALL_CELL` square).
fn draw_cherry(out: &mut [[f32; 4]]) {
    let n = SMALL_CELL as usize;
    let texel = 2.0 / n as f32;
    let center = Vec2::new(0.0, 0.2);
    let radius = 0.6;
    let stem_from = Vec2::new(0.0, center.y - radius * 0.72);
    let stem_ctrl = Vec2::new(0.06, -0.78);
    let stem_to = Vec2::new(0.4, -0.9);
    let l = light();
    for y in 0..n {
        for x in 0..n {
            let p = Vec2::new(
                (x as f32 + 0.5) * texel - 1.0,
                (y as f32 + 0.5) * texel - 1.0,
            );
            // The ball, a dimple at its top.
            let q = p - center;
            let dimple =
                0.09 * (1.0 - smoothstep(0.0, 0.3, q.x.abs())) * smoothstep(0.0, -0.4, q.y);
            let d = q.length() - radius + dimple;
            let mut color = Vec3::ZERO;
            let mut alpha = smoothstep(-texel, texel, -d);
            if alpha > 0.0 {
                let s = (q / radius).clamp_length_max(0.999);
                let normal = Vec3::new(s.x, s.y, (1.0 - s.length_squared()).sqrt());
                let diffuse = ((normal.dot(l) + 0.2) / 1.2).clamp(0.0, 1.0);
                let half = (l + Vec3::Z).normalize();
                let nh = normal.dot(half).max(0.0);
                let dark = Vec3::new(0.42, 0.0, 0.06);
                let red = Vec3::new(0.93, 0.08, 0.16);
                color = dark.lerp(red, diffuse);
                // The cream's light bounced from under it, and its glossy skin.
                color += Vec3::new(0.25, 0.12, 0.1) * smoothstep(0.3, 0.95, s.y);
                color += Vec3::splat(0.95 * nh.powi(70) + 0.15 * nh.powi(10));
            }
            // The stem (in front of the dimple).
            let mut stem = f32::MAX;
            let mut last = stem_from;
            for k in 1..=12 {
                let t = k as f32 / 12.0;
                let b = stem_from * (1.0 - t) * (1.0 - t)
                    + stem_ctrl * 2.0 * t * (1.0 - t)
                    + stem_to * t * t;
                stem = stem.min(segment(p, last, b).0);
                last = b;
            }
            let stem_alpha = smoothstep(texel, -texel, stem - 0.045);
            if stem_alpha > 0.0 {
                let lit = 0.6 + 0.4 * smoothstep(0.045, -0.045, stem);
                let stem_color = Vec3::new(0.36, 0.42, 0.12) * lit;
                color = color.lerp(stem_color, stem_alpha);
                alpha = alpha.max(stem_alpha);
            }
            let shadow_d = (p - center - Vec2::new(0.07, 0.1)).length() - radius;
            let shadow = 0.35 * (1.0 - smoothstep(-0.12, 0.12, shadow_d));
            let fade = 1.0 - smoothstep(0.92, 0.995, p.x.abs().max(p.y.abs()));
            let (c, a) = over((color.min(Vec3::ONE), alpha), (DROP_SHADOW, shadow));
            out[y * n + x] = [c.x, c.y, c.z, a * fade];
        }
    }
}

/// A droplet: a small lumpy dollop of cream, lit like the splats (a `SMALL_CELL` square).
fn draw_droplet(variant: u8, out: &mut [[f32; 4]]) {
    let n = SMALL_CELL as usize;
    let texel = 2.0 / n as f32;
    let seed = 0xD2 + u32::from(variant) * 31;
    let tail = if variant == 0 {
        Vec2::new(-0.2, -0.45)
    } else {
        Vec2::new(0.25, -0.4)
    };
    let distance = |p: Vec2| {
        let warp = 0.08 * (fbm(p.x * 2.5 + 3.0, p.y * 2.5, seed, 3) - 0.5);
        let ball = (p - Vec2::new(0.0, 0.1)).length() - 0.5;
        let (dist, h) = segment(p, Vec2::new(0.0, 0.1), tail);
        smin(ball, dist - 0.2 * (1.0 - 0.7 * h), 0.12) + warp
    };
    for y in 0..n {
        for x in 0..n {
            let p = Vec2::new(
                (x as f32 + 0.5) * texel - 1.0,
                (y as f32 + 0.5) * texel - 1.0,
            );
            let d = distance(p);
            let depth = -d;
            let mut color = CREAM_SHADE;
            let alpha = smoothstep(-texel, texel, depth);
            if alpha > 0.0 {
                // A dome: its normal from the distance's slope.
                let e = texel;
                let g = Vec2::new(
                    distance(p + Vec2::X * e) - distance(p - Vec2::X * e),
                    distance(p + Vec2::Y * e) - distance(p - Vec2::Y * e),
                ) / (2.0 * e);
                let rise = (depth / 0.3).clamp(0.0, 1.0);
                let tilt = (1.0 - rise).powf(1.5) * 2.2;
                let normal = Vec3::new(g.x * tilt, g.y * tilt, 1.0).normalize();
                color = shade_cream(normal, 1.0, depth);
            }
            let shadow_d = distance(p - Vec2::new(0.06, 0.1));
            let shadow = SHADOW_ALPHA * (1.0 - smoothstep(-0.1, 0.1, shadow_d));
            let fade = 1.0 - smoothstep(0.92, 0.995, p.x.abs().max(p.y.abs()));
            let (c, a) = over((color, alpha), (DROP_SHADOW, shadow));
            out[y * n + x] = [c.x, c.y, c.z, a * fade];
        }
    }
}

/// The smear a splat leaves as it slides: a glossy, streaked band, see-through, soft at its top
/// (a `SMALL_CELL` square, stretched to its length on screen).
fn draw_trail(out: &mut [[f32; 4]]) {
    let n = SMALL_CELL as usize;
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let streak = fbm(u * 3.5 + 2.0, v * 0.6, 0x7A11, 3);
            let width = 0.82 + 0.12 * (fbm(1.0, v * 1.3, 0x7A12, 2) - 0.5);
            let across = 1.0 - smoothstep(width - 0.3, width, u.abs());
            let top = smoothstep(-1.0, -0.45, v) * (1.0 - smoothstep(0.85, 0.99, v));
            let alpha = across * top * (0.45 + 0.55 * streak);
            let gloss = (1.0 - smoothstep(0.0, 0.12, (u + 0.35).abs())) * 0.4
                + (1.0 - smoothstep(0.0, 0.07, (u - 0.3).abs())) * 0.2;
            let color = CREAM_SHADE.lerp(CREAM_LIT, 0.55 + 0.45 * streak) + Vec3::splat(gloss);
            out[y * n + x] = [color.x.min(1.0), color.y.min(1.0), color.z.min(1.0), alpha];
        }
    }
}

/// The texture, RGBA (8 bits a channel, **sRGB**, straight alpha), as the GPU takes it.
pub fn cream_pixels() -> Vec<u8> {
    let (w, h) = ATLAS;
    let mut atlas = vec![[0.0f32; 4]; (w * h) as usize];
    let mut blit = |cell: Cell, pixels: &[[f32; 4]]| {
        let (x0, y0, cw, ch) = cell.rect();
        for y in 0..ch {
            for x in 0..cw {
                atlas[((y0 + y) * w + x0 + x) as usize] = pixels[(y * cw + x) as usize];
            }
        }
    };
    let mut splat = vec![[0.0f32; 4]; (SPLAT_CELL * SPLAT_CELL) as usize];
    for variant in 0..SPLAT_VARIANTS {
        draw_splat(variant, &mut splat);
        blit(Cell::Splat(variant), &splat);
    }
    let mut small = vec![[0.0f32; 4]; (SMALL_CELL * SMALL_CELL) as usize];
    draw_cherry(&mut small);
    blit(Cell::Cherry, &small);
    for variant in 0..2 {
        draw_droplet(variant, &mut small);
        blit(Cell::Droplet(variant), &small);
    }
    draw_trail(&mut small);
    blit(Cell::Trail, &small);
    atlas
        .iter()
        .flat_map(|texel| texel.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
        .collect()
}

fn cream_image() -> Image {
    let (width, height) = ATLAS;
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        cream_pixels(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

// --- The layout and its animation ----------------------------------------------------------------

/// A splat on the screen: its middle (x in % of the width, y in % of the height), its size (vh),
/// its drawing (and mirrored), its turn (rad), how far it slides (vh), its rank in the hits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splat {
    pub at: Vec2,
    pub size: f32,
    pub variant: u8,
    pub flip: bool,
    pub turn: f32,
    pub slide: f32,
    pub order: u8,
}

/// The splats: big ones around the edges and in the sky, two small ones in the driving window
/// (it hinders without blinding: the road ahead and the kart stay in sight). The first one has
/// the cherry.
pub const SPLATS: [Splat; 8] = [
    splat(12.0, 43.0, 60.0, 0, false, -0.1, 9.0, 0),
    splat(88.0, 60.0, 52.0, 1, true, 0.12, 7.5, 2),
    splat(63.0, 11.0, 40.0, 2, false, 0.18, 6.0, 4),
    splat(32.0, 8.0, 30.0, 1, false, -0.15, 5.0, 1),
    splat(22.0, 90.0, 42.0, 2, true, 0.08, 4.0, 6),
    splat(92.0, 22.0, 34.0, 0, true, -0.16, 6.5, 3),
    splat(39.0, 62.0, 22.0, 2, false, 0.2, 5.0, 7),
    splat(56.0, 27.0, 21.0, 0, true, -0.08, 4.0, 5),
];

#[allow(clippy::too_many_arguments)]
const fn splat(
    x: f32,
    y: f32,
    size: f32,
    variant: u8,
    flip: bool,
    turn: f32,
    slide: f32,
    order: u8,
) -> Splat {
    Splat {
        at: Vec2::new(x, y),
        size,
        variant,
        flip,
        turn,
        slide,
        order,
    }
}

/// Droplets flung out of each splat.
pub const DROPLETS_PER_SPLAT: usize = 2;
/// Every piece on the screen: a smear and droplets per splat, the splats, the cherry.
pub const PIECES: usize = SPLATS.len() * (2 + DROPLETS_PER_SPLAT) + 1;
/// The cherry: where on its splat (× its size, from its middle, before its turn), its size (×).
const CHERRY_ON: Vec2 = Vec2::new(0.02, -0.15);
const CHERRY_SIZE: f32 = 0.22;
/// The smear: its width (× its splat's size), where it starts (× the size, above the middle).
const TRAIL_WIDTH: f32 = 0.55;
const TRAIL_FROM: f32 = 0.28;

/// What a piece is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Trail(u8),
    Droplet(u8, u8),
    Splat(u8),
    Cherry,
}

/// A piece of the cream, as laid out once: where its middle is at rest (x in % of the width, y in
/// % of the height), its node's size (vh), its drawing, mirrored or not.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CreamPiece {
    pub kind: PieceKind,
    pub at: Vec2,
    pub size: Vec2,
    pub cell: Cell,
    pub flip: bool,
}

/// A droplet of `splat`: its direction, how far it flies (vh) and its size (vh).
fn droplet(splat: &Splat, k: usize) -> (Vec2, f32, f32) {
    let i = splat.order as f32;
    let angle = splat.turn * 3.0 + k as f32 * 2.2 + i * 0.9 + 0.4;
    let dir = Vec2::new(angle.cos(), angle.sin());
    let reach = splat.size * (0.4 + 0.08 * k as f32);
    let size = 2.8 + ((splat.order as usize * 3 + k) % 3) as f32 * 0.9;
    (dir, reach, size)
}

/// The longest smear of `splat` (vh): its slide, its droop and where it starts.
fn trail_length(splat: &Splat, tuning: &CreamTuning) -> f32 {
    splat.slide * tuning.slide.max(0.0)
        + tuning.end_fall.max(0.0)
        + splat.size * (TRAIL_FROM + tuning.droop.max(0.0) * 0.5)
}

/// Every piece, in the order they are drawn (the smears under, the cherry on top).
pub fn pieces(tuning: &CreamTuning) -> [CreamPiece; PIECES] {
    let mut out = [CreamPiece {
        kind: PieceKind::Cherry,
        at: Vec2::ZERO,
        size: Vec2::ZERO,
        cell: Cell::Cherry,
        flip: false,
    }; PIECES];
    let mut n = 0;
    for (i, splat) in SPLATS.iter().enumerate() {
        let length = trail_length(splat, tuning);
        out[n] = CreamPiece {
            kind: PieceKind::Trail(i as u8),
            // Its top where the splat's top starts (moved by the pose), its middle half its
            // length under.
            at: splat.at,
            size: Vec2::new(splat.size * TRAIL_WIDTH, length),
            cell: Cell::Trail,
            flip: splat.flip,
        };
        n += 1;
    }
    for (i, splat) in SPLATS.iter().enumerate() {
        for k in 0..DROPLETS_PER_SPLAT {
            let (_, _, size) = droplet(splat, k);
            out[n] = CreamPiece {
                kind: PieceKind::Droplet(i as u8, k as u8),
                at: splat.at,
                size: Vec2::splat(size),
                cell: Cell::Droplet(((i + k) % 2) as u8),
                flip: (i + k) % 3 == 0,
            };
            n += 1;
        }
    }
    for (i, splat) in SPLATS.iter().enumerate() {
        out[n] = CreamPiece {
            kind: PieceKind::Splat(i as u8),
            at: splat.at,
            size: Vec2::splat(splat.size),
            cell: Cell::Splat(splat.variant),
            flip: splat.flip,
        };
        n += 1;
    }
    out[n] = CreamPiece {
        kind: PieceKind::Cherry,
        at: SPLATS[0].at,
        size: Vec2::splat(SPLATS[0].size * CHERRY_SIZE),
        cell: Cell::Cherry,
        flip: false,
    };
    out
}

/// Where a piece is at a moment: moved from its place at rest (vh, y down), scaled, turned
/// (rad, clockwise), its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub offset: Vec2,
    pub scale: Vec2,
    pub turn: f32,
    pub alpha: f32,
}

impl Pose {
    pub const HIDDEN: Self = Self {
        offset: Vec2::ZERO,
        scale: Vec2::ZERO,
        turn: 0.0,
        alpha: 0.0,
    };
}

/// The moment of the cream: `elapsed` s since it hit, `remaining` s of it, of `duration`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreamClock {
    pub elapsed: f32,
    pub remaining: f32,
    pub duration: f32,
}

impl CreamClock {
    /// Its thickness (0 to 1): full, fading out in its last second (the GD's rule,
    /// `ItemEffects::cream`).
    pub fn thickness(&self) -> f32 {
        self.remaining.clamp(0.0, 1.0)
    }

    /// The pieces' opacity (0 to 1, × `CreamTuning::opacity`): it follows the thickness, a little
    /// rounder (a cream half gone still reads as cream, not as a grey veil).
    pub fn fade(&self) -> f32 {
        self.thickness().powf(0.6)
    }
}

/// A splat's own pose (no smear, no droplet).
fn splat_pose(splat: &Splat, clock: &CreamClock, tuning: &CreamTuning) -> Pose {
    let t = clock.elapsed - f32::from(splat.order) * tuning.stagger;
    let thickness = clock.thickness();
    if t < 0.0 || thickness <= 0.0 {
        return Pose::HIDDEN;
    }
    // The hit: from small, squashed wide, a damped spring back to its size.
    let spring = (-tuning.bounce_damping * t).exp();
    let grow = 1.0 - (1.0 - tuning.impact_scale) * spring * (tuning.bounce_frequency * t).cos();
    let squash = tuning.squash
        * (-tuning.bounce_damping * 1.3 * t).exp()
        * (tuning.bounce_frequency * 1.2 * t).cos();
    // The slide: slow, then faster; the droop as it fades out.
    let span =
        (clock.duration - tuning.slide_after - f32::from(splat.order) * tuning.stagger).max(0.1);
    let along = ((t - tuning.slide_after) / span).clamp(0.0, 1.0);
    let slide = splat.slide * tuning.slide * along.powf(1.6);
    let droop = tuning.droop * (1.0 - thickness);
    let fall = tuning.end_fall * (1.0 - thickness).powi(2);
    let scale = Vec2::new(grow * (1.0 + squash), grow * (1.0 - squash) * (1.0 + droop));
    Pose {
        offset: Vec2::new(0.0, slide + fall + splat.size * droop * 0.5),
        scale,
        turn: splat.turn + 0.05 * spring * (14.0 * t).sin(),
        alpha: tuning.opacity * clock.fade(),
    }
}

/// The pose of `piece` at `clock`.
pub fn pose(piece: &CreamPiece, clock: &CreamClock, tuning: &CreamTuning) -> Pose {
    let thickness = clock.thickness();
    match piece.kind {
        PieceKind::Splat(i) => splat_pose(&SPLATS[usize::from(i)], clock, tuning),
        PieceKind::Trail(i) => {
            let splat = &SPLATS[usize::from(i)];
            let body = splat_pose(splat, clock, tuning);
            if body.alpha <= 0.0 {
                return Pose::HIDDEN;
            }
            // From where its top was to where its middle is (hidden under it): its top stays.
            // At rest the smear's middle is the splat's: moved by half its length, less where
            // it starts.
            let full = piece.size.y.max(0.01);
            let length = (body.offset.y + splat.size * TRAIL_FROM).clamp(0.0, full);
            let shown = smoothstep(0.0, 2.5, body.offset.y);
            Pose {
                offset: Vec2::new(0.0, length / 2.0 - splat.size * TRAIL_FROM),
                scale: Vec2::new(1.0, length / full),
                turn: 0.0,
                alpha: tuning.trail_opacity * clock.fade() * shown,
            }
        }
        PieceKind::Droplet(i, k) => {
            let splat = &SPLATS[usize::from(i)];
            let t = clock.elapsed - f32::from(splat.order) * tuning.stagger;
            if t < 0.0 || thickness <= 0.0 {
                return Pose::HIDDEN;
            }
            let (dir, reach, _) = droplet(splat, usize::from(k));
            let out = 1.0 - (-9.0 * t).exp();
            let body = splat_pose(splat, clock, tuning);
            Pose {
                offset: dir * reach * tuning.droplet_reach * out
                    + Vec2::new(0.0, body.offset.y * 0.6),
                scale: Vec2::splat((t * 9.0).min(1.0)),
                turn: dir.y.atan2(dir.x) + PI / 2.0,
                alpha: tuning.opacity * clock.fade(),
            }
        }
        PieceKind::Cherry => {
            let splat = &SPLATS[0];
            let body = splat_pose(splat, clock, tuning);
            let t = clock.elapsed - f32::from(splat.order) * tuning.stagger - 0.12;
            if body.alpha <= 0.0 || t < 0.0 {
                return Pose::HIDDEN;
            }
            // On its splat, moved with it (its turn, its squash, its slide).
            let on = CHERRY_ON * splat.size * body.scale;
            let (sin, cos) = body.turn.sin_cos();
            let on = Vec2::new(on.x * cos - on.y * sin, on.x * sin + on.y * cos);
            // It falls on it and bounces, smaller and smaller.
            let hop = -tuning.cherry_drop * (-5.0 * t).exp() * (9.0 * t).cos().abs();
            let wobble = 0.3 * (-2.5 * t).exp() * (9.0 * t).sin();
            let squish = 0.18 * (-7.0 * t).exp() * (18.0 * t).sin();
            let grow = 0.5 * (body.scale.x + body.scale.y);
            Pose {
                offset: body.offset + on + Vec2::new(0.0, hop),
                scale: Vec2::new(grow * (1.0 + squish), grow * (1.0 - squish)),
                turn: body.turn + wobble,
                alpha: tuning.opacity * clock.fade(),
            }
        }
    }
}

/// Every piece's pose at `clock`, into `out` (no allocation).
pub fn poses(
    layout: &[CreamPiece; PIECES],
    clock: &CreamClock,
    tuning: &CreamTuning,
    out: &mut [Pose; PIECES],
) {
    for (piece, pose_out) in layout.iter().zip(out.iter_mut()) {
        *pose_out = pose(piece, clock, tuning);
    }
}

/// How opaque the cream is at a point of the screen (x in % of the width, y in % of the height,
/// for a screen `aspect` times wider than high), every piece over the others, `alpha` sampling
/// the texture's opacity (px of it). For the tests and the pictures: the game draws it with the
/// GPU.
pub fn opacity_at(
    point: Vec2,
    aspect: f32,
    layout: &[CreamPiece; PIECES],
    poses: &[Pose; PIECES],
    alpha: &impl Fn(u32, u32) -> f32,
) -> f32 {
    let mut through = 1.0;
    for (piece, pose) in layout.iter().zip(poses) {
        let a = piece_alpha(point, aspect, piece, pose, alpha);
        through *= 1.0 - a;
    }
    1.0 - through
}

/// Where `point` falls in `piece`'s drawing (0 to 1 across its cell), if it does.
pub fn piece_uv(point: Vec2, aspect: f32, piece: &CreamPiece, pose: &Pose) -> Option<Vec2> {
    if pose.alpha <= 0.0 || pose.scale.x.abs() < 1e-4 || pose.scale.y.abs() < 1e-4 {
        return None;
    }
    // In vh, from the piece's middle where it is now.
    let p = Vec2::new(point.x * aspect, point.y);
    let middle = Vec2::new(piece.at.x * aspect, piece.at.y) + pose.offset;
    let d = p - middle;
    // Back through its turn (clockwise on the screen, y down) and its scale.
    let (sin, cos) = pose.turn.sin_cos();
    let local = Vec2::new(d.x * cos + d.y * sin, -d.x * sin + d.y * cos) / pose.scale;
    let mut uv = local / piece.size + Vec2::splat(0.5);
    if piece.flip {
        uv.x = 1.0 - uv.x;
    }
    (uv.cmpge(Vec2::ZERO).all() && uv.cmplt(Vec2::ONE).all()).then_some(uv)
}

fn piece_alpha(
    point: Vec2,
    aspect: f32,
    piece: &CreamPiece,
    pose: &Pose,
    alpha: &impl Fn(u32, u32) -> f32,
) -> f32 {
    let Some(uv) = piece_uv(point, aspect, piece, pose) else {
        return 0.0;
    };
    let (x0, y0, w, h) = piece.cell.rect();
    let x = x0 + ((uv.x * w as f32) as u32).min(w - 1);
    let y = y0 + ((uv.y * h as f32) as u32).min(h - 1);
    alpha(x, y) * pose.alpha
}

// --- The systems ----------------------------------------------------------------------------------

/// The cream's texture, shared by every piece.
#[derive(Resource, Clone, Debug)]
pub struct CreamAssets {
    pub image: Handle<Image>,
}

/// A cream overlay whose pieces are built.
#[derive(Component, Clone, Copy, Debug)]
pub struct CreamDressed;

fn create_cream(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(CreamAssets {
        image: images.add(cream_image()),
    });
}

/// Each new cream overlay of the HUD: its plain blobs out, the pieces in (laid out once, hidden
/// until the cream hits).
fn dress_cream(
    mut commands: Commands,
    assets: Res<CreamAssets>,
    tuning: Res<CreamTuning>,
    overlays: Query<(Entity, Option<&Children>), (With<CreamOverlay>, Without<CreamDressed>)>,
) {
    for (overlay, children) in &overlays {
        for child in children.into_iter().flatten() {
            commands.entity(*child).despawn();
        }
        commands.entity(overlay).insert(CreamDressed);
        for piece in pieces(&tuning) {
            let (x, y, w, h) = piece.cell.rect();
            commands.spawn((
                piece,
                ImageNode {
                    image: assets.image.clone(),
                    rect: Some(Rect::new(
                        x as f32,
                        y as f32,
                        (x + w) as f32,
                        (y + h) as f32,
                    )),
                    flip_x: piece.flip,
                    color: Color::WHITE.with_alpha(0.0),
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(piece.at.x),
                    top: percent(piece.at.y),
                    width: vh(piece.size.x),
                    height: vh(piece.size.y),
                    margin: UiRect {
                        left: vh(-piece.size.x / 2.0),
                        top: vh(-piece.size.y / 2.0),
                        ..default()
                    },
                    ..default()
                },
                UiTransform {
                    scale: Vec2::ZERO,
                    ..default()
                },
                ChildOf(overlay),
            ));
        }
    }
}

/// The pieces' poses, every frame while there is cream (the moment between two ticks too, for a
/// smooth motion at any frame rate), once more when it ends; nothing the rest of the time.
#[allow(clippy::type_complexity)]
fn animate_cream(
    mut was_shown: Local<bool>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<CreamTuning>,
    racers: Query<(Entity, &Racer, &RaceProgress, Has<PlayerKart>)>,
    effects: Query<&ItemEffects>,
    mut pieces: Query<(&CreamPiece, &mut UiTransform, &mut ImageNode)>,
) {
    let clock = followed(racers.iter())
        .and_then(|(entity, _)| effects.get(entity).ok())
        .filter(|e| e.creamed > 0.0)
        .map(|e| {
            let between = fixed.overstep().as_secs_f32().min(e.creamed);
            CreamClock {
                elapsed: (e.creamed_for - e.creamed).max(0.0) + between,
                remaining: e.creamed - between,
                duration: e.creamed_for,
            }
        });
    if clock.is_none() && !*was_shown {
        return;
    }
    *was_shown = clock.is_some();
    let clock = clock.unwrap_or(CreamClock {
        elapsed: 0.0,
        remaining: 0.0,
        duration: 1.0,
    });
    for (piece, mut transform, mut image) in &mut pieces {
        let pose = pose(piece, &clock, &tuning);
        let wanted = UiTransform {
            translation: Val2::new(Val::Vh(pose.offset.x), Val::Vh(pose.offset.y)),
            scale: pose.scale,
            rotation: Rot2::radians(pose.turn),
        };
        transform.set_if_neq(wanted);
        let color = Color::WHITE.with_alpha(pose.alpha);
        if image.color != color {
            image.color = color;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_of(pixels: &[u8]) -> impl Fn(u32, u32) -> f32 + '_ {
        move |x, y| f32::from(pixels[((y * ATLAS.0 + x) * 4 + 3) as usize]) / 255.0
    }

    fn clock(elapsed: f32) -> CreamClock {
        CreamClock {
            elapsed,
            remaining: 4.0 - elapsed,
            duration: 4.0,
        }
    }

    /// The share of a window of the screen (x and y in %, from and to) behind cream more than
    /// half opaque, and its mean opacity, on a 16:9 screen.
    fn hidden(
        pixels: &[u8],
        tuning: &CreamTuning,
        at: &CreamClock,
        from: Vec2,
        to: Vec2,
    ) -> (f32, f32) {
        let layout = pieces(tuning);
        let mut out = [Pose::HIDDEN; PIECES];
        poses(&layout, at, tuning, &mut out);
        let alpha = alpha_of(pixels);
        let (mut covered, mut sum, mut count) = (0, 0.0, 0);
        let steps = 120;
        for j in 0..steps {
            for i in 0..steps {
                let p =
                    from + (to - from) * Vec2::new(i as f32 + 0.5, j as f32 + 0.5) / steps as f32;
                let a = opacity_at(p, 16.0 / 9.0, &layout, &out, &alpha);
                covered += usize::from(a > 0.5);
                sum += a;
                count += 1;
            }
        }
        (covered as f32 / count as f32, sum / count as f32)
    }

    /// Issue #194: the splats are whipped cream, not plain discs: a wavy edge (its radius changes
    /// around it), a relief (lit and shaded parts, glossy crests), a soft drop shadow outside
    /// it; nothing at the edges of a cell (no piece shows its border); each drawing different;
    /// the cherry red; the smear see-through.
    #[test]
    fn the_cream_is_drawn_whipped_and_lit() {
        let pixels = cream_pixels();
        assert_eq!(pixels.len(), (ATLAS.0 * ATLAS.1 * 4) as usize);
        let texel = |x: u32, y: u32| {
            let i = ((y * ATLAS.0 + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]].map(|c| f32::from(c) / 255.0)
        };
        let cells = [
            Cell::Splat(0),
            Cell::Splat(1),
            Cell::Splat(2),
            Cell::Cherry,
            Cell::Droplet(0),
            Cell::Droplet(1),
            Cell::Trail,
        ];
        let mut fills = Vec::new();
        for cell in cells {
            let (x0, y0, w, h) = cell.rect();
            for k in 0..w {
                for (x, y) in [(x0 + k, y0), (x0 + k, y0 + h - 1)] {
                    assert!(texel(x, y)[3] < 0.02, "{cell:?} border at {x},{y}");
                }
            }
            for k in 0..h {
                for (x, y) in [(x0, y0 + k), (x0 + w - 1, y0 + k)] {
                    assert!(texel(x, y)[3] < 0.02, "{cell:?} border at {x},{y}");
                }
            }
            let mut fill = 0;
            for y in 0..h {
                for x in 0..w {
                    fill += usize::from(texel(x0 + x, y0 + y)[3] > 0.5);
                }
            }
            fills.push(fill as f32 / (w * h) as f32);
        }
        for variant in 0..SPLAT_VARIANTS {
            let (x0, y0, w, _) = Cell::Splat(variant).rect();
            let fill = fills[usize::from(variant)];
            assert!((0.25..0.6).contains(&fill), "splat {variant}: {fill}");
            // A wavy edge: how far the cream reaches from the middle, around it.
            let middle = Vec2::splat(w as f32 / 2.0);
            let mut reach = Vec::new();
            for k in 0..72 {
                let angle = k as f32 / 72.0 * TAU;
                let dir = Vec2::new(angle.cos(), angle.sin());
                let mut r = 0.0;
                for step in 0..(w / 2) {
                    let p = middle + dir * step as f32;
                    if texel(x0 + p.x as u32, y0 + p.y as u32)[3] > 0.7 {
                        r = step as f32;
                    }
                }
                reach.push(r / (w as f32 / 2.0));
            }
            let (lo, hi) = reach
                .iter()
                .fold((f32::MAX, 0.0f32), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
            assert!(hi - lo > 0.2, "splat {variant} is a disc: {lo}..{hi}");
            assert!(hi < 0.97, "splat {variant} reaches its cell's edge: {hi}");
            // Its relief: lit and shaded parts of the cream, glossy crests.
            let mut lightness = Vec::new();
            let mut shadow = 0;
            for y in 0..w {
                for x in 0..w {
                    let [r, g, b, a] = texel(x0 + x, y0 + y);
                    if a > 0.95 {
                        lightness.push((r + g + b) / 3.0);
                    } else if a > 0.05 && a < 0.4 && r < 0.4 {
                        shadow += 1;
                    }
                }
            }
            lightness.sort_by(f32::total_cmp);
            let at = |q: f32| lightness[((lightness.len() - 1) as f32 * q) as usize];
            assert!(
                at(0.05) < 0.86,
                "splat {variant} has no shade: {}",
                at(0.05)
            );
            assert!(
                at(0.995) > 0.985,
                "splat {variant} has no gloss: {}",
                at(0.995)
            );
            assert!(at(0.5) > 0.85, "splat {variant} is not cream: {}", at(0.5));
            assert!(
                shadow > 1000,
                "splat {variant} has no drop shadow: {shadow}"
            );
        }
        // Each drawing its own.
        for a in 0..SPLAT_VARIANTS {
            for b in (a + 1)..SPLAT_VARIANTS {
                let (xa, ..) = Cell::Splat(a).rect();
                let (xb, ..) = Cell::Splat(b).rect();
                let mut differ = 0;
                for y in 0..SPLAT_CELL {
                    for x in 0..SPLAT_CELL {
                        differ +=
                            usize::from((texel(xa + x, y)[3] > 0.5) != (texel(xb + x, y)[3] > 0.5));
                    }
                }
                assert!(differ > 8_000, "splats {a} and {b}: {differ}");
            }
        }
        // The cherry is red, the smear see-through.
        let (x0, y0, w, h) = Cell::Cherry.rect();
        let [r, g, b, a] = texel(x0 + w / 2, y0 + h * 3 / 5);
        assert!(
            a > 0.95 && r > 0.5 && g < 0.2 && b < 0.25,
            "{r} {g} {b} {a}"
        );
        let (x0, y0, w, h) = Cell::Trail.rect();
        let [.., a] = texel(x0 + w / 2, y0 + h * 3 / 4);
        assert!((0.2..0.95).contains(&a), "smear {a}");
    }

    /// Acceptance criterion of #194: the cream follows its rule (GD § 8: 4 s, fading out in the
    /// last second) and **hinders without blinding**: all along, the driving window (the road
    /// ahead and the kart, the middle of the screen) stays mostly in sight, the kart's own spot
    /// nearly clear, while the whole screen is well creamed.
    #[test]
    fn the_cream_hinders_without_blinding_and_keeps_its_time() {
        let pixels = cream_pixels();
        let tuning = CreamTuning::default();
        let window = (Vec2::new(32.0, 28.0), Vec2::new(68.0, 85.0));
        let kart = (Vec2::new(43.0, 45.0), Vec2::new(57.0, 75.0));
        let screen = (Vec2::ZERO, Vec2::splat(100.0));
        let mut worst_window = 0.0f32;
        let mut worst_kart = 0.0f32;
        for k in 0..=40 {
            let at = clock(k as f32 * 0.1);
            let (w, _) = hidden(&pixels, &tuning, &at, window.0, window.1);
            let (c, _) = hidden(&pixels, &tuning, &at, kart.0, kart.1);
            worst_window = worst_window.max(w);
            worst_kart = worst_kart.max(c);
        }
        println!("driving window {worst_window:.3}, kart {worst_kart:.3}");
        assert!(worst_window < 0.2, "the driving window: {worst_window}");
        assert!(worst_kart < 0.05, "the kart's spot: {worst_kart}");
        // It does cream the screen: at its middle, a good part of it.
        let (middle, middle_mean) = hidden(&pixels, &tuning, &clock(2.0), screen.0, screen.1);
        println!("screen at 2 s {middle:.3} (mean {middle_mean:.3})");
        assert!(middle > 0.2, "{middle}");
        // Its time: nothing before nor after, full until its last second, then fading.
        let (_, mean_before) = hidden(&pixels, &tuning, &clock(0.0), screen.0, screen.1);
        assert!(mean_before > 0.0, "the first splat hits at once");
        let (_, end) = hidden(&pixels, &tuning, &clock(4.0), screen.0, screen.1);
        assert_eq!(end, 0.0);
        let (_, fading) = hidden(&pixels, &tuning, &clock(3.7), screen.0, screen.1);
        assert!(
            fading < middle_mean * 0.7 && fading > 0.0,
            "{fading} {middle_mean}"
        );
        let layout = pieces(&tuning);
        for k in 0..=40 {
            let at = clock(k as f32 * 0.1);
            for piece in &layout {
                let p = pose(piece, &at, &tuning);
                assert!(p.alpha <= tuning.opacity * at.fade() + 1e-6);
                assert!(p.scale.is_finite() && p.offset.is_finite());
            }
        }
    }

    /// Issue #194's pictures (ignored, run by hand with `FLUFFY_CREAM_OUT=<folder>`): the
    /// texture (raw RGBA, `atlas.rgba`) and every piece's pose at a few moments (`poses.txt`),
    /// for a script to lay them over a screenshot as the UI does (the session has no GPU).
    #[test]
    #[ignore]
    fn write_the_cream_for_pictures() {
        let Ok(folder) = std::env::var("FLUFFY_CREAM_OUT") else {
            return;
        };
        let folder = std::path::PathBuf::from(folder);
        std::fs::write(folder.join("atlas.rgba"), cream_pixels()).unwrap();
        let tuning = CreamTuning::default();
        let layout = pieces(&tuning);
        let mut text = String::new();
        for moment in [0.03, 0.08, 0.16, 0.3, 0.6, 1.5, 2.5, 3.2, 3.6, 3.85] {
            let at = clock(moment);
            text += &format!("t {moment}\n");
            for piece in &layout {
                let p = pose(piece, &at, &tuning);
                let (x, y, w, h) = piece.cell.rect();
                text += &format!(
                    "{x} {y} {w} {h} {} {} {} {} {} {} {} {} {} {} {}\n",
                    piece.at.x,
                    piece.at.y,
                    piece.size.x,
                    piece.size.y,
                    p.offset.x,
                    p.offset.y,
                    p.scale.x,
                    p.scale.y,
                    p.turn,
                    p.alpha,
                    u8::from(piece.flip)
                );
            }
        }
        std::fs::write(folder.join("poses.txt"), text).unwrap();
    }

    /// Issue #194: the arrival and the slide: each splat hits squashed wide, then bounces back
    /// to its size (overshooting once); the splats hit one after the other; then they slide
    /// down, their smear growing above them, and droop as they fade.
    #[test]
    fn the_splats_hit_bounce_and_slide() {
        let tuning = CreamTuning::default();
        let first = &SPLATS[0];
        let hit = splat_pose(first, &clock(0.0), &tuning);
        assert!(hit.scale.x < 0.6 && hit.scale.x > hit.scale.y, "{hit:?}");
        let sizes: Vec<f32> = (0..40)
            .map(|k| splat_pose(first, &clock(k as f32 * 0.01), &tuning).scale.y)
            .collect();
        assert!(sizes.iter().any(|s| *s > 1.03), "no overshoot: {sizes:?}");
        let settled = splat_pose(first, &clock(1.0), &tuning);
        assert!(
            (settled.scale - Vec2::ONE).abs().max_element() < 0.02,
            "{settled:?}"
        );
        // One after the other.
        let last = SPLATS.iter().max_by_key(|s| s.order).unwrap();
        assert_eq!(splat_pose(last, &clock(0.0), &tuning), Pose::HIDDEN);
        assert!(splat_pose(last, &clock(0.5), &tuning).alpha > 0.9);
        // The slide, its smear.
        let layout = pieces(&tuning);
        let trail = layout
            .iter()
            .find(|p| p.kind == PieceKind::Trail(0))
            .unwrap();
        let mut last_offset = -1.0;
        let mut last_length = -1.0;
        for k in 5..=30 {
            let at = clock(k as f32 * 0.1);
            let body = splat_pose(first, &at, &tuning);
            let smear = pose(trail, &at, &tuning);
            assert!(body.offset.y >= last_offset, "{k}");
            let length = smear.scale.y * trail.size.y;
            assert!(length >= last_length - 1e-4, "{k}");
            // Its top stays where the splat's top started.
            // Its top (from the splat's middle at rest) stays where the splat's top started; its
            // bottom is the splat's middle now (under it).
            if smear.alpha > 0.0 {
                let top = smear.offset.y - length / 2.0;
                assert!((top + first.size * TRAIL_FROM).abs() < 0.01, "{k}: {top}");
                let bottom = smear.offset.y + length / 2.0;
                assert!((bottom - body.offset.y).abs() < 0.01, "{k}: {bottom}");
            }
            last_offset = body.offset.y;
            last_length = length;
        }
        let slid = splat_pose(first, &clock(3.0), &tuning).offset.y;
        assert!(slid > first.slide * 0.5, "{slid}");
        assert!(pose(trail, &clock(3.0), &tuning).alpha > 0.3);
        let drooping = splat_pose(first, &clock(3.8), &tuning);
        assert!(drooping.scale.y > drooping.scale.x + 0.05, "{drooping:?}");
    }
}
