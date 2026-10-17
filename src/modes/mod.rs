//! Game modes (GD § 1, issue #19): « Jouer solo » (issue #99: the race menu of #19), the run of a
//! championship (the Grand Prix of #19), and (issue #76) the time trial.
//!
//! - **« Jouer solo »** ([`RaceMenu`], the choice in `setup.rs`): **N** (« nouvelle course »: the
//!   same letter at the same place in AZERTY and QWERTY, free in the game's controls, no Fn on a
//!   Mac), or « Jouer solo » on the home screen and in the menu opened from the game, opens and
//!   closes it, from the race or from the « Pilote » screen (which it replaces). Its three modes:
//!   the **championship** (a cup of the manifest, or a custom one: its number of races, its
//!   circuits chosen one by one or drawn at random), the **time trial** and **a circuit alone**;
//!   then the **mirror**, the **bots** (on: they fill the grid; off: the player alone) and the
//!   **items** (off: no item box on the track, `track::NoItemBoxes`). Up and down (arrows, or the
//!   player's layout's letters): the line; left and right: change it; **Entrée** starts the race
//!   (or the championship, or the time trial) with that choice; **Échap** or N closes it
//!   unchanged. The game is paused while it is open, like the other menus (not over the home
//!   screen: its background runs). Not online: there, the host chooses in the lobby.
//! - **The championship** (`race::GrandPrix`, pure): the races one after the other, in the
//!   mirror mode chosen. When a race's results close (issue #70; Entrée closes them at once
//!   in a championship), the race is scored; Entrée shows the overall standings (`screens.rs`),
//!   then starts the next race (`LoadLevel`), its grid the reverse of the standings
//!   (`RaceGrid::lineup`). After the last race, the **podium** (`podium.rs`): the three first on
//!   their steps, dancing, a still camera on them; Entrée starts the same championship again.
//! - **Time trial** (issue #76, `time_trial.rs`): alone (no bot, no item box), three piments, the
//!   clock and the record of the circuit and mirror, and the ghost of the record
//!   (`ghost.rs`, `trial.rs`). The menu starts it ([`TimeTrial`]); the rest is
//!   [`TimeTrialPlugin`], only with a renderer.
//!
//! This plugin is the logic, headless (the tests drive it with keys and actions). The screen of
//! « Jouer solo » (`menu::solo_screen`, `bevy_ui`) and the time trial (`time_trial.rs`) only
//! come with a renderer: the headless world, the bot races', keeps the archetypes and the systems
//! it was measured with. The standings, the podium's texts and the time trial's HUD are the HUD's
//! `bevy_ui` (issue #100: in Fredoka, with the accents, after the charter: `menu::kit`).

mod ghost;
mod podium;
mod screens;
mod setup;
mod time_trial;
mod trial;

#[cfg(test)]
mod scenario_tests;
#[cfg(test)]
mod time_trial_tests;

pub use ghost::{GHOST_MAX_TICKS, GhostRecorder, GhostTrack, decode_frames, encode_frames};
pub use podium::{PODIUM_AHEAD, PodiumScene, podium_place};
pub use screens::{GrandPrixScreen, StandingsLine, standings_lines};
pub use setup::{
    Catalog, Cup, Formula, GameMode, MIN_RACES, MenuRow, Pick, RaceSetup, cups, draw_circuits,
};
pub use time_trial::{
    GHOST_ALPHA, GhostKart, GhostMaterials, GhostVisual, TimeTrial, TimeTrialPlugin,
    TimeTrialState, TrialHud, TrialRun, TrialText, records_from_text,
};
pub use trial::{
    GAME_VERSION, RECORDS_FORMAT, TIME_TRIAL_STORAGE_KEY, TrialKey, TrialRecord, TrialRecords,
    Verdict,
};

use bevy::prelude::*;

#[cfg(test)]
use crate::admin::tracks;
use crate::admin::tracks::TrackEntry;
use crate::characters::{SelectionAction, SelectionScreen};
use crate::kart::KeyboardLayout;
use crate::level::{LoadLevel, Online};
use crate::options::{OptionsMenu, OptionsSystems};
use crate::race::{
    CupRace, GrandPrix, GrandPrixStage, GrandPrixTuning, RESTART_BUTTONS, RESTART_KEYS, Race,
    RaceGrid, RaceMode, RacePhase, Racer,
};
use crate::track::{GRID_SLOTS, MirrorMode, NoItemBoxes, TrackChoice};

/// Opens and closes the race menu: « N » for « nouvelle course », a letter at the same place and
/// with the same name in AZERTY and QWERTY, free in the game's controls (GD § 13), no Fn on a Mac.
pub const RACE_MENU_KEY: KeyCode = KeyCode::KeyN;

const CONFIRM_KEYS: [KeyCode; 2] = [KeyCode::Enter, KeyCode::NumpadEnter];
const CLOSE_KEYS: [KeyCode; 2] = [KeyCode::Escape, RACE_MENU_KEY];

/// The first cup of the manifest: its first `races` circuits (around, if it has fewer). Issue
/// #99: the cups are [`cups`]; the first one is this one.
pub fn cup_of(circuits: &[TrackEntry], races: usize) -> Vec<CupRace> {
    if circuits.is_empty() {
        return Vec::new();
    }
    (0..races.max(1))
        .map(|i| {
            let entry = &circuits[i % circuits.len()];
            CupRace {
                name: entry.name.clone(),
                path: entry.asset_path(),
            }
        })
        .collect()
}

/// « Jouer solo » (issue #99; the race menu of issue #19): whether it is open, the line with the
/// focus (an index of [`RaceSetup::rows`]), and the choice on it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct RaceMenu {
    pub open: bool,
    pub row: usize,
    pub setup: RaceSetup,
    /// Issue #99: the player's bots (`RaceGrid::bots`) while the races go without them (« Bots :
    /// non »), given back by « Bots : oui ».
    pub kept_bots: Option<usize>,
}

impl RaceMenu {
    /// Its lines now.
    pub fn rows(&self) -> Vec<MenuRow> {
        self.setup.rows()
    }

    /// The line with the focus.
    pub fn row(&self) -> MenuRow {
        let rows = self.rows();
        rows[self.row.min(rows.len() - 1)]
    }
}

/// What the player does with « Jouer solo », from the keyboard, the gamepad or the mouse (the
/// screen's, `menu::solo_screen`).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceMenuAction {
    /// Opens it on the current choice, or closes it if it is open (N).
    Toggle,
    Close,
    /// Moves to the line above (-1) or below (+1), around.
    Row(i32),
    /// The pointer is over this line (an index of [`RaceMenu::rows`]): it takes the focus.
    Focus(usize),
    /// Changes a line by one value, backwards (-1) or forwards (+1).
    Change(MenuRow, i32),
    /// Starts the race, the championship or the time trial of the choice.
    Confirm,
}

/// The player moves on in a Grand Prix (Entrée on its screens).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrandPrixAction {
    /// From the results to the standings, from the standings to the next race or the podium,
    /// from the podium to a new Grand Prix.
    Continue,
}

/// The modes' systems, in `Update`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModeSystems;

pub struct ModesPlugin;

impl Plugin for ModesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RaceMenu>()
            .add_message::<RaceMenuAction>()
            .add_message::<GrandPrixAction>()
            .add_systems(
                Update,
                (
                    // Before the options menu's keys, like the selection screen: a key that closes
                    // the options does not also act here.
                    menu_keys
                        .run_if(resource_exists::<ButtonInput<KeyCode>>)
                        .before(OptionsSystems),
                    apply_menu_actions,
                    (
                        restart_with_the_level.run_if(resource_added::<Race>),
                        score_the_race,
                        grand_prix_keys.run_if(resource_exists::<ButtonInput<KeyCode>>),
                        apply_grand_prix_actions,
                    )
                        .chain()
                        .run_if(resource_exists::<GrandPrix>),
                    podium::set_up_podium,
                    podium::release_the_view,
                    screens::show_grand_prix_screen,
                )
                    .chain()
                    .in_set(ModeSystems),
            );
    }
}

/// The race menu's keys (see the module's documentation). None while the options menu is open
/// over it, nor online (the room chooses).
fn menu_keys(
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<RaceMenu>,
    layout: Option<Res<KeyboardLayout>>,
    options: Option<Res<OptionsMenu>>,
    online: Option<Res<Online>>,
    mut actions: MessageWriter<RaceMenuAction>,
) {
    if options.is_some_and(|menu| menu.open) || online.is_some() {
        return;
    }
    if !menu.open {
        if keys.just_pressed(RACE_MENU_KEY) {
            actions.write(RaceMenuAction::Toggle);
        }
        return;
    }
    let layout = layout.map_or_else(KeyboardLayout::default, |layout| *layout);
    let pressed = |set: &[KeyCode]| keys.any_just_pressed(set.iter().copied());
    let row = || menu.row();
    if pressed(&CONFIRM_KEYS) {
        actions.write(RaceMenuAction::Confirm);
    } else if pressed(&CLOSE_KEYS) {
        actions.write(RaceMenuAction::Close);
    } else if pressed(layout.up_keys()) {
        actions.write(RaceMenuAction::Row(-1));
    } else if pressed(layout.down_keys()) {
        actions.write(RaceMenuAction::Row(1));
    } else if pressed(layout.left_keys()) {
        actions.write(RaceMenuAction::Change(row(), -1));
    } else if pressed(layout.right_keys()) || keys.just_pressed(KeyCode::Space) {
        actions.write(RaceMenuAction::Change(row(), 1));
    }
}

/// The index of the circuit of `choice` in the manifest (the first one off a circuit).
fn circuit_index(choice: &TrackChoice, circuits: &[TrackEntry]) -> usize {
    match choice {
        TrackChoice::Circuit(path) => circuits
            .iter()
            .position(|entry| entry.asset_path() == *path)
            .unwrap_or(0),
        TrackChoice::TestGround => 0,
    }
}

/// Applies the menu's actions. It opens on what is raced now (the mode, the circuit, the mirror,
/// the bots and the items), the rest as it was left (the cup, the custom championship).
/// « Entrée » starts the choice: the mirror and the items (issue #99: `NoItemBoxes`), the bots
/// (issue #99: off, the player's are kept in [`RaceMenu::kept_bots`]), then a race alone on its
/// circuit (a championship in progress ends), a new championship (a cup's circuits, or the
/// custom ones: chosen, or drawn from `RaceGrid::seed`), or (issue #76) a time trial on its
/// circuit: alone, the player's bots kept for later ([`TimeTrial::solo_bots`]: given back by the
/// next choice).
#[allow(clippy::too_many_arguments)]
fn apply_menu_actions(
    mut commands: Commands,
    mut actions: MessageReader<RaceMenuAction>,
    mut menu: ResMut<RaceMenu>,
    (choice, mut mirror, no_boxes): (
        Res<TrackChoice>,
        ResMut<MirrorMode>,
        Option<Res<NoItemBoxes>>,
    ),
    (mut grid, gp_tuning, grand_prix, time_trial): (
        ResMut<RaceGrid>,
        Res<GrandPrixTuning>,
        Option<Res<GrandPrix>>,
        Option<Res<TimeTrial>>,
    ),
    selection: Option<Res<SelectionScreen>>,
    mut selection_actions: MessageWriter<SelectionAction>,
    mut levels: MessageWriter<LoadLevel>,
) {
    for action in actions.read() {
        let catalog = Catalog::game(gp_tuning.races as usize);
        match *action {
            RaceMenuAction::Toggle if menu.open => menu.open = false,
            RaceMenuAction::Toggle => {
                let mut setup = menu.setup.clone();
                setup.mode = if time_trial.is_some() {
                    GameMode::TimeTrial
                } else if grand_prix.is_some() {
                    GameMode::GrandPrix
                } else {
                    GameMode::FreeRace
                };
                setup.circuit = circuit_index(&choice, &catalog.circuits);
                setup.mirror = mirror.0;
                let player_bots = time_trial
                    .as_ref()
                    .map_or(grid.bots, |trial| trial.solo_bots);
                setup.bots = player_bots > 0;
                setup.items = no_boxes.is_none();
                setup.fit(&catalog);
                menu.open = true;
                menu.row = 0;
                menu.setup = setup;
                // It takes the selection screen's place.
                if selection.as_ref().is_some_and(|screen| screen.open) {
                    selection_actions.write(SelectionAction::Cancel);
                }
            }
            RaceMenuAction::Close => menu.open = false,
            _ if !menu.open => {}
            RaceMenuAction::Row(step) => {
                let rows = menu.rows().len() as i32;
                menu.row = (menu.row as i32 + step).rem_euclid(rows) as usize;
            }
            RaceMenuAction::Focus(index) => {
                if index < menu.rows().len() && menu.row != index {
                    menu.row = index;
                }
            }
            RaceMenuAction::Change(row, step) => {
                menu.setup = menu.setup.clone().changed(row, step, &catalog);
                let rows = menu.rows();
                menu.row = rows
                    .iter()
                    .position(|r| *r == row)
                    .unwrap_or(menu.row.min(rows.len() - 1));
            }
            RaceMenuAction::Confirm => {
                menu.open = false;
                let setup = menu.setup.clone();
                mirror.set_if_neq(MirrorMode(setup.mirror));
                // Issue #99: the items on or off (a time trial clears its boxes itself).
                if setup.items {
                    commands.remove_resource::<NoItemBoxes>();
                } else {
                    commands.insert_resource(NoItemBoxes);
                }
                grid.lineup = None;
                // The player's bots: issue #76, kept while a time trial lasts; issue #99, kept
                // while the races go without.
                let mut bots = time_trial
                    .as_ref()
                    .map_or(grid.bots, |trial| trial.solo_bots);
                if time_trial.is_some() {
                    commands.remove_resource::<TimeTrial>();
                }
                if setup.mode != GameMode::TimeTrial {
                    if !setup.bots && bots > 0 {
                        menu.kept_bots = Some(bots);
                        bots = 0;
                    } else if setup.bots && bots == 0 {
                        bots = menu.kept_bots.take().unwrap_or(GRID_SLOTS - 1);
                    }
                }
                if setup.mode == GameMode::TimeTrial {
                    commands.insert_resource(TimeTrial { solo_bots: bots });
                    grid.bots = 0;
                } else {
                    grid.bots = bots;
                }
                match setup.mode {
                    GameMode::FreeRace | GameMode::TimeTrial => {
                        commands.remove_resource::<GrandPrix>();
                        let track = catalog
                            .circuits
                            .get(setup.circuit)
                            .map_or_else(TrackChoice::default, TrackEntry::choice);
                        levels.write(LoadLevel::on(track, RaceMode::Race));
                    }
                    GameMode::GrandPrix => {
                        let races = catalog.races(&setup.program(grid.seed, &catalog));
                        let Some(first) = races.first() else {
                            continue;
                        };
                        let track = TrackChoice::Circuit(first.path.clone());
                        let mut championship = GrandPrix::new(races, grid.bot_count());
                        championship.title = setup.title(&catalog);
                        commands.insert_resource(championship);
                        levels.write(LoadLevel::on(track, RaceMode::Race));
                        // The next random championship is another draw.
                        if setup.formula == Formula::Custom && setup.pick == Pick::Random {
                            menu.setup.draw = menu.setup.draw.wrapping_add(1);
                        }
                    }
                }
            }
        }
    }
}

/// A new race of the Grand Prix is built: the next one (already racing), or the same one again
/// (« Relancer » in the admin panel, a new kart on the selection screen, from its results, its
/// standings or the podium): then its score is taken back and it is raced again.
fn restart_with_the_level(mut grand_prix: ResMut<GrandPrix>, tuning: Res<GrandPrixTuning>) {
    if grand_prix.stage != GrandPrixStage::Racing || grand_prix.scored() {
        grand_prix.restart_race(&tuning.points);
    }
}

/// Scores the race of the Grand Prix once its results closed (issue #70: the racers still on
/// track with their estimated place): the order of the results, as entrants.
fn score_the_race(
    mut grand_prix: ResMut<GrandPrix>,
    tuning: Res<GrandPrixTuning>,
    race: Option<Res<Race>>,
    grid: Res<RaceGrid>,
    racers: Query<&Racer>,
) {
    let Some(race) = race else {
        return;
    };
    if grand_prix.stage != GrandPrixStage::Racing || grand_prix.scored() || !race.results_closed() {
        return;
    }
    let order: Vec<usize> = race
        .results
        .iter()
        .filter_map(|line| racers.get(line.id).ok())
        .map(|racer| grid.entrant_at(racer.grid_slot))
        .collect();
    grand_prix.score(&order, &tuning.points);
    if grand_prix.advance_when_scored {
        grand_prix.advance_when_scored = false;
        grand_prix.stage = GrandPrixStage::Standings;
    }
}

/// Entrée, Espace or the gamepad's A / Start on the screens of a Grand Prix (once the results
/// let a restart through: `RaceFlow::can_restart`). Not while a menu pauses the game.
fn grand_prix_keys(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Option<Res<Time<Virtual>>>,
    race: Option<Res<Race>>,
    menu: Res<RaceMenu>,
    mut actions: MessageWriter<GrandPrixAction>,
) {
    let Some(race) = race else {
        return;
    };
    if menu.open
        || time.is_some_and(|time| time.is_paused())
        || race.phase() != RacePhase::Results
        || !race.flow.can_restart(&race.timings)
    {
        return;
    }
    let key = keys.any_just_pressed(RESTART_KEYS);
    let button = gamepads
        .iter()
        .any(|gamepad| gamepad.any_just_pressed(RESTART_BUTTONS));
    if key || button {
        actions.write(GrandPrixAction::Continue);
    }
}

/// Moves the Grand Prix on (see [`GrandPrixAction::Continue`]).
fn apply_grand_prix_actions(
    mut actions: MessageReader<GrandPrixAction>,
    mut grand_prix: ResMut<GrandPrix>,
    race: Option<ResMut<Race>>,
    mut grid: ResMut<RaceGrid>,
    mut levels: MessageWriter<LoadLevel>,
) {
    let mut race = race;
    for action in actions.read() {
        match (*action, grand_prix.stage) {
            (GrandPrixAction::Continue, GrandPrixStage::Racing) => {
                if grand_prix.scored() {
                    grand_prix.stage = GrandPrixStage::Standings;
                } else if let Some(race) = race.as_mut() {
                    // The results close now (estimated times), then the standings come.
                    race.close_requested = true;
                    grand_prix.advance_when_scored = true;
                }
            }
            (GrandPrixAction::Continue, GrandPrixStage::Standings) => {
                let lineup = grand_prix.next_lineup();
                if grand_prix.next_race() {
                    grid.lineup = Some(lineup);
                    let track = TrackChoice::Circuit(grand_prix.cup[grand_prix.race].path.clone());
                    levels.write(LoadLevel::on(track, RaceMode::Race));
                } else {
                    grand_prix.stage = GrandPrixStage::Podium;
                }
            }
            (GrandPrixAction::Continue, GrandPrixStage::Podium) => {
                // The same championship again: the same races, the same mirror.
                let cup = grand_prix.cup.clone();
                let bots = grand_prix.entrants - 1;
                let title = std::mem::take(&mut grand_prix.title);
                *grand_prix = GrandPrix::new(cup, bots);
                grand_prix.title = title;
                grid.lineup = None;
                if let Some(first) = grand_prix.cup.first() {
                    let track = TrackChoice::Circuit(first.path.clone());
                    levels.write(LoadLevel::on(track, RaceMode::Race));
                }
            }
        }
    }
}
