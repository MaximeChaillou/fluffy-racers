//! What the selection screen keeps between sessions, with a renderer only: the player's color
//! (issue #55), like the options (#17): read before anything is built, written when it changes.
//! The headless tests never touch the storage.
//!
//! Issue #98: the screen itself is the « Pilote » screen of `menu::pilot` (`bevy_ui`, with its 3D
//! preview): no egui window any more.

use bevy::prelude::*;

use super::{COLOR_STORAGE_KEY, PlayerColor};
use crate::admin::storage;
use crate::kart::KartColor;

pub struct CharacterUiPlugin;

impl Plugin for CharacterUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_stored_color)
            .add_systems(Update, store_color);
    }
}

/// The color kept from the previous sessions, before anything is built (the karts, the `Hello`
/// online). An unreadable text is ignored (pink stays), with a warning.
fn load_stored_color(mut color: ResMut<PlayerColor>) {
    let Some(text) = storage::load_key(COLOR_STORAGE_KEY) else {
        return;
    };
    match KartColor::from_stored(&text) {
        Ok(stored) => color.0 = stored,
        Err(error) => warn!("selection: stored color ignored: {error}"),
    }
}

/// A new color is kept at once (not the one loaded at startup: nothing changed).
fn store_color(color: Res<PlayerColor>) {
    if color.is_changed() && !color.is_added() {
        storage::save_key(COLOR_STORAGE_KEY, &color.0.to_stored());
    }
}
