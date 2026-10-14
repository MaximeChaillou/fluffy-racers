//! Items (GD § 8, issue #16): item boxes, the roulette, two slots, and the items of phase 1
//! (pelure, pelote de laine, abeille, piment, croquette dorée), with the MK8DX rules and our own
//! items.
//!
//! - **Boxes**: the circuit file places them (`item_boxes`, spawned by the track as
//!   `ItemBoxSpot`s, which become [`ItemBox`]es). A kart running into one breaks it and draws an
//!   item (two for a double box) from the table of its distance to the leader (`table.rs`,
//!   `assets/data/items.ron`); the box comes back about 2 s later. The draw is deterministic
//!   (race seed, grid slot, draws so far).
//! - **Slots** (`slots.rs`): hand and reserve, the roulette (about 1.5 s), the item button
//!   (`KartInput::item`: used on the press; a pelure or pelote is dragged behind while held and
//!   goes on the release; an "x3" comes out at the first press). Thrown backwards with the
//!   "back" direction held: the brake (Bas/S, B) or the look-back button (C).
//! - **Items out** ([`ItemObject`], `objects.rs`): a pelure lies on the ground; a pelote rolls
//!   straight and bounces off the walls; an abeille follows the track's centerline, then dives on
//!   the kart just ahead in the race. Held out (dragged, or an "x3" around the kart), they are
//!   shields: two items touching break each other. None is a Rapier body: rays against the track
//!   and swept contacts, a few dozen per tick even with 12 karts throwing.
//! - **Hits**: an item touching a kart breaks and tells the kart ([`ItemHit`]); the victim
//!   decides (docs/ARCHITECTURE.md: on the network, the victim's own peer): nothing during a spin
//!   or tumble, the pelican, after the finish or while invincible; otherwise a spin out (pelure,
//!   abeille: about 1 s) or a tumble (pelote: about 1.6 s) (`KartState::Hit`, its drift and boost
//!   lost, its items held out too), 3 croquettes lost (`LoseCroquettes`), then 1 to 2 s of
//!   invincibility with its stat (the kart starts it when the hit ends).
//! - **Piment**: a boost of about 1.2 s through the kart's `Boost`, like every boost (GD § 2).
//!   **Croquette dorée**: 2 croquettes (`Croquettes::gain`, up to 10) and a croquette's mini-boost.
//!
//! Everything runs in `FixedUpdate` at 60 Hz ([`ItemSystems`]): after the karts' input and the
//! race gate, before the croquettes and the driving. `ItemTuning::enabled` switches the items
//! off (no box, nothing held, nothing on the track): the bots' driving tests race without items.
//! Visuals in `visual.rs` ([`ItemVisualPlugin`]); the HUD's two slots in `crate::ui`; the bots'
//! use in `crate::bots`.

mod objects;
mod slots;
mod table;
mod tuning;
mod visual;

#[cfg(test)]
mod scenario_tests;

pub use objects::{
    Hold, KART_RIDE_HEIGHT, Roll, RollStep, WALL_NORMAL_Y, held_offset, roll, segment_box,
    segment_sphere,
};
pub use slots::{ItemSlots, Slot, Use};
pub use table::{
    DrawRow, ITEMS_RON, InvalidTable, ItemKind, ItemTable, Prize, TableFile, TableRow, draw_unit,
};
pub use tuning::ItemTuning;
pub use visual::{ItemVisualAssets, ItemVisualPlugin};

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext};

use crate::TICK_HZ;
use crate::croquettes::{
    CroquettePickedUp, CroquetteSystems, CroquetteTuning, Croquettes, LoseCroquettes, LossCause,
    touches,
};
use crate::kart::{
    Boost, HitReaction, Invincibility, KART_HALF_EXTENTS, Kart, KartInput, KartState, KartSystems,
};
use crate::level::LevelScoped;
use crate::race::{Course, Race, RaceGrid, RaceProgress, RaceSystems, Racer, seconds_to_ticks};
use crate::track::ItemBoxSpot;

/// An item box of the track (GD § 8), on each `ItemBoxSpot`: there to take when `respawn_in` is 0.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemBox {
    /// Seconds before it comes back, 0 when it is there.
    pub respawn_in: f32,
}

impl ItemBox {
    pub fn available(&self) -> bool {
        self.respawn_in <= 0.0
    }
}

/// A kart's item counters (race logs, tests, telemetry).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemStats {
    /// Boxes broken.
    pub boxes: u32,
    /// Items used: thrown, dropped, eaten (a piment), cashed (a croquette dorée).
    pub used: u32,
    /// Times it was hit (spin out or tumble).
    pub hits_taken: u32,
    /// Karts its items hit.
    pub hits_landed: u32,
    /// Items stopped by what it held out (shields).
    pub blocked: u32,
}

/// How an item out moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// Held out by its kart (it follows the kart).
    Held(Hold),
    /// A pelure lying on the ground.
    Ground,
    /// Rolling straight on the ground: a pelote (or an abeille thrown backwards). `fall`: its
    /// falling speed off an edge.
    Rolling { bounces_left: u32, fall: f32 },
    /// An abeille after `target`, the kart that was just ahead of its thrower.
    Homing {
        target: Option<Entity>,
        flight: Flight,
    },
}

/// An abeille's flight (GD § 8: homing in on the kart just ahead).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flight {
    /// Along the track's centerline, at race distance `distance`, `lateral` meters right of it,
    /// above the other karts.
    Track { distance: f32, lateral: f32 },
    /// Straight at its target (straight on once it lost it).
    Dive,
}

/// An item out on the track, or held out around its kart: a pelure, a pelote, an abeille. Its
/// simulated position is `position`; its `Transform` is only the visual, interpolated between
/// the last two ticks (`visual.rs`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[require(Transform, Visibility, LevelScoped)]
pub struct ItemObject {
    pub item: ItemKind,
    /// The kart that threw it (or holds it).
    pub owner: Entity,
    pub motion: Motion,
    pub position: Vec3,
    /// Its position on the previous tick (the swept contacts, the visual).
    pub previous: Vec3,
    pub velocity: Vec3,
    /// Seconds since it came out.
    pub age: f32,
}

impl ItemObject {
    pub fn new(
        item: ItemKind,
        owner: Entity,
        motion: Motion,
        position: Vec3,
        velocity: Vec3,
    ) -> Self {
        Self {
            item,
            owner,
            motion,
            position,
            previous: position,
            velocity,
            age: 0.0,
        }
    }

    /// Its radius (what touches it).
    pub fn radius(&self, tuning: &ItemTuning) -> f32 {
        item_radius(self.item, tuning)
    }

    pub fn held(&self) -> Option<Hold> {
        match self.motion {
            Motion::Held(hold) => Some(hold),
            _ => None,
        }
    }
}

/// The radius of an item out.
pub fn item_radius(item: ItemKind, tuning: &ItemTuning) -> f32 {
    match item {
        ItemKind::Pelure => tuning.pelure_radius,
        ItemKind::Pelote => tuning.pelote_radius,
        ItemKind::Abeille => tuning.abeille_radius,
        ItemKind::Piment | ItemKind::CroquetteDoree => 0.3,
    }
}

/// How a kart reacts to `item` (GD § 8): a tumble for the heavy pelote, a spin out for the light
/// pelure and abeille.
pub fn reaction_to(item: ItemKind) -> HitReaction {
    match item {
        ItemKind::Pelote => HitReaction::Tumble,
        _ => HitReaction::Spin,
    }
}

/// Whether an item hit does nothing to a kart (GD § 8 and § 15): already spinning or tumbling,
/// carried by the pelican, invincible after its last hit, or past the finish line.
pub fn immune_to_items(state: &KartState, invincibility: &Invincibility, finished: bool) -> bool {
    matches!(state, KartState::Hit { .. } | KartState::Respawning)
        || invincibility.is_active()
        || finished
}

/// `kart` was touched by an item of `by` (the item broke on it). The victim decides whether it
/// is hit, and applies the hit to its own kart (docs/ARCHITECTURE.md: on the network the item
/// is the thrower's, the hit the victim's).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct ItemHit {
    pub kart: Entity,
    pub by: Entity,
    pub item: ItemKind,
}

/// The item systems inside `FixedUpdate`: after the input and the race gate, before the
/// croquettes (an item hit costs its croquettes on the same tick) and the driving (a piment boosts
/// on the same tick, a hit kart spins from it).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemSystems;

/// The item rules. Headless: tests run them with `MinimalPlugins`.
pub struct ItemPlugin;

impl Plugin for ItemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemTuning>()
            .insert_resource(ItemTable::embedded())
            .add_message::<ItemHit>()
            .add_message::<LoseCroquettes>()
            .add_message::<CroquettePickedUp>()
            // Every kart holds items; every box spot of the track is a box.
            .register_required_components::<Kart, ItemSlots>()
            .register_required_components::<Kart, ItemStats>()
            .register_required_components::<ItemBoxSpot, ItemBox>()
            .configure_sets(
                FixedUpdate,
                ItemSystems
                    .after(KartSystems::Input)
                    .after(RaceSystems::Gate)
                    .before(CroquetteSystems)
                    .before(KartSystems::Drive),
            )
            .add_systems(
                FixedUpdate,
                (
                    (
                        reset_on_restart.run_if(resource_exists::<Race>),
                        tick_items,
                        take_boxes,
                        use_items,
                        move_items,
                        collide_items,
                        apply_item_hits,
                    )
                        .chain()
                        .run_if(items_on),
                    clear_items.run_if(not(items_on)),
                )
                    .in_set(ItemSystems),
            );
    }
}

/// The items are in the races (`ItemTuning::enabled`); not online yet (issue #9,
/// `level::Online`: they are not shared yet).
pub fn items_on(tuning: Res<ItemTuning>, online: Option<Res<crate::level::Online>>) -> bool {
    tuning.enabled && online.is_none()
}

/// Items switched off: nothing held, nothing out, every box back (and hidden, `visual.rs`).
fn clear_items(
    mut commands: Commands,
    mut karts: Query<&mut ItemSlots>,
    mut boxes: Query<&mut ItemBox>,
    objects: Query<Entity, With<ItemObject>>,
) {
    for mut slots in &mut karts {
        if slots.hand != Slot::Empty
            || slots.reserve != Slot::Empty
            || slots.dragging.is_some()
            || slots.draws != 0
        {
            slots.clear();
        }
    }
    for mut item_box in &mut boxes {
        item_box.set_if_neq(ItemBox::default());
    }
    for entity in &objects {
        commands.entity(entity).try_despawn();
    }
}

/// A race (re)start: empty slots, nothing on the track, every box back.
fn reset_on_restart(
    race: Res<Race>,
    mut commands: Commands,
    mut karts: Query<&mut ItemSlots>,
    mut boxes: Query<&mut ItemBox>,
    objects: Query<Entity, With<ItemObject>>,
) {
    if race.flow.tick != 1 {
        return;
    }
    for mut slots in &mut karts {
        if *slots != ItemSlots::default() {
            slots.clear();
        }
    }
    for mut item_box in &mut boxes {
        item_box.set_if_neq(ItemBox::default());
    }
    for entity in &objects {
        commands.entity(entity).try_despawn();
    }
}

/// The boxes' respawn timers and the roulettes.
fn tick_items(time: Res<Time>, mut boxes: Query<&mut ItemBox>, mut karts: Query<&mut ItemSlots>) {
    let dt = time.delta_secs();
    for mut item_box in &mut boxes {
        if !item_box.available() {
            // Half a tick of tolerance: 2 s are exactly 120 ticks.
            item_box.respawn_in -= dt;
            if item_box.respawn_in <= dt * 0.5 {
                item_box.respawn_in = 0.0;
            }
        }
    }
    for mut slots in &mut karts {
        let spinning = |slot: &Slot| matches!(slot, Slot::Spinning { .. });
        if spinning(&slots.hand) || spinning(&slots.reserve) {
            slots.tick();
        }
    }
}

/// A kart's grid slot for its draws (0 off the race), and its distance to the leader (0 off the
/// race: the leader's row).
fn draw_context(racer: Option<&Racer>, progress: Option<&RaceProgress>) -> (u64, f32) {
    (
        racer.map_or(0, |r| r.grid_slot as u64),
        progress.map_or(0.0, |p| p.gap_to_leader),
    )
}

/// Karts running into a box break it and draw their items (one per free slot, two for a double
/// box), from the table of their distance to the leader. A kart with both slots taken breaks it
/// for nothing (MK8DX).
fn take_boxes(
    tuning: Res<ItemTuning>,
    table: Res<ItemTable>,
    grid: Option<Res<RaceGrid>>,
    mut boxes: Query<(&Transform, &ItemBoxSpot, &mut ItemBox)>,
    mut karts: Query<
        (
            &Transform,
            &KartState,
            &mut ItemSlots,
            &mut ItemStats,
            Option<&Racer>,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
) {
    let seed = grid.map_or(RaceGrid::GAME_SEED, |g| g.seed);
    let roulette = seconds_to_ticks(tuning.roulette_time);
    for (box_transform, spot, mut item_box) in &mut boxes {
        if !item_box.available() {
            continue;
        }
        for (transform, state, mut slots, mut stats, racer, progress) in &mut karts {
            if *state == KartState::Respawning
                || progress.is_some_and(|p| p.finished())
                || !touches(
                    transform,
                    KART_HALF_EXTENTS,
                    tuning.box_reach,
                    box_transform.translation,
                )
            {
                continue;
            }
            item_box.respawn_in = tuning.box_respawn_time.max(f32::MIN_POSITIVE);
            stats.boxes += 1;
            let (slot, gap) = draw_context(racer, progress);
            for _ in 0..if spot.double { 2 } else { 1 } {
                if !slots.has_room() {
                    break;
                }
                let unit = draw_unit(seed, slot, slots.draws);
                slots.draws += 1;
                if let Some((item, count)) = table.draw(gap, unit).item() {
                    slots.give(item, count, roulette);
                }
            }
            break;
        }
    }
}

/// The "back" direction of the item button (GD § 8: thrown backwards): the brake (Bas/S, B) or
/// the look-back button (C).
pub fn aims_back(input: &KartInput) -> bool {
    input.brake > 0.5 || input.look_back
}

/// Where a kart's held item `hold` of `item` is now (world).
fn held_position(
    owner: &Transform,
    hold: Hold,
    item: ItemKind,
    age: f32,
    tuning: &ItemTuning,
) -> Vec3 {
    owner.transform_point(held_offset(hold, 3, item_radius(item, tuning), age, tuning))
}

/// What an abeille thrown by `owner` goes after: the kart just ahead of it in the race, or, off a
/// race, the closest kart ahead of it.
fn abeille_target(
    owner: Entity,
    transform: &Transform,
    race: Option<&Race>,
    karts: &[(Entity, Vec3)],
) -> Option<Entity> {
    if let Some(race) = race
        && let Some(i) = race.standings.iter().position(|e| *e == owner)
    {
        return i.checked_sub(1).map(|ahead| race.standings[ahead]);
    }
    let forward = *transform.forward();
    karts
        .iter()
        .filter(|(e, at)| *e != owner && (*at - transform.translation).dot(forward) > 1.0)
        .min_by(|(_, a), (_, b)| {
            a.distance_squared(transform.translation)
                .total_cmp(&b.distance_squared(transform.translation))
        })
        .map(|(e, _)| *e)
}

/// Where an item thrown by a kart starts and how it moves: `aim_back` thrown backwards, `from`
/// where it was held (a pelure of a row is dropped there). `None` for the items that are not
/// objects (piment, croquette dorée).
struct Launch<'a> {
    kart: Entity,
    transform: &'a Transform,
    progress: Option<&'a RaceProgress>,
    course: Option<&'a Course>,
    race: Option<&'a Race>,
    positions: &'a [(Entity, Vec3)],
}

impl Launch<'_> {
    fn object(
        &self,
        tuning: &ItemTuning,
        item: ItemKind,
        aim_back: bool,
        from: Option<Vec3>,
    ) -> Option<ItemObject> {
        let transform = self.transform;
        let forward = transform.forward().with_y(0.0).normalize_or(Vec3::NEG_Z);
        let radius = item_radius(item, tuning);
        let low = Vec3::Y * (KART_RIDE_HEIGHT - radius);
        // Just off the front (or the back) of the kart, on the ground.
        let reach = forward * (KART_HALF_EXTENTS.z + radius + 0.2);
        let (front, back) = (
            transform.translation + reach - low,
            transform.translation - reach - low,
        );
        let rolling = |bounces_left| Motion::Rolling {
            bounces_left,
            fall: 0.0,
        };
        Some(match item {
            ItemKind::Pelure => {
                // Dropped behind (where it was held, if it was).
                let at = from
                    .unwrap_or(transform.translation - forward * tuning.pelure_drop_distance - low);
                ItemObject::new(item, self.kart, Motion::Ground, at, Vec3::ZERO)
            }
            ItemKind::Pelote => {
                let (at, direction) = if aim_back {
                    (back, -forward)
                } else {
                    (front, forward)
                };
                ItemObject::new(
                    item,
                    self.kart,
                    rolling(tuning.pelote_bounces),
                    at,
                    direction * tuning.pelote_speed,
                )
            }
            // Backwards: straight back on the ground, broken by the first wall.
            ItemKind::Abeille if aim_back => ItemObject::new(
                item,
                self.kart,
                rolling(0),
                back,
                -forward * tuning.abeille_speed,
            ),
            ItemKind::Abeille => {
                let target = abeille_target(self.kart, transform, self.race, self.positions);
                let (flight, at) = match (self.course, self.progress) {
                    (Some(course), Some(progress)) => {
                        let distance = progress.distance + KART_HALF_EXTENTS.z + 0.5;
                        let sample = course.center.at(distance);
                        let lateral = (transform.translation - sample.position).dot(sample.right);
                        (
                            Flight::Track { distance, lateral },
                            sample.point(lateral, tuning.abeille_height),
                        )
                    }
                    _ => (Flight::Dive, front + Vec3::Y * 0.5),
                };
                ItemObject::new(
                    item,
                    self.kart,
                    Motion::Homing { target, flight },
                    at,
                    forward * tuning.abeille_dive_speed,
                )
            }
            ItemKind::Piment | ItemKind::CroquetteDoree => return None,
        })
    }
}

fn spawn_object(commands: &mut Commands, object: ItemObject) {
    commands.spawn((
        Name::new(object.item.label()),
        Transform::from_translation(object.position),
        object,
    ));
}

/// Every kart's item button: drag, release, deploy, fire, use (`slots.rs`). Items go out from
/// the kart; a piment boosts it, a croquette dorée fills its pockets. A kart carried by the
/// pelican or past the finish line loses what it held out and uses nothing; a kart spinning or
/// tumbling uses nothing.
fn use_items(
    tuning: Res<ItemTuning>,
    croquette_tuning: Option<Res<CroquetteTuning>>,
    race: Option<Res<Race>>,
    course: Option<Res<Course>>,
    mut commands: Commands,
    mut positions: Local<Vec<(Entity, Vec3)>>,
    objects: Query<(Entity, &ItemObject)>,
    mut karts: Query<
        (
            Entity,
            &Transform,
            &KartInput,
            &KartState,
            &mut ItemSlots,
            &mut ItemStats,
            &mut Boost,
            Option<&mut Croquettes>,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut picked: MessageWriter<CroquettePickedUp>,
) {
    positions.clear();
    positions.extend(karts.iter().map(|(e, t, ..)| (e, t.translation)));
    let croquettes_tuning = croquette_tuning.as_deref().cloned().unwrap_or_default();
    let started = race.as_ref().is_none_or(|r| r.started());
    for (kart, transform, input, state, mut slots, mut stats, mut boost, croquettes, progress) in
        &mut karts
    {
        // The items this kart holds out, and the one that leaves now: the dragged one, or the
        // last one of an "x3".
        let held_out = || {
            objects
                .iter()
                .filter(move |(_, o)| o.owner == kart && o.held().is_some())
        };
        let leaving = |dragged: bool| {
            held_out()
                .filter(|(_, o)| (o.held() == Some(Hold::Dragged)) == dragged)
                .max_by_key(|(_, o)| match o.held() {
                    Some(Hold::Trail(k) | Hold::Orbit(k)) => k,
                    _ => 0,
                })
                .map(|(e, o)| (e, o.position))
        };
        let out = *state == KartState::Respawning || progress.is_some_and(|p| p.finished());
        if out || state.is_hit() || !started {
            if out && slots.lose_held_out() {
                for (entity, _) in held_out() {
                    commands.entity(entity).try_despawn();
                }
            }
            if slots.button != input.item {
                slots.button = input.item;
            }
            continue;
        }
        let action = {
            let mut next = *slots;
            let action = next.step_button(input.item, aims_back(input));
            slots.set_if_neq(next);
            action
        };
        let launch = Launch {
            kart,
            transform,
            progress,
            course: course.as_deref(),
            race: race.as_deref(),
            positions: &positions,
        };
        let (item, aim_back, from) = match action {
            Use::None => continue,
            Use::Drag(item) => {
                let at = held_position(transform, Hold::Dragged, item, 0.0, &tuning);
                let held = ItemObject::new(item, kart, Motion::Held(Hold::Dragged), at, Vec3::ZERO);
                spawn_object(&mut commands, held);
                continue;
            }
            Use::Deploy { item, count } => {
                for k in 0..count {
                    let hold = if item == ItemKind::Pelure {
                        Hold::Trail(k)
                    } else {
                        Hold::Orbit(k)
                    };
                    let at = held_position(transform, hold, item, 0.0, &tuning);
                    let held = ItemObject::new(item, kart, Motion::Held(hold), at, Vec3::ZERO);
                    spawn_object(&mut commands, held);
                }
                continue;
            }
            Use::Release { item, back } | Use::Fire { item, back } => {
                let from = leaving(matches!(action, Use::Release { .. }));
                if let Some((entity, _)) = from {
                    commands.entity(entity).try_despawn();
                }
                // A pelure is dropped where it was held; pelotes and abeilles leave from the
                // front (or the back).
                let at = from.map(|(_, at)| at).filter(|_| item == ItemKind::Pelure);
                (item, back, at)
            }
            Use::Instant { item, back } => (item, back, None),
        };
        stats.used += 1;
        match item {
            ItemKind::Piment => boost.add(tuning.piment_strength, tuning.piment_duration),
            ItemKind::CroquetteDoree => {
                if let Some(mut held) = croquettes {
                    let gained = held.gain(tuning.golden_croquettes, croquettes_tuning.max_held);
                    for _ in 0..gained {
                        picked.write(CroquettePickedUp {
                            kart,
                            at: transform.translation,
                            count: held.count,
                        });
                    }
                }
                // A croquette's mini-boost, as for every croquette (GD § 7).
                boost.add(
                    croquettes_tuning.pickup_boost_strength,
                    croquettes_tuning.pickup_boost_duration,
                );
            }
            _ => {
                if let Some(object) = launch.object(&tuning, item, aim_back, from) {
                    spawn_object(&mut commands, object);
                }
            }
        }
    }
}

/// Moves every item out: the held ones with their kart, the pelotes rolling (rays against the
/// track), the abeilles along the centerline then diving. Too old, fallen, broken on a wall, or
/// held by a kart that no longer holds it: gone.
fn move_items(
    time: Res<Time>,
    tuning: Res<ItemTuning>,
    rapier: ReadRapierContext,
    course: Option<Res<Course>>,
    mut commands: Commands,
    karts: Query<
        (
            Entity,
            &Transform,
            &KartState,
            &ItemSlots,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut objects: Query<(Entity, &mut ItemObject)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let context = rapier.single().ok();
    // Only the track and the static obstacles, never a kart.
    let filter = QueryFilter::only_fixed().exclude_sensors();
    let cast = |origin: Vec3, direction: Vec3, length: f32| {
        context.as_ref().and_then(|c| {
            c.cast_ray_and_get_normal(origin, direction, length, true, filter)
                .map(|(_, hit)| (hit.time_of_impact, hit.normal))
        })
    };
    // A kart an abeille may go after: on the track, racing.
    let chaseable = |entity: Entity| {
        karts
            .get(entity)
            .ok()
            .filter(|(_, _, state, _, progress)| {
                **state != KartState::Respawning && !progress.is_some_and(|p| p.finished())
            })
            .map(|(_, transform, _, _, progress)| {
                (transform.translation, progress.map(|p| p.distance))
            })
    };
    for (entity, mut object) in &mut objects {
        let mut item = *object;
        item.previous = item.position;
        item.age += dt;
        let radius = item.radius(&tuning);
        let alive = match item.motion {
            Motion::Held(hold) => match karts.get(item.owner) {
                Ok((_, transform, _, slots, _))
                    if slots.held_out().is_some_and(|(held, _)| held == item.item)
                        && (hold == Hold::Dragged) == slots.dragging.is_some() =>
                {
                    item.position = held_position(transform, hold, item.item, item.age, &tuning);
                    item.velocity = (item.position - item.previous) / dt;
                    true
                }
                _ => false,
            },
            Motion::Ground => {
                // Dropped: it lands on the ground under it (a kart in the air drops it there);
                // with no ground below, it is gone.
                if item.age <= 1.5 * dt {
                    match cast(item.position + Vec3::Y, Vec3::NEG_Y, PELURE_DROP_DEPTH)
                        .filter(|(_, normal)| normal.y >= WALL_NORMAL_Y)
                    {
                        Some((distance, normal)) => {
                            let ground = item.position + Vec3::Y * (1.0 - distance);
                            item.position = ground + normal * radius * PELURE_LIFT;
                            item.previous = item.position;
                        }
                        None if context.is_some() => item.age = f32::INFINITY,
                        None => {}
                    }
                }
                item.age < tuning.pelure_lifetime
            }
            Motion::Rolling { bounces_left, fall } => {
                let mut rolling = Roll {
                    position: item.position,
                    velocity: item.velocity,
                    fall,
                    bounces_left,
                };
                let step = roll(&mut rolling, radius, dt, cast);
                item.position = rolling.position;
                item.velocity = rolling.velocity;
                item.motion = Motion::Rolling {
                    bounces_left: rolling.bounces_left,
                    fall: rolling.fall,
                };
                let lifetime = if item.item == ItemKind::Abeille {
                    tuning.abeille_lifetime
                } else {
                    tuning.pelote_lifetime
                };
                !matches!(step, RollStep::Broke | RollStep::Fell) && item.age < lifetime
            }
            Motion::Homing { target, flight } => {
                let mut target = target.filter(|t| chaseable(*t).is_some());
                let flight = match (flight, course.as_deref()) {
                    (Flight::Track { distance, lateral }, Some(course)) => {
                        let distance = distance + tuning.abeille_speed * dt;
                        let sample = course.center.at(distance);
                        let chased = target.and_then(chaseable);
                        let mut lateral = lateral;
                        if let Some((at, _)) = chased {
                            let wanted = (at - sample.position).dot(sample.right);
                            let step = tuning.abeille_lateral_speed * dt;
                            lateral += (wanted - lateral).clamp(-step, step);
                        }
                        let room = (sample.half_width - 0.5).max(0.0);
                        lateral = lateral.clamp(-room, room);
                        item.position = sample.point(lateral, tuning.abeille_height);
                        item.velocity = (item.position - item.previous) / dt;
                        // Close enough, along the track too (not across the infield): dive.
                        let near = |at: Vec3, along: Option<f32>| {
                            at.distance(item.position) < tuning.abeille_dive_distance
                                && along.is_none_or(|d| {
                                    (d - distance).abs() < 2.0 * tuning.abeille_dive_distance
                                })
                        };
                        if target.is_none() {
                            // The leader's abeille: the first kart it comes close to.
                            target = karts
                                .iter()
                                .filter(|(e, ..)| *e != item.owner)
                                .filter_map(|(e, ..)| chaseable(e).map(|c| (e, c)))
                                .find(|(_, (at, along))| near(*at, *along))
                                .map(|(e, _)| e);
                        }
                        match target.and_then(chaseable) {
                            Some((at, along)) if near(at, along) => Flight::Dive,
                            _ => Flight::Track { distance, lateral },
                        }
                    }
                    _ => {
                        // Diving at its target, or straight on without one (or off a race).
                        if let Some((at, _)) = target.and_then(chaseable) {
                            let to = at - item.position;
                            let step = tuning.abeille_dive_speed * dt;
                            item.velocity = to.normalize_or(item.velocity.normalize_or_zero())
                                * tuning.abeille_dive_speed;
                            item.position = if to.length() <= step {
                                at
                            } else {
                                item.position + item.velocity * dt
                            };
                        } else {
                            item.position += item.velocity * dt;
                        }
                        Flight::Dive
                    }
                };
                item.motion = Motion::Homing { target, flight };
                item.age < tuning.abeille_lifetime
            }
        };
        if alive {
            if *object != item {
                *object = item;
            }
        } else {
            commands.entity(entity).try_despawn();
        }
    }
}

/// A dropped pelure finds the ground up to this far below it (m); further, it is gone.
const PELURE_DROP_DEPTH: f32 = 40.0;
/// A pelure lies flat: its center is this share of its radius above the ground.
const PELURE_LIFT: f32 = 0.5;

/// One item out, as the contacts see it.
#[derive(Clone, Copy, Debug)]
struct Contact {
    entity: Entity,
    item: ItemKind,
    owner: Entity,
    from: Vec3,
    to: Vec3,
    radius: f32,
    age: f32,
    held: Option<Hold>,
    moving: bool,
}

/// Contacts this tick, along each item's path since the last tick: two items of different karts
/// touching break each other (a held item is a shield); an item touching a kart breaks on it
/// and tells the kart ([`ItemHit`]). A kart's own items never touch each other; its own pelote or
/// pelure can hit it after `own_item_delay`, its own abeille never.
fn collide_items(
    tuning: Res<ItemTuning>,
    mut commands: Commands,
    mut items: Local<Vec<Contact>>,
    mut broken: Local<Vec<bool>>,
    mut bodies: Local<Vec<(Entity, Transform)>>,
    objects: Query<(Entity, &ItemObject)>,
    karts: Query<(Entity, &Transform, &KartState), With<Kart>>,
    mut slots: Query<(&mut ItemSlots, &mut ItemStats)>,
    mut hits: MessageWriter<ItemHit>,
) {
    items.clear();
    items.extend(objects.iter().map(|(entity, o)| Contact {
        entity,
        item: o.item,
        owner: o.owner,
        from: o.previous,
        to: o.position,
        radius: o.radius(&tuning),
        age: o.age,
        held: o.held(),
        moving: o.motion != Motion::Ground,
    }));
    if items.is_empty() {
        return;
    }
    bodies.clear();
    bodies.extend(
        karts
            .iter()
            .filter(|(_, _, state)| **state != KartState::Respawning)
            .map(|(e, t, _)| (e, *t)),
    );
    broken.clear();
    broken.resize(items.len(), false);
    for i in 0..items.len() {
        if broken[i] {
            continue;
        }
        let a = items[i];
        // The first thing along its path: another item, or a kart.
        let mut first: Option<(f32, Result<usize, Entity>)> = None;
        let mut consider = |t: f32, what: Result<usize, Entity>| {
            if first.is_none_or(|(best, _)| t < best) {
                first = Some((t, what));
            }
        };
        for (j, b) in items.iter().enumerate() {
            if j == i || broken[j] || b.owner == a.owner || !(a.moving || b.moving) {
                continue;
            }
            if let Some(t) = segment_sphere(a.from, a.to, b.to, a.radius + b.radius) {
                consider(t, Ok(j));
            }
        }
        for (kart, transform) in bodies.iter() {
            if *kart == a.owner
                && (a.held.is_some()
                    || a.item == ItemKind::Abeille
                    || a.age < tuning.own_item_delay)
            {
                continue;
            }
            let inverse = transform.rotation.inverse();
            let from = inverse * (a.from - transform.translation);
            let to = inverse * (a.to - transform.translation);
            if let Some(t) = segment_box(from, to, KART_HALF_EXTENTS + Vec3::splat(a.radius)) {
                // An item meets a shield before the kart behind it.
                consider(t + 1e-4, Err(*kart));
            }
        }
        match first {
            Some((_, Ok(j))) => {
                broken[i] = true;
                broken[j] = true;
                for c in [a, items[j]] {
                    if c.held.is_some()
                        && let Ok((mut held, mut stats)) = slots.get_mut(c.owner)
                    {
                        held.lose_one_held_out();
                        stats.blocked += 1;
                    }
                }
            }
            Some((_, Err(kart))) => {
                broken[i] = true;
                if a.held.is_some()
                    && let Ok((mut held, _)) = slots.get_mut(a.owner)
                {
                    held.lose_one_held_out();
                }
                hits.write(ItemHit {
                    kart,
                    by: a.owner,
                    item: a.item,
                });
            }
            None => {}
        }
    }
    for (i, c) in items.iter().enumerate() {
        if broken[i] {
            commands.entity(c.entity).try_despawn();
        }
    }
}

/// The victim decides (docs/ARCHITECTURE.md): an item touching a kart hits it unless it is
/// immune ([`immune_to_items`]). Hit: it spins out or tumbles (`KartState::Hit`, which loses its
/// drift; its boost and what it held out are lost too), and loses its croquettes. Its
/// invincibility starts when the spin or tumble ends (the kart's own rules).
fn apply_item_hits(
    tuning: Res<ItemTuning>,
    mut commands: Commands,
    mut hits: MessageReader<ItemHit>,
    objects: Query<(Entity, &ItemObject)>,
    mut karts: Query<
        (
            &mut KartState,
            &mut Boost,
            &Invincibility,
            &mut ItemSlots,
            &mut ItemStats,
            Option<&RaceProgress>,
        ),
        With<Kart>,
    >,
    mut losses: MessageWriter<LoseCroquettes>,
) {
    for hit in hits.read() {
        let Ok((mut state, mut boost, invincibility, mut slots, mut stats, progress)) =
            karts.get_mut(hit.kart)
        else {
            continue;
        };
        if immune_to_items(
            &state,
            invincibility,
            progress.is_some_and(|p| p.finished()),
        ) {
            continue;
        }
        let reaction = reaction_to(hit.item);
        let seconds = match reaction {
            HitReaction::Spin => tuning.spin_time,
            HitReaction::Tumble => tuning.tumble_time,
        };
        *state = KartState::hit(reaction, seconds, TICK_HZ as f32);
        *boost = Boost::default();
        stats.hits_taken += 1;
        if slots.lose_held_out() {
            for (entity, object) in &objects {
                if object.owner == hit.kart && object.held().is_some() {
                    commands.entity(entity).try_despawn();
                }
            }
        }
        losses.write(LoseCroquettes {
            kart: hit.kart,
            cause: LossCause::ItemHit,
        });
        if let Ok((.., mut by, _)) = karts.get_mut(hit.by)
            && hit.by != hit.kart
        {
            by.hits_landed += 1;
        }
    }
}
