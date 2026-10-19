//! Issue #136: headless scenarios of the end screens' buttons online (`menu::end`): two whole
//! games (`GamePlugin`, the lobby in 3D and the screens, `menu::MainMenuPlugin`) in a room, linked
//! by the in-memory socket of the tests (`socket::memory`). One frame is one tick for every game,
//! in turn. The races are short (one lap, no bot, the players' karts driven by bots); the
//! automatic waits are long (ten minutes): only the buttons move the room on.

use bevy::input::ButtonState;
use bevy::input::gamepad::{GamepadSettings, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};

use super::championship_tests::{quick, race_round, start_championship};
use super::race_tests::{ANA, BOB, Room, bots_drive_the_players, lobby, showing, start_race};
use super::salon_tests::{panel, salon};
use super::*;
use crate::menu::end::END_INPUT_DELAY;
use crate::menu::end_tests::assert_no_line_of_keys;
use crate::menu::{EndMenu, EndScreen, EndText, MainMenuPlugin};
use crate::race::RaceTuning;
use crate::salon::SalonTuning;

/// What never comes by itself in these tests (s): the room waits for its host's buttons.
const LONG: f32 = 600.0;

/// A room of Ana (the host) and Bob in the lobby in 3D, with the game's screens.
fn room_with_screens() -> Room {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob")]);
    for (_, app) in &mut room.games {
        app.add_plugins(MainMenuPlugin { at_launch: false });
        let pad = app
            .world_mut()
            .spawn((Gamepad::default(), GamepadSettings::default()))
            .id();
        app.insert_resource(Pad(pad));
        let mut salon = app.world_mut().resource_mut::<SalonTuning>();
        salon.back_after_results = LONG;
        salon.standings_seconds = LONG;
        salon.podium_seconds = LONG;
    }
    room
}

/// A game's gamepad.
#[derive(Resource)]
struct Pad(Entity);

/// The end screen `app` shows, once its buttons take the keys.
fn end_screen(app: &App) -> Option<EndScreen> {
    let menu = app.world().get_resource::<EndMenu>()?;
    let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
    menu.screen
        .filter(|_| !menu.usable().is_empty() && now - menu.shown_at >= END_INPUT_DELAY)
}

/// Frames until `key`'s game shows the end screen `screen`, its buttons taking the keys.
fn until_end(room: &mut Room, key: PeerKey, screen: EndScreen) {
    for _ in 0..(120 * 60) {
        room.frame();
        if end_screen(room.view(key)) == Some(screen) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_micros(500));
    }
    let shown = room
        .view(key)
        .world()
        .get_resource::<EndMenu>()
        .and_then(|menu| menu.screen);
    panic!("{key:?}: the end screen {screen:?} never came ({shown:?})");
}

/// `key`'s end screen's buttons: (label, small line), in their order.
fn buttons(room: &mut Room, key: PeerKey) -> Vec<(String, String)> {
    let world = room.app(key).world_mut();
    let mut texts: Vec<(usize, bool, String)> = world
        .query::<(&EndText, &Text)>()
        .iter(world)
        .map(|(text, written)| (text.index, text.caption, written.0.clone()))
        .collect();
    texts.sort_by_key(|(index, caption, _)| (*index, *caption));
    texts
        .chunks(2)
        .map(|pair| (pair[0].2.clone(), pair[1].2.clone()))
        .collect()
}

fn labels(room: &mut Room, key: PeerKey) -> Vec<String> {
    buttons(room, key)
        .into_iter()
        .map(|(label, _)| label)
        .collect()
}

fn usable(room: &Room, key: PeerKey) -> Vec<bool> {
    room.view(key)
        .world()
        .resource::<EndMenu>()
        .usable()
        .to_vec()
}

fn focus(room: &Room, key: PeerKey) -> Option<usize> {
    room.view(key).world().resource::<EndMenu>().focus
}

/// A key pressed (one frame of the room) then released (the next), in `key`'s game.
fn tap(room: &mut Room, key: PeerKey, key_code: KeyCode) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        room.app(key).world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        room.frame();
    }
}

/// A button of the gamepad of `key`'s game pressed then released.
fn pad(room: &mut Room, key: PeerKey, button: GamepadButton) {
    let pad = room.view(key).world().resource::<Pad>().0;
    for value in [1.0, 0.0] {
        room.app(key)
            .world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad, button, value,
            )));
        room.frame();
    }
}

fn in_the_lobby(app: &App) -> bool {
    showing(app) == Some(Showing::Salon)
}

/// Issue #136 acceptance online, a race alone: on the results, each has « Retour au salon », its
/// countdown under it, and no line of keys. The host's waits for the table to close (the others
/// may still race), then brings everyone back; a guest's takes it back to the lobby before the
/// others (Bob, with Entrée: in the lobby's scene, Ana still on her results), then the host's
/// (Entrée) brings Ana too, long before the automatic way back.
#[test]
fn online_retour_au_salon_after_a_race() {
    let mut room = room_with_screens();
    for (_, app) in &mut room.games {
        let mut race = app.world_mut().resource_mut::<RaceTuning>();
        race.laps = 1;
        race.results_close_seconds = 1.0;
        app.world_mut().resource_mut::<NetTuning>().finish_grace_s = 3.0;
    }
    start_race(&mut room, ANA, 0, 1);
    bots_drive_the_players(&mut room);
    until_end(&mut room, BOB, EndScreen::OnlineRace { host: false });
    until_end(&mut room, ANA, EndScreen::OnlineRace { host: true });
    room.until(30.0, "the table closed", |app| {
        app.world()
            .get_resource::<crate::race::Race>()
            .is_some_and(crate::race::Race::results_closed)
    });
    room.frame();
    for key in [ANA, BOB] {
        assert_eq!(labels(&mut room, key), ["Retour au salon"], "{key:?}");
        assert_eq!(usable(&room, key), [true], "{key:?}");
        assert_eq!(focus(&room, key), Some(0), "{key:?}");
        assert_no_line_of_keys(room.app(key));
    }
    let (_, host) = &buttons(&mut room, ANA)[0];
    assert!(
        host.starts_with("Dans ") && host.ends_with(" s, pour tout le monde."),
        "{host}"
    );
    let (_, guest) = &buttons(&mut room, BOB)[0];
    assert!(
        guest.starts_with("Dans ") && guest.ends_with(" s, avec tout le monde."),
        "{guest}"
    );

    // Bob first, alone: the lobby's scene for him; Ana still on her results.
    tap(&mut room, BOB, KeyCode::Enter);
    room.until(10.0, "Bob in the lobby", |app| {
        lobby(app).me() != BOB || in_the_lobby(app)
    });
    assert!(!in_the_lobby(room.view(ANA)), "Ana still on her results");
    assert_eq!(
        end_screen(room.view(ANA)),
        Some(EndScreen::OnlineRace { host: true })
    );

    // Ana, the host: everyone in the lobby.
    tap(&mut room, ANA, KeyCode::Enter);
    room.until(10.0, "everyone in the lobby", in_the_lobby);
    assert!(
        lobby(room.view(ANA)).announced().is_none(),
        "the race is over"
    );
    // The lobby's panel, in a room: no line of keys either; « Commandes » is there.
    room.until(5.0, "the panels", |app| panel(app).is_some());
    for key in [ANA, BOB] {
        let view = panel(room.view(key)).unwrap();
        assert!(
            view.buttons
                .iter()
                .any(|b| b.kind == crate::salon::ButtonKind::Controls),
            "{key:?}: {view:?}"
        );
        assert_no_line_of_keys(room.app(key));
    }
}

/// Issue #141: in a room, the lobby's panel shows over its scene (L), and over a race we run (L:
/// our kart coasts, its keys are the panel's, and the race goes on: online, nothing waits); never
/// over the race's end: there the window opens by itself (the host ended it) and the panel waits,
/// hidden and without its banner, the results' buttons having the keys; back in the lobby
/// (« Retour au salon »), it shows.
#[test]
fn online_the_lobby_s_panel_shows_over_the_race_not_over_its_results() {
    use super::salon_tests::banner_shown;
    use crate::kart::PlayerInputMuted;
    use crate::race::Race;
    let mut room = room_with_screens();
    for (_, app) in &mut room.games {
        let mut race = app.world_mut().resource_mut::<RaceTuning>();
        race.laps = 1;
        race.results_close_seconds = 1.0;
        app.world_mut().resource_mut::<NetTuning>().finish_grace_s = 3.0;
    }
    let muted = |app: &App| app.world().resource::<PlayerInputMuted>().0;
    let window = |app: &App| app.world().resource::<LobbyWindow>().open;

    // The lobby's scene: L shows the room's panel, Échap closes it (the banner comes).
    tap(&mut room, ANA, LOBBY_KEY);
    room.frame();
    let view = panel(room.view(ANA)).expect("the panel in the lobby's scene");
    assert!(view.title.starts_with("Salon "), "{view:?}");
    tap(&mut room, ANA, KeyCode::Escape);
    room.frame();
    assert!(panel(room.view(ANA)).is_none());
    assert!(banner_shown(room.view(ANA)));

    // Our race: closed at its start, no banner; L shows the panel over it, our kart coasts and
    // the race goes on; Échap closes it, the keys drive again.
    start_race(&mut room, ANA, 0, 1);
    for _ in 0..30 {
        room.frame();
    }
    for key in [ANA, BOB] {
        let app = room.view(key);
        assert!(!window(app), "{key:?}: closed at the start");
        assert!(panel(app).is_none() && !banner_shown(app), "{key:?}");
        assert!(!muted(app), "{key:?}");
    }
    tap(&mut room, ANA, LOBBY_KEY);
    room.frame();
    let view = panel(room.view(ANA)).expect("the panel over our race");
    assert!(
        view.lines
            .iter()
            .any(|line| line.contains(" en cours sur ")),
        "{view:?}"
    );
    assert!(muted(room.view(ANA)), "our kart coasts");
    assert!(!muted(room.view(BOB)));
    assert!(
        !room
            .view(ANA)
            .world()
            .resource::<Time<Virtual>>()
            .is_paused()
    );
    // Its simulation runs on (the room's clock, from our fixed ticks).
    let at = super::race_tests::common_tick(room.view(ANA));
    for _ in 0..10 {
        room.frame();
    }
    assert!(
        super::race_tests::common_tick(room.view(ANA)) >= at + 8,
        "the race goes on"
    );
    tap(&mut room, ANA, KeyCode::Escape);
    room.frame();
    assert!(panel(room.view(ANA)).is_none() && !window(room.view(ANA)));
    assert!(!muted(room.view(ANA)), "the keys drive again");
    assert!(!banner_shown(room.view(ANA)), "not during our race");

    // Its end: the window opens by itself, the panel waits behind the results.
    bots_drive_the_players(&mut room);
    until_end(&mut room, BOB, EndScreen::OnlineRace { host: false });
    until_end(&mut room, ANA, EndScreen::OnlineRace { host: true });
    room.until(30.0, "the table closed", |app| {
        app.world()
            .get_resource::<Race>()
            .is_some_and(Race::results_closed)
    });
    room.frame();
    for key in [ANA, BOB] {
        let app = room.view(key);
        assert!(window(app), "{key:?}: opened by itself at the end");
        assert!(panel(app).is_none(), "{key:?}: not over the results");
        assert!(!banner_shown(app), "{key:?}: nor its banner");
        assert!(!muted(app), "{key:?}");
        assert_eq!(focus(&room, key), Some(0), "{key:?}: the results' buttons");
    }

    // Bob back in the lobby (« Retour au salon », Entrée): his panel there; Ana still on her
    // results, without it.
    tap(&mut room, BOB, KeyCode::Enter);
    room.until(10.0, "Bob's panel in the lobby", |app| {
        lobby(app).me() != BOB || (in_the_lobby(app) && panel(app).is_some())
    });
    assert!(!in_the_lobby(room.view(ANA)));
    assert!(panel(room.view(ANA)).is_none(), "Ana still on her results");
}

/// Issue #136 acceptance online, a championship of two races: between them, the host has
/// « Course suivante » (its gamepad's A starts it for everyone) and « Retour au salon »; a guest
/// has « Course suivante » greyed, with when and who (its Entrée does nothing); after the last,
/// « Le podium » (the host's Entrée); on the podium, each has « Retour au salon »: Bob goes first,
/// alone, then Ana brings everyone back. A second championship: the host's « Retour au salon »
/// (Droite, Entrée) on its standings stops it for everyone.
#[test]
fn online_the_host_s_buttons_run_the_championship() {
    let mut room = room_with_screens();
    quick(&mut room, ANA);
    for (_, app) in &mut room.games {
        let mut salon = app.world_mut().resource_mut::<SalonTuning>();
        salon.standings_seconds = LONG;
        salon.podium_seconds = LONG;
    }
    start_championship(&mut room, ANA, &[0, 1], 0);
    race_round(&mut room, 1);
    let standings = |host| EndScreen::OnlineStandings { host, last: false };
    until_end(&mut room, ANA, standings(true));
    until_end(&mut room, BOB, standings(false));
    assert_eq!(
        labels(&mut room, ANA),
        ["Course suivante", "Retour au salon"]
    );
    assert_eq!(usable(&room, ANA), [true, true]);
    assert_eq!(focus(&room, ANA), Some(0));
    assert_eq!(labels(&mut room, BOB), ["Course suivante"]);
    assert_eq!(usable(&room, BOB), [false], "the host's");
    assert_eq!(focus(&room, BOB), None);
    let (_, guest) = &buttons(&mut room, BOB)[0];
    assert!(guest.ends_with(" s : l'hôte décide."), "{guest}");
    for key in [ANA, BOB] {
        assert_no_line_of_keys(room.app(key));
    }
    // Bob's Entrée: nothing (the host's choice).
    tap(&mut room, BOB, KeyCode::Enter);
    for _ in 0..30 {
        room.frame();
    }
    assert_eq!(end_screen(room.view(BOB)), Some(standings(false)));
    assert_eq!(lobby(room.view(ANA)).announced().unwrap().round, 1);

    // Ana's A: the next race, for everyone.
    pad(&mut room, ANA, GamepadButton::South);
    race_round(&mut room, 2);
    let last = |host| EndScreen::OnlineStandings { host, last: true };
    until_end(&mut room, ANA, last(true));
    until_end(&mut room, BOB, last(false));
    assert_eq!(labels(&mut room, ANA), ["Le podium", "Retour au salon"]);
    assert_eq!(labels(&mut room, BOB), ["Le podium"]);
    tap(&mut room, ANA, KeyCode::Enter);
    until_end(&mut room, ANA, EndScreen::OnlinePodium { host: true });
    until_end(&mut room, BOB, EndScreen::OnlinePodium { host: false });
    for key in [ANA, BOB] {
        assert_eq!(labels(&mut room, key), ["Retour au salon"], "{key:?}");
        assert_eq!(usable(&room, key), [true]);
        assert_no_line_of_keys(room.app(key));
    }

    // Bob first, alone; then Ana: everyone back, the championship over.
    tap(&mut room, BOB, KeyCode::Enter);
    room.until(10.0, "Bob in the lobby", |app| {
        lobby(app).me() != BOB || in_the_lobby(app)
    });
    assert!(!in_the_lobby(room.view(ANA)), "Ana still on the podium");
    tap(&mut room, ANA, KeyCode::Enter);
    room.until(10.0, "everyone in the lobby", |app| {
        in_the_lobby(app) && lobby(app).championship().is_none()
    });

    // Another championship: the host stops it from its first standings.
    start_championship(&mut room, ANA, &[0, 1], 0);
    race_round(&mut room, 1);
    until_end(&mut room, ANA, standings(true));
    tap(&mut room, ANA, KeyCode::ArrowRight);
    assert_eq!(focus(&room, ANA), Some(1), "« Retour au salon »");
    tap(&mut room, ANA, KeyCode::Enter);
    room.until(
        10.0,
        "everyone in the lobby, the championship stopped",
        |app| in_the_lobby(app) && lobby(app).championship().is_none(),
    );
}
