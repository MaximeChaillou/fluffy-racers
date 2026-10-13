//! HUD texts (pure): what to show, written into reused strings (no allocation once their
//! capacity is reached).
//!
//! Everything is plain ASCII: Bevy's built-in font has no accented letters (a CC0 font will
//! come with the assets, issue #11), hence "FINI !" and "ENTREE".

use std::fmt::Write;

use crate::TICK_HZ;
use crate::race::{Course, Race, RacePhase, RaceProgress, RaceTuning, seconds_to_ticks};

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
pub const PLAYER_NAME: &str = "Toi";

/// Name of a racer on the results screen.
pub fn write_racer_name(out: &mut String, player: bool, grid_slot: usize) {
    out.clear();
    if player {
        out.push_str(PLAYER_NAME);
    } else {
        let _ = write!(out, "Kart {}", grid_slot + 1);
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

    /// The built-in font only has ASCII glyphs.
    #[test]
    fn every_text_is_ascii() {
        let mut texts = vec![
            RESULTS_TITLE.to_string(),
            RESULTS_FOOTER.to_string(),
            PLAYER_NAME.to_string(),
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
        write_racer_name(&mut s, false, 4);
        assert_eq!(s, "Kart 5");
        texts.push(s.clone());
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
