//! The roster (GD § 9, issue #15): characters, karts, wheels and wings, as written in the RON
//! files of `assets/data/`. Pure logic, no ECS: parsing, checks, loadouts. Unit-tested.
//!
//! The files are compiled into the game (`include_str!`, like the circuit manifest of #36): they
//! are there before the first kart spawns, with no request that could fail and no folder to
//! read (impossible in a browser). A test reads them and checks every rule below.

use bevy::prelude::Resource;
use serde::Deserialize;

use crate::kart::{
    DriverEars, KartStats, MIDDLE_LEVEL, PartLook, PartLooks, Stat, StatPoints, Terrain,
};
use crate::track::biome::is_model_id;

use super::paw::PawShape;

/// The four RON files, compiled into the game.
pub const CHARACTERS_RON: &str = include_str!("../../assets/data/characters.ron");
pub const KARTS_RON: &str = include_str!("../../assets/data/karts.ron");
pub const WHEELS_RON: &str = include_str!("../../assets/data/wheels.ron");
pub const WINGS_RON: &str = include_str!("../../assets/data/wings.ron");

/// Weight class of a character (GD § 9): light ones accelerate and turn better, heavy ones go
/// faster and push harder.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeightClass {
    Light,
    Medium,
    Heavy,
}

impl WeightClass {
    pub const ALL: [Self; 3] = [WeightClass::Light, WeightClass::Medium, WeightClass::Heavy];

    pub fn label(self) -> &'static str {
        match self {
            WeightClass::Light => "léger",
            WeightClass::Medium => "moyen",
            WeightClass::Heavy => "lourd",
        }
    }
}

/// A character, as written in `characters.ron`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CharacterData {
    pub name: String,
    pub class: WeightClass,
    /// Size of the driver (1 = medium): the weight class shows on screen.
    pub size: f32,
    /// The driver's 3D model (issue #69), "pack/name": `assets/models/pack/name.glb`, an animal
    /// of Kenney's Cube Pets. Without it (or while it loads, or if the file is missing), the
    /// driver stands in primitives, with the `color` and `ears` below.
    #[serde(default, deserialize_with = "some_model")]
    pub model: Option<String>,
    /// Color of the driver in primitives (red, green, blue, 0 to 1).
    pub color: (f32, f32, f32),
    /// Its ears in primitives.
    pub ears: DriverEars,
    /// Issue #169: what its paw looks like (`characters::PawShape`): left out, a round paw with
    /// toes.
    #[serde(default)]
    pub paw: PawShape,
    pub stats: StatPoints,
}

/// `model: "pack/name"` in the file (no `Some(...)` around it); left out: None.
fn some_model<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}

/// A kart, a set of wheels or a wing, as written in `karts.ron`, `wheels.ron`, `wings.ron`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PartData {
    pub name: String,
    pub stats: StatPoints,
    /// What it looks like on the kart (issue #73, `kart::PartLook`): a body in `karts.ron`, a
    /// set of wheels in `wheels.ron`, a wing in `wings.ron`. Left out: the look of the kart of
    /// before (the Car Kit's kart, its wheels, the pillowcase).
    #[serde(default, deserialize_with = "some_look")]
    pub look: Option<PartLook>,
}

/// `look: Bathtub` in the file (no `Some(...)` around it); left out: None.
fn some_look<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<PartLook>, D::Error> {
    PartLook::deserialize(d).map(Some)
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct CharacterFile {
    default: String,
    characters: Vec<CharacterData>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
struct PartFile {
    default: String,
    parts: Vec<PartData>,
}

/// The four parts of a kart (GD § 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartKind {
    Character,
    Kart,
    Wheels,
    Wing,
}

impl PartKind {
    pub const ALL: [Self; 4] = [
        PartKind::Character,
        PartKind::Kart,
        PartKind::Wheels,
        PartKind::Wing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PartKind::Character => "Personnage",
            PartKind::Kart => "Kart",
            PartKind::Wheels => "Roues",
            PartKind::Wing => "Aile",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A choice of the four parts: indices into the roster's lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Loadout {
    pub character: usize,
    pub kart: usize,
    pub wheels: usize,
    pub wing: usize,
}

impl Loadout {
    pub fn get(&self, kind: PartKind) -> usize {
        match kind {
            PartKind::Character => self.character,
            PartKind::Kart => self.kart,
            PartKind::Wheels => self.wheels,
            PartKind::Wing => self.wing,
        }
    }

    pub fn set(&mut self, kind: PartKind, index: usize) {
        match kind {
            PartKind::Character => self.character = index,
            PartKind::Kart => self.kart = index,
            PartKind::Wheels => self.wheels = index,
            PartKind::Wing => self.wing = index,
        }
    }
}

/// Every character and part of the game.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Roster {
    pub characters: Vec<CharacterData>,
    pub karts: Vec<PartData>,
    pub wheels: Vec<PartData>,
    pub wings: Vec<PartData>,
    /// The default choice of each file (`default: "..."`): the player's loadout until it picks
    /// another one.
    default: Loadout,
}

impl Roster {
    /// The roster compiled into the game. The files are checked by a test; should they still be
    /// broken, the console says why and a single neutral choice stands in (the middle level
    /// everywhere: the game drives as before the stats).
    pub fn embedded() -> Self {
        Self::parse(CHARACTERS_RON, KARTS_RON, WHEELS_RON, WINGS_RON).unwrap_or_else(|error| {
            bevy::log::error!("roster files: {error}; using a neutral roster");
            Self::neutral()
        })
    }

    /// One character and one part of each kind, adding up to the middle level in every stat.
    pub fn neutral() -> Self {
        let part = |name: &str, points: u8| PartData {
            name: name.to_string(),
            stats: StatPoints::uniform(points),
            look: None,
        };
        Self {
            characters: vec![CharacterData {
                name: "Chat".to_string(),
                class: WeightClass::Medium,
                size: 1.0,
                model: None,
                color: (0.95, 0.65, 0.35),
                ears: DriverEars::Pointy,
                paw: PawShape::Toes,
                stats: StatPoints::uniform(4),
            }],
            karts: vec![part("Caisse à savon", 3)],
            wheels: vec![part("Pneus guimauve", 2)],
            wings: vec![part("Taie d'oreiller", 1)],
            default: Loadout::default(),
        }
    }

    /// Reads and checks the four files: every list has at least one entry, no name twice in a
    /// list, at most 10 points per stat, a driver size and color that make sense, a model named
    /// "pack/name", and a `default` that names an entry of its list.
    pub fn parse(characters: &str, karts: &str, wheels: &str, wings: &str) -> Result<Self, String> {
        let characters: CharacterFile =
            ron::from_str(characters).map_err(|e| format!("characters.ron: {e}"))?;
        let parts = |file: &str, text: &str| -> Result<PartFile, String> {
            ron::from_str(text).map_err(|e| format!("{file}: {e}"))
        };
        let karts = parts("karts.ron", karts)?;
        let wheels = parts("wheels.ron", wheels)?;
        let wings = parts("wings.ron", wings)?;

        let mut default = Loadout::default();
        check_list(
            "characters.ron",
            &characters.default,
            characters
                .characters
                .iter()
                .map(|c| (c.name.as_str(), &c.stats)),
        )
        .map(|i| default.character = i)?;
        for c in &characters.characters {
            if !(0.5..=1.6).contains(&c.size) {
                return Err(format!(
                    "characters.ron: {}: size {} out of 0.5..=1.6",
                    c.name, c.size
                ));
            }
            let (r, g, b) = c.color;
            if [r, g, b].iter().any(|v| !(0.0..=1.0).contains(v)) {
                return Err(format!("characters.ron: {}: color out of 0..=1", c.name));
            }
            if let Some(model) = c.model.as_deref().filter(|m| !is_model_id(m)) {
                return Err(format!(
                    "characters.ron: {}: model {model:?} is not \"pack/name\"",
                    c.name
                ));
            }
        }
        for (file, list, kind) in [
            ("karts.ron", &karts, PartKind::Kart),
            ("wheels.ron", &wheels, PartKind::Wheels),
            ("wings.ron", &wings, PartKind::Wing),
        ] {
            let index = check_list(
                file,
                &list.default,
                list.parts.iter().map(|p| (p.name.as_str(), &p.stats)),
            )?;
            default.set(kind, index);
            // Issue #73: a look of its kind.
            for part in &list.parts {
                let Some(look) = part.look else {
                    continue;
                };
                let fits = match kind {
                    PartKind::Kart => look.body().is_some(),
                    PartKind::Wheels => look.wheels().is_some(),
                    PartKind::Wing => look.wing().is_some(),
                    PartKind::Character => false,
                };
                if !fits {
                    return Err(format!(
                        "{file}: {}: look {look:?} is for another kind of part",
                        part.name
                    ));
                }
            }
        }
        Ok(Self {
            characters: characters.characters,
            karts: karts.parts,
            wheels: wheels.parts,
            wings: wings.parts,
            default,
        })
    }

    /// The loadout of the files' `default` entries.
    pub fn default_loadout(&self) -> Loadout {
        self.default
    }

    pub fn count(&self, kind: PartKind) -> usize {
        match kind {
            PartKind::Character => self.characters.len(),
            PartKind::Kart => self.karts.len(),
            PartKind::Wheels => self.wheels.len(),
            PartKind::Wing => self.wings.len(),
        }
    }

    /// Name of the entry `index` of `kind` (bounded to the list).
    pub fn name(&self, kind: PartKind, index: usize) -> &str {
        let last = self.count(kind).saturating_sub(1);
        let index = index.min(last);
        match kind {
            PartKind::Character => &self.characters[index].name,
            PartKind::Kart => &self.karts[index].name,
            PartKind::Wheels => &self.wheels[index].name,
            PartKind::Wing => &self.wings[index].name,
        }
    }

    /// Index of the entry named `name` of `kind`.
    pub fn find(&self, kind: PartKind, name: &str) -> Option<usize> {
        (0..self.count(kind)).find(|i| self.name(kind, *i) == name)
    }

    /// The character of `loadout` (bounded to the list).
    pub fn character(&self, loadout: &Loadout) -> &CharacterData {
        &self.characters[loadout.character.min(self.characters.len() - 1)]
    }

    /// The points of the entry `index` of `kind`.
    pub fn points(&self, kind: PartKind, index: usize) -> StatPoints {
        let index = index.min(self.count(kind).saturating_sub(1));
        match kind {
            PartKind::Character => self.characters[index].stats,
            PartKind::Kart => self.karts[index].stats,
            PartKind::Wheels => self.wheels[index].stats,
            PartKind::Wing => self.wings[index].stats,
        }
    }

    /// What a kart with these parts looks like (issue #73): its body, its wheels, its wing (a
    /// part without a look: the kart of before's).
    pub fn looks(&self, loadout: &Loadout) -> PartLooks {
        let part = |list: &[PartData], index: usize| {
            list.get(index.min(list.len().saturating_sub(1)))
                .and_then(|p| p.look)
        };
        PartLooks {
            body: part(&self.karts, loadout.kart)
                .and_then(PartLook::body)
                .unwrap_or_default(),
            wheels: part(&self.wheels, loadout.wheels)
                .and_then(PartLook::wheels)
                .unwrap_or_default(),
            wing: part(&self.wings, loadout.wing)
                .and_then(PartLook::wing)
                .unwrap_or_default(),
        }
    }

    /// The kart's levels with these four parts (GD § 9).
    pub fn stats(&self, loadout: &Loadout) -> KartStats {
        KartStats::from_parts(&PartKind::ALL.map(|kind| self.points(kind, loadout.get(kind))))
    }

    /// `loadout` with the entry of `kind` moved by `step` (wrapping around the list).
    pub fn cycle(&self, loadout: Loadout, kind: PartKind, step: i32) -> Loadout {
        let n = self.count(kind).max(1) as i32;
        let mut next = loadout;
        next.set(
            kind,
            (loadout.get(kind) as i32 + step).rem_euclid(n) as usize,
        );
        next
    }

    /// Indices of the characters of `class`, in file order.
    pub fn characters_of(&self, class: WeightClass) -> impl Iterator<Item = usize> + '_ {
        self.characters
            .iter()
            .enumerate()
            .filter(move |(_, c)| c.class == class)
            .map(|(i, _)| i)
    }
}

/// Checks a list of the roster; returns the index of its default entry.
fn check_list<'a>(
    file: &str,
    default: &str,
    entries: impl Iterator<Item = (&'a str, &'a StatPoints)>,
) -> Result<usize, String> {
    let entries: Vec<(&str, &StatPoints)> = entries.collect();
    if entries.is_empty() {
        return Err(format!("{file}: empty list"));
    }
    for (i, (name, points)) in entries.iter().enumerate() {
        if name.trim().is_empty() {
            return Err(format!("{file}: entry {i} has no name"));
        }
        if entries[..i].iter().any(|(other, _)| other == name) {
            return Err(format!("{file}: {name} twice"));
        }
        if let Some(stat) = points.out_of_range() {
            return Err(format!(
                "{file}: {name}: {} points of {} (10 at most)",
                points.get(stat),
                stat.label()
            ));
        }
        // Issue #20: the Speed and the Handling on the other terrains too.
        if let Some((stat, terrain, value)) = points.terrain_out_of_range() {
            return Err(format!(
                "{file}: {name}: {value} points of {} ({}) (10 at most)",
                stat.label(),
                terrain.label()
            ));
        }
    }
    entries
        .iter()
        .position(|(name, _)| *name == default)
        .ok_or_else(|| format!("{file}: default {default:?} is not in the list"))
}

/// Whether `stats` is the middle level everywhere (what the default loadout must give), on every
/// terrain too (issue #20).
pub fn is_middle(stats: &KartStats) -> bool {
    Stat::ALL
        .iter()
        .all(|stat| stats.level(*stat) == MIDDLE_LEVEL)
        && Terrain::ALL.iter().all(|terrain| {
            stats.speed_on(*terrain) == MIDDLE_LEVEL && stats.handling_on(*terrain) == MIDDLE_LEVEL
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::{BodyLook, KartTuning, StatEffects, StatsTuning, WheelLook, WingLook};

    fn roster() -> Roster {
        Roster::parse(CHARACTERS_RON, KARTS_RON, WHEELS_RON, WINGS_RON).unwrap()
    }

    /// Issue #15 acceptance: the RON files load, with 9 animals in 3 weight classes, 3 karts,
    /// 3 sets of wheels and 2 wings. Issue #69: the roster Max chose among Kenney's Cube Pets
    /// (2026-10-15), each animal with its own model of the pack.
    #[test]
    fn the_roster_files_load_with_every_character_and_part() {
        let roster = roster();
        let names = |kind| {
            (0..roster.count(kind))
                .map(|i| roster.name(kind, i).to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(PartKind::Character),
            [
                "Poussin",
                "Pingouin",
                "Perroquet",
                "Chat",
                "Lapin",
                "Renard",
                "Cochon",
                "Vache",
                "Éléphant"
            ]
        );
        for (class, animals) in [
            (WeightClass::Light, ["Poussin", "Pingouin", "Perroquet"]),
            (WeightClass::Medium, ["Chat", "Lapin", "Renard"]),
            (WeightClass::Heavy, ["Cochon", "Vache", "Éléphant"]),
        ] {
            let found: Vec<&str> = roster
                .characters_of(class)
                .map(|i| roster.characters[i].name.as_str())
                .collect();
            assert_eq!(found, animals, "{class:?}");
        }
        assert_eq!(roster.count(PartKind::Kart), 3);
        assert_eq!(roster.count(PartKind::Wheels), 3);
        assert_eq!(roster.count(PartKind::Wing), 2);
        assert_eq!(Roster::embedded(), roster);
        // Bigger drivers for heavier classes.
        let size = |name: &str| {
            let i = roster.find(PartKind::Character, name).unwrap();
            roster.characters[i].size
        };
        // Every light one smaller than every medium one, every medium one smaller than every
        // heavy one: the weight class shows on screen.
        let sizes = |class| {
            roster
                .characters_of(class)
                .map(|i| roster.characters[i].size)
                .collect::<Vec<f32>>()
        };
        let biggest = |class| sizes(class).into_iter().fold(f32::MIN, f32::max);
        let smallest = |class| sizes(class).into_iter().fold(f32::MAX, f32::min);
        assert!(biggest(WeightClass::Light) < smallest(WeightClass::Medium));
        assert!(biggest(WeightClass::Medium) < smallest(WeightClass::Heavy));
        assert!(size("Poussin") < size("Chat") && size("Chat") < size("Éléphant"));
        // One color per animal (its primitives), one model per animal, from the Cube Pets.
        for (i, a) in roster.characters.iter().enumerate() {
            for b in &roster.characters[i + 1..] {
                assert_ne!(a.color, b.color, "{} and {}", a.name, b.name);
                assert_ne!(a.model, b.model, "{} and {}", a.name, b.name);
            }
        }
        let models: Vec<&str> = roster
            .characters
            .iter()
            .map(|c| c.model.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(
            models,
            [
                "chick", "penguin", "parrot", "cat", "bunny", "fox", "pig", "cow", "elephant"
            ]
            .map(|animal| format!("cube-pets/animal-{animal}"))
        );
    }

    /// Issue #69: the stats of each weight class stay within the class's bounds, and the
    /// heaviest of all is the elephant, the lightest the chick.
    #[test]
    fn each_weight_class_keeps_its_bounds() {
        let roster = roster();
        for c in &roster.characters {
            let s = |stat| c.stats.get(stat);
            let (speed, weight, acceleration) =
                (s(Stat::Speed), s(Stat::Weight), s(Stat::Acceleration));
            let ok = match c.class {
                WeightClass::Light => speed <= 3 && weight <= 2 && acceleration >= 5,
                WeightClass::Medium => {
                    (3..=5).contains(&speed) && (3..=4).contains(&weight) && acceleration >= 3
                }
                WeightClass::Heavy => speed >= 5 && weight >= 7 && acceleration <= 3,
            };
            assert!(ok, "{}: {:?}", c.name, c.stats);
            assert!(Stat::ALL.iter().all(|stat| s(*stat) <= 8), "{}", c.name);
            assert!(
                Stat::ALL
                    .iter()
                    .map(|stat| u32::from(s(*stat)))
                    .sum::<u32>()
                    <= 32,
                "{}: more points than the others",
                c.name
            );
        }
        let weight = |name: &str| {
            let i = roster.find(PartKind::Character, name).unwrap();
            roster.characters[i].stats.get(Stat::Weight)
        };
        for c in &roster.characters {
            assert!(
                weight("Éléphant") >= c.stats.get(Stat::Weight),
                "{}",
                c.name
            );
            assert!(weight("Poussin") <= c.stats.get(Stat::Weight), "{}", c.name);
        }
    }

    /// Issue #15: the default loadout (a medium character with the standard kart, wheels and
    /// wing) is the middle level in every stat, so it drives exactly like `KartTuning`.
    #[test]
    fn the_default_loadout_is_the_middle_level_everywhere() {
        let roster = roster();
        let default = roster.default_loadout();
        assert_eq!(roster.character(&default).name, "Chat");
        assert_eq!(roster.character(&default).class, WeightClass::Medium);
        assert_eq!(roster.name(PartKind::Kart, default.kart), "Caisse à savon");
        assert_eq!(
            roster.name(PartKind::Wheels, default.wheels),
            "Pneus guimauve"
        );
        assert_eq!(roster.name(PartKind::Wing, default.wing), "Taie d'oreiller");
        let stats = roster.stats(&default);
        assert!(is_middle(&stats), "{stats:?}");
        let effects = StatEffects::new(&stats, &StatsTuning::default());
        assert!(effects.is_neutral());
        assert_eq!(effects.top_speed, 1.0);
        assert_eq!(effects.apply(&KartTuning::default()), KartTuning::default());
        assert!(is_middle(&Roster::neutral().stats(&Loadout::default())));
    }

    /// GD § 9: light characters accelerate and handle better, heavy ones are faster and
    /// heavier; each light or heavy part trades the same way against the standard one.
    #[test]
    fn the_weight_classes_and_parts_trade_off_clearly() {
        let roster = roster();
        let average = |class, stat| {
            let all: Vec<u8> = roster
                .characters_of(class)
                .map(|i| roster.characters[i].stats.get(stat))
                .collect();
            all.iter().map(|v| *v as f32).sum::<f32>() / all.len() as f32
        };
        use WeightClass::*;
        for (better, worse, stat) in [
            (Heavy, Medium, Stat::Speed),
            (Medium, Light, Stat::Speed),
            (Heavy, Medium, Stat::Weight),
            (Medium, Light, Stat::Weight),
            (Light, Medium, Stat::Acceleration),
            (Medium, Heavy, Stat::Acceleration),
            (Light, Medium, Stat::Handling),
            (Medium, Heavy, Stat::Handling),
            (Light, Heavy, Stat::MiniTurbo),
        ] {
            assert!(
                average(better, stat) > average(worse, stat),
                "{better:?} vs {worse:?}, {stat:?}"
            );
        }
        // Every part that is not the standard one gives up something for something else.
        let default = roster.default_loadout();
        for kind in [PartKind::Kart, PartKind::Wheels, PartKind::Wing] {
            let standard = roster.points(kind, default.get(kind));
            for i in (0..roster.count(kind)).filter(|i| *i != default.get(kind)) {
                let part = roster.points(kind, i);
                let gains = Stat::ALL
                    .iter()
                    .filter(|s| part.get(**s) > standard.get(**s));
                let losses = Stat::ALL
                    .iter()
                    .filter(|s| part.get(**s) < standard.get(**s));
                assert!(
                    gains.count() > 0 && losses.count() > 0,
                    "{}",
                    roster.name(kind, i)
                );
            }
        }
    }

    #[test]
    fn a_loadout_cycles_through_each_list() {
        let roster = roster();
        let start = roster.default_loadout();
        let next = roster.cycle(start, PartKind::Wing, 1);
        assert_ne!(next.wing, start.wing);
        assert_eq!(roster.cycle(next, PartKind::Wing, 1).wing, start.wing);
        let back = roster.cycle(start, PartKind::Character, -1);
        assert_eq!(back.character, start.character - 1);
        let first = Loadout::default();
        assert_eq!(roster.cycle(first, PartKind::Character, -1).character, 8);
        assert_eq!(next.character, start.character);
        assert_eq!(
            roster.name(PartKind::Kart, 99),
            roster.name(PartKind::Kart, 2)
        );
    }

    #[test]
    fn broken_roster_files_are_refused_with_a_reason() {
        let part = |name: &str, points: u8| {
            format!(
                "(name: {name:?}, stats: (speed: {points}, acceleration: 1, weight: 1, \
                 handling: 1, traction: 1, mini_turbo: 1, invincibility: 1))"
            )
        };
        let list = |default: &str, parts: &[String]| {
            format!("(default: {default:?}, parts: [{}])", parts.join(","))
        };
        let ok = list("A", &[part("A", 1)]);
        let parse = |karts: &str| Roster::parse(CHARACTERS_RON, karts, &ok, &ok);
        assert!(parse(&ok).is_ok());
        let errors = [
            parse(&list("A", &[part("A", 11)])),
            parse(&list("B", &[part("A", 1)])),
            parse(&list("A", &[part("A", 1), part("A", 2)])),
            parse(&list("A", &[])),
            parse("(default: \"A\", parts: [], colour: 1)"),
            parse("pas du RON"),
        ];
        for error in errors {
            let message = error.unwrap_err();
            assert!(message.starts_with("karts.ron"), "{message}");
        }
        let bad_size = CHARACTERS_RON.replacen("size: 0.78", "size: 3.0", 1);
        assert!(Roster::parse(&bad_size, &ok, &ok, &ok).is_err());
        // Issue #69: a model must be named "pack/name".
        for model in ["animal-cat", "cube-pets/animal cat", "cube-pets/x/y", ""] {
            let bad = CHARACTERS_RON.replacen("\"cube-pets/animal-cat\"", &format!("{model:?}"), 1);
            let error = Roster::parse(&bad, &ok, &ok, &ok).unwrap_err();
            assert!(error.contains("Chat: model"), "{error}");
        }
        // Without a model, the driver stands in primitives.
        let without = CHARACTERS_RON.replacen("model: \"cube-pets/animal-cat\",", "", 1);
        let roster = Roster::parse(&without, &ok, &ok, &ok).unwrap();
        assert_eq!(roster.characters[3].model, None);
        // Issue #20: a terrain's points are checked too, and written without `Some(...)`.
        let terrain = |field: &str, points: u8| {
            list(
                "A",
                &[part("A", 1).replace(
                    "invincibility: 1)",
                    &format!("invincibility: 1, {field}: {points})"),
                )],
            )
        };
        assert!(parse(&terrain("air_speed", 10)).is_ok());
        let error = parse(&terrain("water_handling", 11)).unwrap_err();
        assert!(error.contains("Maniabilité (Eau)"), "{error}");
        assert!(parse(&terrain("air_speeed", 1)).is_err());
        // Issue #73: a look of its kind, written without `Some(...)`; left out, the kart of
        // before's.
        let looking = |look: &str| {
            list(
                "A",
                &[part("A", 1).replace("))", &format!("), look: {look})"))],
            )
        };
        let roster = parse(&looking("Bathtub")).unwrap();
        assert_eq!(roster.karts[0].look, Some(PartLook::Bathtub));
        assert_eq!(roster.looks(&Loadout::default()).body, BodyLook::Bathtub);
        for wrong in ["Tractor", "Feather"] {
            let error = parse(&looking(wrong)).unwrap_err();
            assert!(
                error.starts_with("karts.ron: A: look") && error.contains("another kind"),
                "{error}"
            );
        }
        assert!(parse(&looking("Baignoire")).is_err());
        let error = Roster::parse(CHARACTERS_RON, &ok, &looking("SoapBox"), &ok).unwrap_err();
        assert!(error.starts_with("wheels.ron: A: look SoapBox"), "{error}");
        let error = Roster::parse(CHARACTERS_RON, &ok, &ok, &looking("Spools")).unwrap_err();
        assert!(error.starts_with("wings.ron: A: look Spools"), "{error}");
        assert!(
            Roster::parse(
                CHARACTERS_RON,
                &ok,
                &looking("Tractor"),
                &looking("Feather")
            )
            .is_ok()
        );
        let plain = parse(&ok).unwrap();
        assert_eq!(plain.karts[0].look, None);
        assert_eq!(plain.looks(&Loadout::default()), PartLooks::default());
    }

    /// Issue #73: every part of the files has its own look, of its kind: the three bodies, the
    /// three sets of wheels and the two wings are all different, each the one its name says; the
    /// default loadout looks like the kart of before (the Car Kit's kart, its wheels, the
    /// pillowcase).
    #[test]
    fn every_part_has_its_own_look() {
        let roster = roster();
        let looks = |list: &[PartData]| -> Vec<PartLook> {
            list.iter()
                .map(|p| p.look.unwrap_or_else(|| panic!("{}: no look", p.name)))
                .collect()
        };
        use PartLook::*;
        assert_eq!(looks(&roster.karts), [WickerBasket, SoapBox, Bathtub]);
        assert_eq!(looks(&roster.wheels), [Spools, Marshmallow, Tractor]);
        assert_eq!(looks(&roster.wings), [Pillowcase, Feather]);
        let named = |kind, name: &str| {
            let mut loadout = roster.default_loadout();
            loadout.set(kind, roster.find(kind, name).unwrap());
            roster.looks(&loadout)
        };
        assert_eq!(
            roster.looks(&roster.default_loadout()),
            PartLooks::default()
        );
        assert_eq!(
            named(PartKind::Kart, "Panier en osier").body,
            BodyLook::WickerBasket
        );
        assert_eq!(named(PartKind::Kart, "Baignoire").body, BodyLook::Bathtub);
        assert_eq!(
            named(PartKind::Wheels, "Bobines de fil").wheels,
            WheelLook::Spools
        );
        assert_eq!(
            named(PartKind::Wheels, "Pneus de tracteur").wheels,
            WheelLook::Tractor
        );
        assert_eq!(named(PartKind::Wing, "Plume").wing, WingLook::Feather);
        // Every combination is its own look.
        let mut all = std::collections::BTreeSet::new();
        for kart in 0..3 {
            for wheels in 0..3 {
                for wing in 0..2 {
                    let loadout = Loadout {
                        character: 0,
                        kart,
                        wheels,
                        wing,
                    };
                    all.insert(format!("{:?}", roster.looks(&loadout)));
                }
            }
        }
        assert_eq!(all.len(), 18);
        // Out of the lists: the last one, like the stats.
        let far = Loadout {
            character: 0,
            kart: 99,
            wheels: 99,
            wing: 99,
        };
        assert_eq!(
            roster.looks(&far),
            PartLooks {
                body: BodyLook::Bathtub,
                wheels: WheelLook::Tractor,
                wing: WingLook::Feather
            }
        );
    }

    /// Issue #20 (GD § 9): every part of the files gives its Speed and its Handling on each
    /// terrain (none left to the ground's by default); the default loadout is the middle level on
    /// every terrain (checked by `is_middle` above); some parts are made for a terrain: the
    /// bathtub and the penguin swim, the feather and the parrot glide.
    #[test]
    fn every_part_has_its_terrains() {
        let roster = roster();
        for kind in PartKind::ALL {
            for i in 0..roster.count(kind) {
                let p = roster.points(kind, i);
                let name = roster.name(kind, i);
                for value in [
                    p.water_speed,
                    p.water_handling,
                    p.air_speed,
                    p.air_handling,
                    p.anti_gravity_speed,
                    p.anti_gravity_handling,
                ] {
                    assert!(value.is_some(), "{name}: {p:?}");
                }
            }
        }
        let with = |kind, name: &str| {
            let mut loadout = roster.default_loadout();
            loadout.set(kind, roster.find(kind, name).unwrap());
            roster.stats(&loadout)
        };
        let middle = roster.stats(&roster.default_loadout());
        for terrain in Terrain::ALL {
            assert_eq!(middle.speed_on(terrain), MIDDLE_LEVEL);
            assert_eq!(middle.handling_on(terrain), MIDDLE_LEVEL);
        }
        let bathtub = with(PartKind::Kart, "Baignoire");
        assert!(bathtub.speed_on(Terrain::Water) > bathtub.speed_on(Terrain::Ground));
        let penguin = with(PartKind::Character, "Pingouin");
        assert!(penguin.speed_on(Terrain::Water) > penguin.speed_on(Terrain::Ground));
        assert!(penguin.speed_on(Terrain::Air) < penguin.speed_on(Terrain::Ground));
        let feather = with(PartKind::Wing, "Plume");
        assert!(feather.speed_on(Terrain::Air) > feather.speed_on(Terrain::Ground));
        assert!(feather.handling_on(Terrain::Air) > feather.handling_on(Terrain::Ground));
        let parrot = with(PartKind::Character, "Perroquet");
        assert!(parrot.handling_on(Terrain::Air) > parrot.handling_on(Terrain::Ground));
    }
}
