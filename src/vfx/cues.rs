//! Issue #105: what a kart's tick looks like (pure, unit-tested), the same reading as the sounds'
//! (`audio::cues`, #103): every tick, the particles take a [`VfxSnapshot`] of each kart and
//! compare it with the one of the tick before ([`kart_bursts`]): a new drift tier is its flash, a
//! boost that starts is the look of its source, a hit is its stars... The rules write nothing for
//! the particles: the bot races' world stays the one they were measured with.
//!
//! What a kart throws all along (the sparks of its drift, the embers of its flames, its dust)
//! comes from its state of the frame: [`spark_style`], [`ember_paint`].

use bevy::prelude::*;

use crate::items::ItemKind;
use crate::kart::{HitReaction, KartState, MiniTurbo};

use super::pool::Paint;
use super::tuning::{DriftVfx, VfxTuning};

/// What the particles see of a kart on one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VfxSnapshot {
    pub state: KartState,
    /// The tier its drift reached (none out of a drift).
    pub tier: Option<MiniTurbo>,
    /// Its boost: seconds left.
    pub boost: f32,
    pub stalled: bool,
    /// Its slipstream gauge (s) and whether it charges.
    pub slip_charge: f32,
    pub slip_charging: bool,
    /// Ticks without a wheel on the ground.
    pub airborne_ticks: u32,
    pub gliding: bool,
    pub anti_gravity: bool,
    /// The item ready in its hand, the piment doré's window open, the piñata's items left (a
    /// piment's boost).
    pub hand: Option<ItemKind>,
    pub golden: bool,
    pub pinata: u8,
    /// Cream on it (a chantilly).
    pub creamed: bool,
    /// The speed (m/s) the physics step took from it across its up: a shock; and toward where
    /// (unit: what it hit is that way).
    pub impact: f32,
    pub impact_dir: Vec3,
    /// Paw strikes received so far.
    pub struck: u32,
    /// Issue #130: under the water of a lake.
    pub underwater: bool,
}

/// What else happened to the kart on this tick (messages of the rules).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TickEvents {
    /// The race's GO (the rocket starts are judged).
    pub go: bool,
    /// It picked up a croquette (its mini-boost).
    pub croquette: bool,
    /// It bumped into another kart, this hard (m/s of closing speed; 0: no bump).
    pub bump: f32,
    /// Issue #171: it was already touching another kart (a bump less than
    /// `ImpactVfx::dizzy_contact_gap` ago): the same contact, not a new shock.
    pub contact: bool,
}

/// Where a boost comes from (GD § 2: one system for all; each its look).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoostSource {
    MiniTurbo(MiniTurbo),
    RocketStart,
    Trick,
    Slipstream,
    Piment,
    /// The anti-gravity's spin boost (GD § 10).
    SpinBoost,
    BoostPad,
    /// A croquette's mini-boost.
    Croquette,
}

impl BoostSource {
    /// The color of its flames and embers.
    pub fn paint(self) -> Paint {
        match self {
            BoostSource::MiniTurbo(tier) => Paint::tier(tier.index()),
            BoostSource::RocketStart | BoostSource::BoostPad => Paint::Flame,
            BoostSource::Trick | BoostSource::Croquette => Paint::Gold,
            BoostSource::Slipstream => Paint::Wind,
            BoostSource::Piment => Paint::Piment,
            BoostSource::SpinBoost => Paint::Cyan,
        }
    }
}

/// The one-off effects of a kart's tick (several ticks in a frame: merged).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bursts {
    /// The drift reached this tier: its flash.
    pub tier: Option<MiniTurbo>,
    /// A boost started, from this source.
    pub boost: Option<BoostSource>,
    /// Hit by an item: a spin out or a tumble starts.
    pub hit: Option<HitReaction>,
    /// A trick starts (GD § 5).
    pub trick: bool,
    /// It lands after a while in the air.
    pub landing: bool,
    /// It hit a wall, this hard (m/s; 0: no wall), that way (unit).
    pub wall: f32,
    pub wall_dir: Vec3,
    /// It bumped into another kart, this hard (m/s of closing speed; 0: no bump). Its stars are
    /// the world's, where the karts touched (`systems::WorldBurst::Bump`).
    pub bump: f32,
    /// A chantilly lands on it.
    pub creamed: bool,
    /// Its engine stalls (GD § 4).
    pub stall: bool,
    /// A paw strike landed on it.
    pub struck: bool,
    /// It sees stars over its head for this long from now (s; 0: none).
    pub dizzy: f32,
    /// Issue #130: it went into the water (true) or came out of it (false): a splash.
    pub splash: Option<bool>,
}

impl Bursts {
    /// What two ticks in a row give (the later boost's source wins).
    pub fn merge(&mut self, other: Bursts) {
        self.tier = other.tier.max(self.tier);
        self.boost = other.boost.or(self.boost);
        self.hit = other.hit.or(self.hit);
        self.trick |= other.trick;
        self.landing |= other.landing;
        if other.wall > self.wall {
            self.wall = other.wall;
            self.wall_dir = other.wall_dir;
        }
        self.bump = self.bump.max(other.bump);
        self.creamed |= other.creamed;
        self.stall |= other.stall;
        self.struck |= other.struck;
        self.dizzy = self.dizzy.max(other.dizzy);
        self.splash = other.splash.or(self.splash);
    }

    pub fn is_empty(&self) -> bool {
        *self == Bursts::default()
    }
}

/// The boost started (or grew) on this tick: more seconds left than the last tick's minus this
/// tick (a little tolerance for the rounding).
fn boost_started(prev: &VfxSnapshot, now: &VfxSnapshot, dt: f32) -> bool {
    now.boost > (prev.boost - dt).max(0.0) + 0.02
}

/// Where a boost that starts comes from, from what changed on the tick (as the sounds read it).
pub fn boost_source(prev: &VfxSnapshot, now: &VfxSnapshot, events: TickEvents) -> BoostSource {
    let drifted = matches!(prev.state, KartState::Drifting { .. })
        && !matches!(now.state, KartState::Drifting { .. });
    if drifted && let Some(tier) = prev.tier {
        return BoostSource::MiniTurbo(tier);
    }
    if events.go {
        return BoostSource::RocketStart;
    }
    if prev.state == KartState::Trick && now.state != KartState::Trick {
        return BoostSource::Trick;
    }
    if prev.slip_charging && prev.slip_charge > 0.3 && now.slip_charge == 0.0 {
        return BoostSource::Slipstream;
    }
    if events.croquette {
        return BoostSource::Croquette;
    }
    if matches!(prev.hand, Some(ItemKind::Piment | ItemKind::PimentDore))
        || prev.golden
        || prev.pinata != now.pinata
    {
        return BoostSource::Piment;
    }
    if events.bump > 0.0 && now.anti_gravity {
        return BoostSource::SpinBoost;
    }
    BoostSource::BoostPad
}

/// The one-off effects of a kart's tick: what changed from `prev` to `now` (`dt`: the tick, s).
pub fn kart_bursts(
    prev: &VfxSnapshot,
    now: &VfxSnapshot,
    events: TickEvents,
    tuning: &VfxTuning,
    dt: f32,
) -> Bursts {
    let mut bursts = Bursts::default();
    let impact = &tuning.impact;
    if let KartState::Hit {
        reaction, duration, ..
    } = now.state
        && !prev.state.is_hit()
    {
        bursts.hit = Some(reaction);
        bursts.dizzy = duration as f32 * dt + impact.dizzy_after_hit;
    }
    if now.state == KartState::Trick && prev.state != KartState::Trick {
        bursts.trick = true;
    }
    if let Some(tier) = now.tier
        && prev.tier.is_none_or(|before| tier > before)
    {
        bursts.tier = Some(tier);
    }
    if boost_started(prev, now, dt) {
        bursts.boost = Some(boost_source(prev, now, events));
    }
    if now.stalled && !prev.stalled {
        bursts.stall = true;
    }
    let air_ticks = (tuning.world.landing_air_time / dt.max(1e-4)).round() as u32;
    if prev.airborne_ticks >= air_ticks.max(1) && now.airborne_ticks == 0 && !now.gliding {
        bursts.landing = true;
    }
    if now.creamed && !prev.creamed {
        bursts.creamed = true;
    }
    // A wall (a bump between karts is its own burst, where they touched).
    if now.impact >= impact.wall_speed && events.bump == 0.0 {
        bursts.wall = now.impact;
        bursts.wall_dir = now.impact_dir;
    }
    bursts.bump = events.bump;
    if now.struck > prev.struck {
        bursts.struck = true;
        bursts.dizzy = bursts.dizzy.max(impact.dizzy_after_shock);
    }
    // Issue #171: the moment of the shock, not the contact that lasts.
    if events.bump >= impact.dizzy_bump_speed && !events.contact {
        bursts.dizzy = bursts.dizzy.max(impact.dizzy_after_shock);
    }
    if now.underwater != prev.underwater {
        bursts.splash = Some(now.underwater);
    }
    bursts
}

/// How a drift's sparks look at a tier: how many a wheel throws per second, their width and
/// streak's length (m), their speed (m/s), their life (s) and color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SparkStyle {
    pub rate: f32,
    pub size: f32,
    pub length: f32,
    pub speed: f32,
    pub life: f32,
    pub paint: Paint,
    /// Issue #174: how high they are thrown (against 1 backward), how bright they glow.
    pub rise: f32,
    pub glow: f32,
    /// Issue #174: the flames licking the wheel (tongues per second, their height, how bright),
    /// and the embers floating up (per second).
    pub flame_rate: f32,
    pub flame_size: f32,
    pub flame_glow: f32,
    pub ember_rate: f32,
}

/// The sparks of a drift at `tier`, `progress` of the way (0 to 1) from its threshold to the next
/// tier's: more of them as the charge grows, and more, bigger, longer, faster and longer-lived
/// at each tier, in its color. Issue #174: thrown higher and brighter at each tier; its flames
/// (none in blue) more and taller as the charge grows; its embers (purple).
pub fn spark_style(drift: &DriftVfx, tier: MiniTurbo, progress: f32) -> SparkStyle {
    let i = tier.index();
    let ramp = drift.spark_ramp.clamp(0.0, 1.0);
    let progress = progress.clamp(0.0, 1.0);
    let growing = 1.0 - ramp + ramp * progress;
    SparkStyle {
        rate: drift.spark_rate[i].max(0.0) * growing,
        size: drift.spark_size[i],
        length: drift.spark_length[i],
        speed: drift.spark_speed[i],
        life: drift.spark_life[i],
        paint: Paint::tier(i),
        rise: drift.spark_rise[i],
        glow: drift.spark_glow[i],
        flame_rate: drift.flame_rate[i].max(0.0) * growing,
        flame_size: drift.flame_size[i].max(0.0)
            * (1.0 + (drift.flame_grow.max(0.0) - 1.0) * progress),
        flame_glow: drift.flame_glow[i],
        ember_rate: drift.ember_rate[i].max(0.0),
    }
}

/// How far a drift's `charge` is from its tier's threshold to the next one's (0 to 1; 1 at the
/// highest tier reached, and 0 before the first).
pub fn tier_progress(thresholds: [f32; 3], charge: f32, tier: Option<MiniTurbo>) -> f32 {
    let Some(tier) = tier else {
        return 0.0;
    };
    let i = tier.index();
    let Some(next) = thresholds.get(i + 1) else {
        return 1.0;
    };
    let from = thresholds[i];
    if *next <= from {
        return 1.0;
    }
    ((charge - from) / (next - from)).clamp(0.0, 1.0)
}

/// The color of a kart's flames and embers: its boost's source (a fusée or a pad's orange by
/// default, when its source was not seen).
pub fn ember_paint(source: Option<BoostSource>) -> Paint {
    source.map_or(Paint::Flame, BoostSource::paint)
}

/// The punch of the field of view when the camera's kart starts a boost from `source` (times
/// `BoostVfx::fov_punch`): a mini-turbo's by tier, a croquette's none.
pub fn fov_punch(tuning: &VfxTuning, source: BoostSource) -> f32 {
    let boost = &tuning.boost;
    boost.fov_punch
        * match source {
            BoostSource::MiniTurbo(tier) => boost.fov_tier[tier.index()],
            BoostSource::Croquette => 0.0,
            BoostSource::Slipstream | BoostSource::SpinBoost => 0.7,
            _ => 1.0,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::DriftSide;

    const DT: f32 = 1.0 / 60.0;

    fn drifting(charge: f32, tier: Option<MiniTurbo>) -> VfxSnapshot {
        VfxSnapshot {
            state: KartState::Drifting {
                side: DriftSide::Left,
                charge,
            },
            tier,
            ..Default::default()
        }
    }

    fn bursts(prev: VfxSnapshot, now: VfxSnapshot) -> Bursts {
        kart_bursts(
            &prev,
            &now,
            TickEvents::default(),
            &VfxTuning::default(),
            DT,
        )
    }

    /// Issue #105 acceptance: the sparks' density follows the tier (more at each tier, and more
    /// as the charge nears the next), and so do their size, length, speed, life; each tier its
    /// own color.
    #[test]
    fn the_sparks_grow_with_the_tier_and_take_its_color() {
        let drift = DriftVfx::default();
        let mut last: Option<SparkStyle> = None;
        let mut paints = Vec::new();
        for tier in MiniTurbo::ALL {
            let start = spark_style(&drift, tier, 0.0);
            let end = spark_style(&drift, tier, 1.0);
            assert!(end.rate > start.rate, "{tier:?} ramps up");
            assert_eq!((start.size, start.paint), (end.size, end.paint));
            if let Some(last) = last {
                assert!(
                    start.rate > last.rate,
                    "{tier:?}: denser than the tier before"
                );
                assert!(start.size > last.size && start.length > last.length);
                assert!(start.speed > last.speed && start.life > last.life);
            }
            paints.push(start.paint);
            last = Some(end);
        }
        assert_eq!(paints, [Paint::Blue, Paint::Orange, Paint::Purple]);
        // Issue #174: three tiers really apart, not only more of the same: blue only sparks,
        // orange its flames, purple bigger flames, its embers, its sparks thrown higher; within a
        // tier, the flames grow with the charge.
        let [blue, orange, purple] = MiniTurbo::ALL.map(|tier| spark_style(&drift, tier, 0.5));
        assert_eq!((blue.flame_rate, blue.ember_rate), (0.0, 0.0));
        assert!(orange.flame_rate > 0.0 && orange.ember_rate == 0.0);
        assert!(purple.flame_rate > orange.flame_rate && purple.flame_size > orange.flame_size);
        assert!(purple.ember_rate > 0.0);
        assert!(blue.rise < orange.rise && orange.rise < purple.rise);
        assert!(blue.glow < orange.glow && orange.glow < purple.glow);
        let (start, end) = (
            spark_style(&drift, MiniTurbo::Orange, 0.0),
            spark_style(&drift, MiniTurbo::Orange, 1.0),
        );
        assert!(end.flame_rate > start.flame_rate && end.flame_size > start.flame_size);
        // The progress toward the next tier (GD § 3.4: 270, 570, 870).
        let thresholds = [270.0, 570.0, 870.0];
        assert_eq!(tier_progress(thresholds, 100.0, None), 0.0);
        assert_eq!(tier_progress(thresholds, 270.0, Some(MiniTurbo::Blue)), 0.0);
        assert!((tier_progress(thresholds, 420.0, Some(MiniTurbo::Blue)) - 0.5).abs() < 1e-6);
        assert!((tier_progress(thresholds, 720.0, Some(MiniTurbo::Orange)) - 0.5).abs() < 1e-6);
        assert_eq!(
            tier_progress(thresholds, 870.0, Some(MiniTurbo::Purple)),
            1.0
        );
    }

    /// A flash at each tier reached (once), then the « fwoosh » of the tier at the release, in
    /// its color.
    #[test]
    fn each_tier_flashes_and_the_release_fires_its_mini_turbo() {
        let mut last = drifting(200.0, None);
        assert!(bursts(last, last).is_empty());
        for (tier, charge) in MiniTurbo::ALL.into_iter().zip([270.0, 570.0, 870.0]) {
            let next = drifting(charge, Some(tier));
            assert_eq!(bursts(last, next).tier, Some(tier));
            assert!(bursts(next, next).is_empty(), "once");
            last = next;
        }
        let released = VfxSnapshot {
            boost: 2.63,
            ..Default::default()
        };
        let fired = bursts(last, released);
        assert_eq!(fired.boost, Some(BoostSource::MiniTurbo(MiniTurbo::Purple)));
        assert_eq!(fired.boost.unwrap().paint(), Paint::Purple);
        // The boost counting down: nothing more.
        let later = VfxSnapshot {
            boost: 2.63 - DT,
            ..Default::default()
        };
        assert!(bursts(released, later).is_empty());
        // Released before blue: no boost, nothing.
        assert!(bursts(drifting(100.0, None), VfxSnapshot::default()).is_empty());
    }

    /// Each boost its source, from what changed on the tick (GD § 2), and its color.
    #[test]
    fn each_boost_has_the_look_of_its_source() {
        let rest = VfxSnapshot::default();
        let boosted = VfxSnapshot {
            boost: 1.0,
            ..Default::default()
        };
        let none = TickEvents::default();
        let tuning = VfxTuning::default();
        let source = |prev: VfxSnapshot, now: VfxSnapshot, events: TickEvents| {
            kart_bursts(&prev, &now, events, &tuning, DT).boost
        };
        let go = TickEvents { go: true, ..none };
        assert_eq!(source(rest, boosted, go), Some(BoostSource::RocketStart));
        let trick = VfxSnapshot {
            state: KartState::Trick,
            ..rest
        };
        assert_eq!(source(trick, boosted, none), Some(BoostSource::Trick));
        let charging = VfxSnapshot {
            slip_charging: true,
            slip_charge: 1.1,
            ..rest
        };
        assert_eq!(
            source(charging, boosted, none),
            Some(BoostSource::Slipstream)
        );
        let piment = VfxSnapshot {
            hand: Some(ItemKind::Piment),
            ..rest
        };
        assert_eq!(source(piment, boosted, none), Some(BoostSource::Piment));
        let croquette = TickEvents {
            croquette: true,
            ..none
        };
        assert_eq!(
            source(rest, boosted, croquette),
            Some(BoostSource::Croquette)
        );
        let spin = VfxSnapshot {
            anti_gravity: true,
            ..boosted
        };
        let bump = TickEvents { bump: 4.0, ..none };
        assert_eq!(source(rest, spin, bump), Some(BoostSource::SpinBoost));
        assert_eq!(source(rest, boosted, none), Some(BoostSource::BoostPad));
        assert_eq!(BoostSource::Piment.paint(), Paint::Piment);
        assert_eq!(BoostSource::SpinBoost.paint(), Paint::Cyan);
        assert_eq!(ember_paint(None), Paint::Flame);
        // The field of view's punch: bigger for a higher tier, none for a croquette.
        let punch = |s| fov_punch(&tuning, s);
        assert!(
            punch(BoostSource::MiniTurbo(MiniTurbo::Blue))
                < punch(BoostSource::MiniTurbo(MiniTurbo::Purple))
        );
        assert_eq!(punch(BoostSource::Croquette), 0.0);
        assert_eq!(punch(BoostSource::RocketStart), tuning.boost.fov_punch);
    }

    /// The shocks: a hit (and its stars for as long as it lasts, and a while after), a wall
    /// (not a bump: that one is where the karts touched), a paw strike, a hard bump; a landing,
    /// a chantilly, a stalled engine, a trick.
    #[test]
    fn the_shocks_and_the_moments_have_their_bursts() {
        let tuning = VfxTuning::default();
        let rest = VfxSnapshot::default();
        let hit = VfxSnapshot {
            state: KartState::Hit {
                reaction: HitReaction::Tumble,
                elapsed: 0,
                duration: 60,
            },
            ..rest
        };
        let b = bursts(rest, hit);
        assert_eq!(b.hit, Some(HitReaction::Tumble));
        assert!((b.dizzy - (1.0 + tuning.impact.dizzy_after_hit)).abs() < 1e-4);
        assert!(bursts(hit, hit).is_empty(), "once");
        let wall = VfxSnapshot {
            impact: 9.0,
            ..rest
        };
        assert_eq!(bursts(rest, wall).wall, 9.0);
        let weak = VfxSnapshot {
            impact: 3.0,
            ..rest
        };
        assert_eq!(bursts(rest, weak).wall, 0.0);
        let events = |bump: f32| TickEvents {
            bump,
            ..Default::default()
        };
        let bumped = kart_bursts(&rest, &wall, events(6.0), &tuning, DT);
        assert_eq!(bumped.wall, 0.0, "a bump, not a wall");
        assert_eq!(bumped.bump, 6.0);
        assert_eq!(bumped.dizzy, 0.0);
        let hard = kart_bursts(&rest, &rest, events(14.0), &tuning, DT);
        assert_eq!(hard.dizzy, tuning.impact.dizzy_after_shock);
        // Issue #171: the same hard contact that goes on is no new shock.
        let rubbing = TickEvents {
            contact: true,
            ..events(14.0)
        };
        let still = kart_bursts(&rest, &rest, rubbing, &tuning, DT);
        assert_eq!(still.dizzy, 0.0);
        assert_eq!(
            still.bump, 14.0,
            "its stars and « pof » between the karts stay"
        );
        let struck = VfxSnapshot { struck: 1, ..rest };
        let b = bursts(rest, struck);
        assert!(b.struck && b.dizzy == tuning.impact.dizzy_after_shock);
        let flying = VfxSnapshot {
            airborne_ticks: 30,
            ..rest
        };
        assert!(bursts(flying, rest).landing);
        let hop = VfxSnapshot {
            airborne_ticks: 10,
            ..rest
        };
        assert!(!bursts(hop, rest).landing, "a hop is not a landing");
        assert!(
            bursts(
                rest,
                VfxSnapshot {
                    creamed: true,
                    ..rest
                }
            )
            .creamed
        );
        assert!(
            bursts(
                rest,
                VfxSnapshot {
                    stalled: true,
                    ..rest
                }
            )
            .stall
        );
        let trick = VfxSnapshot {
            state: KartState::Trick,
            ..rest
        };
        assert!(bursts(rest, trick).trick);
        // Two ticks in one frame: both kept.
        let mut merged = bursts(rest, trick);
        merged.merge(bursts(rest, wall));
        assert!(merged.trick && merged.wall == 9.0);
    }

    /// Issue #130: going into the water splashes, coming out of it too (each once); a tick
    /// without the splash keeps the one of the tick before in the frame.
    #[test]
    fn going_in_and_out_of_the_water_splashes() {
        let dry = VfxSnapshot::default();
        let wet = VfxSnapshot {
            underwater: true,
            ..dry
        };
        assert_eq!(bursts(dry, wet).splash, Some(true));
        assert_eq!(bursts(wet, dry).splash, Some(false));
        assert!(bursts(wet, wet).is_empty());
        assert!(bursts(dry, dry).is_empty());
        let mut frame = bursts(dry, wet);
        frame.merge(bursts(wet, wet));
        assert_eq!(frame.splash, Some(true));
    }
}
