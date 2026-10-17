//! The time trial's records (issue #76, GD § 1): the best time of each circuit, speed class and
//! mirror, its laps and its ghost (`ghost.rs`), kept between sessions in the browser's local
//! storage (natively a file of the configuration folder: `admin::storage`), under a key of their
//! own ([`TIME_TRIAL_STORAGE_KEY`]), as a RON text like the options. Pure logic: unit-tested.
//!
//! - **A record per circuit, class and mirror** ([`TrialKey`]): the circuit by its file (the
//!   name may change, the file stays).
//! - **The time in ticks** (60 Hz), as the race counts it: exact, the same from one machine to
//!   another; shown in hundredths.
//! - **A run beats the record** when it is faster (an equal time keeps the old one). Its ghost
//!   then replaces the old one.
//! - **The text** ([`TrialRecords::to_ron`]): its format ([`RECORDS_FORMAT`]), then each record
//!   with the version of the game that set it and its ghost (its parts, its color, its frames in
//!   base64 and their checksum). An unreadable text (not RON, another format, a ghost whose data
//!   is damaged or does not hold its frames, two records for the same race) is refused with its
//!   reason: the game then starts with no record (and a warning in the console).

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::ghost::{
    GHOST_MAX_TICKS, GhostTrack, base64_decode, base64_encode, checksum, decode_frames,
    encode_frames,
};
use crate::kart::{KartColor, SpeedClass};

/// Local storage key (web) / file name (native) of the time trial's records.
pub const TIME_TRIAL_STORAGE_KEY: &str = "fluffy-racers/time-trial";

/// The format of the records' text: a text of another format is not read.
pub const RECORDS_FORMAT: u32 = 1;

/// The version of the game, kept with each record and its ghost.
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which race a record is for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrialKey {
    /// The circuit's file (an asset path: `tracks/prairie.track.ron`).
    pub circuit: String,
    pub class: SpeedClass,
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
    class: SpeedClass,
    mirror: bool,
    time: u32,
    laps: Vec<u32>,
    game: String,
    ghost: Option<StoredGhost>,
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
                    class: record.key.class,
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
        if stored.format != RECORDS_FORMAT {
            return Err(format!(
                "records of format {} (this game reads {RECORDS_FORMAT})",
                stored.format
            ));
        }
        let mut records = Self::default();
        for record in stored.records {
            let key = TrialKey {
                circuit: record.circuit,
                class: record.class,
                mirror: record.mirror,
            };
            let name = format!(
                "{} {}{}",
                key.circuit,
                key.class.label(),
                if key.mirror { " mirrored" } else { "" }
            );
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

    fn key(circuit: &str, class: SpeedClass, mirror: bool) -> TrialKey {
        TrialKey {
            circuit: circuit.to_string(),
            class,
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

    /// A record per circuit, class and mirror; a faster run is the new record, an equal or slower
    /// one is not.
    #[test]
    fn a_faster_run_is_the_new_record_of_its_race_only() {
        let mut records = TrialRecords::default();
        let prairie = key("tracks/prairie.track.ron", SpeedClass::Cc150, false);
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
            key("tracks/prairie.track.ron", SpeedClass::Cc200, false),
            key("tracks/prairie.track.ron", SpeedClass::Cc150, true),
            key("tracks/col.track.ron", SpeedClass::Cc150, false),
        ] {
            assert_eq!(records.judge(&other, 9999).previous, None, "{other:?}");
        }
        records.insert(record(prairie.clone(), 3500, None));
        assert_eq!(records.len(), 1);
        assert_eq!(records.get(&prairie).unwrap().time, 3500);
    }

    /// Issue #76 acceptance: the best times and their ghosts survive a round trip through the
    /// stored text, frame for frame; the version of the game goes with them.
    #[test]
    fn records_and_ghosts_survive_a_round_trip_through_the_storage_text() {
        let mut records = TrialRecords::default();
        records.insert(record(
            key("tracks/prairie.track.ron", SpeedClass::Cc150, false),
            3720,
            Some(3720),
        ));
        records.insert(record(
            key("tracks/col.track.ron", SpeedClass::Cc200, true),
            5100,
            Some(900),
        ));
        records.insert(record(
            key("tracks/lagon.track.ron", SpeedClass::Cc50, false),
            8000,
            None,
        ));
        let text = records.to_ron();
        let back = TrialRecords::from_ron(&text).unwrap();
        assert_eq!(back, records);
        for (a, b) in records.iter().zip(back.iter()) {
            assert_eq!(
                a.ghost.as_ref().map(|g| &g.frames),
                b.ghost.as_ref().map(|g| &g.frames)
            );
        }
        assert!(text.contains("format: 1"), "{}", &text[..200]);
        assert!(text.contains(&format!("game: \"{GAME_VERSION}\"")));
        assert!(text.contains("class: Cc200"));
        // A 62 s ghost (3 laps of Prairie Pompon) in about 100 kB of text.
        let size = TrialRecords {
            records: records.records[..1].to_vec(),
        }
        .to_ron()
        .len();
        assert!(size < 120_000, "{size} bytes");
    }

    /// An unreadable text is refused with its reason (the game keeps no record then): not RON,
    /// another format, a broken ghost, two records for the same race, a time of 0.
    #[test]
    fn unreadable_records_are_refused_with_a_reason() {
        let mut records = TrialRecords::default();
        records.insert(record(
            key("tracks/prairie.track.ron", SpeedClass::Cc150, false),
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
        let other_format = good.replace("format: 1", "format: 2");
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
        for (what, bad) in [
            ("empty", String::new()),
            ("not RON", "n'importe quoi".to_string()),
            ("no records", "(format: 1)".to_string()),
            ("bad class", good.replace("Cc150", "Cc999")),
            ("broken data", broken_data),
            ("cut data", cut_data),
            ("more frames", more_frames),
            ("fewer frames", fewer_frames),
            ("no frames", no_frames),
            ("other format", other_format),
            ("bad check", bad_check),
            ("zero time", zero_time),
            ("twice", twice),
        ] {
            let result = TrialRecords::from_ron(&bad);
            assert!(result.is_err(), "{what}: {result:?}");
            assert!(!result.unwrap_err().is_empty());
        }
        assert_eq!(
            TrialRecords::from_ron("(format: 1, records: [])"),
            Ok(TrialRecords::default())
        );
    }
}
