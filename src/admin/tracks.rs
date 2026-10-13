//! The circuits the admin panel offers (issue #36): the manifest `assets/tracks/index.ron`,
//! built into the game (a browser cannot list a folder, and the list is then there at once).

use serde::Deserialize;

use crate::track::TrackChoice;

/// The manifest's text.
pub const MANIFEST: &str = include_str!("../../assets/tracks/index.ron");

/// A circuit of the manifest.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrackEntry {
    /// The circuit's name (its file's `name`).
    pub name: String,
    /// Its file in `assets/tracks/`.
    pub file: String,
}

impl TrackEntry {
    /// The asset path to load it (relative to `assets/`).
    pub fn asset_path(&self) -> String {
        format!("tracks/{}", self.file)
    }

    pub fn choice(&self) -> TrackChoice {
        TrackChoice::Circuit(self.asset_path())
    }
}

/// Parses a manifest.
pub fn parse_manifest(text: &str) -> Result<Vec<TrackEntry>, ron::error::SpannedError> {
    ron::from_str(text)
}

/// The circuits of the built-in manifest (none if it does not parse: a test makes sure it does).
pub fn circuits() -> Vec<TrackEntry> {
    parse_manifest(MANIFEST).unwrap_or_default()
}

/// What the panel shows for `choice`: the circuit's name, or the test ground.
pub fn label(choice: &TrackChoice, circuits: &[TrackEntry]) -> String {
    match choice {
        TrackChoice::TestGround => TEST_GROUND_LABEL.to_string(),
        TrackChoice::Circuit(path) => circuits
            .iter()
            .find(|entry| entry.asset_path() == *path)
            .map_or_else(|| path.clone(), |entry| entry.name.clone()),
    }
}

/// The flat physics test ground of issue #1, last in the panel's list.
pub const TEST_GROUND_LABEL: &str = "Terrain de test (physique)";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{DEFAULT_CIRCUIT, TrackData, TrackLayout};

    /// The manifest lists every circuit file of the folder (and nothing else), each one parses
    /// and can be built, under the name the manifest gives.
    #[test]
    fn the_manifest_lists_every_circuit_of_the_folder() {
        let entries = parse_manifest(MANIFEST).unwrap();
        assert_eq!(entries, circuits());
        assert_eq!(entries[0].asset_path(), DEFAULT_CIRCUIT);
        assert!(entries.len() >= 2);

        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/tracks");
        let mut files: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".track.ron"))
            .collect();
        files.sort();
        let mut listed: Vec<String> = entries.iter().map(|e| e.file.clone()).collect();
        listed.sort();
        assert_eq!(listed, files, "assets/tracks/index.ron is out of date");

        for entry in &entries {
            let text = std::fs::read_to_string(format!("{folder}/{}", entry.file)).unwrap();
            let data = TrackData::from_ron(&text).unwrap();
            data.validate().unwrap();
            assert_eq!(data.name, entry.name);
            let layout = TrackLayout::new(&data);
            assert_eq!(layout.grid.len(), crate::track::GRID_SLOTS);
        }
    }

    #[test]
    fn the_panel_names_the_circuits() {
        let entries = circuits();
        assert_eq!(label(&TrackChoice::default(), &entries), "Prairie Pompon");
        assert_eq!(label(&TrackChoice::TestGround, &entries), TEST_GROUND_LABEL);
        assert_eq!(
            label(&TrackChoice::Circuit("tracks/x.track.ron".into()), &entries),
            "tracks/x.track.ron"
        );
        assert_eq!(
            entries[1].choice(),
            TrackChoice::Circuit("tracks/anneau.track.ron".into())
        );
    }
}
