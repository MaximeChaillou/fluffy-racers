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
use crate::bots::RacingLine;
use crate::croquettes::{Croquettes, LooseCroquette, TrackCroquette};
use crate::interpolation::PhysicsPose;
use crate::items::{
    ItemBox, ItemKind, ItemObject, ItemSlots, ItemStats, ItemTable, ItemTuning, Motion,
    OnlineItemTable, PillowFlight, Slot, deflate_time,
};
use crate::kart::{
    HitReaction, Invincibility, ItemEffects, Kart, KartInput, KartState, PlayerKart,
};
use crate::race::{Race, RacePhase, Racer};
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

// --- Issue #18: the items of phase 2 that play online ------------------------------------------

/// `key`'s kart input for the next frames.
fn drive(room: &mut Room, key: PeerKey, input: KartInput) {
    let app = room.app(key);
    let kart = player_of(app);
    *app.world_mut().get_mut::<KartInput>(kart).unwrap() = input;
}

const FLAT_OUT: KartInput = KartInput {
    throttle: 1.0,
    brake: 0.0,
    steer: 0.0,
    jump_drift: false,
    item: false,
    look_back: false,
    interact: false,
};

/// Issue #18: Ana lobs a bombe at Bob, 20 m ahead. It flies at the same places on both screens
/// and explodes where it lands, on the same common tick on both (no message needed); Bob's game
/// decides for his kart, in the blast (a tumble), and tells everyone; Ana's kart, out of it, is
/// not hit.
#[test]
fn a_bombe_explodes_where_it_lands_on_every_screen() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 20.0);
    give(&mut room, ANA, ItemKind::Bombe, 1);
    use_item(&mut room, ANA);
    let mut seen: Vec<(PeerKey, ItemId, u32, Vec3)> = Vec::new();
    let mut gone: Vec<(PeerKey, u32)> = Vec::new();
    for _ in 0..120 {
        room.frame();
        for key in [ANA, BOB] {
            let positions = item_positions(room.view(key));
            if positions.is_empty()
                && !items(room.view(key)).log.thrown.is_empty()
                && !gone.iter().any(|(k, _)| *k == key)
            {
                gone.push((key, common_tick(room.view(key))));
            }
            for (id, tick, at) in positions {
                seen.push((key, id, tick, at));
            }
        }
        if gone.len() == 2 && items(room.view(ANA)).log.breaks.len() == 1 {
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
    assert!(!matches!(state(&room, ANA), KartState::Hit { .. }));
    assert_eq!(stats(&room, ANA).hits_landed, 1, "Ana's hit, told by Bob");
    // The same bombe at the same place, tick by tick; gone on the same common tick.
    let mut compared = 0;
    for (_, id, tick, at) in seen.iter().filter(|(key, ..)| *key == BOB) {
        if let Some((.., ana_at)) = seen
            .iter()
            .find(|(key, i, t, _)| *key == ANA && i == id && t == tick)
        {
            compared += 1;
            assert!(at.distance(*ana_at) < 0.01, "{at} vs {ana_at}");
        }
    }
    assert!(compared >= 1);
    assert_eq!(gone.len(), 2);
    assert!(
        gone[0].1.abs_diff(gone[1].1) <= 1,
        "the same tick: {gone:?}"
    );
    let bob_id = id_of(room.view(BOB));
    for key in [ANA, BOB] {
        let log = &items(room.view(key)).log;
        assert_eq!(log.thrown[0].1, ItemKind::Bombe);
        assert!(log.blasts.is_empty(), "it landed: no message");
        assert_eq!(log.breaks.len(), 1, "{key:?}");
        assert_eq!((log.breaks[0].1, log.breaks[0].2), (Some(bob_id), true));
    }
}

/// Issue #18: Bob drops a bombe behind his kart and drives away; Ana, behind him, drives into
/// it. Her game judges the contact: the bombe explodes on her kart (a tumble), and her game
/// tells where: on Bob's screen it explodes there too, out of his reach.
#[test]
fn a_bombe_explodes_on_the_kart_that_touches_it_where_its_game_says() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 25.0);
    give(&mut room, BOB, ItemKind::Bombe, 1);
    let back = KartInput {
        brake: 1.0,
        ..default()
    };
    drive(&mut room, BOB, KartInput { item: true, ..back });
    room.frame();
    drive(&mut room, BOB, back);
    room.frame();
    room.until(1.0, "the bombe on Ana's screen", |app| {
        items(app).objects().count() == 1
    });
    drive(&mut room, BOB, FLAT_OUT);
    drive(&mut room, ANA, FLAT_OUT);
    room.until(3.0, "the blast told", |app| {
        items(app).log.blasts.len() == 1
            && items(app).objects().count() == 0
            && items(app).log.breaks.len() == 1
    });
    assert!(
        matches!(
            state(&room, ANA),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        ),
        "Ana tumbles: {:?}",
        state(&room, ANA)
    );
    assert!(!matches!(state(&room, BOB), KartState::Hit { .. }));
    let (ana, bob) = (items(room.view(ANA)), items(room.view(BOB)));
    assert_eq!(
        ana.log.blasts, bob.log.blasts,
        "the same explosion, the same tick"
    );
    assert_eq!(ana.log.blasts[0].0, ana.log.thrown[0].0);
    let ana_id = id_of(room.view(ANA));
    for key in [ANA, BOB] {
        let breaks = &items(room.view(key)).log.breaks;
        assert_eq!((breaks[0].1, breaks[0].2), (Some(ana_id), true), "{key:?}");
    }
    assert_eq!(stats(&room, BOB).hits_landed, 1);
}

/// Issue #18: Bob drops a pelure close to Ana, then drags a pelote behind him (a shield), close
/// to her too; Ana sounds her klaxon-canard. Her game destroys them and tells everyone: the
/// pelure is gone on both screens, Bob loses his shield; Bob's screen hears the klaxon. No kart
/// is hit.
#[test]
fn a_klaxon_destroys_the_items_around_on_every_screen() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 7.0);
    give(&mut room, BOB, ItemKind::Pelure, 1);
    use_item(&mut room, BOB);
    room.until(1.0, "the pelure on both screens", |app| {
        items(app).objects().count() == 1
    });
    give(&mut room, BOB, ItemKind::Pelote, 1);
    hold_item(&mut room, BOB, true);
    room.until(1.0, "Bob's shield on Ana's screen", |app| {
        !lobby(app).is_host() || items(app).shown_held().count() == 1
    });
    give(&mut room, ANA, ItemKind::KlaxonCanard, 1);
    use_item(&mut room, ANA);
    room.until(1.0, "the items gone everywhere", |app| {
        items(app).objects().count() == 0 && items(app).log.honks.len() == 1
    });
    for _ in 0..10 {
        room.frame();
    }
    assert_eq!(slots(&room, BOB).dragging, None, "Bob's shield is gone");
    assert_eq!(items(room.view(ANA)).shown_held().count(), 0);
    for key in [ANA, BOB] {
        assert!(
            !matches!(state(&room, key), KartState::Hit { .. }),
            "{key:?}"
        );
        let honks = &items(room.view(key)).log.honks;
        assert_eq!(honks[0].0, id_of(room.view(ANA)), "{key:?}");
    }
    assert_eq!(
        items(room.view(ANA)).log.honks,
        items(room.view(BOB)).log.honks,
        "on the same tick"
    );
}

/// Issue #18: online, only the items that play online: since issue #61, every one. The online
/// table is the solo table (every prize, its share), and the debug gifts (a debug build only)
/// give any item online, the fusée too (before #61, the gift of a fusée left the hand as it
/// was).
#[test]
fn online_only_the_items_that_play_online() {
    let mut room = racing();
    let table = room
        .view(ANA)
        .world()
        .resource::<crate::items::OnlineItemTable>()
        .0
        .clone();
    for row in &table.rows {
        assert!(row.prizes.iter().all(|(p, _)| p.online()), "{row:?}");
    }
    assert_eq!(table, ItemTable::embedded(), "the solo table");
    give(&mut room, ANA, ItemKind::Pelure, 1);
    // The gifts go to the player's kart (the test drives it otherwise).
    let ana_kart = player_of(room.app(ANA));
    room.app(ANA)
        .world_mut()
        .entity_mut(ana_kart)
        .insert(PlayerKart);
    room.app(ANA)
        .world_mut()
        .write_message(crate::items::GiveItem(crate::items::Prize::Fusee));
    room.frame();
    assert_eq!(
        slots(&room, ANA).hand,
        Slot::Ready {
            item: ItemKind::Fusee,
            count: 1
        },
        "a fusée online (a debug build)"
    );
    room.app(ANA)
        .world_mut()
        .write_message(crate::items::GiveItem(crate::items::Prize::PimentDore));
    room.frame();
    assert_eq!(
        slots(&room, ANA).hand,
        Slot::Ready {
            item: ItemKind::PimentDore,
            count: 1
        }
    );
    // The piment doré is Ana's own boost: Bob sees its flames.
    room.app(ANA)
        .world_mut()
        .entity_mut(ana_kart)
        .remove::<PlayerKart>();
    use_item(&mut room, ANA);
    room.until(1.0, "the flames on Bob's screen", |app| {
        let world = app.world();
        lobby(app).is_host()
            || world
                .try_query_filtered::<&crate::kart::Boost, With<RemoteKartProxy>>()
                .is_some_and(|mut boosts| boosts.iter(world).any(|b| b.strength > 0.2))
    });
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
    // Issue #61: the same chantilly and dégonflages, the same karts knocked over by a super câlin
    // or a fusée, the same oreillers after the same karts.
    let more = |app: &App| {
        let log = &items(app).log;
        let mut effects = log.effects.clone();
        effects.retain(|(.., tick)| *tick <= cutoff);
        effects.sort_by_key(|(kart, _, tick)| (*tick, *kart));
        let mut rams = log.rams.clone();
        rams.retain(|(.., tick)| *tick <= cutoff);
        rams.sort_by_key(|(kart, by, _, tick)| (*tick, *kart, *by));
        let mut retargets = log.retargets.clone();
        retargets.retain(|(.., tick)| *tick <= cutoff);
        retargets.sort_by_key(|(id, _, tick)| (*tick, id.owner, id.seq));
        (effects, rams, retargets)
    };
    let (m, n) = (more(a), more(b));
    assert_eq!(m.0, n.0, "the same chantilly and dégonflages");
    assert_eq!(m.1, n.1, "the same karts knocked over");
    assert_eq!(m.2, n.2, "the same oreillers after the same karts");
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
        "the same story on both screens (until tick {cutoff}): {} boxes and croquettes taken, {} items out, {} items broken on karts ({landed} hits), {} croquette losses, {} effects, {} karts knocked over, {} oreillers retargeted",
        counts.0,
        counts.1,
        x.len(),
        counts.2,
        m.0.len(),
        m.1.len(),
        m.2.len()
    );
}

// --- Issue #61: the chantilly, the super câlin, the fusée, the dégonflage, the oreiller géant ---

/// `key`'s player kart: its item effects, on its own screen.
fn item_effects(room: &Room, key: PeerKey) -> ItemEffects {
    let app = room.view(key);
    *app.world().get::<ItemEffects>(player_of(app)).unwrap()
}

/// What `key`'s screen shows of the kart of `of`: its item effects (its states say them).
fn shown_effects(app: &App, of: PeerKey) -> ItemEffects {
    let proxy = app
        .world()
        .resource::<KartSync>()
        .remote(of)
        .and_then(|remote| remote.proxy)
        .expect("the other kart");
    *app.world().get::<ItemEffects>(proxy).unwrap()
}

fn tuning(room: &Room) -> ItemTuning {
    room.view(ANA).world().resource::<ItemTuning>().clone()
}

/// Issue #61 acceptance: Bob, behind, uses a chantilly: Ana's kart, ahead in his ranking, gets
/// the cream on her screen, as her game decides; his own kart does not. Ana's chantilly reaches
/// no one (she leads). Invincible, Ana's kart is spared. Both screens tell the same story.
#[test]
fn a_chantilly_creams_the_karts_ahead_on_their_own_screens() {
    let mut room = racing();
    for _ in 0..10 {
        room.frame();
    }
    let (ana_id, bob_id) = (id_of(room.view(ANA)), id_of(room.view(BOB)));
    // Ana has the pole: ahead in both rankings.
    for key in [ANA, BOB] {
        assert_eq!(
            standings(room.app(key)),
            vec![usize::from(ana_id), usize::from(bob_id)],
            "{key:?}"
        );
    }
    let tuning = tuning(&room);
    give(&mut room, BOB, ItemKind::Chantilly, 1);
    use_item(&mut room, BOB);
    room.until(1.0, "the cream on Ana's screen", |app| {
        !lobby(app).is_host()
            || app
                .world()
                .get::<ItemEffects>(player_of(app))
                .unwrap()
                .creamed
                > 0.0
    });
    let ana = item_effects(&room, ANA);
    assert!(ana.creamed > tuning.chantilly_time - 0.5, "{ana:?}");
    assert_eq!(ana.creamed_for, tuning.chantilly_time);
    assert_eq!(item_effects(&room, BOB).creamed, 0.0, "not its user");
    // Ana's own chantilly: no one ahead of her.
    give(&mut room, ANA, ItemKind::Chantilly, 1);
    use_item(&mut room, ANA);
    room.until(1.0, "told", |app| items(app).log.effects.len() == 2);
    for _ in 0..10 {
        room.frame();
    }
    assert_eq!(item_effects(&room, BOB).creamed, 0.0, "behind her");
    // The cream gone; Ana invincible: Bob's next chantilly spares her.
    room.until(5.0, "the cream gone", |app| {
        app.world()
            .get::<ItemEffects>(player_of(app))
            .unwrap()
            .creamed
            == 0.0
    });
    {
        let app = room.app(ANA);
        let kart = player_of(app);
        app.world_mut()
            .get_mut::<Invincibility>(kart)
            .unwrap()
            .remaining = 3.0;
    }
    give(&mut room, BOB, ItemKind::Chantilly, 1);
    use_item(&mut room, BOB);
    room.until(1.0, "told", |app| items(app).log.effects.len() == 3);
    for _ in 0..10 {
        room.frame();
    }
    assert_eq!(item_effects(&room, ANA).creamed, 0.0, "invincible");
    let (a, b) = (&items(room.view(ANA)).log, &items(room.view(BOB)).log);
    assert_eq!(a.effects, b.effects, "the same story");
    let users: Vec<(KartId, ItemKind)> = a.effects.iter().map(|(k, i, _)| (*k, *i)).collect();
    assert_eq!(
        users,
        vec![
            (bob_id, ItemKind::Chantilly),
            (ana_id, ItemKind::Chantilly),
            (bob_id, ItemKind::Chantilly)
        ]
    );
}

/// Issue #61 acceptance: Bob, last of a race with two bots (Ana's), uses a dégonflage. On Ana's
/// screen, her kart and her bots shrink and slow down for a time that follows their rank in
/// Bob's ranking (the leader the longest, GD § 8), and their items fall; Bob's kart is not
/// deflated; Bob sees Ana's kart deflated (its states say it). A dégonflage told with a ranking
/// that is not Ana's (forged here): her karts follow the ranking told, not hers.
#[test]
fn a_degonflage_follows_the_ranking_of_its_user() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 2, 1);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    for key in [ANA, BOB] {
        let app = room.app(key);
        let kart = player_of(app);
        app.world_mut().entity_mut(kart).remove::<PlayerKart>();
    }
    for _ in 0..5 {
        room.frame();
    }
    let order = standings(room.app(BOB));
    assert_eq!(order, vec![0, 1, 2, 3], "the grid's order on Bob's screen");
    let tuning = tuning(&room);
    let expected = |order: &[usize], slot: usize| {
        let rank = order.iter().position(|s| *s == slot).unwrap() as u32 + 1;
        deflate_time(
            rank,
            order.len(),
            tuning.degonflage_time_first,
            tuning.degonflage_time_last,
        )
    };
    give(&mut room, ANA, ItemKind::Pelote, 1);
    give(&mut room, BOB, ItemKind::Degonflage, 1);
    use_item(&mut room, BOB);
    let deflated = |app: &mut App| -> Vec<(usize, f32, ItemSlots)> {
        let world = app.world_mut();
        let mut all: Vec<(usize, f32, ItemSlots)> = world
            .query_filtered::<(&Racer, &ItemEffects, &ItemSlots), With<Kart>>()
            .iter(world)
            .map(|(racer, effects, slots)| (racer.grid_slot, effects.deflated_for, *slots))
            .collect();
        all.sort_by_key(|(slot, ..)| *slot);
        all
    };
    room.until(1.0, "Ana's karts deflated", |app| {
        !lobby(app).is_host() || app.world().resource::<OnlineItems>().log.effects.len() == 1
    });
    let ana_karts = deflated(room.app(ANA));
    assert_eq!(ana_karts.len(), 3, "Ana's kart and her two bots");
    for (slot, time, slots) in &ana_karts {
        assert!(
            (time - expected(&order, *slot)).abs() < 1e-4,
            "slot {slot}: {time} s"
        );
        assert!(
            slots.hand.is_empty() && slots.dragging.is_none(),
            "slot {slot}: {slots:?}"
        );
    }
    assert!(
        ana_karts[0].1 > ana_karts[2].1,
        "the first ones the longest"
    );
    assert_eq!(item_effects(&room, BOB).deflated, 0.0, "its user");
    room.until(1.0, "Ana deflated on Bob's screen", |app| {
        lobby(app).is_host() || shown_effects(app, ANA).deflated > 0.0
    });
    // A ranking that is not Ana's (Bob's kart first, the bots last): her karts follow it.
    let forged = vec![3, 2, 1, 0];
    {
        let app = room.app(ANA);
        let now = common_tick(app);
        let session = items(app).session().unwrap();
        app.world_mut().resource_mut::<OnlineItems>().receive(
            BOB,
            super::Message::Effect(EffectOut {
                session,
                kart: 3,
                item: ItemKind::Degonflage,
                tick: now,
                position: [0.0; 3],
                forward: [0.0, -1.0],
                order: forged.iter().map(|slot| *slot as KartId).collect(),
            }),
        );
    }
    room.frame();
    assert_ne!(standings(room.app(ANA)), forged, "not Ana's ranking");
    for (slot, time, _) in deflated(room.app(ANA)) {
        assert!(
            (time - expected(&forged, slot)).abs() < 1e-4,
            "slot {slot}: {time} s"
        );
    }
}

/// Bob's kart `ahead` meters before Ana's along the bots' racing line, both on it and still;
/// then the time for each screen to show the other's kart there.
fn on_the_line(room: &mut Room, ahead: f32) {
    let line = room.view(ANA).world().resource::<RacingLine>().clone();
    let ana = pose(room, ANA);
    let here = line.locate(ana.translation, None).distance;
    let at = |distance: f32| {
        line.center()
            .at(distance)
            .transform(line.offset_at(distance), GRID_HEIGHT)
    };
    place(room, ANA, at(here));
    place(room, BOB, at(here + ahead));
    for _ in 0..30 {
        room.frame();
    }
}

/// Issue #61 acceptance: Ana, in a super câlin, drives into Bob's kart (still, ahead): Bob sees
/// her pink glow (her states say it), and his game decides that her kart knocks his over (a
/// tumble); it tells everyone: Ana counts her hit, the same on both screens. Her kart is not hit.
#[test]
fn a_super_calin_knocks_over_the_kart_it_runs_into() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 7.0);
    give(&mut room, ANA, ItemKind::SuperCalin, 1);
    use_item(&mut room, ANA);
    assert!(item_effects(&room, ANA).calin > 6.0);
    room.until(1.0, "the glow on Bob's screen", |app| {
        lobby(app).is_host() || shown_effects(app, ANA).calin > 0.0
    });
    drive(&mut room, ANA, FLAT_OUT);
    room.until(3.0, "Bob knocked over, told", |app| {
        items(app).log.rams.len() == 1
    });
    assert!(
        matches!(
            state(&room, BOB),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        ),
        "{:?}",
        state(&room, BOB)
    );
    assert!(!state(&room, ANA).is_hit(), "invincible");
    assert_eq!(stats(&room, ANA).hits_landed, 1, "Ana's hit, told by Bob");
    assert_eq!(stats(&room, BOB).hits_taken, 1);
    let (ana_id, bob_id) = (id_of(room.view(ANA)), id_of(room.view(BOB)));
    for key in [ANA, BOB] {
        let rams = &items(room.view(key)).log.rams;
        assert_eq!(
            (rams[0].0, rams[0].1, rams[0].2),
            (bob_id, ana_id, ItemKind::SuperCalin),
            "{key:?}"
        );
    }
    assert_eq!(
        items(room.view(ANA)).log.rams,
        items(room.view(BOB)).log.rams
    );
}

/// Issue #61 acceptance: Ana's fusée drives her kart along the racing line (her game's
/// autopilot), Bob sees its shell (her states say it) and her kart coming fast; it knocks his
/// kart over on his screen, as his game decides: a tumble, Ana's hit.
#[test]
fn a_fusee_seen_by_the_other_knocks_it_over() {
    let mut room = racing();
    on_the_line(&mut room, 40.0);
    give(&mut room, ANA, ItemKind::Fusee, 1);
    use_item(&mut room, ANA);
    assert!(item_effects(&room, ANA).rocket > 5.0);
    room.until(1.0, "the shell on Bob's screen", |app| {
        lobby(app).is_host() || shown_effects(app, ANA).rocket > 0.0
    });
    room.until(3.0, "Bob knocked over, told", |app| {
        items(app).log.rams.len() == 1
    });
    assert!(
        matches!(
            state(&room, BOB),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        ),
        "{:?}",
        state(&room, BOB)
    );
    assert!(!state(&room, ANA).is_hit());
    assert_eq!(stats(&room, ANA).hits_landed, 1);
    for key in [ANA, BOB] {
        assert_eq!(
            items(room.view(key)).log.rams[0].2,
            ItemKind::Fusee,
            "{key:?}"
        );
    }
    // Over: the shell is gone on Bob's screen too.
    room.until(7.0, "the fusée over", |app| {
        lobby(app).is_host() || shown_effects(app, ANA).rocket == 0.0
    });
}

/// The oreiller géant in a game (its network id and the object), if one is out.
fn pillow_of(app: &App) -> Option<(ItemId, ItemObject)> {
    items(app).objects().find_map(|(entity, id)| {
        app.world()
            .get::<ItemObject>(entity)
            .filter(|o| o.item == ItemKind::OreillerGeant)
            .map(|o| (id, *o))
    })
}

/// Issue #61 acceptance: Ana throws an oreiller géant; the leader of her ranking is Bob's kart,
/// ahead: the oreiller goes after it, by its id, on both screens. On Bob's screen it crushes his
/// kart (a tumble), as his game decides; Ana's kart is not hit. The same story on both screens,
/// and no oreiller left in either record.
#[test]
fn an_oreiller_crushes_the_leader_where_its_game_decides() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 20.0);
    let bob_id = id_of(room.view(BOB));
    for key in [ANA, BOB] {
        assert_eq!(standings(room.app(key))[0], usize::from(bob_id), "{key:?}");
    }
    give(&mut room, ANA, ItemKind::OreillerGeant, 1);
    use_item(&mut room, ANA);
    room.until(1.0, "the oreiller on both screens", |app| {
        pillow_of(app).is_some()
    });
    let bob_kart = player_of(room.view(BOB));
    let (_, on_bob) = pillow_of(room.view(BOB)).unwrap();
    assert!(
        matches!(on_bob.motion, Motion::Pillow { target: Some(target), .. } if target == bob_kart),
        "after Bob's kart: {:?}",
        on_bob.motion
    );
    for key in [ANA, BOB] {
        assert_eq!(
            items(room.view(key)).arbiter.pillows().len(),
            1,
            "{key:?}: it flies"
        );
    }
    room.until(4.0, "Bob crushed", |app| {
        items(app).log.breaks.len() == 1 && pillow_of(app).is_none()
    });
    assert!(
        matches!(
            state(&room, BOB),
            KartState::Hit {
                reaction: HitReaction::Tumble,
                ..
            }
        ),
        "{:?}",
        state(&room, BOB)
    );
    assert!(!state(&room, ANA).is_hit());
    assert_eq!(stats(&room, ANA).hits_landed, 1, "Ana's hit, told by Bob");
    for _ in 0..5 {
        room.frame();
    }
    for key in [ANA, BOB] {
        let items = items(room.view(key));
        assert_eq!(items.log.thrown[0].1, ItemKind::OreillerGeant);
        let breaks = &items.log.breaks;
        assert_eq!((breaks[0].1, breaks[0].2), (Some(bob_id), true), "{key:?}");
        assert!(items.arbiter.pillows().is_empty(), "{key:?}: gone");
    }
}

/// Issue #61 acceptance: Ana's oreiller géant hovers over Bob's kart, the leader; Bob sounds his
/// klaxon-canard: his game destroys it and tells everyone: it is gone on both screens, Bob is not
/// hit.
#[test]
fn a_klaxon_destroys_the_oreiller_of_another_player() {
    let mut room = racing();
    bob_ahead_of_ana(&mut room, 20.0);
    give(&mut room, ANA, ItemKind::OreillerGeant, 1);
    use_item(&mut room, ANA);
    room.until(2.0, "the oreiller over Bob, on his screen", |app| {
        lobby(app).is_host()
            || pillow_of(app).is_some_and(|(_, o)| {
                matches!(
                    o.motion,
                    Motion::Pillow {
                        flight: PillowFlight::Hover { .. },
                        ..
                    }
                )
            })
    });
    for _ in 0..10 {
        room.frame();
    }
    give(&mut room, BOB, ItemKind::KlaxonCanard, 1);
    use_item(&mut room, BOB);
    room.until(1.0, "the oreiller gone everywhere", |app| {
        pillow_of(app).is_none() && items(app).log.honks.len() == 1
    });
    for _ in 0..60 {
        room.frame();
        assert!(!state(&room, BOB).is_hit(), "never crushed");
    }
    for key in [ANA, BOB] {
        let items = items(room.view(key));
        assert_eq!(items.log.breaks.len(), 1, "{key:?}");
        assert_eq!(
            (items.log.breaks[0].1, items.log.breaks[0].2),
            (None, false)
        );
        assert!(items.arbiter.pillows().is_empty(), "{key:?}");
    }
    assert_eq!(stats(&room, ANA).hits_landed, 0);
}

/// Issue #61 acceptance: the oreiller géant's rules online are the host's, on the common clock
/// (shortened for the test: 3 s of race before the first one, 4 s between two, instead of 20 s
/// and 30 s), with a table that almost always gives one. Ana and Bob draw one on the same tick
/// (two boxes) before 3 s: the host refuses both, each gets the same draw without it (a pelure),
/// as solo. After 3 s, on the same tick again: only one is granted (Ana's: the better grid slot),
/// Bob gets a pelure; the same record on both screens. Ana's falls (her peer tells): within 4 s
/// of it, Bob's next one is refused; after, granted.
#[test]
fn two_oreillers_drawn_at_once_online_only_one_is_granted() {
    let mut room = racing();
    let table =
        ItemTable::from_ron("(rows: [(from: 0.0, items: [(OreillerGeant, 99), (Pelure, 1)])])")
            .unwrap();
    for key in [ANA, BOB] {
        let world = room.app(key).world_mut();
        world.insert_resource(OnlineItemTable(table.clone()));
        let mut tuning = world.resource_mut::<ItemTuning>();
        tuning.oreiller_min_race_time = 3.0;
        tuning.oreiller_cooldown = 4.0;
    }
    let (ana_id, bob_id) = (id_of(room.view(ANA)), id_of(room.view(BOB)));
    // Each kart on its own box (0 and 3, across the road), on the same tick; what it drew.
    let draw = |room: &mut Room, keys: &[PeerKey]| {
        for key in keys {
            let index = if *key == ANA { 0 } else { 3 };
            let target = spot(room.app(*key), index, false);
            let road = target.translation - Vec3::Y * (ITEM_BOX_HEIGHT - GRID_HEIGHT);
            let app = room.app(*key);
            let kart = player_of(app);
            app.world_mut().get_mut::<ItemSlots>(kart).unwrap().clear();
            place(
                room,
                *key,
                Transform {
                    translation: road,
                    ..target
                },
            );
        }
        room.frame();
        room.until(1.0, "the host's verdicts", |app| {
            items(app).arbiter.pending().count() == 0
        });
        for _ in 0..3 {
            room.frame();
        }
        // Away from the boxes (they come back without the karts on them).
        for key in keys {
            let at = pose(room, *key);
            place(
                room,
                *key,
                Transform {
                    translation: at.translation - *at.forward() * 8.0,
                    ..at
                },
            );
        }
    };
    let in_hand = |room: &Room, key: PeerKey| slots(room, key).hand.item().map(|(item, _)| item);
    let granted = |app: &App| -> Vec<KartId> {
        items(app)
            .arbiter
            .grants()
            .iter()
            .filter(|g| matches!(g.spot, Spot::Pillow(_)))
            .map(|g| g.kart)
            .collect()
    };
    // Before 3 s of race: refused, the other draw instead.
    assert!(room.view(ANA).world().resource::<Race>().race_ticks() < 120);
    draw(&mut room, &[ANA, BOB]);
    for key in [ANA, BOB] {
        assert_eq!(in_hand(&room, key), Some(ItemKind::Pelure), "{key:?}");
        assert!(granted(room.view(key)).is_empty());
        assert_eq!(items(room.view(key)).log.refused.len(), 1, "{key:?}");
    }
    // Bob's draw as Ana's screen draws it again: the pelure too.
    let drawn = &items(room.view(ANA)).log.draws;
    assert_eq!(drawn.last().unwrap().2, vec![(ItemKind::Pelure, 1)]);
    // After 3 s (the boxes back): the same tick, one oreiller only, Ana's.
    room.until(6.0, "3 s of race", |app| {
        app.world().resource::<Race>().race_ticks() > 190
    });
    draw(&mut room, &[ANA, BOB]);
    assert_eq!(in_hand(&room, ANA), Some(ItemKind::OreillerGeant));
    assert_eq!(in_hand(&room, BOB), Some(ItemKind::Pelure));
    for key in [ANA, BOB] {
        let app = room.view(key);
        assert_eq!(granted(app), vec![ana_id], "{key:?}");
        let pillows = items(app).arbiter.pillows();
        assert_eq!(pillows.len(), 1, "{key:?}");
        assert_eq!(pillows[0].kart, ana_id);
    }
    let (kart, _, drawn) = items(room.view(ANA)).log.draws.last().unwrap();
    assert_eq!(
        (*kart, drawn.as_slice()),
        (bob_id, &[(ItemKind::Pelure, 1)][..])
    );
    let (kart, _, drawn) = items(room.view(BOB)).log.draws.last().unwrap();
    assert_eq!(
        (*kart, drawn.as_slice()),
        (ana_id, &[(ItemKind::OreillerGeant, 1)][..])
    );
    // Ana's oreiller falls: her game tells, both records forget it.
    {
        let app = room.app(ANA);
        let kart = player_of(app);
        app.world_mut().get_mut::<ItemSlots>(kart).unwrap().clear();
    }
    room.until(1.0, "Ana's oreiller gone", |app| {
        items(app).arbiter.pillows().is_empty()
    });
    // Within 4 s of it (the boxes back after 2 s): refused.
    for _ in 0..125 {
        room.frame();
    }
    draw(&mut room, &[BOB]);
    assert_eq!(in_hand(&room, BOB), Some(ItemKind::Pelure), "the cooldown");
    // 4 s after it: granted.
    room.until(3.0, "box 3 back", |app| back_on_now(app, 3));
    draw(&mut room, &[BOB]);
    assert_eq!(in_hand(&room, BOB), Some(ItemKind::OreillerGeant));
    for key in [ANA, BOB] {
        assert_eq!(granted(room.view(key)), vec![ana_id, bob_id], "{key:?}");
    }
}

/// Whether box `index` is there (not taken), in a game.
fn back_on_now(app: &App, index: u16) -> bool {
    let world = app.world();
    world
        .try_query::<(&SpotIndex, &ItemBox)>()
        .is_some_and(|mut boxes| {
            boxes
                .iter(world)
                .any(|(spot, b)| spot.0 == index && b.available())
        })
}

/// Gives `kart` (a game's) the next item of `queue` when its slots are empty (nothing held out):
/// the players start with every item of phase 2, one after the other.
pub(super) fn feed(app: &mut App, kart: Entity, queue: &mut Vec<ItemKind>) {
    let Some(mut slots) = app.world_mut().get_mut::<ItemSlots>(kart) else {
        return;
    };
    if slots.hand.is_empty()
        && slots.reserve.is_empty()
        && slots.dragging.is_none()
        && !queue.is_empty()
    {
        let item = queue.remove(0);
        slots.give(item, 1, 0);
    }
}

/// Issue #61 acceptance (in memory): a race with every item to the end, two players driven by
/// bots and 4 bots (Ana's): the players get, one after the other, the items of phase 2 (Ana: a
/// chantilly, a fusée, a bombe, an oreiller géant; Bob: a dégonflage, a super câlin, an oreiller
/// géant, a klaxon-canard), a bot of Ana's a super câlin, on top of what the boxes give (every
/// item now). The same results on both screens, and the same story of the items (the effects,
/// the karts knocked over, the oreillers' targets too).
#[test]
fn a_race_with_every_item_ends_the_same_for_everyone() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    start_race(&mut room, ANA, 4, 1);
    let drivers = [player_of(room.view(ANA)), player_of(room.view(BOB))];
    bots_drive_the_players(&mut room);
    room.until(8.0, "GO", |app| app.world().resource::<Race>().started());
    let mut queues = [
        vec![
            ItemKind::Chantilly,
            ItemKind::Fusee,
            ItemKind::Bombe,
            ItemKind::OreillerGeant,
        ],
        vec![
            ItemKind::Degonflage,
            ItemKind::SuperCalin,
            ItemKind::OreillerGeant,
            ItemKind::KlaxonCanard,
        ],
    ];
    {
        let world = room.app(ANA).world_mut();
        let mut bots = world.query_filtered::<(&Racer, &mut ItemSlots), With<crate::bots::Bot>>();
        for (racer, mut slots) in bots.iter_mut(world) {
            if racer.grid_slot == 1 {
                slots.give(ItemKind::SuperCalin, 1, 0);
            }
        }
    }
    let mut frames = 0;
    while room.view(ANA).world().resource::<Race>().phase() != RacePhase::Results
        || room.view(BOB).world().resource::<Race>().phase() != RacePhase::Results
    {
        for (index, key) in [ANA, BOB].into_iter().enumerate() {
            feed(room.app(key), drivers[index], &mut queues[index]);
        }
        room.frame();
        frames += 1;
        assert!(frames < 150 * 60, "the race never ended");
    }
    for _ in 0..10 {
        room.frame();
    }
    let (ana, bob) = (
        progress_by_slot(room.app(ANA)),
        progress_by_slot(room.app(BOB)),
    );
    assert_same_progress(&ana, &bob);
    assert_eq!(standings(room.app(ANA)), standings(room.app(BOB)));
    let (ana_id, bob_id) = (
        room.view(ANA).world().get::<NetKart>(drivers[0]).unwrap().0,
        room.view(BOB).world().get::<NetKart>(drivers[1]).unwrap().0,
    );
    for key in [ANA, BOB] {
        let items = items(room.view(key));
        println!("{key:?}: {}", items.summary());
        let effects: Vec<(KartId, ItemKind)> =
            items.log.effects.iter().map(|(k, i, _)| (*k, *i)).collect();
        assert!(
            effects.contains(&(ana_id, ItemKind::Chantilly)),
            "{effects:?}"
        );
        assert!(
            effects.contains(&(bob_id, ItemKind::Degonflage)),
            "{effects:?}"
        );
        assert!(
            items.log.thrown.iter().any(|(id, item, _)| {
                *item == ItemKind::OreillerGeant && (id.owner == ana_id || id.owner == bob_id)
            }),
            "{}",
            items.summary()
        );
    }
    // Each player used at least its first two (the chantilly and the fusée, the dégonflage and
    // the super câlin).
    for (index, key) in [ANA, BOB].into_iter().enumerate() {
        let app = room.view(key);
        let used = app.world().get::<ItemStats>(drivers[index]).unwrap().used;
        assert!(
            used >= 2,
            "{key:?} used {used} items, left: {:?}",
            queues[index]
        );
    }
    let (ana, bob) = (room.view(ANA), room.view(BOB));
    assert_same_story(ana, bob);
}
