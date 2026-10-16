//! Track file format (`assets/tracks/*.track.ron`). A new circuit is just a new file.
//!
//! Positions along the track are given as a **spline parameter**: `3.5` means halfway
//! between control points 3 and 4. Lateral positions (`offset`) go from -1 (left edge of the
//! road) to 1 (right edge). Angles are in degrees.

use bevy::prelude::*;
use serde::Deserialize;

use super::biome::{self, BiomeData, Side};

/// A whole circuit, as written in its RON file. The walls are the same on every circuit
/// (`mesh::WALL_HEIGHT`, `mesh::WALL_THICKNESS`): a file has no say in their height (issue #47).
#[derive(Asset, TypePath, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrackData {
    pub name: String,
    /// Control points of the closed Catmull-Rom centerline. Point 0 is the start line;
    /// the track runs from point 0 to point 1.
    pub points: Vec<ControlPoint>,
    /// Distance between two checkpoints (m).
    #[serde(default = "default_checkpoint_spacing")]
    pub checkpoint_spacing: f32,
    /// Length of the generated mesh segments (m).
    #[serde(default = "default_sample_step")]
    pub sample_step: f32,
    /// Sand / grass patches on the road (max speed drops, GD § 2).
    #[serde(default)]
    pub offroad: Vec<OffroadZone>,
    /// Trick ramps: jumps where a trick gives a boost (GD § 5, issue #13).
    #[serde(default)]
    pub trick_ramps: Vec<TrickRampData>,
    /// Boost pads on the ground (GD § 2, issue #13).
    #[serde(default)]
    pub boost_pads: Vec<Spot>,
    /// Rows of item boxes (GD § 8, issue #16).
    #[serde(default)]
    pub item_boxes: Vec<ItemBoxRow>,
    /// Lines of croquettes (GD § 7, issue #14).
    #[serde(default)]
    pub croquettes: Vec<CroquetteLine>,
    /// Where the tow pelican puts karts back (spline parameters). Empty = every checkpoint.
    #[serde(default)]
    pub respawns: Vec<f32>,
    /// How the circuit is dressed (issue #11): the name of a biome of `assets/data/biomes.ron`
    /// (its colors and its pieces). None: the colors of a circuit without biome, no decor.
    #[serde(default)]
    pub biome: Option<String>,
    /// Seed of the decor's placement: the same pieces at the same places on every load.
    #[serde(default)]
    pub decor_seed: u64,
    /// Pieces placed by hand beyond the walls (a grandstand at the start...), on top of the
    /// biome's.
    #[serde(default)]
    pub props: Vec<PropData>,
    /// Issue #19: this is the mirror image of the file ([`TrackData::mirrored`]), never written in
    /// a file. The starting grid follows (the pole on the other side).
    #[serde(skip)]
    pub mirrored: bool,
}

/// A piece placed by hand beyond the walls (issue #11), facing the track.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PropData {
    /// "pack/name" (`assets/models/pack/name.glb`).
    pub model: String,
    /// Where along the track (spline parameter).
    pub at: f32,
    /// Which side (`Both`: one on each side).
    pub side: Side,
    /// Height (m).
    pub height: f32,
    /// Footprint (m): it keeps that far from the walls.
    pub radius: f32,
    /// Gap between the walls' outer face and the footprint (m).
    #[serde(default)]
    pub gap: f32,
    /// Turns it further than facing the track (degrees).
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "yes")]
    pub shadows: bool,
}

fn yes() -> bool {
    true
}

/// A control point of the centerline.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ControlPoint {
    /// (x, y, z) in meters. y is the height of the road.
    pub pos: (f32, f32, f32),
    /// Road width (m). Generous by default, like kart games (≈ 14 m).
    #[serde(default = "default_width")]
    pub width: f32,
    /// Banking (degrees). Positive lowers the right edge: use it in right-hand turns.
    #[serde(default)]
    pub bank: f32,
    /// Grass strip between the curb and the wall (m).
    #[serde(default = "default_shoulder")]
    pub shoulder: f32,
}

impl ControlPoint {
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.pos.0, self.pos.1, self.pos.2)
    }
}

/// An off-road patch on the road.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OffroadZone {
    /// Start and end along the track (spline parameters).
    pub from: f32,
    pub to: f32,
    /// Lateral extent, -1 (left edge) to 1 (right edge).
    pub left: f32,
    pub right: f32,
}

/// A ramp that sends karts flying.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrickRampData {
    /// Where the ramp starts (spline parameter).
    pub at: f32,
    #[serde(default)]
    pub offset: f32,
    #[serde(default = "default_ramp_width")]
    pub width: f32,
    #[serde(default = "default_ramp_length")]
    pub length: f32,
    /// Slope (degrees).
    #[serde(default = "default_ramp_angle")]
    pub angle: f32,
}

/// A single thing on the road.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spot {
    pub at: f32,
    #[serde(default)]
    pub offset: f32,
}

/// A row of item boxes across the road.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ItemBoxRow {
    pub at: f32,
    pub offsets: Vec<f32>,
    /// Double boxes give two items (GD § 8): put them in risky places.
    #[serde(default)]
    pub double: bool,
}

/// Croquettes lined up along the track.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CroquetteLine {
    pub from: f32,
    pub count: u32,
    /// Distance between two croquettes (m).
    #[serde(default = "default_croquette_spacing")]
    pub spacing: f32,
    #[serde(default)]
    pub offset: f32,
}

fn default_checkpoint_spacing() -> f32 {
    25.0
}
fn default_sample_step() -> f32 {
    1.0
}
fn default_width() -> f32 {
    14.0
}
fn default_shoulder() -> f32 {
    4.0
}
fn default_ramp_width() -> f32 {
    7.0
}
fn default_ramp_length() -> f32 {
    7.0
}
fn default_ramp_angle() -> f32 {
    12.0
}
fn default_croquette_spacing() -> f32 {
    4.0
}

/// A track file that parses but cannot be built.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidTrack(pub String);

impl std::fmt::Display for InvalidTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid track: {}", self.0)
    }
}

impl std::error::Error for InvalidTrack {}

impl TrackData {
    /// Parses a track file. Misspelled fields are errors, not silently ignored. An optional
    /// field is written as its value (`biome: "Prairie"`, not `Some("Prairie")`).
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
    }

    /// Checks the values that would make the generation fail or explode.
    pub fn validate(&self) -> Result<(), InvalidTrack> {
        let fail = |message: String| Err(InvalidTrack(message));
        if self.points.len() < 4 {
            return fail(format!(
                "{} control points, at least 4 needed",
                self.points.len()
            ));
        }
        if !(0.25..=10.0).contains(&self.sample_step) {
            return fail(format!("sample_step {} out of 0.25..=10", self.sample_step));
        }
        if self.checkpoint_spacing < 5.0 {
            return fail(format!(
                "checkpoint_spacing {} below 5 m",
                self.checkpoint_spacing
            ));
        }
        for (i, point) in self.points.iter().enumerate() {
            if point.width < 4.0 || point.shoulder < 0.0 || point.bank.abs() > 45.0 {
                return fail(format!(
                    "point {i}: width {} / shoulder {} / bank {}",
                    point.width, point.shoulder, point.bank
                ));
            }
            let next = &self.points[(i + 1) % self.points.len()];
            if point.position().distance(next.position()) < 1.0 {
                return fail(format!("points {i} and {} are on top of each other", i + 1));
            }
        }
        if let Some(name) = &self.biome
            && biome::biome(name).is_none()
        {
            let known: Vec<&str> = biome::biomes().iter().map(|b| b.name.as_str()).collect();
            return fail(format!(
                "unknown biome {name:?} (known: {})",
                known.join(", ")
            ));
        }
        for prop in &self.props {
            if !biome::is_model_id(&prop.model)
                || !(0.1..=30.0).contains(&prop.height)
                || !(0.1..=12.0).contains(&prop.radius)
                || !(0.0..=biome::MAX_REACH).contains(&(prop.gap + prop.radius))
            {
                return fail(format!(
                    "prop {:?}: not a model (\"pack/name\"), or height {} / radius {} / gap {} \
                     out of range",
                    prop.model, prop.height, prop.radius, prop.gap
                ));
            }
        }
        Ok(())
    }

    /// The circuit's biome (None: no biome, or one the game does not have, which `validate`
    /// refuses).
    pub fn biome(&self) -> Option<&'static BiomeData> {
        self.biome.as_deref().and_then(biome::biome)
    }

    /// The mirror image of the circuit (issue #19, GD § 1: the mirror mode), left and right
    /// swapped: every control point mirrored across the X = 0 plane (x → -x), its banking turned
    /// the other way, everything placed across the road on the other side (offsets, sand patches),
    /// the props on the other side. The centerline of the mirror is exactly the mirror image of
    /// the original (Catmull-Rom is linear in its points), the same length: a point `offset`
    /// across the road at the same spline parameter is the mirror image of the original's at
    /// `-offset`. Mirrored twice, the file comes back.
    pub fn mirrored(&self) -> Self {
        let flip = |offset: f32| -offset;
        let mut data = self.clone();
        data.mirrored = !self.mirrored;
        for point in &mut data.points {
            point.pos.0 = -point.pos.0;
            point.bank = -point.bank;
        }
        for zone in &mut data.offroad {
            (zone.left, zone.right) = (flip(zone.right), flip(zone.left));
        }
        for ramp in &mut data.trick_ramps {
            ramp.offset = flip(ramp.offset);
        }
        for pad in &mut data.boost_pads {
            pad.offset = flip(pad.offset);
        }
        for row in &mut data.item_boxes {
            for offset in &mut row.offsets {
                *offset = flip(*offset);
            }
        }
        for line in &mut data.croquettes {
            line.offset = flip(line.offset);
        }
        for prop in &mut data.props {
            prop.side = prop.side.mirrored();
            prop.yaw = -prop.yaw;
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "(name: \"Square\", points: [(pos: (0.0, 0.0, 0.0)), (pos: (0.0, 0.0, -50.0)), (pos: (50.0, 0.0, -50.0)), (pos: (50.0, 0.0, 0.0))])";

    #[test]
    fn a_minimal_file_is_valid() {
        let data = TrackData::from_ron(MINIMAL).unwrap();
        assert_eq!(data.validate(), Ok(()));
        assert_eq!(data.checkpoint_spacing, 25.0);
    }

    #[test]
    fn misspelled_fields_are_rejected() {
        let text = MINIMAL.replace(
            "(pos: (0.0, 0.0, 0.0))",
            "(pos: (0.0, 0.0, 0.0), banking: 12.0)",
        );
        assert!(TrackData::from_ron(&text).is_err());
    }

    #[test]
    fn values_that_would_break_the_generation_are_rejected() {
        let mut data = TrackData::from_ron(MINIMAL).unwrap();
        data.sample_step = 0.0;
        assert!(data.validate().is_err());
        let mut data = TrackData::from_ron(MINIMAL).unwrap();
        data.checkpoint_spacing = 0.0;
        assert!(data.validate().is_err());
        let mut data = TrackData::from_ron(MINIMAL).unwrap();
        data.points.truncate(3);
        assert!(data.validate().is_err());
        let mut data = TrackData::from_ron(MINIMAL).unwrap();
        data.points[1] = data.points[0];
        assert!(data.validate().is_err());
    }

    /// Issue #11: a circuit names its biome; without one, it has none. A biome the game does not
    /// have is refused with the names of those it has; a prop must name a model.
    #[test]
    fn a_circuit_names_a_biome_the_game_has() {
        let data = TrackData::from_ron(MINIMAL).unwrap();
        assert_eq!(data.biome, None);
        assert!(data.biome().is_none());
        let with = |biome: &str| {
            TrackData::from_ron(&MINIMAL.replace("(name:", &format!("(biome: {biome:?}, name:")))
                .unwrap()
        };
        let prairie = with("Prairie");
        assert_eq!(prairie.validate(), Ok(()));
        assert_eq!(prairie.biome().unwrap().name, "Prairie");
        let error = with("Prairy").validate().unwrap_err();
        assert!(
            error.0.contains("unknown biome \"Prairy\"") && error.0.contains("Guimauve"),
            "{error}"
        );
        let prop = |model: &str| {
            TrackData::from_ron(&MINIMAL.replace(
                "(name:",
                &format!(
                    "(props: [(model: {model:?}, at: 1.0, side: Left, height: 4.0, radius: 3.0)], \
                     name:"
                ),
            ))
            .unwrap()
        };
        assert_eq!(prop("racing-kit/grandStandCovered").validate(), Ok(()));
        assert!(prop("grandStandCovered").validate().is_err());
    }
}
