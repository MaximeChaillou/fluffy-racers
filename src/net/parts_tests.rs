//! Headless scenario of the karts' parts online (issue #73): two whole games (`GamePlugin`) in a
//! room, linked by the in-memory socket of the tests (like `paint_tests.rs`). The parts a player
//! chose (its `Hello`'s loadout) are those its kart shows on the other screen; in a race, the
//! bots the host drives show the same parts on the guest's screen.

use super::race_tests::{ANA, BOB, lobby, on_grid, seated, start_race};
use super::*;
use crate::bots::Bot;
use crate::characters::{PartKind, PlayerLoadout, Roster, SelectionAction};
use crate::kart::{Kart, KartBodyPivot, PartLooks, PlayerKart};
use crate::race::Racer;

/// What the visual of the kart `body` shows of its parts.
fn looks_of(app: &mut App, body: Entity) -> Option<PartLooks> {
    let world = app.world_mut();
    world
        .query::<&KartBodyPivot>()
        .iter(world)
        .find(|pivot| pivot.body == body)
        .map(|pivot| pivot.looks)
}

/// What the kart of network id `kart` of another peer shows of its parts on our screen.
fn remote_looks(app: &mut App, kart: KartId) -> Option<PartLooks> {
    let proxy = app
        .world()
        .resource::<KartSync>()
        .remote_kart(kart)?
        .proxy?;
    looks_of(app, proxy)
}

/// Our own kart's parts on our screen.
fn own_looks(app: &mut App) -> Option<PartLooks> {
    let world = app.world_mut();
    let kart = world
        .query_filtered::<Entity, (With<PlayerKart>, With<Kart>)>()
        .iter(world)
        .next()?;
    looks_of(app, kart)
}

/// Issue #73: Bob picks the bathtub, the tractor tyres and the feather on his selection screen
/// (free driving: his kart spawns again with them): on Ana's screen, his kart shows them, and
/// hers the default parts on his. In a race with 4 bots, each bot shows the same parts on the
/// guest's screen as on the host's (its parts drawn from the race's seed), the players theirs.
#[test]
fn the_other_players_and_the_bots_show_the_same_parts_on_every_screen() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob")]);
    let roster = room.view(ANA).world().resource::<Roster>().clone();
    let pick = |kind, name: &str| SelectionAction::Pick(kind, roster.find(kind, name).unwrap());
    for action in [
        SelectionAction::Open,
        pick(PartKind::Kart, "Baignoire"),
        pick(PartKind::Wheels, "Pneus de tracteur"),
        pick(PartKind::Wing, "Plume"),
        SelectionAction::Confirm,
    ] {
        room.app(BOB).world_mut().write_message(action);
    }
    let bob_loadout = {
        room.frame();
        room.view(BOB).world().resource::<PlayerLoadout>().0
    };
    let bob_looks = roster.looks(&bob_loadout);
    assert_ne!(bob_looks, PartLooks::default());
    let seat = |app: &App, peer: PeerKey| {
        lobby(app)
            .seats()
            .iter()
            .position(|key| *key == peer)
            .unwrap() as KartId
    };
    let (ana_kart, bob_kart) = (seat(room.view(ANA), ANA), seat(room.view(ANA), BOB));
    let mut seen = None;
    for _ in 0..600 {
        room.frame();
        let on_ana = remote_looks(room.app(ANA), bob_kart);
        let on_bob = own_looks(room.app(BOB));
        if on_ana == Some(bob_looks) && on_bob == Some(bob_looks) {
            seen = on_ana;
            break;
        }
    }
    assert_eq!(seen, Some(bob_looks), "Bob's parts on Ana's screen");
    assert_eq!(own_looks(room.app(ANA)), Some(PartLooks::default()));
    assert_eq!(
        remote_looks(room.app(BOB), ana_kart),
        Some(PartLooks::default()),
        "Ana's parts on Bob's screen"
    );

    // A race with 4 bots: the host's bots and those the guest shows wear the same parts.
    let start = start_race(&mut room, ANA, 4, 1);
    room.until(3.0, "the bots shown", |app| {
        let sync = app.world().resource::<KartSync>();
        lobby(app).is_host()
            || (0..4).all(|kart| sync.remote_kart(kart).is_some_and(|r| r.proxy.is_some()))
    });
    room.frame();
    let host = room.app(ANA);
    assert!(lobby(host).is_host() && on_grid(host));
    let world = host.world_mut();
    let bots: Vec<(usize, Entity)> = world
        .query_filtered::<(&Racer, Entity), (With<Bot>, With<Kart>)>()
        .iter(world)
        .map(|(racer, entity)| (racer.grid_slot, entity))
        .collect();
    assert_eq!(bots.len(), 4);
    let mut on_host: Vec<(usize, PartLooks)> = bots
        .iter()
        .map(|(slot, entity)| (*slot, looks_of(host, *entity).unwrap()))
        .collect();
    on_host.sort_by_key(|(slot, _)| *slot);
    let guest = room.app(BOB);
    for (slot, looks) in &on_host {
        assert_eq!(
            remote_looks(guest, *slot as KartId),
            Some(*looks),
            "bot {slot}"
        );
    }
    // The bots' parts are drawn: not all the same.
    assert!(
        on_host.iter().any(|(_, looks)| *looks != on_host[0].1),
        "{on_host:?}"
    );
    // The players keep their parts in the race, on both screens.
    let bob_slot = start.slot_of(BOB).unwrap();
    let ana_slot = start.slot_of(ANA).unwrap();
    assert_eq!(remote_looks(room.app(ANA), bob_slot), Some(bob_looks));
    assert_eq!(own_looks(room.app(BOB)), Some(bob_looks));
    assert_eq!(
        remote_looks(room.app(BOB), ana_slot),
        Some(PartLooks::default())
    );
}
