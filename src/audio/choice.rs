//! Issue #135: **the sound picker** (pure, unit-tested). Every place of the sound design whose
//! sound Max can choose in the admin panel (P, section « Sons ») is a [`Slot`]: the engine, the
//! drift's squeal, its chimes, the boosts, the box, each item, the hits, the cries, the screens...
//! Each slot has its [`Candidate`]s, the first one its sound by default; [`SoundChoices`] holds
//! which one plays (kept in the browser, and shown as a text Max can copy: [`SoundChoices::export`]).
//!
//! - **A one-shot** plays its slot's candidate ([`pick`]): the sound of #103 (`Original`: a file
//!   of Kenney or a synthesis, `SoundEffect::clip`), another file (CC0, `assets/audio/`, credited
//!   in `assets/CREDITS.md`), or another synthesis; with a speed (its pitch) and a gain of its own.
//!   The variants of a slot follow it: the drift's three chimes climb with a file too, a
//!   footstep takes its own file of the three.
//! - **A loop** (the engine, the squeal, the glider's wind, the rumble off the road) plays a
//!   synthesized timbre (`dsp::LoopTimbre`) or a recording (a file made into a seamless loop once,
//!   `dsp::seamless_loop`), at its speed: the engine's pitch still follows the revs (the playback
//!   speed of the recording, times its own).
//!
//! The files of the candidates are only loaded when they are chosen or listened to (those of the
//! sounds by default are loaded with the game).
//!
//! **Issue #235**: the first candidate of every slot (but the footsteps) is a sound generated with
//! ElevenLabs (`assets/audio/elevenlabs/`, Max's decision: not CC0, credited in
//! `assets/CREDITS.md`), so it is the sound by default; #103's comes next, still chosen in the
//! panel. The cries are a file per animal and mood (`EL_CRIES`, in the order `variant` gives).
//!
//! **Issue #247**: the engine by default is SuperTuxKart's small engine
//! (`assets/audio/supertuxkart/`, free licences, Max's decision), ElevenLabs' right after it; its
//! big engine, its skid and its crash are offered too.

use bevy::prelude::*;

use crate::kart::MiniTurbo;

use super::catalog::{Animal, Clip, Mood, SoundEffect};
use super::dsp::{LoopKind, LoopTimbre, Noise, Synth, Tone, Wave};

/// Where a candidate's sound comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// The sound of #103 (`SoundEffect::clip`: a file or a synthesis; a slot of variants keeps
    /// each one's own).
    Original,
    /// A file (its path under `assets/`, its length in seconds: measured, a test decodes it).
    File(&'static (&'static str, f32)),
    /// A file for each variant (a footstep each): the variant's, in turn.
    Files(&'static [(&'static str, f32)]),
    /// A synthesis (rendered once, when it is chosen or listened to).
    Synth(Synth),
    /// A loop synthesized.
    Loop(LoopTimbre),
    /// A loop recorded: its file (path, seconds), low-passed at `lowpass` Hz (0: not) when it is
    /// made into a seamless loop.
    Recording {
        path: &'static str,
        seconds: f32,
        lowpass: f32,
    },
}

/// A sound Max can choose for a slot: its id (kept in the browser, in the text he copies), its
/// name in the panel, its source, its playback speed (its pitch) and its gain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub id: &'static str,
    pub name: &'static str,
    pub source: Source,
    pub speed: f32,
    pub gain: f32,
}

impl Candidate {
    const fn new(id: &'static str, name: &'static str, source: Source) -> Self {
        Self {
            id,
            name,
            source,
            speed: 1.0,
            gain: 1.0,
        }
    }

    const fn speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// The files it plays (none for a synthesis, nor for the sound of #103: its files are the
    /// game's, `catalog::FILES`).
    pub fn files(&self) -> &'static [(&'static str, f32)] {
        match self.source {
            Source::File(file) => std::slice::from_ref(file),
            Source::Files(files) => files,
            _ => &[],
        }
    }

    /// The file of a recorded loop.
    pub fn recording(&self) -> Option<(&'static str, f32)> {
        match self.source {
            Source::Recording { path, seconds, .. } => Some((path, seconds)),
            _ => None,
        }
    }
}

const fn original(id: &'static str, name: &'static str) -> Candidate {
    Candidate::new(id, name, Source::Original)
}

const fn file(
    id: &'static str,
    name: &'static str,
    file: &'static (&'static str, f32),
) -> Candidate {
    Candidate::new(id, name, Source::File(file))
}

const fn synth(id: &'static str, name: &'static str, synth: Synth) -> Candidate {
    Candidate::new(id, name, Source::Synth(synth))
}

const fn looped(id: &'static str, name: &'static str, timbre: LoopTimbre) -> Candidate {
    Candidate::new(id, name, Source::Loop(timbre))
}

const fn recording(
    id: &'static str,
    name: &'static str,
    (path, seconds): (&'static str, f32),
    lowpass: f32,
) -> Candidate {
    Candidate::new(
        id,
        name,
        Source::Recording {
            path,
            seconds,
            lowpass,
        },
    )
}

// The files of the candidates (issue #135), with their length (s, decoded: a test checks it).
// Kenney's packs (CC0), the ones of #103 and more of the same packs.
const IMPACT_PUNCH_HEAVY_000: (&str, f32) =
    ("audio/impact-sounds/impactPunch_heavy_000.ogg", 0.649);
const IMPACT_PUNCH_HEAVY_001: (&str, f32) =
    ("audio/impact-sounds/impactPunch_heavy_001.ogg", 0.536);
const IMPACT_PUNCH_HEAVY_002: (&str, f32) =
    ("audio/impact-sounds/impactPunch_heavy_002.ogg", 0.457);
const IMPACT_SOFT_HEAVY_001: (&str, f32) = ("audio/impact-sounds/impactSoft_heavy_001.ogg", 0.569);
const IMPACT_SOFT_HEAVY_003: (&str, f32) = ("audio/impact-sounds/impactSoft_heavy_003.ogg", 0.550);
const IMPACT_SOFT_HEAVY_004: (&str, f32) = ("audio/impact-sounds/impactSoft_heavy_004.ogg", 0.504);
const IMPACT_SOFT_MEDIUM_002: (&str, f32) =
    ("audio/impact-sounds/impactSoft_medium_002.ogg", 0.156);
const IMPACT_SOFT_MEDIUM_004: (&str, f32) =
    ("audio/impact-sounds/impactSoft_medium_004.ogg", 0.156);
const IMPACT_WOOD_LIGHT_002: (&str, f32) = ("audio/impact-sounds/impactWood_light_002.ogg", 0.275);
const IMPACT_WOOD_HEAVY_001: (&str, f32) = ("audio/impact-sounds/impactWood_heavy_001.ogg", 0.319);
const IMPACT_GENERIC_LIGHT_001: (&str, f32) =
    ("audio/impact-sounds/impactGeneric_light_001.ogg", 0.115);
const IMPACT_GENERIC_LIGHT_002: (&str, f32) =
    ("audio/impact-sounds/impactGeneric_light_002.ogg", 0.161);
const IMPACT_PLANK_MEDIUM_000: (&str, f32) =
    ("audio/impact-sounds/impactPlank_medium_000.ogg", 0.779);
const IMPACT_GLASS_LIGHT_000: (&str, f32) =
    ("audio/impact-sounds/impactGlass_light_000.ogg", 0.231);
const IMPACT_TIN_MEDIUM_000: (&str, f32) = ("audio/impact-sounds/impactTin_medium_000.ogg", 0.179);
const FOOTSTEPS_CARPET: [(&str, f32); 3] = [
    ("audio/impact-sounds/footstep_carpet_000.ogg", 0.167),
    ("audio/impact-sounds/footstep_carpet_001.ogg", 0.164),
    ("audio/impact-sounds/footstep_carpet_003.ogg", 0.159),
];
const FOOTSTEPS_SNOW: [(&str, f32); 3] = [
    ("audio/impact-sounds/footstep_snow_000.ogg", 0.379),
    ("audio/impact-sounds/footstep_snow_001.ogg", 0.388),
    ("audio/impact-sounds/footstep_snow_002.ogg", 0.388),
];
const GLASS_002: (&str, f32) = ("audio/interface-sounds/glass_002.ogg", 0.130);
const GLASS_003: (&str, f32) = ("audio/interface-sounds/glass_003.ogg", 0.127);
const PLUCK_002: (&str, f32) = ("audio/interface-sounds/pluck_002.ogg", 0.182);
const BONG_001: (&str, f32) = ("audio/interface-sounds/bong_001.ogg", 0.133);
const CONFIRMATION_002: (&str, f32) = ("audio/interface-sounds/confirmation_002.ogg", 0.539);
const CONFIRMATION_004: (&str, f32) = ("audio/interface-sounds/confirmation_004.ogg", 0.490);
const SELECT_002: (&str, f32) = ("audio/interface-sounds/select_002.ogg", 0.063);
const DROP_002: (&str, f32) = ("audio/interface-sounds/drop_002.ogg", 0.202);
const TICK_004: (&str, f32) = ("audio/interface-sounds/tick_004.ogg", 0.055);
const BACK_001: (&str, f32) = ("audio/interface-sounds/back_001.ogg", 0.069);
const BACK_004: (&str, f32) = ("audio/interface-sounds/back_004.ogg", 0.094);
const ROLLOVER_4: (&str, f32) = ("audio/ui-audio/rollover4.ogg", 0.124);
const PHASER_UP_1: (&str, f32) = ("audio/digital-audio/phaserUp1.ogg", 0.518);
const PHASER_UP_3: (&str, f32) = ("audio/digital-audio/phaserUp3.ogg", 0.544);
const PHASER_UP_4: (&str, f32) = ("audio/digital-audio/phaserUp4.ogg", 0.379);
const PHASER_UP_5: (&str, f32) = ("audio/digital-audio/phaserUp5.ogg", 0.333);
const PHASER_UP_7: (&str, f32) = ("audio/digital-audio/phaserUp7.ogg", 0.382);
const PHASER_DOWN_1: (&str, f32) = ("audio/digital-audio/phaserDown1.ogg", 0.484);
const PHASER_DOWN_2: (&str, f32) = ("audio/digital-audio/phaserDown2.ogg", 0.321);
const PHASER_DOWN_3: (&str, f32) = ("audio/digital-audio/phaserDown3.ogg", 0.518);
const HIGH_DOWN: (&str, f32) = ("audio/digital-audio/highDown.ogg", 0.530);
const LOW_DOWN: (&str, f32) = ("audio/digital-audio/lowDown.ogg", 0.800);
const ZAP_THREE_TONE_UP: (&str, f32) = ("audio/digital-audio/zapThreeToneUp.ogg", 1.202);
const POWER_UP_4: (&str, f32) = ("audio/digital-audio/powerUp4.ogg", 0.562);
const POWER_UP_9: (&str, f32) = ("audio/digital-audio/powerUp9.ogg", 0.565);
const TWO_TONE_2: (&str, f32) = ("audio/digital-audio/twoTone2.ogg", 0.751);
const PHASE_JUMP_1: (&str, f32) = ("audio/digital-audio/phaseJump1.ogg", 0.467);
const PHASE_JUMP_2: (&str, f32) = ("audio/digital-audio/phaseJump2.ogg", 0.389);
const PHASE_JUMP_5: (&str, f32) = ("audio/digital-audio/phaseJump5.ogg", 0.434);
const PEP_SOUND_2: (&str, f32) = ("audio/digital-audio/pepSound2.ogg", 0.559);
const CLOTH_3: (&str, f32) = ("audio/rpg-audio/cloth3.ogg", 0.477);
const DRAW_KNIFE_3: (&str, f32) = ("audio/rpg-audio/drawKnife3.ogg", 0.486);
const HANDLE_SMALL_LEATHER: (&str, f32) = ("audio/rpg-audio/handleSmallLeather.ogg", 0.338);
const DROP_LEATHER: (&str, f32) = ("audio/rpg-audio/dropLeather.ogg", 0.415);
const SLIME_000: (&str, f32) = ("audio/sci-fi-sounds/slime_000.ogg", 0.500);
const FORCE_FIELD_000: (&str, f32) = ("audio/sci-fi-sounds/forceField_000.ogg", 0.954);
const FORCE_FIELD_002: (&str, f32) = ("audio/sci-fi-sounds/forceField_002.ogg", 0.955);
const EXPLOSION_CRUNCH_002: (&str, f32) = ("audio/sci-fi-sounds/explosionCrunch_002.ogg", 1.256);
const LOW_EXPLOSION: (&str, f32) = ("audio/sci-fi-sounds/lowFrequency_explosion_000.ogg", 2.000);
const LASER_LARGE_002: (&str, f32) = ("audio/sci-fi-sounds/laserLarge_002.ogg", 0.744);
const SPACE_ENGINE_SMALL: (&str, f32) = ("audio/sci-fi-sounds/spaceEngineSmall_002.ogg", 5.000);
const ENGINE_CIRCULAR: (&str, f32) = ("audio/sci-fi-sounds/engineCircular_002.ogg", 5.000);
// OpenGameArt (CC0, each page checked): the engines, the drifts and what Kenney has not.
const ZIPH_MOTOR_01: (&str, f32) = ("audio/ziph/motorseamless01.ogg", 4.000);
const ZIPH_MOTOR_07: (&str, f32) = ("audio/ziph/motorseamless07.ogg", 4.000);
const ZIPH_MOTOR_13: (&str, f32) = ("audio/ziph/motorseamless13.ogg", 4.000);
const MOUTH_MOTOR: (&str, f32) = ("audio/ezduzziteh/motor_looping.ogg", 0.993);
const SCRAPE: (&str, f32) = ("audio/antumdeluge/scrape-2.ogg", 0.801);
const RUBBERDUCK_NOISE: (&str, f32) = ("audio/rubberduck/noise_03.ogg", 2.674);
const RUBBERDUCK_ROLLING: (&str, f32) = ("audio/rubberduck/rolling.ogg", 1.056);
const RUBBERDUCK_MUD_11: (&str, f32) = ("audio/rubberduck/mud_11.ogg", 0.224);
const RUBBERDUCK_MUD_18: (&str, f32) = ("audio/rubberduck/mud_18.ogg", 0.597);
const RUBBERDUCK_SLIME_09: (&str, f32) = ("audio/rubberduck/slime_09.ogg", 0.667);
const RUBBERDUCK_SLIME_15: (&str, f32) = ("audio/rubberduck/slime_15.ogg", 0.518);
const RUBBERDUCK_SPLASH_02: (&str, f32) = ("audio/rubberduck/splash_02.ogg", 0.733);
const RUBBERDUCK_SPLASH_04: (&str, f32) = ("audio/rubberduck/splash_04.ogg", 1.021);
const RUBBERDUCK_SPRING_01: (&str, f32) = ("audio/rubberduck/spring_01.ogg", 1.301);
const RUBBERDUCK_SPRING_05: (&str, f32) = ("audio/rubberduck/spring_05.ogg", 0.655);
const QUBODUP_CRASH: (&str, f32) = ("audio/qubodup/crash.ogg", 0.600);
const ROCHA_SPIN: (&str, f32) = ("audio/rocha57/car_spinning.ogg", 0.690);
const SKETCHMAN_WIND: (&str, f32) = ("audio/sketchman3/wind_woosh_loop.ogg", 5.959);
const BART_SQUEAK: (&str, f32) = ("audio/bart/squeak2.ogg", 0.501);
// Issue #247: sounds of SuperTuxKart (free licences, Max's decision of 2026-11-11: « Récupère les
// sons de tuxkart ou supertuxkart qui sont libres »), `assets/audio/supertuxkart/`: its small
// engine is the engine by default.
const STK_ENGINE_SMALL: (&str, f32) = ("audio/supertuxkart/engine_small.ogg", 4.000);
const STK_ENGINE_LARGE: (&str, f32) = ("audio/supertuxkart/engine_large.ogg", 2.922);
const STK_SKID: (&str, f32) = ("audio/supertuxkart/skid.ogg", 3.000);
const STK_CRASH: (&str, f32) = ("audio/supertuxkart/crash.ogg", 0.390);
// Issue #235: the sounds generated with ElevenLabs (elevenlabs.io, Max's decision of
// 2026-11-11), the first candidate of every slot (but the footsteps): `assets/audio/elevenlabs/`.
const EL_BEE_BUZZ: (&str, f32) = ("audio/elevenlabs/bee_buzz.ogg", 1.500);
const EL_BOOMERANG_THROW: (&str, f32) = ("audio/elevenlabs/boomerang_throw.ogg", 0.883);
const EL_BOOST_PAD: (&str, f32) = ("audio/elevenlabs/boost_pad.ogg", 0.801);
const EL_BOX_BREAK: (&str, f32) = ("audio/elevenlabs/box_break.ogg", 0.613);
const EL_COUNTDOWN_BEEP: (&str, f32) = ("audio/elevenlabs/countdown_beep.ogg", 0.319);
const EL_COUNTDOWN_GO: (&str, f32) = ("audio/elevenlabs/countdown_go.ogg", 0.800);
const EL_CREAM_SQUIRT: (&str, f32) = ("audio/elevenlabs/cream_squirt.ogg", 0.834);
const EL_CREAMED: (&str, f32) = ("audio/elevenlabs/creamed.ogg", 0.502);
const EL_DEFLATE: (&str, f32) = ("audio/elevenlabs/deflate.ogg", 0.941);
const EL_DRIFT_TIER: (&str, f32) = ("audio/elevenlabs/drift_tier.ogg", 1.000);
const EL_ENGINE: (&str, f32) = ("audio/elevenlabs/engine.ogg", 3.000);
const EL_EXPLOSION: (&str, f32) = ("audio/elevenlabs/explosion.ogg", 1.000);
const EL_FALL: (&str, f32) = ("audio/elevenlabs/fall.ogg", 1.000);
const EL_FOOT_JUMP: (&str, f32) = ("audio/elevenlabs/foot_jump.ogg", 0.500);
const EL_FUSEE: (&str, f32) = ("audio/elevenlabs/fusee.ogg", 1.548);
const EL_GHOST: (&str, f32) = ("audio/elevenlabs/ghost.ogg", 1.000);
const EL_GLIDER_OPEN: (&str, f32) = ("audio/elevenlabs/glider_open.ogg", 0.753);
const EL_GRAB: (&str, f32) = ("audio/elevenlabs/grab.ogg", 0.800);
const EL_HONK: (&str, f32) = ("audio/elevenlabs/honk.ogg", 0.480);
const EL_ITEM_BLOCKED: (&str, f32) = ("audio/elevenlabs/item_blocked.ogg", 1.000);
const EL_KART_BUMP: (&str, f32) = ("audio/elevenlabs/kart_bump.ogg", 0.611);
const EL_LANDING: (&str, f32) = ("audio/elevenlabs/landing.ogg", 0.806);
const EL_MINI_TURBO: (&str, f32) = ("audio/elevenlabs/mini_turbo.ogg", 0.561);
const EL_PAW_POF: (&str, f32) = ("audio/elevenlabs/paw_pof.ogg", 0.800);
const EL_PAW_SWING: (&str, f32) = ("audio/elevenlabs/paw_swing.ogg", 0.510);
const EL_PELICAN: (&str, f32) = ("audio/elevenlabs/pelican.ogg", 1.469);
const EL_PELOTE_BOUNCE: (&str, f32) = ("audio/elevenlabs/pelote_bounce.ogg", 0.800);
const EL_PILLOW_FALL: (&str, f32) = ("audio/elevenlabs/pillow_fall.ogg", 0.580);
const EL_PILLOW_LAUNCH: (&str, f32) = ("audio/elevenlabs/pillow_launch.ogg", 0.594);
const EL_PIMENT: (&str, f32) = ("audio/elevenlabs/piment.ogg", 0.414);
const EL_PINATA_BURST: (&str, f32) = ("audio/elevenlabs/pinata_burst.ogg", 1.500);
const EL_ROCKET_START: (&str, f32) = ("audio/elevenlabs/rocket_start.ogg", 0.914);
const EL_ROULETTE_STOP: (&str, f32) = ("audio/elevenlabs/roulette_stop.ogg", 1.200);
const EL_ROULETTE_TICK: (&str, f32) = ("audio/elevenlabs/roulette_tick.ogg", 0.064);
const EL_RUMBLE: (&str, f32) = ("audio/elevenlabs/rumble.ogg", 3.000);
const EL_SLIPSTREAM: (&str, f32) = ("audio/elevenlabs/slipstream.ogg", 0.830);
const EL_SPIN_BOOST: (&str, f32) = ("audio/elevenlabs/spin_boost.ogg", 1.200);
const EL_SPIN_OUT: (&str, f32) = ("audio/elevenlabs/spin_out.ogg", 0.854);
const EL_SPLASH: (&str, f32) = ("audio/elevenlabs/splash.ogg", 0.809);
const EL_SPLOTCH: (&str, f32) = ("audio/elevenlabs/splotch.ogg", 0.654);
const EL_SQUEAL: (&str, f32) = ("audio/elevenlabs/squeal.ogg", 3.000);
const EL_STALL: (&str, f32) = ("audio/elevenlabs/stall.ogg", 0.899);
const EL_STEAL: (&str, f32) = ("audio/elevenlabs/steal.ogg", 0.900);
const EL_SUPER_CALIN: (&str, f32) = ("audio/elevenlabs/super_calin.ogg", 1.000);
const EL_TETHER_BOING: (&str, f32) = ("audio/elevenlabs/tether_boing.ogg", 1.200);
const EL_THROW: (&str, f32) = ("audio/elevenlabs/throw.ogg", 0.455);
const EL_TRICK: (&str, f32) = ("audio/elevenlabs/trick.ogg", 1.200);
const EL_TRICK_BOOST: (&str, f32) = ("audio/elevenlabs/trick_boost.ogg", 1.400);
const EL_TUMBLE: (&str, f32) = ("audio/elevenlabs/tumble.ogg", 0.831);
const EL_UI_BACK: (&str, f32) = ("audio/elevenlabs/ui_back.ogg", 0.400);
const EL_UI_CHANGE: (&str, f32) = ("audio/elevenlabs/ui_change.ogg", 0.319);
const EL_UI_CONFIRM: (&str, f32) = ("audio/elevenlabs/ui_confirm.ogg", 0.458);
const EL_UI_HOVER: (&str, f32) = ("audio/elevenlabs/ui_hover.ogg", 0.269);
const EL_WALL_HIT: (&str, f32) = ("audio/elevenlabs/wall_hit.ogg", 1.000);
const EL_WIND: (&str, f32) = ("audio/elevenlabs/wind.ogg", 3.000);
const EL_CRIES: [(&str, f32); 36] = [
    ("audio/elevenlabs/cry_chick_hurt.ogg", 0.917),
    ("audio/elevenlabs/cry_chick_happy.ogg", 0.711),
    ("audio/elevenlabs/cry_chick_effort.ogg", 0.480),
    ("audio/elevenlabs/cry_chick_win.ogg", 1.968),
    ("audio/elevenlabs/cry_penguin_hurt.ogg", 1.000),
    ("audio/elevenlabs/cry_penguin_happy.ogg", 0.777),
    ("audio/elevenlabs/cry_penguin_effort.ogg", 0.353),
    ("audio/elevenlabs/cry_penguin_win.ogg", 1.924),
    ("audio/elevenlabs/cry_parrot_hurt.ogg", 1.000),
    ("audio/elevenlabs/cry_parrot_happy.ogg", 1.000),
    ("audio/elevenlabs/cry_parrot_effort.ogg", 0.431),
    ("audio/elevenlabs/cry_parrot_win.ogg", 1.883),
    ("audio/elevenlabs/cry_cat_hurt.ogg", 1.000),
    ("audio/elevenlabs/cry_cat_happy.ogg", 0.848),
    ("audio/elevenlabs/cry_cat_effort.ogg", 0.480),
    ("audio/elevenlabs/cry_cat_win.ogg", 2.000),
    ("audio/elevenlabs/cry_bunny_hurt.ogg", 0.779),
    ("audio/elevenlabs/cry_bunny_happy.ogg", 0.896),
    ("audio/elevenlabs/cry_bunny_effort.ogg", 0.388),
    ("audio/elevenlabs/cry_bunny_win.ogg", 1.793),
    ("audio/elevenlabs/cry_fox_hurt.ogg", 0.907),
    ("audio/elevenlabs/cry_fox_happy.ogg", 1.000),
    ("audio/elevenlabs/cry_fox_effort.ogg", 0.480),
    ("audio/elevenlabs/cry_fox_win.ogg", 1.852),
    ("audio/elevenlabs/cry_pig_hurt.ogg", 0.824),
    ("audio/elevenlabs/cry_pig_happy.ogg", 0.839),
    ("audio/elevenlabs/cry_pig_effort.ogg", 0.411),
    ("audio/elevenlabs/cry_pig_win.ogg", 1.790),
    ("audio/elevenlabs/cry_cow_hurt.ogg", 0.952),
    ("audio/elevenlabs/cry_cow_happy.ogg", 0.940),
    ("audio/elevenlabs/cry_cow_effort.ogg", 0.480),
    ("audio/elevenlabs/cry_cow_win.ogg", 1.879),
    ("audio/elevenlabs/cry_elephant_hurt.ogg", 1.000),
    ("audio/elevenlabs/cry_elephant_happy.ogg", 1.000),
    ("audio/elevenlabs/cry_elephant_effort.ogg", 0.480),
    ("audio/elevenlabs/cry_elephant_win.ogg", 2.000),
];

/// The mini-turbo's « fwoosh », darker (#135): lower, its band narrower.
const SOFT_FWOOSH: Noise = Noise {
    tone: 0.05,
    q: 1.1,
    ..Noise::whoosh(300.0, 1600.0, 0.45, 0.2)
};

/// The bee, lower (#135): a slower, rounder buzz.
const LOW_BUZZ: Tone = Tone {
    from_hz: 150.0,
    to_hz: 172.0,
    duration: 0.9,
    wave: Wave::Buzz,
    step: None,
    vibrato: 1.0,
    vibrato_hz: 14.0,
    vibrato_fade: 0.0,
    decay: 1.5,
};

/// The groups of the panel's section « Sons ».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Drive,
    Start,
    Boxes,
    Items,
    Hits,
    World,
    Voices,
    Screens,
    Salon,
}

impl Group {
    pub const ALL: [Self; 9] = [
        Group::Drive,
        Group::Start,
        Group::Boxes,
        Group::Items,
        Group::Hits,
        Group::World,
        Group::Voices,
        Group::Screens,
        Group::Salon,
    ];

    /// Its key in the admin panel's open sections (not one of the panel's own sections).
    pub fn key(self) -> &'static str {
        match self {
            Group::Drive => "sons/conduite",
            Group::Start => "sons/départ",
            Group::Boxes => "sons/boîte",
            Group::Items => "sons/objets",
            Group::Hits => "sons/coups",
            Group::World => "sons/environnements",
            Group::Voices => "sons/cris",
            Group::Screens => "sons/écrans",
            Group::Salon => "sons/salon",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::Drive => "Conduite",
            Group::Start => "Départ",
            Group::Boxes => "Boîte cadeau",
            Group::Items => "Objets",
            Group::Hits => "Coups et chocs",
            Group::World => "Environnements",
            Group::Voices => "Cris",
            Group::Screens => "Écrans",
            Group::Salon => "Salon",
        }
    }
}

/// A place of the sound design whose sound Max chooses (see the module's documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Engine,
    Squeal,
    DriftTier,
    MiniTurbo,
    Piment,
    BoostPad,
    RocketStart,
    TrickBoost,
    Slipstream,
    SpinBoost,
    Trick,
    CountdownBeep,
    CountdownGo,
    Stall,
    BoxBreak,
    RouletteTick,
    RouletteStop,
    Throw,
    Splotch,
    PeloteBounce,
    BeeBuzz,
    Explosion,
    CreamSquirt,
    Creamed,
    SuperCalin,
    Fusee,
    Deflate,
    PillowLaunch,
    PillowFall,
    Honk,
    Ghost,
    Steal,
    PinataBurst,
    BoomerangThrow,
    ItemBlocked,
    SpinOut,
    Tumble,
    PawSwing,
    PawPof,
    Grab,
    TetherBoing,
    KartBump,
    WallHit,
    Fall,
    Pelican,
    Landing,
    GliderOpen,
    Wind,
    Splash,
    Rumble,
    Cries,
    UiHover,
    UiChange,
    UiConfirm,
    UiBack,
    Footsteps,
    FootJump,
}

impl Slot {
    pub const COUNT: usize = 57;

    /// Every slot, in the panel's order (their index: [`Slot::index`]).
    pub const ALL: [Self; Self::COUNT] = {
        use Slot as S;
        [
            S::Engine,
            S::Squeal,
            S::DriftTier,
            S::MiniTurbo,
            S::Piment,
            S::BoostPad,
            S::RocketStart,
            S::TrickBoost,
            S::Slipstream,
            S::SpinBoost,
            S::Trick,
            S::CountdownBeep,
            S::CountdownGo,
            S::Stall,
            S::BoxBreak,
            S::RouletteTick,
            S::RouletteStop,
            S::Throw,
            S::Splotch,
            S::PeloteBounce,
            S::BeeBuzz,
            S::Explosion,
            S::CreamSquirt,
            S::Creamed,
            S::SuperCalin,
            S::Fusee,
            S::Deflate,
            S::PillowLaunch,
            S::PillowFall,
            S::Honk,
            S::Ghost,
            S::Steal,
            S::PinataBurst,
            S::BoomerangThrow,
            S::ItemBlocked,
            S::SpinOut,
            S::Tumble,
            S::PawSwing,
            S::PawPof,
            S::Grab,
            S::TetherBoing,
            S::KartBump,
            S::WallHit,
            S::Fall,
            S::Pelican,
            S::Landing,
            S::GliderOpen,
            S::Wind,
            S::Splash,
            S::Rumble,
            S::Cries,
            S::UiHover,
            S::UiChange,
            S::UiConfirm,
            S::UiBack,
            S::Footsteps,
            S::FootJump,
        ]
    };

    /// Its place in [`Slot::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its loop, if it is one.
    pub fn looping(self) -> Option<LoopKind> {
        match self {
            Slot::Engine => Some(LoopKind::Engine),
            Slot::Squeal => Some(LoopKind::Squeal),
            Slot::Wind => Some(LoopKind::Wind),
            Slot::Rumble => Some(LoopKind::Rumble),
            _ => None,
        }
    }

    /// The slot of a loop.
    pub fn of_loop(kind: LoopKind) -> Self {
        match kind {
            LoopKind::Engine => Slot::Engine,
            LoopKind::Squeal => Slot::Squeal,
            LoopKind::Wind => Slot::Wind,
            LoopKind::Rumble => Slot::Rumble,
        }
    }

    /// The slot of a one-shot (none for the croquettes': their blips are #14's).
    pub fn of(sound: SoundEffect) -> Option<Self> {
        use Slot as S;
        use SoundEffect as E;
        Some(match sound {
            E::CroquettePop | E::CroquettesLost => return None,
            E::RouletteTick => S::RouletteTick,
            E::RouletteStop => S::RouletteStop,
            E::BoxBreak => S::BoxBreak,
            E::DriftTier(_) => S::DriftTier,
            E::MiniTurbo(_) => S::MiniTurbo,
            E::Piment => S::Piment,
            E::BoostPad => S::BoostPad,
            E::RocketStart => S::RocketStart,
            E::TrickBoost => S::TrickBoost,
            E::Slipstream => S::Slipstream,
            E::SpinBoost => S::SpinBoost,
            E::CountdownBeep => S::CountdownBeep,
            E::CountdownGo => S::CountdownGo,
            E::Stall => S::Stall,
            E::Trick => S::Trick,
            E::Throw => S::Throw,
            E::Splotch => S::Splotch,
            E::PeloteBounce => S::PeloteBounce,
            E::BeeBuzz => S::BeeBuzz,
            E::Explosion => S::Explosion,
            E::CreamSquirt => S::CreamSquirt,
            E::Creamed => S::Creamed,
            E::SuperCalin => S::SuperCalin,
            E::Fusee => S::Fusee,
            E::Deflate => S::Deflate,
            E::PillowLaunch => S::PillowLaunch,
            E::PillowFall => S::PillowFall,
            E::Honk => S::Honk,
            E::Ghost => S::Ghost,
            E::Steal => S::Steal,
            E::PinataBurst => S::PinataBurst,
            E::BoomerangThrow => S::BoomerangThrow,
            E::ItemBlocked => S::ItemBlocked,
            E::SpinOut => S::SpinOut,
            E::Tumble => S::Tumble,
            E::PawSwing => S::PawSwing,
            E::PawPof => S::PawPof,
            E::Grab => S::Grab,
            E::TetherBoing => S::TetherBoing,
            E::KartBump => S::KartBump,
            E::WallHit => S::WallHit,
            E::Fall => S::Fall,
            E::Pelican => S::Pelican,
            E::Landing => S::Landing,
            E::GliderOpen => S::GliderOpen,
            E::Splash => S::Splash,
            E::Cry(..) => S::Cries,
            E::UiHover => S::UiHover,
            E::UiChange => S::UiChange,
            E::UiConfirm => S::UiConfirm,
            E::UiBack => S::UiBack,
            E::Footstep(_) => S::Footsteps,
            E::FootJump => S::FootJump,
        })
    }

    /// Its id: kept in the browser and in the text Max copies.
    pub fn id(self) -> &'static str {
        use Slot as S;
        match self {
            S::Engine => "engine",
            S::Squeal => "squeal",
            S::DriftTier => "drift_tier",
            S::MiniTurbo => "mini_turbo",
            S::Piment => "piment",
            S::BoostPad => "boost_pad",
            S::RocketStart => "rocket_start",
            S::TrickBoost => "trick_boost",
            S::Slipstream => "slipstream",
            S::SpinBoost => "spin_boost",
            S::Trick => "trick",
            S::CountdownBeep => "countdown_beep",
            S::CountdownGo => "countdown_go",
            S::Stall => "stall",
            S::BoxBreak => "box_break",
            S::RouletteTick => "roulette_tick",
            S::RouletteStop => "roulette_stop",
            S::Throw => "throw",
            S::Splotch => "splotch",
            S::PeloteBounce => "pelote_bounce",
            S::BeeBuzz => "bee_buzz",
            S::Explosion => "explosion",
            S::CreamSquirt => "cream_squirt",
            S::Creamed => "creamed",
            S::SuperCalin => "super_calin",
            S::Fusee => "fusee",
            S::Deflate => "deflate",
            S::PillowLaunch => "pillow_launch",
            S::PillowFall => "pillow_fall",
            S::Honk => "honk",
            S::Ghost => "ghost",
            S::Steal => "steal",
            S::PinataBurst => "pinata_burst",
            S::BoomerangThrow => "boomerang_throw",
            S::ItemBlocked => "item_blocked",
            S::SpinOut => "spin_out",
            S::Tumble => "tumble",
            S::PawSwing => "paw_swing",
            S::PawPof => "paw_pof",
            S::Grab => "grab",
            S::TetherBoing => "tether_boing",
            S::KartBump => "kart_bump",
            S::WallHit => "wall_hit",
            S::Fall => "fall",
            S::Pelican => "pelican",
            S::Landing => "landing",
            S::GliderOpen => "glider_open",
            S::Wind => "wind",
            S::Splash => "splash",
            S::Rumble => "rumble",
            S::Cries => "cries",
            S::UiHover => "ui_hover",
            S::UiChange => "ui_change",
            S::UiConfirm => "ui_confirm",
            S::UiBack => "ui_back",
            S::Footsteps => "footsteps",
            S::FootJump => "foot_jump",
        }
    }

    /// Its name in the panel.
    pub fn label(self) -> &'static str {
        use Slot as S;
        match self {
            S::Engine => "Moteur",
            S::Squeal => "Dérapage (crissement)",
            S::DriftTier => "Paliers du drift",
            S::MiniTurbo => "Mini-turbo",
            S::Piment => "Piment",
            S::BoostPad => "Plot de boost",
            S::RocketStart => "Départ fusée",
            S::TrickBoost => "Boost d'une figure",
            S::Slipstream => "Aspiration",
            S::SpinBoost => "Spin boost (anti-gravité)",
            S::Trick => "Figure",
            S::CountdownBeep => "Compte à rebours (3, 2, 1)",
            S::CountdownGo => "GO",
            S::Stall => "Moteur qui cale",
            S::BoxBreak => "Boîte qui éclate",
            S::RouletteTick => "Roulette : tic",
            S::RouletteStop => "Roulette : arrêt",
            S::Throw => "Objet lancé",
            S::Splotch => "Cac's (« splotch »)",
            S::PeloteBounce => "Pelote qui rebondit",
            S::BeeBuzz => "Abeille",
            S::Explosion => "Bombe (explosion)",
            S::CreamSquirt => "Chantilly qui gicle",
            S::Creamed => "Crème sur l'écran",
            S::SuperCalin => "Super câlin",
            S::Fusee => "Fusée",
            S::Deflate => "Dégonflé (« pfff »)",
            S::PillowLaunch => "Oreiller qui part",
            S::PillowFall => "Oreiller qui tombe",
            S::Honk => "Klaxon-canard",
            S::Ghost => "Pie voleuse (invisible)",
            S::Steal => "Pie voleuse (vol)",
            S::PinataBurst => "Piñata",
            S::BoomerangThrow => "Boomerang-os",
            S::ItemBlocked => "Objets qui se cassent",
            S::SpinOut => "Tête-à-queue",
            S::Tumble => "Culbute",
            S::PawSwing => "Patte (élan)",
            S::PawPof => "Patte (« pof »)",
            S::Grab => "Attrape",
            S::TetherBoing => "Bras élastique (« boing »)",
            S::KartBump => "Choc entre karts",
            S::WallHit => "Muret",
            S::Fall => "Chute",
            S::Pelican => "Pélican",
            S::Landing => "Atterrissage",
            S::GliderOpen => "Planeur (ouverture)",
            S::Wind => "Vent du planeur",
            S::Splash => "Eau (gerbe)",
            S::Rumble => "Hors-piste (grondement)",
            S::Cries => "Cris des animaux",
            S::UiHover => "Survol",
            S::UiChange => "Valeur qui change",
            S::UiConfirm => "Validation",
            S::UiBack => "Retour",
            S::Footsteps => "Pas",
            S::FootJump => "Saut",
        }
    }

    /// Its group in the panel.
    pub fn group(self) -> Group {
        use Slot as S;
        match self {
            S::Engine
            | S::Squeal
            | S::DriftTier
            | S::MiniTurbo
            | S::Piment
            | S::BoostPad
            | S::RocketStart
            | S::TrickBoost
            | S::Slipstream
            | S::SpinBoost
            | S::Trick => Group::Drive,
            S::CountdownBeep | S::CountdownGo | S::Stall => Group::Start,
            S::BoxBreak | S::RouletteTick | S::RouletteStop => Group::Boxes,
            S::Throw
            | S::Splotch
            | S::PeloteBounce
            | S::BeeBuzz
            | S::Explosion
            | S::CreamSquirt
            | S::Creamed
            | S::SuperCalin
            | S::Fusee
            | S::Deflate
            | S::PillowLaunch
            | S::PillowFall
            | S::Honk
            | S::Ghost
            | S::Steal
            | S::PinataBurst
            | S::BoomerangThrow
            | S::ItemBlocked => Group::Items,
            S::SpinOut
            | S::Tumble
            | S::PawSwing
            | S::PawPof
            | S::Grab
            | S::TetherBoing
            | S::KartBump
            | S::WallHit
            | S::Fall
            | S::Pelican => Group::Hits,
            S::Landing | S::GliderOpen | S::Wind | S::Splash | S::Rumble => Group::World,
            S::Cries => Group::Voices,
            S::UiHover | S::UiChange | S::UiConfirm | S::UiBack => Group::Screens,
            S::Footsteps | S::FootJump => Group::Salon,
        }
    }

    /// Its candidates, the one by default first.
    pub fn candidates(self) -> &'static [Candidate] {
        use Slot as S;
        match self {
            S::Engine => ENGINES,
            S::Squeal => SQUEALS,
            S::DriftTier => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : carillon magique qui monte",
                            &EL_DRIFT_TIER,
                        ),
                        original("synth", "Carillon synthétisé (#103)"),
                        file("glass", "Kenney : verre (Interface Sounds)", &GLASS_003),
                        file(
                            "pluck",
                            "Kenney : corde pincée (Interface Sounds)",
                            &PLUCK_002,
                        ),
                    ]
                }
            }
            S::MiniTurbo => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : « fwoosh » de turbo",
                            &EL_MINI_TURBO,
                        ),
                        original("synth", "« Fwoosh » synthétisé (#103)"),
                        synth(
                            "synth-dark",
                            "« Fwoosh » plus sourd, synthétisé",
                            Synth::Noise(SOFT_FWOOSH),
                        ),
                        file(
                            "phaser",
                            "Kenney : phaser qui monte (Digital Audio)",
                            &PHASER_UP_3,
                        ),
                    ]
                }
            }
            S::Piment => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : flamme qui s'allume", &EL_PIMENT),
                        original("synth", "Feu synthétisé (#103)"),
                        file(
                            "laser",
                            "Kenney : grand laser (Sci-Fi Sounds)",
                            &LASER_LARGE_002,
                        ),
                    ]
                }
            }
            S::BoostPad => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : décharge et élan", &EL_BOOST_PAD),
                        original("phaser-up-2", "Kenney : phaserUp2 (#103)"),
                        file("phaser-up-1", "Kenney : phaserUp1", &PHASER_UP_1),
                        file("phaser-up-4", "Kenney : phaserUp4", &PHASER_UP_4),
                    ]
                }
            }
            S::RocketStart => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : moteur qui s'emballe et part",
                            &EL_ROCKET_START,
                        ),
                        original("high-up", "Kenney : highUp (#103)"),
                        file(
                            "three-tones",
                            "Kenney : trois notes qui montent",
                            &ZAP_THREE_TONE_UP,
                        ),
                    ]
                }
            }
            S::TrickBoost => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : rebond et élan", &EL_TRICK_BOOST),
                        original("power-up-2", "Kenney : powerUp2 (#103)"),
                        file("power-up-4", "Kenney : powerUp4", &POWER_UP_4),
                    ]
                }
            }
            S::Slipstream => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : souffle d'aspiration",
                            &EL_SLIPSTREAM,
                        ),
                        original("synth", "Souffle synthétisé (#103)"),
                        file(
                            "force-field",
                            "Kenney : champ de force (Sci-Fi Sounds)",
                            &FORCE_FIELD_002,
                        ),
                    ]
                }
            }
            S::SpinBoost => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : ressort et tourbillon",
                            &EL_SPIN_BOOST,
                        ),
                        original("phaser-up-6", "Kenney : phaserUp6 (#103)"),
                        file("phaser-up-5", "Kenney : phaserUp5", &PHASER_UP_5),
                    ]
                }
            }
            S::Trick => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : salto dans l'air", &EL_TRICK),
                        original("cloth-1", "Kenney : tissu, cloth1 (#103)"),
                        file("cloth-3", "Kenney : tissu, cloth3", &CLOTH_3),
                    ]
                }
            }
            S::CountdownBeep => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : bip", &EL_COUNTDOWN_BEEP),
                        original("synth", "Bip synthétisé (#103)"),
                        file("glass", "Kenney : verre (Interface Sounds)", &GLASS_002),
                        file("bong", "Kenney : bong (Interface Sounds)", &BONG_001),
                    ]
                }
            }
            S::CountdownGo => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : bip aigu du départ",
                            &EL_COUNTDOWN_GO,
                        ),
                        original("synth", "« GO » synthétisé (#103)"),
                        file(
                            "confirmation",
                            "Kenney : confirmation_004",
                            &CONFIRMATION_004,
                        ),
                    ]
                }
            }
            S::Stall => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : petit moteur qui tousse et cale",
                            &EL_STALL,
                        ),
                        original("synth", "Toux synthétisée (#103)"),
                        file(
                            "low-down",
                            "Kenney : qui descend, grave (Digital Audio)",
                            &LOW_DOWN,
                        ),
                        file("phaser-down", "Kenney : phaserDown3", &PHASER_DOWN_3),
                    ]
                }
            }
            S::BoxBreak => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : carton qui éclate, paillettes",
                            &EL_BOX_BREAK,
                        ),
                        original("drop-4", "Kenney : drop_004 (#103)"),
                        file("drop-2", "Kenney : drop_002", &DROP_002),
                    ]
                }
            }
            S::RouletteTick => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : cliquet", &EL_ROULETTE_TICK),
                        original("tick-2", "Kenney : tick_002 (#103)"),
                        file("tick-4", "Kenney : tick_004", &TICK_004),
                    ]
                }
            }
            S::RouletteStop => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : clochette", &EL_ROULETTE_STOP),
                        original("glass-1", "Kenney : glass_001 (#103)"),
                        file("glass-2", "Kenney : glass_002", &GLASS_002),
                        file("bong", "Kenney : bong_001", &BONG_001),
                    ]
                }
            }
            S::Throw => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : lancer", &EL_THROW),
                        original("synth", "Élan synthétisé (#103)"),
                        file("knife", "Kenney : lame tirée (RPG Audio)", &DRAW_KNIFE_3),
                        file("cloth", "Kenney : tissu, cloth3", &CLOTH_3),
                    ]
                }
            }
            S::Splotch => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : « splotch » mouillé",
                            &EL_SPLOTCH,
                        ),
                        original("synth", "« Splotch » synthétisé (#137)"),
                        file("mud-11", "Rubberduck : boue, mud_11", &RUBBERDUCK_MUD_11),
                        file("mud-18", "Rubberduck : boue, mud_18", &RUBBERDUCK_MUD_18),
                        file("slime", "Kenney : slime_000 (avant #137)", &SLIME_000),
                    ]
                }
            }
            S::PeloteBounce => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : pelote qui rebondit",
                            &EL_PELOTE_BOUNCE,
                        ),
                        original("soft-medium-3", "Kenney : impactSoft_medium_003 (#103)"),
                        file(
                            "soft-medium-4",
                            "Kenney : impactSoft_medium_004",
                            &IMPACT_SOFT_MEDIUM_004,
                        ),
                        file(
                            "generic",
                            "Kenney : impactGeneric_light_002",
                            &IMPACT_GENERIC_LIGHT_002,
                        ),
                    ]
                }
            }
            S::BeeBuzz => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : abeille qui passe", &EL_BEE_BUZZ),
                        original("synth", "Bourdon synthétisé (#103)"),
                        synth(
                            "synth-low",
                            "Bourdon plus grave, synthétisé",
                            Synth::Tone(LOW_BUZZ),
                        ),
                        original("synth-high", "Bourdon synthétisé, plus aigu").speed(1.3),
                    ]
                }
            }
            S::Explosion => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : petite bombe de dessin animé",
                            &EL_EXPLOSION,
                        ),
                        original("crunch-0", "Kenney : explosionCrunch_000 (#103)"),
                        file(
                            "crunch-2",
                            "Kenney : explosionCrunch_002",
                            &EXPLOSION_CRUNCH_002,
                        ),
                        file("low", "Kenney : explosion grave", &LOW_EXPLOSION),
                    ]
                }
            }
            S::CreamSquirt => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : bombe de chantilly",
                            &EL_CREAM_SQUIRT,
                        ),
                        original("synth", "Giclée synthétisée (#103)"),
                        file("slime-09", "Rubberduck : slime_09", &RUBBERDUCK_SLIME_09),
                    ]
                }
            }
            S::Creamed => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : tarte à la crème", &EL_CREAMED),
                        original("slime-0", "Kenney : slime_000 (#103)"),
                        file("slime-15", "Rubberduck : slime_15", &RUBBERDUCK_SLIME_15),
                    ]
                }
            }
            S::SuperCalin => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : câlin magique", &EL_SUPER_CALIN),
                        original("power-up-7", "Kenney : powerUp7 (#103)"),
                        file("power-up-9", "Kenney : powerUp9", &POWER_UP_9),
                    ]
                }
            }
            S::Fusee => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : fusée de feu d'artifice",
                            &EL_FUSEE,
                        ),
                        original("synth", "Fusée synthétisée (#103)"),
                        file("phaser-up-7", "Kenney : phaserUp7", &PHASER_UP_7),
                    ]
                }
            }
            S::Deflate => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : ballon qui se dégonfle",
                            &EL_DEFLATE,
                        ),
                        original("synth", "« Pfff » synthétisé (#103)"),
                        file("phaser-down-2", "Kenney : phaserDown2", &PHASER_DOWN_2),
                        file("high-down", "Kenney : highDown", &HIGH_DOWN),
                    ]
                }
            }
            S::PillowLaunch => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : oreiller catapulté",
                            &EL_PILLOW_LAUNCH,
                        ),
                        original("synth", "Élan synthétisé (#103)"),
                        file("cloth-3", "Kenney : tissu, cloth3", &CLOTH_3),
                    ]
                }
            }
            S::PillowFall => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : oreiller qui tombe",
                            &EL_PILLOW_FALL,
                        ),
                        original("soft-heavy-2", "Kenney : impactSoft_heavy_002 (#103)"),
                        file(
                            "soft-heavy-4",
                            "Kenney : impactSoft_heavy_004",
                            &IMPACT_SOFT_HEAVY_004,
                        ),
                        file(
                            "leather",
                            "Kenney : cuir qui tombe (RPG Audio)",
                            &DROP_LEATHER,
                        ),
                    ]
                }
            }
            S::Honk => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : klaxon de vélo", &EL_HONK),
                        original("synth", "Coin-coin synthétisé (#103)"),
                        file("squeak", "bart : jouet qui couine", &BART_SQUEAK),
                        file(
                            "two-tone",
                            "Kenney : deux notes (Digital Audio)",
                            &TWO_TONE_2,
                        ),
                    ]
                }
            }
            S::Ghost => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : « pouf » qui disparaît",
                            &EL_GHOST,
                        ),
                        original("force-field-1", "Kenney : forceField_001 (#103)"),
                        file("force-field-0", "Kenney : forceField_000", &FORCE_FIELD_000),
                    ]
                }
            }
            S::Steal => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : vol en douce", &EL_STEAL),
                        original("phase-jump-3", "Kenney : phaseJump3 (#103)"),
                        file("phase-jump-1", "Kenney : phaseJump1", &PHASE_JUMP_1),
                        file("phase-jump-5", "Kenney : phaseJump5", &PHASE_JUMP_5),
                    ]
                }
            }
            S::PinataBurst => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : piñata qui éclate",
                            &EL_PINATA_BURST,
                        ),
                        original("plank-2", "Kenney : impactPlank_medium_002 (#103)"),
                        file(
                            "plank-0",
                            "Kenney : impactPlank_medium_000",
                            &IMPACT_PLANK_MEDIUM_000,
                        ),
                        file("drop-2", "Kenney : drop_002", &DROP_002),
                    ]
                }
            }
            S::BoomerangThrow => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : boomerang qui tourne",
                            &EL_BOOMERANG_THROW,
                        ),
                        original("synth", "Tourbillon synthétisé (#103)"),
                        file("knife", "Kenney : lame tirée (RPG Audio)", &DRAW_KNIFE_3),
                    ]
                }
            }
            S::ItemBlocked => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : objets qui se cassent",
                            &EL_ITEM_BLOCKED,
                        ),
                        original("glass-light-2", "Kenney : impactGlass_light_002 (#103)"),
                        file(
                            "glass-light-0",
                            "Kenney : impactGlass_light_000",
                            &IMPACT_GLASS_LIGHT_000,
                        ),
                        file(
                            "tin",
                            "Kenney : impactTin_medium_000",
                            &IMPACT_TIN_MEDIUM_000,
                        ),
                    ]
                }
            }
            S::SpinOut => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : tête-à-queue", &EL_SPIN_OUT),
                        original("synth", "« Wiii » synthétisé (#103)"),
                        file(
                            "toy-car",
                            "Rocha57 : petite voiture qui tourne",
                            &ROCHA_SPIN,
                        ),
                        file("phaser-down", "Kenney : phaserDown3", &PHASER_DOWN_3),
                    ]
                }
            }
            S::Tumble => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : culbute", &EL_TUMBLE),
                        original("soft-heavy-2", "Kenney : impactSoft_heavy_002 (#103)"),
                        file(
                            "soft-heavy-3",
                            "Kenney : impactSoft_heavy_003",
                            &IMPACT_SOFT_HEAVY_003,
                        ),
                        file(
                            "punch-heavy-2",
                            "Kenney : impactPunch_heavy_002",
                            &IMPACT_PUNCH_HEAVY_002,
                        ),
                    ]
                }
            }
            S::PawSwing => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : patte qui fouette l'air",
                            &EL_PAW_SWING,
                        ),
                        original("synth", "« Fouit » synthétisé (#103)"),
                        file("cloth-3", "Kenney : tissu, cloth3", &CLOTH_3),
                    ]
                }
            }
            S::PawPof => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : « pof » de patte", &EL_PAW_POF),
                        original("punch-medium-1", "Kenney : impactPunch_medium_001 (#103)"),
                        file(
                            "punch-heavy-1",
                            "Kenney : impactPunch_heavy_001",
                            &IMPACT_PUNCH_HEAVY_001,
                        ),
                        file(
                            "soft-heavy-1",
                            "Kenney : impactSoft_heavy_001",
                            &IMPACT_SOFT_HEAVY_001,
                        ),
                    ]
                }
            }
            S::Grab => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : attrape", &EL_GRAB),
                        original("cloth-2", "Kenney : tissu, cloth2 (#103)"),
                        file("cloth-3", "Kenney : tissu, cloth3", &CLOTH_3),
                        file(
                            "leather",
                            "Kenney : petit cuir (RPG Audio)",
                            &HANDLE_SMALL_LEATHER,
                        ),
                    ]
                }
            }
            S::TetherBoing => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : élastique qui claque",
                            &EL_TETHER_BOING,
                        ),
                        original("synth", "« Boing » synthétisé (#103)"),
                        file(
                            "spring-01",
                            "Rubberduck : ressort, spring_01",
                            &RUBBERDUCK_SPRING_01,
                        ),
                        file(
                            "spring-05",
                            "Rubberduck : ressort, spring_05",
                            &RUBBERDUCK_SPRING_05,
                        ),
                    ]
                }
            }
            S::KartBump => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : deux karts qui se cognent",
                            &EL_KART_BUMP,
                        ),
                        original("wood-medium-1", "Kenney : impactWood_medium_001 (#103)"),
                        file("crash", "qubodup : choc pour jeu de course", &QUBODUP_CRASH),
                        file(
                            "wood-light-2",
                            "Kenney : impactWood_light_002",
                            &IMPACT_WOOD_LIGHT_002,
                        ),
                        file("stk-crash", "SuperTuxKart : choc (crash)", &STK_CRASH),
                    ]
                }
            }
            S::WallHit => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : muret rembourré", &EL_WALL_HIT),
                        original("soft-heavy-0", "Kenney : impactSoft_heavy_000 (#103)"),
                        file(
                            "wood-heavy-1",
                            "Kenney : impactWood_heavy_001",
                            &IMPACT_WOOD_HEAVY_001,
                        ),
                        file(
                            "punch-heavy-0",
                            "Kenney : impactPunch_heavy_000",
                            &IMPACT_PUNCH_HEAVY_000,
                        ),
                    ]
                }
            }
            S::Fall => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : sifflet qui descend", &EL_FALL),
                        original("synth", "Sifflet synthétisé (#103)"),
                        file("phaser-down-1", "Kenney : phaserDown1", &PHASER_DOWN_1),
                        file("high-down", "Kenney : highDown", &HIGH_DOWN),
                    ]
                }
            }
            S::Pelican => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : grandes ailes qui battent",
                            &EL_PELICAN,
                        ),
                        original("synth", "Cri synthétisé (#103)"),
                        original("synth-low", "Cri synthétisé, plus grave").speed(0.8),
                        original("synth-high", "Cri synthétisé, plus aigu").speed(1.25),
                    ]
                }
            }
            S::Landing => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : atterrissage", &EL_LANDING),
                        original("soft-medium-1", "Kenney : impactSoft_medium_001 (#103)"),
                        file(
                            "soft-medium-2",
                            "Kenney : impactSoft_medium_002",
                            &IMPACT_SOFT_MEDIUM_002,
                        ),
                        file(
                            "generic-1",
                            "Kenney : impactGeneric_light_001",
                            &IMPACT_GENERIC_LIGHT_001,
                        ),
                    ]
                }
            }
            S::GliderOpen => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : toile qui s'ouvre",
                            &EL_GLIDER_OPEN,
                        ),
                        original("cloth-4", "Kenney : tissu, cloth4 (#103)"),
                        file("cloth-3", "Kenney : tissu, cloth3", &CLOTH_3),
                    ]
                }
            }
            S::Wind => {
                const {
                    &[
                        recording(
                            "elevenlabs",
                            "ElevenLabs : vent du planeur (boucle)",
                            EL_WIND,
                            0.0,
                        ),
                        looped("synth", "Vent synthétisé (#103)", LoopTimbre::Wind),
                        recording("woosh", "SketchMan3 : vent (boucle)", SKETCHMAN_WIND, 0.0),
                    ]
                }
            }
            S::Splash => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : plouf", &EL_SPLASH),
                        original("synth", "Gerbe synthétisée (#103)"),
                        file(
                            "splash-04",
                            "Rubberduck : éclaboussure, splash_04",
                            &RUBBERDUCK_SPLASH_04,
                        ),
                        file(
                            "splash-02",
                            "Rubberduck : éclaboussure, splash_02",
                            &RUBBERDUCK_SPLASH_02,
                        ),
                    ]
                }
            }
            S::Rumble => {
                const {
                    &[
                        recording(
                            "elevenlabs",
                            "ElevenLabs : roulement hors-piste (boucle)",
                            EL_RUMBLE,
                            0.0,
                        ),
                        looped("synth", "Grondement synthétisé (#103)", LoopTimbre::Rumble),
                        recording(
                            "rolling",
                            "Rubberduck : roulement (boucle)",
                            RUBBERDUCK_ROLLING,
                            0.0,
                        ),
                    ]
                }
            }
            S::Cries => {
                const {
                    &[
                        Candidate::new(
                            "elevenlabs",
                            "ElevenLabs : un cri par animal et par humeur",
                            Source::Files(&EL_CRIES),
                        ),
                        original("voices", "Voix synthétisées (#103)"),
                        original("higher", "Voix synthétisées, plus aiguës").speed(1.2),
                        original("lower", "Voix synthétisées, plus graves").speed(0.85),
                    ]
                }
            }
            S::UiHover => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : petit « tic » de bois",
                            &EL_UI_HOVER,
                        ),
                        original("rollover-2", "Kenney : rollover2 (#103)"),
                        file("rollover-4", "Kenney : rollover4", &ROLLOVER_4),
                    ]
                }
            }
            S::UiChange => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : clic", &EL_UI_CHANGE),
                        original("pluck-1", "Kenney : pluck_001 (#103)"),
                        file("pluck-2", "Kenney : pluck_002", &PLUCK_002),
                    ]
                }
            }
            S::UiConfirm => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : deux notes qui montent",
                            &EL_UI_CONFIRM,
                        ),
                        original("confirmation-1", "Kenney : confirmation_001 (#103)"),
                        file(
                            "confirmation-2",
                            "Kenney : confirmation_002",
                            &CONFIRMATION_002,
                        ),
                        file("select-2", "Kenney : select_002", &SELECT_002),
                    ]
                }
            }
            S::UiBack => {
                const {
                    &[
                        file(
                            "elevenlabs",
                            "ElevenLabs : deux notes qui descendent",
                            &EL_UI_BACK,
                        ),
                        original("back-2", "Kenney : back_002 (#103)"),
                        file("back-1", "Kenney : back_001", &BACK_001),
                        file("back-4", "Kenney : back_004", &BACK_004),
                    ]
                }
            }
            S::Footsteps => {
                const {
                    &[
                        original("grass", "Kenney : pas dans l'herbe (#103)"),
                        Candidate::new(
                            "carpet",
                            "Kenney : pas sur un tapis",
                            Source::Files(&FOOTSTEPS_CARPET),
                        ),
                        Candidate::new(
                            "snow",
                            "Kenney : pas dans la neige",
                            Source::Files(&FOOTSTEPS_SNOW),
                        ),
                    ]
                }
            }
            S::FootJump => {
                const {
                    &[
                        file("elevenlabs", "ElevenLabs : petit saut", &EL_FOOT_JUMP),
                        original("synth", "Saut synthétisé (#103)"),
                        file("phase-jump-2", "Kenney : phaseJump2", &PHASE_JUMP_2),
                        file("pep", "Kenney : pepSound2", &PEP_SOUND_2),
                    ]
                }
            }
        }
    }

    /// What « ▶ Écouter » plays for a one-shot: its sounds, each this long (s) after the one
    /// before (a loop has its own preview: none here).
    pub fn preview(self) -> &'static [(f32, SoundEffect)] {
        use Slot as S;
        use SoundEffect as E;
        match self {
            S::Engine | S::Squeal | S::Wind | S::Rumble => const { &[] },
            S::DriftTier => {
                const {
                    &[
                        (0.0, E::DriftTier(MiniTurbo::Blue)),
                        (0.45, E::DriftTier(MiniTurbo::Orange)),
                        (0.45, E::DriftTier(MiniTurbo::Purple)),
                    ]
                }
            }
            S::MiniTurbo => {
                const {
                    &[
                        (0.0, E::MiniTurbo(MiniTurbo::Blue)),
                        (0.7, E::MiniTurbo(MiniTurbo::Purple)),
                    ]
                }
            }
            S::Piment => const { &[(0.0, E::Piment)] },
            S::BoostPad => const { &[(0.0, E::BoostPad)] },
            S::RocketStart => const { &[(0.0, E::RocketStart)] },
            S::TrickBoost => const { &[(0.0, E::TrickBoost)] },
            S::Slipstream => const { &[(0.0, E::Slipstream)] },
            S::SpinBoost => const { &[(0.0, E::SpinBoost)] },
            S::Trick => const { &[(0.0, E::Trick)] },
            S::CountdownBeep => {
                const {
                    &[
                        (0.0, E::CountdownBeep),
                        (0.6, E::CountdownBeep),
                        (0.6, E::CountdownBeep),
                    ]
                }
            }
            S::CountdownGo => const { &[(0.0, E::CountdownGo)] },
            S::Stall => const { &[(0.0, E::Stall)] },
            S::BoxBreak => const { &[(0.0, E::BoxBreak)] },
            S::RouletteTick => {
                const {
                    &[
                        (0.0, E::RouletteTick),
                        (0.09, E::RouletteTick),
                        (0.09, E::RouletteTick),
                        (0.12, E::RouletteTick),
                        (0.18, E::RouletteTick),
                    ]
                }
            }
            S::RouletteStop => const { &[(0.0, E::RouletteStop)] },
            S::Throw => const { &[(0.0, E::Throw)] },
            S::Splotch => const { &[(0.0, E::Splotch)] },
            S::PeloteBounce => const { &[(0.0, E::PeloteBounce), (0.35, E::PeloteBounce)] },
            S::BeeBuzz => const { &[(0.0, E::BeeBuzz)] },
            S::Explosion => const { &[(0.0, E::Explosion)] },
            S::CreamSquirt => const { &[(0.0, E::CreamSquirt)] },
            S::Creamed => const { &[(0.0, E::Creamed)] },
            S::SuperCalin => const { &[(0.0, E::SuperCalin)] },
            S::Fusee => const { &[(0.0, E::Fusee)] },
            S::Deflate => const { &[(0.0, E::Deflate)] },
            S::PillowLaunch => const { &[(0.0, E::PillowLaunch)] },
            S::PillowFall => const { &[(0.0, E::PillowFall)] },
            S::Honk => const { &[(0.0, E::Honk)] },
            S::Ghost => const { &[(0.0, E::Ghost)] },
            S::Steal => const { &[(0.0, E::Steal)] },
            S::PinataBurst => const { &[(0.0, E::PinataBurst)] },
            S::BoomerangThrow => const { &[(0.0, E::BoomerangThrow)] },
            S::ItemBlocked => const { &[(0.0, E::ItemBlocked)] },
            S::SpinOut => const { &[(0.0, E::SpinOut)] },
            S::Tumble => const { &[(0.0, E::Tumble)] },
            S::PawSwing => const { &[(0.0, E::PawSwing)] },
            S::PawPof => const { &[(0.0, E::PawPof)] },
            S::Grab => const { &[(0.0, E::Grab)] },
            S::TetherBoing => const { &[(0.0, E::TetherBoing)] },
            S::KartBump => const { &[(0.0, E::KartBump)] },
            S::WallHit => const { &[(0.0, E::WallHit)] },
            S::Fall => const { &[(0.0, E::Fall)] },
            S::Pelican => const { &[(0.0, E::Pelican)] },
            S::Landing => const { &[(0.0, E::Landing)] },
            S::GliderOpen => const { &[(0.0, E::GliderOpen)] },
            S::Splash => const { &[(0.0, E::Splash)] },
            S::Cries => {
                const {
                    &[
                        (0.0, E::Cry(super::Animal::Cat, super::Mood::Hurt)),
                        (0.7, E::Cry(super::Animal::Chick, super::Mood::Happy)),
                        (0.7, E::Cry(super::Animal::Cow, super::Mood::Effort)),
                        (0.6, E::Cry(super::Animal::Pig, super::Mood::Win)),
                    ]
                }
            }
            S::UiHover => const { &[(0.0, E::UiHover)] },
            S::UiChange => const { &[(0.0, E::UiChange)] },
            S::UiConfirm => const { &[(0.0, E::UiConfirm)] },
            S::UiBack => const { &[(0.0, E::UiBack)] },
            S::Footsteps => {
                const {
                    &[
                        (0.0, E::Footstep(0)),
                        (0.32, E::Footstep(1)),
                        (0.32, E::Footstep(2)),
                    ]
                }
            }
            S::FootJump => const { &[(0.0, E::FootJump)] },
        }
    }
}

/// The engines (issue #135): the one by default first. Their pitch follows the revs (the playback
/// speed, times their own); a recording's level is matched to the others' when it is made into a
/// loop. Issue #247: SuperTuxKart's small engine by default (Max: « toujours aussi nulle »,
/// ElevenLabs' one), played at its own speed: from `engine_idle_pitch` to `engine_top_pitch`,
/// about SuperTuxKart's own range (0.6 at rest to 1.27 at top speed in its `kart.cpp`).
const ENGINES: &[Candidate] = &[
    recording(
        "supertuxkart",
        "SuperTuxKart : petit moteur (boucle, par défaut)",
        STK_ENGINE_SMALL,
        0.0,
    ),
    recording(
        "elevenlabs",
        "ElevenLabs : petit moteur de kart (boucle)",
        EL_ENGINE,
        0.0,
    ),
    recording(
        "supertuxkart-large",
        "SuperTuxKart : gros moteur (boucle)",
        STK_ENGINE_LARGE,
        0.0,
    ),
    recording(
        "ziph-07-soft",
        "Ziph : petit moteur de dessin animé, adouci",
        ZIPH_MOTOR_07,
        1400.0,
    )
    .speed(1.25),
    recording(
        "ziph-07",
        "Ziph : petit moteur de dessin animé",
        ZIPH_MOTOR_07,
        0.0,
    )
    .speed(1.25),
    recording("ziph-13", "Ziph : moteur rond, grave", ZIPH_MOTOR_13, 0.0),
    recording(
        "ziph-01",
        "Ziph : moteur au ralenti, qui monte haut",
        ZIPH_MOTOR_01,
        0.0,
    )
    .speed(1.8),
    recording(
        "kenney-small",
        "Kenney : petit moteur doux (Sci-Fi Sounds)",
        SPACE_ENGINE_SMALL,
        0.0,
    ),
    recording(
        "kenney-circular",
        "Kenney : moteur rond (Sci-Fi Sounds)",
        ENGINE_CIRCULAR,
        0.0,
    ),
    recording(
        "mouth",
        "EZduzziteh : moteur fait à la bouche, adouci",
        MOUTH_MOTOR,
        2500.0,
    )
    .speed(0.9),
    looped("synth-soft", "Synthé doux (#135)", LoopTimbre::SoftEngine),
    looped("synth-103", "Synthé de #103 (l'ancien)", LoopTimbre::Engine),
];

/// The drift's squeals (issue #135): the one by default first.
const SQUEALS: &[Candidate] = &[
    recording(
        "elevenlabs",
        "ElevenLabs : pneus qui crissent (boucle)",
        EL_SQUEAL,
        0.0,
    ),
    looped(
        "synth-soft",
        "Crissement doux, synthétisé (#135)",
        LoopTimbre::SoftSqueal,
    ),
    looped(
        "plush",
        "Frottement de peluche, synthétisé (#135)",
        LoopTimbre::Plush,
    ),
    recording("scrape", "AntumDeluge : raclement", SCRAPE, 0.0),
    recording("hiss", "Rubberduck : souffle", RUBBERDUCK_NOISE, 0.0).speed(0.8),
    looped(
        "synth-103",
        "Crissement de #103 (l'ancien)",
        LoopTimbre::Squeal,
    ),
    recording(
        "supertuxkart",
        "SuperTuxKart : pneus qui crissent (boucle)",
        STK_SKID,
        0.0,
    ),
];

/// The variant of a one-shot in its slot: a drift tier, a footstep, a cry (an animal's four
/// moods in a row, the animals in `Animal::ALL`'s order: `EL_CRIES`), 0 otherwise.
fn variant(sound: SoundEffect) -> usize {
    match sound {
        SoundEffect::DriftTier(tier) | SoundEffect::MiniTurbo(tier) => tier.index(),
        SoundEffect::Footstep(step) => usize::from(step),
        SoundEffect::Cry(animal, mood) => {
            let animal = Animal::ALL.iter().position(|a| *a == animal).unwrap_or(0);
            let mood = Mood::ALL.iter().position(|m| *m == mood).unwrap_or(0);
            animal * Mood::ALL.len() + mood
        }
        _ => 0,
    }
}

/// Another file or synthesis for the drift's three tiers: higher at each one (a fourth up each
/// time, like #103's chimes); the mini-turbo's longer (slower) for a higher tier.
fn variant_speed(sound: SoundEffect) -> f32 {
    match sound {
        SoundEffect::DriftTier(tier) => [1.0, 1.335, 1.782][tier.index()],
        SoundEffect::MiniTurbo(tier) => [1.15, 1.0, 0.87][tier.index()],
        _ => 1.0,
    }
}

/// The clip a one-shot plays (see [`Pick`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Picked {
    /// A file (loaded by the game, or when it was chosen).
    File(&'static str),
    /// #103's synthesis of this sound (rendered when the game starts).
    Original(SoundEffect),
    /// Another synthesis: the candidate `index` of the slot (rendered once, when chosen).
    Synth(Slot, u8),
    /// The croquettes' blips (#14), played by their own system.
    Blip,
}

/// What a one-shot plays: its clip, its length (s, at speed 1), and the speed and gain its
/// candidate adds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    pub clip: Picked,
    pub seconds: f32,
    pub speed: f32,
    pub gain: f32,
}

/// #103's own: `sound`'s clip.
fn original_pick(sound: SoundEffect, speed: f32, gain: f32) -> Pick {
    let (clip, seconds) = match sound.clip() {
        Clip::File(path, seconds) => (Picked::File(path), seconds),
        Clip::Synth(synth) => (Picked::Original(sound), synth.duration()),
        Clip::Blip(seconds) => (Picked::Blip, seconds),
    };
    Pick {
        clip,
        seconds,
        speed,
        gain,
    }
}

/// What `sound` plays with the candidate chosen for its slot.
pub fn pick(sound: SoundEffect, choices: &SoundChoices) -> Pick {
    let Some(slot) = Slot::of(sound) else {
        return original_pick(sound, 1.0, 1.0);
    };
    let index = choices.index(slot);
    let candidate = &slot.candidates()[index];
    let (clip, seconds) = match candidate.source {
        Source::File(&(path, seconds)) => (Picked::File(path), seconds),
        Source::Files(files) if !files.is_empty() => {
            let (path, seconds) = files[variant(sound) % files.len()];
            (Picked::File(path), seconds)
        }
        Source::Synth(synth) => (Picked::Synth(slot, index as u8), synth.duration()),
        _ => return original_pick(sound, candidate.speed, candidate.gain),
    };
    Pick {
        clip,
        seconds,
        speed: candidate.speed * variant_speed(sound),
        gain: candidate.gain,
    }
}

/// Which candidate plays in each slot (issue #135): Max's choices in the admin panel, kept in the
/// browser (`fluffy-racers/sound-choices`, [`SoundChoices::export`]). Only with the sound design.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct SoundChoices {
    chosen: [u8; Slot::COUNT],
}

impl Default for SoundChoices {
    fn default() -> Self {
        Self {
            chosen: [0; Slot::COUNT],
        }
    }
}

/// Where the choices are kept (the browser's local storage, a file natively).
pub const SOUND_CHOICES_KEY: &str = "fluffy-racers/sound-choices";

impl SoundChoices {
    /// The candidate chosen for `slot` (its index in [`Slot::candidates`]).
    pub fn index(&self, slot: Slot) -> usize {
        usize::from(self.chosen[slot.index()]).min(slot.candidates().len() - 1)
    }

    /// The candidate chosen for `slot`.
    pub fn candidate(&self, slot: Slot) -> &'static Candidate {
        &slot.candidates()[self.index(slot)]
    }

    /// Chooses the candidate `index` for `slot`; whether it changed anything.
    pub fn choose(&mut self, slot: Slot, index: usize) -> bool {
        if index >= slot.candidates().len() || index == self.index(slot) {
            return false;
        }
        self.chosen[slot.index()] = index as u8;
        true
    }

    /// The slots whose sound is not their default one.
    pub fn changed(&self) -> impl Iterator<Item = (Slot, &'static Candidate)> + '_ {
        Slot::ALL
            .into_iter()
            .filter(|slot| self.index(*slot) != 0)
            .map(|slot| (slot, self.candidate(slot)))
    }

    /// The text Max copies (and what is kept in the browser): one line per sound that is not the
    /// default one, `slot = "candidate"`, with their names in a comment.
    pub fn export(&self) -> String {
        let mut text = String::from(
            "// Sons choisis dans le panneau d'admin (#135) : seulement ceux qui ne sont pas le son par défaut.\n",
        );
        let mut any = false;
        for (slot, candidate) in self.changed() {
            any = true;
            text.push_str(&format!(
                "{} = \"{}\" // {} : {}\n",
                slot.id(),
                candidate.id,
                slot.label(),
                candidate.name
            ));
        }
        if !any {
            text.push_str("// (aucun : tous les sons par défaut)\n");
        }
        text
    }

    /// The choices of a text of [`SoundChoices::export`]: what is not understood is left out
    /// (returned, to be told), the rest by default.
    pub fn import(text: &str) -> (Self, Vec<String>) {
        let mut choices = Self::default();
        let mut ignored = Vec::new();
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((slot, candidate)) = line.split_once('=') else {
                ignored.push(line.to_string());
                continue;
            };
            let (slot, candidate) = (slot.trim(), candidate.trim().trim_matches('"'));
            let found = Slot::ALL
                .into_iter()
                .find(|s| s.id() == slot)
                .and_then(|s| {
                    let index = s.candidates().iter().position(|c| c.id == candidate)?;
                    Some((s, index))
                });
            match found {
                Some((slot, index)) => {
                    choices.choose(slot, index);
                }
                None => ignored.push(line.to_string()),
            }
        }
        (choices, ignored)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::audio::{Animal, Mood};

    /// Every slot is in `ALL` at its index, with its id, its name, its group and at least two
    /// candidates (the sound of #103 and another, issue #135), each id once; every one-shot of the
    /// sound design has its slot (but the croquettes'), every loop too; the loops have loop
    /// sources, the one-shots one-shot sources.
    #[test]
    fn every_slot_has_its_candidates() {
        let mut ids = HashSet::new();
        for (i, slot) in Slot::ALL.iter().enumerate() {
            assert_eq!(slot.index(), i, "{slot:?}");
            assert!(ids.insert(slot.id()), "{slot:?}");
            assert!(!slot.label().is_empty());
            let candidates = slot.candidates();
            assert!(candidates.len() >= 2, "{slot:?}");
            let mut names = HashSet::new();
            for candidate in candidates {
                assert!(names.insert(candidate.id), "{slot:?} {}", candidate.id);
                assert!(candidate.speed > 0.2 && candidate.speed < 3.0);
                assert!(candidate.gain > 0.0 && candidate.gain <= 1.5);
                let looping =
                    matches!(candidate.source, Source::Loop(_) | Source::Recording { .. });
                assert_eq!(
                    looping,
                    slot.looping().is_some(),
                    "{slot:?} {}",
                    candidate.id
                );
                assert!(
                    !candidate.id.contains(['"', '=', '/', ' ']),
                    "{}",
                    candidate.id
                );
            }
            if slot.looping().is_none() {
                assert!(!slot.preview().is_empty(), "{slot:?}");
                for (_, sound) in slot.preview() {
                    assert_eq!(Slot::of(*sound), Some(*slot));
                }
            }
        }
        assert!(Slot::ALL.iter().any(|s| s.group() == Group::Salon));
        let mut keys = HashSet::new();
        for group in Group::ALL {
            assert!(Slot::ALL.iter().any(|s| s.group() == group), "{group:?}");
            assert!(keys.insert(group.key()) && group.key().starts_with("sons/"));
        }
        let mut sounds: Vec<SoundEffect> = SoundEffect::synthesized()
            .chain(SoundEffect::from_files())
            .collect();
        sounds.extend([SoundEffect::CroquettePop, SoundEffect::CroquettesLost]);
        for sound in sounds {
            let croquette = matches!(
                sound,
                SoundEffect::CroquettePop | SoundEffect::CroquettesLost
            );
            assert_eq!(Slot::of(sound).is_none(), croquette, "{sound:?}");
        }
        for kind in LoopKind::ALL {
            assert_eq!(Slot::of_loop(kind).looping(), Some(kind));
        }
        // At least four engines and four drifts (issue #135), files among them.
        for (slot, files) in [(Slot::Engine, 5), (Slot::Squeal, 2)] {
            let candidates = slot.candidates();
            assert!(candidates.len() >= 4, "{slot:?}");
            let recorded = candidates
                .iter()
                .filter(|c| c.recording().is_some())
                .count();
            assert!(recorded >= files, "{slot:?}: {recorded}");
        }
    }

    /// Issue #235 (was #135's): by default every sound is ElevenLabs' (but the footsteps, still
    /// #103's); the sound of #103 is the next candidate, still chosen in the panel; another
    /// candidate changes the clip, keeps each variant's own (the footsteps, each animal's cry),
    /// and the drift's chimes still climb.
    #[test]
    fn a_choice_changes_what_a_sound_plays() {
        let mut choices = SoundChoices::default();
        let mut previous = SoundChoices::default();
        for sound in SoundEffect::from_files().chain(SoundEffect::synthesized()) {
            let slot = Slot::of(sound).unwrap();
            let picked = pick(sound, &choices);
            if slot == Slot::Footsteps {
                assert_eq!(picked, original_pick(sound, 1.0, 1.0), "{sound:?}");
                continue;
            }
            let Picked::File(path) = picked.clip else {
                panic!("{sound:?}: {picked:?}");
            };
            assert!(path.starts_with("audio/elevenlabs/"), "{sound:?}: {path}");
            assert_eq!(choices.candidate(slot).id, "elevenlabs");
            // #103's sound is still there, right after it.
            previous.choose(slot, 1);
            if slot.candidates()[1].source == Source::Original {
                assert_eq!(
                    pick(sound, &previous),
                    original_pick(sound, 1.0, 1.0),
                    "{sound:?}"
                );
            }
        }
        // Each animal its own cry for each mood: 36 files, all different.
        let mut cries = HashSet::new();
        for animal in Animal::ALL {
            for mood in Mood::ALL {
                let Picked::File(path) = pick(SoundEffect::Cry(animal, mood), &choices).clip else {
                    panic!("{animal:?} {mood:?}");
                };
                let name = format!("{animal:?}_{mood:?}").to_lowercase();
                assert!(path.ends_with(&format!("cry_{name}.ogg")), "{path}");
                assert!(cries.insert(path));
            }
        }
        assert_eq!(cries.len(), 36);
        // Another file.
        assert!(choices.choose(Slot::KartBump, 2));
        assert!(!choices.choose(Slot::KartBump, 2), "already");
        assert!(!choices.choose(Slot::KartBump, 99), "no such candidate");
        let bump = pick(SoundEffect::KartBump, &choices);
        assert_eq!(bump.clip, Picked::File("audio/qubodup/crash.ogg"));
        assert_eq!(bump.seconds, QUBODUP_CRASH.1);
        // A footstep each.
        choices.choose(Slot::Footsteps, 2);
        let steps: Vec<Picked> = (0..3)
            .map(|step| pick(SoundEffect::Footstep(step), &choices).clip)
            .collect();
        assert_eq!(
            steps,
            FOOTSTEPS_SNOW
                .iter()
                .map(|(path, _)| Picked::File(path))
                .collect::<Vec<_>>()
        );
        // The chimes climb with a file: ElevenLabs' (by default) and another.
        for index in [0, 2] {
            choices.choose(Slot::DriftTier, index);
            let speeds: Vec<f32> = MiniTurbo::ALL
                .iter()
                .map(|tier| pick(SoundEffect::DriftTier(*tier), &choices).speed)
                .collect();
            assert!(speeds[0] < speeds[1] && speeds[1] < speeds[2], "{speeds:?}");
        }
        // Another synthesis, for every tier.
        choices.choose(Slot::MiniTurbo, 2);
        let fwoosh = pick(SoundEffect::MiniTurbo(MiniTurbo::Orange), &choices);
        assert_eq!(fwoosh.clip, Picked::Synth(Slot::MiniTurbo, 2));
        assert!((fwoosh.seconds - SOFT_FWOOSH.duration).abs() < 1e-6);
        // The same voices, higher: the cries keep each animal's.
        choices.choose(Slot::Cries, 2);
        let cry = pick(SoundEffect::Cry(Animal::Cow, Mood::Hurt), &choices);
        assert_eq!(
            cry.clip,
            Picked::Original(SoundEffect::Cry(Animal::Cow, Mood::Hurt))
        );
        assert!(cry.speed > 1.1);
        // The croquettes keep their blips.
        assert_eq!(pick(SoundEffect::CroquettePop, &choices).clip, Picked::Blip);
        // The loops by default: recordings made into seamless loops by the game, ElevenLabs'
        // but the engine: SuperTuxKart's since issue #247 (ElevenLabs' still chosen, next).
        let defaults = SoundChoices::default();
        for slot in [Slot::Engine, Slot::Squeal, Slot::Wind, Slot::Rumble] {
            let (path, _) = defaults.candidate(slot).recording().unwrap();
            let pack = if slot == Slot::Engine {
                "audio/supertuxkart/"
            } else {
                "audio/elevenlabs/"
            };
            assert!(path.starts_with(pack), "{slot:?}: {path}");
        }
        assert_eq!(defaults.candidate(Slot::Engine).id, "supertuxkart");
        assert_eq!(Slot::Engine.candidates()[1].id, "elevenlabs");
    }

    /// Issue #135: the choices are kept as a text Max can copy: only what changed, by ids, which
    /// loads back; what is not understood is left out and told.
    #[test]
    fn the_choices_go_to_a_text_and_back() {
        let none = SoundChoices::default().export();
        assert!(none.contains("aucun"), "{none}");
        assert_eq!(
            SoundChoices::import(&none),
            (SoundChoices::default(), vec![])
        );
        let mut choices = SoundChoices::default();
        choices.choose(Slot::Engine, 5);
        choices.choose(Slot::Squeal, 3);
        choices.choose(Slot::Honk, 1);
        let text = choices.export();
        assert_eq!(text.lines().count(), 4, "{text}");
        assert!(text.contains("engine = \"ziph-13\""), "{text}");
        assert!(
            text.contains("Moteur : Ziph : moteur rond, grave"),
            "{text}"
        );
        let (back, ignored) = SoundChoices::import(&text);
        assert_eq!(back, choices);
        assert!(ignored.is_empty());
        let (partial, ignored) =
            SoundChoices::import("honk = \"squeak\"\nnope = \"x\"\nengine = \"nope\"\ngarbage");
        assert_eq!(partial.index(Slot::Honk), 2);
        assert_eq!(partial.index(Slot::Engine), 0);
        assert_eq!(ignored.len(), 3, "{ignored:?}");
    }
}
