//! Issue #103: what a kart's tick sounds like (pure, unit-tested). Every tick, the sound design
//! takes a [`KartSnapshot`] of each kart (its state, its boost, its environment...) and compares
//! it with the one of the previous tick ([`kart_cues`]): a new drift tier is a chime, a boost
//! that starts is the « fwoosh » of its source, a hit a spin out or a tumble and a cry... The
//! game's rules write nothing for the sounds: the bot races' world stays the one they were
//! measured with.

use crate::interactions::SwingKind;
use crate::items::ItemKind;
use crate::kart::{HitReaction, KartState, MiniTurbo};
use crate::race::TowReason;

use super::catalog::{Mood, SoundEffect};
use super::tuning::SoundTuning;

/// What the sound design sees of a kart on one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KartSnapshot {
    pub state: KartState,
    /// The tier its drift reached (none out of a drift).
    pub tier: Option<MiniTurbo>,
    /// Its boost's seconds left.
    pub boost: f32,
    pub stalled: bool,
    /// Its slipstream gauge (s) and whether it charges.
    pub slip_charge: f32,
    pub slip_charging: bool,
    /// Ticks without a wheel on the ground.
    pub airborne_ticks: u32,
    pub gliding: bool,
    pub underwater: bool,
    pub anti_gravity: bool,
    /// The pelican carries it (and why it came).
    pub carried: Option<TowReason>,
    /// The item ready in its hand, the piment doré's window open, the piñata's items left.
    pub hand: Option<ItemKind>,
    pub hand_count: u8,
    pub golden: bool,
    pub pinata: u8,
    /// Its item effects (GD § 8): super câlin, fusée, dégonflage, chantilly, pie voleuse.
    pub calin: bool,
    pub rocket: bool,
    pub deflated: bool,
    pub creamed: bool,
    pub ghost: bool,
    /// The speed (m/s) the physics step took from it across its up: a shock.
    pub impact: f32,
    /// Its paw's last swing (its kind, ticks since).
    pub swing: Option<(SwingKind, u32)>,
}

/// What else happened to the kart on this tick (messages of the rules).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickEvents {
    /// The race's GO (the rocket starts are judged).
    pub go: bool,
    /// It picked up a croquette (its mini-boost is the croquette's pop).
    pub croquette: bool,
    /// It bumped into another kart (a shock, and in anti-gravity the spin boost).
    pub bumped: bool,
}

/// The sounds of one kart's tick: at most [`Cues::MAX`] sounds, and maybe a cry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cues {
    sounds: [SoundEffect; Cues::MAX],
    len: usize,
    pub cry: Option<Mood>,
}

impl Default for Cues {
    fn default() -> Self {
        Self {
            sounds: [SoundEffect::UiHover; Cues::MAX],
            len: 0,
            cry: None,
        }
    }
}

impl Cues {
    pub const MAX: usize = 8;

    fn push(&mut self, sound: SoundEffect) {
        if self.len < Self::MAX {
            self.sounds[self.len] = sound;
            self.len += 1;
        }
    }

    fn cry(&mut self, mood: Mood) {
        // A hurt cry is never covered by a happier one on the same tick.
        if self.cry != Some(Mood::Hurt) {
            self.cry = Some(mood);
        }
    }

    pub fn sounds(&self) -> &[SoundEffect] {
        &self.sounds[..self.len]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.cry.is_none()
    }
}

/// The boost started (or grew) on this tick: more seconds left than the last tick's minus this
/// tick (a little tolerance for the rounding).
fn boost_started(prev: &KartSnapshot, now: &KartSnapshot, dt: f32) -> bool {
    now.boost > (prev.boost - dt).max(0.0) + 0.02
}

/// Where a boost that starts comes from (GD § 2: one system for all), from what changed on the
/// tick; `None`: a croquette's mini-boost (its own pop).
fn boost_source(
    prev: &KartSnapshot,
    now: &KartSnapshot,
    events: TickEvents,
) -> Option<SoundEffect> {
    let drifted = matches!(prev.state, KartState::Drifting { .. })
        && !matches!(now.state, KartState::Drifting { .. });
    if drifted && let Some(tier) = prev.tier {
        return Some(SoundEffect::MiniTurbo(tier));
    }
    if events.go {
        return Some(SoundEffect::RocketStart);
    }
    if prev.state == KartState::Trick && now.state != KartState::Trick {
        return Some(SoundEffect::TrickBoost);
    }
    if prev.slip_charging && prev.slip_charge > 0.3 && now.slip_charge == 0.0 {
        return Some(SoundEffect::Slipstream);
    }
    if events.croquette {
        return None;
    }
    if matches!(prev.hand, Some(ItemKind::Piment | ItemKind::PimentDore))
        || prev.golden
        || prev.pinata != now.pinata
    {
        return Some(SoundEffect::Piment);
    }
    if events.bumped && now.anti_gravity {
        return Some(SoundEffect::SpinBoost);
    }
    Some(SoundEffect::BoostPad)
}

/// The sounds of a kart's tick: what changed from `prev` to `now` (`dt`: the tick, s).
pub fn kart_cues(
    prev: &KartSnapshot,
    now: &KartSnapshot,
    events: TickEvents,
    tuning: &SoundTuning,
    dt: f32,
) -> Cues {
    let mut cues = Cues::default();
    // Hit by an item (GD § 8): a spin out or a tumble, and a cry.
    if let KartState::Hit { reaction, .. } = now.state
        && !prev.state.is_hit()
    {
        cues.push(match reaction {
            HitReaction::Spin => SoundEffect::SpinOut,
            HitReaction::Tumble => SoundEffect::Tumble,
        });
        cues.cry(Mood::Hurt);
    }
    // A trick off a ramp (GD § 5): the flip, and a happy cry.
    if now.state == KartState::Trick && prev.state != KartState::Trick {
        cues.push(SoundEffect::Trick);
        cues.cry(Mood::Happy);
    }
    // The drift reaches a tier (GD § 3): its chime.
    if let Some(tier) = now.tier
        && prev.tier.is_none_or(|before| tier > before)
    {
        cues.push(SoundEffect::DriftTier(tier));
    }
    if boost_started(prev, now, dt)
        && let Some(source) = boost_source(prev, now, events)
    {
        cues.push(source);
    }
    // The stalled start (GD § 4).
    if now.stalled && !prev.stalled {
        cues.push(SoundEffect::Stall);
    }
    // The pelican comes (GD § 2): after a fall, the slide whistle first.
    if let Some(reason) = now.carried
        && prev.carried.is_none()
    {
        if reason == TowReason::Fell {
            cues.push(SoundEffect::Fall);
            cues.cry(Mood::Hurt);
        }
        cues.push(SoundEffect::Pelican);
    }
    // The environments (GD § 10).
    let air_ticks = (tuning.landing_air_time / dt.max(1e-4)).round() as u32;
    if prev.airborne_ticks >= air_ticks.max(1) && now.airborne_ticks == 0 && !now.gliding {
        cues.push(SoundEffect::Landing);
    }
    if now.gliding && !prev.gliding {
        cues.push(SoundEffect::GliderOpen);
    }
    if now.underwater != prev.underwater {
        cues.push(SoundEffect::Splash);
    }
    // What its items do to it (GD § 8).
    if now.calin && !prev.calin {
        cues.push(SoundEffect::SuperCalin);
    }
    if now.rocket && !prev.rocket {
        cues.push(SoundEffect::Fusee);
    }
    if now.ghost && !prev.ghost {
        cues.push(SoundEffect::Ghost);
    }
    if now.deflated && !prev.deflated {
        cues.push(SoundEffect::Deflate);
    }
    if now.creamed && !prev.creamed {
        cues.push(SoundEffect::Creamed);
    }
    if now.pinata != 0 && prev.pinata == 0 {
        cues.push(SoundEffect::PinataBurst);
    }
    // A boomerang-os thrown (its count goes down; the last one leaves the hand): heard from the
    // throw, even if a wall sends it back into its thrower's hand on the same tick.
    if prev.hand == Some(ItemKind::Boomerang) {
        let thrown = match now.hand {
            Some(ItemKind::Boomerang) => now.hand_count < prev.hand_count,
            _ => prev.hand_count == 1,
        };
        if thrown {
            cues.push(SoundEffect::BoomerangThrow);
        }
    }
    // A wall (a bump between karts is its own sound).
    if now.impact >= tuning.wall_hit_speed && !events.bumped {
        cues.push(SoundEffect::WallHit);
    }
    // Its paw swings (GD § 15): into a kart or into thin air.
    if let Some((kind, 0)) = now.swing
        && matches!(
            kind,
            SwingKind::Strike | SwingKind::Whiff | SwingKind::GrabMiss | SwingKind::Throw
        )
        && prev.swing != now.swing
    {
        cues.push(SoundEffect::PawSwing);
    }
    cues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::DriftSide;

    const DT: f32 = 1.0 / 60.0;

    fn cues(prev: KartSnapshot, now: KartSnapshot) -> Vec<SoundEffect> {
        kart_cues(
            &prev,
            &now,
            TickEvents::default(),
            &SoundTuning::default(),
            DT,
        )
        .sounds()
        .to_vec()
    }

    fn drifting(tier: Option<MiniTurbo>) -> KartSnapshot {
        KartSnapshot {
            state: KartState::Drifting {
                side: DriftSide::Left,
                charge: 300.0,
            },
            tier,
            ..Default::default()
        }
    }

    /// Issue #103: a chime for each tier, then the « fwoosh » of the tier reached at the release.
    #[test]
    fn each_drift_tier_chimes_and_the_release_fires_its_fwoosh() {
        assert!(cues(drifting(None), drifting(None)).is_empty());
        let mut last = drifting(None);
        for tier in MiniTurbo::ALL {
            let next = drifting(Some(tier));
            assert_eq!(cues(last, next), [SoundEffect::DriftTier(tier)]);
            assert!(cues(next, next).is_empty(), "once");
            last = next;
        }
        let released = KartSnapshot {
            boost: 2.63,
            ..Default::default()
        };
        assert_eq!(
            cues(last, released),
            [SoundEffect::MiniTurbo(MiniTurbo::Purple)]
        );
        // The boost counting down is silent.
        let later = KartSnapshot {
            boost: 2.63 - DT,
            ..Default::default()
        };
        assert!(cues(released, later).is_empty());
        // Released before blue: nothing (no boost).
        assert!(cues(drifting(None), KartSnapshot::default()).is_empty());
    }

    /// Each boost source its sound (GD § 2), from what changed on the tick.
    #[test]
    fn each_boost_sounds_like_its_source() {
        let boosted = KartSnapshot {
            boost: 1.0,
            ..Default::default()
        };
        let tuning = SoundTuning::default();
        let with = |prev: KartSnapshot, now: KartSnapshot, events: TickEvents| {
            kart_cues(&prev, &now, events, &tuning, DT)
                .sounds()
                .to_vec()
        };
        let none = TickEvents::default();
        let rest = KartSnapshot::default();
        assert_eq!(
            with(rest, boosted, TickEvents { go: true, ..none }),
            [SoundEffect::RocketStart]
        );
        let trick = KartSnapshot {
            state: KartState::Trick,
            ..Default::default()
        };
        assert_eq!(with(trick, boosted, none), [SoundEffect::TrickBoost]);
        let charged = KartSnapshot {
            slip_charge: 1.2,
            slip_charging: true,
            ..Default::default()
        };
        let fired = KartSnapshot {
            slip_charging: true,
            ..boosted
        };
        assert_eq!(with(charged, fired, none), [SoundEffect::Slipstream]);
        let piment = KartSnapshot {
            hand: Some(ItemKind::Piment),
            ..Default::default()
        };
        assert_eq!(with(piment, boosted, none), [SoundEffect::Piment]);
        let golden = KartSnapshot {
            golden: true,
            hand: Some(ItemKind::PimentDore),
            ..Default::default()
        };
        assert_eq!(
            with(
                golden,
                KartSnapshot {
                    golden: true,
                    ..boosted
                },
                none
            ),
            [SoundEffect::Piment]
        );
        assert!(
            with(
                rest,
                boosted,
                TickEvents {
                    croquette: true,
                    ..none
                }
            )
            .is_empty(),
            "a croquette's mini-boost is its pop"
        );
        let anti_gravity = KartSnapshot {
            anti_gravity: true,
            ..boosted
        };
        assert_eq!(
            with(
                KartSnapshot {
                    anti_gravity: true,
                    ..rest
                },
                anti_gravity,
                TickEvents {
                    bumped: true,
                    ..none
                }
            ),
            [SoundEffect::SpinBoost]
        );
        assert_eq!(with(rest, boosted, none), [SoundEffect::BoostPad]);
    }

    /// Hits, tricks and falls cry; the stall, the pelican, the environments and the item effects
    /// have their sounds.
    #[test]
    fn hits_tricks_falls_and_the_environments_sound() {
        let tuning = SoundTuning::default();
        let rest = KartSnapshot::default();
        let spin = KartSnapshot {
            state: KartState::hit(HitReaction::Spin, 1.0, 60.0),
            ..Default::default()
        };
        let out = kart_cues(&rest, &spin, TickEvents::default(), &tuning, DT);
        assert_eq!(out.sounds(), [SoundEffect::SpinOut]);
        assert_eq!(out.cry, Some(Mood::Hurt));
        assert!(kart_cues(&spin, &spin, TickEvents::default(), &tuning, DT).is_empty());
        let tumble = KartSnapshot {
            state: KartState::hit(HitReaction::Tumble, 1.6, 60.0),
            ..Default::default()
        };
        assert_eq!(cues(rest, tumble), [SoundEffect::Tumble]);
        let trick = KartSnapshot {
            state: KartState::Trick,
            airborne_ticks: 5,
            ..Default::default()
        };
        let out = kart_cues(&rest, &trick, TickEvents::default(), &tuning, DT);
        assert_eq!(out.sounds(), [SoundEffect::Trick]);
        assert_eq!(out.cry, Some(Mood::Happy));
        assert_eq!(
            cues(
                rest,
                KartSnapshot {
                    stalled: true,
                    ..rest
                }
            ),
            [SoundEffect::Stall]
        );
        let fell = KartSnapshot {
            carried: Some(TowReason::Fell),
            ..rest
        };
        let out = kart_cues(&rest, &fell, TickEvents::default(), &tuning, DT);
        assert_eq!(out.sounds(), [SoundEffect::Fall, SoundEffect::Pelican]);
        assert_eq!(out.cry, Some(Mood::Hurt));
        let stuck = KartSnapshot {
            carried: Some(TowReason::Stuck),
            ..rest
        };
        assert_eq!(cues(rest, stuck), [SoundEffect::Pelican]);
        // Landing after a while in the air; not after a bump of the road.
        let flying = KartSnapshot {
            airborne_ticks: 30,
            ..rest
        };
        assert_eq!(cues(flying, rest), [SoundEffect::Landing]);
        let hop = KartSnapshot {
            airborne_ticks: 6,
            ..rest
        };
        assert!(cues(hop, rest).is_empty());
        assert_eq!(
            cues(
                rest,
                KartSnapshot {
                    gliding: true,
                    ..rest
                }
            ),
            [SoundEffect::GliderOpen]
        );
        assert_eq!(
            cues(
                rest,
                KartSnapshot {
                    underwater: true,
                    ..rest
                }
            ),
            [SoundEffect::Splash]
        );
        assert_eq!(
            cues(
                KartSnapshot {
                    underwater: true,
                    ..rest
                },
                rest
            ),
            [SoundEffect::Splash]
        );
        for (now, sound) in [
            (
                KartSnapshot {
                    calin: true,
                    ..rest
                },
                SoundEffect::SuperCalin,
            ),
            (
                KartSnapshot {
                    rocket: true,
                    ..rest
                },
                SoundEffect::Fusee,
            ),
            (
                KartSnapshot {
                    ghost: true,
                    ..rest
                },
                SoundEffect::Ghost,
            ),
            (
                KartSnapshot {
                    deflated: true,
                    ..rest
                },
                SoundEffect::Deflate,
            ),
            (
                KartSnapshot {
                    creamed: true,
                    ..rest
                },
                SoundEffect::Creamed,
            ),
            (
                KartSnapshot {
                    pinata: 0xff,
                    ..rest
                },
                SoundEffect::PinataBurst,
            ),
            (
                KartSnapshot {
                    impact: 9.0,
                    ..rest
                },
                SoundEffect::WallHit,
            ),
        ] {
            assert_eq!(cues(rest, now), [sound]);
            assert!(cues(now, now).is_empty() || sound == SoundEffect::WallHit);
        }
        // A bump between karts is not a wall.
        let bumped = kart_cues(
            &rest,
            &KartSnapshot {
                impact: 9.0,
                ..rest
            },
            TickEvents {
                bumped: true,
                ..Default::default()
            },
            &tuning,
            DT,
        );
        assert!(bumped.is_empty());
        // A boomerang-os: each throw (not a theft of the three).
        let three = KartSnapshot {
            hand: Some(ItemKind::Boomerang),
            hand_count: 3,
            ..rest
        };
        let two = KartSnapshot {
            hand_count: 2,
            ..three
        };
        let last = KartSnapshot {
            hand_count: 1,
            ..three
        };
        assert_eq!(cues(three, two), [SoundEffect::BoomerangThrow]);
        assert_eq!(cues(last, rest), [SoundEffect::BoomerangThrow]);
        assert!(cues(three, rest).is_empty(), "stolen");
        assert!(cues(two, two).is_empty());
        let swing = KartSnapshot {
            swing: Some((SwingKind::Whiff, 0)),
            ..rest
        };
        assert_eq!(cues(rest, swing), [SoundEffect::PawSwing]);
        let later = KartSnapshot {
            swing: Some((SwingKind::Whiff, 1)),
            ..rest
        };
        assert!(cues(swing, later).is_empty());
    }
}
