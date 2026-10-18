//! The roulette of the HUD's item slots, as a slot machine (issue #102, GD § 8 and § 14): pure
//! logic, unit-tested. Max: « Quand on prend une case cadeau il doit y avoir un petit temps où ça
//! passe tous les cadeaux avant d'en choisir un. »
//!
//! The rules spin the roulette in the slot (`items::Slot::Spinning`, `ItemTuning::roulette_time`,
//! about 1.5 s): the item cannot be used before it stops. On screen:
//! - the items **scroll** through the slot's window, sliding down from the top, about
//!   `ItemTuning::roulette_scroll_speed` a second ([`ReelPlan`]);
//! - over the last `ItemTuning::roulette_slowdown` seconds the reel **slows down** evenly, and it
//!   **stops on the item drawn** at the very tick the rules make it ready (the speed is adjusted a
//!   little so that it lands right on it);
//! - then a « ding » ([`ding_scale`], [`burst`], [`flash`]): the slot squashes and bounces back
//!   like jelly (`docs/UI_CHARTE.md`, « Animations molles »), a ring bursts out of it and its
//!   window flashes. A tick for each item that passes, a « ding » for the stop
//!   (`audio::RouletteSounds`).
//!
//! **The draw does not change**: it is the rules' (deterministic, from `RaceGrid::seed`), and the
//! reel only shows it. The reel is one strip of the sixteen items ([`REEL`]), started at a place
//! drawn from the tick the roulette began ([`reel_start`]: the same through the whole roulette,
//! and when the reserve's roulette moves into the hand): what scrolls says nothing of the item
//! drawn. Only its end is the item drawn (and, when the cell before would be the same item, the
//! one the drawn item took the place of: two neighbours never look alike).
//!
//! The reel follows the fixed ticks of the rules (`Time<Fixed>`: how many ticks the roulette has
//! left, and how far into the next one), not a clock of its own: it stops with the game's pause,
//! and on the tick the item is ready. The HUD's single icon (headless too, `SlotView`) shows the
//! item in the middle of the window; the reel itself, its ding and its sounds are drawn only with
//! a renderer (`ui::reel`).

use std::f32::consts::PI;

use bevy::prelude::*;

use crate::bots::BotRng;
use crate::items::{ItemKind, ItemSlots, ItemTuning, Slot};
use crate::menu::anim;
use crate::race::{seconds_to_ticks, ticks_to_seconds};

/// The reel: the sixteen items, two neighbours never alike (their colors, then their shapes:
/// `ui::icon_shape`'s, and the colors of their icons as rendered, issue #106), so that each one
/// is seen passing.
pub const REEL: [ItemKind; 16] = [
    ItemKind::Pelure,
    ItemKind::Pelote,
    ItemKind::Bombe,
    ItemKind::Piment,
    ItemKind::Chantilly,
    ItemKind::Abeille,
    ItemKind::Degonflage,
    ItemKind::Fusee,
    ItemKind::PieVoleuse,
    ItemKind::CroquetteDoree,
    ItemKind::Pinata,
    ItemKind::PimentDore,
    ItemKind::OreillerGeant,
    ItemKind::KlaxonCanard,
    ItemKind::Boomerang,
    ItemKind::SuperCalin,
];

/// Where the reel is in its roulette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReelPhase {
    /// At full speed.
    Scrolling,
    /// Slowing down to its stop.
    Slowing,
    /// On the item drawn (the roulette is over).
    Stopped,
}

/// How a reel runs through a roulette: full speed, then slowing down evenly to a stop on the
/// `cells`-th item (the item drawn), at the end of the roulette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReelPlan {
    /// Items that go through the window before the item drawn stops in it (at least 1).
    pub cells: u32,
    /// The speed of the scroll before the slowdown (items/s): the one asked, adjusted so that the
    /// reel stops right on its last item.
    pub speed: f32,
    /// Full speed this long (s)...
    pub cruise: f32,
    /// ...then slowing down this long (s)...
    pub slowdown: f32,
    /// ...to the end of the roulette (s).
    pub total: f32,
}

impl ReelPlan {
    /// A roulette of `total` s, scrolling at about `speed` items a second, slowing down over its
    /// last `slowdown` s (at most `total`).
    pub fn new(total: f32, speed: f32, slowdown: f32) -> Self {
        let total = total.max(0.0);
        let slowdown = slowdown.clamp(0.0, total);
        let cruise = total - slowdown;
        // Slowing down evenly to a stop covers half what full speed would.
        let run = cruise + slowdown / 2.0;
        let cells = (speed.max(0.0) * run).round().max(1.0) as u32;
        let speed = if run > 0.0 { cells as f32 / run } else { 0.0 };
        Self {
            cells,
            speed,
            cruise,
            slowdown,
            total,
        }
    }

    /// The plan of the roulettes of `tuning`: their length in whole ticks, as the rules count it.
    pub fn of(tuning: &ItemTuning) -> Self {
        Self::new(
            ticks_to_seconds(seconds_to_ticks(tuning.roulette_time)),
            tuning.roulette_scroll_speed,
            tuning.roulette_slowdown,
        )
    }

    /// How far the reel has run `t` s into the roulette (items, 0 to `cells`).
    pub fn position(&self, t: f32) -> f32 {
        if t >= self.total {
            return self.cells as f32;
        }
        if t <= 0.0 {
            return 0.0;
        }
        if t <= self.cruise {
            return self.speed * t;
        }
        let u = t - self.cruise;
        let slowing = u - u * u / (2.0 * self.slowdown);
        (self.speed * (self.cruise + slowing)).min(self.cells as f32)
    }

    /// Its speed `t` s into the roulette (items/s).
    pub fn speed_at(&self, t: f32) -> f32 {
        if t < 0.0 || t >= self.total {
            0.0
        } else if t <= self.cruise {
            self.speed
        } else {
            self.speed * (1.0 - (t - self.cruise) / self.slowdown)
        }
    }

    /// Its phase `t` s into the roulette.
    pub fn phase(&self, t: f32) -> ReelPhase {
        if t >= self.total {
            ReelPhase::Stopped
        } else if t > self.cruise {
            ReelPhase::Slowing
        } else {
            ReelPhase::Scrolling
        }
    }
}

/// Where a reel starts on [`REEL`] for a roulette that began on the fixed tick `began`: drawn from
/// it (the bots' generator), so that two roulettes start in different places, and a roulette in
/// the same place from its first frame to its last.
pub fn reel_start(began: u64) -> usize {
    (BotRng::new(began).next_u64() % REEL.len() as u64) as usize
}

/// The `cell`-th item of a reel started at `start` that stops on `drawn` at `cells`: the reel's
/// own, but `drawn` at `cells`; and at `cells - 1`, if the reel had `drawn` there, the one `drawn`
/// took the place of (the reel never shows the same item twice in a row).
pub fn reel_item(start: usize, cell: u32, cells: u32, drawn: ItemKind) -> ItemKind {
    let at = |k: u32| REEL[(start + k as usize) % REEL.len()];
    if cell == cells {
        drawn
    } else if cell + 1 == cells && at(cell) == drawn {
        at(cells)
    } else {
        at(cell)
    }
}

/// What a slot's window shows of its spinning roulette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReelFrame {
    /// Items passed (0 to the plan's `cells`): `shown` is the `cell`-th...
    pub cell: u32,
    /// ...slid this far down out of the window (a share of its height, 0 to 1), `next` coming in
    /// from the top the same way.
    pub slide: f32,
    pub shown: ItemKind,
    pub next: ItemKind,
    pub phase: ReelPhase,
}

impl ReelFrame {
    /// The item nearest the middle of the window.
    pub fn centered(&self) -> ItemKind {
        if self.slide < 0.5 {
            self.shown
        } else {
            self.next
        }
    }
}

/// The clock of the roulettes, for the HUD: the rules' fixed ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouletteClock {
    /// The last fixed tick (ticks since the start of the game)...
    pub tick: u64,
    /// ...and how far into the next one (0 to 1).
    pub overstep: f32,
    /// A roulette's length (ticks, `ItemTuning::roulette_time`).
    pub roulette: u32,
    pub plan: ReelPlan,
}

impl RouletteClock {
    pub fn new(fixed: &Time<Fixed>, tuning: &ItemTuning) -> Self {
        Self {
            tick: (fixed.elapsed().as_secs_f64() * crate::TICK_HZ).round() as u64,
            overstep: fixed.overstep_fraction().clamp(0.0, 1.0),
            roulette: seconds_to_ticks(tuning.roulette_time),
            plan: ReelPlan::of(tuning),
        }
    }

    /// The tick a roulette that has `left` ticks to go began on.
    pub fn began(&self, left: u32) -> u64 {
        self.tick
            .saturating_sub(u64::from(self.roulette.saturating_sub(left)))
    }

    /// How long a roulette that has `left` ticks to go has been spinning (s), now.
    pub fn elapsed(&self, left: u32) -> f32 {
        let ticks = self.roulette.saturating_sub(left) as f32 + self.overstep;
        // Never past the end while the rules still spin it: it stops on the tick it is ready.
        ticks.min(self.roulette.saturating_sub(1) as f32 + 0.999) / crate::TICK_HZ as f32
    }

    /// The window of a roulette that has `left` ticks to go and stops on `drawn`, its reel moved
    /// `shift` items along (the reserve's, next to the hand's).
    pub fn frame(&self, left: u32, drawn: ItemKind, shift: usize) -> ReelFrame {
        let t = self.elapsed(left);
        let position = self.plan.position(t);
        // Still spinning: the item drawn is coming in, at most (it is in the middle once the
        // rules stop the roulette).
        let cell = (position.floor() as u32).min(self.plan.cells - 1);
        let start = reel_start(self.began(left)) + shift;
        ReelFrame {
            cell,
            slide: (position - cell as f32).clamp(0.0, 1.0),
            shown: reel_item(start, cell, self.plan.cells, drawn),
            next: reel_item(start, cell + 1, self.plan.cells, drawn),
            phase: self.plan.phase(t),
        }
    }

    /// The windows of a kart's two slots (`None`: no roulette there). The hand and the reserve of
    /// a double box began on the same tick: the reserve's reel is half a reel further on, so that
    /// both do not show the same items side by side.
    pub fn frames(&self, slots: &ItemSlots) -> [Option<ReelFrame>; 2] {
        let spinning = |slot: &Slot| match *slot {
            Slot::Spinning { item, ticks, .. } => Some((item, ticks)),
            _ => None,
        };
        let (hand, reserve) = (spinning(&slots.hand), spinning(&slots.reserve));
        let together = matches!(
            (hand, reserve),
            (Some((_, a)), Some((_, b))) if self.began(a) == self.began(b)
        );
        let shift = if together { REEL.len() / 2 } else { 0 };
        [
            hand.map(|(item, left)| self.frame(left, item, 0)),
            reserve.map(|(item, left)| self.frame(left, item, shift)),
        ]
    }
}

/// The ding of the stop lasts this long (s: under half a second, like every animation of the
/// charter)...
pub const DING_TIME: f32 = 0.45;
/// ...the slot squashed to this height at the stop, it springs back past its size and settles
/// (Hz, damping)...
pub const DING_SQUASH: f32 = 0.72;
pub const DING_HZ: f32 = 4.5;
pub const DING_DAMPING: f32 = 0.32;
/// ...and it grows by this much meanwhile (a bump).
pub const DING_POP: f32 = 0.12;
/// A ring bursts out of the slot at the stop: this long (s), this far out (px), this wide (px).
pub const BURST_TIME: f32 = 0.35;
pub const BURST_REACH: f32 = 18.0;
pub const BURST_WIDTH: f32 = 5.0;
/// Its window flashes at the stop, fading out in this long (s).
pub const FLASH_TIME: f32 = 0.22;

/// The slot's scale `t` s after its roulette stopped: squashed (wider, lower), springing back
/// past its size, then exactly 1 from [`DING_TIME`] on (the charter's squash and stretch,
/// `menu::anim`).
pub fn ding_scale(t: f32) -> Vec2 {
    if !(0.0..DING_TIME).contains(&t) {
        return Vec2::ONE;
    }
    let height = anim::spring(t, DING_SQUASH, 1.0, DING_HZ, DING_DAMPING);
    let bump = 1.0 + DING_POP * (PI * t / DING_TIME).sin();
    anim::squash_and_stretch(height) * bump
}

/// The ring that bursts out of the slot `t` s after the stop: how far out (px) and how opaque
/// (0 to 1); `None` once it is gone.
pub fn burst(t: f32) -> Option<(f32, f32)> {
    if !(0.0..BURST_TIME).contains(&t) {
        return None;
    }
    let x = t / BURST_TIME;
    Some((BURST_REACH * (1.0 - (1.0 - x) * (1.0 - x)), 1.0 - x))
}

/// How bright the slot's window flashes `t` s after the stop (0 to 1).
pub fn flash(t: f32) -> f32 {
    if !(0.0..FLASH_TIME).contains(&t) {
        return 0.0;
    }
    1.0 - t / FLASH_TIME
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::icon_shape;

    fn plan() -> ReelPlan {
        ReelPlan::of(&ItemTuning::default())
    }

    /// The color an icon is seen in: its fill, or its edge for a ring (the croquette dorée).
    fn main_color(item: ItemKind) -> Vec3 {
        let (_, fill, edge, ..) = icon_shape(item);
        let color = if fill.alpha() > 0.5 { fill } else { edge };
        let c = color.to_srgba();
        Vec3::new(c.red, c.green, c.blue)
    }

    /// The reel is every item once, and two neighbours (around the reel too) never look alike:
    /// their main colors far apart.
    #[test]
    fn the_reel_has_every_item_once_and_no_neighbours_alike() {
        for item in ItemKind::ALL {
            assert_eq!(REEL.iter().filter(|i| **i == item).count(), 1, "{item:?}");
        }
        for (k, item) in REEL.iter().enumerate() {
            let next = REEL[(k + 1) % REEL.len()];
            let apart = main_color(*item).distance(main_color(next));
            assert!(apart > 0.4, "{item:?} then {next:?}: {apart}");
        }
    }

    /// Issue #102: with the defaults, the reel scrolls a dozen items a second for most of the
    /// roulette, then slows down evenly and stops right on its last item when the roulette ends
    /// (1.5 s): a dozen items pass, the last ones slowly.
    #[test]
    fn the_reel_scrolls_then_slows_down_and_stops_on_its_last_item() {
        let plan = plan();
        assert_eq!(plan.total, 1.5);
        assert!((12..=16).contains(&plan.cells), "{plan:?}");
        let tuning = ItemTuning::default();
        assert!(
            (plan.speed - tuning.roulette_scroll_speed).abs() < 1.0,
            "{plan:?}"
        );
        assert_eq!(plan.slowdown, tuning.roulette_slowdown);
        // Full speed, then slower and slower, never back; stopped at the end, on its last item.
        let mut last = 0.0;
        let mut speeds = Vec::new();
        for k in 0..=150 {
            let t = k as f32 / 100.0;
            let at = plan.position(t);
            assert!(at >= last - 1e-5, "{t}: {at} < {last}");
            last = at;
            speeds.push(plan.speed_at(t));
            let phase = plan.phase(t);
            match phase {
                ReelPhase::Scrolling => assert!(t <= plan.cruise + 1e-6),
                ReelPhase::Slowing => assert!(t > plan.cruise && t < 1.5),
                ReelPhase::Stopped => assert!(t >= 1.5),
            }
        }
        assert_eq!(plan.position(1.5), plan.cells as f32);
        assert_eq!(plan.position(9.0), plan.cells as f32);
        assert!((plan.position(1.4999) - plan.cells as f32).abs() < 1e-3);
        assert!(speeds.windows(2).all(|w| w[1] <= w[0] + 1e-4));
        // Its speed falls evenly to nothing: half way through the slowdown, half the speed.
        let mid = plan.cruise + plan.slowdown / 2.0;
        assert!((plan.speed_at(mid) - plan.speed / 2.0).abs() < 1e-3);
        // The last item comes in slowly: the last 0.3 s cover less than one item, the first
        // 0.3 s more than three.
        assert!(plan.position(1.5) - plan.position(1.2) < 1.0);
        assert!(plan.position(0.3) > 3.0);
        // Continuous where the slowdown starts.
        let c = plan.cruise;
        assert!((plan.position(c + 1e-4) - plan.position(c - 1e-4)).abs() < 0.01);
    }

    /// Whatever the tuning (admin panel), the reel stops on its last item at the end of the
    /// roulette, and never moves backwards.
    #[test]
    fn any_tuning_stops_the_reel_on_its_last_item() {
        for (total, speed, slowdown) in [
            (1.5, 12.0, 0.7),
            (1.5, 0.0, 0.7),
            (1.0, 30.0, 0.0),
            (2.0, 8.0, 2.0),
            (0.5, 12.0, 5.0),
            (0.0, 12.0, 0.7),
        ] {
            let plan = ReelPlan::new(total, speed, slowdown);
            assert!(plan.cells >= 1, "{plan:?}");
            assert!(plan.slowdown <= total.max(0.0) + 1e-6);
            let mut last = 0.0;
            for k in 0..=100 {
                let at = plan.position(total * k as f32 / 100.0);
                assert!(at >= last - 1e-4 && at <= plan.cells as f32, "{plan:?}");
                last = at;
            }
            assert_eq!(plan.position(total), plan.cells as f32, "{plan:?}");
        }
    }

    /// Issue #102 point 4: the reel stops on the item drawn (the rules'), whatever it started on;
    /// the reel's items before it, two neighbours never the same; the item drawn passes during
    /// the scroll as any other (what scrolls does not tell it).
    #[test]
    fn the_reel_ends_on_the_item_drawn_and_never_shows_one_twice_in_a_row() {
        let cells = plan().cells;
        for start in 0..REEL.len() {
            for drawn in ItemKind::ALL {
                let items: Vec<ItemKind> = (0..=cells)
                    .map(|cell| reel_item(start, cell, cells, drawn))
                    .collect();
                assert_eq!(items[cells as usize], drawn);
                assert!(
                    items.windows(2).all(|w| w[0] != w[1]),
                    "start {start}, {drawn:?}: {items:?}"
                );
                // All but the last two are the reel's own, from its start.
                for (k, item) in items.iter().enumerate().take(cells as usize - 1) {
                    assert_eq!(*item, REEL[(start + k) % REEL.len()]);
                }
            }
        }
        // The reel starts in different places for roulettes a tick apart, and the scroll before
        // the end is the same whatever the item drawn.
        let starts: Vec<usize> = (0..64).map(reel_start).collect();
        assert!(starts.windows(2).filter(|w| w[0] != w[1]).count() > 48);
        let mut seen = starts.clone();
        seen.sort_unstable();
        seen.dedup();
        assert!(seen.len() >= 12, "{seen:?}");
        let start = reel_start(1234);
        for cell in 0..cells - 1 {
            let a = reel_item(start, cell, cells, ItemKind::Pelure);
            let b = reel_item(start, cell, cells, ItemKind::Fusee);
            assert_eq!(a, b, "cell {cell}");
        }
    }

    /// The window of a roulette from its first tick to its last: the reel turns, the item shown
    /// slides down, then the item drawn stops in the middle. Its start stays the same all along,
    /// and in the hand when the reserve's roulette moves in.
    #[test]
    fn the_window_follows_the_ticks_of_the_roulette() {
        let tuning = ItemTuning::default();
        let clock = |tick: u64, overstep: f32| RouletteClock {
            tick,
            overstep,
            roulette: 90,
            plan: ReelPlan::of(&tuning),
        };
        let drawn = ItemKind::Abeille;
        // The box on tick 1000: 90 ticks to go.
        let first = clock(1000, 0.0).frame(90, drawn, 0);
        assert_eq!((first.cell, first.slide), (0, 0.0));
        assert_eq!(first.phase, ReelPhase::Scrolling);
        let start = reel_start(1000);
        let total = ReelPlan::of(&tuning).cells;
        let mut cells = Vec::new();
        for elapsed in 0..90u32 {
            let left = 90 - elapsed;
            for overstep in [0.0, 0.5, 0.99] {
                let frame = clock(1000 + u64::from(elapsed), overstep).frame(left, drawn, 0);
                assert_eq!(frame.shown, reel_item(start, frame.cell, total, drawn));
                assert!((0.0..=1.0).contains(&frame.slide));
                assert_ne!(frame.phase, ReelPhase::Stopped, "the rules still spin it");
                cells.push(frame.cell);
            }
        }
        assert!(cells.windows(2).all(|w| w[1] >= w[0]));
        // On its last tick, the item drawn has (nearly) come in: in the middle of the window.
        let last = clock(1089, 0.99).frame(1, drawn, 0);
        assert_eq!(last.centered(), drawn);
        assert_eq!(last.next, drawn);
        assert!(last.slide > 0.95, "{last:?}");
        // Slowing down at the end, full speed at first.
        assert_eq!(
            clock(1080, 0.0).frame(10, drawn, 0).phase,
            ReelPhase::Slowing
        );
        assert_eq!(
            clock(1010, 0.0).frame(80, drawn, 0).phase,
            ReelPhase::Scrolling
        );
        // A double box: the reserve's reel half a reel further on; alone, the same as the hand's.
        let both = ItemSlots {
            hand: Slot::Spinning {
                item: drawn,
                count: 1,
                ticks: 60,
            },
            reserve: Slot::Spinning {
                item: ItemKind::Piment,
                count: 3,
                ticks: 60,
            },
            ..default()
        };
        let [hand, reserve] = clock(1030, 0.0).frames(&both);
        let (hand, reserve) = (hand.unwrap(), reserve.unwrap());
        assert_eq!(hand.cell, reserve.cell);
        assert_ne!(hand.shown, reserve.shown);
        let later = ItemSlots {
            reserve: Slot::Spinning {
                item: ItemKind::Piment,
                count: 3,
                ticks: 30,
            },
            ..both
        };
        let [_, reserve] = clock(1030, 0.0).frames(&later);
        let alone = clock(1030, 0.0).frame(30, ItemKind::Piment, 0);
        assert_eq!(reserve, Some(alone));
        // The reserve's roulette moves into the hand: the same reel, at the same place.
        let moved = ItemSlots {
            hand: later.reserve,
            reserve: Slot::Empty,
            ..both
        };
        assert_eq!(clock(1030, 0.0).frames(&moved), [Some(alone), None]);
    }

    /// The ding: squashed at the stop (wider than tall), springing back past its size, exactly 1
    /// after [`DING_TIME`] (under half a second); the ring bursts out and fades; the flash fades.
    #[test]
    fn the_ding_squashes_bounces_and_settles() {
        let at_stop = ding_scale(0.0);
        assert!(at_stop.y < 0.8 && at_stop.x > 1.1, "{at_stop}");
        let tallest = (0..45)
            .map(|k| ding_scale(k as f32 / 100.0).y)
            .fold(0.0f32, f32::max);
        assert!(tallest > 1.08, "it bounces past its size: {tallest}");
        assert_eq!(ding_scale(DING_TIME), Vec2::ONE);
        assert_eq!(ding_scale(-0.1), Vec2::ONE);
        assert_eq!(ding_scale(f32::INFINITY), Vec2::ONE);
        const { assert!(DING_TIME < 0.5) };
        // Smooth at its end: nearly 1 just before.
        let end = ding_scale(DING_TIME - 1e-3);
        assert!((end - Vec2::ONE).abs().max_element() < 0.02, "{end}");
        let (near, opaque) = burst(0.0).unwrap();
        assert_eq!((near, opaque), (0.0, 1.0));
        let (far, faint) = burst(BURST_TIME * 0.9).unwrap();
        assert!(far > BURST_REACH * 0.9 && faint < 0.15);
        assert_eq!(burst(BURST_TIME), None);
        assert_eq!(flash(0.0), 1.0);
        assert!(flash(FLASH_TIME / 2.0) > 0.4);
        assert_eq!(flash(FLASH_TIME), 0.0);
        assert_eq!(flash(f32::INFINITY), 0.0);
    }
}
