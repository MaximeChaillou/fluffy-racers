//! Issue #157: the **fingerprint** of a circuit, a 64-bit number computed from everything that
//! makes it, so that the time trial's records (`crate::modes::trial`) start from zero as soon as
//! a circuit changes, even only its decor. Pure logic: unit-tested.
//!
//! - **What it is made of**:
//!   - **its file** (`assets/tracks/*.track.ron`), its whole text: the layout, the road, the
//!     ramps, the zones, the shortcuts, the dangers, the croquettes and the boxes, the pieces
//!     placed by hand, the decor's seed, the name of its biome (its comments and its music line
//!     too: simpler and stricter than a choice of fields);
//!   - **the data of its biome** in `assets/data/biomes.ron` ([`BiomeData`]): its name, its
//!     colors and its light, its rows and its scatter of pieces (their models by name, how many,
//!     how big, where), its far decor (#146), its road (#187) and its terrain (#195). Not its
//!     music (Max: « la musique non »), nor the other biomes.
//! - **What it is not made of** (see `STATUS.md`, « Décisions », #157): the manifest
//!   (`assets/tracks/index.ron`: the names in the menus and the cups, not the circuit; Max: « le
//!   manifeste non »), the music (`assets/data/music.ron`), the models' files (`.glb`: shared by
//!   every circuit of their pack, loaded after the circuit is built; the circuit names them, and
//!   their names are in the fingerprint), the karts, the items and the tunings (the same on every
//!   circuit), and the game's code.
//! - **The same on every machine and in the browser**: FNV-1a on 64 bits ([`Fnv1a`], written
//!   here: no crate), never `DefaultHasher` (its algorithm may change between versions of Rust)
//!   nor `RandomState` (a random key per run). Every number is written in little-endian bytes of
//!   a fixed size (no `usize`: 32 bits in wasm, 64 natively), every float by its bits, every text
//!   with its length; the line ends of the circuit's file are read as `\n` (a checkout on
//!   Windows may give `\r\n`).
//! - **Computed once, when the circuit's file is read** (`TrackData::from_ron`, so the asset
//!   loader too): nothing during the race.

use super::biome::{
    BiomeData, Border, Palette, Paving, PropRow, Rgb, RoadLook, ScatterGroup, Side,
};
use super::far::{FarDecor, FarLayer};
use super::terrain::TerrainData;

/// The version of what is hashed: the first bytes of every fingerprint. To change only if the
/// encoding below changes (every record then starts from zero, which is fine: Max, #157).
/// Issue #146: version 2, the biome's far decor hashed too (every biome got one: every record
/// started from zero anyway). Issue #187: version 3, the biome's road (its paving, its borders)
/// hashed too (every biome got one, and new colors for its road: every record started from zero
/// anyway).
const ENCODING: &[u8] = b"fluffy-racers circuit fingerprint 3\n";

/// FNV-1a, 64 bits (the parameters of its authors: offset basis and prime).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv1a {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub const fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(Self::PRIME);
        }
    }

    pub fn finish(&self) -> u64 {
        self.0
    }

    fn u8(&mut self, value: u8) {
        self.write(&[value]);
    }

    fn u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    /// A text, its length first (two texts in a row never read as one).
    fn str(&mut self, text: &str) {
        self.u64(text.len() as u64);
        self.write(text.as_bytes());
    }

    fn rgb(&mut self, (r, g, b): Rgb) {
        self.f32(r);
        self.f32(g);
        self.f32(b);
    }

    fn models(&mut self, models: &[String]) {
        self.u64(models.len() as u64);
        for model in models {
            self.str(model);
        }
    }

    fn side(&mut self, side: Side) {
        self.u8(match side {
            Side::Both => 0,
            Side::Inside => 1,
            Side::Outside => 2,
            Side::Left => 3,
            Side::Right => 4,
        });
    }
}

/// FNV-1a 64 of `bytes`.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hasher = Fnv1a::new();
    hasher.write(bytes);
    hasher.finish()
}

/// The text with its line ends as `\n`: `\r\n` and a lone `\r` both give `\n`.
pub fn canonical_text(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\r' {
            bytes.next_if_eq(&b'\n');
            out.push(b'\n');
        } else {
            out.push(byte);
        }
    }
    out
}

/// The fingerprint of a circuit: the text of its file (`track_text`) and the data of its biome
/// (`biome`: none for a circuit without one, or naming one the game does not have; such a file is
/// refused by `TrackData::validate` anyway).
pub fn circuit_fingerprint(track_text: &str, biome: Option<&BiomeData>) -> u64 {
    let mut hasher = Fnv1a::new();
    hasher.write(ENCODING);
    let text = canonical_text(track_text);
    hasher.u64(text.len() as u64);
    hasher.write(&text);
    match biome {
        None => hasher.u8(0),
        Some(biome) => {
            hasher.u8(1);
            write_biome(&mut hasher, biome);
        }
    }
    hasher.finish()
}

/// Every field of the biome but its music, in a fixed order. Each struct is taken apart field by
/// field: a field added to one of them does not compile until it is hashed here (or said why
/// not).
fn write_biome(hasher: &mut Fnv1a, biome: &BiomeData) {
    let BiomeData {
        name,
        palette,
        rows,
        scatter,
        // The music: not part of the circuit (Max, #157).
        music: _,
        far,
        road,
        terrain,
    } = biome;
    hasher.str(name);
    let Palette {
        sky,
        ambient,
        ambient_brightness,
        ground,
        asphalt,
        curbs,
        shoulder,
        sand,
        walls,
        wall_top,
        skirt,
    } = *palette;
    for rgb in [
        sky, ambient, ground, asphalt, curbs.0, curbs.1, shoulder, sand, walls.0, walls.1,
        wall_top, skirt,
    ] {
        hasher.rgb(rgb);
    }
    hasher.f32(ambient_brightness);
    hasher.u64(rows.len() as u64);
    for row in rows {
        let PropRow {
            models,
            spacing,
            offset,
            height,
            radius,
            gap,
            side,
            yaw,
            shadows,
        } = row;
        hasher.models(models);
        for value in [*spacing, *offset, *height, *radius, *gap, *yaw] {
            hasher.f32(value);
        }
        hasher.side(*side);
        hasher.bool(*shadows);
    }
    hasher.u64(scatter.len() as u64);
    for group in scatter {
        let ScatterGroup {
            models,
            count,
            height,
            radius,
            distance,
            side,
            shadows,
        } = group;
        hasher.models(models);
        hasher.u32(*count);
        for value in [height.0, height.1, *radius, distance.0, distance.1] {
            hasher.f32(value);
        }
        hasher.side(*side);
        hasher.bool(*shadows);
    }
    // Issue #146: its far decor, after the rest.
    let FarDecor { fog, layers } = far;
    hasher.f32(fog.0);
    hasher.f32(fog.1);
    hasher.u64(layers.len() as u64);
    for layer in layers {
        write_far_layer(hasher, layer);
    }
    // Issue #187: its road, after the rest.
    write_road(hasher, road);
    // Issue #195: its terrain, last of all, and only when it has one: nothing follows it, so a
    // biome without a terrain hashes exactly as in encoding 3 (the value written down in the
    // tests stays), and every biome of the game, which has one, gets a new fingerprint (every
    // record starts from zero: the ground off the road is solid where it rises).
    if let Some(terrain) = terrain {
        write_terrain(hasher, terrain);
    }
}

/// A terrain (issue #195): every field in the order of its declaration, the snow `None` as a 0
/// and `Some` as a 1 before it.
fn write_terrain(hasher: &mut Fnv1a, terrain: &TerrainData) {
    let TerrainData {
        height,
        scale,
        ridges,
        cliffs,
        terrace,
        floor,
        clear,
        rise,
        rock,
        steep,
        snow,
    } = *terrain;
    hasher.u8(1);
    for value in [height, scale, ridges, cliffs, terrace, floor, clear, rise] {
        hasher.f32(value);
    }
    hasher.rgb(rock);
    hasher.f32(steep);
    match snow {
        None => hasher.u8(0),
        Some((color, at)) => {
            hasher.u8(1);
            hasher.rgb(color);
            hasher.f32(at);
        }
    }
}

/// A road's look (issue #187): its paving and its border (their place in `Paving::ALL` and
/// `Border::ALL`), then its colors, each `None` as a 0 and `Some` as a 1 before it.
fn write_road(hasher: &mut Fnv1a, road: &RoadLook) {
    let RoadLook {
        paving,
        border,
        accent,
        asphalt,
        curbs,
    } = *road;
    let place = |all: &[bool]| all.iter().position(|b| *b).unwrap_or(0) as u8;
    hasher.u8(place(&Paving::ALL.map(|p| p == paving)));
    hasher.u8(place(&Border::ALL.map(|b| b == border)));
    for color in [accent, asphalt] {
        match color {
            None => hasher.u8(0),
            Some(rgb) => {
                hasher.u8(1);
                hasher.rgb(rgb);
            }
        }
    }
    match curbs {
        None => hasher.u8(0),
        Some((a, b)) => {
            hasher.u8(1);
            hasher.rgb(a);
            hasher.rgb(b);
        }
    }
}

/// A layer of a far decor (issue #146): its kind, then every field in the order of its
/// declaration.
fn write_far_layer(hasher: &mut Fnv1a, layer: &FarLayer) {
    let top = |hasher: &mut Fnv1a, top: &Option<(Rgb, f32)>| match top {
        None => hasher.u8(0),
        Some((color, at)) => {
            hasher.u8(1);
            hasher.rgb(*color);
            hasher.f32(*at);
        }
    };
    match layer {
        FarLayer::Sea { color, from, beach } => {
            hasher.u8(0);
            hasher.rgb(*color);
            hasher.f32(*from);
            hasher.f32(*beach);
        }
        FarLayer::Range {
            distance,
            depth,
            height,
            spacing,
            color,
            top: t,
        } => {
            hasher.u8(1);
            for value in [*distance, *depth, height.0, height.1, *spacing] {
                hasher.f32(value);
            }
            hasher.rgb(*color);
            top(hasher, t);
        }
        FarLayer::Peaks {
            count,
            distance,
            height,
            radius,
            flat,
            sides,
            color,
            top: t,
        } => {
            hasher.u8(2);
            hasher.u32(*count);
            for value in [
                distance.0, distance.1, height.0, height.1, radius.0, radius.1, *flat,
            ] {
                hasher.f32(value);
            }
            hasher.u32(*sides);
            hasher.rgb(*color);
            top(hasher, t);
        }
        FarLayer::Clouds {
            count,
            distance,
            altitude,
            size,
            color,
        } => {
            hasher.u8(3);
            hasher.u32(*count);
            for value in [
                distance.0, distance.1, altitude.0, altitude.1, size.0, size.1,
            ] {
                hasher.f32(value);
            }
            hasher.rgb(*color);
        }
        FarLayer::Pieces {
            models,
            count,
            height,
            radius,
            distance,
            side,
        } => {
            hasher.u8(4);
            hasher.models(models);
            hasher.u32(*count);
            for value in [height.0, height.1, *radius, distance.0, distance.1] {
                hasher.f32(value);
            }
            hasher.side(*side);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::tracks::circuits;
    use crate::track::TrackData;
    use crate::track::biome::{BIOMES_RON, biome, parse_biomes};

    /// The text of a circuit of the game, read from its file.
    fn circuit_text(asset_path: &str) -> String {
        let path = format!("{}/assets/{asset_path}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
    }

    /// The same on every machine: the published values of FNV-1a 64 (its authors' test
    /// vectors), and a fingerprint written down once, of a known text and a known biome. Only
    /// integer arithmetic on bytes of a fixed order and size: what native and wasm compute alike.
    /// Should this value change, every record of the players starts from zero: change the
    /// encoding only on purpose (`ENCODING`). Issue #146: encoding 2 (the far decor), the values
    /// written anew, and checked again by the second implementation.
    #[test]
    fn the_fingerprint_of_a_known_circuit_is_written_down() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a_64(b"foobar"), 0x8594_4171_f739_67e8);
        let mut parts = Fnv1a::new();
        parts.write(b"foo");
        parts.write(b"bar");
        assert_eq!(parts.finish(), fnv1a_64(b"foobar"));

        let text = "(\n    name: \"Essai\",\n    biome: \"Essai\",\n)\n";
        let biome = BiomeData {
            name: "Essai".into(),
            // Written out (not `Palette::default()`, which may change): only the encoding moves
            // this value.
            palette: Palette {
                sky: (0.55, 0.80, 0.95),
                ambient: (1.0, 1.0, 1.0),
                ambient_brightness: 600.0,
                ground: (0.45, 0.75, 0.35),
                asphalt: (0.42, 0.43, 0.48),
                curbs: ((0.85, 0.20, 0.20), (0.95, 0.95, 0.95)),
                shoulder: (0.40, 0.70, 0.32),
                sand: (0.90, 0.80, 0.55),
                walls: ((0.72, 0.68, 0.90), (0.72, 0.68, 0.90)),
                wall_top: (0.95, 0.95, 0.95),
                skirt: (0.55, 0.45, 0.35),
            },
            rows: vec![PropRow {
                models: vec!["racing-kit/flagRed".into()],
                spacing: 36.0,
                offset: 18.0,
                height: 4.5,
                radius: 1.0,
                gap: 0.6,
                side: Side::Outside,
                yaw: 0.0,
                shadows: true,
            }],
            scatter: vec![ScatterGroup {
                models: vec!["nature-kit/tree_oak".into(), "nature-kit/rock_tallA".into()],
                count: 12,
                height: (14.0, 20.0),
                radius: 3.0,
                distance: (4.0, 40.0),
                side: Side::Both,
                shadows: false,
            }],
            music: Some("pure-raceway".into()),
            // Issue #146: one layer of each kind.
            far: FarDecor {
                fog: (220.0, 1100.0),
                layers: vec![
                    FarLayer::Sea {
                        color: (0.2, 0.5, 0.75),
                        from: 90.0,
                        beach: 20.0,
                    },
                    FarLayer::Range {
                        distance: 260.0,
                        depth: 140.0,
                        height: (60.0, 130.0),
                        spacing: 34.0,
                        color: (0.45, 0.5, 0.55),
                        top: Some(((0.97, 0.98, 1.0), 95.0)),
                    },
                    FarLayer::Peaks {
                        count: 9,
                        distance: (60.0, 200.0),
                        height: (40.0, 90.0),
                        radius: (25.0, 50.0),
                        flat: 0.5,
                        sides: 7,
                        color: (0.8, 0.45, 0.3),
                        top: None,
                    },
                    FarLayer::Clouds {
                        count: 14,
                        distance: (100.0, 600.0),
                        altitude: (90.0, 160.0),
                        size: (40.0, 90.0),
                        color: (1.0, 1.0, 1.0),
                    },
                    FarLayer::Pieces {
                        models: vec!["nature-kit/tree_oak".into()],
                        count: 40,
                        height: (18.0, 30.0),
                        radius: 6.0,
                        distance: (70.0, 180.0),
                        side: Side::Outside,
                    },
                ],
            },
            // Issue #187: a road with an accent.
            road: RoadLook {
                paving: Paving::Planks,
                border: Border::Logs,
                accent: Some((0.6, 0.45, 0.3)),
                asphalt: None,
                curbs: None,
            },
            // Issue #195: no terrain, hashed as nothing (the values below, of encoding 3, stay).
            terrain: None,
        };
        // Checked by a second implementation of the encoding, outside the game (Python: the
        // same bytes, `struct.pack('<f')` for the floats): the same values. Issue #187: encoding
        // 3 (the road), written anew and checked again the same way.
        assert_eq!(
            circuit_fingerprint(text, Some(&biome)),
            0xbb4b_dd31_3fbe_51be,
            "{:#018x}",
            circuit_fingerprint(text, Some(&biome))
        );
        assert_eq!(
            circuit_fingerprint(text, None),
            0x6d40_754e_a452_87ad,
            "{:#018x}",
            circuit_fingerprint(text, None)
        );
        // Line ends and the biome's music do not count.
        let mut music = biome.clone();
        music.music = None;
        assert_eq!(
            circuit_fingerprint(&text.replace('\n', "\r\n"), Some(&music)),
            0xbb4b_dd31_3fbe_51be
        );
    }

    /// Issue #157: nothing changed, the same fingerprint: read twice, or with the line ends of
    /// another checkout (`\r\n`), or mirrored (the mirror is the record's other key). Every
    /// circuit of the game has its own.
    #[test]
    fn an_unchanged_circuit_keeps_its_fingerprint() {
        let mut seen: Vec<(String, u64)> = Vec::new();
        for entry in circuits() {
            let path = entry.asset_path();
            let text = circuit_text(&path);
            let data = TrackData::from_ron(&text).unwrap();
            let biome = data.biome.as_deref().and_then(biome);
            assert!(biome.is_some(), "{path}: a circuit of the game has a biome");
            assert_eq!(
                data.fingerprint,
                circuit_fingerprint(&text, biome),
                "{path}"
            );
            assert_eq!(
                TrackData::from_ron(&text).unwrap().fingerprint,
                data.fingerprint,
                "{path}: read again"
            );
            let windows = text.replace("\r\n", "\n").replace('\n', "\r\n");
            assert_ne!(windows, text);
            assert_eq!(
                TrackData::from_ron(&windows).unwrap().fingerprint,
                data.fingerprint,
                "{path}: \\r\\n"
            );
            assert_eq!(data.mirrored().fingerprint, data.fingerprint, "{path}");
            assert!(
                seen.iter().all(|(_, other)| *other != data.fingerprint),
                "{path} has the fingerprint of another circuit: {seen:?}"
            );
            seen.push((path, data.fingerprint));
        }
        assert!(seen.len() >= 7, "{seen:?}");
        assert_eq!(canonical_text("a\r\nb\rc\n"), b"a\nb\nc\n");
    }

    /// Issue #157 acceptance: changing a single piece of decor of a circuit changes its
    /// fingerprint: a piece placed by hand in its file (its model, or only where it stands), the
    /// decor's seed, a piece of its biome (its model, one piece more, its size), a color of its
    /// biome; issue #146: its far decor (the fog, a range, a piece, a sea). Not the biome's music
    /// (Max: « la musique non »), nor another biome.
    #[test]
    fn changing_one_piece_of_decor_changes_the_fingerprint() {
        let text = circuit_text("tracks/prairie.track.ron");
        let data = TrackData::from_ron(&text).unwrap();
        let fingerprint = data.fingerprint;
        let changed = |text: &str| TrackData::from_ron(text).unwrap().fingerprint;

        // A piece placed by hand: another model, or the same one a little further.
        let prop = &data.props[0];
        let model = format!("model: \"{}\"", prop.model);
        assert!(text.contains(&model));
        let other = data
            .props
            .iter()
            .find(|p| p.model != prop.model)
            .unwrap()
            .model
            .clone();
        let swapped = text.replacen(&model, &format!("model: \"{other}\""), 1);
        assert_ne!(TrackData::from_ron(&swapped).unwrap().props, data.props);
        assert_ne!(changed(&swapped), fingerprint, "another model");
        let at = format!("at: {:?}", prop.at);
        let start = text.find(&model).unwrap();
        let at_start = start + text[start..].find(&at).expect("its place after its model");
        let mut moved = text.clone();
        moved.replace_range(
            at_start..at_start + at.len(),
            &format!("at: {:?}", prop.at + 0.01),
        );
        assert_ne!(TrackData::from_ron(&moved).unwrap().props, data.props);
        assert_ne!(changed(&moved), fingerprint, "moved");
        // The decor's seed (the biome's pieces elsewhere).
        let seed = format!("decor_seed: {}", data.decor_seed);
        assert!(text.contains(&seed));
        let reseeded = text.replace(&seed, &format!("decor_seed: {}", data.decor_seed + 1));
        assert_ne!(changed(&reseeded), fingerprint, "the decor's seed");

        // Its biome's data: one change at a time.
        let biomes = parse_biomes(BIOMES_RON).unwrap();
        let prairie = biomes.iter().find(|b| b.name == "Prairie").unwrap();
        assert_eq!(data.biome.as_deref(), Some("Prairie"));
        assert_eq!(circuit_fingerprint(&text, Some(prairie)), fingerprint);
        let with = |change: &dyn Fn(&mut BiomeData)| {
            let mut biome = prairie.clone();
            change(&mut biome);
            assert_ne!(&biome, prairie);
            circuit_fingerprint(&text, Some(&biome))
        };
        let other_model = biomes
            .iter()
            .find(|b| b.name != "Prairie")
            .unwrap()
            .models()[0]
            .to_string();
        for (what, change) in [
            (
                "a model of its scatter",
                &(|b: &mut BiomeData| b.scatter[0].models[0] = other_model.clone())
                    as &dyn Fn(&mut BiomeData),
            ),
            ("one piece more", &|b| b.scatter[0].count += 1),
            ("a piece taller", &|b| b.scatter[0].height.1 += 0.5),
            ("a row's spacing", &|b| b.rows[0].spacing += 1.0),
            ("a row's side", &|b| b.rows[0].side = Side::Inside),
            ("no shadow", &|b| {
                b.scatter[0].shadows = !b.scatter[0].shadows
            }),
            ("the sky's color", &|b| b.palette.sky.2 -= 0.01),
            ("the light", &|b| b.palette.ambient_brightness += 1.0),
            // Issue #187: its road's look.
            ("the road's paving", &|b| b.road.paving = Paving::Cobbles),
            ("the road's border", &|b| b.road.border = Border::Candies),
            ("the road's accent", &|b| {
                b.road.accent = Some((0.5, 0.5, 0.5))
            }),
            // Issue #146: its far decor.
            ("the fog", &|b| b.far.fog.1 += 10.0),
            ("a far range higher", &|b| {
                for layer in &mut b.far.layers {
                    if let FarLayer::Range { height, .. } = layer {
                        height.1 += 1.0;
                    }
                }
            }),
            ("a far piece more", &|b| {
                for layer in &mut b.far.layers {
                    if let FarLayer::Pieces { count, .. } = layer {
                        *count += 1;
                    }
                }
            }),
            // Issue #195: its terrain.
            ("its terrain higher", &|b| {
                if let Some(t) = &mut b.terrain {
                    t.height += 1.0;
                }
            }),
            ("its terrain's snow", &|b| {
                if let Some(t) = &mut b.terrain {
                    t.snow = Some(((1.0, 1.0, 1.0), 30.0));
                }
            }),
            ("no terrain", &|b| b.terrain = None),
            ("a sea", &|b| {
                b.far.layers.push(FarLayer::Sea {
                    color: (0.1, 0.5, 0.7),
                    from: 60.0,
                    beach: 10.0,
                })
            }),
        ] {
            assert_ne!(with(change), fingerprint, "{what}");
        }
        let mut music = prairie.clone();
        music.music = Some("autre-musique".into());
        assert_eq!(
            circuit_fingerprint(&text, Some(&music)),
            fingerprint,
            "the music"
        );
        assert_ne!(circuit_fingerprint(&text, None), fingerprint, "no biome");
    }
}
