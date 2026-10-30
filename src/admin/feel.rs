//! The « Ressenti » knobs of the admin panel (issue #155): a handful of sliders, one per feeling
//! (the drift, the speed, the grip...), each one driving several tuning values at once. Max sets
//! what he likes without knowing the dozens of values underneath; they stay in « Avancé ».
//!
//! Pure logic, no egui and no `World` (the registry of the tunings, `super::tuning`, applies it):
//!
//! - a knob ([`Knob`]) goes from 0 to 100, **50 in the middle gives exactly the defaults** (the
//!   value is the default itself, not a computation that would land next to it);
//! - each value it drives is a [`Link`]: the default times a factor, `low` at 0 and `high` at
//!   100, **linear on each half** (1 in the middle): `factor = 1 + (high − 1) × t` above the
//!   middle, `1 + (low − 1) × (−t)` below, `t = (position − 50) / 50`; then clamped to the link's
//!   bounds (a chance stays at most 1) and rounded for whole numbers. With `low` and `high` on
//!   either side of 1, the value only moves one way along the slider;
//! - a link names a value by its `Resource.path`; `[*]` stands for any index (the three bot
//!   levels), and a path names everything under it too (`mini_turbo_durations` its three tiers).
//!
//! Every value is driven by one knob at most (a test checks it).

/// The middle of a knob: the game's defaults.
pub const MIDDLE: u8 = 50;
/// The end of a knob (the other end is 0).
pub const KNOB_MAX: u8 = 100;
/// The first part of a knob's entry in the text of « Copier mes réglages »: `"Ressenti.derapage"`.
pub const FEEL_PREFIX: &str = "Ressenti.";

/// The knobs whose middle moved, with the position of before that is their middle now: since
/// #170, « Dérapage » at 50 gives what 80 gave (the drift Max chose on 2026-10-21, the new
/// defaults). A text kept from before (`tuning::EXPORT_HEADER_BEFORE_170`) puts them back in the
/// middle rather than add its position to the new defaults (an 80 would count twice).
pub const RECENTRED: &[(Knob, u8)] = &[(Knob::Drift, 80)];

/// One feeling of the « Ressenti » section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Knob {
    Drift,
    Speed,
    Grip,
    Boosts,
    Bumps,
    Bots,
    Items,
    Camera,
    Effects,
    Sounds,
    Music,
}

impl Knob {
    pub const ALL: [Knob; 11] = [
        Knob::Drift,
        Knob::Speed,
        Knob::Grip,
        Knob::Boosts,
        Knob::Bumps,
        Knob::Bots,
        Knob::Items,
        Knob::Camera,
        Knob::Effects,
        Knob::Sounds,
        Knob::Music,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }

    /// Its name in the text of « Copier mes réglages » (`"Ressenti.<key>"`): plain letters, never
    /// renamed (a text kept in the browser reads it back).
    pub fn key(self) -> &'static str {
        match self {
            Knob::Drift => "derapage",
            Knob::Speed => "vitesse",
            Knob::Grip => "tenue",
            Knob::Boosts => "boosts",
            Knob::Bumps => "chocs",
            Knob::Bots => "bots",
            Knob::Items => "objets",
            Knob::Camera => "camera",
            Knob::Effects => "effets",
            Knob::Sounds => "sons",
            Knob::Music => "musique",
        }
    }

    pub fn from_key(key: &str) -> Option<Knob> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// Its name in the panel.
    pub fn label(self) -> &'static str {
        match self {
            Knob::Drift => "Dérapage",
            Knob::Speed => "Vitesse",
            Knob::Grip => "Tenue de route",
            Knob::Boosts => "Boosts",
            Knob::Bumps => "Chocs",
            Knob::Bots => "Bots",
            Knob::Items => "Objets",
            Knob::Camera => "Caméra",
            Knob::Effects => "Effets",
            Knob::Sounds => "Sons",
            Knob::Music => "Musique",
        }
    }

    /// What the low end (0) does.
    pub fn low(self) -> &'static str {
        match self {
            Knob::Drift => "← plus serré et vif",
            Knob::Speed => "← plus lent",
            Knob::Grip => "← plus glissant",
            Knob::Boosts => "← plus doux",
            Knob::Bumps => "← plus doux",
            Knob::Bots => "← plus tranquilles",
            Knob::Items => "← plus rares",
            Knob::Camera => "← plus près, plus vive",
            Knob::Effects => "← plus sobres",
            Knob::Sounds => "← plus discrets",
            Knob::Music => "← plus discrète",
        }
    }

    /// What the high end (100) does.
    pub fn high(self) -> &'static str {
        match self {
            Knob::Drift => "plus large et glissant →",
            Knob::Speed => "plus rapide →",
            Knob::Grip => "plus accrocheur →",
            Knob::Boosts => "plus forts, plus longs →",
            Knob::Bumps => "ça tape plus fort →",
            Knob::Bots => "plus forts, plus teigneux →",
            Knob::Items => "plus d'objets, des forts →",
            Knob::Camera => "plus loin, plus souple →",
            Knob::Effects => "plus de particules →",
            Knob::Sounds => "plus forts →",
            Knob::Music => "plus forte →",
        }
    }

    /// What it drives, in a few words (the panel's tooltip, before the list of the values).
    pub fn summary(self) -> &'static str {
        match self {
            Knob::Drift => {
                "Le rayon et l'angle du drift, l'arrière léger, la mise en courbe et les paliers \
                 de mini-turbo."
            }
            Knob::Speed => {
                "La vitesse max et l'accélération ensemble ; les objets lancés suivent (pelote, \
                 abeille, fusée, boomerang)."
            }
            Knob::Grip => {
                "L'adhérence, le hors-piste et la suspension (plus ou moins rebondissante)."
            }
            Knob::Boosts => {
                "La force et la durée des mini-turbos, des plots, des piments, du départ fusée, \
                 des figures et de l'aspiration."
            }
            Knob::Bumps => {
                "Les chocs entre karts (le pivotement gardé), le coup de patte (poussée, \
                 vitesse perdue, embardée, jet, attrape lâchée) et la vitesse gardée quand un \
                 objet touche."
            }
            Knob::Bots => {
                "Leur niveau (précision, vitesse, drifts), leur rattrapage et leur agressivité \
                 (objets, coups de patte, attrapes). Au prochain « Relancer » pour certains."
            }
            Knob::Items => {
                "Le retour des boîtes, la durée de la roulette, et les objets forts plus ou \
                 moins près du leader."
            }
            Knob::Camera => "La distance, la hauteur et la réactivité de la caméra.",
            Knob::Effects => "La quantité de particules et le tremblement d'écran.",
            Knob::Sounds => {
                "Le volume de chaque famille de bruitages (le choix des sons reste dans « Sons »)."
            }
            Knob::Music => "Le volume de la musique dans le mix.",
        }
    }
}

/// One value driven by a knob (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    pub knob: Knob,
    /// `Resource.path`, `[*]` for any index.
    pub pattern: &'static str,
    /// Factor of the default at 0...
    pub low: f64,
    /// ...and at 100.
    pub high: f64,
    /// Bounds of the value (after the factor).
    pub min: f64,
    pub max: f64,
}

const fn link(knob: Knob, pattern: &'static str, low: f64, high: f64) -> Link {
    Link {
        knob,
        pattern,
        low,
        high,
        min: f64::NEG_INFINITY,
        max: f64::INFINITY,
    }
}

/// A link whose value stays within `[min, max]` (a chance, a share, a volume).
const fn bounded(knob: Knob, pattern: &'static str, low: f64, high: f64, max: f64) -> Link {
    Link {
        knob,
        pattern,
        low,
        high,
        min: 0.0,
        max,
    }
}

use Knob::*;

/// Every value the knobs drive, knob by knob. A factor below 1 at 0 and above 1 at 100 makes
/// the value grow along the slider; the other way round, shrink.
pub const LINKS: &[Link] = &[
    // --- Dérapage: tight and lively (0) to wide and slippery (100) -------------------------------
    // Recentred by #170: the middle is now what 80 gave (Max's drift, the new defaults). The
    // tight side keeps the factors of #155; the wide side, which started from wider values, goes
    // a little less far (× 1.2 instead of × 1.35 on the radii...) so that its end stays within
    // the playable bounds of `feel_tests` (wide radius ≤ 80 m, slip angle ≤ 0.5 rad, damping
    // ≥ 0.3). Before: radii 0.75 / 1.35, angles 0.7 / 1.5, frequency 1.3 / 0.7, damping
    // 1.3 / 0.6, curve 0.6 / 1.5, hop 1.3 / 0.75, thresholds 0.8 / 1.2.
    link(Drift, "KartTuning.drift_radius_tight", 0.75, 1.2),
    link(Drift, "KartTuning.drift_radius_wide", 0.75, 1.2),
    link(Drift, "KartTuning.drift_slip_angle_tight", 0.7, 1.25),
    link(Drift, "KartTuning.drift_slip_angle_wide", 0.7, 1.25),
    link(Drift, "KartTuning.drift_slip_frequency", 1.3, 0.75),
    bounded(Drift, "KartTuning.drift_slip_damping", 1.3, 0.65, 1.0),
    link(Drift, "KartTuning.drift_curve_in_time", 0.6, 1.25),
    link(Drift, "KartTuning.drift_curve_out_time", 0.6, 1.25),
    link(Drift, "KartTuning.hop_steer_response", 1.3, 0.75),
    link(Drift, "KartTuning.mini_turbo_thresholds", 0.8, 1.15),
    // --- Vitesse: top speed and acceleration, the thrown items with them -------------------------
    link(Speed, "KartTuning.max_speed", 0.8, 1.2),
    link(Speed, "KartTuning.time_to_max_speed", 1.25, 0.75),
    link(Speed, "ItemTuning.pelote_speed", 0.8, 1.2),
    link(Speed, "ItemTuning.abeille_speed", 0.8, 1.2),
    link(Speed, "ItemTuning.abeille_dive_speed", 0.8, 1.2),
    link(Speed, "ItemTuning.fusee_speed", 0.8, 1.2),
    link(Speed, "ItemTuning.boomerang_speed", 0.8, 1.2),
    // --- Tenue de route: slippery (0) to grippy (100) ---------------------------------------------
    link(Grip, "KartTuning.lateral_grip", 0.5, 1.5),
    // Issue #193: recentred on 35 % (Max: « comme du sable »): from 28 % at 0 to 50 % at 100;
    // the brake at the entry of the grass (75 m/s² in the middle), harder at 0. Before (#171):
    // 0.875 / 1.375 around 40 % (35 % to 55 %); before #171: 0.75 / 1.3 around 55 %.
    bounded(Grip, "KartTuning.offroad_speed_factor", 0.8, 1.43, 1.0),
    link(Grip, "KartTuning.offroad_decel", 1.3, 0.7),
    link(Grip, "KartTuning.suspension_damping", 0.6, 1.6),
    link(Grip, "KartTuning.tilt_damping", 0.7, 1.4),
    // --- Boosts: strength and duration of every boost ---------------------------------------------
    link(Boosts, "KartTuning.boost_accel", 0.8, 1.25),
    link(Boosts, "KartTuning.mini_turbo_durations", 0.6, 1.5),
    link(Boosts, "KartTuning.mini_turbo_strengths", 0.6, 1.4),
    link(Boosts, "KartTuning.rocket_start_max_duration", 0.6, 1.5),
    link(Boosts, "KartTuning.rocket_start_min_duration", 0.6, 1.5),
    link(Boosts, "KartTuning.rocket_start_strength", 0.6, 1.4),
    link(Boosts, "KartTuning.trick_boost_duration", 0.6, 1.5),
    link(Boosts, "KartTuning.trick_boost_strength", 0.6, 1.4),
    link(Boosts, "KartTuning.slipstream_boost_duration", 0.6, 1.5),
    link(Boosts, "KartTuning.slipstream_boost_strength", 0.6, 1.4),
    link(Boosts, "KartTuning.boost_pad_duration", 0.6, 1.5),
    link(Boosts, "KartTuning.boost_pad_strength", 0.6, 1.4),
    link(
        Boosts,
        "KartTuning.environment.spin_boost_duration",
        0.6,
        1.5,
    ),
    link(
        Boosts,
        "KartTuning.environment.spin_boost_strength",
        0.6,
        1.4,
    ),
    link(Boosts, "ItemTuning.piment_duration", 0.6, 1.5),
    link(Boosts, "ItemTuning.piment_strength", 0.6, 1.4),
    // --- Chocs: the bumps between karts (#154), the paw (#34), the hits of the items (#196) -----
    bounded(Bumps, "KartTuning.bump_spin_keep", 0.3, 3.0, 1.0),
    link(Bumps, "KartTuning.bump_spin_max", 0.4, 2.5),
    link(Bumps, "InteractionTuning.strike_push_speed", 0.5, 1.6),
    link(Bumps, "InteractionTuning.strike_push_time", 0.8, 1.25),
    // Issue #193: 35 % in the middle (10 % before, × 0.5 / 2.0): 17.5 % at 0, 52.5 % at 100;
    // the swerve seen and the loss of a kart let go with it.
    bounded(Bumps, "InteractionTuning.strike_speed_loss", 0.5, 1.5, 1.0),
    link(Bumps, "InteractionTuning.strike_swerve", 0.5, 1.5),
    bounded(Bumps, "InteractionTuning.release_speed_loss", 0.5, 1.5, 1.0),
    link(Bumps, "InteractionTuning.throw_push_factor", 0.7, 1.4),
    // Issue #196: the shift's cap follows the push; a kart hit by an item keeps less of its speed
    // the harder it hits (12 % / 8 % in the middle: 19 % / 13 % at 0, 6 % / 4 % at 100).
    link(Bumps, "InteractionTuning.strike_push_max", 0.5, 1.6),
    bounded(Bumps, "KartTuning.hit_spin_keep", 1.6, 0.5, 1.0),
    bounded(Bumps, "KartTuning.hit_tumble_keep", 1.6, 0.5, 1.0),
    // --- Bots: level, catch-up and aggressiveness, the three levels together ----------------------
    link(Bots, "BotTuning.skills[*].wobble", 1.6, 0.5),
    bounded(Bots, "BotTuning.skills[*].cruise_speed", 0.95, 1.03, 1.0),
    bounded(Bots, "BotTuning.skills[*].corner_margin", 0.95, 1.015, 1.0),
    bounded(Bots, "BotTuning.skills[*].drift_chance", 0.7, 1.2, 1.0),
    link(Bots, "BotTuning.skills[*].item_delay", 1.8, 0.5),
    bounded(Bots, "BotTuning.skills[*].strike_chance", 0.3, 1.6, 1.0),
    bounded(Bots, "BotTuning.skills[*].grab_chance", 0.3, 1.4, 1.0),
    link(Bots, "BotTuning.catch_up_max", 0.0, 2.0),
    // --- Objets: how often, how long the roulette, how strong -------------------------------------
    link(Items, "ItemTuning.box_respawn_time", 2.0, 0.5),
    link(Items, "ItemTuning.roulette_time", 1.5, 0.6),
    link(Items, "ItemTuning.roulette_slowdown", 1.5, 0.6),
    link(Items, "ItemTuning.gap_scale", 0.6, 1.6),
    // --- Caméra: close and lively (0) to far and smooth (100) -------------------------------------
    link(Camera, "CameraTuning.distance", 0.75, 1.4),
    link(Camera, "CameraTuning.height", 0.75, 1.4),
    link(Camera, "CameraTuning.swing_frequency", 1.4, 0.6),
    link(Camera, "CameraTuning.vertical_frequency", 1.4, 0.6),
    link(Camera, "CameraTuning.look_frequency", 1.4, 0.6),
    // --- Effets: the particles (#105) and the screen shakes ---------------------------------------
    link(Effects, "VfxTuning.max_particles", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.spark_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.flash_sparks", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.smoke_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.release_sparks", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.trail_rate", 0.2, 1.6),
    // Issue #174: the drift's flames (orange, purple: blue has none), embers (purple) and halo,
    // the release's blast of flames.
    link(Effects, "VfxTuning.drift.flame_rate[1]", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.flame_rate[2]", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.ember_rate[2]", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.halo_glow", 0.2, 1.6),
    link(Effects, "VfxTuning.drift.release_flames", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.ember_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.speed_line_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.rocket_smoke", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.trick_sparkles", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.slipstream_streaks", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.spin_sparks", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.pad_sparks", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.piment_sparks", 0.2, 1.6),
    link(Effects, "VfxTuning.boost.stall_puffs", 0.2, 1.6),
    // Issue #185: the soot of an engine that stays stalled.
    link(Effects, "VfxTuning.boost.stall_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.pof_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.wall_stars", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.wall_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.paw_stars", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.confetti", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.blast_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.gift_confetti", 0.2, 1.6),
    link(Effects, "VfxTuning.impact.cream_blobs", 0.2, 1.6),
    link(Effects, "VfxTuning.world.croquette_sparkles", 0.2, 1.6),
    link(Effects, "VfxTuning.world.lost_sparkles", 0.2, 1.6),
    link(Effects, "VfxTuning.world.dust_rate", 0.2, 1.6),
    // Issue #193: the dust of a kart braking into the grass.
    link(Effects, "VfxTuning.world.dust_plough", 0.2, 1.6),
    link(Effects, "VfxTuning.world.landing_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.world.anti_gravity_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.world.glide_wind_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.world.splash_drops", 0.2, 1.6),
    link(Effects, "VfxTuning.world.splash_foam", 0.2, 1.6),
    link(Effects, "VfxTuning.world.wake_rate", 0.2, 1.6),
    // Issue #176: the tyres spinning at a standing start, the cloud at GO, the puff setting off
    // again from a stop, the kart's little shakes on the grid.
    link(Effects, "VfxTuning.wheels.smoke_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.wheels.dust_rate", 0.2, 1.6),
    link(Effects, "VfxTuning.wheels.go_rocket_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.wheels.go_normal_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.wheels.go_stall_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.wheels.launch_puffs", 0.2, 1.6),
    link(Effects, "VfxTuning.wheels.shake", 0.0, 2.0),
    link(Effects, "VfxTuning.impact.shake_bump", 0.0, 2.0),
    link(Effects, "VfxTuning.impact.shake_wall", 0.0, 2.0),
    link(Effects, "VfxTuning.impact.shake_hit", 0.0, 2.0),
    link(Effects, "InteractionTuning.struck_shake", 0.0, 2.0),
    // --- Sons: the volume of each family (0 to 2) -------------------------------------------------
    bounded(Sounds, "SoundTuning.engine_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.drive_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.item_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.hit_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.race_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.voice_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.screen_volume", 0.25, 2.0, 2.0),
    bounded(Sounds, "SoundTuning.world_volume", 0.25, 2.0, 2.0),
    // --- Musique ----------------------------------------------------------------------------------
    bounded(Music, "MusicTuning.volume", 0.2, 2.0, 2.0),
];

impl Link {
    /// Whether this link drives the value at `full_path` (`Resource.path`).
    pub fn matches(&self, full_path: &str) -> bool {
        matches_pattern(self.pattern, full_path)
    }

    /// The factor of the default at `position` (1 in the middle).
    pub fn factor(&self, position: u8) -> f64 {
        let t = (f64::from(position.min(KNOB_MAX)) - f64::from(MIDDLE)) / f64::from(MIDDLE);
        if t >= 0.0 {
            1.0 + (self.high - 1.0) * t
        } else {
            1.0 + (self.low - 1.0) * -t
        }
    }

    /// The value this link gives the field whose default is `default`, at `position`: exactly
    /// the default in the middle, the same kind of value everywhere. A switch or a choice is
    /// never driven: it keeps its default.
    pub fn value(&self, default: super::Value, position: u8) -> super::Value {
        if position == MIDDLE {
            return default;
        }
        let Some(number) = default.as_f64() else {
            return default;
        };
        let scaled = (number * self.factor(position)).clamp(self.min, self.max);
        default.with_number(scaled).unwrap_or(default)
    }
}

/// `pattern` names `path`: the same text, `[*]` matching any index, and a pattern names the
/// values under it (`a` names `a[2]` and `a.b`, not `ab`).
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    let (mut p, mut s) = (pattern.as_bytes(), path.as_bytes());
    loop {
        if p.is_empty() {
            return s.is_empty() || s[0] == b'[' || s[0] == b'.';
        }
        if p.starts_with(b"[*]") {
            let Some(rest) = s.strip_prefix(b"[") else {
                return false;
            };
            let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
            let Some(rest) = rest[digits..].strip_prefix(b"]") else {
                return false;
            };
            if digits == 0 {
                return false;
            }
            p = &p[3..];
            s = rest;
            continue;
        }
        if s.first() != p.first() {
            return false;
        }
        p = &p[1..];
        s = &s[1..];
    }
}

/// The link that drives `full_path`, if any.
pub fn link_for(full_path: &str) -> Option<usize> {
    LINKS.iter().position(|l| l.matches(full_path))
}

/// Where each knob stands. 50 everywhere by default: the game's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feel {
    positions: [u8; Knob::ALL.len()],
}

impl Default for Feel {
    fn default() -> Self {
        Self {
            positions: [MIDDLE; Knob::ALL.len()],
        }
    }
}

impl Feel {
    pub fn position(&self, knob: Knob) -> u8 {
        self.positions[knob.index()]
    }

    pub fn set(&mut self, knob: Knob, position: u8) {
        self.positions[knob.index()] = position.min(KNOB_MAX);
    }

    /// Every knob in the middle.
    pub fn is_middle(&self) -> bool {
        self.positions.iter().all(|p| *p == MIDDLE)
    }

    /// The knobs off the middle, in the panel's order.
    pub fn moved(&self) -> impl Iterator<Item = (Knob, u8)> + '_ {
        Knob::ALL
            .into_iter()
            .map(|k| (k, self.position(k)))
            .filter(|(_, p)| *p != MIDDLE)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Value;
    use super::*;

    #[test]
    fn patterns_name_their_values_and_those_under_them() {
        let yes = [
            ("KartTuning.max_speed", "KartTuning.max_speed"),
            (
                "KartTuning.mini_turbo_durations",
                "KartTuning.mini_turbo_durations[2]",
            ),
            ("BotTuning.skills[*].wobble", "BotTuning.skills[0].wobble"),
            ("BotTuning.skills[*].wobble", "BotTuning.skills[12].wobble"),
            ("VfxTuning.drift", "VfxTuning.drift.spark_rate[1]"),
        ];
        for (pattern, path) in yes {
            assert!(matches_pattern(pattern, path), "{pattern} / {path}");
        }
        let no = [
            ("KartTuning.max_speed", "KartTuning.max_speed_fov"),
            ("KartTuning.max_speed", "KartTuning.max_spee"),
            ("BotTuning.skills[*].wobble", "BotTuning.skills[].wobble"),
            (
                "BotTuning.skills[*].wobble",
                "BotTuning.skills[1].wobble_period",
            ),
            ("BotTuning.skills[*].wobble", "BotTuning.skills.wobble"),
            ("CameraTuning.height", "KartTuning.height"),
        ];
        for (pattern, path) in no {
            assert!(!matches_pattern(pattern, path), "{pattern} / {path}");
        }
    }

    #[test]
    fn the_factor_is_one_in_the_middle_and_linear_on_each_half() {
        let l = link(Drift, "KartTuning.max_speed", 0.6, 1.5);
        assert_eq!(l.factor(MIDDLE), 1.0);
        assert_eq!(l.factor(0), 0.6);
        assert_eq!(l.factor(KNOB_MAX), 1.5);
        assert!((l.factor(25) - 0.8).abs() < 1e-12);
        assert!((l.factor(75) - 1.25).abs() < 1e-12);
        // Past the end: the end.
        assert_eq!(l.factor(200), 1.5);
    }

    #[test]
    fn values_keep_their_kind_and_their_bounds() {
        let chance = bounded(Bots, "BotTuning.skills[*].grab_chance", 0.3, 1.4, 1.0);
        assert_eq!(chance.value(Value::F32(0.7), MIDDLE), Value::F32(0.7));
        let Value::F32(high) = chance.value(Value::F32(0.7), KNOB_MAX) else {
            panic!("not a float");
        };
        assert!((high - 0.98).abs() < 1e-6, "{high}");
        assert_eq!(chance.value(Value::F32(0.9), KNOB_MAX), Value::F32(1.0));
        let count = link(Effects, "VfxTuning.impact.pof_puffs", 0.2, 1.6);
        assert_eq!(count.value(Value::U32(6), 0), Value::U32(1));
        assert_eq!(count.value(Value::U32(6), KNOB_MAX), Value::U32(10));
        assert_eq!(count.value(Value::Usize(900), 0), Value::Usize(180));
        // A switch is never driven.
        assert_eq!(count.value(Value::Bool(true), 0), Value::Bool(true));
    }

    #[test]
    fn knobs_have_a_key_a_name_and_both_ends() {
        for knob in Knob::ALL {
            assert_eq!(Knob::from_key(knob.key()), Some(knob));
            assert!(knob.key().bytes().all(|c| c.is_ascii_lowercase()));
            assert!(knob.low().starts_with('←') && knob.high().ends_with('→'));
            // The charter's short lines (`docs/UI_CHARTE.md`, « Ton des textes »).
            assert!(knob.low().chars().count() <= 48 && knob.high().chars().count() <= 48);
            assert!(LINKS.iter().filter(|l| l.knob == knob).count() >= 1);
        }
        assert_eq!(Knob::from_key("turbo"), None);
    }

    #[test]
    fn every_link_moves_one_way_with_one_on_either_side() {
        for l in LINKS {
            assert!(
                (l.low < 1.0 && l.high > 1.0) || (l.low > 1.0 && l.high < 1.0),
                "{}: {} / {}",
                l.pattern,
                l.low,
                l.high
            );
            assert!(l.low >= 0.0 && l.min <= l.max, "{}", l.pattern);
        }
    }

    #[test]
    fn the_feel_starts_in_the_middle() {
        let mut feel = Feel::default();
        assert!(feel.is_middle());
        assert_eq!(feel.moved().count(), 0);
        feel.set(Knob::Bots, 130);
        assert_eq!(feel.position(Knob::Bots), KNOB_MAX);
        assert_eq!(feel.moved().collect::<Vec<_>>(), [(Knob::Bots, KNOB_MAX)]);
    }
}
