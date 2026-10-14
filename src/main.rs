use bevy::asset::AssetMetaCheck;
use bevy::log::{DEFAULT_FILTER, LogPlugin};
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Fluffy Racers".into(),
                        // On the web: fill the browser window and keep keys (arrows, space).
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                // Our assets have no `.meta` files: don't request them (404s on the web, and
                // `trunk serve` answers them with index.html, which breaks loading).
                .set(AssetPlugin {
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                // The steps of each WebRTC connection in the console (issue #49: « created
                // offer », « Ice gathering completed »...). Only in a room: solo has no socket.
                .set(LogPlugin {
                    filter: format!("{DEFAULT_FILTER},matchbox_socket=debug"),
                    ..default()
                }),
        )
        .add_plugins(fluffy_racers::GamePlugin)
        .run();
}
