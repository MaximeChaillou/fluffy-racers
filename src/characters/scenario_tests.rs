//! Headless scenarios of the characters and the selection screen (issue #15): the whole game
//! (`GamePlugin`) with `MinimalPlugins`, one fixed tick per update. The egui interface is not
//! there (no renderer): the keys and the `SelectionAction`s its buttons write are.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy_rapier3d::prelude::ColliderMassProperties;

use super::*;
use crate::bots::{Bot, bot_loadout};
use crate::kart::{
    DriverVisual, Kart, KartStats, MaxSpeedFactors, MaxSpeedScale, PlayerKart, Stat, StatEffects,
};
use crate::race::{Race, RaceGrid, RacePhase, Racer};
use crate::tests::{app_on, app_with_bots, player};
use crate::track::TrackChoice;

/// A key pressed this frame (`just_pressed`), as the window sends it.
fn tap(app: &mut App, key_code: KeyCode, logical_key: Key) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: logical_key.clone(),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

fn screen(app: &App) -> SelectionScreen {
    *app.world().resource::<SelectionScreen>()
}

fn paused(app: &App) -> bool {
    app.world().resource::<Time<Virtual>>().is_paused()
}

fn mass(app: &App, kart: Entity) -> f32 {
    match app.world().get::<ColliderMassProperties>(kart) {
        Some(ColliderMassProperties::MassProperties(properties)) => properties.mass,
        other => panic!("{other:?}"),
    }
}

/// Runs updates until the player kart and, in a race, the race exist (after a `LoadLevel`).
fn wait_for_the_level(app: &mut App) {
    for _ in 0..200 {
        app.update();
        let world = app.world_mut();
        let players = world
            .query_filtered::<(), With<PlayerKart>>()
            .iter(world)
            .count();
        if players == 1 && world.contains_resource::<Race>() {
            return;
        }
    }
    panic!("the level was never rebuilt");
}

/// Issue #15: without choosing, the player drives the default loadout, the middle level: every
/// multiplier 1, the usual mass and top speed. The selection screen does not open in the
/// headless game (it needs a renderer), so it never holds the tests up.
#[test]
fn the_player_drives_the_default_loadout_at_the_middle_level() {
    let mut app = app_on(TrackChoice::TestGround);
    assert!(!screen(&app).open);
    assert!(!paused(&app));
    let kart = player(&mut app);
    let roster = app.world().resource::<Roster>().clone();
    let loadout = app.world().resource::<PlayerLoadout>().0;
    assert_eq!(loadout, roster.default_loadout());
    let world = app.world();
    assert_eq!(world.get::<KartStats>(kart), Some(&KartStats::default()));
    assert!(world.get::<StatEffects>(kart).unwrap().is_neutral());
    assert_eq!(world.get::<MaxSpeedFactors>(kart).unwrap().stats, 1.0);
    assert_eq!(world.get::<MaxSpeedScale>(kart), Some(&MaxSpeedScale(1.0)));
    assert_eq!(mass(&app, kart), 150.0);
    // The driver: the medium size.
    let world = app.world_mut();
    let sizes: Vec<f32> = world
        .query::<&DriverVisual>()
        .iter(world)
        .map(|d| d.size)
        .collect();
    assert_eq!(sizes, [1.0]);
}

/// Issue #15: G opens the selection screen and pauses the game; the arrows pick the parts (the
/// stats follow), Entrée races with them: the level is rebuilt, the player's kart has the new
/// stats and driver, the race starts over from the grid, and the choice stays for the next
/// restart. W/A/S/D work too (Z/Q/S/D on an AZERTY keyboard).
#[test]
fn the_selection_screen_picks_the_parts_and_restarts_the_race() {
    let mut app = app_with_bots(TrackChoice::default(), 2);
    wait_for_the_level(&mut app);
    for _ in 0..90 {
        app.update();
    }
    let race_tick = |app: &App| app.world().resource::<Race>().flow.tick;
    assert!(race_tick(&app) > 60);
    let old_player = player(&mut app);
    let roster = app.world().resource::<Roster>().clone();

    tap(&mut app, KeyCode::KeyG, Key::Character("g".into()));
    assert!(screen(&app).open);
    assert_eq!(screen(&app).choice, roster.default_loadout());
    assert!(paused(&app));
    // Paused: the race clock does not move.
    let frozen = race_tick(&app);
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(race_tick(&app), frozen);

    // Character: Chat → Lapin → Renard → Ours (right three times), then down to the kart:
    // Caisse à savon → Baignoire (D, the right of W/A/S/D), down to the wheels (S), right to
    // the tractor tyres.
    for _ in 0..3 {
        tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    }
    assert_eq!(roster.character(&screen(&app).choice).name, "Ours");
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    assert_eq!(screen(&app).kind(), PartKind::Kart);
    tap(&mut app, KeyCode::KeyD, Key::Character("d".into()));
    tap(&mut app, KeyCode::KeyS, Key::Character("s".into()));
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight);
    // Up twice and back: the rows wrap around (wheels → kart → character → wing → character).
    tap(&mut app, KeyCode::KeyW, Key::Character("w".into()));
    tap(&mut app, KeyCode::ArrowUp, Key::ArrowUp);
    tap(&mut app, KeyCode::ArrowUp, Key::ArrowUp);
    assert_eq!(screen(&app).kind(), PartKind::Wing);
    tap(&mut app, KeyCode::ArrowDown, Key::ArrowDown);
    let choice = screen(&app).choice;
    let names = PartKind::ALL.map(|kind| roster.name(kind, choice.get(kind)).to_string());
    assert_eq!(
        names,
        ["Ours", "Baignoire", "Pneus de tracteur", "Taie d'oreiller"]
    );
    // Nothing applied yet.
    assert_eq!(
        app.world().resource::<PlayerLoadout>().0,
        roster.default_loadout()
    );

    tap(&mut app, KeyCode::Enter, Key::Enter);
    assert!(!screen(&app).open);
    assert!(!paused(&app));
    assert_eq!(app.world().resource::<PlayerLoadout>().0, choice);
    wait_for_the_level(&mut app);
    let kart = player(&mut app);
    assert_ne!(kart, old_player);
    assert!(app.world().get_entity(old_player).is_err());
    let stats = roster.stats(&choice);
    assert_eq!(app.world().get::<KartStats>(kart), Some(&stats));
    assert_eq!(stats.level(Stat::Speed), 15);
    assert_eq!(stats.level(Stat::Weight), 16);
    let factors = *app.world().get::<MaxSpeedFactors>(kart).unwrap();
    assert!((factors.stats - 1.04).abs() < 1e-5, "{factors:?}");
    assert!(mass(&app, kart) > 170.0, "{}", mass(&app, kart));
    assert_eq!(app.world().resource::<Race>().phase(), RacePhase::Grid);
    // A big bear in the player's kart (the two bots: a mouse and a cat).
    let world = app.world_mut();
    let mut sizes: Vec<f32> = world
        .query::<&DriverVisual>()
        .iter(world)
        .map(|d| d.size)
        .collect();
    sizes.sort_by(f32::total_cmp);
    assert_eq!(sizes, [0.78, 1.0, 1.25]);

    // A restart keeps the choice.
    app.world_mut()
        .write_message(crate::level::LoadLevel::restart(
            crate::race::RaceMode::Race,
        ));
    wait_for_the_level(&mut app);
    let kart = player(&mut app);
    assert_eq!(app.world().get::<KartStats>(kart), Some(&stats));
}

/// Issue #15: Échap (or G again) closes the screen without changing anything: no new level,
/// the game goes on.
#[test]
fn closing_the_selection_screen_changes_nothing() {
    let mut app = app_with_bots(TrackChoice::default(), 0);
    wait_for_the_level(&mut app);
    let kart = player(&mut app);
    for (close, logical) in [
        (KeyCode::Escape, Key::Escape),
        (KeyCode::KeyG, Key::Character("g".into())),
    ] {
        tap(&mut app, KeyCode::KeyG, Key::Character("g".into()));
        assert!(screen(&app).open && paused(&app));
        tap(&mut app, KeyCode::ArrowLeft, Key::ArrowLeft);
        assert_ne!(
            screen(&app).choice,
            app.world().resource::<PlayerLoadout>().0
        );
        tap(&mut app, close, logical);
        assert!(!screen(&app).open);
        assert!(!paused(&app));
        let roster = app.world().resource::<Roster>().clone();
        assert_eq!(
            app.world().resource::<PlayerLoadout>().0,
            roster.default_loadout()
        );
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(player(&mut app), kart, "the level was not rebuilt");
    }
    // The buttons of the screen: a click on « Course ! » is a `Confirm`.
    app.world_mut().write_message(SelectionAction::Open);
    app.world_mut()
        .write_message(SelectionAction::Pick(PartKind::Character, 0));
    app.world_mut().write_message(SelectionAction::Confirm);
    app.update();
    wait_for_the_level(&mut app);
    let roster = app.world().resource::<Roster>().clone();
    let me = player(&mut app);
    assert_eq!(
        roster
            .character(&app.world().resource::<PlayerLoadout>().0)
            .name,
        "Souris"
    );
    assert_eq!(
        app.world().get::<KartStats>(me),
        Some(&roster.stats(&{
            let mut l = roster.default_loadout();
            l.character = 0;
            l
        }))
    );
}

/// Issue #15: the bots of the game's grid get their own characters and parts (`bot_loadout`):
/// their stats, top speed factor and driver follow, and they are not all alike.
#[test]
fn the_bots_race_with_their_own_characters_and_parts() {
    let mut app = app_with_bots(TrackChoice::default(), 11);
    wait_for_the_level(&mut app);
    for _ in 0..3 {
        app.update();
    }
    let roster = app.world().resource::<Roster>().clone();
    let seed = app.world().resource::<RaceGrid>().seed;
    let world = app.world_mut();
    let bots: Vec<(usize, KartStats, StatEffects, MaxSpeedFactors)> = world
        .query_filtered::<(&Racer, &KartStats, &StatEffects, &MaxSpeedFactors), With<Bot>>()
        .iter(world)
        .map(|(racer, stats, effects, factors)| (racer.grid_slot, *stats, *effects, *factors))
        .collect();
    assert_eq!(bots.len(), 11);
    for (slot, stats, effects, factors) in &bots {
        assert_eq!(
            *stats,
            roster.stats(&bot_loadout(&roster, seed, *slot)),
            "slot {slot}"
        );
        assert_eq!(factors.stats, effects.top_speed);
    }
    let mut distinct: Vec<KartStats> = Vec::new();
    for (_, stats, ..) in &bots {
        if !distinct.contains(stats) {
            distinct.push(*stats);
        }
    }
    assert!(distinct.len() >= 8, "{distinct:?}");
    // Their drivers: the three sizes of the weight classes, on 12 karts.
    let mut sizes: Vec<f32> = world
        .query::<&DriverVisual>()
        .iter(world)
        .map(|d| d.size)
        .collect();
    assert_eq!(sizes.len(), 12);
    sizes.sort_by(f32::total_cmp);
    sizes.dedup();
    assert!(sizes.len() >= 6, "{sizes:?}");
    let karts = world.query_filtered::<(), With<Kart>>().iter(world).count();
    assert_eq!(karts, 12);
}

/// Issue #15: the bounds of the stats are in the admin panel (`StatsTuning`, by reflection, like
/// every tuning resource): a value edited there applies at the next restart, when the karts
/// spawn again.
#[test]
fn a_stats_tuning_edited_in_the_admin_panel_applies_at_the_restart() {
    use crate::admin::{Tunings, Value};
    let mut app = app_on(TrackChoice::TestGround);
    let roster = app.world().resource::<Roster>().clone();
    let mut bear = roster.default_loadout();
    bear.character = roster.find(PartKind::Character, "Ours").unwrap();
    app.world_mut().resource_mut::<PlayerLoadout>().0 = bear;
    let restart = |app: &mut App| {
        let old = player(app);
        app.world_mut()
            .write_message(crate::level::LoadLevel::restart(
                crate::race::RaceMode::Race,
            ));
        for _ in 0..20 {
            app.update();
            let world = app.world_mut();
            let karts: Vec<Entity> = world
                .query_filtered::<Entity, With<PlayerKart>>()
                .iter(world)
                .collect();
            if karts.len() == 1 && karts[0] != old {
                return world.get::<MaxSpeedFactors>(karts[0]).unwrap().stats;
            }
        }
        panic!("the level was never rebuilt");
    };
    // The bear with the standard parts: Speed 6 + 3 + 2 + 1 = 12, 1 + 0.08 × 0.2.
    let speed = restart(&mut app);
    assert!((speed - 1.016).abs() < 1e-5, "{speed}");
    app.world_mut()
        .resource_scope(|world, tunings: Mut<Tunings>| {
            tunings.set(world, "StatsTuning.speed_top_speed.at_20", Value::F32(1.18))
        })
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<crate::kart::StatsTuning>()
            .speed_top_speed
            .at_20,
        1.18
    );
    let speed = restart(&mut app);
    assert!((speed - 1.036).abs() < 1e-5, "{speed}");
}
