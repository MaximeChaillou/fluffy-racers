//! The ghost of the time trial (issue #76, GD § 1): a run recorded tick by tick, kept with the
//! best time, played back as a translucent kart that touches nothing. Pure logic: unit-tested.
//!
//! - **What is recorded: the transforms**, not the inputs. Inputs (a few bytes a tick) replayed
//!   by the simulation would only be faithful while the simulation stays the same: the same
//!   binary does replay a race exactly (issues #12 and #40), but one more system or one more
//!   archetype in the game changes every race (`STATUS.md`, « Comment ajouter une vérification de
//!   course »), so a ghost kept in the browser would go wrong at the next version of the game.
//!   The pose of every tick, played as it was, stays right whatever the version (the ghost keeps
//!   the version of the game that recorded it, `trial::TrialRecord::game`).
//! - **Every tick** (60 Hz), from the grid (race tick 0, the pose before GO) to the finish (its
//!   last frame is the tick it crossed the line): the ghost is at the recorded pose on every tick,
//!   to the quantization. A frame per race tick: frame `k` is the pose at the end of race tick
//!   `k`, as the kart's own visual shows it between two ticks.
//! - **Quantized like the network** (issue #8): each frame is a [`KartSnapshot`]
//!   (`KartSnapshot::quantize`: the position to the millimeter, the rotation in 48 bits, the
//!   velocities, and the look: drift, sparks, boost flames, jump, trick, wheels, glider, water).
//! - **Played back** between two frames with the network's interpolation
//!   ([`net::interpolate`](crate::net::interpolate)).
//! - **Kept** compact ([`encode_frames`]): each frame says only what changed since the one
//!   before (the position against its extrapolation, the velocities, the rotation and the look
//!   bytes that changed), in variable-length integers: about 16 bytes a tick instead of 48 (a
//!   lap of 20 s: about 19 kB), written in base64 ([`base64_encode`]) inside the RON of the
//!   records, with their [`checksum`].
//!
//! The recording ([`GhostRecorder`]) reuses a buffer allocated once, for
//! [`GHOST_MAX_TICKS`] ticks: nothing is allocated while racing.

use bevy::math::Vec3;

use crate::kart::KartColor;
use crate::net::{KartPose, KartSnapshot, SampleLimits, interpolate};

/// The longest run a ghost keeps (ticks): 5 minutes. A longer run still sets its time; its ghost
/// stops there.
pub const GHOST_MAX_TICKS: usize = 5 * 60 * 60;

/// How many look bytes a frame has (see [`look_bytes`]).
const LOOK_BYTES: usize = 12;

/// A recorded run: its frames (frame `k` at race tick `k`), the parts and the color of its kart.
#[derive(Clone, Debug, PartialEq)]
pub struct GhostTrack {
    pub frames: Vec<KartSnapshot>,
    /// Its four parts, as on the wire (`Hello`): character, kart, wheels, wing.
    pub loadout: [u8; 4],
    pub color: KartColor,
}

impl GhostTrack {
    /// The race tick of its last frame (the finish, for a whole run).
    pub fn last_tick(&self) -> u32 {
        self.frames.len().saturating_sub(1) as u32
    }

    /// Its pose at race tick `tick` (fractional): on the grid before GO, between two frames
    /// afterwards; `None` once past its last frame (it crossed the line: gone).
    pub fn sample(&self, tick: f64, limits: &SampleLimits) -> Option<KartPose> {
        let first = self.frames.first()?;
        if tick <= 0.0 {
            return Some(first.pose());
        }
        let index = tick.floor() as usize;
        let a = self.frames.get(index)?;
        match self.frames.get(index + 1) {
            // On a tick: its frame, exactly.
            Some(b) if tick > f64::from(a.tick) => {
                Some(interpolate(&a.pose(), &b.pose(), tick, limits).pose)
            }
            Some(_) => Some(a.pose()),
            None => (tick <= f64::from(a.tick)).then(|| a.pose()),
        }
    }

    /// Where it is at race tick `tick` (whole ticks), as recorded.
    pub fn position_at(&self, tick: u32) -> Option<Vec3> {
        self.frames
            .get(tick as usize)
            .map(|frame| frame.pose().position)
    }
}

/// Records a run, a frame per race tick, in a buffer allocated once.
#[derive(Clone, Debug)]
pub struct GhostRecorder {
    frames: Vec<KartSnapshot>,
    capacity: usize,
}

impl Default for GhostRecorder {
    fn default() -> Self {
        Self::with_capacity(GHOST_MAX_TICKS)
    }
}

impl GhostRecorder {
    /// A recorder of `ticks` frames at most (allocated now).
    pub fn with_capacity(ticks: usize) -> Self {
        let capacity = ticks.max(1);
        Self {
            frames: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn frames(&self) -> &[KartSnapshot] {
        &self.frames
    }

    /// A new run: nothing recorded (the buffer kept).
    pub fn clear(&mut self) {
        self.frames.clear();
    }

    /// The pose `frame` of race tick `race_ticks`. Before GO (race tick 0), it is the grid: the
    /// run starts over from it. Then a frame per tick, up to the finish tick `finish` (none
    /// after). A tick missed (the countdown skipped: the first tick seen is past GO) is filled
    /// with this pose. Beyond the capacity, nothing more.
    pub fn record(&mut self, race_ticks: u32, frame: KartSnapshot, finish: Option<u32>) {
        let frame = KartSnapshot {
            tick: race_ticks,
            kart: 0,
            session: 0,
            ..frame
        };
        if race_ticks == 0 {
            self.frames.clear();
            self.frames.push(frame);
            return;
        }
        if finish.is_some_and(|finish| race_ticks > finish) {
            return;
        }
        while self.frames.len() <= race_ticks as usize && self.frames.len() < self.capacity {
            let tick = self.frames.len() as u32;
            self.frames.push(KartSnapshot { tick, ..frame });
        }
    }

    /// The frames recorded, for the ghost of a record; the recorder takes a new buffer of the
    /// same capacity (allocated once per record, never while racing).
    pub fn take(&mut self) -> Vec<KartSnapshot> {
        std::mem::replace(&mut self.frames, Vec::with_capacity(self.capacity))
    }
}

/// The bytes of a frame's look, in a fixed order: state, flags, hit (2), boost (2), slip,
/// slipstream, wheels (2, little-endian), items, piñata.
fn look_bytes(frame: &KartSnapshot) -> [u8; LOOK_BYTES] {
    let wheels = frame.wheels.to_le_bytes();
    [
        frame.state,
        frame.flags,
        frame.hit[0],
        frame.hit[1],
        frame.boost[0],
        frame.boost[1],
        frame.slip as u8,
        frame.slipstream,
        wheels[0],
        wheels[1],
        frame.items,
        frame.pinata,
    ]
}

fn set_look_bytes(frame: &mut KartSnapshot, bytes: &[u8; LOOK_BYTES]) {
    frame.state = bytes[0];
    frame.flags = bytes[1];
    frame.hit = [bytes[2], bytes[3]];
    frame.boost = [bytes[4], bytes[5]];
    frame.slip = bytes[6] as i8;
    frame.slipstream = bytes[7];
    frame.wheels = u16::from_le_bytes([bytes[8], bytes[9]]);
    frame.items = bytes[10];
    frame.pinata = bytes[11];
}

/// A rotation's 48 bits (`net::pack_rotation`): the index of its largest component, and its
/// three others (15 bits each, signed).
fn split_rotation(rotation: [u8; 6]) -> (u8, [i32; 3]) {
    let mut wide = [0u8; 8];
    wide[..6].copy_from_slice(&rotation);
    let bits = u64::from_le_bytes(wide);
    let parts = std::array::from_fn(|i| {
        let raw = ((bits >> (2 + 15 * i)) & 0x7fff) as i32;
        if raw >= 0x4000 { raw - 0x8000 } else { raw }
    });
    ((bits & 0b11) as u8, parts)
}

fn join_rotation(largest: u8, parts: [i32; 3]) -> [u8; 6] {
    let mut bits = u64::from(largest & 0b11);
    for (i, part) in parts.iter().enumerate() {
        bits |= ((*part as u64) & 0x7fff) << (2 + 15 * i);
    }
    let bytes = bits.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]
}

fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn push_signed(out: &mut Vec<u8>, value: i64) {
    push_varint(out, zigzag(value));
}

/// Reads the bytes of [`encode_frames`].
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| "the ghost's data ends too soon".to_string())?;
        self.at += 1;
        Ok(byte)
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a number of the ghost's data is too long".to_string())
    }

    fn signed(&mut self) -> Result<i64, String> {
        Ok(unzigzag(self.varint()?))
    }
}

/// The frames in their compact form (see the module's documentation). Per frame: two bytes (the
/// look bytes that changed, one bit each; the rotation's largest component), the position
/// against its extrapolation from the two frames before (mm), the velocity and the spin against
/// the frame before, the rotation's three components against the frame before (or as they are
/// when its largest component changed), then the look bytes that changed. The tick of frame `k`
/// is `k`.
pub fn encode_frames(frames: &[KartSnapshot]) -> Vec<u8> {
    let mut out = Vec::with_capacity(frames.len() * 18);
    let empty = KartSnapshot::default();
    for (k, frame) in frames.iter().enumerate() {
        let previous = if k > 0 { &frames[k - 1] } else { &empty };
        let before = if k > 1 { &frames[k - 2] } else { previous };
        let look = look_bytes(frame);
        let old_look = look_bytes(previous);
        let mut mask = 0u16;
        for (i, (new, old)) in look.iter().zip(&old_look).enumerate() {
            if new != old {
                mask |= 1 << i;
            }
        }
        let (largest, parts) = split_rotation(frame.rotation);
        let (old_largest, old_parts) = split_rotation(previous.rotation);
        out.extend_from_slice(&(mask | u16::from(largest) << 12).to_le_bytes());
        for axis in 0..3 {
            let predicted =
                2 * i64::from(previous.position[axis]) - i64::from(before.position[axis]);
            push_signed(&mut out, i64::from(frame.position[axis]) - predicted);
        }
        for axis in 0..3 {
            push_signed(
                &mut out,
                i64::from(frame.velocity[axis]) - i64::from(previous.velocity[axis]),
            );
        }
        for axis in 0..3 {
            push_signed(
                &mut out,
                i64::from(frame.spin[axis]) - i64::from(previous.spin[axis]),
            );
        }
        let relative = k > 0 && largest == old_largest;
        for (part, old) in parts.iter().zip(old_parts) {
            push_signed(
                &mut out,
                i64::from(*part) - if relative { i64::from(old) } else { 0 },
            );
        }
        for (i, byte) in look.iter().enumerate() {
            if mask & (1 << i) != 0 {
                out.push(*byte);
            }
        }
    }
    out
}

/// The `count` frames of [`encode_frames`]'s bytes. Bytes that do not hold exactly `count`
/// frames, or a value out of its field's range, are refused with the reason.
pub fn decode_frames(bytes: &[u8], count: usize) -> Result<Vec<KartSnapshot>, String> {
    if count == 0 || count > GHOST_MAX_TICKS {
        return Err(format!("a ghost of {count} frames"));
    }
    let mut reader = Reader { bytes, at: 0 };
    let mut frames: Vec<KartSnapshot> = Vec::with_capacity(count);
    let empty = KartSnapshot::default();
    for k in 0..count {
        let previous = if k > 0 { frames[k - 1] } else { empty };
        let before = if k > 1 { frames[k - 2] } else { previous };
        let header = u16::from_le_bytes([reader.byte()?, reader.byte()?]);
        if header >> 14 != 0 {
            return Err(format!("frame {k}: a header bit that means nothing"));
        }
        let mask = header & 0x0fff;
        let largest = (header >> 12) as u8;
        let mut frame = KartSnapshot {
            tick: k as u32,
            ..previous
        };
        for axis in 0..3 {
            let predicted =
                2 * i64::from(previous.position[axis]) - i64::from(before.position[axis]);
            let value = predicted + reader.signed()?;
            frame.position[axis] =
                i32::try_from(value).map_err(|_| format!("frame {k}: a position out of range"))?;
        }
        for axis in 0..3 {
            let value = i64::from(previous.velocity[axis]) + reader.signed()?;
            frame.velocity[axis] =
                i16::try_from(value).map_err(|_| format!("frame {k}: a velocity out of range"))?;
        }
        for axis in 0..3 {
            let value = i64::from(previous.spin[axis]) + reader.signed()?;
            frame.spin[axis] =
                i16::try_from(value).map_err(|_| format!("frame {k}: a spin out of range"))?;
        }
        let (old_largest, old_parts) = split_rotation(previous.rotation);
        let relative = k > 0 && largest == old_largest;
        let mut parts = [0i32; 3];
        for (part, old) in parts.iter_mut().zip(old_parts) {
            let value = reader.signed()? + if relative { i64::from(old) } else { 0 };
            if !(-0x4000..0x4000).contains(&value) {
                return Err(format!("frame {k}: a rotation out of range"));
            }
            *part = value as i32;
        }
        frame.rotation = join_rotation(largest, parts);
        let mut look = look_bytes(&previous);
        for (i, byte) in look.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *byte = reader.byte()?;
            }
        }
        set_look_bytes(&mut frame, &look);
        frames.push(frame);
    }
    if reader.at != bytes.len() {
        return Err(format!(
            "{} bytes after the ghost's {count} frames",
            bytes.len() - reader.at
        ));
    }
    Ok(frames)
}

/// A 32-bit FNV-1a hash of the frames' bytes, kept with them: a text damaged in the storage is
/// refused rather than played as a ghost gone astray (a byte lost can still decode).
pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash: u32, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Bytes as base64 text (the standard alphabet, with `=` padding): it fits in a RON string.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes of [`base64_encode`]'s text; anything else is refused.
pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return Err("base64 text of a length that is not a multiple of 4".to_string());
    }
    let value = |c: u8| -> Result<u32, String> {
        BASE64
            .iter()
            .position(|b| *b == c)
            .map(|v| v as u32)
            .ok_or_else(|| format!("{:?} is not base64", c as char))
    };
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let chunks = text.len() / 4;
    for (index, chunk) in text.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != chunks) {
            return Err("misplaced base64 padding".to_string());
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let digit = if i >= 4 - padding { 0 } else { value(*c)? };
            n |= digit << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - padding]);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::kart::{HitReaction, MiniTurbo};
    use crate::net::{KartLook, Phase};
    use bevy::math::Quat;

    /// A run of `ticks` ticks: a kart that drives a curve, jumps, drifts with its sparks, boosts,
    /// is carried by the pelican (a straight line up and back), with its wheels moving.
    pub(crate) fn run(ticks: u32) -> Vec<KartSnapshot> {
        (0..=ticks)
            .map(|k| {
                let t = k as f32 / 60.0;
                let angle = t * 0.4;
                let speed = (t * 8.0).min(26.0);
                let position = Vec3::new(
                    40.0 * angle.cos() - 40.0,
                    0.55 + if (300..330).contains(&k) {
                        2.0 * ((k - 300) as f32 / 30.0 * std::f32::consts::PI).sin()
                    } else {
                        0.0
                    },
                    -40.0 * angle.sin(),
                );
                let look = KartLook {
                    phase: match k {
                        300..330 => Phase::Airborne,
                        400..520 => Phase::Drifting,
                        700..850 => Phase::Towed,
                        _ => Phase::Grounded,
                    },
                    drift_right: k % 2 == 0,
                    sparks: (460..520).contains(&k).then_some(MiniTurbo::Orange),
                    reaction: HitReaction::Spin,
                    boost_strength: if (520..580).contains(&k) { 0.3 } else { 0.0 },
                    boost_remaining: if (520..580).contains(&k) {
                        (580 - k) as f32 / 60.0
                    } else {
                        0.0
                    },
                    slip: if (400..520).contains(&k) { -0.3 } else { 0.0 },
                    wheels: std::array::from_fn(|i| {
                        (!(300..330).contains(&k)).then_some(0.05 * ((k as usize + i) % 5) as f32)
                    }),
                    gliding: (900..960).contains(&k),
                    ..Default::default()
                };
                let pose = KartPose {
                    tick: k,
                    position,
                    rotation: Quat::from_rotation_y(-angle) * Quat::from_rotation_z(0.01 * t.sin()),
                    velocity: Vec3::new(-speed * angle.sin(), 0.0, -speed * angle.cos()),
                    spin: Vec3::new(0.0, -0.4, 0.0),
                    look,
                };
                KartSnapshot::quantize(0, &pose)
            })
            .collect()
    }

    /// The compact form gives back every frame exactly, bit for bit; it is about a third of
    /// the network's 48 bytes a frame.
    #[test]
    fn the_compact_frames_come_back_bit_for_bit() {
        let frames = run(70 * 60);
        let bytes = encode_frames(&frames);
        let back = decode_frames(&bytes, frames.len()).unwrap();
        for (k, (a, b)) in frames.iter().zip(&back).enumerate() {
            assert_eq!(a, b, "frame {k}");
        }
        assert_eq!(back.len(), frames.len());
        let per_frame = bytes.len() as f32 / frames.len() as f32;
        assert!(per_frame < 20.0, "{per_frame} bytes a frame");
        // One frame too many, one too few, a byte more or less: refused.
        assert!(decode_frames(&bytes, frames.len() + 1).is_err());
        assert!(decode_frames(&bytes, frames.len() - 1).is_err());
        assert!(decode_frames(&bytes[..bytes.len() - 1], frames.len()).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode_frames(&longer, frames.len()).is_err());
        assert!(decode_frames(&[], 0).is_err());
    }

    /// Any rotation the network packs (any largest component, any sign) survives the frames'
    /// form, also when the largest component changes from a frame to the next.
    #[test]
    fn every_rotation_survives_the_frames() {
        let frames: Vec<KartSnapshot> = (0..400)
            .map(|k| {
                let a = k as f32 * 0.05;
                let rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, a * 3.0, a, -a * 2.0);
                KartSnapshot::quantize(
                    0,
                    &KartPose {
                        tick: k,
                        rotation,
                        position: Vec3::new(1e6, -2e5, 3.0) * (k as f32).sin(),
                        ..Default::default()
                    },
                )
            })
            .collect();
        let back = decode_frames(&encode_frames(&frames), frames.len()).unwrap();
        assert_eq!(back, frames);
        for frame in &frames {
            let (largest, parts) = split_rotation(frame.rotation);
            assert_eq!(join_rotation(largest, parts), frame.rotation);
        }
    }

    /// The recorder: the grid until GO, then a frame per tick to the finish, none after; a
    /// skipped countdown fills the ticks it did not see; nothing beyond its capacity; a new run
    /// keeps its buffer.
    #[test]
    fn the_recorder_keeps_a_frame_per_race_tick_to_the_finish() {
        let frames = run(100);
        let mut recorder = GhostRecorder::with_capacity(80);
        let capacity = recorder.frames.capacity();
        for _ in 0..5 {
            recorder.record(0, frames[3], None);
        }
        assert_eq!(recorder.frames().len(), 1, "the grid, once");
        for k in 1..=60 {
            recorder.record(k, frames[k as usize], (k >= 50).then_some(50));
        }
        assert_eq!(
            recorder.frames().len(),
            51,
            "up to the finish tick, included"
        );
        assert_eq!(recorder.frames()[50].position, frames[50].position);
        for (k, frame) in recorder.frames().iter().enumerate() {
            assert_eq!(frame.tick, k as u32);
        }
        recorder.clear();
        recorder.record(5, frames[5], None);
        assert_eq!(recorder.frames().len(), 6, "a countdown skipped");
        assert!(
            recorder
                .frames()
                .iter()
                .all(|f| f.position == frames[5].position)
        );
        for k in 6..200 {
            recorder.record(k, frames[(k % 100) as usize], None);
        }
        assert_eq!(recorder.frames().len(), 80, "its capacity");
        assert_eq!(recorder.frames.capacity(), capacity, "never grown");
        let taken = recorder.take();
        assert_eq!(taken.len(), 80);
        assert!(recorder.frames().is_empty());
        assert_eq!(recorder.frames.capacity(), capacity);
    }

    /// Played back, the ghost is at the recorded pose on every tick (to the quantization), in
    /// between two ticks between them, on the grid before GO, and gone after its last frame.
    #[test]
    fn the_ghost_is_where_it_was_recorded_on_every_tick() {
        let frames = run(1000);
        let ghost = GhostTrack {
            frames: frames.clone(),
            loadout: [0; 4],
            color: KartColor::default(),
        };
        let limits = crate::net::NetTuning::default().limits();
        for (k, frame) in frames.iter().enumerate() {
            let pose = ghost.sample(k as f64, &limits).unwrap();
            let recorded = frame.pose();
            assert!(
                pose.position.distance(recorded.position) < 1e-4,
                "tick {k}: {} vs {}",
                pose.position,
                recorded.position
            );
            assert!(pose.rotation.angle_between(recorded.rotation) < 1e-3);
            assert_eq!(pose.look.phase, recorded.look.phase, "tick {k}");
        }
        let half = ghost.sample(10.5, &limits).unwrap().position;
        let (a, b) = (frames[10].pose().position, frames[11].pose().position);
        assert!(
            half.distance((a + b) / 2.0) < 0.01,
            "{half} between {a} and {b}"
        );
        assert_eq!(
            ghost.sample(-3.0, &limits).unwrap().position,
            frames[0].pose().position
        );
        assert!(ghost.sample(1000.0, &limits).is_some());
        assert!(ghost.sample(1000.2, &limits).is_none(), "past the finish");
        assert_eq!(ghost.last_tick(), 1000);
    }

    #[test]
    fn the_checksum_sees_a_byte_changed_lost_or_moved() {
        let bytes = encode_frames(&run(300));
        let sum = checksum(&bytes);
        assert_eq!(checksum(b""), 0x811c_9dc5);
        assert_eq!(checksum(b"a"), 0xe40c_292c, "FNV-1a");
        let mut changed = bytes.clone();
        changed[100] ^= 1;
        assert_ne!(checksum(&changed), sum);
        assert_ne!(checksum(&bytes[6..]), sum);
        let mut moved = bytes.clone();
        moved.swap(10, 11);
        if moved != bytes {
            assert_ne!(checksum(&moved), sum);
        }
    }

    #[test]
    fn base64_goes_there_and_back() {
        for length in 0..40 {
            let bytes: Vec<u8> = (0..length).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64_encode(&bytes);
            assert_eq!(text.len() % 4, 0);
            assert_eq!(base64_decode(&text).unwrap(), bytes, "{text}");
        }
        assert_eq!(base64_encode(b"Fluffy"), "Rmx1ZmZ5");
        assert_eq!(base64_encode(b"Flu"), "Rmx1");
        assert_eq!(base64_encode(b"Fl"), "Rmw=");
        assert_eq!(base64_decode("Rmw=").unwrap(), b"Fl");
        for bad in ["Rmw", "Rm=w", "R===", "Rm w", "Rmé=", "Rmw=Rmw="] {
            assert!(base64_decode(bad).is_err(), "{bad}");
        }
    }
}
