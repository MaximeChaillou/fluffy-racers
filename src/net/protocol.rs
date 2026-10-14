//! The wire protocol between peers (issue #7, `docs/ARCHITECTURE.md` § « Réseau »). Pure logic,
//! no Bevy and no socket: unit-tested.
//!
//! **Format**: one message per packet, little-endian, no padding.
//!
//! | byte | content |
//! |---|---|
//! | 0 | [`PROTOCOL_VERSION`]: a peer of another version is shown as such, never misread |
//! | 1 | the kind of message ([`Message::kind`]) |
//! | 2… | its fields, in order |
//!
//! Strings are a length byte then UTF-8; lists a count byte then their items; booleans one byte,
//! 0 or 1. Decoding never panics: a packet that is empty, truncated, too long, of an unknown kind,
//! of another version, or with an impossible value (a boolean of 2, a float that is not finite,
//! more seats than players) is refused with its [`DecodeError`].
//!
//! **Channels** ([`Channel`]): the lobby and the events go on the reliable channel; the pings and
//! the kart states (issue #8, [`KartSnapshot`], 30 per second) on the unreliable one: a
//! retransmitted ping would measure the retransmission, and a lost kart state is replaced by the
//! next one 33 ms later.
//!
//! Version 2 (issue #8): the kart states.

use std::fmt;

use crate::interactions::TetherEnd;

/// Version of this protocol: first byte of every packet. Bump it whenever a message changes.
pub const PROTOCOL_VERSION: u8 = 2;

/// Players in a room at most (GD § 1: 12 racers).
pub const MAX_PLAYERS: usize = 12;

/// A player's name on the wire: 16 characters at most (`Profile::clean_name`), so at most 64
/// bytes of UTF-8.
pub const MAX_NAME_CHARS: usize = 16;
pub const MAX_NAME_BYTES: usize = 64;

/// A peer of the room: the `PeerId` (a UUID) the signaling server gave it, as a number. The host
/// is the smallest one (`docs/ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerKey(pub u128);

impl PeerKey {
    /// A short tag to tell two players apart on screen (the first 4 hexadecimal digits).
    pub fn tag(self) -> String {
        format!("{:04x}", (self.0 >> 112) as u16)
    }
}

/// The two data channels of the socket, in the order they are created.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Ordered, retransmitted: lobby and events.
    Reliable,
    /// Unordered, never retransmitted: pings and kart states (issue #8).
    Unreliable,
}

impl Channel {
    pub const ALL: [Self; 2] = [Channel::Reliable, Channel::Unreliable];

    /// Index of the channel in the socket.
    pub const fn index(self) -> usize {
        match self {
            Channel::Reliable => 0,
            Channel::Unreliable => 1,
        }
    }
}

/// Who a player is, sent to every peer on connection and when it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    /// Name shown in the lobby ([`Profile::clean_name`]).
    pub name: String,
    /// Indices of the character, kart, wheels and wing (issue #15, `characters::Loadout`): every
    /// peer knows the stats of every kart from the lobby on.
    pub loadout: [u8; 4],
    /// « Prêt ».
    pub ready: bool,
    /// The player has a seat in the room (see `lobby`): newcomers wait for the host's.
    pub seated: bool,
}

impl Profile {
    /// A name fit for the wire and the screen: no control character, no space at either end, at
    /// most [`MAX_NAME_CHARS`] characters.
    pub fn clean_name(name: &str) -> String {
        name.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(MAX_NAME_CHARS)
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// The host starts a race (issue #9 will send it): everything a peer needs to build the same
/// race, and when it starts on the host's clock (`lobby::Lobby::host_time_ms`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RaceStart {
    /// `RaceGrid::seed` (issue #40): every peer draws the same bots.
    pub seed: u64,
    /// Index of the circuit in `assets/tracks/index.ron`.
    pub track: u8,
    pub laps: u8,
    /// Bots that complete the grid.
    pub bots: u8,
    /// When the countdown starts, in ms on the host's clock.
    pub countdown_at_ms: u32,
}

/// A kart on the network: its slot on the grid (issue #8 maps it to the kart's entity).
pub type KartId = u8;

/// `Coup` (`interactions::KartStruck`, issue #34): the striker tells the victim, which decides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub striker: KartId,
    pub target: KartId,
    pub tick: u32,
    /// Tick of the target's state the striker saw (`tick_vu`).
    pub seen: u32,
    pub striker_position: [f32; 3],
    /// Horizontal direction of the push, unit (x, z of the world).
    pub direction: [f32; 2],
    /// Push speed, the Weights and a throw's factor applied (m/s).
    pub push: f32,
    pub throw: bool,
}

/// `CoupReçu` / `CoupIgnoré` (`interactions::StrikeAnswer`): the victim's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrikeReply {
    pub striker: KartId,
    pub target: KartId,
    pub landed: bool,
    pub throw: bool,
}

/// `Attrape` (`interactions::KartGrabbed`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grab {
    pub grabber: KartId,
    pub target: KartId,
    pub tick: u32,
    pub seen: u32,
    pub grabber_position: [f32; 3],
    /// Rest length of the tether (m).
    pub rest: f32,
}

/// `FinAttrape` (`interactions::TetherEnded`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TetherBreak {
    pub grabber: KartId,
    pub target: KartId,
    pub reason: TetherEnd,
}

/// A kart's state at a tick (issue #8, `docs/ARCHITECTURE.md` § « Synchronisation »): the peer
/// that has authority over the kart sends it 30 times a second, on the unreliable channel.
/// Quantized by `net::snapshot` (`KartSnapshot::quantize`, `KartSnapshot::pose`): every field is
/// the integer that travels, so that what is read writes back to the same bytes. 47 bytes with
/// the version and the kind (fewer than [`MAX_KART_STATE_BYTES`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KartSnapshot {
    /// Tick of the room's common clock (60 per second, on the host's clock: `net::TickClock`).
    pub tick: u32,
    /// The kart: its grid slot (the same id as the interactions' [`KartId`]).
    pub kart: KartId,
    /// Position (mm).
    pub position: [i32; 3],
    /// Rotation, « smallest three » on 48 bits (`snapshot::pack_rotation`); the last bit is 0.
    pub rotation: [u8; 6],
    /// Linear velocity (cm/s).
    pub velocity: [i16; 3],
    /// Angular velocity (mrad/s).
    pub spin: [i16; 3],
    /// Bits 0-2: the phase (grounded, hop, drift, airborne, trick, hit, towed); 3: drifting to
    /// the right; 4-5: the sparks' tier (0: none); 6: tumbling (hit); 7: 0.
    pub state: u8,
    /// Bit 0: stalled engine; 1: invincible; the others are 0.
    pub flags: u8,
    /// Hit by an item: ticks since the hit, and its duration (ticks).
    pub hit: [u8; 2],
    /// Boost: strength (1/200: 60 = +30 %) and time left (ticks).
    pub boost: [u8; 2],
    /// The drift's slip angle (5 mrad).
    pub slip: i8,
    /// The slipstream gauge while it charges (1 to 255), 0 otherwise.
    pub slipstream: u8,
    /// The 4 wheels, 4 bits each (front left first): 0 in the air, 1 to 15 on the ground with a
    /// spring compressed by 0 to 0.7 m.
    pub wheels: u16,
}

/// A kart state fits in a small unreliable packet (issue #8: fewer than 64 bytes).
pub const MAX_KART_STATE_BYTES: usize = 64;

/// Every message of the protocol.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Who I am (on connection, and when my name, loadout or seat changes).
    Hello(Profile),
    /// « Prêt » or not.
    Ready(bool),
    /// The host's list of seated players, in seat order (arrival order).
    Seats(Vec<PeerKey>),
    /// The host refuses a newcomer: the room already has [`MAX_PLAYERS`] players.
    RoomFull,
    /// I leave the room (a closed tab says nothing: the signaling server tells the others).
    Bye,
    /// The host starts the race (issue #9).
    StartRace(RaceStart),
    Strike(Strike),
    StrikeReply(StrikeReply),
    Grab(Grab),
    TetherBreak(TetherBreak),
    /// Latency and clock: `sent_ms` is the sender's clock, echoed back.
    Ping {
        sent_ms: u32,
    },
    /// The answer: the ping's time, and the clock of the peer that answers.
    Pong {
        echo_ms: u32,
        clock_ms: u32,
    },
    /// The state of a kart of the sender (issue #8), 30 times a second.
    KartState(KartSnapshot),
}

const HELLO: u8 = 1;
const READY: u8 = 2;
const SEATS: u8 = 3;
const ROOM_FULL: u8 = 4;
const BYE: u8 = 5;
const START_RACE: u8 = 6;
const STRIKE: u8 = 16;
const STRIKE_REPLY: u8 = 17;
const GRAB: u8 = 18;
const TETHER_BREAK: u8 = 19;
const PING: u8 = 32;
const PONG: u8 = 33;
const KART_STATE: u8 = 40;

/// The reasons a tether ends, in their wire order.
const TETHER_ENDS: [TetherEnd; 10] = [
    TetherEnd::Released,
    TetherEnd::Thrown,
    TetherEnd::Escaped,
    TetherEnd::TimeUp,
    TetherEnd::Stretched,
    TetherEnd::Hit,
    TetherEnd::Towed,
    TetherEnd::Finished,
    TetherEnd::Refused,
    TetherEnd::Gone,
];

impl Message {
    /// Its kind: the second byte of the packet.
    pub fn kind(&self) -> u8 {
        match self {
            Message::Hello(_) => HELLO,
            Message::Ready(_) => READY,
            Message::Seats(_) => SEATS,
            Message::RoomFull => ROOM_FULL,
            Message::Bye => BYE,
            Message::StartRace(_) => START_RACE,
            Message::Strike(_) => STRIKE,
            Message::StrikeReply(_) => STRIKE_REPLY,
            Message::Grab(_) => GRAB,
            Message::TetherBreak(_) => TETHER_BREAK,
            Message::Ping { .. } => PING,
            Message::Pong { .. } => PONG,
            Message::KartState(_) => KART_STATE,
        }
    }

    /// The channel it goes on.
    pub fn channel(&self) -> Channel {
        match self {
            Message::Ping { .. } | Message::Pong { .. } | Message::KartState(_) => {
                Channel::Unreliable
            }
            _ => Channel::Reliable,
        }
    }

    /// Writes the packet into `out` (emptied first: a buffer reused for every message).
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.clear();
        out.push(PROTOCOL_VERSION);
        out.push(self.kind());
        match self {
            Message::Hello(profile) => {
                put_name(out, &profile.name);
                out.extend_from_slice(&profile.loadout);
                out.push(u8::from(profile.ready) | u8::from(profile.seated) << 1);
            }
            Message::Ready(ready) => out.push(u8::from(*ready)),
            Message::Seats(seats) => {
                let count = seats.len().min(MAX_PLAYERS);
                out.push(count as u8);
                for seat in &seats[..count] {
                    out.extend_from_slice(&seat.0.to_le_bytes());
                }
            }
            Message::RoomFull | Message::Bye => {}
            Message::StartRace(start) => {
                out.extend_from_slice(&start.seed.to_le_bytes());
                out.extend_from_slice(&[start.track, start.laps, start.bots]);
                out.extend_from_slice(&start.countdown_at_ms.to_le_bytes());
            }
            Message::Strike(strike) => {
                out.extend_from_slice(&[strike.striker, strike.target]);
                put_u32(out, strike.tick);
                put_u32(out, strike.seen);
                put_floats(out, &strike.striker_position);
                put_floats(out, &strike.direction);
                put_floats(out, &[strike.push]);
                out.push(u8::from(strike.throw));
            }
            Message::StrikeReply(reply) => {
                out.extend_from_slice(&[reply.striker, reply.target]);
                out.push(u8::from(reply.landed) | u8::from(reply.throw) << 1);
            }
            Message::Grab(grab) => {
                out.extend_from_slice(&[grab.grabber, grab.target]);
                put_u32(out, grab.tick);
                put_u32(out, grab.seen);
                put_floats(out, &grab.grabber_position);
                put_floats(out, &[grab.rest]);
            }
            Message::TetherBreak(end) => {
                let reason = TETHER_ENDS.iter().position(|r| *r == end.reason);
                out.extend_from_slice(&[end.grabber, end.target, reason.unwrap_or(0) as u8]);
            }
            Message::Ping { sent_ms } => put_u32(out, *sent_ms),
            Message::Pong { echo_ms, clock_ms } => {
                put_u32(out, *echo_ms);
                put_u32(out, *clock_ms);
            }
            Message::KartState(kart) => {
                put_u32(out, kart.tick);
                out.push(kart.kart);
                for value in kart.position {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.extend_from_slice(&kart.rotation);
                for value in kart.velocity.iter().chain(&kart.spin) {
                    out.extend_from_slice(&value.to_le_bytes());
                }
                out.extend_from_slice(&[kart.state, kart.flags]);
                out.extend_from_slice(&kart.hit);
                out.extend_from_slice(&kart.boost);
                out.extend_from_slice(&kart.slip.to_le_bytes());
                out.push(kart.slipstream);
                out.extend_from_slice(&kart.wheels.to_le_bytes());
            }
        }
    }

    /// The packet, in a buffer of its own.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode(&mut out);
        out
    }

    /// Reads a packet. Never panics, whatever the bytes (see the module's documentation).
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (&version, rest) = bytes.split_first().ok_or(DecodeError::Empty)?;
        if version != PROTOCOL_VERSION {
            return Err(DecodeError::OtherVersion(version));
        }
        let (&kind, rest) = rest.split_first().ok_or(DecodeError::Truncated)?;
        let mut reader = Reader(rest);
        let message = match kind {
            HELLO => {
                let name = reader.name()?;
                let loadout = reader.array::<4>()?;
                let flags = reader.u8()?;
                if flags > 0b11 {
                    return Err(DecodeError::Invalid("profile flags"));
                }
                Message::Hello(Profile {
                    name,
                    loadout,
                    ready: flags & 1 != 0,
                    seated: flags & 2 != 0,
                })
            }
            READY => Message::Ready(reader.bool()?),
            SEATS => {
                let count = usize::from(reader.u8()?);
                if count > MAX_PLAYERS {
                    return Err(DecodeError::Invalid("more seats than players"));
                }
                let mut seats = Vec::with_capacity(count);
                for _ in 0..count {
                    seats.push(PeerKey(u128::from_le_bytes(reader.array()?)));
                }
                Message::Seats(seats)
            }
            ROOM_FULL => Message::RoomFull,
            BYE => Message::Bye,
            START_RACE => Message::StartRace(RaceStart {
                seed: u64::from_le_bytes(reader.array()?),
                track: reader.u8()?,
                laps: reader.u8()?,
                bots: reader.u8()?,
                countdown_at_ms: reader.u32()?,
            }),
            STRIKE => Message::Strike(Strike {
                striker: reader.u8()?,
                target: reader.u8()?,
                tick: reader.u32()?,
                seen: reader.u32()?,
                striker_position: reader.floats()?,
                direction: reader.floats()?,
                push: reader.floats::<1>()?[0],
                throw: reader.bool()?,
            }),
            STRIKE_REPLY => {
                let striker = reader.u8()?;
                let target = reader.u8()?;
                let flags = reader.u8()?;
                if flags > 0b11 {
                    return Err(DecodeError::Invalid("strike reply flags"));
                }
                Message::StrikeReply(StrikeReply {
                    striker,
                    target,
                    landed: flags & 1 != 0,
                    throw: flags & 2 != 0,
                })
            }
            GRAB => Message::Grab(Grab {
                grabber: reader.u8()?,
                target: reader.u8()?,
                tick: reader.u32()?,
                seen: reader.u32()?,
                grabber_position: reader.floats()?,
                rest: reader.floats::<1>()?[0],
            }),
            TETHER_BREAK => Message::TetherBreak(TetherBreak {
                grabber: reader.u8()?,
                target: reader.u8()?,
                reason: *TETHER_ENDS
                    .get(usize::from(reader.u8()?))
                    .ok_or(DecodeError::Invalid("tether end"))?,
            }),
            PING => Message::Ping {
                sent_ms: reader.u32()?,
            },
            PONG => Message::Pong {
                echo_ms: reader.u32()?,
                clock_ms: reader.u32()?,
            },
            KART_STATE => {
                let kart = KartSnapshot {
                    tick: reader.u32()?,
                    kart: reader.u8()?,
                    position: [reader.i32()?, reader.i32()?, reader.i32()?],
                    rotation: reader.array()?,
                    velocity: [reader.i16()?, reader.i16()?, reader.i16()?],
                    spin: [reader.i16()?, reader.i16()?, reader.i16()?],
                    state: reader.u8()?,
                    flags: reader.u8()?,
                    hit: reader.array()?,
                    boost: reader.array()?,
                    slip: i8::from_le_bytes(reader.array()?),
                    slipstream: reader.u8()?,
                    wheels: u16::from_le_bytes(reader.array()?),
                };
                if kart.rotation[5] & 0x80 != 0 {
                    return Err(DecodeError::Invalid("kart rotation"));
                }
                // Phase 7 does not exist; bit 7 is free.
                if kart.state & 0x07 == 0x07 || kart.state & 0x80 != 0 {
                    return Err(DecodeError::Invalid("kart state"));
                }
                if kart.flags > 0b11 {
                    return Err(DecodeError::Invalid("kart flags"));
                }
                Message::KartState(kart)
            }
            other => return Err(DecodeError::UnknownKind(other)),
        };
        if !reader.0.is_empty() {
            return Err(DecodeError::TrailingBytes);
        }
        Ok(message)
    }
}

/// Why a packet was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    /// The first byte: the peer runs another version of the game.
    OtherVersion(u8),
    UnknownKind(u8),
    /// The packet stops before the end of its message.
    Truncated,
    /// The message ends before the end of the packet.
    TrailingBytes,
    /// A value no message can have.
    Invalid(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Empty => write!(f, "empty packet"),
            DecodeError::OtherVersion(version) => {
                write!(f, "protocol version {version} (ours: {PROTOCOL_VERSION})")
            }
            DecodeError::UnknownKind(kind) => write!(f, "unknown message kind {kind}"),
            DecodeError::Truncated => write!(f, "truncated packet"),
            DecodeError::TrailingBytes => write!(f, "bytes after the message"),
            DecodeError::Invalid(what) => write!(f, "invalid {what}"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    // A name longer than the wire allows is cut at a character boundary.
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    out.push(end as u8);
    out.extend_from_slice(&name.as_bytes()[..end]);
}

/// Reads a packet's fields, front to back; every read checks the length.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(DecodeError::Truncated)?;
        self.0 = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid("boolean")),
        }
    }

    /// `N` floats, all finite: a NaN would spread through the physics.
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], DecodeError> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = f32::from_le_bytes(self.array()?);
            if !value.is_finite() {
                return Err(DecodeError::Invalid("float"));
            }
        }
        Ok(values)
    }

    fn name(&mut self) -> Result<String, DecodeError> {
        let length = usize::from(self.u8()?);
        if length > MAX_NAME_BYTES {
            return Err(DecodeError::Invalid("name length"));
        }
        let bytes = self.0.get(..length).ok_or(DecodeError::Truncated)?;
        self.0 = &self.0[length..];
        let name = std::str::from_utf8(bytes).map_err(|_| DecodeError::Invalid("name"))?;
        Ok(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of every message, with values that use every byte of their fields.
    fn every_message() -> Vec<Message> {
        vec![
            Message::Hello(Profile {
                name: "Max".into(),
                loadout: [3, 0, 2, 1],
                ready: false,
                seated: true,
            }),
            Message::Hello(Profile {
                name: "Éloïse la très rapide"
                    .chars()
                    .take(MAX_NAME_CHARS)
                    .collect(),
                loadout: [8, 2, 2, 1],
                ready: true,
                seated: false,
            }),
            Message::Hello(Profile::default()),
            Message::Ready(true),
            Message::Ready(false),
            Message::Seats(vec![]),
            Message::Seats(
                (0..MAX_PLAYERS as u128)
                    .map(|i| PeerKey(u128::MAX - i * 0x0123_4567_89ab_cdef))
                    .collect(),
            ),
            Message::RoomFull,
            Message::Bye,
            Message::StartRace(RaceStart {
                seed: 0xdead_beef_0123_4567,
                track: 1,
                laps: 3,
                bots: 10,
                countdown_at_ms: 123_456_789,
            }),
            Message::Strike(Strike {
                striker: 0,
                target: 11,
                tick: 4_000_000_000,
                seen: 3_999_999_994,
                striker_position: [-12.5, 0.75, 301.25],
                direction: [0.6, -0.8],
                push: 9.1,
                throw: true,
            }),
            Message::StrikeReply(StrikeReply {
                striker: 5,
                target: 7,
                landed: true,
                throw: false,
            }),
            Message::StrikeReply(StrikeReply {
                striker: 7,
                target: 5,
                landed: false,
                throw: true,
            }),
            Message::Grab(Grab {
                grabber: 2,
                target: 3,
                tick: 600,
                seen: 594,
                grabber_position: [1.0, 2.0, -3.0],
                rest: 2.75,
            }),
            Message::Ping { sent_ms: u32::MAX },
            Message::Pong {
                echo_ms: 17,
                clock_ms: 1_000_000,
            },
            Message::KartState(KartSnapshot::default()),
            Message::KartState(KartSnapshot {
                tick: 4_000_000_123,
                kart: 11,
                position: [i32::MIN, -1, i32::MAX],
                rotation: [0xff, 0x00, 0x12, 0x34, 0x56, 0x7f],
                velocity: [i16::MIN, 0, i16::MAX],
                spin: [-1, 1_234, -32_000],
                state: 0b0111_1110,
                flags: 0b11,
                hit: [17, 96],
                boost: [60, 255],
                slip: -128,
                slipstream: 255,
                wheels: 0xfedc,
            }),
        ]
        .into_iter()
        .chain(TETHER_ENDS.iter().map(|reason| {
            Message::TetherBreak(TetherBreak {
                grabber: 9,
                target: 4,
                reason: *reason,
            })
        }))
        .collect()
    }

    /// Issue #7 acceptance: every message comes back the same from its bytes.
    #[test]
    fn every_message_survives_a_round_trip() {
        let mut kinds = Vec::new();
        let mut buffer = Vec::new();
        for message in every_message() {
            message.encode(&mut buffer);
            assert_eq!(buffer[0], PROTOCOL_VERSION);
            assert_eq!(buffer[1], message.kind());
            assert_eq!(Message::decode(&buffer), Ok(message.clone()), "{buffer:?}");
            assert_eq!(message.to_bytes(), buffer);
            kinds.push(message.kind());
        }
        // Every kind is tested, and no two messages share one.
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), 13);
        assert_eq!(
            TETHER_ENDS.len(),
            10,
            "a TetherEnd was added: give it a wire byte"
        );
    }

    /// The pings and the kart states (issue #8) go on the unreliable channel, everything else on
    /// the reliable one.
    #[test]
    fn only_the_pings_and_the_kart_states_are_unreliable() {
        for message in every_message() {
            let unreliable = matches!(
                message,
                Message::Ping { .. } | Message::Pong { .. } | Message::KartState(_)
            );
            assert_eq!(
                message.channel() == Channel::Unreliable,
                unreliable,
                "{message:?}"
            );
        }
        assert_eq!(Channel::Reliable.index(), 0);
        assert_eq!(Channel::Unreliable.index(), 1);
    }

    /// Compact: the lobby fits in a few dozen bytes, a ping in 6.
    #[test]
    fn messages_are_compact() {
        assert_eq!(Message::Ping { sent_ms: 1 }.to_bytes().len(), 6);
        assert_eq!(Message::Ready(true).to_bytes().len(), 3);
        let hello = Message::Hello(Profile {
            name: "Max".into(),
            ..Default::default()
        });
        assert_eq!(hello.to_bytes().len(), 2 + 1 + 3 + 4 + 1);
        let seats = Message::Seats(vec![PeerKey(1); MAX_PLAYERS]);
        assert_eq!(seats.to_bytes().len(), 3 + 16 * MAX_PLAYERS);
        // Issue #8: a kart state in fewer than 64 bytes, whatever its values.
        for message in every_message() {
            if let Message::KartState(_) = message {
                let length = message.to_bytes().len();
                assert_eq!(length, 47);
                assert!(length < MAX_KART_STATE_BYTES);
            }
        }
        for message in every_message() {
            assert!(message.to_bytes().len() <= 200, "{message:?}");
        }
    }

    /// Every packet cut short is refused, never misread, never a panic.
    #[test]
    fn a_truncated_packet_is_refused() {
        for message in every_message() {
            let bytes = message.to_bytes();
            for end in 0..bytes.len() {
                let expected = if end == 0 {
                    DecodeError::Empty
                } else {
                    DecodeError::Truncated
                };
                assert_eq!(
                    Message::decode(&bytes[..end]),
                    Err(expected),
                    "{message:?} cut at {end}"
                );
            }
        }
    }

    #[test]
    fn bytes_after_the_message_are_refused() {
        for message in every_message() {
            let mut bytes = message.to_bytes();
            bytes.push(0);
            assert_eq!(Message::decode(&bytes), Err(DecodeError::TrailingBytes));
        }
    }

    #[test]
    fn another_version_or_an_unknown_kind_is_refused() {
        let mut bytes = Message::Bye.to_bytes();
        for version in [0, PROTOCOL_VERSION + 1, 255] {
            bytes[0] = version;
            assert_eq!(
                Message::decode(&bytes),
                Err(DecodeError::OtherVersion(version))
            );
        }
        // A future version is recognised from its first byte alone.
        assert_eq!(
            Message::decode(&[PROTOCOL_VERSION + 1]),
            Err(DecodeError::OtherVersion(PROTOCOL_VERSION + 1))
        );
        let known: Vec<u8> = every_message().iter().map(Message::kind).collect();
        for kind in 0..=255u8 {
            if !known.contains(&kind) {
                assert_eq!(
                    Message::decode(&[PROTOCOL_VERSION, kind]),
                    Err(DecodeError::UnknownKind(kind))
                );
            }
        }
        assert!(DecodeError::OtherVersion(9).to_string().contains('9'));
    }

    #[test]
    fn impossible_values_are_refused() {
        let v = PROTOCOL_VERSION;
        let invalid = |bytes: &[u8]| matches!(Message::decode(bytes), Err(DecodeError::Invalid(_)));
        // A boolean of 2.
        assert!(invalid(&[v, READY, 2]));
        // Unknown profile flags.
        assert!(invalid(&[v, HELLO, 0, 0, 0, 0, 0, 0b100]));
        assert!(invalid(&[v, STRIKE_REPLY, 0, 0, 0b100]));
        // A name too long, a name that is not UTF-8.
        let mut long = vec![v, HELLO, MAX_NAME_BYTES as u8 + 1];
        long.extend(std::iter::repeat_n(b'a', MAX_NAME_BYTES + 1 + 5));
        assert!(invalid(&long));
        assert!(invalid(&[v, HELLO, 2, 0xc3, 0x28, 0, 0, 0, 0, 0]));
        // More seats than players.
        let mut seats = vec![v, SEATS, MAX_PLAYERS as u8 + 1];
        seats.extend(std::iter::repeat_n(0, 16 * (MAX_PLAYERS + 1)));
        assert!(invalid(&seats));
        // An unknown reason to end a tether.
        assert!(invalid(&[v, TETHER_BREAK, 0, 1, TETHER_ENDS.len() as u8]));
        // A kart state: phase 7, the free bits of the state, the flags and the rotation.
        let kart = Message::KartState(KartSnapshot::default()).to_bytes();
        let (rotation, state, flags) = (
            2 + 4 + 1 + 12 + 5,
            2 + 4 + 1 + 12 + 6 + 12,
            2 + 4 + 1 + 12 + 6 + 12 + 1,
        );
        for (at, value) in [
            (state, 0x07),
            (state, 0x80),
            (flags, 0b100),
            (flags, 0x80),
            (rotation, 0x80),
        ] {
            let mut bad = kart.clone();
            bad[at] = value;
            assert!(invalid(&bad), "byte {at} = {value:#x}");
        }
        assert!(Message::decode(&kart).is_ok());
        // A float that is not finite.
        let mut grab = Message::Grab(Grab {
            grabber: 0,
            target: 1,
            tick: 0,
            seen: 0,
            grabber_position: [0.0; 3],
            rest: 1.0,
        })
        .to_bytes();
        let rest = grab.len() - 4;
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            grab[rest..].copy_from_slice(&bad.to_le_bytes());
            assert!(invalid(&grab));
        }
    }

    /// Random bytes, and every message with each of its bytes changed: refused or read, never a
    /// panic.
    #[test]
    fn garbage_never_panics() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let length = (next() % 80) as usize;
            let mut bytes: Vec<u8> = (0..length).map(|_| next() as u8).collect();
            if let Some(first) = bytes.first_mut() {
                *first = PROTOCOL_VERSION;
            }
            let _ = Message::decode(&bytes);
        }
        for message in every_message() {
            let bytes = message.to_bytes();
            for at in 0..bytes.len() {
                for value in [0, 1, 2, 0x7f, 0x80, 0xff] {
                    let mut changed = bytes.clone();
                    changed[at] = value;
                    if let Ok(read) = Message::decode(&changed) {
                        // What was read writes back to the same bytes.
                        assert_eq!(read.to_bytes(), changed, "{message:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn names_are_cleaned_and_cut_at_a_character() {
        assert_eq!(Profile::clean_name("  Max \n"), "Max");
        assert_eq!(Profile::clean_name("a\u{7}b"), "ab");
        assert_eq!(
            Profile::clean_name("Éloïse la très rapide"),
            "Éloïse la très r"
        );
        assert_eq!(Profile::clean_name("abcdefghijklmno pq"), "abcdefghijklmno");
        // A name that does not fit the wire is cut at a character boundary, still UTF-8.
        let wide = "é".repeat(40);
        let hello = Message::Hello(Profile {
            name: wide,
            ..Default::default()
        });
        let Ok(Message::Hello(read)) = Message::decode(&hello.to_bytes()) else {
            panic!("not read back");
        };
        assert_eq!(read.name, "é".repeat(MAX_NAME_BYTES / 2));
        assert_eq!(PeerKey(0xabcd << 112).tag(), "abcd");
    }
}
