//! Issue #185: **the drawings of the smoke**, made by the game at startup (pure, unit-tested: no
//! file, no crate), in one texture of [`VARIANTS`] cells side by side, with its mipmaps (a puff far
//! away stays a soft puff, never a flickering pixel), **alpha premultiplied** (its color already
//! times its opacity: the mipmaps and the filtering never darken nor lighten its edge).
//!
//! Each puff is a cloud of fractal noise (fBm: value noise, five octaves): a round body whose
//! outline is warped by a coarse noise, its density broken up by a finer one, its edge fading
//! smoothly to nothing well inside its cell (no quad shows its border). Its brightness has the
//! relief of its lumps (lighter where it is thick, a little darker in its hollows); the light of
//! the sun and the shade under it are the corners' colors (`batch.rs`), so a puff that turns stays
//! lit from above. Four variants, all different, picked at random (deterministically) per puff.

/// The number of drawings (one per cell, side by side).
pub const VARIANTS: u32 = 4;
/// A cell's side (px) at full size.
pub const SIZE: u32 = 128;
/// The mipmaps, the full size included (down to cells of 4 px).
pub const MIPS: u32 = 6;

/// The texture's size at full size (px).
pub fn smoke_size() -> (u32, u32) {
    (VARIANTS * SIZE, SIZE)
}

/// The corners of `variant`'s cell (u0, v0, u1, v1), a texel and a half inside (its neighbours
/// never bleed in).
pub fn uv(variant: u8) -> [f32; 4] {
    let column = u32::from(variant) % VARIANTS;
    let (w, h) = smoke_size();
    let inset = 1.5;
    let x = (column * SIZE) as f32;
    [
        (x + inset) / w as f32,
        inset / h as f32,
        (x + SIZE as f32 - inset) / w as f32,
        (SIZE as f32 - inset) / h as f32,
    ]
}

/// A number from 0 to 1 for the corner (`x`, `y`) of the noise's grid.
fn hash(x: i64, y: i64, seed: u32) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x8DA6_B343)
        ^ (y as u64).wrapping_mul(0xD816_3841)
        ^ u64::from(seed.wrapping_mul(0xCB1A_B31F));
    h &= 0xFFFF_FFFF;
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995) & 0xFFFF_FFFF;
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_215.0
}

fn quintic(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise (0 to 1), smooth.
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (quintic(x - xi), quintic(y - yi));
    let (xi, yi) = (xi as i64 & 0xFFFF, yi as i64 & 0xFFFF);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1, yi, seed);
    let c = hash(xi, yi + 1, seed);
    let d = hash(xi + 1, yi + 1, seed);
    let top = a + (b - a) * fx;
    let bottom = c + (d - c) * fx;
    top + (bottom - top) * fy
}

/// Fractal noise (0 to 1): `octaves` of the value noise, each twice finer and half as strong.
fn fbm(mut x: f32, mut y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += amplitude * noise(x, y, seed.wrapping_add(octave * 101));
        total += amplitude;
        amplitude *= 0.5;
        x = x * 2.03 + 17.1;
        y = y * 2.03 + 3.7;
    }
    sum / total
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `variant`'s drawing at (`u`, `v`), both from 0 to 1 across its cell: its brightness and its
/// opacity (not premultiplied).
pub fn texel(variant: u8, u: f32, v: f32) -> (f32, f32) {
    let seed = 0x51ED + u32::from(variant) * 7919;
    let (x, y) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
    // Its outline, warped by a coarse noise.
    let wx = fbm(u * 2.5 + 11.3, v * 2.5 + 2.1, seed, 4) - 0.5;
    let wy = fbm(u * 2.5 + 5.7, v * 2.5 + 9.4, seed ^ 0xABC, 4) - 0.5;
    let (qx, qy) = (x + 0.42 * wx, y + 0.42 * wy);
    let body = (1.0 - (qx * qx + qy * qy)).clamp(0.0, 1.0).powf(1.5);
    // Its density, broken up by a finer one.
    let detail = fbm(u * 4.5 + 3.3, v * 4.5 + 7.7, seed ^ 0x5F3, 5);
    let density = body * (0.85 + 1.5 * (detail - 0.5));
    // Nothing near its cell's edge, whatever the warp.
    let edge = 1.0 - smoothstep(0.78, 0.99, (x * x + y * y).sqrt());
    let alpha = smoothstep(0.03, 0.75, density) * edge;
    let fine = fbm(u * 9.0 + 1.9, v * 9.0 + 4.4, seed ^ 0x77, 3);
    let shade = (0.7 + 0.45 * (detail - 0.5) + 0.25 * (fine - 0.5) + 0.12 * body).clamp(0.55, 1.0);
    (shade, alpha)
}

/// The texture, RGBA (linear, 8 bits a channel, **premultiplied**: its brightness times its
/// opacity in RGB), its mipmaps after it (each half the one before), as the GPU takes it.
pub fn smoke_pixels() -> Vec<u8> {
    let (w, h) = smoke_size();
    let mut levels: Vec<Vec<[f32; 4]>> = Vec::with_capacity(MIPS as usize);
    let mut full = vec![[0.0f32; 4]; (w * h) as usize];
    for variant in 0..VARIANTS {
        for py in 0..SIZE {
            for px in 0..SIZE {
                let u = (px as f32 + 0.5) / SIZE as f32;
                let v = (py as f32 + 0.5) / SIZE as f32;
                let (shade, alpha) = texel(variant as u8, u, v);
                let grey = shade * alpha;
                full[(py * w + variant * SIZE + px) as usize] = [grey, grey, grey, alpha];
            }
        }
    }
    levels.push(full);
    let (mut lw, mut lh) = (w, h);
    for _ in 1..MIPS {
        let last = levels.last().expect("the full size");
        let (nw, nh) = (lw / 2, lh / 2);
        let mut next = vec![[0.0f32; 4]; (nw * nh) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let texel = last[((y * 2 + dy) * lw + x * 2 + dx) as usize];
                    for c in 0..4 {
                        sum[c] += texel[c] * 0.25;
                    }
                }
                next[(y * nw + x) as usize] = sum;
            }
        }
        levels.push(next);
        (lw, lh) = (nw, nh);
    }
    levels
        .iter()
        .flatten()
        .flat_map(|texel| texel.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #185: every puff is soft (nothing at the edges of its cell: a quad never shows its
    /// border, no neighbour bleeds in), thick in its middle and thinning out toward its edge, its
    /// opacity smooth (no hard step from one texel to the next anywhere), and the four drawings
    /// differ.
    #[test]
    fn the_puffs_are_soft_and_all_different() {
        let n = SIZE as usize;
        let at = |i: usize| (i as f32 + 0.5) / n as f32;
        let mut cells = Vec::new();
        for variant in 0..VARIANTS as u8 {
            let mut alpha = vec![0.0f32; n * n];
            for y in 0..n {
                for x in 0..n {
                    alpha[y * n + x] = texel(variant, at(x), at(y)).1;
                }
            }
            for k in 0..n {
                for (x, y) in [(k, 0), (k, n - 1), (0, k), (n - 1, k)] {
                    assert!(alpha[y * n + x] < 0.01, "{variant}: its edge at ({x}, {y})");
                }
            }
            // Thick in the middle, thin toward the edge (rings of the cell).
            let ring = |from: f32, to: f32| {
                let (mut sum, mut count) = (0.0, 0);
                for y in 0..n {
                    for x in 0..n {
                        let r = ((at(x) * 2.0 - 1.0).powi(2) + (at(y) * 2.0 - 1.0).powi(2)).sqrt();
                        if (from..to).contains(&r) {
                            sum += alpha[y * n + x];
                            count += 1;
                        }
                    }
                }
                sum / count as f32
            };
            let (middle, half, rim) = (ring(0.0, 0.25), ring(0.45, 0.6), ring(0.8, 1.0));
            assert!(
                middle > 0.6 && half < middle && rim < half * 0.5 && rim < 0.12,
                "{variant}: {middle} {half} {rim}"
            );
            // Smooth: no hard step between neighbours (a soft edge, not a cut-out).
            for y in 0..n {
                for x in 1..n {
                    let step = (alpha[y * n + x] - alpha[y * n + x - 1]).abs();
                    assert!(step < 0.2, "{variant}: a step of {step} at ({x}, {y})");
                }
            }
            let mean = alpha.iter().sum::<f32>() / alpha.len() as f32;
            assert!((0.15..0.45).contains(&mean), "{variant}: {mean}");
            cells.push(alpha);
        }
        for a in 0..cells.len() {
            for b in a + 1..cells.len() {
                let apart: f32 = cells[a]
                    .iter()
                    .zip(&cells[b])
                    .map(|(x, y)| (x - y).abs())
                    .sum::<f32>()
                    / cells[a].len() as f32;
                assert!(apart > 0.05, "{a} and {b} look alike: {apart}");
            }
        }
        // Its relief: lighter and darker lumps, never black.
        let shades: Vec<f32> = (0..n).map(|x| texel(0, at(x), 0.5).0).collect();
        let (low, high) = shades
            .iter()
            .fold((1.0f32, 0.0f32), |(l, h), s| (l.min(*s), h.max(*s)));
        assert!(
            low >= 0.55 && high <= 1.0 && high - low > 0.1,
            "{low} {high}"
        );
        for variant in 0..VARIANTS as u8 {
            let [u0, v0, u1, v1] = uv(variant);
            assert!(u0 >= 0.0 && v0 >= 0.0 && u1 <= 1.0 && v1 <= 1.0 && u0 < u1 && v0 < v1);
        }
    }

    /// The texture: premultiplied (its color never brighter than its opacity), as many bytes as
    /// the GPU expects with its mipmaps, the smallest level still showing each puff.
    #[test]
    fn the_texture_is_premultiplied_with_its_mipmaps() {
        let (w, h) = smoke_size();
        let pixels = smoke_pixels();
        let expected: u32 = (0..MIPS).map(|l| (w >> l) * (h >> l) * 4).sum();
        assert_eq!(pixels.len() as u32, expected);
        for texel in pixels.chunks(4) {
            assert!(texel[0] <= texel[3] && texel[0] == texel[1] && texel[1] == texel[2]);
        }
        let offset: u32 = (0..MIPS - 1).map(|l| (w >> l) * (h >> l) * 4).sum();
        let lw = w >> (MIPS - 1);
        let cell = SIZE >> (MIPS - 1);
        for variant in 0..VARIANTS {
            let (x, y) = (variant * cell + cell / 2, cell / 2);
            let alpha = pixels[(offset + (y * lw + x) * 4 + 3) as usize];
            assert!(alpha > 60, "{variant}: {alpha}");
        }
    }
}
