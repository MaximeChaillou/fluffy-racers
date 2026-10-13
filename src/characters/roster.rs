//! The roster (GD § 9, issue #15): characters, karts, wheels and wings, as written in the RON
//! files of `assets/data/`. Pure logic, no ECS: parsing, checks, loadouts. Unit-tested.
//!
//! The files are compiled into the game (`include_str!`, like the circuit manifest of #36): they
//! are there before the first kart spawns, with no request that could fail and no folder to
//! read (impossible in a browser). A test reads them and checks every rule below.

use bevy::prelude::Resource;
use serde::Deserialize;

use crate::kart::{DriverEars, KartStats, MIDDLE_LEVEL, Stat, StatPoints};

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
    /// Color of the driver (red, green, blue, 0 to 1).
    pub color: (f32, f32, f32),
    pub ears: DriverEars,
    pub stats: StatPoints,
}

/// A kart, a set of wheels or a wing, as written in `karts.ron`, `wheels.ron`, `wings.ron`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PartData {
    pub name: String,
    pub stats: StatPoints,
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
        };
        Self {
            characters: vec![CharacterData {
                name: "Chat".to_string(),
                class: WeightClass::Medium,
                size: 1.0,
                color: (0.95, 0.65, 0.35),
                ears: DriverEars::Pointy,
                stats: StatPoints::uniform(4),
            }],
            karts: vec![part("Caisse à savon", 3)],
            wheels: vec![part("Pneus guimauve", 2)],
            wings: vec![part("Taie d'oreiller", 1)],
            default: Loadout::default(),
        }
    }

    /// Reads and checks the four files: every list has at least one entry, no name twice in a
    /// list, at most 10 points per stat, a driver size and color that make sense, and a
    /// `default` that names an entry of its list.
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
    }
    entries
        .iter()
        .position(|(name, _)| *name == default)
        .ok_or_else(|| format!("{file}: default {default:?} is not in the list"))
}

/// Whether `stats` is the middle level everywhere (what the default loadout must give).
pub fn is_middle(stats: &KartStats) -> bool {
    Stat::ALL
        .iter()
        .all(|stat| stats.level(*stat) == MIDDLE_LEVEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::{KartTuning, StatEffects, StatsTuning};

    fn roster() -> Roster {
        Roster::parse(CHARACTERS_RON, KARTS_RON, WHEELS_RON, WINGS_RON).unwrap()
    }

    /// Issue #15 acceptance: the RON files load, with 9 animals in 3 weight classes, 3 karts,
    /// 3 sets of wheels and 2 wings.
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
                "Souris", "Poussin", "Hamster", "Chat", "Lapin", "Renard", "Ours", "Morse", "Vache"
            ]
        );
        for (class, animals) in [
            (WeightClass::Light, ["Souris", "Poussin", "Hamster"]),
            (WeightClass::Medium, ["Chat", "Lapin", "Renard"]),
            (WeightClass::Heavy, ["Ours", "Morse", "Vache"]),
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
        assert!(size("Souris") < size("Chat") && size("Chat") < size("Ours"));
        // One color per animal.
        for (i, a) in roster.characters.iter().enumerate() {
            for b in &roster.characters[i + 1..] {
                assert_ne!(a.color, b.color, "{} and {}", a.name, b.name);
            }
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
    }
}
