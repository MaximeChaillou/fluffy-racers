//! A bot's paw (GD § 12 and 15, issue #34): pure logic, no ECS. Unit-tested.
//!
//! The bot uses the Interaction button through its `KartInput` (`interact`), like a player, and
//! its jump / drift button to break free. With its level ([`BotSkill`]):
//! - **an occasion** is a kart coming in reach while the paw is ready (GD § 15). It strikes in
//!   `strike_chance` of them (Easy 10 %, Normal 30 %, Hard 60 %), never in its own drift or hop
//!   (it keeps its line), never at the same kart twice within `interaction_retarget` (5 s). A Hard
//!   bot prefers to push toward the nearer edge of the track (`strike_edge_bias`), and strikes
//!   almost always a kart its push would take off the road (issue #196: `strike_payoff_chance`);
//! - **it grabs** a faster kart passing it (`grab_min_speed_gain`, a boost) in `grab_chance` of
//!   the occasions (Easy never), holds it `grab_hold`, then lets go; a Hard bot throws it away
//!   from itself (`throw`);
//! - **held**, it breaks free with a jump after `escape_delay` (1 / 0.5 / 0.25 s);
//! - no preference for the player: the karts in reach are all the same to it.

use bevy::prelude::*;

use super::rng::BotRng;
use super::tuning::{BotSkill, BotTuning};

/// Mixed into a bot's seed for its paw's draws: apart from its driving and item draws.
const PAW_SEED: u64 = 0x9A_3F00_5EED_0034;
/// The karts a bot keeps track of (in reach, struck lately): more are ignored, never a fault.
const TRACKED: usize = 4;

/// A kart within the paw's reach, as the bot sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReachKart {
    pub entity: Entity,
    /// To the right (1) or the left (-1).
    pub side: f32,
    /// Just ahead (only a grab reaches it), rather than to the side.
    pub ahead: bool,
    /// How much faster it goes than the bot, along the bot's heading (m/s).
    pub faster_by: f32,
    /// It is on the side of the track's nearer edge (a wall, the grass, the void).
    pub toward_edge: bool,
    /// Issue #196: a strike would push it off the road (it is near the edge on the side of the
    /// push: into the grass, the void, against a wall).
    pub pays_off: bool,
}

/// What the bot knows about its paw and the karts in reach this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct PawSenses<'a> {
    /// It may use its paw (racing, not hit, towed or past the line).
    pub can_act: bool,
    /// The paw is ready (no cooldown, nothing going on).
    pub ready: bool,
    /// In its own drift or hop.
    pub drifting: bool,
    /// On the ground (it may grab).
    pub grounded: bool,
    /// The paw is reaching out (a grab, before it caught anything).
    pub reaching: bool,
    /// Holding a kart: since how long (s), on which side.
    pub holding: Option<(f32, f32)>,
    /// Held by a kart, since how long (s).
    pub held_for: Option<f32>,
    /// How long a press must last to grab (`InteractionTuning::tap_time`, s).
    pub tap_time: f32,
    /// The karts in reach now.
    pub in_reach: &'a [ReachKart],
    /// Tick duration (s).
    pub dt: f32,
}

/// The buttons the bot's paw presses this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PawButtons {
    /// `KartInput::interact`.
    pub interact: bool,
    /// A press of jump / drift: breaking free.
    pub jump: bool,
    /// The stick, for a throw (the tick it lets go).
    pub steer: Option<f32>,
}

/// What the bot does with its paw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PawPlan {
    #[default]
    Idle,
    /// A paw strike: down one tick, released the next (the strike goes on the release).
    Tap,
    /// A grab: the button held for `time` s so far.
    Grab { time: f32 },
}

/// A bot's memory of its paw between ticks.
#[derive(Component, Clone, Debug)]
pub struct BotPawBrain {
    rng: BotRng,
    plan: PawPlan,
    /// The karts in reach on the last tick: a kart coming in reach is an occasion.
    seen: [Option<Entity>; TRACKED],
    /// The karts it went for lately, and how long ago (s).
    recent: [Option<(Entity, f32)>; TRACKED],
    /// It already jumped to break free of this tether.
    jumped: bool,
    /// Occasions it had, and what it made of them (tests, logs).
    pub occasions: u32,
    pub strikes: u32,
    pub grabs: u32,
}

impl BotPawBrain {
    /// A fresh bot: `seed` is its bot seed (`bots::bot_seed`).
    pub fn new(seed: u64) -> Self {
        Self {
            rng: BotRng::new(seed ^ PAW_SEED),
            plan: PawPlan::Idle,
            seen: [None; TRACKED],
            recent: [None; TRACKED],
            jumped: false,
            occasions: 0,
            strikes: 0,
            grabs: 0,
        }
    }

    fn went_for_lately(&self, kart: Entity) -> bool {
        self.recent.iter().flatten().any(|(e, _)| *e == kart)
    }

    /// Remembers it went for `kart` (the oldest one is forgotten when full).
    fn went_for(&mut self, kart: Entity) {
        let slot = self
            .recent
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                (0..TRACKED)
                    .max_by(|a, b| {
                        let age = |i: usize| self.recent[i].map_or(0.0, |(_, t)| t);
                        age(*a).total_cmp(&age(*b))
                    })
                    .unwrap_or(0)
            });
        self.recent[slot] = Some((kart, 0.0));
    }
}

/// One tick of the bot's paw.
pub fn paw_buttons(
    brain: &mut BotPawBrain,
    skill: &BotSkill,
    tuning: &BotTuning,
    senses: &PawSenses,
) -> PawButtons {
    let dt = senses.dt;
    for recent in &mut brain.recent {
        if let Some((_, age)) = recent {
            *age += dt;
            if *age >= tuning.interaction_retarget {
                *recent = None;
            }
        }
    }
    // The occasions: the karts coming in reach now.
    let mut fresh = [None::<ReachKart>; TRACKED];
    for (i, kart) in senses.in_reach.iter().take(TRACKED).enumerate() {
        if !brain.seen.contains(&Some(kart.entity)) {
            fresh[i] = Some(*kart);
        }
    }
    brain.seen = [None; TRACKED];
    for (seen, kart) in brain.seen.iter_mut().zip(senses.in_reach) {
        *seen = Some(kart.entity);
    }
    if !senses.can_act {
        brain.plan = PawPlan::Idle;
        brain.jumped = false;
        return PawButtons::default();
    }

    let mut buttons = PawButtons::default();
    // Held: a jump after a moment, to break free (the tether breaks when it lands).
    match senses.held_for {
        Some(held) if held >= skill.escape_delay && !brain.jumped && senses.grounded => {
            brain.jumped = true;
            buttons.jump = true;
        }
        Some(_) => {}
        None => brain.jumped = false,
    }

    match brain.plan {
        PawPlan::Tap => {
            // Released: the strike goes.
            brain.plan = PawPlan::Idle;
            return buttons;
        }
        PawPlan::Grab { time } => {
            let time = time + dt;
            let pressing = time < senses.tap_time + 2.0 * dt;
            match senses.holding {
                Some((held, side)) if held >= skill.grab_hold => {
                    // Lets go: away from itself for a throw (it steers toward the kart's side).
                    brain.plan = PawPlan::Idle;
                    if skill.throw {
                        buttons.steer = Some(side * 0.8);
                    }
                }
                Some(_) => {
                    brain.plan = PawPlan::Grab { time };
                    buttons.interact = true;
                }
                None if pressing || senses.reaching => {
                    brain.plan = PawPlan::Grab { time };
                    buttons.interact = true;
                }
                // Nobody caught, or the tether is over.
                None => brain.plan = PawPlan::Idle,
            }
            return buttons;
        }
        PawPlan::Idle => {}
    }
    if !senses.ready {
        return buttons;
    }
    for kart in fresh.iter().flatten() {
        brain.occasions += 1;
        if brain.went_for_lately(kart.entity) {
            continue;
        }
        // A faster kart passing: hold on to it, to get towed.
        let grab = senses.grounded
            && skill.grab_chance > 0.0
            && kart.faster_by >= tuning.grab_min_speed_gain
            && brain.rng.chance(skill.grab_chance);
        if grab {
            brain.went_for(kart.entity);
            brain.grabs += 1;
            brain.plan = PawPlan::Grab { time: 0.0 };
            buttons.interact = true;
            return buttons;
        }
        if kart.ahead || senses.drifting {
            continue;
        }
        let bias = if kart.toward_edge {
            1.0 + skill.strike_edge_bias
        } else {
            1.0 - skill.strike_edge_bias
        };
        let mut chance = skill.strike_chance * bias;
        // Issue #196: where the push takes it off the road, a Hard bot (almost) always strikes.
        if kart.pays_off {
            chance = chance.max(skill.strike_payoff_chance);
        }
        if brain.rng.chance(chance.clamp(0.0, 1.0)) {
            brain.went_for(kart.entity);
            brain.strikes += 1;
            brain.plan = PawPlan::Tap;
            buttons.interact = true;
            return buttons;
        }
    }
    buttons
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::BotDifficulty;

    const DT: f32 = 1.0 / 60.0;

    fn entity(index: u32) -> Entity {
        Entity::from_raw_u32(index).unwrap()
    }

    fn beside(index: u32) -> ReachKart {
        ReachKart {
            entity: entity(index),
            side: 1.0,
            ahead: false,
            faster_by: 0.0,
            toward_edge: false,
            pays_off: false,
        }
    }

    fn senses(in_reach: &[ReachKart]) -> PawSenses<'_> {
        PawSenses {
            can_act: true,
            ready: true,
            grounded: true,
            tap_time: 0.2,
            in_reach,
            dt: DT,
            ..default()
        }
    }

    /// Share of the occasions a bot of `difficulty` strikes at: a new kart comes in reach every
    /// other tick.
    fn strike_rate(difficulty: BotDifficulty, toward_edge: bool) -> f32 {
        strike_rate_where(difficulty, toward_edge, false)
    }

    /// [`strike_rate`], the push taking the kart off the road (`pays_off`) or not.
    fn strike_rate_where(difficulty: BotDifficulty, toward_edge: bool, pays_off: bool) -> f32 {
        let tuning = BotTuning::default();
        let skill = *tuning.skill(difficulty);
        let mut struck = 0;
        let tries = 4000;
        for seed in 0..tries {
            let mut brain = BotPawBrain::new(seed);
            let kart = ReachKart {
                toward_edge,
                pays_off,
                ..beside(7)
            };
            let buttons = paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart]));
            if buttons.interact {
                struck += 1;
            }
        }
        struck as f32 / tries as f32
    }

    /// GD § 15: 10 / 30 / 60 % of the occasions; a Hard bot prefers the karts toward the edge.
    #[test]
    fn bots_strike_in_a_share_of_the_occasions_with_their_level() {
        for (difficulty, chance) in [
            (BotDifficulty::Easy, 0.1),
            (BotDifficulty::Normal, 0.3),
            (BotDifficulty::Hard, 0.6),
        ] {
            let rate = (strike_rate(difficulty, true) + strike_rate(difficulty, false)) / 2.0;
            assert!((rate - chance).abs() < 0.03, "{difficulty:?}: {rate}");
        }
        assert!(
            strike_rate(BotDifficulty::Hard, true) > strike_rate(BotDifficulty::Hard, false) + 0.2
        );
        let normal = (
            strike_rate(BotDifficulty::Normal, true),
            strike_rate(BotDifficulty::Normal, false),
        );
        assert!((normal.0 - normal.1).abs() < 0.04, "{normal:?}");
    }

    /// Issue #196 (« les bots Difficile utilisent la patte aux endroits où elle rapporte »): a
    /// Hard bot strikes 90 % of the karts its push would take off the road (the grass, the void,
    /// a wall), against 60 % on average elsewhere; the Easy and Normal bots make no difference.
    #[test]
    fn hard_bots_strike_where_the_push_takes_a_kart_off_the_road() {
        let hard = strike_rate_where(BotDifficulty::Hard, true, true);
        assert!((hard - 0.9).abs() < 0.03, "{hard}");
        assert!(hard > strike_rate(BotDifficulty::Hard, true) + 0.05);
        for difficulty in [BotDifficulty::Easy, BotDifficulty::Normal] {
            for toward_edge in [false, true] {
                assert_eq!(
                    strike_rate_where(difficulty, toward_edge, true),
                    strike_rate(difficulty, toward_edge),
                    "{difficulty:?}"
                );
            }
        }
    }

    /// A strike is a press of one tick (released at once, the strike goes on the release); a
    /// kart staying in reach is no new occasion; the same kart is not struck again within 5 s;
    /// never in its own drift; never while the paw rests.
    #[test]
    fn a_bot_taps_once_per_occasion_and_lets_a_kart_be_for_five_seconds() {
        let tuning = BotTuning::default();
        let mut skill = *tuning.skill(BotDifficulty::Hard);
        skill.strike_chance = 1.0;
        skill.strike_edge_bias = 0.0;
        let kart = beside(7);
        let mut brain = BotPawBrain::new(1);
        assert!(paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart])).interact);
        assert!(!paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart])).interact);
        // Still in reach: no occasion.
        for _ in 0..60 {
            assert!(!paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart])).interact);
        }
        // Out of reach, then back within 5 s: not again.
        paw_buttons(&mut brain, &skill, &tuning, &senses(&[]));
        assert!(!paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart])).interact);
        // Another kart: yes.
        assert!(paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart, beside(8)])).interact);
        // After 5 s: the first one again.
        let mut brain = BotPawBrain::new(2);
        paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart]));
        for _ in 0..(5.1 / DT) as usize {
            paw_buttons(&mut brain, &skill, &tuning, &senses(&[]));
        }
        assert!(paw_buttons(&mut brain, &skill, &tuning, &senses(&[kart])).interact);
        // In its own drift, or with the paw resting: no.
        let only = [kart];
        let mut brain = BotPawBrain::new(3);
        let drifting = PawSenses {
            drifting: true,
            ..senses(&only)
        };
        assert!(!paw_buttons(&mut brain, &skill, &tuning, &drifting).interact);
        let mut brain = BotPawBrain::new(4);
        let resting = PawSenses {
            ready: false,
            ..senses(&only)
        };
        assert!(!paw_buttons(&mut brain, &skill, &tuning, &resting).interact);
        // Not racing: nothing.
        let mut brain = BotPawBrain::new(5);
        let out = PawSenses {
            can_act: false,
            ..senses(&only)
        };
        assert_eq!(
            paw_buttons(&mut brain, &skill, &tuning, &out),
            PawButtons::default()
        );
    }

    /// GD § 15: a faster kart passing (Normal sometimes, Hard often, Easy never) is grabbed: the
    /// button held through the tether, released after `grab_hold`; a Hard bot steers to throw it
    /// away from itself.
    #[test]
    fn bots_grab_a_faster_kart_and_hard_ones_throw_it() {
        let tuning = BotTuning::default();
        let faster = ReachKart {
            faster_by: 6.0,
            side: -1.0,
            ..beside(9)
        };
        assert_eq!(tuning.skill(BotDifficulty::Easy).grab_chance, 0.0);
        let mut hard = *tuning.skill(BotDifficulty::Hard);
        hard.grab_chance = 1.0;
        let passing = [faster];
        let mut brain = BotPawBrain::new(1);
        assert!(paw_buttons(&mut brain, &hard, &tuning, &senses(&[faster])).interact);
        // Pressing until the paw reaches out (0.2 s), reaching...
        for _ in 0..12 {
            assert!(paw_buttons(&mut brain, &hard, &tuning, &senses(&[faster])).interact);
        }
        let reaching = PawSenses {
            reaching: true,
            ..senses(&passing)
        };
        assert!(paw_buttons(&mut brain, &hard, &tuning, &reaching).interact);
        // ...holding until `grab_hold`.
        let holding = |held: f32| PawSenses {
            holding: Some((held, -1.0)),
            ..senses(&passing)
        };
        assert!(paw_buttons(&mut brain, &hard, &tuning, &holding(0.5)).interact);
        let out = paw_buttons(&mut brain, &hard, &tuning, &holding(hard.grab_hold));
        assert!(!out.interact);
        assert_eq!(
            out.steer,
            Some(-0.8),
            "thrown to its side, away from the bot"
        );
        // A Normal bot lets go without a throw.
        let mut normal = *tuning.skill(BotDifficulty::Normal);
        normal.grab_chance = 1.0;
        let mut brain = BotPawBrain::new(1);
        paw_buttons(&mut brain, &normal, &tuning, &senses(&[faster]));
        let out = paw_buttons(&mut brain, &normal, &tuning, &holding(normal.grab_hold));
        assert_eq!(out, PawButtons::default());
        // Nobody caught: it lets go after the press.
        let mut brain = BotPawBrain::new(1);
        paw_buttons(&mut brain, &hard, &tuning, &senses(&[faster]));
        for _ in 0..20 {
            paw_buttons(&mut brain, &hard, &tuning, &senses(&[]));
        }
        assert!(!paw_buttons(&mut brain, &hard, &tuning, &senses(&[])).interact);
        // Not faster: a strike occasion, not a grab.
        let mut brain = BotPawBrain::new(1);
        let same_speed = ReachKart {
            faster_by: 0.5,
            ..faster
        };
        let mut never_strikes = hard;
        never_strikes.strike_chance = 0.0;
        assert!(!paw_buttons(&mut brain, &never_strikes, &tuning, &senses(&[same_speed])).interact);
    }

    /// GD § 15: held, it jumps after 1 / 0.5 / 0.25 s, once.
    #[test]
    fn a_held_bot_jumps_free_after_a_delay_with_its_level() {
        let tuning = BotTuning::default();
        for (difficulty, delay) in [
            (BotDifficulty::Easy, 1.0),
            (BotDifficulty::Normal, 0.5),
            (BotDifficulty::Hard, 0.25),
        ] {
            let skill = *tuning.skill(difficulty);
            assert_eq!(skill.escape_delay, delay);
            let mut brain = BotPawBrain::new(1);
            let mut jumps = Vec::new();
            for tick in 0..120 {
                let held = PawSenses {
                    held_for: Some(tick as f32 * DT),
                    ready: false,
                    ..senses(&[])
                };
                if paw_buttons(&mut brain, &skill, &tuning, &held).jump {
                    jumps.push(tick);
                }
            }
            assert_eq!(jumps, [(delay / DT).round() as usize], "{difficulty:?}");
        }
    }
}
