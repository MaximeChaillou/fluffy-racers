//! « Jouer solo » (issue #99, GD § 1): the choice of what to race, pure (no ECS, unit-tested).
//! `super` keeps it in `RaceMenu` and applies it; `menu::solo_screen` shows it.
//!
//! - **Three modes** ([`GameMode`]), in this order: the **championship** (« Championnat », the
//!   Grand Prix of issue #19), the **time trial** (« Contre-la-montre », issue #76) and **a
//!   circuit alone** (« Circuit seul », the free race of before).
//! - **A championship** is **a cup** ([`Formula::Cup`]: the cups of the manifest, four circuits
//!   each in its order, [`cups`]; one without its four circuits yet is not offered) or **a custom
//!   one** ([`Formula::Custom`]): a number of races (2 to the number of circuits), then its
//!   circuits **chosen one by one** ([`Pick::Chosen`], raced in that order) or **drawn at random**
//!   ([`Pick::Random`], [`draw_circuits`]: always the same draw for the same seed, no circuit twice
//!   while there are circuits left).
//! - **The settings**: the mirror; **bots** on or off (on: they fill the grid to 12, as before;
//!   off: the player alone); **items** on or off (off: no item box on the track, the croquettes
//!   stay). The time trial has only its circuit and the mirror: it is alone, with its three
//!   piments. No class (issue #118: a single speed).
//! - **The lines** ([`RaceSetup::rows`]): the mode first, then what the mode needs, the mirror, the
//!   bots and the items; a custom championship with its circuits chosen has one more line per race
//!   (« Course 1 »...), last. Each one goes around ([`RaceSetup::changed`]).

use std::borrow::Cow;

use crate::admin::tracks::{CupIcon, Manifest, TrackEntry};
use crate::bots::BotRng;
use crate::race::CupRace;

/// What to race (GD § 1: « Modes »).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameMode {
    /// The championship (« Championnat », the Grand Prix of issue #19): a cup, or a custom one;
    /// points at each race, then the podium.
    GrandPrix,
    /// Issue #76: alone on the circuit chosen, against the clock and the record's ghost.
    TimeTrial,
    /// One race on the circuit chosen (« Circuit seul »).
    #[default]
    FreeRace,
}

impl GameMode {
    /// In the order of the screen (Max: « championnat, contre la montre, circuit seul »).
    pub const ALL: [Self; 3] = [GameMode::GrandPrix, GameMode::TimeTrial, GameMode::FreeRace];

    pub fn label(self) -> &'static str {
        match self {
            GameMode::GrandPrix => "Championnat",
            GameMode::TimeTrial => "Contre-la-montre",
            GameMode::FreeRace => "Circuit seul",
        }
    }

    /// Whether it races the circuit chosen (a championship races its own circuits).
    pub fn picks_a_circuit(self) -> bool {
        self != GameMode::GrandPrix
    }
}

/// What a championship races.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Formula {
    /// A cup of the manifest.
    #[default]
    Cup,
    /// A number of races, its circuits chosen or drawn.
    Custom,
}

impl Formula {
    pub fn label(self) -> &'static str {
        match self {
            Formula::Cup => "Une coupe",
            Formula::Custom => "Personnalisé",
        }
    }
}

/// How the circuits of a custom championship are chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pick {
    /// Drawn at random ([`draw_circuits`]).
    #[default]
    Random,
    /// One by one, raced in that order.
    Chosen,
}

impl Pick {
    pub fn label(self) -> &'static str {
        match self {
            Pick::Random => "Au hasard",
            Pick::Chosen => "Au choix",
        }
    }
}

/// A line of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuRow {
    Mode,
    /// A championship: a cup or a custom one.
    Formula,
    /// The cup.
    Cup,
    /// A custom championship: its number of races...
    Races,
    /// ...drawn or chosen...
    Pick,
    /// ...and, chosen, the circuit of each race (0 = the first).
    Race(usize),
    /// The circuit of a time trial or of a race alone.
    Circuit,
    Mirror,
    Bots,
    Items,
}

impl MenuRow {
    pub fn label(self) -> Cow<'static, str> {
        Cow::Borrowed(match self {
            MenuRow::Mode => "Mode",
            MenuRow::Formula => "Formule",
            MenuRow::Cup => "Coupe",
            MenuRow::Races => "Courses",
            MenuRow::Pick => "Circuits",
            MenuRow::Race(race) => return Cow::Owned(format!("Course {}", race + 1)),
            MenuRow::Circuit => "Circuit",
            MenuRow::Mirror => "Miroir",
            MenuRow::Bots => "Bots",
            MenuRow::Items => "Objets",
        })
    }

    /// Whether the line is one of the races of a custom championship (the screen shows them apart,
    /// with the program).
    pub fn is_race(self) -> bool {
        matches!(self, MenuRow::Race(_))
    }
}

/// A cup the championship offers: its name, its icon and its circuits (indices of the manifest).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cup {
    pub name: String,
    pub icon: CupIcon,
    pub circuits: Vec<usize>,
}

/// The cups of `manifest`, each with its `size` circuits (`GrandPrixTuning::races`: 4), in the
/// order of the manifest: cup `k` races circuits `k × size` to `k × size + size - 1`. A cup that
/// does not have them all yet is not offered.
pub fn cups(manifest: &Manifest, size: usize) -> Vec<Cup> {
    let size = size.max(1);
    manifest
        .cups
        .iter()
        .enumerate()
        .filter_map(|(k, cup)| {
            let first = k * size;
            (first + size <= manifest.circuits.len()).then(|| Cup {
                name: cup.name.clone(),
                icon: cup.icon,
                circuits: (first..first + size).collect(),
            })
        })
        .collect()
}

/// What the choice is made among: the circuits of the manifest and the cups it offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub circuits: Vec<TrackEntry>,
    pub cups: Vec<Cup>,
}

impl Catalog {
    /// The catalog of `manifest`, cups of `cup_size` circuits.
    pub fn of(manifest: &Manifest, cup_size: usize) -> Self {
        Self {
            circuits: manifest.circuits.clone(),
            cups: cups(manifest, cup_size),
        }
    }

    /// The game's (the built-in manifest).
    pub fn game(cup_size: usize) -> Self {
        Self::of(&crate::admin::tracks::manifest(), cup_size)
    }

    /// The name of circuit `circuit`.
    pub fn name(&self, circuit: usize) -> &str {
        self.circuits
            .get(circuit)
            .map_or("", |entry| entry.name.as_str())
    }

    /// The races of `program` (circuit indices), as a championship runs them.
    pub fn races(&self, program: &[usize]) -> Vec<CupRace> {
        program
            .iter()
            .filter_map(|circuit| self.circuits.get(*circuit))
            .map(|entry| CupRace {
                name: entry.name.clone(),
                path: entry.asset_path(),
            })
            .collect()
    }
}

/// Mixed into the seed of a draw (its own generator).
const DRAW_SEED: u64 = 0xC1_2C_17_5D_2A_77;
/// Spreads the draw's number over all 64 bits.
const DRAW_SPREAD: u64 = 0x9E37_79B9_7F4A_7C15;

/// Issue #99: `races` circuits drawn among `circuits` (indices), at random, but always the same
/// for the same `seed` (`RaceGrid::seed`) and the same `draw` (the screen counts its draws: a new
/// one each time the player asks for another): **no circuit twice while there are circuits
/// left**. Beyond (more races than circuits), another round of every circuit, never the last one
/// again right away.
pub fn draw_circuits(seed: u64, draw: u32, races: usize, circuits: usize) -> Vec<usize> {
    let mut drawn = Vec::with_capacity(races);
    if circuits == 0 {
        return drawn;
    }
    let mut rng = BotRng::new(seed ^ DRAW_SEED ^ u64::from(draw).wrapping_mul(DRAW_SPREAD));
    let mut round: Vec<usize> = Vec::with_capacity(circuits);
    while drawn.len() < races {
        round.clear();
        round.extend(0..circuits);
        // Fisher-Yates.
        for i in (1..circuits).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            round.swap(i, j);
        }
        if circuits > 1 && drawn.last() == round.first() {
            round.swap(0, 1);
        }
        let left = races - drawn.len();
        drawn.extend(round.iter().take(left));
    }
    drawn
}

/// The choice of the screen (see the module's documentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceSetup {
    pub mode: GameMode,
    /// The circuit of a time trial or of a race alone: an index of the manifest
    /// (`assets/tracks/index.ron`).
    pub circuit: usize,
    pub mirror: bool,
    /// Bots fill the grid (not in a time trial: it is alone).
    pub bots: bool,
    /// Item boxes on the track (not in a time trial: three piments only).
    pub items: bool,
    pub formula: Formula,
    /// The cup: an index of [`Catalog::cups`].
    pub cup: usize,
    /// A custom championship's races...
    pub races: usize,
    pub pick: Pick,
    /// ...their circuits when chosen (as many as `races`)...
    pub chosen: Vec<usize>,
    /// ...and the number of the draw when drawn ([`draw_circuits`]).
    pub draw: u32,
}

impl Default for RaceSetup {
    fn default() -> Self {
        Self {
            mode: GameMode::default(),
            circuit: 0,
            mirror: false,
            bots: true,
            items: true,
            formula: Formula::default(),
            cup: 0,
            // GD § 1: a championship has four races.
            races: 4,
            pick: Pick::default(),
            chosen: Vec::new(),
            draw: 0,
        }
    }
}

/// The fewest races of a custom championship.
pub const MIN_RACES: usize = 2;

impl RaceSetup {
    /// The lines of the screen for this choice (see the module's documentation).
    pub fn rows(&self) -> Vec<MenuRow> {
        let mut rows = vec![MenuRow::Mode];
        match self.mode {
            GameMode::GrandPrix => {
                rows.push(MenuRow::Formula);
                match self.formula {
                    Formula::Cup => rows.push(MenuRow::Cup),
                    Formula::Custom => rows.extend([MenuRow::Races, MenuRow::Pick]),
                }
            }
            GameMode::TimeTrial | GameMode::FreeRace => rows.push(MenuRow::Circuit),
        }
        rows.push(MenuRow::Mirror);
        if self.mode != GameMode::TimeTrial {
            rows.extend([MenuRow::Bots, MenuRow::Items]);
        }
        if self.mode == GameMode::GrandPrix
            && self.formula == Formula::Custom
            && self.pick == Pick::Chosen
        {
            rows.extend((0..self.races).map(MenuRow::Race));
        }
        rows
    }

    /// Keeps the choice within `catalog`: a circuit, a cup and races that exist (a custom
    /// championship of 2 races at least, as many circuits chosen as races: the next ones not
    /// chosen yet), a cup only if there is one.
    pub fn fit(&mut self, catalog: &Catalog) {
        let circuits = catalog.circuits.len();
        if circuits == 0 {
            return;
        }
        self.circuit = self.circuit.min(circuits - 1);
        if catalog.cups.is_empty() {
            self.formula = Formula::Custom;
        }
        self.cup = self.cup.min(catalog.cups.len().saturating_sub(1));
        self.races = self.races.clamp(MIN_RACES.min(circuits), circuits);
        for chosen in &mut self.chosen {
            *chosen = (*chosen).min(circuits - 1);
        }
        self.chosen.truncate(self.races);
        while self.chosen.len() < self.races {
            let next = (0..circuits)
                .find(|c| !self.chosen.contains(c))
                .unwrap_or(self.chosen.len() % circuits);
            self.chosen.push(next);
        }
    }

    /// The choice with the line `row` changed by `step` values, around (see the module's
    /// documentation). A new number of races, or the draw asked again, draws again.
    pub fn changed(mut self, row: MenuRow, step: i32, catalog: &Catalog) -> Self {
        let around = |value: usize, count: usize| -> usize {
            if count == 0 {
                return 0;
            }
            (value as i64 + i64::from(step)).rem_euclid(count as i64) as usize
        };
        let flip = step % 2 != 0;
        let circuits = catalog.circuits.len();
        match row {
            MenuRow::Mode => {
                let all = GameMode::ALL;
                let at = all.iter().position(|m| *m == self.mode).unwrap_or(0);
                self.mode = all[around(at, all.len())];
            }
            MenuRow::Formula if flip && !catalog.cups.is_empty() => {
                self.formula = match self.formula {
                    Formula::Cup => Formula::Custom,
                    Formula::Custom => Formula::Cup,
                };
            }
            MenuRow::Formula => {}
            MenuRow::Cup => self.cup = around(self.cup, catalog.cups.len()),
            MenuRow::Races if circuits > MIN_RACES => {
                let low = MIN_RACES;
                self.races = low + around(self.races.max(low) - low, circuits + 1 - low);
                self.draw = self.draw.wrapping_add(1);
            }
            MenuRow::Races => {}
            MenuRow::Pick if flip => {
                self.pick = match self.pick {
                    Pick::Random => Pick::Chosen,
                    Pick::Chosen => {
                        self.draw = self.draw.wrapping_add(1);
                        Pick::Random
                    }
                };
            }
            MenuRow::Pick => {}
            MenuRow::Race(race) => {
                if let Some(chosen) = self.chosen.get_mut(race) {
                    *chosen = around(*chosen, circuits);
                }
            }
            MenuRow::Circuit => self.circuit = around(self.circuit, circuits),
            MenuRow::Mirror if flip => self.mirror = !self.mirror,
            MenuRow::Bots if flip => self.bots = !self.bots,
            MenuRow::Items if flip => self.items = !self.items,
            MenuRow::Mirror | MenuRow::Bots | MenuRow::Items => {}
        }
        self.fit(catalog);
        self
    }

    /// The value of a line, as the screen writes it.
    pub fn value(&self, row: MenuRow, catalog: &Catalog) -> String {
        let yes_no = |on: bool| if on { "Oui" } else { "Non" }.to_string();
        match row {
            MenuRow::Mode => self.mode.label().to_string(),
            MenuRow::Formula => self.formula.label().to_string(),
            MenuRow::Cup => catalog
                .cups
                .get(self.cup)
                .map_or_else(String::new, |cup| cup.name.clone()),
            MenuRow::Races => self.races.to_string(),
            MenuRow::Pick => self.pick.label().to_string(),
            MenuRow::Race(race) => self
                .chosen
                .get(race)
                .map_or_else(String::new, |c| catalog.name(*c).to_string()),
            MenuRow::Circuit => catalog.name(self.circuit).to_string(),
            MenuRow::Mirror => yes_no(self.mirror),
            MenuRow::Bots => yes_no(self.bots),
            MenuRow::Items => yes_no(self.items),
        }
    }

    /// What a line is for, under the lines when it has the focus (`docs/UI_CHARTE.md`: a small
    /// line, 48 characters at most).
    pub fn help(&self, row: MenuRow, catalog: &Catalog) -> String {
        match row {
            MenuRow::Mode => match self.mode {
                GameMode::GrandPrix => "Des courses, des points, puis le podium.",
                GameMode::TimeTrial => "Seul contre le chrono, avec trois piments.",
                GameMode::FreeRace => "Une course, sur le circuit de ton choix.",
            }
            .to_string(),
            MenuRow::Formula => match self.formula {
                Formula::Cup => "Les quatre circuits d'une coupe, dans l'ordre.",
                Formula::Custom => "Tes courses, sur les circuits de ton choix.",
            }
            .to_string(),
            MenuRow::Cup => "Chaque coupe a ses quatre circuits.".to_string(),
            MenuRow::Races => format!(
                "De {} à {} courses.",
                MIN_RACES.min(catalog.circuits.len()),
                catalog.circuits.len()
            ),
            MenuRow::Pick => match self.pick {
                Pick::Random => "Tirés au sort, chacun une fois.",
                Pick::Chosen => "Choisis chaque course, dans l'ordre.",
            }
            .to_string(),
            MenuRow::Race(race) => format!("Le circuit de la course {}.", race + 1),
            MenuRow::Circuit => "Le circuit de la course.".to_string(),
            MenuRow::Mirror => "Le circuit inversé gauche-droite.".to_string(),
            MenuRow::Bots => if self.bots {
                "Des bots complètent la grille à 12."
            } else {
                "Toi seul sur la piste."
            }
            .to_string(),
            MenuRow::Items => if self.items {
                "Des boîtes à objets sur la piste."
            } else {
                "Aucune boîte ; les croquettes restent."
            }
            .to_string(),
        }
    }

    /// The circuits raced, in order (indices of the manifest): the cup's, the custom ones (chosen,
    /// or drawn from `seed`: `RaceGrid::seed`), or the circuit of a race alone or a time trial.
    pub fn program(&self, seed: u64, catalog: &Catalog) -> Vec<usize> {
        match (self.mode, self.formula, self.pick) {
            (GameMode::GrandPrix, Formula::Cup, _) => catalog
                .cups
                .get(self.cup)
                .map_or_else(Vec::new, |cup| cup.circuits.clone()),
            (GameMode::GrandPrix, Formula::Custom, Pick::Chosen) => self.chosen.clone(),
            (GameMode::GrandPrix, Formula::Custom, Pick::Random) => {
                draw_circuits(seed, self.draw, self.races, catalog.circuits.len())
            }
            (GameMode::TimeTrial | GameMode::FreeRace, ..) => vec![self.circuit],
        }
    }

    /// The name of the championship (the standings' title): the cup's, or « Championnat ».
    pub fn title(&self, catalog: &Catalog) -> String {
        match (self.formula, catalog.cups.get(self.cup)) {
            (Formula::Cup, Some(cup)) => cup.name.clone(),
            _ => GameMode::GrandPrix.label().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::tracks::{CupEntry, manifest};

    fn entry(k: usize) -> TrackEntry {
        TrackEntry {
            name: format!("Circuit {k}"),
            file: format!("c{k}.track.ron"),
        }
    }

    /// A manifest of `circuits` circuits and `cups` cups.
    fn manifest_of(circuits: usize, cups: usize) -> Manifest {
        Manifest {
            circuits: (0..circuits).map(entry).collect(),
            cups: (0..cups)
                .map(|k| CupEntry {
                    name: format!("Coupe {k}"),
                    icon: CupIcon::ALL[k % CupIcon::ALL.len()],
                })
                .collect(),
        }
    }

    /// Issue #99: the cups of the manifest have four circuits each, in its order; one without
    /// its four circuits is not offered. The game's: its first cup is the first four circuits
    /// (the Grand Prix of #19), each cup has its own name and icon, and the names are original.
    #[test]
    fn the_cups_take_the_circuits_four_by_four() {
        let six = cups(&manifest_of(6, 2), 4);
        assert_eq!(six.len(), 1, "the second one has 2 circuits out of 4");
        assert_eq!(six[0].circuits, [0, 1, 2, 3]);
        assert_eq!(six[0].name, "Coupe 0");
        let eight = cups(&manifest_of(8, 3), 4);
        assert_eq!(eight.len(), 2);
        assert_eq!(eight[1].circuits, [4, 5, 6, 7]);
        assert!(cups(&manifest_of(3, 1), 4).is_empty());
        // Cups of another size (the admin panel's `GrandPrixTuning::races`).
        assert_eq!(cups(&manifest_of(6, 2), 3)[1].circuits, [3, 4, 5]);

        let game = manifest();
        let offered = cups(&game, 4);
        assert!(!offered.is_empty());
        assert_eq!(offered[0].circuits, [0, 1, 2, 3]);
        assert_eq!(
            game.cups.len(),
            game.circuits.len().div_ceil(4),
            "a name for each cup"
        );
        for (i, a) in game.cups.iter().enumerate() {
            assert!(a.name.starts_with("Coupe ") && a.name.chars().count() <= 16);
            for b in &game.cups[i + 1..] {
                assert_ne!(a.name, b.name);
                assert_ne!(a.icon, b.icon);
            }
        }
        // Not the cups of the karting games of reference (CLAUDE.md: original names only).
        for taken in [
            "Champignon",
            "Fleur",
            "Étoile",
            "Spéciale",
            "Carapace",
            "Banane",
            "Feuille",
            "Éclair",
            "Œuf",
            "Triforce",
            "Clochette",
        ] {
            assert!(game.cups.iter().all(|cup| !cup.name.contains(taken)));
        }
    }

    /// Issue #99 acceptance: a random custom championship draws the same circuits with the same
    /// seed (and the same draw), without any circuit twice; another seed or another draw gives
    /// another order; more races than circuits: each round has every circuit once, never the
    /// same twice in a row.
    #[test]
    fn a_random_championship_draws_the_same_circuits_with_the_same_seed_without_doubles() {
        for circuits in [2, 4, 6, 24] {
            for races in MIN_RACES..=circuits {
                for seed in [0, 1, 42, u64::MAX] {
                    for draw in [0, 1, 7] {
                        let a = draw_circuits(seed, draw, races, circuits);
                        assert_eq!(a, draw_circuits(seed, draw, races, circuits));
                        assert_eq!(a.len(), races);
                        let mut sorted = a.clone();
                        sorted.sort_unstable();
                        sorted.dedup();
                        assert_eq!(sorted.len(), races, "no double: {a:?}");
                        assert!(a.iter().all(|c| *c < circuits));
                    }
                }
            }
        }
        // The draws differ from one seed, or one draw, to the other.
        let six: Vec<Vec<usize>> = (0..20).map(|seed| draw_circuits(seed, 0, 6, 6)).collect();
        let mut distinct = six.clone();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() >= 15, "{distinct:?}");
        assert_ne!(draw_circuits(0, 0, 6, 6), draw_circuits(0, 1, 6, 6));
        // Every circuit comes first now and then.
        let firsts: std::collections::BTreeSet<usize> = (0..60)
            .map(|seed| draw_circuits(seed, 0, 4, 6)[0])
            .collect();
        assert_eq!(firsts.len(), 6);
        // More races than circuits (not offered, but safe).
        let long = draw_circuits(3, 0, 10, 4);
        assert_eq!(long.len(), 10);
        for round in long.chunks(4) {
            let mut sorted = round.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), round.len(), "{long:?}");
        }
        assert!(long.windows(2).all(|pair| pair[0] != pair[1]), "{long:?}");
        assert_eq!(draw_circuits(1, 0, 3, 1), [0, 0, 0]);
        assert!(draw_circuits(1, 0, 3, 0).is_empty());
    }

    /// The lines of each mode: the championship's (a cup, or a custom one: its races, how they
    /// are chosen, and when chosen, a line per race, last), the time trial's (the circuit and the
    /// mirror: no bots, no items line), a race alone's; no class line (issue #118).
    #[test]
    fn each_mode_has_its_lines() {
        use MenuRow::*;
        let catalog = Catalog::of(&manifest_of(6, 2), 4);
        let mut setup = RaceSetup::default();
        setup.fit(&catalog);
        assert_eq!(setup.mode, GameMode::FreeRace);
        assert_eq!(setup.rows(), [Mode, Circuit, Mirror, Bots, Items]);
        let trial = setup.clone().changed(Mode, -1, &catalog);
        assert_eq!(trial.mode, GameMode::TimeTrial);
        assert_eq!(trial.rows(), [Mode, Circuit, Mirror]);
        let cup = trial.clone().changed(Mode, -1, &catalog);
        assert_eq!(cup.mode, GameMode::GrandPrix);
        assert_eq!(cup.rows(), [Mode, Formula, Cup, Mirror, Bots, Items]);
        let custom = cup.clone().changed(Formula, 1, &catalog);
        assert_eq!(
            custom.rows(),
            [Mode, Formula, Races, Pick, Mirror, Bots, Items]
        );
        let chosen = custom.clone().changed(Pick, 1, &catalog);
        assert_eq!(
            chosen.rows(),
            [
                Mode,
                Formula,
                Races,
                Pick,
                Mirror,
                Bots,
                Items,
                Race(0),
                Race(1),
                Race(2),
                Race(3)
            ]
        );
        for row in chosen.rows() {
            assert!(!row.label().is_empty());
            assert!(!chosen.value(row, &catalog).is_empty(), "{row:?}");
            let help = chosen.help(row, &catalog);
            assert!(!help.is_empty() && help.chars().count() <= 48, "{help}");
        }
        assert_eq!(Race(2).label(), "Course 3");
        for setup in [&setup, &trial, &cup, &custom, &chosen] {
            for row in setup.rows() {
                for help in [setup.help(row, &catalog), {
                    let flipped = setup.clone().changed(row, 1, &catalog);
                    flipped.help(row, &catalog)
                }] {
                    assert!(help.chars().count() <= 48, "{help}");
                }
            }
        }
        assert_eq!(
            GameMode::ALL.map(GameMode::label),
            ["Championnat", "Contre-la-montre", "Circuit seul"]
        );
    }

    /// Each line goes around, both ways: the mode, the cup (only those offered), the number of
    /// races (2 to the number of circuits), the circuits (chosen ones too), the switches; a new
    /// number of races or « Au hasard » again is a new draw; the chosen circuits start with the
    /// circuits not chosen yet and follow the number of races.
    #[test]
    fn the_lines_go_around() {
        use MenuRow::*;
        let catalog = Catalog::of(&manifest_of(6, 2), 4);
        let n = catalog.circuits.len();
        let mut setup = RaceSetup::default();
        setup.fit(&catalog);
        let mode = |s: &RaceSetup, step| s.clone().changed(Mode, step, &catalog).mode;
        assert_eq!(mode(&setup, 1), GameMode::GrandPrix, "around");
        assert_eq!(mode(&setup, -1), GameMode::TimeTrial);
        assert_eq!(mode(&setup, 3), GameMode::FreeRace);
        assert_eq!(setup.clone().changed(Circuit, -1, &catalog).circuit, n - 1);
        assert_eq!(setup.clone().changed(Circuit, 7, &catalog).circuit, 1);
        for row in [Mirror, Bots, Items] {
            let once = setup.clone().changed(row, 1, &catalog);
            assert_ne!(once, setup, "{row:?}");
            assert_eq!(once.changed(row, -1, &catalog), setup, "{row:?}");
        }
        assert!(!setup.clone().changed(Bots, 1, &catalog).bots);
        assert!(!setup.clone().changed(Items, 1, &catalog).items);
        assert!(setup.clone().changed(Mirror, 1, &catalog).mirror);

        let mut gp = setup.clone().changed(Mode, 1, &catalog);
        // One cup offered: it stays.
        assert_eq!(gp.clone().changed(Cup, 1, &catalog).cup, 0);
        assert_eq!(gp.value(Cup, &catalog), "Coupe 0");
        assert_eq!(gp.program(0, &catalog), [0, 1, 2, 3]);
        assert_eq!(gp.title(&catalog), "Coupe 0");
        gp = gp.changed(Formula, 1, &catalog);
        assert_eq!(gp.formula, super::Formula::Custom);
        assert_eq!(gp.title(&catalog), "Championnat");
        assert_eq!(gp.races, 4);
        let draw = gp.draw;
        let five = gp.clone().changed(Races, 1, &catalog);
        assert_eq!(five.races, 5);
        assert_ne!(five.draw, draw, "a new number of races: a new draw");
        assert_eq!(gp.clone().changed(Races, 3, &catalog).races, 2, "around");
        assert_eq!(gp.clone().changed(Races, -2, &catalog).races, 2);
        assert_eq!(gp.clone().changed(Races, -3, &catalog).races, n);
        assert_eq!(five.program(9, &catalog).len(), 5);
        let chosen = gp.clone().changed(Pick, 1, &catalog);
        assert_eq!(chosen.pick, super::Pick::Chosen);
        assert_eq!(chosen.chosen, [0, 1, 2, 3], "not chosen yet first");
        assert_eq!(chosen.program(9, &catalog), [0, 1, 2, 3]);
        let changed = chosen.clone().changed(Race(1), -1, &catalog);
        assert_eq!(changed.chosen, [0, 0, 2, 3], "a circuit twice if chosen so");
        let six = changed.clone().changed(Races, 2, &catalog);
        assert_eq!(
            six.chosen,
            [0, 0, 2, 3, 1, 4],
            "the next ones not chosen yet"
        );
        let two = six.changed(Races, 1, &catalog);
        assert_eq!(
            (two.races, two.chosen.as_slice()),
            (2, &[0, 0][..]),
            "around"
        );
        let random = chosen.clone().changed(Pick, 1, &catalog);
        assert_eq!(random.pick, super::Pick::Random);
        assert_ne!(random.draw, chosen.draw, "a new draw");
        assert_eq!(
            random.program(5, &catalog),
            draw_circuits(5, random.draw, 4, n)
        );
        // Without a cup, a championship is a custom one.
        let none = Catalog::of(&manifest_of(3, 1), 4);
        let mut alone = RaceSetup {
            mode: GameMode::GrandPrix,
            ..RaceSetup::default()
        };
        alone.fit(&none);
        assert_eq!(alone.formula, super::Formula::Custom);
        assert_eq!(alone.races, 3, "no more races than circuits");
        assert_eq!(alone.clone().changed(Formula, 1, &none), alone);
    }
}
