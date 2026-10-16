//! A kart's state on the wire (issue #8, `docs/ARCHITECTURE.md` § « Synchronisation »): what the
//! peer that has authority over a kart says about it 30 times a second, and how it is quantized.
//! Pure logic: unit-tested.
//!
//! - [`KartPose`]: the state in floats, as the game knows it: tick of the common clock, position,
//!   rotation, linear and angular velocities, and what the other screens need to draw the kart
//!   ([`KartLook`]: the phase, drift side and sparks, a hit's spin or tumble, the boost flames, a
//!   stalled engine, the blinking of invincibility, the slipstream's wind lines, the drift's slip
//!   angle and the wheels; what it holds out, issue #53; a super câlin, a fusée, a dégonflage,
//!   issue #61; a piñata out, an item a pie voleuse can take, invisible, issue #21).
//! - [`KartSnapshot`] (`protocol.rs`): the same, quantized, 48 bytes. Position to the millimeter
//!   (±2 100 km), rotation as « smallest three » on 48 bits (about 0.005°), velocities to the
//!   cm/s (±327 m/s) and the mrad/s (±32 rad/s).
//!
//! Nothing here depends on the order the snapshots arrive in: that is the buffer's job
//! (`interp.rs`).

use bevy::math::{Quat, Vec3};

use super::protocol::{HELD_ITEMS, ITEMS_HIDDEN, ITEMS_STEALABLE, KartId, KartSnapshot};
use crate::items::{ItemKind, ItemSlots, Slot};
use crate::kart::{
    Boost, DriftMotion, DriftSide, HitReaction, Invincibility, ItemEffects, KartEnvironment,
    KartState, KartTuning, KartWheels, MiniTurbo, Slipstream, Stall,
};

/// Position step on the wire (m).
pub const POSITION_STEP: f32 = 0.001;
/// Velocity step (m/s).
pub const VELOCITY_STEP: f32 = 0.01;
/// Angular velocity step (rad/s).
pub const SPIN_STEP: f32 = 0.001;
/// Drift slip angle step (rad): ±0.635 rad.
pub const SLIP_STEP: f32 = 0.005;
/// Boost strength step: 0 to +127.5 %.
pub const BOOST_STEP: f32 = 0.005;
/// Spring compression step of a wheel (m): 0 to 0.7 m.
pub const WHEEL_STEP: f32 = 0.05;
/// The simulation's tick (s): the boost's time left travels in ticks.
const TICK: f32 = 1.0 / crate::TICK_HZ as f32;
/// The three smaller components of a unit quaternion are within ±1/√2: 15 bits each.
const ROTATION_SCALE: f32 = 16_383.0;

/// What a kart is doing, for the screens that draw it (`KartState` without its counters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Grounded,
    Hop,
    Drifting,
    Airborne,
    Trick,
    /// Hit by an item: spinning out or tumbling ([`KartLook::reaction`]).
    Hit,
    /// Carried by the tow pelican.
    Towed,
}

impl Phase {
    const ALL: [Phase; 7] = [
        Phase::Grounded,
        Phase::Hop,
        Phase::Drifting,
        Phase::Airborne,
        Phase::Trick,
        Phase::Hit,
        Phase::Towed,
    ];

    fn index(self) -> u8 {
        Self::ALL.iter().position(|p| *p == self).unwrap_or(0) as u8
    }
}

/// What another screen needs to draw a kart besides its pose (`kart::visual`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KartLook {
    pub phase: Phase,
    /// Drifting to the right (else to the left).
    pub drift_right: bool,
    /// The tier the drift's sparks show.
    pub sparks: Option<MiniTurbo>,
    /// Hit by an item: the reaction, ticks since the hit, and its duration (ticks).
    pub reaction: HitReaction,
    pub hit_elapsed: u32,
    pub hit_duration: u32,
    /// The boost: strength (0.3 = +30 %) and time left (s): the flames.
    pub boost_strength: f32,
    pub boost_remaining: f32,
    /// The engine stalled at the start: puffs and a shaking body.
    pub stalled: bool,
    /// Invincible after an item hit: the kart blinks.
    pub invincible: bool,
    /// The slipstream gauge, while it charges (0 to 1): the wind lines.
    pub slipstream: Option<f32>,
    /// The drift's slip angle (rad, positive: nose to the right).
    pub slip: f32,
    /// Each wheel's spring compression (m), `None` in the air.
    pub wheels: [Option<f32>; 4],
    /// What the kart holds out (issue #53): its shields, drawn on every screen.
    pub held: Option<HeldLook>,
    /// Issue #61: in a super câlin (its pink glow; the karts it runs into tumble), in a fusée
    /// (its shell; the same), deflated (smaller).
    pub calin: bool,
    pub rocket: bool,
    pub deflated: bool,
    /// Issue #20: its glider open (the wing), under water (the bubbles): two more bits of the
    /// flags.
    pub gliding: bool,
    pub underwater: bool,
    /// Issue #21: the piñata's items left around it (one bit each, `items::PINATA_ITEMS`; 0
    /// without a piñata out); it holds an item a pie voleuse can take
    /// (`ItemSlots::stealable`); it is invisible (a pie voleuse).
    pub pinata: u8,
    pub stealable: bool,
    pub hidden: bool,
}

/// What a kart holds out around it (issue #53, `ItemSlots::held_out`): a single pelure or pelote
/// dragged behind, or an "x3" out (pelures in a row, pelotes or abeilles turning around).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldLook {
    pub item: ItemKind,
    pub count: u8,
    pub dragged: bool,
}

impl HeldLook {
    /// What `slots` hold out.
    pub fn of(slots: &ItemSlots) -> Option<Self> {
        let (item, count) = slots.held_out()?;
        HELD_ITEMS.contains(&item).then_some(Self {
            item,
            count: count.clamp(1, 3),
            dragged: slots.dragging.is_some(),
        })
    }

    /// Slots that hold the same out: a remote kart's on our screen, for its shields.
    pub fn slots(held: Option<Self>) -> ItemSlots {
        match held {
            None => ItemSlots::default(),
            Some(held) if held.dragged => ItemSlots {
                dragging: Some(held.item),
                ..Default::default()
            },
            Some(held) => ItemSlots {
                hand: Slot::Ready {
                    item: held.item,
                    count: held.count,
                },
                deployed: true,
                ..Default::default()
            },
        }
    }

    /// On the wire (`KartSnapshot::items`).
    fn byte(held: Option<Self>) -> u8 {
        held.and_then(|held| {
            let kind = HELD_ITEMS.iter().position(|i| *i == held.item)? as u8 + 1;
            let count = if held.dragged {
                1
            } else {
                held.count.clamp(1, 3)
            };
            Some(kind | count << 2 | u8::from(held.dragged && kind <= 2) << 4)
        })
        .unwrap_or(0)
    }

    /// From the wire (a value `protocol::valid_held_items` refuses: nothing).
    fn from_byte(items: u8) -> Option<Self> {
        if !super::protocol::valid_held_items(items) {
            return None;
        }
        let item = *HELD_ITEMS.get(usize::from(items & 0b11).checked_sub(1)?)?;
        Some(Self {
            item,
            count: (items >> 2) & 0b11,
            dragged: items & 0b1_0000 != 0,
        })
    }
}

/// The placeholder of an item a remote kart holds in a slot (issue #21): its states only say it
/// holds one a pie voleuse can take, not which. Only `ItemSlots::stealable` reads it.
const UNKNOWN_ITEM: Slot = Slot::Ready {
    item: ItemKind::Pelure,
    count: 1,
};

impl KartLook {
    /// What `slots` say on the wire (issues #53 and #21): what they hold out, the piñata's items
    /// left, whether a pie voleuse could take an item.
    pub fn set_items(&mut self, slots: Option<&ItemSlots>) {
        self.held = slots.and_then(HeldLook::of);
        self.pinata = slots
            .filter(|s| s.deployed && s.hand.item().is_some_and(|(i, _)| i == ItemKind::Pinata))
            .map_or(0, |s| s.pinata);
        self.stealable = slots.is_some_and(|s| s.stealable().is_some());
    }

    /// Slots that look the same (a remote kart's on our screen): its shields, its piñata's
    /// items (issue #21), and an unknown item in its reserve if a pie voleuse can take one.
    pub fn slots(&self) -> ItemSlots {
        let mut slots = HeldLook::slots(self.held);
        if self.pinata != 0 && self.held.is_none() {
            slots.hand = Slot::Ready {
                item: ItemKind::Pinata,
                count: self.pinata.count_ones() as u8,
            };
            slots.deployed = true;
            slots.pinata = self.pinata;
        }
        if self.stealable {
            slots.reserve = UNKNOWN_ITEM;
        }
        slots
    }

    /// Whether `slots` (a remote kart's on our screen) look like these items.
    pub fn shows_slots(&self, slots: &ItemSlots) -> bool {
        let shown = self.slots();
        HeldLook::of(slots) == self.held
            && slots.pinata == shown.pinata
            && slots.stealable().is_some() == self.stealable
    }
}

/// The parts of a kart that its look comes from.
pub struct LookParts<'a> {
    pub state: &'a KartState,
    /// The tier its sparks show (`KartState::mini_turbo`, with its own tuning).
    pub sparks: Option<MiniTurbo>,
    pub motion: &'a DriftMotion,
    pub boost: &'a Boost,
    pub stall: &'a Stall,
    pub invincibility: &'a Invincibility,
    pub slipstream: &'a Slipstream,
    /// `Slipstream::fill` with its tuning.
    pub slipstream_fill: f32,
    pub wheels: &'a KartWheels,
    /// What the items of phase 2 do to it (issue #61).
    pub effects: &'a ItemEffects,
    /// Where it is (issue #20): gliding, under water.
    pub environment: &'a KartEnvironment,
}

impl KartLook {
    /// The look of a kart, from its components.
    pub fn of(parts: &LookParts) -> Self {
        let mut look = KartLook {
            sparks: parts.sparks,
            boost_strength: if parts.boost.is_active() {
                parts.boost.strength
            } else {
                0.0
            },
            boost_remaining: parts.boost.remaining,
            stalled: parts.stall.is_active(),
            invincible: parts.invincibility.is_active(),
            slipstream: parts.slipstream.charging.then_some(parts.slipstream_fill),
            slip: parts.motion.slip,
            wheels: std::array::from_fn(|i| {
                parts.wheels.on_ground[i].then_some(parts.wheels.compression[i])
            }),
            calin: parts.effects.calin > 0.0,
            rocket: parts.effects.rocket > 0.0,
            deflated: parts.effects.deflated > 0.0,
            gliding: parts.environment.gliding,
            underwater: parts.environment.underwater,
            hidden: parts.effects.hidden(),
            ..Default::default()
        };
        look.phase = match *parts.state {
            KartState::Grounded => Phase::Grounded,
            KartState::Hop { side, .. } => {
                look.drift_right = side == Some(DriftSide::Right);
                Phase::Hop
            }
            KartState::Drifting { side, .. } => {
                look.drift_right = side == DriftSide::Right;
                Phase::Drifting
            }
            KartState::Airborne => Phase::Airborne,
            KartState::Trick => Phase::Trick,
            KartState::Hit {
                reaction,
                elapsed,
                duration,
            } => {
                look.reaction = reaction;
                look.hit_elapsed = elapsed;
                look.hit_duration = duration;
                Phase::Hit
            }
            KartState::Respawning => Phase::Towed,
        };
        look
    }

    /// A `KartState` that looks the same on screen. A drift's charge is the threshold of the
    /// tier its sparks show, with `tuning` (the kart's own: its stats move the thresholds).
    pub fn kart_state(&self, tuning: &KartTuning) -> KartState {
        let side = if self.drift_right {
            DriftSide::Right
        } else {
            DriftSide::Left
        };
        match self.phase {
            Phase::Grounded => KartState::Grounded,
            Phase::Hop => KartState::Hop {
                ticks: 0,
                side: Some(side),
            },
            Phase::Drifting => KartState::Drifting {
                side,
                charge: self
                    .sparks
                    .map_or(0.0, |tier| tuning.mini_turbo_thresholds[tier.index()]),
            },
            Phase::Airborne => KartState::Airborne,
            Phase::Trick => KartState::Trick,
            Phase::Hit => KartState::Hit {
                reaction: self.reaction,
                elapsed: self.hit_elapsed,
                duration: self.hit_duration.max(1),
            },
            Phase::Towed => KartState::Respawning,
        }
    }

    /// The `Boost` that shows the same flames.
    pub fn boost(&self) -> Boost {
        Boost {
            strength: self.boost_strength,
            remaining: if self.boost_strength > 0.0 {
                self.boost_remaining
            } else {
                0.0
            },
        }
    }

    /// The `KartWheels` that put the wheels at the same place.
    pub fn kart_wheels(&self) -> KartWheels {
        let mut wheels = KartWheels::default();
        for (i, wheel) in self.wheels.iter().enumerate() {
            wheels.on_ground[i] = wheel.is_some();
            wheels.compression[i] = wheel.unwrap_or(0.0);
        }
        wheels
    }
}

/// A kart's state at a tick, in floats (see the module's documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KartPose {
    /// Tick of the room's common clock.
    pub tick: u32,
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    /// Angular velocity (rad/s).
    pub spin: Vec3,
    pub look: KartLook,
}

impl KartSnapshot {
    /// The state of kart `kart` on the wire: rounded to the nearest step, clamped to what the
    /// fields hold.
    pub fn quantize(kart: KartId, pose: &KartPose) -> Self {
        let look = &pose.look;
        let state = look.phase.index()
            | u8::from(look.drift_right) << 3
            | (look.sparks.map_or(0, |tier| tier.index() as u8 + 1)) << 4
            | u8::from(look.reaction == HitReaction::Tumble) << 6;
        let mut wheels = 0u16;
        for (i, wheel) in look.wheels.iter().enumerate() {
            let value = wheel.map_or(0, |compression| {
                1 + (compression / WHEEL_STEP).round().clamp(0.0, 14.0) as u16
            });
            wheels |= value << (4 * i);
        }
        Self {
            tick: pose.tick,
            kart,
            session: 0,
            position: pose.position.to_array().map(|v| step_i32(v, POSITION_STEP)),
            rotation: pack_rotation(pose.rotation),
            velocity: pose.velocity.to_array().map(|v| step_i16(v, VELOCITY_STEP)),
            spin: pose.spin.to_array().map(|v| step_i16(v, SPIN_STEP)),
            state,
            flags: u8::from(look.stalled)
                | u8::from(look.invincible) << 1
                | u8::from(look.calin) << 2
                | u8::from(look.rocket) << 3
                | u8::from(look.deflated) << 4
                | u8::from(look.gliding) << 5
                | u8::from(look.underwater) << 6,
            hit: [
                look.hit_elapsed.min(255) as u8,
                look.hit_duration.min(255) as u8,
            ],
            boost: [
                (look.boost_strength / BOOST_STEP).round().clamp(0.0, 255.0) as u8,
                (look.boost_remaining / TICK).round().clamp(0.0, 255.0) as u8,
            ],
            slip: (look.slip / SLIP_STEP).round().clamp(-127.0, 127.0) as i8,
            slipstream: look
                .slipstream
                .map_or(0, |fill| (fill * 255.0).round().clamp(1.0, 255.0) as u8),
            wheels,
            items: HeldLook::byte(look.held)
                | if look.stealable { ITEMS_STEALABLE } else { 0 }
                | if look.hidden { ITEMS_HIDDEN } else { 0 },
            // A piñata out is all it holds out.
            pinata: if look.held.is_none() { look.pinata } else { 0 },
        }
    }

    /// The state in floats (whatever the bytes: a garbled rotation comes back as a unit one).
    pub fn pose(&self) -> KartPose {
        let tier = (self.state >> 4) & 0b11;
        let look = KartLook {
            phase: Phase::ALL
                .get(usize::from(self.state & 0b111))
                .copied()
                .unwrap_or_default(),
            drift_right: self.state & 0b1000 != 0,
            sparks: (tier > 0).then(|| MiniTurbo::ALL[usize::from(tier - 1)]),
            reaction: if self.state & 0b100_0000 != 0 {
                HitReaction::Tumble
            } else {
                HitReaction::Spin
            },
            hit_elapsed: u32::from(self.hit[0]),
            hit_duration: u32::from(self.hit[1]),
            boost_strength: f32::from(self.boost[0]) * BOOST_STEP,
            boost_remaining: f32::from(self.boost[1]) * TICK,
            stalled: self.flags & 1 != 0,
            invincible: self.flags & 2 != 0,
            slipstream: (self.slipstream > 0).then(|| f32::from(self.slipstream) / 255.0),
            slip: f32::from(self.slip) * SLIP_STEP,
            wheels: std::array::from_fn(|i| {
                let value = (self.wheels >> (4 * i)) & 0xf;
                (value > 0).then(|| f32::from(value - 1) * WHEEL_STEP)
            }),
            held: HeldLook::from_byte(self.items),
            calin: self.flags & 0b100 != 0,
            rocket: self.flags & 0b1000 != 0,
            deflated: self.flags & 0b1_0000 != 0,
            gliding: self.flags & 0b10_0000 != 0,
            underwater: self.flags & 0b100_0000 != 0,
            pinata: self.pinata,
            stealable: self.items & ITEMS_STEALABLE != 0,
            hidden: self.items & ITEMS_HIDDEN != 0,
        };
        KartPose {
            tick: self.tick,
            position: Vec3::from_array(self.position.map(|v| v as f32 * POSITION_STEP)),
            rotation: unpack_rotation(self.rotation),
            velocity: Vec3::from_array(self.velocity.map(|v| f32::from(v) * VELOCITY_STEP)),
            spin: Vec3::from_array(self.spin.map(|v| f32::from(v) * SPIN_STEP)),
            look,
        }
    }
}

fn step_i32(value: f32, step: f32) -> i32 {
    let steps = f64::from(value) / f64::from(step);
    if steps.is_finite() {
        steps
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    } else {
        0
    }
}

fn step_i16(value: f32, step: f32) -> i16 {
    let steps = value / step;
    if steps.is_finite() {
        steps.round().clamp(-32_767.0, 32_767.0) as i16
    } else {
        0
    }
}

/// A rotation in 48 bits, « smallest three »: bits 0-1 say which component of the unit
/// quaternion is the largest (it is made positive: `q` and `-q` are the same rotation), and the
/// three others follow, 15 bits each (signed), scaled from ±1/√2. Bit 47 stays 0.
pub fn pack_rotation(rotation: Quat) -> [u8; 6] {
    let q = if rotation.is_finite() && rotation.length_squared() > 1e-12 {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    };
    let components = q.to_array();
    let largest = (0..4)
        .max_by(|a, b| components[*a].abs().total_cmp(&components[*b].abs()))
        .unwrap_or(3);
    let sign = if components[largest] < 0.0 { -1.0 } else { 1.0 };
    let mut bits = largest as u64;
    let mut shift = 2;
    for (i, component) in components.iter().enumerate() {
        if i == largest {
            continue;
        }
        let scaled = (component * sign * std::f32::consts::SQRT_2 * ROTATION_SCALE)
            .round()
            .clamp(-ROTATION_SCALE, ROTATION_SCALE) as i32;
        bits |= (scaled as u64 & 0x7fff) << shift;
        shift += 15;
    }
    let bytes = bits.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]
}

/// The rotation of [`pack_rotation`]'s 48 bits; any bits give a unit quaternion.
pub fn unpack_rotation(bytes: [u8; 6]) -> Quat {
    let mut wide = [0u8; 8];
    wide[..6].copy_from_slice(&bytes);
    let bits = u64::from_le_bytes(wide);
    let largest = (bits & 0b11) as usize;
    let mut components = [0.0f32; 4];
    let mut sum = 0.0;
    let mut shift = 2;
    for (i, component) in components.iter_mut().enumerate() {
        if i == largest {
            continue;
        }
        let raw = ((bits >> shift) & 0x7fff) as i32;
        let signed = if raw >= 0x4000 { raw - 0x8000 } else { raw };
        *component = signed as f32 / (ROTATION_SCALE * std::f32::consts::SQRT_2);
        sum += *component * *component;
        shift += 15;
    }
    components[largest] = (1.0 - sum).max(0.0).sqrt();
    let q = Quat::from_array(components);
    if q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::net::Message;

    /// A random source for the tests (xorshift).
    pub(crate) struct Draw(pub u64);

    impl Draw {
        pub fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// Uniform in [lo, hi).
        pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (hi - lo) * (self.next() >> 40) as f32 / (1u64 << 24) as f32
        }
    }

    fn look() -> KartLook {
        KartLook {
            phase: Phase::Drifting,
            drift_right: true,
            sparks: Some(MiniTurbo::Orange),
            reaction: HitReaction::Tumble,
            hit_elapsed: 12,
            hit_duration: 96,
            boost_strength: 0.3,
            boost_remaining: 1.2,
            stalled: false,
            invincible: true,
            slipstream: Some(0.4),
            slip: -0.21,
            wheels: [Some(0.12), Some(0.3), None, Some(0.0)],
            held: Some(HeldLook {
                item: ItemKind::Pelote,
                count: 3,
                dragged: false,
            }),
            calin: true,
            rocket: false,
            deflated: true,
            gliding: true,
            underwater: false,
            pinata: 0,
            stealable: true,
            hidden: true,
        }
    }

    /// Issue #8 acceptance: a kart state survives the wire, and its quantization errors stay
    /// within their steps, on random poses across a big circuit.
    #[test]
    fn a_kart_state_survives_the_wire_within_its_steps() {
        let mut draw = Draw(0x1234_5678_9abc_def1);
        let mut worst = [0.0f32; 4];
        for i in 0..5_000 {
            let axis = Vec3::new(
                draw.range(-1.0, 1.0),
                draw.range(-1.0, 1.0),
                draw.range(-1.0, 1.0),
            )
            .normalize_or(Vec3::Y);
            let pose = KartPose {
                tick: 1_000 + i,
                position: Vec3::new(
                    draw.range(-2_000.0, 2_000.0),
                    draw.range(-50.0, 200.0),
                    draw.range(-2_000.0, 2_000.0),
                ),
                rotation: Quat::from_axis_angle(axis, draw.range(-6.3, 6.3)),
                velocity: Vec3::new(
                    draw.range(-45.0, 45.0),
                    draw.range(-30.0, 30.0),
                    draw.range(-45.0, 45.0),
                ),
                spin: Vec3::new(
                    draw.range(-8.0, 8.0),
                    draw.range(-8.0, 8.0),
                    draw.range(-8.0, 8.0),
                ),
                look: look(),
            };
            let wire = KartSnapshot::quantize(7, &pose);
            let Ok(Message::KartState(read)) =
                Message::decode(&Message::KartState(wire).to_bytes())
            else {
                panic!("not read back");
            };
            assert_eq!(read, wire);
            assert_eq!(read.kart, 7);
            let back = read.pose();
            assert_eq!(back.tick, pose.tick);
            let position = (back.position - pose.position).abs().max_element();
            // The angle between the two rotations (from the sine: precise for small angles).
            let turn = back.rotation * pose.rotation.inverse();
            let angle = 2.0 * turn.xyz().length().min(1.0).asin();
            let velocity = (back.velocity - pose.velocity).abs().max_element();
            let spin = (back.spin - pose.spin).abs().max_element();
            for (worst, error) in worst.iter_mut().zip([position, angle, velocity, spin]) {
                *worst = worst.max(error);
            }
        }
        let [position, angle, velocity, spin] = worst;
        // Half a step, plus the floats' own rounding 2 km away (about 0.1 mm).
        assert!(position <= 0.0006, "position error {position} m");
        assert!(angle < 0.000_2, "rotation error {angle} rad");
        assert!(velocity <= 0.005 + 1e-5, "velocity error {velocity} m/s");
        assert!(
            spin <= 0.000_5 + 1e-6,
            "angular velocity error {spin} rad/s"
        );
    }

    /// What the other screens need comes back the same (to its steps).
    #[test]
    fn the_look_comes_back() {
        let pose = KartPose {
            look: look(),
            ..Default::default()
        };
        let back = KartSnapshot::quantize(0, &pose).pose().look;
        assert_eq!(back.phase, Phase::Drifting);
        assert!(back.drift_right);
        assert_eq!(back.sparks, Some(MiniTurbo::Orange));
        assert_eq!(back.reaction, HitReaction::Tumble);
        assert_eq!((back.hit_elapsed, back.hit_duration), (12, 96));
        assert!((back.boost_strength - 0.3).abs() <= BOOST_STEP / 2.0);
        assert!((back.boost_remaining - 1.2).abs() <= TICK / 2.0 + 1e-6);
        assert!(!back.stalled && back.invincible);
        // Issue #61: the super câlin, the fusée and the dégonflage, each on its own.
        assert!(back.calin && !back.rocket && back.deflated);
        for (calin, rocket, deflated) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            let pose = KartPose {
                look: KartLook {
                    calin,
                    rocket,
                    deflated,
                    stalled: true,
                    invincible: false,
                    ..look()
                },
                ..Default::default()
            };
            let wire = KartSnapshot::quantize(0, &pose);
            let back = wire.pose().look;
            assert_eq!(
                (back.calin, back.rocket, back.deflated),
                (calin, rocket, deflated)
            );
            assert!(back.stalled && !back.invincible);
            assert!(wire.flags <= crate::net::protocol::KART_FLAGS);
        }
        assert!((back.slipstream.unwrap() - 0.4).abs() < 0.003);
        assert!((back.slip + 0.21).abs() <= SLIP_STEP / 2.0 + 1e-6);
        assert_eq!(back.wheels[2], None);
        assert_eq!(back.wheels[3], Some(0.0));
        // Issue #53: what it holds out comes back, and gives slots that hold the same out.
        assert_eq!(back.held, look().held);
        for held in [
            None,
            Some(HeldLook {
                item: ItemKind::Pelure,
                count: 1,
                dragged: true,
            }),
            Some(HeldLook {
                item: ItemKind::Pelote,
                count: 1,
                dragged: true,
            }),
            Some(HeldLook {
                item: ItemKind::Pelure,
                count: 2,
                dragged: false,
            }),
            Some(HeldLook {
                item: ItemKind::Abeille,
                count: 1,
                dragged: false,
            }),
        ] {
            let pose = KartPose {
                look: KartLook { held, ..look() },
                ..Default::default()
            };
            assert_eq!(KartSnapshot::quantize(0, &pose).pose().look.held, held);
            assert_eq!(HeldLook::of(&HeldLook::slots(held)), held);
        }
        for (wheel, sent) in back.wheels.iter().zip(look().wheels) {
            if let (Some(back), Some(sent)) = (wheel, sent) {
                assert!((back - sent).abs() <= WHEEL_STEP / 2.0 + 1e-6);
            }
        }
        // Every phase, both reactions, every tier.
        for phase in Phase::ALL {
            for sparks in [None, Some(MiniTurbo::Blue), Some(MiniTurbo::Purple)] {
                let look = KartLook {
                    phase,
                    sparks,
                    slipstream: None,
                    ..look()
                };
                let pose = KartPose {
                    look,
                    ..Default::default()
                };
                let back = KartSnapshot::quantize(0, &pose).pose().look;
                assert_eq!((back.phase, back.sparks), (phase, sparks));
                assert_eq!(back.slipstream, None);
            }
        }
        // A tiny slipstream gauge still says it charges.
        let pose = KartPose {
            look: KartLook {
                slipstream: Some(0.0),
                ..look()
            },
            ..Default::default()
        };
        assert_eq!(
            KartSnapshot::quantize(0, &pose).pose().look.slipstream,
            Some(1.0 / 255.0)
        );
    }

    /// Issue #21: a piñata out (its items left), an item a pie voleuse can take, invisible:
    /// they come back from the wire, each on its own; the slots they give a remote kart hold the
    /// same piñata out, and an item to take. A piñata out is all a kart holds out.
    #[test]
    fn the_items_of_phase_three_come_back() {
        use crate::items::{PINATA_FULL, Use};
        let mut pinata = ItemSlots::default();
        pinata.give(ItemKind::Pinata, 8, 0);
        pinata.give(ItemKind::Bombe, 1, 0);
        assert_eq!(
            pinata.step_button(true, false),
            Use::Deploy {
                item: ItemKind::Pinata,
                count: 8
            }
        );
        pinata.take_pinata(2);
        let mut out = KartLook::default();
        out.set_items(Some(&pinata));
        assert_eq!(out.held, None);
        assert_eq!(out.pinata, PINATA_FULL & !0b100);
        assert!(out.stealable, "the bombe in the reserve");
        for (pinata, stealable, hidden) in [
            (0, false, false),
            (0b1011_0101, true, false),
            (0, false, true),
            (0xff, true, true),
        ] {
            let pose = KartPose {
                look: KartLook {
                    held: None,
                    pinata,
                    stealable,
                    hidden,
                    ..look()
                },
                ..Default::default()
            };
            let wire = KartSnapshot::quantize(0, &pose);
            let back = wire.pose().look;
            assert_eq!(
                (back.pinata, back.stealable, back.hidden),
                (pinata, stealable, hidden)
            );
            assert!(crate::net::protocol::valid_held_items(wire.items));
            let slots = back.slots();
            assert!(back.shows_slots(&slots));
            assert_eq!(slots.stealable().is_some(), stealable);
            assert_eq!(
                slots.held_out().map(|(_, n)| n),
                (pinata != 0).then(|| pinata.count_ones() as u8)
            );
            assert!((0..8).all(|k| slots.has_pinata(k) == (pinata & (1 << k) != 0)));
        }
        // A piñata out with something held out cannot be (it never is): the piñata is left out.
        let both = KartPose {
            look: KartLook {
                pinata: 0xff,
                ..look()
            },
            ..Default::default()
        };
        let wire = KartSnapshot::quantize(0, &both);
        assert_eq!(wire.pinata, 0);
        assert_eq!(wire.pose().look.held, look().held);
    }

    /// The look of a kart's components, and the components that look the same.
    #[test]
    fn a_look_goes_from_the_components_and_back() {
        let tuning = KartTuning::default();
        let state = KartState::Drifting {
            side: DriftSide::Left,
            charge: tuning.mini_turbo_thresholds[1] + 3.0,
        };
        let wheels = KartWheels {
            on_ground: [true, true, false, true],
            compression: [0.2, 0.25, 0.0, 0.1],
            ..Default::default()
        };
        let motion = DriftMotion {
            slip: 0.15,
            ..Default::default()
        };
        let boost = Boost {
            strength: 0.25,
            remaining: 0.5,
        };
        let look = KartLook::of(&LookParts {
            state: &state,
            sparks: state.mini_turbo(&tuning, false),
            motion: &motion,
            boost: &boost,
            stall: &Stall::default(),
            invincibility: &Invincibility { remaining: 0.3 },
            slipstream: &Slipstream {
                charge: 0.2,
                charging: true,
            },
            slipstream_fill: 0.5,
            wheels: &wheels,
            effects: &ItemEffects {
                rocket: 2.0,
                deflated: 1.0,
                ..Default::default()
            },
            environment: &KartEnvironment {
                underwater: true,
                ..Default::default()
            },
        });
        assert_eq!(look.phase, Phase::Drifting);
        assert!(!look.calin && look.rocket && look.deflated);
        assert!(look.underwater && !look.gliding);
        assert_eq!(look.sparks, Some(MiniTurbo::Orange));
        assert!(!look.drift_right && look.invincible && !look.stalled);
        assert_eq!(look.slipstream, Some(0.5));
        assert_eq!(look.wheels, [Some(0.2), Some(0.25), None, Some(0.1)]);
        // Back: the same sparks, the same side, the same flames and wheels.
        let shown = look.kart_state(&tuning);
        assert_eq!(shown.mini_turbo(&tuning, false), Some(MiniTurbo::Orange));
        assert!(matches!(
            shown,
            KartState::Drifting {
                side: DriftSide::Left,
                ..
            }
        ));
        assert_eq!(look.boost(), boost);
        assert_eq!(look.kart_wheels().on_ground, wheels.on_ground);
        assert_eq!(look.kart_wheels().compression, wheels.compression);
        // A hit, a tow.
        let hit = KartState::Hit {
            reaction: HitReaction::Spin,
            elapsed: 20,
            duration: 60,
        };
        let look = KartLook::of(&LookParts {
            state: &hit,
            sparks: None,
            motion: &motion,
            boost: &Boost::default(),
            stall: &Stall { remaining: 0.5 },
            invincibility: &Invincibility::default(),
            slipstream: &Slipstream::default(),
            slipstream_fill: 0.0,
            wheels: &wheels,
            effects: &ItemEffects::default(),
            environment: &KartEnvironment {
                gliding: true,
                ..Default::default()
            },
        });
        assert_eq!(look.kart_state(&tuning), hit);
        assert!(look.gliding && !look.underwater);
        assert!(!look.calin && !look.rocket && !look.deflated);
        assert!(look.stalled && look.slipstream.is_none());
        assert_eq!(look.boost(), Boost::default());
        let towed = LookParts {
            state: &KartState::Respawning,
            sparks: None,
            motion: &motion,
            boost: &Boost::default(),
            stall: &Stall::default(),
            invincibility: &Invincibility::default(),
            slipstream: &Slipstream::default(),
            slipstream_fill: 0.0,
            wheels: &wheels,
            effects: &ItemEffects::default(),
            environment: &KartEnvironment::default(),
        };
        assert_eq!(
            KartLook::of(&towed).kart_state(&tuning),
            KartState::Respawning
        );
    }

    /// Any 48 bits give a unit rotation, never a panic nor a NaN; a rotation and its opposite
    /// quaternion travel the same.
    #[test]
    fn rotations_are_robust() {
        let mut draw = Draw(42);
        for _ in 0..10_000 {
            let bytes = draw.next().to_le_bytes();
            let q = unpack_rotation([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]);
            assert!(q.is_finite() && (q.length() - 1.0).abs() < 1e-4, "{q}");
        }
        let q = Quat::from_rotation_y(2.0) * Quat::from_rotation_x(0.3);
        assert_eq!(pack_rotation(q), pack_rotation(-q));
        assert_eq!(pack_rotation(Quat::NAN), pack_rotation(Quat::IDENTITY));
        assert_eq!(pack_rotation(q)[5] & 0x80, 0);
        // Non-finite values travel as 0, clamped values to the edge.
        let pose = KartPose {
            position: Vec3::new(f32::NAN, 1e12, -1e12),
            velocity: Vec3::new(f32::INFINITY, 1_000.0, -1_000.0),
            ..Default::default()
        };
        let wire = KartSnapshot::quantize(0, &pose);
        assert_eq!(wire.position, [0, i32::MAX, i32::MIN]);
        assert_eq!(wire.velocity, [0, 32_767, -32_767]);
    }
}
