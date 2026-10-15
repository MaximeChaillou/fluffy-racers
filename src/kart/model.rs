//! The kart's model (issue #11): the kart of Kenney's Car Kit (CC0), in place of the primitive
//! box and cylinders. Purely visual: the kart's collider, its suspension and everything the
//! simulation does are unchanged; the model follows the body like the primitives did.
//!
//! The model is scaled to the kart's box (2 m long, like the collider), its front turned to -Z,
//! its wheels on the road when the suspension is at rest. Its body is split in two: the shell
//! (the colormap's cell the shell is painted with) wears the kart's color, the shared material of
//! its palette color (issue #55, `KartVisualAssets::paint`), shaded by the model's own gradient
//! (grey vertex colors); the rest keeps the model's colors. The file's driver (a character in a
//! helmet) is left out: our animal sits in the seat.
//!
//! Pure here (`kart_model_data`, tested on a made-up model and on the real file in
//! `crate::models`); `visual.rs` dresses the karts' visuals with it once it is baked.

use bevy::prelude::*;

use crate::models::bake::{split_by_uv, transformed};
use crate::models::{BakedModel, BakedPart};
use crate::track::GRID_HEIGHT;
use crate::track::mesh::MeshData;

/// The model's file (`assets/models/car-kit/kart-oobi.glb`).
pub const KART_MODEL: &str = "car-kit/kart-oobi";
/// How much bigger than the file the kart is drawn: 2 m long, like its collider.
pub const KART_MODEL_SCALE: f32 = 1.4;
/// The file's nodes: the body, the wheels (in the order of `WHEEL_ANCHORS`: front left, front
/// right, back left, back right), the driver (left out; its feet show where the seat is).
const BODY: &str = "kart-oobi";
const WHEELS: [&str; 4] = [
    "wheel-front-left",
    "wheel-front-right",
    "wheel-back-left",
    "wheel-back-right",
];
const DRIVER: &str = "character";
/// The shell's paint: the colormap's cell in column 5, row 2 (the colormap has 8 × 4 cells),
/// as texture coordinates.
const PAINT_CELL: Rect = Rect {
    min: Vec2::new(5.0 / 8.0, 2.0 / 4.0),
    max: Vec2::new(6.0 / 8.0, 3.0 / 4.0),
};

/// The kart model, ready to draw, in the space of the visual's pivot (`KartBodyPivot`: the
/// kart's body center, its front toward -Z, the road `GRID_HEIGHT` below at rest).
#[derive(Clone, Debug, PartialEq)]
pub struct KartModelData {
    /// The shell, grey (1 at its lightest): times the kart's color.
    pub paint: MeshData,
    /// The rest of the body, in the model's colors.
    pub trim: MeshData,
    /// A wheel around its hub (the front left one): every wheel draws it, the right ones turned
    /// half a turn around the vertical (a wheel looks the same from both sides of its axle): one
    /// mesh for the four.
    pub wheel: MeshData,
    /// Where each wheel's hub is with the suspension at rest (the order of `WHEEL_ANCHORS`).
    pub hubs: [Vec3; 4],
    /// How each wheel's mesh is turned (half a turn for the right ones).
    pub wheel_turns: [Quat; 4],
    pub wheel_radius: f32,
    /// Where the driver sits (its bottom).
    pub seat: Vec3,
}

/// The model's file turned and scaled into the pivot's space, then moved so that its body is
/// centered along the kart (`shift` on z).
fn to_pivot(shift: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(0.0, -GRID_HEIGHT, shift))
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_scale(Vec3::splat(KART_MODEL_SCALE))
}

/// Linear luminance of a vertex color.
fn luminance(c: &[f32; 4]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The kart model from the baked file (None: a part is missing, the primitives stay).
pub fn kart_model_data(model: &BakedModel) -> Option<KartModelData> {
    let body = model.part(BODY)?;
    let mut found = Vec::with_capacity(WHEELS.len());
    for name in WHEELS {
        found.push(model.part(name)?);
    }
    let wheels: [&BakedPart; 4] = found.try_into().ok()?;

    // Center the body along the kart.
    let turned = transformed(&body.mesh, to_pivot(0.0) * body.transform);
    let (min_z, max_z) = turned
        .positions
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p[2]), hi.max(p[2]))
        });
    if min_z > max_z {
        return None;
    }
    let shift = -(min_z + max_z) / 2.0;
    let pivot = to_pivot(shift);

    let (paint, trim) = split_by_uv(body, |uv| PAINT_CELL.contains(uv));
    let mut paint = transformed(&paint, pivot * body.transform);
    let trim = transformed(&trim, pivot * body.transform);
    let lightest = paint.colors.iter().map(luminance).fold(0.0, f32::max);
    for color in &mut paint.colors {
        let grey = if lightest > 0.0 {
            luminance(color) / lightest
        } else {
            1.0
        };
        *color = [grey, grey, grey, 1.0];
    }

    // Each wheel turned and scaled around its own hub; its hub where the file puts it.
    let around_hub = Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_scale(Vec3::splat(KART_MODEL_SCALE));
    let hubs = wheels.map(|w| pivot.transform_point3(w.transform.transform_point3(Vec3::ZERO)));
    // The node's own rotation and scale (not its translation: that is the hub).
    let (scale, rotation, _) = wheels[0].transform.to_scale_rotation_translation();
    let wheel = transformed(
        &wheels[0].mesh,
        around_hub * Mat4::from_scale_rotation_translation(scale, rotation, Vec3::ZERO),
    );
    let wheel_turns = hubs.map(|hub| {
        if hub.x.signum() == hubs[0].x.signum() {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_y(std::f32::consts::PI)
        }
    });
    let wheel_radius = wheel
        .positions
        .iter()
        .map(|p| p[1].abs())
        .fold(0.0, f32::max);

    // The driver's feet in the file: the seat.
    let seat = match model.part(DRIVER) {
        Some(driver) => {
            let feet = driver
                .mesh
                .positions
                .iter()
                .map(|p| {
                    (pivot * driver.transform)
                        .transform_point3(Vec3::from(*p))
                        .y
                })
                .fold(f32::MAX, f32::min);
            let at = (pivot * driver.transform).transform_point3(Vec3::ZERO);
            Vec3::new(0.0, feet, at.z)
        }
        None => Vec3::new(0.0, 0.0, 0.0),
    };

    Some(KartModelData {
        paint,
        trim,
        wheel,
        hubs,
        wheel_turns,
        wheel_radius,
        seat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::bake::{Primitive, bake_primitive};

    /// A box part, its texture coordinates all at `uv`.
    fn cube(name: &str, at: Vec3, half: Vec3, uv: [f32; 2], color: [f32; 4]) -> BakedPart {
        let mut part = BakedPart {
            name: name.to_string(),
            transform: Mat4::from_translation(at),
            ..default()
        };
        let mut positions = Vec::new();
        for x in [-1.0, 1.0] {
            for y in [-1.0, 1.0] {
                for z in [-1.0, 1.0] {
                    positions.push((Vec3::new(x, y, z) * half).to_array());
                }
            }
        }
        let indices = [0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5];
        bake_primitive(
            &mut part,
            &Primitive {
                positions: &positions,
                normals: None,
                uvs: Some(&[uv; 8]),
                indices: Some(&indices),
            },
            |_| color,
        );
        part
    }

    /// A kart like the file's: a body 1.4 long (its front toward +Z), half of it in the paint's
    /// cell, four wheels of radius 0.2, and a driver sitting 0.3 above the ground.
    fn made_up_kart() -> BakedModel {
        let paint_uv = [5.5 / 8.0, 2.5 / 4.0];
        let mut body = cube(
            "kart-oobi",
            Vec3::new(0.0, 0.4, 0.1),
            Vec3::new(0.45, 0.25, 0.7),
            paint_uv,
            [0.5, 0.5, 0.6, 1.0],
        );
        let trim = cube(
            "x",
            Vec3::ZERO,
            Vec3::splat(0.1),
            [0.1, 0.1],
            [0.2, 0.2, 0.2, 1.0],
        );
        crate::models::bake::append_transformed(&mut body.mesh, &trim.mesh, Mat4::IDENTITY);
        body.uvs.extend_from_slice(&trim.uvs);
        let wheel = |name: &str, x: f32, z: f32| {
            cube(
                name,
                Vec3::new(x, 0.2, z),
                Vec3::new(0.1, 0.2, 0.2),
                [0.3, 0.6],
                [0.1; 4],
            )
        };
        BakedModel {
            parts: vec![
                body,
                wheel("wheel-front-left", 0.3, 0.35),
                wheel("wheel-front-right", -0.3, 0.35),
                wheel("wheel-back-left", 0.3, -0.35),
                wheel("wheel-back-right", -0.3, -0.35),
                cube(
                    "character",
                    Vec3::new(0.0, 0.55, 0.0),
                    Vec3::splat(0.25),
                    [0.0; 2],
                    [1.0; 4],
                ),
            ],
            ..default()
        }
    }

    #[test]
    fn the_model_is_turned_scaled_and_centered_on_the_kart() {
        let kart = kart_model_data(&made_up_kart()).unwrap();
        let s = KART_MODEL_SCALE;
        // The body: 1.4 × 1.4 = 1.96 m long, centered along the kart, its bottom 0.15 × 1.4
        // above the road.
        let (lo, hi) = kart
            .paint
            .positions
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            });
        assert!((hi.z - lo.z - 1.4 * s).abs() < 1e-4 && (hi.z + lo.z).abs() < 1e-4);
        assert!((lo.y - (0.15 * s - GRID_HEIGHT)).abs() < 1e-4, "{lo}");
        // The file's front wheels (+Z) are at the front (-Z), its left wheels (+X) on the left
        // (-X), as `WHEEL_ANCHORS` has them.
        let [fl, fr, bl, br] = kart.hubs;
        assert!(
            fl.z < 0.0 && fr.z < 0.0 && bl.z > 0.0 && br.z > 0.0,
            "{:?}",
            kart.hubs
        );
        assert!(fl.x < 0.0 && bl.x < 0.0 && fr.x > 0.0 && br.x > 0.0);
        assert!((fl.x + 0.3 * s).abs() < 1e-4);
        // Wheels on the road at rest: their hub one radius above it.
        assert!((kart.wheel_radius - 0.2 * s).abs() < 1e-4);
        for hub in kart.hubs {
            assert!(
                (hub.y - (kart.wheel_radius - GRID_HEIGHT)).abs() < 1e-4,
                "{hub}"
            );
        }
        // One wheel mesh around its hub, the right wheels turned half a turn.
        let center = kart
            .wheel
            .positions
            .iter()
            .map(|p| Vec3::from(*p))
            .sum::<Vec3>()
            / kart.wheel.positions.len() as f32;
        assert!(center.length() < 1e-4, "{center}");
        for (hub, turn) in kart.hubs.iter().zip(kart.wheel_turns) {
            let turned = turn != Quat::IDENTITY;
            assert_eq!(turned, hub.x > 0.0, "{hub}");
        }
        // The seat: the driver's feet, 0.3 × 1.4 above the road.
        assert!(
            (kart.seat.y - (0.3 * s - GRID_HEIGHT)).abs() < 1e-4,
            "{}",
            kart.seat
        );
    }

    #[test]
    fn the_shell_is_painted_and_the_rest_keeps_its_colors() {
        let kart = kart_model_data(&made_up_kart()).unwrap();
        // The paint's cell: the big box's 4 triangles; the small one is trim.
        assert_eq!(kart.paint.triangle_count(), 4);
        assert_eq!(kart.trim.triangle_count(), 4);
        // Grey, 1 at its lightest: the kart's color shows as it is.
        assert!(
            kart.paint
                .colors
                .iter()
                .all(|c| c[0] == c[1] && c[1] == c[2])
        );
        let lightest = kart.paint.colors.iter().map(|c| c[0]).fold(0.0, f32::max);
        assert!((lightest - 1.0).abs() < 1e-5);
        assert!(kart.trim.colors.iter().all(|c| *c == [0.2, 0.2, 0.2, 1.0]));
    }

    #[test]
    fn a_model_without_its_wheels_is_not_a_kart() {
        let mut model = made_up_kart();
        model.parts.retain(|p| p.name != "wheel-back-left");
        assert!(kart_model_data(&model).is_none());
        assert!(kart_model_data(&BakedModel::default()).is_none());
    }
}
