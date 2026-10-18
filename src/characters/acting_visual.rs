//! Issue #130: the karts' bodies and their drivers act on screen (`acting.rs`), **only with a
//! renderer** ([`ActingPlugin`]: the bot races' world has nothing of it).
//!
//! Each frame, for every kart's driver ([`Acting`], on its `kart::DriverVisual`, made the first
//! time it is seen):
//!
//! - **the body**: its springs feel the kart's visual (its pose on screen: interpolated, or
//!   placed by the network for another peer's kart), and tilt and squash its shell
//!   (`kart::BodyPaint`, whose `Transform` nobody else writes) around the bottom of the body;
//! - **the driver**: its figure (`KartState::Trick` starts one), its look back (overtaken: its
//!   race rank dropped, the kart now just ahead of it close by; or one of its items out behind
//!   it, `items::ItemObject` just spawned), its sulk (over the line among the last,
//!   `animation::sulks`), its dizziness (the stars of #105, `vfx::KartVfx::dizzy`);
//! - **laid over the driver's pose, never instead**: what the others write on the driver (its
//!   seat and size, its turn toward a grabber or its lean off balance of #34) is its base, taken
//!   in field by field ([`acting::adopt`]); the body's pose carries its seat, the acting goes on
//!   top. The wobble (#12) and the clips (#69) move the driver's parts, under it: they add up.
//!
//! **Fluidity**: a dozen drivers each step two small springs and a few timers, and write two
//! transforms; nothing allocated, nothing spawned (`vfx::alloc_tests` counts it). **Purely
//! visual**: it reads the game, never writes to it. Online, everything comes from what the
//! other peers' karts already show (their state, their progress, their items): nothing is sent.

use std::f32::consts::PI;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::acting::{
    ActingFacts, BodyWobble, DriverActing, acted, adopt, shell_transform, yaw_toward,
};
use super::animation::{DriverTuning, sulks};
use super::wobble::WobbleTuning;
use crate::interpolation::InterpolationSystems;
use crate::items::ItemObject;
use crate::kart::{BodyPaint, DriverVisual, KartBodyPivot, KartState};
use crate::race::{OnPodium, Race, RaceProgress};
use crate::vfx::KartVfx;

/// A driver's acting (issue #130), on its `kart::DriverVisual`: its kart's body springs, its own
/// acting, and what it remembers.
#[derive(Component, Clone, Copy, Debug)]
pub struct Acting {
    /// The kart it drives (its simulated body, or the body of another peer's kart).
    pub kart: Entity,
    /// Its kart's shell, under the same pivot (`kart::BodyPaint`).
    pub shell: Option<Entity>,
    pub body: BodyWobble,
    pub driver: DriverActing,
    /// The kart that overtook it, while it looks at it.
    pub looking_at: Option<Entity>,
    /// Its rank on the last frame (0: not racing).
    rank: u32,
    /// The driver's pose without the acting (its seat, its size, what the others wrote), and
    /// what the acting wrote last.
    base: Transform,
    written: Transform,
}

impl Acting {
    pub fn new(kart: Entity, shell: Option<Entity>) -> Self {
        let never = Transform {
            translation: Vec3::NAN,
            rotation: Quat::from_xyzw(f32::NAN, f32::NAN, f32::NAN, f32::NAN),
            scale: Vec3::NAN,
        };
        Self {
            kart,
            shell,
            body: BodyWobble::default(),
            driver: DriverActing::default(),
            looking_at: None,
            rank: 0,
            base: Transform::IDENTITY,
            written: never,
        }
    }

    /// The driver's pose without the acting.
    pub fn base(&self) -> Transform {
        self.base
    }
}

/// The acting's system (each frame, the karts placed and the others' poses of the driver
/// written, before the transforms are propagated).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActingSystems;

/// The karts' bodies and their drivers act (see the module's documentation). Only with a
/// renderer.
pub struct ActingPlugin;

impl Plugin for ActingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, act.in_set(ActingSystems))
            .configure_sets(
                PostUpdate,
                ActingSystems
                    .after(InterpolationSystems)
                    // Over the turn and the lean of #34, never under them.
                    .after(crate::interactions::pose_drivers)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// Which way a kart's driver turns away when it sulks (−1 or 1): its own, from its entity.
fn side_of(kart: Entity) -> f32 {
    if (kart.to_bits() as u32).wrapping_mul(2_654_435_761) & (1 << 15) == 0 {
        1.0
    } else {
        -1.0
    }
}

/// Each driver acts this frame (see the module's documentation). A new driver gets its
/// [`Acting`] (only a kart's: not the « Pilote » screen's preview, which has no race state).
pub(crate) fn act(
    mut commands: Commands,
    time: Res<Time>,
    (wobble, tuning): (Res<WobbleTuning>, Res<DriverTuning>),
    race: Option<Res<Race>>,
    new: Query<(Entity, &ChildOf), (With<DriverVisual>, Without<Acting>)>,
    pivots: Query<(&KartBodyPivot, &ChildOf, &Children)>,
    shells: Query<(), With<BodyPaint>>,
    karts: Query<(
        &KartState,
        Option<&RaceProgress>,
        Option<&KartVfx>,
        Has<OnPodium>,
    )>,
    places: Query<&Transform, (Without<DriverVisual>, Without<BodyPaint>)>,
    items: Query<&ItemObject, Added<ItemObject>>,
    mut drivers: Query<
        (&mut Acting, &ChildOf, &mut Transform),
        (With<DriverVisual>, Without<BodyPaint>),
    >,
    mut bodies: Query<&mut Transform, (With<BodyPaint>, Without<DriverVisual>)>,
) {
    for (driver, child_of) in &new {
        let Ok((pivot, _, children)) = pivots.get(child_of.parent()) else {
            continue;
        };
        if !karts.contains(pivot.body) {
            continue;
        }
        let shell = children.iter().find(|child| shells.contains(*child));
        commands
            .entity(driver)
            .try_insert(Acting::new(pivot.body, shell));
    }
    let dt = time.delta_secs();
    let pivot = Vec3::Y * wobble.kart.pivot_height;
    let race = race.as_deref();
    let racing = race.is_some_and(Race::started);
    let racers = race.map_or(0, |race| race.standings.len());
    for (mut acting, child_of, mut transform) in &mut drivers {
        let acting = &mut *acting;
        let Ok((_, root_of, _)) = pivots.get(child_of.parent()) else {
            continue;
        };
        let Ok(root) = places.get(root_of.parent()) else {
            continue;
        };
        let Ok((state, progress, vfx, podium)) = karts.get(acting.kart) else {
            continue;
        };
        // The body, from where the kart is shown.
        acting.body.step(
            dt,
            root.translation,
            root.rotation,
            &wobble.kart,
            wobble.teleport_distance,
        );
        let kart = places.get(acting.kart).ok().copied();
        let finished = progress.is_some_and(|p| p.finish.is_some());
        // Overtaken: its rank dropped, the kart now just ahead of it close by.
        let rank = progress.map_or(0, |p| p.rank);
        let running = racing && !finished && *state != KartState::Respawning;
        if running
            && acting.rank > 0
            && rank > acting.rank
            && let (Some(race), Some(kart)) = (race, kart)
            && let Some(by) = (rank as usize)
                .checked_sub(2)
                .and_then(|i| race.standings.get(i))
            && let Ok(other) = places.get(*by)
            && other.translation.distance(kart.translation) <= tuning.look_back_range
        {
            acting.looking_at = Some(*by);
            let yaw = yaw_toward(kart.translation, kart.rotation, other.translation);
            acting
                .driver
                .look_back(yaw, tuning.look_back_seconds, &tuning);
        }
        acting.rank = if running { rank } else { 0 };
        if acting.driver.look <= 0.0 {
            acting.looking_at = None;
        }
        if let (Some(by), Some(kart)) = (acting.looking_at, kart)
            && let Ok(other) = places.get(by)
        {
            let yaw = yaw_toward(kart.translation, kart.rotation, other.translation);
            acting.driver.follow(yaw, &tuning);
        }
        // An item of its thrown or dropped behind it: a look over its shoulder, on its side.
        if let Some(kart) = kart {
            for item in &items {
                if item.owner != acting.kart || item.held().is_some() {
                    continue;
                }
                let to = kart.rotation.inverse() * (item.position - kart.translation);
                if to.z > 0.3 {
                    acting.looking_at = None;
                    let yaw = if to.x >= 0.0 { -PI } else { PI };
                    acting
                        .driver
                        .look_back(yaw, tuning.throw_look_seconds, &tuning);
                }
            }
        }
        let place = race.and_then(|race| race.standings.iter().position(|e| *e == acting.kart));
        let facts = ActingFacts {
            dt,
            tricking: *state == KartState::Trick,
            sulking: finished && !podium && place.is_some_and(|p| sulks(p, racers, &tuning)),
            dizzy: vfx.map_or(0.0, |vfx| vfx.dizzy),
            side: side_of(acting.kart),
        };
        acting.driver.step(&facts, &tuning);
        // The shell, then the driver over what the others gave it.
        if let Some(shell) = acting.shell
            && let Ok(mut shell) = bodies.get_mut(shell)
        {
            shell.set_if_neq(shell_transform(&acting.body.pose, pivot));
        }
        adopt(&mut acting.base, &acting.written, &transform);
        let next = acted(&acting.base, &acting.body.pose, pivot, &acting.driver.pose);
        transform.set_if_neq(next);
        acting.written = next;
    }
}
