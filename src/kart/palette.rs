//! The colors of the karts' bodies (issue #55): one palette of 12 bright, readable colors, shared
//! by the players and the bots. Pure logic, unit-tested; the visual keeps one material per color
//! (`KartVisualAssets`), shared by every kart that wears it.
//!
//! - **The player** chooses its color on the selection screen (`characters`, G), kept between
//!   sessions; pink until it chooses ([`KartColor::default`]: the player's kart was always pink).
//! - **The bots** keep the color of their grid slot ([`BOT_COLORS`], the colors they always had,
//!   issue #6), unless a player wears it: then they take the closest free one
//!   ([`bot_colors`]).
//! - **Online**, two players may choose the same color: the one with the first seat keeps it, the
//!   other wears the closest free one ([`resolve_players`]). Every peer computes the same from
//!   the seats (the same order everywhere) and the `Hello` of each player, whatever the order in
//!   which these arrived.
//!
//! « Closest »: the smallest distance between the two colors' sRGB bytes (integers: the same
//! result on every machine), the first of the palette on a tie.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Colors in the palette.
pub const PALETTE_SIZE: usize = 12;

/// A color of the palette. Its index ([`KartColor::index`]) is its byte on the wire (`Hello`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KartColor {
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Teal,
    SkyBlue,
    Blue,
    Violet,
    /// The player's color until it chooses another one.
    #[default]
    Pink,
    Brown,
    White,
}

impl KartColor {
    /// The palette, in the order of the selection screen (around the color wheel, then brown
    /// and white).
    pub const ALL: [Self; PALETTE_SIZE] = [
        KartColor::Red,
        KartColor::Orange,
        KartColor::Yellow,
        KartColor::Lime,
        KartColor::Green,
        KartColor::Teal,
        KartColor::SkyBlue,
        KartColor::Blue,
        KartColor::Violet,
        KartColor::Pink,
        KartColor::Brown,
        KartColor::White,
    ];

    /// Its place in [`KartColor::ALL`]: its byte on the wire.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// The color of this byte; `None` outside the palette.
    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    /// Its sRGB bytes. The bots' colors are those they always had (issue #6), the pink the
    /// player's kart always had.
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            KartColor::Red => [230, 51, 51],
            KartColor::Orange => [255, 140, 38],
            KartColor::Yellow => [242, 217, 51],
            KartColor::Lime => [140, 217, 64],
            KartColor::Green => [38, 153, 77],
            KartColor::Teal => [26, 179, 179],
            KartColor::SkyBlue => [89, 179, 255],
            KartColor::Blue => [51, 77, 217],
            KartColor::Violet => [140, 77, 217],
            KartColor::Pink => [242, 115, 166],
            KartColor::Brown => [140, 89, 51],
            KartColor::White => [242, 242, 242],
        }
    }

    /// For the body's material.
    pub fn color(self) -> Color {
        let [r, g, b] = self.rgb();
        Color::srgb_u8(r, g, b)
    }

    /// Its name, as in « tu es en bleu ciel ».
    pub const fn label(self) -> &'static str {
        match self {
            KartColor::Red => "rouge",
            KartColor::Orange => "orange",
            KartColor::Yellow => "jaune",
            KartColor::Lime => "vert pomme",
            KartColor::Green => "vert",
            KartColor::Teal => "turquoise",
            KartColor::SkyBlue => "bleu ciel",
            KartColor::Blue => "bleu",
            KartColor::Violet => "violet",
            KartColor::Pink => "rose",
            KartColor::Brown => "marron",
            KartColor::White => "blanc",
        }
    }

    /// Its name at the start of a line: « Bleu ciel ».
    pub fn title(self) -> String {
        let label = self.label();
        let mut chars = label.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    }

    /// The color `step` places further in the palette (`step` < 0: backwards), around.
    pub fn cycled(self, step: i32) -> Self {
        let at = i32::from(self.index()) + step;
        Self::ALL[at.rem_euclid(PALETTE_SIZE as i32) as usize]
    }

    /// How far apart two colors look: the squared distance of their sRGB bytes.
    pub fn distance(self, other: Self) -> u32 {
        self.rgb()
            .iter()
            .zip(other.rgb())
            .map(|(a, b)| (i32::from(*a) - i32::from(b)).unsigned_abs().pow(2))
            .sum()
    }

    /// The closest color to this one that is not `taken` (itself if it is free); `None` when the
    /// whole palette is taken. A tie goes to the first of the palette.
    pub fn nearest_free(self, taken: ColorSet) -> Option<Self> {
        Self::ALL
            .into_iter()
            .filter(|color| !taken.contains(*color))
            .min_by_key(|color| (self.distance(*color), color.index()))
    }

    /// The text kept between sessions (`characters`, issue #55): its name in the code (RON).
    pub fn to_stored(self) -> String {
        ron::to_string(&self).unwrap_or_default()
    }

    /// Reads a kept text; an unknown color, or anything else, gives the reason (the game then
    /// keeps its color).
    pub fn from_stored(text: &str) -> Result<Self, String> {
        ron::from_str(text.trim()).map_err(|error| error.to_string())
    }
}

/// A set of colors of the palette (no allocation: a bit per color).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ColorSet(u16);

impl ColorSet {
    pub fn insert(&mut self, color: KartColor) {
        self.0 |= 1 << color.index();
    }

    pub fn contains(self, color: KartColor) -> bool {
        self.0 & (1 << color.index()) != 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl FromIterator<KartColor> for ColorSet {
    fn from_iter<I: IntoIterator<Item = KartColor>>(colors: I) -> Self {
        let mut set = ColorSet::default();
        for color in colors {
            set.insert(color);
        }
        set
    }
}

/// The color of the bots by grid slot: the colors they always had (issue #6). The player was
/// pink, a color no bot wore: it is the last slot's, which a bot never takes (one slot always
/// stays for a player, `RaceGrid::bot_count`).
pub const BOT_COLORS: [KartColor; PALETTE_SIZE] = [
    KartColor::Red,
    KartColor::Orange,
    KartColor::Yellow,
    KartColor::Lime,
    KartColor::Green,
    KartColor::Teal,
    KartColor::SkyBlue,
    KartColor::Blue,
    KartColor::Violet,
    KartColor::Brown,
    KartColor::White,
    KartColor::Pink,
];

/// The color of the bot on grid slot `slot` when no player wears it.
pub fn bot_color(slot: usize) -> KartColor {
    BOT_COLORS[slot % PALETTE_SIZE]
}

/// The colors the players wear, in seat order (online: `Lobby::seats`, the same order on every
/// peer), from the colors they chose (`None`: not known yet, its `Hello` has not arrived; it
/// takes nothing). A color chosen by several players stays with the first of them; each of the
/// others, in seat order, wears the color closest to its choice that nobody else wears (nor
/// chose first). Beyond [`PALETTE_SIZE`] players (never: a room holds 12), the others are
/// `None`.
pub fn resolve_players(
    chosen: impl IntoIterator<Item = Option<KartColor>>,
) -> [Option<KartColor>; PALETTE_SIZE] {
    let mut chosen_in_order = [None; PALETTE_SIZE];
    for (slot, color) in chosen_in_order.iter_mut().zip(chosen) {
        *slot = color;
    }
    let mut worn = [None; PALETTE_SIZE];
    let mut taken = ColorSet::default();
    // First, every color goes to the first player who chose it.
    for (worn, color) in worn.iter_mut().zip(chosen_in_order) {
        if let Some(color) = color
            && !taken.contains(color)
        {
            taken.insert(color);
            *worn = Some(color);
        }
    }
    // Then the others, in seat order, take the closest free one.
    for (worn, color) in worn.iter_mut().zip(chosen_in_order) {
        if let (None, Some(color)) = (*worn, color) {
            let free = color.nearest_free(taken).unwrap_or(color);
            taken.insert(free);
            *worn = Some(free);
        }
    }
    worn
}

/// The colors of the karts by grid slot, for `bots` bots on the front slots (0, 1...) with
/// players wearing `players`: each bot keeps its own color ([`BOT_COLORS`]) unless a player
/// wears it; those, in slot order, take the color closest to theirs that neither a player nor
/// another bot wears. With no free color left (never: 12 colors for 12 karts), a bot keeps its
/// own. The slots after the bots keep the bots' colors (the players' slots: not used).
pub fn bot_colors(bots: usize, players: ColorSet) -> [KartColor; PALETTE_SIZE] {
    let mut colors = BOT_COLORS;
    let bots = bots.min(PALETTE_SIZE);
    let mut taken = players;
    for color in &colors[..bots] {
        if !players.contains(*color) {
            taken.insert(*color);
        }
    }
    for color in &mut colors[..bots] {
        if players.contains(*color)
            && let Some(free) = color.nearest_free(taken)
        {
            taken.insert(free);
            *color = free;
        }
    }
    colors
}

/// The color of a kart's body (on the simulated kart, or on the body of another peer's kart):
/// its visual follows it, even when it changes (online, issue #55: a player's color can change
/// in the middle of a race).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KartPaint(pub KartColor);

/// Online (issue #55, `net`): the colors the players of the race wear, which the bots avoid.
/// Without it (solo), the bots avoid the local player's color (`characters::PlayerColor`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OnlinePlayerColors(pub ColorSet);

#[cfg(test)]
mod tests {
    use super::*;
    use KartColor::*;

    #[test]
    fn the_palette_has_twelve_distinct_colors_and_its_bytes() {
        for (index, color) in KartColor::ALL.iter().enumerate() {
            assert_eq!(usize::from(color.index()), index);
            assert_eq!(KartColor::from_index(color.index()), Some(*color));
            for other in &KartColor::ALL[index + 1..] {
                assert_ne!(color.rgb(), other.rgb());
                assert_ne!(color.label(), other.label());
                // Readable side by side: no two colors too close.
                assert!(color.distance(*other) > 4_000, "{color:?} {other:?}");
            }
        }
        for index in PALETTE_SIZE as u8..=255 {
            assert_eq!(KartColor::from_index(index), None);
        }
        // The player's kart stays pink until it chooses, and the bots keep their colors.
        assert_eq!(KartColor::default(), Pink);
        assert_eq!(Pink.rgb(), [242, 115, 166]);
        assert_eq!(bot_color(3), Lime);
        assert_eq!(SkyBlue.title(), "Bleu ciel");
        assert_eq!(SkyBlue.label(), "bleu ciel");
    }

    #[test]
    fn the_selection_line_goes_around_the_palette() {
        assert_eq!(Pink.cycled(1), Brown);
        assert_eq!(White.cycled(1), Red);
        assert_eq!(Red.cycled(-1), White);
        assert_eq!(Blue.cycled(0), Blue);
        let mut color = Red;
        for _ in 0..PALETTE_SIZE {
            color = color.cycled(1);
        }
        assert_eq!(color, Red);
    }

    /// Issue #55: the choice is kept as a short text and comes back the same; a text that is not
    /// a color of the palette is refused (the game then keeps its color).
    #[test]
    fn the_color_survives_a_round_trip_through_the_storage_text() {
        for color in KartColor::ALL {
            let text = color.to_stored();
            assert_eq!(KartColor::from_stored(&text), Ok(color), "{text}");
        }
        assert_eq!(SkyBlue.to_stored(), "SkyBlue");
        assert_eq!(KartColor::from_stored(" Blue\n"), Ok(Blue));
        for bad in ["", "Charcoal", "bleu", "3", "(color: Blue)", "Blue Red"] {
            assert!(KartColor::from_stored(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_closest_free_color_looks_like_the_one_taken() {
        let none = ColorSet::default();
        for color in KartColor::ALL {
            assert_eq!(color.nearest_free(none), Some(color));
        }
        let taken = |colors: &[KartColor]| colors.iter().copied().collect::<ColorSet>();
        assert_eq!(Red.nearest_free(taken(&[Red])), Some(Orange));
        assert_eq!(Blue.nearest_free(taken(&[Blue])), Some(Violet));
        assert_eq!(Teal.nearest_free(taken(&[Teal])), Some(SkyBlue));
        assert_eq!(Pink.nearest_free(taken(&[Pink])), Some(Violet));
        assert_eq!(Red.nearest_free(taken(&[Red, Orange])), Some(Brown));
        assert_eq!(Red.nearest_free(KartColor::ALL.into_iter().collect()), None);
        let all_but_green: ColorSet = KartColor::ALL.into_iter().filter(|c| *c != Green).collect();
        assert_eq!(White.nearest_free(all_but_green), Some(Green));
        assert_eq!(all_but_green.len(), 11);
    }

    /// Issue #55: two players who chose the same color: the first seat keeps it, the other wears
    /// the closest free one; nobody wears the same color as another.
    #[test]
    fn the_first_seat_keeps_its_color_and_the_others_take_the_closest_free_one() {
        assert_eq!(
            resolve_players([Some(Red), Some(Red)])[..2],
            [Some(Red), Some(Orange)]
        );
        // The third chose orange: it keeps it, the second takes the next closest to red.
        assert_eq!(
            resolve_players([Some(Red), Some(Red), Some(Orange)])[..3],
            [Some(Red), Some(Brown), Some(Orange)]
        );
        // Unknown players take nothing; no conflict, nothing changes.
        assert_eq!(
            resolve_players([None, Some(Blue), Some(Green)])[..3],
            [None, Some(Blue), Some(Green)]
        );
        // Twelve players, all pink: twelve different colors, the first one pink.
        let worn = resolve_players([Some(Pink); 12]);
        assert_eq!(worn[0], Some(Pink));
        assert_eq!(
            worn.iter().flatten().copied().collect::<ColorSet>().len(),
            12
        );
        // A thirteenth (never in a room) is left out.
        assert_eq!(resolve_players([Some(Pink); 13]), worn);
    }

    /// Issue #55: the result depends on the seats and the choices only: whatever the order in
    /// which the players' choices became known (every order of 4 arrivals), once all of them
    /// are known, every peer computes the same colors.
    #[test]
    fn the_colors_do_not_depend_on_the_order_of_arrival() {
        let seats = ["ana", "bob", "cyd", "dan"];
        let chosen = [Blue, Red, Blue, Red];
        let mut results = Vec::new();
        // Every order of the 4 arrivals.
        let mut orders: Vec<Vec<usize>> = vec![vec![]];
        for player in 0..seats.len() {
            orders = orders
                .iter()
                .flat_map(|order| {
                    (0..=order.len()).map(move |at| {
                        let mut longer = order.clone();
                        longer.insert(at, player);
                        longer
                    })
                })
                .collect();
        }
        assert_eq!(orders.len(), 24);
        for order in orders {
            // The peer learns the choices one by one, in this order.
            let mut known: Vec<(&str, KartColor)> = Vec::new();
            let mut worn = [None; PALETTE_SIZE];
            for i in order {
                known.push((seats[i], chosen[i]));
                worn = resolve_players(seats.iter().map(|seat| {
                    known
                        .iter()
                        .find(|(name, _)| name == seat)
                        .map(|(_, color)| *color)
                }));
            }
            results.push(worn);
        }
        let expected = [Some(Blue), Some(Red), Some(Violet), Some(Orange)];
        for worn in results {
            assert_eq!(worn[..4], expected);
        }
    }

    /// Issue #55: the bots keep their colors, unless a player wears one: that bot takes a free
    /// color. Twelve karts, twelve colors.
    #[test]
    fn the_bots_avoid_the_players_colors() {
        // Solo by default: the player in pink, the 11 bots in the colors they always had.
        let solo = bot_colors(11, [Pink].into_iter().collect());
        assert_eq!(solo[..11], BOT_COLORS[..11]);
        assert!(!solo[..11].contains(&Pink));
        // The player in red: the first bot (red) wears the only free color, pink.
        let red = bot_colors(11, [Red].into_iter().collect());
        assert_eq!(red[0], Pink);
        assert_eq!(red[1..11], BOT_COLORS[1..11]);
        let mut all: ColorSet = red[..11].iter().copied().collect();
        all.insert(Red);
        assert_eq!(all.len(), 12);
        // Two players (red, orange) and 4 bots: the first two bots move to the closest free
        // colors (neither a player's nor another bot's).
        let players: ColorSet = [Red, Orange].into_iter().collect();
        let four = bot_colors(4, players);
        assert_eq!(four[..4], [Brown, Pink, Yellow, Lime]);
        // Players in colors no bot of the grid wears (the white bot is slot 10, not on a grid
        // of 10 bots): the bots keep theirs.
        let pink_white: ColorSet = [Pink, White].into_iter().collect();
        assert_eq!(bot_colors(10, pink_white)[..10], BOT_COLORS[..10]);
        // No bot: nothing to do.
        assert_eq!(bot_colors(0, players), BOT_COLORS);
    }
}
