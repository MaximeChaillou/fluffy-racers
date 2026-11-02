//! Issue #101: the lobby's screen, drawn with `bevy_ui` after the charter (`docs/UI_CHARTE.md` §
//! « Le salon (#101) »), over the lobby's 3D scene: it replaces the egui window of issues #7 to
//! #99. What it shows is `view.rs`'s (pure, tested); here, its tree, the keys, the gamepad and the
//! mouse.
//!
//! - **The panel** (L, `net::LOBBY_KEY`; the gamepad's « Select »; it opens by itself on
//!   arriving in a room): on the veil, its title (« Salon ABCDE », « Multijoueur » out of a room),
//!   a cream panel of 900 px in two columns: on the left, who does what, **the host's settings**
//!   (Circuit, Miroir, Bots, Objets: a line each, « < » and « > »; disabled for the others, with
//!   their reason), the main action (« Lancer la course », disabled with its reason, or « Prêt ! »),
//!   « Pilote », « Commandes » (issue #136: the options, their list of commands) and
//!   « Quitter »; on the right, **the players** (their color, pseudo, animal, link and ping,
//!   « hôte », « prêt »), the invitation link with « Copier », the problems of connection (#49).
//!   Issue #136: no line of keys under it. Up and down (or Z/S, W/S) go through its lines and
//!   buttons, left and right change a line, Entrée (Espace, A) chooses, Échap or L (B, Start)
//!   close it: then our character walks again.
//! - **The banner** (#64) at the top while the panel is closed, in a room (not during a race we
//!   run nor over its results). Issue #136: it is the panel's button (a click opens it; Entrée
//!   too, in the scene), instead of « (L) » in its text and of the line of keys at the bottom of
//!   the scene.
//! - **One screen at a time**: not while « Pilote », the options, « Jouer solo » or the pause are
//!   open (it comes back after them), nor over a race's end in a room (its results, standings,
//!   podium: the room comes back to the lobby after them). Issue #141 ([`Behind`]): over the
//!   home's background (« Multijoueur »: a race of bots, not ours) and over a race (L): in solo
//!   the game waits under it, online our kart coasts (`characters::pause_in_menus`).
//!
//! **Closed, nothing is left** (its nodes are `MenuNode`s); its resource ([`SalonMenu`]) comes the
//! first time it opens.

use bevy::picking::Pickable;
use bevy::picking::events::{PointerClick, PointerOut, PointerOver};
use bevy::prelude::*;
use bevy::text::Justify;

use super::scene::SalonRoot;
use super::view::{ButtonKind, SalonRow, SalonView, ViewInput, salon_view};
use crate::characters::{PlayerLoadout, Roster, SelectionAction, SelectionScreen};
use crate::kart::KeyboardLayout;
use crate::level::Online;
use crate::menu::style::{self, ButtonState, palette, size, u};
use crate::menu::{MainMenu, MenuAssets, MenuNode, Showcase, StickLatch, anim, kit};
use crate::modes::Catalog;
use crate::modes::RaceMenu;
use crate::net::{
    LobbyWindow, NetAction, NetSession, NetSettings, OnlineRace, RoomFacts, character_name,
    invitation, is_local_signal, loopback_page, now_ms,
};
use crate::options::{OptionsAction, OptionsMenu};
use crate::race::{GrandPrixTuning, Race, RacePhase};

/// What the player does with the lobby's screen: the keys and the gamepad (`salon_keys`), the
/// mouse (its nodes' observers).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SalonAction {
    /// Up (-1) or down (+1), through its lines and buttons (around).
    Move(i32),
    /// Left (-1) or right (+1) on the line with the focus.
    Change(i32),
    /// This line or button (its place among those that take the focus) takes it: the pointer.
    Focus(usize),
    /// What has the focus is chosen (Entrée, Espace, A).
    Choose,
    /// A line's « < » or « > » clicked.
    Step(SalonRow, i32),
    /// A button clicked.
    Press(ButtonKind),
}

/// What the screen remembers (real times, s). There once it first opened.
#[derive(Resource, Debug)]
pub struct SalonMenu {
    /// What has the focus: an index of [`focusables`].
    pub focus: usize,
    pub opened_at: f64,
    pub focused_at: f64,
    /// When « Copier » was last chosen.
    pub copied_at: Option<f64>,
    /// The circuits (and issue #122 the cups) the host chooses from.
    pub catalog: Catalog,
    /// What the screen shows (computed once a frame while it is wanted).
    pub view: Option<SalonView>,
    /// The gamepad's stick, as steps (vertical, horizontal).
    latches: [StickLatch; 2],
}

/// « Copié ! » stays this long (s).
const COPIED_SECONDS: f64 = 2.0;

/// What can take the focus on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focusable {
    Row(SalonRow),
    /// A button, by its index in the view.
    Button(usize),
}

/// What takes the focus, in order: the settings' lines (when they can change: issue #122, the
/// mode's, then a custom championship's races on the right), then the buttons that can be used.
pub fn focusables(view: &SalonView) -> Vec<Focusable> {
    let mut all: Vec<Focusable> = Vec::new();
    if let Some(seat) = view.seat.as_ref().filter(|seat| seat.editable) {
        all.extend(seat.rows.iter().copied().map(Focusable::Row));
    }
    all.extend(
        view.buttons
            .iter()
            .enumerate()
            .filter(|(_, button)| button.enabled)
            .map(|(index, _)| Focusable::Button(index)),
    );
    all
}

/// The help under the lines: what the line with the focus is for; for the others, why they
/// cannot change.
pub fn help_of(view: &SalonView, focus: Option<Focusable>) -> String {
    let Some(seat) = view.seat.as_ref() else {
        return String::new();
    };
    match (focus, seat.locked) {
        (Some(Focusable::Row(row)), _) => seat.help(row).to_string(),
        (_, Some(reason)) => reason.to_string(),
        _ => String::new(),
    }
}

/// The root of the screen (the veil), with what its tree was built for.
#[derive(Component, Clone, Debug, PartialEq)]
#[require(MenuNode)]
pub struct SalonScreenRoot {
    pub shape: Shape,
}

/// What a tree is built for: another shape (other lines, buttons, players...), another tree; the
/// texts change in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    lines: usize,
    seat: Option<bool>,
    /// Issue #122: the settings' lines, and the program's.
    rows: Vec<SalonRow>,
    program: usize,
    buttons: Vec<(ButtonKind, bool)>,
    players: usize,
    invite: bool,
    notes: usize,
}

fn shape_of(view: &SalonView) -> Shape {
    Shape {
        lines: view.lines.len(),
        seat: view.seat.as_ref().map(|seat| seat.editable),
        rows: view
            .seat
            .as_ref()
            .map_or_else(Vec::new, |seat| seat.rows.clone()),
        program: view.seat.as_ref().map_or(0, |seat| seat.program.len()),
        buttons: view.buttons.iter().map(|b| (b.kind, b.enabled)).collect(),
        players: view.players.len(),
        invite: view.invite.is_some(),
        notes: view.notes.len(),
    }
}

/// The column of the title, the panel and the keys: it pops in.
#[derive(Component, Clone, Copy, Debug)]
#[require(MenuNode)]
pub struct SalonColumn;

/// A line of the settings: its pill takes the focus's look.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SalonLineNode {
    pub row: SalonRow,
}

/// A button of the screen (its index in the view).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SalonButtonNode {
    pub index: usize,
    pub kind: ButtonKind,
}

/// A player's line (its index in the view), and its color's dot.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SalonPlayerNode(pub usize);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SalonDot(pub usize);

/// A badge of a player's line: « hôte » (else « prêt »).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub struct SalonBadge {
    pub player: usize,
    pub host: bool,
}

/// The banner at the top (#64), while the panel is closed in a room.
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct SalonBanner;

/// Which text of the view a node writes.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(MenuNode)]
pub enum SalonText {
    Title,
    Line(usize),
    Value(SalonRow),
    Help,
    Label(usize),
    Caption(usize),
    Header,
    PlayerName(usize),
    PlayerAnimal(usize),
    PlayerLink(usize),
    Invite,
    Note(usize),
    Banner,
    /// Issue #122: a championship's program: its header, its races.
    ProgramTitle,
    Program(usize),
}

/// The text a node writes, from the view.
fn text_of(view: &SalonView, key: SalonText, help: &str) -> String {
    let get = |list: &[String], i: usize| list.get(i).cloned().unwrap_or_default();
    match key {
        SalonText::Title => view.title.clone(),
        SalonText::Line(i) => get(&view.lines, i),
        SalonText::Value(row) => view
            .seat
            .as_ref()
            .map(|seat| seat.value(row).to_string())
            .unwrap_or_default(),
        SalonText::Help => help.to_string(),
        SalonText::Label(i) => view
            .buttons
            .get(i)
            .map(|b| b.label.clone())
            .unwrap_or_default(),
        SalonText::Caption(i) => view
            .buttons
            .get(i)
            .map(|b| b.caption.clone())
            .unwrap_or_default(),
        SalonText::Header => view.players_header.clone(),
        SalonText::PlayerName(i) => view
            .players
            .get(i)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
        SalonText::PlayerAnimal(i) => view
            .players
            .get(i)
            .map(|p| p.animal.clone())
            .unwrap_or_default(),
        SalonText::PlayerLink(i) => view
            .players
            .get(i)
            .map(|p| p.link.clone())
            .unwrap_or_default(),
        SalonText::Invite => view.invite.clone().unwrap_or_default(),
        SalonText::Note(i) => get(&view.notes, i),
        SalonText::Banner => String::new(),
        SalonText::ProgramTitle => view
            .seat
            .as_ref()
            .map(|seat| seat.program_title.clone())
            .unwrap_or_default(),
        SalonText::Program(i) => view
            .seat
            .as_ref()
            .map(|seat| get(&seat.program, i))
            .unwrap_or_default(),
    }
}

/// The real time (s).
fn now(time: &Time<Real>) -> f64 {
    time.elapsed_secs_f64()
}

/// Issue #141: what the lobby's screen (its panel, its banner) would show over. Before #141, any
/// race hid them, and the home's background is a race (`menu::Showcase`): « Multijoueur » on the
/// home showed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behind {
    /// No race we play: the lobby's scene, free driving, a level being built, or the home's
    /// background (a race of bots behind the home, never ours: issue #89).
    NoRace,
    /// A race we play (or, online, watch), until its finish.
    Race,
    /// Its end: its finish, its results, a championship's standings and podium (the end screens of
    /// #136); online, from the moment the host ended it (our race ends a tick later).
    End,
}

impl Behind {
    /// From the level's race (`race`), whether it is the home's background, and whether the
    /// room's race is over (the host ended it).
    pub fn of(race: Option<&Race>, background: bool, room_race_over: bool) -> Self {
        match race {
            None => Behind::NoRace,
            Some(_) if background => Behind::NoRace,
            Some(_) if room_race_over => Behind::End,
            Some(race) => match race.phase() {
                RacePhase::Finished | RacePhase::Results => Behind::End,
                RacePhase::Grid | RacePhase::Countdown | RacePhase::Racing => Behind::Race,
            },
        }
    }

    /// The lobby's screen shows over it: anything but the end of a race in a room (`online`): the
    /// room comes back to the lobby after its end screens, the panel waits for them to go (the
    /// charter). Out of a room, nothing comes after them: L opens the panel over them, as N or G
    /// open their screens.
    pub fn shows_the_lobby(self, online: bool) -> bool {
        !(online && self == Behind::End)
    }

    /// The panel shown over it holds the game, as the other screens do over a race: in solo the
    /// game waits; online our kart coasts (its keys are the panel's). Not over no race: the home's
    /// background never waits (#89), our character in the lobby's scene waits by itself (#101).
    pub fn held_by_the_lobby(self, online: bool) -> bool {
        self != Behind::NoRace && self.shows_the_lobby(online)
    }
}

/// The screen is wanted: the window open, no other screen over it, and (issue #141) over what it
/// can show over ([`Behind::shows_the_lobby`]: not over the end of a race in a room).
#[allow(clippy::too_many_arguments)]
pub fn screen_wanted(
    window: &LobbyWindow,
    selection: Option<&SelectionScreen>,
    options: Option<&OptionsMenu>,
    race_menu: Option<&RaceMenu>,
    main_menu: Option<&MainMenu>,
    behind: Behind,
    online: bool,
) -> bool {
    window.open
        && !selection.is_some_and(|s| s.open)
        && !options.is_some_and(|o| o.open)
        && !race_menu.is_some_and(|r| r.open)
        && !main_menu.is_some_and(|m| m.open)
        && behind.shows_the_lobby(online)
}

/// The screen's systems run: it is wanted, or something of it is shown (to go), or the banner
/// could show, or Entrée could open it (in the scene).
#[allow(clippy::too_many_arguments)]
pub fn screen_wanted_or_shown(
    window: Option<Res<LobbyWindow>>,
    roots: Query<(), Or<(With<SalonScreenRoot>, With<SalonBanner>)>>,
    scenes: Query<(), With<SalonRoot>>,
) -> bool {
    !roots.is_empty()
        || !scenes.is_empty()
        || window.is_some_and(|window| window.open || window.banner)
}

/// The panel is there.
pub fn screen_shown(roots: Query<(), With<SalonScreenRoot>>) -> bool {
    !roots.is_empty()
}

/// The keys and the gamepad on the screen (see the module's documentation): the gamepad's
/// « Select » opens and closes it in a room; open, up and down, left and right, Entrée, Espace
/// or A; B or Start close it (Échap and L are `net`'s). Issue #136: closed, in the lobby's
/// scene, Entrée opens it (the banner is its button). Not while another screen has the keys.
#[allow(clippy::too_many_arguments)]
pub fn salon_keys(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    layout: Option<Res<KeyboardLayout>>,
    window: Res<LobbyWindow>,
    session: Option<Res<NetSession>>,
    (selection, options, race_menu, main_menu): (
        Option<Res<SelectionScreen>>,
        Option<Res<OptionsMenu>>,
        Option<Res<RaceMenu>>,
        Option<Res<MainMenu>>,
    ),
    menu: Option<ResMut<SalonMenu>>,
    capture: Option<Res<crate::admin::KeyboardCapture>>,
    (scenes, panels): (Query<(), With<SalonRoot>>, Query<(), With<SalonScreenRoot>>),
    mut actions: MessageWriter<SalonAction>,
    mut net: MessageWriter<NetAction>,
) {
    let others = selection.is_some_and(|s| s.open)
        || options.is_some_and(|o| o.open)
        || race_menu.is_some_and(|r| r.open)
        || main_menu.is_some_and(|m| m.open)
        || capture.is_some_and(|c| c.0);
    let pressed = |button: GamepadButton| gamepads.iter().any(|g| g.just_pressed(button));
    if others {
        return;
    }
    if pressed(super::SALON_BUTTON) && (window.open || session.is_some()) {
        net.write(NetAction::Toggle);
        return;
    }
    // Issue #136: walking in the lobby's scene, Entrée opens its panel, as a click on the banner
    // (no line of keys tells L any more). Not Espace: it jumps there.
    let enter = keys
        .as_ref()
        .is_some_and(|keys| keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]));
    if enter && !window.open && session.is_some() && !scenes.is_empty() {
        net.write(NetAction::Toggle);
        return;
    }
    // Issue #136: only on the panel shown (the window is open, hidden, over a race's results:
    // there, the end screen's buttons have the keys).
    let Some(mut menu) = menu.filter(|_| window.open && !panels.is_empty()) else {
        return;
    };
    let (y, x) = gamepads.iter().fold((0.0f32, 0.0f32), |(y, x), gamepad| {
        let stick = |axis| gamepad.get(axis).unwrap_or(0.0);
        let (sy, sx) = (
            stick(GamepadAxis::LeftStickY),
            stick(GamepadAxis::LeftStickX),
        );
        (
            if sy.abs() > y.abs() { sy } else { y },
            if sx.abs() > x.abs() { sx } else { x },
        )
    });
    let (vertical, horizontal) = (menu.latches[0].step(y), -menu.latches[1].step(x));
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    let key = |set: &[KeyCode]| {
        keys.as_ref()
            .is_some_and(|keys| keys.any_just_pressed(set.iter().copied()))
    };
    if pressed(GamepadButton::East) || pressed(GamepadButton::Start) {
        net.write(NetAction::Close);
    } else if key(&crate::menu::CONFIRM_KEYS) || pressed(GamepadButton::South) {
        actions.write(SalonAction::Choose);
    } else if key(layout.up_keys()) || pressed(GamepadButton::DPadUp) || vertical < 0 {
        actions.write(SalonAction::Move(-1));
    } else if key(layout.down_keys()) || pressed(GamepadButton::DPadDown) || vertical > 0 {
        actions.write(SalonAction::Move(1));
    } else if key(layout.left_keys()) || pressed(GamepadButton::DPadLeft) || horizontal < 0 {
        actions.write(SalonAction::Change(-1));
    } else if key(layout.right_keys()) || pressed(GamepadButton::DPadRight) || horizontal > 0 {
        actions.write(SalonAction::Change(1));
    }
}

/// What the screen's actions do: the focus moves, a setting changes (the host's: the room's
/// settings for everyone), a button does its thing (a room created, joined or left, « Prêt »,
/// the race started, « Pilote », the link copied).
#[allow(clippy::too_many_arguments)]
pub fn apply_salon_actions(
    mut actions: MessageReader<SalonAction>,
    menu: Option<ResMut<SalonMenu>>,
    session: Option<Res<NetSession>>,
    clipboard: Option<ResMut<bevy::clipboard::Clipboard>>,
    time: Res<Time<Real>>,
    mut net: MessageWriter<NetAction>,
    mut selection: MessageWriter<SelectionAction>,
    mut options: MessageWriter<OptionsAction>,
) {
    let Some(mut menu) = menu else {
        actions.read().for_each(drop);
        return;
    };
    let Some(view) = menu.view.clone() else {
        actions.read().for_each(drop);
        return;
    };
    let mut clipboard = clipboard;
    let now = now(&time);
    let list = focusables(&view);
    // Issue #122: a championship drawn at random from the room's seed.
    let seed = session
        .as_deref()
        .map_or(0, |session| crate::net::room_seed(&session.room));
    for action in actions.read() {
        // A button to press, or a setting to change.
        let (pressed, stepped) = match *action {
            SalonAction::Move(step) => {
                if !list.is_empty() {
                    let count = list.len() as i32;
                    let focus = (menu.focus as i32 + step).rem_euclid(count) as usize;
                    set_focus(&mut menu, focus, now);
                }
                (None, None)
            }
            SalonAction::Focus(index) => {
                if index < list.len() {
                    set_focus(&mut menu, index, now);
                }
                (None, None)
            }
            SalonAction::Change(step) => match list.get(menu.focus) {
                Some(Focusable::Row(row)) => (None, Some((*row, step))),
                _ => (None, None),
            },
            SalonAction::Step(row, step) => (None, Some((row, step))),
            SalonAction::Choose => match list.get(menu.focus) {
                Some(Focusable::Button(index)) => {
                    (view.buttons.get(*index).map(|button| button.kind), None)
                }
                Some(Focusable::Row(row)) => (None, Some((*row, 1))),
                None => (None, None),
            },
            SalonAction::Press(kind) => (
                view.buttons
                    .iter()
                    .any(|b| b.kind == kind && b.enabled)
                    .then_some(kind),
                None,
            ),
        };
        if let (Some((row, step)), Some(seat)) =
            (stepped, view.seat.as_ref().filter(|seat| seat.editable))
        {
            // From the room's settings of now (the view is a frame old: a click right after
            // another one changes what the first one chose, not what was there before it).
            let setup = session
                .as_deref()
                .and_then(|s| s.lobby.as_ref())
                .map_or(seat.setup, |lobby| lobby.setup());
            net.write(NetAction::ChooseSetup(row.changed(
                setup,
                step,
                seed,
                &menu.catalog,
            )));
        }
        let Some(kind) = pressed else {
            continue;
        };
        match kind {
            ButtonKind::CreateRoom => {
                net.write(NetAction::CreateRoom);
            }
            ButtonKind::Retry | ButtonKind::JoinAgain | ButtonKind::JoinAsGuest => {
                if let Some(session) = session.as_deref() {
                    net.write(NetAction::Join(session.room.clone()));
                }
            }
            ButtonKind::Ready => {
                net.write(NetAction::Ready(true));
            }
            ButtonKind::NotReady => {
                net.write(NetAction::Ready(false));
            }
            ButtonKind::StartRace => {
                net.write(NetAction::StartRace);
            }
            // Issue #225: the game the server hosts.
            ButtonKind::JoinGame => {
                net.write(NetAction::JoinGame);
            }
            ButtonKind::Copy => {
                if let (Some(link), Some(clipboard)) = (view.invite.as_deref(), clipboard.as_mut())
                    && let Err(error) = clipboard.set_text(link.to_string())
                {
                    warn!("lobby: the link could not be copied: {error:?}");
                }
                menu.copied_at = Some(now);
            }
            ButtonKind::Pilot => {
                selection.write(SelectionAction::Open);
            }
            // Issue #136: the options' screen, its commands on the right (the panel comes back
            // after it, like after « Pilote »).
            ButtonKind::Controls => {
                options.write(OptionsAction::Toggle);
            }
            ButtonKind::Leave => {
                net.write(NetAction::Leave);
                net.write(NetAction::Close);
            }
            ButtonKind::Back => {
                net.write(NetAction::Close);
            }
        }
    }
}

fn set_focus(menu: &mut SalonMenu, focus: usize, now: f64) {
    if menu.focus != focus {
        menu.focus = focus;
        menu.focused_at = now;
    }
}

/// The view of now, and whether the screen and the banner are wanted. Issue #136: no line of
/// keys at the bottom of the scene any more (« Commandes » in the panel opens them).
#[allow(clippy::too_many_arguments)]
pub fn sync_salon_tree(
    mut commands: Commands,
    window: Res<LobbyWindow>,
    session: Option<Res<NetSession>>,
    race: Option<Res<OnlineRace>>,
    (selection, options, race_menu, main_menu): (
        Option<Res<SelectionScreen>>,
        Option<Res<OptionsMenu>>,
        Option<Res<RaceMenu>>,
        Option<Res<MainMenu>>,
    ),
    (racing, background, online): (
        Option<Res<Race>>,
        Option<Res<Showcase>>,
        Option<Res<Online>>,
    ),
    (roster, settings, loadout, assets, cups): (
        Res<Roster>,
        Res<NetSettings>,
        Option<Res<PlayerLoadout>>,
        Option<Res<MenuAssets>>,
        Option<Res<GrandPrixTuning>>,
    ),
    menu: Option<ResMut<SalonMenu>>,
    time: Res<Time<Real>>,
    roots: Query<(Entity, &SalonScreenRoot)>,
    banners: Query<Entity, With<SalonBanner>>,
    cameras: Query<(), With<Camera>>,
) {
    let others = selection.as_deref().is_some_and(|s| s.open)
        || options.as_deref().is_some_and(|o| o.open)
        || race_menu.as_deref().is_some_and(|r| r.open)
        || main_menu.as_deref().is_some_and(|m| m.open);
    // Issue #141: over the home's background too (a race, not ours), not over a room's race end.
    let behind = Behind::of(
        racing.as_deref(),
        background.is_some(),
        race.as_deref()
            .is_some_and(|race| race.shared.over().is_some()),
    );
    let online = online.is_some();
    let wanted = screen_wanted(
        &window,
        selection.as_deref(),
        options.as_deref(),
        race_menu.as_deref(),
        main_menu.as_deref(),
        behind,
        online,
    ) && !cameras.is_empty();
    let (medium, semibold) = kit::faces(assets.as_deref());
    // The banner (#64): in a room, out of a race we run (`net::banner_shown`), the panel closed;
    // issue #141: like the panel, not over a race's end in a room, and over the home's background.
    let banner = (window.banner && !window.open && !others && behind.shows_the_lobby(online))
        .then(|| {
            RoomFacts::of(session.as_deref(), race.as_deref(), now_ms(&time))
                .and_then(|facts| facts.banner())
        })
        .flatten();
    match (banner, banners.iter().next()) {
        (Some(banner), None) => spawn_banner(&mut commands, &banner.to_string(), &semibold),
        (Some(banner), Some(entity)) => {
            commands.queue(write_banner(entity, banner.to_string()));
        }
        (None, Some(entity)) => commands.entity(entity).despawn(),
        (None, None) => {}
    }
    if !wanted {
        for (entity, _) in &roots {
            commands.entity(entity).despawn();
        }
        return;
    }
    let now = now(&time);
    // Issue #122: the cups of « Jouer solo » (`GrandPrixTuning::races` circuits each).
    let cup_size = cups.map_or(4, |tuning| tuning.races as usize);
    let Some(mut menu) = menu else {
        commands.insert_resource(SalonMenu {
            focus: 0,
            opened_at: now,
            focused_at: now,
            copied_at: None,
            catalog: Catalog::game(cup_size),
            view: None,
            latches: [StickLatch::default(); 2],
        });
        return;
    };
    let session = session.as_deref();
    let link = session.map(invitation);
    let pseudo = settings
        .pseudo
        .clone()
        .unwrap_or_else(|| character_name(loadout.as_deref().map(|l| &l.0), Some(&roster)));
    let view = salon_view(&ViewInput {
        session,
        race: race.as_deref(),
        roster: Some(&roster),
        catalog: &menu.catalog,
        pseudo: &pseudo,
        link: link.as_deref(),
        copied: menu.copied_at.is_some_and(|at| now - at < COPIED_SECONDS),
        loopback_page: loopback_page(),
        signal: session.map_or(&settings.signal, |s| &s.signal),
        local_signal: is_local_signal(session.map_or(&settings.signal, |s| &s.signal)),
        server: settings.server.as_deref(),
        now_ms: now_ms(&time),
    });
    let focus_count = focusables(&view).len();
    if menu.focus >= focus_count {
        menu.focus = focus_count.saturating_sub(1);
    }
    let shape = shape_of(&view);
    let mut shown = false;
    for (entity, root) in &roots {
        if !shown && root.shape == shape {
            shown = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !shown {
        if roots.is_empty() {
            // It opens now: it pops in, the focus on its first line (a tree built again while
            // open does not pop).
            menu.opened_at = now;
            menu.focus = 0;
            menu.focused_at = now;
        }
        spawn_tree(&mut commands, &view, shape, &menu, &medium, &semibold);
    }
    if menu.view.as_ref() != Some(&view) {
        menu.view = Some(view);
    }
}

/// A command: the banner writes `text`.
fn write_banner(entity: Entity, text: String) -> impl FnOnce(&mut World) {
    move |world: &mut World| {
        // Its text is in its pill (issue #136: the pill is the button; it was looked for one
        // level too high, and the banner kept its first text).
        let pills = world
            .get::<Children>(entity)
            .map(|c| c.to_vec())
            .unwrap_or_default();
        let texts: Vec<Entity> = pills
            .iter()
            .filter_map(|pill| world.get::<Children>(*pill))
            .flat_map(|children| children.iter().collect::<Vec<_>>())
            .collect();
        for child in texts {
            if let Some(mut written) = world.get_mut::<Text>(child)
                && written.0 != text
            {
                written.0.clone_from(&text);
            }
        }
    }
}

/// Issue #136: the banner's pill, a button: a click opens the panel (Entrée too, in the scene).
#[derive(Component, Clone, Copy, Debug, Default)]
#[require(MenuNode)]
pub struct SalonBannerPill;

/// The banner's colors: the cream pill, Mangue under the pointer (the charter's focus).
fn banner_colors(hovered: bool) -> (BackgroundColor, BorderColor) {
    if hovered {
        (
            BackgroundColor(palette::MANGUE),
            BorderColor::all(palette::ABRICOT),
        )
    } else {
        (
            BackgroundColor(palette::GUIMAUVE),
            BorderColor::all(palette::LILAS),
        )
    }
}

/// The banner's pill, as the pointer is over it (or not).
type BannerPills<'w, 's> =
    Query<'w, 's, (&'static mut BackgroundColor, &'static mut BorderColor), With<SalonBannerPill>>;

/// The pointer over the banner's pill (or out of it).
fn hover_banner(pills: &mut BannerPills, hovered: bool) {
    let (fill, depth) = banner_colors(hovered);
    for (mut background, mut border) in pills {
        background.set_if_neq(fill);
        border.set_if_neq(depth);
    }
}

fn spawn_banner(commands: &mut Commands, text: &str, face: &Handle<Font>) {
    commands
        .spawn((
            Name::new("Salon : bandeau"),
            SalonBanner,
            Node {
                position_type: PositionType::Absolute,
                top: u(12.0),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(15),
            Pickable::IGNORE,
        ))
        .with_children(|banner| {
            let (fill, depth) = banner_colors(false);
            banner
                .spawn((
                    SalonBannerPill,
                    Node {
                        padding: UiRect::axes(u(18.0), u(8.0)),
                        border: UiRect::bottom(u(size::LINE_DEPTH)),
                        border_radius: BorderRadius::all(u(22.0)),
                        ..default()
                    },
                    fill,
                    depth,
                ))
                .with_children(|pill| {
                    pill.spawn((
                        SalonText::Banner,
                        kit::text(text, face, 18.0, palette::MYRTILLE),
                        Pickable::IGNORE,
                    ));
                })
                // Issue #136: the banner is the panel's button (no « (L) » in it any more).
                .observe(|_: On<PointerOver>, mut pills: BannerPills| {
                    hover_banner(&mut pills, true);
                })
                .observe(|_: On<PointerOut>, mut pills: BannerPills| {
                    hover_banner(&mut pills, false);
                })
                .observe(|_: On<PointerClick>, mut net: MessageWriter<NetAction>| {
                    net.write(NetAction::Toggle);
                });
        });
}

/// The look of a line or a button: the charter's button, focused or not, or disabled.
fn look(focused: bool, enabled: bool) -> style::ButtonLook {
    let state = match (enabled, focused) {
        (false, _) => ButtonState::Disabled,
        (true, true) => ButtonState::Focused,
        (true, false) => ButtonState::Normal,
    };
    style::button_look(state, palette::FRAISE)
}

/// The accent of a button: its badge's color.
fn accent(kind: ButtonKind) -> Color {
    match kind {
        ButtonKind::StartRace | ButtonKind::Ready | ButtonKind::NotReady => palette::MENTHE,
        ButtonKind::CreateRoom | ButtonKind::Retry | ButtonKind::JoinAgain => palette::FRAISE,
        ButtonKind::JoinAsGuest | ButtonKind::JoinGame => palette::FRAISE,
        ButtonKind::Copy => palette::LAGON,
        ButtonKind::Pilot => palette::CAROTTE,
        // Issue #136: the options' color (their list of commands).
        ButtonKind::Controls => palette::PRUNE,
        ButtonKind::Leave | ButtonKind::Back => palette::MYRTILLE,
    }
}

fn text(
    key: SalonText,
    content: impl Into<String>,
    face: &Handle<Font>,
    px: f32,
    color: Color,
) -> impl Bundle {
    (key, kit::text(content, face, px, color), Pickable::IGNORE)
}

fn spawn_tree(
    commands: &mut Commands,
    view: &SalonView,
    shape: Shape,
    menu: &SalonMenu,
    medium: &Handle<Font>,
    semibold: &Handle<Font>,
) {
    let list = focusables(view);
    let focus = list.get(menu.focus).copied();
    let help = help_of(view, focus);
    commands
        .spawn((
            Name::new("Salon"),
            SalonScreenRoot { shape },
            GlobalZIndex(20),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(palette::VOILE.with_alpha(0.0)),
        ))
        .with_children(|root| {
            root.spawn((
                SalonColumn,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: u(10.0),
                    ..default()
                },
                UiTransform::from_scale(Vec2::splat(anim::POP_FROM)),
                Pickable::IGNORE,
            ))
            .with_children(|column| {
                column
                    .spawn(text(
                        SalonText::Title,
                        view.title.clone(),
                        semibold,
                        size::PILOT_TITLE,
                        palette::NEIGE,
                    ))
                    .insert(TextShadow {
                        offset: Vec2::new(0.0, 4.0),
                        color: palette::FRAISE,
                    });
                column
                    .spawn((
                        MenuNode,
                        Node {
                            flex_direction: FlexDirection::Row,
                            width: u(size::PILOT_WIDTH),
                            padding: UiRect::all(u(size::PANEL_PADDING)),
                            column_gap: u(size::PILOT_GAP),
                            border_radius: BorderRadius::all(u(size::PANEL_RADIUS)),
                            ..default()
                        },
                        BackgroundColor(palette::GUIMAUVE),
                        BoxShadow::new(
                            palette::OMBRE,
                            Val::ZERO,
                            u(size::PANEL_SHADOW),
                            Val::ZERO,
                            Val::ZERO,
                        ),
                        Pickable::IGNORE,
                    ))
                    .with_children(|panel| {
                        spawn_left(panel, view, (&list, focus), &help, medium, semibold);
                        spawn_right(panel, view, (&list, focus), medium, semibold);
                    });
            });
        });
}

/// The size of a setting's pill: its height, its text, the gap under it (px at 720).
#[derive(Clone, Copy, Debug, PartialEq)]
struct LineSize {
    height: f32,
    text: f32,
    gap: f32,
}

/// The pills of the charter (`size::LINE_HEIGHT`)...
const LINE: LineSize = LineSize {
    height: size::LINE_HEIGHT,
    text: size::LINE_TEXT,
    gap: size::LINE_GAP,
};
/// ...issue #122: smaller when the championship's lines would not fit in the panel (more than
/// [`FULL_LINES`] of them)...
const COMPACT_LINE: LineSize = LineSize {
    height: 36.0,
    text: 18.0,
    gap: 5.0,
};
/// ...smaller still beyond [`COMPACT_LINES`] (a custom championship: seven lines)...
const TIGHT_LINE: LineSize = LineSize {
    height: 30.0,
    text: 16.0,
    gap: 3.0,
};
/// ...and for a custom championship's races, on the right, with the program.
const RACE_LINE: LineSize = LineSize {
    height: 30.0,
    text: 16.0,
    gap: 3.0,
};
/// The most settings' lines of the charter's size on the left.
const FULL_LINES: usize = 5;
/// ...and of the compact size.
const COMPACT_LINES: usize = 6;

/// The size of the settings' lines on the left, for `count` of them.
fn line_size(count: usize) -> LineSize {
    match count {
        count if count > COMPACT_LINES => TIGHT_LINE,
        count if count > FULL_LINES => COMPACT_LINE,
        _ => LINE,
    }
}

/// The left column: the lines, the settings, the help, the buttons (« Copier » is on the
/// right, with the link).
#[allow(clippy::too_many_arguments)]
fn spawn_left(
    panel: &mut ChildSpawnerCommands,
    view: &SalonView,
    (list, focus): (&[Focusable], Option<Focusable>),
    help: &str,
    medium: &Handle<Font>,
    semibold: &Handle<Font>,
) {
    panel
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                width: u(size::SOLO_LEFT),
                flex_shrink: 0.0,
                row_gap: u(size::LINE_GAP),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|left| {
            for (index, line) in view.lines.iter().enumerate() {
                let (face, px) = if index == 1 {
                    (semibold, 19.0)
                } else {
                    (medium, size::SOLO_HELP)
                };
                left.spawn((
                    text(
                        SalonText::Line(index),
                        line.clone(),
                        face,
                        px,
                        palette::MYRTILLE,
                    ),
                    Node {
                        margin: UiRect::left(u(6.0)),
                        ..default()
                    },
                ));
            }
            if let Some(seat) = view.seat.as_ref() {
                // Issue #122: the mode's lines (the races of a custom one are on the right).
                let rows: Vec<SalonRow> =
                    seat.rows.iter().copied().filter(|r| !r.is_race()).collect();
                let size = line_size(rows.len());
                left.spawn((
                    MenuNode,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: u(size.gap),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|lines| {
                    for row in rows {
                        let index = list.iter().position(|f| *f == Focusable::Row(row));
                        let state = (index, seat.editable, focus);
                        spawn_line(lines, view, row, state, size, semibold);
                    }
                });
                left.spawn((
                    text(
                        SalonText::Help,
                        help,
                        medium,
                        size::SOLO_HELP,
                        palette::MYRTILLE.with_alpha(0.75),
                    ),
                    Node {
                        margin: UiRect::left(u(18.0)),
                        min_height: u(size::SOLO_HELP * 1.3),
                        ..default()
                    },
                ));
            }
            let mut buttons = view
                .buttons
                .iter()
                .enumerate()
                .filter(|(_, b)| b.kind != ButtonKind::Copy);
            // The main action, wide.
            if let Some((index, _)) = buttons.next() {
                spawn_button(left, view, list, index, focus, medium, semibold);
            }
            left.spawn((
                MenuNode,
                Node {
                    column_gap: u(12.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                for (index, _) in buttons {
                    spawn_button(row, view, list, index, focus, medium, semibold);
                }
            });
        });
}

/// A setting's pill: its label, « < », its value, « > » (its value written by
/// [`refresh_salon_tree`]).
fn spawn_line(
    column: &mut ChildSpawnerCommands,
    view: &SalonView,
    row: SalonRow,
    (index, editable, focus): (Option<usize>, bool, Option<Focusable>),
    line_size: LineSize,
    face: &Handle<Font>,
) {
    let look = look(focus == Some(Focusable::Row(row)), editable);
    let value = view
        .seat
        .as_ref()
        .map(|seat| seat.value(row).to_string())
        .unwrap_or_default();
    let mut line = column.spawn((
        Name::new(row.label().into_owned()),
        SalonLineNode { row },
        Node {
            height: u(line_size.height),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: u(8.0),
            padding: UiRect {
                left: u(18.0),
                right: u(8.0),
                ..default()
            },
            border: UiRect::bottom(u(size::LINE_DEPTH.min(line_size.height / 8.0))),
            border_radius: BorderRadius::all(u(line_size.height / 2.0)),
            ..default()
        },
        BackgroundColor(look.fill),
        BorderColor::all(look.depth),
        UiTransform::IDENTITY,
    ));
    // The arrows a little smaller than the pill.
    let arrow_size = size::ARROW.min(line_size.height - 6.0);
    line.with_children(|line| {
        line.spawn((
            kit::text(row.label(), face, line_size.text, look.label),
            MenuNode,
            Pickable::IGNORE,
            Node {
                width: u(size::LINE_LABEL_WIDTH * line_size.text / size::LINE_TEXT),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        for step in [-1, 1] {
            let mut arrow = line.spawn((
                MenuNode,
                Node {
                    width: u(arrow_size),
                    height: u(arrow_size),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(if editable {
                    palette::LILAS
                } else {
                    palette::BRUME_FONCEE
                }),
            ));
            arrow.with_children(|arrow| {
                arrow.spawn((
                    kit::text(
                        if step < 0 { "<" } else { ">" },
                        face,
                        line_size.text,
                        look.label,
                    ),
                    MenuNode,
                    Pickable::IGNORE,
                ));
            });
            if editable {
                arrow.observe(
                    move |_: On<PointerClick>, mut actions: MessageWriter<SalonAction>| {
                        actions.write(SalonAction::Step(row, step));
                    },
                );
            }
            if step < 0 {
                line.spawn((
                    text(
                        SalonText::Value(row),
                        value.clone(),
                        face,
                        line_size.text,
                        look.label,
                    ),
                    TextLayout::justify(Justify::Center),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ));
            }
        }
    });
    if let Some(index) = index {
        line.observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<SalonAction>| {
                actions.write(SalonAction::Focus(index));
            },
        );
    }
}

/// A button: a pill with its depth, its accent's dot, its label and its small line.
#[allow(clippy::too_many_arguments)]
fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    view: &SalonView,
    list: &[Focusable],
    index: usize,
    focus: Option<Focusable>,
    medium: &Handle<Font>,
    semibold: &Handle<Font>,
) {
    let Some(button) = view.buttons.get(index) else {
        return;
    };
    let kind = button.kind;
    let focus_index = list.iter().position(|f| *f == Focusable::Button(index));
    let look = look(focus == Some(Focusable::Button(index)), button.enabled);
    let main = index == 0;
    // Issue #136: three buttons under the main one (« Pilote », « Commandes », « Quitter »): each
    // as wide as its label, the room left shared out.
    let mut node = parent.spawn((
        Name::new(button.label.clone()),
        SalonButtonNode { index, kind },
        Node {
            flex_grow: 1.0,
            flex_basis: if main { Val::ZERO } else { Val::Auto },
            min_height: u(size::PILOT_BUTTON_HEIGHT),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(u(if main { 14.0 } else { 10.0 }), u(4.0)),
            border: UiRect::bottom(u(size::BUTTON_DEPTH)),
            border_radius: BorderRadius::all(u(size::PILOT_BUTTON_HEIGHT / 2.0)),
            ..default()
        },
        BackgroundColor(look.fill),
        BorderColor::all(look.depth),
        UiTransform::IDENTITY,
    ));
    node.with_children(|node| {
        node.spawn((
            MenuNode,
            Node {
                column_gap: u(10.0),
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|top| {
            top.spawn((
                MenuNode,
                Node {
                    width: u(18.0),
                    height: u(18.0),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(if button.enabled {
                    accent(kind)
                } else {
                    palette::BRUME_FONCEE
                }),
                Pickable::IGNORE,
            ));
            top.spawn(text(
                SalonText::Label(index),
                button.label.clone(),
                semibold,
                if main {
                    size::LINE_TEXT + 5.0
                } else {
                    size::LINE_TEXT + 1.0
                },
                look.label,
            ));
        });
        if main {
            node.spawn((
                text(
                    SalonText::Caption(index),
                    button.caption.clone(),
                    medium,
                    size::CAPTION,
                    look.caption,
                ),
                TextLayout::justify(Justify::Center),
            ));
        }
    });
    if let Some(focus_index) = focus_index {
        node.observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<SalonAction>| {
                actions.write(SalonAction::Focus(focus_index));
            },
        )
        .observe(
            move |_: On<PointerClick>, mut actions: MessageWriter<SalonAction>| {
                actions.write(SalonAction::Press(kind));
            },
        );
    }
}

/// The right column: the players, issue #122 a championship's program (its races, the host's
/// lines when chosen one by one), the invitation, the notes.
fn spawn_right(
    panel: &mut ChildSpawnerCommands,
    view: &SalonView,
    (list, focus): (&[Focusable], Option<Focusable>),
    medium: &Handle<Font>,
    semibold: &Handle<Font>,
) {
    panel
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: u(4.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|right| {
            if !view.players_header.is_empty() {
                right.spawn(text(
                    SalonText::Header,
                    view.players_header.clone(),
                    semibold,
                    22.0,
                    palette::MYRTILLE,
                ));
            }
            for (index, player) in view.players.iter().enumerate() {
                right
                    .spawn((
                        SalonPlayerNode(index),
                        kit::row(28.0, 8.0, player.me),
                        Pickable::IGNORE,
                    ))
                    .with_children(|line| {
                        line.spawn((
                            SalonDot(index),
                            Node {
                                width: u(16.0),
                                height: u(16.0),
                                flex_shrink: 0.0,
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(
                                player.color.map_or(palette::BRUME_FONCEE, |c| c.color()),
                            ),
                            Pickable::IGNORE,
                        ));
                        line.spawn((
                            text(
                                SalonText::PlayerName(index),
                                player.name.clone(),
                                semibold,
                                17.0,
                                palette::MYRTILLE,
                            ),
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        line.spawn(text(
                            SalonText::PlayerAnimal(index),
                            player.animal.clone(),
                            medium,
                            14.0,
                            palette::MYRTILLE.with_alpha(0.7),
                        ));
                        line.spawn(text(
                            SalonText::PlayerLink(index),
                            player.link.clone(),
                            medium,
                            14.0,
                            if player.trouble {
                                palette::FRAISE
                            } else {
                                palette::MYRTILLE.with_alpha(0.7)
                            },
                        ));
                        for host in [true, false] {
                            line.spawn((
                                SalonBadge {
                                    player: index,
                                    host,
                                },
                                Node {
                                    padding: UiRect::axes(u(8.0), u(1.0)),
                                    border_radius: BorderRadius::all(u(10.0)),
                                    display: if (host && player.host) || (!host && player.ready) {
                                        Display::Flex
                                    } else {
                                        Display::None
                                    },
                                    ..default()
                                },
                                BackgroundColor(if host {
                                    palette::MANGUE
                                } else {
                                    palette::MENTHE
                                }),
                                Pickable::IGNORE,
                            ))
                            .with_children(|badge| {
                                badge.spawn((
                                    kit::text(
                                        if host { "hôte" } else { "prêt" },
                                        medium,
                                        13.0,
                                        palette::MYRTILLE,
                                    ),
                                    MenuNode,
                                    Pickable::IGNORE,
                                ));
                            });
                        }
                    });
            }
            if let Some(seat) = view.seat.as_ref().filter(|s| !s.program_title.is_empty()) {
                spawn_program(right, view, seat, (list, focus), medium, semibold);
            }
            if view.invite.is_some() {
                right.spawn((
                    MenuNode,
                    kit::text("Invite tes amis :", semibold, 17.0, palette::MYRTILLE),
                    Node {
                        margin: UiRect::top(u(8.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                right
                    .spawn((
                        MenuNode,
                        Node {
                            column_gap: u(10.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|invite| {
                        invite.spawn((
                            text(
                                SalonText::Invite,
                                view.invite.clone().unwrap_or_default(),
                                medium,
                                12.0,
                                palette::MYRTILLE.with_alpha(0.8),
                            ),
                            Node {
                                flex_grow: 1.0,
                                flex_shrink: 1.0,
                                max_width: u(250.0),
                                ..default()
                            },
                        ));
                        if let Some(index) =
                            view.buttons.iter().position(|b| b.kind == ButtonKind::Copy)
                        {
                            spawn_copy(invite, view, list, index, medium, semibold);
                        }
                    });
            }
            for (index, note) in view.notes.iter().enumerate() {
                right.spawn(text(
                    SalonText::Note(index),
                    note.clone(),
                    medium,
                    14.0,
                    if index == 0 {
                        palette::FRAISE
                    } else {
                        palette::MYRTILLE.with_alpha(0.8)
                    },
                ));
            }
        });
}

/// Issue #122: a championship's program, under the players: its header, then each race (« 1 ·
/// Prairie Pompon »), or for a custom one chosen one by one the host's lines, a pill each.
fn spawn_program(
    right: &mut ChildSpawnerCommands,
    view: &SalonView,
    seat: &super::view::Seat,
    (list, focus): (&[Focusable], Option<Focusable>),
    medium: &Handle<Font>,
    semibold: &Handle<Font>,
) {
    right.spawn((
        text(
            SalonText::ProgramTitle,
            seat.program_title.clone(),
            semibold,
            17.0,
            palette::MYRTILLE,
        ),
        Node {
            margin: UiRect::top(u(8.0)),
            ..default()
        },
    ));
    let races: Vec<SalonRow> = seat.rows.iter().copied().filter(|r| r.is_race()).collect();
    if races.is_empty() {
        for (index, race) in seat.program.iter().enumerate() {
            right.spawn((
                text(
                    SalonText::Program(index),
                    race.clone(),
                    medium,
                    15.0,
                    palette::MYRTILLE.with_alpha(0.85),
                ),
                Node {
                    margin: UiRect::left(u(8.0)),
                    ..default()
                },
            ));
        }
        return;
    }
    right
        .spawn((
            MenuNode,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: u(RACE_LINE.gap),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|lines| {
            for row in races {
                let index = list.iter().position(|f| *f == Focusable::Row(row));
                spawn_line(
                    lines,
                    view,
                    row,
                    (index, seat.editable, focus),
                    RACE_LINE,
                    semibold,
                );
            }
        });
}

/// « Copier », by the link (its small line says « Copié ! » once it was).
fn spawn_copy(
    parent: &mut ChildSpawnerCommands,
    view: &SalonView,
    list: &[Focusable],
    index: usize,
    medium: &Handle<Font>,
    semibold: &Handle<Font>,
) {
    let focus_index = list.iter().position(|f| *f == Focusable::Button(index));
    let look = look(false, true);
    parent
        .spawn((
            Name::new("Copier"),
            SalonButtonNode {
                index,
                kind: ButtonKind::Copy,
            },
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(u(14.0), u(4.0)),
                border: UiRect::bottom(u(size::LINE_DEPTH)),
                border_radius: BorderRadius::all(u(18.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(look.fill),
            BorderColor::all(look.depth),
            UiTransform::IDENTITY,
        ))
        .with_children(|copy| {
            copy.spawn(text(
                SalonText::Label(index),
                view.buttons[index].label.clone(),
                semibold,
                size::LINE_TEXT,
                look.label,
            ));
            copy.spawn(text(
                SalonText::Caption(index),
                view.buttons[index].caption.clone(),
                medium,
                12.0,
                look.caption,
            ));
        })
        .observe(
            move |_: On<PointerOver>, mut actions: MessageWriter<SalonAction>| {
                if let Some(focus) = focus_index {
                    actions.write(SalonAction::Focus(focus));
                }
            },
        )
        .observe(
            move |_: On<PointerClick>, mut actions: MessageWriter<SalonAction>| {
                actions.write(SalonAction::Press(ButtonKind::Copy));
            },
        );
}

/// The texts change in place (the pings, the names, the captions...), the focus takes its look
/// and grows a little, the column pops in, the veil fades in.
#[allow(clippy::type_complexity)]
pub fn refresh_salon_tree(
    menu: Option<Res<SalonMenu>>,
    time: Res<Time<Real>>,
    mut texts: Query<(&SalonText, &mut Text, Option<&mut TextColor>)>,
    mut veils: Query<&mut BackgroundColor, With<SalonScreenRoot>>,
    mut columns: Query<
        &mut UiTransform,
        (
            With<SalonColumn>,
            Without<SalonLineNode>,
            Without<SalonButtonNode>,
        ),
    >,
    mut lines: Query<
        (
            &SalonLineNode,
            &mut UiTransform,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            Without<SalonScreenRoot>,
            Without<SalonColumn>,
            Without<SalonButtonNode>,
        ),
    >,
    mut buttons: Query<
        (
            &SalonButtonNode,
            &mut UiTransform,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        (
            Without<SalonScreenRoot>,
            Without<SalonColumn>,
            Without<SalonLineNode>,
        ),
    >,
    mut badges: Query<(&SalonBadge, &mut Node)>,
    mut dots: Query<
        (&SalonDot, &mut BackgroundColor),
        (
            Without<SalonScreenRoot>,
            Without<SalonLineNode>,
            Without<SalonButtonNode>,
        ),
    >,
) {
    let Some(menu) = menu else {
        return;
    };
    let Some(view) = menu.view.as_ref() else {
        return;
    };
    let now = now(&time);
    let list = focusables(view);
    let focus = list.get(menu.focus).copied();
    let help = help_of(view, focus);
    for (key, mut written, _) in &mut texts {
        if *key == SalonText::Banner {
            continue;
        }
        let wanted = text_of(view, *key, &help);
        if written.0 != wanted {
            written.0 = wanted;
        }
    }
    let opened = (now - menu.opened_at) as f32;
    for mut veil in &mut veils {
        veil.set_if_neq(BackgroundColor(
            palette::VOILE.with_alpha(palette::VOILE.alpha() * anim::veil(opened)),
        ));
    }
    for mut transform in &mut columns {
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(anim::panel_pop(
            opened,
        ))));
    }
    let since = (now - menu.focused_at) as f32;
    let scale = |focused: bool| {
        if focused {
            anim::focus_scale(true, since)
        } else {
            1.0
        }
    };
    let editable = view.seat.as_ref().is_some_and(|seat| seat.editable);
    for (line, mut transform, mut fill, mut depth) in &mut lines {
        let focused = focus == Some(Focusable::Row(line.row));
        let look = look(focused, editable);
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(scale(focused))));
    }
    for (button, mut transform, mut fill, mut depth) in &mut buttons {
        let enabled = view.buttons.get(button.index).is_some_and(|b| b.enabled);
        let focused = focus == Some(Focusable::Button(button.index));
        let look = look(focused, enabled);
        fill.set_if_neq(BackgroundColor(look.fill));
        depth.set_if_neq(BorderColor::all(look.depth));
        transform.set_if_neq(UiTransform::from_scale(Vec2::splat(scale(focused))));
    }
    for (badge, mut node) in &mut badges {
        let shown = view.players.get(badge.player).is_some_and(|player| {
            if badge.host {
                player.host
            } else {
                player.ready
            }
        });
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    for (dot, mut fill) in &mut dots {
        let color = view
            .players
            .get(dot.0)
            .and_then(|player| player.color)
            .map_or(palette::BRUME_FONCEE, |color| color.color());
        fill.set_if_neq(BackgroundColor(color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::RaceTuning;

    /// A race at tick `tick`, over at `over_at` (its awaited racers finished).
    fn race_at(tick: u32, over_at: Option<u32>) -> Race {
        let mut race = Race::new(&RaceTuning::default());
        race.flow.tick = tick;
        race.flow.over_at = over_at;
        race
    }

    /// Issue #141: what is behind the lobby's screen. No race, or the home's background (a race,
    /// not ours, whatever its phase): nothing. A race before its finish: a race; from its finish
    /// (or the host's end, online, a tick before ours): its end.
    #[test]
    fn behind_the_lobby_is_a_race_its_end_or_nothing() {
        let timings = race_at(0, None).timings;
        let racing = race_at(timings.before_go() + 60, None);
        assert_eq!(racing.phase(), RacePhase::Racing);
        let finished = race_at(timings.before_go() + 60, Some(timings.before_go() + 50));
        assert_eq!(finished.phase(), RacePhase::Finished);
        let results = race_at(timings.before_go() + 600, Some(timings.before_go() + 50));
        assert_eq!(results.phase(), RacePhase::Results);
        let grid = race_at(1, None);
        assert_eq!(grid.phase(), RacePhase::Grid);

        assert_eq!(Behind::of(None, false, false), Behind::NoRace);
        assert_eq!(Behind::of(None, true, false), Behind::NoRace);
        for race in [&grid, &racing, &finished, &results] {
            assert_eq!(
                Behind::of(Some(race), true, false),
                Behind::NoRace,
                "{race:?}"
            );
        }
        assert_eq!(Behind::of(Some(&grid), false, false), Behind::Race);
        assert_eq!(Behind::of(Some(&racing), false, false), Behind::Race);
        assert_eq!(Behind::of(Some(&finished), false, false), Behind::End);
        assert_eq!(Behind::of(Some(&results), false, false), Behind::End);
        assert_eq!(
            Behind::of(Some(&racing), false, true),
            Behind::End,
            "the host's end"
        );
    }

    /// Issue #141: the lobby's screen shows over anything but the end of a race in a room; its
    /// panel holds the game over a race only (solo: the game waits; online: our kart coasts).
    #[test]
    fn the_lobby_shows_over_all_but_a_room_s_race_end_and_holds_a_race() {
        // (behind, in a room): shows, holds.
        let cases = [
            ((Behind::NoRace, false), (true, false)),
            ((Behind::NoRace, true), (true, false)),
            ((Behind::Race, false), (true, true)),
            ((Behind::Race, true), (true, true)),
            ((Behind::End, false), (true, true)),
            ((Behind::End, true), (false, false)),
        ];
        for ((behind, online), (shows, holds)) in cases {
            assert_eq!(behind.shows_the_lobby(online), shows, "{behind:?} {online}");
            assert_eq!(
                behind.held_by_the_lobby(online),
                holds,
                "{behind:?} {online}"
            );
        }
        // The panel: the window open, no other screen over it, over what it shows over.
        let open = LobbyWindow {
            open: true,
            banner: false,
        };
        let wanted = |window: &LobbyWindow, menu: Option<&MainMenu>, behind, online| {
            screen_wanted(window, None, None, None, menu, behind, online)
        };
        assert!(
            wanted(&open, None, Behind::NoRace, false),
            "the home's « Multijoueur »"
        );
        assert!(wanted(&open, None, Behind::Race, false), "L in a race");
        assert!(
            wanted(&open, None, Behind::End, false),
            "L on a race's results, solo"
        );
        assert!(
            wanted(&open, None, Behind::NoRace, true),
            "the lobby's scene"
        );
        assert!(!wanted(&open, None, Behind::End, true), "a room's results");
        assert!(!wanted(
            &LobbyWindow::default(),
            None,
            Behind::NoRace,
            false
        ));
        let pause = MainMenu {
            open: true,
            in_game: true,
            ..default()
        };
        assert!(
            !wanted(&open, Some(&pause), Behind::Race, false),
            "one screen at a time"
        );
    }
}
