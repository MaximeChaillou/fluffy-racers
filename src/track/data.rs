//! Track file format (`assets/tracks/*.track.ron`). A new circuit is just a new file.
//!
//! Positions along the track are given as a **spline parameter**: `3.5` means halfway
//! between control points 3 and 4. Lateral positions (`offset`) go from -1 (left edge of the
//! road) to 1 (right edge). Angles are in degrees.

use bevy::prelude::*;
use serde::Deserialize;

use super::biome::{self, BiomeData, Palette, RoadLook, Side};
use super::far::FarLayer;
use super::fingerprint;

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
    /// Issue #180 (GD § 16, rule 3): bumps, a real rise and fall of the road (not a ramp), as
    /// high as a kart and its driver at least ([`MIN_BUMP_HEIGHT`]): a fast kart takes off from
    /// its crest (a trick works there, GD § 5), a slow one rolls over it.
    #[serde(default)]
    pub bumps: Vec<BumpData>,
    /// Issue #180: drops, a clean step down in the road: it stops at the top and goes on lower
    /// in the same line, nothing joining the two (a trick works off the edge). Not a fall: the
    /// pelican does not come.
    #[serde(default)]
    pub drops: Vec<DropData>,
    /// Lakes (GD § 10, issue #20): a basin dug in the ground, full of water up to `level`. A kart
    /// whose center is under the water is under water (the stats « eau »). The road may dip
    /// below the ground into a lake only.
    #[serde(default)]
    pub lakes: Vec<LakeData>,
    /// Anti-gravity zones (GD § 10, issue #78): stretches of the centerline where the karts'
    /// gravity follows the road's normal, so that they drive on walls and ceilings. Only there may
    /// the road bank beyond 45° (up to [`MAX_ANTI_GRAVITY_BANK`]); the zone must begin and end
    /// where the road is still flat enough to drive on without it. It may turn (issue #96: a turn
    /// banked like a wall); then banked into the turn, its normal toward the inside, or the karts
    /// fly off it.
    #[serde(default)]
    pub anti_gravity: Vec<AntiGravityData>,
    /// Issue #107 (GD « Conception des circuits »): stretches with no wall on one side of the road
    /// or both: its shoulder ends on a drop (or a slope). A kart that goes over falls, and the tow
    /// pelican drops it back **before** the stretch (no drop point in it).
    #[serde(default)]
    pub open_edges: Vec<OpenEdgeData>,
    /// Issue #107: shortcuts off the centerline (a hidden road, a grass path, a risky jump over
    /// a drop), from the centerline at `from` to the centerline at `to`. The race counts them
    /// right (`crate::race::Course::project`); the bots take them by level (`crate::bots`).
    #[serde(default)]
    pub shortcuts: Vec<ShortcutData>,
    /// Issue #144 (GD « Conception des circuits », step 2): obstacles on the road, fixed (a rock,
    /// a trunk, a pillar: the karts bump into them like into a wall) or moving (a rolling log, a
    /// swinging stone: they knock a kart over). Their motion comes from the race clock and the
    /// race seed: the same on every peer (`crate::track::hazard`).
    #[serde(default)]
    pub obstacles: Vec<ObstacleData>,
    /// Issue #144: passers-by (original creatures) that cross or guard a part of the road, back
    /// and forth: they knock a kart over.
    #[serde(default)]
    pub npcs: Vec<NpcData>,
    /// Issue #144: things to drive through (hay bales, balloons, crates), in rows across the road
    /// like the item boxes: they burst, slow the kart down a little (or not at all), and come
    /// back after a while.
    #[serde(default)]
    pub breakables: Vec<BreakableRow>,
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
    /// Issue #104: its music, a piece of `assets/data/music.ron` (`music: "id"`). None: its
    /// biome's (see [`TrackData::music_id`]).
    #[serde(default)]
    pub music: Option<String>,
    /// Issue #146: more far decor than its biome's (`super::far`: a range, peaks, pieces...),
    /// placed after it. Only on a circuit with a biome.
    #[serde(default)]
    pub far_decor: Vec<FarLayer>,
    /// Issue #187: its own road's look (`super::biome::RoadLook`: its paving, its borders, and
    /// maybe their colors), in place of its biome's. None: its biome's. Purely what it looks like.
    #[serde(default)]
    pub road: Option<RoadLook>,
    /// Issue #119 (GD § 1, « Les courses longues »): an **open** circuit, a long race from its
    /// start to a finish elsewhere, in sections instead of laps (`open: (finish: 600.0, ...)`).
    /// None: a circuit of laps, as before.
    #[serde(default)]
    pub open: Option<OpenData>,
    /// Issue #19: this is the mirror image of the file ([`TrackData::mirrored`]), never written in
    /// a file. The starting grid follows (the pole on the other side).
    #[serde(skip)]
    pub mirrored: bool,
    /// Issue #157: the circuit's fingerprint (`super::fingerprint`): its file's text and its
    /// biome's data, computed as the file is read ([`TrackData::from_ron`]), never written in a
    /// file. The time trial keeps each record with it. The same mirrored.
    #[serde(skip)]
    pub fingerprint: u64,
}

/// Issue #119: an open circuit (GD § 1, « Les courses longues »): one long way from the start line
/// (point 0) to the finish line, in sections. Its centerline still closes up (a Catmull-Rom loop,
/// as every circuit's): its last stretch, **the way back** from `road_end` round to `road_start`,
/// is never built nor driven (no road, no wall, no decor, no checkpoint), and a barrier closes the
/// road at both ends. The race runs once from the start line to the finish (no lap); the karts
/// that finished coast down the run-out after it; the grid sits on the lead-in before the start
/// line. Every position is a spline parameter (like `at`), in this order along the loop: the
/// start line (0), `sections`, `finish`, `road_end`, the way back, `road_start` (before the last
/// point), the start line again.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OpenData {
    /// The finish line.
    pub finish: f32,
    /// Where the road ends, after the run-out ([`MIN_RUN_OUT`] at least after the finish).
    pub road_end: f32,
    /// Where the road begins, on the lead-in ([`MIN_LEAD_IN`] at least before the start line,
    /// round the loop): the grid stands on it.
    pub road_start: f32,
    /// Where each section after the first one begins (the start line begins the first one): « Section 2/3 »...
    pub sections: Vec<f32>,
}

/// Issue #119: an open circuit's run-out is this long at least (m): a kart that crossed the finish
/// line at full speed coasts to a stop before its end (from 35 m/s at `KartTuning::coast_decel`,
/// 6 m/s², about 100 m).
pub const MIN_RUN_OUT: f32 = 130.0;
/// Issue #119: its lead-in, before the start line, is this long at least (m): the grid (38 m) and
/// the race's backward window behind it.
pub const MIN_LEAD_IN: f32 = 70.0;
/// Issue #119: a section is this long at least (m), and there are 2 to [`MAX_SECTIONS`] of them.
pub const MIN_SECTION: f32 = 200.0;
pub const MAX_SECTIONS: usize = 6;

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

/// Issue #180: a bump of the road, from `at` (a spline parameter, where it starts) over `length`
/// meters along the centerline: the road rises `height` meters and falls back, a smooth hump
/// (half a cosine wave up, half down), its walls, shoulders and skirts with it. Its middle half
/// (around the crest, where a kart takes off) is a trick ramp.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BumpData {
    pub at: f32,
    pub height: f32,
    pub length: f32,
}

/// Issue #180: a drop, a step down of `height` meters at `at` (a spline parameter): the segment
/// of the centerline it is on (from point `floor(at)` to the next one) is level with its first
/// point up to `at`, and with the next one after it. Those two points are `height` apart, the
/// upper one first (the file is refused otherwise), and the road reaches and leaves them level. The
/// last [`DROP_LIP`] meters of the upper road are a trick ramp.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DropData {
    pub at: f32,
    pub height: f32,
}

/// Issue #180 (Max: « ces bosses font au moins la hauteur du kart + perso »): a bump is this high
/// at least (m). A kart and its tallest driver stand about 2 m high (the elephant, its ears; a
/// test measures it).
pub const MIN_BUMP_HEIGHT: f32 = 2.2;
/// Issue #180: and this high at most (m).
pub const MAX_BUMP_HEIGHT: f32 = 4.0;
/// Issue #180: a bump is from this long to [`MAX_BUMP_LENGTH`] (m)...
pub const MIN_BUMP_LENGTH: f32 = 16.0;
pub const MAX_BUMP_LENGTH: f32 = 60.0;
/// ...and its steepest slope (`π × height / length`) this at most: the karts drive up it.
pub const MAX_BUMP_SLOPE: f32 = 0.35;
/// Issue #180: the share of a bump, around its crest, that is a trick ramp: from this far into
/// it to this far from its end (its convex part, where a fast kart leaves the road).
pub const BUMP_TRICK_SHARE: f32 = 0.25;
/// Issue #180 (Max: « par exemple 2 à 6 m »): a drop is from this high to [`MAX_DROP_HEIGHT`]
/// (m): at most twice `RaceTuning::fall_depth` (5 m), so that a kart at the foot of the step is
/// never counted below the upper road by more than it (the race projects it on the closest road).
pub const MIN_DROP_HEIGHT: f32 = 2.0;
pub const MAX_DROP_HEIGHT: f32 = 8.0;
/// Issue #180: the last meters of the upper road before a drop are a trick ramp (m).
pub const DROP_LIP: f32 = 8.0;
/// Issue #180: after a drop, the lower road runs on this far at least (m) with nothing in the way
/// (no gap, zone, ramp, bump or other drop), turning [`DROP_RUN_OUT_TURN`] at most: a kart off the
/// edge at full speed lands on it, in its line.
pub const DROP_RUN_OUT: f32 = 35.0;
pub const DROP_RUN_OUT_TURN: f32 = 30.0;

/// An anti-gravity zone (issue #78), between two spline parameters.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AntiGravityData {
    pub from: f32,
    pub to: f32,
}

/// Issue #107: the side(s) of the road a stretch without wall is on (seen by the driver).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeSide {
    Left,
    Right,
    Both,
}

impl EdgeSide {
    /// The same side on the mirrored circuit (issue #19): left and right swap.
    pub fn mirrored(self) -> Self {
        match self {
            EdgeSide::Left => EdgeSide::Right,
            EdgeSide::Right => EdgeSide::Left,
            EdgeSide::Both => EdgeSide::Both,
        }
    }

    /// Whether it opens the side `sign` of the road (-1 left, 1 right).
    pub fn opens(self, sign: f32) -> bool {
        match self {
            EdgeSide::Both => true,
            EdgeSide::Left => sign < 0.0,
            EdgeSide::Right => sign > 0.0,
        }
    }
}

/// Issue #107: a stretch of the road with no wall on `side`, between two spline parameters.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OpenEdgeData {
    pub from: f32,
    pub to: f32,
    pub side: EdgeSide,
}

/// Issue #107: what a shortcut is made of.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutKind {
    /// A grass path, no wall: off-road (GD § 2: about 35 % of the top speed since #193, 40 %
    /// since #171), worth it only with a boost (which ignores the off-road). Issue #193 (Max:
    /// « je veux que les raccourcis soient tous en herbe, même ceux cachés ») : every shortcut is
    /// one, the hidden ones too; the hidden road of before (`Road`, asphalt and curbs) is gone,
    /// and a file that still says `kind: Road` is refused.
    Grass,
    /// A risky jump: a trick ramp on the road, then nothing (the drop) until the landing on the
    /// road further on. Short, and the kart falls. Issue #193: its ramp is grass (off-road): a
    /// kart without a boost slows down on it and falls short; with a boost, it flies.
    Jump,
}

/// Issue #107: a shortcut off the centerline, from the centerline at `from` to the centerline
/// at `to` (spline parameters), along `points`: its first point on the road near `from`, its
/// last one on the road near `to`, both on a side of the road without wall (`open_edges`). A
/// `Grass` shortcut is built `width` meters wide along its points (their height its
/// surface's); a `Jump` is built as its trick ramp only: from `points[1]` toward `points[2]`
/// (`points[0]`, on the road before it, lines the karts up with it), the flight over the drop
/// to the landing near its last point.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShortcutData {
    pub from: f32,
    pub to: f32,
    pub kind: ShortcutKind,
    /// Its path, (x, y, z) in meters, at least 2 points (4 for a jump).
    pub points: Vec<(f32, f32, f32)>,
    /// Its width (m): what is built, and the corridor the race counts on.
    #[serde(default = "default_shortcut_width")]
    pub width: f32,
    /// A jump's trick ramp: its width, length (m) and slope (degrees).
    #[serde(default = "default_ramp_width")]
    pub ramp_width: f32,
    #[serde(default = "default_jump_ramp_length")]
    pub ramp_length: f32,
    #[serde(default = "default_jump_ramp_angle")]
    pub ramp_angle: f32,
    /// Issue #156 (GD « Conception des circuits »): what closes or hides its entrance, if
    /// anything (a grass path; never a jump): `cover: Some((kind: Panel, at: 18.0))`.
    #[serde(default)]
    pub cover: Option<CoverData>,
}

/// Issue #156: what closes or hides a shortcut's entrance (GD « Conception des circuits »: a
/// shortcut is hidden, and opens only to who looks for it or has what it takes).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoverKind {
    /// A big wooden sign board across the path, a little askew: solid without a boost (the kart
    /// bounces gently off it), it bursts into splinters under a kart with a boost going (a
    /// mini-turbo, a pad, a piment), and comes back after a while
    /// (`HazardTuning::panel_respawn_seconds`).
    Panel,
    /// A curtain of tall grass and leaves across the path: one drives through it without slowing
    /// down, it hides what is behind it.
    Foliage,
    /// Issue #110: a waterfall across the path, falling from a mossy lintel on two rocks: one
    /// drives through it without slowing down (only seen, like the foliage), it hides what is
    /// behind it.
    Waterfall,
    /// Issue #110: a stack of crates across the path, two and three high: it bursts as a kart
    /// drives through it (splinters, a « pof », a little slower: a crate's
    /// `HazardTuning::crate_slowdown`), and comes back after a while
    /// (`HazardTuning::cover_respawn_seconds`, issue #193: sooner than the crates on the road).
    Crates,
}

/// Issue #156: a cover of a shortcut, `at` meters along its path from its start, across it.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CoverData {
    pub kind: CoverKind,
    pub at: f32,
}

/// Issue #156: a cover is this wide (m): the path it closes is narrower, by a margin on each
/// side ([`COVER_OVERLAP`]). Wider than a path of the guide (4 to 5 m): seen at an angle, it
/// still hides the way behind it.
pub const COVER_WIDTH: f32 = 8.0;
/// Issue #156: a cover overlaps each side of its path by this much at least (m).
pub const COVER_OVERLAP: f32 = 0.25;

impl ShortcutData {
    pub fn point(&self, i: usize) -> Vec3 {
        let (x, y, z) = self.points[i];
        Vec3::new(x, y, z)
    }
}

fn default_shortcut_width() -> f32 {
    9.0
}
fn default_jump_ramp_length() -> f32 {
    8.0
}
fn default_jump_ramp_angle() -> f32 {
    15.0
}

/// Issue #144: what an obstacle is. The first three stand still (a static collider: the karts
/// bump into them like into a wall); the last two move (no collider: touched, they knock a kart
/// over).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObstacleKind {
    /// A boulder, about 2.2 m wide and 1.6 m high (at `size` 1).
    Rock,
    /// A fallen trunk lying on the road, about 5.9 m long (turned by `yaw` from the road's
    /// direction).
    Trunk,
    /// A pillar of rock, about 1.6 m wide and 6 m high.
    Pillar,
    /// A log lying along the road, rolling across it back and forth over `sweep`.
    RollingLog,
    /// A stone hanging over the road from an arch, swinging across it over `sweep`.
    SwingingStone,
}

impl ObstacleKind {
    /// Whether it moves (and knocks a kart over) rather than stands still.
    pub fn moves(self) -> bool {
        matches!(self, ObstacleKind::RollingLog | ObstacleKind::SwingingStone)
    }
}

/// Issue #144: an obstacle on the road, at the spline parameter `at`, `offset` across it (-1 the
/// left edge, 1 the right one; up to 1.3: on the shoulder).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObstacleData {
    pub kind: ObstacleKind,
    pub at: f32,
    #[serde(default)]
    pub offset: f32,
    /// Its scale (1: the sizes of [`ObstacleKind`]).
    #[serde(default = "default_hazard_size")]
    pub size: f32,
    /// Turned this much from the road's direction (degrees, a fixed obstacle).
    #[serde(default)]
    pub yaw: f32,
    /// A moving one: how far it goes on each side of `offset` (the same unit: 1 = half the
    /// road's width). 0 for a fixed one.
    #[serde(default)]
    pub sweep: f32,
    /// A moving one: the time of a whole back and forth (s).
    #[serde(default = "default_hazard_period")]
    pub period: f32,
}

/// Issue #144: the passers-by, original creatures (never a character of another brand).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NpcKind {
    /// A big round woolly bison, slow and stubborn: it guards a passage, pacing across it.
    Bison,
}

/// Issue #144: a passer-by, pacing back and forth across the road at the spline parameter `at`,
/// between `offset - sweep` and `offset + sweep` (1 = half the road's width), stopping a moment
/// at each end; touched, it knocks a kart over.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NpcData {
    pub kind: NpcKind,
    pub at: f32,
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub sweep: f32,
    /// The time of a whole back and forth, its stops included (s).
    #[serde(default = "default_hazard_period")]
    pub period: f32,
    #[serde(default = "default_hazard_size")]
    pub size: f32,
}

/// Issue #144: what bursts when a kart drives through it.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BreakableKind {
    /// A bale of hay: a cloud of straw, a little slower.
    HayBale,
    /// A balloon on a string: it pops, no slower.
    Balloon,
    /// A wooden crate: splinters, a very little slower.
    Crate,
}

/// Issue #144: a row of things to drive through, across the road at the spline parameter `at`
/// (like a row of item boxes).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BreakableRow {
    pub kind: BreakableKind,
    pub at: f32,
    pub offsets: Vec<f32>,
}

fn default_hazard_size() -> f32 {
    1.0
}
fn default_hazard_period() -> f32 {
    6.0
}

/// The steepest banking outside the anti-gravity zones (degrees): the karts drive there with the
/// world's gravity.
pub const MAX_BANK: f32 = 45.0;
/// The steepest banking in an anti-gravity zone (degrees): a wall at 90° (issue #78), the road
/// upside down at 180° (issue #94: a ceiling, the karts under it).
pub const MAX_ANTI_GRAVITY_BANK: f32 = 180.0;
/// How much the centerline turns over the anti-gravity zone `zone` (issue #94): the sum of the
/// changes of its heading seen from above, sample to sample (degrees). Issue #96: a zone may turn
/// (the bots' racing line follows it in the track's own coordinates, `bots::line`); this is only
/// a measure now.
pub fn anti_gravity_turn(center: &super::layout::CenterLine, zone: &AntiGravityData) -> f32 {
    let from = center.param_to_distance(zone.from);
    let length = center.wrap(center.param_to_distance(zone.to) - from);
    let step = center.length() / center.samples().len() as f32;
    let count = (length / step).ceil().max(1.0) as usize;
    let heading = |k: usize| {
        let forward = center.at(from + length * k as f32 / count as f32).forward;
        Vec2::new(forward.x, forward.z).normalize_or_zero()
    };
    (1..=count)
        .map(|k| {
            let (a, b) = (heading(k - 1), heading(k));
            a.perp_dot(b).atan2(a.dot(b)).abs()
        })
        .sum::<f32>()
        .to_degrees()
}

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
    /// Issue #104: the name of its music: its own, or its biome's (`music` in `biomes.ron`).
    pub fn music_id(&self) -> Option<&str> {
        self.music.as_deref().or_else(|| {
            self.biome
                .as_deref()
                .and_then(biome::biome)
                .and_then(|biome| biome.music.as_deref())
        })
    }

    /// Parses a track file. Misspelled fields are errors, not silently ignored. An optional
    /// field is written as its value (`biome: "Prairie"`, not `Some("Prairie")`).
    /// Issue #157: its fingerprint is computed here, from `text` and its biome.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        let mut data: Self = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)?;
        data.fingerprint =
            fingerprint::circuit_fingerprint(text, data.biome.as_deref().and_then(biome::biome));
        Ok(data)
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
        self.validate_edges_and_shortcuts()?;
        self.validate_hazards()?;
        self.validate_open()?;
        self.validate_relief()?;
        // Issue #187: its own road's look.
        if let Some(road) = &self.road {
            road.validate().or_else(fail)?;
        }
        // Issue #146: its own far decor, on top of its biome's.
        if !self.far_decor.is_empty() && self.biome.is_none() {
            return fail("far_decor without a biome".to_string());
        }
        for layer in &self.far_decor {
            layer.validate().or_else(fail)?;
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

    /// Issue #107: the stretches without wall (each on a side, out of the anti-gravity zones and
    /// the gaps, half the circuit at most) and the shortcuts (each from one side of the road
    /// without wall to another, its points apart, as wide as a lane at least; a jump with its
    /// ramp). Their mouths are checked on the centerline: the first point near the road at
    /// `from`, the last one near the road at `to`, on a side without wall there.
    fn validate_edges_and_shortcuts(&self) -> Result<(), InvalidTrack> {
        let fail = |message: String| Err(InvalidTrack(message));
        let n = self.points.len() as f32;
        let within =
            |from: f32, to: f32, p: f32| (p - from).rem_euclid(n) <= (to - from).rem_euclid(n);
        for edge in &self.open_edges {
            let length = (edge.to - edge.from).rem_euclid(n);
            if !(edge.from.is_finite() && edge.to.is_finite()) || length < 0.05 || length > n / 2.0
            {
                return fail(format!(
                    "open edge {}..{}: empty, or more than half the circuit",
                    edge.from, edge.to
                ));
            }
            let overlaps = |from: f32, to: f32| {
                within(from, to, edge.from) || within(edge.from, edge.to, from)
            };
            if self.anti_gravity.iter().any(|z| overlaps(z.from, z.to))
                || self.gaps.iter().any(|g| overlaps(g.from, g.to))
            {
                return fail(format!(
                    "open edge {}..{} on an anti-gravity zone or a gap",
                    edge.from, edge.to
                ));
            }
        }
        if self.shortcuts.is_empty() {
            return Ok(());
        }
        let center = super::layout::CenterLine::of(self);
        for (k, shortcut) in self.shortcuts.iter().enumerate() {
            let span = (shortcut.to - shortcut.from).rem_euclid(n);
            let least = if shortcut.kind == ShortcutKind::Jump {
                4
            } else {
                2
            };
            let finite = shortcut.from.is_finite()
                && shortcut.to.is_finite()
                && shortcut
                    .points
                    .iter()
                    .all(|(x, y, z)| x.is_finite() && y.is_finite() && z.is_finite());
            if !finite
                || shortcut.points.len() < least
                || span < 0.05
                || span > n / 2.0
                || !(4.0..=30.0).contains(&shortcut.width)
            {
                return fail(format!(
                    "shortcut {k}: {} points ({least} at least), {}..{} (empty, or more than half \
                     the circuit), width {} (4 to 30 m)",
                    shortcut.points.len(),
                    shortcut.from,
                    shortcut.to,
                    shortcut.width
                ));
            }
            if shortcut
                .points
                .windows(2)
                .any(|w| Vec3::from(w[0]).distance(Vec3::from(w[1])) < 1.0)
            {
                return fail(format!("shortcut {k}: two points on top of each other"));
            }
            if shortcut.kind == ShortcutKind::Jump
                && (!(2.0..=20.0).contains(&shortcut.ramp_length)
                    || !(3.0..=30.0).contains(&shortcut.ramp_angle)
                    || !(2.0..=shortcut.width).contains(&shortcut.ramp_width))
            {
                return fail(format!(
                    "shortcut {k}: its ramp {} m long (2 to 20), {}° (3 to 30), {} m wide (2 to \
                     its width)",
                    shortcut.ramp_length, shortcut.ramp_angle, shortcut.ramp_width
                ));
            }
            // Issue #156: its cover, on its path (a grass path), across all of it.
            if let Some(cover) = shortcut.cover {
                let length = super::shortcut::Shortcut::new(&center, shortcut).length();
                if shortcut.kind == ShortcutKind::Jump
                    || !cover.at.is_finite()
                    || cover.at < 2.0
                    || cover.at > length - 2.0
                    || shortcut.width > COVER_WIDTH - 2.0 * COVER_OVERLAP
                {
                    return fail(format!(
                        "shortcut {k}: its cover {:?} at {} m (2 m from its ends at least, on a \
                         path of {length:.0} m), on a {:?} of {} m ({} m at most; never on a jump)",
                        cover.kind,
                        cover.at,
                        shortcut.kind,
                        shortcut.width,
                        COVER_WIDTH - 2.0 * COVER_OVERLAP
                    ));
                }
            }
            // Its mouths: near the road at `from` and `to`, on a side without wall there.
            let ends = [
                (shortcut.from, shortcut.point(0)),
                (shortcut.to, shortcut.point(shortcut.points.len() - 1)),
            ];
            for (param, point) in ends {
                let s = center.at(center.param_to_distance(param));
                let offset = point - s.position;
                let (along, lateral, height) =
                    (offset.dot(s.forward), offset.dot(s.right), offset.dot(s.up));
                let reach = super::mesh::wall_offset(&s) + shortcut.width;
                let open = self
                    .open_edges
                    .iter()
                    .any(|edge| edge.side.opens(lateral) && within(edge.from, edge.to, param));
                if along.abs() > shortcut.width
                    || height.abs() > 3.0
                    || lateral.abs() > reach
                    || !open
                {
                    return fail(format!(
                        "shortcut {k}: its end at {param} is not by the road there ({along:.1} m \
                         along, {lateral:.1} m across, {reach:.1} at most, {height:.1} m above), \
                         or not on a side without wall"
                    ));
                }
            }
        }
        Ok(())
    }

    /// Issue #144: the obstacles, the passers-by and the things to drive through: on the road
    /// (out of the gaps and the anti-gravity zones), a sensible size, a moving one with its sweep
    /// and its period, a fixed one without; the road's free part never closed (a moving one or a
    /// passer-by sweeps one side of the road at most: from 0.1 off the middle).
    fn validate_hazards(&self) -> Result<(), InvalidTrack> {
        let fail = |message: String| Err(InvalidTrack(message));
        let n = self.points.len() as f32;
        let within =
            |from: f32, to: f32, p: f32| (p - from).rem_euclid(n) <= (to - from).rem_euclid(n);
        let placed = |at: f32| {
            at.is_finite()
                && !self.gaps.iter().any(|g| within(g.from, g.to, at))
                && !self.anti_gravity.iter().any(|z| within(z.from, z.to, at))
        };
        // A sweep leaves the other side of the road free: it never crosses the middle by more
        // than this (in half widths).
        const MIDDLE: f32 = 0.1;
        let one_side = |offset: f32, sweep: f32| {
            let (low, high) = (offset - sweep, offset + sweep);
            high <= 1.3 && low >= -1.3 && (low >= -MIDDLE || high <= MIDDLE)
        };
        for (k, o) in self.obstacles.iter().enumerate() {
            let moving = o.kind.moves();
            let motion = if moving {
                (0.05..=1.0).contains(&o.sweep)
                    && (1.0..=30.0).contains(&o.period)
                    && one_side(o.offset, o.sweep)
            } else {
                o.sweep == 0.0
            };
            if !placed(o.at)
                || !(-1.3..=1.3).contains(&o.offset)
                || !(0.3..=4.0).contains(&o.size)
                || !o.yaw.is_finite()
                || !motion
            {
                return fail(format!(
                    "obstacle {k} ({:?}): at {} (on the road, out of the gaps and the anti-gravity                      zones), offset {} (1.3 at most), size {} (0.3 to 4), sweep {} (0 when fixed;                      0.05 to 1 when moving, on one side of the road), period {} (1 to 30 s)",
                    o.kind, o.at, o.offset, o.size, o.sweep, o.period
                ));
            }
        }
        for (k, npc) in self.npcs.iter().enumerate() {
            if !placed(npc.at)
                || !(0.0..=1.0).contains(&npc.sweep)
                || !(1.0..=30.0).contains(&npc.period)
                || !(0.5..=2.0).contains(&npc.size)
                || !one_side(npc.offset, npc.sweep)
            {
                return fail(format!(
                    "passer-by {k} ({:?}): at {}, offset {} and sweep {} (on one side of the                      road), period {} (1 to 30 s), size {} (0.5 to 2)",
                    npc.kind, npc.at, npc.offset, npc.sweep, npc.period, npc.size
                ));
            }
        }
        for (k, row) in self.breakables.iter().enumerate() {
            if !placed(row.at)
                || row.offsets.is_empty()
                || row.offsets.iter().any(|o| !(-1.0..=1.0).contains(o))
            {
                return fail(format!(
                    "row of things to drive through {k} ({:?}): at {}, offsets {:?} (at least \
                     one, each from -1 to 1)",
                    row.kind, row.at, row.offsets
                ));
            }
        }
        Ok(())
    }

    /// Issue #119: an open circuit's sections, finish and ends, in their order along the loop, a
    /// run-out, a lead-in and sections long enough; nothing of the circuit on its way back (a
    /// gap, an anti-gravity zone, a side without wall, a shortcut, an obstacle, a passer-by, a
    /// thing to drive through, a ramp, a pad, a box, a croquette, a sand patch, a drop point, a
    /// prop).
    fn validate_open(&self) -> Result<(), InvalidTrack> {
        let Some(open) = &self.open else {
            return Ok(());
        };
        let fail = |message: String| Err(InvalidTrack(message));
        let n = self.points.len() as f32;
        let mut order = vec![0.0];
        order.extend(&open.sections);
        order.extend([open.finish, open.road_end, open.road_start, n]);
        if order.iter().any(|p| !p.is_finite())
            || order.windows(2).any(|w| w[0] >= w[1])
            || open.sections.is_empty()
            || open.sections.len() + 1 > MAX_SECTIONS
        {
            return fail(format!(
                "open circuit: 0 < sections {:?} < finish {} < road_end {} < road_start {} < {n}                  (the points), 2 to {MAX_SECTIONS} sections",
                open.sections, open.finish, open.road_end, open.road_start
            ));
        }
        let center = super::layout::CenterLine::of(self);
        let distance = |p: f32| center.param_to_distance(p);
        let run_out = distance(open.road_end) - distance(open.finish);
        let lead_in = center.length() - distance(open.road_start);
        if run_out < MIN_RUN_OUT || lead_in < MIN_LEAD_IN {
            return fail(format!(
                "open circuit: a run-out of {run_out:.0} m ({MIN_RUN_OUT} at least), a lead-in of                  {lead_in:.0} m ({MIN_LEAD_IN} at least)"
            ));
        }
        let starts: Vec<f32> = std::iter::once(0.0)
            .chain(open.sections.iter().copied())
            .chain([open.finish])
            .collect();
        if let Some(w) = starts
            .windows(2)
            .find(|w| distance(w[1]) - distance(w[0]) < MIN_SECTION)
        {
            return fail(format!(
                "open circuit: the section from {} to {} is shorter than {MIN_SECTION} m",
                w[0], w[1]
            ));
        }
        // On the road: from `road_start` round to `road_end` (a range's both ends, and the way
        // back never inside it).
        let on_road = |p: f32| {
            let p = p.rem_euclid(n);
            p <= open.road_end || p >= open.road_start
        };
        let range_on_road = |from: f32, to: f32| {
            on_road(from) && on_road(to) && {
                let (from, to) = (from.rem_euclid(n), to.rem_euclid(n));
                // Through the start line (from the lead-in), or forward before the end.
                if from >= open.road_start {
                    to >= from || to <= open.road_end
                } else {
                    to >= from && to <= open.road_end
                }
            }
        };
        let ranges = self
            .gaps
            .iter()
            .map(|g| ("gap", g.from, g.to))
            .chain(
                self.anti_gravity
                    .iter()
                    .map(|z| ("anti-gravity zone", z.from, z.to)),
            )
            .chain(self.open_edges.iter().map(|e| ("open edge", e.from, e.to)))
            .chain(self.shortcuts.iter().map(|s| ("shortcut", s.from, s.to)))
            .chain(self.offroad.iter().map(|z| ("sand patch", z.from, z.to)));
        for (what, from, to) in ranges {
            if !range_on_road(from, to) {
                return fail(format!(
                    "open circuit: the {what} {from}..{to} is on the way back (off the road)"
                ));
            }
        }
        let spots = self
            .obstacles
            .iter()
            .map(|o| ("obstacle", o.at))
            .chain(self.npcs.iter().map(|p| ("passer-by", p.at)))
            .chain(
                self.breakables
                    .iter()
                    .map(|b| ("thing to drive through", b.at)),
            )
            .chain(
                self.trick_ramps
                    .iter()
                    .chain(&self.glide_ramps)
                    .map(|r| ("ramp", r.at)),
            )
            .chain(self.boost_pads.iter().map(|p| ("boost pad", p.at)))
            .chain(self.item_boxes.iter().map(|b| ("item box", b.at)))
            .chain(self.croquettes.iter().map(|c| ("croquette", c.from)))
            .chain(self.respawns.iter().map(|r| ("drop point", *r)))
            .chain(self.props.iter().map(|p| ("prop", p.at)));
        for (what, at) in spots {
            if !on_road(at) {
                return fail(format!(
                    "open circuit: the {what} at {at} is on the way back (off the road)"
                ));
            }
        }
        Ok(())
    }

    /// Issue #180: the bumps and the drops. A bump: from [`MIN_BUMP_HEIGHT`] to
    /// [`MAX_BUMP_HEIGHT`] high, from [`MIN_BUMP_LENGTH`] to [`MAX_BUMP_LENGTH`] long, its slope
    /// [`MAX_BUMP_SLOPE`] at most. A drop: from [`MIN_DROP_HEIGHT`] to [`MAX_DROP_HEIGHT`] high,
    /// inside its segment (a tenth of it from each end at least), whose two points are its height
    /// apart, the upper one first; the lower road straight enough ([`DROP_RUN_OUT_TURN`]) for
    /// [`DROP_RUN_OUT`] meters after it. Each one clear of the others (a drop's lip and run-out
    /// included), of the gaps, the anti-gravity zones, the ramps, the start line and its grid; on
    /// an open circuit, on its road.
    fn validate_relief(&self) -> Result<(), InvalidTrack> {
        if self.bumps.is_empty() && self.drops.is_empty() {
            return Ok(());
        }
        let fail = |message: String| Err(InvalidTrack(message));
        let n = self.points.len();
        for (k, bump) in self.bumps.iter().enumerate() {
            if !bump.at.is_finite()
                || !(MIN_BUMP_HEIGHT..=MAX_BUMP_HEIGHT).contains(&bump.height)
                || !(MIN_BUMP_LENGTH..=MAX_BUMP_LENGTH).contains(&bump.length)
                || std::f32::consts::PI * bump.height / bump.length > MAX_BUMP_SLOPE
            {
                return fail(format!(
                    "bump {k} at {}: {} m high ({MIN_BUMP_HEIGHT} to {MAX_BUMP_HEIGHT}), {} m long \
                     ({MIN_BUMP_LENGTH} to {MAX_BUMP_LENGTH}), its slope π × height / length \
                     {MAX_BUMP_SLOPE} at most",
                    bump.at, bump.height, bump.length
                ));
            }
        }
        for (k, drop) in self.drops.iter().enumerate() {
            let at = drop.at.rem_euclid(n as f32);
            let along = at - at.floor();
            let i = (at.floor() as usize).min(n - 1);
            let (upper, lower) = (self.points[i].pos.1, self.points[(i + 1) % n].pos.1);
            if !drop.at.is_finite()
                || !(MIN_DROP_HEIGHT..=MAX_DROP_HEIGHT).contains(&drop.height)
                || !(0.1..=0.9).contains(&along)
                || (upper - lower - drop.height).abs() > 0.05
            {
                return fail(format!(
                    "drop {k} at {}: {} m high ({MIN_DROP_HEIGHT} to {MAX_DROP_HEIGHT}), inside its \
                     segment (0.1 to 0.9 along it), its points {i} and {} that much apart (the \
                     upper one first: {upper} and {lower})",
                    drop.at,
                    drop.height,
                    (i + 1) % n
                ));
            }
            let segment = |d: &DropData| (d.at.rem_euclid(n as f32).floor() as usize).min(n - 1);
            if self.drops[..k].iter().any(|other| {
                let gap = (segment(other) + n - i) % n;
                gap <= 1 || gap >= n - 1
            }) {
                return fail(format!(
                    "drop {k} at {}: on the segment of another drop or next to it",
                    drop.at
                ));
            }
        }
        let center = super::layout::CenterLine::of(self);
        let length = center.length();
        let distance = |p: f32| center.param_to_distance(p);
        // What each one takes along the track, as (what, start, length) (m).
        let mut taken: Vec<(String, f32, f32)> = Vec::new();
        for (k, bump) in center.bumps().iter().enumerate() {
            taken.push((format!("bump {k}"), bump.from, bump.length));
        }
        for (k, cliff) in center.cliffs().iter().enumerate() {
            taken.push((
                format!("drop {k}"),
                cliff.at - DROP_LIP,
                DROP_LIP + DROP_RUN_OUT,
            ));
            // The lower road goes on in the line of the upper one.
            let flat = |s: super::layout::TrackSample| Vec2::new(s.forward.x, s.forward.z);
            let turn = flat(center.at(cliff.at + 0.5))
                .angle_to(flat(center.at(cliff.at + DROP_RUN_OUT)))
                .abs()
                .to_degrees();
            if turn > DROP_RUN_OUT_TURN {
                return fail(format!(
                    "drop {k}: the road below it turns {turn:.0}° in {DROP_RUN_OUT} m \
                     ({DROP_RUN_OUT_TURN}° at most)"
                ));
            }
        }
        let overlap = |a: (f32, f32), b: (f32, f32)| {
            (b.0 - a.0).rem_euclid(length) < a.1 || (a.0 - b.0).rem_euclid(length) < b.1
        };
        for (i, (what, start, along)) in taken.iter().enumerate() {
            if let Some((other, ..)) = taken[..i]
                .iter()
                .find(|(_, s, l)| overlap((*start, *along), (*s, *l)))
            {
                return fail(format!("{what} and {other} overlap"));
            }
        }
        let range = |from: f32, to: f32| {
            let start = distance(from);
            (start, (distance(to) - start).rem_euclid(length))
        };
        let others = self
            .gaps
            .iter()
            .map(|g| ("a gap", range(g.from, g.to)))
            .chain(
                self.anti_gravity
                    .iter()
                    .map(|z| ("an anti-gravity zone", range(z.from, z.to))),
            )
            .chain(
                self.trick_ramps
                    .iter()
                    .chain(&self.glide_ramps)
                    .map(|r| ("a ramp", (distance(r.at), r.length))),
            )
            .chain(
                self.shortcuts
                    .iter()
                    .filter(|s| s.kind == ShortcutKind::Jump)
                    .map(|s| ("a risky jump", range(s.from, s.to))),
            )
            // The start line and the grid behind it (its last row 38 m behind the line).
            .chain([("the start line or its grid", (-40.0, 45.0))]);
        for (other, place) in others {
            if let Some((what, ..)) = taken.iter().find(|(_, s, l)| overlap((*s, *l), place)) {
                return fail(format!("{what} on {other}"));
            }
        }
        if center.open().is_some()
            && let Some((what, ..)) = taken
                .iter()
                .find(|(_, s, l)| !center.on_the_road(*s) || !center.on_the_road(s + l))
        {
            return fail(format!("open circuit: {what} is off the road"));
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

    /// Issue #146: its far decor, its biome's and its own layers (None without a biome).
    pub fn far_decor(&self) -> Option<super::far::FarDecor> {
        super::far::far_decor_of(self.biome(), &self.far_decor)
    }

    /// Issue #187: the look of its road: its own (`road` in its file), else its biome's, else the
    /// asphalt of a circuit without biome.
    pub fn road_look(&self) -> RoadLook {
        self.road
            .or_else(|| self.biome().map(|biome| biome.road))
            .unwrap_or_default()
    }

    /// Its colors: its biome's palette (or the one without biome), its road's own colors in it
    /// (issue #187: a circuit's `road` may change its `asphalt` and its `curbs`).
    pub fn palette(&self) -> Palette {
        let palette = self.biome().map_or_else(Palette::default, |b| b.palette);
        self.road.map_or(palette, |road| road.paint(&palette))
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
    /// along the track: they do not move, nor do the anti-gravity zones of issue #78; issue #107: the
    /// sides without wall swap, the shortcuts move across the plane). The centerline of the mirror is exactly the mirror image of
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
        // Issue #107: the stretches without wall on the other side, the shortcuts across the
        // X = 0 plane.
        for edge in &mut data.open_edges {
            edge.side = edge.side.mirrored();
        }
        for shortcut in &mut data.shortcuts {
            for point in &mut shortcut.points {
                point.0 = -point.0;
            }
        }
        // Issue #144: the obstacles, the passers-by and the things to drive through on the other
        // side of the road, turned the other way.
        for obstacle in &mut data.obstacles {
            obstacle.offset = flip(obstacle.offset);
            obstacle.yaw = -obstacle.yaw;
        }
        for npc in &mut data.npcs {
            npc.offset = flip(npc.offset);
        }
        for row in &mut data.breakables {
            for offset in &mut row.offsets {
                *offset = flip(*offset);
            }
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "(name: \"Square\", points: [(pos: (0.0, 0.0, 0.0)), (pos: (0.0, 0.0, -50.0)), (pos: (50.0, 0.0, -50.0)), (pos: (50.0, 0.0, 0.0))])";

    /// Issue #119: an open circuit's file is refused when its ends are out of order, its run-out
    /// or its lead-in too short, a section too short, no section after the first one, or
    /// something of the circuit on its way back; its mirror keeps its ends.
    #[test]
    fn an_open_circuit_that_makes_no_sense_is_rejected() {
        let base = crate::track::test_open_circuit();
        assert_eq!(base.validate(), Ok(()));
        assert_eq!(base.mirrored().open, base.open);
        assert_eq!(base.mirrored().validate(), Ok(()));
        let broken = |change: &dyn Fn(&mut TrackData)| {
            let mut data = base.clone();
            change(&mut data);
            data.validate().unwrap_err().0
        };
        for (what, change) in [
            (
                "order",
                &(|d: &mut TrackData| d.open.as_mut().unwrap().finish = 120.0)
                    as &dyn Fn(&mut TrackData),
            ),
            ("order", &|d: &mut TrackData| {
                d.open.as_mut().unwrap().sections = vec![64.0, 36.0];
            }),
            ("order", &|d: &mut TrackData| {
                d.open.as_mut().unwrap().sections.clear();
            }),
            ("order", &|d: &mut TrackData| {
                d.open.as_mut().unwrap().road_start = 140.0;
            }),
            ("run-out", &|d: &mut TrackData| {
                d.open.as_mut().unwrap().road_end = 100.0;
            }),
            ("lead-in", &|d: &mut TrackData| {
                d.open.as_mut().unwrap().road_start = 135.0;
            }),
            ("shorter", &|d: &mut TrackData| {
                d.open.as_mut().unwrap().sections = vec![36.0, 40.0];
            }),
            ("way back", &|d: &mut TrackData| {
                d.boost_pads.push(Spot {
                    at: 120.0,
                    offset: 0.0,
                });
            }),
            ("way back", &|d: &mut TrackData| {
                d.gaps.push(GapData {
                    from: 110.0,
                    to: 116.0,
                });
            }),
        ] {
            let error = broken(change);
            let wanted = match what {
                "order" => "open circuit: 0 < sections",
                "run-out" => "a run-out of",
                "lead-in" => "a lead-in of",
                "shorter" => "shorter than",
                _ => "on the way back",
            };
            assert!(error.contains(wanted), "{what}: {error}");
        }
    }

    /// Issue #156: a shortcut's cover: on a grass path (never a jump), 2 m from its ends
    /// at least, the path narrow enough for it to stand across it; the mirror keeps it.
    #[test]
    fn a_shortcut_s_cover_that_makes_no_sense_is_rejected() {
        let base = TrackData::from_ron(&MINIMAL.replace(
            "(name:",
            "(open_edges: [(from: 0.3, to: 1.7, side: Right)], \
             shortcuts: [(from: 0.5, to: 1.5, kind: Grass, width: 5.0, \
             cover: Some((kind: Panel, at: 8.0)), \
             points: [(5.0, 0.0, -25.0), (20.0, 0.0, -35.0), (25.0, 0.0, -45.0)])], name:",
        ))
        .unwrap();
        assert_eq!(base.validate(), Ok(()));
        let cover = base.shortcuts[0].cover.unwrap();
        assert_eq!((cover.kind, cover.at), (CoverKind::Panel, 8.0));
        let broken = |change: &dyn Fn(&mut ShortcutData)| {
            let mut data = base.clone();
            change(&mut data.shortcuts[0]);
            data.validate().is_err()
        };
        // Issue #110: the waterfall and the stack of crates stand on a path the same way.
        for kind in [CoverKind::Foliage, CoverKind::Waterfall, CoverKind::Crates] {
            assert!(!broken(&|s| s.cover.as_mut().unwrap().kind = kind));
            assert!(broken(&|s| {
                s.cover.as_mut().unwrap().kind = kind;
                s.width = COVER_WIDTH;
            }));
        }
        assert!(!broken(&|s| s.width = COVER_WIDTH - 2.0 * COVER_OVERLAP));
        assert!(broken(&|s| s.width = COVER_WIDTH));
        assert!(broken(&|s| s.cover.as_mut().unwrap().at = 1.0));
        assert!(broken(&|s| s.cover.as_mut().unwrap().at = 30.0));
        assert!(broken(&|s| s.cover.as_mut().unwrap().at = f32::NAN));
        assert!(broken(&|s| {
            s.kind = ShortcutKind::Jump;
            s.points = vec![
                (3.0, 0.0, -18.0),
                (5.0, 0.0, -25.0),
                (12.0, 0.0, -30.0),
                (25.0, 0.0, -45.0),
            ];
            s.ramp_width = 4.0;
        }));
        let mirrored = base.mirrored();
        assert_eq!(mirrored.shortcuts[0].cover, base.shortcuts[0].cover);
        assert_eq!(mirrored.validate(), Ok(()));
    }

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

    /// Issue #107: the sides without wall and the shortcuts are checked: a stretch on a side, out
    /// of the gaps, not empty; a shortcut with its points apart, as wide as a lane, a jump with
    /// its ramp, each end by the road at its parameter and on a side without wall there. Mirrored,
    /// the sides swap, the shortcuts move across, the file comes back mirrored twice.
    #[test]
    fn sides_without_wall_and_shortcuts_that_make_no_sense_are_rejected() {
        // The square: 50 m sides, its first side heading north (-Z), the right side east.
        let base = TrackData::from_ron(&MINIMAL.replace(
            "(name:",
            "(open_edges: [(from: 0.3, to: 1.7, side: Right)], \
             shortcuts: [(from: 0.5, to: 1.5, kind: Grass, \
             points: [(5.0, 0.0, -25.0), (20.0, 0.0, -35.0), (25.0, 0.0, -45.0)])], name:",
        ))
        .unwrap();
        assert_eq!(base.validate(), Ok(()));
        assert_eq!(base.shortcuts[0].width, 9.0, "the default width");
        // Issue #193: every shortcut is grass (or a jump, its ramp grass): a road is refused.
        let road = MINIMAL.replace(
            "(name:",
            "(open_edges: [(from: 0.3, to: 1.7, side: Right)], \
             shortcuts: [(from: 0.5, to: 1.5, kind: Road, \
             points: [(5.0, 0.0, -25.0), (20.0, 0.0, -35.0), (25.0, 0.0, -45.0)])], name:",
        );
        assert!(
            TrackData::from_ron(&road).is_err(),
            "a road shortcut is refused"
        );
        let broken = |change: &dyn Fn(&mut TrackData)| {
            let mut data = base.clone();
            change(&mut data);
            data.validate().is_err()
        };
        assert!(broken(&|d| d.open_edges[0].to = d.open_edges[0].from));
        assert!(broken(&|d| d.open_edges[0].from = f32::NAN));
        assert!(broken(&|d| d.gaps.push(GapData { from: 1.0, to: 1.2 })));
        // Its mouth on a side with its wall: refused.
        assert!(broken(&|d| d.open_edges[0].side = EdgeSide::Left));
        assert!(broken(&|d| d.open_edges[0].to = 0.4));
        assert!(!broken(&|d| d.open_edges[0].side = EdgeSide::Both));
        // Far from the road at its end, or above it.
        assert!(broken(&|d| d.shortcuts[0].points[2] = (60.0, 0.0, -45.0)));
        assert!(broken(&|d| d.shortcuts[0].points[0].1 = 5.0));
        assert!(broken(&|d| d.shortcuts[0].points.truncate(1)));
        assert!(broken(
            &|d| d.shortcuts[0].points[1] = d.shortcuts[0].points[0]
        ));
        assert!(broken(&|d| d.shortcuts[0].width = 2.0));
        assert!(broken(&|d| d.shortcuts[0].to = d.shortcuts[0].from));
        // A jump: four points at least, and a ramp that makes sense.
        assert!(broken(&|d| d.shortcuts[0].kind = ShortcutKind::Jump));
        let jump = |change: &dyn Fn(&mut ShortcutData)| {
            let mut data = base.clone();
            let shortcut = &mut data.shortcuts[0];
            shortcut.kind = ShortcutKind::Jump;
            shortcut.points = vec![
                (3.0, 0.0, -18.0),
                (5.0, 0.0, -25.0),
                (12.0, 0.0, -30.0),
                (25.0, 0.0, -45.0),
            ];
            change(shortcut);
            data.validate().is_err()
        };
        assert!(!jump(&|_| {}));
        assert!(jump(&|s| s.ramp_angle = 45.0));
        assert!(jump(&|s| s.ramp_length = 1.0));
        assert!(jump(&|s| s.ramp_width = 20.0));
        let mirrored = base.mirrored();
        assert_eq!(mirrored.open_edges[0].side, EdgeSide::Left);
        assert_eq!(mirrored.shortcuts[0].points[0], (-5.0, 0.0, -25.0));
        assert_eq!(mirrored.validate(), Ok(()));
        assert_eq!(mirrored.mirrored(), base);
    }

    /// Issue #144: the obstacles, the passers-by and the things to drive through are checked: on
    /// the road, out of the gaps, a sensible size; a moving one sweeps one side of the road only,
    /// with a period; a fixed one does not sweep; a row has something in it. Mirrored, they go to
    /// the other side of the road, turned the other way; mirrored twice, the file comes back.
    #[test]
    fn obstacles_passers_by_and_things_to_drive_through_that_make_no_sense_are_rejected() {
        let base = TrackData::from_ron(&MINIMAL.replace(
            "(name:",
            "(gaps: [(from: 2.2, to: 2.6)], \
             obstacles: [(kind: Rock, at: 0.5, offset: 0.8, yaw: 20.0), \
             (kind: SwingingStone, at: 1.5, offset: 0.5, sweep: 0.4, period: 4.0)], \
             npcs: [(kind: Bison, at: 3.5, offset: -0.5, sweep: 0.3)], \
             breakables: [(kind: HayBale, at: 0.8, offsets: [-0.5, 0.5])], name:",
        ))
        .unwrap();
        assert_eq!(base.validate(), Ok(()));
        assert_eq!(base.obstacles[0].size, 1.0, "the default size");
        assert_eq!(base.npcs[0].period, 6.0, "the default period");
        let broken = |change: &dyn Fn(&mut TrackData)| {
            let mut data = base.clone();
            change(&mut data);
            data.validate().is_err()
        };
        assert!(broken(&|d| d.obstacles[0].at = 2.4), "in a gap");
        assert!(broken(&|d| d.obstacles[0].at = f32::NAN));
        assert!(broken(&|d| d.obstacles[0].offset = 1.5));
        assert!(broken(&|d| d.obstacles[0].size = 0.1));
        assert!(
            broken(&|d| d.obstacles[0].sweep = 0.3),
            "a rock does not move"
        );
        assert!(
            broken(&|d| d.obstacles[1].sweep = 0.0),
            "a swinging stone does"
        );
        assert!(broken(&|d| d.obstacles[1].period = 0.2));
        // Across the middle of the road: the whole road closed at times.
        assert!(broken(&|d| d.obstacles[1].offset = 0.0));
        assert!(!broken(&|d| d.obstacles[1].offset = 0.45));
        assert!(broken(&|d| d.npcs[0].sweep = 0.8));
        assert!(broken(&|d| d.npcs[0].size = 3.0));
        assert!(broken(&|d| d.npcs[0].at = 2.3), "in a gap");
        assert!(broken(&|d| d.breakables[0].offsets.clear()));
        assert!(broken(&|d| d.breakables[0].offsets[0] = 1.2));
        let mirrored = base.mirrored();
        assert_eq!(mirrored.obstacles[0].offset, -0.8);
        assert_eq!(mirrored.obstacles[0].yaw, -20.0);
        assert_eq!(mirrored.npcs[0].offset, 0.5);
        assert_eq!(mirrored.breakables[0].offsets, vec![0.5, -0.5]);
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

    /// Issue #180: a stadium (two straights of 300 m, two half circles of 50 m), a point every
    /// 10 m: 6 m high from the start line to point 12, a drop of 4 m between points 12 and 13
    /// (at 12.5), 2 m high from there round the far turn, back up to 6 m along the other straight;
    /// a bump on the first straight after the drop.
    fn stadium() -> TrackData {
        let mut points = Vec::new();
        for i in 0..30 {
            points.push((0.0, if i <= 12 { 6.0 } else { 2.0 }, -10.0 * i as f32));
        }
        for i in 0..15 {
            let a = std::f32::consts::PI * (1.0 - i as f32 / 15.0);
            points.push((50.0 + 50.0 * a.cos(), 2.0, -300.0 - 50.0 * a.sin()));
        }
        for i in 0..30 {
            let y = 2.0 + 4.0 * i as f32 / 29.0;
            points.push((100.0, y, -300.0 + 10.0 * i as f32));
        }
        for i in 0..15 {
            let a = std::f32::consts::PI * i as f32 / 15.0;
            points.push((50.0 + 50.0 * a.cos(), 6.0, 50.0 * a.sin()));
        }
        let points: Vec<String> = points
            .iter()
            .map(|(x, y, z)| format!("(pos: ({x}, {y}, {z}))"))
            .collect();
        TrackData::from_ron(&format!(
            "(name: \"Stade\", points: [{}], bumps: [(at: 17.0, height: 2.2, length: 30.0)], \
             drops: [(at: 12.5, height: 4.0)])",
            points.join(", ")
        ))
        .unwrap()
    }

    /// Issue #180: bumps and drops that make no sense are refused: a bump lower than a kart and
    /// its driver, too short or too long, too steep; a drop whose points are not its height
    /// apart, the upper one first, too low or too high, at the very end of its segment, next to
    /// another one; a bump on a drop's lip or on the road below it, on the start line or its
    /// grid, on a ramp, in a gap; a drop whose road below turns away. The mirror keeps them.
    #[test]
    fn bumps_and_drops_that_make_no_sense_are_rejected() {
        let base = stadium();
        assert_eq!(base.validate(), Ok(()));
        assert_eq!(base.mirrored().validate(), Ok(()));
        assert_eq!(base.mirrored().bumps, base.bumps);
        assert_eq!(base.mirrored().drops, base.drops);
        let broken = |change: &dyn Fn(&mut TrackData)| {
            let mut data = base.clone();
            change(&mut data);
            data.validate().is_err()
        };
        assert!(broken(&|d| d.bumps[0].height = 2.0));
        assert!(broken(&|d| d.bumps[0].height = 4.5));
        assert!(broken(&|d| d.bumps[0].length = 12.0));
        assert!(broken(&|d| d.bumps[0].length = 70.0));
        assert!(broken(&|d| {
            d.bumps[0].height = 4.0;
            d.bumps[0].length = 30.0;
        }));
        assert!(!broken(&|d| d.bumps[0].length = 40.0));
        assert!(broken(&|d| d.bumps[0].at = f32::NAN));
        assert!(broken(&|d| d.drops[0].height = 3.0));
        assert!(broken(&|d| d.drops[0].at = 12.02));
        assert!(broken(&|d| d.drops[0].at = 11.5));
        assert!(broken(&|d| {
            d.points[13].pos.1 = 7.0;
            d.drops[0].height = -1.0;
        }));
        assert!(broken(&|d| {
            for p in &mut d.points[..13] {
                p.pos.1 = 3.5;
            }
            d.drops[0].height = 1.5;
        }));
        assert!(broken(&|d| {
            for p in &mut d.points[..13] {
                p.pos.1 = 12.0;
            }
            d.drops[0].height = 10.0;
        }));
        assert!(broken(&|d| d.drops.push(DropData {
            at: 13.5,
            height: 4.0
        })));
        // Clear of each other: the bump on the drop's lip, or on the road below it.
        assert!(broken(&|d| d.bumps[0].at = 10.0));
        assert!(broken(&|d| d.bumps[0].at = 14.0));
        assert!(!broken(&|d| d.bumps[0].at = 16.8));
        // Not on the start line nor its grid, a ramp or a gap.
        assert!(broken(&|d| d.bumps[0].at = 0.2));
        assert!(broken(&|d| d.bumps[0].at = 86.0));
        assert!(broken(&|d| {
            d.trick_ramps.push(TrickRampData {
                at: 18.0,
                offset: 0.0,
                width: 6.0,
                length: 6.0,
                angle: 12.0,
            })
        }));
        assert!(broken(&|d| d.gaps.push(GapData {
            from: 18.0,
            to: 18.5
        })));
        // The road below a drop goes on in its line: not into the far turn.
        assert!(broken(&|d| {
            for (i, p) in d.points.iter_mut().enumerate() {
                p.pos.1 = if i <= 29 { 6.0 } else { 2.0 };
            }
            d.drops[0].at = 29.5;
            d.bumps.clear();
        }));
    }
}
