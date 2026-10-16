//! HUD texts (pure): what to show, written into reused strings (no allocation once their
//! capacity is reached).
//!
//! Everything is plain ASCII: Bevy's built-in font has no accented letters (a CC0 font will
//! come with the assets, issue #11), hence "FINI !" and "ENTREE".

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

/// Static labels of the HUD and the results screen.
pub const RESULTS_TITLE: &str = "CLASSEMENT";
pub const RESULTS_FOOTER: &str = "ESPACE ou ENTREE (manette : A) : rejouer";
/// Issue #19: in a Grand Prix, the same keys go on to its standings.
pub const RESULTS_FOOTER_GRAND_PRIX: &str = "ESPACE ou ENTREE (manette : A) : Grand Prix";
/// Online (issue #9): the host starts the next race.
pub const RESULTS_FOOTER_ONLINE: &str = "Course suivante : l'hote la lance depuis le salon (L)";
pub const PLAYER_NAME: &str = "Toi";
/// Issue #70: the results screen, for a racer still on track, one that gave up (online: its
/// player left), and an estimated time (`ESTIMATE_MARK` before it).
pub const RESULTS_RACING: &str = "en course";
pub const RESULTS_GAVE_UP: &str = "abandon";
pub const RESULTS_ESTIMATED: &str = "Temps estime";
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

/// Name of a racer on the results screen. `name`: another player's (online, issue #9), written
/// in the HUD's font: its accented letters without their accents, any other letter it lacks as
/// "?".
pub fn write_racer_name(out: &mut String, player: bool, grid_slot: usize, name: Option<&str>) {
    out.clear();
    if player {
        out.push_str(PLAYER_NAME);
    } else if let Some(name) = name {
        out.extend(name.chars().map(ascii_letter));
    } else {
        let _ = write!(out, "Kart {}", grid_slot + 1);
    }
}

/// A letter the HUD's font has (ASCII): the French accented ones lose their accent.
pub fn ascii_letter(c: char) -> char {
    match c {
        ' '..='~' => c,
        'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
        'À' | 'Â' | 'Ä' | 'Á' | 'Ã' | 'Å' => 'A',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'î' | 'ï' | 'í' | 'ì' => 'i',
        'Î' | 'Ï' | 'Í' | 'Ì' => 'I',
        'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
        'Ô' | 'Ö' | 'Ó' | 'Ò' | 'Õ' => 'O',
        'ù' | 'û' | 'ü' | 'ú' => 'u',
        'Ù' | 'Û' | 'Ü' | 'Ú' => 'U',
        'ç' => 'c',
        'Ç' => 'C',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ÿ' => 'y',
        _ => '?',
    }
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
        assert_eq!(cells(estimated), ("~1:45.35".into(), "Temps estime".into()));
        assert_eq!(
            cells(ResultsTime::GaveUp),
            ("abandon".into(), String::new())
        );
    }

    /// The built-in font only has ASCII glyphs.
    #[test]
    fn every_text_is_ascii() {
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
        // Issue #9: another player's name, its accents dropped.
        write_racer_name(&mut s, false, 4, Some("Éloïse ça va ✨"));
        assert_eq!(s, "Eloise ca va ?");
        texts.push(s.clone());
        write_racer_name(&mut s, true, 4, Some("Max"));
        assert_eq!(s, PLAYER_NAME);
        for text in texts {
            assert!(text.is_ascii(), "{text}");
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
