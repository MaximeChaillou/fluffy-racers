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
    /// Glide ramps (GD § 10, issue #20): leaving one opens the glider. A trick works off them
    /// too. Put one just before a [`gaps`](Self::gaps) entry, its top at the gap's start.
    #[serde(default)]
    pub glide_ramps: Vec<TrickRampData>,
    /// Stretches of the centerline with no road at all (issue #20): the karts glide over them,
    /// from the lip of a glide ramp to the landing. Nothing is built there (road, walls); the
    /// centerline still runs through, as the corridor of the flight (checkpoints, the bots'
    /// line).
    #[serde(default)]
    pub gaps: Vec<GapData>,
    /// Lakes (GD § 10, issue #20): a basin dug in the ground, full of water up to `level`. A kart
    /// whose center is under the water is under water (the stats « eau »). The road may dip
    /// below the ground into a lake only.
    #[serde(default)]
    pub lakes: Vec<LakeData>,
    /// Anti-gravity zones (GD § 10, issue #78): stretches of the centerline where the karts'
    /// gravity follows the road's normal, so that they drive on walls. Only there may the road
    /// bank beyond 45° (up to [`MAX_ANTI_GRAVITY_BANK`]); the zone must begin and end where the
    /// road is still flat enough to drive on without it.
    #[serde(default)]
    pub anti_gravity: Vec<AntiGravityData>,
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

/// A stretch with no road (issue #20), between two spline parameters.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GapData {
    pub from: f32,
    pub to: f32,
}

/// An anti-gravity zone (issue #78), between two spline parameters.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AntiGravityData {
    pub from: f32,
    pub to: f32,
}

/// The steepest banking outside the anti-gravity zones (degrees): the karts drive there with the
/// world's gravity.
pub const MAX_BANK: f32 = 45.0;
/// The steepest banking in an anti-gravity zone (degrees, issue #78): a wall. Beyond, a road upside
/// down (a tube) is for a later issue (its skirts, its camera).
pub const MAX_ANTI_GRAVITY_BANK: f32 = 90.0;

/// A lake (issue #20): a rectangle of the ground plane, dug down to `floor` and full of water up
/// to `level` (m). `level` is below the ground (`mesh::GROUND_HEIGHT`): the water stays in its
/// basin.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LakeData {
    /// Center (x, z) and size (along x, along z) of the rectangle (m).
    pub center: (f32, f32),
    pub size: (f32, f32),
    /// Height of the water's surface (m).
    #[serde(default = "default_lake_level")]
    pub level: f32,
    /// Height of the basin's floor (m).
    pub floor: f32,
}

fn default_lake_level() -> f32 {
    -0.4
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
            if point.width < 4.0
                || point.shoulder < 0.0
                || !point.bank.is_finite()
                || point.bank.abs() > MAX_ANTI_GRAVITY_BANK
            {
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
        let n = self.points.len() as f32;
        for gap in &self.gaps {
            let length = (gap.to - gap.from).rem_euclid(n);
            if !(gap.from.is_finite() && gap.to.is_finite()) || length < 0.05 || length > n / 4.0 {
                return fail(format!(
                    "gap {}..{}: empty, or more than a quarter of the circuit",
                    gap.from, gap.to
                ));
            }
        }
        for (i, a) in self.gaps.iter().enumerate() {
            for b in &self.gaps[i + 1..] {
                let starts_in = |g: &GapData, p: f32| {
                    (p - g.from).rem_euclid(n) < (g.to - g.from).rem_euclid(n)
                };
                if starts_in(a, b.from) || starts_in(b, a.from) {
                    return fail(format!(
                        "gaps {}..{} and {}..{} overlap",
                        a.from, a.to, b.from, b.to
                    ));
                }
            }
        }
        // Issue #78: the anti-gravity zones, apart from each other and from the gaps, half the
        // circuit at most; beyond 45°, the road banks only in one of them.
        let within = |zone: &AntiGravityData, p: f32| {
            (p - zone.from).rem_euclid(n) <= (zone.to - zone.from).rem_euclid(n)
        };
        for zone in &self.anti_gravity {
            let length = (zone.to - zone.from).rem_euclid(n);
            if !(zone.from.is_finite() && zone.to.is_finite()) || length < 0.5 || length > n / 2.0 {
                return fail(format!(
                    "anti-gravity zone {}..{}: shorter than half a segment, or more than half \
                     the circuit",
                    zone.from, zone.to
                ));
            }
            let in_gap =
                |g: &GapData, p: f32| (p - g.from).rem_euclid(n) < (g.to - g.from).rem_euclid(n);
            if let Some(gap) = self
                .gaps
                .iter()
                .find(|g| within(zone, g.from) || in_gap(g, zone.from))
            {
                return fail(format!(
                    "anti-gravity zone {}..{} on the gap {}..{}",
                    zone.from, zone.to, gap.from, gap.to
                ));
            }
        }
        for (i, a) in self.anti_gravity.iter().enumerate() {
            for b in &self.anti_gravity[i + 1..] {
                if within(a, b.from) || within(b, a.from) {
                    return fail(format!(
                        "anti-gravity zones {}..{} and {}..{} overlap",
                        a.from, a.to, b.from, b.to
                    ));
                }
            }
        }
        if let Some(param) = self.steep_banking_outside_the_zones() {
            return fail(format!(
                "banked beyond {MAX_BANK}° at {param}, out of an anti-gravity zone"
            ));
        }
        for lake in &self.lakes {
            let ground = super::mesh::GROUND_HEIGHT;
            if !(lake.size.0 >= 10.0 && lake.size.1 >= 10.0)
                || !lake.center.0.is_finite()
                || !lake.center.1.is_finite()
                || lake.level.is_nan()
                || lake.level >= ground
                || !(lake.floor <= lake.level - 2.0 && lake.floor >= -60.0)
            {
                return fail(format!(
                    "lake at {:?}: size {:?} (10 m at least), level {} (below the ground, {ground}), \
                     floor {} (2 m below the level at least, down to -60 m)",
                    lake.center, lake.size, lake.level, lake.floor
                ));
            }
        }
        for (i, a) in self.lakes.iter().enumerate() {
            for b in &self.lakes[i + 1..] {
                let apart = |c: f32, d: f32, s: f32, t: f32| (c - d).abs() >= (s + t) / 2.0;
                if !apart(a.center.0, b.center.0, a.size.0, b.size.0)
                    && !apart(a.center.1, b.center.1, a.size.1, b.size.1)
                {
                    return fail(format!(
                        "lakes at {:?} and {:?} overlap",
                        a.center, b.center
                    ));
                }
            }
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

    /// Issue #78: the first spline parameter (in steps of 1/16) where the road banks beyond
    /// [`MAX_BANK`] out of every anti-gravity zone, if there is one. The banking blends between
    /// two control points (`layout::bank_between`), so a steep point banks its neighbouring
    /// segments too.
    pub fn steep_banking_outside_the_zones(&self) -> Option<f32> {
        const STEPS: usize = 16;
        let n = self.points.len();
        let slopes = super::layout::bank_slopes(&self.points, &self.anti_gravity);
        (0..n * STEPS).find_map(|k| {
            let (i, t) = (k / STEPS, (k % STEPS) as f32 / STEPS as f32);
            let (a, b) = (self.points[i].bank, self.points[(i + 1) % n].bank);
            let bank = super::layout::bank_between(a, b, (slopes[i], slopes[(i + 1) % n]), t);
            let param = k as f32 / STEPS as f32;
            let zoned = self.anti_gravity.iter().any(|zone| {
                let n = n as f32;
                (param - zone.from).rem_euclid(n) <= (zone.to - zone.from).rem_euclid(n)
            });
            (bank.abs() > MAX_BANK + 1e-3 && !zoned).then_some(param)
        })
    }

    /// The circuit's biome (None: no biome, or one the game does not have, which `validate`
    /// refuses).
    pub fn biome(&self) -> Option<&'static BiomeData> {
        self.biome.as_deref().and_then(biome::biome)
    }

    /// The mirror image of the circuit (issue #19, GD § 1: the mirror mode), left and right
    /// swapped: every control point mirrored across the X = 0 plane (x → -x), its banking turned
    /// the other way, everything placed across the road on the other side (offsets, sand patches),
    /// the props on the other side, the lakes across the X = 0 plane (issue #20; the gaps are
    /// along the track: they do not move, nor do the anti-gravity zones of issue #78). The centerline of the mirror is exactly the mirror image of
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
        for ramp in data.trick_ramps.iter_mut().chain(&mut data.glide_ramps) {
            ramp.offset = flip(ramp.offset);
        }
        for lake in &mut data.lakes {
            lake.center.0 = -lake.center.0;
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

    /// Issue #20: the gaps and the lakes are checked: a gap is not empty, nor more than a quarter
    /// of the circuit, nor on another one; a lake is big enough, its water below the ground, its
    /// floor below its water, and not on another lake. Mirrored, a lake moves across, a gap does
    /// not, a glide ramp goes to the other side of the road.
    #[test]
    fn gaps_and_lakes_that_make_no_sense_are_rejected() {
        let base = TrackData::from_ron(&MINIMAL.replace(
            "(name:",
            "(gaps: [(from: 1.2, to: 1.6)], glide_ramps: [(at: 1.0, offset: 0.2)], \
             lakes: [(center: (25.0, -25.0), size: (30.0, 20.0), floor: -8.0)], name:",
        ))
        .unwrap();
        assert_eq!(base.validate(), Ok(()));
        assert_eq!(base.lakes[0].level, -0.4, "the default level");
        let broken = |change: &dyn Fn(&mut TrackData)| {
            let mut data = base.clone();
            change(&mut data);
            data.validate().is_err()
        };
        assert!(broken(&|d| d.gaps[0].to = 1.2));
        assert!(broken(&|d| d.gaps[0].to = 2.5));
        assert!(broken(&|d| d.gaps.push(GapData { from: 1.4, to: 1.9 })));
        assert!(!broken(&|d| d.gaps.push(GapData { from: 3.0, to: 3.5 })));
        assert!(broken(&|d| d.gaps[0].from = f32::NAN));
        assert!(broken(&|d| d.lakes[0].size.0 = 5.0));
        assert!(broken(&|d| d.lakes[0].level = 0.5));
        assert!(broken(&|d| d.lakes[0].level = f32::NAN));
        assert!(broken(&|d| d.lakes[0].floor = -1.0));
        assert!(broken(&|d| d.lakes[0].floor = -100.0));
        assert!(broken(&|d| d.lakes.push(d.lakes[0])));
        assert!(!broken(&|d| {
            let mut other = d.lakes[0];
            other.center.0 += 31.0;
            d.lakes.push(other)
        }));
        let mirrored = base.mirrored();
        assert_eq!(mirrored.lakes[0].center, (-25.0, -25.0));
        assert_eq!(mirrored.gaps, base.gaps);
        assert_eq!(mirrored.glide_ramps[0].offset, -0.2);
        assert_eq!(mirrored.validate(), Ok(()));
        assert_eq!(mirrored.mirrored(), base);
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
