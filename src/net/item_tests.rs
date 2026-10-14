//! Headless scenarios of the items and the croquettes online (issue #53): two whole games
//! (`GamePlugin`) in a room, linked by the in-memory socket of the tests, as in `race_tests.rs`
//! (its helpers: `Room`, `seated`, `start_race`...). Every claim, verdict, item and croquette goes
//! through its bytes. The test drives the players' karts (no keyboard): it places them, gives
//! them items and presses their item button.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use super::race_tests::{
    ANA, BOB, Room, assert_same_progress, bots_drive_the_players, common_tick, lobby, player_of,
    progress_by_slot, seated, standings, start_race,
};
use super::*;
use crate::croquettes::{Croquettes, LooseCroquette, TrackCroquette};
use crate::interpolation::PhysicsPose;
use crate::items::{ItemBox, ItemKind, ItemObject, ItemSlots, ItemStats, Motion, Slot};
use crate::kart::{HitReaction, Invincibility, KartInput, KartState, PlayerKart};
use crate::race::{Race, RacePhase};
use crate::track::{GRID_HEIGHT, ITEM_BOX_HEIGHT, SpotIndex};

/// Two players in a race of one lap without bots, after GO; the test drives both karts.
fn racing() -> Room {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 0, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    for key in [ANA, BOB] {
        let app = room.app(key);
        let kart = player_of(app);
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    }
    room
}

fn items(app: &App) -> &OnlineItems {
    app.world().resource::<OnlineItems>()
}

/// The network id of a game's player kart.
fn id_of(app: &App) -> KartId {
    app.world().get::<NetKart>(player_of(app)).unwrap().0
}

/// Puts `key`'s kart at `transform`, still (a teleport: no slide).
fn place(room: &mut Room, key: PeerKey, transform: Transform) {
    let app = room.app(key);
    let kart = player_of(app);
    let world = app.world_mut();
    *world.get_mut::<Transform>(kart).unwrap() = transform;
    world.get_mut::<PhysicsPose>(kart).unwrap().snap(transform);
    *world.get_mut::<Velocity>(kart).unwrap() = Velocity::zero();
}

/// `key`'s kart: where it is.
fn pose(room: &Room, key: PeerKey) -> Transform {
    let app = room.view(key);
    *app.world().get::<Transform>(player_of(app)).unwrap()
}

/// Gives `key`'s kart `count` of `item`, ready.
fn give(room: &mut Room, key: PeerKey, item: ItemKind, count: u8) {
    let app = room.app(key);
    let kart = player_of(app);
    let mut slots = app.world_mut().get_mut::<ItemSlots>(kart).unwrap();
    slots.clear();
    slots.give(item, count, 0);
}

/// `key`'s item button, held or not, for the next frames.
fn hold_item(room: &mut Room, key: PeerKey, held: bool) {
    let app = room.app(key);
    let kart = player_of(app);
    app.world_mut().get_mut::<KartInput>(kart).unwrap().item = held;
}

/// Presses and releases `key`'s item button (one frame each).
fn use_item(room: &mut Room, key: PeerKey) {
    hold_item(room, key, true);
    room.frame();
    hold_item(room, key, false);
    room.frame();
}

/// Bob's kart `ahead` meters in front of Ana's, on her lane, both still; then the time for each
/// screen to show the other's kart there.
fn bob_ahead_of_ana(room: &mut Room, ahead: f32) {
    let ana = pose(room, ANA);
    let bob = Transform {
        translation: ana.translation + *ana.forward() * ahead,
        ..ana
    };
    place(room, ANA, ana);
    place(room, BOB, bob);
    for _ in 0..30 {
        room.frame();
    }
}

fn state(room: &Room, key: PeerKey) -> KartState {
    let app = room.view(key);
    *app.world().get::<KartState>(player_of(app)).unwrap()
}

fn stats(room: &Room, key: PeerKey) -> ItemStats {
    let app = room.view(key);
    *app.world().get::<ItemStats>(player_of(app)).unwrap()
}

fn slots(room: &Room, key: PeerKey) -> ItemSlots {
    let app = room.view(key);
    *app.world().get::<ItemSlots>(player_of(app)).unwrap()
}

/// The positions of the network's items in a game, by id, at this frame's common tick.
fn item_positions(app: &App) -> Vec<(ItemId, u32, Vec3)> {
    let tick = common_tick(app);
    items(app)
        .objects()
        .filter_map(|(entity, id)| {
            let object = app.world().get::<ItemObject>(entity)?;
            Some((id, tick, object.position))
        })
        .collect()
}

/// The common tick a box (or a croquette) of the track comes back on, in a game (`None`: it is
/// there).
fn back_on(app: &mut App, index: u16, croquette: bool) -> Option<u32> {
    let now = common_tick(app);
    let world = app.world_mut();
    let left = if croquette {
        world
            .query::<(&SpotIndex, &TrackCroquette)>()
            .iter(world)
            .find(|(spot, _)| spot.0 == index)
            .map(|(_, c)| c.respawn_in)
            .unwrap()
    } else {
        world
            .query::<(&SpotIndex, &ItemBox)>()
            .iter(world)
            .find(|(spot, _)| spot.0 == index)
            .map(|(_, b)| b.respawn_in)
            .unwrap()
    };
    (left > 0.0).then(|| now + (left * crate::TICK_HZ as f32).round() as u32)
}

/// Where a spot of the track is (a box, or a croquette).
fn spot(app: &mut App, index: u16, croquette: bool) -> Transform {
    let world = app.world_mut();
    if croquette {
        world
            .query_filtered::<(&SpotIndex, &Transform), With<TrackCroquette>>()
            .iter(world)
            .find(|(spot, _)| spot.0 == index)
            .map(|(_, t)| *t)
            .unwrap()
    } else {
        world
            .query_filtered::<(&SpotIndex, &Transform), With<ItemBox>>()
            .iter(world)
            .find(|(spot, _)| spot.0 == index)
            .map(|(_, t)| *t)
            .unwrap()
    }
}

/// Issue #53 acceptance: Ana and Bob drive into the same box on the same tick. Each takes it on
/// its own screen (its roulette spins at once), and asks the host: only one gets it (the earlier
/// claim, the better grid slot on the same tick: Ana), the other's roulette stops, empty. The box
/// is gone on both screens and comes back on the same tick. Bob draws Ana's roulette again from
/// her claim: the same item as hers.
#[test]
fn a_box_is_taken_by_one_kart_only_and_gone_for_everyone() {
    let mut room = racing();
    let target = spot(room.app(ANA), 1, false);
    // Both karts touch box 1 (1.15 m on either side of it), and no other box.
    let road = target.translation - Vec3::Y * (ITEM_BOX_HEIGHT - GRID_HEIGHT);
    for (key, side) in [(ANA, -1.15), (BOB, 1.15)] {
        place(
            &mut room,
            key,
            Transform {
                translation: road + *target.right() * side,
                ..target
            },
        );
    }
    room.frame();
    for key in [ANA, BOB] {
        assert_eq!(stats(&room, key).boxes, 1, "{key:?} broke it");
        assert!(
            matches!(slots(&room, key).hand, Slot::Spinning { .. }),
            "{key:?}: its roulette spins at once"
        );
    }
    room.until(1.0, "the host's verdicts", |app| {
        items(app).arbiter.pending().count() == 0 && items(app).arbiter.grants().len() == 1
    });
    for _ in 0..5 {
        room.frame();
    }
    let ana_id = id_of(room.view(ANA));
    for key in [ANA, BOB] {
        let grants = items(room.view(key)).arbiter.grants().to_vec();
        assert_eq!(grants.len(), 1, "{key:?}: {grants:?}");
        assert_eq!(grants[0].spot, Spot::Box(1));
        assert_eq!(grants[0].kart, ana_id, "{key:?}: Ana's claim first");
    }
    assert!(matches!(slots(&room, ANA).hand, Slot::Spinning { .. }));
    let bob = slots(&room, BOB);
    assert_eq!(
        bob,
        ItemSlots {
            draws: bob.draws,
            button: bob.button,
            ..ItemSlots::default()
        },
        "Bob's roulette stopped, empty"
    );
    assert_eq!(items(room.view(BOB)).log.refused.len(), 1);
    // Gone on both screens, back on the same tick (2 s after Ana's claim).
    let backs: Vec<Option<u32>> = [ANA, BOB]
        .iter()
        .map(|key| back_on(room.app(*key), 1, false))
        .collect();
    println!("box 1 back on {backs:?}");
    let (a, b) = (backs[0].unwrap(), backs[1].unwrap());
    assert!(a.abs_diff(b) <= 1, "{a} vs {b}");
    assert!(
        a.abs_diff(grants_tick(&room, ANA) + 120) <= 1,
        "2 s after the claim"
    );
    // The same roulette on both sides: Bob draws Ana's item from her claim.
    let drawn = items(room.view(BOB)).log.draws.clone();
    assert_eq!(drawn.len(), 1, "{drawn:?}");
    let (kart, _, prizes) = &drawn[0];
    assert_eq!(*kart, ana_id);
    let Slot::Spinning { item, count, .. } = slots(&room, ANA).hand else {
        panic!("Ana's roulette");
    };
    assert_eq!(prizes.as_slice(), &[(item, count)], "the same draw");
    // Both karts away; 2 s after the claim, back on both screens.
    for key in [ANA, BOB] {
        let at = pose(&room, key);
        place(
            &mut room,
            key,
            Transform {
                translation: at.translation - *at.forward() * 8.0,
                ..at
            },
        );
    }
    room.until(3.0, "box 1 back", |app| common_tick(app) > a + 1);
    for key in [ANA, BOB] {
        assert_eq!(back_on(room.app(key), 1, false), None, "{key:?}");
    }
}

/// The tick of the first grant in a game.
fn grants_tick(room: &Room, key: PeerKey) -> u32 {
    items(room.view(key)).arbiter.grants()[0].tick
}

/// Issue #53 acceptance: Ana throws a pelote at Bob, 12 m ahead. It rolls at the same place on
/// both screens (the same position at the same common tick); Bob's game decides that it hits
/// his kart (a tumble), tells everyone: the pelote is gone on both screens, Ana counts her hit.
/// Invincible, Bob's kart is not hit by the next one: « ignoré », gone all the same.
#[test]
fn a_pelote_hits_where_the_victim_decides() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 12.0);
    give(&mut room, ANA, ItemKind::Pelote, 1);
    use_item(&mut room, ANA);
    let mut seen: Vec<(PeerKey, ItemId, u32, Vec3)> = Vec::new();
    for _ in 0..60 {
        room.frame();
        for key in [ANA, BOB] {
            for (id, tick, at) in item_positions(room.view(key)) {
                seen.push((key, id, tick, at));
            }
        }
        if matches!(state(&room, BOB), KartState::Hit { .. })
            && items(room.view(ANA)).objects().count() == 0
        {
            break;
        }
    }
    assert!(
        matches!(
            state(&room, BOB),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        ),
        "Bob tumbles: {:?}",
        state(&room, BOB)
    );
    assert_eq!(stats(&room, BOB).hits_taken, 1);
    assert_eq!(stats(&room, ANA).hits_landed, 1, "Ana's hit, told by Bob");
    for key in [ANA, BOB] {
        assert_eq!(items(room.view(key)).objects().count(), 0, "{key:?}");
    }
    // The same pelote at the same place on both screens, tick by tick.
    let mut compared = 0;
    let mut worst = 0.0f32;
    for (_, id, tick, at) in seen.iter().filter(|(key, ..)| *key == BOB) {
        if let Some((.., ana_at)) = seen
            .iter()
            .find(|(key, i, t, _)| *key == ANA && i == id && t == tick)
        {
            compared += 1;
            worst = worst.max(at.distance(*ana_at));
        }
    }
    println!("the pelote on both screens: {compared} ticks compared, {worst:.4} m apart at worst");
    assert!(compared >= 1, "{seen:?}");
    assert!(worst < 0.01);
    // The hit in both logs: Bob's kart, landed.
    let bob_id = id_of(room.view(BOB));
    for key in [ANA, BOB] {
        let breaks = &items(room.view(key)).log.breaks;
        assert_eq!(breaks.len(), 1, "{key:?}");
        assert_eq!((breaks[0].1, breaks[0].2), (Some(bob_id), true), "{key:?}");
    }

    // The tumble over; Bob invincible; another pelote.
    room.until(3.0, "the tumble over", |app| {
        lobby(app).is_host()
            || !matches!(
                app.world().get::<KartState>(player_of(app)).unwrap(),
                KartState::Hit { .. }
            )
    });
    bob_ahead_of_ana(&mut room, 12.0);
    {
        let app = room.app(BOB);
        let kart = player_of(app);
        app.world_mut()
            .get_mut::<Invincibility>(kart)
            .unwrap()
            .remaining = 3.0;
    }
    give(&mut room, ANA, ItemKind::Pelote, 1);
    use_item(&mut room, ANA);
    room.until(1.0, "the pelote gone everywhere", |app| {
        items(app).log.breaks.len() == 2 && items(app).objects().count() == 0
    });
    assert!(!matches!(state(&room, BOB), KartState::Hit { .. }));
    assert_eq!(stats(&room, BOB).hits_taken, 1);
    assert_eq!(stats(&room, ANA).hits_landed, 1, "ignored: not a hit");
    for key in [ANA, BOB] {
        let last = items(room.view(key)).log.breaks[1];
        assert_eq!((last.1, last.2), (Some(bob_id), false), "{key:?}");
    }
}

/// Issue #53: Bob, on the grid behind Ana (she has the pole), throws an abeille: it goes after the
/// kart just ahead of him in his ranking, Ana's, by her id; Ana's game sends it after her own
/// kart, which it hits (a spin out), as her game decides.
#[test]
fn an_abeille_goes_after_the_kart_ahead() {
    let mut room = racing();
    let ana_id = id_of(room.view(ANA));
    give(&mut room, BOB, ItemKind::Abeille, 1);
    use_item(&mut room, BOB);
    room.until(1.0, "the abeille on Ana's screen", |app| {
        !lobby(app).is_host() || items(app).objects().count() == 1
    });
    let (entity, _) = items(room.view(ANA)).objects().next().unwrap();
    let ana_kart = player_of(room.view(ANA));
    let object = *room.view(ANA).world().get::<ItemObject>(entity).unwrap();
    assert_eq!(object.item, ItemKind::Abeille);
    assert!(
        matches!(object.motion, Motion::Homing { target: Some(target), .. } if target == ana_kart),
        "after Ana's kart: {:?}",
        object.motion
    );
    room.until(3.0, "Ana hit", |app| {
        items(app).log.breaks.len() == 1 && items(app).objects().count() == 0
    });
    assert!(
        matches!(
            state(&room, ANA),
            KartState::Hit {
                reaction: HitReaction::Spin,
                ..
            }
        ),
        "{:?}",
        state(&room, ANA)
    );
    assert_eq!(stats(&room, BOB).hits_landed, 1);
    for key in [ANA, BOB] {
        let breaks = &items(room.view(key)).log.breaks;
        assert_eq!((breaks[0].1, breaks[0].2), (Some(ana_id), true), "{key:?}");
    }
}

/// Issue #53: Bob drags a pelote behind his kart (a shield): Ana sees it there. Her pelote meets
/// it on Bob's screen: both break, Bob's kart is not hit; Ana's pelote is gone on her screen too,
/// and so is Bob's shield.
#[test]
fn a_dragged_pelote_shields_its_kart() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 15.0);
    give(&mut room, BOB, ItemKind::Pelote, 1);
    hold_item(&mut room, BOB, true);
    room.until(1.0, "Bob's shield on Ana's screen", |app| {
        !lobby(app).is_host() || items(app).shown_held().count() == 1
    });
    assert_eq!(slots(&room, BOB).dragging, Some(ItemKind::Pelote));
    give(&mut room, ANA, ItemKind::Pelote, 1);
    use_item(&mut room, ANA);
    room.until(1.5, "the pelotes broken", |app| {
        items(app).log.breaks.len() == 1 && items(app).objects().count() == 0
    });
    for _ in 0..20 {
        room.frame();
    }
    assert!(!matches!(state(&room, BOB), KartState::Hit { .. }));
    assert_eq!(slots(&room, BOB).dragging, None, "the shield broke");
    assert_eq!(stats(&room, BOB).blocked, 1);
    assert_eq!(stats(&room, ANA).hits_landed, 0);
    for key in [ANA, BOB] {
        let breaks = &items(room.view(key)).log.breaks;
        assert_eq!((breaks[0].1, breaks[0].2), (None, false), "{key:?}");
    }
    assert_eq!(items(room.view(ANA)).shown_held().count(), 0);
}

/// Issue #53: hit by Ana's pelote, Bob's kart loses 3 of its 5 croquettes: they fly out at the
/// same places on both screens. Ana drives onto them and picks them up (the host grants them):
/// they are gone on Bob's screen too. Then both karts on the same croquette of the track on the
/// same tick: only one keeps it.
#[test]
fn croquettes_lost_by_one_are_picked_up_by_another() {
    let mut room = racing();
    {
        let app = room.app(BOB);
        let kart = player_of(app);
        app.world_mut().get_mut::<Croquettes>(kart).unwrap().count = 5;
    }
    bob_ahead_of_ana(&mut room, 12.0);
    give(&mut room, ANA, ItemKind::Pelote, 1);
    use_item(&mut room, ANA);
    room.until(2.0, "the croquettes on both screens", |app| {
        items(app).loose().count() == 3
    });
    let scattered = |app: &App| -> Vec<(LooseId, Vec3)> {
        let mut all: Vec<(LooseId, Vec3)> = items(app)
            .loose()
            .map(|(entity, id)| (id, app.world().get::<LooseCroquette>(entity).unwrap().to))
            .collect();
        all.sort_by_key(|(id, _)| id.index);
        all
    };
    let (on_bob, on_ana) = (scattered(room.view(BOB)), scattered(room.view(ANA)));
    println!("scattered: {on_bob:?}");
    assert_eq!(on_bob.len(), 3);
    for ((a, x), (b, y)) in on_bob.iter().zip(&on_ana) {
        assert_eq!(a, b);
        assert!(x.distance(*y) < 1e-4, "{x} vs {y}");
    }
    let bob = room.view(BOB);
    assert_eq!(
        bob.world().get::<Croquettes>(player_of(bob)).unwrap().count,
        2
    );
    // Landed, within Ana's reach: she drives onto each one.
    for _ in 0..40 {
        room.frame();
    }
    let ana = pose(&room, ANA);
    for (_, at) in &on_ana {
        let onto = Transform {
            translation: Vec3::new(at.x, ana.translation.y, at.z),
            ..ana
        };
        place(&mut room, ANA, onto);
        for _ in 0..3 {
            room.frame();
        }
    }
    room.until(1.0, "picked up", |app| {
        items(app).loose().count() == 0 && items(app).arbiter.pending().count() == 0
    });
    let ana_app = room.view(ANA);
    assert_eq!(
        ana_app
            .world()
            .get::<Croquettes>(player_of(ana_app))
            .unwrap()
            .count,
        3
    );
    let ana_id = id_of(room.view(ANA));
    for key in [ANA, BOB] {
        let app = room.app(key);
        let loose = app
            .world_mut()
            .query::<&LooseCroquette>()
            .iter(app.world())
            .count();
        assert_eq!(loose, 0, "{key:?}: none left");
        let grants: Vec<(Spot, KartId)> = items(room.view(key))
            .arbiter
            .grants()
            .iter()
            .map(|g| (g.spot, g.kart))
            .collect();
        assert_eq!(
            grants
                .iter()
                .filter(|(spot, kart)| matches!(spot, Spot::Loose(_)) && *kart == ana_id)
                .count(),
            3,
            "{key:?}: {grants:?}"
        );
    }

    // Both karts on croquette 0 of the track on the same tick: one keeps it.
    let croquette = spot(room.app(ANA), 0, true);
    let road = croquette.translation - Vec3::Y * (crate::track::CROQUETTE_HEIGHT - GRID_HEIGHT);
    let counts = |room: &Room| {
        [ANA, BOB].map(|key| {
            let app = room.view(key);
            app.world().get::<Croquettes>(player_of(app)).unwrap().count
        })
    };
    let before = counts(&room);
    for (key, side) in [(ANA, -1.1), (BOB, 1.1)] {
        place(
            &mut room,
            key,
            Transform {
                translation: road + *croquette.right() * side,
                ..croquette
            },
        );
    }
    room.frame();
    let predicted = counts(&room);
    assert_eq!(
        predicted,
        [before[0] + 1, before[1] + 1],
        "both took it here"
    );
    room.until(1.0, "the verdicts", |app| {
        items(app).arbiter.pending().count() == 0
    });
    for _ in 0..5 {
        room.frame();
    }
    assert_eq!(
        counts(&room),
        [before[0] + 1, before[1]],
        "Ana's: Bob's goes back"
    );
    let backs: Vec<Option<u32>> = [ANA, BOB]
        .iter()
        .map(|key| back_on(room.app(*key), 0, true))
        .collect();
    assert!(
        backs[0].unwrap().abs_diff(backs[1].unwrap()) <= 1,
        "{backs:?}"
    );
}

/// Issue #53 acceptance (in memory): a race with items and croquettes, two players driven by
/// bots and 4 bots, to the end: the same results on both screens, and the same story of the
/// items: the same boxes and croquettes taken by the same karts on the same ticks, the same items
/// thrown, the same hits, the same croquettes lost.
#[test]
fn a_race_with_items_ends_the_same_for_everyone() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    let start = start_race(&mut room, ANA, 4, 1);
    bots_drive_the_players(&mut room);
    // A lap is short: the players start with three abeilles each (their bot brains throw them
    // at the karts ahead), on top of what the boxes give.
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    for key in [ANA, BOB] {
        let slot = start.slot_of(key).unwrap();
        let app = room.app(key);
        let world = app.world_mut();
        let mut karts = world.query::<(&NetKart, &mut ItemSlots)>();
        for (net, mut slots) in karts.iter_mut(world) {
            if net.0 == slot {
                slots.give(ItemKind::Abeille, 3, 0);
            }
        }
    }
    room.until(150.0, "the results", |app| {
        app.world().resource::<Race>().phase() == RacePhase::Results
    });
    // What was on its way at the end arrives.
    for _ in 0..10 {
        room.frame();
    }
    let (ana, bob) = (
        progress_by_slot(room.app(ANA)),
        progress_by_slot(room.app(BOB)),
    );
    assert_same_progress(&ana, &bob);
    assert_eq!(standings(room.app(ANA)), standings(room.app(BOB)));
    let (summary, thrown) = {
        let items = items(room.view(ANA));
        (items.summary(), items.log.thrown.len())
    };
    println!("{summary}");
    let boxes = items(room.view(ANA))
        .arbiter
        .grants()
        .iter()
        .filter(|g| matches!(g.spot, Spot::Box(_)))
        .count();
    assert!(boxes >= 1, "the karts took boxes: {summary}");
    assert!(thrown >= 3, "they used items: {summary}");
    let (ana, bob) = (room.view(ANA), room.view(BOB));
    assert_same_story(ana, bob);
}

/// Two games tell the same story of the items (issue #53): the same boxes and croquettes taken
/// by the same karts on the same ticks, in the same order; the same items thrown; the same hits
/// (every item broken on a kart, landed or not); the same croquettes lost, at the same places.
/// Only what is older than a second: what happened just now may still be on its way.
pub(super) fn assert_same_story(a: &App, b: &App) {
    let cutoff = common_tick(a).min(common_tick(b)).saturating_sub(60);
    let story = |app: &App| {
        let items = items(app);
        let grants: Vec<ClaimKey> = items
            .arbiter
            .grants()
            .iter()
            .copied()
            .filter(|g| g.tick <= cutoff)
            .collect();
        let mut thrown: Vec<(ItemId, ItemKind, u32)> = items
            .log
            .thrown
            .iter()
            .copied()
            .filter(|(.., tick)| *tick <= cutoff)
            .collect();
        thrown.sort_by_key(|(id, _, tick)| (*tick, id.owner, id.seq));
        let mut scatters = items.log.scatters.clone();
        scatters.retain(|(_, _, tick, _)| *tick <= cutoff);
        scatters.sort_by_key(|(kart, loss, ..)| (*kart, *loss));
        (grants, thrown, scatters)
    };
    let (x, y) = (story(a), story(b));
    assert_eq!(x.0, y.0, "the same boxes and croquettes, in the same order");
    assert_eq!(x.1, y.1, "the same items out");
    assert_eq!(x.2, y.2, "the same croquettes lost");
    let counts = (x.0.len(), x.1.len(), x.2.len());
    // Every hit one side knew a second ago, the other knows too.
    let hits = |app: &App| -> Vec<(ItemId, Option<KartId>, bool, u32)> {
        items(app)
            .log
            .breaks
            .iter()
            .copied()
            .filter(|(_, kart, ..)| kart.is_some())
            .collect()
    };
    let (x, y) = (hits(a), hits(b));
    let landed = x.iter().filter(|(.., landed, _)| *landed).count();
    for (mine, theirs) in [(&x, &y), (&y, &x)] {
        for (id, kart, landed, at) in mine.iter().filter(|(.., at)| *at <= cutoff) {
            assert!(
                theirs
                    .iter()
                    .any(|(i, k, l, _)| i == id && k == kart && l == landed),
                "hit of {id:?} on {kart:?} (landed: {landed}, tick {at}) not on the other screen"
            );
        }
    }
    println!(
        "the same story on both screens (until tick {cutoff}): {} boxes and croquettes taken, {} items out, {} items broken on karts ({landed} hits), {} croquette losses",
        counts.0,
        counts.1,
        x.len(),
        counts.2
    );
}
