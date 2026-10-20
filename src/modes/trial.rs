//! The time trial's records (issue #76, GD § 1): the best time of each circuit and mirror, its
//! laps and its ghost (`ghost.rs`), kept between sessions in the browser's local storage
//! (natively a file of the configuration folder: `admin::storage`), under a key of their own
//! ([`TIME_TRIAL_STORAGE_KEY`]), as a RON text like the options. Pure logic: unit-tested.
//!
//! - **A record per circuit and mirror** ([`TrialKey`]): the circuit by its file (the name may
//!   change, the file stays), **and its fingerprint** (issue #157, `crate::track::fingerprint`:
//!   its file and its biome's data). **A circuit that changed, even only its decor, has no
//!   record**: the one kept with another fingerprint is not read (no time, no ghost, no message),
//!   and the next record of that circuit and mirror replaces it, in the storage too. A circuit
//!   changed back finds its record again (it is the same circuit). Nothing to do by hand when a
//!   circuit changes.
//! - **The time in ticks** (60 Hz), as the race counts it: exact, the same from one machine to
//!   another; shown in hundredths.
//! - **A run beats the record** when it is faster (an equal time keeps the old one). Its ghost
//!   then replaces the old one.
//! - **The text** ([`TrialRecords::to_ron`]): its format ([`RECORDS_FORMAT`]), then each record
//!   with its circuit's fingerprint, the version of the game that set it and its ghost (its
//!   parts, its color, its frames in base64 and their checksum). An unreadable text (not RON,
//!   another format, a record without fingerprint, a ghost whose data is damaged or does not hold
//!   its frames, two records for the same race) is refused with its reason: the game then starts
//!   with no record (and a warning in the console).
//! - **The texts of before issue #157** (formats 1 to 4: records without fingerprint, kept by
//!   circuit, then by speed class until #118, and left out by hand for the circuits redrawn by
//!   #108 and #109) are ignored, without an error: nothing tells on which version of its circuit
//!   a record was set. The next record kept writes the text of today, without them.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::ghost::{
    GHOST_MAX_TICKS, GhostTrack, base64_decode, base64_encode, checksum, decode_frames,
    encode_frames,
};
use crate::kart::KartColor;

/// Local storage key (web) / file name (native) of the time trial's records.
pub const TIME_TRIAL_STORAGE_KEY: &str = "fluffy-racers/time-trial";

/// The format of the records' text (issue #157: 5, each record with its circuit's fingerprint).
/// A text of an earlier format (1 to 4) is ignored without an error, one of another format is
/// refused.
pub const RECORDS_FORMAT: u32 = 5;

/// The version of the game, kept with each record and its ghost.
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which race a record is for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrialKey {
    /// The circuit's file (an asset path: `tracks/prairie.track.ron`).
    pub circuit: String,
    pub mirror: bool,
    /// Issue #157: the circuit's fingerprint (`TrackData::fingerprint`): a record of another
    /// fingerprint is the record of another version of the circuit.
    pub fingerprint: u64,
}

impl TrialKey {
    /// The same circuit and mirror, whatever their versions.
    fn same_race(&self, other: &Self) -> bool {
        self.circuit == other.circuit && self.mirror == other.mirror
    }
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

/// Every record kept: one per circuit and mirror, of the fingerprint the circuit had when it was
/// set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrialRecords {
    records: Vec<TrialRecord>,
}

/// Only the format of a kept text: read first, to ignore the texts of before #157 whatever their
/// shape.
#[derive(Deserialize)]
struct StoredFormat {
    format: u32,
}

/// The records' text (see the module's documentation).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecords {
    format: u32,
    records: Vec<StoredRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRecord {
    circuit: String,
    /// Issue #157: its circuit's fingerprint when it was set.
    fingerprint: u64,
    mirror: bool,
    time: u32,
    laps: Vec<u32>,
    game: String,
    ghost: Option<StoredGhost>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

    /// The record of `key`: none if the one of its circuit and mirror was set on another version
    /// of the circuit (another fingerprint).
    pub fn get(&self, key: &TrialKey) -> Option<&TrialRecord> {
        self.records.iter().find(|record| record.key == *key)
    }

    /// What a run of `time` ticks on `key` does: the new record if it is faster than the one
    /// kept, or if there is none (of this version of the circuit).
    pub fn judge(&self, key: &TrialKey, time: u32) -> Verdict {
        let previous = self.get(key).map(|record| record.time);
        Verdict {
            record: previous.is_none_or(|best| time < best),
            previous,
        }
    }

    /// Keeps `record` as the record of its circuit and mirror: the one before it goes, even one
    /// of another fingerprint (issue #157: the record of a circuit that changed since leaves the
    /// storage with the first record of its new version).
    pub fn insert(&mut self, record: TrialRecord) {
        match self
            .records
            .iter_mut()
            .find(|r| r.key.same_race(&record.key))
        {
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
                    fingerprint: record.key.fingerprint,
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

    /// Reads a kept text; an unreadable one gives its reason, one of before #157 gives no record
    /// (see the module's documentation).
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let StoredFormat { format } = ron::from_str(text).map_err(|error| error.to_string())?;
        match format {
            RECORDS_FORMAT => {}
            // Issue #157: records without fingerprint, ignored.
            1..RECORDS_FORMAT => return Ok(Self::default()),
            format => {
                return Err(format!(
                    "records of format {format} (this game reads {RECORDS_FORMAT}, and ignores \
                     1 to {})",
                    RECORDS_FORMAT - 1
                ));
            }
        }
        let stored: StoredRecords = ron::from_str(text).map_err(|error| error.to_string())?;
        let mut records = Self::default();
        for record in stored.records {
            let key = TrialKey {
                circuit: record.circuit,
                mirror: record.mirror,
                fingerprint: record.fingerprint,
            };
            let name = format!(
                "{}{}",
                key.circuit,
                if key.mirror { " mirrored" } else { "" }
            );
            if records.records.iter().any(|r| r.key.same_race(&key)) {
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
    use crate::track::TrackData;

    /// A fingerprint for the tests: one per circuit (its name hashed), as if each had its own.
    fn fingerprint_of(circuit: &str) -> u64 {
        crate::track::fingerprint::fnv1a_64(circuit.as_bytes())
    }

    fn key(circuit: &str, mirror: bool) -> TrialKey {
        TrialKey {
            circuit: circuit.to_string(),
            mirror,
            fingerprint: fingerprint_of(circuit),
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

    /// The text of `records` as the game wrote it before issue #157 (format 2, 3 or 4): the same,
    /// without the fingerprints.
    fn text_before_157(records: &TrialRecords, format: u32) -> String {
        let mut old = String::new();
        for line in records.to_ron().split_inclusive('\n') {
            if !line.trim_start().starts_with("fingerprint: ") {
                old.push_str(line);
            }
        }
        assert!(!old.contains("fingerprint"));
        old.replace(
            &format!("format: {RECORDS_FORMAT},"),
            &format!("format: {format},"),
        )
    }

    /// The text of `records` as the game wrote it before issue #118 (format 1): without the
    /// fingerprints, each record with its class (`classes`, in order: `Cc150`...) on its own line,
    /// between its circuit and its mirror.
    fn format_1_text(records: &TrialRecords, classes: &[&str]) -> String {
        let text = text_before_157(records, 1);
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
        old
    }

    /// A record per circuit and mirror; a faster run is the new record, an equal or slower one is
    /// not. Issue #157: another version of the circuit (another fingerprint) has no record.
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
        let changed = TrialKey {
            fingerprint: prairie.fingerprint ^ 1,
            ..prairie.clone()
        };
        for other in [
            key("tracks/prairie.track.ron", true),
            key("tracks/col.track.ron", false),
            changed,
        ] {
            assert_eq!(records.judge(&other, 9999).previous, None, "{other:?}");
            assert!(records.get(&other).is_none(), "{other:?}");
        }
        records.insert(record(prairie.clone(), 3500, None));
        assert_eq!(records.len(), 1);
        assert_eq!(records.get(&prairie).unwrap().time, 3500);
    }

    /// Issue #76 acceptance: the best times and their ghosts survive a round trip through the
    /// stored text, frame for frame; the version of the game goes with them. Issue #118: the
    /// text has no speed class. Issue #157: each record with its circuit's fingerprint.
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
        assert!(
            text.contains(&format!("format: {RECORDS_FORMAT}")),
            "{}",
            &text[..200]
        );
        assert!(text.contains(&format!("game: \"{GAME_VERSION}\"")));
        assert!(!text.contains("class"));
        for circuit in ["prairie", "col", "lagon"] {
            let fingerprint = fingerprint_of(&format!("tracks/{circuit}.track.ron"));
            assert!(
                text.contains(&format!("fingerprint: {fingerprint},")),
                "{circuit}"
            );
        }
        // A 62 s ghost (3 laps of Prairie Pompon) in about 100 kB of text.
        let size = TrialRecords {
            records: records.records[..1].to_vec(),
        }
        .to_ron()
        .len();
        assert!(size < 120_000, "{size} bytes");
    }

    /// Issue #157 acceptance (replaces the tests of #108 and #109, which left out by hand the
    /// records of the circuits they redrawn, kept in a list with a format each: now, any change of
    /// a circuit does it, and nothing is kept by hand). **A changed circuit has no record**: one
    /// piece of decor of Prairie Pompon moved (its real file), its fingerprint changes, and its
    /// records (mirrored or not) are no longer read, neither in memory nor after a reload of the
    /// stored text: no time, no ghost, the first run is the first record. The other circuits keep
    /// theirs, ghosts frame for frame. **Nothing changed: the record stays**, after a reload too.
    /// The first record of the new version replaces the old one, which leaves the stored text.
    #[test]
    fn a_changed_circuit_has_no_record() {
        let text = include_str!("../../assets/tracks/prairie.track.ron");
        let before = TrackData::from_ron(text).unwrap().fingerprint;
        let piece = "(model: \"racing-kit/flagCheckers\", at: 0.0,";
        assert_eq!(text.matches(piece).count(), 1);
        let moved = text.replace(piece, "(model: \"racing-kit/flagCheckers\", at: 0.5,");
        let after = TrackData::from_ron(&moved).unwrap().fingerprint;
        assert_ne!(after, before, "one piece of decor moved");
        let prairie = |mirror: bool, fingerprint: u64| TrialKey {
            circuit: "tracks/prairie.track.ron".into(),
            mirror,
            fingerprint,
        };

        let mut all = TrialRecords::default();
        all.insert(record(prairie(false, before), 3720, Some(3720)));
        all.insert(record(prairie(true, before), 3900, None));
        all.insert(record(
            key("tracks/crique.track.ron", true),
            5100,
            Some(900),
        ));
        let reloaded = TrialRecords::from_ron(&all.to_ron()).unwrap();
        assert_eq!(reloaded, all);
        for records in [&all, &reloaded] {
            // Nothing changed: the records, ghost included.
            assert_eq!(records.get(&prairie(false, before)).unwrap().time, 3720);
            assert_eq!(
                records
                    .get(&prairie(false, before))
                    .unwrap()
                    .ghost
                    .as_ref()
                    .map(|g| &g.frames),
                all.records[0].ghost.as_ref().map(|g| &g.frames)
            );
            assert_eq!(records.get(&prairie(true, before)).unwrap().time, 3900);
            // Changed: none, mirrored or not.
            for mirror in [false, true] {
                assert!(records.get(&prairie(mirror, after)).is_none());
                assert_eq!(
                    records.judge(&prairie(mirror, after), 99_999),
                    Verdict {
                        record: true,
                        previous: None
                    }
                );
            }
            let crique = records.get(&key("tracks/crique.track.ron", true)).unwrap();
            assert_eq!(crique.time, 5100);
            assert_eq!(
                crique.ghost.as_ref().map(|g| &g.frames),
                all.records[2].ghost.as_ref().map(|g| &g.frames)
            );
        }

        // The first record of the new version: the old one goes, from the stored text too.
        let mut records = reloaded;
        records.insert(record(prairie(false, after), 4100, None));
        assert_eq!(records.len(), 3);
        assert!(records.get(&prairie(false, before)).is_none());
        assert_eq!(records.get(&prairie(false, after)).unwrap().time, 4100);
        let kept = records.to_ron();
        assert_eq!(kept.matches(&format!("fingerprint: {after},")).count(), 1);
        assert_eq!(kept.matches(&format!("fingerprint: {before},")).count(), 1);
        let back = TrialRecords::from_ron(&kept).unwrap();
        assert_eq!(back, records);
        assert!(back.get(&prairie(false, before)).is_none());
        // The mirror's old record stays until its own next record (no ghost to drive through
        // walls: it is never read on the new version).
        assert!(back.get(&prairie(true, after)).is_none());
        assert_eq!(back.get(&prairie(true, before)).unwrap().time, 3900);
    }

    /// Issue #157: the texts of before it (formats 1 to 4: records without fingerprint) are
    /// ignored, without an error: no record, even for the circuits never redrawn, even a damaged
    /// one. Kept again, the text of today has only the new records. (Before #157, the records of
    /// format 1 in the default class were read, and those of formats 2 and 3 but for the circuits
    /// redrawn since: replaced by this, which is stricter.)
    #[test]
    fn the_records_of_before_issue_157_are_ignored() {
        let mut kept = TrialRecords::default();
        kept.insert(record(
            key("tracks/ruban.track.ron", false),
            3720,
            Some(3720),
        ));
        kept.insert(record(
            key("tracks/crique.track.ron", true),
            5100,
            Some(900),
        ));
        kept.insert(record(key("tracks/prairie.track.ron", true), 3900, None));
        let mut texts: Vec<(String, String)> = (2..=4)
            .map(|format| (format!("format {format}"), text_before_157(&kept, format)))
            .collect();
        texts.push((
            "format 1".into(),
            format_1_text(&kept, &["Cc150", "Cc200", "Cc150"]),
        ));
        // Broken texts of before: ignored too (no warning to give: nothing was lost).
        let good = text_before_157(&kept, 4);
        let check_start = good.find("check: ").unwrap() + 7;
        let mut bad_check = good.clone();
        let digit = &good[check_start..check_start + 1];
        bad_check.replace_range(
            check_start..check_start + 1,
            if digit == "1" { "2" } else { "1" },
        );
        texts.push(("a damaged ghost before".into(), bad_check));
        texts.push((
            "zero time before".into(),
            good.replace("time: 3720", "time: 0"),
        ));
        let twice = {
            let mut twice = kept.clone();
            twice.records.push(twice.records[0].clone());
            format_1_text(&twice, &["Cc150"; 4])
        };
        texts.push(("twice before".into(), twice));
        texts.push((
            "a bad class before".into(),
            format_1_text(&kept, &["Cc999"; 3]),
        ));
        // A text written by the game of issue #76 (a record without ghost), by hand.
        texts.push((
            "by hand".into(),
            r#"(
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
    ],
)"#
            .into(),
        ));
        for (what, text) in &texts {
            assert!(!text.contains("fingerprint"), "{what}");
            assert_eq!(
                TrialRecords::from_ron(text),
                Ok(TrialRecords::default()),
                "{what}"
            );
        }
        // The first record kept afterwards: the text of today, with it alone.
        let mut records = TrialRecords::from_ron(&texts[0].1).unwrap();
        records.insert(record(key("tracks/crique.track.ron", true), 5000, None));
        let today = records.to_ron();
        assert!(today.contains(&format!("format: {RECORDS_FORMAT},")));
        assert_eq!(today.matches("circuit: ").count(), 1);
        assert_eq!(TrialRecords::from_ron(&today), Ok(records));
    }

    /// An unreadable text is refused with its reason (the game keeps no record then): not RON,
    /// another format, a broken ghost, two records for the same race (issue #157: even of two
    /// versions of the circuit), a time of 0; issue #118: a class in the text of today; issue
    /// #157: a record without fingerprint in the text of today.
    #[test]
    fn unreadable_records_are_refused_with_a_reason() {
        let mut records = TrialRecords::default();
        records.insert(record(
            key("tracks/crique.track.ron", false),
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
        let today = format!("format: {RECORDS_FORMAT}");
        let other_format = good.replace(&today, &format!("format: {}", RECORDS_FORMAT + 1));
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
        let two_versions = {
            let mut twice = records.clone();
            let mut other = twice.records[0].clone();
            other.key.fingerprint ^= 1;
            twice.records.push(other);
            twice.to_ron()
        };
        let no_fingerprint = text_before_157(&records, RECORDS_FORMAT);
        let with_class = format_1_text(&records, &["Cc150"]).replace("format: 1", &today);
        for (what, bad) in [
            ("empty", String::new()),
            ("not RON", "n'importe quoi".to_string()),
            ("no records", format!("({today})")),
            ("broken data", broken_data),
            ("cut data", cut_data),
            ("more frames", more_frames),
            ("fewer frames", fewer_frames),
            ("no frames", no_frames),
            ("other format", other_format),
            ("format 0", good.replace(&today, "format: 0")),
            ("bad check", bad_check),
            ("zero time", zero_time),
            ("twice", twice),
            ("two versions", two_versions),
            ("no fingerprint", no_fingerprint),
            ("a class today", with_class),
        ] {
            let result = TrialRecords::from_ron(&bad);
            assert!(result.is_err(), "{what}: {result:?}");
            assert!(!result.unwrap_err().is_empty());
        }
        for format in 1..=RECORDS_FORMAT {
            assert_eq!(
                TrialRecords::from_ron(&format!("(format: {format}, records: [])")),
                Ok(TrialRecords::default())
            );
        }
    }
}
