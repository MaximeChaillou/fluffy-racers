//! The time trial's records (issue #76, GD § 1): the best time of each circuit and mirror, its
//! laps and its ghost (`ghost.rs`), kept between sessions in the browser's local storage
//! (natively a file of the configuration folder: `admin::storage`), under a key of their own
//! ([`TIME_TRIAL_STORAGE_KEY`]), as a RON text like the options. Pure logic: unit-tested.
//!
//! - **A record per circuit and mirror** ([`TrialKey`]): the circuit by its file (the name may
//!   change, the file stays).
//! - **The time in ticks** (60 Hz), as the race counts it: exact, the same from one machine to
//!   another; shown in hundredths.
//! - **A run beats the record** when it is faster (an equal time keeps the old one). Its ghost
//!   then replaces the old one.
//! - **The text** ([`TrialRecords::to_ron`]): its format ([`RECORDS_FORMAT`]), then each record
//!   with the version of the game that set it and its ghost (its parts, its color, its frames in
//!   base64 and their checksum). An unreadable text (not RON, another format, a ghost whose data
//!   is damaged or does not hold its frames, two records for the same race) is refused with its
//!   reason: the game then starts with no record (and a warning in the console).
//! - **The text of before issue #118** ([`FORMAT_WITH_CLASSES`]), when the game had speed classes
//!   and a record per circuit, class and mirror, is still read: the records of the class the game
//!   is raced in (it was the default one, `Cc150`) are those of their circuit and mirror, the
//!   others are left out (no error). The next record kept writes the text of today.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::ghost::{
    GHOST_MAX_TICKS, GhostTrack, base64_decode, base64_encode, checksum, decode_frames,
    encode_frames,
};
use crate::kart::KartColor;

/// Local storage key (web) / file name (native) of the time trial's records.
pub const TIME_TRIAL_STORAGE_KEY: &str = "fluffy-racers/time-trial";

/// The format of the records' text: a text of another format is not read (but for
/// [`FORMAT_WITH_CLASSES`]).
pub const RECORDS_FORMAT: u32 = 2;

/// The format of the records' text before issue #118: each record had a speed class too (see the
/// module's documentation).
pub const FORMAT_WITH_CLASSES: u32 = 1;

/// The version of the game, kept with each record and its ghost.
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which race a record is for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrialKey {
    /// The circuit's file (an asset path: `tracks/prairie.track.ron`).
    pub circuit: String,
    pub mirror: bool,
}

/// The best run of a race.
#[derive(Clone, Debug, PartialEq)]
pub struct TrialRecord {
    pub key: TrialKey,
    /// Its time (ticks, 60 Hz).
    pub time: u32,
    /// The time of each lap (ticks).
    pub laps: Vec<u32>,
    /// The version of the game that set it.
    pub game: String,
    /// Its ghost (shared with the run that plays it back: never copied).
    pub ghost: Option<Arc<GhostTrack>>,
}

/// What a finished run did against the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// It is the new record.
    pub record: bool,
    /// The record before it (ticks), if there was one.
    pub previous: Option<u32>,
}

/// Every record kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrialRecords {
    records: Vec<TrialRecord>,
}

/// The records' text (see the module's documentation).
#[derive(Serialize, Deserialize)]
struct StoredRecords {
    format: u32,
    records: Vec<StoredRecord>,
}

#[derive(Serialize, Deserialize)]
struct StoredRecord {
    circuit: String,
    /// Only in the text of [`FORMAT_WITH_CLASSES`]: the record's speed class. Never written.
    #[serde(default, skip_serializing, deserialize_with = "read_format_1_class")]
    class: Option<Format1Class>,
    mirror: bool,
    time: u32,
    laps: Vec<u32>,
    game: String,
    ghost: Option<StoredGhost>,
}

/// The speed classes of [`FORMAT_WITH_CLASSES`], as its text names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
enum Format1Class {
    Cc50,
    Cc100,
    /// The class the game is raced in (the default one then): its records are kept.
    Cc150,
    Cc200,
}

/// The class of a record of [`FORMAT_WITH_CLASSES`], written there without `Some(...)`.
fn read_format_1_class<'de, D>(deserializer: D) -> Result<Option<Format1Class>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Format1Class::deserialize(deserializer).map(Some)
}

#[derive(Serialize, Deserialize)]
struct StoredGhost {
    /// Character, kart, wheels, wing.
    loadout: [u8; 4],
    color: KartColor,
    frames: u32,
    /// `ghost::encode_frames`, in base64.
    data: String,
    /// `ghost::checksum` of those bytes.
    check: u32,
}

impl TrialRecords {
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TrialRecord> {
        self.records.iter()
    }

    /// The record of `key`.
    pub fn get(&self, key: &TrialKey) -> Option<&TrialRecord> {
        self.records.iter().find(|record| record.key == *key)
    }

    /// What a run of `time` ticks on `key` does: the new record if it is faster than the one
    /// kept, or if there is none.
    pub fn judge(&self, key: &TrialKey, time: u32) -> Verdict {
        let previous = self.get(key).map(|record| record.time);
        Verdict {
            record: previous.is_none_or(|best| time < best),
            previous,
        }
    }

    /// Keeps `record` as the record of its race (the one before it goes).
    pub fn insert(&mut self, record: TrialRecord) {
        match self.records.iter_mut().find(|r| r.key == record.key) {
            Some(old) => *old = record,
            None => self.records.push(record),
        }
    }

    /// The text kept in the browser.
    pub fn to_ron(&self) -> String {
        let stored = StoredRecords {
            format: RECORDS_FORMAT,
            records: self
                .records
                .iter()
                .map(|record| StoredRecord {
                    circuit: record.key.circuit.clone(),
                    class: None,
                    mirror: record.key.mirror,
                    time: record.time,
                    laps: record.laps.clone(),
                    game: record.game.clone(),
                    ghost: record.ghost.as_ref().map(|ghost| {
                        let bytes = encode_frames(&ghost.frames);
                        StoredGhost {
                            loadout: ghost.loadout,
                            color: ghost.color,
                            frames: ghost.frames.len() as u32,
                            check: checksum(&bytes),
                            data: base64_encode(&bytes),
                        }
                    }),
                })
                .collect(),
        };
        ron::ser::to_string_pretty(&stored, ron::ser::PrettyConfig::default())
            .unwrap_or_else(|_| String::new())
    }

    /// Reads a kept text; an unreadable one gives its reason (see the module's documentation).
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let stored: StoredRecords = ron::from_str(text).map_err(|error| error.to_string())?;
        let with_classes = match stored.format {
            RECORDS_FORMAT => false,
            FORMAT_WITH_CLASSES => true,
            format => {
                return Err(format!(
                    "records of format {format} (this game reads {RECORDS_FORMAT}, and \
                     {FORMAT_WITH_CLASSES})"
                ));
            }
        };
        let mut records = Self::default();
        for record in stored.records {
            let key = TrialKey {
                circuit: record.circuit,
                mirror: record.mirror,
            };
            let name = format!(
                "{}{}",
                key.circuit,
                if key.mirror { " mirrored" } else { "" }
            );
            match (with_classes, record.class) {
                (false, None) | (true, Some(Format1Class::Cc150)) => {}
                // A record of another class (format 1): left out.
                (true, Some(_)) => continue,
                (true, None) => return Err(format!("{name}: no speed class (format 1)")),
                (false, Some(_)) => {
                    return Err(format!("{name}: a speed class (format {RECORDS_FORMAT})"));
                }
            }
            if records.get(&key).is_some() {
                return Err(format!("two records for {name}"));
            }
            if record.time == 0 {
                return Err(format!("{name}: a time of 0"));
            }
            let ghost = match record.ghost {
                None => None,
                Some(ghost) => {
                    let count = ghost.frames as usize;
                    if count == 0 || count > GHOST_MAX_TICKS {
                        return Err(format!("{name}: a ghost of {count} frames"));
                    }
                    let bytes =
                        base64_decode(&ghost.data).map_err(|error| format!("{name}: {error}"))?;
                    if checksum(&bytes) != ghost.check {
                        return Err(format!("{name}: the ghost's data is damaged"));
                    }
                    let frames =
                        decode_frames(&bytes, count).map_err(|error| format!("{name}: {error}"))?;
                    Some(Arc::new(GhostTrack {
                        frames,
                        loadout: ghost.loadout,
                        color: ghost.color,
                    }))
                }
            };
            records.records.push(TrialRecord {
                key,
                time: record.time,
                laps: record.laps,
                game: record.game,
                ghost,
            });
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::ghost::tests::run;

    fn key(circuit: &str, mirror: bool) -> TrialKey {
        TrialKey {
            circuit: circuit.to_string(),
            mirror,
        }
    }

    fn record(key: TrialKey, time: u32, ghost_ticks: Option<u32>) -> TrialRecord {
        TrialRecord {
            key,
            time,
            laps: vec![time / 3, time / 3, time - 2 * (time / 3)],
            game: GAME_VERSION.to_string(),
            ghost: ghost_ticks.map(|ticks| {
                Arc::new(GhostTrack {
                    frames: run(ticks),
                    loadout: [3, 1, 2, 0],
                    color: KartColor::ALL[4],
                })
            }),
        }
    }

    /// The text of `records` as the game wrote it before issue #118 ([`FORMAT_WITH_CLASSES`]):
    /// each record with its class (`classes`, in order: `Cc150`...) on its own line, between its
    /// circuit and its mirror.
    fn format_1_text(records: &TrialRecords, classes: &[&str]) -> String {
        let text = records.to_ron();
        assert_eq!(text.matches("mirror: ").count(), classes.len());
        let mut old = String::new();
        let mut classes = classes.iter();
        for line in text.split_inclusive('\n') {
            if let Some(field) = line.find("mirror: ") {
                let class = classes.next().unwrap();
                old.push_str(&format!("{}class: {class},\n", &line[..field]));
            }
            old.push_str(line);
        }
        old.replace(
            &format!("format: {RECORDS_FORMAT},"),
            &format!("format: {FORMAT_WITH_CLASSES},"),
        )
    }

    /// A record per circuit and mirror; a faster run is the new record, an equal or slower one is
    /// not.
    #[test]
    fn a_faster_run_is_the_new_record_of_its_race_only() {
        let mut records = TrialRecords::default();
        let prairie = key("tracks/prairie.track.ron", false);
        assert_eq!(
            records.judge(&prairie, 4000),
            Verdict {
                record: true,
                previous: None
            }
        );
        records.insert(record(prairie.clone(), 4000, None));
        let slower = records.judge(&prairie, 4001);
        assert_eq!((slower.record, slower.previous), (false, Some(4000)));
        assert!(!records.judge(&prairie, 4000).record, "an equal time");
        assert!(records.judge(&prairie, 3999).record);
        for other in [
            key("tracks/prairie.track.ron", true),
            key("tracks/col.track.ron", false),
        ] {
            assert_eq!(records.judge(&other, 9999).previous, None, "{other:?}");
        }
        records.insert(record(prairie.clone(), 3500, None));
        assert_eq!(records.len(), 1);
        assert_eq!(records.get(&prairie).unwrap().time, 3500);
    }

    /// Issue #76 acceptance: the best times and their ghosts survive a round trip through the
    /// stored text, frame for frame; the version of the game goes with them. Issue #118: the
    /// text has no speed class.
    #[test]
    fn records_and_ghosts_survive_a_round_trip_through_the_storage_text() {
        let mut records = TrialRecords::default();
        records.insert(record(
            key("tracks/prairie.track.ron", false),
            3720,
            Some(3720),
        ));
        records.insert(record(key("tracks/col.track.ron", true), 5100, Some(900)));
        records.insert(record(key("tracks/lagon.track.ron", false), 8000, None));
        let text = records.to_ron();
        let back = TrialRecords::from_ron(&text).unwrap();
        assert_eq!(back, records);
        for (a, b) in records.iter().zip(back.iter()) {
            assert_eq!(
                a.ghost.as_ref().map(|g| &g.frames),
                b.ghost.as_ref().map(|g| &g.frames)
            );
        }
        assert!(text.contains("format: 2"), "{}", &text[..200]);
        assert!(text.contains(&format!("game: \"{GAME_VERSION}\"")));
        assert!(!text.contains("class"));
        // A 62 s ghost (3 laps of Prairie Pompon) in about 100 kB of text.
        let size = TrialRecords {
            records: records.records[..1].to_vec(),
        }
        .to_ron()
        .len();
        assert!(size < 120_000, "{size} bytes");
    }

    /// Issue #118 acceptance: the records kept before it (format 1: a record per circuit, class
    /// and mirror) are still read. Those of the class the game is raced in (`Cc150`, its default)
    /// come back as the records of their circuit and mirror, times, laps, version and ghosts
    /// frame for frame; those of the other classes are left out, without an error (even when
    /// they are the only ones, or share a race with a `Cc150` one). Kept again, they are written
    /// in the text of today.
    #[test]
    fn the_records_of_before_issue_118_are_read_in_the_default_class_only() {
        let mut kept = TrialRecords::default();
        kept.insert(record(
            key("tracks/prairie.track.ron", false),
            3720,
            Some(3720),
        ));
        kept.insert(record(key("tracks/col.track.ron", true), 5100, Some(900)));
        let mut before = kept.clone();
        before.records.extend([
            // The same race as a kept one, in another class (and faster).
            record(key("tracks/prairie.track.ron", false), 3000, Some(3000)),
            record(key("tracks/lagon.track.ron", false), 8000, None),
            record(key("tracks/col.track.ron", false), 6000, Some(500)),
        ]);
        let old = format_1_text(&before, &["Cc150", "Cc150", "Cc200", "Cc50", "Cc100"]);
        assert!(old.contains("format: 1") && old.contains("class: Cc200"));
        let read = TrialRecords::from_ron(&old).unwrap();
        assert_eq!(read, kept);
        for (a, b) in kept.iter().zip(read.iter()) {
            assert_eq!(
                a.ghost.as_ref().map(|g| &g.frames),
                b.ghost.as_ref().map(|g| &g.frames)
            );
        }
        assert_eq!(TrialRecords::from_ron(&read.to_ron()), Ok(kept.clone()));
        assert!(read.to_ron().contains("format: 2"));
        // Only other classes: no record, no error.
        let others = format_1_text(&before, &["Cc50", "Cc100", "Cc200", "Cc200", "Cc50"]);
        assert_eq!(TrialRecords::from_ron(&others), Ok(TrialRecords::default()));
        // A text written by the game of issue #76 (a record without ghost), by hand.
        let by_hand = r#"(
    format: 1,
    records: [
        (
            circuit: "tracks/anneau.track.ron",
            class: Cc150,
            mirror: true,
            time: 2400,
            laps: [
                800,
                790,
                810,
            ],
            game: "0.1.0",
            ghost: None,
        ),
        (
            circuit: "tracks/anneau.track.ron",
            class: Cc200,
            mirror: true,
            time: 2000,
            laps: [
                700,
                650,
                650,
            ],
            game: "0.1.0",
            ghost: None,
        ),
    ],
)"#;
        let read = TrialRecords::from_ron(by_hand).unwrap();
        assert_eq!(read.len(), 1);
        let anneau = read.get(&key("tracks/anneau.track.ron", true)).unwrap();
        assert_eq!(
            (
                anneau.time,
                &anneau.laps,
                anneau.game.as_str(),
                &anneau.ghost
            ),
            (2400, &vec![800, 790, 810], "0.1.0", &None)
        );
    }

    /// An unreadable text is refused with its reason (the game keeps no record then): not RON,
    /// another format, a broken ghost, two records for the same race, a time of 0; issue #118: a
    /// class in the text of today, none or an unknown one in the text of before.
    #[test]
    fn unreadable_records_are_refused_with_a_reason() {
        let mut records = TrialRecords::default();
        records.insert(record(
            key("tracks/prairie.track.ron", false),
            700,
            Some(700),
        ));
        let good = records.to_ron();
        assert!(TrialRecords::from_ron(&good).is_ok());
        let data_start = good.find("data: \"").unwrap() + 7;
        let mut broken_data = good.clone();
        broken_data.replace_range(data_start..data_start + 4, "!!!!");
        let mut cut_data = good.clone();
        cut_data.replace_range(data_start..data_start + 8, "");
        let more_frames = good.replace("frames: 701", "frames: 702");
        let fewer_frames = good.replace("frames: 701", "frames: 700");
        let no_frames = good.replace("frames: 701", "frames: 0");
        let other_format = good.replace("format: 2", "format: 3");
        let check_start = good.find("check: ").unwrap() + 7;
        let mut bad_check = good.clone();
        let digit = &good[check_start..check_start + 1];
        bad_check.replace_range(
            check_start..check_start + 1,
            if digit == "1" { "2" } else { "1" },
        );
        let zero_time = good.replace("time: 700", "time: 0");
        let twice = {
            let mut twice = records.clone();
            twice.records.push(twice.records[0].clone());
            twice.to_ron()
        };
        let old = format_1_text(&records, &["Cc150"]);
        assert!(TrialRecords::from_ron(&old).is_ok());
        let twice_before = {
            let mut twice = records.clone();
            twice.records.push(twice.records[0].clone());
            format_1_text(&twice, &["Cc150", "Cc150"])
        };
        for (what, bad) in [
            ("empty", String::new()),
            ("not RON", "n'importe quoi".to_string()),
            ("no records", "(format: 2)".to_string()),
            ("broken data", broken_data),
            ("cut data", cut_data),
            ("more frames", more_frames),
            ("fewer frames", fewer_frames),
            ("no frames", no_frames),
            ("other format", other_format),
            ("bad check", bad_check),
            ("zero time", zero_time),
            ("twice", twice),
            ("a class today", old.replace("format: 1", "format: 2")),
            ("bad class before", old.replace("Cc150", "Cc999")),
            ("no class before", good.replace("format: 2", "format: 1")),
            ("twice before", twice_before),
            ("zero time before", old.replace("time: 700", "time: 0")),
        ] {
            let result = TrialRecords::from_ron(&bad);
            assert!(result.is_err(), "{what}: {result:?}");
            assert!(!result.unwrap_err().is_empty());
        }
        for format in [1, 2] {
            assert_eq!(
                TrialRecords::from_ron(&format!("(format: {format}, records: [])")),
                Ok(TrialRecords::default())
            );
        }
    }
}
