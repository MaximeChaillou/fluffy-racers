//! Issue #104: the music (GD § 14, « La musique »), pure and unit-tested:
//!
//! - **the catalog** ([`Catalog`]): every piece the game plays, from `assets/data/music.ron`
//!   (compiled into the game, like the biomes): its file, its length (what loops), its tempo, its
//!   level, its faster version for the last lap, whether it is a jingle; and what the screens
//!   play (the home, the lobby, the last lap, the finish by place, the podium). A circuit names
//!   its piece in its `.track.ron` (`music`), or takes its biome's (`assets/data/biomes.ron`);
//! - **what is shown** ([`MusicScene`]: the home, the lobby, a circuit before GO, racing or on its last
//!   lap, the player's finish and its place, the podium) and **what it plays** ([`Program`]: a
//!   jingle first or not, then a piece in a loop, its speed, how the music heard before goes out
//!   and how this one comes in);
//! - **the deck** ([`Deck`]): one piece heard at a time (the one before has faded out before the
//!   next starts: a single piece decoded at once), a jingle and its music as one voice (the music
//!   on the jingle's last sample, as the composer chained them), the fades.

use std::sync::OnceLock;

use serde::Deserialize;

use super::tuning::MusicTuning;

/// The catalog's file, compiled into the game.
pub const MUSIC_RON: &str = include_str!("../../assets/data/music.ron");

/// A piece of music, as written in `music.ron`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    /// Its name, for the circuits, the biomes and the screens.
    pub id: String,
    /// Its file under `assets/` (OGG Vorbis).
    pub file: String,
    /// Its length (s): what loops. Measured on the decoded file (a test checks it).
    pub seconds: f64,
    /// Its tempo (beats per minute), for a loop (a jingle: 0).
    #[serde(default)]
    pub bpm: f32,
    /// Its level (0 to 1): the pieces sound as loud as each other (from their measured loudness).
    pub volume: f32,
    /// The version of the last lap: the same music, faster (a higher tempo). Without one, the
    /// piece itself plays faster (`MusicTuning::final_lap_speed`).
    #[serde(default)]
    pub faster: Option<String>,
    /// Played once (then the music that follows it), not in a loop.
    #[serde(default)]
    pub jingle: bool,
}

/// A jingle and the music after it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pair {
    pub jingle: String,
    pub music: String,
}

/// The finish's jingle and music, by the player's place ([`Tier`]).
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Finish {
    pub best: Pair,
    pub good: Pair,
    pub bad: Pair,
}

/// `music.ron`, as written.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct MusicFile {
    home: String,
    salon: String,
    final_lap: String,
    finish: Finish,
    podium: Pair,
    pieces: Vec<Piece>,
}

/// How the player finished: its place against `MusicTuning::best_place` and `good_place`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Best,
    Good,
    Bad,
}

impl Tier {
    pub const ALL: [Self; 3] = [Tier::Best, Tier::Good, Tier::Bad];

    /// The tier of `place` (from 1; 0, an unknown place, is the last).
    pub fn of(place: u32, tuning: &MusicTuning) -> Self {
        match place {
            0 => Tier::Bad,
            p if p <= tuning.best_place => Tier::Best,
            p if p <= tuning.good_place => Tier::Good,
            _ => Tier::Bad,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The catalog, its names resolved (indices of `pieces`).
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub pieces: Vec<Piece>,
    pub home: usize,
    pub salon: usize,
    /// The last lap's jingle.
    pub final_lap: usize,
    /// The finish's (jingle, music), by [`Tier`].
    pub finish: [(usize, usize); 3],
    /// The podium's (jingle, music).
    pub podium: (usize, usize),
    faster: Vec<Option<usize>>,
}

impl Catalog {
    /// Reads and checks `music.ron`: each name known, unique, each piece where it may be (a
    /// jingle where a jingle is played, a loop elsewhere), each faster version faster.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: MusicFile = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| format!("music.ron: {e}"))?;
        let pieces = file.pieces;
        for (i, piece) in pieces.iter().enumerate() {
            let fail = |why: &str| Err(format!("music.ron: {}: {why}", piece.id));
            if piece.id.is_empty() || pieces[..i].iter().any(|p| p.id == piece.id) {
                return fail("an empty or repeated id");
            }
            if !piece.file.starts_with("music/") || !piece.file.ends_with(".ogg") {
                return fail("its file is music/<source>/<name>.ogg");
            }
            if !(0.5..=900.0).contains(&piece.seconds) {
                return fail("a length of 0.5 s to 15 min");
            }
            if !piece.jingle && !(40.0..=300.0).contains(&piece.bpm) {
                return fail("a tempo of 40 to 300 bpm");
            }
            if !(piece.volume > 0.0 && piece.volume <= 1.0) {
                return fail("a level above 0, up to 1");
            }
        }
        let index = |id: &str| {
            pieces
                .iter()
                .position(|p| p.id == id)
                .ok_or_else(|| format!("music.ron: no piece {id}"))
        };
        let looped = |id: &str| -> Result<usize, String> {
            let i = index(id)?;
            if pieces[i].jingle {
                return Err(format!("music.ron: {id} is a jingle, a loop is wanted"));
            }
            Ok(i)
        };
        let jingle = |id: &str| -> Result<usize, String> {
            let i = index(id)?;
            if !pieces[i].jingle {
                return Err(format!("music.ron: {id} is not a jingle"));
            }
            Ok(i)
        };
        let pair = |pair: &Pair| -> Result<(usize, usize), String> {
            Ok((jingle(&pair.jingle)?, looped(&pair.music)?))
        };
        let mut faster = Vec::with_capacity(pieces.len());
        for piece in &pieces {
            faster.push(match &piece.faster {
                None => None,
                Some(id) => {
                    let i = looped(id)?;
                    if piece.jingle || pieces[i].bpm <= piece.bpm || pieces[i].faster.is_some() {
                        return Err(format!(
                            "music.ron: {id} is no faster version of {}",
                            piece.id
                        ));
                    }
                    Some(i)
                }
            });
        }
        Ok(Self {
            home: looped(&file.home)?,
            salon: looped(&file.salon)?,
            final_lap: jingle(&file.final_lap)?,
            finish: [
                pair(&file.finish.best)?,
                pair(&file.finish.good)?,
                pair(&file.finish.bad)?,
            ],
            podium: pair(&file.podium)?,
            faster,
            pieces,
        })
    }

    /// The piece named `id`.
    pub fn index(&self, id: &str) -> Option<usize> {
        self.pieces.iter().position(|p| p.id == id)
    }

    /// The last lap's version of `piece`, if it has one.
    pub fn faster(&self, piece: usize) -> Option<usize> {
        self.faster.get(piece).copied().flatten()
    }

    /// The finish's (jingle, music) for `tier`.
    pub fn finish(&self, tier: Tier) -> (usize, usize) {
        self.finish[tier.index()]
    }

    /// The length of `piece` (s; 0 for none).
    pub fn seconds(&self, piece: usize) -> f64 {
        self.pieces.get(piece).map_or(0.0, |p| p.seconds)
    }
}

/// The catalog compiled into the game. The file is checked by a test; should it still be
/// broken, the console says why and there is no music.
pub fn catalog() -> Option<&'static Catalog> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            Catalog::parse(MUSIC_RON)
                .map_err(|error| bevy::log::error!("{error}"))
                .ok()
        })
        .as_ref()
}

/// What the game shows, as the music sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MusicScene {
    /// Nothing to play: a level loading, the test ground.
    #[default]
    Silent,
    /// The home screen (issue #89), and the screens it opens.
    Home,
    /// The lobby in 3D (issue #101).
    Salon,
    /// A circuit and its piece: before GO (`started: false`, nothing yet: the countdown beeps),
    /// then racing, then on the player's last lap.
    Circuit {
        piece: usize,
        started: bool,
        last_lap: bool,
    },
    /// The player has finished: its jingle, then the results' music, by its place.
    Finished(Tier),
    /// The podium of a championship.
    Podium,
}

/// What the music plays: a jingle first (once) or not, then a piece in a loop (at `speed`);
/// the piece heard before goes out over `leave` (s), and the loop comes in over `enter` (s; 0:
/// at once, as at GO or after a jingle).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Program {
    pub jingle: Option<usize>,
    pub music: Option<usize>,
    pub speed: f32,
    pub leave: f32,
    pub enter: f32,
}

impl MusicScene {
    /// What this scene plays.
    pub fn program(self, catalog: &Catalog, tuning: &MusicTuning) -> Program {
        let quiet = Program {
            jingle: None,
            music: None,
            speed: 1.0,
            leave: tuning.fade_out,
            enter: 0.0,
        };
        let screen = |music| Program {
            music: Some(music),
            enter: tuning.fade_in,
            ..quiet
        };
        let after = |(jingle, music): (usize, usize), leave| Program {
            jingle: Some(jingle),
            music: Some(music),
            leave,
            ..quiet
        };
        match self {
            MusicScene::Silent | MusicScene::Circuit { started: false, .. } => quiet,
            MusicScene::Home => screen(catalog.home),
            MusicScene::Salon => screen(catalog.salon),
            MusicScene::Circuit {
                piece,
                last_lap: false,
                ..
            } => Program {
                music: Some(piece),
                ..quiet
            },
            MusicScene::Circuit { piece, .. } => {
                let faster = catalog.faster(piece);
                Program {
                    speed: if faster.is_some() {
                        1.0
                    } else {
                        tuning.final_lap_speed.clamp(1.0, 2.0)
                    },
                    ..after(
                        (catalog.final_lap, faster.unwrap_or(piece)),
                        tuning.jingle_fade,
                    )
                }
            }
            MusicScene::Finished(tier) => after(catalog.finish(tier), tuning.finish_fade),
            MusicScene::Podium => after(catalog.podium, tuning.fade_out),
        }
    }

    /// The pieces to have ready for this scene, into `out` (cleared first): those it plays, and
    /// those that may come next (on a circuit: its faster version, the last lap's jingle, the
    /// finish's jingles and musics; after the finish: the podium's). A jingle and its music
    /// start together: both are ready before.
    pub fn wanted(self, catalog: &Catalog, tuning: &MusicTuning, out: &mut Vec<usize>) {
        out.clear();
        let program = self.program(catalog, tuning);
        out.extend(program.jingle);
        out.extend(program.music);
        match self {
            MusicScene::Circuit { piece, .. } => {
                out.push(piece);
                out.extend(catalog.faster(piece));
                out.push(catalog.final_lap);
                for tier in Tier::ALL {
                    let (jingle, music) = catalog.finish(tier);
                    out.extend([jingle, music]);
                }
            }
            MusicScene::Finished(_) => out.extend([catalog.podium.0, catalog.podium.1]),
            _ => {}
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// The piece heard: `piece` (in a loop, or once if it is a jingle), after its `intro` (a jingle,
/// played once first: the same voice, the music starting on the jingle's last sample, as the
/// composer chained them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    pub piece: usize,
    pub intro: Option<usize>,
    /// The piece itself is a jingle: played once, then the voice ends.
    pub jingle: bool,
    /// Its playback speed.
    pub speed: f32,
    /// Its fade (0 to 1).
    pub gain: f32,
    /// When it started (s, real time).
    pub since: f64,
    /// Going out, from full in this long (s).
    pub leaving: Option<f32>,
    /// Coming in, in this long (s).
    enter: f32,
}

impl Heard {
    /// What is heard at `now`: the intro for its length, then the piece. `length`: a piece's
    /// length (s).
    pub fn sounding(&self, now: f64, length: impl Fn(usize) -> f64) -> usize {
        match self.intro {
            Some(intro) if now < self.since + length(intro) / f64::from(self.speed.max(0.1)) => {
                intro
            }
            _ => self.piece,
        }
    }
}

/// Plays the programs, one piece heard at a time (see the module's documentation).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deck {
    /// The program wanted (none before the first).
    pub program: Option<Program>,
    /// The piece heard.
    pub heard: Option<Heard>,
    /// The program is still to start.
    pending: bool,
    /// Voices started so far: each one is a new voice.
    pub started: u32,
}

impl Deck {
    /// One frame at `now` (s, real time), `dt` (s) after the last one, wanting `program`.
    /// `length`: a piece's length (s); `ready`: whether a piece can play now (loaded). The
    /// program's jingle and its music are one voice, the jingle first; a jingle not ready is
    /// left out (the music alone), a music not ready is waited for.
    pub fn update(
        &mut self,
        program: Program,
        now: f64,
        dt: f32,
        length: impl Fn(usize) -> f64,
        ready: impl Fn(usize) -> bool,
    ) {
        if self.program != Some(program) {
            self.program = Some(program);
            self.pending = true;
            match &mut self.heard {
                // The same loop goes on (its speed may change).
                Some(heard)
                    if !heard.jingle
                        && heard.leaving.is_none()
                        && program.jingle.is_none()
                        && program.music == Some(heard.piece) =>
                {
                    heard.speed = program.speed;
                    self.pending = false;
                }
                Some(heard) => {
                    // Going out already: the quicker of its fade and this one's.
                    let leave = program.leave.max(0.0);
                    heard.leaving = Some(heard.leaving.map_or(leave, |l| l.min(leave)));
                }
                None => {}
            }
        }
        if let Some(heard) = &mut self.heard {
            if let Some(leave) = heard.leaving {
                heard.gain -= if leave > 0.0 { dt / leave } else { 1.0 };
                if heard.gain <= 0.0 {
                    self.heard = None;
                }
            } else if heard.jingle && now >= heard.since + length(heard.piece) {
                self.heard = None;
            } else if heard.gain < 1.0 {
                heard.gain = if heard.enter > 0.0 {
                    (heard.gain + dt / heard.enter).min(1.0)
                } else {
                    1.0
                };
            }
        }
        if self.heard.is_some() || !self.pending {
            return;
        }
        let intro = program.jingle.filter(|jingle| ready(*jingle));
        match program.music {
            Some(music) if ready(music) => {
                // After its jingle, or at GO, at once; after another screen's music, it comes in.
                let enter = if program.jingle.is_some() {
                    0.0
                } else {
                    program.enter
                };
                self.start(music, intro, false, program.speed, now, enter);
                self.pending = false;
            }
            Some(_) => {}
            None => {
                if let Some(jingle) = intro {
                    self.start(jingle, None, true, 1.0, now, 0.0);
                }
                self.pending = false;
            }
        }
    }

    fn start(
        &mut self,
        piece: usize,
        intro: Option<usize>,
        jingle: bool,
        speed: f32,
        now: f64,
        enter: f32,
    ) {
        self.started = self.started.wrapping_add(1);
        self.heard = Some(Heard {
            piece,
            intro,
            jingle,
            speed,
            gain: if enter > 0.0 { 0.0 } else { 1.0 },
            since: now,
            leaving: None,
            enter,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f32 = 1.0 / 60.0;

    fn tuning() -> MusicTuning {
        MusicTuning::default()
    }

    /// Runs `deck` for `frames` frames of 1/60 s from `*now`, wanting `program`, every piece
    /// ready; returns what was heard on each frame.
    fn run(
        deck: &mut Deck,
        catalog: &Catalog,
        program: Program,
        now: &mut f64,
        frames: usize,
    ) -> Vec<Option<Heard>> {
        (0..frames)
            .map(|_| {
                *now += f64::from(FRAME);
                deck.update(program, *now, FRAME, |p| catalog.seconds(p), |_| true);
                deck.heard
            })
            .collect()
    }

    /// Issue #104: the catalog of the game reads, every name resolved: the screens' pieces,
    /// the jingles where jingles are played, each faster version faster than its piece.
    #[test]
    fn the_catalog_reads_and_every_name_resolves() {
        let catalog = Catalog::parse(MUSIC_RON).unwrap();
        assert_eq!(super::catalog(), Some(&catalog));
        assert!(!catalog.pieces[catalog.home].jingle);
        assert!(!catalog.pieces[catalog.salon].jingle);
        assert_ne!(catalog.home, catalog.salon, "a music each");
        assert!(catalog.pieces[catalog.final_lap].jingle);
        for tier in Tier::ALL {
            let (jingle, music) = catalog.finish(tier);
            assert!(catalog.pieces[jingle].jingle && !catalog.pieces[music].jingle);
        }
        let mut finishes: Vec<usize> = Tier::ALL.iter().map(|t| catalog.finish(*t).1).collect();
        finishes.dedup();
        assert_eq!(finishes.len(), 3, "a music for each tier");
        let (jingle, music) = catalog.podium;
        assert!(catalog.pieces[jingle].jingle && !catalog.pieces[music].jingle);
        for (i, piece) in catalog.pieces.iter().enumerate() {
            if let Some(faster) = catalog.faster(i) {
                assert!(catalog.pieces[faster].bpm > piece.bpm, "{}", piece.id);
            }
        }
        // A misspelled name, a jingle as a loop, a slower « faster »: refused.
        let broken = MUSIC_RON.replacen("home: \"", "home: \"x", 1);
        assert!(Catalog::parse(&broken).is_err());
        let jingle_home = MUSIC_RON.replacen(
            &format!("home: \"{}\"", catalog.pieces[catalog.home].id),
            &format!("home: \"{}\"", catalog.pieces[catalog.final_lap].id),
            1,
        );
        assert!(Catalog::parse(&jingle_home).is_err());
    }

    fn catalog_of_test() -> Catalog {
        Catalog::parse(MUSIC_RON).unwrap()
    }

    /// The circuit's piece of the catalog that has a faster version, and one that has none.
    fn circuit_pieces(catalog: &Catalog) -> (usize, Option<usize>) {
        let with = (0..catalog.pieces.len())
            .find(|i| catalog.faster(*i).is_some())
            .unwrap();
        let without = (0..catalog.pieces.len()).find(|i| {
            !catalog.pieces[*i].jingle
                && catalog.faster(*i).is_none()
                && !catalog.faster.contains(&Some(*i))
        });
        (with, without)
    }

    /// Issue #104: each screen its music; a circuit says nothing before GO, then its piece at
    /// once; its last lap: the jingle, then its faster version (or itself, faster); the finish:
    /// the jingle of its tier, then its music; the podium too.
    #[test]
    fn each_scene_plays_its_program() {
        let catalog = catalog_of_test();
        let tuning = tuning();
        let program = |scene: MusicScene| scene.program(&catalog, &tuning);
        assert_eq!(program(MusicScene::Silent).music, None);
        assert_eq!(program(MusicScene::Home).music, Some(catalog.home));
        assert_eq!(program(MusicScene::Home).enter, tuning.fade_in);
        assert_eq!(program(MusicScene::Salon).music, Some(catalog.salon));
        let (piece, other) = circuit_pieces(&catalog);
        let circuit = |started, last_lap| MusicScene::Circuit {
            piece,
            started,
            last_lap,
        };
        assert_eq!(program(circuit(false, false)).music, None, "the countdown");
        let racing = program(circuit(true, false));
        assert_eq!(
            (racing.jingle, racing.music, racing.enter, racing.speed),
            (None, Some(piece), 0.0, 1.0)
        );
        let last = program(circuit(true, true));
        assert_eq!(last.jingle, Some(catalog.final_lap));
        assert_eq!(last.music, catalog.faster(piece));
        assert_eq!(last.speed, 1.0);
        assert_eq!(last.leave, tuning.jingle_fade);
        if let Some(other) = other {
            let last = program(MusicScene::Circuit {
                piece: other,
                started: true,
                last_lap: true,
            });
            assert_eq!(last.music, Some(other));
            assert!(last.speed > 1.0, "{}", last.speed);
        }
        for tier in Tier::ALL {
            let finish = program(MusicScene::Finished(tier));
            assert_eq!(
                (finish.jingle, finish.music),
                (Some(catalog.finish(tier).0), Some(catalog.finish(tier).1))
            );
            assert_eq!(finish.leave, tuning.finish_fade, "not dead");
        }
        let podium = program(MusicScene::Podium);
        assert_eq!(
            (podium.jingle, podium.music),
            (Some(catalog.podium.0), Some(catalog.podium.1))
        );
        // The places: 1st the best, up to 3rd good, then bad.
        assert_eq!(Tier::of(1, &tuning), Tier::Best);
        assert_eq!(Tier::of(2, &tuning), Tier::Good);
        assert_eq!(Tier::of(3, &tuning), Tier::Good);
        assert_eq!(Tier::of(4, &tuning), Tier::Bad);
        assert_eq!(Tier::of(0, &tuning), Tier::Bad);
        // On a circuit, its next pieces are made ready.
        let mut wanted = Vec::new();
        circuit(false, false).wanted(&catalog, &tuning, &mut wanted);
        assert!(wanted.contains(&piece) && wanted.contains(&catalog.final_lap));
        assert!(wanted.contains(&catalog.faster(piece).unwrap()));
        assert!(wanted.contains(&catalog.finish(Tier::Bad).0));
        MusicScene::Home.wanted(&catalog, &tuning, &mut wanted);
        assert_eq!(wanted, [catalog.home]);
    }

    /// Issue #104 (« Transitions »): from one screen to the next, the music heard goes out, then
    /// the next comes in: never two at once; a jingle plays its length, then its music at once.
    #[test]
    fn the_deck_fades_from_one_piece_to_the_next() {
        let catalog = catalog_of_test();
        let tuning = tuning();
        let mut deck = Deck::default();
        let mut now = 0.0;
        let home = MusicScene::Home.program(&catalog, &tuning);
        let heard = run(&mut deck, &catalog, home, &mut now, 60);
        assert_eq!(deck.started, 1);
        assert_eq!(heard[0].unwrap().piece, catalog.home);
        assert!(heard[0].unwrap().gain < 0.1, "it comes in");
        let full = (tuning.fade_in / FRAME).ceil() as usize + 1;
        assert_eq!(heard[full].unwrap().gain, 1.0);
        // To the lobby: the home's music goes out over `fade_out`, then the lobby's comes in.
        let salon = MusicScene::Salon.program(&catalog, &tuning);
        let heard = run(&mut deck, &catalog, salon, &mut now, 120);
        let out = heard
            .iter()
            .position(|h| h.is_some_and(|h| h.piece == catalog.salon))
            .unwrap();
        let expected = (tuning.fade_out / FRAME) as usize;
        assert!(
            out.abs_diff(expected) <= 2,
            "{out} frames, {expected} wanted"
        );
        assert!(
            heard[..out]
                .windows(2)
                .all(|w| w[1].unwrap().gain < w[0].unwrap().gain),
            "it goes down"
        );
        assert_eq!(deck.started, 2);
        // The finish: the race's music out (slowly), then one voice: the jingle for its length,
        // then its music, at once.
        let (piece, _) = circuit_pieces(&catalog);
        let racing = MusicScene::Circuit {
            piece,
            started: true,
            last_lap: false,
        }
        .program(&catalog, &tuning);
        run(&mut deck, &catalog, racing, &mut now, 200);
        assert_eq!(deck.heard.unwrap().piece, piece);
        let finish = MusicScene::Finished(Tier::Good).program(&catalog, &tuning);
        let (jingle, music) = catalog.finish(Tier::Good);
        let frames = ((tuning.finish_fade as f64 + catalog.seconds(jingle)) * 60.0) as usize + 30;
        let start = now;
        let heard = run(&mut deck, &catalog, finish, &mut now, frames);
        let starts = heard
            .iter()
            .position(|h| h.is_some_and(|h| h.piece == music))
            .unwrap();
        let fade = (tuning.finish_fade / FRAME) as usize;
        assert!(starts.abs_diff(fade) <= 2, "{starts} frames, about {fade}");
        let voice = heard[starts].unwrap();
        assert_eq!((voice.intro, voice.gain), (Some(jingle), 1.0), "at once");
        let at = |frame: usize| start + f64::from(FRAME) * (frame + 1) as f64;
        let length = (catalog.seconds(jingle) * 60.0) as usize;
        let seconds = |p| catalog.seconds(p);
        assert_eq!(voice.sounding(at(starts + length - 2), seconds), jingle);
        assert_eq!(voice.sounding(at(starts + length + 2), seconds), music);
        assert!(
            heard[starts..].iter().all(|h| *h == Some(voice)),
            "one voice"
        );
        // Never two pieces: each start comes after the end of the one before.
        assert_eq!(deck.started, 4);
    }

    /// The same loop goes on when the program keeps it; a jingle not ready is left out; a loop
    /// not ready is waited for.
    #[test]
    fn the_deck_keeps_its_loop_and_waits_for_what_loads() {
        let catalog = catalog_of_test();
        let tuning = tuning();
        let (piece, _) = circuit_pieces(&catalog);
        let mut deck = Deck::default();
        let racing = MusicScene::Circuit {
            piece,
            started: true,
            last_lap: false,
        }
        .program(&catalog, &tuning);
        deck.update(racing, 0.0, FRAME, |p| catalog.seconds(p), |p| p != piece);
        assert_eq!(deck.heard, None, "it waits for its piece");
        deck.update(racing, 0.1, FRAME, |p| catalog.seconds(p), |_| true);
        assert_eq!(deck.heard.unwrap().piece, piece);
        let faster = Program {
            speed: 1.1,
            ..racing
        };
        deck.update(faster, 0.2, FRAME, |p| catalog.seconds(p), |_| true);
        assert_eq!(deck.started, 1, "the same voice");
        assert_eq!(deck.heard.unwrap().speed, 1.1);
        let last = MusicScene::Circuit {
            piece,
            started: true,
            last_lap: true,
        }
        .program(&catalog, &tuning);
        let mut now = 0.2;
        for _ in 0..30 {
            now += f64::from(FRAME);
            deck.update(
                last,
                now,
                FRAME,
                |p| catalog.seconds(p),
                |p| p != catalog.final_lap,
            );
        }
        let heard = deck.heard.unwrap();
        assert_eq!(heard.piece, catalog.faster(piece).unwrap());
        assert_eq!(heard.intro, None, "no jingle");
        assert_eq!(deck.started, 2);
        // Its music not ready: the jingle waits with it.
        let finish = MusicScene::Finished(Tier::Bad).program(&catalog, &tuning);
        let (jingle, music) = catalog.finish(Tier::Bad);
        for _ in 0..200 {
            now += f64::from(FRAME);
            deck.update(finish, now, FRAME, |p| catalog.seconds(p), |p| p != music);
        }
        assert_eq!(deck.heard, None);
        deck.update(finish, now, FRAME, |p| catalog.seconds(p), |_| true);
        assert_eq!(deck.heard.unwrap().intro, Some(jingle));
        // A program of a jingle alone: the jingle once, then nothing.
        let alone = Program {
            jingle: Some(jingle),
            music: None,
            ..finish
        };
        let mut deck = Deck::default();
        let mut now = 0.0;
        let heard = run(&mut deck, &catalog, alone, &mut now, 400);
        assert!(heard[0].is_some_and(|h| h.piece == jingle && h.jingle));
        let ends = heard.iter().position(Option::is_none).unwrap();
        assert!(ends.abs_diff((catalog.seconds(jingle) * 60.0) as usize) <= 2);
    }
}
