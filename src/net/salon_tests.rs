//! Headless scenarios of the lobby in 3D online (issue #101): two or three whole games
//! (`GamePlugin` and [`SalonPlugin`]) in a room, linked by the in-memory socket of the tests
//! (`socket::memory`, the harness of `race_tests`): every message goes through its bytes, the
//! lobby, the room's clock, the characters' states (the host's bundles) and the strikes go the
//! way they go between browsers. One frame is one tick for every game, in turn.

use super::race_tests::{ANA, BOB, CYD, Room, lobby, on_grid, showing, start_race};
use super::*;
use crate::characters::{Ragdoll, RagdollPhase};
use crate::interpolation::PhysicsPose;
use crate::salon::{
    PlayerWalker, RemoteWalkerBody, SalonPlugin, SalonRoot, SalonTuning, Walker, WalkerControl,
    WalkerLook, WalkerVisual, walk::front_of,
};
use crate::track::{TrackChoice, TrackLayout};

/// A room of `players` in the lobby in 3D: seated, on foot in its scene, each seeing every other
/// one's character; the lobby's screen closed (its keys walk).
pub(super) fn salon(players: &[(PeerKey, &str)]) -> Room {
    let mut room = Room::new(players);
    for (_, app) in &mut room.games {
        app.add_plugins(SalonPlugin);
    }
    let count = players.len();
    room.until(15.0, "everyone in the lobby, on foot", |app| {
        showing(app) == Some(Showing::Salon)
            && app.world().get_resource::<FootSync>().is_some_and(|foot| {
                foot.walkers().len() == count - 1 && foot.walkers().iter().all(|w| w.body.is_some())
            })
    });
    for (_, app) in &mut room.games {
        app.world_mut().resource_mut::<LobbyWindow>().open = false;
    }
    room
}

fn our_walker(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<PlayerWalker>>()
        .single(world)
        .expect("our character")
}

/// The body of `peer`'s character in `app` (another player's).
fn their_walker(app: &App, peer: PeerKey) -> Entity {
    app.world()
        .resource::<FootSync>()
        .walker(peer)
        .and_then(|walker| walker.body)
        .expect("their character")
}

fn at(app: &App, entity: Entity) -> Vec3 {
    app.world().get::<Transform>(entity).unwrap().translation
}

fn hold(app: &mut App, keys: &[KeyCode]) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.release_all();
    for key in keys {
        input.press(*key);
    }
}

/// Puts our character at `position` (its bottom on the lawn), facing `heading`.
fn put(app: &mut App, position: Vec3, heading: f32) {
    let body = our_walker(app);
    let world = app.world_mut();
    let mut transform = world.get_mut::<Transform>(body).unwrap();
    transform.translation = Vec3::new(position.x, 0.51, position.z);
    transform.rotation = Quat::from_rotation_y(heading);
    let placed = *transform;
    world.get_mut::<PhysicsPose>(body).unwrap().snap(placed);
    world.get_mut::<WalkerControl>(body).unwrap().heading = heading;
}

/// Issue #101 acceptance: seated, everyone is on foot in the lobby's own scene (no circuit, no
/// kart of anyone, no race), each character of a player of the room on every screen, with its
/// animal, color and pseudo; one walking in its game walks in the other's: where its game had
/// it (bounded), smoothly (no jump between two frames).
#[test]
fn two_peers_see_each_others_character_walk() {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob")]);
    for (key, app) in &mut room.games {
        assert_eq!(*app.world().resource::<TrackChoice>(), TrackChoice::Salon);
        assert!(app.world().get_resource::<TrackLayout>().is_none());
        let world = app.world_mut();
        let karts = world.query::<&crate::kart::Kart>().iter(world).count();
        assert_eq!(karts, 0, "{key:?}: on foot");
        let scenes = world.query::<&SalonRoot>().iter(world).count();
        assert_eq!(scenes, 1);
        let walkers = world.query::<&Walker>().iter(world).count();
        assert_eq!(walkers, 2, "{key:?}: ours and the other's");
    }
    let bob_in_ana = their_walker(room.view(ANA), BOB);
    let walker = room.view(ANA).world().get::<Walker>(bob_in_ana).unwrap();
    assert_eq!(walker.name, "Bob");
    let colors = (lobby(room.view(ANA)).color_of(BOB), Some(walker.color));
    assert_eq!(colors.0, colors.1, "Bob's color as the room gives it");
    // Ana walks toward the camera's -Z for 1.5 s.
    let ana = our_walker(room.app(ANA));
    let start = at(room.view(ANA), ana);
    let ana_in_bob = their_walker(room.view(BOB), ANA);
    let visual_in_bob = room
        .view(BOB)
        .world()
        .resource::<FootSync>()
        .walker(ANA)
        .and_then(|w| w.visual)
        .unwrap();
    hold(room.app(ANA), &[KeyCode::ArrowUp]);
    let mut truth: Vec<Vec3> = Vec::new();
    let mut shown: Vec<Vec3> = Vec::new();
    for _ in 0..90 {
        room.frame();
        truth.push(at(room.view(ANA), ana));
        shown.push(at(room.view(BOB), visual_in_bob));
    }
    hold(room.app(ANA), &[]);
    for _ in 0..60 {
        room.frame();
    }
    let end = at(room.view(ANA), ana);
    assert!(end.z < start.z - 4.0, "Ana walked: {start} → {end}");
    let seen = at(room.view(BOB), ana_in_bob);
    assert!(
        seen.distance(end) < 0.3,
        "Bob shows Ana where she stopped: {seen} vs {end}"
    );
    // Smooth: no step much larger than Ana's own in a tick.
    let worst_truth = truth
        .windows(2)
        .map(|w| w[0].distance(w[1]))
        .fold(0.0f32, f32::max);
    let worst_shown = shown
        .windows(2)
        .map(|w| w[0].distance(w[1]))
        .fold(0.0f32, f32::max);
    assert!(
        worst_shown < worst_truth * 1.6 + 0.02,
        "smooth: {worst_shown} m a frame (Ana: {worst_truth})"
    );
    // While walking, Bob showed her behind her, by about the delay.
    let behind = truth[60].distance(shown[60]);
    assert!(behind < 1.5, "{behind} m behind");
}

/// Issue #101 acceptance: a paw's strike on another player's character: its game decides (in
/// reach of where it was), it falls over **on both screens** (its own ragdoll, and its states on
/// the striker's), then gets up on its own on both.
#[test]
fn a_strike_knocks_the_other_over_on_both_screens_and_it_gets_up() {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob")]);
    // Ana in the middle facing -Z, Bob just in front of her.
    put(room.app(ANA), Vec3::ZERO, 0.0);
    put(room.app(BOB), front_of(0.0) * 1.2, 0.0);
    for _ in 0..60 {
        room.frame();
    }
    let bob_in_ana = their_walker(room.view(ANA), BOB);
    let shown = at(room.view(ANA), bob_in_ana);
    assert!(
        shown.distance(Vec3::new(0.0, shown.y, -1.2)) < 0.1,
        "Ana sees Bob in front of her: {shown}"
    );
    hold(room.app(ANA), &[KeyCode::KeyX]);
    room.frame();
    hold(room.app(ANA), &[]);
    let bob = our_walker(room.app(BOB));
    let phase_in_bob = |room: &Room| room.view(BOB).world().get::<Ragdoll>(bob).unwrap().phase;
    let phase_in_ana = |room: &Room| {
        room.view(ANA)
            .world()
            .get::<WalkerLook>(bob_in_ana)
            .unwrap()
            .phase
    };
    let mut bob_fell = false;
    let mut ana_saw = false;
    for _ in 0..60 {
        room.frame();
        bob_fell |= phase_in_bob(&room) == RagdollPhase::Tumbling;
        ana_saw |= phase_in_ana(&room) == RagdollPhase::Tumbling;
        if bob_fell && ana_saw {
            break;
        }
    }
    assert!(bob_fell, "Bob's game took the strike");
    assert!(ana_saw, "Ana sees Bob tumbling");
    let ana = our_walker(room.app(ANA));
    let control = *room.view(ANA).world().get::<WalkerControl>(ana).unwrap();
    assert_eq!(control.strikes, 1);
    // Up again on both screens.
    let mut up_in_bob = false;
    let mut up_in_ana = false;
    for _ in 0..(6 * 60) {
        room.frame();
        up_in_bob = phase_in_bob(&room) == RagdollPhase::Upright;
        up_in_ana = phase_in_ana(&room) == RagdollPhase::Upright;
        if up_in_bob && up_in_ana {
            break;
        }
    }
    assert!(up_in_bob && up_in_ana, "up again on both screens");
    let up = room.view(BOB).world().get::<Transform>(bob).unwrap().up();
    assert!(up.y > 0.999);
    // A strike from too far is refused by its victim (out of reach of where it was).
    put(room.app(BOB), front_of(0.0) * 1.2, 0.0);
    for _ in 0..90 {
        room.frame();
    }
    let knocks = room.view(BOB).world().get::<Ragdoll>(bob).unwrap().knocks;
    let tick = common_tick(room.view(ANA));
    room.app(ANA)
        .world_mut()
        .resource_mut::<FootSync>()
        .send_strike(FootStrike {
            target: BOB,
            tick,
            seen: tick,
            position: foot::foot_position(Vec3::new(9.0, 0.5, 9.0)),
            direction: [0, -127],
        });
    for _ in 0..30 {
        room.frame();
    }
    assert_eq!(
        room.view(BOB).world().get::<Ragdoll>(bob).unwrap().knocks,
        knocks,
        "too far: Bob decides it missed"
    );
}

/// The common tick of this frame in `app`.
fn common_tick(app: &App) -> u32 {
    super::race_tests::common_tick(app)
}

/// Issue #101 acceptance: the host starts the race from the lobby as before (« Lancer la
/// course »): everyone on the grid, and **the lobby's scene gone at the start** (its characters,
/// visuals, scene and network state); the host's settings again (or the results closed a while):
/// everyone back on foot in the lobby.
#[test]
fn the_race_starts_from_the_lobby_and_the_lobby_comes_back_after() {
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob"), (CYD, "Cyd")]);
    start_race(&mut room, ANA, 2, 1);
    for (key, app) in &mut room.games {
        assert!(on_grid(app), "{key:?}");
        let world = app.world_mut();
        let lobby_left = world.query::<&SalonRoot>().iter(world).count()
            + world.query::<&Walker>().iter(world).count()
            + world.query::<&WalkerVisual>().iter(world).count()
            + world.query::<&RemoteWalkerBody>().iter(world).count()
            + world.query::<&Ragdoll>().iter(world).count();
        assert_eq!(lobby_left, 0, "{key:?}: nothing of the lobby in the race");
        assert!(world.get_resource::<FootSync>().is_none(), "{key:?}");
        assert!(world.get_resource::<TrackLayout>().is_some());
    }
    // The race on: a few seconds of it.
    for _ in 0..120 {
        room.frame();
    }
    // The host's results closed: a moment after, everyone back in the lobby (the host says so).
    room.app(ANA)
        .world_mut()
        .resource_mut::<SalonTuning>()
        .back_after_results = 0.2;
    {
        let world = room.app(ANA).world_mut();
        let mut race = world.resource_mut::<OnlineRace>();
        race.shared.close(Vec::new());
    }
    room.until(10.0, "everyone back in the lobby", |app| {
        showing(app) == Some(Showing::Salon)
            && app.world().get_resource::<FootSync>().is_some_and(|foot| {
                foot.walkers().len() == 2 && foot.walkers().iter().all(|w| w.body.is_some())
            })
    });
    for (_, app) in &mut room.games {
        assert!(app.world().get_resource::<OnlineRace>().is_none());
        assert!(lobby(app).announced().is_none());
    }
}

/// Issue #101: twelve players in the lobby: each sees the eleven others, and the host's packets
/// to a client stay one per tick of states (the bundles), with every other character.
#[test]
fn twelve_players_in_the_lobby_see_each_other() {
    let players: Vec<(PeerKey, String)> = (0..12u128)
        .map(|i| (PeerKey((i + 1) << 100), format!("J{i}")))
        .collect();
    let named: Vec<(PeerKey, &str)> = players.iter().map(|(k, n)| (*k, n.as_str())).collect();
    let mut room = salon(&named);
    for _ in 0..60 {
        room.frame();
    }
    for (key, app) in &room.games {
        let foot = app.world().resource::<FootSync>();
        assert_eq!(foot.walkers().len(), 11, "{key:?}");
        for walker in foot.walkers() {
            assert!(
                walker.received >= 20,
                "{key:?} of {:?}: {}",
                walker.peer,
                walker.received
            );
        }
    }
}

/// What the lobby's screen of `app` shows, while its panel is there.
pub(super) fn panel(app: &App) -> Option<crate::salon::SalonView> {
    let shown = app
        .world()
        .iter_entities()
        .any(|e| e.contains::<crate::salon::SalonScreenRoot>());
    let menu = app.world().get_resource::<crate::salon::SalonMenu>()?;
    menu.view.clone().filter(|_| shown)
}

/// Issue #136: the banner's button (`crate::salon::SalonBannerPill`: a click, or Entrée in the
/// scene, opens the panel) is shown in `app`. It took the place of the line of keys at the bottom
/// of the scene (`SalonKeys`, gone: « Commandes » in the panel lists them).
fn banner_shown(app: &App) -> bool {
    app.world()
        .iter_entities()
        .any(|e| e.contains::<crate::salon::SalonBannerPill>())
}

/// One key pressed then let go, a frame each.
fn tap(room: &mut Room, key: PeerKey, key_code: KeyCode) {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
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

pub(super) fn act_on_panel(room: &mut Room, key: PeerKey, action: crate::salon::SalonAction) {
    room.app(key).world_mut().write_message(action);
    room.frame();
}

/// Issue #101: the lobby's screen of the charter in a room, over its scene: its title (the
/// room's code), the players (the host's badge, who is ready), the host's settings (the host
/// changes them, the others see them and why they cannot), « Prêt ! », then « Lancer la course »,
/// disabled with its reason until everyone is ready; issue #136: « Commandes » opens the options
/// (their commands) and the panel comes back after them; Échap closes it (the banner, its button,
/// comes; the character walks), Entrée or L open it again; the host launches the race from it (without bots, in
/// mirror: its settings), and nothing of it is left on the grid.
#[test]
fn the_lobby_screen_runs_the_room_and_launches_the_race() {
    use crate::salon::{ButtonKind, SalonAction, SalonRow};
    let mut room = salon(&[(ANA, "Ana"), (BOB, "Bob")]);
    for key in [ANA, BOB] {
        room.app(key).world_mut().resource_mut::<LobbyWindow>().open = true;
    }
    room.until(2.0, "the panels", |app| {
        panel(app).is_some_and(|view| view.players.len() == 2)
    });
    let code = room
        .view(ANA)
        .world()
        .resource::<NetSession>()
        .room
        .to_string();
    let host = panel(room.view(ANA)).unwrap();
    assert_eq!(host.title, format!("Salon {code}"));
    assert_eq!(host.buttons[0].kind, ButtonKind::StartRace);
    assert!(!host.buttons[0].enabled, "{host:?}");
    assert!(!host.buttons[0].caption.is_empty(), "its reason: {host:?}");
    assert!(host.seat.as_ref().is_some_and(|seat| seat.editable));
    let names: Vec<(&str, bool, bool)> = host
        .players
        .iter()
        .map(|p| (p.name.as_str(), p.host, p.ready))
        .collect();
    assert!(names.contains(&("Ana (toi)", true, false)), "{names:?}");
    assert!(names.contains(&("Bob", false, false)), "{names:?}");
    let guest = panel(room.view(BOB)).unwrap();
    assert_eq!(guest.buttons[0].kind, ButtonKind::Ready);
    assert!(
        guest
            .seat
            .as_ref()
            .is_some_and(|seat| !seat.editable && seat.locked.is_some()),
        "{guest:?}"
    );
    assert!(
        guest
            .lines
            .iter()
            .any(|l| l.starts_with("Réglages de l'hôte")),
        "{guest:?}"
    );
    assert!(!banner_shown(room.view(ANA)), "the panel over the scene");

    // The host's settings: a guest's step changes nothing; the host's reach everyone.
    let before = lobby(room.view(ANA)).setup();
    act_on_panel(&mut room, BOB, SalonAction::Step(SalonRow::Mirror, 1));
    for _ in 0..10 {
        room.frame();
    }
    assert_eq!(lobby(room.view(ANA)).setup(), before);
    act_on_panel(&mut room, ANA, SalonAction::Step(SalonRow::Mirror, 1));
    act_on_panel(&mut room, ANA, SalonAction::Step(SalonRow::Bots, 1));
    room.until(2.0, "the guest sees the host's settings", |app| {
        let setup = lobby(app).setup();
        setup.mirror && !setup.bots
    });
    assert!(lobby(room.view(ANA)).setup().mirror);

    // Bob is ready: the host can launch.
    act_on_panel(&mut room, BOB, SalonAction::Press(ButtonKind::Ready));
    room.until(2.0, "the host can launch, Bob is ready", |app| {
        panel(app).is_some_and(|view| match view.buttons[0].kind {
            ButtonKind::StartRace => view.buttons[0].enabled,
            kind => kind == ButtonKind::NotReady,
        })
    });
    let guest = panel(room.view(BOB)).unwrap();
    assert_eq!(guest.buttons[0].kind, ButtonKind::NotReady);
    let host = panel(room.view(ANA)).unwrap();
    assert!(
        host.players.iter().any(|p| p.name == "Bob" && p.ready),
        "{host:?}"
    );

    // Issue #136: « Commandes » opens the options (their list of commands), the panel hidden
    // under them; Échap closes them, and the panel is back.
    act_on_panel(&mut room, ANA, SalonAction::Press(ButtonKind::Controls));
    room.frame();
    assert!(
        room.view(ANA)
            .world()
            .resource::<crate::options::OptionsMenu>()
            .open
    );
    assert!(panel(room.view(ANA)).is_none(), "one screen at a time");
    tap(&mut room, ANA, KeyCode::Escape);
    room.frame();
    assert!(
        !room
            .view(ANA)
            .world()
            .resource::<crate::options::OptionsMenu>()
            .open
    );
    assert!(panel(room.view(ANA)).is_some(), "the panel is back");

    // Échap: the panel closes, the banner comes (issue #136: its button, no more line of keys
    // at the bottom of the scene); Entrée (as a click on it) opens the panel again; so does L.
    tap(&mut room, ANA, KeyCode::Escape);
    room.frame();
    assert!(!room.view(ANA).world().resource::<LobbyWindow>().open);
    assert!(panel(room.view(ANA)).is_none());
    assert!(banner_shown(room.view(ANA)));
    assert!(
        room.view(ANA)
            .world()
            .get_resource::<NetSession>()
            .is_some(),
        "still in the room"
    );
    tap(&mut room, ANA, KeyCode::Enter);
    room.frame();
    assert!(panel(room.view(ANA)).is_some());
    assert!(!banner_shown(room.view(ANA)));
    tap(&mut room, ANA, KeyCode::Escape);
    room.frame();
    assert!(banner_shown(room.view(ANA)));
    tap(&mut room, ANA, LOBBY_KEY);
    room.frame();
    assert!(panel(room.view(ANA)).is_some());
    assert!(!banner_shown(room.view(ANA)));

    // « Lancer la course »: on the grid, without bots, in mirror; nothing of the screen left.
    act_on_panel(&mut room, ANA, SalonAction::Press(ButtonKind::StartRace));
    room.until(10.0, "everyone on the grid", on_grid);
    // Both karts on each screen, ours and the other's (it comes with its first states), never
    // more: no bot.
    let karts = |app: &App| {
        let entities = || app.world().iter_entities();
        (
            entities()
                .filter(|e| e.contains::<crate::kart::Kart>())
                .count(),
            entities()
                .filter(|e| e.contains::<RemoteKartProxy>())
                .count(),
        )
    };
    room.until(5.0, "both karts on each screen", |app| karts(app) == (1, 1));
    for _ in 0..30 {
        room.frame();
    }
    for (key, app) in &mut room.games {
        assert_eq!(karts(app), (1, 1), "{key:?}: the players only");
        let world = app.world_mut();
        assert!(
            world.resource::<crate::track::MirrorMode>().0,
            "{key:?}: in mirror"
        );
        let screen_left = world
            .query::<&crate::salon::SalonScreenRoot>()
            .iter(world)
            .count()
            + world
                .query::<&crate::salon::SalonBanner>()
                .iter(world)
                .count()
            + world.query::<&SalonRoot>().iter(world).count();
        assert_eq!(screen_left, 0, "{key:?}");
    }
}
