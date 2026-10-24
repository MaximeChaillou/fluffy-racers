//! The « Jouer solo » screen (issue #99), pure: its texts (the title, the header of its races, the
//! keys), the place of its lines (on the left the settings, on the right the races). The vignette
//! of a circuit (issue #181: its line seen from above) is `vignette.rs`. The choice itself is
//! `modes::setup` (`RaceSetup`, headless); its tree is `solo_screen.rs`.

use crate::modes::{Catalog, Formula, GameMode, MenuRow, Pick, RaceSetup};

/// The screen's title.
pub const TITLE: &str = "Jouer solo";
/// Its buttons.
pub const START_LABEL: &str = "C'est parti !";
pub const BACK_LABEL: &str = "Retour";

/// Whether a line is shown on the right, with the races (the chosen circuits of a custom
/// championship), or on the left with the settings.
pub fn on_the_right(row: MenuRow) -> bool {
    row.is_race()
}

/// The header of the races, on the right: the cup's name, « Tes 5 courses » (a custom
/// championship: « tirées au sort » when drawn), or the circuit's name (a race alone, a time
/// trial).
pub fn header(setup: &RaceSetup, catalog: &Catalog) -> String {
    match setup.mode {
        GameMode::GrandPrix => match setup.formula {
            Formula::Cup => setup.title(catalog),
            Formula::Custom => {
                let races = format!("Tes {} courses", setup.races);
                match setup.pick {
                    Pick::Random => format!("{races}, tirées au sort"),
                    Pick::Chosen => races,
                }
            }
        },
        GameMode::TimeTrial | GameMode::FreeRace => catalog.name(setup.circuit).to_string(),
    }
}

/// Issue #110: the right column shows this many races at most. A custom championship has up to
/// as many races as there are circuits (10 now, 24 at the end): beyond this, the column shows a
/// window of them that follows the race with the focus, and a line says how many are hidden.
pub const MAX_CARDS: usize = 8;

/// Issue #110: the races shown on the right, out of `races`: all of them up to [`MAX_CARDS`];
/// beyond, [`MAX_CARDS`] of them, the race with the focus (`focus`, if a race has it) among them
/// with three before it when it can (the first ones without the focus on a race).
pub fn shown_races(races: usize, focus: Option<usize>) -> std::ops::Range<usize> {
    if races <= MAX_CARDS {
        return 0..races;
    }
    let first = focus
        .map_or(0, |race| race.saturating_sub(MAX_CARDS / 2 - 1))
        .min(races - MAX_CARDS);
    first..first + MAX_CARDS
}

/// Issue #110: the line under the races when some are hidden (`before` above the ones shown,
/// `after` below them), or none.
pub fn hidden_races(before: usize, after: usize) -> Option<String> {
    let plural = |n: usize| if n > 1 { "s" } else { "" };
    match (before, after) {
        (0, 0) => None,
        (b, 0) => Some(format!("+ {b} course{} plus haut", plural(b))),
        (0, a) => Some(format!("+ {a} course{} plus bas", plural(a))),
        (b, a) => Some(format!("+ {b} plus haut, + {a} plus bas")),
    }
}

/// The record line of a time trial: « Ton record : 0:40.00 », or none yet.
pub fn record_line(record: Option<&str>) -> String {
    match record {
        Some(time) => format!("Ton record : {time}"),
        None => "Pas encore de record : à toi de jouer !".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #110: up to eight races, all shown; more, eight of them, the one with the focus among
    /// them (three before it when it can), and a line for the others.
    #[test]
    fn the_races_shown_follow_the_focus() {
        assert_eq!(shown_races(4, None), 0..4);
        assert_eq!(shown_races(8, Some(7)), 0..8);
        assert_eq!(shown_races(10, None), 0..8);
        assert_eq!(shown_races(10, Some(2)), 0..8);
        assert_eq!(shown_races(10, Some(5)), 2..10);
        assert_eq!(shown_races(10, Some(9)), 2..10);
        assert_eq!(shown_races(24, Some(12)), 9..17);
        for races in 2..=24 {
            for focus in 0..races {
                let shown = shown_races(races, Some(focus));
                assert!(shown.contains(&focus) && shown.len() == races.min(MAX_CARDS));
            }
        }
        assert_eq!(hidden_races(0, 0), None);
        assert_eq!(hidden_races(0, 2).unwrap(), "+ 2 courses plus bas");
        assert_eq!(hidden_races(1, 0).unwrap(), "+ 1 course plus haut");
        assert_eq!(hidden_races(3, 5).unwrap(), "+ 3 plus haut, + 5 plus bas");
    }

    /// The header and the keys.
    #[test]
    fn the_screen_says_what_is_raced() {
        let catalog = Catalog::game(4);
        let mut setup = RaceSetup::default();
        setup.fit(&catalog);
        assert_eq!(header(&setup, &catalog), "Prairie Pompon");
        setup.mode = GameMode::GrandPrix;
        assert_eq!(header(&setup, &catalog), "Coupe Doudou");
        setup.formula = Formula::Custom;
        setup.races = 5;
        assert_eq!(header(&setup, &catalog), "Tes 5 courses, tirées au sort");
        setup.pick = Pick::Chosen;
        assert_eq!(header(&setup, &catalog), "Tes 5 courses");
        assert!(record_line(Some("0:40.00")).ends_with("0:40.00"));
        for line in [record_line(None), START_LABEL.into(), BACK_LABEL.into()] {
            assert!(line.chars().count() <= 48);
        }
        assert!(on_the_right(MenuRow::Race(0)) && !on_the_right(MenuRow::Bots));
    }
}
