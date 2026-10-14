//! Restarting the level, or switching to another track, while the game runs (issue #36: the
//! admin panel's « Relancer » and circuit choice).
//!
//! A level is the track and everything on it: the karts and their visuals, the croquettes, the
//! items (boxes and items out, issue #16), the pelicans, the HUD. Each of its root entities
//! carries [`LevelScoped`]: the track builders put it on their pieces, and it is a required
//! component of the karts, of their visuals (and the pelicans: anything following a simulated
//! body), of the scattered croquettes and their bursts, of the items out (`items::ItemObject`),
//! and of the HUD. [`LoadLevel`] despawns all of them, removes the resources built for the level
//! (`TrackLayout`, `PlayerStart`, `Course`, `Race`, `RacingLine`), and lets the usual path build
//! it again, as at startup: the track (its file stays loaded, so a restart is immediate), then the
//! player, the bots, the race and the HUD. The chase camera stays and follows the new player kart
//! (`crate::spawn_player`).

use bevy::prelude::*;

use crate::bots::RacingLine;
use crate::croquettes::{CroquetteBurst, LooseCroquette};
use crate::interpolation::InterpolatedFrom;
use crate::kart::Kart;
use crate::race::{Course, Race, RaceMode};
use crate::track::{PlayerStart, TrackChoice, TrackHandle, TrackLayout, test_ground};
use crate::ui::{CroquetteCounterAnimation, HudRoot, ResultsScreen};

/// The level is played online (issue #9, `crate::net`: inserted while we drive in a room, free
/// driving or a race). What follows from it, outside the network:
/// - **no pause**: the menus (options, selection screen, admin panel) leave the game running, our
///   kart goes on existing for the others (it coasts while a menu is open);
/// - **no item and no croquette** yet: they are not shared yet (issue #53, the next step),
///   so they are off (`ItemTuning::enabled` and `CroquetteTuning` untouched);
/// - **no local restart** of a race: the host starts the races.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Online;

/// On every root entity of the current level: [`LoadLevel`] despawns them (with their children).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct LevelScoped;

/// Asks to build the level again: on `track` (`None`: the current track again, even one handed
/// over directly as a `TrackHandle`), as a race or for free driving.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct LoadLevel {
    pub track: Option<TrackChoice>,
    pub mode: RaceMode,
}

impl LoadLevel {
    /// The current track again: everything back to the start (« Relancer »).
    pub fn restart(mode: RaceMode) -> Self {
        Self { track: None, mode }
    }

    /// Another track (or the same file again).
    pub fn on(track: TrackChoice, mode: RaceMode) -> Self {
        Self {
            track: Some(track),
            mode,
        }
    }
}

/// Order of the reload: in `PreUpdate`, so that the level is rebuilt by the usual `Update`
/// systems of the same frame (or of the next ones, while a new circuit file loads).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LevelSystems;

pub struct LevelPlugin;

impl Plugin for LevelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LoadLevel>()
            .register_required_components::<Kart, LevelScoped>()
            .register_required_components::<InterpolatedFrom, LevelScoped>()
            .register_required_components::<LooseCroquette, LevelScoped>()
            .register_required_components::<CroquetteBurst, LevelScoped>()
            .register_required_components::<HudRoot, LevelScoped>()
            .register_required_components::<ResultsScreen, LevelScoped>()
            .add_systems(
                PreUpdate,
                load_level
                    .in_set(LevelSystems)
                    .run_if(on_message::<LoadLevel>),
            );
    }
}

/// Clears the level and starts building the requested one (the last request of the frame).
fn load_level(
    mut commands: Commands,
    mut requests: MessageReader<LoadLevel>,
    scoped: Query<Entity, (With<LevelScoped>, Without<ChildOf>)>,
    mut choice: ResMut<TrackChoice>,
    mut mode: ResMut<RaceMode>,
    handle: Option<Res<TrackHandle>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(request) = requests.read().last().cloned() else {
        return;
    };
    for entity in &scoped {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<TrackLayout>();
    commands.remove_resource::<PlayerStart>();
    commands.remove_resource::<Course>();
    commands.remove_resource::<Race>();
    commands.remove_resource::<RacingLine>();
    commands.insert_resource(CroquetteCounterAnimation::default());
    mode.set_if_neq(request.mode);

    let Some(track) = request.track else {
        // Same track: a circuit is rebuilt from its `TrackHandle` (still loaded) by the track
        // plugin; the test ground is built here, also when it replaced a circuit that failed to
        // load (no `TrackHandle` left).
        if *choice == TrackChoice::TestGround || handle.is_none() {
            test_ground::build(&mut commands, &mut meshes, &mut materials);
        }
        return;
    };
    match &track {
        // Already loaded (the current `TrackHandle` keeps it): the same handle comes back and the
        // circuit is rebuilt at once. Otherwise the file loads first.
        TrackChoice::Circuit(path) => {
            commands.insert_resource(TrackHandle(assets.load(path.clone())));
        }
        TrackChoice::TestGround => {
            commands.remove_resource::<TrackHandle>();
            test_ground::build(&mut commands, &mut meshes, &mut materials);
        }
    }
    choice.set_if_neq(track);
}
