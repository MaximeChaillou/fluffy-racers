//! Issue #103: what each sound is (pure). Every [`SoundEffect`] is either a file of a CC0 pack
//! of Kenney (`assets/audio/<pack>/`, credited in `assets/CREDITS.md`: [`FILES`]) or a sound
//! synthesized when the game starts (`dsp`: [`synth_of`]), with its family (its volume in the
//! admin panel, [`Family`]) and its length (how long its voice is taken, [`SoundEffect::length`]).

use crate::kart::MiniTurbo;

use super::dsp::{Noise, Synth, Tone, Voice, Wave};

/// A sound to play: an entity with this component is a sound being played (the mixer takes it,
/// gives it its clip and lets it go when it ends). The tests look for these entities: they say
/// which sound a game event asked for, without playing it.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SoundEffect {
    // The croquettes (GD § 7 and 14, issue #14: synthesized blips, `audio::Sounds`).
    CroquettePop,
    CroquettesLost,
    // The roulette (GD § 8, issue #102): a tick for each item that passes, a « ding » at the
    // stop (Kenney's Interface Sounds since #103).
    RouletteTick,
    RouletteStop,
    /// A gift box bursts (GD § 8).
    BoxBreak,
    // The drift (GD § 3): a chime each time the charge reaches a tier, higher at each one...
    DriftTier(MiniTurbo),
    /// ...and the « fwoosh » of its mini-turbo at the release.
    MiniTurbo(MiniTurbo),
    // The boosts (GD § 2): the piment (also the piment doré's presses), a boost pad, the rocket
    // start (GD § 4), a trick's boost on landing (GD § 5), the slipstream's (GD § 6), the spin
    // boost of the anti-gravity (GD § 10).
    Piment,
    BoostPad,
    RocketStart,
    TrickBoost,
    Slipstream,
    SpinBoost,
    // The start (GD § 1 and 4): the countdown's beeps and its « GO », the engine that stalls.
    // The last lap's and the finish's jingles are the music's since #104 (`audio::music`).
    CountdownBeep,
    CountdownGo,
    Stall,
    /// A trick off a ramp (GD § 5): the flip.
    Trick,
    // The items (GD § 8): thrown, dropped, what they do.
    /// A pelote, a bombe or anything lobbed: the throw.
    Throw,
    /// The Cac's (issue #137): its soft « splotch » as it lands, and as a kart slides on it.
    Splotch,
    PeloteBounce,
    BeeBuzz,
    Explosion,
    CreamSquirt,
    /// The player's screen gets cream.
    Creamed,
    SuperCalin,
    Fusee,
    Deflate,
    PillowLaunch,
    PillowFall,
    Honk,
    /// The pie voleuse: invisible...
    Ghost,
    /// ...and what it steals.
    Steal,
    PinataBurst,
    BoomerangThrow,
    /// Two items broke on each other (a shield).
    ItemBlocked,
    // Hits and shocks (GD § 8, 10 and 15).
    SpinOut,
    Tumble,
    /// A paw swings (into a kart or the air)...
    PawSwing,
    /// ...and lands: the « pof ».
    PawPof,
    Grab,
    /// A tether lets go: the « boing ».
    TetherBoing,
    KartBump,
    WallHit,
    /// Falling off the track: the slide whistle...
    Fall,
    /// ...and the pelican that comes.
    Pelican,
    // The environments (GD § 10).
    Landing,
    GliderOpen,
    Splash,
    /// An animal's cry (GD § 14): each one its voice, each mood its contour.
    Cry(Animal, Mood),
    // The screens (docs/UI_CHARTE.md): the focus moves, a value changes, a choice, back.
    UiHover,
    UiChange,
    UiConfirm,
    UiBack,
    // On foot in the lobby (issue #101): a step on the lawn, a jump.
    Footstep(u8),
    FootJump,
}

/// The nine animals (GD § 9, issue #69), and their voices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Animal {
    Chick,
    Penguin,
    Parrot,
    #[default]
    Cat,
    Bunny,
    Fox,
    Pig,
    Cow,
    Elephant,
}

impl Animal {
    pub const ALL: [Self; 9] = [
        Animal::Chick,
        Animal::Penguin,
        Animal::Parrot,
        Animal::Cat,
        Animal::Bunny,
        Animal::Fox,
        Animal::Pig,
        Animal::Cow,
        Animal::Elephant,
    ];

    /// The animal of a character's model (`assets/data/characters.ron`: « cube-pets/animal-cat »);
    /// the cat for anything else (a driver in primitives).
    pub fn of_model(model: Option<&str>) -> Self {
        let name = model.and_then(|m| m.rsplit('/').next()).unwrap_or("");
        match name.trim_start_matches("animal-") {
            "chick" => Animal::Chick,
            "penguin" => Animal::Penguin,
            "parrot" => Animal::Parrot,
            "bunny" => Animal::Bunny,
            "fox" => Animal::Fox,
            "pig" => Animal::Pig,
            "cow" => Animal::Cow,
            "elephant" => Animal::Elephant,
            _ => Animal::Cat,
        }
    }
}

/// Why an animal cries (GD § 14).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mood {
    /// Hit (an item, a paw, a fall): a falling « ouin ».
    Hurt,
    /// A trick: a rising « youpi ».
    Happy,
    /// It strikes with its paw: a short « hop ».
    Effort,
    /// On the podium: a long, wobbling cheer.
    Win,
}

impl Mood {
    pub const ALL: [Self; 4] = [Mood::Hurt, Mood::Happy, Mood::Effort, Mood::Win];
}

/// The families of sounds, each with its volume in the admin panel (`SoundTuning`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// The drift and the boosts.
    Drive,
    Items,
    /// Hits, shocks, falls.
    Hits,
    /// The countdown, the croquettes and the roulette (the HUD's).
    Race,
    /// The animals' cries.
    Voices,
    /// The screens.
    Screens,
    /// The environments, and the lobby on foot.
    World,
}

/// Where a sound comes from: a file (its path under `assets/`, and its length in seconds,
/// measured: a test decodes each file and checks it), or a synthesis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Clip {
    File(&'static str, f32),
    Synth(Synth),
    /// The croquettes' blips (`audio::Sounds`): played by their own system.
    Blip(f32),
}

/// Every file the game plays (issue #103), with its length (s): all of them, and only them, are
/// in `assets/audio/` (the test checks it, and their lines in `assets/CREDITS.md`).
pub const FILES: [(&str, f32); 30] = [
    ("audio/impact-sounds/impactPunch_medium_001.ogg", 0.405),
    ("audio/impact-sounds/impactWood_medium_001.ogg", 0.341),
    ("audio/impact-sounds/impactSoft_heavy_000.ogg", 0.527),
    ("audio/impact-sounds/impactSoft_medium_001.ogg", 0.202),
    ("audio/impact-sounds/impactSoft_heavy_002.ogg", 0.574),
    ("audio/impact-sounds/impactPlank_medium_002.ogg", 0.779),
    ("audio/impact-sounds/impactGlass_light_002.ogg", 0.211),
    ("audio/impact-sounds/impactSoft_medium_003.ogg", 0.156),
    ("audio/impact-sounds/footstep_grass_000.ogg", 0.783),
    ("audio/impact-sounds/footstep_grass_001.ogg", 0.687),
    ("audio/impact-sounds/footstep_grass_002.ogg", 0.707),
    ("audio/interface-sounds/back_002.ogg", 0.092),
    ("audio/interface-sounds/confirmation_001.ogg", 0.290),
    ("audio/interface-sounds/pluck_001.ogg", 0.112),
    ("audio/interface-sounds/tick_002.ogg", 0.024),
    ("audio/interface-sounds/glass_001.ogg", 0.292),
    ("audio/interface-sounds/drop_004.ogg", 0.295),
    ("audio/ui-audio/rollover2.ogg", 0.063),
    ("audio/digital-audio/powerUp7.ogg", 0.527),
    ("audio/digital-audio/phaserUp2.ogg", 0.429),
    ("audio/digital-audio/phaserUp6.ogg", 0.344),
    ("audio/digital-audio/powerUp2.ogg", 0.490),
    ("audio/digital-audio/highUp.ogg", 0.553),
    ("audio/digital-audio/phaseJump3.ogg", 0.460),
    ("audio/sci-fi-sounds/slime_000.ogg", 0.500),
    ("audio/sci-fi-sounds/explosionCrunch_000.ogg", 0.778),
    ("audio/sci-fi-sounds/forceField_001.ogg", 0.954),
    ("audio/rpg-audio/cloth2.ogg", 0.416),
    ("audio/rpg-audio/cloth4.ogg", 0.385),
    ("audio/rpg-audio/cloth1.ogg", 0.661),
];

/// The file of `FILES` named `path`, as a clip.
const fn file(index: usize) -> Clip {
    Clip::File(FILES[index].0, FILES[index].1)
}

/// The cartoon spring of a tether that lets go.
const BOING: Tone = Tone {
    from_hz: 330.0,
    to_hz: 300.0,
    duration: 0.55,
    wave: Wave::Round,
    step: None,
    vibrato: 5.0,
    vibrato_hz: 13.0,
    vibrato_fade: 4.5,
    decay: 3.5,
};

/// The slide whistle of a fall: down from high, in a second.
const FALL: Tone = Tone {
    from_hz: 1500.0,
    to_hz: 260.0,
    duration: 1.0,
    wave: Wave::Hollow,
    step: None,
    vibrato: 0.4,
    vibrato_hz: 6.0,
    vibrato_fade: 0.0,
    decay: 1.6,
};

/// The Cac's « splotch » (issue #137): soft and wet, synthesized (no pack has one that is
/// ours): a burst of noise through a band falling from the mids to the lows, at once loud then
/// dying fast (a hit, not a whoosh), sprinkled with wet pops, over the « bloup » of a falling
/// tone. Lower and shorter than the splash of the water.
pub const SPLOTCH: Noise = Noise {
    from_hz: 1300.0,
    to_hz: 170.0,
    q: 1.6,
    duration: 0.3,
    swell: 0.02,
    crackle: 60.0,
    tremolo_hz: 0.0,
    tone: 0.3,
};

/// Spinning out: a wobbling « wheee » going down.
const SPIN: Tone = Tone {
    from_hz: 900.0,
    to_hz: 420.0,
    duration: 0.8,
    wave: Wave::Round,
    step: None,
    vibrato: 2.5,
    vibrato_hz: 9.0,
    vibrato_fade: 0.0,
    decay: 2.0,
};

/// The bee: a kazoo buzz.
const BUZZ: Tone = Tone {
    from_hz: 210.0,
    to_hz: 250.0,
    duration: 0.9,
    wave: Wave::Buzz,
    step: None,
    vibrato: 1.2,
    vibrato_hz: 18.0,
    vibrato_fade: 0.0,
    decay: 1.5,
};

/// The engine coughing and stopping (GD § 4: the stalled start).
const STALL: Noise = Noise {
    from_hz: 180.0,
    to_hz: 90.0,
    q: 2.5,
    duration: 0.9,
    swell: 0.05,
    crackle: 0.0,
    tremolo_hz: 11.0,
    tone: 0.4,
};

/// The countdown: a beep for 3, 2 and 1...
const BEEP: Tone = Tone {
    decay: 1.2,
    ..Tone::sweep(660.0, 660.0, 0.22, Wave::Hollow)
};

/// ...and a higher, longer one for GO.
const GO: Tone = Tone {
    decay: 1.0,
    ..Tone::sweep(1320.0, 1320.0, 0.6, Wave::Hollow)
};

/// A chime at each tier of the drift, higher each time (two notes, the second a fifth up).
fn tier_chime(tier: MiniTurbo) -> Tone {
    let base = [880.0, 1175.0, 1568.0][tier.index()];
    Tone {
        step: Some((0.06, 1.5)),
        decay: 3.0,
        ..Tone::sweep(base, base, 0.22, Wave::Round)
    }
}

/// The mini-turbo's « fwoosh »: longer and brighter for a higher tier.
fn fwoosh(tier: MiniTurbo) -> Noise {
    let i = tier.index() as f32;
    Noise {
        tone: 0.08,
        ..Noise::whoosh(
            500.0 + 250.0 * i,
            3500.0 + 1500.0 * i,
            0.35 + 0.15 * i,
            0.15,
        )
    }
}

/// The voice of `animal`: its pitch (Hz), its formants (Hz), its breath, its grunt (Hz).
fn timbre(animal: Animal) -> (f32, [f32; 2], f32, f32) {
    match animal {
        Animal::Chick => (1900.0, [2600.0, 4200.0], 0.05, 0.0),
        Animal::Penguin => (430.0, [750.0, 1250.0], 0.15, 0.0),
        Animal::Parrot => (950.0, [1300.0, 2700.0], 0.55, 0.0),
        Animal::Cat => (560.0, [650.0, 1700.0], 0.1, 0.0),
        Animal::Bunny => (1450.0, [1900.0, 3200.0], 0.1, 0.0),
        Animal::Fox => (760.0, [950.0, 2100.0], 0.2, 0.0),
        Animal::Pig => (190.0, [520.0, 1350.0], 0.3, 26.0),
        Animal::Cow => (125.0, [330.0, 850.0], 0.08, 0.0),
        Animal::Elephant => (360.0, [820.0, 1900.0], 0.25, 0.0),
    }
}

/// An animal's cry for a mood (GD § 14: « drôles et mous »): the animal gives the voice, the
/// mood the contour and the length.
pub fn cry(animal: Animal, mood: Mood) -> Voice {
    let (pitch, formants, breath, grunt_hz) = timbre(animal);
    // The cow and the elephant take their time.
    let long = match animal {
        Animal::Cow => 1.6,
        Animal::Elephant => 1.3,
        Animal::Chick | Animal::Bunny => 0.7,
        _ => 1.0,
    };
    let (contour, peak_at, duration, vibrato, vowel) = match mood {
        Mood::Hurt => ([1.15, 1.25, 0.72], 0.2, 0.42, 0.6, 0.8),
        Mood::Happy => ([0.9, 1.4, 1.25], 0.6, 0.38, 0.3, 1.25),
        Mood::Effort => ([1.0, 1.1, 0.95], 0.4, 0.16, 0.0, 1.1),
        Mood::Win => ([0.95, 1.35, 1.1], 0.45, 0.7, 1.4, 1.2),
    };
    Voice {
        pitch: contour.map(|c| pitch * c),
        peak_at,
        duration: duration * long,
        formants_from: formants,
        formants_to: formants.map(|f| f * vowel),
        breath,
        vibrato,
        grunt_hz,
    }
}

/// The klaxon-canard: a duck's quack, nasal and short.
const QUACK: Voice = Voice {
    pitch: [520.0, 560.0, 430.0],
    peak_at: 0.25,
    duration: 0.32,
    formants_from: [1050.0, 1650.0],
    formants_to: [850.0, 1400.0],
    breath: 0.25,
    vibrato: 0.0,
    grunt_hz: 0.0,
};

/// The pelican: a low, breathy squawk.
const PELICAN: Voice = Voice {
    pitch: [300.0, 360.0, 240.0],
    peak_at: 0.3,
    duration: 0.5,
    formants_from: [700.0, 1500.0],
    formants_to: [600.0, 1200.0],
    breath: 0.6,
    vibrato: 0.8,
    grunt_hz: 0.0,
};

impl SoundEffect {
    /// Its clip: a file, a synthesis, or a blip of the croquettes.
    pub fn clip(self) -> Clip {
        use SoundEffect as S;
        match self {
            S::CroquettePop => Clip::Blip(super::CROQUETTE_POP.duration),
            S::CroquettesLost => Clip::Blip(super::CROQUETTE_LOST.duration),
            S::PawPof => file(0),
            S::KartBump => file(1),
            S::WallHit => file(2),
            S::Landing => file(3),
            S::Tumble | S::PillowFall => file(4),
            S::PinataBurst => file(5),
            S::ItemBlocked => file(6),
            S::PeloteBounce => file(7),
            S::Footstep(step) => file(8 + usize::from(step % 3)),
            S::UiBack => file(11),
            S::UiConfirm => file(12),
            S::UiChange => file(13),
            S::RouletteTick => file(14),
            S::RouletteStop => file(15),
            S::BoxBreak => file(16),
            S::UiHover => file(17),
            S::SuperCalin => file(18),
            S::BoostPad => file(19),
            S::SpinBoost => file(20),
            S::TrickBoost => file(21),
            S::RocketStart => file(22),
            S::Steal => file(23),
            S::Creamed => file(24),
            S::Explosion => file(25),
            S::Ghost => file(26),
            S::Grab => file(27),
            S::GliderOpen => file(28),
            S::Trick => file(29),
            S::DriftTier(tier) => Clip::Synth(Synth::Tone(tier_chime(tier))),
            S::MiniTurbo(tier) => Clip::Synth(Synth::Noise(fwoosh(tier))),
            S::Piment => Clip::Synth(Synth::Noise(Noise {
                crackle: 90.0,
                ..Noise::whoosh(250.0, 2400.0, 0.6, 0.2)
            })),
            S::Slipstream => Clip::Synth(Synth::Noise(Noise {
                q: 0.9,
                tone: 0.05,
                ..Noise::whoosh(700.0, 1600.0, 0.8, 0.45)
            })),
            S::CountdownBeep => Clip::Synth(Synth::Tone(BEEP)),
            S::CountdownGo => Clip::Synth(Synth::Tone(GO)),
            S::Stall => Clip::Synth(Synth::Noise(STALL)),
            S::Throw => Clip::Synth(Synth::Noise(Noise::whoosh(600.0, 1800.0, 0.28, 0.35))),
            S::Splotch => Clip::Synth(Synth::Noise(SPLOTCH)),
            S::BeeBuzz => Clip::Synth(Synth::Tone(BUZZ)),
            S::CreamSquirt => Clip::Synth(Synth::Noise(Noise {
                q: 0.8,
                ..Noise::whoosh(3000.0, 1200.0, 0.35, 0.1)
            })),
            S::Fusee => Clip::Synth(Synth::Noise(Noise {
                crackle: 140.0,
                ..Noise::whoosh(150.0, 1800.0, 1.2, 0.3)
            })),
            S::Deflate => Clip::Synth(Synth::Noise(Noise {
                q: 3.0,
                tone: 0.25,
                ..Noise::whoosh(1800.0, 250.0, 0.9, 0.1)
            })),
            S::PillowLaunch => Clip::Synth(Synth::Noise(Noise {
                q: 0.9,
                ..Noise::whoosh(300.0, 900.0, 0.7, 0.5)
            })),
            S::Honk => Clip::Synth(Synth::Voice(QUACK)),
            S::BoomerangThrow => Clip::Synth(Synth::Noise(Noise {
                tremolo_hz: 9.0,
                ..Noise::whoosh(500.0, 1200.0, 0.7, 0.3)
            })),
            S::SpinOut => Clip::Synth(Synth::Tone(SPIN)),
            S::PawSwing => Clip::Synth(Synth::Noise(Noise::whoosh(900.0, 2600.0, 0.18, 0.4))),
            S::TetherBoing => Clip::Synth(Synth::Tone(BOING)),
            S::Fall => Clip::Synth(Synth::Tone(FALL)),
            S::Pelican => Clip::Synth(Synth::Voice(PELICAN)),
            S::Splash => Clip::Synth(Synth::Noise(Noise {
                q: 0.7,
                crackle: 60.0,
                ..Noise::whoosh(1400.0, 300.0, 0.55, 0.05)
            })),
            S::Cry(animal, mood) => Clip::Synth(Synth::Voice(cry(animal, mood))),
            S::FootJump => Clip::Synth(Synth::Tone(Tone {
                decay: 3.0,
                ..Tone::sweep(380.0, 760.0, 0.14, Wave::Round)
            })),
        }
    }

    /// Its length (s), at playback speed 1: how long it holds a voice.
    pub fn length(self) -> f32 {
        match self.clip() {
            Clip::File(_, seconds) | Clip::Blip(seconds) => seconds,
            Clip::Synth(synth) => synth.duration(),
        }
    }

    /// Its family (its volume).
    pub fn family(self) -> Family {
        use SoundEffect as S;
        match self {
            S::DriftTier(_)
            | S::MiniTurbo(_)
            | S::Piment
            | S::BoostPad
            | S::RocketStart
            | S::TrickBoost
            | S::Slipstream
            | S::SpinBoost
            | S::Trick => Family::Drive,
            S::BoxBreak
            | S::Throw
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
            | S::ItemBlocked => Family::Items,
            S::SpinOut
            | S::Tumble
            | S::PawSwing
            | S::PawPof
            | S::Grab
            | S::TetherBoing
            | S::KartBump
            | S::WallHit
            | S::Fall
            | S::Pelican => Family::Hits,
            S::CroquettePop
            | S::CroquettesLost
            | S::RouletteTick
            | S::RouletteStop
            | S::CountdownBeep
            | S::CountdownGo
            | S::Stall => Family::Race,
            S::Cry(..) => Family::Voices,
            S::UiHover | S::UiChange | S::UiConfirm | S::UiBack => Family::Screens,
            S::Landing | S::GliderOpen | S::Splash | S::Footstep(_) | S::FootJump => Family::World,
        }
    }

    /// Every sound a synthesis gives (rendered once when the game starts).
    pub fn synthesized() -> impl Iterator<Item = SoundEffect> {
        let tiers = MiniTurbo::ALL
            .into_iter()
            .flat_map(|t| [SoundEffect::DriftTier(t), SoundEffect::MiniTurbo(t)]);
        let cries = Animal::ALL
            .into_iter()
            .flat_map(|a| Mood::ALL.into_iter().map(move |m| SoundEffect::Cry(a, m)));
        use SoundEffect as S;
        [
            S::Piment,
            S::Slipstream,
            S::CountdownBeep,
            S::CountdownGo,
            S::Stall,
            S::Throw,
            S::Splotch,
            S::BeeBuzz,
            S::CreamSquirt,
            S::Fusee,
            S::Deflate,
            S::PillowLaunch,
            S::Honk,
            S::BoomerangThrow,
            S::SpinOut,
            S::PawSwing,
            S::TetherBoing,
            S::Fall,
            S::Pelican,
            S::Splash,
            S::FootJump,
        ]
        .into_iter()
        .chain(tiers)
        .chain(cries)
    }

    /// Every sound played from a file (one per file, a footstep each).
    pub fn from_files() -> impl Iterator<Item = SoundEffect> {
        use SoundEffect as S;
        [
            S::PawPof,
            S::KartBump,
            S::WallHit,
            S::Landing,
            S::Tumble,
            S::PinataBurst,
            S::ItemBlocked,
            S::PeloteBounce,
            S::Footstep(0),
            S::Footstep(1),
            S::Footstep(2),
            S::UiBack,
            S::UiConfirm,
            S::UiChange,
            S::RouletteTick,
            S::RouletteStop,
            S::BoxBreak,
            S::UiHover,
            S::SuperCalin,
            S::BoostPad,
            S::SpinBoost,
            S::TrickBoost,
            S::RocketStart,
            S::Steal,
            S::Creamed,
            S::Explosion,
            S::Ghost,
            S::Grab,
            S::GliderOpen,
            S::Trick,
        ]
        .into_iter()
    }
}

/// How loud a sound is mixed, before the volumes (0 to 1): the countdown over the rest, the
/// footsteps and the hover under it.
pub fn base_volume(sound: SoundEffect) -> f32 {
    use SoundEffect as S;
    match sound {
        S::UiHover | S::Footstep(_) | S::RouletteTick => 0.45,
        S::PeloteBounce | S::Landing | S::PawSwing => 0.6,
        S::CountdownBeep | S::CountdownGo => 0.9,
        _ => 0.75,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sound has a clip, a length and a family; the files are each played by one sound,
    /// and each synthesized sound is listed (rendered at the start).
    #[test]
    fn every_sound_has_its_clip_and_every_file_its_sound() {
        let mut files: Vec<&str> = SoundEffect::from_files()
            .map(|s| match s.clip() {
                Clip::File(path, _) => path,
                other => panic!("{s:?}: {other:?}"),
            })
            .collect();
        files.sort();
        files.dedup();
        let mut all: Vec<&str> = FILES.iter().map(|(path, _)| *path).collect();
        all.sort();
        assert_eq!(files, all);
        let synthesized: Vec<SoundEffect> = SoundEffect::synthesized().collect();
        assert_eq!(synthesized.len(), 21 + 6 + 36);
        for sound in &synthesized {
            assert!(matches!(sound.clip(), Clip::Synth(_)), "{sound:?}");
            assert!(sound.length() > 0.05 && sound.length() < 2.0, "{sound:?}");
        }
        for sound in SoundEffect::from_files() {
            assert!(sound.length() > 0.02 && sound.length() < 1.0, "{sound:?}");
            assert!(base_volume(sound) > 0.0 && base_volume(sound) <= 1.0);
        }
    }

    /// Issue #103: each animal its voice, each mood its contour: the nine cries of a mood all
    /// differ, the four moods of an animal too; a hurt cry falls, a happy one rises.
    #[test]
    fn each_animal_cries_with_its_own_voice() {
        for mood in Mood::ALL {
            for (i, a) in Animal::ALL.iter().enumerate() {
                for b in &Animal::ALL[i + 1..] {
                    assert_ne!(cry(*a, mood), cry(*b, mood), "{a:?} {b:?}");
                }
            }
        }
        for animal in Animal::ALL {
            let hurt = cry(animal, Mood::Hurt);
            assert!(hurt.pitch[2] < hurt.pitch[0], "{animal:?} hurt falls");
            let happy = cry(animal, Mood::Happy);
            assert!(happy.pitch[2] > happy.pitch[0], "{animal:?} happy rises");
            assert!(cry(animal, Mood::Effort).duration < hurt.duration);
        }
        // The cow is the lowest and longest, the chick the highest.
        let lowest = Animal::ALL
            .iter()
            .min_by(|a, b| cry(**a, Mood::Hurt).pitch[0].total_cmp(&cry(**b, Mood::Hurt).pitch[0]));
        assert_eq!(lowest, Some(&Animal::Cow));
        assert_eq!(
            Animal::of_model(Some("cube-pets/animal-elephant")),
            Animal::Elephant
        );
        assert_eq!(
            Animal::of_model(Some("cube-pets/animal-chick")),
            Animal::Chick
        );
        assert_eq!(Animal::of_model(None), Animal::Cat);
    }

    /// The drift's chimes rise with the tiers, and so do the mini-turbos' « fwoosh » (longer).
    #[test]
    fn the_drift_tiers_sound_higher_and_higher() {
        let pitch = |tier| tier_chime(tier).from_hz;
        assert!(pitch(MiniTurbo::Blue) < pitch(MiniTurbo::Orange));
        assert!(pitch(MiniTurbo::Orange) < pitch(MiniTurbo::Purple));
        let length = |tier| SoundEffect::MiniTurbo(tier).length();
        assert!(length(MiniTurbo::Blue) < length(MiniTurbo::Purple));
    }
}
