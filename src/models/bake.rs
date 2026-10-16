//! Baking a model (issue #11): each node with a mesh becomes a [`BakedPart`], its triangles
//! colored by their material or their texture (vertex colors), so that every model of every pack
//! is drawn with the same material (`ModelMaterials::vertex_colors`), and the pieces of a circuit
//! can be merged into a couple of meshes. Pure, no ECS: `super::bake_gltf` reads Bevy's glTF
//! assets into it.

use bevy::prelude::*;

use crate::track::mesh::MeshData;

/// A node of a model with a mesh: its triangles in the node's own space, each vertex with the
/// color of its material (times its texture's texel, linear), and the node in the model's space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BakedPart {
    pub name: String,
    /// The node in the model's space (its parents' transforms included).
    pub transform: Mat4,
    pub mesh: MeshData,
    /// The texture coordinates of each vertex (0 without a texture): where in the pack's
    /// colormap it is (the kart's paint, `kart::model`).
    pub uvs: Vec<[f32; 2]>,
}

/// A node of a model's tree (issue #69): its name, its parent (an index into
/// [`BakedModel::nodes`], always before it), its own transform (relative to its parent) and its
/// part, if it has a mesh (an index into [`BakedModel::parts`]). An animated model needs it: its
/// animations move the nodes (the Cube Pets' body, tail and legs), so each node becomes an entity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BakedNode {
    pub name: String,
    pub parent: Option<usize>,
    pub local: Transform,
    pub part: Option<usize>,
}

/// A model: its parts, and the tree of its nodes (issue #69). glTF's axes: +Y up, the model's
/// front toward +Z.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BakedModel {
    pub parts: Vec<BakedPart>,
    /// Every node of the file, from its roots down (a parent before its children).
    pub nodes: Vec<BakedNode>,
}

/// One primitive of a mesh, as read from the file.
pub struct Primitive<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: Option<&'a [[f32; 3]]>,
    pub uvs: Option<&'a [[f32; 2]]>,
    /// Triangles (a triangle list); `None`: the vertices in order.
    pub indices: Option<&'a [u32]>,
}

/// Adds `primitive` to `part`, each vertex colored by `color` (linear RGBA), given its texture
/// coordinates (None without them). A primitive without normals gets the flat normal of its
/// triangles' first one.
pub fn bake_primitive(
    part: &mut BakedPart,
    primitive: &Primitive,
    color: impl Fn(Option<[f32; 2]>) -> [f32; 4],
) {
    let base = part.mesh.positions.len() as u32;
    let count = primitive.positions.len();
    let uv_at = |i: usize| primitive.uvs.and_then(|uvs| uvs.get(i).copied());
    let mut normals: Vec<[f32; 3]> = match primitive.normals {
        Some(normals) if normals.len() == count => normals.to_vec(),
        _ => vec![[0.0, 1.0, 0.0]; count],
    };
    let indices: Vec<u32> = match primitive.indices {
        Some(indices) => indices
            .iter()
            .copied()
            .filter(|i| (*i as usize) < count)
            .collect(),
        None => (0..count as u32).collect(),
    };
    let triangles = indices.len() / 3 * 3;
    if primitive.normals.is_none() {
        for t in indices[..triangles].as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| Vec3::from(primitive.positions[i as usize]));
            let n = (b - a).cross(c - a).normalize_or(Vec3::Y).to_array();
            for i in t {
                normals[*i as usize] = n;
            }
        }
    }
    for (i, (position, normal)) in primitive.positions.iter().zip(normals).enumerate() {
        part.mesh.positions.push(*position);
        part.mesh.normals.push(normal);
        part.mesh.colors.push(color(uv_at(i)));
        part.uvs.push(uv_at(i).unwrap_or_default());
    }
    part.mesh
        .indices
        .extend(indices[..triangles].iter().map(|i| base + i));
}

/// `mesh` moved by `transform`: positions, normals (by the inverse transpose, renormalized) and,
/// when `transform` mirrors, the triangles' winding (their front stays outside).
pub fn transformed(mesh: &MeshData, transform: Mat4) -> MeshData {
    let mut out = MeshData::default();
    append_transformed(&mut out, mesh, transform);
    out
}

/// Appends `mesh` moved by `transform` to `out` (see [`transformed`]).
pub fn append_transformed(out: &mut MeshData, mesh: &MeshData, transform: Mat4) {
    let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
    let base = out.positions.len() as u32;
    out.positions.extend(
        mesh.positions
            .iter()
            .map(|p| transform.transform_point3(Vec3::from(*p)).to_array()),
    );
    out.normals.extend(mesh.normals.iter().map(|n| {
        (normal_matrix * Vec3::from(*n))
            .normalize_or(Vec3::Y)
            .to_array()
    }));
    out.colors.extend_from_slice(&mesh.colors);
    let mirrored = transform.determinant() < 0.0;
    for t in mesh.indices.as_chunks::<3>().0 {
        let (b, c) = if mirrored { (t[2], t[1]) } else { (t[1], t[2]) };
        out.indices
            .extend_from_slice(&[base + t[0], base + b, base + c]);
    }
}

/// The triangles of `part` whose texture coordinates (at their center) `inside` takes, and the
/// others: two meshes in the part's own space.
pub fn split_by_uv(part: &BakedPart, inside: impl Fn(Vec2) -> bool) -> (MeshData, MeshData) {
    let (mut yes, mut no) = (MeshData::default(), MeshData::default());
    for t in part.mesh.indices.as_chunks::<3>().0 {
        let center = t
            .iter()
            .map(|i| Vec2::from(part.uvs[*i as usize]))
            .sum::<Vec2>()
            / 3.0;
        let out = if inside(center) { &mut yes } else { &mut no };
        let base = out.positions.len() as u32;
        for i in t {
            let i = *i as usize;
            out.positions.push(part.mesh.positions[i]);
            out.normals.push(part.mesh.normals[i]);
            out.colors.push(part.mesh.colors[i]);
        }
        out.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    (yes, no)
}

impl BakedModel {
    pub fn part(&self, name: &str) -> Option<&BakedPart> {
        self.parts.iter().find(|p| p.name == name)
    }

    /// The names from the root of the node `index` down to it (the node included): what the
    /// glTF loader names an animation's target with (`AnimationTargetId::from_names`).
    pub fn path(&self, index: usize) -> Vec<&str> {
        let mut path = Vec::new();
        let mut at = Some(index);
        while let Some(i) = at {
            let Some(node) = self.nodes.get(i) else {
                break;
            };
            path.push(node.name.as_str());
            at = node.parent;
        }
        path.reverse();
        path
    }

    /// The box around the model in its own space (None: no vertex).
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        self.parts
            .iter()
            .flat_map(|part| {
                part.mesh
                    .positions
                    .iter()
                    .map(|p| part.transform.transform_point3(Vec3::from(*p)))
            })
            .fold(None, |bounds, p| match bounds {
                None => Some((p, p)),
                Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
            })
    }

    /// How far it reaches from its vertical axis through the middle of its bounds (m, in its
    /// own space).
    pub fn reach(&self) -> f32 {
        let Some((lo, hi)) = self.bounds() else {
            return 0.0;
        };
        let axis = ((lo + hi) / 2.0).xz();
        self.parts
            .iter()
            .flat_map(|part| {
                part.mesh
                    .positions
                    .iter()
                    .map(|p| part.transform.transform_point3(Vec3::from(*p)))
            })
            .map(|p| p.xz().distance(axis))
            .fold(0.0, f32::max)
    }

    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.mesh.triangle_count()).sum()
    }

    /// Appends the whole model, standing on the ground: the middle of its bounds' bottom at
    /// `position`, `height` high (at most `radius` from its axis: never wider than its footprint),
    /// turned by `yaw`. Returns its scale.
    pub fn place(
        &self,
        out: &mut MeshData,
        position: Vec3,
        yaw: f32,
        height: f32,
        radius: f32,
    ) -> f32 {
        let Some((lo, hi)) = self.bounds() else {
            return 0.0;
        };
        let size = hi.y - lo.y;
        let reach = self.reach();
        let mut scale = if size > 1e-4 { height / size } else { 1.0 };
        if reach * scale > radius {
            scale = radius / reach;
        }
        let bottom = Vec3::new((lo.x + hi.x) / 2.0, lo.y, (lo.z + hi.z) / 2.0);
        let placed = Mat4::from_scale_rotation_translation(
            Vec3::splat(scale),
            Quat::from_rotation_y(yaw),
            position,
        ) * Mat4::from_translation(-bottom);
        for part in &self.parts {
            append_transformed(out, &part.mesh, placed * part.transform);
        }
        scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit square on the ground (two triangles), its four corners' texture coordinates.
    fn square(uvs: [[f32; 2]; 4]) -> BakedPart {
        let mut part = BakedPart::default();
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, -1.0],
            [0.0, 0.0, -1.0],
        ];
        bake_primitive(
            &mut part,
            &Primitive {
                positions: &positions,
                normals: None,
                uvs: Some(&uvs),
                indices: Some(&[0, 1, 2, 0, 2, 3]),
            },
            |uv| match uv {
                Some([u, _]) if u > 0.5 => [1.0, 0.0, 0.0, 1.0],
                _ => [0.0, 0.0, 1.0, 1.0],
            },
        );
        part
    }

    #[test]
    fn a_primitive_takes_the_color_of_its_texels_and_a_flat_normal() {
        let part = square([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(part.mesh.triangle_count(), 2);
        assert_eq!(part.mesh.colors[0], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(part.mesh.colors[1], [1.0, 0.0, 0.0, 1.0]);
        // Counterclockwise seen from above: facing up.
        assert!(part.mesh.normals.iter().all(|n| (n[1] - 1.0).abs() < 1e-6));
        // A second primitive goes after the first, its indices shifted.
        let mut two = part.clone();
        bake_primitive(
            &mut two,
            &Primitive {
                positions: &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                normals: Some(&[[0.0, 0.0, 1.0]; 3]),
                uvs: None,
                indices: None,
            },
            |_| [1.0; 4],
        );
        assert_eq!(two.mesh.indices[6..], [4, 5, 6]);
        assert_eq!(two.uvs.len(), 7);
    }

    #[test]
    fn a_mirrored_transform_keeps_the_front_outside() {
        let part = square([[0.0; 2]; 4]);
        let mirrored = transformed(&part.mesh, Mat4::from_scale(Vec3::new(-2.0, 1.0, 1.0)));
        assert_eq!(mirrored.positions[1], [-2.0, 0.0, 0.0]);
        // The winding flips, so its triangles' geometric normal still faces up, like its normals.
        let t = &mirrored.indices[..3];
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(mirrored.positions[i as usize]));
        assert!((b - a).cross(c - a).y > 0.0);
        assert!(mirrored.normals.iter().all(|n| n[1] > 0.99));
    }

    #[test]
    fn triangles_split_by_where_they_are_in_the_texture() {
        // Left triangle (0, 1, 2) has its center at u = 2/3, the other at u = 1/3.
        let part = square([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let (right, left) = split_by_uv(&part, |uv| uv.x > 0.5);
        assert_eq!(right.triangle_count(), 1);
        assert_eq!(left.triangle_count(), 1);
        assert_eq!(right.positions[2], [1.0, 0.0, -1.0]);
    }

    #[test]
    fn a_placed_model_stands_on_the_ground_at_its_height_within_its_footprint() {
        let mut model = BakedModel::default();
        // A 1 × 2 × 1 box made of its two opposite corners' squares, off its origin.
        let mut part = square([[0.0; 2]; 4]);
        part.mesh.positions.push([0.0, 2.0, 0.0]);
        part.mesh.normals.push([0.0, 1.0, 0.0]);
        part.mesh.colors.push([1.0; 4]);
        part.uvs.push([0.0; 2]);
        part.transform = Mat4::from_translation(Vec3::new(3.0, 1.0, 0.0));
        model.parts.push(part);
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!(
            (lo, hi),
            (Vec3::new(3.0, 1.0, -1.0), Vec3::new(4.0, 3.0, 0.0))
        );
        let mut out = MeshData::default();
        let at = Vec3::new(10.0, -0.25, 5.0);
        let scale = model.place(&mut out, at, 0.3, 6.0, 10.0);
        assert!((scale - 3.0).abs() < 1e-5);
        let (min_y, max_y) = out
            .positions
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        assert!((min_y - at.y).abs() < 1e-4 && (max_y - at.y - 6.0).abs() < 1e-4);
        for p in &out.positions {
            assert!(Vec2::new(p[0], p[2]).distance(at.xz()) <= 3.0 * model.reach() + 1e-4);
        }
        // Too wide for its footprint: smaller than asked.
        let mut narrow = MeshData::default();
        let scale = model.place(&mut narrow, at, 0.0, 6.0, 1.0);
        assert!(scale * model.reach() <= 1.0 + 1e-5);
        for p in &narrow.positions {
            assert!(Vec2::new(p[0], p[2]).distance(at.xz()) <= 1.0 + 1e-4);
        }
    }
}
