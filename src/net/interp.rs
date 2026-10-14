//! Remote karts on screen (issue #8, `docs/ARCHITECTURE.md` § « Synchronisation »): the states
//! of a kart another peer drives arrive 30 times a second, late, out of order, sometimes twice,
//! sometimes never. Pure logic: unit-tested.
//!
//! - [`SnapshotBuffer`]: the last states of a kart, by tick. [`SnapshotBuffer::sample`] gives
//!   its pose at any tick, fractional: between two states it **interpolates** (position along
//!   the curve their velocities give, rotation by slerp); past the newest it **extrapolates**
//!   with its velocities, for [`NetTuning::max_extrapolation_ms`](super::NetTuning) at most, then
//!   the kart stays where it got to; between two states too far apart for the kart to drive
//!   (the tow pelican, a reset) it **teleports**.
//! - [`Playback`]: which tick of a remote kart to show at each moment. Its states arrive at
//!   least `lag` after their tick (the windowed minimum of the latency, clock offsets included);
//!   it is shown `delay` later still (about 100 ms: the buffer), so that the next state is
//!   usually there. When the lag changes, the playback runs up to 10 % faster or slower until it
//!   catches up, so that the kart never jumps; beyond 250 ms, it jumps.
//! - [`TickClock`]: the room's common tick (60 per second on the host's clock, from
//!   `Lobby::host_time_ms`) of each local tick, steady: it moves only when the estimate drifts
//!   by more than 2 ticks (a pause, a new host).
//! - [`PoseHistory`]: our own kart's last 30 positions by common tick, for the victim's check of
//!   a paw strike or a grab (issue #34: `tick_vu`).
//!
//! Nothing here allocates once created: the buffers have a fixed capacity.

use bevy::math::{Quat, Vec3};

use super::snapshot::KartPose;

/// Ticks per second of the common clock.
const TICKS_PER_SECOND: f64 = crate::TICK_HZ;
/// Milliseconds per tick.
pub const TICK_MS: f64 = 1_000.0 / TICKS_PER_SECOND;

/// States kept per remote kart: about a second at 30 per second.
pub const BUFFER_CAPACITY: usize = 32;

/// A state this many ticks older than the newest one means the sender's clock jumped back (a new
/// host): the buffer starts over from it (2 s; no state arrives that late otherwise).
pub const RESTART_TICKS: u32 = 120;

/// Between two states further apart than this (ticks), the position follows a straight line
/// instead of the curve of their velocities (a collision in a long gap would bend it).
const CURVE_MAX_TICKS: f64 = 15.0;

/// Our own kart's positions kept for the victim's check (docs/ARCHITECTURE.md: 30 ticks, 0.5 s).
pub const HISTORY_TICKS: usize = 30;

/// The common clock moves only when the estimate drifts this far (ticks).
pub const CLOCK_HYSTERESIS_TICKS: f64 = 2.0;

/// Buckets of the lag's windowed minimum.
const LAG_BUCKETS: usize = 8;

/// What became of a state given to the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Insert {
    Added,
    /// Already there (sent twice, or a copy).
    Duplicate,
    /// Older than everything kept, the buffer full: useless.
    TooOld,
    /// The sender's clock jumped back: the buffer starts over from it.
    Restarted,
}

/// How a pose was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleKind {
    /// Before the oldest state kept: that one.
    Oldest,
    /// Between two states.
    Interpolated,
    /// Between two states too far apart to drive: the later one.
    Teleport,
    /// After the newest state, carried on with its velocities.
    Extrapolated,
    /// Too long after the newest state: where the extrapolation stopped.
    Frozen,
}

/// The pose of a kart at a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// `tick` is the state shown (rounded down).
    pub pose: KartPose,
    pub kind: SampleKind,
}

/// The limits of [`SnapshotBuffer::sample`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleLimits {
    /// The longest extrapolation (ticks).
    pub max_extrapolation_ticks: f64,
    /// Two states further apart than this speed allows (m/s) are a teleport.
    pub teleport_speed: f32,
}

/// The last states of a remote kart, oldest first (see the module's documentation).
#[derive(Clone, Debug)]
pub struct SnapshotBuffer {
    poses: Vec<KartPose>,
}

impl Default for SnapshotBuffer {
    fn default() -> Self {
        Self {
            poses: Vec::with_capacity(BUFFER_CAPACITY + 1),
        }
    }
}

impl SnapshotBuffer {
    pub fn len(&self) -> usize {
        self.poses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.poses.is_empty()
    }

    pub fn newest(&self) -> Option<&KartPose> {
        self.poses.last()
    }

    pub fn oldest(&self) -> Option<&KartPose> {
        self.poses.first()
    }

    pub fn clear(&mut self) {
        self.poses.clear();
    }

    /// Keeps a state, in tick order, whatever order they come in.
    pub fn insert(&mut self, pose: KartPose) -> Insert {
        let Some(newest) = self.poses.last() else {
            self.poses.push(pose);
            return Insert::Added;
        };
        if pose.tick.saturating_add(RESTART_TICKS) < newest.tick {
            self.poses.clear();
            self.poses.push(pose);
            return Insert::Restarted;
        }
        let at = match self.poses.binary_search_by_key(&pose.tick, |p| p.tick) {
            Ok(_) => return Insert::Duplicate,
            Err(at) => at,
        };
        if at == 0 && self.poses.len() >= BUFFER_CAPACITY {
            return Insert::TooOld;
        }
        self.poses.insert(at, pose);
        if self.poses.len() > BUFFER_CAPACITY {
            self.poses.remove(0);
        }
        Insert::Added
    }

    /// The kart's pose at `tick` (fractional), `None` before any state.
    pub fn sample(&self, tick: f64, limits: &SampleLimits) -> Option<Sample> {
        let oldest = self.poses.first()?;
        let newest = self.poses.last()?;
        if tick < f64::from(oldest.tick) {
            return Some(Sample {
                pose: *oldest,
                kind: SampleKind::Oldest,
            });
        }
        if tick >= f64::from(newest.tick) {
            return Some(extrapolate(newest, tick, limits));
        }
        // The first state after `tick`: there is one, and one at or before it.
        let after = self.poses.partition_point(|p| f64::from(p.tick) <= tick);
        let (a, b) = (&self.poses[after - 1], &self.poses[after]);
        Some(between(a, b, tick, limits))
    }
}

/// Between `a` and `b` (`a.tick <= tick < b.tick`).
fn between(a: &KartPose, b: &KartPose, tick: f64, limits: &SampleLimits) -> Sample {
    let span = f64::from(b.tick - a.tick);
    let seconds = (span / TICKS_PER_SECOND) as f32;
    let distance = a.position.distance(b.position);
    if distance > limits.teleport_speed * seconds {
        return Sample {
            pose: *b,
            kind: SampleKind::Teleport,
        };
    }
    let into = tick - f64::from(a.tick);
    let t = (into / span) as f32;
    let position = if span <= CURVE_MAX_TICKS {
        hermite(a.position, a.velocity, b.position, b.velocity, t, seconds)
    } else {
        a.position.lerp(b.position, t)
    };
    let mut look = a.look;
    look.slip = a.look.slip + (b.look.slip - a.look.slip) * t;
    look.hit_elapsed = a.look.hit_elapsed + into as u32;
    look.boost_remaining = (a.look.boost_remaining - (into / TICKS_PER_SECOND) as f32).max(0.0);
    if let (Some(from), Some(to)) = (a.look.slipstream, b.look.slipstream) {
        look.slipstream = Some(from + (to - from) * t);
    }
    for (wheel, (from, to)) in look
        .wheels
        .iter_mut()
        .zip(a.look.wheels.iter().zip(&b.look.wheels))
    {
        if let (Some(from), Some(to)) = (from, to) {
            *wheel = Some(from + (to - from) * t);
        }
    }
    Sample {
        pose: KartPose {
            tick: tick.floor() as u32,
            position,
            rotation: a.rotation.slerp(b.rotation, t),
            velocity: a.velocity.lerp(b.velocity, t),
            spin: a.spin.lerp(b.spin, t),
            look,
        },
        kind: SampleKind::Interpolated,
    }
}

/// After the newest state: carried on with its velocities, for a while at most.
fn extrapolate(newest: &KartPose, tick: f64, limits: &SampleLimits) -> Sample {
    let ahead = tick - f64::from(newest.tick);
    let frozen = ahead > limits.max_extrapolation_ticks;
    let ticks = ahead.min(limits.max_extrapolation_ticks.max(0.0));
    let seconds = (ticks / TICKS_PER_SECOND) as f32;
    let mut look = newest.look;
    look.boost_remaining = (look.boost_remaining - seconds).max(0.0);
    Sample {
        pose: KartPose {
            tick: newest.tick + ticks as u32,
            position: newest.position + newest.velocity * seconds,
            rotation: (Quat::from_scaled_axis(newest.spin * seconds) * newest.rotation).normalize(),
            velocity: newest.velocity,
            spin: newest.spin,
            look,
        },
        kind: if frozen {
            SampleKind::Frozen
        } else {
            SampleKind::Extrapolated
        },
    }
}

/// The cubic curve from `p0` to `p1` leaving with velocity `v0` and arriving with `v1`, at `t`
/// (0 to 1) of `seconds`: a kart that turns between two states follows its turn.
fn hermite(p0: Vec3, v0: Vec3, p1: Vec3, v1: Vec3, t: f32, seconds: f32) -> Vec3 {
    let (t2, t3) = (t * t, t * t * t);
    p0 * (2.0 * t3 - 3.0 * t2 + 1.0)
        + v0 * ((t3 - 2.0 * t2 + t) * seconds)
        + p1 * (-2.0 * t3 + 3.0 * t2)
        + v1 * ((t3 - t2) * seconds)
}

/// Which tick of a remote kart to show at each moment (see the module's documentation).
#[derive(Clone, Debug)]
pub struct Playback {
    /// The lag's windowed minimum: per bucket of arrival time, its id and its minimum (ms).
    ids: [i64; LAG_BUCKETS],
    mins: [f64; LAG_BUCKETS],
    /// Local time minus the stamp of the state shown (ms): the lag plus the delay, once known.
    offset: Option<f64>,
}

/// The settings of a [`Playback`] (`NetTuning`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackSettings {
    /// Shown this long after the fastest states arrive (ms).
    pub delay_ms: f64,
    /// The lag is the minimum of the last this long (ms).
    pub window_ms: f64,
    /// The playback runs at most this much faster or slower to catch up (0.1: 10 %).
    pub correction: f64,
    /// Further than this from where it should be (ms), it jumps.
    pub snap_ms: f64,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            ids: [i64::MIN; LAG_BUCKETS],
            mins: [0.0; LAG_BUCKETS],
            offset: None,
        }
    }
}

impl Playback {
    /// A state of tick `tick` arrived at `now_ms` (local time).
    pub fn arrived(&mut self, now_ms: f64, tick: u32, window_ms: f64) {
        let lag = now_ms - f64::from(tick) * TICK_MS;
        let id = bucket(now_ms, window_ms);
        let slot = id.rem_euclid(LAG_BUCKETS as i64) as usize;
        if self.ids[slot] != id {
            self.ids[slot] = id;
            self.mins[slot] = lag;
        } else if lag < self.mins[slot] {
            self.mins[slot] = lag;
        }
    }

    /// How late the fastest states of the last window arrived (ms): `None` when none did.
    pub fn lag_ms(&self, now_ms: f64, window_ms: f64) -> Option<f64> {
        let now = bucket(now_ms, window_ms);
        self.ids
            .iter()
            .zip(&self.mins)
            .filter(|(id, _)| **id > now - LAG_BUCKETS as i64 && **id <= now)
            .map(|(_, min)| *min)
            .min_by(f64::total_cmp)
    }

    /// Once a frame: the offset moves toward the lag plus the delay, `dt_ms` after the last
    /// frame, without making the kart jump (unless it is too far).
    pub fn advance(&mut self, now_ms: f64, dt_ms: f64, settings: &PlaybackSettings) {
        let Some(lag) = self.lag_ms(now_ms, settings.window_ms) else {
            return;
        };
        let target = lag + settings.delay_ms;
        self.offset = Some(match self.offset {
            Some(offset) if (target - offset).abs() <= settings.snap_ms => {
                let step = settings.correction * dt_ms.max(0.0);
                offset + (target - offset).clamp(-step, step)
            }
            _ => target,
        });
    }

    /// The tick to show at `now_ms` (fractional), once a state arrived.
    pub fn tick_at(&self, now_ms: f64) -> Option<f64> {
        self.offset.map(|offset| (now_ms - offset) / TICK_MS)
    }

    /// Local time minus the stamp of the state shown (ms).
    pub fn offset_ms(&self) -> Option<f64> {
        self.offset
    }
}

fn bucket(now_ms: f64, window_ms: f64) -> i64 {
    let length = (window_ms / LAG_BUCKETS as f64).max(1.0);
    (now_ms / length).floor() as i64
}

/// The room's common tick of each local tick (see the module's documentation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickClock {
    /// Common tick minus local tick.
    offset: Option<i64>,
}

impl TickClock {
    /// A clock already set (the tests, or a known offset).
    pub fn with_offset(offset: i64) -> Self {
        Self {
            offset: Some(offset),
        }
    }

    /// The estimate of `common - local` now (fractional ticks). Returns whether the clock moved.
    pub fn observe(&mut self, estimate: f64) -> bool {
        if !estimate.is_finite() {
            return false;
        }
        match self.offset {
            Some(offset) if (estimate - offset as f64).abs() <= CLOCK_HYSTERESIS_TICKS => false,
            _ => {
                self.offset = Some(estimate.round() as i64);
                true
            }
        }
    }

    /// The common tick of local tick `local`, once the clock is set.
    pub fn common(&self, local: u32) -> Option<u32> {
        self.offset
            .map(|offset| (i64::from(local) + offset).clamp(0, i64::from(u32::MAX)) as u32)
    }

    /// The local tick (fractional) of a common one.
    pub fn local(&self, common: f64) -> Option<f64> {
        self.offset.map(|offset| common - offset as f64)
    }

    pub fn is_set(&self) -> bool {
        self.offset.is_some()
    }
}

/// Our own kart's last [`HISTORY_TICKS`] positions, by common tick (a ring, no allocation).
#[derive(Clone, Copy, Debug)]
pub struct PoseHistory {
    entries: [(u32, Vec3); HISTORY_TICKS],
    len: usize,
    /// Where the next one goes.
    next: usize,
}

impl Default for PoseHistory {
    fn default() -> Self {
        Self {
            entries: [(0, Vec3::ZERO); HISTORY_TICKS],
            len: 0,
            next: 0,
        }
    }
}

impl PoseHistory {
    /// Our kart was at `position` on `tick`. A tick that goes back (the clock moved) starts over.
    pub fn record(&mut self, tick: u32, position: Vec3) {
        if let Some((newest, _)) = self.newest() {
            if tick == newest {
                let last = (self.next + HISTORY_TICKS - 1) % HISTORY_TICKS;
                self.entries[last].1 = position;
                return;
            }
            if tick < newest {
                self.len = 0;
            }
        }
        self.entries[self.next] = (tick, position);
        self.next = (self.next + 1) % HISTORY_TICKS;
        self.len = (self.len + 1).min(HISTORY_TICKS);
    }

    fn newest(&self) -> Option<(u32, Vec3)> {
        (self.len > 0).then(|| self.entries[(self.next + HISTORY_TICKS - 1) % HISTORY_TICKS])
    }

    /// The newest tick recorded.
    pub fn newest_tick(&self) -> Option<u32> {
        self.newest().map(|(tick, _)| tick)
    }

    /// Where our kart was on `tick`: the last position recorded at or before it, the newest for a
    /// tick to come, `None` before the oldest kept (an event too old).
    pub fn position_at(&self, tick: u32) -> Option<Vec3> {
        let (newest, position) = self.newest()?;
        if tick >= newest {
            return Some(position);
        }
        (0..self.len)
            .map(|back| self.entries[(self.next + HISTORY_TICKS - 1 - back) % HISTORY_TICKS])
            .find(|(at, _)| *at <= tick)
            .map(|(_, position)| position)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::snapshot::{KartLook, Phase};

    const LIMITS: SampleLimits = SampleLimits {
        max_extrapolation_ticks: 15.0,
        teleport_speed: 80.0,
    };

    /// A kart at `position` on `tick`, driving at `velocity`.
    fn pose(tick: u32, position: Vec3, velocity: Vec3) -> KartPose {
        KartPose {
            tick,
            position,
            rotation: Quat::IDENTITY,
            velocity,
            spin: Vec3::ZERO,
            look: KartLook::default(),
        }
    }

    /// A kart driving straight at 30 m/s along -Z: its state at `tick`.
    fn straight(tick: u32) -> KartPose {
        let velocity = Vec3::new(0.0, 0.0, -30.0);
        pose(tick, velocity * (tick as f32 / 60.0), velocity)
    }

    /// Issue #8 acceptance: between two states, the position, the rotation (slerp) and the
    /// velocities are interpolated.
    #[test]
    fn interpolates_between_two_states() {
        let mut buffer = SnapshotBuffer::default();
        let mut a = pose(100, Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -30.0));
        a.rotation = Quat::from_rotation_y(0.2);
        a.look.slip = 0.1;
        let mut b = pose(102, Vec3::new(0.0, 1.0, -1.0), Vec3::new(0.0, 0.0, -30.0));
        b.rotation = Quat::from_rotation_y(0.4);
        b.look.slip = 0.3;
        b.spin = Vec3::Y;
        assert_eq!(buffer.insert(a), Insert::Added);
        assert_eq!(buffer.insert(b), Insert::Added);
        let half = buffer.sample(101.0, &LIMITS).unwrap();
        assert_eq!(half.kind, SampleKind::Interpolated);
        assert_eq!(half.pose.tick, 101);
        // Driving straight at the speed that joins them: halfway.
        assert!(half.pose.position.distance(Vec3::new(0.0, 1.0, -0.5)) < 1e-5);
        let yaw = half.pose.rotation.to_euler(bevy::math::EulerRot::YXZ).0;
        assert!((yaw - 0.3).abs() < 1e-5, "{yaw}");
        assert!((half.pose.look.slip - 0.2).abs() < 1e-6);
        assert!((half.pose.spin - Vec3::Y * 0.5).length() < 1e-6);
        // At the ends, the states themselves.
        let start = buffer.sample(100.0, &LIMITS).unwrap();
        assert!(start.pose.position.distance(a.position) < 1e-6);
        let near_end = buffer.sample(101.999, &LIMITS).unwrap();
        assert!(near_end.pose.position.distance(b.position) < 1e-3);
        // A quarter of the way, a constant speed: a quarter of the distance.
        let quarter = buffer.sample(100.5, &LIMITS).unwrap();
        assert!((quarter.pose.position.z + 0.25).abs() < 1e-5);
    }

    /// A kart that turns between two states follows the curve of its velocities, not the chord.
    #[test]
    fn a_turn_follows_its_curve() {
        // A circle of 20 m at 20 m/s (1 rad/s): two states 6 ticks apart.
        let at = |tick: u32| {
            let angle = tick as f32 / 60.0;
            let position = Vec3::new(20.0 * angle.cos(), 0.0, 20.0 * angle.sin());
            let velocity = Vec3::new(-20.0 * angle.sin(), 0.0, 20.0 * angle.cos());
            pose(tick, position, velocity)
        };
        let mut buffer = SnapshotBuffer::default();
        buffer.insert(at(0));
        buffer.insert(at(6));
        let middle = buffer.sample(3.0, &LIMITS).unwrap().pose.position;
        let truth = at(3).position;
        let chord = at(0).position.lerp(at(6).position, 0.5);
        assert!(middle.distance(truth) < 0.002, "{middle} vs {truth}");
        assert!(chord.distance(truth) > 0.02);
    }

    /// Issue #8 acceptance: past the newest state, the kart carries on with its velocity for
    /// 250 ms at most (15 ticks here), then stays there.
    #[test]
    fn extrapolation_is_bounded() {
        let mut buffer = SnapshotBuffer::default();
        buffer.insert(straight(200));
        let mut newest = straight(202);
        newest.spin = Vec3::new(0.0, 1.0, 0.0);
        buffer.insert(newest);
        let soon = buffer.sample(208.0, &LIMITS).unwrap();
        assert_eq!(soon.kind, SampleKind::Extrapolated);
        assert!(soon.pose.position.distance(straight(208).position) < 1e-4);
        let (_, yaw) = soon.pose.rotation.to_axis_angle();
        assert!((yaw - 0.1).abs() < 1e-4, "6 ticks at 1 rad/s: {yaw}");
        for late in [217.5, 230.0, 1_000.0, 1e9] {
            let frozen = buffer.sample(late, &LIMITS).unwrap();
            assert_eq!(frozen.kind, SampleKind::Frozen);
            assert!(
                frozen.pose.position.distance(straight(217).position) < 1e-4,
                "{late}: {}",
                frozen.pose.position
            );
            assert_eq!(frozen.pose.tick, 217);
        }
        // Right at the limit, not frozen yet.
        assert_eq!(
            buffer.sample(217.0, &LIMITS).unwrap().kind,
            SampleKind::Extrapolated
        );
        // Before the oldest state: that one.
        let early = buffer.sample(150.0, &LIMITS).unwrap();
        assert_eq!(early.kind, SampleKind::Oldest);
        assert_eq!(early.pose, straight(200));
        assert!(SnapshotBuffer::default().sample(0.0, &LIMITS).is_none());
    }

    /// The states come out of order, twice, or never: the buffer keeps them in order, once, and
    /// interpolates across the gaps.
    #[test]
    fn the_buffer_handles_disorder_duplicates_and_losses() {
        let mut buffer = SnapshotBuffer::default();
        for tick in [10, 14, 12, 12, 18, 16, 14] {
            buffer.insert(straight(tick));
        }
        let ticks: Vec<u32> = buffer.poses.iter().map(|p| p.tick).collect();
        assert_eq!(ticks, [10, 12, 14, 16, 18]);
        assert_eq!(buffer.insert(straight(12)), Insert::Duplicate);
        // 20 and 22 lost: from 18 to 24, still on the line.
        buffer.insert(straight(24));
        for tenth in 180..240 {
            let tick = tenth as f64 / 10.0;
            let sample = buffer.sample(tick, &LIMITS).unwrap();
            assert_eq!(sample.kind, SampleKind::Interpolated, "{tick}");
            let truth = -30.0 * tick as f32 / 60.0;
            assert!((sample.pose.position.z - truth).abs() < 1e-4, "{tick}");
        }
        // Full: the oldest go first, and older than everything kept is useless.
        for tick in 25..80 {
            buffer.insert(straight(tick));
        }
        assert_eq!(buffer.len(), BUFFER_CAPACITY);
        assert_eq!(buffer.oldest().unwrap().tick, 80 - BUFFER_CAPACITY as u32);
        assert_eq!(buffer.insert(straight(30)), Insert::TooOld);
        assert_eq!(buffer.newest().unwrap().tick, 79);
        // A late one inside the range still finds its place.
        let mut late = straight(1_000);
        late.tick = 60;
        assert_eq!(buffer.insert(late), Insert::Duplicate);
        // The sender's clock jumped back by more than 2 s: start over.
        buffer.insert(straight(5_000));
        // Late by 2 s exactly: still a state of the same clock.
        assert_eq!(
            buffer.insert(straight(5_000 - RESTART_TICKS)),
            Insert::Added
        );
        assert_eq!(buffer.insert(straight(100)), Insert::Restarted);
        assert_eq!(buffer.len(), 1);
    }

    /// Two states too far apart for a kart to drive (the tow pelican, a reset): no slide
    /// between them, the later one at once.
    #[test]
    fn a_jump_is_a_teleport() {
        let mut buffer = SnapshotBuffer::default();
        buffer.insert(pose(0, Vec3::ZERO, Vec3::ZERO));
        // 30 m in 2 ticks: 900 m/s.
        let far = pose(2, Vec3::new(30.0, 0.0, 0.0), Vec3::ZERO);
        buffer.insert(far);
        let sample = buffer.sample(0.5, &LIMITS).unwrap();
        assert_eq!(sample.kind, SampleKind::Teleport);
        assert_eq!(sample.pose, far);
        // 2 m in 2 ticks (60 m/s) is driving.
        buffer.insert(pose(4, Vec3::new(32.0, 0.0, 0.0), Vec3::ZERO));
        assert_eq!(
            buffer.sample(3.0, &LIMITS).unwrap().kind,
            SampleKind::Interpolated
        );
    }

    /// The look follows: the slip interpolated, a hit's clock and the boost's time running.
    #[test]
    fn the_look_runs_between_states() {
        let mut a = straight(0);
        a.look = KartLook {
            phase: Phase::Hit,
            hit_elapsed: 10,
            hit_duration: 60,
            boost_strength: 0.3,
            boost_remaining: 1.0,
            slipstream: Some(0.2),
            wheels: [Some(0.1), None, Some(0.2), Some(0.3)],
            ..Default::default()
        };
        let mut b = straight(4);
        b.look = KartLook {
            slipstream: Some(0.4),
            wheels: [Some(0.3), Some(0.3), None, Some(0.1)],
            ..a.look
        };
        let mut buffer = SnapshotBuffer::default();
        buffer.insert(a);
        buffer.insert(b);
        let look = buffer.sample(2.0, &LIMITS).unwrap().pose.look;
        assert_eq!(look.phase, Phase::Hit);
        assert_eq!(look.hit_elapsed, 12);
        assert!((look.boost_remaining - (1.0 - 2.0 / 60.0)).abs() < 1e-5);
        assert!((look.slipstream.unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(look.wheels[1], None);
        assert_eq!(look.wheels[2], Some(0.2));
        assert!((look.wheels[0].unwrap() - 0.2).abs() < 1e-6);
    }

    fn settings() -> PlaybackSettings {
        PlaybackSettings {
            delay_ms: 100.0,
            window_ms: 2_000.0,
            correction: 0.1,
            snap_ms: 250.0,
        }
    }

    /// The playback shows a kart `delay` after its fastest states arrive: with 120 ms of
    /// latency and ±20 ms of jitter, 100 ms behind the fastest, so that the next state is
    /// almost always there.
    #[test]
    fn the_playback_waits_for_the_states() {
        let mut playback = Playback::default();
        let settings = settings();
        assert_eq!(playback.tick_at(0.0), None);
        // The sender's clock is 5 s ahead of ours; 120 ± 20 ms of latency.
        let mut draw = crate::net::snapshot::tests::Draw(7);
        let mut now = 0.0;
        let mut late = 0;
        let mut checked = 0;
        for frame in 0..600u32 {
            now = frame as f64 * TICK_MS;
            // A state every 2 ticks, stamped on the sender's clock.
            if frame % 2 == 0 && frame >= 12 {
                let sent = now - 120.0 - draw.range(-20.0, 20.0) as f64;
                let tick = ((sent + 5_000.0) / TICK_MS).round() as u32;
                playback.arrived(now, tick, settings.window_ms);
            }
            playback.advance(now, TICK_MS, &settings);
            if let Some(shown) = playback.tick_at(now) {
                // The newest state that surely arrived was sent 140 ms ago.
                let newest = (now - 140.0 + 5_000.0) / TICK_MS;
                if frame > 60 {
                    checked += 1;
                    if shown > newest - 2.0 {
                        late += 1;
                    }
                }
                // Never in the future of what was sent.
                assert!(shown < (now - 100.0 + 5_000.0) / TICK_MS);
            }
        }
        assert!(checked > 500 && late == 0, "{late} of {checked}");
        let lag = playback.lag_ms(now, settings.window_ms).unwrap();
        assert!((lag - (100.0 - 5_000.0)).abs() < 12.0, "{lag}");
    }

    /// When the lag changes a little, the playback catches up slowly (at most 10 % faster or
    /// slower); a big change makes it jump.
    #[test]
    fn the_playback_catches_up_smoothly() {
        let settings = settings();
        let mut playback = Playback::default();
        let mut now = 0.0;
        let feed = |playback: &mut Playback, now: f64, lag: f64| {
            let tick = ((now - lag) / TICK_MS).floor() as u32;
            playback.arrived(now, tick, settings.window_ms);
        };
        for _ in 0..120 {
            now += TICK_MS;
            feed(&mut playback, now, 50.0);
            playback.advance(now, TICK_MS, &settings);
        }
        let before = playback.tick_at(now).unwrap();
        // The latency drops by 40 ms: the window's minimum follows at once; the playback moves
        // 1.67 ms per frame at most.
        let mut previous = before;
        for _ in 0..60 {
            now += TICK_MS;
            feed(&mut playback, now, 10.0);
            playback.advance(now, TICK_MS, &settings);
            let shown = playback.tick_at(now).unwrap();
            let step = shown - previous;
            assert!(step > 0.89 && step < 1.11, "{step}");
            previous = shown;
        }
        // The latency grows by 1 s: the old minimum lasts the window (2 s), then it jumps.
        for frame in 0..200 {
            now += TICK_MS;
            feed(&mut playback, now, 1_010.0);
            playback.advance(now, TICK_MS, &settings);
            let shown = playback.tick_at(now).unwrap();
            if frame < 100 {
                assert!(shown - previous > 0.85, "frame {frame}");
            }
            previous = shown;
        }
        let offset = playback.offset_ms().unwrap();
        assert!((offset - 1_110.0).abs() < 20.0, "{offset}");
        // Nothing arrives for a while: the playback goes on as it was.
        let still = playback.offset_ms();
        playback.advance(now + 10_000.0, TICK_MS, &settings);
        assert_eq!(playback.offset_ms(), still);
    }

    /// The common clock follows its estimate only when it drifts by more than 2 ticks: frame
    /// jitter never moves it, a pause or a new host does.
    #[test]
    fn the_common_clock_is_steady() {
        let mut clock = TickClock::default();
        assert_eq!(clock.common(10), None);
        assert!(clock.observe(1_000.4));
        assert_eq!(clock.common(10), Some(1_010));
        for jitter in [-1.9, 1.2, 0.6, -0.4, 1.99] {
            assert!(!clock.observe(1_000.0 + jitter));
        }
        assert_eq!(clock.common(10), Some(1_010));
        assert!(clock.observe(1_300.2), "a pause of 5 s");
        assert_eq!(clock.common(10), Some(1_310));
        assert!(!clock.observe(f64::NAN));
        assert_eq!(clock.local(1_310.5), Some(10.5));
        assert_eq!(TickClock::with_offset(-20).common(5), Some(0));
    }

    /// Our kart's 30 last positions: the one at a tick, the newest for a tick to come, nothing
    /// older than 30 ticks.
    #[test]
    fn the_history_keeps_30_ticks() {
        let mut history = PoseHistory::default();
        assert_eq!(history.position_at(5), None);
        for tick in 100..200 {
            history.record(tick, Vec3::new(tick as f32, 0.0, 0.0));
        }
        assert_eq!(history.len(), HISTORY_TICKS);
        assert_eq!(history.position_at(199), Some(Vec3::new(199.0, 0.0, 0.0)));
        assert_eq!(history.position_at(185), Some(Vec3::new(185.0, 0.0, 0.0)));
        assert_eq!(history.position_at(170), Some(Vec3::new(170.0, 0.0, 0.0)));
        assert_eq!(history.position_at(169), None);
        assert_eq!(history.position_at(250), Some(Vec3::new(199.0, 0.0, 0.0)));
        // A tick recorded twice keeps the last position; a missed tick takes the one before.
        history.record(199, Vec3::ONE);
        assert_eq!(history.position_at(199), Some(Vec3::ONE));
        history.record(202, Vec3::Y);
        assert_eq!(history.position_at(201), Some(Vec3::ONE));
        // The clock went back: start over.
        history.record(50, Vec3::Z);
        assert_eq!(history.len(), 1);
        assert_eq!(history.position_at(50), Some(Vec3::Z));
        assert_eq!(history.position_at(49), None);
    }
}
