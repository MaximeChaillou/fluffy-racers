//! Issue #101: the players' characters on foot in the lobby, on the network
//! (`docs/ARCHITECTURE.md` § « Le salon en 3D »). Pure logic, unit-tested; the lobby's scene
//! (`crate::salon`) moves them.
//!
//! - **Each player has authority over its character**: its peer simulates it (walks, jumps,
//!   tumbles) and tells its state 30 times a second ([`FootState`], 33 bytes, unreliable): a client
//!   to the host, the host to each client in **one packet** with every other character
//!   ([`Message::FootStates`], [`FootRelay`]), like the karts' states (issue #63).
//! - **The others' characters** ([`RemoteWalker`]): their states fill a buffer each, shown about
//!   100 ms after they arrive, interpolated (the karts' [`SnapshotBuffer`] and [`Playback`], issue
//!   #8: the curve of their velocities, a slerp of their rotation, the tumble carried on past the
//!   newest state for 250 ms at most), and what they look like ([`FootLook`]: their ragdoll, in
//!   the air, their paw) from the newest state shown ([`LookTrack`]).
//! - **A paw's strike on foot** ([`FootStrike`]): the striker judges its reach on its screen and
//!   tells everyone (the host relays it); **the victim decides** ([`FootSync::drain_strikes_in`]:
//!   only those for us), from where it was at the tick the striker saw (its own history), and
//!   falls over if it is not down or protected. Everyone sees it fall by its states: nothing more
//!   is sent.
//!
//! [`FootSync`] exists only while the lobby's scene is up in a room: nothing of it in a race, nor
//! solo, nor in the headless world of the bot races.

use bevy::math::{Quat, Vec3};
use bevy::prelude::{Entity, Resource};

use super::interp::{
    BUFFER_CAPACITY, Insert, Playback, RESTART_TICKS, SampleLimits, SnapshotBuffer, TICK_MS,
};
use super::protocol::{FootState, FootStrike, MAX_PLAYERS, Message, PeerKey};
use super::snapshot::{KartPose, pack_rotation, unpack_rotation};
use super::tuning::NetTuning;
use crate::characters::RagdollPhase;
use crate::kart::KartColor;

/// Position step of a character on the wire (m): ±327 m on 16 bits.
pub const FOOT_POSITION_STEP: f32 = 0.01;
/// Velocity step (m/s): ±327 m/s.
pub const FOOT_VELOCITY_STEP: f32 = 0.01;
/// Angular velocity step (rad/s): ±32 rad/s.
pub const FOOT_SPIN_STEP: f32 = 0.001;

/// What another screen needs to draw a character besides its pose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FootLook {
    /// Its ragdoll (`characters::Ragdoll`), and how long it has been so (ticks, to 255 on the
    /// wire).
    pub phase: RagdollPhase,
    pub phase_ticks: u32,
    /// Off the ground (a jump, a bouncer, a fall).
    pub airborne: bool,
    /// Its paw's swing: ticks since it began, and on its right (else its left).
    pub paw: Option<(u32, bool)>,
}

impl FootLook {
    /// The same look `ticks` later: the counters run on.
    pub fn later(self, ticks: u32) -> Self {
        Self {
            phase_ticks: self.phase_ticks.saturating_add(ticks),
            paw: self.paw.map(|(t, right)| (t.saturating_add(ticks), right)),
            ..self
        }
    }
}

/// A character's state in floats, as the game knows it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FootPose {
    /// Tick of the room's common clock.
    pub tick: u32,
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    /// Angular velocity (rad/s).
    pub spin: Vec3,
    pub look: FootLook,
}

impl FootPose {
    /// The pose for the karts' buffer (`SnapshotBuffer`), without its look.
    pub fn kart_pose(&self) -> KartPose {
        KartPose {
            tick: self.tick,
            position: self.position,
            rotation: self.rotation,
            velocity: self.velocity,
            spin: self.spin,
            look: Default::default(),
        }
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

fn phase_bits(phase: RagdollPhase) -> u8 {
    match phase {
        RagdollPhase::Upright => 0,
        RagdollPhase::Tumbling => 1,
        RagdollPhase::GettingUp => 2,
    }
}

impl FootState {
    /// The state of a character on the wire: rounded to the nearest step, clamped to what the
    /// fields hold.
    pub fn quantize(pose: &FootPose) -> Self {
        let look = &pose.look;
        let pose_bits = phase_bits(look.phase)
            | u8::from(look.airborne) << 2
            | u8::from(look.paw.is_some()) << 3
            | u8::from(look.paw.is_some_and(|(_, right)| right)) << 4;
        Self {
            tick: pose.tick,
            position: pose
                .position
                .to_array()
                .map(|v| step_i16(v, FOOT_POSITION_STEP)),
            rotation: pack_rotation(pose.rotation),
            velocity: pose
                .velocity
                .to_array()
                .map(|v| step_i16(v, FOOT_VELOCITY_STEP)),
            spin: pose.spin.to_array().map(|v| step_i16(v, FOOT_SPIN_STEP)),
            pose: pose_bits,
            pose_ticks: look.phase_ticks.min(255) as u8,
            paw_ticks: look.paw.map_or(0, |(ticks, _)| ticks.min(255) as u8),
        }
    }

    /// The state in floats (whatever the bytes: a garbled rotation comes back as a unit one).
    pub fn pose(&self) -> FootPose {
        let phase = match self.pose & 0b11 {
            1 => RagdollPhase::Tumbling,
            2 => RagdollPhase::GettingUp,
            _ => RagdollPhase::Upright,
        };
        FootPose {
            tick: self.tick,
            position: Vec3::from_array(self.position.map(|v| f32::from(v) * FOOT_POSITION_STEP)),
            rotation: unpack_rotation(self.rotation),
            velocity: Vec3::from_array(self.velocity.map(|v| f32::from(v) * FOOT_VELOCITY_STEP)),
            spin: Vec3::from_array(self.spin.map(|v| f32::from(v) * FOOT_SPIN_STEP)),
            look: FootLook {
                phase,
                phase_ticks: u32::from(self.pose_ticks),
                airborne: self.pose & 0b100 != 0,
                paw: (self.pose & 0b1000 != 0)
                    .then(|| (u32::from(self.paw_ticks), self.pose & 0b1_0000 != 0)),
            },
        }
    }
}

/// The looks of a remote character by tick, alongside its poses' buffer: what the state shown
/// says (the newest at or before the tick shown), its counters run on to that tick. Nothing
/// allocated once created.
#[derive(Clone, Debug)]
pub struct LookTrack {
    looks: Vec<(u32, FootLook)>,
}

impl Default for LookTrack {
    fn default() -> Self {
        Self {
            looks: Vec::with_capacity(BUFFER_CAPACITY + 1),
        }
    }
}

impl LookTrack {
    pub fn len(&self) -> usize {
        self.looks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.looks.is_empty()
    }

    pub fn clear(&mut self) {
        self.looks.clear();
    }

    /// Keeps the look of a state, in tick order, whatever order they come in.
    pub fn insert(&mut self, tick: u32, look: FootLook) {
        if let Some((newest, _)) = self.looks.last()
            && tick.saturating_add(RESTART_TICKS) < *newest
        {
            // Its clock jumped back: start over, like the poses' buffer.
            self.looks.clear();
        }
        let at = match self.looks.binary_search_by_key(&tick, |(t, _)| *t) {
            Ok(_) => return,
            Err(at) => at,
        };
        if at == 0 && self.looks.len() >= BUFFER_CAPACITY {
            return;
        }
        self.looks.insert(at, (tick, look));
        if self.looks.len() > BUFFER_CAPACITY {
            self.looks.remove(0);
        }
    }

    /// The look at `tick` (fractional): the newest state's at or before it, its counters run on
    /// (before the oldest: the oldest's).
    pub fn at(&self, tick: f64) -> Option<FootLook> {
        let after = self.looks.partition_point(|(t, _)| f64::from(*t) <= tick);
        let (at, look) = *self.looks.get(after.saturating_sub(1))?;
        let ahead = (tick - f64::from(at)).max(0.0).floor() as u32;
        Some(look.later(ahead))
    }
}

/// Another player's character, as we know it.
#[derive(Clone, Debug)]
pub struct RemoteWalker {
    /// Its player (the peer that has authority over it).
    pub peer: PeerKey,
    /// Its animal (an index of the roster), the color its player wears in the room (issue #55),
    /// its pseudo.
    pub character: usize,
    pub color: KartColor,
    pub name: String,
    pub buffer: SnapshotBuffer,
    pub looks: LookTrack,
    pub playback: Playback,
    /// States received.
    pub received: u32,
    /// Its body and its visual, once spawned (`crate::salon`).
    pub body: Option<Entity>,
    pub visual: Option<Entity>,
    /// Its animal changed: spawned again.
    pub respawn: bool,
    present: bool,
}

impl RemoteWalker {
    fn new(entry: &WalkerEntry) -> Self {
        Self {
            peer: entry.peer,
            character: entry.character,
            color: entry.color,
            name: entry.name.to_string(),
            buffer: SnapshotBuffer::default(),
            looks: LookTrack::default(),
            playback: Playback::default(),
            received: 0,
            body: None,
            visual: None,
            respawn: false,
            present: true,
        }
    }

    /// The common tick shown at `now_ms` (local virtual time), once a state arrived: the tick a
    /// striker saw.
    pub fn shown_tick(&self, now_ms: f64) -> Option<f64> {
        self.playback.tick_at(now_ms)
    }

    /// Where it is shown at `now_ms` and what it looks like then: interpolated between its
    /// states, carried on past the newest (bounded), its look from the state shown. Before any
    /// playback, its newest state.
    pub fn shown(&self, now_ms: f64, limits: &SampleLimits) -> Option<(KartPose, FootLook)> {
        match self.shown_tick(now_ms) {
            Some(tick) => {
                let pose = self.buffer.sample(tick, limits)?.pose;
                Some((pose, self.looks.at(tick).unwrap_or_default()))
            }
            None => {
                let pose = *self.buffer.newest()?;
                Some((
                    pose,
                    self.looks.at(f64::from(pose.tick)).unwrap_or_default(),
                ))
            }
        }
    }
}

/// A player of the room with a character in the lobby: see [`FootSync::set_walkers`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkerEntry<'a> {
    pub peer: PeerKey,
    pub character: usize,
    pub color: KartColor,
    pub name: &'a str,
}

/// What the host relays of the characters' states (issue #101, like the karts' of issue #63):
/// the newest state of each client's character since the last bundles, and each client gets one
/// packet with every other character (the host's, the other clients'). Nothing allocated once
/// warm.
#[derive(Debug, Default)]
pub struct FootRelay {
    fresh: Vec<(PeerKey, FootState)>,
    last_ms: Option<u64>,
}

impl FootRelay {
    /// A state of a client's character arrived: the newest goes in the next bundles.
    pub fn push(&mut self, origin: PeerKey, state: FootState) {
        if let Some(entry) = self.fresh.iter_mut().find(|(peer, _)| *peer == origin) {
            if state.tick >= entry.1.tick || entry.1.tick > state.tick + RESTART_TICKS {
                entry.1 = state;
            }
        } else if self.fresh.len() < MAX_PLAYERS {
            self.fresh.push((origin, state));
        }
    }

    /// Whether the bundles leave now (`now_ms`, real time): with our own state (`ours`), or once
    /// `interval_ms` went by with the clients' only.
    pub fn due(&self, now_ms: u64, ours: bool, interval_ms: u64) -> bool {
        ours || (!self.fresh.is_empty()
            && self.last_ms.is_none_or(|last| now_ms >= last + interval_ms))
    }

    /// The bundle for `client`: our state (`own`, as `me`), then the fresh states of the other
    /// clients; `None` when there is nothing for it.
    pub fn bundle_for(
        &self,
        client: PeerKey,
        me: PeerKey,
        own: Option<FootState>,
    ) -> Option<Message> {
        let mut states: Vec<(PeerKey, FootState)> = Vec::with_capacity(MAX_PLAYERS);
        states.extend(own.map(|state| (me, state)));
        states.extend(
            self.fresh
                .iter()
                .filter(|(origin, _)| *origin != client && *origin != me)
                .copied(),
        );
        states.truncate(MAX_PLAYERS);
        (!states.is_empty()).then_some(Message::FootStates(states))
    }

    /// The bundles left at `now_ms`: the next ones wait for new states.
    pub fn sent(&mut self, now_ms: u64) {
        self.fresh.clear();
        self.last_ms = Some(now_ms);
    }

    /// A client left: its states go no further.
    pub fn forget(&mut self, peer: PeerKey) {
        self.fresh.retain(|(origin, _)| *origin != peer);
    }

    /// The states waiting for the next bundles.
    pub fn waiting(&self) -> usize {
        self.fresh.len()
    }
}

/// The characters on foot of a peer in the lobby (see the module's documentation). There only
/// while the lobby's scene is up, in a room.
#[derive(Resource, Debug)]
pub struct FootSync {
    me: PeerKey,
    walkers: Vec<RemoteWalker>,
    /// Our character's states to send, newest last (a few at most: the socket takes them every
    /// frame).
    outgoing: Vec<FootState>,
    strikes_out: Vec<FootStrike>,
    strikes_in: Vec<(PeerKey, FootStrike)>,
    /// The host's bundles.
    pub relay: FootRelay,
    /// Bodies and visuals of characters gone, to despawn.
    gone: Vec<Entity>,
}

/// Our states waiting to be sent, at most.
const MAX_OUTGOING: usize = 8;

impl FootSync {
    pub fn new(me: PeerKey) -> Self {
        Self {
            me,
            walkers: Vec::with_capacity(MAX_PLAYERS),
            outgoing: Vec::with_capacity(MAX_OUTGOING),
            strikes_out: Vec::new(),
            strikes_in: Vec::new(),
            relay: FootRelay::default(),
            gone: Vec::new(),
        }
    }

    pub fn me(&self) -> PeerKey {
        self.me
    }

    pub fn walkers(&self) -> &[RemoteWalker] {
        &self.walkers
    }

    pub fn walkers_mut(&mut self) -> &mut [RemoteWalker] {
        &mut self.walkers
    }

    /// The character of `peer`.
    pub fn walker(&self, peer: PeerKey) -> Option<&RemoteWalker> {
        self.walkers.iter().find(|walker| walker.peer == peer)
    }

    /// The other players of the room (ours is left out): those not listed any more are gone
    /// (their character disappears); one whose animal changed is spawned again; its color and
    /// its pseudo change in place.
    pub fn set_walkers<'a>(&mut self, entries: impl IntoIterator<Item = WalkerEntry<'a>>) {
        for walker in &mut self.walkers {
            walker.present = false;
        }
        for entry in entries {
            if entry.peer == self.me {
                continue;
            }
            match self.walkers.iter_mut().find(|w| w.peer == entry.peer) {
                Some(walker) => {
                    walker.present = true;
                    if walker.character != entry.character {
                        walker.character = entry.character;
                        walker.respawn = true;
                    }
                    walker.color = entry.color;
                    if walker.name != entry.name {
                        walker.name.clear();
                        walker.name.push_str(entry.name);
                    }
                }
                None => self.walkers.push(RemoteWalker::new(&entry)),
            }
        }
        let gone = &mut self.gone;
        self.walkers.retain(|walker| {
            if !walker.present {
                gone.extend(walker.body.iter().chain(&walker.visual));
            }
            walker.present
        });
    }

    /// The bodies and visuals of the characters gone, to despawn.
    pub fn take_gone(&mut self) -> std::vec::Drain<'_, Entity> {
        self.gone.drain(..)
    }

    /// A state of `from`'s character arrived at `now_ms` (local virtual time): into its buffer
    /// (a peer we do not know: dropped).
    pub fn receive(&mut self, from: PeerKey, state: FootState, now_ms: f64, tuning: &NetTuning) {
        let Some(walker) = self.walkers.iter_mut().find(|w| w.peer == from) else {
            return;
        };
        let pose = state.pose();
        walker.received += 1;
        if walker.buffer.insert(pose.kart_pose()) == Insert::Restarted {
            walker.playback = Playback::default();
        }
        walker.looks.insert(pose.tick, pose.look);
        walker
            .playback
            .arrived(now_ms, state.tick, f64::from(tuning.lag_window_ms.max(1.0)));
    }

    /// Once a frame, at `now_ms` (local virtual time), `dt_ms` after the last one: the
    /// playbacks move on.
    pub fn update(&mut self, now_ms: f64, dt_ms: f64, tuning: &NetTuning) {
        let settings = tuning.playback();
        for walker in &mut self.walkers {
            walker.playback.advance(now_ms, dt_ms, &settings);
        }
    }

    /// Our character's state, to send.
    pub fn send(&mut self, state: FootState) {
        if self.outgoing.len() >= MAX_OUTGOING {
            self.outgoing.remove(0);
        }
        self.outgoing.push(state);
    }

    /// Our newest state waiting (the host's bundles carry it), without taking it.
    pub fn newest_outgoing(&self) -> Option<FootState> {
        self.outgoing.last().copied()
    }

    /// Our states to send, oldest first.
    pub fn drain_outgoing(&mut self) -> std::vec::Drain<'_, FootState> {
        self.outgoing.drain(..)
    }

    /// A strike of our character's paw, to tell everyone.
    pub fn send_strike(&mut self, strike: FootStrike) {
        self.strikes_out.push(strike);
    }

    pub fn drain_strikes_out(&mut self) -> std::vec::Drain<'_, FootStrike> {
        self.strikes_out.drain(..)
    }

    /// A strike from `from`: kept if it is on our character (we decide), else nothing (its
    /// target's peer decides; we see it fall by its states).
    pub fn receive_strike(&mut self, from: PeerKey, strike: FootStrike) {
        if strike.target == self.me && from != self.me {
            self.strikes_in.push((from, strike));
        }
    }

    /// The strikes on our character, oldest first, with their striker.
    pub fn drain_strikes_in(&mut self) -> std::vec::Drain<'_, (PeerKey, FootStrike)> {
        self.strikes_in.drain(..)
    }
}

/// The direction of a strike on the wire (× 127), from a horizontal vector (none: forward).
pub fn strike_direction(direction: Vec3) -> [i8; 2] {
    let flat = Vec3::new(direction.x, 0.0, direction.z).normalize_or(Vec3::NEG_Z);
    let wire = [flat.x, flat.z].map(|v| (v * 127.0).round().clamp(-127.0, 127.0) as i8);
    if wire == [0, 0] { [0, -127] } else { wire }
}

/// The direction of a strike from the wire (horizontal, unit).
pub fn direction_of(wire: [i8; 2]) -> Vec3 {
    Vec3::new(f32::from(wire[0]), 0.0, f32::from(wire[1])).normalize_or(Vec3::NEG_Z)
}

/// A position on the wire (cm), and back.
pub fn foot_position(position: Vec3) -> [i16; 3] {
    position.to_array().map(|v| step_i16(v, FOOT_POSITION_STEP))
}

pub fn position_of(wire: [i16; 3]) -> Vec3 {
    Vec3::from_array(wire.map(|v| f32::from(v) * FOOT_POSITION_STEP))
}

/// Whether the victim takes a strike: no older than `max_age_ms` since its tick (the victim's
/// common tick now: `now_tick`), and the striker's position within `reach` of where the victim
/// was at the tick the striker saw (`seen_position`: from its history; `None`, too old to know:
/// refused).
pub fn strike_lands(
    strike: &FootStrike,
    now_tick: u32,
    seen_position: Option<Vec3>,
    reach: f32,
    max_age_ms: f32,
) -> bool {
    let age_ms = f64::from(now_tick.saturating_sub(strike.tick)) * TICK_MS;
    if age_ms > f64::from(max_age_ms) {
        return false;
    }
    let Some(seen) = seen_position else {
        return false;
    };
    let striker = position_of(strike.position);
    let flat = Vec3::new(seen.x - striker.x, 0.0, seen.z - striker.z);
    flat.length() <= reach && (seen.y - striker.y).abs() <= reach
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::interp::SampleKind;

    const ANA: PeerKey = PeerKey(1 << 100);
    const BOB: PeerKey = PeerKey(2 << 100);
    const CYD: PeerKey = PeerKey(3 << 100);

    fn pose(tick: u32, x: f32) -> FootPose {
        FootPose {
            tick,
            position: Vec3::new(x, 0.45, -2.0),
            rotation: Quat::from_rotation_y(0.4),
            velocity: Vec3::new(3.0, 0.0, 0.0),
            spin: Vec3::ZERO,
            look: FootLook::default(),
        }
    }

    fn entry(peer: PeerKey, character: usize) -> WalkerEntry<'static> {
        WalkerEntry {
            peer,
            character,
            color: KartColor::Red,
            name: "Bob",
        }
    }

    /// A state reads back the same within its steps, every look survives, and what is read
    /// writes back to the same bytes; garbage stays finite.
    #[test]
    fn a_state_on_foot_comes_back_the_same() {
        let looks = [
            FootLook::default(),
            FootLook {
                phase: RagdollPhase::Tumbling,
                phase_ticks: 37,
                airborne: true,
                paw: None,
            },
            FootLook {
                phase: RagdollPhase::GettingUp,
                phase_ticks: 400,
                airborne: false,
                paw: Some((5, true)),
            },
            FootLook {
                phase: RagdollPhase::Upright,
                phase_ticks: 0,
                airborne: false,
                paw: Some((300, false)),
            },
        ];
        for look in looks {
            let original = FootPose {
                tick: 4_000_000_000,
                position: Vec3::new(-12.345, 0.9, 30.006),
                rotation: (Quat::from_rotation_x(1.0) * Quat::from_rotation_y(-2.0)).normalize(),
                velocity: Vec3::new(4.2, -9.0, 0.003),
                spin: Vec3::new(9.0, -3.0, 0.5),
                look,
            };
            let state = FootState::quantize(&original);
            let back = state.pose();
            assert_eq!(back.tick, original.tick);
            assert!((back.position - original.position).abs().max_element() <= 0.005 + 1e-4);
            assert!(back.rotation.angle_between(original.rotation) < 1e-3);
            assert!((back.velocity - original.velocity).abs().max_element() <= 0.005 + 1e-4);
            assert!((back.spin - original.spin).abs().max_element() <= 0.0005 + 1e-5);
            assert_eq!(back.look.phase, look.phase);
            assert_eq!(back.look.airborne, look.airborne);
            assert_eq!(back.look.phase_ticks, look.phase_ticks.min(255));
            assert_eq!(
                back.look.paw,
                look.paw.map(|(t, right)| (t.min(255), right))
            );
            assert_eq!(FootState::quantize(&back), state, "{look:?}");
            let bytes = Message::FootState(state).to_bytes();
            assert_eq!(Message::decode(&bytes), Ok(Message::FootState(state)));
        }
        // Out of range: clamped, never wrapped; NaN: zero.
        let far = FootState::quantize(&FootPose {
            position: Vec3::new(1e6, f32::NAN, -1e6),
            ..pose(1, 0.0)
        });
        assert_eq!(far.position, [32_767, 0, -32_767]);
        assert!(far.pose().position.is_finite());
    }

    /// The look shown is the newest state's at or before the tick shown, its counters run on;
    /// in any order of arrival.
    #[test]
    fn the_look_follows_the_state_shown() {
        let mut track = LookTrack::default();
        assert_eq!(track.at(10.0), None);
        let down = FootLook {
            phase: RagdollPhase::Tumbling,
            phase_ticks: 4,
            ..default()
        };
        track.insert(20, down);
        track.insert(10, FootLook::default());
        track.insert(20, FootLook::default());
        assert_eq!(track.len(), 2, "a copy is ignored");
        assert_eq!(track.at(5.0), Some(FootLook::default()), "the oldest");
        assert_eq!(
            track.at(15.5),
            Some(FootLook {
                phase_ticks: 5,
                ..default()
            }),
            "tick 10's, its counter run on"
        );
        assert_eq!(track.at(20.0), Some(down));
        let later = track.at(27.9).unwrap();
        assert_eq!(later.phase, RagdollPhase::Tumbling);
        assert_eq!(later.phase_ticks, 4 + 7);
        // Full: the oldest go, and an old state cannot come in.
        for tick in 0..100 {
            track.insert(100 + tick, FootLook::default());
        }
        assert_eq!(track.len(), BUFFER_CAPACITY);
        // (Tick 150: older than the oldest kept, 168, but not a clock gone back.)
        track.insert(150, down);
        assert_eq!(track.len(), BUFFER_CAPACITY);
        assert!(
            track
                .at(150.0)
                .is_some_and(|look| look.phase == RagdollPhase::Upright)
        );
        // A clock that jumps back: the track starts over.
        track.insert(10, down);
        assert_eq!(track.len(), 1);
        assert_eq!(track.at(50.0).unwrap().phase_ticks, 4 + 40);
    }

    /// Issue #101: the other players' characters, by peer: ours is left out; one gone
    /// disappears (its entities to despawn), one with another animal is spawned again, one whose
    /// color or pseudo changed keeps its entities; states from a peer we do not know are
    /// dropped.
    #[test]
    fn the_walkers_follow_the_room() {
        let mut sync = FootSync::new(ANA);
        sync.set_walkers([entry(ANA, 0), entry(BOB, 3), entry(CYD, 1)]);
        assert_eq!(sync.walkers().len(), 2);
        assert!(sync.walker(ANA).is_none());
        let (body, visual) = (
            Entity::from_raw_u32(5).unwrap(),
            Entity::from_raw_u32(6).unwrap(),
        );
        sync.walkers_mut()[1].body = Some(body);
        sync.walkers_mut()[1].visual = Some(visual);
        let tuning = NetTuning::default();
        sync.receive(BOB, FootState::quantize(&pose(10, 1.0)), 100.0, &tuning);
        sync.receive(
            PeerKey(9),
            FootState::quantize(&pose(10, 1.0)),
            100.0,
            &tuning,
        );
        assert_eq!(sync.walker(BOB).unwrap().received, 1);
        let renamed = WalkerEntry {
            name: "Bobby",
            color: KartColor::SkyBlue,
            ..entry(BOB, 3)
        };
        sync.set_walkers([entry(ANA, 0), renamed, entry(CYD, 2)]);
        let bob = sync.walker(BOB).unwrap();
        assert_eq!(
            (bob.name.as_str(), bob.color),
            ("Bobby", KartColor::SkyBlue)
        );
        assert!(!bob.respawn);
        assert!(sync.walker(CYD).unwrap().respawn, "another animal");
        sync.set_walkers([entry(BOB, 3)]);
        assert_eq!(sync.walkers().len(), 1);
        let gone: Vec<Entity> = sync.take_gone().collect();
        assert_eq!(gone, [body, visual]);
    }

    /// A remote character is shown between its states, about the delay after they arrive, with
    /// the look of the state shown; the tumble goes on past its newest state.
    #[test]
    fn a_remote_character_is_shown_between_its_states() {
        let mut sync = FootSync::new(ANA);
        sync.set_walkers([entry(BOB, 0)]);
        let tuning = NetTuning::default();
        let limits = tuning.limits();
        // States every 2 ticks, arriving 50 ms after their tick.
        let mut now = 0.0;
        for i in 0..20u32 {
            let tick = 100 + 2 * i;
            let mut p = pose(tick, i as f32 * 0.1);
            if i >= 10 {
                p.look.phase = RagdollPhase::Tumbling;
                p.look.phase_ticks = 2 * (i - 10);
            }
            now = f64::from(tick) * TICK_MS + 50.0;
            sync.receive(BOB, FootState::quantize(&p), now, &tuning);
            sync.update(now, 2.0 * TICK_MS, &tuning);
        }
        let bob = sync.walker(BOB).unwrap();
        let shown = bob.shown_tick(now).unwrap();
        // 50 ms of lag and the delay behind the newest arrival.
        let behind_ms = (f64::from(138u32) - shown) * TICK_MS + 50.0;
        assert!(
            (behind_ms - 50.0 - f64::from(tuning.interpolation_delay_ms)).abs() < 20.0,
            "{behind_ms}"
        );
        let (pose_shown, look) = bob.shown(now, &limits).unwrap();
        let sample = bob.buffer.sample(shown, &limits).unwrap();
        assert_eq!(sample.kind, SampleKind::Interpolated);
        assert!((pose_shown.position - sample.pose.position).length() < 1e-6);
        assert_eq!(look.phase, RagdollPhase::Tumbling);
        // Long after: carried on, then frozen; the look's counters run on.
        let (late, look) = bob.shown(now + 5_000.0, &limits).unwrap();
        assert!(late.position.is_finite());
        assert!(look.phase_ticks > 20);
    }

    /// The host's bundles: each client gets every other character (the host's own first), never
    /// its own; nothing when there is nothing for it; the newest state of a client only.
    #[test]
    fn the_host_bundles_the_characters() {
        const HOST: PeerKey = PeerKey(7 << 100);
        let mut relay = FootRelay::default();
        assert!(!relay.due(0, false, 33));
        relay.push(ANA, FootState::quantize(&pose(10, 1.0)));
        relay.push(ANA, FootState::quantize(&pose(8, 0.0)));
        relay.push(BOB, FootState::quantize(&pose(11, 2.0)));
        assert_eq!(relay.waiting(), 2);
        assert!(relay.due(0, false, 33));
        let own = FootState::quantize(&pose(12, 3.0));
        let Some(Message::FootStates(to_ana)) = relay.bundle_for(ANA, HOST, Some(own)) else {
            panic!("a bundle for Ana");
        };
        assert_eq!(
            to_ana.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
            [HOST, BOB]
        );
        let Some(Message::FootStates(to_bob)) = relay.bundle_for(BOB, HOST, None) else {
            panic!("a bundle for Bob");
        };
        assert_eq!(to_bob.len(), 1);
        assert_eq!(to_bob[0].1.tick, 10, "Ana's newest");
        relay.sent(0);
        assert!(!relay.due(10, false, 33), "nothing new");
        assert!(relay.bundle_for(ANA, HOST, None).is_none());
        relay.push(BOB, FootState::quantize(&pose(13, 2.0)));
        assert!(!relay.due(20, false, 33), "the interval");
        assert!(relay.due(33, false, 33));
        relay.forget(BOB);
        assert_eq!(relay.waiting(), 0);
    }

    /// Issue #101: the victim decides: a strike in reach of where it was at the tick seen, and
    /// young enough, lands; too far, too old, or a tick it no longer knows, it does not. Only
    /// the strikes on our character are kept.
    #[test]
    fn the_victim_decides_a_strike_on_foot() {
        let strike = FootStrike {
            target: ANA,
            tick: 100,
            seen: 94,
            position: foot_position(Vec3::new(1.0, 0.45, 0.0)),
            direction: strike_direction(Vec3::new(-1.0, 0.3, 0.0)),
        };
        assert_eq!(direction_of(strike.direction), Vec3::NEG_X);
        let at = |x: f32| Some(Vec3::new(x, 0.45, 0.0));
        assert!(strike_lands(&strike, 104, at(-0.6), 2.4, 300.0));
        assert!(!strike_lands(&strike, 104, at(-1.6), 2.4, 300.0), "too far");
        assert!(
            !strike_lands(&strike, 100 + 19, at(0.0), 2.4, 300.0),
            "too old"
        );
        assert!(strike_lands(&strike, 100 + 17, at(0.0), 2.4, 300.0));
        assert!(
            !strike_lands(&strike, 104, None, 2.4, 300.0),
            "unknown tick"
        );
        let high = Some(Vec3::new(0.5, 4.0, 0.0));
        assert!(
            !strike_lands(&strike, 104, high, 2.4, 300.0),
            "up on a bouncer"
        );
        assert_eq!(
            strike_direction(Vec3::Y),
            [0, -127],
            "no direction: forward"
        );
        let mut sync = FootSync::new(ANA);
        sync.receive_strike(BOB, strike);
        sync.receive_strike(
            BOB,
            FootStrike {
                target: CYD,
                ..strike
            },
        );
        sync.receive_strike(ANA, strike);
        let kept: Vec<(PeerKey, FootStrike)> = sync.drain_strikes_in().collect();
        assert_eq!(kept, [(BOB, strike)]);
        let back = position_of(foot_position(Vec3::new(1.234, -0.5, 9.99)));
        assert!(
            (back - Vec3::new(1.23, -0.5, 9.99)).length() < 1e-4,
            "{back}"
        );
    }

    fn default<T: Default>() -> T {
        T::default()
    }
}
