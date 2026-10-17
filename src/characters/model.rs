//! The drivers' models (issue #69): the animals of Kenney's Cube Pets (CC0), sitting in the kart's
//! seat in place of the primitive ball. Pure here (tested on a made-up animal, and on the real
//! files in `crate::models`); `visual.rs` spawns them and plays their animations.
//!
//! A Cube Pet is a tree of nodes: the file's root, a `root`, a `body` (the head and the body in
//! one block, with a `Group` for the face's details, the snout, the beak), a `tail` for most of
//! them (the elephant's is its trunk), `leg-*` for the legs and `wing-*` for the birds. Its clips
//! move the nodes (no skinning): each node becomes an entity, named for the clips by its path
//! from the file's root (`AnimationTargetId`, as Bevy's glTF loader names them). The legs are
//! left out of a driver: they would stick out under the seat. Issue #101: they are kept apart
//! ([`AnimalModelData::legs`]) for the animals on foot of the lobby, which walk and run on them
//! (the files' `walk` and `run` clips move them). The `body`, the `tail` and the birds' wings
//! keep a marker ([`AnimalPart`]): the soft wobble (#12, `wobble.rs`) hangs from them. The ears
//! are part of the body's mesh in every Cube Pet (no node of their own): they lean with it.
//!
//! In the file the animal stands on the ground, its front toward +Z, its body 1.25 wide. In the
//! kart it is turned toward -Z (the kart's front), scaled ([`DriverTuning::model_scale`] times its
//! size), its body's bottom on the seat, centered over it.

use bevy::prelude::*;

use crate::models::BakedModel;
use crate::track::mesh::MeshData;

use super::animation::DriverTuning;

/// The node of a Cube Pet's body (with its head), of its tail and of a bird's wings.
pub const BODY: &str = "body";
pub const TAIL: &str = "tail";
pub const LEFT_WING: &str = "wing-left";
pub const RIGHT_WING: &str = "wing-right";
/// The legs' nodes start with this: left out of a driver, kept apart for an animal on foot.
const LEG: &str = "leg-";

/// A part of an animal that the wobble (#12) moves. The wings are the birds' (chick, penguin,
/// parrot), on the animal's left (+X in the file, where it faces +Z) and right.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimalPart {
    Body,
    Tail,
    LeftWing,
    RightWing,
}

/// A node of an animal, ready to spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimalNode {
    pub name: String,
    /// Its parent in [`AnimalModelData::nodes`] (always before it); None: the file's root.
    pub parent: Option<usize>,
    /// Its transform relative to its parent, at rest (the clips move it).
    pub local: Transform,
    /// Its triangles in its own space, colored (the shared vertex colors material).
    pub mesh: Option<MeshData>,
    /// The names from the file's root down to it: the clips' name for it.
    pub path: Vec<String>,
    pub part: Option<AnimalPart>,
}

impl AnimalNode {
    /// The direction from its origin (its pivot: a tail's or a wing's root, the body's bottom)
    /// toward the middle of its mesh, in its parent's space: where it hangs, what the wobble
    /// swings (#12). Up without a mesh.
    pub fn lever(&self) -> Vec3 {
        let Some(mesh) = self.mesh.as_ref().filter(|m| !m.positions.is_empty()) else {
            return Vec3::Y;
        };
        let (lo, hi) = mesh
            .positions
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            });
        (self.local.rotation * ((lo + hi) / 2.0)).normalize_or(Vec3::Y)
    }
}

/// An animal, ready to sit in a kart (or, issue #101, to stand on its legs).
#[derive(Clone, Debug, PartialEq)]
pub struct AnimalModelData {
    /// From the file's root down, a parent before its children; no leg.
    pub nodes: Vec<AnimalNode>,
    /// Issue #101: its legs, each with its parent in [`AnimalModelData::nodes`] (the `root`):
    /// only an animal on foot has them.
    pub legs: Vec<AnimalNode>,
    /// The middle of its body's bottom at rest, in the file's space: what sits on the seat.
    pub seat_point: Vec3,
    /// Issue #101: its body's box at rest, in the file's space (the animal stands on the
    /// ground, `y = 0`): how big a character on foot is.
    pub body_box: (Vec3, Vec3),
}

impl AnimalModelData {
    /// Where the file's root goes under the driver (`kart::DriverVisual`, whose scale is the
    /// character's size): turned toward the kart's front (-Z), scaled, the middle of its body's
    /// bottom on the seat (the driver's origin), `seat_sink` lower.
    pub fn placement(&self, tuning: &DriverTuning) -> Transform {
        let rotation = Quat::from_rotation_y(std::f32::consts::PI);
        let scale = tuning.model_scale.max(0.01);
        Transform {
            translation: -(rotation * (self.seat_point * scale)) - Vec3::Y * tuning.seat_sink,
            rotation,
            scale: Vec3::splat(scale),
        }
    }

    /// The index of the node named `name`.
    pub fn node(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }
}

/// The animal of a baked Cube Pet (None: no tree, or no body with a mesh: the primitives stay).
/// Only the first root's tree, the legs apart.
pub fn animal_model_data(model: &BakedModel) -> Option<AnimalModelData> {
    model.nodes.first().filter(|n| n.parent.is_none())?;
    let mut nodes: Vec<AnimalNode> = Vec::with_capacity(model.nodes.len());
    let mut legs: Vec<AnimalNode> = Vec::new();
    // Where each of the file's nodes is in `nodes` (None: left out).
    let mut kept: Vec<Option<usize>> = vec![None; model.nodes.len()];
    // The body's vertices in the file's space, at rest.
    let mut body: Option<Vec<Vec3>> = None;
    // Each node's transform in the file's space, at rest.
    let mut world: Vec<Mat4> = Vec::with_capacity(model.nodes.len());
    for (i, node) in model.nodes.iter().enumerate() {
        let parent_world = node.parent.map_or(Mat4::IDENTITY, |p| world[p]);
        world.push(parent_world * node.local.to_matrix());
        let parent = match node.parent {
            // Another root: left out.
            None if i > 0 => continue,
            None => None,
            Some(p) => match kept[p] {
                Some(at) => Some(at),
                None => continue,
            },
        };
        if node.name.starts_with(LEG) {
            // Its own children (none in the Cube Pets) are left out with it.
            legs.push(AnimalNode {
                name: node.name.clone(),
                parent,
                local: node.local,
                mesh: node
                    .part
                    .and_then(|p| model.parts.get(p))
                    .map(|p| p.mesh.clone()),
                path: model.path(i).into_iter().map(str::to_string).collect(),
                part: None,
            });
            continue;
        }
        let part = match node.name.as_str() {
            BODY => Some(AnimalPart::Body),
            TAIL => Some(AnimalPart::Tail),
            LEFT_WING => Some(AnimalPart::LeftWing),
            RIGHT_WING => Some(AnimalPart::RightWing),
            _ => None,
        };
        let mesh = node
            .part
            .and_then(|p| model.parts.get(p))
            .map(|p| p.mesh.clone());
        if let (Some(AnimalPart::Body), Some(mesh)) = (part, &mesh) {
            let at = world[i];
            body = Some(
                mesh.positions
                    .iter()
                    .map(|p| at.transform_point3(Vec3::from(*p)))
                    .collect(),
            );
        }
        kept[i] = Some(nodes.len());
        nodes.push(AnimalNode {
            name: node.name.clone(),
            parent,
            local: node.local,
            mesh,
            path: model.path(i).into_iter().map(str::to_string).collect(),
            part,
        });
    }
    let (lo, hi) = body?.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
        (lo.min(*p), hi.max(*p))
    });
    if lo.x > hi.x {
        return None;
    }
    Some(AnimalModelData {
        nodes,
        legs,
        seat_point: Vec3::new((lo.x + hi.x) / 2.0, lo.y, (lo.z + hi.z) / 2.0),
        body_box: (lo, hi),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BakedNode, BakedPart};

    /// A box mesh from `lo` to `hi`, in its node's space.
    fn block(lo: Vec3, hi: Vec3) -> MeshData {
        let mut mesh = MeshData::default();
        for x in [lo.x, hi.x] {
            for y in [lo.y, hi.y] {
                for z in [lo.z, hi.z] {
                    mesh.positions.push([x, y, z]);
                    mesh.normals.push([0.0, 1.0, 0.0]);
                    mesh.colors.push([0.5, 0.5, 0.5, 1.0]);
                }
            }
        }
        mesh.indices.extend_from_slice(&[0, 1, 3, 0, 3, 2]);
        mesh
    }

    /// A Cube Pet like the files: "animal-test" → "root" → "body" (1.25 wide, 1.4 high, 0.18
    /// above the ground) → "Group" and "tail"; "root" → two legs; another root (left out).
    fn made_up_animal() -> BakedModel {
        let node = |name: &str, parent: Option<usize>, at: Vec3, part: Option<usize>| BakedNode {
            name: name.to_string(),
            parent,
            local: Transform::from_translation(at),
            part,
        };
        let part = |name: &str, mesh: MeshData| BakedPart {
            name: name.to_string(),
            mesh,
            ..default()
        };
        BakedModel {
            parts: vec![
                part(
                    "body",
                    block(Vec3::new(-0.625, 0.0, -0.625), Vec3::new(0.625, 1.4, 0.725)),
                ),
                part("Group", block(Vec3::ZERO, Vec3::splat(0.1))),
                part("tail", block(Vec3::ZERO, Vec3::new(0.2, 1.0, 0.5))),
                part("leg-front-left", block(Vec3::ZERO, Vec3::splat(0.3))),
                part("leg-back-left", block(Vec3::ZERO, Vec3::splat(0.3))),
            ],
            nodes: vec![
                node("animal-test", None, Vec3::ZERO, None),
                node("root", Some(0), Vec3::ZERO, None),
                node("body", Some(1), Vec3::new(0.0, 0.18, 0.0), Some(0)),
                node("Group", Some(2), Vec3::new(0.18, 0.43, 0.63), Some(1)),
                node("tail", Some(2), Vec3::new(0.0, 0.64, -0.55), Some(2)),
                node(
                    "leg-front-left",
                    Some(1),
                    Vec3::new(0.25, 0.3, 0.25),
                    Some(3),
                ),
                node(
                    "leg-back-left",
                    Some(1),
                    Vec3::new(0.25, 0.3, -0.25),
                    Some(4),
                ),
                node("another", None, Vec3::ZERO, None),
                node("inside", Some(7), Vec3::ZERO, None),
            ],
        }
    }

    /// Issue #69: the tree is kept without the legs (nor another root); each node knows its
    /// path from the file's root (the clips' name for it) and its parent; the body and the tail
    /// are marked for the wobble (#12); the body's bottom is what sits on the seat. Issue #101:
    /// the legs are kept apart, under the `root`, with their meshes and their paths, for an
    /// animal on foot; the body's box says how big it stands.
    #[test]
    fn an_animal_keeps_its_tree_without_its_legs() {
        let animal = animal_model_data(&made_up_animal()).unwrap();
        let names: Vec<&str> = animal.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["animal-test", "root", "body", "Group", "tail"]);
        let legs: Vec<(&str, Option<usize>)> = animal
            .legs
            .iter()
            .map(|n| (n.name.as_str(), n.parent))
            .collect();
        assert_eq!(
            legs,
            [("leg-front-left", Some(1)), ("leg-back-left", Some(1))]
        );
        assert!(animal.legs.iter().all(|leg| leg.mesh.is_some()));
        assert_eq!(
            animal.legs[0].path,
            ["animal-test", "root", "leg-front-left"]
        );
        let (lo, hi) = animal.body_box;
        assert!(
            (lo - Vec3::new(-0.625, 0.18, -0.625)).length() < 1e-5,
            "{lo}"
        );
        assert!((hi - Vec3::new(0.625, 1.58, 0.725)).length() < 1e-5, "{hi}");
        let parents: Vec<Option<usize>> = animal.nodes.iter().map(|n| n.parent).collect();
        assert_eq!(parents, [None, Some(0), Some(1), Some(2), Some(2)]);
        assert_eq!(
            animal.nodes[4].path,
            ["animal-test", "root", "body", "tail"]
        );
        assert_eq!(animal.nodes[0].path, ["animal-test"]);
        assert_eq!(animal.nodes[2].part, Some(AnimalPart::Body));
        assert_eq!(animal.nodes[4].part, Some(AnimalPart::Tail));
        assert_eq!(animal.nodes[3].part, None);
        assert!(animal.nodes[1].mesh.is_none() && animal.nodes[2].mesh.is_some());
        assert_eq!(animal.node("tail"), Some(4));
        assert_eq!(
            animal.nodes[2].local,
            Transform::from_xyz(0.0, 0.18, 0.0),
            "at rest"
        );
        assert!((animal.seat_point - Vec3::new(0.0, 0.18, 0.05)).length() < 1e-5);
        // Issue #12: where each part hangs from its pivot, toward the middle of its mesh (up
        // without one).
        let tail = animal.nodes[4].lever();
        assert!((tail - Vec3::new(0.1, 0.5, 0.25).normalize()).length() < 1e-5);
        assert_eq!(animal.nodes[1].lever(), Vec3::Y);
        assert!(animal.nodes[2].lever().y > 0.99);
        // A bird's wings are marked too, left and right.
        let mut bird = made_up_animal();
        bird.nodes[5].name = "wing-left".to_string();
        bird.nodes[6].name = "wing-right".to_string();
        let bird = animal_model_data(&bird).unwrap();
        let parts: Vec<Option<AnimalPart>> = bird.nodes.iter().map(|n| n.part).collect();
        assert_eq!(
            parts,
            [
                None,
                None,
                Some(AnimalPart::Body),
                None,
                Some(AnimalPart::Tail),
                Some(AnimalPart::LeftWing),
                Some(AnimalPart::RightWing)
            ]
        );
        // No body (or a body without a mesh): not an animal.
        let mut model = made_up_animal();
        model.nodes[2].part = None;
        assert!(animal_model_data(&model).is_none());
        assert!(animal_model_data(&BakedModel::default()).is_none());
    }

    /// Issue #69: in the kart, the animal faces the kart's front (-Z), at its scale, the middle
    /// of its body's bottom on the seat (the driver's origin), sunk as tuned.
    #[test]
    fn an_animal_sits_on_the_seat_facing_the_front() {
        let animal = animal_model_data(&made_up_animal()).unwrap();
        let tuning = DriverTuning::default();
        let placed = animal.placement(&tuning);
        let s = tuning.model_scale;
        // The middle of the body's bottom: on the seat, sunk.
        let seat = placed.transform_point(animal.seat_point);
        assert!(
            (seat - Vec3::new(0.0, -tuning.seat_sink, 0.0)).length() < 1e-5,
            "{seat}"
        );
        // Its face (+Z in the file) toward the kart's front (-Z), its left (+X) on the kart's
        // left (-X).
        let face = placed.transform_point(animal.seat_point + Vec3::Z);
        let left = placed.transform_point(animal.seat_point + Vec3::X);
        assert!(
            (face - seat - Vec3::new(0.0, 0.0, -s)).length() < 1e-5,
            "{face}"
        );
        assert!(
            (left - seat - Vec3::new(-s, 0.0, 0.0)).length() < 1e-5,
            "{left}"
        );
        // The body's top: 1.4 × the scale above the seat.
        let top = placed.transform_point(animal.seat_point + Vec3::Y * 1.4);
        assert!((top.y - seat.y - 1.4 * s).abs() < 1e-5);
        // Sunk deeper: lower.
        let sunk = DriverTuning {
            seat_sink: 0.2,
            ..default()
        };
        let lower = animal.placement(&sunk).transform_point(animal.seat_point);
        assert!((lower.y + 0.2).abs() < 1e-5);
    }
}
