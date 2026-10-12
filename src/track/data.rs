//! Track file format (`assets/tracks/*.track.ron`). A new circuit is just a new file.
//!
//! Positions along the track are given as a **spline parameter**: `3.5` means halfway
//! between control points 3 and 4. Lateral positions (`offset`) go from -1 (left edge of the
//! road) to 1 (right edge). Angles are in degrees.

use bevy::prelude::*;
use serde::Deserialize;

/// A whole circuit, as written in its RON file.
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
    /// Height of the low walls (m).
    #[serde(default = "default_wall_height")]
    pub wall_height: f32,
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
fn default_wall_height() -> f32 {
    0.8
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
    /// Parses a track file. Misspelled fields are errors, not silently ignored.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
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
        if self.wall_height < 0.0 {
            return fail(format!("negative wall_height {}", self.wall_height));
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
        Ok(())
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
}
