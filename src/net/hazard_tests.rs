//! Issue #144 acceptance, online: two whole games in a room (the in-memory socket of the tests,
//! `race_tests`) race on the Corniche Cannelle. The hazards move from the race clock and the
//! race's seed: the bison walks the same way in both games, tick for tick; it knocks over Bob's
//! kart in Bob's game (the victim decides), at a tick and a place where Ana's game has it too,
//! and Ana sees Bob's kart tumble.

use std::collections::HashMap;

use bevy_rapier3d::prelude::Velocity;

use super::race_tests::{ANA, BOB, player, seated, start_race};
use super::*;
use crate::interpolation::PhysicsPose;
use crate::kart::{HitReaction, KartState, PlayerKart};
use crate::race::{Course, Race, RaceGrid, RaceProgress, Tow};
use crate::track::hazard::{clock_seconds, hazard_phase, kart_capsule};
use crate::track::{GRID_HEIGHT, HazardBody, HazardKind, NpcKind, TrackLayout};

fn flow_tick(app: &App) -> u32 {
    app.world().resource::<Race>().flow.tick
}

/// The bison of the circuit: its entity, and its place in the layout.
fn bison(app: &mut App) -> (Entity, crate::track::Hazard) {
    let layout = app.world().resource::<TrackLayout>().clone();
    let hazard = layout
        .hazards
        .iter()
        .find(|h| h.kind == HazardKind::Npc(NpcKind::Bison))
        .expect("no bison")
        .clone();
    let world = app.world_mut();
    let entity = world
        .query::<(Entity, &HazardBody)>()
        .iter(world)
        .find(|(_, body)| body.0 == hazard.index)
        .map(|(entity, _)| entity)
        .unwrap();
    (entity, hazard)
}

#[test]
fn online_the_bison_walks_alike_and_its_victim_decides() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    let index = crate::admin::tracks::circuits()
        .iter()
        .position(|c| c.name == "Corniche Cannelle")
        .expect("the Corniche Cannelle in the manifest") as u8;
    room.act(ANA, NetAction::ChooseCircuit(index));
    room.until(20.0, "free driving on the Corniche Cannelle", |app| {
        app.world()
            .get_resource::<TrackLayout>()
            .is_some_and(|layout| layout.name == "Corniche Cannelle")
    });
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    let seeds: Vec<u64> = room
        .games
        .iter()
        .map(|(_, app)| app.world().resource::<RaceGrid>().seed)
        .collect();
    assert_eq!(seeds[0], seeds[1], "the host's seed in both games");
    let ana_kart = player(room.app(ANA));
    let bob_kart = player(room.app(BOB));
    for (key, kart) in [(ANA, ana_kart), (BOB, bob_kart)] {
        room.app(key)
            .world_mut()
            .entity_mut(kart)
            .remove::<PlayerKart>();
    }
    let (ana_bison, hazard) = bison(room.app(ANA));
    let (bob_bison, _) = bison(room.app(BOB));

    // The bison, tick for tick, in both games.
    let mut seen: [HashMap<u32, Vec3>; 2] = [HashMap::new(), HashMap::new()];
    for _ in 0..150 {
        room.frame();
        for (k, (key, entity)) in [(ANA, ana_bison), (BOB, bob_bison)].into_iter().enumerate() {
            let app = room.view(key);
            let at = app.world().get::<Transform>(entity).unwrap().translation;
            seen[k].insert(flow_tick(app), at);
        }
    }
    let common: Vec<u32> = seen[0]
        .keys()
        .filter(|tick| seen[1].contains_key(tick))
        .copied()
        .collect();
    assert!(common.len() > 100, "{} ticks seen by both", common.len());
    for tick in &common {
        assert_eq!(seen[0][tick], seen[1][tick], "tick {tick}");
    }
    let (lowest, highest) = seen[0].values().fold((f32::MAX, f32::MIN), |(l, h), at| {
        let across = (*at - hazard.road.position).dot(hazard.road.right);
        (l.min(across), h.max(across))
    });
    assert!(highest - lowest > 0.5, "it walks ({lowest} to {highest})");

    // Bob's kart stands in the middle of the bison's walk, in Bob's game.
    let standing = hazard.road.transform(hazard.lateral, GRID_HEIGHT);
    {
        let app = room.app(BOB);
        let spacing = app.world().resource::<Course>().checkpoints[1];
        let world = app.world_mut();
        *world.get_mut::<Transform>(bob_kart).unwrap() = standing;
        world
            .get_mut::<PhysicsPose>(bob_kart)
            .unwrap()
            .snap(standing);
        *world.get_mut::<Velocity>(bob_kart).unwrap() = Velocity::zero();
        let mut progress = world.get_mut::<RaceProgress>(bob_kart).unwrap();
        progress.passed = 1 + (hazard.road.distance / spacing).floor() as u32;
        progress.distance = hazard.road.distance;
        world
            .get_mut::<Tow>(bob_kart)
            .unwrap()
            .reset(standing.translation);
    }
    let mut knocked = None;
    for _ in 0..(8 * 60) {
        room.frame();
        let app = room.view(BOB);
        if app.world().get::<KartState>(bob_kart).unwrap().is_hit() {
            knocked = Some((
                flow_tick(app),
                *app.world().get::<Transform>(bob_kart).unwrap(),
            ));
            break;
        }
    }
    let (tick, _) = knocked.expect("the bison never knocked Bob's kart over in 8 s");
    assert!(
        matches!(
            room.view(BOB).world().get::<KartState>(bob_kart),
            Some(KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            })
        ),
        "a tumble"
    );
    // Ana's game has the bison on Bob's kart at that tick (the same clock, the same seed): it
    // was there, by her rules too.
    let ana = room.view(ANA);
    let seed = ana.world().resource::<RaceGrid>().seed;
    let layout = ana.world().resource::<TrackLayout>();
    let her_bison = &layout.hazards[usize::from(hazard.index)];
    let body =
        her_bison.body(&her_bison.pose(clock_seconds(tick), hazard_phase(seed, hazard.index)));
    assert!(
        body.gap(&kart_capsule(&standing)).0 <= 0.3,
        "Ana's bison at tick {tick} is not on Bob's kart"
    );
    // Ana sees Bob's kart tumble (its state, from Bob's game).
    room.until(1.0, "Bob's kart tumbling on Ana's screen", |app| {
        if !super::race_tests::lobby(app).is_host() {
            return true;
        }
        let sync = app.world().resource::<KartSync>();
        let Some(proxy) = sync.remote(BOB).and_then(|r| r.proxy) else {
            return false;
        };
        matches!(
            app.world().get::<KartState>(proxy),
            Some(KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            })
        )
    });
    // Ana's kart, far from it, was never knocked over.
    assert!(
        !room
            .view(ANA)
            .world()
            .get::<KartState>(ana_kart)
            .unwrap()
            .is_hit()
    );
}
