//! Game modes (GD § 1, issue #19): the race menu, the run of a Grand Prix, and (issue #76) the
//! time trial.
//!
//! - **Race menu** ([`RaceMenu`]): **N** (« nouvelle course »: the same letter at the same place
//!   in AZERTY and QWERTY, free in the game's controls, no Fn on a Mac) opens and closes it, from
//!   the race or from the selection screen (which it replaces). Its lines: the **mode** (free race,
//!   Grand Prix or time trial), the **circuit** (a free race or a time trial; a Grand Prix races
//!   the cup, the first four circuits of `assets/tracks/index.ron`) and the **mirror**
//!   (`track::MirrorMode`). Up and down (arrows, or the player's layout's letters): the line; left
//!   and right: change it; **Entrée** starts the race (or the Grand Prix, or the time trial) with
//!   that choice; **Échap** or N closes it unchanged. The game is paused while it is open, like
//!   the other menus. Not online: there, the room chooses (no mirror).
//! - **Grand Prix** (`race::GrandPrix`, pure): the races of the cup one after the other, in the
//!   mirror mode chosen. When a race's results close (issue #70; Entrée closes them at once
//!   in a Grand Prix), the race is scored; Entrée shows the overall standings (`screens.rs`), then
//!   starts the next race (`LoadLevel`), its grid the reverse of the standings
//!   (`RaceGrid::lineup`). After the last race, the **podium** (`podium.rs`): the three first on
//!   their steps, dancing, a still camera on them; Entrée starts a new Grand Prix.
//! - **Time trial** (issue #76, `time_trial.rs`): alone (no bot, no item box), three piments, the
//!   clock and the record of the circuit and mirror, and the ghost of the record
//!   (`ghost.rs`, `trial.rs`). The menu starts it ([`TimeTrial`]); the rest is
//!   [`TimeTrialPlugin`], only with a renderer.
//!
//! This plugin is the logic, headless (the tests drive it with keys and actions). The menu's egui
//! window (`ui.rs`) and the time trial (`time_trial.rs`) only come with a renderer: the headless
//! world, the bot races', keeps the archetypes and the systems it was measured with. The
//! standings, the podium's texts and the time trial's HUD are the HUD's `bevy_ui` (ASCII only:
//! its font has no accents).

mod ghost;
mod podium;
mod screens;
mod time_trial;
mod trial;
mod ui;

#[cfg(test)]
mod scenario_tests;
#[cfg(test)]
mod time_trial_tests;

pub use ghost::{GHOST_MAX_TICKS, GhostRecorder, GhostTrack, decode_frames, encode_frames};
pub use podium::{PODIUM_AHEAD, PodiumScene, podium_place};
pub use screens::{GrandPrixScreen, StandingsLine, standings_lines};
pub use time_trial::{
    GHOST_ALPHA, GhostKart, GhostMaterials, GhostVisual, TimeTrial, TimeTrialPlugin,
    TimeTrialState, TrialHud, TrialRun, TrialText, records_from_text,
};
pub use trial::{
    GAME_VERSION, RECORDS_FORMAT, TIME_TRIAL_STORAGE_KEY, TrialKey, TrialRecord, TrialRecords,
    Verdict,
};
pub use ui::ModesUiPlugin;

use bevy::prelude::*;

use crate::admin::tracks::{self, TrackEntry};
use crate::characters::{SelectionAction, SelectionScreen};
use crate::kart::KeyboardLayout;
use crate::level::{LoadLevel, Online};
use crate::options::{OptionsMenu, OptionsSystems};
use crate::race::{
    CupRace, GrandPrix, GrandPrixStage, GrandPrixTuning, RESTART_BUTTONS, RESTART_KEYS, Race,
    RaceGrid, RaceMode, RacePhase, Racer,
};
use crate::track::{MirrorMode, TrackChoice};

/// Opens and closes the race menu: « N » for « nouvelle course », a letter at the same place and
/// with the same name in AZERTY and QWERTY, free in the game's controls (GD § 13), no Fn on a Mac.
pub const RACE_MENU_KEY: KeyCode = KeyCode::KeyN;

const CONFIRM_KEYS: [KeyCode; 2] = [KeyCode::Enter, KeyCode::NumpadEnter];
const CLOSE_KEYS: [KeyCode; 2] = [KeyCode::Escape, RACE_MENU_KEY];

/// What to race (GD § 1: « Modes »).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// One race on the circuit chosen.
    #[default]
    FreeRace,
    /// Four races on the cup's circuits, with points.
    GrandPrix,
    /// Issue #76: alone on the circuit chosen, against the clock and the record's ghost.
    TimeTrial,
}

impl GameMode {
    pub const ALL: [Self; 3] = [GameMode::FreeRace, GameMode::GrandPrix, GameMode::TimeTrial];

    pub fn label(self) -> &'static str {
        match self {
            GameMode::FreeRace => "Course libre",
            GameMode::GrandPrix => "Grand Prix",
            GameMode::TimeTrial => "Contre-la-montre",
        }
    }

    /// Whether it races the circuit chosen (a Grand Prix races the cup).
    pub fn picks_a_circuit(self) -> bool {
        self != GameMode::GrandPrix
    }
}

/// A choice of the race menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RaceSetup {
    pub mode: GameMode,
    /// The circuit of a free race or a time trial: an index of the manifest
    /// (`assets/tracks/index.ron`).
    pub circuit: usize,
    pub mirror: bool,
}

/// The lines of the race menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuRow {
    Mode,
    Circuit,
    Mirror,
}

impl MenuRow {
    pub const ALL: [Self; 3] = [MenuRow::Mode, MenuRow::Circuit, MenuRow::Mirror];

    pub fn label(self) -> &'static str {
        match self {
            MenuRow::Mode => "Mode",
            MenuRow::Circuit => "Circuit",
            MenuRow::Mirror => "Miroir",
        }
    }
}

impl RaceSetup {
    /// The choice with the line `row` changed by `step` values (around). The circuit line of a
    /// Grand Prix does not change (it races the cup); a time trial's does, like a free race's.
    pub fn changed(mut self, row: MenuRow, step: i32, circuits: usize) -> Self {
        match row {
            MenuRow::Mode => {
                let all = GameMode::ALL;
                let at = all.iter().position(|m| *m == self.mode).unwrap_or(0) as i32;
                self.mode = all[(at + step).rem_euclid(all.len() as i32) as usize];
            }
            MenuRow::Circuit if self.mode.picks_a_circuit() && circuits > 0 => {
                self.circuit = (self.circuit as i32 + step).rem_euclid(circuits as i32) as usize;
            }
            MenuRow::Circuit => {}
            MenuRow::Mirror => {
                if step % 2 != 0 {
                    self.mirror = !self.mirror;
                }
            }
        }
        self
    }

    /// The value of a line, as the menu shows it.
    pub fn value(&self, row: MenuRow, circuits: &[TrackEntry], races: usize) -> String {
        match row {
            MenuRow::Mode => self.mode.label().to_string(),
            MenuRow::Circuit => match self.mode {
                GameMode::FreeRace | GameMode::TimeTrial => circuits
                    .get(self.circuit)
                    .map_or_else(String::new, |entry| entry.name.clone()),
                GameMode::GrandPrix => cup_of(circuits, races)
                    .iter()
                    .map(|race| race.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" · "),
            },
            MenuRow::Mirror => if self.mirror { "Oui" } else { "Non" }.to_string(),
        }
    }
}

/// The cup of a Grand Prix: the first `races` circuits of the manifest (around, if it has fewer).
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

/// The race menu: whether it is open, the line being changed, and the choice on it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RaceMenu {
    pub open: bool,
    pub row: usize,
    pub setup: RaceSetup,
}

impl RaceMenu {
    pub fn row(&self) -> MenuRow {
        MenuRow::ALL[self.row.min(MenuRow::ALL.len() - 1)]
    }
}

/// What the player does with the race menu, from the keyboard or the egui window.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceMenuAction {
    /// Opens it on the current choice, or closes it if it is open (N).
    Toggle,
    Close,
    /// Moves to the line above (-1) or below (+1).
    Row(i32),
    /// Changes a line by one value, backwards (-1) or forwards (+1).
    Change(MenuRow, i32),
    /// Sets the whole choice (the egui window's clicks).
    Set(RaceSetup),
    /// Starts the race or the Grand Prix of the choice.
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
        if app.is_plugin_added::<bevy::render::RenderPlugin>() {
            app.add_plugins(ModesUiPlugin);
        }
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
    let row = menu.row();
    if pressed(&CONFIRM_KEYS) {
        actions.write(RaceMenuAction::Confirm);
    } else if pressed(&CLOSE_KEYS) {
        actions.write(RaceMenuAction::Close);
    } else if pressed(layout.up_keys()) {
        actions.write(RaceMenuAction::Row(-1));
    } else if pressed(layout.down_keys()) {
        actions.write(RaceMenuAction::Row(1));
    } else if pressed(layout.left_keys()) {
        actions.write(RaceMenuAction::Change(row, -1));
    } else if pressed(layout.right_keys()) || keys.just_pressed(KeyCode::Space) {
        actions.write(RaceMenuAction::Change(row, 1));
    }
}

/// The choice the menu opens on: what is being raced now.
fn current_setup(
    choice: &TrackChoice,
    circuits: &[TrackEntry],
    mirror: bool,
    mode: GameMode,
) -> RaceSetup {
    let circuit = match choice {
        TrackChoice::Circuit(path) => circuits
            .iter()
            .position(|entry| entry.asset_path() == *path)
            .unwrap_or(0),
        TrackChoice::TestGround => 0,
    };
    RaceSetup {
        mode,
        circuit,
        mirror,
    }
}

/// Applies the menu's actions. « Entrée » starts the choice: the mirror, then a
/// free race on its circuit (a Grand Prix in progress ends), a new Grand Prix on the cup, or
/// (issue #76) a time trial on its circuit: alone, the player's bots kept for later
/// ([`TimeTrial::solo_bots`]: given back by the next choice).
#[allow(clippy::too_many_arguments)]
fn apply_menu_actions(
    mut commands: Commands,
    mut actions: MessageReader<RaceMenuAction>,
    mut menu: ResMut<RaceMenu>,
    (choice, mut mirror): (Res<TrackChoice>, ResMut<MirrorMode>),
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
        let circuits = tracks::circuits();
        match *action {
            RaceMenuAction::Toggle if menu.open => menu.open = false,
            RaceMenuAction::Toggle => {
                *menu = RaceMenu {
                    open: true,
                    row: 0,
                    setup: current_setup(
                        &choice,
                        &circuits,
                        mirror.0,
                        if time_trial.is_some() {
                            GameMode::TimeTrial
                        } else if grand_prix.is_some() {
                            GameMode::GrandPrix
                        } else {
                            GameMode::FreeRace
                        },
                    ),
                };
                // It takes the selection screen's place.
                if selection.as_ref().is_some_and(|screen| screen.open) {
                    selection_actions.write(SelectionAction::Cancel);
                }
            }
            RaceMenuAction::Close => menu.open = false,
            _ if !menu.open => {}
            RaceMenuAction::Row(step) => {
                let rows = MenuRow::ALL.len() as i32;
                menu.row = (menu.row as i32 + step).rem_euclid(rows) as usize;
            }
            RaceMenuAction::Change(row, step) => {
                menu.setup = menu.setup.changed(row, step, circuits.len());
                if let Some(index) = MenuRow::ALL.iter().position(|r| *r == row) {
                    menu.row = index;
                }
            }
            RaceMenuAction::Set(setup) => menu.setup = setup,
            RaceMenuAction::Confirm => {
                menu.open = false;
                let setup = menu.setup;
                mirror.set_if_neq(MirrorMode(setup.mirror));
                grid.lineup = None;
                // Issue #76: the player's bots, kept while a time trial lasts.
                if setup.mode == GameMode::TimeTrial {
                    let solo_bots = time_trial
                        .as_ref()
                        .map_or(grid.bots, |trial| trial.solo_bots);
                    commands.insert_resource(TimeTrial { solo_bots });
                    grid.bots = 0;
                } else if let Some(trial) = &time_trial {
                    commands.remove_resource::<TimeTrial>();
                    grid.bots = trial.solo_bots;
                }
                match setup.mode {
                    GameMode::FreeRace | GameMode::TimeTrial => {
                        commands.remove_resource::<GrandPrix>();
                        let track = circuits
                            .get(setup.circuit)
                            .map_or_else(TrackChoice::default, TrackEntry::choice);
                        levels.write(LoadLevel::on(track, RaceMode::Race));
                    }
                    GameMode::GrandPrix => {
                        let cup = cup_of(&circuits, gp_tuning.races as usize);
                        let Some(first) = cup.first() else {
                            continue;
                        };
                        let track = TrackChoice::Circuit(first.path.clone());
                        commands.insert_resource(GrandPrix::new(cup, grid.bot_count()));
                        levels.write(LoadLevel::on(track, RaceMode::Race));
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
                // A new Grand Prix on the same cup, the same mirror.
                let cup = grand_prix.cup.clone();
                let bots = grand_prix.entrants - 1;
                *grand_prix = GrandPrix::new(cup, bots);
                grid.lineup = None;
                if let Some(first) = grand_prix.cup.first() {
                    let track = TrackChoice::Circuit(first.path.clone());
                    levels.write(LoadLevel::on(track, RaceMode::Race));
                }
            }
        }
    }
}
