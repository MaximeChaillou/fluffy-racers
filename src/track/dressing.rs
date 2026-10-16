//! Dressing a circuit with its decor (issue #11): once the circuit is built, its biome's and its
//! file's models are asked for (only those: another circuit's are dropped), and once they are
//! all baked or missing, the pieces (`decor.rs`) are merged into two meshes, those that cast
//! shadows and the small ones that do not: two draw calls (and the first one's shadows) for the
//! whole decor, with the models' shared material. A missing model stands in a primitive tree.
//!
//! Only with a renderer (Bevy's glTF plugin): headless, a circuit has no decor, as nothing would
//! see it. The pieces never touch the circuit's colliders: the decor is purely visual.

use bevy::gltf::Gltf;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::circuit::{TrackHandle, to_mesh};
use super::data::TrackData;
use super::decor::{decor_models, place_decor};
use super::layout::TrackLayout;
use super::mesh::MeshData;
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
/// there or missing, dresses it.
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
    mut wanted: Local<Vec<String>>,
) {
    if !dressed.is_empty() {
        return;
    }
    let (Some(handle), Some(shared)) = (handle, shared) else {
        return;
    };
    let Some(data) = tracks.get(&handle.0) else {
        return;
    };
    let ids = decor_models(data.biome(), &data.props);
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

    let plan = place_decor(&layout.center, data.biome(), &data.props, data.decor_seed);
    for piece in &plan.left_out {
        warn!("decor of {}: no room for {piece}", layout.name);
    }
    let stand_in = stand_in_tree();
    let (mut casting, mut small) = (MeshData::default(), MeshData::default());
    let mut stand_ins = 0;
    for piece in &plan.pieces {
        let model = match models.get(&plan.models[piece.model as usize]) {
            Some(model) => model.as_ref(),
            None => {
                stand_ins += 1;
                &stand_in
            }
        };
        let out = if piece.shadows {
            &mut casting
        } else {
            &mut small
        };
        model.place(out, piece.position, piece.yaw, piece.height, piece.radius);
    }
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
