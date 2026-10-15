//! The items and their distribution (GD § 8, issues #16 and #18): pure logic, no ECS.
//! Unit-tested.
//!
//! - [`ItemKind`]: the items a kart can hold and use: those of phase 1 (pelure, pelote de laine,
//!   abeille, piment, croquette dorée, issue #16) and of phase 2 (issue #18: piment doré, bombe à
//!   confettis, chantilly, super câlin, fusée à confettis, dégonflage, oreiller géant,
//!   klaxon-canard).
//! - [`Prize`]: what a box can give, as written in the distribution table: an item, alone or by
//!   three (`PelureX3`...).
//! - [`ItemTable`]: the distribution by distance to the leader, read from `assets/data/items.ron`
//!   (compiled into the game). The file keeps the game design's full table; a table can leave
//!   some prizes out ([`ItemTable::from_file_with`]: online, the items that would not travel on
//!   the network, [`Prize::online`]: none since issue #61), and their weight goes to the others of
//!   the same row, proportionally. A draw can also leave one out for a moment ([`DrawRow::pick_without`]: the
//!   oreiller géant's rules, [`pillow_allowed`]).
//! - **Deterministic draws** ([`draw_unit`]): from the race seed (`RaceGrid::seed`), the kart's
//!   grid slot and how many items it drew before. Same race, same draws on every machine: the
//!   tests are reproducible and, on the network, every peer can check what a kart drew.

use serde::Deserialize;

/// The distribution table of the game (GD § 8), compiled into the game.
pub const ITEMS_RON: &str = include_str!("../../assets/data/items.ron");

/// An item a kart can hold, use, throw or drop (GD § 8, phases 1 and 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKind {
    /// Trap on the ground: spin out. Dragged behind as a shield.
    Pelure,
    /// Pelote de laine: rolls straight, bounces off the walls: tumble. Dragged as a shield; the
    /// "x3" turns around the kart.
    Pelote,
    /// Homes in on the kart just ahead: spin out. The "x3" turns around the kart.
    Abeille,
    /// A boost of about 1.2 s.
    Piment,
    /// Two croquettes.
    CroquetteDoree,
    /// Piment doré (issue #18): boosts at will for about 7 s, one per press.
    PimentDore,
    /// Bombe à confettis (issue #18): thrown ahead (or dropped behind), it explodes in a zone:
    /// tumble.
    Bombe,
    /// Chantilly (issue #18): cream on the screen of the racers ahead; the bots drive less
    /// precisely.
    Chantilly,
    /// Super câlin (issue #18): about 7 s invincible and faster; the karts it runs into tumble.
    SuperCalin,
    /// Fusée à confettis (issue #18): a very fast autopilot along the racing line for about 6 s;
    /// the karts it runs into tumble.
    Fusee,
    /// Dégonflage (issue #18): every opponent shrinks and slows down, and drops its items; the
    /// first ones stay small longer.
    Degonflage,
    /// Oreiller géant (issue #18): flies to the leader and crushes it (tumble, a small zone).
    /// One at a time on the track, none before 20 s of race, 30 s between two.
    OreillerGeant,
    /// Klaxon-canard (issue #18): a shock wave around the kart that destroys the items close by,
    /// the oreiller géant included.
    KlaxonCanard,
}

impl ItemKind {
    pub const ALL: [Self; 13] = [
        Self::Pelure,
        Self::Pelote,
        Self::Abeille,
        Self::Piment,
        Self::CroquetteDoree,
        Self::PimentDore,
        Self::Bombe,
        Self::Chantilly,
        Self::SuperCalin,
        Self::Fusee,
        Self::Degonflage,
        Self::OreillerGeant,
        Self::KlaxonCanard,
    ];

    /// Held out behind the kart while the item button is held (GD § 8: pelure, pelote).
    pub fn draggable(self) -> bool {
        matches!(self, Self::Pelure | Self::Pelote)
    }

    /// Something that goes on the track (an object), rather than an effect on the kart.
    pub fn is_object(self) -> bool {
        matches!(
            self,
            Self::Pelure | Self::Pelote | Self::Abeille | Self::Bombe | Self::OreillerGeant
        )
    }

    /// Its name in the HUD and the logs (ASCII: the HUD font has no accents).
    pub fn label(self) -> &'static str {
        match self {
            Self::Pelure => "Pelure",
            Self::Pelote => "Pelote",
            Self::Abeille => "Abeille",
            Self::Piment => "Piment",
            Self::CroquetteDoree => "Croquette doree",
            Self::PimentDore => "Piment dore",
            Self::Bombe => "Bombe a confettis",
            Self::Chantilly => "Chantilly",
            Self::SuperCalin => "Super calin",
            Self::Fusee => "Fusee a confettis",
            Self::Degonflage => "Degonflage",
            Self::OreillerGeant => "Oreiller geant",
            Self::KlaxonCanard => "Klaxon-canard",
        }
    }

    /// Its name with the accents (egui has them: the admin panel).
    pub fn name(self) -> &'static str {
        match self {
            Self::Pelure => "Pelure",
            Self::Pelote => "Pelote de laine",
            Self::Abeille => "Abeille",
            Self::Piment => "Piment",
            Self::CroquetteDoree => "Croquette dorée",
            Self::PimentDore => "Piment doré",
            Self::Bombe => "Bombe à confettis",
            Self::Chantilly => "Chantilly",
            Self::SuperCalin => "Super câlin",
            Self::Fusee => "Fusée à confettis",
            Self::Degonflage => "Dégonflage",
            Self::OreillerGeant => "Oreiller géant",
            Self::KlaxonCanard => "Klaxon-canard",
        }
    }

    /// It plays online: what it does reaches the other peers. Since issue #61, every item does
    /// (the chantilly, the super câlin, the fusée, the dégonflage and the oreiller géant came
    /// last). An item that would not (one of phase 3 before its network part) says so here: the
    /// online table (`ItemTable::online`) leaves it out, its weight given to the rest of its row.
    pub fn online(self) -> bool {
        true
    }
}

/// What a box can give, as in the distribution table (`assets/data/items.ron`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prize {
    Pelure,
    PelureX3,
    Pelote,
    PeloteX3,
    Abeille,
    AbeilleX3,
    Piment,
    PimentX3,
    CroquetteDoree,
    // --- Phase 2 (GD § 8, issue #18).
    PimentDore,
    Bombe,
    Chantilly,
    SuperCalin,
    Fusee,
    Degonflage,
    OreillerGeant,
    KlaxonCanard,
}

impl Prize {
    pub const ALL: [Self; 17] = [
        Prize::Pelure,
        Prize::PelureX3,
        Prize::Pelote,
        Prize::PeloteX3,
        Prize::Abeille,
        Prize::AbeilleX3,
        Prize::Piment,
        Prize::PimentX3,
        Prize::CroquetteDoree,
        Prize::PimentDore,
        Prize::Bombe,
        Prize::Chantilly,
        Prize::SuperCalin,
        Prize::Fusee,
        Prize::Degonflage,
        Prize::OreillerGeant,
        Prize::KlaxonCanard,
    ];

    /// The item and how many of it (every prize of the table is in the game since issue #18).
    pub fn item(self) -> Option<(ItemKind, u8)> {
        Some(match self {
            Prize::Pelure => (ItemKind::Pelure, 1),
            Prize::PelureX3 => (ItemKind::Pelure, 3),
            Prize::Pelote => (ItemKind::Pelote, 1),
            Prize::PeloteX3 => (ItemKind::Pelote, 3),
            Prize::Abeille => (ItemKind::Abeille, 1),
            Prize::AbeilleX3 => (ItemKind::Abeille, 3),
            Prize::Piment => (ItemKind::Piment, 1),
            Prize::PimentX3 => (ItemKind::Piment, 3),
            Prize::CroquetteDoree => (ItemKind::CroquetteDoree, 1),
            Prize::PimentDore => (ItemKind::PimentDore, 1),
            Prize::Bombe => (ItemKind::Bombe, 1),
            Prize::Chantilly => (ItemKind::Chantilly, 1),
            Prize::SuperCalin => (ItemKind::SuperCalin, 1),
            Prize::Fusee => (ItemKind::Fusee, 1),
            Prize::Degonflage => (ItemKind::Degonflage, 1),
            Prize::OreillerGeant => (ItemKind::OreillerGeant, 1),
            Prize::KlaxonCanard => (ItemKind::KlaxonCanard, 1),
        })
    }

    pub fn implemented(self) -> bool {
        self.item().is_some()
    }

    /// In the game and playing online (`ItemKind::online`).
    pub fn online(self) -> bool {
        self.item().is_some_and(|(item, _)| item.online())
    }

    /// Its name in the admin panel ("x3" for a triple).
    pub fn name(self) -> String {
        match self.item() {
            Some((item, count)) if count > 1 => format!("{} x{count}", item.name()),
            Some((item, _)) => item.name().to_string(),
            None => format!("{self:?}"),
        }
    }
}

/// A row of the table file: from this distance to the leader on, these prizes with these
/// weights.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableRow {
    pub from: f32,
    pub items: Vec<(Prize, u32)>,
}

/// The table file (`assets/data/items.ron`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableFile {
    pub rows: Vec<TableRow>,
}

/// A row of the distribution, as the game draws from it: the implemented prizes only.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawRow {
    pub from: f32,
    /// Prizes with their cumulative weight (the last one is the row's total).
    pub prizes: Vec<(Prize, u32)>,
}

impl DrawRow {
    pub fn total(&self) -> u32 {
        self.prizes.last().map_or(0, |(_, w)| *w)
    }

    /// The share of `prize` in this row (0 to 1).
    pub fn probability(&self, prize: Prize) -> f32 {
        let mut previous = 0;
        for (p, cumulative) in &self.prizes {
            if *p == prize {
                return (*cumulative - previous) as f32 / self.total().max(1) as f32;
            }
            previous = *cumulative;
        }
        0.0
    }

    /// The prize for `unit`, a uniform draw in `[0, 1)`.
    pub fn pick(&self, unit: f32) -> Prize {
        let target = (unit.clamp(0.0, 1.0) as f64 * self.total() as f64) as u32;
        self.prizes
            .iter()
            .find(|(_, cumulative)| target < *cumulative)
            .or(self.prizes.last())
            .map(|(p, _)| *p)
            .expect("a row of the item table without any prize")
    }

    /// The prize for `unit` without `left_out` (the oreiller géant while its rules forbid it,
    /// issue #18): its weight goes to the others of the row, proportionally. A row with nothing
    /// else draws as usual.
    pub fn pick_without(&self, unit: f32, left_out: Prize) -> Prize {
        let weight = |i: usize| self.prizes[i].1 - if i == 0 { 0 } else { self.prizes[i - 1].1 };
        let others: u32 = (0..self.prizes.len())
            .filter(|i| self.prizes[*i].0 != left_out)
            .map(weight)
            .sum();
        if others == 0 {
            return self.pick(unit);
        }
        let target = (unit.clamp(0.0, 1.0) as f64 * others as f64) as u32;
        let mut cumulative = 0;
        let mut last = None;
        for i in 0..self.prizes.len() {
            let prize = self.prizes[i].0;
            if prize == left_out {
                continue;
            }
            cumulative += weight(i);
            last = Some(prize);
            if target < cumulative {
                return prize;
            }
        }
        last.expect("a row with other prizes")
    }
}

/// The distribution of the items by distance to the leader (GD § 8), ready to draw from.
#[derive(bevy::prelude::Resource, Clone, Debug, PartialEq)]
pub struct ItemTable {
    pub rows: Vec<DrawRow>,
}

/// Why a table file cannot be used.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidTable(pub String);

impl std::fmt::Display for InvalidTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid item table: {}", self.0)
    }
}

impl std::error::Error for InvalidTable {}

impl ItemTable {
    /// The game's table (`assets/data/items.ron`), checked by a test: every prize.
    pub fn embedded() -> Self {
        Self::from_ron(ITEMS_RON).expect("assets/data/items.ron")
    }

    /// The game's table online (issue #18): the items that play online only
    /// ([`Prize::online`]), the others' weight given to the rest of their row. Since issue #61,
    /// every item plays online: it is the solo table.
    pub fn online() -> Self {
        Self::from_ron_with(ITEMS_RON, Prize::online).expect("assets/data/items.ron")
    }

    /// Reads a table file: rows by increasing distance, the first one from 0 (the leader's).
    pub fn from_ron(text: &str) -> Result<Self, InvalidTable> {
        Self::from_ron_with(text, Prize::implemented)
    }

    /// Same, with the prizes `available` only.
    pub fn from_ron_with(
        text: &str,
        available: impl Fn(Prize) -> bool,
    ) -> Result<Self, InvalidTable> {
        let file: TableFile =
            ron::from_str(text).map_err(|error| InvalidTable(error.to_string()))?;
        Self::from_file_with(&file, available)
    }

    pub fn from_file(file: &TableFile) -> Result<Self, InvalidTable> {
        Self::from_file_with(file, Prize::implemented)
    }

    /// Leaves out the prizes not `available` and gives their weight to the others of the same
    /// row, proportionally (GD § 8): that is just the row's remaining weights, drawn from their
    /// own total. A row with nothing available left uses the closest row before it that has
    /// something (the next one after it for the first rows).
    pub fn from_file_with(
        file: &TableFile,
        available: impl Fn(Prize) -> bool,
    ) -> Result<Self, InvalidTable> {
        let fail = |message: String| Err(InvalidTable(message));
        if file.rows.is_empty() {
            return fail("no row".to_string());
        }
        if file.rows[0].from != 0.0 {
            return fail(format!(
                "the first row starts at {} m, not 0",
                file.rows[0].from
            ));
        }
        for pair in file.rows.windows(2) {
            if pair[1].from <= pair[0].from || !pair[1].from.is_finite() {
                return fail(format!(
                    "rows not in increasing distance: {} then {}",
                    pair[0].from, pair[1].from
                ));
            }
        }
        let kept: Vec<Vec<(Prize, u32)>> = file
            .rows
            .iter()
            .map(|row| {
                let mut cumulative = 0;
                row.items
                    .iter()
                    .filter(|(prize, weight)| available(*prize) && *weight > 0)
                    .map(|(prize, weight)| {
                        cumulative += weight;
                        (*prize, cumulative)
                    })
                    .collect()
            })
            .collect();
        if kept.iter().all(|prizes| prizes.is_empty()) {
            return fail("no available item in any row".to_string());
        }
        let rows = file
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let prizes = (0..=i)
                    .rev()
                    .chain(i + 1..file.rows.len())
                    .map(|j| &kept[j])
                    .find(|prizes| !prizes.is_empty())
                    .cloned()
                    .unwrap_or_default();
                DrawRow {
                    from: row.from,
                    prizes,
                }
            })
            .collect();
        Ok(Self { rows })
    }

    /// The row for a kart `gap` meters behind the leader (the leader: 0).
    pub fn row(&self, gap: f32) -> &DrawRow {
        let gap = if gap.is_finite() { gap.max(0.0) } else { 0.0 };
        self.rows
            .iter()
            .rev()
            .find(|row| gap >= row.from)
            .unwrap_or(&self.rows[0])
    }

    /// What a box gives a kart `gap` meters behind the leader, for a uniform draw `unit`.
    pub fn draw(&self, gap: f32, unit: f32) -> Prize {
        self.row(gap).pick(unit)
    }

    /// Same, with the oreiller géant left out unless its rules allow one now
    /// ([`pillow_allowed`]).
    pub fn draw_with_pillow(&self, gap: f32, unit: f32, pillow: bool) -> Prize {
        if pillow {
            self.draw(gap, unit)
        } else {
            self.row(gap).pick_without(unit, Prize::OreillerGeant)
        }
    }
}

/// The oreiller géant's rules (GD § 8, issue #18): a box may give one only `min_race_ticks` after
/// GO (`race_ticks`: ticks since GO, `None` off a race: never), while no other one is in the
/// race (`exists`: in a kart's slot, its roulette or flying), and `cooldown_ticks` after the last
/// one was drawn (`last_draw`, on the same race clock).
pub fn pillow_allowed(
    race_ticks: Option<u32>,
    exists: bool,
    last_draw: Option<u32>,
    min_race_ticks: u32,
    cooldown_ticks: u32,
) -> bool {
    let Some(now) = race_ticks else {
        return false;
    };
    now >= min_race_ticks
        && !exists
        && last_draw.is_none_or(|last| now >= last.saturating_add(cooldown_ticks))
}

/// SplitMix64's mixer: spreads neighbouring inputs over all 64 bits.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The uniform draw (in `[0, 1)`) of a kart's `draw`-th item (0 = its first) in a race of seed
/// `race_seed`, for the kart on grid slot `slot`. Deterministic: the same race gives every kart
/// the same draws, whatever the machine.
pub fn draw_unit(race_seed: u64, slot: u64, draw: u32) -> f32 {
    let seed = mix(mix(mix(race_seed ^ 0x17E5_D1CE) ^ slot) ^ draw as u64);
    // The top 24 bits: exactly representable in an f32.
    (seed >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GD's table, with every weight in %: the rows sum to 100.
    #[test]
    fn the_game_table_is_the_game_designs_full_table() {
        let file: TableFile = ron::from_str(ITEMS_RON).unwrap();
        let froms: Vec<f32> = file.rows.iter().map(|r| r.from).collect();
        assert_eq!(froms, [0.0, 15.0, 50.0, 120.0, 250.0]);
        for row in &file.rows {
            let total: u32 = row.items.iter().map(|(_, w)| w).sum();
            assert_eq!(total, 100, "row from {} m", row.from);
        }
        // Phase 2 items are in the file (the table stays complete).
        assert!(
            file.rows
                .iter()
                .flat_map(|r| &r.items)
                .any(|(p, _)| *p == Prize::OreillerGeant)
        );
    }

    /// Issue #18: every item of the table is in the game: the solo table draws each prize with
    /// the game design's share, nothing left out (the rows still sum to 100).
    #[test]
    fn every_prize_of_the_table_is_in_the_game() {
        let file: TableFile = ron::from_str(ITEMS_RON).unwrap();
        let table = ItemTable::embedded();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(Prize::ALL.iter().all(|p| p.implemented()));
        for (row, drawn) in file.rows.iter().zip(&table.rows) {
            assert_eq!(drawn.total(), 100, "row from {} m", row.from);
            assert_eq!(drawn.prizes.len(), row.items.len());
            for (prize, weight) in &row.items {
                assert!(close(drawn.probability(*prize), *weight as f32 / 100.0));
            }
        }
        // Every phase 2 item is somewhere in the table.
        for item in &ItemKind::ALL[5..] {
            assert!(
                table
                    .rows
                    .iter()
                    .flat_map(|r| &r.prizes)
                    .any(|(p, _)| p.item().map(|(i, _)| i) == Some(*item)),
                "{item:?}"
            );
        }
    }

    /// The five items of phase 2 that did not play online until issue #61 (chantilly, super
    /// câlin, fusée, dégonflage, oreiller géant), left out by hand: the table without them.
    fn without_the_five() -> ItemTable {
        ItemTable::from_ron_with(ITEMS_RON, |p: Prize| {
            !matches!(
                p,
                Prize::Chantilly
                    | Prize::SuperCalin
                    | Prize::Fusee
                    | Prize::Degonflage
                    | Prize::OreillerGeant
            )
        })
        .unwrap()
    }

    /// GD § 8: the items left out of a table give their weight to the others of the row,
    /// proportionally. Online, every item plays since issue #61: the online table is the solo
    /// table, each prize with the game design's exact share. The redistribution is still checked,
    /// the same way as when the five items of phase 2 that came last were left out of the online
    /// table (#18): the leader's row keeps its 100 (pelure 30, pelote 25, croquette dorée 20,
    /// pelure x3 10, klaxon-canard 10, bombe 5); 120 to 250 m: piment x3 25, abeille x3 20 and
    /// piment doré 15, out of 60; beyond 250 m: piment doré 20 and piment x3 15, out of 35.
    #[test]
    fn items_left_out_give_their_weight_to_the_others_of_the_row() {
        let online = ItemTable::online();
        assert_eq!(online, ItemTable::embedded(), "online: the solo table");
        assert!(Prize::ALL.iter().all(|p| p.online()));
        assert!(ItemKind::ALL.iter().all(|i| i.online()));
        let file: TableFile = ron::from_str(ITEMS_RON).unwrap();
        for (row, drawn) in file.rows.iter().zip(&online.rows) {
            assert_eq!(drawn.total(), 100, "row from {} m", row.from);
            for (prize, weight) in &row.items {
                assert!((drawn.probability(*prize) - *weight as f32 / 100.0).abs() < 1e-6);
            }
        }
        let table = without_the_five();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        let leader = table.row(0.0);
        assert_eq!(leader.total(), 100);
        assert!(close(leader.probability(Prize::KlaxonCanard), 0.1));
        assert!(close(leader.probability(Prize::Bombe), 0.05));
        let second = table.row(20.0);
        assert_eq!(second.total(), 95);
        assert!(close(second.probability(Prize::Abeille), 25.0 / 95.0));
        assert_eq!(second.probability(Prize::Chantilly), 0.0);
        assert_eq!(table.row(60.0).total(), 85);
        let far = table.row(200.0);
        assert_eq!(far.total(), 60);
        assert!(close(far.probability(Prize::PimentX3), 25.0 / 60.0));
        assert!(close(far.probability(Prize::AbeilleX3), 20.0 / 60.0));
        assert!(close(far.probability(Prize::PimentDore), 15.0 / 60.0));
        assert_eq!(far.probability(Prize::Fusee), 0.0);
        let last = table.row(1000.0);
        assert_eq!(
            last.prizes,
            vec![(Prize::PimentDore, 20), (Prize::PimentX3, 35)]
        );
        assert!(close(last.probability(Prize::PimentDore), 20.0 / 35.0));
        for row in &table.rows {
            let sum: f32 = row.prizes.iter().map(|(p, _)| row.probability(*p)).sum();
            assert!(close(sum, 1.0), "row from {}", row.from);
            assert!(row.prizes.iter().all(|(p, _)| {
                !matches!(
                    p,
                    Prize::Chantilly
                        | Prize::SuperCalin
                        | Prize::Fusee
                        | Prize::Degonflage
                        | Prize::OreillerGeant
                )
            }));
        }
    }

    /// The row follows the distance to the leader: the leader uses the first one, a negative or
    /// invalid gap too; each row up to the next one.
    #[test]
    fn the_row_follows_the_distance_to_the_leader() {
        let table = ItemTable::embedded();
        for (gap, from) in [
            (0.0, 0.0),
            (-5.0, 0.0),
            (f32::NAN, 0.0),
            (14.9, 0.0),
            (15.0, 15.0),
            (49.0, 15.0),
            (50.0, 50.0),
            (119.0, 50.0),
            (120.0, 120.0),
            (250.0, 250.0),
            (9000.0, 250.0),
        ] {
            assert_eq!(table.row(gap).from, from, "{gap} m");
        }
    }

    /// 10 000 draws of a row (as the game draws them: from the race seed and the kart) give
    /// each prize its share within 4 standard deviations, and nothing else.
    fn assert_draws_follow(
        row: &DrawRow,
        seed: u64,
        draw: impl Fn(f32) -> Prize,
        without: &[Prize],
    ) {
        const DRAWS: u32 = 10_000;
        let mut counts: Vec<(Prize, u32)> = Vec::new();
        for n in 0..DRAWS {
            let prize = draw(draw_unit(seed, 4, n));
            assert!(prize.implemented(), "{prize:?}");
            match counts.iter_mut().find(|(p, _)| *p == prize) {
                Some((_, n)) => *n += 1,
                None => counts.push((prize, 1)),
            }
        }
        let kept: u32 = row
            .prizes
            .iter()
            .filter(|(p, _)| !without.contains(p))
            .map(|(p, _)| (row.probability(*p) * row.total() as f32).round() as u32)
            .sum();
        for (prize, _) in &row.prizes {
            let seen = counts
                .iter()
                .find(|(c, _)| c == prize)
                .map_or(0, |(_, n)| *n) as f32
                / DRAWS as f32;
            if without.contains(prize) {
                assert_eq!(seen, 0.0, "{prize:?} left out");
                continue;
            }
            let p = row.probability(*prize) * row.total() as f32 / kept as f32;
            let sigma = (p * (1.0 - p) / DRAWS as f32).sqrt();
            assert!(
                (seen - p).abs() <= 4.0 * sigma + 1e-6,
                "row from {} m, {prize:?}: {seen} drawn, {p} expected (σ {sigma})",
                row.from
            );
        }
        let expected = row.prizes.iter().filter(|(p, _)| !without.contains(p));
        assert_eq!(counts.len(), expected.count(), "row from {} m", row.from);
    }

    /// Acceptance criterion of #16, again for #18 and #61: 10 000 draws per row, from the race
    /// seed and the kart (as the game draws), give each prize its share within 4 standard
    /// deviations, in the solo table (every item), online (the solo table since #61) and in a
    /// table that leaves items out (their weight redistributed); and without the oreiller géant
    /// when its rules forbid it (its weight goes to the rest of the row).
    #[test]
    fn ten_thousand_draws_follow_the_table() {
        for table in [
            ItemTable::embedded(),
            ItemTable::online(),
            without_the_five(),
        ] {
            for (r, row) in table.rows.iter().enumerate() {
                let gap = row.from + 1.0;
                let seed = r as u64 * 7 + 3;
                assert_draws_follow(row, seed, |unit| table.draw(gap, unit), &[]);
                assert_draws_follow(
                    row,
                    seed + 100,
                    |unit| table.draw_with_pillow(gap, unit, false),
                    &[Prize::OreillerGeant],
                );
                // Allowed: the oreiller in its share.
                assert_draws_follow(
                    row,
                    seed + 200,
                    |unit| table.draw_with_pillow(gap, unit, true),
                    &[],
                );
            }
        }
    }

    /// GD § 8 (issue #18): no oreiller géant before 20 s of race, while another one is in the
    /// race (a kart holds it, its roulette spins, it flies), nor within 30 s of the last one
    /// drawn; never off a race.
    #[test]
    fn the_pillow_rules() {
        let (min, cooldown) = (1200, 1800);
        let allowed = |now, exists, last| pillow_allowed(now, exists, last, min, cooldown);
        assert!(!allowed(None, false, None), "off a race");
        assert!(!allowed(Some(0), false, None));
        assert!(!allowed(Some(1199), false, None), "before 20 s");
        assert!(allowed(Some(1200), false, None));
        assert!(!allowed(Some(5000), true, None), "one at a time");
        assert!(
            !allowed(Some(5000), false, Some(3300)),
            "29 s after the last"
        );
        assert!(
            allowed(Some(5100), false, Some(3300)),
            "30 s after the last"
        );
        assert!(!allowed(Some(5100), true, Some(3300)));
        // A row of only the oreiller draws it anyway (nothing else to give).
        let row = DrawRow {
            from: 0.0,
            prizes: vec![(Prize::OreillerGeant, 5)],
        };
        assert_eq!(
            row.pick_without(0.5, Prize::OreillerGeant),
            Prize::OreillerGeant
        );
        // Cumulative weights: pelure 10, oreiller 15, pelote 25.
        let row = DrawRow {
            from: 0.0,
            prizes: vec![
                (Prize::Pelure, 10),
                (Prize::OreillerGeant, 25),
                (Prize::Pelote, 50),
            ],
        };
        assert_eq!(row.pick_without(0.0, Prize::OreillerGeant), Prize::Pelure);
        assert_eq!(row.pick_without(0.3, Prize::OreillerGeant), Prize::Pelote);
        assert_eq!(row.pick_without(0.999, Prize::OreillerGeant), Prize::Pelote);
        assert_eq!(row.pick(0.3), Prize::OreillerGeant);
    }

    /// The draws are deterministic, different from one kart, one draw and one race to the other,
    /// and uniform enough.
    #[test]
    fn draws_are_deterministic_and_uniform() {
        assert_eq!(draw_unit(3, 5, 7), draw_unit(3, 5, 7));
        assert_ne!(draw_unit(3, 5, 7), draw_unit(3, 5, 8));
        assert_ne!(draw_unit(3, 5, 7), draw_unit(3, 6, 7));
        assert_ne!(draw_unit(3, 5, 7), draw_unit(4, 5, 7));
        let mut buckets = [0u32; 10];
        for draw in 0..10_000 {
            let u = draw_unit(0, 0, draw);
            assert!((0.0..1.0).contains(&u));
            buckets[(u * 10.0) as usize] += 1;
        }
        assert!(
            buckets.iter().all(|n| (900..1100).contains(n)),
            "{buckets:?}"
        );
    }

    /// A row left without any available item takes the closest row before it; a file that is
    /// not a table, or whose rows are out of order, is refused with the reason. (Until #18, the
    /// bombe and the fusée were not in the game: here they are left out by hand.)
    #[test]
    fn a_row_without_available_items_and_invalid_files() {
        let available = |p: Prize| !matches!(p, Prize::Bombe | Prize::Fusee);
        let table = ItemTable::from_ron_with(
            "(rows: [(from: 0.0, items: [(Pelure, 10)]), (from: 20.0, items: [(Bombe, 10)]), \
             (from: 40.0, items: [(Piment, 3), (Fusee, 9)])])",
            available,
        )
        .unwrap();
        assert_eq!(table.draw(30.0, 0.5), Prize::Pelure);
        assert_eq!(table.draw(45.0, 0.99), Prize::Piment);
        let first_empty = ItemTable::from_ron_with(
            "(rows: [(from: 0.0, items: [(Bombe, 10)]), (from: 20.0, items: [(Pelote, 1)])])",
            available,
        )
        .unwrap();
        assert_eq!(first_empty.draw(0.0, 0.0), Prize::Pelote);
        // Every prize available: the bombe is drawn.
        let all = ItemTable::from_ron(
            "(rows: [(from: 0.0, items: [(Bombe, 10)]), (from: 20.0, items: [(Pelote, 1)])])",
        )
        .unwrap();
        assert_eq!(all.draw(0.0, 0.0), Prize::Bombe);
        assert_eq!(
            ItemTable::from_ron_with("(rows: [(from: 0.0, items: [(Bombe, 1)])])", available)
                .unwrap_err()
                .0,
            "no available item in any row"
        );
        for (text, reason) in [
            ("(rows: [])", "no row"),
            ("(rows: [(from: 5.0, items: [(Pelure, 1)])])", "first row"),
            (
                "(rows: [(from: 0.0, items: [(Pelure, 1)]), (from: 0.0, items: [(Pelote, 1)])])",
                "increasing",
            ),
            (
                "(rows: [(from: 0.0, items: [(Pelure, 0)])])",
                "no available",
            ),
            (
                "(rows: [(from: 0.0, items: [(Chaussette, 1)])])",
                "Chaussette",
            ),
            ("(rangs: [])", "rangs"),
        ] {
            let error = ItemTable::from_ron(text).unwrap_err();
            assert!(error.0.contains(reason), "{text}: {error}");
        }
    }
}
