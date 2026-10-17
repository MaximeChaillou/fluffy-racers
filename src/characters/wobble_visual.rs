//! The soft characters on screen (issue #12): each driver's wobble (`wobble.rs`), stepped every
//! frame from what its kart shows, then laid over its parts.
//!
//! - **The parts**: a Cube Pet's `body` (lean and squash, around its bottom on the seat), its
//!   `tail` and a bird's wings (their own springs, around their roots): marked [`AnimalPart`] by
//!   `model.rs`, they get a [`WobbleLayer`] when the animal sits in its kart (`visual.rs`). A driver
//!   still in primitives (headless, or a missing file) leans and squashes as a whole: each of its
//!   primitives (ball, eyes, ears) gets a layer around the seat.
//! - **Over the clip, never instead**: the clips write the animal's nodes each frame
//!   (`AnimationSystems`, `PostUpdate`); the layer runs after them, takes what they wrote as its
//!   base and multiplies the wobble over it (a field the clip did not write this frame keeps the
//!   last base: nothing piles up). The dance at the finish stays the dance, leaning.
//! - **Inputs**: the kart's visual (its interpolated pose, `crate::interpolation`; online, the
//!   other players' karts as `net` shows them), its `KartState` (hit by an item) and its `Paw`
//!   (struck): what every peer already has. Nothing here touches the simulation, nothing is sent.
//!
//! **Fluidity**: per frame, a dozen drivers each step five small springs (at most 8 substeps at
//! 30 fps) and lay them over 1 to 5 parts. No allocation, no entity spawned: the components are
//! added once per driver and part.

use bevy::app::AnimationSystems;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::model::AnimalPart;
use super::wobble::{LayerPivot, WobbleInput, WobblePose, WobbleState, WobbleTuning, layered};
use crate::interactions::Paw;
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{DriverPrimitive, DriverVisual, KartBodyPivot, KartState};

/// A driver's wobble (on its `kart::DriverVisual`): its springs and its pose of this frame.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct DriverWobble(pub WobbleState);

/// Which of its driver's springs moves a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WobbleSlot {
    /// The lean and the squash.
    Body,
    Tail,
    LeftWing,
    RightWing,
}

impl From<AnimalPart> for WobbleSlot {
    fn from(part: AnimalPart) -> Self {
        match part {
            AnimalPart::Body => WobbleSlot::Body,
            AnimalPart::Tail => WobbleSlot::Tail,
            AnimalPart::LeftWing => WobbleSlot::LeftWing,
            AnimalPart::RightWing => WobbleSlot::RightWing,
        }
    }
}

/// On a part the wobble is laid over: its driver, its spring, around what it turns, and its
/// pose without the wobble.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WobbleLayer {
    /// The driver (`kart::DriverVisual`) whose [`DriverWobble`] moves it.
    pub driver: Entity,
    pub slot: WobbleSlot,
    pub pivot: LayerPivot,
    /// From the driver's frame (its kart's) to the frame of the part's parent: an animal is
    /// turned to face the kart's front (`AnimalModelData::placement`).
    pub frame: Quat,
    /// Its pose without the wobble: its rest, or what its clip wrote this frame.
    pub base: Transform,
    /// What the wobble wrote last: a field that is not that any more was written by another
    /// (the clip), and is the new base.
    written: Transform,
}

impl WobbleLayer {
    pub fn new(driver: Entity, slot: WobbleSlot, pivot: LayerPivot, frame: Quat) -> Self {
        let never = Transform {
            translation: Vec3::NAN,
            rotation: Quat::from_xyzw(f32::NAN, f32::NAN, f32::NAN, f32::NAN),
            scale: Vec3::NAN,
        };
        Self {
            driver,
            slot,
            pivot,
            frame,
            base: Transform::IDENTITY,
            written: never,
        }
    }

    /// Takes in the part's pose of this frame: what another system wrote since the last frame
    /// (its clip) is its base now, field by field; what still is the wobble's keeps the base.
    pub fn adopt(&mut self, current: &Transform) {
        if current.translation != self.written.translation {
            self.base.translation = current.translation;
        }
        if current.rotation != self.written.rotation {
            self.base.rotation = current.rotation;
        }
        if current.scale != self.written.scale {
            self.base.scale = current.scale;
        }
    }

    /// Its pose with `pose` laid over its base.
    pub fn over(&self, pose: &WobblePose) -> Transform {
        let (rotation, scale) = match self.slot {
            WobbleSlot::Body => (pose.lean, pose.squash),
            WobbleSlot::Tail => (pose.tail, Vec3::ONE),
            WobbleSlot::LeftWing => (pose.wings[0], Vec3::ONE),
            WobbleSlot::RightWing => (pose.wings[1], Vec3::ONE),
        };
        let rotation = self.frame * rotation * self.frame.inverse();
        layered(&self.base, self.pivot, rotation, scale)
    }

    /// Remembers what it wrote.
    fn wrote(&mut self, transform: Transform) {
        self.written = transform;
    }
}

pub struct WobblePlugin;

impl Plugin for WobblePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WobbleTuning>().add_systems(
            PostUpdate,
            (layer_primitives, step_wobbles, lay_wobbles)
                .chain()
                // The karts' visuals placed for this frame, the clips played: then the wobble,
                // before the transforms are propagated.
                .after(InterpolationSystems)
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        );
    }
}

/// A driver still in primitives leans and squashes as a whole: each primitive gets a layer around
/// the seat (the driver's origin). Once per primitive.
fn layer_primitives(
    mut commands: Commands,
    primitives: Query<(Entity, &ChildOf), (With<DriverPrimitive>, Without<WobbleLayer>)>,
) {
    for (primitive, child_of) in &primitives {
        commands.entity(primitive).try_insert(WobbleLayer::new(
            child_of.parent(),
            WobbleSlot::Body,
            LayerPivot::Parent,
            Quat::IDENTITY,
        ));
    }
}

/// Each driver's wobble moves on by this frame, from its kart's visual (where it is shown), its
/// state and its paw. A new driver gets its wobble (an animal has it with its levers already,
/// `visual.rs`).
fn step_wobbles(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<WobbleTuning>,
    new: Query<Entity, (With<DriverVisual>, Without<DriverWobble>)>,
    mut drivers: Query<(&mut DriverWobble, &ChildOf)>,
    pivots: Query<(&KartBodyPivot, &ChildOf)>,
    roots: Query<&Transform, With<InterpolatedFrom>>,
    karts: Query<(&KartState, Option<&Paw>)>,
) {
    for driver in &new {
        commands
            .entity(driver)
            .try_insert_if_new(DriverWobble::default());
    }
    let dt = time.delta_secs();
    for (mut wobble, child_of) in &mut drivers {
        let Ok((pivot, pivot_of)) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(root) = roots.get(pivot_of.parent()) else {
            continue;
        };
        let (hit, struck) = karts.get(pivot.body).map_or((None, None), |(state, paw)| {
            let hit = match *state {
                KartState::Hit { reaction, .. } => Some(reaction),
                _ => None,
            };
            (hit, paw.map(|paw| (paw.struck_count, paw.struck_side)))
        });
        let input = WobbleInput {
            dt,
            position: root.translation,
            rotation: root.rotation,
            hit,
            struck,
        };
        wobble.0.step(&input, &tuning);
    }
}

/// Lays each driver's pose over its parts: over what their clip wrote this frame, or their rest.
pub(crate) fn lay_wobbles(
    drivers: Query<&DriverWobble>,
    mut layers: Query<(&mut WobbleLayer, &mut Transform)>,
) {
    for (mut layer, mut transform) in &mut layers {
        let Ok(wobble) = drivers.get(layer.driver) else {
            continue;
        };
        layer.adopt(&transform);
        let next = layer.over(&wobble.0.pose);
        transform.set_if_neq(next);
        layer.wrote(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer takes what the clip writes as its base, field by field, and never piles the
    /// wobble up over its own output: a field nobody rewrote keeps the base it had.
    #[test]
    fn a_layer_follows_the_clip_and_never_piles_up() {
        let mut layer = WobbleLayer::new(
            Entity::PLACEHOLDER,
            WobbleSlot::Body,
            LayerPivot::Own,
            Quat::from_rotation_y(std::f32::consts::PI).inverse(),
        );
        let pose = WobblePose {
            lean: Quat::from_rotation_x(0.2),
            squash: Vec3::new(1.1, 0.8, 1.1),
            ..default()
        };
        // The clip's pose of the first frame.
        let clip = Transform::from_xyz(0.0, 0.18, 0.0).with_rotation(Quat::from_rotation_z(0.1));
        let mut transform = clip;
        for _ in 0..3 {
            layer.adopt(&transform);
            transform = layer.over(&pose);
            layer.wrote(transform);
        }
        // Three frames without the clip (no animation player): the same, not three times.
        assert_eq!(layer.base, clip);
        assert_eq!(transform, layer.over(&pose));
        assert!((transform.scale.y - 0.8).abs() < 1e-6);
        // The lean is turned into the animal's frame: about the kart's X is about the file's -X.
        let expected = Quat::from_rotation_x(-0.2) * clip.rotation;
        assert!(transform.rotation.angle_between(expected) < 1e-5);
        // The clip writes a new rotation only: it is the base, the scale stays the base's.
        let turned = Quat::from_rotation_z(-0.3);
        transform.rotation = turned;
        layer.adopt(&transform);
        assert_eq!(layer.base.rotation, turned);
        assert_eq!(layer.base.scale, Vec3::ONE);
        assert_eq!(layer.base.translation, clip.translation);
        // At rest, the clip's pose itself.
        let rest = layer.over(&WobblePose::default());
        assert!(rest.rotation.angle_between(turned) < 1e-6);
        assert_eq!(rest.scale, Vec3::ONE);
    }
}
