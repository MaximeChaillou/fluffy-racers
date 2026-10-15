//! A kart's two item slots and its item button (GD § 8, issue #16): pure logic, no ECS.
//! Unit-tested.
//!
//! - **Two slots**: the hand (used next) and the reserve. A box fills the hand if it is empty,
//!   else the reserve; a double box both. With both slots taken, the box breaks for nothing (as
//!   in MK8DX). The item spins in its roulette first ([`Slot::Spinning`], about 1.5 s): it cannot
//!   be used before it stops. When the hand is used up, the reserve moves into it.
//! - **The item button** ([`ItemSlots::step_button`], one call per tick with the button's state):
//!   - a single **pelure or pelote** goes out behind the kart when the button is pressed and
//!     stays there while it is held (a shield against what comes from behind); the release drops
//!     the pelure, or throws the pelote. Once it is out it no longer takes a slot: the reserve
//!     moves to the hand, and the kart may take a box meanwhile (as in MK8DX);
//!   - an **"x3"** of pelures, pelotes or abeilles comes out at the first press (the pelures in a
//!     row behind, the pelotes and abeilles turning around the kart) and stays in the hand: each
//!     next press drops or throws one;
//!   - the others are used at the press: abeille, piment (one at a time for an "x3"), croquette
//!     dorée, and those of phase 2 (issue #18);
//!   - a **piment doré** stays in the hand for its window (about 7 s from the first press,
//!     [`ItemSlots::start_golden`]): each press is a boost; then it is used up.
//!   - Thrown **backwards** when the "back" direction is held at the throw (the brake, Bas/S, or
//!     the look-back button, C), forwards otherwise. A pelure is always dropped behind.
//! - Hit by an item or carried by the pelican, a kart loses what it held out
//!   ([`ItemSlots::lose_held_out`]): the dragged item, the rest of an "x3" out.

use super::table::ItemKind;

/// One of a kart's two item slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Slot {
    #[default]
    Empty,
    /// The roulette spins (GD § 8): `count` of `item` come out in `ticks` ticks.
    Spinning {
        item: ItemKind,
        count: u8,
        ticks: u32,
    },
    /// Ready to use: `count` of `item` left (3, 2, 1 for an "x3").
    Ready { item: ItemKind, count: u8 },
}

impl Slot {
    pub fn is_empty(&self) -> bool {
        *self == Slot::Empty
    }

    /// The item in the slot, spinning or ready, and how many.
    pub fn item(&self) -> Option<(ItemKind, u8)> {
        match *self {
            Slot::Empty => None,
            Slot::Spinning { item, count, .. } | Slot::Ready { item, count } => Some((item, count)),
        }
    }
}

/// What the item button did this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Use {
    #[default]
    None,
    /// A single pelure or pelote goes out behind the kart (dragged while the button is held).
    Drag(ItemKind),
    /// The button was released: the dragged item is dropped (pelure) or thrown (pelote),
    /// backwards when `back`.
    Release { item: ItemKind, back: bool },
    /// The hand's "x3" comes out: `count` of `item` around (or behind) the kart.
    Deploy { item: ItemKind, count: u8 },
    /// One of the "x3" out is thrown (or dropped), backwards when `back`.
    Fire { item: ItemKind, back: bool },
    /// Used right away: abeille (thrown), piment, croquette dorée.
    Instant { item: ItemKind, back: bool },
}

/// A kart's items (on every kart with the item rules).
#[derive(bevy::prelude::Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemSlots {
    /// Used next.
    pub hand: Slot,
    pub reserve: Slot,
    /// The hand's "x3" is out around (or behind) the kart: each press throws or drops one.
    pub deployed: bool,
    /// A single pelure or pelote held out behind the kart (button held).
    pub dragging: Option<ItemKind>,
    /// The item button on the previous tick (a press is an edge).
    pub button: bool,
    /// Items drawn in this race: the seed of the next draw (`table::draw_unit`).
    pub draws: u32,
    /// The piment doré's window (issue #18): ticks left, 0 before its first press. It stays in
    /// the hand meanwhile; at the end it is used up.
    pub golden: u32,
}

impl ItemSlots {
    /// A box would give something: a slot is free.
    pub fn has_room(&self) -> bool {
        self.hand.is_empty() || self.reserve.is_empty()
    }

    /// Free slots (0 to 2).
    pub fn free_slots(&self) -> usize {
        [self.hand, self.reserve]
            .iter()
            .filter(|s| s.is_empty())
            .count()
    }

    /// A box gives `count` of `item`: into the hand if it is free, else the reserve, spinning for
    /// `ticks` ticks (0: ready at once). Returns false with no free slot.
    pub fn give(&mut self, item: ItemKind, count: u8, ticks: u32) -> bool {
        let slot = if ticks == 0 {
            Slot::Ready { item, count }
        } else {
            Slot::Spinning { item, count, ticks }
        };
        if self.hand.is_empty() {
            self.hand = slot;
        } else if self.reserve.is_empty() {
            self.reserve = slot;
        } else {
            return false;
        }
        true
    }

    /// Debug (issue #18): `count` of `item` in the hand, ready; what was there is gone (an "x3"
    /// out, a piment doré's window). The reserve stays.
    pub fn force(&mut self, item: ItemKind, count: u8) {
        self.hand = Slot::Ready {
            item,
            count: count.max(1),
        };
        self.deployed = false;
        self.golden = 0;
    }

    /// Whether something counts down: a roulette, the piment doré's window.
    pub fn ticking(&self) -> bool {
        let spinning = |slot: &Slot| matches!(slot, Slot::Spinning { .. });
        spinning(&self.hand) || spinning(&self.reserve) || self.golden > 0
    }

    /// The piment doré in the hand was pressed (issue #18): its window of `ticks` opens, unless
    /// it is open already.
    pub fn start_golden(&mut self, ticks: u32) {
        if self.golden == 0
            && matches!(
                self.hand,
                Slot::Ready {
                    item: ItemKind::PimentDore,
                    ..
                }
            )
        {
            self.golden = ticks.max(1);
        }
    }

    /// One tick of the roulettes: a spinning slot stops on its item when its time is up. The
    /// piment doré's window counts down; closed, the piment doré is used up.
    pub fn tick(&mut self) {
        if self.golden > 0 {
            self.golden -= 1;
            if self.golden == 0
                && matches!(
                    self.hand,
                    Slot::Ready {
                        item: ItemKind::PimentDore,
                        ..
                    }
                )
            {
                self.shift();
            }
        }
        for slot in [&mut self.hand, &mut self.reserve] {
            if let Slot::Spinning { item, count, ticks } = *slot {
                *slot = if ticks <= 1 {
                    Slot::Ready { item, count }
                } else {
                    Slot::Spinning {
                        item,
                        count,
                        ticks: ticks - 1,
                    }
                };
            }
        }
    }

    /// The reserve moves into the emptied hand.
    fn shift(&mut self) {
        self.hand = self.reserve;
        self.reserve = Slot::Empty;
        self.deployed = false;
    }

    /// One of the hand's items is used: one less, the reserve moves in after the last one.
    fn use_one(&mut self) {
        if let Slot::Ready { item, count } = self.hand {
            if count > 1 {
                self.hand = Slot::Ready {
                    item,
                    count: count - 1,
                };
            } else {
                self.shift();
            }
        }
    }

    /// One tick of the item button: `held` its state, `back` whether the "back" direction is
    /// held (a throw goes backwards).
    pub fn step_button(&mut self, held: bool, back: bool) -> Use {
        let pressed = held && !self.button;
        let released = !held && self.button;
        self.button = held;
        if released && let Some(item) = self.dragging.take() {
            return Use::Release { item, back };
        }
        if !pressed || self.dragging.is_some() {
            return Use::None;
        }
        let Slot::Ready { item, count } = self.hand else {
            // Nothing, or the roulette still spins.
            return Use::None;
        };
        if self.deployed {
            self.use_one();
            return Use::Fire { item, back };
        }
        if count == 1 && item.draggable() {
            self.dragging = Some(item);
            self.shift();
            return Use::Drag(item);
        }
        if count > 1 && item.is_object() {
            self.deployed = true;
            return Use::Deploy { item, count };
        }
        // The piment doré stays in the hand for its window: a boost per press.
        if item != ItemKind::PimentDore {
            self.use_one();
        }
        Use::Instant { item, back }
    }

    /// Items held out around the kart: the dragged one, or the "x3" out (`Some((item, count))`).
    pub fn held_out(&self) -> Option<(ItemKind, u8)> {
        if let Some(item) = self.dragging {
            return Some((item, 1));
        }
        match self.hand {
            Slot::Ready { item, count } if self.deployed => Some((item, count)),
            _ => None,
        }
    }

    /// Hit by an item or carried by the pelican: what the kart held out is lost (the dragged
    /// item, the rest of an "x3" out). The slots keep the rest. Returns whether it lost anything.
    pub fn lose_held_out(&mut self) -> bool {
        let dragging = self.dragging.take().is_some();
        let deployed = self.deployed;
        if deployed {
            self.shift();
        }
        dragging || deployed
    }

    /// One of the items held out broke (it stopped another item, or hit a kart): the dragged
    /// one, else one of the "x3" out (the reserve moves in after the last one).
    pub fn lose_one_held_out(&mut self) {
        if self.dragging.take().is_none() && self.deployed {
            self.use_one();
        }
    }

    /// A box this kart took was another kart's after all (online, issue #53: the host gave it to
    /// a kart that touched it first): the roulettes it started stop, empty, in the slots it
    /// `filled` (hand, reserve). An item already out of its roulette stays (the verdict came too
    /// late to take it back). The reserve moves into an emptied hand.
    pub fn revoke(&mut self, filled: [bool; 2]) {
        for (slot, filled) in [&mut self.hand, &mut self.reserve].into_iter().zip(filled) {
            if filled && matches!(slot, Slot::Spinning { .. }) {
                *slot = Slot::Empty;
            }
        }
        if self.hand.is_empty() && !self.reserve.is_empty() {
            self.shift();
        }
    }

    /// Online (issue #61): the host refused the oreiller géant this kart drew (its rules forbid
    /// one now): the kart gets `count` of `item` instead (the same draw without the oreiller, as
    /// solo), in the slot that holds the oreiller (the reserve first: the later draw), its
    /// roulette going on. Returns false when no slot holds one (it was thrown already).
    pub fn swap_pillow(&mut self, item: ItemKind, count: u8) -> bool {
        for slot in [&mut self.reserve, &mut self.hand] {
            match *slot {
                Slot::Spinning {
                    item: ItemKind::OreillerGeant,
                    ticks,
                    ..
                } => {
                    *slot = Slot::Spinning { item, count, ticks };
                    return true;
                }
                Slot::Ready {
                    item: ItemKind::OreillerGeant,
                    ..
                } => {
                    *slot = Slot::Ready { item, count };
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Whether a slot holds `item` (ready, or in its roulette).
    pub fn holds(&self, item: ItemKind) -> bool {
        [self.hand, self.reserve]
            .iter()
            .any(|slot| slot.item().is_some_and(|(held, _)| held == item))
    }

    /// Dégonflé (issue #18): its items fall: both slots (roulettes and the piment doré's window
    /// too) and what it held out. The draws go on counting. Returns whether it lost anything.
    pub fn drop_all(&mut self) -> bool {
        let emptied = Self {
            button: self.button,
            draws: self.draws,
            ..Self::default()
        };
        let lost = *self != emptied;
        *self = emptied;
        lost
    }

    /// Everything is gone (a race restarts, the items are switched off): empty slots, nothing
    /// out, no draw yet.
    pub fn clear(&mut self) {
        *self = Self {
            button: self.button,
            ..Self::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ItemKind::*;

    fn ready(item: ItemKind, count: u8) -> Slot {
        Slot::Ready { item, count }
    }

    /// Presses the button once (one tick down, one tick up), `back` held or not.
    fn press(slots: &mut ItemSlots, back: bool) -> (Use, Use) {
        (
            slots.step_button(true, back),
            slots.step_button(false, back),
        )
    }

    /// Acceptance criterion of #16: a box fills the hand, then the reserve; with both taken it
    /// gives nothing; a double box fills both.
    #[test]
    fn a_box_fills_the_hand_then_the_reserve_and_nothing_when_both_are_taken() {
        let mut slots = ItemSlots::default();
        assert!(slots.has_room());
        assert_eq!(slots.free_slots(), 2);
        assert!(slots.give(Pelure, 1, 0));
        assert_eq!(slots.hand, ready(Pelure, 1));
        assert!(slots.give(Piment, 3, 0));
        assert_eq!(slots.reserve, ready(Piment, 3));
        assert!(!slots.has_room());
        assert!(!slots.give(Abeille, 1, 0));
        assert_eq!(
            (slots.hand, slots.reserve),
            (ready(Pelure, 1), ready(Piment, 3))
        );
        // A double box on empty slots: both.
        let mut double = ItemSlots::default();
        assert!(double.give(Pelote, 1, 90) && double.give(Abeille, 1, 90));
        assert_eq!(double.free_slots(), 0);
    }

    /// The roulette spins `ticks` ticks (GD § 8: about 1.5 s, 90 ticks); the item cannot be used
    /// before it stops.
    #[test]
    fn the_roulette_spins_before_the_item_can_be_used() {
        let mut slots = ItemSlots::default();
        slots.give(Piment, 1, 90);
        for _ in 0..89 {
            slots.tick();
            assert!(matches!(slots.hand, Slot::Spinning { .. }));
            assert_eq!(press(&mut slots, false).0, Use::None);
        }
        slots.tick();
        assert_eq!(slots.hand, ready(Piment, 1));
        assert_eq!(
            press(&mut slots, false).0,
            Use::Instant {
                item: Piment,
                back: false
            }
        );
        assert!(slots.hand.is_empty());
    }

    /// Acceptance criterion of #16: the hand is used first, then the reserve moves into it; a
    /// held button uses one item per press only.
    #[test]
    fn the_reserve_moves_into_the_hand_once_it_is_used() {
        let mut slots = ItemSlots::default();
        slots.give(CroquetteDoree, 1, 0);
        slots.give(Abeille, 1, 0);
        assert_eq!(
            slots.step_button(true, false),
            Use::Instant {
                item: CroquetteDoree,
                back: false
            }
        );
        assert_eq!(slots.hand, ready(Abeille, 1));
        assert!(slots.reserve.is_empty());
        // Still held: nothing more.
        assert_eq!(slots.step_button(true, false), Use::None);
        assert_eq!(slots.step_button(false, false), Use::None);
        // Thrown backwards with the "back" direction held.
        assert_eq!(
            press(&mut slots, true).0,
            Use::Instant {
                item: Abeille,
                back: true
            }
        );
        assert_eq!(slots, ItemSlots::default());
    }

    /// GD § 8: holding the button keeps a pelure or a pelote behind the kart; releasing drops
    /// the pelure or throws the pelote (backwards with the "back" direction held at the
    /// release). Once out, it frees its slot: the reserve moves in, a box can fill the reserve.
    #[test]
    fn a_pelure_or_pelote_is_dragged_while_the_button_is_held() {
        let mut slots = ItemSlots::default();
        slots.give(Pelote, 1, 0);
        slots.give(Piment, 1, 0);
        assert_eq!(slots.step_button(true, false), Use::Drag(Pelote));
        assert_eq!(slots.dragging, Some(Pelote));
        assert_eq!(slots.held_out(), Some((Pelote, 1)));
        assert_eq!(slots.hand, ready(Piment, 1));
        assert!(slots.has_room());
        for _ in 0..100 {
            assert_eq!(slots.step_button(true, false), Use::None);
        }
        assert!(slots.give(Pelure, 1, 0));
        assert_eq!(
            slots.step_button(false, true),
            Use::Release {
                item: Pelote,
                back: true
            }
        );
        assert_eq!(slots.dragging, None);
        assert_eq!(
            (slots.hand, slots.reserve),
            (ready(Piment, 1), ready(Pelure, 1))
        );
        // A tap drops at once.
        let mut tap = ItemSlots::default();
        tap.give(Pelure, 1, 0);
        assert_eq!(
            press(&mut tap, false),
            (
                Use::Drag(Pelure),
                Use::Release {
                    item: Pelure,
                    back: false
                }
            )
        );
    }

    /// An "x3" of pelotes, abeilles or pelures comes out at the first press and stays in the
    /// hand; each next press throws one; after the third the reserve moves in. A "Piment x3" is
    /// three boosts, one per press.
    #[test]
    fn an_x3_comes_out_then_goes_one_press_at_a_time() {
        for item in [Pelote, Abeille, Pelure] {
            let mut slots = ItemSlots::default();
            slots.give(item, 3, 0);
            slots.give(CroquetteDoree, 1, 0);
            assert_eq!(press(&mut slots, false).0, Use::Deploy { item, count: 3 });
            assert_eq!(slots.held_out(), Some((item, 3)));
            for left in [2, 1] {
                assert_eq!(
                    press(&mut slots, left == 1).0,
                    Use::Fire {
                        item,
                        back: left == 1
                    }
                );
                assert_eq!(slots.held_out(), Some((item, left)));
                assert!(!slots.has_room());
            }
            assert_eq!(press(&mut slots, false).0, Use::Fire { item, back: false });
            assert_eq!(slots.held_out(), None);
            assert_eq!(slots.hand, ready(CroquetteDoree, 1));
            assert!(!slots.deployed);
        }
        let mut chili = ItemSlots::default();
        chili.give(Piment, 3, 0);
        for left in [2, 1, 0] {
            assert_eq!(
                press(&mut chili, false).0,
                Use::Instant {
                    item: Piment,
                    back: false
                }
            );
            assert_eq!(
                chili.hand.item().map(|(_, n)| n),
                (left > 0).then_some(left)
            );
        }
    }

    /// Hit (or towed), a kart loses what it held out, not what is in its slots.
    #[test]
    fn a_hit_loses_what_was_held_out() {
        let mut dragging = ItemSlots::default();
        dragging.give(Pelure, 1, 0);
        dragging.give(Abeille, 1, 0);
        dragging.step_button(true, false);
        assert!(dragging.lose_held_out());
        assert_eq!(dragging.held_out(), None);
        assert_eq!(dragging.hand, ready(Abeille, 1));
        // Releasing the button afterwards drops nothing.
        assert_eq!(dragging.step_button(false, false), Use::None);

        let mut orbit = ItemSlots::default();
        orbit.give(Pelote, 3, 0);
        orbit.give(Piment, 1, 0);
        press(&mut orbit, false);
        press(&mut orbit, false);
        assert!(orbit.lose_held_out());
        assert_eq!(orbit.hand, ready(Piment, 1));
        assert!(orbit.reserve.is_empty());

        // One of them breaks on something: one less out.
        let mut shield = ItemSlots::default();
        shield.give(Pelure, 3, 0);
        press(&mut shield, false);
        shield.lose_one_held_out();
        assert_eq!(shield.held_out(), Some((Pelure, 2)));
        let mut dragged = ItemSlots::default();
        dragged.give(Pelote, 1, 0);
        dragged.step_button(true, false);
        dragged.lose_one_held_out();
        assert_eq!(dragged.held_out(), None);
        assert_eq!(dragged.step_button(false, false), Use::None);

        let mut nothing = ItemSlots::default();
        nothing.give(Pelote, 3, 0);
        assert!(!nothing.lose_held_out());
        assert_eq!(nothing.hand, ready(Pelote, 3));
        nothing.clear();
        assert_eq!(nothing, ItemSlots::default());
    }

    /// Issue #18 (GD § 8): a piment doré stays in the hand for its window, from the first press:
    /// every press meanwhile is a boost (`Use::Instant`), the reserve waits; the window closed,
    /// it is used up and the reserve moves in. Before the first press, nothing counts down.
    #[test]
    fn a_piment_dore_boosts_at_every_press_for_its_window() {
        let mut slots = ItemSlots::default();
        slots.give(PimentDore, 1, 0);
        slots.give(Pelote, 1, 0);
        assert!(!slots.ticking());
        for _ in 0..100 {
            slots.tick();
        }
        assert_eq!(
            slots.hand,
            ready(PimentDore, 1),
            "waits for its first press"
        );
        let golden = Use::Instant {
            item: PimentDore,
            back: false,
        };
        assert_eq!(press(&mut slots, false).0, golden);
        slots.start_golden(420);
        assert_eq!(slots.golden, 420);
        assert!(slots.ticking());
        let mut presses = 1;
        for tick in 1..420 {
            slots.tick();
            if tick % 20 == 0 {
                assert_eq!(press(&mut slots, false).0, golden, "tick {tick}");
                // Pressed again: the window does not start over.
                slots.start_golden(420);
                presses += 1;
            }
            assert_eq!(slots.hand, ready(PimentDore, 1));
            assert_eq!(slots.golden, 420 - tick);
        }
        assert_eq!(presses, 21);
        slots.tick();
        assert_eq!(slots.golden, 0);
        assert_eq!(
            slots.hand,
            ready(Pelote, 1),
            "used up, the reserve moves in"
        );
        assert!(slots.reserve.is_empty());
        // Only a piment doré in the hand opens a window.
        let mut other = ItemSlots::default();
        other.give(Piment, 1, 0);
        other.start_golden(420);
        assert_eq!(other.golden, 0);
    }

    /// Issue #18 (debug key): an item forced into the hand replaces what was there, ready, an
    /// "x3" out included; the reserve stays.
    #[test]
    fn a_forced_item_replaces_the_hand() {
        let mut slots = ItemSlots::default();
        slots.give(Pelote, 3, 0);
        slots.give(Piment, 1, 0);
        press(&mut slots, false);
        assert!(slots.deployed);
        slots.force(Bombe, 1);
        assert_eq!(
            (slots.hand, slots.reserve),
            (ready(Bombe, 1), ready(Piment, 1))
        );
        assert_eq!(slots.held_out(), None);
        assert_eq!(
            press(&mut slots, false).0,
            Use::Instant {
                item: Bombe,
                back: false
            }
        );
        assert_eq!(slots.hand, ready(Piment, 1));
    }

    /// Issue #18: dégonflé, a kart drops all its items (slots, roulettes, what it held out, a
    /// piment doré's window); it keeps counting its draws (the next ones stay deterministic).
    #[test]
    fn a_deflated_kart_drops_all_its_items() {
        let mut slots = ItemSlots::default();
        slots.give(Pelote, 3, 0);
        slots.give(Abeille, 1, 90);
        slots.draws = 7;
        press(&mut slots, false);
        assert!(slots.drop_all());
        assert_eq!(
            slots,
            ItemSlots {
                draws: 7,
                ..ItemSlots::default()
            }
        );
        assert!(!slots.drop_all(), "nothing left");
        let mut golden = ItemSlots::default();
        golden.give(PimentDore, 1, 0);
        golden.start_golden(100);
        golden.drop_all();
        assert_eq!((golden.hand, golden.golden), (Slot::Empty, 0));
    }

    /// Issue #53: a box the host gave to another kart: the roulettes it started stop, empty; an
    /// item already out stays; the reserve moves into an emptied hand.
    #[test]
    fn a_box_given_to_another_kart_stops_its_roulettes() {
        let mut slots = ItemSlots::default();
        slots.give(Pelure, 1, 90);
        slots.give(Abeille, 1, 90);
        slots.revoke([true, true]);
        assert_eq!(slots, ItemSlots::default());
        // The hand was already there: only the reserve's roulette stops.
        let mut slots = ItemSlots::default();
        slots.give(Piment, 1, 0);
        slots.give(Pelote, 3, 90);
        slots.revoke([false, true]);
        assert_eq!((slots.hand, slots.reserve), (ready(Piment, 1), Slot::Empty));
        // The hand's roulette stops, the reserve (from an earlier box) moves in.
        let mut slots = ItemSlots::default();
        slots.give(Pelote, 1, 90);
        slots.give(Piment, 1, 0);
        slots.revoke([true, false]);
        assert_eq!((slots.hand, slots.reserve), (ready(Piment, 1), Slot::Empty));
        // Too late: the roulette stopped, the item stays.
        let mut slots = ItemSlots::default();
        slots.give(Abeille, 1, 1);
        slots.tick();
        slots.revoke([true, false]);
        assert_eq!(slots.hand, ready(Abeille, 1));
    }

    /// Issue #61: the host refused an oreiller géant: the slot that holds it gets the other draw
    /// instead, its roulette going on (the reserve first); nothing to swap once it is thrown.
    #[test]
    fn a_refused_oreiller_becomes_the_other_draw() {
        let mut slots = ItemSlots::default();
        slots.give(OreillerGeant, 1, 90);
        slots.tick();
        assert!(slots.holds(OreillerGeant));
        assert!(slots.swap_pillow(Piment, 3));
        assert_eq!(
            slots.hand,
            Slot::Spinning {
                item: Piment,
                count: 3,
                ticks: 89
            }
        );
        assert!(!slots.holds(OreillerGeant));
        assert!(!slots.swap_pillow(Pelure, 1), "no oreiller left");
        // Two oreillers (two boxes, two claims): the reserve's goes first; a ready one too.
        let mut two = ItemSlots::default();
        two.give(OreillerGeant, 1, 0);
        two.give(OreillerGeant, 1, 30);
        assert!(two.swap_pillow(Bombe, 1));
        assert_eq!(two.hand, ready(OreillerGeant, 1));
        assert!(matches!(
            two.reserve,
            Slot::Spinning {
                item: Bombe,
                ticks: 30,
                ..
            }
        ));
        assert!(two.swap_pillow(Chantilly, 1));
        assert_eq!(two.hand, ready(Chantilly, 1));
        // Thrown: the slots no longer hold it.
        let mut thrown = ItemSlots::default();
        thrown.give(OreillerGeant, 1, 0);
        assert_eq!(
            press(&mut thrown, false).0,
            Use::Instant {
                item: OreillerGeant,
                back: false
            }
        );
        assert!(!thrown.swap_pillow(Piment, 1));
        assert_eq!(thrown, ItemSlots::default());
    }
}
