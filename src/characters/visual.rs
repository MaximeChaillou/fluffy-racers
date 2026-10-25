//! The drivers' animals in the karts (issue #69): each character's Cube Pet, loaded with its
//! clips (`Models::request_animated`), baked into one mesh per node (the models' shared vertex
//! colors material: no material per animal nor per kart), then seated in every kart it drives,
//! where it plays what its kart is going through (`animation.rs`).
//!
//! **Fluidity**: the meshes of an animal and its `AnimationGraph` are made once, shared by every
//! kart it drives (12 karts, 12 `AnimationPlayer`s, at most 9 graphs); a driver is dressed once,
//! in `PostUpdate` on the frame its kart appears (its primitives never show); each frame, each
//! driver only compares what it should play with what it plays (nothing allocated), and Bevy's
//! animation systems move a handful of nodes per kart.
//!
//! **Fallback**: a driver stays in primitives (its ball, eyes and ears, `kart::visual`) until
//! its animal is ready, and for good without a glTF loader (headless) or when the file is missing
//! (a warning). Without Bevy's animation plugin, the animal sits still.
//!
//! **The wobble (#12)**: the `body`, `tail` and wing nodes carry [`AnimalPart`](super::AnimalPart)
//! and a [`WobbleLayer`]; the driver its [`DriverWobble`], with where its tail and wings hang.
//! Their `Transform` is written by the clip each frame (`AnimationSystems`, in `PostUpdate`), so
//! the wobble goes on top of it, after `AnimationSystems` and before `TransformSystems::Propagate`
//! (`wobble_visual.rs`).

use std::sync::Arc;
use std::time::Duration;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::app::AnimationSystems;
use bevy::ecs::query::QueryFilter;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::Roster;
use super::animation::{AnimalAnimation, DriverFacts, DriverTuning, Finish, driver_animation};
use super::model::{AnimalModelData, AnimalPart, BODY, animal_model_data};
use super::paw::{PawShape, paw_model};
use super::wobble::{LayerPivot, WobbleLevers, WobbleState};
use super::wobble_visual::{DriverWobble, WobbleLayer};
use crate::interactions::Paw;
use crate::kart::{DriverPrimitive, DriverVisual, KartBodyPivot, KartState};
use crate::models::{Keep, ModelMaterials, ModelSystems, Models};
use crate::race::{OnPodium, Race, RaceProgress};
use crate::track::circuit::to_mesh;

/// One animal, ready to sit in karts: its nodes' meshes and its clips, shared by every kart.
#[derive(Clone, Debug)]
pub struct AnimalVisual {
    pub data: AnimalModelData,
    /// Each node's mesh (by node).
    pub meshes: Vec<Option<Handle<Mesh>>>,
    /// Each node's name for the clips.
    pub targets: Vec<AnimationTargetId>,
    /// Issue #101: each leg's mesh and name for the clips (`AnimalModelData::legs`): only an
    /// animal on foot spawns them.
    pub leg_meshes: Vec<Option<Handle<Mesh>>>,
    pub leg_targets: Vec<AnimationTargetId>,
    /// Its graph and the node of each animation in it (`AnimalAnimation::index`); None: the
    /// file has no idle, or the game no animation plugin (it sits still).
    pub graph: Option<(Handle<AnimationGraph>, AnimationNodes)>,
    /// How long its idle lasts (s): each kart starts it at its own moment.
    pub idle_seconds: f32,
    pub material: Handle<StandardMaterial>,
    /// Issue #169: its paw (its arm and its hand, in its colors), shared by every kart it drives
    /// and every character of the lobby; None without a body.
    pub paw: Option<AnimalPaw>,
}

/// Issue #169: an animal's paw on screen (`paw.rs`): its arm's and its hand's meshes (in the
/// models' shared material), how long its hand is (in the animal's units), where its arm comes
/// out (in its body's node, on its left: [`super::PawModel::shoulder`]), and the nodes it hangs from:
/// its body, and a bird's wings (its left, then its right), which its arm replaces while out.
#[derive(Clone, Debug)]
pub struct AnimalPaw {
    pub arm: Handle<Mesh>,
    pub hand: Handle<Mesh>,
    pub hand_length: f32,
    pub shoulder: Vec3,
    /// Its body's node and its wings' (indices in `AnimalModelData::nodes`).
    pub body: usize,
    pub wings: [Option<usize>; 2],
}

impl AnimalPaw {
    /// Its shoulders in the frame where `body` (its body's node) is `at`: on the left of that
    /// frame (-X: the animal's left, turned to face -Z), then on its right.
    pub fn shoulders(&self, at: &Affine3A) -> [Vec3; 2] {
        [
            at.transform_point3(self.shoulder),
            at.transform_point3(self.shoulder * Vec3::new(-1.0, 1.0, 1.0)),
        ]
    }
}

/// Where a character's animal is.
#[derive(Clone, Debug, Default)]
pub enum AnimalSlot {
    /// The character has no model.
    #[default]
    None,
    /// Loading (its model's id).
    Waiting(String),
    Ready(Arc<AnimalVisual>),
    /// Missing, broken, or not a Cube Pet: its drivers stay in primitives.
    Missing,
}

/// Each character's animal, by roster index.
#[derive(Resource, Clone, Debug, Default)]
pub struct Animals(pub Vec<AnimalSlot>);

impl Animals {
    pub fn get(&self, character: usize) -> Option<&Arc<AnimalVisual>> {
        match self.0.get(character) {
            Some(AnimalSlot::Ready(animal)) => Some(animal),
            _ => None,
        }
    }

    pub fn waiting(&self) -> bool {
        self.0.iter().any(|s| matches!(s, AnimalSlot::Waiting(_)))
    }

    fn any_ready(&self) -> bool {
        self.0.iter().any(|s| matches!(s, AnimalSlot::Ready(_)))
    }
}

/// On a driver once it is dressed (its animal, or its primitives for good).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverDressed;

/// The animal under a driver: where it sits (its `Transform`), its `AnimationPlayer`. `kart`:
/// the kart it drives (its simulated body, or the body of another peer's kart); `playing`: what
/// it plays; `nodes`: its graph's node of each animation. Issue #169: `body`, its body's entity
/// (where its arms come out), and a bird's wings (its left, then its right).
#[derive(Component, Clone, Copy, Debug)]
pub struct AnimalRoot {
    pub kart: Entity,
    pub character: usize,
    pub playing: AnimalAnimation,
    pub nodes: Option<AnimationNodes>,
    pub body: Option<Entity>,
    pub wings: [Option<Entity>; 2],
}

/// The node of each animation in an animal's graph (`AnimalAnimation::index`).
pub type AnimationNodes = [AnimationNodeIndex; AnimalAnimation::ALL.len()];

pub struct AnimalVisualPlugin;

impl Plugin for AnimalVisualPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Animals>()
            .add_systems(Startup, request_animals)
            .add_systems(
                Update,
                (
                    prepare_animals
                        .after(ModelSystems)
                        .run_if(|animals: Res<Animals>| animals.waiting()),
                    animate_drivers,
                )
                    .chain(),
            )
            // After the karts spawned in `Update`, before the clips and the transforms: a new
            // kart shows its animal on its very first frame, never its primitives.
            .add_systems(
                PostUpdate,
                dress_drivers
                    .run_if(|animals: Res<Animals>| animals.any_ready())
                    .before(AnimationSystems)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// Asks for every character's animal with its clips, kept for the whole game.
fn request_animals(
    roster: Res<Roster>,
    models: Option<ResMut<Models>>,
    server: Res<AssetServer>,
    gltfs: Option<Res<Assets<bevy::gltf::Gltf>>>,
    mut animals: ResMut<Animals>,
) {
    let Some(mut models) = models else {
        return;
    };
    let server = gltfs.map(|_| &*server);
    animals.0 = roster
        .characters
        .iter()
        .map(|character| match &character.model {
            Some(id) => {
                models.request_animated(id, Keep::Always, server);
                AnimalSlot::Waiting(id.clone())
            }
            None => AnimalSlot::None,
        })
        .collect();
}

/// Once an animal's model is baked: its meshes and its graph (once for every kart). A model
/// missing, or that is not a Cube Pet, leaves its drivers in primitives (said once).
fn prepare_animals(
    models: Res<Models>,
    roster: Res<Roster>,
    shared: Option<Res<ModelMaterials>>,
    mut animals: ResMut<Animals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut graphs: Option<ResMut<Assets<AnimationGraph>>>,
    clips: Option<Res<Assets<AnimationClip>>>,
) {
    let Some(shared) = shared else {
        return;
    };
    for (character, slot) in animals.0.iter_mut().enumerate() {
        let AnimalSlot::Waiting(id) = slot else {
            continue;
        };
        if models.missing(id) {
            *slot = AnimalSlot::Missing;
            continue;
        }
        let Some(baked) = models.get(id) else {
            continue;
        };
        let Some(data) = animal_model_data(baked) else {
            warn!("model {id}: not an animal (no body); its drivers stay in primitives");
            *slot = AnimalSlot::Missing;
            continue;
        };
        let file_clips = models
            .clips(id)
            .cloned()
            .unwrap_or_else(|| Vec::new().into());
        let clip = |animation: AnimalAnimation| {
            file_clips
                .iter()
                .find(|(name, _)| name == animation.clip())
                .map(|(_, handle)| handle.clone())
        };
        let graph = match (graphs.as_mut(), clip(AnimalAnimation::Idle)) {
            (Some(graphs), Some(idle)) => {
                // A clip the file lacks plays the idle.
                let handles =
                    AnimalAnimation::ALL.map(|animation| clip(animation).unwrap_or(idle.clone()));
                let (graph, nodes) = AnimationGraph::from_clips(handles);
                <AnimationNodes>::try_from(nodes)
                    .ok()
                    .map(|nodes| (graphs.add(graph), nodes))
            }
            (Some(_), None) => {
                warn!("model {id}: no idle clip; its drivers sit still");
                None
            }
            // No animation plugin: they sit still.
            (None, _) => None,
        };
        let idle_seconds = clip(AnimalAnimation::Idle)
            .and_then(|idle| clips.as_ref()?.get(&idle).map(|c| c.duration()))
            .unwrap_or(1.0);
        // Issue #169: its paw, in its colors.
        let shape = roster
            .characters
            .get(character)
            .map_or(PawShape::default(), |c| c.paw);
        let paw = paw_model(&data, shape).and_then(|paw| {
            let wing = |part: AnimalPart| data.nodes.iter().position(|n| n.part == Some(part));
            Some(AnimalPaw {
                arm: meshes.add(to_mesh(&paw.arm)),
                hand: meshes.add(to_mesh(&paw.hand)),
                hand_length: paw.hand_length,
                shoulder: paw.shoulder,
                body: data.node(BODY)?,
                wings: [wing(AnimalPart::LeftWing), wing(AnimalPart::RightWing)],
            })
        });
        let animal = AnimalVisual {
            paw,
            meshes: data
                .nodes
                .iter()
                .map(|node| node.mesh.as_ref().map(|m| meshes.add(to_mesh(m))))
                .collect(),
            targets: data
                .nodes
                .iter()
                .map(|node| AnimationTargetId::from_iter(&node.path))
                .collect(),
            leg_meshes: data
                .legs
                .iter()
                .map(|node| node.mesh.as_ref().map(|m| meshes.add(to_mesh(m))))
                .collect(),
            leg_targets: data
                .legs
                .iter()
                .map(|node| AnimationTargetId::from_iter(&node.path))
                .collect(),
            data,
            graph,
            idle_seconds,
            material: shared.vertex_colors.clone(),
        };
        *slot = AnimalSlot::Ready(Arc::new(animal));
    }
}

/// Seats each driver's animal in its kart, in place of its primitives, once its animal is
/// ready; a driver without one keeps its primitives. Once per driver.
fn dress_drivers(
    mut commands: Commands,
    animals: Res<Animals>,
    tuning: Res<DriverTuning>,
    drivers: Query<(Entity, &DriverVisual, &ChildOf, &Children), Without<DriverDressed>>,
    primitives: Query<(), With<DriverPrimitive>>,
    pivots: Query<&KartBodyPivot>,
) {
    for (driver, visual, child_of, children) in &drivers {
        let animal = match visual.character.and_then(|c| animals.0.get(c)) {
            Some(AnimalSlot::Waiting(_)) => continue,
            Some(AnimalSlot::Ready(animal)) => animal.clone(),
            _ => {
                commands.entity(driver).insert(DriverDressed);
                continue;
            }
        };
        let Ok(pivot) = pivots.get(child_of.parent()) else {
            continue;
        };
        for child in children.iter().filter(|c| primitives.contains(*c)) {
            commands.entity(child).despawn();
        }
        commands.entity(driver).insert(DriverDressed);
        spawn_animal(
            &mut commands,
            &animal,
            &tuning,
            driver,
            pivot.body,
            visual.character.unwrap_or_default(),
        );
    }
}

/// Spawns `animal` under `driver`, its nodes animated by its root's `AnimationPlayer`, which
/// starts its idle at a moment of its own (from the kart's entity: 12 karts do not sway in step).
/// Its body, tail and wings get their wobble's layer (#12), the driver its wobble, which knows
/// where they hang.
fn spawn_animal(
    commands: &mut Commands,
    animal: &AnimalVisual,
    tuning: &DriverTuning,
    driver: Entity,
    kart: Entity,
    character: usize,
) {
    let placement = animal.data.placement(tuning);
    // From the wobble's frame (the kart's) into the animal's (its file's), and the other way.
    let to_file = placement.rotation.inverse();
    commands
        .entity(driver)
        .insert(DriverWobble(WobbleState::new(levers(animal, &placement))));
    let mut root = commands.spawn((placement, Visibility::default(), ChildOf(driver)));
    if let Some((graph, nodes)) = &animal.graph {
        let mut player = AnimationPlayer::default();
        let mut transitions = AnimationTransitions::new();
        let idle = AnimalAnimation::Idle;
        let phase = (kart.to_bits() as u32).wrapping_mul(2_654_435_761) as f32 / u32::MAX as f32;
        transitions
            .play(&mut player, nodes[idle.index()], Duration::ZERO)
            .set_speed(idle.speed(tuning))
            .seek_to(phase * animal.idle_seconds)
            .repeat();
        root.insert((player, transitions, AnimationGraphHandle(graph.clone())));
    }
    let root = root.id();
    let mut spawned: Vec<Entity> = Vec::with_capacity(animal.data.nodes.len());
    for (i, node) in animal.data.nodes.iter().enumerate() {
        let parent = node.parent.map_or(root, |p| spawned[p]);
        let mut entity = commands.spawn((
            node.local,
            Visibility::default(),
            animal.targets[i],
            AnimatedBy(root),
            ChildOf(parent),
        ));
        if let Some(mesh) = &animal.meshes[i] {
            entity.insert((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(animal.material.clone()),
            ));
        }
        if let Some(part) = node.part {
            entity.insert((
                part,
                WobbleLayer::new(driver, part.into(), LayerPivot::Own, to_file),
            ));
        }
        spawned.push(entity.id());
    }
    let (body, wings) = paw_nodes(animal, &spawned);
    commands.entity(root).insert(AnimalRoot {
        kart,
        character,
        playing: AnimalAnimation::Idle,
        nodes: animal.graph.as_ref().map(|(_, nodes)| *nodes),
        body,
        wings,
    });
}

/// Issue #169: `node`'s transform in the frame of its ancestor `frame`, from their `Transform`s
/// as they are now (this frame's clip, wobble and acting, before they are propagated): what a
/// paw follows. None when `frame` is not one of its ancestors (16 levels up at most).
pub fn transform_in<F: QueryFilter>(
    node: Entity,
    frame: Entity,
    nodes: &Query<(&Transform, &ChildOf), F>,
) -> Option<Affine3A> {
    let mut at = Affine3A::IDENTITY;
    let mut entity = node;
    for _ in 0..16 {
        let (transform, child_of) = nodes.get(entity).ok()?;
        at = transform.compute_affine() * at;
        if child_of.parent() == frame {
            return Some(at);
        }
        entity = child_of.parent();
    }
    None
}

/// Issue #169: the entities of `animal`'s body and wings among its `spawned` nodes (in
/// `AnimalModelData::nodes`' order), where its paw hangs from.
pub fn paw_nodes(
    animal: &AnimalVisual,
    spawned: &[Entity],
) -> (Option<Entity>, [Option<Entity>; 2]) {
    let Some(paw) = &animal.paw else {
        return (None, [None; 2]);
    };
    let at = |i: Option<usize>| i.and_then(|i| spawned.get(i).copied());
    (at(Some(paw.body)), paw.wings.map(at))
}

/// Where the tail and the wings of `animal` hang once placed by `placement`, in its wobble's
/// frame (the kart's, or the walker's: its front toward -Z): the animal's left (+X in its file)
/// is on that frame's left (-X).
fn levers(animal: &AnimalVisual, placement: &Transform) -> WobbleLevers {
    let mut tail = None;
    let mut wings = [None; 2];
    for node in &animal.data.nodes {
        let lever = placement.rotation * node.lever();
        match node.part {
            Some(AnimalPart::Tail) => tail = Some(lever),
            Some(AnimalPart::LeftWing) => wings[0] = Some(lever),
            Some(AnimalPart::RightWing) => wings[1] = Some(lever),
            _ => {}
        }
    }
    WobbleLevers {
        tail,
        wings: wings
            .iter()
            .any(Option::is_some)
            .then(|| [wings[0].unwrap_or(Vec3::NEG_X), wings[1].unwrap_or(Vec3::X)]),
    }
}

/// Issue #101: `animal` on its legs under `parent` (a walker of the lobby, its front toward
/// -Z), placed by `placement` (turned toward -Z, scaled, its feet where the walker's are), with
/// `bundle` on its root. Its nodes and its legs are animated by its root's `AnimationPlayer`
/// (its graph: idle at first, from a moment of its own given by `phase`, 0 to 1); its body,
/// tail and wings get their wobble's layer, moved by the [`DriverWobble`] given to `wobbler`
/// (with where they hang). Returns its root, and its nodes (in `AnimalModelData::nodes`' order).
pub fn spawn_animal_on_foot(
    commands: &mut Commands,
    animal: &AnimalVisual,
    placement: Transform,
    parent: Entity,
    wobbler: Entity,
    phase: f32,
    bundle: impl Bundle,
) -> (Entity, Vec<Entity>) {
    let to_file = placement.rotation.inverse();
    commands
        .entity(wobbler)
        .insert(DriverWobble(WobbleState::new(levers(animal, &placement))));
    let mut root = commands.spawn((placement, Visibility::default(), ChildOf(parent), bundle));
    if let Some((graph, nodes)) = &animal.graph {
        let mut player = AnimationPlayer::default();
        let mut transitions = AnimationTransitions::new();
        transitions
            .play(
                &mut player,
                nodes[AnimalAnimation::Idle.index()],
                Duration::ZERO,
            )
            .seek_to(phase.clamp(0.0, 1.0) * animal.idle_seconds)
            .repeat();
        root.insert((player, transitions, AnimationGraphHandle(graph.clone())));
    }
    let root = root.id();
    let mut spawned: Vec<Entity> = Vec::with_capacity(animal.data.nodes.len());
    for (i, node) in animal.data.nodes.iter().enumerate() {
        let parent = node.parent.map_or(root, |p| spawned[p]);
        let mut entity = commands.spawn((
            node.local,
            Visibility::default(),
            animal.targets[i],
            AnimatedBy(root),
            ChildOf(parent),
        ));
        if let Some(mesh) = &animal.meshes[i] {
            entity.insert((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(animal.material.clone()),
            ));
        }
        if let Some(part) = node.part {
            entity.insert((
                part,
                WobbleLayer::new(wobbler, part.into(), LayerPivot::Own, to_file),
            ));
        }
        spawned.push(entity.id());
    }
    for (i, leg) in animal.data.legs.iter().enumerate() {
        let parent = leg.parent.map_or(root, |p| spawned[p]);
        let mut entity = commands.spawn((
            leg.local,
            Visibility::default(),
            animal.leg_targets[i],
            AnimatedBy(root),
            ChildOf(parent),
        ));
        if let Some(mesh) = &animal.leg_meshes[i] {
            entity.insert((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(animal.material.clone()),
            ));
        }
    }
    (root, spawned)
}

/// Issue #98: `animal` standing still at `transform` (no clip, no wobble: the portraits of the
/// « Pilote » screen, rendered once), its nodes' meshes in the models' shared material, `bundle`
/// on its root. Returns its root.
pub fn spawn_animal_still(
    commands: &mut Commands,
    animal: &AnimalVisual,
    transform: Transform,
    bundle: impl Bundle,
) -> Entity {
    let root = commands
        .spawn((transform, Visibility::default(), bundle))
        .id();
    let mut spawned: Vec<Entity> = Vec::with_capacity(animal.data.nodes.len());
    for (i, node) in animal.data.nodes.iter().enumerate() {
        let parent = node.parent.map_or(root, |p| spawned[p]);
        let mut entity = commands.spawn((node.local, Visibility::default(), ChildOf(parent)));
        if let Some(mesh) = &animal.meshes[i] {
            entity.insert((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(animal.material.clone()),
            ));
        }
        spawned.push(entity.id());
    }
    root
}

/// What each driver plays, from what its kart is going through (`animation.rs`): a new
/// animation fades in when it changes. Nothing allocated: a dozen comparisons a frame.
fn animate_drivers(
    tuning: Res<DriverTuning>,
    race: Option<Res<Race>>,
    karts: Query<(
        &KartState,
        Option<&Paw>,
        Option<&RaceProgress>,
        Option<&OnPodium>,
    )>,
    mut drivers: Query<(
        &mut AnimalRoot,
        &mut AnimationPlayer,
        &mut AnimationTransitions,
    )>,
) {
    let race_ticks = race.as_ref().map_or(0, |race| race.race_ticks());
    for (mut root, mut player, mut transitions) in &mut drivers {
        let (Some(nodes), Ok((state, paw, progress, podium))) = (root.nodes, karts.get(root.kart))
        else {
            continue;
        };
        let finish = progress.and_then(|p| p.finish).and_then(|finish| {
            let place = race
                .as_ref()?
                .standings
                .iter()
                .position(|e| *e == root.kart)?;
            Some(Finish {
                place,
                ticks_ago: race_ticks.saturating_sub(finish),
            })
        });
        let facts = DriverFacts {
            hit: matches!(state, KartState::Hit { .. }),
            struck_ticks: paw
                .filter(|paw| paw.struck_count > 0)
                .map(|paw| paw.struck_ticks),
            finish,
            podium: podium.map(|podium| podium.0),
            racers: race.as_ref().map_or(0, |race| race.standings.len()),
        };
        let wanted = driver_animation(&facts, &tuning);
        if wanted == root.playing {
            continue;
        }
        root.playing = wanted;
        let fade = Duration::from_secs_f32(tuning.crossfade_seconds.max(0.01));
        transitions
            .play(&mut player, nodes[wanted.index()], fade)
            .set_speed(wanted.speed(&tuning))
            .repeat();
    }
}
