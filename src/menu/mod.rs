//! The game's screens with the art direction (issue #56, `docs/UI_CHARTE.md`), step 1: the
//! charter (`style.rs`, `anim.rs`) and the first screen built with it, the **main menu**.
//!
//! - **Entries** (`nav.rs`): « Course solo » opens the race menu (N, issue #19), « En ligne » the
//!   lobby (L, issue #7: create or join a game), « Options » the options menu (O, issue #17),
//!   « Pilote et kart » the selection screen (G, issue #15). Opened from the game, « Reprendre »
//!   comes first. Online, « Course solo » is disabled (the host chooses the race), with its reason.
//! - **When**: at launch, before the race (with a renderer only: [`MainMenuPlugin::at_launch`]);
//!   with `?room=…` in the address, straight to the lobby without it (and the selection screen,
//!   as before). Then **Échap** ([`MAIN_MENU_KEY`]) or the gamepad's **Start**
//!   ([`MAIN_MENU_BUTTON`]) open it from the game, when no other screen is open (Échap closes
//!   those first). A screen opened from the menu (or over it, by its shortcut) comes back to the
//!   menu when it closes without starting anything: Échap in the options goes back to the menu;
//!   « Course ! » races.
//! - **Pause**: in solo, the game waits while it is open, like the other menus; **online, never**
//!   (Max's decision, issue #9): the kart rolls on with its momentum, only its keys are the
//!   menu's (`characters::pause_in_menus`, `kart::PlayerInputMuted`).
//! - **Keys** (no Fn on a Mac, AZERTY and QWERTY alike): up and down (the arrows, or Z/S in
//!   AZERTY and W/S in QWERTY: the layout's keys), **Entrée** or **Espace** to choose, **Échap**
//!   to go back; the gamepad's cross or left stick, **A** to choose, **B** or **Start** to go
//!   back; the mouse: the pointer gives the focus, a click chooses. One focus for the three. A
//!   button squashes while it is held and stretches when it is released; what it does comes just
//!   after ([`anim::ACTIVATE_DELAY`]).
//! - **Drawn with `bevy_ui`** (`screen.rs`, the choice is argued in the charter): the tree only
//!   exists while the menu is open (spawned when it opens, despawned when it closes); no egui
//!   context for it. Closed, the menu costs a few resource reads per frame.
//!
//! **Only in a game with a renderer** (`crate::GamePlugin`): the headless game has none of it,
//! so that its world stays the one the bot races were measured on (a resource is an entity in
//! Bevy 0.20, and one more archetype before the grid changes every bot race: `STATUS.md`). The
//! tests add [`MainMenuPlugin`] to a headless game themselves; everything in it runs headless.

mod anim;
mod nav;
mod screen;
mod style;

#[cfg(test)]
mod scenario_tests;

pub use anim::ACTIVATE_DELAY;
pub use nav::{Entry, GAME_ENTRIES, LAUNCH_ENTRIES, StickLatch, enabled, entries, moved};
pub use screen::{MainMenuRoot, MenuButton, MenuNode};
pub use style::{ButtonLook, ButtonState, UI_FILES, button_look};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::characters::{SelectionAction, SelectionScreen};
use crate::kart::KeyboardLayout;
use crate::level::{LoadLevel, Online};
use crate::modes::{RaceMenu, RaceMenuAction};
use crate::net::{LobbyWindow, NetAction, NetSession};
use crate::options::{OptionsAction, OptionsMenu, OptionsSystems};
use crate::race::{Race, RacePhase};

/// Opens the main menu from the game (and goes back from it): Échap, no Fn on a Mac, the same
/// key in AZERTY and QWERTY. While another screen is open, Échap closes that screen instead.
pub const MAIN_MENU_KEY: KeyCode = KeyCode::Escape;
/// The gamepad's Start opens it too (GD § 13), and closes it. Not on the results screen, where
/// Start races again (`race::RESTART_BUTTONS`).
pub const MAIN_MENU_BUTTON: GamepadButton = GamepadButton::Start;

/// Chooses the entry with the focus (on release).
pub const CONFIRM_KEYS: [KeyCode; 3] = [KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space];
const CONFIRM_BUTTON: GamepadButton = GamepadButton::South;
const BACK_BUTTONS: [GamepadButton; 2] = [GamepadButton::East, MAIN_MENU_BUTTON];

/// A screen opened from the menu waits this many frames to show up before the menu comes back.
const HAND_OVER_FRAMES: u8 = 3;

/// A screen the main menu opens, or that opens over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
    /// The race menu (N).
    RaceMenu,
    /// The lobby window (L).
    Lobby,
    /// The options menu (O).
    Options,
    /// The selection screen (G).
    Selection,
}

impl Screen {
    pub const ALL: [Self; 4] = [
        Screen::RaceMenu,
        Screen::Lobby,
        Screen::Options,
        Screen::Selection,
    ];

    /// The entry of the menu that opens it.
    pub fn entry(self) -> Entry {
        match self {
            Screen::RaceMenu => Entry::Solo,
            Screen::Lobby => Entry::Online,
            Screen::Options => Entry::Options,
            Screen::Selection => Entry::Garage,
        }
    }

    /// The game waits while it is open in solo: every one but the lobby (issue #7).
    pub fn pauses(self) -> bool {
        self != Screen::Lobby
    }
}

/// The main menu: whether it is open, its focus, what is pressed, and the screen it opened.
/// The real times (s, `Time<Real>`: the game is paused behind it) drive its animations.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct MainMenu {
    pub open: bool,
    /// Opened from the game (Échap, Start): « Reprendre » is its first entry. At launch, not.
    pub in_game: bool,
    /// The entry with the focus (an index of `entries(in_game)`).
    pub focus: usize,
    /// The entry held down (a key, the gamepad's A, the mouse), squashed.
    pub pressed: Option<usize>,
    /// The entry released at this time: what it does comes [`ACTIVATE_DELAY`] later.
    pub activating: Option<(usize, f64)>,
    /// When it opened (its tree, when it showed up: `screen.rs`).
    pub opened_at: f64,
    /// When the focus last moved, and the entry that lost it.
    pub focused_at: f64,
    pub unfocused: Option<usize>,
    pub pressed_at: f64,
    /// The last button let go, when, and how squashed it was then.
    pub released: Option<usize>,
    pub released_at: f64,
    pub released_from: f32,
    /// The screen opened from the menu (or over it): closing it without starting anything comes
    /// back to the menu.
    pub child: Option<Screen>,
    /// A screen the menu is opening (frames waited): until it shows, the menu still holds the
    /// game.
    pub handing_over: Option<(Screen, u8)>,
    /// Whether another screen was open at the end of the last frame: Échap closed it, it does
    /// not also open the menu.
    pub others_open: bool,
}

impl MainMenu {
    /// The menu's entries.
    pub fn entries(&self) -> &'static [Entry] {
        entries(self.in_game)
    }

    /// The entry with the focus.
    pub fn focused(&self) -> Entry {
        let entries = self.entries();
        entries[self.focus.min(entries.len() - 1)]
    }

    /// The menu holds the game: open, opening a screen, or one of its screens is open (until the
    /// menu knows whether it comes back). In solo the game waits; online, the kart coasts.
    pub fn holds_the_game(&self) -> bool {
        self.open
            || self.handing_over.is_some_and(|(screen, _)| screen.pauses())
            || self.child.is_some_and(Screen::pauses)
    }

    /// Opens it at `now`, the focus on `entry` if it can be used (else the first that can).
    fn show(&mut self, in_game: bool, entry: Option<Entry>, online: bool, now: f64) {
        let entries = entries(in_game);
        let focus = entry
            .filter(|entry| enabled(*entry, online))
            .and_then(|entry| entries.iter().position(|e| *e == entry))
            .unwrap_or_else(|| nav::first_enabled(entries, online));
        *self = MainMenu {
            open: true,
            in_game,
            focus,
            opened_at: now,
            focused_at: now - anim::FOCUS_TIME as f64,
            released_at: now - anim::RELEASE_TIME as f64,
            released_from: 1.0,
            others_open: self.others_open,
            ..default()
        };
    }

    fn set_focus(&mut self, focus: usize, now: f64) {
        if focus != self.focus {
            self.unfocused = Some(self.focus);
            self.focus = focus;
            self.focused_at = now;
        }
    }

    /// The pressed button lets go at `now` (squashed for as long as it was held).
    fn let_go(&mut self, now: f64) {
        if let Some(index) = self.pressed.take() {
            self.released = Some(index);
            self.released_from = anim::press_height((now - self.pressed_at) as f32);
            self.released_at = now;
        }
    }

    fn close(&mut self) {
        self.open = false;
        self.pressed = None;
        self.activating = None;
    }
}

/// What the player does with the main menu: the keys and the gamepad (`menu_input`), the mouse
/// (the buttons' observers, `screen.rs`), the launch.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainMenuAction {
    /// Opens it from the game: « Reprendre » first.
    Open,
    /// Opens it at launch.
    OpenAtLaunch,
    /// Closes it (back to the game).
    Close,
    /// Moves the focus up (-1) or down (+1).
    Move(i32),
    /// The pointer is over this entry: it takes the focus.
    Hover(usize),
    /// This entry is held down.
    Press(usize),
    /// Released over this entry: if it was the one held, what it does comes next.
    Release(usize),
    /// The press ends elsewhere (the pointer left): nothing happens.
    Unpress,
}

/// The main menu's systems, in `Update`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MainMenuSystems;

/// The main menu (see the module's documentation).
#[derive(Default)]
pub struct MainMenuPlugin {
    /// Opens it when the game starts (the game's, with a renderer), or the lobby with `?room=`.
    /// The tests leave it off: their game starts racing at once, as before.
    pub at_launch: bool,
}

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MainMenu>()
            .add_message::<MainMenuAction>()
            // Before the other menus' keys and the pause (`characters::pause_in_menus`, after the
            // options): the frame that opens the menu, or a screen from it, already waits.
            .configure_sets(Update, MainMenuSystems.before(OptionsSystems))
            .add_systems(Startup, screen::load_menu_assets)
            .add_systems(
                Update,
                (
                    menu_input,
                    apply_menu_actions,
                    activate,
                    screen::sync_tree.run_if(screen::tree_wanted_or_shown),
                    screen::animate_tree.run_if(|menu: Res<MainMenu>| menu.open),
                )
                    .chain()
                    .in_set(MainMenuSystems),
            )
            .add_systems(PostUpdate, follow_the_screens);
        if self.at_launch {
            app.add_systems(Update, at_launch.run_if(run_once).before(menu_input));
        }
    }
}

/// The other screens, open or not.
#[derive(SystemParam)]
struct OpenScreens<'w> {
    options: Option<Res<'w, OptionsMenu>>,
    selection: Option<Res<'w, SelectionScreen>>,
    race_menu: Option<Res<'w, RaceMenu>>,
    lobby: Option<Res<'w, LobbyWindow>>,
}

impl OpenScreens<'_> {
    fn is_open(&self, screen: Screen) -> bool {
        match screen {
            Screen::RaceMenu => self.race_menu.as_ref().is_some_and(|m| m.open),
            Screen::Lobby => self.lobby.as_ref().is_some_and(|w| w.open),
            Screen::Options => self.options.as_ref().is_some_and(|m| m.open),
            Screen::Selection => self.selection.as_ref().is_some_and(|s| s.open),
        }
    }

    fn first_open(&self) -> Option<Screen> {
        Screen::ALL.into_iter().find(|screen| self.is_open(*screen))
    }
}

/// The menu's clock: real time (s), which runs while the game is paused behind it.
fn now(time: &Time<Real>) -> f64 {
    time.elapsed_secs_f64()
}

/// The game starts on the main menu; with a room asked in the address (`?room=`, `--room`: a
/// `Join` or `Host` written at startup by `net`), on the lobby instead, with the selection screen
/// as before.
fn at_launch(
    mut net: MessageReader<NetAction>,
    mut selection: MessageWriter<SelectionAction>,
    mut actions: MessageWriter<MainMenuAction>,
) {
    let room = net
        .read()
        .any(|action| matches!(action, NetAction::Join(_) | NetAction::Host(_)));
    if room {
        selection.write(SelectionAction::Open);
    } else {
        actions.write(MainMenuAction::OpenAtLaunch);
    }
}

/// The keys and the gamepad (see the module's documentation).
#[allow(clippy::too_many_arguments)]
fn menu_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    layout: Option<Res<KeyboardLayout>>,
    menu: Res<MainMenu>,
    screens: OpenScreens,
    race: Option<Res<Race>>,
    mut latch: Local<StickLatch>,
    mut actions: MessageWriter<MainMenuAction>,
) {
    let key = |set: &[KeyCode]| {
        keys.as_ref()
            .is_some_and(|keys| keys.any_just_pressed(set.iter().copied()))
    };
    let key_up = |set: &[KeyCode]| {
        keys.as_ref()
            .is_some_and(|keys| keys.any_just_released(set.iter().copied()))
    };
    let button = |set: &[GamepadButton]| {
        gamepads
            .iter()
            .any(|gamepad| gamepad.any_just_pressed(set.iter().copied()))
    };
    let stick = gamepads
        .iter()
        .filter_map(|gamepad| gamepad.get(GamepadAxis::LeftStickY))
        .fold(0.0f32, |a, b| if b.abs() > a.abs() { b } else { a });
    let step = latch.step(stick);
    if !menu.open {
        // Échap closes the other screens first; not over a screen the menu opened.
        let free = !menu.others_open
            && screens.first_open().is_none()
            && menu.handing_over.is_none()
            && menu.child.is_none();
        // On the results screen, Start races again.
        let results = race.is_some_and(|race| race.phase() == RacePhase::Results);
        if free && (key(&[MAIN_MENU_KEY]) || (!results && button(&[MAIN_MENU_BUTTON]))) {
            actions.write(MainMenuAction::Open);
        }
        return;
    }
    if menu.activating.is_some() {
        return;
    }
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    if key(&[MAIN_MENU_KEY]) || button(&BACK_BUTTONS) {
        actions.write(MainMenuAction::Close);
        return;
    }
    if key(layout.up_keys()) || button(&[GamepadButton::DPadUp]) || step < 0 {
        actions.write(MainMenuAction::Move(-1));
    } else if key(layout.down_keys()) || button(&[GamepadButton::DPadDown]) || step > 0 {
        actions.write(MainMenuAction::Move(1));
    }
    if key(&CONFIRM_KEYS) || button(&[CONFIRM_BUTTON]) {
        actions.write(MainMenuAction::Press(menu.focus));
    }
    let released = gamepads
        .iter()
        .any(|gamepad| gamepad.just_released(CONFIRM_BUTTON));
    if key_up(&CONFIRM_KEYS) || released {
        actions.write(MainMenuAction::Release(menu.focus));
    }
}

fn apply_menu_actions(
    mut actions: MessageReader<MainMenuAction>,
    mut menu: ResMut<MainMenu>,
    online: Option<Res<Online>>,
    time: Res<Time<Real>>,
) {
    let (online, now) = (online.is_some(), now(&time));
    for action in actions.read() {
        let entries = menu.entries();
        let usable = |index: usize| entries.get(index).is_some_and(|e| enabled(*e, online));
        match *action {
            MainMenuAction::Open | MainMenuAction::OpenAtLaunch if menu.open => {}
            MainMenuAction::Open => menu.show(true, None, online, now),
            MainMenuAction::OpenAtLaunch => menu.show(false, None, online, now),
            MainMenuAction::Close => {
                if menu.open {
                    menu.close();
                }
            }
            _ if !menu.open || menu.activating.is_some() => {}
            MainMenuAction::Move(step) => {
                menu.let_go(now);
                let focus = moved(entries, menu.focus, step, online);
                menu.set_focus(focus, now);
            }
            MainMenuAction::Hover(index) => {
                if usable(index) {
                    menu.set_focus(index, now);
                }
            }
            MainMenuAction::Press(index) => {
                if usable(index) && menu.pressed.is_none() {
                    menu.set_focus(index, now);
                    menu.pressed = Some(index);
                    menu.pressed_at = now;
                }
            }
            MainMenuAction::Release(index) => {
                if menu.pressed == Some(index) {
                    menu.let_go(now);
                    menu.activating = Some((index, now));
                }
            }
            MainMenuAction::Unpress => menu.let_go(now),
        }
    }
}

/// What the entry released does, once its stretch was seen ([`ACTIVATE_DELAY`]).
#[allow(clippy::too_many_arguments)]
fn activate(
    mut menu: ResMut<MainMenu>,
    time: Res<Time<Real>>,
    online: Option<Res<Online>>,
    (options, race_menu, lobby): (
        Option<Res<OptionsMenu>>,
        Option<Res<RaceMenu>>,
        Option<ResMut<LobbyWindow>>,
    ),
    mut options_actions: MessageWriter<OptionsAction>,
    mut race_actions: MessageWriter<RaceMenuAction>,
    mut selection_actions: MessageWriter<SelectionAction>,
) {
    let Some((index, at)) = menu.activating else {
        return;
    };
    if now(&time) - at < f64::from(ACTIVATE_DELAY) {
        return;
    }
    let entry = menu.entries()[index.min(menu.entries().len() - 1)];
    menu.close();
    if !enabled(entry, online.is_some()) {
        return;
    }
    let screen = match entry {
        Entry::Resume => return,
        Entry::Solo => {
            if !race_menu.is_some_and(|m| m.open) {
                race_actions.write(RaceMenuAction::Toggle);
            }
            Screen::RaceMenu
        }
        Entry::Online => {
            if let Some(mut lobby) = lobby
                && !lobby.open
            {
                lobby.open = true;
            }
            Screen::Lobby
        }
        Entry::Options => {
            if !options.is_some_and(|m| m.open) {
                options_actions.write(OptionsAction::Toggle);
            }
            Screen::Options
        }
        Entry::Garage => {
            selection_actions.write(SelectionAction::Open);
            Screen::Selection
        }
    };
    menu.handing_over = Some((screen, 0));
}

/// After the frame's screens (`PostUpdate`): the screen the menu opened shows up and becomes its
/// child; a screen opened over the menu (its shortcut) takes its place; a child closed without
/// starting a race (nor joining a room, for the lobby) brings the menu back, the focus on its
/// entry. Also notes whether another screen is open, for Échap next frame.
fn follow_the_screens(
    mut menu: ResMut<MainMenu>,
    screens: OpenScreens,
    mut levels: MessageReader<LoadLevel>,
    session: Option<Res<NetSession>>,
    online: Option<Res<Online>>,
    time: Res<Time<Real>>,
) {
    let started = levels.read().count() > 0;
    let open = screens.first_open();
    if let Some((screen, frames)) = menu.handing_over {
        if screens.is_open(screen) {
            menu.handing_over = None;
            menu.child = Some(screen);
        } else if frames >= HAND_OVER_FRAMES {
            // It did not open: back to the menu.
            menu.handing_over = None;
            let in_game = menu.in_game;
            menu.show(in_game, Some(screen.entry()), online.is_some(), now(&time));
        } else {
            menu.handing_over = Some((screen, frames + 1));
        }
    } else if menu.open {
        if let Some(screen) = open {
            menu.close();
            menu.child = Some(screen);
        }
    } else if let Some(child) = menu.child
        && !screens.is_open(child)
    {
        if let Some(other) = open {
            menu.child = Some(other);
        } else {
            menu.child = None;
            let joined = child == Screen::Lobby && session.is_some();
            if !started && !joined {
                let in_game = menu.in_game;
                menu.show(in_game, Some(child.entry()), online.is_some(), now(&time));
            }
        }
    }
    let others = open.is_some();
    if menu.others_open != others {
        menu.others_open = others;
    }
}
