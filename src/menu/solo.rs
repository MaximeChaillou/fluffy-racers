//! The « Jouer solo » screen (issue #99), pure: its texts (the title, the header of its races, the
//! keys), the place of its lines (on the left the settings, on the right the races), and the
//! vignette of a circuit (its outline seen from above, in the box of the vignette). The choice
//! itself is `modes::setup` (`RaceSetup`, headless); its tree is `solo_screen.rs`.

use bevy::math::Vec2;

use crate::modes::{Catalog, Formula, GameMode, MenuRow, Pick, RaceSetup};
use crate::track::TrackData;

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

/// Points of a circuit's outline for a vignette: its centerline seen from above (x right, z
/// down the vignette; mirrored, x the other way), every `step` meters at most along its control
/// points' polyline, in the unit box `[0, 1]²`, centered and keeping its proportions in a box of
/// `aspect` (width / height). `count` points, evenly spaced along it.
pub fn outline(data: &TrackData, mirror: bool, aspect: f32, count: usize) -> Vec<Vec2> {
    let points: Vec<Vec2> = data
        .points
        .iter()
        .map(|point| {
            let x = if mirror { -point.pos.0 } else { point.pos.0 };
            Vec2::new(x, point.pos.2)
        })
        .collect();
    spread(&smooth(&points), aspect, count)
}

/// A closed Catmull-Rom curve through `points` (the circuit's centerline is one), 8 points per
/// segment.
fn smooth(points: &[Vec2]) -> Vec<Vec2> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let at = |i: usize| points[i % n];
    let mut curve = Vec::with_capacity(n * 8);
    for i in 0..n {
        let (p0, p1, p2, p3) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
        for k in 0..8 {
            let t = k as f32 / 8.0;
            let (t2, t3) = (t * t, t * t * t);
            curve.push(
                0.5 * ((2.0 * p1)
                    + (p2 - p0) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3),
            );
        }
    }
    curve
}

/// `count` points evenly spaced along the closed polyline `points`, fitted in the unit box (see
/// [`outline`]).
fn spread(points: &[Vec2], aspect: f32, count: usize) -> Vec<Vec2> {
    if points.len() < 2 || count == 0 {
        return Vec::new();
    }
    let n = points.len();
    let lengths: Vec<f32> = (0..n)
        .map(|i| points[i].distance(points[(i + 1) % n]))
        .collect();
    let total: f32 = lengths.iter().sum();
    if total <= f32::EPSILON {
        return Vec::new();
    }
    let mut spaced = Vec::with_capacity(count);
    let (mut segment, mut walked) = (0usize, 0.0f32);
    for k in 0..count {
        let wanted = total * k as f32 / count as f32;
        while walked + lengths[segment] < wanted && segment + 1 < n {
            walked += lengths[segment];
            segment += 1;
        }
        let t = ((wanted - walked) / lengths[segment].max(f32::EPSILON)).clamp(0.0, 1.0);
        spaced.push(points[segment].lerp(points[(segment + 1) % n], t));
    }
    // Into the unit box: the same scale on both axes in a box `aspect` times wider than high
    // (the box of the whole curve: the same for any count).
    let (min, max) = points.iter().fold(
        (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
        |(min, max), p| (min.min(*p), max.max(*p)),
    );
    let size = (max - min).max(Vec2::splat(1e-3));
    let aspect = aspect.max(1e-3);
    // Box units: x in [0, aspect], y in [0, 1].
    let scale = (aspect / size.x).min(1.0 / size.y);
    let offset = (Vec2::new(aspect, 1.0) - size * scale) / 2.0;
    spaced
        .iter()
        .map(|p| {
            let q = (*p - min) * scale + offset;
            Vec2::new(q.x / aspect, q.y)
        })
        .collect()
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

    fn prairie() -> TrackData {
        TrackData::from_ron(include_str!("../../assets/tracks/prairie.track.ron")).unwrap()
    }

    /// The vignette's outline: `count` points in the unit box, touching two of its opposite sides
    /// (centered on the other axis), evenly spaced; mirrored, the same outline flipped left to
    /// right; the first point is the start line.
    #[test]
    fn the_outline_fits_its_vignette() {
        let data = prairie();
        for aspect in [1.0, 1.8, 0.5] {
            let points = outline(&data, false, aspect, 60);
            assert_eq!(points.len(), 60);
            let (min, max) = points.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(min, max), p| (min.min(*p), max.max(*p)),
            );
            assert!(min.x >= -1e-4 && min.y >= -1e-4, "{min}");
            assert!(max.x <= 1.0 + 1e-4 && max.y <= 1.0 + 1e-4, "{max}");
            // It fills the box on one axis (to the spacing of its points), centered on both.
            let touches_x = min.x < 0.03 && max.x > 0.97;
            let touches_y = min.y < 0.03 && max.y > 0.97;
            assert!(touches_x || touches_y, "{aspect}: {min} {max}");
            assert!(((min.x + max.x) / 2.0 - 0.5).abs() < 0.03, "{aspect}");
            assert!(((min.y + max.y) / 2.0 - 0.5).abs() < 0.03, "{aspect}");
            // Evenly spaced, in the vignette's own units.
            let gaps: Vec<f32> = points
                .windows(2)
                .map(|w| ((w[1] - w[0]) * Vec2::new(aspect, 1.0)).length())
                .collect();
            let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
            assert!(
                gaps.iter().all(|g| (g - mean).abs() < 0.35 * mean),
                "{gaps:?}"
            );
            let mirrored = outline(&data, true, aspect, 60);
            for (a, b) in points.iter().zip(&mirrored) {
                assert!((a.x - (1.0 - b.x)).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3);
            }
        }
        // The start line first, whatever the count.
        let first = outline(&data, false, 1.0, 10)[0];
        assert!(outline(&data, false, 1.0, 400)[0].distance(first) < 1e-5);
        assert!(outline(&data, false, 1.0, 0).is_empty());
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
