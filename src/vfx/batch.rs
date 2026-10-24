//! Issue #174: the glowing particles and the smoke, **drawn as quads of two shared meshes** (pure,
//! unit-tested; the systems copy the quads into the meshes each frame). One draw for every glow
//! (additive: their order does not matter, nothing to sort), one for the smoke (blended: sorted
//! back to front here). Before, each particle was an entity of its own: a draw call each, and a
//! transparent particle a sorted one, which the browser (WebGL2, one thread) would pay for.
//!
//! Each quad faces the camera as its shape says (`pool::Shape`): a spark is a streak along its
//! flight, turned toward the camera around it; a flame rises from its base the same way; a glow
//! and a puff face it; a ring lies across its axis. Its color is its own, per corner: its paint
//! (linear), cooled toward its deep color as it dies, times how bright it still is (above 1, the
//! atlas's white core clamps to white: `atlas.rs`). The halos under the karts are quads of the
//! glow mesh too, given by the systems each frame (`GroundHalo`).
//!
//! The buffers are made once, at their most (the pool's slots and the halos): nothing is
//! allocated while racing.

use bevy::prelude::*;

use super::atlas::Cell;
use super::pool::{Batch, Paint, Particle, Shape};

/// The most halos in a frame (a halo per kart: 12 on the grid, the online players' too).
pub const MAX_HALOS: usize = 32;

/// The linear colors of the paints and of their deep colors (what a spark cools to), as the
/// quads take them.
#[derive(Clone, Debug, PartialEq)]
pub struct QuadColors {
    pub paint: [Vec3; Paint::COUNT],
    pub deep: [Vec3; Paint::COUNT],
}

impl Default for QuadColors {
    fn default() -> Self {
        Self {
            paint: [Vec3::ONE; Paint::COUNT],
            deep: [Vec3::splat(0.5); Paint::COUNT],
        }
    }
}

/// The linear color of an sRGB one.
pub fn linear([r, g, b]: [f32; 3]) -> Vec3 {
    let c = Color::srgb(r, g, b).to_linear();
    Vec3::new(c.red, c.green, c.blue)
}

/// A halo on the ground under a kart (issue #174: a false light of its drift's tier).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroundHalo {
    /// Its middle (world), a little over the ground, and the ground's up there.
    pub center: Vec3,
    pub up: Vec3,
    /// Its width (m), its linear color times its brightness.
    pub size: f32,
    pub color: Vec3,
}

/// The quads of one mesh: four corners each, their positions (around `origin`), texture
/// coordinates and colors. At most `cap` quads.
#[derive(Clone, Debug)]
pub struct Quads {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    cap: usize,
}

impl Quads {
    pub fn new(cap: usize) -> Self {
        Self {
            positions: Vec::with_capacity(cap * 4),
            uvs: Vec::with_capacity(cap * 4),
            colors: Vec::with_capacity(cap * 4),
            cap,
        }
    }

    pub fn clear(&mut self) {
        self.positions.clear();
        self.uvs.clear();
        self.colors.clear();
    }

    pub fn len(&self) -> usize {
        self.positions.len() / 4
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    /// One quad: its corners in order around it, `cell`'s drawing on it (`u` across the first
    /// edge, `v` from the first two corners to the last two), each corner's color. Ignored when
    /// full (never grows), or when a corner is not a number.
    pub fn push(&mut self, corners: [Vec3; 4], cell: Cell, colors: [Vec4; 4]) {
        if self.len() >= self.cap || !corners.iter().all(|c| c.is_finite()) {
            return;
        }
        let [u0, v0, u1, v1] = cell.uv();
        self.positions.extend(corners.map(|c| c.to_array()));
        self.uvs.extend([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
        self.colors.extend(colors.map(|c| c.to_array()));
    }

    /// An empty quad (the meshes never are empty: the GPU wants a buffer).
    pub fn push_nothing(&mut self) {
        let colors = [Vec4::ZERO; 4];
        self.push([Vec3::ZERO; 4], Cell::Soft, colors);
    }
}

/// The indices of `quads` quads (two triangles each), into `indices` (cleared, never grown past
/// its capacity: the most quads, made once).
pub fn quad_indices(indices: &mut Vec<u16>, quads: usize) {
    let quads = quads.min(indices.capacity() / 6);
    indices.truncate(quads * 6);
    for q in indices.len() / 6..quads {
        let k = (q * 4) as u16;
        indices.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
}

/// How bright a glowing particle still is at `t` (0 to 1 of its life): full at first, then it
/// dies out (and shrinks with its curve).
fn brightness(particle: &Particle, t: f32) -> f32 {
    particle.glow * (1.0 - t * t)
}

/// The cell of a batched particle's shape.
pub fn cell(particle: &Particle) -> Cell {
    match particle.shape {
        Shape::Glint => Cell::Streak,
        Shape::Tongue if particle.variant % 2 == 1 => Cell::FlameB,
        Shape::Tongue => Cell::FlameA,
        Shape::Shock => Cell::Ring,
        Shape::Haze if particle.variant % 2 == 1 => Cell::PuffB,
        Shape::Haze => Cell::PuffA,
        _ => Cell::Soft,
    }
}

/// A side of a streak along `along` at `at`, turned toward `camera` (half of `width`).
fn side(along: Vec3, at: Vec3, camera: Vec3, half: f32) -> Vec3 {
    let toward = camera - at;
    along
        .cross(toward)
        .try_normalize()
        .unwrap_or_else(|| along.any_orthonormal_vector())
        * half
}

/// The two axes of a quad facing `camera` at `at`, turned by `angle`.
fn facing(at: Vec3, camera: Vec3, angle: f32) -> (Vec3, Vec3) {
    let toward = (camera - at).normalize_or(Vec3::Z);
    let right = Vec3::Y.cross(toward).normalize_or(Vec3::X);
    let up = toward.cross(right);
    let (s, c) = angle.sin_cos();
    (right * c + up * s, up * c - right * s)
}

/// The quad of a batched particle (`Shape::batched`), around `origin`, seen from `camera`, into
/// `quads`. Nothing for a particle that is over or has no batch.
pub fn particle_quad(
    particle: &Particle,
    camera: Vec3,
    origin: Vec3,
    colors: &QuadColors,
    quads: &mut Quads,
) {
    let Some(batch) = particle.shape.batched() else {
        return;
    };
    if !particle.alive() {
        return;
    }
    let t = particle.progress();
    let size = particle.current_size().max(0.0);
    let at = particle.position - origin;
    let eye = camera - origin;
    let paint = colors.paint[particle.paint.index()];
    let base = paint.lerp(
        colors.paint[particle.tint_paint.index()],
        particle.tint.clamp(0.0, 1.0),
    );
    let deep = colors.deep[particle.paint.index()];
    let cooled = |k: f32| base.lerp(deep, (particle.cool * k).clamp(0.0, 1.0));
    if batch == Batch::Haze {
        // The smoke: it shows in, then thins out as it spreads.
        let alpha = particle.glow * (t * 8.0).min(1.0) * (1.0 - t).powf(1.3);
        let (right, up) = facing(at, eye, particle.angle);
        let (right, up) = (right * size * 0.5 * particle.stretch, up * size * 0.5);
        let color = base.extend(alpha.clamp(0.0, 1.0));
        quads.push(
            [
                at - right - up,
                at + right - up,
                at + right + up,
                at - right + up,
            ],
            cell(particle),
            [color; 4],
        );
        return;
    }
    let bright = brightness(particle, t);
    match particle.shape {
        Shape::Glint => {
            // Its head where it is, its tail behind, longer the faster it flies.
            let along = particle.velocity.normalize_or(particle.axis);
            let half = size * 1.5;
            let length = size * particle.stretch + particle.velocity.length() * particle.smear;
            let head = at + along * half * 0.6;
            let tail = at - along * length;
            let (sh, st) = (side(along, head, eye, half), side(along, tail, eye, half));
            let head_color = (cooled(t) * bright).extend(1.0);
            let tail_color = (cooled((t + 0.4).min(1.0)) * bright * 0.8).extend(1.0);
            quads.push(
                [head - sh, head + sh, tail + st, tail - st],
                Cell::Streak,
                [head_color, head_color, tail_color, tail_color],
            );
        }
        Shape::Tongue => {
            // From its base (where it is) up along its flight, its tip its deep color.
            let along = particle.velocity.normalize_or(particle.axis);
            let half = size * 0.5;
            let tip = at + along * size * particle.stretch;
            let (sb, stip) = (side(along, at, eye, half), side(along, tip, eye, half));
            let base_color = (cooled(t * 0.5) * bright).extend(1.0);
            let tip_color = (base.lerp(deep, 0.85) * bright * 0.9).extend(1.0);
            quads.push(
                [at - sb, at + sb, tip + stip, tip - stip],
                cell(particle),
                [base_color, base_color, tip_color, tip_color],
            );
        }
        Shape::Shock => {
            let (a, b) = particle.axis.normalize_or(Vec3::Y).any_orthonormal_pair();
            let (s, c) = particle.angle.sin_cos();
            let half = size * 1.25;
            let (a, b) = ((a * c + b * s) * half, (b * c - a * s) * half);
            let color = (cooled(t) * bright).extend(1.0);
            quads.push(
                [at - a - b, at + a - b, at + a + b, at - a + b],
                Cell::Ring,
                [color; 4],
            );
        }
        _ => {
            let (right, up) = facing(at, eye, particle.angle);
            let (right, up) = (right * size * 0.5, up * size * 0.5);
            let color = (cooled(t) * bright).extend(1.0);
            quads.push(
                [
                    at - right - up,
                    at + right - up,
                    at + right + up,
                    at - right + up,
                ],
                Cell::Soft,
                [color; 4],
            );
        }
    }
}

/// A halo on the ground, into the glow's `quads`, around `origin`.
pub fn halo_quad(halo: &GroundHalo, origin: Vec3, quads: &mut Quads) {
    let (a, b) = halo.up.normalize_or(Vec3::Y).any_orthonormal_pair();
    let half = halo.size * 0.5;
    let (a, b) = (a * half, b * half);
    let at = halo.center - origin;
    let color = halo.color.extend(1.0);
    quads.push(
        [at - a - b, at + a - b, at + a + b, at - a + b],
        Cell::Halo,
        [color; 4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfx::pool::{Curve, Facing};

    fn colors() -> QuadColors {
        let mut colors = QuadColors::default();
        colors.paint[Paint::Purple.index()] = Vec3::new(0.6, 0.1, 1.0);
        colors.deep[Paint::Purple.index()] = Vec3::new(1.0, 0.05, 0.4);
        colors
    }

    fn quad(particle: &Particle, camera: Vec3) -> Quads {
        let mut quads = Quads::new(4);
        particle_quad(particle, camera, Vec3::ZERO, &colors(), &mut quads);
        quads
    }

    fn corners(quads: &Quads) -> [Vec3; 4] {
        std::array::from_fn(|i| Vec3::from_array(quads.positions[i]))
    }

    /// A spark is a streak from its head (where it is) back along its flight, longer the faster
    /// it flies, turned toward the camera (its face across the view); its head burns brighter
    /// than its tail, and it cools to its deep color as it dies.
    #[test]
    fn a_spark_streaks_along_its_flight_facing_the_camera() {
        let mut spark = Particle::new(Shape::Glint, Paint::Purple, Vec3::ZERO, 1.0, 0.08);
        spark.velocity = Vec3::new(0.0, 0.0, 6.0);
        spark.stretch = 3.0;
        spark.smear = 0.05;
        spark.glow = 3.0;
        spark.cool = 1.0;
        let camera = Vec3::new(10.0, 0.0, 0.0);
        let q = quad(&spark, camera);
        assert_eq!(q.len(), 1);
        let c = corners(&q);
        // Its head ahead, its tail behind: 0.08 × 3 + 6 × 0.05 = 0.54 m.
        let (head, tail) = ((c[0] + c[1]) / 2.0, (c[2] + c[3]) / 2.0);
        assert!(
            head.z > 0.0 && (tail.z + 0.54).abs() < 1e-4,
            "{head} {tail}"
        );
        // Across the view: its width along Y (the camera is along X).
        assert!((c[1] - c[0]).x.abs() < 1e-5 && (c[1] - c[0]).y.abs() > 0.2);
        let faster = {
            let mut s = spark;
            s.velocity *= 2.0;
            corners(&quad(&s, camera))
        };
        assert!(faster[2].z < c[2].z, "longer when faster");
        let [head_color, _, tail_color, _] = std::array::from_fn(|i| Vec4::from(q.colors[i]));
        assert!(head_color.x > 1.0, "its core burns white: {head_color}");
        assert!(tail_color.length() < head_color.length());
        // Dying, it cools toward its deep color (more red than blue now).
        let mut old = spark;
        old.age = 0.8;
        let q = quad(&old, camera);
        let dying = Vec4::from(q.colors[0]);
        assert!(dying.length() < head_color.length() * 0.5);
        assert!(dying.x / dying.z > head_color.x / head_color.z);
    }

    /// A flame rises from its base along its flight, its tip its deep color; a glow and a puff
    /// face the camera; a ring lies across its axis; the smoke thins out; nothing for a solid
    /// shape or a particle that is over; never more quads than the cap.
    #[test]
    fn each_shape_has_its_quad() {
        let camera = Vec3::new(0.0, 2.0, 10.0);
        let mut flame = Particle::new(Shape::Tongue, Paint::Purple, Vec3::ZERO, 0.2, 0.3);
        flame.velocity = Vec3::Y * 2.0;
        flame.stretch = 2.0;
        flame.curve = Curve::Flash;
        flame.age = 0.1;
        let q = quad(&flame, camera);
        let c = corners(&q);
        assert!(
            (c[0].y).abs() < 1e-5 && (c[2].y - 0.6).abs() < 1e-4,
            "{c:?}"
        );
        let (base, tip) = (Vec4::from(q.colors[0]), Vec4::from(q.colors[2]));
        assert!(tip.x / tip.z > base.x / base.z, "pink tip: {base} {tip}");
        let mut glow = Particle::new(Shape::Glow, Paint::Purple, Vec3::ZERO, 1.0, 0.5);
        glow.facing = Facing::Camera;
        let c = corners(&quad(&glow, camera));
        let normal = (c[1] - c[0]).cross(c[3] - c[0]).normalize();
        assert!(normal.dot(camera.normalize()).abs() > 0.99, "{normal}");
        let mut ring = Particle::new(Shape::Shock, Paint::Purple, Vec3::ZERO, 1.0, 1.0);
        ring.axis = Vec3::Y;
        let c = corners(&quad(&ring, camera));
        assert!(c.iter().all(|p| p.y.abs() < 1e-5), "flat: {c:?}");
        let mut puff = Particle::new(Shape::Haze, Paint::Smoke, Vec3::ZERO, 1.0, 0.4);
        puff.glow = 0.5;
        puff.age = 0.2;
        let early = Vec4::from(quad(&puff, camera).colors[0]).w;
        puff.age = 0.8;
        let late = Vec4::from(quad(&puff, camera).colors[0]).w;
        assert!(early > 0.2 && late < early * 0.3, "{early} {late}");
        let solid = Particle::new(Shape::Cube, Paint::Purple, Vec3::ZERO, 1.0, 0.5);
        assert!(quad(&solid, camera).is_empty());
        let mut over = glow;
        over.age = 2.0;
        assert!(quad(&over, camera).is_empty());
        let mut quads = Quads::new(2);
        for _ in 0..5 {
            particle_quad(&glow, camera, Vec3::ZERO, &colors(), &mut quads);
        }
        assert_eq!(quads.len(), 2);
        assert_eq!(quads.positions.capacity(), 8, "never grown");
        let halo = GroundHalo {
            center: Vec3::new(3.0, 0.0, 0.0),
            up: Vec3::Y,
            size: 2.0,
            color: Vec3::ONE,
        };
        let mut quads = Quads::new(1);
        halo_quad(&halo, Vec3::new(1.0, 0.0, 0.0), &mut quads);
        let c = corners(&quads);
        assert!(
            c.iter()
                .all(|p| p.y.abs() < 1e-6 && (p.x - 2.0).abs() <= std::f32::consts::SQRT_2 + 1e-5)
        );
    }

    /// The indices: two triangles per quad, as many as asked (at most the capacity), never grown.
    #[test]
    fn the_indices_follow_the_quads() {
        let mut indices = Vec::with_capacity(6 * 3);
        quad_indices(&mut indices, 2);
        assert_eq!(indices, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        quad_indices(&mut indices, 1);
        assert_eq!(indices.len(), 6);
        quad_indices(&mut indices, 10);
        assert_eq!(indices.len(), 18);
        assert_eq!(indices.capacity(), 18);
    }
}
