//! Dressing a circuit with its decor (issue #11): once the circuit is built, its biome's and its
//! file's models are asked for (only those: another circuit's are dropped), and once they are
//! all baked or missing, the pieces (`decor.rs`) are merged into two meshes, those that cast
//! shadows and the small ones that do not: two draw calls (and the first one's shadows) for the
//! whole decor, with the models' shared material. A missing model stands in a primitive tree.
//! Issue #146: its far decor (`far.rs`: the ground beyond, the silhouettes, the clouds, the big
//! pieces at mid-distance) in a third mesh, without shadows, and its fog on the chase camera.
//!
//! Only with a renderer (Bevy's glTF plugin): headless, a circuit has no decor, as nothing would
//! see it. The pieces never touch the circuit's colliders: the decor is purely visual.

use bevy::gltf::Gltf;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::biome::Palette;
use super::circuit::{TrackHandle, to_mesh};
use super::data::TrackData;
use super::decor::{decor_models, place_decor_around};
use super::far::place_far_decor;
use super::layout::{CenterLine, TrackLayout};
use super::mesh::MeshData;
use super::shortcut::Shortcut;
use super::water::Lake;
use crate::camera::ChaseCamera;
use crate::level::LevelScoped;
use crate::models::bake::{BakedModel, BakedPart, Primitive, bake_primitive};
use crate::models::{Keep, ModelMaterials, Models};

/// On the decor of the circuit (the root of its meshes): it is dressed.
#[derive(Component, Clone, Copy, Debug)]
pub struct CircuitDecor {
    /// Pieces placed, and how many of them stand in primitives (a missing model).
    pub pieces: usize,
    pub stand_ins: usize,
    pub triangles: usize,
}

/// Asks for the models of the circuit being driven (once per circuit) and, once they are all
/// there or missing, dresses it: its decor, and (issue #146) its far decor and its fog on the
/// chase camera.
#[allow(clippy::too_many_arguments)]
pub fn dress_circuit(
    mut commands: Commands,
    handle: Option<Res<TrackHandle>>,
    tracks: Res<Assets<TrackData>>,
    layout: Res<TrackLayout>,
    dressed: Query<(), With<CircuitDecor>>,
    mut models: ResMut<Models>,
    server: Res<AssetServer>,
    shared: Option<Res<ModelMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<Entity, With<ChaseCamera>>,
    mut wanted: Local<Vec<String>>,
    mut pending_fog: Local<Option<DistanceFog>>,
) {
    if !dressed.is_empty() {
        // The fog of a circuit dressed before its chase camera was there (its models were
        // already loaded): on the camera once it is.
        if pending_fog.is_some() && !cameras.is_empty() {
            let fog = pending_fog.take().unwrap_or_default();
            for camera in &cameras {
                commands.entity(camera).insert(fog.clone());
            }
        }
        return;
    }
    *pending_fog = None;
    let (Some(handle), Some(shared)) = (handle, shared) else {
        return;
    };
    let Some(data) = tracks.get(&handle.0) else {
        return;
    };
    let ids = circuit_models(data);
    if *wanted != ids {
        models.keep_for_the_level(&ids);
        for id in &ids {
            models.request(id, Keep::Level, Some(&server));
        }
        *wanted = ids;
    }
    if !wanted.iter().all(|id| models.settled(id)) {
        return;
    }

    // The mirror mode (issue #19): the mirror image of the file's decor, piece by piece.
    // Issue #20: never in a lake (the file's).
    // Issue #107: nor on its shortcuts (the file's, like its centerline).
    let lakes: Vec<Lake> = data.lakes.iter().map(Lake::new).collect();
    let center = CenterLine::of(data);
    let shortcuts: Vec<Shortcut> = data
        .shortcuts
        .iter()
        .map(|s| Shortcut::new(&center, s))
        .collect();
    let plan = place_decor_around(
        &center,
        data.biome(),
        &data.props,
        data.decor_seed,
        &lakes,
        &shortcuts,
    );
    // Issue #146: the far decor, around the near one (the file's too, mirrored after).
    let far = data.far_decor().map(|far| {
        let palette = data.biome().map(|b| b.palette).unwrap_or_default();
        let plan = place_far_decor(
            &center,
            &palette,
            &far,
            data.decor_seed,
            &plan,
            &lakes,
            &shortcuts,
        );
        if layout.mirrored {
            plan.mirrored()
        } else {
            plan
        }
    });
    let plan = if layout.mirrored {
        plan.mirrored()
    } else {
        plan
    };
    for piece in &plan.left_out {
        warn!("decor of {}: no room for {piece}", layout.name);
    }
    let stand_in = std::sync::Arc::new(stand_in_tree());
    let mut stand_ins = 0;
    let mut model_of = |id: &str| match models.get(id) {
        Some(model) => model.clone(),
        None => {
            stand_ins += 1;
            stand_in.clone()
        }
    };
    let (mut casting, mut small) = (MeshData::default(), MeshData::default());
    for piece in &plan.pieces {
        let model = model_of(&plan.models[piece.model as usize]);
        let out = if piece.shadows {
            &mut casting
        } else {
            &mut small
        };
        model.place(out, piece.position, piece.yaw, piece.height, piece.radius);
    }
    let far_mesh = far.as_ref().map(|far| {
        let mut mesh = far.mesh();
        for piece in &far.pieces {
            let model = model_of(&far.models[piece.model as usize]);
            model.place(
                &mut mesh,
                piece.position,
                piece.yaw,
                piece.height,
                piece.radius,
            );
        }
        mesh
    });
    let triangles = casting.triangle_count() + small.triangle_count();
    info!(
        "decor of {}: {} pieces ({} models, {stand_ins} in primitives), {triangles} triangles \
         in 2 meshes",
        layout.name,
        plan.pieces.len(),
        plan.models.len()
    );
    commands
        .spawn((
            Name::new(format!("Decor: {}", layout.name)),
            CircuitDecor {
                pieces: plan.pieces.len(),
                stand_ins,
                triangles,
            },
            LevelScoped,
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|decor| {
            if !casting.indices.is_empty() {
                decor.spawn((
                    Name::new("Decor (shadows)"),
                    Mesh3d(meshes.add(to_mesh(&casting))),
                    MeshMaterial3d(shared.vertex_colors.clone()),
                ));
            }
            if !small.indices.is_empty() {
                decor.spawn((
                    Name::new("Decor (small, no shadows)"),
                    Mesh3d(meshes.add(to_mesh(&small))),
                    MeshMaterial3d(shared.vertex_colors.clone()),
                    NotShadowCaster,
                ));
            }
        });

    // Issue #146: the far decor in one mesh, one material (fully rough, as the ground's: the
    // ground beyond meets the slab without a seam), no shadows, no collider; the fog on the
    // chase camera, in the sky's color.
    let Some((far, mesh)) = far.zip(far_mesh) else {
        for camera in &cameras {
            commands.entity(camera).remove::<DistanceFog>();
        }
        return;
    };
    info!(
        "far decor of {}: {} pieces ({} left out), {} triangles in 1 mesh",
        layout.name,
        far.pieces.len(),
        far.left_out,
        mesh.triangle_count()
    );
    commands.spawn((
        Name::new(format!("Far decor: {}", layout.name)),
        CircuitFarDecor {
            pieces: far.pieces.len(),
            triangles: mesh.triangle_count(),
        },
        LevelScoped,
        Mesh3d(meshes.add(to_mesh(&mesh))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::default(),
        NotShadowCaster,
    ));
    let (r, g, b) = data
        .biome()
        .map_or(Palette::default().sky, |biome| biome.palette.sky);
    let fog = far_fog(Color::srgb(r, g, b), far.fog);
    if cameras.is_empty() {
        *pending_fog = Some(fog.clone());
    }
    for camera in &cameras {
        commands.entity(camera).insert(fog.clone());
    }
}

/// The fog of a far decor (issue #146): linear, from `start` to `end` meters, into the sky's
/// color (no glow toward the sun).
pub fn far_fog(sky: Color, (start, end): (f32, f32)) -> DistanceFog {
    DistanceFog {
        color: sky,
        directional_light_color: Color::NONE,
        falloff: FogFalloff::Linear { start, end },
        ..default()
    }
}

/// Every model a circuit's decor may place, each once: its decor's (`decor_models`) and its far
/// decor's (issue #146). What to load for it.
pub fn circuit_models(data: &TrackData) -> Vec<String> {
    let mut ids = decor_models(data.biome(), &data.props);
    if let Some(far) = data.far_decor() {
        for id in far.models() {
            if !ids.iter().any(|m| m == id) {
                ids.push(id.to_string());
            }
        }
    }
    ids
}

/// On the far decor of the circuit (issue #146): it is dressed.
#[derive(Component, Clone, Copy, Debug)]
pub struct CircuitFarDecor {
    pub pieces: usize,
    pub triangles: usize,
}

/// Whether the game can load models (Bevy's glTF plugin, with a renderer).
pub fn can_load_models(gltfs: Option<Res<Assets<Gltf>>>) -> bool {
    gltfs.is_some()
}

/// The stand-in of a missing model: a primitive tree (a trunk and a ball of leaves, as the
/// circuits had before the models), 1 m high, to be scaled like any model.
pub fn stand_in_tree() -> BakedModel {
    let mut part = BakedPart {
        name: "stand-in".to_string(),
        ..default()
    };
    let shapes: [(Mesh, Vec3, [f32; 4]); 2] = [
        (
            Cylinder::new(0.06, 0.4).mesh().resolution(6).build(),
            Vec3::new(0.0, 0.2, 0.0),
            Color::srgb(0.50, 0.33, 0.20).to_linear().to_f32_array(),
        ),
        (
            Sphere::new(0.32)
                .mesh()
                .ico(1)
                .unwrap_or_else(|_| Sphere::new(0.32).mesh().uv(8, 6)),
            Vec3::new(0.0, 0.66, 0.0),
            Color::srgb(0.25, 0.60, 0.30).to_linear().to_f32_array(),
        ),
    ];
    for (mesh, at, color) in shapes {
        let positions: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) => {
                p.iter().map(|p| (Vec3::from(*p) + at).to_array()).collect()
            }
            _ => continue,
        };
        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) => Some(n.clone()),
            _ => None,
        };
        let indices: Option<Vec<u32>> =
            mesh.indices().map(|i| i.iter().map(|i| i as u32).collect());
        bake_primitive(
            &mut part,
            &Primitive {
                positions: &positions,
                normals: normals.as_deref(),
                uvs: None,
                indices: indices.as_deref(),
            },
            |_| color,
        );
    }
    BakedModel {
        parts: vec![part],
        ..default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_is_a_one_meter_tree() {
        let tree = stand_in_tree();
        let (lo, hi) = tree.bounds().unwrap();
        assert!(lo.y.abs() < 1e-4 && (hi.y - 0.98).abs() < 0.02, "{lo} {hi}");
        assert!(tree.triangle_count() > 20 && tree.triangle_count() < 200);
        assert!(tree.reach() < 0.35);
    }
}
