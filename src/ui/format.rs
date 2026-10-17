//! HUD texts (pure): what to show, written into reused strings (no allocation once their
//! capacity is reached).
//!
//! Issue #100: the HUD writes in Fredoka, the charter's font (`menu::style`), which has every
//! letter of French: « Entrée », « Temps estimé », « Éloïse » on the results. [`writable`] says
//! which characters it can draw (a test reads them in the font files): every text of the HUD is
//! made of them, and a pseudo keeps only them (`net::pseudo`).

use std::fmt::Write;

use crate::TICK_HZ;
use crate::race::{
    Course, Race, RacePhase, RaceProgress, RaceTuning, ResultTime, seconds_to_ticks,
};

/// Big message in the middle of the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Banner {
    #[default]
    None,
    /// Countdown number: 3, 2, 1.
    Count(u32),
    Go,
    FinalLap,
    /// The followed racer crossed the finish line.
    Finished,
}

impl Banner {
    pub fn write(&self, out: &mut String) {
        out.clear();
        let _ = match self {
            Banner::None => Ok(()),
            Banner::Count(n) => write!(out, "{n}"),
            Banner::Go => write!(out, "GO !"),
            Banner::FinalLap => write!(out, "DERNIER TOUR !"),
            Banner::Finished => write!(out, "FINI !"),
        };
    }
}

/// Everything the HUD shows for the followed racer. Compared from frame to frame: a text is
/// rewritten only when its value changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HudValues {
    pub rank: u32,
    pub racers: u32,
    pub lap: u32,
    pub laps: u32,
    /// The racer's chrono, in hundredths of a second (what is displayed).
    pub time: u32,
    /// Best lap, in hundredths of a second.
    pub best_lap: Option<u32>,
    pub banner: Banner,
    /// Croquettes held (GD § 7, issue #14).
    pub croquettes: u32,
}

impl HudValues {
    pub fn new(
        race: &Race,
        course: &Course,
        tuning: &RaceTuning,
        progress: &RaceProgress,
        racers: usize,
        croquettes: u32,
    ) -> Self {
        let race_ticks = race.race_ticks();
        Self {
            rank: progress.rank.max(1),
            racers: racers as u32,
            lap: progress.current_lap(course),
            laps: course.laps,
            time: hundredths(progress.time(race_ticks)),
            best_lap: progress.best_lap().map(hundredths),
            banner: banner(race, course, tuning, progress),
            croquettes,
        }
    }
}

/// The banner for the followed racer.
pub fn banner(
    race: &Race,
    course: &Course,
    tuning: &RaceTuning,
    progress: &RaceProgress,
) -> Banner {
    let phase = race.phase();
    if let Some(n) = race.flow.countdown_number(&race.timings) {
        return Banner::Count(n);
    }
    if matches!(phase, RacePhase::Grid | RacePhase::Results) {
        return Banner::None;
    }
    if progress.finished() {
        return Banner::Finished;
    }
    if race.flow.shows_go(&race.timings) {
        return Banner::Go;
    }
    let on_final_lap = course.laps > 1 && progress.laps_completed(course) + 1 == course.laps;
    let lap_ticks = race.race_ticks().saturating_sub(progress.lap_start);
    if on_final_lap && lap_ticks < seconds_to_ticks(tuning.final_lap_banner_seconds) {
        return Banner::FinalLap;
    }
    Banner::None
}

/// Ticks → hundredths of a second (rounded down).
pub fn hundredths(ticks: u32) -> u32 {
    (ticks as f64 * 100.0 / TICK_HZ) as u32
}

/// French ordinal: "1er", "2e", "12e".
pub fn write_ordinal(out: &mut String, rank: u32) {
    out.clear();
    let _ = write!(out, "{rank}{}", if rank == 1 { "er" } else { "e" });
}

/// "/ 12": number of racers, next to the rank.
pub fn write_racer_count(out: &mut String, racers: u32) {
    out.clear();
    let _ = write!(out, "/ {racers}");
}

/// "Tour 2/3".
pub fn write_lap(out: &mut String, lap: u32, laps: u32) {
    out.clear();
    let _ = write!(out, "Tour {lap}/{laps}");
}

/// A time in hundredths: "1:02.35"; `None`: "-:--.--".
pub fn write_time(out: &mut String, hundredths: Option<u32>) {
    match hundredths {
        Some(h) => {
            let _ = write!(out, "{}:{:02}.{:02}", h / 6000, (h / 100) % 60, h % 100);
        }
        None => out.push_str("-:--.--"),
    }
}

/// "Meilleur tour 0:20.12".
pub fn write_best_lap(out: &mut String, hundredths: Option<u32>) {
    out.clear();
    out.push_str("Meilleur tour ");
    write_time(out, hundredths);
}

/// Croquettes held (GD § 7, issue #14): "x 0".
pub fn write_croquettes(out: &mut String, count: u32) {
    out.clear();
    let _ = write!(out, "x {count}");
}

/// Debug overlay (F3): "FPS 60 | 16.7 ms | Karts 12 | Sim 0.85 ms" (dashes until measured).
pub fn write_debug_overlay(
    out: &mut String,
    fps: Option<f64>,
    frame_ms: Option<f64>,
    karts: usize,
    sim_ms: Option<f64>,
) {
    out.clear();
    let _ = match fps {
        Some(fps) => write!(out, "FPS {fps:.0}"),
        None => write!(out, "FPS -"),
    };
    let _ = match frame_ms {
        Some(ms) => write!(out, " | {ms:.1} ms"),
        None => write!(out, " | - ms"),
    };
    let _ = write!(out, " | Karts {karts}");
    let _ = match sim_ms {
        Some(ms) => write!(out, " | Sim {ms:.2} ms"),
        None => write!(out, " | Sim - ms"),
    };
}

/// Static labels of the HUD and the results screen (issue #100: with their accents).
pub const RESULTS_TITLE: &str = "Classement";
pub const RESULTS_FOOTER: &str = "Espace ou Entrée (manette : A) : on rejoue";
/// Issue #19: in a Grand Prix (issue #99: a championship), the same keys go on to its standings.
pub const RESULTS_FOOTER_GRAND_PRIX: &str = "Espace ou Entrée (manette : A) : classement général";
/// Online (issue #9): the host starts the next race.
pub const RESULTS_FOOTER_ONLINE: &str = "Course suivante : l'hôte la lance depuis le salon (L)";
pub const PLAYER_NAME: &str = "Toi";
/// Issue #70: the results screen, for a racer still on track, one that gave up (online: its
/// player left), and an estimated time (`ESTIMATE_MARK` before it).
pub const RESULTS_RACING: &str = "en course";
pub const RESULTS_GAVE_UP: &str = "abandon";
pub const RESULTS_ESTIMATED: &str = "Temps estimé";
pub const ESTIMATE_MARK: char = '~';

/// What a line of the results screen says of its racer's race (issue #70). Times in hundredths
/// of a second (what is displayed). Compared from frame to frame: a line is rewritten only when
/// it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultsTime {
    /// Still on track, on lap `lap` of `laps`.
    Racing { lap: u32, laps: u32 },
    /// Crossed the line: its time and best lap.
    Finished { time: u32, best_lap: Option<u32> },
    /// Still on track when the results closed: its estimated time.
    Estimated(u32),
    /// Gave up (online: its player left).
    GaveUp,
}

impl ResultsTime {
    /// A racer's line: live from its progress, or its line of the closed results (`closed`).
    pub fn of(progress: &RaceProgress, course: &Course, closed: Option<ResultTime>) -> Self {
        let best_lap = progress.best_lap().map(hundredths);
        match closed.map_or_else(|| progress.finish.map(ResultTime::Finished), Some) {
            Some(ResultTime::Finished(ticks)) => Self::Finished {
                time: hundredths(ticks),
                best_lap,
            },
            Some(ResultTime::Estimated(ticks)) => Self::Estimated(hundredths(ticks)),
            Some(ResultTime::GaveUp) => Self::GaveUp,
            None => Self::Racing {
                lap: progress.current_lap(course),
                laps: course.laps,
            },
        }
    }

    /// The time column: "1:02.35", "en course", "~1:45.20", "abandon".
    pub fn write_time(&self, out: &mut String) {
        out.clear();
        match *self {
            Self::Racing { .. } => out.push_str(RESULTS_RACING),
            Self::Finished { time, .. } => write_time(out, Some(time)),
            Self::Estimated(time) => {
                out.push(ESTIMATE_MARK);
                write_time(out, Some(time));
            }
            Self::GaveUp => out.push_str(RESULTS_GAVE_UP),
        }
    }

    /// The column after it: "Meilleur tour 0:20.12", "Tour 2/3", "Temps estime", nothing.
    pub fn write_detail(&self, out: &mut String) {
        match *self {
            Self::Racing { lap, laps } => write_lap(out, lap, laps),
            Self::Finished { best_lap, .. } => write_best_lap(out, best_lap),
            Self::Estimated(_) => {
                out.clear();
                out.push_str(RESULTS_ESTIMATED);
            }
            Self::GaveUp => out.clear(),
        }
    }
}

/// Name of a racer on the results screen. `name`: another player's (online, issue #9), as typed
/// (issue #100: the HUD's font has its accents: « Éloïse »).
pub fn write_racer_name(out: &mut String, player: bool, grid_slot: usize, name: Option<&str>) {
    out.clear();
    if player {
        out.push_str(PLAYER_NAME);
    } else if let Some(name) = name {
        out.push_str(name);
    } else {
        let _ = write!(out, "Kart {}", grid_slot + 1);
    }
}

/// The characters of Fredoka (`assets/ui/fonts/`, both weights), as ranges of code points (the
/// fonts' `cmap`, read again by a test): Basic Latin, Latin-1 (all of French: « é », « ç », « œ »,
/// « Ÿ », the guillemets), a few letters of Latin Extended-A, the Hebrew alphabet, the dashes,
/// the curly quotes, the ellipsis, « € ».
const FREDOKA: &[(u32, u32)] = &[
    (0x0020, 0x007E),
    (0x00A0, 0x00AC),
    (0x00AE, 0x00FF),
    (0x0131, 0x0131),
    (0x0141, 0x0142),
    (0x0152, 0x0153),
    (0x0160, 0x0161),
    (0x0178, 0x0178),
    (0x017D, 0x017E),
    (0x0192, 0x0192),
    (0x02C6, 0x02C7),
    (0x02D8, 0x02DD),
    (0x0300, 0x0304),
    (0x0306, 0x0308),
    (0x030A, 0x030C),
    (0x0327, 0x0328),
    (0x05B0, 0x05BC),
    (0x05BE, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C7, 0x05C7),
    (0x05D0, 0x05EA),
    (0x05F3, 0x05F4),
    (0x2013, 0x2014),
    (0x2018, 0x201A),
    (0x201C, 0x201E),
    (0x2020, 0x2022),
    (0x2026, 0x2026),
    (0x2030, 0x2030),
    (0x2039, 0x203A),
    (0x2044, 0x2044),
    (0x20AA, 0x20AA),
    (0x20AC, 0x20AC),
    (0x2122, 0x2122),
    (0x2212, 0x2212),
    (0xFB01, 0xFB02),
    (0xFB2A, 0xFB36),
    (0xFB38, 0xFB3C),
    (0xFB3E, 0xFB3E),
    (0xFB40, 0xFB41),
    (0xFB43, 0xFB44),
    (0xFB46, 0xFB4B),
];

/// Issue #100: whether the HUD's font, Fredoka, can draw `c` (never a control character: it has
/// none). The HUD, the screens and the tags only write these; a pseudo keeps only them (an emoji
/// would be a blank box).
pub fn writable(c: char) -> bool {
    let c = c as u32;
    FREDOKA.iter().any(|&(first, last)| first <= c && c <= last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::{FlowTimings, RaceFlow};

    #[test]
    fn ordinals_ranks_and_laps() {
        let mut s = String::new();
        write_ordinal(&mut s, 1);
        assert_eq!(s, "1er");
        write_ordinal(&mut s, 2);
        assert_eq!(s, "2e");
        write_ordinal(&mut s, 12);
        assert_eq!(s, "12e");
        write_racer_count(&mut s, 12);
        assert_eq!(s, "/ 12");
        write_lap(&mut s, 2, 3);
        assert_eq!(s, "Tour 2/3");
        write_croquettes(&mut s, 0);
        assert_eq!(s, "x 0");
        write_croquettes(&mut s, 10);
        assert_eq!(s, "x 10");
    }

    #[test]
    fn times_in_minutes_seconds_hundredths() {
        assert_eq!(hundredths(60), 100);
        assert_eq!(hundredths(1), 1);
        assert_eq!(hundredths(59), 98);
        let mut s = String::new();
        write_time(&mut s, Some(hundredths(60 * 62 + 21)));
        assert_eq!(s, "1:02.35");
        s.clear();
        write_time(&mut s, Some(0));
        assert_eq!(s, "0:00.00");
        write_best_lap(&mut s, None);
        assert_eq!(s, "Meilleur tour -:--.--");
        write_best_lap(&mut s, Some(2012));
        assert_eq!(s, "Meilleur tour 0:20.12");
    }

    #[test]
    fn debug_overlay_shows_fps_frame_time_karts_and_simulation_cost() {
        let mut s = String::with_capacity(64);
        let capacity = s.capacity();
        write_debug_overlay(&mut s, Some(59.6), Some(16.78), 12, Some(0.853));
        assert_eq!(s, "FPS 60 | 16.8 ms | Karts 12 | Sim 0.85 ms");
        write_debug_overlay(&mut s, None, None, 1, None);
        assert_eq!(s, "FPS - | - ms | Karts 1 | Sim - ms");
        write_debug_overlay(&mut s, Some(1234.0), Some(999.99), 12, Some(123.456));
        assert!(s.is_ascii());
        assert_eq!(s.capacity(), capacity, "no new allocation");
    }

    #[test]
    fn rewriting_a_text_does_not_allocate_again() {
        let mut s = String::with_capacity(32);
        let capacity = s.capacity();
        for h in [0, 999, 123_456, 7] {
            write_best_lap(&mut s, Some(h));
            assert_eq!(s.capacity(), capacity);
        }
    }

    /// Issue #70: a line of the results screen, by the state of its racer's race: finished (its
    /// time and best lap), still on track (its lap), estimated once the results closed (marked),
    /// gave up. Rewriting a line does not allocate again.
    #[test]
    fn a_results_line_says_how_its_racers_race_went() {
        let (_, course) = crate::race::course_for_tests();
        let mut progress = RaceProgress::on_grid(&course, course.grid[0].translation, 3);
        assert_eq!(
            ResultsTime::of(&progress, &course, None),
            ResultsTime::Racing { lap: 1, laps: 3 }
        );
        // Two laps done, the third one on.
        let n = course.checkpoint_count();
        for tick in 1..=2 * n + 1 {
            progress.validate(&course, tick * 30);
        }
        let racing = ResultsTime::of(&progress, &course, None);
        assert_eq!(racing, ResultsTime::Racing { lap: 3, laps: 3 });
        let lap1 = progress.lap_times[0];
        let best = hundredths(progress.best_lap().unwrap());
        assert_eq!(best, hundredths(lap1.min(progress.lap_times[1])));
        // Once the results closed: its estimate.
        let estimated = ResultsTime::of(&progress, &course, Some(ResultTime::Estimated(6_321)));
        assert_eq!(estimated, ResultsTime::Estimated(10_535));
        // Then it finishes.
        for tick in 2 * n + 2..=3 * n + 1 {
            progress.validate(&course, tick * 30);
        }
        let finish = progress.finish.unwrap();
        let finished = ResultsTime::of(&progress, &course, None);
        assert_eq!(
            finished,
            ResultsTime::Finished {
                time: hundredths(finish),
                best_lap: Some(best),
            }
        );
        // Its line of the closed results says the same.
        assert_eq!(
            ResultsTime::of(&progress, &course, Some(ResultTime::Finished(finish))),
            finished
        );
        assert_eq!(
            ResultsTime::of(&progress, &course, Some(ResultTime::GaveUp)),
            ResultsTime::GaveUp
        );

        let mut time = String::with_capacity(32);
        let mut detail = String::with_capacity(32);
        let capacity = (time.capacity(), detail.capacity());
        let mut cells = |line: ResultsTime| {
            line.write_time(&mut time);
            line.write_detail(&mut detail);
            assert_eq!((time.capacity(), detail.capacity()), capacity);
            (time.clone(), detail.clone())
        };
        assert_eq!(cells(racing), ("en course".into(), "Tour 3/3".into()));
        assert_eq!(
            cells(ResultsTime::Finished {
                time: 6_235,
                best_lap: Some(2_012)
            }),
            ("1:02.35".into(), "Meilleur tour 0:20.12".into())
        );
        assert_eq!(
            cells(ResultsTime::Finished {
                time: 6_235,
                best_lap: None
            }),
            ("1:02.35".into(), "Meilleur tour -:--.--".into())
        );
        assert_eq!(cells(estimated), ("~1:45.35".into(), "Temps estimé".into()));
        assert_eq!(
            cells(ResultsTime::GaveUp),
            ("abandon".into(), String::new())
        );
    }

    /// Issue #100: the HUD writes in Fredoka, with the accents: every text of the HUD and of the
    /// results is made of its characters, another player's name is written as typed (« Éloïse »,
    /// no longer « Eloise »).
    #[test]
    fn every_text_is_written_in_fredoka_with_its_accents() {
        let mut texts = vec![
            RESULTS_TITLE.to_string(),
            RESULTS_FOOTER.to_string(),
            RESULTS_FOOTER_GRAND_PRIX.to_string(),
            RESULTS_FOOTER_ONLINE.to_string(),
            PLAYER_NAME.to_string(),
            RESULTS_RACING.to_string(),
            RESULTS_GAVE_UP.to_string(),
            RESULTS_ESTIMATED.to_string(),
            ESTIMATE_MARK.to_string(),
        ];
        for banner in [
            Banner::Count(3),
            Banner::Go,
            Banner::FinalLap,
            Banner::Finished,
        ] {
            let mut s = String::new();
            banner.write(&mut s);
            texts.push(s);
        }
        let mut s = String::new();
        write_best_lap(&mut s, Some(5));
        texts.push(s.clone());
        write_racer_name(&mut s, false, 4, None);
        assert_eq!(s, "Kart 5");
        texts.push(s.clone());
        // Issue #9: another player's name; issue #100: with its accents.
        write_racer_name(&mut s, false, 4, Some("Éloïse ça va"));
        assert_eq!(s, "Éloïse ça va");
        texts.push(s.clone());
        write_racer_name(&mut s, true, 4, Some("Max"));
        assert_eq!(s, PLAYER_NAME);
        for text in &texts {
            assert!(text.chars().all(writable), "{text}");
        }
        // The accents are there.
        assert!(RESULTS_FOOTER.contains("Entrée") && RESULTS_ESTIMATED.contains('é'));
        assert!(RESULTS_FOOTER_ONLINE.contains("l'hôte"));
        assert!(RESULTS_FOOTER_GRAND_PRIX.ends_with("classement général"));
    }

    /// The code points of a TrueType font (its `cmap`: the Unicode subtable of format 4).
    fn code_points(font: &[u8]) -> std::collections::BTreeSet<u32> {
        let u16_at = |at: usize| u16::from_be_bytes([font[at], font[at + 1]]);
        let u32_at =
            |at: usize| u32::from_be_bytes([font[at], font[at + 1], font[at + 2], font[at + 3]]);
        let tables = u16_at(4) as usize;
        let cmap = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|at| &font[*at..*at + 4] == b"cmap")
            .map(|at| u32_at(at + 8) as usize)
            .expect("a cmap table");
        let mut points = std::collections::BTreeSet::new();
        for i in 0..u16_at(cmap + 2) as usize {
            let sub = cmap + u32_at(cmap + 4 + 8 * i + 4) as usize;
            if u16_at(sub) != 4 {
                continue;
            }
            let segments = u16_at(sub + 6) as usize / 2;
            for k in 0..segments {
                let end = u16_at(sub + 14 + 2 * k) as u32;
                let start = u16_at(sub + 16 + 2 * segments + 2 * k) as u32;
                if start != 0xFFFF {
                    points.extend(start..=end);
                }
            }
        }
        points
    }

    /// Issue #100: [`writable`] is exactly what both weights of Fredoka can draw (their `cmap`),
    /// all of French among it, and no control character, no emoji.
    #[test]
    fn writable_is_what_fredoka_draws() {
        const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ui/fonts");
        let [medium, semibold] = ["Fredoka-Medium.ttf", "Fredoka-SemiBold.ttf"]
            .map(|name| code_points(&std::fs::read(format!("{FONTS}/{name}")).unwrap()));
        assert!(medium.len() > 300, "{}", medium.len());
        for code in 0..=0xFFFFu32 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let both = medium.contains(&code) && semibold.contains(&code);
            assert_eq!(writable(c), both, "U+{code:04X} {c:?}");
        }
        for c in "àâäçéèêëîïôöùûüÿœæÀÂÄÇÉÈÊËÎÏÔÖÙÛÜŸŒÆ«»’…–—·€".chars()
        {
            assert!(writable(c), "{c}");
        }
        for c in ['\t', '\n', '\u{7}', '\u{7f}', '😀', '✨', '→'] {
            assert!(!writable(c), "{c:?}");
        }
    }

    #[test]
    fn banner_follows_the_race() {
        let (_, course) = crate::race::course_for_tests();
        let tuning = RaceTuning::default();
        let mut race = Race::new(&tuning);
        let t: FlowTimings = race.timings;
        let mut progress = RaceProgress::on_grid(&course, course.grid[0].translation, 3);
        let at = |race: &mut Race, tick: u32| {
            race.flow = RaceFlow {
                tick,
                over_at: None,
            };
        };
        at(&mut race, 10);
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::None);
        at(&mut race, t.grid + 1);
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::Count(3));
        at(&mut race, RaceFlow::two_tick(&t));
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::Count(2));
        at(&mut race, RaceFlow::go_tick(&t));
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::Go);
        at(&mut race, RaceFlow::go_tick(&t) + 200);
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::None);

        // Start of the last lap: "DERNIER TOUR !" for 2 s.
        progress.passed = 2 * course.checkpoint_count() + 1;
        progress.lap_start = race.race_ticks() - 10;
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::FinalLap);
        progress.lap_start = race.race_ticks() - 121;
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::None);

        progress.finish = Some(race.race_ticks());
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::Finished);
        race.flow.over_at = Some(race.flow.tick - t.finish);
        assert_eq!(race.phase(), RacePhase::Results);
        assert_eq!(banner(&race, &course, &tuning, &progress), Banner::None);
    }
}
