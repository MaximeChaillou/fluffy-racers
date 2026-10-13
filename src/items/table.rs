//! The items and their distribution (GD § 8, issue #16): pure logic, no ECS. Unit-tested.
//!
//! - [`ItemKind`]: the items a kart can hold and use. Phase 1 only (GD § 8): pelure, pelote de
//!   laine, abeille, piment, croquette dorée.
//! - [`Prize`]: what a box can give, as written in the distribution table: an item, alone or by
//!   three (`PelureX3`...), and the items of phase 2 that are not in the game yet.
//! - [`ItemTable`]: the distribution by distance to the leader, read from `assets/data/items.ron`
//!   (compiled into the game). The file keeps the game design's full table; the prizes not
//!   implemented yet ([`Prize::item`] is `None`) are removed here and their weight goes to the
//!   others of the same row, proportionally. When a phase 2 item comes, mapping its prize to an
//!   item is all it takes.
//! - **Deterministic draws** ([`draw_unit`]): from the race seed (`RaceGrid::seed`), the kart's
//!   grid slot and how many items it drew before. Same race, same draws on every machine: the
//!   tests are reproducible and, on the network, every peer can check what a kart drew.

use serde::Deserialize;

/// The distribution table of the game (GD § 8), compiled into the game.
pub const ITEMS_RON: &str = include_str!("../../assets/data/items.ron");

/// An item a kart can hold, use, throw or drop (GD § 8, phase 1).
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
}

impl ItemKind {
    pub const ALL: [Self; 5] = [
        Self::Pelure,
        Self::Pelote,
        Self::Abeille,
        Self::Piment,
        Self::CroquetteDoree,
    ];

    /// Held out behind the kart while the item button is held (GD § 8: pelure, pelote).
    pub fn draggable(self) -> bool {
        matches!(self, Self::Pelure | Self::Pelote)
    }

    /// Something that goes on the track (an object), rather than an effect on the kart.
    pub fn is_object(self) -> bool {
        matches!(self, Self::Pelure | Self::Pelote | Self::Abeille)
    }

    /// Its name in the HUD and the logs (ASCII: the HUD font has no accents).
    pub fn label(self) -> &'static str {
        match self {
            Self::Pelure => "Pelure",
            Self::Pelote => "Pelote",
            Self::Abeille => "Abeille",
            Self::Piment => "Piment",
            Self::CroquetteDoree => "Croquette doree",
        }
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
    // --- Phase 2 (GD § 8): in the table, not in the game yet.
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
    /// The item and how many of it, or `None` while the item is not implemented: the table
    /// leaves it out and gives its weight to the others.
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
            Prize::PimentDore
            | Prize::Bombe
            | Prize::Chantilly
            | Prize::SuperCalin
            | Prize::Fusee
            | Prize::Degonflage
            | Prize::OreillerGeant
            | Prize::KlaxonCanard => return None,
        })
    }

    pub fn implemented(self) -> bool {
        self.item().is_some()
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
    /// The game's table (`assets/data/items.ron`), checked by a test.
    pub fn embedded() -> Self {
        Self::from_ron(ITEMS_RON).expect("assets/data/items.ron")
    }

    /// Reads a table file: rows by increasing distance, the first one from 0 (the leader's).
    pub fn from_ron(text: &str) -> Result<Self, InvalidTable> {
        let file: TableFile =
            ron::from_str(text).map_err(|error| InvalidTable(error.to_string()))?;
        Self::from_file(&file)
    }

    /// Removes the prizes not implemented yet and gives their weight to the others of the same
    /// row, proportionally (GD § 8): that is just the row's remaining weights, drawn from their
    /// own total. A row with nothing implemented left uses the closest row before it that has
    /// something (the next one after it for the first rows).
    pub fn from_file(file: &TableFile) -> Result<Self, InvalidTable> {
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
        let implemented: Vec<Vec<(Prize, u32)>> = file
            .rows
            .iter()
            .map(|row| {
                let mut cumulative = 0;
                row.items
                    .iter()
                    .filter(|(prize, weight)| prize.implemented() && *weight > 0)
                    .map(|(prize, weight)| {
                        cumulative += weight;
                        (*prize, cumulative)
                    })
                    .collect()
            })
            .collect();
        if implemented.iter().all(|prizes| prizes.is_empty()) {
            return fail("no implemented item in any row".to_string());
        }
        let rows = file
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let prizes = (0..=i)
                    .rev()
                    .chain(i + 1..file.rows.len())
                    .map(|j| &implemented[j])
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

    /// GD § 8: the items not implemented yet are removed and their weight goes to the others of
    /// the row, proportionally. Leader's row: pelure 30, pelote 25, croquette dorée 20, pelure x3
    /// 10 (the klaxon-canard 10 and the bombe 5 removed) → shares out of 85.
    #[test]
    fn missing_items_give_their_weight_to_the_others_of_the_row() {
        let table = ItemTable::embedded();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        let leader = table.row(0.0);
        assert_eq!(leader.total(), 85);
        assert!(close(leader.probability(Prize::Pelure), 30.0 / 85.0));
        assert!(close(leader.probability(Prize::Pelote), 25.0 / 85.0));
        assert!(close(
            leader.probability(Prize::CroquetteDoree),
            20.0 / 85.0
        ));
        assert!(close(leader.probability(Prize::PelureX3), 10.0 / 85.0));
        assert_eq!(leader.probability(Prize::KlaxonCanard), 0.0);
        // Far behind: only piment x3 is in the game yet.
        let last = table.row(1000.0);
        assert_eq!(last.prizes, vec![(Prize::PimentX3, 15)]);
        assert!(close(last.probability(Prize::PimentX3), 1.0));
        // 120 to 250 m: piment x3 25 and abeille x3 20.
        let far = table.row(200.0);
        assert!(close(far.probability(Prize::PimentX3), 25.0 / 45.0));
        assert!(close(far.probability(Prize::AbeilleX3), 20.0 / 45.0));
        for row in &table.rows {
            let sum: f32 = row.prizes.iter().map(|(p, _)| row.probability(*p)).sum();
            assert!(close(sum, 1.0), "row from {}", row.from);
            assert!(row.prizes.iter().all(|(p, _)| p.implemented()));
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

    /// Acceptance criterion of #16: 10 000 draws per row of the table, from the race seed and
    /// the kart (as the game draws), give each prize its share (with the redistribution) within
    /// 4 standard deviations; never an item that is not in the game.
    #[test]
    fn ten_thousand_draws_follow_the_table() {
        let table = ItemTable::embedded();
        const DRAWS: u32 = 10_000;
        for (r, row) in table.rows.iter().enumerate() {
            let gap = row.from + 1.0;
            let mut counts: Vec<(Prize, u32)> = Vec::new();
            for draw in 0..DRAWS {
                let prize = table.draw(gap, draw_unit(r as u64 * 7 + 3, 4, draw));
                assert!(prize.implemented(), "{prize:?}");
                match counts.iter_mut().find(|(p, _)| *p == prize) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((prize, 1)),
                }
            }
            for (prize, _) in &row.prizes {
                let p = row.probability(*prize);
                let seen = counts
                    .iter()
                    .find(|(c, _)| c == prize)
                    .map_or(0, |(_, n)| *n) as f32
                    / DRAWS as f32;
                let sigma = (p * (1.0 - p) / DRAWS as f32).sqrt();
                assert!(
                    (seen - p).abs() <= 4.0 * sigma + 1e-6,
                    "row from {} m, {prize:?}: {seen} drawn, {p} expected (σ {sigma})",
                    row.from
                );
            }
            assert_eq!(counts.len(), row.prizes.len(), "row from {} m", row.from);
        }
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

    /// A row left without any implemented item takes the closest row before it; a file that is
    /// not a table, or whose rows are out of order, is refused with the reason.
    #[test]
    fn a_row_without_implemented_items_and_invalid_files() {
        let table = ItemTable::from_ron(
            "(rows: [(from: 0.0, items: [(Pelure, 10)]), (from: 20.0, items: [(Bombe, 10)]), \
             (from: 40.0, items: [(Piment, 3), (Fusee, 9)])])",
        )
        .unwrap();
        assert_eq!(table.draw(30.0, 0.5), Prize::Pelure);
        assert_eq!(table.draw(45.0, 0.99), Prize::Piment);
        let first_empty = ItemTable::from_ron(
            "(rows: [(from: 0.0, items: [(Bombe, 10)]), (from: 20.0, items: [(Pelote, 1)])])",
        )
        .unwrap();
        assert_eq!(first_empty.draw(0.0, 0.0), Prize::Pelote);
        for (text, reason) in [
            ("(rows: [])", "no row"),
            ("(rows: [(from: 5.0, items: [(Pelure, 1)])])", "first row"),
            (
                "(rows: [(from: 0.0, items: [(Pelure, 1)]), (from: 0.0, items: [(Pelote, 1)])])",
                "increasing",
            ),
            (
                "(rows: [(from: 0.0, items: [(Bombe, 1)])])",
                "no implemented",
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
