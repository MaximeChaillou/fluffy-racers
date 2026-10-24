//! Issue #174: the drawings of the glowing particles, made by the game at startup (pure,
//! unit-tested: no file, no crate), in one texture of 4 × 2 cells (`Cell`), with its mipmaps (a
//! spark far away stays a soft spark, not a flickering pixel).
//!
//! The glows are drawn in grey on a transparent ground, their brightness in all four channels:
//! the particle's color (above 1 where it burns) times it, so a core at full brightness clamps to
//! white and its edge shows the color (`batch.rs`). Issue #185: the smoke's drawings (two lumpy
//! blobs here before) are a texture of their own (`smoke.rs`).

/// The cells of the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cell {
    /// A spark's streak: `u` across it (a thin white core, a wider halo), `v` along it (its head
    /// at 0, bright, its tail fading to 1).
    Streak,
    /// A soft round glow (a flash, an ember).
    Soft,
    /// Two tongues of flame: their base at `v` 0 (wide, hot), their tip at 1.
    FlameA,
    FlameB,
    /// A glowing ring (a shockwave).
    Ring,
    /// A wide soft disc: the halo on the ground under a kart.
    Halo,
}

impl Cell {
    pub const ALL: [Cell; 6] = [
        Cell::Streak,
        Cell::Soft,
        Cell::FlameA,
        Cell::FlameB,
        Cell::Ring,
        Cell::Halo,
    ];
    pub const COLUMNS: u32 = 4;
    pub const ROWS: u32 = 2;
    /// A cell's side (px) at full size.
    pub const SIZE: u32 = 128;
    /// The atlas's mipmaps, the full size included (down to cells of 8 px).
    pub const MIPS: u32 = 5;

    pub fn index(self) -> u32 {
        self as u32
    }

    /// Its corners in the atlas (u0, v0, u1, v1), a texel inside (its neighbours never bleed in
    /// at full size).
    pub fn uv(self) -> [f32; 4] {
        let (column, row) = (self.index() % Self::COLUMNS, self.index() / Self::COLUMNS);
        let (w, h) = (
            (Self::COLUMNS * Self::SIZE) as f32,
            (Self::ROWS * Self::SIZE) as f32,
        );
        let inset = 1.5;
        let x = (column * Self::SIZE) as f32;
        let y = (row * Self::SIZE) as f32;
        [
            (x + inset) / w,
            (y + inset) / h,
            (x + Self::SIZE as f32 - inset) / w,
            (y + Self::SIZE as f32 - inset) / h,
        ]
    }

    /// Its drawing at (`u`, `v`), both from 0 to 1 across the cell: its brightness (grey) and its
    /// opacity.
    pub fn texel(self, u: f32, v: f32) -> (f32, f32) {
        let x = u * 2.0 - 1.0;
        let y = v * 2.0 - 1.0;
        let r = (x * x + y * y).sqrt();
        let glow = |p: f32| {
            let p = p.clamp(0.0, 1.0);
            (p, p)
        };
        match self {
            Cell::Streak => {
                let across = (gauss(x, 0.16) + 0.35 * gauss(x, 0.5)) * (1.0 - x * x).max(0.0);
                let head = smoothstep(0.0, 0.1, v);
                let along =
                    head * (1.0 - v).max(0.0).powf(1.2) * (1.0 + 0.5 * gauss(v - 0.08, 0.08));
                glow(across * along)
            }
            Cell::Soft => {
                glow((0.85 * gauss(r, 0.28) + 0.35 * gauss(r, 0.62)) * (1.0 - r * r).max(0.0))
            }
            Cell::FlameA | Cell::FlameB => {
                let b = self == Cell::FlameB;
                // The base at v 0: wide, rounded; it narrows to a licking tip.
                let width = 0.85
                    * (1.0 - v).max(0.0).powf(if b { 1.1 } else { 0.8 })
                    * smoothstep(-0.08, 0.2, v);
                let phase = if b { 2.1 } else { 0.0 };
                let middle = (if b { 0.2 } else { 0.13 }) * v * (v * 6.0 + phase).sin();
                let d = (x - middle).abs() / width.max(1e-3);
                let body = (1.0 - d * d).max(0.0).powf(0.35);
                let core = gauss(d, 0.35) * (1.0 - v).max(0.0).powi(2);
                // Licks: brighter and darker bands rising along it.
                let licks = 0.82 + 0.18 * (v * 13.0 - x * 3.0 + phase).sin();
                let fade = (1.0 - v).max(0.0).powf(0.6) * smoothstep(0.0, 0.08, v);
                glow((body * (0.6 + 0.4 * (1.0 - v)) * licks + 0.5 * core) * fade)
            }
            Cell::Ring => glow(
                (gauss(r - 0.78, 0.07) + 0.2 * gauss(r - 0.62, 0.22)) * smoothstep(1.0, 0.9, r),
            ),
            Cell::Halo => glow((1.0 - r * r).max(0.0).powf(1.2) * 0.8 + 0.2 * gauss(r, 0.3)),
        }
    }
}

fn gauss(x: f32, width: f32) -> f32 {
    (-(x / width).powi(2)).exp()
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The atlas's size at full size (px).
pub fn atlas_size() -> (u32, u32) {
    (Cell::COLUMNS * Cell::SIZE, Cell::ROWS * Cell::SIZE)
}

/// The atlas, RGBA (linear, 8 bits a channel), its mipmaps after it (each half the one before),
/// as the GPU takes it.
pub fn atlas_pixels() -> Vec<u8> {
    let (w, h) = atlas_size();
    let mut levels: Vec<Vec<[f32; 4]>> = Vec::with_capacity(Cell::MIPS as usize);
    let mut full = vec![[0.0f32; 4]; (w * h) as usize];
    for cell in Cell::ALL {
        let (cx, cy) = (
            cell.index() % Cell::COLUMNS * Cell::SIZE,
            cell.index() / Cell::COLUMNS * Cell::SIZE,
        );
        for py in 0..Cell::SIZE {
            for px in 0..Cell::SIZE {
                let u = (px as f32 + 0.5) / Cell::SIZE as f32;
                let v = (py as f32 + 0.5) / Cell::SIZE as f32;
                let (grey, alpha) = cell.texel(u, v);
                full[((cy + py) * w + cx + px) as usize] = [grey, grey, grey, alpha];
            }
        }
    }
    levels.push(full);
    let (mut lw, mut lh) = (w, h);
    for _ in 1..Cell::MIPS {
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

    /// Every drawing is empty at the edges of its cell (no neighbour bleeds in, a quad has no
    /// visible edge), the glows are brightest in their middle, the streak at its head, the flames
    /// at their base; the cells do not overlap and stay inside the atlas.
    #[test]
    fn the_drawings_glow_in_their_middle_and_fade_at_their_edges() {
        for cell in Cell::ALL {
            for k in 0..=20 {
                let t = k as f32 / 20.0;
                for (u, v) in [(t, 0.0), (t, 1.0), (0.0, t), (1.0, t)] {
                    let (_, alpha) = cell.texel(u, v);
                    assert!(alpha < 0.06, "{cell:?} at ({u}, {v}): {alpha}");
                }
            }
            let [u0, v0, u1, v1] = cell.uv();
            assert!(u0 >= 0.0 && v0 >= 0.0 && u1 <= 1.0 && v1 <= 1.0 && u0 < u1 && v0 < v1);
        }
        for cell in [Cell::Soft, Cell::Halo] {
            let (_, middle) = cell.texel(0.5, 0.5);
            let (_, aside) = cell.texel(0.8, 0.5);
            assert!(middle > 0.8 && middle > aside, "{cell:?}: {middle} {aside}");
        }
        let (head, _) = Cell::Streak.texel(0.5, 0.1);
        let (tail, _) = Cell::Streak.texel(0.5, 0.8);
        let (edge, _) = Cell::Streak.texel(0.8, 0.1);
        assert!(
            head > 0.9 && tail < head * 0.4 && edge < head * 0.3,
            "{head} {tail} {edge}"
        );
        for flame in [Cell::FlameA, Cell::FlameB] {
            let (base, _) = flame.texel(0.5, 0.2);
            let (tip, _) = flame.texel(0.5, 0.85);
            assert!(base > 0.7 && tip < base * 0.5, "{flame:?}: {base} {tip}");
        }
        let (ring, _) = Cell::Ring.texel(0.5 + 0.39, 0.5);
        let (inside, _) = Cell::Ring.texel(0.5, 0.5);
        assert!(ring > 0.9 && inside < 0.2, "{ring} {inside}");
    }

    /// The atlas and its mipmaps: as many bytes as the GPU expects, the smallest level still
    /// showing each cell.
    #[test]
    fn the_atlas_has_its_mipmaps() {
        let (w, h) = atlas_size();
        let pixels = atlas_pixels();
        let expected: u32 = (0..Cell::MIPS).map(|l| (w >> l) * (h >> l) * 4).sum();
        assert_eq!(pixels.len() as u32, expected);
        // The last level: 8 px cells; the soft glow's middle is still lit.
        let offset: u32 = (0..Cell::MIPS - 1).map(|l| (w >> l) * (h >> l) * 4).sum();
        let lw = w >> (Cell::MIPS - 1);
        let cell = Cell::SIZE >> (Cell::MIPS - 1);
        let (x, y) = (cell + cell / 2, cell / 2);
        let alpha = pixels[(offset + (y * lw + x) * 4 + 3) as usize];
        assert!(alpha > 100, "{alpha}");
    }
}
