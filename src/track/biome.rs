//! Biomes (issue #11): how a circuit is dressed, as written in `assets/data/biomes.ron`. A
//! circuit file names its biome; several circuits can share one, and one biome can mix several
//! packs of models. Pure data and checks, no ECS: the placement of the pieces is in `decor.rs`,
//! the models' loading in `crate::models`.
//!
//! The file is compiled into the game (`include_str!`, like the roster of #15): there is nothing
//! to request before the circuit is built, and a circuit naming a biome that does not exist is
//! refused when it loads, like a misspelled field (`TrackData::validate`).

use std::sync::OnceLock;

use serde::Deserialize;

/// The biomes' file, compiled into the game.
pub const BIOMES_RON: &str = include_str!("../../assets/data/biomes.ron");

/// An sRGB color, 0 to 1 per channel, as the files write it.
pub type Rgb = (f32, f32, f32);

/// The colors of a biome: the sky, the light in the shade, the ground and the track. The pairs
/// alternate along the track (curb stripes, wall panels).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub sky: Rgb,
    /// Color of the ambient light: what lights the faces the sun does not reach.
    pub ambient: Rgb,
    /// Its strength (cd/m², Bevy's `GlobalAmbientLight::brightness`; Bevy's default is 80, under
    /// which the walls' side in the shade was almost black, issue #47).
    pub ambient_brightness: f32,
    pub ground: Rgb,
    pub asphalt: Rgb,
    pub curbs: (Rgb, Rgb),
    /// The grass strip between the curbs and the walls.
    pub shoulder: Rgb,
    /// The off-road patches on the road (sand).
    pub sand: Rgb,
    /// The faces of the walls, in panels of two colors (issue #11: the barriers' look).
    pub walls: (Rgb, Rgb),
    pub wall_top: Rgb,
    /// The skirts under the raised parts.
    pub skirt: Rgb,
}

impl Default for Palette {
    /// A circuit without a biome (the tests' circuits): the colors from before the biomes, with
    /// the lighter walls and the ambient light of issue #11.
    fn default() -> Self {
        Self {
            sky: (0.55, 0.80, 0.95),
            ambient: (1.0, 1.0, 1.0),
            ambient_brightness: DEFAULT_AMBIENT_BRIGHTNESS,
            ground: (0.45, 0.75, 0.35),
            asphalt: (0.42, 0.43, 0.48),
            curbs: ((0.85, 0.20, 0.20), (0.95, 0.95, 0.95)),
            shoulder: (0.40, 0.70, 0.32),
            sand: (0.90, 0.80, 0.55),
            walls: ((0.72, 0.68, 0.90), (0.72, 0.68, 0.90)),
            wall_top: (0.95, 0.95, 0.95),
            skirt: (0.55, 0.45, 0.35),
        }
    }
}

/// The ambient light without a biome (the test ground too), cd/m².
pub const DEFAULT_AMBIENT_BRIGHTNESS: f32 = 600.0;

/// Where pieces stand, beyond the walls.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// Both sides of the track.
    #[default]
    Both,
    /// Inside the loop (the infield).
    Inside,
    /// Outside the loop.
    Outside,
    /// On the driver's left.
    Left,
    /// On the driver's right.
    Right,
}

impl Side {
    /// The same side on the mirrored circuit (issue #19): left and right swap, the inside and
    /// the outside of the loop stay.
    pub fn mirrored(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            other => other,
        }
    }
}

/// Pieces strewn beyond the walls (deterministic for a circuit).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScatterGroup {
    /// Models drawn in turn at random ("pack/name": `assets/models/pack/name.glb`).
    pub models: Vec<String>,
    pub count: u32,
    /// Height of a piece (m), drawn between the two.
    pub height: (f32, f32),
    /// Footprint of a piece (m): it keeps that far from the walls and from the other pieces.
    pub radius: f32,
    /// Gap between the walls' outer face and the footprint (m), drawn between the two.
    pub distance: (f32, f32),
    #[serde(default)]
    pub side: Side,
    /// Whether the pieces cast shadows (the small ones do not: cheaper).
    #[serde(default = "yes")]
    pub shadows: bool,
}

/// Pieces in a row along the walls, facing the track.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PropRow {
    /// Models taken in turn.
    pub models: Vec<String>,
    /// Distance between two pieces along the track (m).
    pub spacing: f32,
    /// Where the first one stands, from the start line (m).
    #[serde(default)]
    pub offset: f32,
    pub height: f32,
    pub radius: f32,
    /// Gap between the walls' outer face and the footprint (m).
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub side: Side,
    /// Turns the pieces further than facing the track (degrees).
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "yes")]
    pub shadows: bool,
}

fn yes() -> bool {
    true
}

/// A biome, as written in `biomes.ron`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BiomeData {
    pub name: String,
    pub palette: Palette,
    #[serde(default)]
    pub rows: Vec<PropRow>,
    #[serde(default)]
    pub scatter: Vec<ScatterGroup>,
}

impl BiomeData {
    /// Every model the biome places, each once, in the order of the file.
    pub fn models(&self) -> Vec<&str> {
        let mut models: Vec<&str> = Vec::new();
        let all = self
            .rows
            .iter()
            .flat_map(|row| &row.models)
            .chain(self.scatter.iter().flat_map(|group| &group.models));
        for model in all {
            if !models.contains(&model.as_str()) {
                models.push(model);
            }
        }
        models
    }

    /// Checks the values that would make the placement fail or look broken.
    pub fn validate(&self) -> Result<(), String> {
        let fail = |what: String| Err(format!("biome {}: {what}", self.name));
        if self.name.is_empty() {
            return Err("a biome without a name".to_string());
        }
        let p = &self.palette;
        let colors = [
            p.sky, p.ambient, p.ground, p.asphalt, p.curbs.0, p.curbs.1, p.shoulder, p.sand,
            p.walls.0, p.walls.1, p.wall_top, p.skirt,
        ];
        if colors
            .iter()
            .any(|(r, g, b)| [r, g, b].iter().any(|v| !(0.0..=1.0).contains(*v)))
        {
            return fail("a color out of 0..=1".to_string());
        }
        if !(0.0..=5000.0).contains(&p.ambient_brightness) {
            return fail(format!(
                "ambient_brightness {} out of 0..=5000",
                p.ambient_brightness
            ));
        }
        for row in &self.rows {
            check_models(&row.models).or_else(fail)?;
            if row.spacing < 2.0 || !(0.1..=30.0).contains(&row.height) {
                return fail(format!(
                    "row of {:?}: spacing {} below 2 m or height {} out of 0.1..=30",
                    row.models, row.spacing, row.height
                ));
            }
            check_footprint(row.radius, row.gap, row.gap).or_else(fail)?;
        }
        for group in &self.scatter {
            check_models(&group.models).or_else(fail)?;
            let (low, high) = group.height;
            if group.count > 2000 || !(0.1..=30.0).contains(&low) || high < low || high > 30.0 {
                return fail(format!(
                    "{:?}: count {} above 2000, or height {:?} not within 0.1..=30",
                    group.models, group.count, group.height
                ));
            }
            check_footprint(group.radius, group.distance.0, group.distance.1).or_else(fail)?;
        }
        Ok(())
    }
}

/// A footprint and its distance from the walls that the ground under the circuit can hold.
fn check_footprint(radius: f32, near: f32, far: f32) -> Result<(), String> {
    if !(0.1..=12.0).contains(&radius) || near < 0.0 || far < near || far + radius > MAX_REACH {
        return Err(format!(
            "radius {radius} out of 0.1..=12, or distance {near}..{far} not within 0..{MAX_REACH} m"
        ));
    }
    Ok(())
}

/// How far a piece may stand from the walls (m): the ground reaches 80 m beyond the circuit.
pub const MAX_REACH: f32 = 60.0;

/// A model's name: "pack/name", letters, digits, `-` and `_` only (the file
/// `assets/models/pack/name.glb`).
pub fn is_model_id(id: &str) -> bool {
    let mut parts = id.split('/');
    let valid = |part: Option<&str>| {
        part.is_some_and(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    };
    valid(parts.next()) && valid(parts.next()) && parts.next().is_none()
}

/// The pack of a model ("nature-kit" for "nature-kit/tree_oak").
pub fn pack_of(id: &str) -> &str {
    id.split('/').next().unwrap_or(id)
}

/// The asset path of a model (relative to `assets/`).
pub fn model_path(id: &str) -> String {
    format!("models/{id}.glb")
}

fn check_models(models: &[String]) -> Result<(), String> {
    if models.is_empty() {
        return Err("a list of models is empty".to_string());
    }
    match models.iter().find(|id| !is_model_id(id)) {
        Some(bad) => Err(format!("{bad:?} is not a model (\"pack/name\")")),
        None => Ok(()),
    }
}

/// Reads and checks the biomes' file: each biome valid, no name twice.
pub fn parse_biomes(text: &str) -> Result<Vec<BiomeData>, String> {
    let biomes: Vec<BiomeData> = ron::from_str(text).map_err(|e| format!("biomes.ron: {e}"))?;
    for (i, biome) in biomes.iter().enumerate() {
        biome.validate()?;
        if biomes[..i].iter().any(|b| b.name == biome.name) {
            return Err(format!("biomes.ron: {} twice", biome.name));
        }
    }
    Ok(biomes)
}

/// The biomes compiled into the game. The file is checked by a test; should it still be broken,
/// the console says why and there is no biome (every circuit that names one is then refused).
pub fn biomes() -> &'static [BiomeData] {
    static BIOMES: OnceLock<Vec<BiomeData>> = OnceLock::new();
    BIOMES.get_or_init(|| {
        parse_biomes(BIOMES_RON).unwrap_or_else(|error| {
            bevy::log::error!("{error}");
            Vec::new()
        })
    })
}

/// The biome of that name, if the game has it.
pub fn biome(name: &str) -> Option<&'static BiomeData> {
    biomes().iter().find(|b| b.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #11, and #19 (Max: « des ambiances variées »): four ambiances, the meadow, the
    /// pastry land, the snow and the beach; issue #20: a fifth, the lagoon.
    #[test]
    fn the_biomes_file_is_valid_and_has_the_five_ambiances() {
        let biomes = parse_biomes(BIOMES_RON).unwrap();
        let names: Vec<&str> = biomes.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["Prairie", "Guimauve", "Neige", "Plage", "Lagon"]);
        assert_eq!(biomes, super::biomes());
        // Distinct ambiances, two by two: other sky, ground and road, and no model in common.
        for (i, one) in biomes.iter().enumerate() {
            for other in &biomes[i + 1..] {
                let (p, q) = (&one.palette, &other.palette);
                assert_ne!(p.sky, q.sky, "{} / {}", one.name, other.name);
                assert_ne!(p.ground, q.ground, "{} / {}", one.name, other.name);
                assert_ne!(p.asphalt, q.asphalt, "{} / {}", one.name, other.name);
                let (a, b) = (one.models(), other.models());
                assert!(a.iter().all(|m| !b.contains(m)), "{a:?} / {b:?}");
            }
        }
        // Each mixes two packs (issue #11: a biome can mix packs).
        for biome in &biomes {
            let mut packs: Vec<&str> = biome.models().iter().map(|m| pack_of(m)).collect();
            packs.dedup();
            packs.sort();
            packs.dedup();
            assert_eq!(packs.len(), 2, "{}: {packs:?}", biome.name);
        }
        assert!(biome("Prairie").is_some());
        assert!(biome("Prairy").is_none());
    }

    #[test]
    fn model_names_are_pack_slash_name() {
        for good in ["nature-kit/tree_oak", "food-kit/donut-sprinkles", "a/b"] {
            assert!(is_model_id(good), "{good}");
        }
        for bad in [
            "",
            "tree_oak",
            "nature-kit/",
            "/tree",
            "a/b/c",
            "../x/y",
            "nature-kit/tree oak",
            "nature-kit/tree.glb",
        ] {
            assert!(!is_model_id(bad), "{bad}");
        }
        assert_eq!(pack_of("nature-kit/tree_oak"), "nature-kit");
        assert_eq!(
            model_path("nature-kit/tree_oak"),
            "models/nature-kit/tree_oak.glb"
        );
    }

    #[test]
    fn broken_biomes_are_refused_with_a_reason() {
        let good = parse_biomes(BIOMES_RON).unwrap();
        let text = |biomes: &[BiomeData]| {
            // Back to RON through the same fields: a minimal writer for the tests.
            let one = |b: &BiomeData| {
                format!(
                    "(name: {:?}, palette: {}, scatter: [{}])",
                    b.name,
                    palette_ron(&b.palette),
                    b.scatter
                        .iter()
                        .map(|g| format!(
                            "(models: {:?}, count: {}, height: {:?}, radius: {}, distance: {:?})",
                            g.models, g.count, g.height, g.radius, g.distance
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            format!(
                "[{}]",
                biomes.iter().map(one).collect::<Vec<_>>().join(", ")
            )
        };
        assert!(parse_biomes(&text(&good[..1])).is_ok());
        // The same name twice.
        let twice = [good[0].clone(), good[0].clone()];
        assert!(parse_biomes(&text(&twice)).unwrap_err().contains("twice"));
        // A model that is not "pack/name".
        let mut bad = good[0].clone();
        bad.scatter[0].models[0] = "tree_oak".into();
        assert!(
            parse_biomes(&text(&[bad]))
                .unwrap_err()
                .contains("tree_oak")
        );
        // A footprint beyond the ground, a color out of range, a misspelled field.
        let mut far = good[0].clone();
        far.scatter[0].distance = (10.0, 70.0);
        assert!(parse_biomes(&text(&[far])).is_err());
        let mut color = good[0].clone();
        color.palette.ground = (0.5, 1.5, 0.5);
        assert!(parse_biomes(&text(&[color])).unwrap_err().contains("color"));
        let misspelled = text(&good[..1]).replace("count:", "cout:");
        assert!(parse_biomes(&misspelled).is_err());
    }

    fn palette_ron(p: &Palette) -> String {
        format!(
            "(sky: {:?}, ambient: {:?}, ambient_brightness: {:?}, ground: {:?}, asphalt: {:?}, \
             curbs: {:?}, shoulder: {:?}, sand: {:?}, walls: {:?}, wall_top: {:?}, skirt: {:?})",
            p.sky,
            p.ambient,
            p.ambient_brightness,
            p.ground,
            p.asphalt,
            p.curbs,
            p.shoulder,
            p.sand,
            p.walls,
            p.wall_top,
            p.skirt
        )
    }
}
