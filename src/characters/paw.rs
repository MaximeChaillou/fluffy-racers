//! The animals' paws (issue #169, GD § 14 and 15): the arm that comes out of a driver for a strike
//! or a grab (`interactions::visual`), and of a character on foot in the lobby (`salon::walker`),
//! is the animal's own. Pure here (meshes and points, tested on the real files in
//! `crate::models`); the visuals only place them.
//!
//! - **Its paw** ([`PawShape`], declared per character in `characters.ron`): a round paw with
//!   four toes and their pads (the cat, the fox), a long one with three toes (the bunny), a
//!   cloven hoof (the pig, the cow), a big round foot with its nails (the elephant), or the
//!   bird's own wing (the chick, the penguin, the parrot). Low-poly soft blocks
//!   (`models::shape`), like the Cube Pets' bevelled cubes.
//! - **Its colors, taken on the model** ([`PawColors`]): the fur of its front leg, the color of
//!   its foot (the lower ring of the leg: the fox's dark socks, the pig's and the cow's hooves,
//!   the elephant's pale feet) and of its sole (the pads, the nails); a bird's wing keeps its own
//!   colors, its arm the color of the wing's root.
//! - **Where it comes out** ([`PawModel::shoulder`]): measured on the model, in its body's node
//!   (which the clips and the wobble move: the arm follows them): a bird's wing root, else the
//!   side of the body a third of the way up, just inside its surface. One point per animal, on
//!   its left (+X in its file), mirrored for its right; scaled with the animal, so a small one's
//!   arm leaves its own body.
//!
//! The arm's mesh is 1 long along +Y (stretched by its transform: the elastic band); the hand's
//! is centered on its middle, from its wrist along +Y, its palm (the pads, the elephant's nails) toward -Z,
//! the back of the hand toward +Z.
//! Both in the animal's units (its file's), drawn with the models' shared material.

use bevy::prelude::*;
use serde::Deserialize;

use crate::models::shape::{Detail, bounds, mix, rounded_box, srgb};
use crate::track::mesh::MeshData;

use super::model::{AnimalModelData, AnimalNode, AnimalPart, BODY};

/// What an animal's paw looks like (`paw` in `characters.ron`).
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PawShape {
    /// A round paw, four toes and their pads: the cat, the fox.
    #[default]
    Toes,
    /// A long paw, three toes: the bunny.
    Long,
    /// A cloven hoof: the pig, the cow.
    Hoof,
    /// A big round foot, its nails at the front: the elephant.
    Stomp,
    /// The bird's own wing: the chick, the penguin, the parrot.
    Wing,
}

impl PawShape {
    pub const ALL: [Self; 5] = [
        PawShape::Toes,
        PawShape::Long,
        PawShape::Hoof,
        PawShape::Stomp,
        PawShape::Wing,
    ];
}

/// The colors of a paw (linear, like the models' vertex colors): its fur (the arm), its foot (the
/// end of the paw, its toes, its hooves) and its sole (the pads, the nails).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PawColors {
    pub fur: [f32; 4],
    pub foot: [f32; 4],
    pub sole: [f32; 4],
}

impl PawColors {
    /// Without a model (the drivers in primitives): a cream paw, its pads pink.
    pub fn cream() -> Self {
        Self {
            fur: srgb(0.98, 0.9, 0.78),
            foot: srgb(1.0, 0.94, 0.85),
            sole: srgb(0.9, 0.55, 0.6),
        }
    }
}

/// An animal's paw, ready to be made into meshes.
#[derive(Clone, Debug, PartialEq)]
pub struct PawModel {
    pub shape: PawShape,
    /// The arm: 1 long along +Y, from its shoulder (y = 0) to its wrist (y = 1).
    pub arm: MeshData,
    /// The hand: centered on its middle, from its wrist (y = -`hand_length` / 2) along +Y, its
    /// palm toward -Z.
    pub hand: MeshData,
    /// How long the hand is along +Y.
    pub hand_length: f32,
    /// Where the arm comes out, in the space of the body's node, on the animal's left (+X in its
    /// file): mirror X for its right.
    pub shoulder: Vec3,
    pub colors: PawColors,
}

/// How far inside its body's surface a shoulder is (in the animal's units).
const SHOULDER_INSET: f32 = 0.06;
/// How high on the flat of its body's side a shoulder is (a share of it): about where the birds'
/// wings are.
const SHOULDER_HEIGHT: f32 = 0.25;
/// How far behind the middle of its body's side a shoulder is (a share of its half depth).
const SHOULDER_BACK: f32 = 0.25;
/// A wing used as a hand is this much bigger than on the body (before [`HAND_SCALE`]).
const WING_HAND_SCALE: f32 = 1.2;
/// Every hand is this much bigger than it is built: it reads from the chase camera.
const HAND_SCALE: f32 = 1.15;

/// The node named `name` among `nodes`.
fn named<'a>(nodes: &'a [AnimalNode], name: &str) -> Option<&'a AnimalNode> {
    nodes.iter().find(|n| n.name == name)
}

/// The mean of some colors weighted (None: none, or no weight).
fn mean(colors: impl Iterator<Item = ([f32; 4], f32)>) -> Option<[f32; 4]> {
    let (sum, weight) = colors.fold(([0.0f32; 4], 0.0f32), |(sum, w), (c, k)| {
        ([0, 1, 2, 3].map(|i| sum[i] + c[i] * k), w + k)
    });
    (weight > 0.0).then(|| sum.map(|s| s / weight))
}

/// How far apart two colors are (the largest of their channels' differences).
fn apart(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max)
}

/// `color` darker (`by` from 0 to 1).
fn darker(color: [f32; 4], by: f32) -> [f32; 4] {
    let k = 1.0 - by.clamp(0.0, 1.0);
    [color[0] * k, color[1] * k, color[2] * k, color[3]]
}

/// The triangles of `mesh`: their middle, their facing (unit), their area and their color (their
/// first corner's: a Cube Pet's face takes one color of its palette).
fn faces(mesh: &MeshData) -> impl Iterator<Item = (Vec3, Vec3, f32, [f32; 4])> + '_ {
    mesh.indices.as_chunks::<3>().0.iter().filter_map(|t| {
        let corner = |i: u32| mesh.positions.get(i as usize).map(|p| Vec3::from(*p));
        let (a, b, c) = (corner(t[0])?, corner(t[1])?, corner(t[2])?);
        let cross = (b - a).cross(c - a);
        let color = *mesh.colors.get(t[0] as usize)?;
        Some((
            (a + b + c) / 3.0,
            cross.normalize_or_zero(),
            cross.length() / 2.0,
            color,
        ))
    })
}

/// The colors of a leg's mesh (a Cube Pet's leg: a bevelled block hanging from its top), face by
/// face: its fur on its sides, its foot on its lower ring, its sole underneath. The pads stand out
/// from the foot: a sole of the foot's color is darkened.
pub fn leg_colors(leg: &MeshData) -> Option<PawColors> {
    let (lo, hi) = bounds(leg)?;
    let height = (hi.y - lo.y).max(1e-4);
    let fur = mean(
        faces(leg)
            .filter(|(p, n, ..)| {
                (lo.y + 0.35 * height..hi.y - 0.1 * height).contains(&p.y) && n.y.abs() < 0.6
            })
            .map(|(.., area, c)| (c, area)),
    )?;
    let foot = mean(
        faces(leg)
            .filter(|(p, n, ..)| p.y < lo.y + 0.25 * height && n.y > -0.8)
            .map(|(.., area, c)| (c, area)),
    )
    .unwrap_or(fur);
    let sole = mean(
        faces(leg)
            .filter(|(_, n, ..)| n.y < -0.8)
            .map(|(.., area, c)| (c, area)),
    )
    .unwrap_or(foot);
    let sole = if apart(sole, foot) < 0.04 {
        darker(foot, 0.45)
    } else {
        sole
    };
    Some(PawColors { fur, foot, sole })
}

/// The colors of a wing's mesh (its root along -X, its tip along +X), face by face: its root's for
/// the arm, its tip's for the foot.
pub fn wing_colors(wing: &MeshData) -> Option<PawColors> {
    let (lo, hi) = bounds(wing)?;
    let length = (hi.x - lo.x).max(1e-4);
    let along = |from: f32, to: f32| {
        mean(
            faces(wing)
                .filter(|(p, ..)| (lo.x + from * length..=lo.x + to * length).contains(&p.x))
                .map(|(.., area, c)| (c, area)),
        )
    };
    let fur = along(0.0, 0.35)?;
    let foot = along(0.65, 1.0).unwrap_or(fur);
    Some(PawColors {
        fur,
        foot,
        sole: darker(foot, 0.3),
    })
}

/// The front left leg of `data` (or its first leg).
fn front_leg(data: &AnimalModelData) -> Option<&AnimalNode> {
    named(&data.legs, "leg-front-left").or(data.legs.first())
}

/// The left wing of `data` (a bird's), with its mesh.
fn left_wing(data: &AnimalModelData) -> Option<&AnimalNode> {
    data.nodes
        .iter()
        .find(|n| n.part == Some(AnimalPart::LeftWing) && n.mesh.is_some())
}

/// The colors of `data`'s paw, taken on its model: its wing's for a [`PawShape::Wing`] (its leg's
/// without a wing), else its front leg's; cream without either.
pub fn paw_colors(data: &AnimalModelData, shape: PawShape) -> PawColors {
    let wing = || left_wing(data).and_then(|w| wing_colors(w.mesh.as_ref()?));
    let leg = || front_leg(data).and_then(|l| leg_colors(l.mesh.as_ref()?));
    match shape {
        PawShape::Wing => wing().or_else(leg),
        _ => leg().or_else(wing),
    }
    .unwrap_or_else(PawColors::cream)
}

/// How far along `dir` (a unit vector) the ray from `origin` first meets one of `mesh`'s
/// triangles (None: never).
pub fn first_hit(mesh: &MeshData, origin: Vec3, dir: Vec3) -> Option<f32> {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|t| {
            let corner = |i: u32| mesh.positions.get(i as usize).map(|p| Vec3::from(*p));
            let (a, b, c) = (corner(t[0])?, corner(t[1])?, corner(t[2])?);
            // Möller–Trumbore.
            let (e1, e2) = (b - a, c - a);
            let h = dir.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-9 {
                return None;
            }
            let f = 1.0 / det;
            let s = origin - a;
            let u = f * s.dot(h);
            let q = s.cross(e1);
            let v = f * dir.dot(q);
            let d = f * e2.dot(q);
            ((0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && d > 1e-6).then_some(d)
        })
        .reduce(f32::min)
}

/// Where `data`'s arm comes out, in its body's node, on its left (see [`PawModel::shoulder`]): a
/// bird's wing root (for a [`PawShape::Wing`]); else on the flat of its body's side (measured
/// from inside, by rays: the elephant's ears, the bunny's, its face's details do not count), a
/// quarter of the way up that flat, a little behind its middle (behind the elephant's ears),
/// just inside its surface. None without a body.
pub fn shoulder(data: &AnimalModelData, shape: PawShape) -> Option<Vec3> {
    let body_index = data.node(BODY)?;
    let body = &data.nodes[body_index];
    let mesh = body.mesh.as_ref()?;
    if shape == PawShape::Wing
        && let Some(wing) = left_wing(data).filter(|w| w.parent == Some(body_index))
    {
        return Some(wing.local.translation - Vec3::X * SHOULDER_INSET);
    }
    let (lo, hi) = bounds(mesh)?;
    let middle = (lo + hi) / 2.0;
    // The body's side at each height, from its middle.
    const STEPS: usize = 48;
    let side_at = |y: f32, z: f32| first_hit(mesh, Vec3::new(middle.x, y, z), Vec3::X);
    let mut widest = 0.0f32;
    let mut sides = [None; STEPS];
    for (k, side) in sides.iter_mut().enumerate().skip(1) {
        let y = lo.y + (hi.y - lo.y) * k as f32 / STEPS as f32;
        *side = side_at(y, middle.z).map(|d| (y, d));
        widest = widest.max(side.map_or(0.0, |(_, d)| d));
    }
    // Its flat: where it is (almost) at its widest.
    let flat = sides
        .iter()
        .flatten()
        .filter(|(_, d)| *d >= widest * 0.98)
        .map(|(y, _)| *y);
    let (bottom, top) = flat.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
    if bottom > top {
        return None;
    }
    let y = bottom + SHOULDER_HEIGHT * (top - bottom);
    let ahead = first_hit(mesh, Vec3::new(middle.x, y, middle.z), Vec3::Z)?;
    let behind = first_hit(mesh, Vec3::new(middle.x, y, middle.z), Vec3::NEG_Z)?;
    let z = middle.z + (ahead - behind) / 2.0 - SHOULDER_BACK * (ahead + behind) / 2.0;
    let x = middle.x + side_at(y, z)?;
    Some(Vec3::new(x - SHOULDER_INSET, y, z))
}

/// A soft block (a rounded box, low-poly: the Cube Pets' bevelled cubes) centered on `center`.
fn block(
    mesh: &mut MeshData,
    center: Vec3,
    half: Vec3,
    round: (f32, f32),
    detail: Detail,
    color: impl Fn(Vec3) -> [f32; 4],
) {
    rounded_box(
        mesh,
        Mat4::from_translation(center),
        half,
        round.0,
        round.1,
        detail,
        color,
    );
}

/// Fine enough for a hand's palm, and for its small parts.
const PALM: Detail = Detail::new(16, 9);
const SMALL: Detail = Detail::new(10, 6);

/// A paw with toes (`toes` of them across its end) and their pads: `palm` its half size (its
/// length along Y), `toe` a toe's half size. Returns its length.
fn toed_hand(mesh: &mut MeshData, colors: &PawColors, palm: Vec3, toes: usize, toe: Vec3) -> f32 {
    let PawColors { fur, foot, sole } = *colors;
    // The palm, its fur turning into the foot's color toward the toes.
    block(mesh, Vec3::Y * palm.y, palm, (0.45, 0.4), PALM, move |p| {
        mix(fur, foot, (p.y / palm.y + 0.6).clamp(0.0, 1.0))
    });
    // The big pad under the palm.
    block(
        mesh,
        Vec3::new(0.0, palm.y * 0.8, -palm.z * 0.86),
        Vec3::new(palm.x * 0.62, palm.y * 0.55, palm.z * 0.3),
        (0.6, 0.6),
        SMALL,
        move |_| sole,
    );
    // The toes across the end, the middle ones a little further, each its bean underneath.
    let spread = 2.0 * (palm.x - toe.x) / (toes.max(2) - 1) as f32;
    for i in 0..toes {
        let k = i as f32 - (toes - 1) as f32 / 2.0;
        let further = if k.abs() < 1.0 { 0.03 } else { 0.0 };
        let at = Vec3::new(k * spread, 2.0 * palm.y + toe.y * 0.35 + further, 0.0);
        block(mesh, at, toe, (0.5, 0.5), SMALL, move |_| foot);
        block(
            mesh,
            at + Vec3::new(0.0, toe.y * 0.1, -toe.z * 0.82),
            Vec3::new(toe.x * 0.72, toe.y * 0.62, toe.z * 0.34),
            (0.6, 0.6),
            SMALL,
            move |_| sole,
        );
    }
    2.0 * palm.y + toe.y * 1.35 + 0.03
}

/// A cloven hoof: the end of the arm, then two halves of hoof. Returns its length.
fn hoof_hand(mesh: &mut MeshData, colors: &PawColors) -> f32 {
    let PawColors { fur, foot, .. } = *colors;
    let wrist = Vec3::new(0.2, 0.09, 0.17);
    block(
        mesh,
        Vec3::Y * wrist.y,
        wrist,
        (0.4, 0.4),
        PALM,
        move |_| fur,
    );
    let half = Vec3::new(0.1, 0.13, 0.16);
    let tip = darker(foot, 0.25);
    for x in [-1.0, 1.0] {
        block(
            mesh,
            Vec3::new(x * (half.x + 0.004), 2.0 * wrist.y + half.y * 0.85, 0.0),
            half,
            (0.35, 0.45),
            PALM,
            move |p| mix(foot, tip, (p.y / half.y + 1.0) / 2.0),
        );
    }
    2.0 * wrist.y + half.y * 1.85
}

/// A big round foot, its sole at its end and three nails at its front. Returns its length.
fn stomp_hand(mesh: &mut MeshData, colors: &PawColors) -> f32 {
    let PawColors { fur, foot, sole } = *colors;
    // Its nails: its sole's color, paler (they show on the foot).
    let nail = mix(sole, srgb(1.0, 1.0, 1.0), 0.45);
    let column = Vec3::new(0.3, 0.2, 0.27);
    block(
        mesh,
        Vec3::Y * column.y,
        column,
        (0.35, 0.85),
        PALM,
        move |p| mix(fur, foot, (p.y / column.y + 0.5).clamp(0.0, 1.0)),
    );
    // The sole, a pale disc at the end.
    block(
        mesh,
        Vec3::Y * (2.0 * column.y),
        Vec3::new(column.x * 0.9, 0.03, column.z * 0.9),
        (0.6, 0.85),
        PALM,
        move |_| sole,
    );
    // Three nails on its front (its palm's side: what the camera sees), at its end.
    for x in [-0.15, 0.0, 0.15] {
        block(
            mesh,
            Vec3::new(x, 2.0 * column.y - 0.07, -column.z * 0.88),
            Vec3::new(0.075, 0.07, 0.06),
            (0.55, 0.55),
            SMALL,
            move |_| nail,
        );
    }
    2.0 * column.y + 0.03
}

/// A bird's wing as a hand: its root at the wrist, its length along +Y, its top toward +Z (its
/// front edge along X), a little bigger. Returns its length.
fn wing_hand(mesh: &mut MeshData, wing: &MeshData) -> f32 {
    let Some((lo, hi)) = bounds(wing) else {
        return 0.0;
    };
    // (x, y, z) of the wing → (z, x, y) of the hand: a rotation (its determinant is 1).
    let turn = Mat4::from_cols(Vec4::Y, Vec4::Z, Vec4::X, Vec4::W);
    let at = Mat4::from_scale(Vec3::splat(WING_HAND_SCALE))
        * turn
        * Mat4::from_translation(Vec3::new(-lo.x, -(lo.y + hi.y) / 2.0, -(lo.z + hi.z) / 2.0));
    crate::models::bake::append_transformed(mesh, wing, at);
    (hi.x - lo.x) * WING_HAND_SCALE
}

/// The arm: a soft bar from its shoulder (y = 0) to its wrist (y = 1), `half` its half section,
/// its fur a little darker at the shoulder.
fn arm_mesh(colors: &PawColors, half: Vec2) -> MeshData {
    let mut mesh = MeshData::default();
    let fur = colors.fur;
    let root = darker(fur, 0.12);
    block(
        &mut mesh,
        Vec3::Y * 0.5,
        Vec3::new(half.x, 0.5, half.y),
        (0.2, 0.45),
        Detail::new(12, 9),
        move |p| mix(root, fur, (p.y + 0.5).clamp(0.0, 1.0)),
    );
    mesh
}

/// The arm's and the hand's meshes of a paw of `shape` in `colors`; a [`PawShape::Wing`] uses
/// `wing` (the bird's left wing; a round paw without it). The shoulder is left at the origin.
pub fn paw_meshes(shape: PawShape, colors: &PawColors, wing: Option<&MeshData>) -> PawModel {
    let mut hand = MeshData::default();
    let (hand_length, arm) = match (shape, wing) {
        (PawShape::Wing, Some(wing)) => (
            wing_hand(&mut hand, wing),
            arm_mesh(colors, Vec2::new(0.18, 0.07)),
        ),
        (PawShape::Long, _) => (
            toed_hand(
                &mut hand,
                colors,
                Vec3::new(0.19, 0.22, 0.14),
                3,
                Vec3::new(0.065, 0.09, 0.11),
            ),
            arm_mesh(colors, Vec2::new(0.12, 0.11)),
        ),
        (PawShape::Hoof, _) => (
            hoof_hand(&mut hand, colors),
            arm_mesh(colors, Vec2::new(0.13, 0.12)),
        ),
        (PawShape::Stomp, _) => (
            stomp_hand(&mut hand, colors),
            arm_mesh(colors, Vec2::new(0.19, 0.18)),
        ),
        (PawShape::Toes | PawShape::Wing, _) => (
            toed_hand(
                &mut hand,
                colors,
                Vec3::new(0.24, 0.17, 0.16),
                4,
                Vec3::new(0.062, 0.08, 0.12),
            ),
            arm_mesh(colors, Vec2::new(0.15, 0.14)),
        ),
    };
    // A cartoon's paw: a little bigger than the leg it comes from; centered on its middle.
    let hand = crate::models::bake::transformed(
        &hand,
        Mat4::from_translation(Vec3::NEG_Y * hand_length * HAND_SCALE / 2.0)
            * Mat4::from_scale(Vec3::splat(HAND_SCALE)),
    );
    PawModel {
        shape,
        arm,
        hand,
        hand_length: hand_length * HAND_SCALE,
        shoulder: Vec3::ZERO,
        colors: *colors,
    }
}

/// `data`'s paw of `shape`: its colors taken on its model, its shoulder measured on its body
/// (None without a body).
pub fn paw_model(data: &AnimalModelData, shape: PawShape) -> Option<PawModel> {
    let shoulder = shoulder(data, shape)?;
    let colors = paw_colors(data, shape);
    let wing = left_wing(data).and_then(|w| w.mesh.as_ref());
    Some(PawModel {
        shoulder,
        ..paw_meshes(shape, &colors, wing)
    })
}

/// The paw of the drivers in primitives (no model): round, cream.
pub fn primitive_paw() -> PawModel {
    paw_meshes(PawShape::Toes, &PawColors::cream(), None)
}

/// `node`'s transform at rest in its animal's file (the product of its parents' local
/// transforms).
pub fn rest_transform(data: &AnimalModelData, node: usize) -> Mat4 {
    let mut at = Mat4::IDENTITY;
    let mut index = Some(node);
    while let Some(i) = index {
        let Some(n) = data.nodes.get(i) else {
            break;
        };
        at = n.local.to_matrix() * at;
        index = n.parent;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adds the quad `corners` facing `out`, of one color.
    fn face(mesh: &mut MeshData, corners: [Vec3; 4], out: Vec3, color: [f32; 4]) {
        let base = mesh.positions.len() as u32;
        for p in corners {
            mesh.positions.push(p.to_array());
            mesh.normals.push(out.to_array());
            mesh.colors.push(color);
        }
        let [a, b, c, _] = corners;
        let turned = (b - a).cross(c - a).dot(out) < 0.0;
        for t in [[0, 1, 2], [0, 2, 3]] {
            let t = if turned { [t[0], t[2], t[1]] } else { t };
            mesh.indices.extend(t.map(|i| base + i));
        }
    }

    /// A Cube Pet's leg: a block 0.376 wide and 0.3 high hanging from its top, its sides of one
    /// color, its lower ring of another, its sole of a third.
    fn leg(fur: [f32; 4], foot: [f32; 4], sole: [f32; 4]) -> MeshData {
        let mut mesh = MeshData::default();
        let h = 0.188;
        // Each side, from its first corner (x, z) to the next one, facing out.
        for (x, z) in [(1.0f32, 1.0f32), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let out = Vec3::new(x - z, 0.0, z + x).normalize();
            let corner = |y: f32, k: f32| {
                // Along the side, from its first corner to the next one.
                let from = Vec3::new(x * h, y, z * h);
                let next = Vec3::new(-z * h, y, x * h);
                from.lerp(next, k)
            };
            face(
                &mut mesh,
                [
                    corner(-0.05, 0.0),
                    corner(-0.2, 0.0),
                    corner(-0.2, 1.0),
                    corner(-0.05, 1.0),
                ],
                out,
                fur,
            );
            face(
                &mut mesh,
                [
                    corner(-0.24, 0.0),
                    corner(-0.29, 0.0),
                    corner(-0.29, 1.0),
                    corner(-0.24, 1.0),
                ],
                out,
                foot,
            );
        }
        let square = |y: f32| {
            [
                Vec3::new(-h, y, -h),
                Vec3::new(h, y, -h),
                Vec3::new(h, y, h),
                Vec3::new(-h, y, h),
            ]
        };
        face(&mut mesh, square(-0.3), Vec3::NEG_Y, sole);
        face(&mut mesh, square(0.0), Vec3::Y, fur);
        mesh
    }

    /// The colors of a paw come from the leg: its sides, its foot, its sole; a sole of the
    /// foot's color is darkened (the pads stand out). A wing's: its root's and its tip's.
    #[test]
    fn a_paw_takes_its_colors_on_the_leg_or_the_wing() {
        let (fur, foot, sole) = (
            srgb(0.4, 0.4, 0.5),
            srgb(0.5, 0.52, 0.6),
            srgb(0.3, 0.3, 0.35),
        );
        let colors = leg_colors(&leg(fur, foot, sole)).unwrap();
        for (got, wanted) in [(colors.fur, fur), (colors.foot, foot), (colors.sole, sole)] {
            assert!(apart(got, wanted) < 1e-5, "{colors:?}");
        }
        let plain = leg_colors(&leg(fur, foot, foot)).unwrap();
        assert!(apart(plain.sole, foot) > 0.1, "{plain:?}");
        assert!(leg_colors(&MeshData::default()).is_none());

        // A flat wing: its root (from x = -0.1) of one color, its tip (to 0.47) of another.
        let mut wing = MeshData::default();
        let strip = |x0: f32, x1: f32| {
            [
                Vec3::new(x0, 0.0, -0.3),
                Vec3::new(x1, 0.0, -0.3),
                Vec3::new(x1, 0.0, 0.3),
                Vec3::new(x0, 0.0, 0.3),
            ]
        };
        face(&mut wing, strip(-0.1, 0.05), Vec3::Y, fur);
        face(&mut wing, strip(0.05, 0.3), Vec3::Y, mix(fur, foot, 0.5));
        face(&mut wing, strip(0.3, 0.47), Vec3::Y, foot);
        let colors = wing_colors(&wing).unwrap();
        assert!(apart(colors.fur, fur) < 1e-5 && apart(colors.foot, foot) < 1e-5);
    }

    /// Every shape is a real paw: a hand wider than its arm, longer than wide is not required,
    /// its end (the toes, the hooves, the sole) of the foot's color, and pads or nails of the
    /// sole's color where it has some; all of it facing out, nothing NaN.
    #[test]
    fn every_shape_is_a_paw_wider_than_its_arm() {
        let colors = PawColors {
            fur: srgb(0.4, 0.4, 0.5),
            foot: srgb(0.8, 0.5, 0.3),
            sole: srgb(0.2, 0.2, 0.25),
        };
        let mut wing = MeshData::default();
        rounded_box(
            &mut wing,
            Mat4::from_translation(Vec3::new(0.19, 0.0, 0.0)),
            Vec3::new(0.29, 0.1, 0.3),
            0.4,
            0.4,
            SMALL,
            |p| {
                if p.x > 0.0 {
                    srgb(1.0, 0.8, 0.3)
                } else {
                    srgb(1.0, 0.6, 0.2)
                }
            },
        );
        for shape in PawShape::ALL {
            let paw = paw_meshes(shape, &colors, Some(&wing));
            let (alo, ahi) = bounds(&paw.arm).unwrap();
            let (hlo, hhi) = bounds(&paw.hand).unwrap();
            assert!(
                (alo.y).abs() < 1e-4 && (ahi.y - 1.0).abs() < 1e-4,
                "{shape:?}: 0 to 1"
            );
            assert!(
                hhi.x - hlo.x > 1.5 * (ahi.x - alo.x),
                "{shape:?}: the hand wider than the arm"
            );
            let half = paw.hand_length / 2.0;
            assert!(
                (hlo.y + half).abs() < 0.02 && (hhi.y - half).abs() < 0.02,
                "{shape:?}: {} long, from {} to {}: centered",
                paw.hand_length,
                hlo.y,
                hhi.y
            );
            for mesh in [&paw.arm, &paw.hand] {
                assert!(mesh.positions.iter().flatten().all(|v| v.is_finite()));
                assert_eq!(mesh.positions.len(), mesh.colors.len());
                assert!(mesh.triangle_count() > 50);
            }
            if shape == PawShape::Wing {
                // The wing's own colors.
                assert!(paw.hand.colors.contains(&srgb(1.0, 0.8, 0.3)));
                continue;
            }
            assert!(paw.arm.colors.contains(&colors.fur), "{shape:?}: its fur");
            // The end of the hand: the foot's color.
            let end: Vec<[f32; 4]> = paw
                .hand
                .positions
                .iter()
                .zip(&paw.hand.colors)
                .filter(|(p, _)| p[1] > paw.hand_length * 0.3)
                .map(|(_, c)| *c)
                .collect();
            assert!(
                end.iter()
                    .any(|c| apart(*c, colors.foot) < 0.2 && apart(*c, colors.sole) > 0.05),
                "{shape:?}: its end of the foot's color"
            );
            if shape != PawShape::Hoof {
                assert!(
                    paw.hand.colors.contains(&colors.sole),
                    "{shape:?}: pads or nails"
                );
            }
        }
        // Without a wing, a bird would get a round paw rather than nothing.
        let paw = paw_meshes(PawShape::Wing, &colors, None);
        assert!(paw.hand.triangle_count() > 50);
    }
}
