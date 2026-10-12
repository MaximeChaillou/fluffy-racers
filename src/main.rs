use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Fluffy Racers".into(),
                // On the web: fill the browser window and keep keys (arrows, space) for the game.
                fit_canvas_to_parent: true,
                prevent_default_event_handling: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(fluffy_racers::GamePlugin)
        .run();
}
