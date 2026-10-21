//! Issue #144 (GD « Conception des circuits », step 2): the hazards of a circuit in the game,
//! their rules (`super::hazard` is their pure geometry and motion).
//!
//! - **Spawned with the circuit** ([`spawn_hazards`], from `circuit::build_circuit_when_loaded`):
//!   one entity per hazard ([`HazardBody`], its number in `TrackLayout::hazards`). A fixed
//!   obstacle is a static collider (`Surface::Wall`: the karts bump into it like into a wall); a
//!   moving one and a passer-by have no collider (their pose follows the race clock, with a
//!   `PhysicsPose` for the interpolation of their look); a thing to drive through has its state
//!   ([`Breakable`]).
//! - **Their systems** ([`HazardSystems`], in `FixedUpdate`, after the items and the paws, before
//!   the croquettes and the driving): the moving hazards placed for the tick (the race clock and
//!   the race seed: the same on every peer); our karts that touch a moving one knocked over (a
//!   tumble: `KartState::Hit`, its drift and boost lost, `CroquetteTuning::knock_loss`
//!   croquettes, a push away from it, then its invincibility: the victim decides, online too: its
//!   state goes to the other peers like an item's tumble); the things to drive through burst
//!   under any kart, ours slowed down a little, and come back.
//! - **Only on a circuit that has hazards**: the systems are added to the schedule when such a
//!   circuit is built ([`install_hazard_systems`], once), never before. A system more in the
//!   schedule changes the order Bevy gives to the others, and an entity, a component or a resource
//!   more before the grid changes the order of the contacts in Rapier: the bot races of every
//!   circuit would change (`STATUS.md`, « Comment ajouter une vérification de course »). A circuit
//!   without hazards has none of them: its world is exactly the one of before #144.
//! - **Their look** (with a renderer only, [`HazardVisualPlugin`], `super::hazard_visual`): the
//!   models in primitives, the passers-by walking, the bursts' effects (`crate::vfx`) and sounds
//!   (`crate::audio`).
//! - **Issue #156: the covers of the shortcuts.** A **panel** ([`Panel`]) is a static collider
//!   across its shortcut's path (`Surface::Wall`, a softer bounce than a wall's:
//!   `HazardTuning::panel_bounce`) and its state: one of our karts with a boost going that comes
//!   to it breaks it ([`break_panels`]: its collider off, splinters and a sound through its
//!   [`Breakable`], like a crate's), and it stands again `HazardTuning::panel_respawn_seconds`
//!   later, on a tick of the hazards' clock. Online, the kart's peer decides and tells
//!   (`net::PanelDown`); each peer has it down until the latest tick it was told: the same on
//!   every peer. Its system is added only with the first circuit that has a panel
//!   ([`install_panel_systems`]): the circuits with hazards but no panel keep their schedule. A
//!   **curtain of foliage** is only seen (no collider, no system): one drives through it.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    Collider, ColliderDisabled, Friction, Restitution, RigidBody, Velocity,
};

use super::data::{BreakableKind, CoverKind, ObstacleKind};
use super::hazard::{
    Capsule, Hazard, HazardKind, Shape, clock_seconds, hazard_phase, kart_capsule, push_direction,
};
use super::{Surface, TrackLayout, WALL_RESTITUTION};
use crate::TICK_HZ;
use crate::croquettes::{CroquetteSystems, LoseCroquettes, LossCause};
use crate::interactions::InteractionSystems;
use crate::interpolation::PhysicsPose;
use crate::items::{ItemSystems, immune_to_items};
use crate::kart::{Boost, HitReaction, Invincibility, Kart, KartState, KartSystems, Remote};
use crate::level::LevelScoped;
use crate::net::{KartSync, NetKart, OnlineItems};
use crate::race::{HazardTuning, Race, RaceGrid, RaceProgress, RaceSystems, RaceTuning};

pub use super::hazard_visual::HazardVisualPlugin;

/// A hazard of the circuit: its number in `TrackLayout::hazards`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HazardBody(pub u16);

/// A thing to drive through: there when `respawn_in` is 0.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Breakable {
    /// Seconds before it comes back.
    pub respawn_in: f32,
    /// It burst on this tick, under this kart (what the effects and the sounds read).
    pub burst_by: Option<Entity>,
    /// Times it burst since the circuit was built.
    pub bursts: u32,
}

impl Breakable {
    pub fn available(&self) -> bool {
        self.respawn_in <= 0.0
    }
}

/// The hazards' systems (`FixedUpdate`), on a circuit that has hazards only.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HazardSystems;

/// The hazards' systems are in the schedule (from the first circuit with hazards on).
#[derive(Resource, Debug)]
pub struct HazardSystemsInstalled;

/// Issue #156: the panels' system is in the schedule (from the first circuit with a panel on).
#[derive(Resource, Debug)]
pub struct PanelSystemsInstalled;

/// Issue #156: a shortcut's panel: down (broken) until tick `down_until` of the hazards' clock
/// ([`hazard_tick`]); its collider is off while it is down (`ColliderDisabled`: out of the
/// contacts and of the rays, the camera's and the items' too), and until none of our karts is in
/// it once it stands again (it never throws a kart out of it).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Panel {
    pub down_until: u32,
    /// Its collider is on (it blocks the karts).
    pub solid: bool,
}

/// Spawns the hazards of `layout` (nothing at all without any), and has their systems added to
/// the schedule.
pub fn spawn_hazards(commands: &mut Commands, layout: &TrackLayout) {
    if layout.hazards.is_empty() {
        return;
    }
    let mut panels = false;
    for hazard in &layout.hazards {
        let name = Name::new(format!("Hazard {}: {:?}", hazard.index, hazard.kind));
        let mut entity = commands.spawn((LevelScoped, HazardBody(hazard.index), name));
        let pose = hazard.pose(0.0, 0.0);
        if hazard.kind.stands() {
            // The collider's center, over the foot.
            let shape = hazard.shape();
            let center = pose
                .with_translation(pose.translation + hazard.road.up * shape.height * hazard.size);
            let size = hazard.size;
            let collider = match hazard.kind {
                HazardKind::Obstacle(ObstacleKind::Trunk) => {
                    Collider::capsule_z(shape.half_length * size, shape.radius * size)
                }
                HazardKind::Obstacle(ObstacleKind::Pillar) => {
                    Collider::cylinder(shape.half_length * size, shape.radius * size)
                }
                _ => Collider::cuboid(
                    shape.half_across * size,
                    shape.height * size,
                    shape.half_along * size,
                ),
            };
            entity.insert((
                center,
                RigidBody::Fixed,
                collider,
                Restitution::coefficient(WALL_RESTITUTION),
                Friction::coefficient(0.0),
                Surface::Wall,
            ));
        } else if hazard.kind.closes() {
            // Issue #156: a board across the path, standing; its bounce follows its tuning.
            let shape = hazard.shape();
            let center = pose.with_translation(pose.translation + hazard.road.up * shape.height);
            let half = Shape::cover_box(CoverKind::Panel);
            entity.insert((
                center,
                RigidBody::Fixed,
                Collider::cuboid(half.x, half.y, half.z),
                Restitution::coefficient(WALL_RESTITUTION),
                Friction::coefficient(0.0),
                Surface::Wall,
                Breakable::default(),
                Panel {
                    down_until: 0,
                    solid: true,
                },
            ));
            panels = true;
        } else if hazard.kind.moves() {
            entity.insert((pose, PhysicsPose::new(pose)));
        } else if hazard.kind.bursts() {
            entity.insert((pose, Breakable::default()));
        } else {
            // Issue #156: a curtain of foliage, only seen.
            entity.insert(pose);
        }
    }
    commands.queue(install_hazard_systems);
    if panels {
        commands.queue(install_panel_systems);
    }
}

/// Adds the hazards' systems to `FixedUpdate`, once (see the module's documentation).
pub fn install_hazard_systems(world: &mut World) {
    if world.contains_resource::<HazardSystemsInstalled>() {
        return;
    }
    world.insert_resource(HazardSystemsInstalled);
    world.schedule_scope(FixedUpdate, |_, schedule| {
        schedule.configure_sets(
            HazardSystems
                .after(KartSystems::Input)
                .after(RaceSystems::Gate)
                .after(ItemSystems)
                .after(InteractionSystems)
                .before(CroquetteSystems)
                .before(KartSystems::Drive)
                // Between two levels (a reload), no circuit.
                .run_if(resource_exists::<TrackLayout>),
        );
        schedule.add_systems(
            (move_hazards, knock_karts, burst_breakables)
                .chain()
                .in_set(HazardSystems),
        );
    });
}

/// Issue #156: adds the panels' system to `FixedUpdate`, once (after the things to drive
/// through, in [`HazardSystems`]; see the module's documentation).
pub fn install_panel_systems(world: &mut World) {
    if world.contains_resource::<PanelSystemsInstalled>() {
        return;
    }
    world.insert_resource(PanelSystemsInstalled);
    world.schedule_scope(FixedUpdate, |_, schedule| {
        schedule.add_systems(break_panels.in_set(HazardSystems).after(burst_breakables));
    });
}

/// The race clock (s): the race's flow tick (online, the room's: the same on every peer); out
/// of a race (free driving), the room's common tick online, the simulation's own solo.
pub fn hazard_clock(race: Option<&Race>, sync: Option<&KartSync>, fixed: &Time<Fixed>) -> f32 {
    clock_seconds(hazard_tick(race, sync, fixed))
}

/// The race clock's tick (see [`hazard_clock`]).
pub fn hazard_tick(race: Option<&Race>, sync: Option<&KartSync>, fixed: &Time<Fixed>) -> u32 {
    if let Some(race) = race {
        return race.flow.tick;
    }
    let local = (fixed.elapsed_secs_f64() * TICK_HZ).round() as u32;
    sync.and_then(|sync| sync.clock.common(local))
        .unwrap_or(local)
}

/// The moving hazards where the race clock puts them.
fn move_hazards(
    race: Option<Res<Race>>,
    sync: Option<Res<KartSync>>,
    fixed: Res<Time<Fixed>>,
    grid: Res<RaceGrid>,
    layout: Res<TrackLayout>,
    mut bodies: Query<(&HazardBody, &mut Transform), With<PhysicsPose>>,
) {
    let seconds = hazard_clock(race.as_deref(), sync.as_deref(), &fixed);
    for (body, mut transform) in &mut bodies {
        let Some(hazard) = layout.hazards.get(usize::from(body.0)) else {
            continue;
        };
        *transform = hazard.pose(seconds, hazard_phase(grid.seed, hazard.index));
    }
}

/// Our karts that touch a moving obstacle or a passer-by are knocked over (the victim decides:
/// an immune kart, tumbling, carried by the pelican, invincible or past the finish, is not).
#[allow(clippy::type_complexity)]
fn knock_karts(
    tuning: Res<RaceTuning>,
    layout: Res<TrackLayout>,
    bodies: Query<(&HazardBody, &Transform), With<PhysicsPose>>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &mut KartState,
            &mut Boost,
            &mut Velocity,
            &Invincibility,
            Option<&RaceProgress>,
        ),
        (With<Kart>, Without<HazardBody>),
    >,
    mut losses: MessageWriter<LoseCroquettes>,
) {
    let tuning = &tuning.hazards;
    for (kart, transform, mut state, mut boost, mut velocity, invincibility, progress) in &mut karts
    {
        if immune_to_items(
            &state,
            invincibility,
            progress.is_some_and(|p| p.finished()),
        ) {
            continue;
        }
        let shape = kart_capsule(transform);
        let touched = bodies.iter().find_map(|(body, pose)| {
            let hazard = layout.hazards.get(usize::from(body.0))?;
            let (gap, on_hazard, on_kart) = hazard.body(pose).gap(&shape);
            (gap <= tuning.contact_margin).then_some((hazard, on_hazard, on_kart))
        });
        let Some((hazard, on_hazard, on_kart)) = touched else {
            continue;
        };
        knock(
            tuning,
            hazard,
            (on_hazard, on_kart),
            (&mut state, &mut boost, &mut velocity),
        );
        losses.write(LoseCroquettes {
            kart,
            cause: LossCause::Knocked,
        });
    }
}

/// Knocks a kart over (see [`knock_karts`]): a tumble, its drift and boost gone, most of its
/// speed lost, a push away from `hazard` (from the hazard's point closest to it toward the kart's,
/// along the road).
pub fn knock(
    tuning: &HazardTuning,
    hazard: &Hazard,
    (on_hazard, on_kart): (Vec3, Vec3),
    (state, boost, velocity): (&mut KartState, &mut Boost, &mut Velocity),
) {
    *state = KartState::hit(HitReaction::Tumble, tuning.knock_seconds, TICK_HZ as f32);
    *boost = Boost::default();
    let up = hazard.road.up;
    let linear = velocity.linear;
    let vertical = up * linear.dot(up);
    velocity.linear = (linear - vertical) * tuning.knock_speed_kept
        + vertical
        + push_direction(on_hazard, on_kart, up) * tuning.knock_push;
}

/// What bursting a thing to drive through costs a kart: the share of its speed it loses.
pub fn slowdown(tuning: &HazardTuning, kind: BreakableKind) -> f32 {
    match kind {
        BreakableKind::HayBale => tuning.hay_slowdown,
        BreakableKind::Crate => tuning.crate_slowdown,
        BreakableKind::Balloon => tuning.balloon_slowdown,
    }
    .clamp(0.0, 1.0)
}

/// The things to drive through: one there bursts under the first kart that touches it (ours or
/// another peer's: each peer bursts what it sees, the effect is ours only), ours slowed down a
/// little; it comes back after `HazardTuning::respawn_seconds`. All back when a race (re)starts.
#[allow(clippy::type_complexity)]
fn burst_breakables(
    time: Res<Time>,
    tuning: Res<RaceTuning>,
    layout: Res<TrackLayout>,
    race: Option<Res<Race>>,
    mut breakables: Query<(&HazardBody, &Transform, &mut Breakable)>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &KartState,
            Option<&mut Velocity>,
            Has<Kart>,
        ),
        (Or<(With<Kart>, With<Remote>)>, Without<HazardBody>),
    >,
) {
    let dt = time.delta_secs();
    let restart = race.is_some_and(|race| race.flow.tick == 1);
    let tuning = &tuning.hazards;
    for (body, transform, mut breakable) in &mut breakables {
        if breakable.burst_by.is_some() {
            breakable.burst_by = None;
        }
        if restart && !breakable.available() {
            breakable.respawn_in = 0.0;
        }
        if !breakable.available() {
            // Half a tick of tolerance, like the item boxes.
            breakable.respawn_in -= dt;
            if breakable.respawn_in <= dt * 0.5 {
                breakable.respawn_in = 0.0;
            }
            continue;
        }
        let Some(hazard) = layout.hazards.get(usize::from(body.0)) else {
            continue;
        };
        // Issue #110: a shortcut's stack of crates bursts like a crate.
        let Some(kind) = hazard.kind.burst_kind().filter(|_| hazard.kind.bursts()) else {
            continue;
        };
        let shape = hazard.body(transform);
        for (kart, kart_transform, state, velocity, ours) in &mut karts {
            if *state == KartState::Respawning || !kart_capsule(kart_transform).touches(&shape) {
                continue;
            }
            breakable.respawn_in = tuning.respawn_seconds.max(f32::MIN_POSITIVE);
            breakable.bursts += 1;
            breakable.burst_by = Some(kart);
            if ours && let Some(mut velocity) = velocity {
                velocity.linear *= 1.0 - slowdown(tuning, kind);
            }
            break;
        }
    }
}

/// Issue #156: how many ticks a panel stays down.
pub fn panel_down_ticks(tuning: &HazardTuning) -> u32 {
    (tuning.panel_respawn_seconds.max(0.0) * TICK_HZ as f32)
        .round()
        .max(1.0) as u32
}

/// Issue #156: whether a kart's contact shape `kart` reaches the box of a panel (half extents
/// `half` around `center`, turned by `rotation`), with `margin` more: a few points along the
/// kart's segment against the box.
pub fn reaches_box(kart: &Capsule, center: Vec3, rotation: Quat, half: Vec3, margin: f32) -> bool {
    let inverse = rotation.inverse();
    (0..=4).any(|k| {
        let point = kart.a.lerp(kart.b, k as f32 / 4.0);
        let local = inverse * (point - center);
        let outside = (local.abs() - half).max(Vec3::ZERO);
        outside.length() <= kart.radius + margin
    })
}

/// Issue #156: the shortcuts' panels. Online, the breaks the other peers told (the latest one
/// counts: down until its tick plus the respawn time); then one of our karts with a boost going
/// that comes to a panel standing breaks it (its contact shape stretched by two ticks of its
/// way: it never bumps into it), and we tell. A panel down has its collider off; standing again,
/// it is solid once none of our karts is in it. All stand when a race (re)starts.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn break_panels(
    mut commands: Commands,
    tuning: Res<RaceTuning>,
    layout: Res<TrackLayout>,
    (race, sync, fixed): (Option<Res<Race>>, Option<Res<KartSync>>, Res<Time<Fixed>>),
    mut online: Option<ResMut<OnlineItems>>,
    mut panels: Query<(
        Entity,
        &HazardBody,
        &Transform,
        &mut Panel,
        &mut Breakable,
        &mut Restitution,
    )>,
    karts: Query<
        (
            Entity,
            &Transform,
            &Velocity,
            &Boost,
            &KartState,
            Option<&NetKart>,
        ),
        (With<Kart>, Without<HazardBody>),
    >,
) {
    let tuning = &tuning.hazards;
    let now = hazard_tick(race.as_deref(), sync.as_deref(), &fixed);
    let restart = race.as_ref().is_some_and(|race| race.flow.tick == 1);
    let down_for = panel_down_ticks(tuning);
    let told = online
        .as_mut()
        .map(|online| online.take_panels())
        .unwrap_or_default();
    let dt = 1.0 / TICK_HZ as f32;
    let half = Shape::cover_box(CoverKind::Panel);
    for (entity, body, transform, mut panel, mut breakable, mut restitution) in &mut panels {
        let Some(hazard) = layout.hazards.get(usize::from(body.0)) else {
            continue;
        };
        if restitution.coefficient != tuning.panel_bounce {
            restitution.coefficient = tuning.panel_bounce;
        }
        if restart {
            panel.down_until = 0;
        }
        breakable.burst_by = None;
        let was_down = now < panel.down_until;
        for (index, tick, kart) in &told {
            if *index == body.0 {
                panel.down_until = panel.down_until.max(tick.saturating_add(down_for));
                if !was_down && now < panel.down_until && breakable.burst_by.is_none() {
                    breakable.burst_by = Some(kart.unwrap_or(entity));
                    breakable.bursts += 1;
                }
            }
        }
        if now >= panel.down_until {
            let contact = hazard.body(&hazard.pose(0.0, 0.0));
            let breaker = karts.iter().find(|(_, kart, velocity, boost, state, _)| {
                if !boost.is_active() || **state == KartState::Respawning {
                    return false;
                }
                let mut shape = kart_capsule(kart);
                shape.b += velocity.linear * dt * 2.0;
                contact.gap(&shape).0 <= tuning.panel_reach
            });
            if let Some((kart, _, _, _, _, net)) = breaker {
                panel.down_until = now.saturating_add(down_for);
                breakable.burst_by = Some(kart);
                breakable.bursts += 1;
                if let (Some(online), Some(net)) = (online.as_mut(), net) {
                    online.panel_down(net.0, body.0, now);
                }
            }
        }
        let down = now < panel.down_until;
        breakable.respawn_in = if down {
            (panel.down_until - now) as f32 * dt
        } else {
            0.0
        };
        // Solid when it stands and none of our karts is in it.
        let solid = !down
            && (panel.solid
                || !karts.iter().any(|(_, kart, ..)| {
                    reaches_box(
                        &kart_capsule(kart),
                        transform.translation,
                        transform.rotation,
                        half,
                        0.05,
                    )
                }));
        if solid != panel.solid {
            panel.solid = solid;
            if solid {
                commands.entity(entity).remove::<ColliderDisabled>();
            } else {
                commands.entity(entity).insert(ColliderDisabled);
            }
        }
    }
}
