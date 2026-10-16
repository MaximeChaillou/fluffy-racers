//! Character and kart statistics (GD § 9, issue #15): pure logic, no ECS systems. Everything here
//! is unit-tested.
//!
//! - **Seven stats**: Speed, Acceleration, Weight, Handling, Traction (off-road and slippery
//!   surfaces), Mini-turbo and Invincibility (the last two are hidden in MK8DX). The Speed and the
//!   Handling have a variant per terrain (GD § 9, issue #20, [`Terrain`]): ground, water, air
//!   (the glider) and anti-gravity; the other stats are the same everywhere. A part that does not
//!   give a terrain's points gives its ground ones there.
//! - **Four parts** make a kart: the character, the kart, the wheels and the wing. Each part
//!   gives 0 to 10 points per stat ([`StatPoints`]); their sum, bounded to 0..=20, is the kart's
//!   **level** in that stat ([`KartStats`]). Menus show `(level + 3) / 4`, 0.75 to 5.75.
//! - **Effects**: each level becomes a multiplier of the [`KartTuning`] values, by linear
//!   interpolation between the bounds of [`StatsTuning`] (à régler), **exactly 1 at the middle
//!   level (10)**. The default loadout (a medium character with the standard kart, wheels and
//!   wing) adds up to 10 everywhere, so it drives exactly like the tuning says: the stats
//!   modulate Max's settings, they do not replace them. With symmetric bounds the interpolation
//!   is GD § 9's `lerp(low, high, level / 20)`; asymmetric bounds still pass through 1 at 10.
//! - **Converted once**, when the kart spawns ([`StatEffects::new`]); every tick the driving
//!   applies those multipliers to the current `KartTuning` ([`StatEffects::tuning`]: a few
//!   multiplications, no allocation), so a value edited in the admin panel still applies at once.
//!   The top speed goes through the kart's `MaxSpeedFactors::stats`, like the croquettes.
//! - **Weight in the interactions between karts** (GD § 15, issue #34): [`strike_push_factor`]
//!   and [`tether_force_scale`], from the kart's Weight level ([`KartStats::weight`]).

use std::borrow::Cow;

use bevy::prelude::*;
use serde::Deserialize;

use super::tuning::KartTuning;

/// Most points a part gives in one stat (GD § 9).
pub const MAX_PART_POINTS: u8 = 10;
/// Highest level of a kart in one stat (GD § 9): the sum of its four parts is bounded to this.
pub const MAX_LEVEL: u8 = 20;
/// The middle level: every multiplier is exactly 1 there.
pub const MIDDLE_LEVEL: u8 = 10;

/// The seven statistics (GD § 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stat {
    Speed,
    Acceleration,
    Weight,
    Handling,
    /// Off-road and slippery surfaces.
    Traction,
    /// Hidden in MK8DX.
    MiniTurbo,
    /// Hidden in MK8DX.
    Invincibility,
}

impl Stat {
    pub const ALL: [Self; 7] = [
        Stat::Speed,
        Stat::Acceleration,
        Stat::Weight,
        Stat::Handling,
        Stat::Traction,
        Stat::MiniTurbo,
        Stat::Invincibility,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Its name in the game (French, with accents: egui has them).
    pub fn label(self) -> &'static str {
        match self {
            Stat::Speed => "Vitesse",
            Stat::Acceleration => "Accélération",
            Stat::Weight => "Poids",
            Stat::Handling => "Maniabilité",
            Stat::Traction => "Adhérence",
            Stat::MiniTurbo => "Mini-turbo",
            Stat::Invincibility => "Invincibilité",
        }
    }
}

/// The points one part (character, kart, wheels or wing) gives in each stat: 0 to 10
/// (GD § 9). As written in the RON files of `assets/data/`.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StatPoints {
    pub speed: u8,
    pub acceleration: u8,
    pub weight: u8,
    pub handling: u8,
    pub traction: u8,
    pub mini_turbo: u8,
    pub invincibility: u8,
    /// Issue #20 (GD § 9): the Speed and the Handling on the other terrains, written
    /// `water_speed: 4` in the files. Left out: the ground's (`speed`, `handling`).
    #[serde(default, deserialize_with = "some_points")]
    pub water_speed: Option<u8>,
    #[serde(default, deserialize_with = "some_points")]
    pub air_speed: Option<u8>,
    #[serde(default, deserialize_with = "some_points")]
    pub anti_gravity_speed: Option<u8>,
    #[serde(default, deserialize_with = "some_points")]
    pub water_handling: Option<u8>,
    #[serde(default, deserialize_with = "some_points")]
    pub air_handling: Option<u8>,
    #[serde(default, deserialize_with = "some_points")]
    pub anti_gravity_handling: Option<u8>,
}

/// `water_speed: 4` in a file (no `Some(...)` around it); left out: None.
fn some_points<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u8>, D::Error> {
    u8::deserialize(d).map(Some)
}

/// The terrains of GD § 9 (issue #20): the Speed and the Handling have a variant for each. The
/// kart is on the ground, under water (a lake, GD § 10), in the air with its glider open, or on
/// an anti-gravity stretch (not built yet: issue of the anti-gravity).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Terrain {
    #[default]
    Ground,
    Water,
    Air,
    AntiGravity,
}

impl Terrain {
    pub const ALL: [Self; 4] = [
        Terrain::Ground,
        Terrain::Water,
        Terrain::Air,
        Terrain::AntiGravity,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Its name in the game (French, with accents: egui has them).
    pub fn label(self) -> &'static str {
        match self {
            Terrain::Ground => "Sol",
            Terrain::Water => "Eau",
            Terrain::Air => "Air",
            Terrain::AntiGravity => "Anti-gravité",
        }
    }
}

impl StatPoints {
    /// The same points in every stat (and on every terrain).
    pub const fn uniform(points: u8) -> Self {
        Self {
            speed: points,
            acceleration: points,
            weight: points,
            handling: points,
            traction: points,
            mini_turbo: points,
            invincibility: points,
            water_speed: None,
            air_speed: None,
            anti_gravity_speed: None,
            water_handling: None,
            air_handling: None,
            anti_gravity_handling: None,
        }
    }

    /// The same points with these of the Speed and the Handling on `terrain` (issue #20).
    pub fn on_terrain(mut self, terrain: Terrain, speed: u8, handling: u8) -> Self {
        let (s, h) = match terrain {
            Terrain::Ground => {
                self.speed = speed;
                self.handling = handling;
                return self;
            }
            Terrain::Water => (&mut self.water_speed, &mut self.water_handling),
            Terrain::Air => (&mut self.air_speed, &mut self.air_handling),
            Terrain::AntiGravity => (
                &mut self.anti_gravity_speed,
                &mut self.anti_gravity_handling,
            ),
        };
        *s = Some(speed);
        *h = Some(handling);
        self
    }

    /// Its points of Speed and Handling on `terrain` (issue #20): the ground's where the part
    /// gives none.
    pub fn terrain(&self, terrain: Terrain) -> (u8, u8) {
        let (speed, handling) = match terrain {
            Terrain::Ground => (None, None),
            Terrain::Water => (self.water_speed, self.water_handling),
            Terrain::Air => (self.air_speed, self.air_handling),
            Terrain::AntiGravity => (self.anti_gravity_speed, self.anti_gravity_handling),
        };
        (
            speed.unwrap_or(self.speed),
            handling.unwrap_or(self.handling),
        )
    }

    /// The first terrain variant above [`MAX_PART_POINTS`], if any (a broken data file): the stat
    /// (Speed or Handling), its terrain and its points.
    pub fn terrain_out_of_range(&self) -> Option<(Stat, Terrain, u8)> {
        Terrain::ALL.into_iter().find_map(|terrain| {
            let (speed, handling) = self.terrain(terrain);
            if speed > MAX_PART_POINTS {
                Some((Stat::Speed, terrain, speed))
            } else if handling > MAX_PART_POINTS {
                Some((Stat::Handling, terrain, handling))
            } else {
                None
            }
        })
    }

    pub fn get(&self, stat: Stat) -> u8 {
        match stat {
            Stat::Speed => self.speed,
            Stat::Acceleration => self.acceleration,
            Stat::Weight => self.weight,
            Stat::Handling => self.handling,
            Stat::Traction => self.traction,
            Stat::MiniTurbo => self.mini_turbo,
            Stat::Invincibility => self.invincibility,
        }
    }

    /// The first stat above [`MAX_PART_POINTS`], if any (a broken data file).
    pub fn out_of_range(&self) -> Option<Stat> {
        Stat::ALL
            .into_iter()
            .find(|stat| self.get(*stat) > MAX_PART_POINTS)
    }
}

/// A kart's level in each stat, 0 to 20 (GD § 9): the sum of its four parts, bounded. On every
/// kart; without parts (the kart scenarios), the middle level everywhere.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct KartStats {
    levels: [u8; 7],
    /// Issue #20: the Speed and the Handling on water, in the air and in anti-gravity
    /// (`Terrain::index() - 1`): `[speed, handling]`. On the ground, `levels`.
    terrains: [[u8; 2]; 3],
}

impl Default for KartStats {
    fn default() -> Self {
        Self::uniform(MIDDLE_LEVEL)
    }
}

impl KartStats {
    /// The sum of the parts' points, each part bounded to 10 per stat and the total to 20; on
    /// each terrain, the parts' points there (their ground ones where they give none).
    pub fn from_parts(parts: &[StatPoints]) -> Self {
        let sum = |points: &dyn Fn(&StatPoints) -> u8| {
            let sum: u32 = parts
                .iter()
                .map(|part| points(part).min(MAX_PART_POINTS) as u32)
                .sum();
            sum.min(MAX_LEVEL as u32) as u8
        };
        let mut levels = [0u8; 7];
        for stat in Stat::ALL {
            levels[stat.index()] = sum(&|part| part.get(stat));
        }
        let mut terrains = [[0u8; 2]; 3];
        for terrain in [Terrain::Water, Terrain::Air, Terrain::AntiGravity] {
            terrains[terrain.index() - 1] = [
                sum(&|part| part.terrain(terrain).0),
                sum(&|part| part.terrain(terrain).1),
            ];
        }
        Self { levels, terrains }
    }

    /// The same level in every stat, on every terrain (tests, scenarios).
    pub fn uniform(level: u8) -> Self {
        let level = level.min(MAX_LEVEL);
        Self {
            levels: [level; 7],
            terrains: [[level; 2]; 3],
        }
    }

    /// The same stats with `stat` at `level` (bounded to 20): on the ground for the Speed and the
    /// Handling (their other terrains do not move).
    pub fn with(mut self, stat: Stat, level: u8) -> Self {
        self.levels[stat.index()] = level.min(MAX_LEVEL);
        self
    }

    /// The same stats with the Speed and the Handling at these levels on `terrain` (issue #20).
    pub fn with_terrain(mut self, terrain: Terrain, speed: u8, handling: u8) -> Self {
        let (speed, handling) = (speed.min(MAX_LEVEL), handling.min(MAX_LEVEL));
        match terrain {
            Terrain::Ground => {
                self.levels[Stat::Speed.index()] = speed;
                self.levels[Stat::Handling.index()] = handling;
            }
            _ => self.terrains[terrain.index() - 1] = [speed, handling],
        }
        self
    }

    /// The level of `stat`, on the ground for the Speed and the Handling.
    pub fn level(&self, stat: Stat) -> u8 {
        self.levels[stat.index()]
    }

    /// The Speed level on `terrain` (issue #20).
    pub fn speed_on(&self, terrain: Terrain) -> u8 {
        match terrain {
            Terrain::Ground => self.level(Stat::Speed),
            _ => self.terrains[terrain.index() - 1][0],
        }
    }

    /// The Handling level on `terrain` (issue #20).
    pub fn handling_on(&self, terrain: Terrain) -> u8 {
        match terrain {
            Terrain::Ground => self.level(Stat::Handling),
            _ => self.terrains[terrain.index() - 1][1],
        }
    }

    /// The Weight level: what the interactions between karts read (GD § 15, issue #34).
    pub fn weight(&self) -> u8 {
        self.level(Stat::Weight)
    }

    /// What the menus show (GD § 9): `(level + 3) / 4`, 0.75 to 5.75.
    pub fn display(&self, stat: Stat) -> f32 {
        display_level(self.level(stat))
    }
}

/// `(level + 3) / 4` (GD § 9): 0.75 at level 0, 3.25 at the middle, 5.75 at 20.
pub fn display_level(level: u8) -> f32 {
    (level.min(MAX_LEVEL) as f32 + 3.0) / 4.0
}

/// Bounds of a stat's effect (à régler): the multiplier at level 0 and at level 20. In between
/// it is linear on each half and exactly 1 at the middle level ([`StatRange::factor`]).
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
pub struct StatRange {
    pub at_0: f32,
    pub at_20: f32,
}

impl StatRange {
    pub const fn new(at_0: f32, at_20: f32) -> Self {
        Self { at_0, at_20 }
    }

    /// The multiplier at `level`: `at_0` at 0, exactly 1 at 10, `at_20` at 20 (linear on each
    /// half; one straight line when the bounds are symmetric around 1, GD § 9's
    /// `lerp(at_0, at_20, level / 20)`).
    pub fn factor(&self, level: u8) -> f32 {
        self.factor_at(level.min(MAX_LEVEL) as f32 / MAX_LEVEL as f32)
    }

    /// Same with `t` from 0 to 1 (0.5 = the middle, exactly 1).
    pub fn factor_at(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t >= 0.5 {
            1.0 + (self.at_20 - 1.0) * (t - 0.5) * 2.0
        } else {
            1.0 + (self.at_0 - 1.0) * (0.5 - t) * 2.0
        }
    }

    /// A plain linear interpolation from `at_0` (level 0) to `at_20` (level 20): for the values
    /// that are not multipliers (the invincibility, in seconds).
    pub fn lerp(&self, level: u8) -> f32 {
        let t = level.min(MAX_LEVEL) as f32 / MAX_LEVEL as f32;
        self.at_0 + (self.at_20 - self.at_0) * t
    }
}

/// How the stats change the kart (GD § 9, issue #15): the bounds of each effect, at level 0 and
/// at level 20. Our own starting points, à régler; GD § 9's example is the top speed,
/// `lerp(0.92, 1.08, level / 20)`. Read when a kart spawns: an edit applies at the next restart.
///
/// Reflected (`Reflect`) so that the admin panel (issue #36) edits every field.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct StatsTuning {
    // --- Speed ----------------------------------------------------------------------------------
    /// Top speed multiplier (GD § 9: 0.92 to 1.08), boosted or not: the kart's
    /// `MaxSpeedFactors::stats`.
    pub speed_top_speed: StatRange,

    // --- Acceleration ---------------------------------------------------------------------------
    /// Engine acceleration multiplier: `time_to_max_speed` is divided by it (0.8 to 1.25: from
    /// 3.1 s to 2 s to the top speed, 2.5 s in the middle).
    pub acceleration_engine: StatRange,

    // --- Weight ---------------------------------------------------------------------------------
    /// Mass multiplier (and rotational inertia, in proportion): who pushes whom when karts
    /// collide (GD § 9). 0.7 to 1.3: 105 to 195 kg, 150 kg in the middle.
    pub weight_mass: StatRange,
    /// GD § 15: push of a paw strike or a throw, `× factor_at((striker − target + 20) / 40)`:
    /// 0.7 when the lightest hits the heaviest, 1.3 the other way round, 1 between equals.
    /// For issue #34 ([`strike_push_factor`]).
    pub weight_strike_push: StatRange,
    /// GD § 15: each kart of a tether receives its force divided by `factor(its Weight)`, 0.8 to
    /// 1.2. For issue #34 ([`tether_force_scale`]).
    pub weight_tether_divisor: StatRange,

    // --- Handling -------------------------------------------------------------------------------
    /// Multiplier of the normal steering's yaw rates (`steer_rate`, `steer_rate_at_max_speed`):
    /// tighter turns.
    pub handling_steer_rate: StatRange,
    /// Multiplier of the steering's responsiveness (`steer_response`, `hop_steer_response`): a
    /// livelier stick.
    pub handling_steer_response: StatRange,
    /// Multiplier of the drift radii (`drift_radius_tight`, `drift_radius_wide`, issue #30):
    /// below 1 at 20, a nimbler kart drifts tighter.
    pub handling_drift_radius: StatRange,

    // --- Traction -------------------------------------------------------------------------------
    /// Multiplier of the off-road top speed (`offroad_speed_factor`, GD § 2: 55 %): 0.85 to 1.15,
    /// 47 % to 63 %. No slippery surface yet (ice, water: phase 3).
    pub traction_offroad: StatRange,

    // --- Mini-turbo -----------------------------------------------------------------------------
    /// Multiplier of the drift's charge thresholds (GD § 3.4: 270 / 570 / 870 in the middle):
    /// below 1 at 20, the tiers come sooner.
    pub mini_turbo_thresholds: StatRange,
    /// Multiplier of the mini-turbos' boost durations (GD § 3.4).
    pub mini_turbo_durations: StatRange,
    /// Multiplier of the mini-turbos' and the trick's boost strengths.
    pub mini_turbo_strengths: StatRange,
    /// Multiplier of the trick boost's duration (GD § 5: 0.5 to 0.8 s with the Mini-turbo stat;
    /// 0.77 to 1.23 of the 0.65 s in the middle).
    pub mini_turbo_trick_duration: StatRange,

    // --- Invincibility --------------------------------------------------------------------------
    /// Protection after an item hit, in seconds (GD § 8: 1 to 2 s): a plain interpolation, not a
    /// multiplier (1.5 s in the middle). Starts on an item hit (`LossCause::ItemHit`, the entry
    /// point of issue #16): a second item hit meanwhile costs no croquette.
    pub invincibility_seconds: StatRange,

    // --- Terrains (GD § 9, issue #20) -----------------------------------------------------------
    /// The Speed and the Handling under water (GD § 10: the stats « eau »), from the kart's
    /// levels there. The same bounds as on the ground by default. The water's own slowness and
    /// floatiness are the environment's (`EnvironmentTuning`), whatever the stats.
    pub water: TerrainRanges,
    /// The same in the air, glider open (GD § 10: the stats « air »): the glide's speed and
    /// turns.
    pub air: TerrainRanges,
    /// The same in anti-gravity (not built yet: ready for its issue).
    pub anti_gravity: TerrainRanges,
}

/// The bounds of the Speed and the Handling's effects on one terrain other than the ground
/// (issue #20): the same effects as [`StatsTuning`]'s `speed_top_speed` and `handling_*`, from
/// the kart's levels on that terrain. Exactly 1 at the middle level, like every range.
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
pub struct TerrainRanges {
    /// Top speed multiplier (the glide's speed in the air).
    pub speed_top_speed: StatRange,
    /// Yaw rates (the glide's turn rate in the air).
    pub handling_steer_rate: StatRange,
    /// Steering responsiveness.
    pub handling_steer_response: StatRange,
    /// Drift radii.
    pub handling_drift_radius: StatRange,
}

impl TerrainRanges {
    /// The ground's bounds of `tuning`.
    pub fn ground(tuning: &StatsTuning) -> Self {
        Self {
            speed_top_speed: tuning.speed_top_speed,
            handling_steer_rate: tuning.handling_steer_rate,
            handling_steer_response: tuning.handling_steer_response,
            handling_drift_radius: tuning.handling_drift_radius,
        }
    }
}

impl StatsTuning {
    /// The bounds of `terrain`'s Speed and Handling.
    pub fn terrain(&self, terrain: Terrain) -> TerrainRanges {
        match terrain {
            Terrain::Ground => TerrainRanges::ground(self),
            Terrain::Water => self.water,
            Terrain::Air => self.air,
            Terrain::AntiGravity => self.anti_gravity,
        }
    }
}

impl Default for StatsTuning {
    fn default() -> Self {
        let speed_top_speed = StatRange::new(0.92, 1.08);
        let handling_steer_rate = StatRange::new(0.9, 1.1);
        let handling_steer_response = StatRange::new(0.8, 1.2);
        let handling_drift_radius = StatRange::new(1.1, 0.9);
        // Issue #20: every terrain starts with the ground's bounds.
        let terrain = TerrainRanges {
            speed_top_speed,
            handling_steer_rate,
            handling_steer_response,
            handling_drift_radius,
        };
        Self {
            speed_top_speed,
            acceleration_engine: StatRange::new(0.8, 1.25),
            weight_mass: StatRange::new(0.7, 1.3),
            weight_strike_push: StatRange::new(0.7, 1.3),
            weight_tether_divisor: StatRange::new(0.8, 1.2),
            handling_steer_rate,
            handling_steer_response,
            handling_drift_radius,
            traction_offroad: StatRange::new(0.85, 1.15),
            mini_turbo_thresholds: StatRange::new(1.1, 0.9),
            mini_turbo_durations: StatRange::new(0.85, 1.15),
            mini_turbo_strengths: StatRange::new(0.9, 1.1),
            mini_turbo_trick_duration: StatRange::new(0.77, 1.23),
            invincibility_seconds: StatRange::new(1.0, 2.0),
            water: terrain,
            air: terrain,
            anti_gravity: terrain,
        }
    }
}

/// What a kart's stats do, converted once when it spawns ([`StatEffects::new`]): multipliers of
/// the `KartTuning` values (1 = unchanged), the protection after an item hit and the Weight level.
/// On every kart; without stats, neutral (every multiplier 1).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct StatEffects {
    /// Top speed (`MaxSpeedFactors::stats`).
    pub top_speed: f32,
    /// Engine acceleration (`time_to_max_speed` divided by it).
    pub acceleration: f32,
    /// Mass and rotational inertia.
    pub mass: f32,
    /// Normal steering's yaw rates.
    pub steer_rate: f32,
    /// Steering responsiveness.
    pub steer_response: f32,
    /// Drift radii.
    pub drift_radius: f32,
    /// Off-road top speed factor.
    pub offroad: f32,
    /// Mini-turbo charge thresholds.
    pub mini_turbo_thresholds: f32,
    /// Mini-turbo boost durations.
    pub mini_turbo_durations: f32,
    /// Mini-turbo and trick boost strengths.
    pub boost_strengths: f32,
    /// Trick boost duration.
    pub trick_duration: f32,
    /// Protection after an item hit (s, GD § 8).
    pub invincibility: f32,
    /// The Weight level (0 to 20), for the interactions between karts (GD § 15).
    pub weight: u8,
    /// Issue #20: the effects of the Speed and the Handling on each terrain
    /// (`Terrain::index()`; the ground's are the fields above). [`StatEffects::on`] puts them in
    /// place.
    pub terrains: [TerrainEffects; 4],
}

/// What the Speed and the Handling do on one terrain (issue #20): the fields of [`StatEffects`]
/// they move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainEffects {
    pub top_speed: f32,
    pub steer_rate: f32,
    pub steer_response: f32,
    pub drift_radius: f32,
}

impl TerrainEffects {
    fn new(stats: &KartStats, tuning: &StatsTuning, terrain: Terrain) -> Self {
        let ranges = tuning.terrain(terrain);
        let (speed, handling) = (stats.speed_on(terrain), stats.handling_on(terrain));
        Self {
            top_speed: ranges.speed_top_speed.factor(speed),
            steer_rate: ranges.handling_steer_rate.factor(handling),
            steer_response: ranges.handling_steer_response.factor(handling),
            drift_radius: ranges.handling_drift_radius.factor(handling),
        }
    }
}

impl Default for StatEffects {
    fn default() -> Self {
        Self::new(&KartStats::default(), &StatsTuning::default())
    }
}

impl StatEffects {
    /// Converts `stats` with the bounds of `tuning`.
    pub fn new(stats: &KartStats, tuning: &StatsTuning) -> Self {
        let level = |stat| stats.level(stat);
        let terrains = Terrain::ALL.map(|terrain| TerrainEffects::new(stats, tuning, terrain));
        let ground = terrains[Terrain::Ground.index()];
        Self {
            top_speed: ground.top_speed,
            acceleration: tuning.acceleration_engine.factor(level(Stat::Acceleration)),
            mass: tuning.weight_mass.factor(level(Stat::Weight)),
            steer_rate: ground.steer_rate,
            steer_response: ground.steer_response,
            drift_radius: ground.drift_radius,
            offroad: tuning.traction_offroad.factor(level(Stat::Traction)),
            mini_turbo_thresholds: tuning.mini_turbo_thresholds.factor(level(Stat::MiniTurbo)),
            mini_turbo_durations: tuning.mini_turbo_durations.factor(level(Stat::MiniTurbo)),
            boost_strengths: tuning.mini_turbo_strengths.factor(level(Stat::MiniTurbo)),
            trick_duration: tuning
                .mini_turbo_trick_duration
                .factor(level(Stat::MiniTurbo)),
            invincibility: tuning
                .invincibility_seconds
                .lerp(level(Stat::Invincibility)),
            weight: stats.weight(),
            terrains,
        }
    }

    /// The effects on `terrain` (issue #20): the Speed and the Handling of that terrain in place
    /// of the ground's. On the ground, exactly `self`.
    pub fn on(&self, terrain: Terrain) -> Self {
        if terrain == Terrain::Ground {
            return *self;
        }
        let t = self.terrains[terrain.index()];
        Self {
            top_speed: t.top_speed,
            steer_rate: t.steer_rate,
            steer_response: t.steer_response,
            drift_radius: t.drift_radius,
            ..*self
        }
    }

    /// Every multiplier is 1: the kart drives exactly as `KartTuning` says.
    pub fn is_neutral(&self) -> bool {
        [
            self.acceleration,
            self.mass,
            self.steer_rate,
            self.steer_response,
            self.drift_radius,
            self.offroad,
            self.mini_turbo_thresholds,
            self.mini_turbo_durations,
            self.boost_strengths,
            self.trick_duration,
        ]
        .iter()
        .all(|factor| *factor == 1.0)
    }

    /// `base` with this kart's multipliers applied (the top speed apart: it is a
    /// `MaxSpeedFactors` factor). A copy on the stack, no allocation.
    pub fn apply(&self, base: &KartTuning) -> KartTuning {
        let mut t = base.clone();
        t.time_to_max_speed = base.time_to_max_speed / self.acceleration.max(0.01);
        t.mass = base.mass * self.mass;
        t.inertia = base.inertia * self.mass;
        t.steer_rate = base.steer_rate * self.steer_rate;
        t.steer_rate_at_max_speed = base.steer_rate_at_max_speed * self.steer_rate;
        t.steer_response = base.steer_response * self.steer_response;
        t.hop_steer_response = base.hop_steer_response * self.steer_response;
        t.drift_radius_tight = base.drift_radius_tight * self.drift_radius;
        t.drift_radius_wide = base.drift_radius_wide * self.drift_radius;
        t.offroad_speed_factor = base.offroad_speed_factor * self.offroad;
        t.mini_turbo_thresholds = base
            .mini_turbo_thresholds
            .map(|threshold| threshold * self.mini_turbo_thresholds);
        t.mini_turbo_durations = base
            .mini_turbo_durations
            .map(|duration| duration * self.mini_turbo_durations);
        t.mini_turbo_strengths = base
            .mini_turbo_strengths
            .map(|strength| strength * self.boost_strengths);
        t.trick_boost_duration = base.trick_boost_duration * self.trick_duration;
        t.trick_boost_strength = base.trick_boost_strength * self.boost_strengths;
        t
    }

    /// The tuning this kart drives with: `base` itself when neutral (exactly `KartTuning`),
    /// otherwise [`StatEffects::apply`].
    pub fn tuning<'a>(&self, base: &'a KartTuning) -> Cow<'a, KartTuning> {
        if self.is_neutral() {
            Cow::Borrowed(base)
        } else {
            Cow::Owned(self.apply(base))
        }
    }
}

/// Protection after an item hit (GD § 8: 1 to 2 s with the Invincibility stat). Started for
/// the kart's [`StatEffects::invincibility`] when the spin out or tumble of an item hit ends
/// (`KartState::Hit`, issue #16); meanwhile no item hits it (`items::immune_to_items`), and the
/// kart blinks. Counted down with the boosts (`KartSystems::Timers`). Issue #34 makes the paw
/// strikes and tethers skip it too (GD § 15).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Invincibility {
    /// Seconds left.
    pub remaining: f32,
}

impl Invincibility {
    pub fn is_active(&self) -> bool {
        self.remaining > 0.0
    }

    /// Starts (or extends) the protection for `seconds`.
    pub fn start(&mut self, seconds: f32) {
        self.remaining = self.remaining.max(seconds);
    }

    pub fn tick(&mut self, dt: f32) {
        self.remaining = (self.remaining - dt).max(0.0);
    }
}

/// GD § 15 (issue #34): multiplier of the push of a paw strike or a throw, from the Weight levels
/// of the striker and of its target: `× factor_at((striker − target + 20) / 40)`, 1 between
/// equals, 1.3 when the heaviest (20) hits the lightest (0), 0.7 the other way round.
pub fn strike_push_factor(tuning: &StatsTuning, striker_weight: u8, target_weight: u8) -> f32 {
    let difference = striker_weight.min(MAX_LEVEL) as f32 - target_weight.min(MAX_LEVEL) as f32;
    let t = (difference + MAX_LEVEL as f32) / (2.0 * MAX_LEVEL as f32);
    tuning.weight_strike_push.factor_at(t)
}

/// GD § 15 (issue #34): share of a tether's force a kart of this Weight level receives, `1 /
/// factor(weight)`: 1.25 for the lightest (it gets towed), 1 in the middle, 0.83 for the
/// heaviest (it hardly slows down).
pub fn tether_force_scale(tuning: &StatsTuning, weight: u8) -> f32 {
    1.0 / tuning.weight_tether_divisor.factor(weight).max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(speed: u8, acceleration: u8) -> StatPoints {
        StatPoints {
            speed,
            acceleration,
            ..StatPoints::uniform(2)
        }
    }

    /// Issue #15 acceptance: the levels are the sum of the four parts, bounded to 0..=20, each
    /// part counting 10 points at most.
    #[test]
    fn levels_add_up_the_parts_and_stay_within_bounds() {
        let stats =
            KartStats::from_parts(&[points(4, 6), points(3, 1), points(2, 0), points(1, 0)]);
        assert_eq!(stats.level(Stat::Speed), 10);
        assert_eq!(stats.level(Stat::Acceleration), 7);
        assert_eq!(stats.level(Stat::Weight), 8);
        // Four parts at 10: 40, bounded to 20.
        let maxed = KartStats::from_parts(&[StatPoints::uniform(10); 4]);
        assert!(Stat::ALL.iter().all(|s| maxed.level(*s) == MAX_LEVEL));
        // A broken part (12 points) counts 10.
        let broken = KartStats::from_parts(&[StatPoints::uniform(12)]);
        assert!(Stat::ALL.iter().all(|s| broken.level(*s) == 10));
        assert_eq!(
            StatPoints::uniform(11).out_of_range(),
            Some(Stat::Speed),
            "11 points is too many"
        );
        assert_eq!(StatPoints::uniform(10).out_of_range(), None);
        // No part: 0.
        let none = KartStats::from_parts(&[]);
        assert!(Stat::ALL.iter().all(|s| none.level(*s) == 0));
        assert_eq!(KartStats::uniform(30), KartStats::uniform(20));
        assert_eq!(KartStats::default(), KartStats::uniform(MIDDLE_LEVEL));
        assert_eq!(KartStats::default().with(Stat::Weight, 25).weight(), 20);
    }

    /// GD § 9: menus show `(level + 3) / 4`, from 0.75 to 5.75.
    #[test]
    fn the_menu_shows_level_plus_three_over_four() {
        assert_eq!(display_level(0), 0.75);
        assert_eq!(display_level(10), 3.25);
        assert_eq!(display_level(20), 5.75);
        assert_eq!(display_level(25), 5.75);
        assert_eq!(KartStats::uniform(1).display(Stat::Handling), 1.0);
        assert_eq!(KartStats::uniform(13).display(Stat::Speed), 4.0);
    }

    /// The middle level is exactly 1, and the bounds are reached at 0 and 20: symmetric bounds
    /// give GD § 9's straight `lerp(0.92, 1.08, level / 20)`.
    #[test]
    fn a_range_is_one_in_the_middle_and_linear_between_its_bounds() {
        let range = StatRange::new(0.92, 1.08);
        assert_eq!(range.factor(MIDDLE_LEVEL), 1.0);
        assert!((range.factor(0) - 0.92).abs() < 1e-6);
        assert!((range.factor(20) - 1.08).abs() < 1e-6);
        for level in 0..=20u8 {
            let lerp = 0.92 + 0.16 * level as f32 / 20.0;
            assert!((range.factor(level) - lerp).abs() < 1e-6, "{level}");
            assert!((range.lerp(level) - lerp).abs() < 1e-6, "{level}");
        }
        // Asymmetric bounds still go through 1 at the middle.
        let odd = StatRange::new(0.5, 1.1);
        assert_eq!(odd.factor(10), 1.0);
        assert!((odd.factor(5) - 0.75).abs() < 1e-6);
        assert!((odd.factor(15) - 1.05).abs() < 1e-6);
        // Decreasing ranges (drift radius, thresholds) too.
        let down = StatRange::new(1.1, 0.9);
        assert!(down.factor(20) < down.factor(10) && down.factor(10) < down.factor(0));
        // A plain interpolation (seconds): GD § 8, 1 to 2 s.
        let seconds = StatsTuning::default().invincibility_seconds;
        assert_eq!(seconds.lerp(0), 1.0);
        assert_eq!(seconds.lerp(10), 1.5);
        assert_eq!(seconds.lerp(20), 2.0);
    }

    /// Issue #15: the middle level drives exactly like `KartTuning`, whatever its values (Max
    /// tunes them in the admin panel): the stats modulate around them.
    #[test]
    fn the_middle_level_gives_exactly_the_kart_tuning() {
        let effects = StatEffects::new(&KartStats::default(), &StatsTuning::default());
        assert!(effects.is_neutral());
        assert_eq!(effects.top_speed, 1.0);
        assert_eq!(effects, StatEffects::default());
        let tuned = KartTuning {
            max_speed: 31.0,
            time_to_max_speed: 2.2,
            drift_radius_tight: 19.0,
            ..default()
        };
        for base in [KartTuning::default(), tuned] {
            assert_eq!(effects.apply(&base), base);
            assert!(matches!(effects.tuning(&base), Cow::Borrowed(_)));
        }
        // GD § 3.4: the thresholds of the middle are 270 / 570 / 870.
        assert_eq!(
            effects.apply(&KartTuning::default()).mini_turbo_thresholds,
            [270.0, 570.0, 870.0]
        );
    }

    /// Each stat changes its own values, in the right direction.
    #[test]
    fn each_stat_changes_its_own_values() {
        let tuning = StatsTuning::default();
        let base = KartTuning::default();
        let middle = KartStats::default();
        let with = |stat, level| StatEffects::new(&middle.with(stat, level), &tuning);

        let fast = with(Stat::Speed, 20);
        assert!((fast.top_speed - 1.08).abs() < 1e-6);
        // The top speed is a `MaxSpeedFactors` factor: the tuning does not change.
        assert!(fast.is_neutral());
        assert_eq!(fast.apply(&base), base);

        let quick = with(Stat::Acceleration, 20).apply(&base);
        assert!((quick.time_to_max_speed - 2.0).abs() < 1e-5);
        let slow = with(Stat::Acceleration, 0).apply(&base);
        assert!((slow.time_to_max_speed - 3.125).abs() < 1e-5);

        let heavy = with(Stat::Weight, 20).apply(&base);
        assert!((heavy.mass - 195.0).abs() < 1e-3);
        assert!((heavy.inertia - base.inertia * 1.3).length() < 1e-3);
        assert!((with(Stat::Weight, 0).apply(&base).mass - 105.0).abs() < 1e-3);

        let nimble = with(Stat::Handling, 20).apply(&base);
        assert!(nimble.drift_radius_tight < base.drift_radius_tight);
        assert!(nimble.drift_radius_wide < base.drift_radius_wide);
        assert!(nimble.steer_rate > base.steer_rate);
        assert!(nimble.steer_rate_at_max_speed > base.steer_rate_at_max_speed);
        assert!(nimble.steer_response > base.steer_response);
        assert!(nimble.hop_steer_response > base.hop_steer_response);

        let grippy = with(Stat::Traction, 20).apply(&base);
        assert!((grippy.offroad_speed_factor - 0.55 * 1.15).abs() < 1e-6);

        let turbo = with(Stat::MiniTurbo, 20).apply(&base);
        for i in 0..3 {
            assert!(turbo.mini_turbo_thresholds[i] < base.mini_turbo_thresholds[i]);
            assert!(turbo.mini_turbo_durations[i] > base.mini_turbo_durations[i]);
            assert!(turbo.mini_turbo_strengths[i] > base.mini_turbo_strengths[i]);
        }
        // GD § 5: the trick boost lasts 0.5 to 0.8 s with the Mini-turbo stat.
        assert!((turbo.trick_boost_duration - 0.8).abs() < 0.01);
        let no_turbo = with(Stat::MiniTurbo, 0).apply(&base);
        assert!((no_turbo.trick_boost_duration - 0.5).abs() < 0.01);
        assert!(turbo.trick_boost_strength > base.trick_boost_strength);

        assert_eq!(with(Stat::Invincibility, 0).invincibility, 1.0);
        assert_eq!(with(Stat::Invincibility, 20).invincibility, 2.0);
        assert_eq!(with(Stat::Weight, 17).weight, 17);
        // Nothing else moves: the rest of the tuning is the base's.
        let mut only_engine = base.clone();
        only_engine.time_to_max_speed = quick.time_to_max_speed;
        assert_eq!(quick, only_engine);
    }

    /// GD § 15 (issue #34): a heavy kart strikes and throws harder, between equals ×1; a tether
    /// tows a light kart and hardly slows a heavy one down.
    #[test]
    fn weight_scales_the_interactions_between_karts() {
        let t = StatsTuning::default();
        assert_eq!(strike_push_factor(&t, 10, 10), 1.0);
        assert_eq!(strike_push_factor(&t, 3, 3), 1.0);
        assert!((strike_push_factor(&t, 20, 0) - 1.3).abs() < 1e-6);
        assert!((strike_push_factor(&t, 0, 20) - 0.7).abs() < 1e-6);
        // lerp(0.7, 1.3, (16 − 4 + 20) / 40) = 1.18.
        assert!((strike_push_factor(&t, 16, 4) - 1.18).abs() < 1e-6);
        assert!(strike_push_factor(&t, 12, 8) > 1.0 && strike_push_factor(&t, 8, 12) < 1.0);
        assert_eq!(tether_force_scale(&t, 10), 1.0);
        assert!((tether_force_scale(&t, 0) - 1.0 / 0.8).abs() < 1e-5);
        assert!((tether_force_scale(&t, 20) - 1.0 / 1.2).abs() < 1e-5);
        assert!(tether_force_scale(&t, 3) > tether_force_scale(&t, 16));
    }

    #[test]
    fn the_invincibility_counts_down_and_never_shrinks_on_a_restart() {
        let mut invincible = Invincibility::default();
        assert!(!invincible.is_active());
        invincible.start(1.5);
        invincible.tick(1.0);
        assert!(invincible.is_active());
        invincible.start(0.2);
        assert!((invincible.remaining - 0.5).abs() < 1e-6);
        invincible.tick(0.6);
        assert!(!invincible.is_active());
        assert_eq!(invincible.remaining, 0.0);
    }

    /// Issue #20 (GD § 9): the Speed and the Handling have a variant per terrain. A part that
    /// gives none there counts its ground points; the parts' points add up on each terrain; the
    /// middle level is exactly 1 on every terrain, so the default loadout drives the same
    /// everywhere; on the ground, `on` changes nothing at all.
    #[test]
    fn the_speed_and_the_handling_have_a_variant_per_terrain() {
        let tuning = StatsTuning::default();
        // The ranges of every terrain start as the ground's.
        for terrain in Terrain::ALL {
            assert_eq!(tuning.terrain(terrain), TerrainRanges::ground(&tuning));
        }
        let middle = StatEffects::new(&KartStats::default(), &tuning);
        for terrain in Terrain::ALL {
            assert_eq!(middle.on(terrain), middle, "{terrain:?}");
            assert_eq!(KartStats::default().speed_on(terrain), MIDDLE_LEVEL);
            assert_eq!(KartStats::default().handling_on(terrain), MIDDLE_LEVEL);
        }
        // A part without terrain points: its ground points everywhere.
        let plain = StatPoints {
            speed: 3,
            handling: 7,
            ..StatPoints::uniform(2)
        };
        for terrain in Terrain::ALL {
            assert_eq!(plain.terrain(terrain), (3, 7));
        }
        // With them: its own, and they add up.
        let swimmer = plain.on_terrain(Terrain::Water, 6, 5);
        assert_eq!(swimmer.terrain(Terrain::Water), (6, 5));
        assert_eq!(swimmer.terrain(Terrain::Air), (3, 7));
        let stats = KartStats::from_parts(&[swimmer, plain.on_terrain(Terrain::Air, 1, 9), plain]);
        assert_eq!(stats.level(Stat::Speed), 9);
        assert_eq!(stats.speed_on(Terrain::Ground), 9);
        assert_eq!(stats.handling_on(Terrain::Ground), 20);
        assert_eq!(stats.speed_on(Terrain::Water), 12);
        assert_eq!(stats.handling_on(Terrain::Water), 19);
        assert_eq!(stats.speed_on(Terrain::Air), 7);
        assert_eq!(stats.handling_on(Terrain::Air), 20, "bounded to 20");
        assert_eq!(stats.speed_on(Terrain::AntiGravity), 9);
        // A broken terrain value is seen.
        assert_eq!(plain.terrain_out_of_range(), None);
        assert_eq!(
            plain
                .on_terrain(Terrain::AntiGravity, 4, 11)
                .terrain_out_of_range(),
            Some((Stat::Handling, Terrain::AntiGravity, 11))
        );
        // The effects: the water's Speed at 20, its Handling at 0; the ground does not move.
        let stats = KartStats::default().with_terrain(Terrain::Water, 20, 0);
        let effects = StatEffects::new(&stats, &tuning);
        assert_eq!(effects.top_speed, 1.0);
        assert!(effects.is_neutral());
        let water = effects.on(Terrain::Water);
        assert!((water.top_speed - 1.08).abs() < 1e-6);
        assert!((water.steer_rate - 0.9).abs() < 1e-6);
        assert!((water.steer_response - 0.8).abs() < 1e-6);
        assert!((water.drift_radius - 1.1).abs() < 1e-6);
        // Only those four: the rest is the kart's.
        assert_eq!(water.acceleration, effects.acceleration);
        assert_eq!(water.mini_turbo_thresholds, effects.mini_turbo_thresholds);
        assert_eq!(effects.on(Terrain::Air), effects);
        assert_eq!(effects.on(Terrain::Ground), effects);
        // Each terrain's own bounds.
        let tuned = StatsTuning {
            air: TerrainRanges {
                speed_top_speed: StatRange::new(0.5, 1.5),
                ..TerrainRanges::ground(&tuning)
            },
            ..tuning.clone()
        };
        let fast_air = StatEffects::new(
            &KartStats::default().with_terrain(Terrain::Air, 20, 10),
            &tuned,
        );
        assert!((fast_air.on(Terrain::Air).top_speed - 1.5).abs() < 1e-6);
        let labels: Vec<&str> = Terrain::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(labels, ["Sol", "Eau", "Air", "Anti-gravité"]);
    }

    #[test]
    fn every_stat_has_a_french_label() {
        let labels: Vec<&str> = Stat::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels,
            [
                "Vitesse",
                "Accélération",
                "Poids",
                "Maniabilité",
                "Adhérence",
                "Mini-turbo",
                "Invincibilité"
            ]
        );
        for (i, stat) in Stat::ALL.iter().enumerate() {
            assert_eq!(stat.index(), i);
        }
    }
}
