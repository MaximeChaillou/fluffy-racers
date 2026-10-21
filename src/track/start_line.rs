//! The start/finish line of a circuit (issue #79): a checkered band across the road and an arch
//! over it with a checkered banner, exactly where the laps are counted (distance 0 of the
//! centerline: checkpoint 0 of `crate::race::course`), framed in the colors of its biome's walls.
//!
//! Pure (no ECS): a [`MeshData`] drawn with the circuit's mesh (`circuit.rs`: no extra draw call)
//! and never solid: no collider, the physics does not change. The posts stand on the ground beyond
//! the walls; the decor keeps clear of them ([`StartLine::footprints`], `decor.rs`).

use std::f32::consts::FRAC_1_SQRT_2;

use bevy::prelude::*;

use super::biome::{Palette, Rgb};
use super::layout::{CenterLine, TrackSample};
use super::mesh::{CURB_WIDTH, MeshData, WALL_THICKNESS, wall_offset};
use super::water::{Lake, ground_height};

/// Depth of the checkered band on the road, along the track (m), centered on the line.
pub const LINE_DEPTH: f32 = 1.6;
/// Rows of squares of the band, and of the banner's checks.
const ROWS: usize = 2;
/// Wanted width of a square (m): a whole number of them across the road and across the banner.
const SQUARE: f32 = 0.8;
/// Height of the band above the road (m): drawn over it without flickering (the sand patches sit
/// 3 cm above it).
pub const LINE_HEIGHT: f32 = 0.02;
/// The arch's two posts: square, this wide (m)...
pub const POST_WIDTH: f32 = 1.0;
/// ...this far beyond the walls' outer face (m)...
pub const POST_GAP: f32 = 0.3;
/// ...and rising this much above the banner (m), in stripes this tall of the walls' two colors.
const POST_TOP: f32 = 0.5;
const POST_STRIPE: f32 = 1.0;
/// The banner between the posts: its bottom this high above the road (m), far above the chase
/// camera (`CameraTuning::height`: 2.4 m above the kart's center)...
pub const BANNER_BOTTOM: f32 = 6.0;
/// ...this tall (m: a border in the biome's color, two rows of checks, a border)...
pub const BANNER_HEIGHT: f32 = 2.4;
const BANNER_BORDER: f32 = 0.4;
/// ...and this thick along the track (m), centered on the line.
pub const BANNER_THICKNESS: f32 = 0.6;
/// The checks' colors (sRGB).
const DARK: Rgb = (0.07, 0.07, 0.09);
const LIGHT: Rgb = (0.97, 0.97, 0.97);

/// A palette color as the mesh takes it (sRGB, opaque).
fn rgba((r, g, b): Rgb) -> [f32; 4] {
    [r, g, b, 1.0]
}

/// Adds the rectangle `origin + [0, 1]·u + [0, 1]·v`, its front facing `u × v`.
fn rect(mesh: &mut MeshData, origin: Vec3, u: Vec3, v: Vec3, color: [f32; 4]) {
    mesh.quad([origin, origin + u, origin + u + v, origin + v], color);
}

/// The start/finish line of a circuit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartLine {
    /// The centerline on the line: where it is, the race direction (`forward`), across the road.
    pub sample: TrackSample,
}

impl StartLine {
    /// The line of the circuit of `center`: at its distance 0, the start line, where a lap is
    /// counted (`TrackLayout::checkpoints[0]`).
    pub fn of(center: &CenterLine) -> Self {
        Self {
            sample: center.at(0.0),
        }
    }

    /// Issue #119: every line of the circuit of `center`: its start line, and on an open circuit
    /// its finish line too (elsewhere: an arch of its own).
    pub fn all(center: &CenterLine) -> Vec<Self> {
        std::iter::once(Self::of(center))
            .chain(center.open().map(|open| Self {
                sample: center.at(open.finish),
            }))
            .collect()
    }

    /// Half the band's width: the road and its curbs.
    pub fn half_width(&self) -> f32 {
        self.sample.half_width + CURB_WIDTH
    }

    /// From the centerline to the middle of each post (m): beyond the walls.
    pub fn post_offset(&self) -> f32 {
        wall_offset(&self.sample) + WALL_THICKNESS + POST_GAP + POST_WIDTH / 2.0
    }

    /// From the centerline to the inner face of each post, where the banner ends (m).
    pub fn banner_half_width(&self) -> f32 {
        self.post_offset() - POST_WIDTH / 2.0
    }

    /// The arch's frame: across the road (to the right), up (vertical: the posts stand upright)
    /// and back (against the race direction), right-handed.
    fn frame(&self) -> (Vec3, Vec3, Vec3) {
        let right = self.sample.forward.cross(Vec3::Y).normalize_or(Vec3::X);
        (right, Vec3::Y, right.cross(Vec3::Y))
    }

    /// The middle of each post at the road's height, left then right.
    pub fn posts(&self) -> [Vec3; 2] {
        let (right, ..) = self.frame();
        let offset = self.post_offset();
        [-1.0, 1.0].map(|side| self.sample.position + right * side * offset)
    }

    /// The posts' footprints on the ground plane (middle, radius): the decor keeps clear of them.
    pub fn footprints(&self) -> [(Vec2, f32); 2] {
        self.posts().map(|p| (p.xz(), POST_WIDTH * FRAC_1_SQRT_2))
    }

    /// Height of the posts' top above the road (m).
    pub fn top(&self) -> f32 {
        BANNER_BOTTOM + BANNER_HEIGHT + POST_TOP
    }

    /// The band and the arch, in the colors of `palette`; the posts go down to the ground (or to
    /// the floor of a lake).
    pub fn mesh(&self, palette: &Palette, lakes: &[Lake]) -> MeshData {
        let mut mesh = self.band();
        mesh.append(&self.arch(palette, lakes));
        mesh
    }

    /// The checkered band on the road, from curb to curb: [`ROWS`] rows of squares, its middle
    /// exactly on the line, [`LINE_HEIGHT`] above the road.
    pub fn band(&self) -> MeshData {
        let s = &self.sample;
        let half = self.half_width();
        let columns = ((2.0 * half / SQUARE).round() as usize).max(2);
        let width = 2.0 * half / columns as f32;
        let depth = LINE_DEPTH / ROWS as f32;
        let at = |lateral: f32, along: f32| s.point(lateral, LINE_HEIGHT) + s.forward * along;
        let mut mesh = MeshData::default();
        for row in 0..ROWS {
            let along = -LINE_DEPTH / 2.0 + row as f32 * depth;
            for column in 0..columns {
                let lateral = -half + column as f32 * width;
                let color = if (row + column) % 2 == 0 { DARK } else { LIGHT };
                // Seen from above: right × forward is up.
                rect(
                    &mut mesh,
                    at(lateral, along),
                    s.right * width,
                    s.forward * depth,
                    rgba(color),
                );
            }
        }
        mesh
    }

    /// The arch: two upright posts beyond the walls, striped in the two colors of the walls'
    /// panels and capped with the color of their top, from the ground to [`Self::top`]; between
    /// them over the road, the banner: two rows of checks on both faces between two borders of
    /// the biome's color (the walls' second panel color, the one that is not white).
    pub fn arch(&self, palette: &Palette, lakes: &[Lake]) -> MeshData {
        let (right, up, back) = self.frame();
        let base = self.sample.position;
        let (panel, accent, cap) = (
            rgba(palette.walls.0),
            rgba(palette.walls.1),
            rgba(palette.wall_top),
        );
        let mut mesh = MeshData::default();

        // The posts: their four sides stripe by stripe, from the road up and down from it.
        let half = POST_WIDTH / 2.0;
        let (r, b) = (right * half, back * half);
        for foot in self.posts() {
            let bottom = ground_height(lakes, foot.xz()).min(base.y) - base.y;
            let first = (bottom / POST_STRIPE).floor() as i32;
            let last = (self.top() / POST_STRIPE).ceil() as i32;
            for k in first..last {
                let lo = (k as f32 * POST_STRIPE).max(bottom);
                let hi = ((k + 1) as f32 * POST_STRIPE).min(self.top());
                if hi <= lo {
                    continue;
                }
                let color = if k.rem_euclid(2) == 0 { accent } else { panel };
                let h = up * (hi - lo);
                let corner = foot + up * lo;
                // Right, left, back and front sides, each facing out.
                rect(&mut mesh, corner + r - b, h, b * 2.0, color);
                rect(&mut mesh, corner - r - b, b * 2.0, h, color);
                rect(&mut mesh, corner - r + b, r * 2.0, h, color);
                rect(&mut mesh, corner - r - b, h, r * 2.0, color);
            }
            rect(
                &mut mesh,
                foot + up * self.top() - r - b,
                b * 2.0,
                r * 2.0,
                cap,
            );
        }

        // The banner. Its faces: the one the karts see coming (back), the other one (front).
        let inner = self.banner_half_width();
        let columns = ((2.0 * inner / SQUARE).round() as usize).max(2);
        let width = 2.0 * inner / columns as f32;
        let check = (BANNER_HEIGHT - 2.0 * BANNER_BORDER) / ROWS as f32;
        let thick = back * (BANNER_THICKNESS / 2.0);
        // Each cell of a face: from left to right (m from the centerline), from low to high (m
        // above the banner's bottom), its color.
        let mut cells = vec![
            (-inner, inner, 0.0, BANNER_BORDER, accent),
            (
                -inner,
                inner,
                BANNER_HEIGHT - BANNER_BORDER,
                BANNER_HEIGHT,
                accent,
            ),
        ];
        for row in 0..ROWS {
            let lo = BANNER_BORDER + row as f32 * check;
            for column in 0..columns {
                let left = -inner + column as f32 * width;
                let color = if (row + column) % 2 == 0 { DARK } else { LIGHT };
                cells.push((left, left + width, lo, lo + check, rgba(color)));
            }
        }
        for (left, right_edge, lo, hi, color) in cells {
            let corner = base + right * left + up * (BANNER_BOTTOM + lo);
            let (w, h) = (right * (right_edge - left), up * (hi - lo));
            rect(&mut mesh, corner + thick, w, h, color);
            rect(&mut mesh, corner - thick, h, w, color);
        }
        // Its top and its underside (seen from under it, driving through).
        let corner = base - right * inner - thick;
        let across = right * (2.0 * inner);
        let depth = back * BANNER_THICKNESS;
        rect(
            &mut mesh,
            corner + up * (BANNER_BOTTOM + BANNER_HEIGHT),
            depth,
            across,
            accent,
        );
        rect(
            &mut mesh,
            corner + up * BANNER_BOTTOM,
            across,
            depth,
            accent,
        );
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::{Course, RaceProgress, RaceTuning};
    use crate::track::TrackLayout;
    use crate::track::data::TrackData;
    use crate::track::mesh::GROUND_HEIGHT;

    /// Every circuit of the game (`assets/tracks/index.ron`) and its mirror image (issue #19).
    fn every_circuit_and_its_mirror() -> Vec<TrackData> {
        crate::admin::tracks::circuits()
            .iter()
            .flat_map(|entry| {
                let text = std::fs::read_to_string(format!(
                    "{}/assets/{}",
                    env!("CARGO_MANIFEST_DIR"),
                    entry.asset_path()
                ))
                .unwrap();
                let data = TrackData::from_ron(&text).unwrap();
                [data.clone(), data.mirrored()]
            })
            .collect()
    }

    fn name(data: &TrackData) -> String {
        format!(
            "{}{}",
            data.name,
            if data.mirrored { " (miroir)" } else { "" }
        )
    }

    fn vertices(mesh: &MeshData) -> impl Iterator<Item = Vec3> + '_ {
        mesh.positions.iter().map(|p| Vec3::from_array(*p))
    }

    /// Each quad of `mesh` (4 vertices in a row, `MeshData::quad`): its corners and its normal.
    fn quads(mesh: &MeshData) -> impl Iterator<Item = ([Vec3; 4], Vec3)> + '_ {
        mesh.positions
            .chunks(4)
            .zip(mesh.normals.chunks(4))
            .map(|(p, n)| {
                (
                    [0, 1, 2, 3].map(|i| Vec3::from_array(p[i])),
                    Vec3::from_array(n[0]),
                )
            })
    }

    /// Issue #79 acceptance, on every circuit of the game and its mirror: the checkered band is
    /// exactly on the line where the race rules count a lap. Each of its vertices, projected by
    /// the race (`Course::project`, the lap's own measure), is at one of the three distances of
    /// its rows' edges: its middle edge on the line, to the centimeter, across the whole road (it
    /// is the line, perpendicular to the track); its two other edges 0.8 m before and after it, to
    /// 3 cm (the race measures them from the centerline's samples 1 m away, whose frame turns a
    /// little on a start straight that bends gently: 2.1 cm at the curbs of the Crique Câline). It
    /// covers the road from curb to curb, just above it, facing up. A point of the band 1 cm past
    /// its middle completes a lap, a point 1 cm short of it does not. Issue #119: an open
    /// circuit's finish line too (it ends the race).
    #[test]
    fn the_band_is_on_the_line_where_the_laps_are_counted() {
        let tuning = RaceTuning::default();
        for data in every_circuit_and_its_mirror() {
            let name = name(&data);
            let layout = TrackLayout::new(&data);
            let course = Course::new(&layout, &tuning);
            assert_eq!(course.checkpoints[0], 0.0, "{name}: the lap line");
            let lines = StartLine::all(&layout.center);
            assert_eq!(lines[0].sample, layout.checkpoints[0].sample, "{name}");
            // Issue #119: an open circuit has its finish line too, where the race ends: checked
            // the same way (its band about the finish's race distance, from the window of a racer
            // about to finish).
            assert_eq!(lines.len(), if course.is_open() { 2 } else { 1 }, "{name}");
            let n = course.checkpoint_count();
            for (k, line) in lines.iter().enumerate() {
                let (at, passed) = if k == 0 {
                    (0.0, 1)
                } else {
                    (course.finish_distance(), n)
                };
                let band = line.band();
                assert!(band.triangle_count() >= 2 * 2 * 16, "{name}");
                let (mut left, mut right, mut on_the_line) = (f32::MAX, f32::MIN, 0);
                for v in vertices(&band) {
                    let p = course.project(passed, v);
                    let along = p.distance - at;
                    let (edge, tolerance) = if along.abs() < LINE_DEPTH / 4.0 {
                        on_the_line += 1;
                        (0.0, 0.01)
                    } else {
                        (LINE_DEPTH / 2.0 * along.signum(), 0.03)
                    };
                    assert!(
                        (along - edge).abs() < tolerance,
                        "{name}: a corner of the band {along:.3} m from the line"
                    );
                    assert!(
                        (p.height - LINE_HEIGHT).abs() < 0.005,
                        "{name}: {}",
                        p.height
                    );
                    left = left.min(p.lateral);
                    right = right.max(p.lateral);
                }
                assert_eq!(
                    on_the_line,
                    band.positions.len() / 2,
                    "{name}: half on the line"
                );
                let curbs = line.sample.half_width + CURB_WIDTH;
                assert!(
                    (left + curbs).abs() < 0.01 && (right - curbs).abs() < 0.01,
                    "{name}: the band from {left} to {right}, the curbs at ±{curbs}"
                );
                for (_, normal) in quads(&band) {
                    assert!(normal.dot(line.sample.up) > 0.999, "{name}: facing up");
                }
                if k == 0 && course.is_open() {
                    // The start line of an open circuit is crossed once, at the start (the race's
                    // tests: `race::progress`).
                    continue;
                }

                // The race: about to finish lap 1 (next checkpoint: the line), on the middle of
                // the band, then just past it. Issue #119: about to finish the race, on an open
                // circuit's finish line.
                let middle =
                    |along: f32| line.sample.point(0.3, LINE_HEIGHT) + line.sample.forward * along;
                let mut progress = RaceProgress::on_grid(&course, course.grid[0].translation, 3);
                for _ in 0..n {
                    progress.validate(&course, 100);
                }
                let short = course.project(progress.passed, middle(-0.01));
                assert_eq!(
                    progress.clone().update(&course, &short, Some(200), true),
                    None
                );
                let past = course.project(progress.passed, middle(0.01));
                let wanted = if course.is_open() {
                    crate::race::ProgressEvent::Finished
                } else {
                    crate::race::ProgressEvent::Lap(1)
                };
                assert_eq!(
                    progress.update(&course, &past, Some(200), true),
                    Some(wanted),
                    "{name}"
                );
            }
        }
    }

    /// Issue #79, on every circuit and its mirror: the arch stands on the line, across the track.
    /// The banner's two faces are each half its thickness from the line (to the centimeter: the
    /// arch is centered on it and perpendicular to the track), and so are the posts' sides (to
    /// 5 cm, measured from the next samples, 13 m out), the posts' middles on the line. Over
    /// the road, the curbs, the shoulders and the walls, nothing is lower than the banner's
    /// bottom, 6 m up; the posts stand beyond the walls' outer face, on the ground (or the floor
    /// of a lake), one on each side, as far from the centerline. Every face faces out (the
    /// circuit's material draws one side only).
    #[test]
    fn the_arch_stands_over_the_line_beyond_the_walls() {
        let tuning = RaceTuning::default();
        for data in every_circuit_and_its_mirror() {
            let name = name(&data);
            let layout = TrackLayout::new(&data);
            let course = Course::new(&layout, &tuning);
            let line = StartLine::of(&layout.center);
            let arch = line.arch(
                &data.biome().map(|b| b.palette).unwrap_or_default(),
                &layout.lakes,
            );
            let outer = wall_offset(&line.sample) + WALL_THICKNESS;
            let inner = line.banner_half_width();
            assert!(
                inner > outer + POST_GAP - 1e-4,
                "{name}: the posts beyond the walls"
            );
            let road = line.sample.position.y;
            let (mut lowest, mut sides) = (f32::MAX, [0usize; 2]);
            for v in vertices(&arch) {
                let p = course.project(1, v);
                let from_line = p.distance.abs();
                let banner = (from_line - BANNER_THICKNESS / 2.0).abs() < 0.01;
                // The posts' sides 0.5 m away are measured from the next samples' frame (see
                // above): 3.4 cm off at the posts of the Crique Câline, 13 m out.
                let post = (from_line - POST_WIDTH / 2.0).abs() < 0.05;
                assert!(
                    banner || post,
                    "{name}: a corner of the arch {:.3} m from the line",
                    p.distance
                );
                // Over the road, the curbs, the shoulders and the walls: only the banner, high.
                if p.lateral.abs() < inner - 0.01 {
                    assert!(banner, "{name}: a post over the track at {}", p.lateral);
                    assert!(p.height > BANNER_BOTTOM - 0.01, "{name}: {} m up", p.height);
                }
                if post {
                    sides[usize::from(p.lateral > 0.0)] += 1;
                    lowest = lowest.min(v.y);
                }
            }
            assert!(sides[0] > 0 && sides[0] == sides[1], "{name}: {sides:?}");
            let ground = line
                .footprints()
                .map(|(p, _)| ground_height(&layout.lakes, p))
                .into_iter()
                .fold(f32::MAX, f32::min);
            assert!(ground <= GROUND_HEIGHT);
            assert!(
                (lowest - ground).abs() < 1e-3,
                "{name}: the posts end at {lowest}"
            );
            let [left, right] = line.posts().map(|p| course.project(1, p));
            assert!(left.lateral < -(outer + POST_GAP) && right.lateral > outer + POST_GAP);
            assert!(
                (left.lateral + right.lateral).abs() < 0.01,
                "{name}: centered"
            );
            assert!(
                left.distance.abs() < 0.01 && right.distance.abs() < 0.01,
                "{name}"
            );

            // Every face out of the part it belongs to: the banner, or the post on its side.
            let banner = line.sample.position + Vec3::Y * (BANNER_BOTTOM + BANNER_HEIGHT / 2.0);
            for (corners, normal) in quads(&arch) {
                let middle = corners.iter().sum::<Vec3>() / 4.0;
                let p = course.project(1, middle);
                let part = if p.lateral.abs() < inner - 0.01 {
                    banner
                } else {
                    let post = line.posts()[usize::from(p.lateral > 0.0)];
                    post.with_y(middle.y.min(road + line.top() - 0.1))
                };
                assert!(
                    normal.dot(middle - part) > 0.0,
                    "{name}: a face at {middle} faces in ({normal})"
                );
            }
        }
    }

    /// The arch in its biome's colors: the posts striped in the two colors of the walls' panels,
    /// the banner's border in the second one (the colored one), the checks black and white.
    #[test]
    fn the_arch_wears_the_colors_of_its_biome() {
        for data in every_circuit_and_its_mirror() {
            let name = name(&data);
            let palette = data.biome().expect("a biome").palette;
            let layout = TrackLayout::new(&data);
            let arch = StartLine::of(&layout.center).arch(&palette, &layout.lakes);
            let linear = |c: Rgb| Color::srgb(c.0, c.1, c.2).to_linear().to_f32_array();
            for color in [
                palette.walls.0,
                palette.walls.1,
                palette.wall_top,
                DARK,
                LIGHT,
            ] {
                let wanted = linear(color);
                assert!(
                    arch.colors
                        .iter()
                        .any(|c| c.iter().zip(wanted).all(|(a, b)| (a - b).abs() < 1e-5)),
                    "{name}: no {color:?} on the arch"
                );
            }
            assert!(arch.colors.iter().all(|c| {
                [
                    palette.walls.0,
                    palette.walls.1,
                    palette.wall_top,
                    DARK,
                    LIGHT,
                ]
                .iter()
                .any(|color| {
                    c.iter()
                        .zip(linear(*color))
                        .all(|(a, b)| (a - b).abs() < 1e-5)
                })
            }));
        }
    }

    /// A few hundred triangles: nothing next to the circuit's tens of thousands.
    #[test]
    fn the_line_costs_a_few_hundred_triangles() {
        for data in every_circuit_and_its_mirror() {
            let layout = TrackLayout::new(&data);
            let mesh = StartLine::of(&layout.center).mesh(&Palette::default(), &layout.lakes);
            let triangles = mesh.triangle_count();
            assert!(
                (100..1200).contains(&triangles),
                "{}: {triangles}",
                data.name
            );
        }
    }
}
