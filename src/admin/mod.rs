//! Admin panel (issue #36): Max tunes the game while playing it, then pastes his values in an
//! issue, and they become the new defaults.
//!
//! - **P** (same key in AZERTY and QWERTY, no Fn on a Mac) opens and closes it, as does the small
//!   « admin » button at the bottom of the screen; `?admin` in the page address (`--admin`
//!   natively) opens it at startup.
//! - **Réglages**: every tuning resource ([`Tunings`]: `KartTuning`, `CroquetteTuning`,
//!   `BotTuning` with each difficulty's `BotSkill`, `CameraTuning`, `RaceTuning`, `StatsTuning`
//!   of issue #15, `ItemTuning` of issue #16, `InteractionTuning` of issue #34), edited by
//!   reflection (`tuning.rs`): a field added later shows up with no extra code; a new tuning
//!   resource is one line in [`AdminPlugin`]. Modified values stand out and each one, or each
//!   resource, goes back to its default in a click.
//! - **Partager**: « Copier mes réglages » gives a RON text with only the modified values (and
//!   their defaults in comments) to paste in an issue; a pasted text loads back. The modified
//!   values are kept between sessions (`storage.rs`: the browser's local storage).
//! - **Test**: circuit (manifest `assets/tracks/index.ron`, `tracks.rs`), race or free driving,
//!   number and level of the bots, restart, skip the countdown, slow motion and pause; and the
//!   player kart's telemetry.
//!
//! This plugin is the logic, headless (the tests run it): panel state, keyboard capture, skipped
//! countdown. The egui interface (`ui.rs`) is only added with a renderer, and costs nothing
//! while the panel is closed (no egui context then).

pub mod storage;
pub mod tracks;
pub mod tuning;
mod ui;

#[cfg(test)]
mod scenario_tests;

pub use tuning::{Change, Field, ImportReport, SetError, Tuning, Tunings, Value};
pub use ui::AdminUiPlugin;

use bevy::input::InputSystems;
use bevy::prelude::*;

use crate::bots::BotTuning;
use crate::camera::CameraTuning;
use crate::croquettes::CroquetteTuning;
use crate::interactions::InteractionTuning;
use crate::items::ItemTuning;
use crate::kart::{KartTuning, StatsTuning};
use crate::race::{Race, RaceTuning, RocketStart};

/// Opens and closes the panel: a letter at the same place and with the same name in AZERTY and
/// QWERTY, free in the game's controls (GD § 13), and reachable without Fn on a Mac (unlike F1 to
/// F12).
pub const ADMIN_PANEL_KEY: KeyCode = KeyCode::KeyP;

/// Whether the panel is open.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdminPanel {
    pub open: bool,
}

/// Test options of the session (not stored).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdminOptions {
    /// Every race starts at GO, without grid and countdown.
    pub skip_countdown: bool,
}

/// The keyboard is the panel's (typing in a text field, a list open): the game sees no key. The
/// kart never reads the keyboard; its input systems read `ButtonInput<KeyCode>`, which is
/// emptied each frame while this is true (egui reads the keyboard events, not that).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardCapture(pub bool);

pub struct AdminPlugin;

impl Plugin for AdminPlugin {
    fn build(&self, app: &mut App) {
        let mut tunings = Tunings::default();
        // A new tuning resource: one more line here (it must derive `Reflect` and `Default`).
        tunings
            .register::<KartTuning>()
            .register::<CroquetteTuning>()
            .register::<BotTuning>()
            .register::<CameraTuning>()
            .register::<RaceTuning>()
            .register::<StatsTuning>()
            .register::<ItemTuning>()
            .register::<InteractionTuning>();
        app.insert_resource(tunings)
            .init_resource::<AdminPanel>()
            .init_resource::<AdminOptions>()
            .init_resource::<KeyboardCapture>()
            .add_systems(
                PreUpdate,
                release_keyboard.after(InputSystems).run_if(
                    |capture: Res<KeyboardCapture>, panel: Res<AdminPanel>| capture.0 && panel.open,
                ),
            )
            .add_systems(
                Update,
                (
                    toggle_panel.run_if(resource_exists::<ButtonInput<KeyCode>>),
                    skip_countdown
                        .run_if(|options: Res<AdminOptions>| options.skip_countdown)
                        .run_if(resource_exists::<Race>),
                ),
            );
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            app.add_plugins(AdminUiPlugin);
        }
    }
}

/// The game sees no key this frame (see [`KeyboardCapture`]).
fn release_keyboard(mut keys: ResMut<ButtonInput<KeyCode>>) {
    keys.reset_all();
}

fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut panel: ResMut<AdminPanel>) {
    if keys.just_pressed(ADMIN_PANEL_KEY) {
        panel.open = !panel.open;
    }
}

/// « Toujours sauter le compte à rebours »: a race (re)starting goes straight to GO.
fn skip_countdown(mut race: ResMut<Race>, mut starts: Query<&mut RocketStart>) {
    if race.started() || race.flow.over_at.is_some() {
        return;
    }
    let race = &mut *race;
    if race.flow.skip_countdown(&race.timings) {
        // No rocket start judged on a countdown that did not happen.
        for mut start in &mut starts {
            start.set_if_neq(RocketStart::default());
        }
    }
}

/// « GO ! »: the race in progress skips what is left of its countdown. Returns whether it did.
pub fn skip_countdown_now(world: &mut World) -> bool {
    let skipped = {
        let Some(mut race) = world.get_resource_mut::<Race>() else {
            return false;
        };
        if race.started() {
            return false;
        }
        let race = &mut *race;
        race.flow.skip_countdown(&race.timings)
    };
    if skipped {
        for mut start in world.query::<&mut RocketStart>().iter_mut(world) {
            start.set_if_neq(RocketStart::default());
        }
    }
    skipped
}
