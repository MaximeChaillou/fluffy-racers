//! Headless scenarios of the pseudo (issue #80): three whole games (`GamePlugin`) in a star
//! (Ana hosts, Bob and Cyd are her clients), linked by the in-memory socket of the tests
//! (`socket::memory`, the helpers of `race_tests.rs`), with the tags over the karts
//! ([`NameTagPlugin`], which only comes with a renderer in the game); and a solo game, for the
//! keys typed in the lobby's field.
//!
//! The lobby's egui field is not there (no renderer): a new pseudo is what it writes
//! (`NetSettings::pseudo`, cleaned by `pseudo::clean`), and the field having the keyboard is
//! what its focus sets (`admin::KeyboardCapture`, the field itself: `ui::tests`).

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy_rapier3d::prelude::Velocity;

use super::race_tests::{ANA, BOB, CYD, Room, bots_drive_the_players, lobby, seated, start_race};
use super::tags::NameTag;
use super::*;
use crate::admin::{AdminPanel, KeyboardCapture};
use crate::characters::SelectionScreen;
use crate::items::ItemSlots;
use crate::kart::{Boost, KartInput};
use crate::modes::RaceMenu;
use crate::race::{Race, RacePhase};
use crate::track::TrackChoice;
use crate::ui::DebugOverlay;

/// The names in a game's lobby (its own included), sorted.
fn lobby_names(app: &App) -> Vec<String> {
    let now = now_ms(app.world().resource::<Time<Real>>());
    let mut names: Vec<String> = lobby(app)
        .players(now)
        .into_iter()
        .map(|row| row.name)
        .collect();
    names.sort();
    names
}

/// A game's tags over the karts: (the pseudo, the text it writes), sorted.
fn tags(app: &App) -> Vec<(String, String)> {
    let world = app.world();
    let mut tags: Vec<(String, String)> = world
        .iter_entities()
        .filter_map(|entity| entity.get::<NameTag>())
        .map(|tag| {
            let text = world.get::<Text>(tag.text()).map(|text| text.0.clone());
            (tag.name.clone(), text.unwrap_or_default())
        })
        .collect();
    tags.sort();
    tags
}

/// The names on a game's results screen, sorted.
fn results_names(app: &mut App) -> Vec<String> {
    let mut names: Vec<String> = crate::ui::results_tests::results_lines(app)
        .into_iter()
        .map(|[_, name, ..]| name)
        .collect();
    names.sort();
    names
}

/// `key` types a new pseudo in the lobby's field (what the field gives: `pseudo::clean`).
fn rename(room: &mut Room, key: PeerKey, typed: &str) {
    room.app(key)
        .world_mut()
        .resource_mut::<NetSettings>()
        .pseudo = pseudo::clean(typed);
}

fn pair(name: &str, text: &str) -> (String, String) {
    (name.to_string(), text.to_string())
}

/// Issue #80 acceptance (in memory, a star of three): every game tags the other players' karts
/// (never the bots'), their pseudos in the HUD's letters. Bob, a client, types a new pseudo in the
/// lobby: it reaches the host and, relayed by it, the other client, in the lobby, on its tag and,
/// after the race, on the results screen, its accents included everywhere (issue #100: the HUD
/// writes in Fredoka; they were dropped on the tag and the results before); a new pseudo while
/// the results are shown changes them at once. His kart is renamed where it is, not
/// spawned again.
#[test]
fn a_new_pseudo_reaches_everyone_in_the_lobby_on_the_results_and_over_the_karts() {
    let mut room = seated(&[(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd")]);
    for (_, app) in &mut room.games {
        NameTagPlugin.build(app);
    }
    room.until(2.0, "a tag over each other player's kart", |app| {
        tags(app).len() == 2
    });
    assert_eq!(
        tags(room.view(ANA)),
        [pair("Bob", "Bob"), pair("Cyd", "Cyd")]
    );
    assert_eq!(
        tags(room.view(BOB)),
        [pair("Ana", "Ana"), pair("Cyd", "Cyd")]
    );
    let bobs_kart = |room: &Room, key| {
        room.view(key)
            .world()
            .resource::<KartSync>()
            .remote(BOB)
            .and_then(|remote| remote.proxy)
    };
    let (at_ana, at_cyd) = (bobs_kart(&room, ANA), bobs_kart(&room, CYD));
    assert!(at_ana.is_some() && at_cyd.is_some());

    rename(&mut room, BOB, "  Éloïse 2 😀");
    let renamed = ["Ana", "Cyd", "Éloïse 2"];
    room.until(2.0, "Bob's new pseudo in every lobby", |app| {
        lobby_names(app) == renamed
    });
    // Bob has no tag of his own.
    room.until(1.0, "Bob's new pseudo over his kart", |app| {
        lobby(app).me() == BOB || tags(app).contains(&pair("Éloïse 2", "Éloïse 2"))
    });
    for (key, other) in [(ANA, "Cyd"), (CYD, "Ana")] {
        assert_eq!(
            tags(room.view(key)),
            [pair(other, other), pair("Éloïse 2", "Éloïse 2")],
            "{key:?}"
        );
    }
    assert_eq!(bobs_kart(&room, ANA), at_ana, "renamed, not spawned again");
    assert_eq!(bobs_kart(&room, CYD), at_cyd);

    // A race: the new pseudo on the results screen of the others.
    start_race(&mut room, ANA, 2, 1);
    bots_drive_the_players(&mut room);
    room.until(150.0, "the results", |app| {
        app.world().resource::<Race>().phase() == RacePhase::Results
    });
    for _ in 0..10 {
        room.frame();
    }
    for key in [ANA, CYD] {
        let names = results_names(room.app(key));
        assert!(
            names.contains(&"Éloïse 2".to_string()),
            "{key:?}: {names:?}"
        );
        assert!(!names.iter().any(|name| name.contains("Bob")), "{names:?}");
    }
    let names = results_names(room.app(BOB));
    assert!(names.contains(&"Ana".to_string()) && names.contains(&"Cyd".to_string()));
    // The bots have no tag.
    for (key, app) in &room.games {
        assert_eq!(tags(app).len(), 2, "{key:?}: {:?}", tags(app));
    }

    // Renamed again on the results screen: the lines follow at once.
    let in_race = bobs_kart(&room, ANA);
    assert!(in_race.is_some());
    rename(&mut room, BOB, "Zoé");
    room.until(2.0, "Bob's second pseudo in every lobby", |app| {
        lobby_names(app) == ["Ana", "Cyd", "Zoé"]
    });
    for _ in 0..3 {
        room.frame();
    }
    for key in [ANA, CYD] {
        let names = results_names(room.app(key));
        assert!(names.contains(&"Zoé".to_string()), "{key:?}: {names:?}");
        assert!(
            !names.iter().any(|name| name.contains("Éloïse")),
            "{names:?}"
        );
        assert!(
            tags(room.view(key)).contains(&pair("Zoé", "Zoé")),
            "{key:?}"
        );
    }
    assert_eq!(bobs_kart(&room, ANA), in_race, "renamed, not spawned again");
}

/// Issue #80: solo, no room: no tag, and nothing of the tags runs.
#[test]
fn solo_has_no_name_tag() {
    let mut app = crate::tests::app_on(TrackChoice::TestGround);
    NameTagPlugin.build(&mut app);
    for _ in 0..30 {
        app.update();
    }
    assert!(app.world().get_resource::<KartSync>().is_none());
    assert!(tags(&app).is_empty());
}

/// A key pressed then released, a frame each, as the window sends it.
fn tap(app: &mut App, key_code: KeyCode) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

/// What the game's shortcuts would change: the windows, the player's items and boost, the debug
/// overlay.
fn shortcuts_state(app: &mut App) -> (bool, bool, bool, bool, bool, String, String) {
    let kart = crate::tests::player(app);
    let world = app.world_mut();
    let overlay = world
        .query_filtered::<&Visibility, With<DebugOverlay>>()
        .iter(world)
        .map(|visibility| format!("{visibility:?}"))
        .collect::<String>();
    let kart_state = format!(
        "{:?} {:?}",
        world.get::<ItemSlots>(kart),
        world.get::<Boost>(kart)
    );
    (
        world.resource::<AdminPanel>().open,
        world.resource::<SelectionScreen>().open,
        world.resource::<crate::options::OptionsMenu>().open,
        world.resource::<LobbyWindow>().open,
        world.resource::<RaceMenu>().open,
        overlay,
        kart_state,
    )
}

fn speed(app: &mut App) -> f32 {
    let kart = crate::tests::player(app);
    app.world().get::<Velocity>(kart).unwrap().linear.length()
}

/// Issue #80: while the pseudo's field has the keyboard (the lobby open, the panel closed), the
/// letters and keys typed reach neither the kart nor a shortcut: P, G, O, L, N, I, R, B, C, F3,
/// Échap, Entrée... do nothing, the accelerator held does not drive. Once the field lets go
/// (Entrée, a click elsewhere), the keys are the game's again: L closes the lobby, the up arrow
/// drives.
#[test]
fn typing_a_pseudo_neither_drives_nor_triggers_a_shortcut() {
    let mut app = crate::tests::app_on(TrackChoice::TestGround);
    app.world_mut().resource_mut::<LobbyWindow>().open = true;
    app.world_mut().resource_mut::<KeyboardCapture>().0 = true;
    assert!(!app.world().resource::<AdminPanel>().open);
    let before = shortcuts_state(&mut app);
    let typed = [
        KeyCode::KeyZ,
        KeyCode::KeyO,
        KeyCode::Digit2,
        KeyCode::KeyP,
        KeyCode::KeyG,
        KeyCode::KeyL,
        KeyCode::KeyN,
        KeyCode::KeyI,
        KeyCode::KeyR,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyX,
        KeyCode::KeyF,
        KeyCode::KeyE,
        KeyCode::KeyW,
        KeyCode::KeyQ,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::Space,
        KeyCode::ShiftLeft,
        KeyCode::ArrowUp,
        KeyCode::ArrowLeft,
        KeyCode::F3,
        KeyCode::Escape,
        KeyCode::Enter,
        KeyCode::Backspace,
    ];
    let kart = crate::tests::player(&mut app);
    for key in typed {
        tap(&mut app, key);
        assert_eq!(
            app.world().get::<KartInput>(kart),
            Some(&KartInput::default()),
            "{key:?} reached the kart"
        );
        assert_eq!(shortcuts_state(&mut app), before, "{key:?} did something");
    }
    crate::tests::hold_keys(&mut app, &[KeyCode::ArrowUp, KeyCode::KeyW, KeyCode::KeyZ]);
    for _ in 0..60 {
        app.update();
    }
    assert!(speed(&mut app) < 0.1, "{}", speed(&mut app));
    assert_eq!(shortcuts_state(&mut app), before);

    // The field lets go: the game has its keys again.
    app.world_mut().resource_mut::<KeyboardCapture>().0 = false;
    tap(&mut app, KeyCode::KeyL);
    assert!(
        !app.world().resource::<LobbyWindow>().open,
        "L closes the lobby"
    );
    crate::tests::hold_keys(&mut app, &[KeyCode::ArrowUp]);
    for _ in 0..60 {
        app.update();
    }
    assert!(speed(&mut app) > 10.0, "{}", speed(&mut app));
}
