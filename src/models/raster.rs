//! Icons rendered from a model on the CPU, once (issue #106: the items' icons in the HUD and its
//! roulette are their models, seen in 3/4). A tiny rasterizer: the model's triangles turned to
//! the view, drawn with a depth buffer at a few samples per pixel (smooth edges), lit like the
//! game (a sun from the top left, a soft fill, a highlight and a rim), then a thick outline around
//! its silhouette (read at a glance, on the sky as on the road). Pure, no ECS and no GPU: the same
//! icon in the browser, natively and in the headless tests.

use bevy::prelude::*;

use crate::track::mesh::MeshData;

/// How a model is turned for its icon: `yaw` around its up axis first, then `pitch` (its top
/// tilted toward the viewer), then `roll` (in the picture's plane). Orthographic, seen from +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconView {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

impl IconView {
    /// The usual 3/4 view: a little from the right, a little from above.
    pub const THREE_QUARTER: Self = Self {
        yaw: -0.55,
        pitch: 0.4,
        roll: 0.0,
    };

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_z(self.roll)
            * Quat::from_rotation_x(self.pitch)
            * Quat::from_rotation_y(self.yaw)
    }
}

/// How an icon is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconStyle {
    /// Its side (px).
    pub size: u32,
    /// Samples per pixel along each side (its edges are smooth).
    pub supersample: u32,
    /// The empty border around the model and its outline (a share of the side).
    pub margin: f32,
    /// The outline's thickness (px).
    pub outline: f32,
    /// The outline's color (sRGB).
    pub outline_color: [f32; 3],
    /// Where the light comes from, in the picture (x right, y up, z toward the viewer).
    pub light: Vec3,
    /// The light the shade keeps (0: black shadows, 1: no shading).
    pub ambient: f32,
}

impl Default for IconStyle {
    fn default() -> Self {
        Self {
            size: 128,
            supersample: 2,
            margin: 0.035,
            outline: 5.0,
            // The charter's Myrtille, a little darker: the HUD's ink.
            outline_color: [0.16, 0.12, 0.31],
            light: Vec3::new(-0.45, 0.75, 0.55),
            ambient: 0.42,
        }
    }
}

/// An icon: `size` × `size` pixels, sRGB with their opacity (straight, not premultiplied), from
/// the top row down; `body` says which pixels the model itself covers (more than half), the
/// outline apart.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub size: u32,
    pub pixels: Vec<[u8; 4]>,
    pub body: Vec<bool>,
}

impl Icon {
    /// The share of its pixels the model covers (its outline apart).
    pub fn coverage(&self) -> f32 {
        self.body.iter().filter(|b| **b).count() as f32 / self.body.len().max(1) as f32
    }

    /// The mean color of the model's own pixels (sRGB, 0 to 1): the color it is seen in.
    pub fn mean_color(&self) -> Vec3 {
        let mut sum = Vec3::ZERO;
        let mut count = 0.0;
        for (pixel, body) in self.pixels.iter().zip(&self.body) {
            if *body {
                sum += Vec3::new(pixel[0] as f32, pixel[1] as f32, pixel[2] as f32) / 255.0;
                count += 1.0;
            }
        }
        if count > 0.0 { sum / count } else { Vec3::ZERO }
    }

    /// The share of pixels covered by both icons' bodies among those covered by either (1: the
    /// same silhouette).
    pub fn overlap(&self, other: &Icon) -> f32 {
        let (mut both, mut either) = (0, 0);
        for (a, b) in self.body.iter().zip(&other.body) {
            both += usize::from(*a && *b);
            either += usize::from(*a || *b);
        }
        both as f32 / either.max(1) as f32
    }

    /// Its pixels as bytes, row after row (RGBA, sRGB): an image's data.
    pub fn bytes(&self) -> Vec<u8> {
        self.pixels.iter().flatten().copied().collect()
    }
}

/// A sample of the picture being drawn: its depth (toward the viewer: bigger is nearer) and its
/// lit color (linear), or nothing.
#[derive(Clone, Copy)]
struct Sample {
    depth: f32,
    color: Vec3,
}

/// Renders `mesh` (its vertex colors linear, its normals smooth or flat) as an icon: turned by
/// `view`, fit in the picture (its margin and outline around it), lit and outlined.
pub fn render_icon(mesh: &MeshData, view: IconView, style: &IconStyle) -> Icon {
    let size = style.size.max(8);
    let ss = style.supersample.max(1);
    let side = (size * ss) as usize;
    let turn = view.rotation();
    let points: Vec<Vec3> = mesh
        .positions
        .iter()
        .map(|p| turn * Vec3::from(*p))
        .collect();
    let normals: Vec<Vec3> = mesh.normals.iter().map(|n| turn * Vec3::from(*n)).collect();
    let mut pixels = vec![[0u8; 4]; (size * size) as usize];
    let mut body = vec![false; (size * size) as usize];
    let Some((lo, hi)) = points
        .iter()
        .fold(None, |b: Option<(Vec3, Vec3)>, p| match b {
            None => Some((*p, *p)),
            Some((lo, hi)) => Some((lo.min(*p), hi.max(*p))),
        })
    else {
        return Icon { size, pixels, body };
    };
    let extent = (hi - lo).xy().max_element().max(1e-6);
    let border = style.margin * size as f32 + style.outline;
    let scale = (size as f32 - 2.0 * border).max(1.0) * ss as f32 / extent;
    let middle = (lo + hi).xy() / 2.0;
    let half = side as f32 / 2.0;
    let screen = |p: Vec3| {
        Vec3::new(
            half + (p.x - middle.x) * scale,
            half - (p.y - middle.y) * scale,
            p.z,
        )
    };
    let light = style.light.normalize_or(Vec3::Y);
    let halfway = (light + Vec3::Z).normalize();
    let shade = |normal: Vec3, albedo: Vec3| -> Vec3 {
        // A normal facing away (a single-sided part seen from behind) is seen from its front.
        let n = if normal.z < 0.0 { -normal } else { normal }.normalize_or(Vec3::Z);
        let diffuse = ((n.dot(light) + 0.35) / 1.35).clamp(0.0, 1.0);
        let lit = albedo * (style.ambient + (1.0 - style.ambient) * diffuse);
        let highlight = n.dot(halfway).max(0.0).powf(28.0) * 0.28;
        let rim = (1.0 - n.z.max(0.0)).powi(3) * 0.16;
        lit + Vec3::splat(highlight) + albedo.lerp(Vec3::ONE, 0.5) * rim
    };
    let mut samples: Vec<Option<Sample>> = vec![None; side * side];
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| i as usize);
        let (pa, pb, pc) = (screen(points[a]), screen(points[b]), screen(points[c]));
        let area = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = pa.x.min(pb.x).min(pc.x).floor().max(0.0) as usize;
        let x1 = (pa.x.max(pb.x).max(pc.x).ceil() as usize).min(side);
        let y0 = pa.y.min(pb.y).min(pc.y).floor().max(0.0) as usize;
        let y1 = (pa.y.max(pb.y).max(pc.y).ceil() as usize).min(side);
        let color = |i: usize| {
            let c = mesh.colors.get(i).copied().unwrap_or([1.0; 4]);
            Vec3::new(c[0], c[1], c[2])
        };
        let (ca, cb, cc) = (color(a), color(b), color(c));
        let normal = |i: usize| normals.get(i).copied().unwrap_or(Vec3::Z);
        let (na, nb, nc) = (normal(a), normal(b), normal(c));
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let wa = ((pb.x - px) * (pc.y - py) - (pb.y - py) * (pc.x - px)) / area;
                let wb = ((pc.x - px) * (pa.y - py) - (pc.y - py) * (pa.x - px)) / area;
                let wc = 1.0 - wa - wb;
                if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                    continue;
                }
                let depth = wa * pa.z + wb * pb.z + wc * pc.z;
                let slot = &mut samples[y * side + x];
                if slot.is_some_and(|s| s.depth >= depth) {
                    continue;
                }
                let n = na * wa + nb * wb + nc * wc;
                let albedo = ca * wa + cb * wb + cc * wc;
                *slot = Some(Sample {
                    depth,
                    color: shade(n, albedo),
                });
            }
        }
    }
    // The outline: every sample within its thickness of the model (a chamfer distance, close to
    // the true distance).
    let reach = style.outline * ss as f32;
    let far = f32::MAX / 4.0;
    let mut distance: Vec<f32> = samples
        .iter()
        .map(|s| if s.is_some() { 0.0 } else { far })
        .collect();
    let diagonal = std::f32::consts::SQRT_2;
    for y in 0..side {
        for x in 0..side {
            let mut d = distance[y * side + x];
            if x > 0 {
                d = d.min(distance[y * side + x - 1] + 1.0);
            }
            if y > 0 {
                d = d.min(distance[(y - 1) * side + x] + 1.0);
                if x > 0 {
                    d = d.min(distance[(y - 1) * side + x - 1] + diagonal);
                }
                if x + 1 < side {
                    d = d.min(distance[(y - 1) * side + x + 1] + diagonal);
                }
            }
            distance[y * side + x] = d;
        }
    }
    for y in (0..side).rev() {
        for x in (0..side).rev() {
            let mut d = distance[y * side + x];
            if x + 1 < side {
                d = d.min(distance[y * side + x + 1] + 1.0);
            }
            if y + 1 < side {
                d = d.min(distance[(y + 1) * side + x] + 1.0);
                if x + 1 < side {
                    d = d.min(distance[(y + 1) * side + x + 1] + diagonal);
                }
                if x > 0 {
                    d = d.min(distance[(y + 1) * side + x - 1] + diagonal);
                }
            }
            distance[y * side + x] = d;
        }
    }
    let ink = Color::srgb(
        style.outline_color[0],
        style.outline_color[1],
        style.outline_color[2],
    )
    .to_linear();
    let ink = Vec3::new(ink.red, ink.green, ink.blue);
    let per_pixel = (ss * ss) as f32;
    for py in 0..size as usize {
        for px in 0..size as usize {
            let (mut sum, mut covered, mut outlined) = (Vec3::ZERO, 0.0, 0.0);
            for sy in 0..ss as usize {
                for sx in 0..ss as usize {
                    let i = (py * ss as usize + sy) * side + px * ss as usize + sx;
                    match samples[i] {
                        Some(sample) => {
                            sum += sample.color;
                            covered += 1.0;
                        }
                        None if distance[i] <= reach => outlined += 1.0,
                        None => {}
                    }
                }
            }
            let (model, line) = (covered / per_pixel, outlined / per_pixel);
            let alpha = model + line;
            if alpha <= 0.0 {
                continue;
            }
            let model_color = if covered > 0.0 {
                sum / covered
            } else {
                Vec3::ZERO
            };
            let linear = (model_color * model + ink * line) / alpha;
            let color = Color::linear_rgb(linear.x, linear.y, linear.z).to_srgba();
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let at = py * size as usize + px;
            pixels[at] = [
                byte(color.red),
                byte(color.green),
                byte(color.blue),
                byte(alpha),
            ];
            body[at] = model > 0.5;
        }
    }
    Icon { size, pixels, body }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::shape::{Detail, ellipsoid, solid, srgb};

    fn ball(color: [f32; 4]) -> MeshData {
        let mut mesh = MeshData::default();
        ellipsoid(
            &mut mesh,
            Mat4::IDENTITY,
            Vec3::ONE,
            Detail::new(24, 16),
            solid(color),
        );
        mesh
    }

    fn pixel(icon: &Icon, x: u32, y: u32) -> [u8; 4] {
        icon.pixels[(y * icon.size + x) as usize]
    }

    /// A ball is a disc in the middle of its icon, its outline around it, nothing in the
    /// corners; lit from the top left, darker at the bottom right.
    #[test]
    fn a_ball_is_a_lit_disc_with_its_outline() {
        let style = IconStyle::default();
        let icon = render_icon(&ball(srgb(0.2, 0.6, 1.0)), IconView::THREE_QUARTER, &style);
        assert_eq!(icon.pixels.len(), 128 * 128);
        // The disc fills the picture but its margin and outline: about π/4 of what is left.
        let inner = 128.0 - 2.0 * (style.margin * 128.0 + style.outline);
        let expected = std::f32::consts::FRAC_PI_4 * inner * inner / (128.0 * 128.0);
        assert!(
            (icon.coverage() - expected).abs() < 0.03,
            "{} vs {expected}",
            icon.coverage()
        );
        assert_eq!(pixel(&icon, 1, 1)[3], 0, "the corners stay empty");
        assert_eq!(pixel(&icon, 64, 64)[3], 255);
        // The outline: dark and opaque just outside the disc.
        let edge = (64.0 + inner / 2.0 + style.outline / 2.0) as u32;
        let ring = pixel(&icon, edge, 64);
        assert!(ring[3] > 200 && ring[2] < 100, "{ring:?}");
        let (top_left, bottom_right) = (pixel(&icon, 45, 45), pixel(&icon, 83, 83));
        let light = |p: [u8; 4]| p[0] as u32 + p[1] as u32 + p[2] as u32;
        assert!(
            light(top_left) > light(bottom_right) + 60,
            "{top_left:?} {bottom_right:?}"
        );
        // Seen in its own color.
        let mean = icon.mean_color();
        assert!(mean.z > mean.y && mean.y > mean.x, "{mean}");
    }

    /// What is nearer hides what is behind it.
    #[test]
    fn the_nearest_part_is_seen() {
        let mut mesh = ball(srgb(1.0, 0.0, 0.0));
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(0.0, 0.0, 1.2)),
            Vec3::splat(0.3),
            Detail::new(12, 8),
            solid(srgb(0.0, 1.0, 0.0)),
        );
        let view = IconView {
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        };
        let icon = render_icon(&mesh, view, &IconStyle::default());
        let center = pixel(&icon, 64, 64);
        assert!(center[1] > 150 && center[0] < 60, "{center:?}");
    }

    /// The same silhouette overlaps fully, another one less.
    #[test]
    fn silhouettes_compare() {
        let style = IconStyle::default();
        let view = IconView::THREE_QUARTER;
        let a = render_icon(&ball([1.0; 4]), view, &style);
        let mut long = MeshData::default();
        ellipsoid(
            &mut long,
            Mat4::IDENTITY,
            Vec3::new(1.0, 0.3, 0.3),
            Detail::new(16, 10),
            solid([1.0; 4]),
        );
        let b = render_icon(&long, view, &style);
        assert!((a.overlap(&a) - 1.0).abs() < 1e-6);
        assert!(a.overlap(&b) < 0.6, "{}", a.overlap(&b));
        assert_eq!(a.bytes().len(), 128 * 128 * 4);
    }
}
