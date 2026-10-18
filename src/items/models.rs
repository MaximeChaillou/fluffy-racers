//! The items' models (issue #106, GD § 8 and 14): one per item of [`ItemKind::ALL`], the
//! croquette's and the gift box's, round and soft, each one read at a glance (its own silhouette,
//! its own colors, far from the decor's). Pure, no ECS: `visual.rs` makes their meshes, `icons.rs`
//! their icons, the same data.
//!
//! - **Built in code** ([`model`]), in soft shapes (`models::shape`) merged into one mesh per
//!   model (vertex colors: the models' shared material): every model has this form, the one the
//!   headless game and a missing file stand in.
//! - **From a CC0 file** ([`MODEL_FILES`], Kenney's packs in `assets/models/`): the gift box (the
//!   Holiday Kit's round present, in sky blue, gold for a double box). Once the file is baked
//!   ([`from_file`]), it takes the place of the built one, at its size and place.
//!
//! Each model is in the space of its item: +Y up, its front toward +Z, its unit the item's radius
//! (`items::item_radius`), its origin the item's position (the ground is under it: at −0.5 for a
//! pelure, at −1 for what rolls on it). The croquette's and the gift box's unit is the meter.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use super::table::ItemKind;
use crate::models::BakedModel;
use crate::models::bake::append_transformed;
use crate::models::shape::{
    Detail, Ends, at, bounds, cone_rings, ellipsoid, mix, moved, rounded_box, solid, srgb, torus,
    tube,
};
use crate::track::mesh::MeshData;

/// What a model is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    Item(ItemKind),
    /// A croquette of the track (GD § 7).
    Croquette,
    /// A box of the track (GD § 8), and a double box.
    GiftBox,
    DoubleGiftBox,
}

impl Look {
    /// Every model: the sixteen items, the croquette, the two boxes.
    pub fn all() -> impl Iterator<Item = Look> {
        ItemKind::ALL.into_iter().map(Look::Item).chain([
            Look::Croquette,
            Look::GiftBox,
            Look::DoubleGiftBox,
        ])
    }

    /// Its file, if it has one ([`MODEL_FILES`]).
    pub fn file(self) -> Option<&'static str> {
        MODEL_FILES
            .iter()
            .find(|(look, _)| *look == self)
            .map(|(_, file)| *file)
    }
}

/// The models taken from a CC0 file (Kenney's packs, `assets/models/<pack>/`, credited in
/// `assets/CREDITS.md`), by name ("pack/name").
///
/// Only the gift box (the Holiday Kit's round present, its paper and ribbon in the game's
/// colors): the packs' other candidates read less well than the models built in code, in the
/// HUD's small icons as on the track (the Food Kit's pepper looks like a carrot, its whipped
/// cream like a heap of snow; the Platformer Kit's bomb has no confetti, its heart no face).
pub const MODEL_FILES: [(Look, &str); 2] = [
    (Look::GiftBox, "holiday-kit/present-a-round"),
    (Look::DoubleGiftBox, "holiday-kit/present-a-round"),
];

/// A model: its body, and the parts that move on their own (each one with where it sits on the
/// body).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemModel {
    /// Everything that does not move on its own, merged: one mesh.
    pub body: MeshData,
    /// The abeille's right wing (its left one is the same, mirrored), hinged at its own origin:
    /// it beats around the body's length (Z). Its hinge on the body (the left one at −x).
    pub wing: Option<(MeshData, Vec3)>,
    /// The bombe's spark, at the tip of its fuse: it glows and blinks.
    pub spark: Option<(MeshData, Vec3)>,
    /// The gift box's lid with its bow, its origin where it sits on the box: it wobbles, and
    /// pops off when the box bursts.
    pub lid: Option<(MeshData, Vec3)>,
}

impl ItemModel {
    /// The whole model at rest as one mesh (the icon's): the body and its parts in place (both
    /// wings, raised a little).
    pub fn merged(&self) -> MeshData {
        let mut all = self.body.clone();
        if let Some((wing, hinge)) = &self.wing {
            for side in [1.0f32, -1.0] {
                let turn = Mat4::from_translation(*hinge * Vec3::new(side, 1.0, 1.0))
                    * Mat4::from_scale(Vec3::new(side, 1.0, 1.0))
                    * Mat4::from_rotation_z(WING_REST);
                append_transformed(&mut all, wing, turn);
            }
        }
        for (part, offset) in [&self.spark, &self.lid].into_iter().flatten() {
            append_transformed(&mut all, part, Mat4::from_translation(*offset));
        }
        all
    }

    pub fn triangle_count(&self) -> usize {
        self.merged().triangle_count()
    }
}

/// The abeille's wings at rest (the icon), raised this much (rad).
pub const WING_REST: f32 = 0.35;

// --- The palette ------------------------------------------------------------------------------

/// The models' colors (sRGB): bright, candy-like, far from the decor's greens, greys and sands.
mod color {
    pub const BANANA: (f32, f32, f32) = (1.0, 0.86, 0.18);
    pub const BANANA_TIP: (f32, f32, f32) = (0.42, 0.26, 0.1);
    pub const BANANA_INSIDE: (f32, f32, f32) = (1.0, 0.95, 0.7);
    pub const YARN: (f32, f32, f32) = (0.97, 0.36, 0.62);
    pub const YARN_DARK: (f32, f32, f32) = (0.78, 0.16, 0.45);
    pub const BEE: (f32, f32, f32) = (1.0, 0.8, 0.1);
    pub const INK: (f32, f32, f32) = (0.11, 0.09, 0.12);
    pub const WHITE: (f32, f32, f32) = (0.98, 0.98, 1.0);
    pub const WING: (f32, f32, f32) = (0.9, 0.96, 1.0);
    pub const NAVY: (f32, f32, f32) = (0.2, 0.33, 0.86);
    pub const STEEL: (f32, f32, f32) = (0.6, 0.62, 0.7);
    pub const FUSE: (f32, f32, f32) = (0.95, 0.83, 0.55);
    pub const SPARK: (f32, f32, f32) = (1.0, 0.72, 0.15);
    pub const CHILI: (f32, f32, f32) = (0.92, 0.14, 0.08);
    pub const STEM: (f32, f32, f32) = (0.25, 0.7, 0.22);
    pub const GOLD: (f32, f32, f32) = (1.0, 0.78, 0.16);
    pub const GOLD_DEEP: (f32, f32, f32) = (0.9, 0.55, 0.08);
    pub const CREAM: (f32, f32, f32) = (1.0, 0.97, 0.9);
    pub const CUP: (f32, f32, f32) = (1.0, 0.55, 0.7);
    pub const CHERRY: (f32, f32, f32) = (0.85, 0.08, 0.2);
    pub const HUG: (f32, f32, f32) = (1.0, 0.38, 0.7);
    pub const BLUSH: (f32, f32, f32) = (1.0, 0.62, 0.78);
    pub const ROCKET: (f32, f32, f32) = (1.0, 0.45, 0.1);
    pub const FIN: (f32, f32, f32) = (0.9, 0.12, 0.3);
    pub const WINDOW: (f32, f32, f32) = (0.35, 0.78, 1.0);
    pub const BALLOON: (f32, f32, f32) = (0.55, 0.84, 1.0);
    pub const BALLOON_DARK: (f32, f32, f32) = (0.22, 0.45, 0.82);
    pub const PILLOW: (f32, f32, f32) = (0.78, 0.64, 1.0);
    pub const PIPING: (f32, f32, f32) = (0.47, 0.3, 0.9);
    pub const DUCK: (f32, f32, f32) = (1.0, 0.88, 0.15);
    pub const HORN: (f32, f32, f32) = (1.0, 0.5, 0.05);
    pub const MAGPIE: (f32, f32, f32) = (0.1, 0.1, 0.17);
    pub const MAGPIE_SHEEN: (f32, f32, f32) = (0.16, 0.28, 0.55);
    pub const BEAK: (f32, f32, f32) = (0.3, 0.3, 0.35);
    pub const PINATA: (f32, f32, f32) = (0.3, 0.78, 1.0);
    pub const PINATA_CONES: [(f32, f32, f32); 6] = [
        (1.0, 0.3, 0.55),
        (1.0, 0.85, 0.2),
        (0.45, 0.9, 0.35),
        (1.0, 0.55, 0.12),
        (0.72, 0.42, 1.0),
        (1.0, 0.3, 0.55),
    ];
    pub const BONE: (f32, f32, f32) = (0.98, 0.47, 0.2);
    pub const BONE_SHADE: (f32, f32, f32) = (0.84, 0.3, 0.12);
    pub const KIBBLE: (f32, f32, f32) = (0.9, 0.58, 0.26);
    pub const KIBBLE_DARK: (f32, f32, f32) = (0.62, 0.34, 0.12);
    pub const BOX: (f32, f32, f32) = (0.42, 0.78, 1.0);
    pub const RIBBON: (f32, f32, f32) = (1.0, 0.32, 0.6);
    pub const DOUBLE_BOX: (f32, f32, f32) = (1.0, 0.8, 0.22);
    pub const DOUBLE_RIBBON: (f32, f32, f32) = (0.9, 0.14, 0.16);
}

fn c((r, g, b): (f32, f32, f32)) -> [f32; 4] {
    srgb(r, g, b)
}

const BLOB: Detail = Detail::new(16, 11);
const SMALL: Detail = Detail::new(10, 7);
const TINY: Detail = Detail::new(7, 5);

/// An eye looking out along `toward`: a white ball and its black pupil.
fn eye(mesh: &mut MeshData, center: Vec3, size: f32, toward: Vec3) {
    ellipsoid(
        mesh,
        Mat4::from_translation(center),
        Vec3::splat(size),
        SMALL,
        solid(c(color::WHITE)),
    );
    ellipsoid(
        mesh,
        Mat4::from_translation(center + toward.normalize() * size * 0.62),
        Vec3::splat(size * 0.55),
        TINY,
        solid(c(color::INK)),
    );
}

// --- The models built in code -------------------------------------------------------------------

/// The model of `look` built in code (what a missing file and the headless game stand in).
pub fn model(look: Look) -> ItemModel {
    match look {
        Look::Item(item) => item_model(item),
        Look::Croquette => ItemModel {
            body: croquette(c(color::KIBBLE), c(color::KIBBLE_DARK)),
            ..default()
        },
        Look::GiftBox => gift_box(c(color::BOX), c(color::RIBBON)),
        Look::DoubleGiftBox => gift_box(c(color::DOUBLE_BOX), c(color::DOUBLE_RIBBON)),
    }
}

fn item_model(item: ItemKind) -> ItemModel {
    let body = |body| ItemModel { body, ..default() };
    match item {
        ItemKind::Pelure => body(pelure()),
        ItemKind::Pelote => body(pelote()),
        ItemKind::Abeille => abeille(),
        ItemKind::Piment => body(piment(c(color::CHILI), c(color::STEM))),
        ItemKind::PimentDore => body(piment(c(color::GOLD), c(color::GOLD_DEEP))),
        ItemKind::CroquetteDoree => body(golden_croquettes()),
        ItemKind::Bombe => bombe(),
        ItemKind::Chantilly => body(chantilly()),
        ItemKind::SuperCalin => body(super_calin()),
        ItemKind::Fusee => body(fusee()),
        ItemKind::Degonflage => body(degonflage()),
        ItemKind::OreillerGeant => body(oreiller()),
        ItemKind::KlaxonCanard => body(klaxon_canard()),
        ItemKind::PieVoleuse => body(pie_voleuse()),
        ItemKind::Pinata => body(pinata()),
        ItemKind::Boomerang => body(boomerang()),
    }
}

/// A banana peel lying on the ground (at −0.5): its stub standing in the middle, its stalk on
/// top, four floppy petals splayed out, brown at their tips.
fn pelure() -> MeshData {
    let mut mesh = MeshData::default();
    let (yellow, tip, inside) = (
        c(color::BANANA),
        c(color::BANANA_TIP),
        c(color::BANANA_INSIDE),
    );
    // The stub: a soft column, pale inside at its top.
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, -0.5, 0.0), 0.0),
            (Vec3::new(0.0, -0.45, 0.0), 0.34),
            (Vec3::new(0.0, 0.05, 0.0), 0.3),
            (Vec3::new(0.02, 0.42, 0.0), 0.22),
            (Vec3::new(0.05, 0.62, 0.0), 0.12),
            (Vec3::new(0.06, 0.66, 0.0), 0.0),
        ],
        Ends::Poles,
        12,
        1.0,
        Vec3::X,
        move |p| if p.y > 0.5 { tip } else { yellow },
    );
    for k in 0..4 {
        let angle = TAU * (k as f32 + 0.15) / 4.0;
        // Out along +X, then turned around: up a little, out, down onto the ground, the tip
        // curling up.
        let path: Vec<(Vec3, f32)> = [
            (0.12, 0.18, 0.2),
            (0.42, 0.26, 0.25),
            (0.78, 0.02, 0.27),
            (1.05, -0.36, 0.24),
            (1.3, -0.42, 0.17),
            (1.45, -0.34, 0.0),
        ]
        .iter()
        .map(|(x, y, r)| (Vec3::new(*x, *y, 0.0), *r))
        .collect();
        tube(
            &mut mesh,
            at(Vec3::ZERO, Quat::from_rotation_y(angle)),
            &path,
            Ends::Poles,
            10,
            0.38,
            Vec3::Y,
            move |p| {
                let base = if p.y > -0.1 && p.x < 0.5 {
                    mix(inside, yellow, (p.x / 0.5).clamp(0.0, 1.0))
                } else {
                    yellow
                };
                mix(base, tip, ((p.x - 1.18) / 0.2).clamp(0.0, 1.0))
            },
        );
    }
    mesh
}

/// A ball of pink yarn, its strands wound around it every way, its loose end hanging.
fn pelote() -> MeshData {
    let mut mesh = MeshData::default();
    let (yarn, dark) = (c(color::YARN), c(color::YARN_DARK));
    ellipsoid(
        &mut mesh,
        Mat4::IDENTITY,
        Vec3::splat(0.9),
        BLOB,
        solid(yarn),
    );
    for (k, (x, z)) in [
        (FRAC_PI_2, 0.0),
        (FRAC_PI_2, 1.05),
        (FRAC_PI_2, -1.05),
        (0.35, 0.5),
    ]
    .into_iter()
    .enumerate()
    {
        let strand = if k % 2 == 0 {
            dark
        } else {
            mix(yarn, dark, 0.55)
        };
        torus(
            &mut mesh,
            at(
                Vec3::ZERO,
                Quat::from_rotation_y(z) * Quat::from_rotation_x(x),
            ),
            0.9,
            0.075,
            Detail::new(28, 5),
            solid(strand),
        );
    }
    // The loose end, from its side down to the ground.
    let end: Vec<(Vec3, f32)> = [
        (0.62, -0.55, 0.36),
        (0.82, -0.72, 0.42),
        (1.02, -0.9, 0.38),
        (1.22, -0.95, 0.5),
        (1.35, -0.95, 0.7),
    ]
    .iter()
    .map(|(x, y, z)| (Vec3::new(*x, *y, *z), 0.07))
    .collect();
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &end,
        Ends::Poles,
        6,
        1.0,
        Vec3::Y,
        solid(dark),
    );
    mesh
}

/// A round bee looking ahead (+Z): a yellow body with two black stripes, big eyes, two antennae,
/// a stinger behind; its two wings beat ([`ItemModel::wing`]).
fn abeille() -> ItemModel {
    let mut mesh = MeshData::default();
    let (bee, ink) = (c(color::BEE), c(color::INK));
    // Its length along Z: its rows are rings across it (the stripes).
    ellipsoid(
        &mut mesh,
        at(Vec3::ZERO, Quat::from_rotation_x(FRAC_PI_2)),
        Vec3::new(0.82, 1.0, 0.8),
        Detail::new(16, 15),
        move |p| {
            // `p.y` runs along the body (turned: toward +Z).
            let z = p.y;
            let stripe = (-0.5..-0.18).contains(&z) || (0.08..0.36).contains(&z);
            if stripe { ink } else { bee }
        },
    );
    for side in [-1.0f32, 1.0] {
        eye(
            &mut mesh,
            Vec3::new(side * 0.32, 0.22, 0.78),
            0.22,
            Vec3::new(side * 0.2, 0.05, 1.0),
        );
        tube(
            &mut mesh,
            Mat4::IDENTITY,
            &[
                (Vec3::new(side * 0.18, 0.66, 0.55), 0.045),
                (Vec3::new(side * 0.28, 0.98, 0.72), 0.04),
                (Vec3::new(side * 0.4, 1.12, 0.86), 0.035),
            ],
            Ends::Open,
            6,
            1.0,
            Vec3::X,
            solid(ink),
        );
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(side * 0.4, 1.12, 0.86)),
            Vec3::splat(0.1),
            TINY,
            solid(ink),
        );
    }
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, -0.02, -0.9), 0.0),
            (Vec3::new(0.0, -0.02, -0.92), 0.17),
            (Vec3::new(0.0, -0.06, -1.12), 0.08),
            (Vec3::new(0.0, -0.1, -1.32), 0.0),
        ],
        Ends::Poles,
        8,
        1.0,
        Vec3::Y,
        solid(ink),
    );
    let mut wing = MeshData::default();
    ellipsoid(
        &mut wing,
        at(Vec3::new(0.55, 0.0, -0.1), Quat::from_rotation_y(0.35)),
        Vec3::new(0.6, 0.05, 0.36),
        SMALL,
        solid(c(color::WING)),
    );
    ItemModel {
        body: mesh,
        wing: Some((wing, Vec3::new(0.42, 0.66, -0.08))),
        ..default()
    }
}

/// A chili lying along Z, curved, its tip down; a green cap and stem (`stem`), or all gold.
fn piment(body: [f32; 4], stem: [f32; 4]) -> MeshData {
    let mut mesh = MeshData::default();
    let rings: Vec<(Vec3, f32)> = (0..=10)
        .map(|k| {
            let t = k as f32 / 10.0;
            let z = 1.0 - 2.0 * t;
            let y = 0.35 * (t * PI * 0.9).sin() - 0.3 * t * t;
            let r = match k {
                0 | 10 => 0.0,
                _ => 0.36 * (1.0 - t).powf(0.7) * (t * 9.0).min(1.0).sqrt() + 0.02,
            };
            (Vec3::new(0.0, y, z), r)
        })
        .collect();
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &rings,
        Ends::Poles,
        12,
        1.0,
        Vec3::Y,
        solid(body),
    );
    // The cap over its thick end, and the stem out of it.
    ellipsoid(
        &mut mesh,
        at(Vec3::new(0.0, 0.03, 0.9), Quat::from_rotation_x(FRAC_PI_2)),
        Vec3::new(0.3, 0.14, 0.3),
        SMALL,
        solid(stem),
    );
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, 0.05, 0.98), 0.07),
            (Vec3::new(0.0, 0.18, 1.12), 0.06),
            (Vec3::new(0.0, 0.32, 1.16), 0.05),
        ],
        Ends::Open,
        6,
        1.0,
        Vec3::Y,
        solid(stem),
    );
    mesh
}

/// A croquette: a puffy ring of kibble standing on its edge (facing Z), toasted at its rim. In
/// meters (the size of the croquettes of GD § 7).
fn croquette(light: [f32; 4], dark: [f32; 4]) -> MeshData {
    let mut mesh = MeshData::default();
    // Puffy: its hole almost closed, a biscuit more than a ring; toasted at its rim, a few
    // darker crumbs.
    torus(
        &mut mesh,
        at(Vec3::ZERO, Quat::from_rotation_x(FRAC_PI_2)),
        0.165,
        0.145,
        Detail::new(14, 8),
        move |p| {
            let out = (Vec2::new(p.x, p.z).length() - 0.165) / 0.145;
            let crumb = (p.x * 41.0).sin() * (p.y * 37.0).sin() * (p.z * 43.0).sin() > 0.35;
            let toast = (out * 0.7 + p.y.abs() * 2.0).clamp(0.0, 1.0);
            mix(light, dark, if crumb { 0.85 } else { toast })
        },
    );
    mesh
}

/// The croquette dorée: two golden croquettes, one before the other (it gives two).
fn golden_croquettes() -> MeshData {
    let one = croquette(c(color::GOLD), c(color::GOLD_DEEP));
    let mut mesh = MeshData::default();
    for (offset, turn) in [
        (Vec3::new(-0.28, 0.22, -0.2), -0.4),
        (Vec3::new(0.25, -0.18, 0.2), 0.35),
    ] {
        append_transformed(
            &mut mesh,
            &one,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(2.6),
                Quat::from_rotation_y(turn),
                offset,
            ),
        );
    }
    mesh
}

/// A royal blue bomb with confetti stuck on it, a steel cap, a fuse; its spark glows at the fuse's
/// tip ([`ItemModel::spark`]).
fn bombe() -> ItemModel {
    let mut mesh = MeshData::default();
    ellipsoid(
        &mut mesh,
        Mat4::IDENTITY,
        Vec3::splat(0.92),
        BLOB,
        solid(c(color::NAVY)),
    );
    // Confetti dots, spread over its front and sides (a golden angle apart).
    for k in 0..12 {
        let y = 0.75 - 1.4 * (k as f32 + 0.5) / 12.0;
        let angle = k as f32 * 2.399_963;
        let ring = (1.0 - y * y).sqrt();
        let normal = Vec3::new(ring * angle.cos(), y, ring * angle.sin());
        let dot = color::PINATA_CONES[k % 5];
        ellipsoid(
            &mut mesh,
            at(normal * 0.9, Quat::from_rotation_arc(Vec3::Y, normal)),
            Vec3::new(0.13, 0.05, 0.13),
            TINY,
            solid(c(dot)),
        );
    }
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &cone_rings(Vec3::new(0.0, 0.78, 0.0), 0.24, 0.3, 0.26, 1),
        Ends::Poles,
        12,
        1.0,
        Vec3::X,
        solid(c(color::STEEL)),
    );
    let fuse_tip = Vec3::new(0.24, 1.34, 0.06);
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, 0.98, 0.0), 0.07),
            (Vec3::new(0.06, 1.14, 0.02), 0.065),
            (Vec3::new(0.16, 1.26, 0.04), 0.06),
            (fuse_tip, 0.055),
        ],
        Ends::Open,
        6,
        1.0,
        Vec3::X,
        solid(c(color::FUSE)),
    );
    ItemModel {
        body: mesh,
        spark: Some((spark_mesh(), fuse_tip)),
        ..default()
    }
}

/// A spark: a little star of six points.
fn spark_mesh() -> MeshData {
    let mut mesh = MeshData::default();
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        ellipsoid(
            &mut mesh,
            at(Vec3::ZERO, Quat::from_rotation_arc(Vec3::Y, axis)),
            Vec3::new(0.07, 0.2, 0.07),
            TINY,
            solid(c(color::SPARK)),
        );
    }
    mesh
}

/// Whipped cream in a pink cup, a cherry on top.
fn chantilly() -> MeshData {
    let mut mesh = MeshData::default();
    let cream = c(color::CREAM);
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &cone_rings(Vec3::new(0.0, -0.95, 0.0), 0.55, 0.5, 0.82, 2),
        Ends::Poles,
        14,
        1.0,
        Vec3::X,
        solid(c(color::CUP)),
    );
    // The swirl: three rings, smaller and smaller, and a peak.
    for (k, (y, major, minor)) in [(-0.3, 0.6, 0.28), (0.05, 0.42, 0.25), (0.35, 0.24, 0.2)]
        .into_iter()
        .enumerate()
    {
        torus(
            &mut mesh,
            at(
                Vec3::new(0.0, y, 0.0),
                Quat::from_rotation_y(k as f32 * 0.8),
            ),
            major,
            minor,
            Detail::new(16, 7),
            solid(cream),
        );
    }
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &cone_rings(Vec3::new(0.0, 0.3, 0.0), 0.5, 0.22, 0.0, 3),
        Ends::Poles,
        10,
        1.0,
        Vec3::X,
        solid(cream),
    );
    ellipsoid(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.08, 0.9, 0.05)),
        Vec3::splat(0.16),
        SMALL,
        solid(c(color::CHERRY)),
    );
    mesh
}

/// A plush pink heart, two little eyes and its blushing cheeks.
fn super_calin() -> MeshData {
    let mut mesh = MeshData::default();
    let hug = c(color::HUG);
    for side in [-1.0f32, 1.0] {
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(side * 0.42, 0.32, 0.0)),
            Vec3::new(0.56, 0.56, 0.42),
            BLOB,
            solid(hug),
        );
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(side * 0.3, 0.12, 0.42)),
            Vec3::new(0.09, 0.12, 0.05),
            TINY,
            solid(c(color::INK)),
        );
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(side * 0.52, -0.08, 0.36)),
            Vec3::new(0.12, 0.07, 0.04),
            TINY,
            solid(c(color::BLUSH)),
        );
    }
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, 0.4, 0.0), 0.0),
            (Vec3::new(0.0, 0.32, 0.0), 0.62),
            (Vec3::new(0.0, -0.1, 0.0), 0.55),
            (Vec3::new(0.0, -0.55, 0.0), 0.3),
            (Vec3::new(0.0, -0.86, 0.0), 0.08),
            (Vec3::new(0.0, -0.95, 0.0), 0.0),
        ],
        Ends::Poles,
        14,
        0.68,
        Vec3::Z,
        solid(hug),
    );
    mesh
}

/// A rocket standing (its nose up, +Y): an orange body with a band of confetti colors, a white
/// nose, a round window, three red fins and its nozzle.
fn fusee() -> MeshData {
    let mut mesh = MeshData::default();
    let (rocket, white) = (c(color::ROCKET), c(color::WHITE));
    let profile = [
        (-0.95, 0.0),
        (-0.95, 0.3),
        (-0.7, 0.4),
        (-0.2, 0.45),
        (0.3, 0.42),
        (0.6, 0.33),
        (0.85, 0.18),
        (1.0, 0.06),
        (1.04, 0.0),
    ];
    let rings: Vec<(Vec3, f32)> = profile
        .iter()
        .map(|(y, r)| (Vec3::new(0.0, *y, 0.0), *r))
        .collect();
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &rings,
        Ends::Poles,
        14,
        1.0,
        Vec3::X,
        move |p| {
            if p.y > 0.52 {
                white
            } else if (-0.52..-0.38).contains(&p.y) {
                // A band of confetti colors around it.
                let k = ((p.x.atan2(p.z) + PI) / TAU * 6.0) as usize % 5;
                c(color::PINATA_CONES[k])
            } else {
                rocket
            }
        },
    );
    ellipsoid(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.0, 0.12, 0.4)),
        Vec3::new(0.19, 0.19, 0.08),
        SMALL,
        solid(c(color::WINDOW)),
    );
    torus(
        &mut mesh,
        at(Vec3::new(0.0, 0.12, 0.41), Quat::from_rotation_x(FRAC_PI_2)),
        0.2,
        0.045,
        Detail::new(14, 5),
        solid(white),
    );
    for k in 0..3 {
        let angle = TAU * k as f32 / 3.0 + PI / 6.0;
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        ellipsoid(
            &mut mesh,
            at(
                out * 0.42 + Vec3::Y * -0.7,
                Quat::from_rotation_y(-angle) * Quat::from_rotation_z(0.25),
            ),
            Vec3::new(0.3, 0.34, 0.06),
            SMALL,
            solid(c(color::FIN)),
        );
    }
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, -0.9, 0.0), 0.2),
            (Vec3::new(0.0, -1.05, 0.0), 0.26),
            (Vec3::new(0.0, -1.08, 0.0), 0.0),
        ],
        Ends::Open,
        12,
        1.0,
        Vec3::X,
        solid(c(color::STEEL)),
    );
    mesh
}

/// A balloon going flat: pale blue, wrinkled and lopsided, its knot below and the air puffing out
/// of it.
fn degonflage() -> MeshData {
    let mut mesh = MeshData::default();
    let (balloon, dark) = (c(color::BALLOON), c(color::BALLOON_DARK));
    let rows = 12;
    crate::models::shape::surface(
        &mut mesh,
        Mat4::IDENTITY,
        16,
        rows,
        Ends::Poles,
        |u, row| {
            let v = row as f32 / (rows - 1) as f32;
            let (theta, phi) = (PI * v, TAU * u);
            // Wrinkles around it, its top caved in a little.
            let wrinkle = 1.0 + 0.09 * (6.0 * phi).sin() * theta.sin().powi(2);
            let squash = 1.0 - 0.25 * (1.0 - v).powi(3);
            Vec3::new(
                0.78 * wrinkle * theta.sin() * phi.cos(),
                0.1 + 0.85 * squash * theta.cos() - 0.12 * theta.sin() * phi.cos(),
                0.7 * wrinkle * theta.sin() * phi.sin(),
            )
        },
        move |p| mix(balloon, dark, ((-p.y - 0.3) * 0.8).clamp(0.0, 0.5)),
    );
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, -0.7, 0.0), 0.0),
            (Vec3::new(0.0, -0.72, 0.0), 0.14),
            (Vec3::new(0.0, -0.86, 0.0), 0.05),
            (Vec3::new(0.0, -0.95, 0.0), 0.11),
            (Vec3::new(0.0, -0.98, 0.0), 0.0),
        ],
        Ends::Poles,
        8,
        1.0,
        Vec3::X,
        solid(dark),
    );
    for (k, (x, y, z, r)) in [
        (0.25, -0.98, 0.18, 0.12),
        (0.48, -1.05, 0.3, 0.16),
        (0.76, -1.08, 0.38, 0.2),
    ]
    .into_iter()
    .enumerate()
    {
        let white = mix(c(color::WHITE), balloon, 0.25 + 0.15 * k as f32);
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(x, y, z)),
            Vec3::splat(r),
            SMALL,
            solid(white),
        );
    }
    mesh
}

/// A big puffy lilac pillow, asleep: violet piping around it, a tuft at each corner, two
/// closed eyes and pink cheeks on top.
fn oreiller() -> MeshData {
    let mut mesh = MeshData::default();
    let (pillow, piping) = (c(color::PILLOW), c(color::PIPING));
    rounded_box(
        &mut mesh,
        Mat4::IDENTITY,
        Vec3::new(1.15, 0.4, 0.8),
        0.7,
        0.32,
        Detail::new(28, 19),
        move |p| if p.y.abs() < 0.05 { piping } else { pillow },
    );
    for (x, z) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        ellipsoid(
            &mut mesh,
            at(
                Vec3::new(x * 1.14, 0.0, z * 0.8),
                Quat::from_rotation_y(-x * z * 0.6),
            ),
            Vec3::new(0.22, 0.12, 0.16),
            SMALL,
            solid(piping),
        );
    }
    // Asleep: two closed eyes (arcs) and blushing cheeks, on top, facing +Z.
    for side in [-1.0f32, 1.0] {
        let eye: Vec<(Vec3, f32)> = (0..5)
            .map(|k| {
                let a = PI * k as f32 / 4.0;
                (
                    Vec3::new(side * 0.38 + 0.13 * a.cos(), 0.42, 0.1 + 0.07 * a.sin()),
                    0.035,
                )
            })
            .collect();
        tube(
            &mut mesh,
            Mat4::IDENTITY,
            &eye,
            Ends::Open,
            5,
            1.0,
            Vec3::Y,
            solid(c(color::INK)),
        );
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(Vec3::new(side * 0.6, 0.39, 0.32)),
            Vec3::new(0.14, 0.03, 0.09),
            TINY,
            solid(c(color::BLUSH)),
        );
    }
    mesh
}

/// A rubber duck whose beak is a horn's bell: yellow, an orange bell, two eyes, its tail up.
fn klaxon_canard() -> MeshData {
    let mut mesh = MeshData::default();
    let duck = c(color::DUCK);
    let horn = c(color::HORN);
    ellipsoid(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.0, -0.32, -0.12)),
        Vec3::new(0.72, 0.55, 0.88),
        BLOB,
        solid(duck),
    );
    ellipsoid(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.0, 0.42, 0.28)),
        Vec3::splat(0.5),
        BLOB,
        solid(duck),
    );
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, -0.05, -0.85), 0.0),
            (Vec3::new(0.0, -0.08, -0.82), 0.22),
            (Vec3::new(0.0, 0.08, -1.05), 0.12),
            (Vec3::new(0.0, 0.25, -1.15), 0.0),
        ],
        Ends::Poles,
        8,
        0.6,
        Vec3::X,
        solid(duck),
    );
    // The bell: out of its face, flaring, its mouth darker inside.
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, 0.32, 0.62), 0.0),
            (Vec3::new(0.0, 0.32, 0.64), 0.15),
            (Vec3::new(0.0, 0.33, 0.85), 0.11),
            (Vec3::new(0.0, 0.34, 1.02), 0.16),
            (Vec3::new(0.0, 0.35, 1.14), 0.3),
            (Vec3::new(0.0, 0.35, 1.17), 0.34),
            (Vec3::new(0.0, 0.35, 1.1), 0.0),
        ],
        Ends::Poles,
        14,
        1.0,
        Vec3::Y,
        move |p| {
            if (p.z - 1.1).abs() < 1e-3 {
                mix(horn, c(color::INK), 0.55)
            } else {
                horn
            }
        },
    );
    for side in [-1.0f32, 1.0] {
        eye(
            &mut mesh,
            Vec3::new(side * 0.25, 0.6, 0.62),
            0.13,
            Vec3::new(side * 0.3, 0.1, 1.0),
        );
        ellipsoid(
            &mut mesh,
            at(
                Vec3::new(side * 0.62, -0.18, -0.2),
                Quat::from_rotation_x(-0.35),
            ),
            Vec3::new(0.14, 0.32, 0.5),
            SMALL,
            solid(mix(duck, horn, 0.18)),
        );
    }
    mesh
}

/// A magpie, black and white, its long tail behind, a golden ring in its beak (it steals).
fn pie_voleuse() -> MeshData {
    let mut mesh = MeshData::default();
    let (black, sheen, white) = (c(color::MAGPIE), c(color::MAGPIE_SHEEN), c(color::WHITE));
    ellipsoid(
        &mut mesh,
        at(Vec3::new(0.0, -0.1, -0.05), Quat::from_rotation_x(0.25)),
        Vec3::new(0.52, 0.56, 0.82),
        BLOB,
        move |p| {
            if p.y < -0.05 && p.z > -0.5 {
                white
            } else {
                black
            }
        },
    );
    ellipsoid(
        &mut mesh,
        Mat4::from_translation(Vec3::new(0.0, 0.48, 0.56)),
        Vec3::splat(0.4),
        BLOB,
        solid(black),
    );
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &[
            (Vec3::new(0.0, 0.42, 0.9), 0.0),
            (Vec3::new(0.0, 0.42, 0.9), 0.13),
            (Vec3::new(0.0, 0.38, 1.12), 0.06),
            (Vec3::new(0.0, 0.34, 1.32), 0.0),
        ],
        Ends::Poles,
        8,
        0.75,
        Vec3::Y,
        solid(c(color::BEAK)),
    );
    torus(
        &mut mesh,
        at(
            Vec3::new(0.0, 0.18, 1.18),
            Quat::from_rotation_z(FRAC_PI_2) * Quat::from_rotation_y(0.3),
        ),
        0.2,
        0.055,
        Detail::new(14, 6),
        solid(c(color::GOLD)),
    );
    for side in [-1.0f32, 1.0] {
        eye(
            &mut mesh,
            Vec3::new(side * 0.24, 0.6, 0.82),
            0.11,
            Vec3::new(side * 0.4, 0.0, 1.0),
        );
        ellipsoid(
            &mut mesh,
            at(
                Vec3::new(side * 0.5, 0.02, -0.12),
                Quat::from_rotation_x(0.3) * Quat::from_rotation_z(side * -0.15),
            ),
            Vec3::new(0.12, 0.36, 0.7),
            SMALL,
            move |p| if p.z > 0.2 { white } else { sheen },
        );
    }
    let tail: Vec<(Vec3, f32)> = [
        (-0.7, -0.05, 0.0),
        (-0.72, -0.05, 0.2),
        (-1.0, 0.05, 0.22),
        (-1.35, 0.18, 0.2),
        (-1.65, 0.3, 0.12),
        (-1.75, 0.34, 0.0),
    ]
    .iter()
    .map(|(z, y, r)| (Vec3::new(0.0, *y, *z), *r))
    .collect();
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &tail,
        Ends::Poles,
        8,
        0.3,
        Vec3::Y,
        solid(sheen),
    );
    mesh
}

/// A star piñata: a sky blue ball, six cones in candy colors with a white band, a tassel at each
/// tip.
fn pinata() -> MeshData {
    let mut mesh = MeshData::default();
    ellipsoid(
        &mut mesh,
        Mat4::IDENTITY,
        Vec3::splat(0.55),
        BLOB,
        solid(c(color::PINATA)),
    );
    let white = c(color::WHITE);
    let directions = (0..6).map(|k| {
        let a = TAU * k as f32 / 6.0 + FRAC_PI_2;
        Vec3::new(a.cos(), a.sin(), 0.0)
    });
    for (k, direction) in directions
        .chain([Vec3::new(0.0, 0.15, 1.0).normalize()])
        .enumerate()
    {
        let main = c(color::PINATA_CONES[k % 6]);
        let length = if k == 6 { 0.8 } else { 1.0 };
        tube(
            &mut mesh,
            at(Vec3::ZERO, Quat::from_rotation_arc(Vec3::Y, direction)),
            &cone_rings(Vec3::Y * 0.3, length - 0.3, 0.3, 0.0, 4),
            Ends::Poles,
            10,
            1.0,
            Vec3::X,
            move |p| {
                if (0.55..0.68).contains(&(p.y / length)) {
                    white
                } else {
                    main
                }
            },
        );
        ellipsoid(
            &mut mesh,
            Mat4::from_translation(direction * (length + 0.06)),
            Vec3::splat(0.1),
            TINY,
            solid(c(color::PINATA_CONES[(k + 2) % 6])),
        );
    }
    mesh
}

/// A rubber toy bone (a dog's), bright orange, bent like a boomerang, flat (it spins flat): its
/// shaft curved around, two knobs at each end. Orange, not bone white: far from the snow, the
/// sand and the marshmallow ground of the circuits (GD § 14).
fn boomerang() -> MeshData {
    let mut mesh = MeshData::default();
    let (bone, shade) = (c(color::BONE), c(color::BONE_SHADE));
    // Bent like a boomerang: a wide V, rounded at its elbow.
    let arc = |t: f32| {
        let a = -1.2 + 2.4 * t;
        Vec3::new(a.sin() * 0.95, 0.0, a.cos() * 0.95 - 0.6)
    };
    let rings: Vec<(Vec3, f32)> = (0..=12)
        .map(|k| {
            let t = k as f32 / 12.0;
            (arc(t), 0.2 + 0.04 * (t * PI).sin())
        })
        .collect();
    tube(
        &mut mesh,
        Mat4::IDENTITY,
        &rings,
        Ends::Open,
        10,
        0.85,
        Vec3::Y,
        solid(bone),
    );
    for t in [0.0f32, 1.0] {
        let end = arc(t);
        let along = (arc(if t == 0.0 { 0.05 } else { 0.95 }) - end).normalize();
        let across = along.cross(Vec3::Y);
        for side in [-1.0f32, 1.0] {
            ellipsoid(
                &mut mesh,
                Mat4::from_translation(end - along * 0.08 + across * side * 0.19),
                Vec3::new(0.26, 0.21, 0.26),
                SMALL,
                move |p| mix(bone, shade, (-p.y / 0.21).clamp(0.0, 1.0) * 0.8),
            );
        }
    }
    mesh
}

/// A gift box: a soft box of `paper` crossed by its `ribbon`, its lid with the bow on top
/// ([`ItemModel::lid`]). In meters: about 1 m wide.
fn gift_box(paper: [f32; 4], ribbon: [f32; 4]) -> ItemModel {
    // A soft box of `half` its half sizes, its ribbon crossing over it: two bands a little
    // proud of it.
    let wrapped = |mesh: &mut MeshData, at: Mat4, half: Vec3, round_y: f32, rows: usize| {
        rounded_box(
            mesh,
            at,
            half,
            round_y,
            0.3,
            Detail::new(24, rows),
            solid(paper),
        );
        for across in [Vec3::new(0.24, 1.03, 1.02), Vec3::new(1.02, 1.03, 0.24)] {
            rounded_box(
                mesh,
                at,
                half * across,
                round_y,
                0.3,
                Detail::new(16, rows),
                solid(ribbon),
            );
        }
    };
    let mut body = MeshData::default();
    wrapped(
        &mut body,
        Mat4::IDENTITY,
        Vec3::new(0.48, 0.38, 0.48),
        0.35,
        13,
    );
    // The lid from its bottom up (where it sits on the box).
    let mut lid = MeshData::default();
    wrapped(
        &mut lid,
        Mat4::from_translation(Vec3::Y * 0.09),
        Vec3::new(0.53, 0.09, 0.53),
        0.4,
        9,
    );
    // The bow: two loops and a knot.
    for side in [-1.0f32, 1.0] {
        torus(
            &mut lid,
            at(
                Vec3::new(side * 0.17, 0.29, 0.0),
                Quat::from_rotation_z(side * -0.75) * Quat::from_rotation_x(FRAC_PI_2),
            ),
            0.15,
            0.055,
            Detail::new(14, 6),
            solid(ribbon),
        );
    }
    ellipsoid(
        &mut lid,
        Mat4::from_translation(Vec3::new(0.0, 0.23, 0.0)),
        Vec3::new(0.09, 0.08, 0.09),
        SMALL,
        solid(ribbon),
    );
    ItemModel {
        body,
        lid: Some((lid, Vec3::new(0.0, 0.27, 0.0))),
        ..default()
    }
}

// --- The models from files -----------------------------------------------------------------------

/// The model of `look` from its baked file (Kenney's, [`MODEL_FILES`]): its parts merged, its
/// colors changed where the game wants its own (the boxes' paper and ribbon), fit at the size and
/// place of the model built in code (`built`). A part named `lid` (the present's) is the model's
/// lid; its other moving parts stay the built model's. None if the file has no triangle.
pub fn from_file(look: Look, baked: &BakedModel, built: &ItemModel) -> Option<ItemModel> {
    if baked.triangle_count() == 0 {
        return None;
    }
    let (mut body, mut lid) = (MeshData::default(), MeshData::default());
    for part in &baked.parts {
        let out = if part.name == "lid" {
            &mut lid
        } else {
            &mut body
        };
        append_transformed(out, &part.mesh, part.transform);
    }
    for color in body.colors.iter_mut().chain(lid.colors.iter_mut()) {
        *color = file_color(look, *color);
    }
    // Fit: the file's bounds (its lid included) on the built model's.
    let mut file_all = body.clone();
    file_all.append(&lid);
    let mut built_all = built.body.clone();
    if let Some((built_lid, offset)) = &built.lid {
        append_transformed(&mut built_all, built_lid, Mat4::from_translation(*offset));
    }
    let ((lo, hi), (want_lo, want_hi)) = (bounds(&file_all)?, bounds(&built_all)?);
    let scale = (want_hi - want_lo).max_element() / (hi - lo).max_element().max(1e-6);
    let fit = Mat4::from_translation((want_lo + want_hi) / 2.0)
        * Mat4::from_scale(Vec3::splat(scale))
        * Mat4::from_translation(-(lo + hi) / 2.0);
    let mut model = ItemModel {
        body: moved(&body, fit),
        ..built.clone()
    };
    if !lid.indices.is_empty() {
        // The lid in its own space, from the bottom of its middle: where it sits on the box.
        let lid = moved(&lid, fit);
        let (llo, lhi) = bounds(&lid)?;
        let seat = Vec3::new((llo.x + lhi.x) / 2.0, llo.y, (llo.z + lhi.z) / 2.0);
        model.lid = Some((moved(&lid, Mat4::from_translation(-seat)), seat));
    }
    Some(model)
}

/// A vertex color of a file (linear) in the game's colors for `look`: a box's white paper in its
/// paper's color, its ribbon in its ribbon's, their shades kept.
fn file_color(look: Look, color: [f32; 4]) -> [f32; 4] {
    let (paper, ribbon) = match look {
        Look::GiftBox => (color::BOX, color::RIBBON),
        Look::DoubleGiftBox => (color::DOUBLE_BOX, color::DOUBLE_RIBBON),
        _ => return color,
    };
    let s = Color::linear_rgba(color[0], color[1], color[2], 1.0).to_srgba();
    let (lightest, darkest) = (
        s.red.max(s.green).max(s.blue),
        s.red.min(s.green).min(s.blue),
    );
    let tinted = |(r, g, b): (f32, f32, f32), shade: f32| srgb(r * shade, g * shade, b * shade);
    if darkest > 0.6 {
        tinted(paper, lightest.min(1.0))
    } else {
        tinted(ribbon, (0.4 + 0.65 * lightest).min(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every model is there, in soft shapes, not too heavy (a few thousand triangles at most,
    /// dozens of them on the track with 12 karts), facing out, in its item's space.
    #[test]
    fn every_model_is_built_low_poly() {
        let mut total = 0;
        for look in Look::all() {
            let model = model(look);
            let triangles = model.triangle_count();
            println!("{look:?}: {triangles} triangles");
            total += triangles;
            assert!(triangles > 50 && triangles < 3_000, "{look:?}: {triangles}");
            let mesh = model.merged();
            assert_eq!(mesh.positions.len(), mesh.normals.len());
            assert_eq!(mesh.positions.len(), mesh.colors.len());
            let (lo, hi) = bounds(&mesh).unwrap();
            let reach = match look {
                // In meters.
                Look::Croquette => 0.4,
                Look::GiftBox | Look::DoubleGiftBox => 0.8,
                // In its radius: a model about as big as what touches it.
                Look::Item(_) => 1.9,
            };
            assert!(
                lo.abs().max_element() < reach && hi.abs().max_element() < reach,
                "{look:?}: {lo} {hi}"
            );
            assert!(
                (hi - lo).max_element() > reach * 0.5,
                "{look:?} is too small: {lo} {hi}"
            );
        }
        println!("all: {total} triangles");
        assert!(total < 25_000, "{total}");
    }

    /// The pelure lies on the ground under it (its position is half its radius above it), what
    /// rolls touches it (a radius below).
    #[test]
    fn the_models_stand_on_their_ground() {
        let bottom = |item| bounds(&model(Look::Item(item)).merged()).unwrap().0.y;
        assert!((bottom(ItemKind::Pelure) + 0.5).abs() < 0.03);
        for item in [ItemKind::Pelote, ItemKind::Bombe] {
            assert!(
                (bottom(item) + 1.0).abs() < 0.12,
                "{item:?}: {}",
                bottom(item)
            );
        }
    }

    /// The files' models are only those of `MODEL_FILES`, and a missing one stands in its built
    /// form.
    #[test]
    fn the_models_from_files_are_named_once_each() {
        assert_eq!(Look::Item(ItemKind::Pelure).file(), None);
        assert_eq!(Look::GiftBox.file(), Some("holiday-kit/present-a-round"));
        let mut seen = std::collections::HashSet::new();
        for (look, _) in MODEL_FILES {
            assert!(seen.insert(look), "{look:?} twice");
        }
        assert!(from_file(Look::GiftBox, &BakedModel::default(), &model(Look::GiftBox)).is_none());
    }

    /// A file's model takes the place and size of the built one; its colors are the game's.
    #[test]
    fn a_file_takes_the_built_models_place_and_colors() {
        use crate::models::bake::{BakedPart, Primitive, bake_primitive};
        // A white box with a red lid on top, as a present's file would be.
        let part = |name: &str, color: [f32; 4], y: f32| {
            let mut mesh = MeshData::default();
            rounded_box(
                &mut mesh,
                Mat4::IDENTITY,
                Vec3::new(0.27, 0.15, 0.27),
                0.5,
                0.5,
                Detail::new(8, 6),
                solid(color),
            );
            let mut baked = BakedPart {
                name: name.to_string(),
                transform: Mat4::from_translation(Vec3::Y * y),
                ..default()
            };
            bake_primitive(
                &mut baked,
                &Primitive {
                    positions: &mesh.positions,
                    normals: Some(&mesh.normals),
                    uvs: None,
                    indices: Some(&mesh.indices),
                },
                |_| color,
            );
            baked
        };
        let file = BakedModel {
            parts: vec![
                part("present-a-round", srgb(1.0, 1.0, 1.0), 0.15),
                part("lid", srgb(0.9, 0.1, 0.1), 0.3),
            ],
            nodes: Vec::new(),
        };
        let built = model(Look::DoubleGiftBox);
        let made = from_file(Look::DoubleGiftBox, &file, &built).unwrap();
        let (lid, seat) = made.lid.clone().unwrap();
        assert!(!lid.indices.is_empty());
        let (want_lo, want_hi) = bounds(&built.merged()).unwrap();
        let (lo, hi) = bounds(&made.merged()).unwrap();
        assert!(((hi - lo).max_element() - (want_hi - want_lo).max_element()).abs() < 1e-3);
        assert!(((lo + hi) - (want_lo + want_hi)).length() < 1e-3);
        assert!(seat.y > 0.0);
        // The white paper turned gold, the red lid the double box's red.
        let paper = Color::linear_rgba(
            made.body.colors[0][0],
            made.body.colors[0][1],
            made.body.colors[0][2],
            1.0,
        )
        .to_srgba();
        assert!(
            paper.red > 0.9 && paper.green > 0.7 && paper.blue < 0.4,
            "{paper:?}"
        );
        let ribbon = Color::linear_rgba(lid.colors[0][0], lid.colors[0][1], lid.colors[0][2], 1.0)
            .to_srgba();
        assert!(ribbon.red > 0.5 && ribbon.green < 0.3, "{ribbon:?}");
    }
}
