//! The options kept between sessions (issue #17, see `super`). Only with a renderer: the headless
//! tests drive the menu with its keys and its [`OptionsAction`](super::OptionsAction)s, and never
//! touch the storage. Issue #100: the screen is `menu::options_screen` (`bevy_ui`, after the
//! charter), which replaces the egui window of #17.

use bevy::prelude::*;

use super::{OPTIONS_STORAGE_KEY, OptionsSystems, PlayerOptions};
use crate::admin::storage;

pub struct OptionsUiPlugin;

impl Plugin for OptionsUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_stored_options)
            .add_systems(Update, store_options.after(OptionsSystems));
    }
}

/// The options kept from the previous sessions, before anything is built. An unreadable text
/// is ignored (the defaults stay), with a warning.
fn load_stored_options(mut options: ResMut<PlayerOptions>) {
    let Some(text) = storage::load_key(OPTIONS_STORAGE_KEY) else {
        return;
    };
    match PlayerOptions::from_ron(&text) {
        Ok(stored) => *options = stored,
        Err(error) => warn!("options: stored options ignored: {error}"),
    }
}

/// Every change is kept at once (not the options loaded at startup: nothing changed).
fn store_options(options: Res<PlayerOptions>) {
    if options.is_changed() && !options.is_added() {
        storage::save_key(OPTIONS_STORAGE_KEY, &options.to_ron());
    }
}
