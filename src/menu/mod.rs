//! The game's screens with the art direction (`docs/UI_CHARTE.md`): the charter (`style.rs`,
//! `anim.rs`, issue #56) and the screens built with it: the **home screen** (issue #89) and the
//! menu opened from the game.
//!
//! - **The home screen** (issue #89): the game's title, big, and four buttons, in this order:
//!   « Jouer solo » opens its screen (issue #99, N: `solo.rs`, `solo_screen.rs`, [`SoloPlugin`]),
//!   « Multijoueur » the lobby (L, issue #7: create or join a game), « Options » the options menu
//!   (O, issue #17), « Pilote » the screen of the pilot and its kart (G, issues #15 and #98:
//!   `pilot.rs`, `pilot_screen.rs`, `preview.rs`, [`PilotPlugin`]).
//!   **Behind it, the game runs** (`showcase.rs`): a few karts racing and striking and grabbing
//!   each other, filmed by a trailer camera (`trailer.rs`), framed on the right of the screen (the
//!   column of the home stands on the left). No pause there, nor over the screens it opens: their
//!   background goes on.
//! - **When**: at launch, with a renderer only ([`MainMenuPlugin::at_launch`]); with `?room=…` in
//!   the address, straight to the lobby without it (and the selection screen, as before). Then
//!   when a race is left (« Quitter » in the menu opened from the game), and when a room is left.
//!   **Échap** on it (or the gamepad's B or Start) races at once, on the circuit behind (« rouler
//!   tout de suite », as before the home screen).
//! - **The menu opened from the game, the pause** (issue #100): **Échap** ([`MAIN_MENU_KEY`]) or
//!   the gamepad's **Start** ([`MAIN_MENU_BUTTON`]), when no other screen is open (Échap closes
//!   those first): « Reprendre », « Recommencer » (the race again from its grid: the level
//!   rebuilt), « Options », « Quitter » (back to the home screen; online, out of the room too).
//!   The other screens keep their shortcuts (N, L, G). Online, « Recommencer » is disabled (the
//!   host starts the races), with its reason. In solo the game waits while it is open (its title:
//!   « Pause »); **online, never** (Max's decision, issue #9; its title: « Pas de pause ! »): the
//!   kart rolls on with its momentum, only its keys are the menu's (`characters::pause_in_menus`,
//!   `kart::PlayerInputMuted`).
//! - **A screen opened from a menu** (or over it, by its shortcut) comes back to it when it
//!   closes without starting anything: Échap in the options goes back; « Course ! » races.
//! - **Keys** (no Fn on a Mac, AZERTY and QWERTY alike): up and down (the arrows, or Z/S in
//!   AZERTY and W/S in QWERTY: the layout's keys), **Entrée** or **Espace** to choose, **Échap**
//!   to go back; the gamepad's cross or left stick, **A** to choose, **B** or **Start** to go
//!   back; the mouse: the pointer gives the focus, a click chooses. One focus for the three. A
//!   button squashes while it is held and stretches when it is released; what it does comes just
//!   after ([`anim::ACTIVATE_DELAY`]).
//! - **Drawn with `bevy_ui`** (`screen.rs`, the choice is argued in the charter): the tree only
//!   exists while a menu is open (spawned when it opens, despawned when it closes); no egui
//!   context for it. Closed, the menu costs a few resource reads per frame; the background is
//!   gone with its level once a race starts.
//!
//! **Only in a game with a renderer** (`crate::GamePlugin`): the headless game has none of it,
//! so that its world stays the one the bot races were measured on (a resource is an entity in
//! Bevy 0.20, and one more archetype before the grid changes every bot race: `STATUS.md`). The
//! tests add [`MainMenuPlugin`] to a headless game themselves; everything in it runs headless.

mod anim;
pub mod kit;
mod nav;
mod options_screen;
pub mod pilot;
mod pilot_screen;
pub mod preview;
mod screen;
mod showcase;
pub mod solo;
mod solo_screen;
pub mod style;
pub mod trailer;

#[cfg(test)]
mod home_tests;
#[cfg(test)]
mod options_tests;
#[cfg(test)]
mod pilot_tests;
#[cfg(test)]
mod scenario_tests;
#[cfg(test)]
mod solo_tests;

pub use anim::ACTIVATE_DELAY;
pub use nav::{
    Entry, GAME_ENTRIES, LAUNCH_ENTRIES, StickLatch, enabled, entries, game_title, moved,
};
pub use options_screen::{
    ControlLine, OptionsArrow, OptionsBack, OptionsColumn, OptionsHelp, OptionsHint, OptionsLine,
    OptionsRoot, OptionsUi, OptionsValue,
};
pub use pilot_screen::{
    ColorSwatch, LineValue, PilotArrow, PilotButton, PilotColumn, PilotHint, PilotLine, PilotRoot,
    PilotStorage, PilotUi, PortraitCell, PreviewFrame, PseudoField, PseudoHint, StatBar, StatValue,
    TerrainValue,
};
pub use preview::{
    LayerRoot, PilotPreview, PortraitScene, PortraitState, Portraits, PreviewBody, PreviewCamera,
    PreviewKart, PreviewScene,
};
pub use screen::{MainMenuRoot, MenuAssets, MenuButton, MenuNode, TitleWord};
pub use showcase::{HomeTuning, SHOWCASE_LAPS, Showcase, ShowcaseKart, ShowcasePhase, pack_scale};
pub use solo_screen::{
    RaceCard, SoloArrow, SoloButton, SoloColumn, SoloHeader, SoloHelp, SoloHint, SoloLine,
    SoloRoot, SoloShape, SoloUi, SoloValue, Vignette,
};
pub use style::{ButtonLook, ButtonState, UI_FILES, button_look};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::admin::Tunings;
use crate::characters::{SelectionAction, SelectionScreen};
use crate::kart::{KartSystems, KeyboardLayout};
use crate::level::{LevelSystems, LoadLevel, Online};
use crate::modes::{RaceMenu, RaceMenuAction};
use crate::net::{LobbyWindow, NetAction, NetSession};
use crate::options::{OptionsAction, OptionsMenu, OptionsSystems};
use crate::race::{Race, RaceMode, RacePhase};

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
    /// « Jouer solo » (issue #99, N).
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

/// The home screen, or the menu opened from the game: whether it is open, its focus, what is
/// pressed, and the screen it opened. The real times (s, `Time<Real>`: the game is paused behind
/// the menu opened from the game) drive its animations.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct MainMenu {
    pub open: bool,
    /// Opened from the game (Échap, Start): « Reprendre » is its first entry. Otherwise, the home
    /// screen (issue #89).
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

    /// The menu opened from the game holds it: open, opening a screen, or one of its screens is
    /// open (until the menu knows whether it comes back). In solo the game waits; online, the kart
    /// coasts. The home screen never holds it (issue #89): its background is the game.
    pub fn holds_the_game(&self) -> bool {
        self.in_game
            && (self.open
                || self.handing_over.is_some_and(|(screen, _)| screen.pauses())
                || self.child.is_some_and(Screen::pauses))
    }

    /// Issue #89: the home screen is there: open, opening a screen, or one of its screens is
    /// open. Its background runs behind them all.
    pub fn on_home(&self) -> bool {
        !self.in_game && (self.open || self.handing_over.is_some() || self.child.is_some())
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

/// What the player does with the menus: the keys and the gamepad (`menu_input`), the mouse (the
/// buttons' observers, `screen.rs`), the launch.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainMenuAction {
    /// Opens the menu from the game: « Reprendre » first.
    Open,
    /// Opens the home screen (at launch).
    OpenAtLaunch,
    /// Closes it: back to the game; on the home screen, a race at once (« rouler tout de
    /// suite »).
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

/// The home screen and the menu opened from the game (see the module's documentation).
#[derive(Default)]
pub struct MainMenuPlugin {
    /// Opens the home screen when the game starts (the game's, with a renderer), or the lobby
    /// with `?room=`. The tests leave it off: their game starts racing at once, as before.
    pub at_launch: bool,
}

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MainMenu>()
            .init_resource::<HomeTuning>()
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
            // Issue #89: the home screen's background (`showcase.rs`).
            .add_systems(PreUpdate, showcase::watch_the_levels.before(LevelSystems))
            .add_systems(
                Update,
                (
                    showcase::start_the_background.run_if(showcase::background_building),
                    (showcase::follow_the_tuning, showcase::hide_the_race_screen)
                        .run_if(showcase::background_running),
                )
                    .chain(),
            )
            .add_systems(
                FixedUpdate,
                showcase::keep_the_pack_together
                    .after(KartSystems::Input)
                    .before(KartSystems::Drive)
                    .run_if(showcase::background_running),
            )
            .add_systems(
                PostUpdate,
                (
                    (follow_the_screens, showcase::want_the_background).chain(),
                    showcase::film_the_background
                        .after(crate::camera::chase)
                        .before(crate::camera::tint_under_water)
                        .run_if(showcase::background_running),
                ),
            );
        // Issue #98: the « Pilote » screen (the game adds it first, keeping the pseudo in the
        // browser; the tests' menu without).
        if !app.is_plugin_added::<PilotPlugin>() {
            app.add_plugins(PilotPlugin { storage: false });
        }
        // Issue #99: the « Jouer solo » screen.
        app.add_plugins(SoloPlugin);
        // Its tuning in the admin panel, next to the others (when the panel is there: the game's).
        if let Some(mut tunings) = app.world_mut().get_resource_mut::<Tunings>() {
            tunings.register::<HomeTuning>();
        }
        if self.at_launch {
            app.add_systems(Update, at_launch.run_if(run_once).before(menu_input));
        }
    }
}

/// Issue #98: the « Pilote » screen (`pilot.rs`, `pilot_screen.rs`, `preview.rs`): the view of
/// `characters::SelectionScreen` with `bevy_ui`, its 3D preview and its portraits, the gamepad, the
/// mouse and the pseudo's field. With a renderer only, like the menu: nothing of it in the
/// headless world of the bot races.
pub struct PilotPlugin {
    /// Keeps the pseudo typed in the browser (the game), or not (the tests).
    pub storage: bool,
}

impl Plugin for PilotPlugin {
    fn build(&self, app: &mut App) {
        use bevy::camera::visibility::VisibilitySystems;
        use bevy::transform::TransformSystems;

        use crate::characters::apply_selection_actions;
        use crate::interpolation::InterpolationSystems;

        // Nothing else in the world until the screen first opens (its resources come then): the
        // home's background, which the menu's tests measure, is the same world as before it.
        if self.storage {
            app.insert_resource(pilot_screen::PilotStorage(true));
        }
        app.add_systems(
            Update,
            (
                pilot_screen::pilot_gamepad.before(apply_selection_actions),
                (
                    pilot_screen::sync_pilot_tree,
                    (
                        pilot_screen::follow_the_field,
                        (
                            pilot_screen::refresh_pilot_tree,
                            pilot_screen::animate_pilot_tree,
                            preview::follow_the_choice,
                        )
                            .run_if(pilot_screen::pilot_open),
                    )
                        .chain()
                        .run_if(resource_exists::<pilot_screen::PilotUi>),
                )
                    .chain()
                    .run_if(pilot_screen::pilot_open_or_shown)
                    .after(apply_selection_actions),
                preview::render_portraits
                    .after(pilot_screen::sync_pilot_tree)
                    .run_if(resource_exists::<preview::Portraits>),
            ),
        )
        .add_systems(
            PostUpdate,
            (
                preview::pose_the_preview
                    .after(InterpolationSystems)
                    .before(TransformSystems::Propagate)
                    .run_if(resource_exists::<preview::PilotPreview>),
                // The 3D of the screen is drawn on its own layers: every entity of its scenes
                // (the kart visual's children, spawned by the visuals' systems, too).
                preview::spread_the_layers
                    .after(TransformSystems::Propagate)
                    .before(VisibilitySystems::CheckVisibility)
                    .run_if(preview::layers_to_spread),
            ),
        );
    }
}

/// Issue #99: the « Jouer solo » screen (`solo.rs`, `solo_screen.rs`): the view of
/// `modes::RaceMenu` with `bevy_ui`, the gamepad and the mouse. With a renderer only, like the
/// menu: nothing of it in the headless world of the bot races, and nothing in the world before
/// it first opens (its resource comes then).
///
/// Issue #100: **and the options screen** (`options_screen.rs`, the view of
/// `options::OptionsMenu`): each system of the solo screen does the same step for the options
/// screen too (its `SystemParam`s: `OptionsGamepad`, `OptionsSync`, `OptionsRefresh`,
/// `OptionsAnimate`), each screen checking that it is open. **No system of its own**: a new
/// system (a new function, or a `pipe`) changes the order in which the game's systems run (each
/// system type is a set of Bevy's schedule, and the sets go through hash maps when the order is
/// computed), and the home's background with it: its pack spread out, from 15 m to 32 m
/// (`home_tests`, the same as #98's). The same systems, with more parameters, keep the order.
pub struct SoloPlugin;

impl Plugin for SoloPlugin {
    fn build(&self, app: &mut App) {
        use crate::modes::ModeSystems;

        // Issue #100: each of these systems drives both screens (see above); their conditions
        // say whether either is open.
        app.add_systems(
            Update,
            (
                solo_screen::solo_gamepad.before(ModeSystems),
                (
                    solo_screen::sync_solo_tree,
                    (
                        solo_screen::refresh_solo_tree,
                        solo_screen::animate_solo_tree,
                    )
                        .chain()
                        .run_if(solo_screen::solo_open)
                        .run_if(solo_screen::screens_ui_exist),
                )
                    .chain()
                    .run_if(solo_screen::solo_open_or_shown)
                    .after(ModeSystems),
            ),
        );
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

/// The game starts on the home screen; with a room asked in the address (`?room=`, `--room`: a
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
    mut levels: MessageWriter<LoadLevel>,
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
                    // Issue #89: from the home screen, a race at once on the circuit behind it
                    // (its background goes with that level), with the player's grid.
                    if !menu.in_game && !online {
                        levels.write(LoadLevel::restart(RaceMode::Race));
                    }
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
    (options, race_menu, lobby, session): (
        Option<Res<OptionsMenu>>,
        Option<Res<RaceMenu>>,
        Option<ResMut<LobbyWindow>>,
        Option<Res<NetSession>>,
    ),
    (mode, mut levels): (Option<Res<RaceMode>>, MessageWriter<LoadLevel>),
    mut options_actions: MessageWriter<OptionsAction>,
    mut race_actions: MessageWriter<RaceMenuAction>,
    mut selection_actions: MessageWriter<SelectionAction>,
    mut net_actions: MessageWriter<NetAction>,
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
        // Issue #100: the race again from its grid, as « Relancer » in the admin panel (in a
        // championship, its points are taken back: `modes`).
        Entry::Restart => {
            let mode = mode.map_or(RaceMode::Race, |mode| *mode);
            levels.write(LoadLevel::restart(mode));
            return;
        }
        // Issue #89: back to the home screen (its background replaces the race); in a room, out
        // of it first: the home comes once it is left (`follow_the_screens`).
        Entry::Quit => {
            if session.is_some() {
                net_actions.write(NetAction::Leave);
            }
            if online.is_none() {
                let now = now(&time);
                menu.show(false, None, false, now);
            }
            return;
        }
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
/// entry. Also notes whether another screen is open, for Échap next frame. Issue #89: a room
/// left brings the home screen; so does the home's background with nothing over it (a lobby
/// closed once its room failed).
#[allow(clippy::too_many_arguments)]
fn follow_the_screens(
    mut menu: ResMut<MainMenu>,
    screens: OpenScreens,
    mut levels: MessageReader<LoadLevel>,
    session: Option<Res<NetSession>>,
    online: Option<Res<Online>>,
    showcase: Option<Res<Showcase>>,
    time: Res<Time<Real>>,
    mut was_online: Local<bool>,
) {
    // A level of somebody else's (the home's background is not a race started).
    let started = levels
        .read_with_id()
        .filter(|(_, id)| !showcase.as_ref().is_some_and(|s| s.asked(id.id)))
        .count()
        > 0;
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
    // Issue #89: out of a room, the home screen.
    let left = *was_online && online.is_none();
    *was_online = online.is_some();
    // The home's background with nothing over it (a lobby closed while its room was joining,
    // then failed): the home comes back.
    let alone = showcase.as_ref().is_some_and(|s| s.running())
        && !started
        && session.is_none()
        && open.is_none()
        && menu.child.is_none()
        && menu.handing_over.is_none()
        && !menu.open;
    if online.is_none() && ((left && !menu.on_home()) || alone) {
        menu.show(false, None, false, now(&time));
    }
    let others = open.is_some();
    if menu.others_open != others {
        menu.others_open = others;
    }
}
