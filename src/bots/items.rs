//! A bot's items (GD § 8 and 12, issue #16): pure logic, no ECS. Unit-tested.
//!
//! The bot presses its item button through its `KartInput` (`item`, and `look_back` to throw
//! backwards), like a player. Simple but believable:
//! - it waits a little once an item is ready (`BotSkill::item_delay`: an Easy bot is slow to use
//!   its items, a Hard one quick);
//! - **piment**: on a straight (no turn close ahead), not already boosted;
//! - **croquette dorée**: as soon as it has room for the croquettes;
//! - **pelure**: dragged behind as a shield (`BotSkill::item_shield`: how often), then dropped
//!   after a few seconds, or at once in front of a kart close behind;
//! - **pelote**: thrown at a kart ahead in its sights (`BotSkill::item_aim`), backwards at a kart
//!   close behind, dragged as a shield meanwhile; thrown ahead anyway after a while;
//! - **abeille**: thrown as soon as a kart is ahead of it in the race (the leader keeps it, or
//!   throws it back at a kart close behind);
//! - an **"x3"**: out at once, then one at a time with the same rules (the abeilles about once a
//!   second, the pelures behind it when a kart follows close or every few seconds).
//! - phase 2 (issue #18):
//!   - **piment doré**: on a straight, then a boost after the other while its window lasts;
//!   - **bombe**: thrown at a kart ahead in its sights (a wider cone than a pelote: it blows up
//!     around), dropped behind for a kart close behind;
//!   - **chantilly**, **dégonflage**, **oreiller géant**: as soon as a kart is ahead in the race
//!     (the leader keeps its oreiller; the others go anyway after a while);
//!   - **super câlin**: with a kart close by to knock over, or on a straight;
//!   - **fusée**: when it is ready (the autopilot follows the racing line);
//!   - **klaxon-canard**: when another kart's item comes close (an oreiller géant above it), or
//!     after a while.
//! - phase 3 (issue #21):
//!   - **pie voleuse**: as soon as a kart is ahead in the race (the leader after a while: it
//!     steals from behind);
//!   - **piñata**: out at once, then one press about every second (the item in front goes);
//!   - **boomerang-os**: like a pelote: at a kart in its sights, backwards at a kart close
//!     behind, ahead anyway after a while (each throw once the last one is back). Issue #88: it
//!     steers toward its target at the throw ([`boomerang_aim`]): the boomerang curves with the
//!     steering.
//!
//! The bots also go for the item boxes a little (`driver.rs`, `Senses::item_box`).

use super::rng::BotRng;
use super::tuning::{BotSkill, BotTuning};
use crate::items::{ItemKind, ItemTuning, boomerang_curve};

/// Mixed into a bot's seed for its item draws: apart from its driving draws.
const ITEM_SEED: u64 = 0x17E3_5EED_0016;

/// What the bot knows about its items and the karts around this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ItemSenses {
    /// The item ready in its hand, and how many (not while its roulette spins).
    pub hand: Option<(ItemKind, u8)>,
    /// What it holds out: a dragged pelure or pelote, or an "x3" out (`ItemSlots::held_out`).
    pub held_out: Option<(ItemKind, u8)>,
    /// It drags one item behind (button held).
    pub dragging: bool,
    /// The closest kart ahead within `BotTuning::item_ahead_range`: distance (m) and angle off
    /// the kart's heading (rad).
    pub ahead: Option<(f32, f32)>,
    /// The closest kart behind within `BotTuning::item_behind_range`: distance and angle off the
    /// straight-back direction.
    pub behind: Option<(f32, f32)>,
    /// Its place in the race (1 = leader; 0 off a race).
    pub rank: u32,
    /// Croquettes held, and the most it can hold.
    pub croquettes: u8,
    pub max_croquettes: u8,
    /// Straight track ahead before the next turn (m).
    pub straight: f32,
    /// Its kart is boosted.
    pub boosting: bool,
    /// It may use items (racing, not spinning or towed).
    pub can_act: bool,
    /// Its piment doré's window is open (issue #18).
    pub golden: bool,
    /// Issue #193: it keeps its boost item (a piment, a piment doré) for the shortcut it heads
    /// for...
    pub saving: bool,
    /// ...and uses it now, at its entrance...
    pub boost_now: bool,
    /// ...and it is on a shortcut: a piment doré's next boost as soon as one ends.
    pub on_shortcut: bool,
    /// Another kart's item is close (issue #18: for the klaxon-canard).
    pub danger: bool,
    /// Tick duration (s).
    pub dt: f32,
}

/// The item button the bot presses this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemButtons {
    /// `KartInput::item`.
    pub item: bool,
    /// `KartInput::look_back`: throws backwards.
    pub back: bool,
}

/// A bot's memory of its items between ticks.
#[derive(bevy::prelude::Component, Clone, Debug)]
pub struct BotItemBrain {
    rng: BotRng,
    /// How long the item in its hand has been ready (s).
    pub ready_for: f32,
    /// The item it waited for (a new one restarts `ready_for`).
    waiting_for: Option<(ItemKind, u8)>,
    /// Keeps the dragged item behind this long more (s).
    pub drag_left: f32,
    /// Next shot of an "x3" in this long (s).
    pub next_shot: f32,
    /// The button last tick: a press needs it released first.
    last: ItemButtons,
}

impl BotItemBrain {
    /// A fresh bot: `seed` is its bot seed (`bots::bot_seed`).
    pub fn new(seed: u64) -> Self {
        Self {
            rng: BotRng::new(seed ^ ITEM_SEED),
            ready_for: 0.0,
            waiting_for: None,
            drag_left: 0.0,
            next_shot: 0.0,
            last: ItemButtons::default(),
        }
    }

    /// A press this tick (the button down after a tick up), `back` to throw backwards.
    fn tap(&self, back: bool) -> ItemButtons {
        if self.last.item {
            ItemButtons::default()
        } else {
            ItemButtons { item: true, back }
        }
    }
}

/// Whether a kart is in its sights ahead: close enough, within `cone` of its heading.
fn in_sights(ahead: Option<(f32, f32)>, cone: f32) -> bool {
    ahead.is_some_and(|(_, angle)| angle.abs() <= cone)
}

/// Whether a kart follows close behind, right in its wake.
fn followed_close(behind: Option<(f32, f32)>) -> bool {
    behind.is_some_and(|(_, angle)| angle.abs() <= 0.35)
}

/// The steering a bot throws its boomerang-os with (issue #88: « ils braquent vers leur
/// cible »): the boomerang curves with it (`items::boomerang_curve`), so the bot steers for the
/// arc that meets the kart `ahead` (distance, angle off its heading, right positive) where that
/// kart will be once the boomerang catches up with it, as if it kept going at the bot's own
/// `speed` along the heading (no further than the boomerang's range). The steering gives no curve
/// below a light touch (the default one), so a kart dead ahead gets the lightest curve that
/// still counts, or the default one when it is closer.
pub fn boomerang_aim(ahead: (f32, f32), speed: f32, tuning: &ItemTuning) -> f32 {
    let (distance, angle) = ahead;
    let v = tuning.boomerang_speed.max(1.0);
    let side = distance * angle.sin();
    let along = (distance * angle.cos()).max(0.0);
    // It catches up at `v - speed` (at least a third of its speed: a kart in a boost).
    let closing = (v - speed.max(0.0)).max(v / 3.0);
    let meet = (along * v / closing)
        .min(tuning.boomerang_range)
        .max(along.min(1.0));
    // The circle tangent to the heading through (side, meet): its curvature, right positive.
    let curvature = 2.0 * side / (side * side + meet * meet).max(1e-3);
    let wanted = -curvature * v;
    let full = tuning.boomerang_steer_curve.max(1e-3);
    let steer = (-wanted / full).clamp(-1.0, 1.0);
    if steer.abs() >= tuning.boomerang_straight_steer {
        return steer;
    }
    // Under the light touch: the lightest curve on its side, or the default one if closer.
    let light = if steer >= 0.0 {
        tuning.boomerang_straight_steer
    } else {
        -tuning.boomerang_straight_steer
    };
    let error = |steer: f32| (boomerang_curve(steer, tuning) - wanted).abs();
    if error(0.0) < error(light) {
        0.0
    } else {
        light
    }
}

/// One tick of the bot's item button.
pub fn item_buttons(
    brain: &mut BotItemBrain,
    skill: &BotSkill,
    tuning: &BotTuning,
    senses: &ItemSenses,
) -> ItemButtons {
    let buttons = decide(brain, skill, tuning, senses);
    brain.last = buttons;
    buttons
}

fn decide(
    brain: &mut BotItemBrain,
    skill: &BotSkill,
    tuning: &BotTuning,
    senses: &ItemSenses,
) -> ItemButtons {
    let dt = senses.dt;
    if !senses.can_act {
        brain.ready_for = 0.0;
        brain.drag_left = 0.0;
        return ItemButtons::default();
    }
    brain.next_shot = (brain.next_shot - dt).max(0.0);

    // Dragging a pelure or pelote: hold it, then let it go (at a kart behind, at a kart in its
    // sights, or when the time is up).
    if senses.dragging {
        brain.drag_left -= dt;
        let pelote = senses
            .held_out
            .is_some_and(|(item, _)| item == ItemKind::Pelote);
        let back = followed_close(senses.behind);
        let release =
            back || brain.drag_left <= 0.0 || (pelote && in_sights(senses.ahead, skill.item_aim));
        return if release {
            ItemButtons {
                item: false,
                back: back && pelote,
            }
        } else {
            ItemButtons {
                item: true,
                back: false,
            }
        };
    }

    // An "x3" out: one at a time.
    if let Some((item, _)) = senses.held_out {
        if brain.next_shot > 0.0 {
            return ItemButtons::default();
        }
        let shot = match item {
            ItemKind::Pelote if in_sights(senses.ahead, skill.item_aim) => Some(false),
            ItemKind::Pelote | ItemKind::Abeille if followed_close(senses.behind) => Some(true),
            ItemKind::Abeille if senses.rank > 1 => Some(false),
            ItemKind::Pelure if followed_close(senses.behind) => Some(false),
            ItemKind::Pelure => {
                (brain.rng.chance(dt / tuning.item_drop_period.max(dt))).then_some(false)
            }
            // Issue #21: the piñata's item in front, one after the other.
            ItemKind::Pinata => Some(false),
            _ => None,
        };
        return match shot {
            Some(back) => {
                let buttons = brain.tap(back);
                if buttons.item {
                    brain.next_shot = tuning.item_shot_period;
                }
                buttons
            }
            None => ItemButtons::default(),
        };
    }

    // An item ready in the hand: after a moment's thought.
    let Some((item, count)) = senses.hand else {
        brain.waiting_for = None;
        brain.ready_for = 0.0;
        return ItemButtons::default();
    };
    if brain.waiting_for != Some((item, count)) {
        brain.waiting_for = Some((item, count));
        brain.ready_for = 0.0;
    }
    brain.ready_for += dt;
    // Issue #193: the boost item kept for a shortcut goes at its entrance, however fresh.
    if senses.boost_now && matches!(item, ItemKind::Piment | ItemKind::PimentDore) {
        return brain.tap(false);
    }
    if brain.ready_for < skill.item_delay {
        return ItemButtons::default();
    }
    let patience_over = brain.ready_for >= skill.item_delay + tuning.item_patience;
    let behind = followed_close(senses.behind);
    let ahead_in_race = senses.rank > 1;
    let press = match (item, count) {
        // Issue #193: on a shortcut, a piment doré's boosts one after the other; the boost item
        // kept for a shortcut stays in the hand until its entrance.
        (ItemKind::PimentDore, _) if senses.golden && senses.on_shortcut => {
            (!senses.boosting).then_some(false)
        }
        (ItemKind::Piment | ItemKind::PimentDore, _) if senses.saving => None,
        // Issue #18.
        (ItemKind::PimentDore, _) if senses.golden => (!senses.boosting
            && senses.straight >= tuning.item_piment_straight * tuning.item_golden_straight)
            .then_some(false),
        (ItemKind::PimentDore, _) => {
            ((senses.straight >= tuning.item_piment_straight && !senses.boosting) || patience_over)
                .then_some(false)
        }
        (ItemKind::Bombe, _) => {
            let [near, far] = tuning.item_bomb_range;
            let aimed = senses.ahead.is_some_and(|(distance, angle)| {
                (near..=far).contains(&distance)
                    && angle.abs() <= skill.item_aim * tuning.item_bomb_cone
            });
            if aimed {
                Some(false)
            } else if behind {
                Some(true)
            } else {
                patience_over.then_some(false)
            }
        }
        (ItemKind::Chantilly | ItemKind::Degonflage, _) => {
            (ahead_in_race || patience_over).then_some(false)
        }
        // Never on itself: the leader keeps it.
        (ItemKind::OreillerGeant, _) => ahead_in_race.then_some(false),
        (ItemKind::SuperCalin, _) => {
            let close = |around: Option<(f32, f32)>| {
                around.is_some_and(|(distance, _)| distance <= tuning.item_ram_range)
            };
            (close(senses.ahead)
                || close(senses.behind)
                || senses.straight >= tuning.item_piment_straight
                || patience_over)
                .then_some(false)
        }
        (ItemKind::Fusee, _) => Some(false),
        (ItemKind::KlaxonCanard, _) => (senses.danger || patience_over).then_some(false),
        (ItemKind::CroquetteDoree, _) => {
            (senses.croquettes < senses.max_croquettes || patience_over).then_some(false)
        }
        (ItemKind::Piment, _) => {
            ((senses.straight >= tuning.item_piment_straight && !senses.boosting) || patience_over)
                .then_some(false)
        }
        // Issue #21.
        (ItemKind::PieVoleuse, _) => (ahead_in_race || patience_over).then_some(false),
        (ItemKind::Boomerang, _) if in_sights(senses.ahead, skill.item_aim) => Some(false),
        (ItemKind::Boomerang, _) if behind => Some(true),
        (ItemKind::Boomerang, _) => patience_over.then_some(false),
        // An "x3" comes out at once, a piñata too.
        (ItemKind::Pelure | ItemKind::Pelote | ItemKind::Abeille, 2..) | (ItemKind::Pinata, _) => {
            Some(false)
        }
        (ItemKind::Abeille, _) if senses.rank > 1 => Some(false),
        (ItemKind::Abeille, _) if behind => Some(true),
        (ItemKind::Abeille, _) => patience_over.then_some(false),
        (ItemKind::Pelote, _) if in_sights(senses.ahead, skill.item_aim) => Some(false),
        (ItemKind::Pelote, _) if behind => Some(true),
        (ItemKind::Pelure | ItemKind::Pelote, _) => {
            // Dragged as a shield (held), or dropped / thrown ahead after a while.
            if brain.rng.chance(skill.item_shield) {
                brain.drag_left = brain
                    .rng
                    .range(tuning.item_shield_time[0], tuning.item_shield_time[1]);
                return brain.tap(false);
            }
            (item == ItemKind::Pelure || patience_over).then_some(false)
        }
    };
    match press {
        Some(back) => brain.tap(back),
        None => ItemButtons::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::BotDifficulty;

    const DT: f32 = 1.0 / 60.0;

    /// Issue #18: each item of phase 2 goes at its moment, from the item's delay on.
    #[test]
    fn phase_two_items_go_at_their_moment() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Normal);
        let ticks = (skill.item_delay / DT) as usize + 5;
        let press = |senses: &ItemSenses| {
            first_press(&mut BotItemBrain::new(6), skill, &tuning, senses, ticks)
                .map(|(_, buttons)| buttons)
        };
        let ahead = ItemButtons {
            item: true,
            back: false,
        };
        let back = ItemButtons {
            item: true,
            back: true,
        };
        // A bombe: at a kart ahead in the blast's range, dropped for one close behind.
        let mut bomb = senses(Some((ItemKind::Bombe, 1)));
        assert_eq!(press(&bomb), None);
        bomb.ahead = Some((15.0, 0.2));
        assert_eq!(press(&bomb), Some(ahead));
        bomb.ahead = Some((3.0, 0.0));
        assert_eq!(press(&bomb), None, "too close: its own blast");
        bomb.ahead = None;
        bomb.behind = Some((5.0, 0.1));
        assert_eq!(press(&bomb), Some(back));
        // Chantilly, dégonflage, oreiller géant: with a kart ahead in the race.
        for item in [
            ItemKind::Chantilly,
            ItemKind::Degonflage,
            ItemKind::OreillerGeant,
        ] {
            let mut leader = senses(Some((item, 1)));
            leader.rank = 1;
            assert_eq!(press(&leader), None, "{item:?}");
            assert_eq!(press(&senses(Some((item, 1)))), Some(ahead), "{item:?}");
        }
        // The leader never throws its oreiller, even after a long while.
        let mut leader = senses(Some((ItemKind::OreillerGeant, 1)));
        leader.rank = 1;
        let long = ((skill.item_delay + 3.0 * tuning.item_patience) / DT) as usize;
        assert!(first_press(&mut BotItemBrain::new(6), skill, &tuning, &leader, long).is_none());
        // Super câlin: a kart close by (or a straight).
        let mut calin = senses(Some((ItemKind::SuperCalin, 1)));
        calin.straight = 10.0;
        assert_eq!(press(&calin), None);
        calin.behind = Some((6.0, 0.4));
        assert_eq!(press(&calin), Some(ahead));
        // Fusée: when ready.
        assert_eq!(press(&senses(Some((ItemKind::Fusee, 1)))), Some(ahead));
        // Klaxon: when an item comes close.
        let mut horn = senses(Some((ItemKind::KlaxonCanard, 1)));
        assert_eq!(press(&horn), None);
        horn.danger = true;
        assert_eq!(press(&horn), Some(ahead));
    }

    /// Issue #21: the pie voleuse with a kart ahead in the race (the leader after a while); the
    /// piñata out at once, then its items one press at a time; the boomerang-os at a kart in its
    /// sights, backwards at a kart close behind.
    #[test]
    fn phase_three_items_go_at_their_moment() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Normal);
        let ticks = (skill.item_delay / DT) as usize + 5;
        let press = |senses: &ItemSenses| {
            first_press(&mut BotItemBrain::new(9), skill, &tuning, senses, ticks)
                .map(|(_, buttons)| buttons)
        };
        let ahead = ItemButtons {
            item: true,
            back: false,
        };
        let back = ItemButtons {
            item: true,
            back: true,
        };
        let pie = senses(Some((ItemKind::PieVoleuse, 1)));
        assert_eq!(press(&pie), Some(ahead));
        let mut leader = pie;
        leader.rank = 1;
        assert_eq!(press(&leader), None);
        let long = ((skill.item_delay + tuning.item_patience) / DT) as usize + 5;
        assert!(first_press(&mut BotItemBrain::new(9), skill, &tuning, &leader, long).is_some());
        // The piñata: out at once, then a press every shot period.
        assert_eq!(press(&senses(Some((ItemKind::Pinata, 8)))), Some(ahead));
        let mut out = senses(None);
        out.held_out = Some((ItemKind::Pinata, 8));
        let shots = run(&mut BotItemBrain::new(9), skill, &tuning, &out, 200);
        assert!(shots.len() >= 3, "{shots:?}");
        for pair in shots.windows(2) {
            let gap = (pair[1].0 - pair[0].0) as f32 * DT;
            assert!(gap >= tuning.item_shot_period - 1e-3, "{gap}");
        }
        // The boomerang-os: in its sights, behind, or after a while.
        let mut boomerang = senses(Some((ItemKind::Boomerang, 3)));
        assert_eq!(press(&boomerang), None);
        boomerang.ahead = Some((18.0, 0.02));
        assert_eq!(press(&boomerang), Some(ahead));
        boomerang.ahead = None;
        boomerang.behind = Some((5.0, 0.1));
        assert_eq!(press(&boomerang), Some(back));
    }

    /// Issue #88: a bot steers its boomerang-os toward its target at the throw: a kart ahead on
    /// its right, to the right; on its left, to the left; further to the side, more (up to full
    /// lock); dead ahead, the lightest curve that counts. The arc it gets passes close to where
    /// the target will be, much closer than the default curve.
    #[test]
    fn bots_steer_their_boomerang_toward_their_target() {
        let t = ItemTuning::default();
        let speed = 25.0;
        let right = boomerang_aim((12.0, 0.15), speed, &t);
        let left = boomerang_aim((12.0, -0.15), speed, &t);
        assert!(right >= t.boomerang_straight_steer, "{right}");
        assert!((left + right).abs() < 1e-5, "{left} {right}");
        let wide = boomerang_aim((12.0, 0.4), speed, &t);
        assert!(wide > right, "{wide} {right}");
        assert_eq!(boomerang_aim((6.0, 1.2), speed, &t), 1.0, "full lock");
        let ahead = boomerang_aim((15.0, 0.0), speed, &t);
        assert_eq!(ahead.abs(), t.boomerang_straight_steer, "{ahead}");
        // Where the arc passes, against where the target will be (a slower kart: within range).
        for (distance, angle) in [(10.0f32, 0.2f32), (8.0, -0.3), (12.0, 0.1)] {
            let speed = 20.0;
            let v = t.boomerang_speed;
            let along = distance * angle.cos();
            let meet = (along * v / (v - speed)).min(t.boomerang_range);
            let side = distance * angle.sin();
            // The lateral offset of an arc of yaw rate `curve` (left positive) after `meet` m.
            let offset = |curve: f32| {
                let radius = v / curve.abs().max(1e-6);
                let swept = meet / radius;
                -curve.signum() * radius * (1.0 - swept.cos())
            };
            let aimed = offset(boomerang_curve(
                boomerang_aim((distance, angle), speed, &t),
                &t,
            ));
            let straight = offset(t.boomerang_curve);
            assert!(
                (aimed - side).abs() < 1.0 && (aimed - side).abs() < (straight - side).abs(),
                "{distance} m, {angle} rad: aimed {aimed}, straight {straight}, target {side}"
            );
        }
    }

    /// Issue #18: a piment doré on a straight, then a boost after the other while its window is
    /// open (when the last boost is over), none in a turn.
    #[test]
    fn a_piment_dore_boosts_one_after_the_other() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Hard);
        let mut brain = BotItemBrain::new(7);
        let ticks = (skill.item_delay / DT) as usize + 3;
        let golden = senses(Some((ItemKind::PimentDore, 1)));
        assert!(first_press(&mut brain, skill, &tuning, &golden, ticks).is_some());
        let open = ItemSenses {
            golden: true,
            ..golden
        };
        // Unboosted: again (the button up one tick, down the next).
        assert_eq!(run(&mut brain, skill, &tuning, &open, 2).len(), 1);
        let boosted = ItemSenses {
            boosting: true,
            ..open
        };
        assert!(run(&mut brain, skill, &tuning, &boosted, 60).is_empty());
        let turn = ItemSenses {
            straight: 5.0,
            ..open
        };
        assert!(run(&mut brain, skill, &tuning, &turn, 60).is_empty());
    }

    fn senses(hand: Option<(ItemKind, u8)>) -> ItemSenses {
        ItemSenses {
            hand,
            rank: 5,
            max_croquettes: 10,
            straight: 100.0,
            can_act: true,
            dt: DT,
            ..Default::default()
        }
    }

    /// Runs `ticks` ticks with `senses`; the ticks the button went down (a press) and how.
    fn run(
        brain: &mut BotItemBrain,
        skill: &BotSkill,
        tuning: &BotTuning,
        senses: &ItemSenses,
        ticks: usize,
    ) -> Vec<(usize, ItemButtons)> {
        let mut presses = Vec::new();
        let mut last = false;
        for tick in 0..ticks {
            let buttons = item_buttons(brain, skill, tuning, senses);
            if buttons.item && !last {
                presses.push((tick, buttons));
            }
            last = buttons.item;
        }
        presses
    }

    /// The first press within `ticks` ticks (the kart would then change: the test stops there).
    fn first_press(
        brain: &mut BotItemBrain,
        skill: &BotSkill,
        tuning: &BotTuning,
        senses: &ItemSenses,
        ticks: usize,
    ) -> Option<(usize, ItemButtons)> {
        (0..ticks).find_map(|tick| {
            let buttons = item_buttons(brain, skill, tuning, senses);
            buttons.item.then_some((tick, buttons))
        })
    }

    /// A bot thinks a moment before using an item: an Easy bot longer than a Hard one.
    #[test]
    fn harder_bots_use_their_items_sooner() {
        let tuning = BotTuning::default();
        let first_use = |difficulty: BotDifficulty| {
            let mut brain = BotItemBrain::new(1);
            first_press(
                &mut brain,
                tuning.skill(difficulty),
                &tuning,
                &senses(Some((ItemKind::Piment, 1))),
                600,
            )
            .map(|(tick, _)| tick)
        };
        let (easy, normal, hard) = (
            first_use(BotDifficulty::Easy).unwrap(),
            first_use(BotDifficulty::Normal).unwrap(),
            first_use(BotDifficulty::Hard).unwrap(),
        );
        assert!(hard < normal && normal < easy, "{hard} {normal} {easy}");
    }

    /// The piment on a straight, not on the way into a turn, and not on top of a boost (unless
    /// it waited too long); a croquette dorée as soon as there is room for it.
    #[test]
    fn a_piment_on_a_straight_a_croquette_doree_when_there_is_room() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Hard);
        let mut turn = senses(Some((ItemKind::Piment, 1)));
        turn.straight = 10.0;
        let ticks = ((skill.item_delay + tuning.item_patience) / DT) as usize - 2;
        let press = |senses: &ItemSenses| {
            first_press(&mut BotItemBrain::new(2), skill, &tuning, senses, ticks).is_some()
        };
        assert!(!press(&turn));
        let mut boosted = senses(Some((ItemKind::Piment, 1)));
        boosted.boosting = true;
        assert!(!press(&boosted));
        assert!(press(&senses(Some((ItemKind::Piment, 1)))));
        let mut full = senses(Some((ItemKind::CroquetteDoree, 1)));
        full.croquettes = 10;
        assert!(!press(&full));
        full.croquettes = 7;
        assert!(press(&full));
    }

    /// Issue #193: a piment (or a piment doré) kept for the shortcut ahead is not used on the
    /// straight, even after a long wait; it goes at the shortcut's entrance at once, however
    /// fresh; on the shortcut, a piment doré's next boost as soon as one ends.
    #[test]
    fn a_piment_kept_for_a_shortcut_goes_at_its_entrance() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Easy);
        let long = ((skill.item_delay + 3.0 * tuning.item_patience) / DT) as usize;
        for item in [ItemKind::Piment, ItemKind::PimentDore] {
            let mut kept = senses(Some((item, 1)));
            kept.saving = true;
            let mut brain = BotItemBrain::new(4);
            assert_eq!(first_press(&mut brain, skill, &tuning, &kept, long), None);
            let mut now = kept;
            now.boost_now = true;
            assert_eq!(
                first_press(&mut BotItemBrain::new(4), skill, &tuning, &now, 1).map(|p| p.0),
                Some(0),
                "{item:?}"
            );
        }
        let mut golden = senses(Some((ItemKind::PimentDore, 1)));
        golden.golden = true;
        golden.on_shortcut = true;
        golden.straight = 0.0;
        assert!(first_press(&mut BotItemBrain::new(5), skill, &tuning, &golden, long).is_some());
        golden.boosting = true;
        assert!(first_press(&mut BotItemBrain::new(5), skill, &tuning, &golden, long).is_none());
    }

    /// A pelote goes at a kart in its sights ahead, backwards at a kart close behind; an abeille
    /// as soon as a kart is ahead in the race, and the leader throws it back at a follower.
    #[test]
    fn projectiles_go_at_a_kart_ahead_or_back_at_a_follower() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Normal);
        let ticks = (skill.item_delay / DT) as usize + 5;
        let press = |senses: &ItemSenses, seed| {
            first_press(&mut BotItemBrain::new(seed), skill, &tuning, senses, ticks)
                .map(|(_, buttons)| buttons)
        };
        let mut aimed = senses(Some((ItemKind::Pelote, 1)));
        aimed.ahead = Some((15.0, 0.02));
        assert_eq!(
            press(&aimed, 3),
            Some(ItemButtons {
                item: true,
                back: false
            })
        );
        let mut chased = senses(Some((ItemKind::Pelote, 1)));
        chased.behind = Some((6.0, 0.1));
        assert_eq!(
            press(&chased, 3),
            Some(ItemButtons {
                item: true,
                back: true
            })
        );

        let bee = senses(Some((ItemKind::Abeille, 1)));
        assert_eq!(
            press(&bee, 4),
            Some(ItemButtons {
                item: true,
                back: false
            })
        );
        let mut leader = senses(Some((ItemKind::Abeille, 1)));
        leader.rank = 1;
        assert_eq!(press(&leader, 4), None);
        leader.behind = Some((5.0, -0.2));
        assert_eq!(
            press(&leader, 4),
            Some(ItemButtons {
                item: true,
                back: true
            })
        );
    }

    /// A pelure dragged as a shield is held, then dropped after a while, or at once when a kart
    /// follows close; a pelote dragged goes at a kart that comes into its sights.
    #[test]
    fn a_shield_is_held_then_let_go() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Hard);
        assert!(skill.item_shield > 0.5);
        // Find a seed that shields (Hard: most of the time): pressed, and kept down.
        let ticks = (skill.item_delay / DT) as usize + 3;
        let mut brain = (0..20)
            .map(BotItemBrain::new)
            .find_map(|mut b| {
                let pelure = senses(Some((ItemKind::Pelure, 1)));
                first_press(&mut b, skill, &tuning, &pelure, ticks)?;
                (b.drag_left > 0.0).then_some(b)
            })
            .expect("a Hard bot shields");
        assert!(brain.last.item, "pressed and held");
        let mut dragging = senses(None);
        dragging.dragging = true;
        dragging.held_out = Some((ItemKind::Pelure, 1));
        assert!(item_buttons(&mut brain, skill, &tuning, &dragging).item);
        // A kart close behind: dropped in its way.
        let mut followed = dragging;
        followed.behind = Some((4.0, 0.0));
        assert!(!item_buttons(&mut brain, skill, &tuning, &followed).item);
        // Otherwise held until its time is up.
        let mut held_brain = brain.clone();
        held_brain.drag_left = 1.0;
        held_brain.last.item = true;
        let mut held_ticks = 0;
        while item_buttons(&mut held_brain, skill, &tuning, &dragging).item {
            held_ticks += 1;
            assert!(held_ticks < 120);
        }
        assert!((55..=61).contains(&held_ticks), "{held_ticks}");
        // A dragged pelote goes at a kart in its sights.
        let mut aiming = dragging;
        aiming.held_out = Some((ItemKind::Pelote, 1));
        aiming.ahead = Some((20.0, 0.0));
        let mut pelote_brain = brain.clone();
        pelote_brain.drag_left = 5.0;
        let buttons = item_buttons(&mut pelote_brain, skill, &tuning, &aiming);
        assert!(!buttons.item && !buttons.back);
    }

    /// An "x3" comes out at once, then goes one shot at a time, no faster than the shot period.
    #[test]
    fn an_x3_goes_one_shot_at_a_time() {
        let tuning = BotTuning::default();
        let skill = tuning.skill(BotDifficulty::Hard);
        let mut brain = BotItemBrain::new(5);
        let ticks = (skill.item_delay / DT) as usize + 3;
        let x3 = senses(Some((ItemKind::Abeille, 3)));
        assert!(first_press(&mut brain, skill, &tuning, &x3, ticks).is_some());
        let mut orbit = senses(None);
        orbit.held_out = Some((ItemKind::Abeille, 3));
        let shots = run(&mut brain, skill, &tuning, &orbit, 180);
        assert!(shots.len() >= 2, "{shots:?}");
        for pair in shots.windows(2) {
            let gap = (pair[1].0 - pair[0].0) as f32 * DT;
            assert!(gap >= tuning.item_shot_period - 1e-3, "{gap}");
        }
        // Spinning or towed: nothing.
        let mut idle = orbit;
        idle.can_act = false;
        assert!(run(&mut brain, skill, &tuning, &idle, 120).is_empty());
    }
}
