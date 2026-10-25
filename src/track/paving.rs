//! Issue #187: **the drawings of the roads**, made by the game when a circuit is built (pure,
//! unit-tested: no file, no crate): one texture for the paving of the road (the [`Paving`] of
//! its biome or of its circuit) and one for its borders (the [`Border`]), each with its mipmaps
//! (a road far away stays a calm surface, never a flickering one). **Purely what the road looks
//! like** (GD § 16: « le revêtement est un habillage »): nothing of it reaches the race.
//!
//! - **The paving** repeats every [`PAVING_TILE`] meters across and along the road (its UVs in
//!   meters: `mesh::generate`), [`PAVING_SIZE`] pixels a side (64 px a meter). Every drawing
//!   tiles seamlessly (periodic noise and cells). **Its mean color is the road's color** (the
//!   palette's `asphalt`, in linear light: what the eye sees from afar, and what the mipmaps
//!   reach): the drawing is made around it, then each channel is scaled so that its mean is
//!   exactly that color. So the vignette (#181), the dust of the tyres (#174) and the readability
//!   tests (#106) see the road as it is drawn.
//! - **The border** is 1 m across (`mesh::CURB_WIDTH`, its whole width: 0 by the road, 1 at its
//!   outer edge) and repeats every [`BORDER_TILE`] meters along it, in the palette's two `curbs`
//!   colors.
//! - Their relief (a cobble's dome, a log's roundness, a crack's depth) is painted in: one flat
//!   material, no normal map (a texture less to sample on WebGL2).

use super::biome::{Border, Paving, Rgb};

/// The paving repeats every this many meters, across and along the road.
pub const PAVING_TILE: f32 = 8.0;
/// Its side (px) at full size: 64 px a meter.
pub const PAVING_SIZE: u32 = 512;
/// Its mipmaps, the full size included (down to 8 px).
pub const PAVING_MIPS: u32 = 7;
/// The border repeats every this many meters along the road.
pub const BORDER_TILE: f32 = 4.0;
/// Its size (px) at full size: across its meter, and along its tile (64 px a meter).
pub const BORDER_SIZE: (u32, u32) = (64, 256);
/// Its mipmaps, the full size included (down to 2 × 8 px).
pub const BORDER_MIPS: u32 = 6;

/// A texture as the GPU takes it: RGBA, 8 bits a channel, **sRGB**, its mipmaps after it (each
/// half the one before).
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub data: Vec<u8>,
}

impl Texture {
    /// The texel (`x`, `y`) of the full size, sRGB from 0 to 1.
    pub fn texel(&self, x: u32, y: u32) -> Rgb {
        let i = ((y * self.width + x) * 4) as usize;
        let c = |k: usize| f32::from(self.data[i + k]) / 255.0;
        (c(0), c(1), c(2))
    }

    /// Its mean color at full size, averaged in linear light, given back in sRGB.
    pub fn mean(&self) -> Rgb {
        let mut sum = [0.0f64; 3];
        for y in 0..self.height {
            for x in 0..self.width {
                let (r, g, b) = self.texel(x, y);
                for (s, c) in sum.iter_mut().zip([r, g, b]) {
                    *s += f64::from(to_linear(c));
                }
            }
        }
        let n = f64::from(self.width * self.height);
        let [r, g, b] = sum.map(|s| to_srgb((s / n) as f32));
        (r, g, b)
    }
}

/// sRGB (0 to 1) to linear light.
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light to sRGB (0 to 1).
pub fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn lin((r, g, b): Rgb) -> [f32; 3] {
    [to_linear(r), to_linear(g), to_linear(b)]
}

// --- Noise that tiles: every lattice wraps around its tile. ---

/// A number from 0 to 1 for the corner (`x`, `y`) of a grid.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_215.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    smooth(((x - from) / (to - from)).clamp(0.0, 1.0))
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    a.map(|c| c * k)
}

/// Value noise (0 to 1) on a grid of `fx` × `fy` cells per tile, at (`u`, `v`) in tiles: it
/// wraps around the tile.
fn noise(u: f32, v: f32, fx: i32, fy: i32, seed: u32) -> f32 {
    let (x, y) = (u * fx as f32, v * fy as f32);
    let (xi, yi) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - xi), smooth(y - yi));
    let (xi, yi) = (xi as i32, yi as i32);
    let at = |i: i32, j: i32| hash(i.rem_euclid(fx), j.rem_euclid(fy), seed);
    let top = at(xi, yi) + (at(xi + 1, yi) - at(xi, yi)) * tx;
    let bottom = at(xi, yi + 1) + (at(xi + 1, yi + 1) - at(xi, yi + 1)) * tx;
    top + (bottom - top) * ty
}

/// Fractal noise (0 to 1): `octaves` of [`noise`], each twice finer and half as strong.
fn fbm(u: f32, v: f32, fx: i32, fy: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        let k = 1 << octave;
        sum += amplitude * noise(u, v, fx * k, fy * k, seed.wrapping_add(octave * 101));
        total += amplitude;
        amplitude *= 0.5;
    }
    sum / total
}

/// The nearest points of a jittered grid of `nx` × `ny` cells per tile (wrapping around it), at
/// (`u`, `v`) in tiles: distances in meters (the tile `tile` × `tile_y` meters).
#[derive(Clone, Copy, Debug)]
struct Cells {
    /// To the nearest point, and to the second nearest.
    f1: f32,
    f2: f32,
    /// A number from 0 to 1 of the nearest one's cell.
    id: f32,
    /// From the nearest point to (`u`, `v`), in meters.
    offset: (f32, f32),
}

#[allow(clippy::too_many_arguments)]
fn cells(u: f32, v: f32, nx: i32, ny: i32, jitter: f32, size: (f32, f32), seed: u32) -> Cells {
    let (x, y) = (u * nx as f32, v * ny as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let (cw, ch) = (size.0 / nx as f32, size.1 / ny as f32);
    let mut best = Cells {
        f1: f32::MAX,
        f2: f32::MAX,
        id: 0.0,
        offset: (0.0, 0.0),
    };
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = (cx + i, cy + j);
            let (wx, wy) = (gx.rem_euclid(nx), gy.rem_euclid(ny));
            let px = gx as f32 + 0.5 + jitter * (hash(wx, wy, seed) - 0.5);
            let py = gy as f32 + 0.5 + jitter * (hash(wx, wy, seed ^ 0x9E37) - 0.5);
            let (dx, dy) = ((x - px) * cw, (y - py) * ch);
            let d = (dx * dx + dy * dy).sqrt();
            if d < best.f1 {
                best.f2 = best.f1;
                best.f1 = d;
                best.id = hash(wx, wy, seed ^ 0x51ED);
                best.offset = (dx, dy);
            } else if d < best.f2 {
                best.f2 = d;
            }
        }
    }
    best
}

// --- The pavings. ---

/// The colors a drawing is made of: its mean color and its accent, linear.
#[derive(Clone, Copy, Debug)]
struct Inks {
    base: [f32; 3],
    accent: [f32; 3],
}

/// The paving `paving` at (`u`, `v`), both in tiles (0 to 1: across and along the road), its color
/// linear (before its mean is made the road's).
fn paving_texel(paving: Paving, inks: &Inks, u: f32, v: f32) -> [f32; 3] {
    let t = PAVING_TILE;
    let (x, y) = (u * t, v * t);
    let base = inks.base;
    let grain = |f: i32, seed: u32| fbm(u, v, f, f, 2, seed) - 0.5;
    let blotch = |f: i32, seed: u32| fbm(u, v, f, f, 4, seed) - 0.5;
    match paving {
        Paving::Asphalt => {
            let k = 1.0 + 0.22 * blotch(4, 1) + 0.3 * grain(256, 2);
            let c = cells(u, v, 160, 160, 0.9, (t, t), 3);
            let speck = if c.id < 0.3 && c.f1 < 0.012 {
                1.25
            } else {
                1.0
            };
            scale(base, k * speck)
        }
        Paving::Dirt => {
            // Packed earth: broad lighter and darker patches, a fine grit.
            let k = 1.0 + 0.75 * blotch(3, 11) + 0.3 * blotch(12, 15) + 0.3 * grain(160, 12);
            let mut color = scale(base, k);
            // Fine dark gravel.
            let grit = cells(u, v, 120, 120, 0.9, (t, t), 16);
            if grit.id < 0.3 && grit.f1 < 0.01 {
                color = scale(color, 0.7);
            }
            // Pebbles of every size, in a fifth of the cells, more in some places than others.
            let c = cells(u, v, 32, 32, 0.85, (t, t), 13);
            let many = noise(u, v, 6, 6, 17);
            let radius = 0.02 + 0.05 * hash((c.id * 1e5) as i32, 1, 14).powi(2);
            if c.id < 0.12 + 0.45 * many && c.f1 < radius {
                let r = c.f1 / radius;
                let dome = (1.0 - r * r).sqrt();
                let tone = 1.2 + 0.4 * hash((c.id * 1e5) as i32, 2, 18);
                let stone = mix(scale(base, tone), [0.5, 0.48, 0.45], 0.4);
                color = scale(stone, 0.6 + 0.5 * dome - 0.25 * (c.offset.1 / radius));
            } else if c.id < 0.12 + 0.45 * many && c.f1 < radius * 1.4 && c.offset.1 > 0.0 {
                // Its shadow, toward the bottom.
                color = scale(color, 0.72);
            }
            color
        }
        Paving::Cobbles => {
            // Rows across the road, every 8 / 22 m; in each row, stones 0.3 to 0.55 m long (8 / 20 m
            // on average), each row shifted its own way.
            let rows = 22.0;
            let row = (v * rows).floor();
            let shift = hash(row as i32, 0, 24);
            let per_row = 20.0;
            let along = u * per_row + shift * per_row;
            let k0 = along.floor();
            // Uneven stones: each joint moved a little along the row.
            let joint = |k: f32| {
                k + 0.35 * (hash((k as i32).rem_euclid(per_row as i32), row as i32, 25) - 0.5)
            };
            let (mut a, mut b) = (joint(k0), joint(k0 + 1.0));
            let mut k = k0;
            if along < a {
                (b, a) = (a, joint(k0 - 1.0));
                k = k0 - 1.0;
            } else if along > b {
                (a, b) = (b, joint(k0 + 2.0));
                k = k0 + 1.0;
            }
            let (sw, sh) = ((b - a) * t / per_row, t / rows);
            let lx = (along - (a + b) / 2.0) * t / per_row;
            let ly = ((v * rows).rem_euclid(1.0) - 0.5) * sh;
            let id = hash(
                (k as i32).rem_euclid(per_row as i32),
                (row as i32).rem_euclid(rows as i32),
                21,
            );
            // A rounded box, a little smaller than its place (the joint).
            let gap = 0.028;
            let round = 0.1;
            let qx = (lx.abs() - (sw / 2.0 - gap - round)).max(0.0);
            let qy = (ly.abs() - (sh / 2.0 - gap - round)).max(0.0);
            let outside = (qx * qx + qy * qy).sqrt() - round;
            let joint = scale(base, 0.32 + 0.2 * grain(96, 22));
            if outside > 0.0 {
                return joint;
            }
            let depth = (-outside / 0.08).min(1.0);
            let dome = (depth * (2.0 - depth)).sqrt();
            let tint = 0.78 + 0.42 * id;
            let hue = hash((k as i32).rem_euclid(per_row as i32), row as i32, 26);
            let warm = [
                base[0] * (0.9 + 0.2 * hue),
                base[1],
                base[2] * (1.1 - 0.2 * hue),
            ];
            let light = 0.6 + 0.5 * dome - 0.8 * ly.max(0.0) * (1.0 - dome);
            scale(warm, tint * light * (1.0 + 0.2 * grain(160, 23)))
        }
        Paving::Flagstones => {
            let c = cells(u, v, 10, 10, 0.85, (t, t), 31);
            let edge = c.f2 - c.f1;
            let tint = 0.84 + 0.3 * c.id;
            let k = tint * (1.0 + 0.14 * blotch(16, 32) + 0.14 * grain(160, 33));
            let bevel = smoothstep(0.02, 0.08, edge);
            let slab = scale(base, k * (0.8 + 0.25 * bevel));
            let joint = scale(base, 0.38 + 0.2 * grain(64, 34));
            mix(joint, slab, smoothstep(0.012, 0.024, edge))
        }
        Paving::Planks => {
            // Planks across the road, 0.32 m wide (25 a tile), 8 / 3 m long, their ends
            // staggered from one plank to the next.
            let planks = 25.0;
            let row = (v * planks).floor();
            let ly = ((v * planks).rem_euclid(1.0) - 0.5) * (t / planks);
            let offset = hash(row as i32, 0, 41);
            let along = u * 3.0 + offset;
            let piece = along.floor();
            let lx = (along.rem_euclid(1.0) - 0.5) * (t / 3.0);
            let id = hash(row as i32, (piece as i32).rem_euclid(3), 42);
            let tint = 0.80 + 0.36 * id;
            // The grain along the plank: fine lines, warped.
            let warp = fbm(u, v, 3, 25, 3, 43);
            let lines = (std::f32::consts::TAU * (v * planks * 7.0 + 2.5 * warp)).sin();
            let streak = fbm(u, v, 24, 100, 3, 44) - 0.5;
            let wood = tint * (1.0 + 0.07 * lines + 0.24 * streak);
            let half = t / planks / 2.0;
            let gap = 0.016;
            let into_gap = (half - ly.abs()).min(t / 6.0 - lx.abs());
            if into_gap < gap {
                return scale(base, 0.22);
            }
            // Worn edges, a little lighter on the top one (the light from above), darker below.
            let near = smoothstep(gap, gap + 0.03, into_gap);
            let edge = if ly < 0.0 { 1.08 } else { 0.85 };
            let mut k = wood * (edge + (1.0 - edge) * near);
            // Two nails at each end.
            for end in [-1.0f32, 1.0] {
                for side in [-1.0f32, 1.0] {
                    let nx = lx - end * (t / 6.0 - 0.09);
                    let ny = ly - side * 0.08;
                    if nx * nx + ny * ny < 0.012 * 0.012 {
                        k = 0.4;
                    }
                }
            }
            scale(base, k)
        }
        Paving::PackedSand => {
            // Ripples across the road, about every 0.44 m (18 a tile), wavy.
            let warp = fbm(u, v, 3, 3, 3, 51) - 0.5;
            let phase = v * 18.0 + 1.2 * warp + 0.15 * (std::f32::consts::TAU * u * 2.0).sin();
            let p = phase.rem_euclid(1.0);
            // A gentle slope up, a steeper one down: lit on one side.
            let ripple = if p < 0.7 { p / 0.7 } else { (1.0 - p) / 0.3 };
            let damp = smoothstep(0.1, 0.25, blotch(4, 52));
            let k = (0.88 + 0.22 * ripple) * (1.0 + 0.24 * grain(192, 53)) * (1.0 - 0.08 * damp);
            let mut color = scale(base, k);
            // A few shells.
            let c = cells(u, v, 30, 30, 0.9, (t, t), 54);
            if c.id < 0.12 && c.f1 < 0.035 {
                color = mix(scale(base, 1.45), [0.95, 0.85, 0.8], 0.5);
            }
            color
        }
        Paving::CrackedEarth => {
            let c = cells(u, v, 12, 12, 0.9, (t, t), 61);
            let edge = c.f2 - c.f1;
            let tint = 0.88 + 0.22 * c.id;
            let k = tint * (1.0 + 0.2 * blotch(8, 62) + 0.18 * grain(160, 63));
            // The plates' edges curl up a little: lighter along the crack.
            let rim = 1.0 + 0.14 * (1.0 - smoothstep(0.02, 0.07, edge));
            let plate = scale(base, k * rim);
            // Thin cracks inside some plates.
            let fine = cells(u, v, 36, 36, 0.9, (t, t), 64);
            let thin = fine.f2 - fine.f1 < 0.008 && noise(u, v, 12, 12, 65) > 0.55;
            let crack = scale(base, 0.25);
            let mut color = mix(crack, plate, smoothstep(0.01, 0.022, edge));
            if thin {
                color = scale(color, 0.6);
            }
            color
        }
        Paving::ForestPath => {
            let k = 1.0 + 0.34 * blotch(5, 71) + 0.24 * grain(128, 72);
            let mut color = scale(base, k);
            // Bits of leaves and twigs.
            let litter = cells(u, v, 70, 70, 0.9, (t, t), 79);
            if litter.id < 0.25 && litter.f1 < 0.018 {
                color = scale(color, 0.62 + 0.6 * litter.id * 4.0);
            }
            // Moss, in soft patches, tufted.
            let moss = smoothstep(0.0, 0.22, blotch(6, 73) + 0.2 * grain(48, 74));
            let tuft = 1.0 + 0.45 * grain(200, 75);
            color = mix(color, scale(inks.accent, tuft), moss * 0.9);
            // Roots across the road, one every 1.6 m, broken, dark, rounded.
            let warp = fbm(u, v, 2, 2, 3, 76) - 0.5;
            let phase = v * 5.0 + 0.9 * warp;
            let across = (phase.rem_euclid(1.0) - 0.5) * (t / 5.0);
            let width = 0.06 + 0.05 * noise(u, v, 16, 5, 77);
            let shown = noise(u, v, 6, 5, 78) > 0.38;
            if shown && across.abs() < width {
                let r = across / width;
                let round = (1.0 - r * r).sqrt();
                let bark = 1.0 + 0.3 * (fbm(u, v, 64, 5, 2, 70) - 0.5);
                color = scale(base, (0.45 + 0.55 * round - 0.2 * r) * bark);
            } else if shown && across > 0.0 && across < width * 1.8 {
                color = scale(color, 0.6);
            }
            color
        }
        Paving::Sugar => {
            // Stripes along the road, 0.5 m of each color (8 pairs a tile), their edges gently
            // wavy.
            let wave = 0.06 * (std::f32::consts::TAU * v * 2.0).sin();
            let phase = (u * 8.0 + wave + 0.06 * (fbm(u, v, 4, 4, 2, 81) - 0.5)).rem_euclid(1.0);
            let tri = (phase - 0.5).abs() * 2.0;
            let stripe = smoothstep(0.44, 0.56, tri);
            let k = 1.0 + 0.12 * grain(200, 82) + 0.06 * blotch(6, 83);
            let mut color = scale(mix(base, inks.accent, stripe), k);
            // Each stripe rounded: lighter down its middle.
            let middle = 1.0 - ((tri - 0.5).abs() * 2.0);
            color = scale(color, 0.9 + 0.14 * middle);
            // Sugar crystals that sparkle.
            let c = cells(u, v, 200, 200, 0.9, (t, t), 84);
            if c.id < 0.35 && c.f1 < 0.007 {
                color = mix(color, [1.0, 1.0, 1.0], 0.7);
            }
            color
        }
        Paving::Licorice => {
            // Ridges along the road, every 1 / 3 m (24 a tile): rounded tops, dark grooves, a
            // shine on each top.
            let phase = u * 24.0 + 0.06 * (fbm(u, v, 3, 3, 2, 91) - 0.5);
            let ridge = (std::f32::consts::TAU * phase).cos();
            let groove = smoothstep(-0.75, -0.95, ridge);
            let shine = smoothstep(0.82, 0.98, ridge) * (0.7 + 0.6 * noise(u, v, 24, 40, 92));
            let k = (0.85 + 0.35 * ridge.max(0.0))
                * (1.0 - 0.55 * groove)
                * (1.0 + 0.08 * grain(128, 93));
            mix(scale(base, k), [0.85, 0.8, 0.9], 0.45 * shine)
        }
        Paving::GroomedSnow => {
            // The groomer's ridges along the road (every 0.2 m: 40 a tile), sharp-topped, the
            // seam of its passes every 4 m; soft drifts.
            let phase = u * 40.0 + 0.25 * (fbm(u, v, 2, 8, 2, 101) - 0.5);
            let p = phase.rem_euclid(1.0);
            let ridge = if p < 0.5 { p * 2.0 } else { (1.0 - p) * 2.0 };
            let seam = (u * 2.0 + 0.02 * (fbm(u, v, 2, 6, 2, 105) - 0.5)).rem_euclid(1.0);
            let seam = 1.0 - 0.22 * (1.0 - smoothstep(0.0, 0.012, seam.min(1.0 - seam)));
            let drift = 0.14 * blotch(4, 102);
            let k = (0.88 + 0.24 * ridge) * seam * (1.0 + drift) * (1.0 + 0.08 * grain(200, 103));
            let mut color = scale(base, k);
            let c = cells(u, v, 220, 220, 0.9, (t, t), 104);
            if c.id < 0.25 && c.f1 < 0.006 {
                color = mix(color, [1.0, 1.0, 1.0], 0.8);
            }
            color
        }
        Paving::Leaves => {
            let k = 1.0 + 0.25 * blotch(5, 111) + 0.22 * grain(128, 112);
            let color = scale(base, k);
            // Fallen leaves, in a jittered grid (14 cells a tile, 0.57 m), a leaf in most cells:
            // an ellipse at an angle, its midrib, its shadow. The neighbours' leaves overlap.
            let n = 14;
            let (cx, cy) = ((u * n as f32).floor() as i32, (v * n as f32).floor() as i32);
            let cell = t / n as f32;
            let mut leaf_color = None;
            let mut shadow = false;
            for j in -1..=1 {
                for i in -1..=1 {
                    let (gx, gy) = (cx + i, cy + j);
                    let (wx, wy) = (gx.rem_euclid(n), gy.rem_euclid(n));
                    if hash(wx, wy, 113) > 0.7 {
                        continue;
                    }
                    let px = (gx as f32 + 0.5 + 0.8 * (hash(wx, wy, 114) - 0.5)) * cell;
                    let py = (gy as f32 + 0.5 + 0.8 * (hash(wx, wy, 115) - 0.5)) * cell;
                    let angle = hash(wx, wy, 116) * std::f32::consts::TAU;
                    let length = 0.13 + 0.08 * hash(wx, wy, 117);
                    let (s, c) = angle.sin_cos();
                    let ellipse = |dx: f32, dy: f32| {
                        let (a, b) = (dx * c + dy * s, -dx * s + dy * c);
                        ((a / length).powi(2) + (b / (length * 0.5)).powi(2), b)
                    };
                    let (inside, b) = ellipse(x - px, y - py);
                    if inside < 1.0 {
                        let hue = hash(wx, wy, 118);
                        let a = inks.accent;
                        let leaf = if hue < 0.45 {
                            a
                        } else if hue < 0.75 {
                            [a[0] * 0.8, a[1] * 0.45, a[2] * 0.6]
                        } else {
                            [a[0] * 1.2, a[1] * 2.4, a[2] * 0.8]
                        };
                        let rib = if b.abs() < 0.006 { 0.7 } else { 1.0 };
                        let curl = 0.85 + 0.3 * (1.0 - inside);
                        leaf_color = Some(scale(leaf, rib * curl * (1.0 + 0.15 * grain(200, 119))));
                    } else if ellipse(x - px - 0.012, y - py - 0.018).0 < 1.0 {
                        // The shadow of a leaf, a little to the side.
                        shadow = true;
                    }
                }
            }
            match leaf_color {
                Some(leaf) => leaf,
                None if shadow => scale(color, 0.7),
                None => color,
            }
        }
    }
}

/// The border `border` at (`u`, `v`): `u` across it (0 by the road, 1 at its outer edge, its meter),
/// `v` along it in tiles (0 to 1: [`BORDER_TILE`] meters), its color linear.
fn border_texel(border: Border, curbs: (Rgb, Rgb), u: f32, v: f32) -> [f32; 3] {
    let (c0, c1) = (lin(curbs.0), lin(curbs.1));
    let t = BORDER_TILE;
    let y = v * t;
    let grain = |fx: i32, fy: i32, seed: u32| fbm(u, v, fx, fy, 2, seed) - 0.5;
    match border {
        Border::Stripes => {
            // 2 m of each color, rounded across (lit on top), a dark seam between two.
            let block = (y / 2.0).floor() as i32 % 2;
            let color = if block == 0 { c0 } else { c1 };
            let bulge = (std::f32::consts::PI * u).sin();
            let seam = (y / 2.0).rem_euclid(1.0);
            let seam = smoothstep(0.0, 0.012, seam.min(1.0 - seam));
            scale(
                color,
                (0.78 + 0.26 * bulge) * (0.75 + 0.25 * seam) * (1.0 + 0.1 * grain(32, 128, 201)),
            )
        }
        Border::Logs => {
            // Logs across the border, 0.25 m thick (16 a tile), each its own length; beyond its
            // cut end, the earth.
            let logs = 16.0;
            let k = (v * logs).floor();
            let id = hash(k as i32, 0, 211);
            let across = (v * logs).rem_euclid(1.0) * 2.0 - 1.0;
            let end = 0.86 + 0.1 * id;
            let earth = scale(c0, 0.35 + 0.15 * grain(32, 128, 212));
            if u > end {
                return earth;
            }
            let round = (1.0 - across * across).max(0.0).sqrt();
            if round < 0.3 {
                return scale(earth, 0.6);
            }
            let light = 0.35 + 0.75 * round - 0.2 * across;
            // Its cut end: rings of the light wood.
            if u > end - 0.06 {
                let r = across.abs();
                let rings = 0.85 + 0.15 * (r * 28.0 + 3.0 * id).sin();
                return scale(c1, light * rings);
            }
            // Its bark: streaks along the log.
            let bark = 1.0 + 0.45 * (fbm(u, v, 6, 128, 3, 213) - 0.5);
            scale(mix(c0, c1, 0.15 * id), light * bark)
        }
        Border::Stones => {
            // Rounded stones, about a third of a meter (3 across, 12 a tile along), each its
            // shade between the two colors; dark joints.
            let c = cells(u, v, 3, 12, 0.75, (1.0, t), 221);
            let edge = c.f2 - c.f1;
            if edge < 0.03 {
                return scale(mix(c0, c1, 0.5), 0.3);
            }
            let dome = smoothstep(0.03, 0.17, edge);
            let light = 0.6 + 0.5 * dome - 0.6 * (c.offset.1 / 0.3).clamp(-0.3, 0.3) * (1.0 - dome);
            scale(
                mix(c0, c1, c.id),
                light * (1.0 + 0.14 * grain(32, 128, 222)),
            )
        }
        Border::Candies => {
            // Round sweets, 0.56 m across, every 2 / 3 m on a ground of sugar; a swirl of the
            // candy's color and white in each; three shades of the first color in turn.
            let n = 6.0;
            let k = (v * n).floor();
            let dy = ((v * n).rem_euclid(1.0) - 0.5) * (t / n);
            let dx = u - 0.5;
            let r = (dx * dx + dy * dy).sqrt();
            let radius = 0.28;
            let ground = scale(c1, 0.92 + 0.12 * grain(32, 128, 231));
            if r > radius {
                // Its shadow toward the bottom.
                let (sx, sy) = (dx - 0.02, dy - 0.03);
                if (sx * sx + sy * sy).sqrt() < radius {
                    return scale(ground, 0.72);
                }
                return ground;
            }
            let shade = match k as i32 % 3 {
                0 => c0,
                1 => mix(c0, [1.0, 1.0, 1.0], 0.35),
                _ => scale(c0, 0.75),
            };
            let angle = dy.atan2(dx);
            let swirl = (4.0 * angle + r * 20.0).sin();
            let candy = mix(shade, c1, smoothstep(0.2, 0.4, swirl));
            let dome = (1.0 - (r / radius).powi(2)).sqrt();
            let rim = if r > radius - 0.016 { 0.7 } else { 1.0 };
            let shine = if (dx + 0.08).powi(2) + (dy + 0.1).powi(2) < 0.04 * 0.04 {
                0.6
            } else {
                0.0
            };
            mix(
                scale(candy, (0.65 + 0.4 * dome) * rim),
                [1.0, 1.0, 1.0],
                shine,
            )
        }
    }
}

/// The accent of a paving: its own, or a shade of its mean color.
fn accent_of(base: Rgb, accent: Option<Rgb>) -> [f32; 3] {
    accent.map_or_else(|| scale(lin(base), 1.3), lin)
}

/// The paving `paving` of mean color `base` (sRGB) and accent `accent`, its texture: drawn, its
/// mean made exactly `base` (each channel, in linear light), with its mipmaps.
pub fn paving_texture(paving: Paving, base: Rgb, accent: Option<Rgb>) -> Texture {
    let inks = Inks {
        base: lin(base),
        accent: accent_of(base, accent),
    };
    let n = PAVING_SIZE;
    let mut full = Vec::with_capacity((n * n) as usize);
    for py in 0..n {
        for px in 0..n {
            let u = (px as f32 + 0.5) / n as f32;
            let v = (py as f32 + 0.5) / n as f32;
            full.push(paving_texel(paving, &inks, u, v));
        }
    }
    // Its mean, made the road's color.
    let mut mean = [0.0f64; 3];
    for texel in &full {
        for k in 0..3 {
            mean[k] += f64::from(texel[k]);
        }
    }
    let gain: [f32; 3] = [0, 1, 2].map(|k| {
        let m = (mean[k] / full.len() as f64) as f32;
        if m > 1e-6 { inks.base[k] / m } else { 1.0 }
    });
    for texel in &mut full {
        for k in 0..3 {
            texel[k] *= gain[k];
        }
    }
    with_mips(full, n, n, PAVING_MIPS)
}

/// The border `border` in the colors `curbs` (sRGB), its texture with its mipmaps.
pub fn border_texture(border: Border, curbs: (Rgb, Rgb)) -> Texture {
    let (w, h) = BORDER_SIZE;
    let mut full = Vec::with_capacity((w * h) as usize);
    for py in 0..h {
        for px in 0..w {
            let u = (px as f32 + 0.5) / w as f32;
            let v = (py as f32 + 0.5) / h as f32;
            full.push(border_texel(border, curbs, u, v));
        }
    }
    with_mips(full, w, h, BORDER_MIPS)
}

/// The linear texels of the full size, and their mipmaps (each texel the mean of the four under
/// it, in linear light), as sRGB bytes, opaque.
fn with_mips(full: Vec<[f32; 3]>, width: u32, height: u32, mips: u32) -> Texture {
    let mut data = Vec::new();
    let mut level = full;
    let (mut w, mut h) = (width, height);
    for m in 0..mips {
        for texel in &level {
            for c in texel {
                data.push((to_srgb(*c) * 255.0).round() as u8);
            }
            data.push(255);
        }
        if m + 1 == mips {
            break;
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity((nw * nh) as usize);
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = [0.0f32; 3];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                    let texel = level[(sy * w + sx) as usize];
                    for k in 0..3 {
                        sum[k] += texel[k] * 0.25;
                    }
                }
                next.push(sum);
            }
        }
        level = next;
        (w, h) = (nw, nh);
    }
    Texture {
        width,
        height,
        mips,
        data,
    }
}

/// How rough a paving's material is (Bevy's `perceptual_roughness`): the liquorice and the sugar
/// shine, the rest is matte.
pub fn roughness(paving: Paving) -> f32 {
    match paving {
        Paving::Licorice => 0.32,
        Paving::Sugar => 0.55,
        Paving::GroomedSnow => 0.75,
        _ => 0.92,
    }
}

/// How the road's brightness varies slowly along it (a factor around 1, every few tens of meters),
/// so that its texture's tile never shows as a grid from afar; at `distance` m along a loop of
/// `length` m (the same at its end as at its start).
pub fn tint_along(distance: f32, length: f32) -> f32 {
    let knots = (length / 37.0).round().max(1.0);
    let x = distance / length.max(1.0) * knots;
    let (i, t) = (x.floor(), smooth(x - x.floor()));
    let knots = knots as i32;
    let at = |k: i32| hash(k.rem_euclid(knots), 0, 7);
    let (a, b) = (at(i as i32), at(i as i32 + 1));
    1.0 + 0.12 * (a + (b - a) * t - 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    /// The most a meter of road's edge is stretched (or squeezed) along the texture.
    const MAX_STRETCH: f32 = 2.0;

    fn close(a: Rgb, b: Rgb, tolerance: f32) -> bool {
        (a.0 - b.0).abs() <= tolerance
            && (a.1 - b.1).abs() <= tolerance
            && (a.2 - b.2).abs() <= tolerance
    }

    /// Issue #187: every paving's mean is the road's color (its `asphalt`, in linear light: what
    /// the vignette, the dust and the readability tests take for the road), and it is not flat
    /// (a drawing); every drawing tiles (its left column next to its right one, its top row next
    /// to its bottom one, no more apart than two neighbours inside); every border too.
    #[test]
    fn each_paving_has_the_road_s_mean_color_and_tiles() {
        let base = (0.55, 0.45, 0.35);
        for paving in Paving::ALL {
            let texture = paving_texture(paving, base, Some((0.4, 0.6, 0.3)));
            let mean = texture.mean();
            assert!(close(mean, base, 0.012), "{paving:?}: {mean:?}");
            let n = texture.width;
            let luma = |x: u32, y: u32| {
                let (r, g, b) = texture.texel(x % n, y % n);
                0.3 * r + 0.59 * g + 0.11 * b
            };
            let mut inside = 0.0;
            let mut seam = 0.0;
            let mut spread = 0.0;
            let average = 0.3 * base.0 + 0.59 * base.1 + 0.11 * base.2;
            for k in 0..n {
                for j in [n / 3, 2 * n / 3] {
                    inside +=
                        (luma(j, k) - luma(j + 1, k)).abs() + (luma(k, j) - luma(k, j + 1)).abs();
                }
                seam += (luma(n - 1, k) - luma(0, k)).abs() + (luma(k, n - 1) - luma(k, 0)).abs();
                spread += (luma(k, k) - average).abs();
            }
            assert!(
                seam < inside * 0.75 + 1.0,
                "{paving:?}: its seam {seam}, inside {inside}"
            );
            assert!(spread / n as f32 > 0.01, "{paving:?}: flat ({spread})");
            assert_eq!(
                texture.data.len() as u32,
                (0..PAVING_MIPS)
                    .map(|m| (n >> m) * (n >> m) * 4)
                    .sum::<u32>()
            );
        }
        for border in Border::ALL {
            let texture = border_texture(border, ((0.6, 0.3, 0.2), (0.95, 0.9, 0.8)));
            let (w, h) = BORDER_SIZE;
            assert_eq!(
                texture.data.len() as u32,
                (0..BORDER_MIPS)
                    .map(|m| (w >> m) * (h >> m) * 4)
                    .sum::<u32>()
            );
            // Across the seam, and across the same places of the pattern inside it (a quarter, a
            // half, three quarters of a tile along: a stripe's end, the gap between two logs).
            let across = |y0: u32, y1: u32| {
                (0..w)
                    .map(|x| {
                        let (a, b) = (texture.texel(x, y0), texture.texel(x, y1));
                        (a.0 - b.0).abs() + (a.1 - b.1).abs()
                    })
                    .sum::<f32>()
            };
            let seam = across(h - 1, 0);
            let inside = [h / 4, h / 2, 3 * h / 4]
                .map(|y| across(y - 1, y))
                .into_iter()
                .fold(0.0f32, f32::max);
            assert!(
                seam < inside * 1.5 + 2.0,
                "{border:?}: its seam {seam}, inside {inside}"
            );
        }
    }

    /// The circuits of the game, read from their files.
    fn game_circuits() -> Vec<(String, crate::track::TrackData)> {
        crate::admin::tracks::circuits()
            .iter()
            .map(|entry| {
                let path = format!(
                    "{}/assets/{}",
                    env!("CARGO_MANIFEST_DIR"),
                    entry.asset_path()
                );
                let text = std::fs::read_to_string(&path).unwrap();
                (
                    entry.name.clone(),
                    crate::track::TrackData::from_ron(&text).unwrap(),
                )
            })
            .collect()
    }

    /// Issue #187 acceptance: every biome declares its road (its paving and its border: the
    /// field is required, `biome::parse_biomes`), and every one of its textures is made by the
    /// game (no file: nothing to credit), at its size with its mipmaps, opaque. So does every
    /// circuit of the game (its biome's road, or its own: the Ruban Réglisse's liquorice). Each
    /// biome has its own paving: no two the same, and none the plain asphalt of before.
    #[test]
    fn each_biome_has_its_road_made_by_the_game() {
        let biomes = crate::track::biome::biomes();
        assert_eq!(biomes.len(), 10);
        let mut seen: Vec<Paving> = Vec::new();
        for biome in biomes {
            let road = biome.road;
            assert_ne!(road.paving, Paving::Asphalt, "{}", biome.name);
            assert!(
                !seen.contains(&road.paving),
                "{}: {:?} twice",
                biome.name,
                road.paving
            );
            seen.push(road.paving);
        }
        let mut drawn = Vec::new();
        for (name, data) in game_circuits() {
            let look = data.road_look();
            let palette = data.palette();
            if data.road.is_none() {
                assert_eq!(look, data.biome().unwrap().road, "{name}");
                assert_eq!(palette, data.biome().unwrap().palette, "{name}");
            }
            let key = (
                look.paving,
                look.border,
                palette.asphalt,
                palette.curbs,
                look.accent,
            );
            if drawn.contains(&key) {
                continue;
            }
            drawn.push(key);
            let paving = paving_texture(look.paving, palette.asphalt, look.accent);
            let border = border_texture(look.border, palette.curbs);
            for (texture, (w, h), mips) in [
                (&paving, (PAVING_SIZE, PAVING_SIZE), PAVING_MIPS),
                (&border, BORDER_SIZE, BORDER_MIPS),
            ] {
                assert_eq!((texture.width, texture.height, texture.mips), (w, h, mips));
                let bytes: u32 = (0..mips)
                    .map(|m| (w >> m).max(1) * (h >> m).max(1) * 4)
                    .sum();
                assert_eq!(texture.data.len() as u32, bytes, "{name}");
                assert!(texture.data.chunks(4).all(|t| t[3] == 255), "{name}");
            }
        }
        // The Ruban Réglisse: its own liquorice, in the pastry land.
        let ruban = game_circuits()
            .into_iter()
            .find(|(name, _)| name == "Ruban Réglisse")
            .unwrap()
            .1;
        assert_eq!(ruban.road_look().paving, Paving::Licorice);
        assert_eq!(ruban.biome().unwrap().name, "Guimauve");
        assert_ne!(
            ruban.palette().asphalt,
            ruban.biome().unwrap().palette.asphalt
        );
        assert_eq!(
            ruban.palette().ground,
            ruban.biome().unwrap().palette.ground
        );
        assert!(seen.len() < Paving::ALL.len());
    }

    /// The distance between two sRGB colors (0 to 1 a channel), as the items' readability test
    /// of #106 measures it.
    fn apart(a: Rgb, b: Rgb) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
    }

    /// The road stands out this far at least (`apart`) from what lies beside it.
    const ROAD_APART: f32 = 0.25;

    /// Issue #187 acceptance (readability first): on every circuit of the game, the road as drawn
    /// (its paving's mean color, measured on its texture) stands out from its shoulders, its
    /// ground (the grass) and its sand patches; its accent (the moss, the leaves, the sugar's
    /// stripes) from its shoulders and its ground too; and what marks the road stands out on it:
    /// the boost pads, the anti-gravity zones' stripes and curbs, the sand patches (above). The
    /// items keep theirs: `items::icons::the_items_on_the_track_stand_out_from_the_decor` (no
    /// threshold lowered, the circuits' own roads in it).
    #[test]
    fn the_road_stands_out_from_its_shoulders_and_its_marks_from_it() {
        use crate::track::circuit::{ANTI_GRAVITY_PAD_COLOR, BOOST_PAD_COLOR};
        use crate::track::mesh::{ANTI_GRAVITY_CURBS, ANTI_GRAVITY_STRIPE};
        let mut problems = Vec::new();
        let mut check = |what: String, a: Rgb, b: Rgb| {
            let d = apart(a, b);
            println!("{what}: {d:.3}");
            if d < ROAD_APART {
                problems.push(format!("{what}: {d:.3}"));
            }
        };
        for (name, data) in game_circuits() {
            let look = data.road_look();
            let p = data.palette();
            let road = paving_texture(look.paving, p.asphalt, look.accent).mean();
            assert!(
                apart(road, p.asphalt) < 0.02,
                "{name}: {road:?} {:?}",
                p.asphalt
            );
            for (beside, color) in [
                ("shoulder", p.shoulder),
                ("ground", p.ground),
                ("sand", p.sand),
            ] {
                check(format!("{name}: road / {beside}"), road, color);
            }
            if let Some(accent) = look.accent {
                for (beside, color) in [("shoulder", p.shoulder), ("ground", p.ground)] {
                    check(format!("{name}: its accent / {beside}"), accent, color);
                }
            }
            check(format!("{name}: boost pad / road"), BOOST_PAD_COLOR, road);
            if !data.anti_gravity.is_empty() {
                check(
                    format!("{name}: anti-gravity stripe / road"),
                    ANTI_GRAVITY_STRIPE,
                    road,
                );
                check(
                    format!("{name}: anti-gravity pad / road"),
                    ANTI_GRAVITY_PAD_COLOR,
                    road,
                );
                let (blue, white) = ANTI_GRAVITY_CURBS;
                let curb = (
                    (blue.0 + white.0) / 2.0,
                    (blue.1 + white.1) / 2.0,
                    (blue.2 + white.2) / 2.0,
                );
                check(format!("{name}: anti-gravity curbs / road"), curb, road);
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// Issue #187: the paving's UVs never stretch the road's drawing much. A meter of road is a
    /// meter of texture (`world / texture`, along each side of each quad), on every circuit of the
    /// game and its mirror:
    /// - **a paving that lies on the ground** (earth, sand, slabs, cracks, leaves: its UVs the
    ///   ground's meters) is not stretched at all in the turns, however tight: only on a slope (a
    ///   bump's, 1 / cos of it) and in the anti-gravity zones (where it follows the road): less
    ///   than 2 % of the road's area is stretched more than 1.3 times;
    /// - **a paving that follows the road** (planks and cobbles in rows across it, which fan out in
    ///   a turn as they would be laid; stripes and ridges along it, which a stretch along the road
    ///   does not change): across, exactly; along, its outer edge stretched as much as its inner
    ///   one is squeezed (`mesh::paving_rate`), never more than [`MAX_STRETCH`] in the tightest
    ///   hairpins;
    /// - **the borders**: their own meters along their middle (their edges within 1.6).
    #[test]
    fn the_paving_never_stretches_much() {
        use crate::track::layout::CenterLine;
        use crate::track::mesh::generate;
        let mut worst: (f32, String) = (1.0, String::new());
        let mut lying = (0.0f32, 0.0f32);
        for (name, data) in game_circuits() {
            let follows = data.road_look().paving.follows_the_road();
            for data in [data.clone(), data.mirrored()] {
                let center = CenterLine::of(&data);
                let meshes = generate(&center, &data, &data.palette());
                for (what, painted) in [("paving", &meshes.paving), ("borders", &meshes.borders)] {
                    let m = &painted.mesh;
                    assert_eq!(painted.uvs.len(), m.positions.len(), "{name}");
                    assert!(!m.indices.is_empty(), "{name}: {what}");
                    // Each quad: corners 0 → 1 across at its start, 3 → 2 at its end.
                    for q in 0..m.positions.len() / 4 {
                        let p = |k: usize| Vec3::from_array(m.positions[q * 4 + k]);
                        let uv = |k: usize| painted.uvs[q * 4 + k];
                        let mut quad: f32 = 1.0;
                        let edges: &[(usize, usize)] = if what == "paving" {
                            &[(0, 1), (0, 3), (1, 2)]
                        } else {
                            &[(0, 3), (1, 2)]
                        };
                        for &(a, b) in edges {
                            let world = p(a).distance(p(b));
                            let (du, dv) = (uv(b)[0] - uv(a)[0], uv(b)[1] - uv(a)[1]);
                            let texture = if what == "paving" {
                                (du * du + dv * dv).sqrt() * PAVING_TILE
                            } else {
                                dv.abs() * BORDER_TILE
                            };
                            if world < 0.05 {
                                continue;
                            }
                            let stretch = world / texture;
                            quad = quad.max(stretch.max(1.0 / stretch));
                        }
                        if what == "borders" {
                            assert!(quad < 1.6, "{name}: a border stretched {quad}");
                            continue;
                        }
                        if follows {
                            if quad > worst.0 {
                                worst = (quad, format!("{name} (mirror {})", data.mirrored));
                            }
                        } else {
                            let area =
                                p(0).distance(p(1)) * p(0).distance(p(3)).max(p(1).distance(p(2)));
                            lying.0 += area;
                            if quad > 1.3 {
                                lying.1 += area;
                            }
                        }
                    }
                }
            }
        }
        println!("following the road, the worst stretch: {worst:?}");
        println!(
            "lying on the ground, stretched more than 1.3: {:.2} %",
            100.0 * lying.1 / lying.0
        );
        assert!(worst.0 < MAX_STRETCH, "{worst:?}");
        assert!(lying.1 / lying.0 < 0.02, "{} %", 100.0 * lying.1 / lying.0);
    }

    /// Not a test: writes every biome's textures (`FLUFFY_PAVING_DUMP` folder), raw RGB.
    #[test]
    #[ignore]
    fn dump_textures() {
        let dir = std::env::var("FLUFFY_PAVING_DUMP").unwrap();
        let mut looks: Vec<(String, crate::track::biome::RoadLook, Rgb, (Rgb, Rgb))> =
            crate::track::biome::biomes()
                .iter()
                .map(|b| (b.name.clone(), b.road, b.palette.asphalt, b.palette.curbs))
                .collect();
        let ruban =
            crate::track::TrackData::from_ron(include_str!("../../assets/tracks/ruban.track.ron"))
                .unwrap();
        looks.push((
            "Ruban".into(),
            ruban.road_look(),
            ruban.palette().asphalt,
            ruban.palette().curbs,
        ));
        for (name, look, base, curbs) in looks {
            let started = std::time::Instant::now();
            let p = paving_texture(look.paving, base, look.accent);
            let b = border_texture(look.border, curbs);
            println!("{name}: {:?} {:?} {:?}", started.elapsed(), p.mean(), base);
            for (what, t) in [("paving", &p), ("border", &b)] {
                let rgb: Vec<u8> = t.data[..(t.width * t.height * 4) as usize]
                    .chunks(4)
                    .flat_map(|c| [c[0], c[1], c[2]])
                    .collect();
                std::fs::write(
                    format!("{dir}/{name}-{what}-{}x{}.rgb", t.width, t.height),
                    rgb,
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn the_colors_go_to_linear_and_back() {
        for c in [0.0, 0.02, 0.2, 0.5, 0.9, 1.0] {
            assert!((to_srgb(to_linear(c)) - c).abs() < 1e-4, "{c}");
        }
        for d in [0.0, 13.0, 200.0, 1500.0] {
            let k = tint_along(d, 1500.0);
            assert!((tint_along(d + 1500.0, 1500.0) - k).abs() < 1e-4, "{d}");
            assert!((0.93..=1.07).contains(&k), "{d}: {k}");
        }
    }
}
