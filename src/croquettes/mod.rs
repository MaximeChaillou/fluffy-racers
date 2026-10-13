//! Croquettes (GD § 7, issue #14): our equivalent of the coins of MK8DX.
//!
//! - **Held**: every kart holds 0 to 10 croquettes ([`Croquettes`]), each worth +1 % of top
//!   speed. The bonus is the kart's `MaxSpeedFactors::croquettes`, multiplied with the other
//!   factors (bots' catch-up...) into its `MaxSpeedScale`.
//! - **On the track**: the circuit file lines croquettes up (`croquettes` in
//!   `assets/tracks/*.track.ron`, spawned by the track as `CroquetteSpot`s, which become
//!   [`TrackCroquette`]s). A kart touching one picks it up: +1 croquette (still taken at 10) and
//!   a mini-boost of 0.3 s through the kart's `Boost`, like every boost. It comes back 10 s
//!   later.
//! - **Lost**: bumped by another kart (`kart::KartBump`), a kart loses 1; hit by an item, 3
//!   (issue #16 writes [`LoseCroquettes`]). Never below 0. The lost croquettes fly out behind
//!   the kart and land on the ground ([`LooseCroquette`]): anyone can pick them up, the kart that
//!   lost them a bit later, until they vanish.
//!
//! Everything runs in `FixedUpdate` at 60 Hz ([`CroquetteSystems`]): after the karts' input and
//! the race gate, before the karts drive, so a pick-up boosts the kart on this very tick. Pure
//! logic in `logic.rs`; visuals in `visual.rs` ([`CroquetteVisualPlugin`]); the HUD counter is in
//! `crate::ui`, the sounds in `crate::audio`, both fed by [`CroquettePickedUp`] and
//! [`CroquettesLost`].

mod logic;
mod tuning;
mod visual;

#[cfg(test)]
mod scenario_tests;

pub use logic::{
    Croquettes, LooseCroquette, LossCause, PICKUP_BUCKETS, PICKUP_CELL, PickupEntry, PickupGrid,
    TrackCroquette, flight_position, scatter_direction, touches,
};
pub use tuning::CroquetteTuning;
pub use visual::{CroquetteBurst, CroquetteLook, CroquetteVisualAssets, CroquetteVisualPlugin};

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext};

use crate::kart::{
    Boost, KART_HALF_EXTENTS, Kart, KartBump, KartState, KartSystems, MaxSpeedFactors,
};
use crate::race::{Race, RaceSystems};
use crate::track::CroquetteSpot;

/// Makes `kart` lose croquettes (GD § 7). The entry point of the item hits: issue #16 writes
/// `LoseCroquettes { kart, cause: LossCause::ItemHit }` (3 croquettes). Bumps between karts
/// come from `kart::KartBump` and need nothing more.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct LoseCroquettes {
    pub kart: Entity,
    pub cause: LossCause,
}

/// `kart` picked up the croquette at `at` and now holds `count` (HUD pop, sound, effect).
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CroquettePickedUp {
    pub kart: Entity,
    pub at: Vec3,
    pub count: u8,
}

/// `kart` lost `lost` croquettes (they fly out), and now holds `count`.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct CroquettesLost {
    pub kart: Entity,
    pub lost: u8,
    pub count: u8,
    pub cause: LossCause,
}

/// The croquette systems inside `FixedUpdate`: after the input and the race gate, before the
/// karts drive.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CroquetteSystems;

/// The croquette rules. Headless: tests run them with `MinimalPlugins`.
pub struct CroquettePlugin;

impl Plugin for CroquettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CroquetteTuning>()
            .add_message::<LoseCroquettes>()
            .add_message::<CroquettePickedUp>()
            .add_message::<CroquettesLost>()
            // Every kart holds croquettes; every croquette spot of the track is a croquette.
            .register_required_components::<Kart, Croquettes>()
            .register_required_components::<CroquetteSpot, TrackCroquette>()
            .configure_sets(
                FixedUpdate,
                CroquetteSystems
                    .after(KartSystems::Input)
                    .after(RaceSystems::Gate)
                    .before(KartSystems::Drive),
            )
            .add_systems(
                FixedUpdate,
                (
                    reset_on_restart.run_if(resource_exists::<Race>),
                    tick_croquettes,
                    lose_croquettes,
                    pick_up_croquettes,
                    update_speed_factors,
                )
                    .chain()
                    .in_set(CroquetteSystems),
            );
    }
}

/// A race (re)start: empty pockets, every croquette of the track back, the scattered ones gone.
fn reset_on_restart(
    race: Res<Race>,
    mut commands: Commands,
    mut karts: Query<&mut Croquettes>,
    mut track: Query<&mut TrackCroquette>,
    loose: Query<Entity, With<LooseCroquette>>,
) {
    if race.flow.tick != 1 {
        return;
    }
    for mut held in &mut karts {
        held.set_if_neq(Croquettes::default());
    }
    for mut croquette in &mut track {
        croquette.set_if_neq(TrackCroquette::default());
    }
    for entity in &loose {
        commands.entity(entity).try_despawn();
    }
}

/// Respawn timers of the track's croquettes, age of the scattered ones (they vanish in the
/// end), bump grace of the karts.
fn tick_croquettes(
    time: Res<Time>,
    tuning: Res<CroquetteTuning>,
    mut commands: Commands,
    mut track: Query<&mut TrackCroquette>,
    mut loose: Query<(Entity, &mut LooseCroquette)>,
    mut karts: Query<&mut Croquettes>,
) {
    let dt = time.delta_secs();
    for mut croquette in &mut track {
        if !croquette.available() {
            croquette.step(dt);
        }
    }
    for (entity, mut croquette) in &mut loose {
        croquette.age += dt;
        if croquette.expired(&tuning) {
            commands.entity(entity).try_despawn();
        }
    }
    for mut held in &mut karts {
        if held.bump_grace > 0.0 {
            held.bump_grace = (held.bump_grace - dt).max(0.0);
        }
    }
}

/// Bumps (from the last physics step) and item hits: the kart loses croquettes, which fly out
/// behind it. A bump costs nothing during the grace period of the previous one.
fn lose_croquettes(
    tuning: Res<CroquetteTuning>,
    rapier: ReadRapierContext,
    mut commands: Commands,
    mut bumps: MessageReader<KartBump>,
    mut hits: MessageReader<LoseCroquettes>,
    mut lost_messages: MessageWriter<CroquettesLost>,
    mut karts: Query<(&Transform, &mut Croquettes), With<Kart>>,
) {
    let context = rapier.single().ok();
    // Only the track and the static obstacles stop a flying croquette, never a kart.
    let filter = QueryFilter::only_fixed().exclude_sensors();
    let cast = |origin: Vec3, direction: Vec3, length: f32| {
        context
            .as_ref()
            .and_then(|c| c.cast_ray(origin, direction, length, true, filter))
            .map(|(_, toi)| toi)
    };
    let bumped = bumps.read().map(|bump| (bump.kart, LossCause::Bump));
    let hit = hits.read().map(|hit| (hit.kart, hit.cause));
    for (kart, cause) in bumped.chain(hit) {
        let Ok((transform, mut held)) = karts.get_mut(kart) else {
            continue;
        };
        if cause == LossCause::Bump {
            if held.bump_grace > 0.0 {
                continue;
            }
            held.bump_grace = tuning.bump_grace;
        }
        let lost = held.lose(cause.croquettes(&tuning));
        if lost == 0 {
            continue;
        }
        lost_messages.write(CroquettesLost {
            kart,
            lost,
            count: held.count,
            cause,
        });
        // They fly out of the kart, over the road behind it: stopped short of the walls, they
        // land on whatever ground is there.
        let center = transform.translation;
        let from = center + Vec3::Y * 0.3;
        for i in 0..lost {
            let direction = scatter_direction(*transform.forward(), i, lost, tuning.scatter_angle);
            let wall_margin = 0.5;
            let reach = cast(center, direction, tuning.scatter_distance + wall_margin)
                .map_or(tuning.scatter_distance, |toi| (toi - wall_margin).max(0.0));
            let over = center + direction * reach;
            // From a little above (a slope rising behind the kart) down to well below (a kart
            // hit in the air, on a jump).
            let (above, below) = (2.0, 20.0);
            let ground = cast(over + Vec3::Y * above, Vec3::NEG_Y, above + below)
                .map_or(center.y - KART_HALF_EXTENTS.y, |toi| over.y + above - toi);
            let to = over.with_y(ground + tuning.loose_rest_height);
            commands.spawn((
                Name::new("Scattered croquette"),
                LooseCroquette::new(from, to, kart),
                Transform::from_translation(from),
            ));
        }
    }
}

/// Every kart touching a croquette picks it up: +1 (up to 10) and the mini-boost. The
/// croquettes are bucketed on a grid first, so each kart only tests the few around it.
fn pick_up_croquettes(
    tuning: Res<CroquetteTuning>,
    mut grid: Local<PickupGrid>,
    mut commands: Commands,
    mut track: Query<(Entity, &Transform, &mut TrackCroquette)>,
    loose: Query<(Entity, &LooseCroquette)>,
    mut karts: Query<(Entity, &Transform, &KartState, &mut Croquettes, &mut Boost), With<Kart>>,
    mut picked: MessageWriter<CroquettePickedUp>,
) {
    grid.clear();
    for (entity, transform, croquette) in &track {
        if croquette.available() {
            grid.insert(entity, transform.translation, None);
        }
    }
    for (entity, croquette) in &loose {
        // Not yet within reach, or vanishing this tick.
        if croquette.age < tuning.loose_pickup_delay || croquette.expired(&tuning) {
            continue;
        }
        let owner = croquette.dropped_by;
        let not_for = (!croquette.pickable_by(&tuning, owner)).then_some(owner);
        grid.insert(entity, croquette.position(&tuning), not_for);
    }
    if grid.is_empty() {
        return;
    }
    for (kart, transform, state, mut held, mut boost) in &mut karts {
        if *state == KartState::Respawning {
            continue;
        }
        grid.take_touching(
            kart,
            transform,
            KART_HALF_EXTENTS,
            tuning.pickup_reach,
            |entry| {
                match track.get_mut(entry.entity) {
                    Ok((_, _, mut croquette)) => croquette.take(&tuning),
                    Err(_) => {
                        commands.entity(entry.entity).try_despawn();
                    }
                }
                held.gain(1, tuning.max_held);
                boost.add(tuning.pickup_boost_strength, tuning.pickup_boost_duration);
                picked.write(CroquettePickedUp {
                    kart,
                    at: entry.position,
                    count: held.count,
                });
            },
        );
    }
}

/// The croquettes' top speed factor of every kart (+1 % each).
fn update_speed_factors(
    tuning: Res<CroquetteTuning>,
    mut karts: Query<(&Croquettes, &mut MaxSpeedFactors)>,
) {
    for (held, mut factors) in &mut karts {
        let factor = held.speed_factor(&tuning);
        if factors.croquettes != factor {
            factors.croquettes = factor;
        }
    }
}
