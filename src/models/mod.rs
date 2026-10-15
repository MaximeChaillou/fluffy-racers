//! 3D models (issue #11): low-poly CC0 packs by Kenney, in `assets/models/<pack>/` (one GLB per
//! model, the pack's licence next to them, a line per pack in `assets/CREDITS.md`).
//!
//! A model is asked for by its name, "pack/name" ([`Models::request`]); it is loaded as glTF by
//! Bevy's asset server, then baked ([`bake`]): every part colored by its material or its texture
//! (vertex colors). Every model of every pack is then drawn with one shared material
//! ([`ModelMaterials::vertex_colors`]), and the pieces of a circuit are merged into two meshes
//! (`track::dressing`): a handful of draw calls for hundreds of pieces. The files' own meshes and
//! textures stay on the CPU (never sent to the GPU) and are dropped once baked.
//!
//! Only with a renderer: without Bevy's glTF plugin (the headless tests, `MinimalPlugins`),
//! every model is missing at once, and what wanted it stands in primitives, as before the models
//! (a missing or broken file too: a warning, no panic).

pub mod bake;

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{LoadState, RenderAssetUsages};
use bevy::gltf::{Gltf, GltfLoaderSettings, GltfMaterial, GltfMesh, GltfNode};
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;

pub use bake::{BakedModel, BakedPart, Primitive};

use crate::track::biome::model_path;

/// How long a model is kept once baked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// For the whole game (the karts).
    Always,
    /// For the current circuit: dropped when another circuit no longer wants it.
    Level,
}

#[derive(Clone, Debug)]
enum Slot {
    Loading(Handle<Gltf>),
    Ready(Arc<BakedModel>),
    /// Not found, broken, or no glTF loader (headless).
    Missing,
}

/// The models asked for, by name ("pack/name"): loading, baked, or missing.
#[derive(Resource, Default, Debug)]
pub struct Models {
    slots: HashMap<String, (Slot, Keep)>,
    loading: usize,
}

impl Models {
    /// Asks for model `id` (nothing if it was already asked for; a model kept for the level
    /// becomes kept always if asked so). `server`: the asset server when the game can load glTF
    /// files, None headless (the model is missing at once).
    pub fn request(&mut self, id: &str, keep: Keep, server: Option<&AssetServer>) {
        if let Some((_, kept)) = self.slots.get_mut(id) {
            if keep == Keep::Always {
                *kept = Keep::Always;
            }
            return;
        }
        let slot = match server {
            Some(server) => {
                self.loading += 1;
                Slot::Loading(
                    server
                        .load_builder()
                        .with_settings(|settings: &mut GltfLoaderSettings| {
                            // Read on the CPU to be baked, never drawn as they are.
                            settings.load_meshes = RenderAssetUsages::MAIN_WORLD;
                            settings.load_materials = RenderAssetUsages::MAIN_WORLD;
                            settings.load_cameras = false;
                            settings.load_lights = false;
                            settings.load_animations = false;
                        })
                        .load(model_path(id)),
                )
            }
            None => Slot::Missing,
        };
        self.slots.insert(id.to_string(), (slot, keep));
    }

    /// The baked model, once it is there.
    pub fn get(&self, id: &str) -> Option<&Arc<BakedModel>> {
        match self.slots.get(id) {
            Some((Slot::Ready(model), _)) => Some(model),
            _ => None,
        }
    }

    /// Whether `id` was asked for and is no longer loading: baked or missing.
    pub fn settled(&self, id: &str) -> bool {
        matches!(
            self.slots.get(id),
            Some((Slot::Ready(_) | Slot::Missing, _))
        )
    }

    /// Whether `id` is known to be missing.
    pub fn missing(&self, id: &str) -> bool {
        matches!(self.slots.get(id), Some((Slot::Missing, _)))
    }

    /// Drops the models kept for the level that `wanted` does not name (another circuit).
    pub fn keep_for_the_level(&mut self, wanted: &[String]) {
        let mut dropped_loading = 0;
        self.slots.retain(|id, (slot, keep)| {
            let kept = *keep == Keep::Always || wanted.iter().any(|w| w == id);
            if !kept && matches!(slot, Slot::Loading(_)) {
                dropped_loading += 1;
            }
            kept
        });
        self.loading -= dropped_loading;
    }

    /// How many models are asked for, and how many of them are baked.
    pub fn counts(&self) -> (usize, usize) {
        let ready = self
            .slots
            .values()
            .filter(|(slot, _)| matches!(slot, Slot::Ready(_)))
            .count();
        (self.slots.len(), ready)
    }
}

/// The material every model is drawn with: white, lit, its colors from the mesh's vertex colors
/// (the karts' paint apart: `kart::KartVisualAssets::paint`).
#[derive(Resource, Clone, Debug)]
pub struct ModelMaterials {
    pub vertex_colors: Handle<StandardMaterial>,
}

/// After the models are baked, in `Update`: what uses them comes after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelSystems;

pub struct ModelPlugin;

impl Plugin for ModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Models>()
            .add_systems(Startup, create_model_materials)
            .add_systems(
                Update,
                bake_models
                    .in_set(ModelSystems)
                    .run_if(resource_exists::<Assets<Gltf>>)
                    .run_if(|models: Res<Models>| models.loading > 0),
            );
    }
}

fn create_model_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(ModelMaterials {
        vertex_colors: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.85,
            ..default()
        }),
    });
}

/// Bakes the models whose file (and textures) finished loading, and gives up on those that
/// failed (a warning: what wanted them stands in primitives).
#[allow(clippy::too_many_arguments)]
fn bake_models(
    mut models: ResMut<Models>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<GltfMaterial>>,
    images: Res<Assets<Image>>,
) {
    let models = &mut *models;
    for (id, (slot, _)) in models.slots.iter_mut() {
        let Slot::Loading(handle) = slot else {
            continue;
        };
        match server.load_state(handle.id()) {
            LoadState::Failed(error) => {
                warn!("model {id}: {error}; standing in primitives");
                *slot = Slot::Missing;
                models.loading -= 1;
            }
            LoadState::Loaded => {
                // Its textures too (a missing texture leaves its material's color).
                let textures = server.recursive_dependency_load_state(handle.id());
                if !(textures.is_loaded() || textures.is_failed()) {
                    continue;
                }
                let Some(gltf) = gltfs.get(handle.id()) else {
                    continue;
                };
                let baked = bake_gltf(gltf, &nodes, &gltf_meshes, &meshes, &materials, &images);
                *slot = if baked.triangle_count() == 0 {
                    warn!("model {id}: no triangle; standing in primitives");
                    Slot::Missing
                } else {
                    Slot::Ready(Arc::new(baked))
                };
                models.loading -= 1;
            }
            _ => {}
        }
    }
}

/// Reads a loaded glTF file into a [`BakedModel`]: every node with a mesh, from the file's roots
/// down (transforms accumulated), each primitive colored by its material's base color times its
/// base color texture's texel at each vertex (nearest texel).
pub fn bake_gltf(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    gltf_meshes: &Assets<GltfMesh>,
    meshes: &Assets<Mesh>,
    materials: &Assets<GltfMaterial>,
    images: &Assets<Image>,
) -> BakedModel {
    let all: Vec<&GltfNode> = gltf.nodes.iter().filter_map(|h| nodes.get(h)).collect();
    let is_child = |node: &GltfNode| {
        all.iter().any(|parent| {
            parent
                .children
                .iter()
                .any(|child| nodes.get(child).is_some_and(|c| c.index == node.index))
        })
    };
    let mut model = BakedModel::default();
    let mut stack: Vec<(&GltfNode, Mat4)> = all
        .iter()
        .filter(|node| !is_child(node))
        .map(|node| (*node, Mat4::IDENTITY))
        .collect();
    while let Some((node, parent)) = stack.pop() {
        let transform = parent * node.transform.to_matrix();
        stack.extend(
            node.children
                .iter()
                .filter_map(|child| nodes.get(child))
                .map(|child| (child, transform)),
        );
        let Some(mesh) = node.mesh.as_ref().and_then(|m| gltf_meshes.get(m)) else {
            continue;
        };
        let mut part = BakedPart {
            name: node.name.clone(),
            transform,
            ..default()
        };
        for primitive in &mesh.primitives {
            let Some(data) = meshes.get(&primitive.mesh) else {
                continue;
            };
            let material = primitive.material.as_ref().and_then(|m| materials.get(m));
            let base = material.map_or(LinearRgba::WHITE, |m| m.base_color.to_linear());
            let texture = material
                .and_then(|m| m.base_color_texture.as_ref())
                .and_then(|t| images.get(t));
            add_primitive(&mut part, data, base, texture);
        }
        model.parts.push(part);
    }
    // The file's order, whatever the walk's.
    model.parts.sort_by(|a, b| a.name.cmp(&b.name));
    model
}

/// Bakes one of Bevy's meshes into `part` (triangle lists only).
fn add_primitive(part: &mut BakedPart, mesh: &Mesh, base: LinearRgba, texture: Option<&Image>) {
    if mesh.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
        return;
    }
    let float3 = |attribute| match mesh.try_attribute_option(attribute) {
        Ok(Some(VertexAttributeValues::Float32x3(values))) => Some(values.as_slice()),
        _ => None,
    };
    let Some(positions) = float3(Mesh::ATTRIBUTE_POSITION) else {
        return;
    };
    let normals = float3(Mesh::ATTRIBUTE_NORMAL);
    let uvs = match mesh.try_attribute_option(Mesh::ATTRIBUTE_UV_0) {
        Ok(Some(VertexAttributeValues::Float32x2(values))) => Some(values.as_slice()),
        _ => None,
    };
    let indices: Option<Vec<u32>> = match mesh.try_indices_option() {
        Ok(Some(Indices::U16(i))) => Some(i.iter().map(|i| u32::from(*i)).collect()),
        Ok(Some(Indices::U32(i))) => Some(i.clone()),
        _ => None,
    };
    let texel = |uv: Option<[f32; 2]>| -> LinearRgba {
        let (Some(image), Some([u, v])) = (texture, uv) else {
            return LinearRgba::WHITE;
        };
        let (w, h) = (image.width().max(1), image.height().max(1));
        let x = ((u.rem_euclid(1.0) * w as f32) as u32).min(w - 1);
        let y = ((v.rem_euclid(1.0) * h as f32) as u32).min(h - 1);
        image
            .get_color_at(x, y)
            .map_or(LinearRgba::WHITE, |c| c.to_linear())
    };
    bake::bake_primitive(
        part,
        &Primitive {
            positions,
            normals,
            uvs,
            indices: indices.as_deref(),
        },
        |uv| {
            let c = texel(uv);
            [
                c.red * base.red,
                c.green * base.green,
                c.blue * base.blue,
                1.0,
            ]
        },
    );
}

#[cfg(test)]
mod tests;
