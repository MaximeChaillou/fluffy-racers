//! Headless scenarios of the kart synchronization (issue #8).
//!
//! - Two whole games (`GamePlugin`, Prairie Pompon in free driving) linked by an in-memory
//!   transport: what one sends, the other receives, through the network simulator or not. The
//!   kart driven in A is seen in B where A drove it (bounded error), smoothly (no jump between
//!   two frames), at 120 frames per second.
//! - Our kart against the kinematic body of a remote kart: a side shunt, a teleport onto it, a
//!   correction into it; and its wheels never stand on it.

use std::time::Duration;

use bevy::time::TimeUpdateStrategy;
use bevy_rapier3d::prelude::{ColliderDisabled, Velocity};

use super::*;
use crate::characters::Roster;
use crate::kart::{Kart, KartInput, KartTuning, KartWheels, PlayerKart, kart_body};
use crate::level::LoadLevel;
use crate::race::{RaceGrid, RaceMode};
use crate::track::{PlayerStart, Surface, TrackChoice, spawn_static_box};
use crate::{SimulationPlugin, TICK_HZ};

const ANA: PeerKey = PeerKey(1 << 100);
const BOB: PeerKey = PeerKey(2 << 100);

fn virtual_ms(app: &App) -> f64 {
    app.world().resource::<Time<Virtual>>().elapsed_secs_f64() * 1_000.0
}

fn local_tick(app: &App) -> u32 {
    (app.world().resource::<Time<Fixed>>().elapsed_secs_f64() * TICK_HZ).round() as u32
}

/// Whatever `from` sent since the last call arrives in `to` now (in memory: the network
/// simulator of `to` decides how late, if at all).
fn transfer(from: &mut App, sender: PeerKey, to: &mut App) {
    let states: Vec<KartSnapshot> = from
        .world_mut()
        .resource_mut::<KartSync>()
        .drain_outgoing()
        .collect();
    let now = virtual_ms(to);
    let tuning = to.world().resource::<NetTuning>().clone();
    let mut sync = to.world_mut().resource_mut::<KartSync>();
    for state in states {
        sync.receive(sender, state, now, &tuning);
    }
}

/// A whole game in a room, Prairie Pompon in free driving from grid slot `slot`, `other` in the
/// room on `other_slot`; 120 frames per second (the fixed 60 Hz tick every other frame). The
/// common clock is the local one (both games start together).
fn online_game(me: PeerKey, slot: usize, other: PeerKey, other_slot: usize) -> App {
    let mut app = crate::tests::game_app_unloaded(TrackChoice::default(), 0);
    let mut sync = KartSync::new(me, slot as KartId, 1);
    sync.clock = TickClock::with_offset(0);
    sync.set_players([(other, [3, 1, 2, 1], other_slot)]);
    app.insert_resource(RaceMode::FreeRoam)
        .insert_resource(OnlineSlot(slot))
        .insert_resource(sync)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 120.0,
        )));
    crate::tests::wait_for_the_player(&mut app);
    app
}

/// What B saw of A's kart, frame by frame.
#[derive(Clone, Copy, Debug)]
struct Seen {
    /// The common tick shown (fractional), and B's own tick then (its visuals' time).
    tick: f64,
    now: f64,
    position: Vec3,
}

/// How a drive of A looked in B.
#[derive(Debug, Default)]
struct Drive {
    /// Frames compared, after the start.
    frames: usize,
    /// Distance between where B showed A's kart and where A's kart was at that tick (m).
    worst_error: f32,
    mean_error: f32,
    /// The largest change of the displacement between two frames (m): a jump shows here...
    worst_jerk: f32,
    /// ...beyond the same change of A's kart at the same ticks (a boost pad, a bump: A's kart
    /// itself jerks there).
    worst_jerk_excess: f32,
    /// The largest displacement in a frame (m).
    worst_step: f32,
    /// Distance between the extrapolated position of A's kart (for a tether) and where it was
    /// at the same tick, at worst (m).
    worst_extrapolation: f32,
    /// Distance A drove (m).
    driven: f32,
    /// How far behind A's kart B showed it, on average (ms).
    mean_delay_ms: f64,
    states: u32,
    lost: u64,
}

/// A drives Prairie Pompon's first straight for `seconds` (full throttle, a light S-curve);
/// B watches. B's network simulator: `netsim`.
fn drive(netsim: NetSimTuning, seconds: f32) -> Drive {
    let mut ana = online_game(ANA, 0, BOB, 1);
    let mut bob = online_game(BOB, 1, ANA, 0);
    bob.world_mut().resource_mut::<NetTuning>().simulator = netsim;
    // What was sent while the tracks loaded is forgotten.
    ana.world_mut()
        .resource_mut::<KartSync>()
        .drain_outgoing()
        .for_each(drop);
    bob.world_mut()
        .resource_mut::<KartSync>()
        .drain_outgoing()
        .for_each(drop);
    let kart = crate::tests::player(&mut ana);
    let start = ana.world().get::<Transform>(kart).unwrap().translation;
    // A's kart, tick by tick; what B shows of it, frame by frame.
    let mut truth: Vec<(u32, Vec3)> = Vec::new();
    let mut seen: Vec<Option<Seen>> = Vec::new();
    let mut extrapolation = 0.0f32;
    let frames = (seconds * 120.0) as usize;
    for frame in 0..frames {
        {
            let mut keys = ana.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ArrowUp);
            let t = frame as f32 / 120.0;
            // A light S-curve, within the road.
            if (2.0..2.2).contains(&t) {
                keys.press(KeyCode::ArrowRight);
            } else {
                keys.release(KeyCode::ArrowRight);
            }
            if (2.6..2.8).contains(&t) {
                keys.press(KeyCode::ArrowLeft);
            } else {
                keys.release(KeyCode::ArrowLeft);
            }
        }
        ana.update();
        bob.update();
        transfer(&mut ana, ANA, &mut bob);
        transfer(&mut bob, BOB, &mut ana);
        let tick = local_tick(&ana);
        let position = ana.world().get::<Transform>(kart).unwrap().translation;

        match truth.last_mut() {
            Some(last) if last.0 == tick => last.1 = position,
            _ => truth.push((tick, position)),
        }
        // What B shows, one tick behind its virtual time (like every kart's visual).
        let sync = bob.world().resource::<KartSync>();
        let remote = sync.remote(ANA).unwrap();
        let shown_at = virtual_ms(&bob) - 1_000.0 / TICK_HZ;
        let view = remote
            .visual
            .zip(remote.shown_tick(shown_at))
            .map(|(visual, tick)| Seen {
                tick,
                now: shown_at / TICK_MS,
                position: bob.world().get::<Transform>(visual).unwrap().translation,
            });
        seen.push(view);
        // Where a tether would pull: A's kart now, extrapolated from its newest state.
        if frame > 240 {
            let now = f64::from(local_tick(&bob));
            let limits = bob.world().resource::<NetTuning>().limits();
            if let Some(at) = remote.position_at(now, &limits) {
                let truth_now = truth.last().unwrap();
                if truth_now.0 == now as u32 {
                    extrapolation = extrapolation.max(at.distance(truth_now.1));
                }
            }
        }
    }
    let truth_at = |tick: f64| -> Option<Vec3> {
        let first = truth.first()?.0;
        let index = tick - f64::from(first);
        let (i, t) = (index.floor() as usize, index.fract() as f32);
        let a = truth.get(i)?;
        let b = truth.get(i + 1).unwrap_or(a);
        Some(a.1.lerp(b.1, t))
    };
    let mut result = Drive {
        worst_extrapolation: extrapolation,
        driven: truth.last().unwrap().1.distance(start),
        ..Default::default()
    };
    let (mut error_sum, mut delay_sum) = (0.0, 0.0);
    // After the first second: the playback has settled.
    for frame in 120..frames {
        let (Some(view), Some(previous), Some(before)) =
            (seen[frame], seen[frame - 1], seen[frame - 2])
        else {
            continue;
        };
        let Some(truth) = truth_at(view.tick) else {
            continue;
        };
        let error = view.position.distance(truth);
        result.worst_error = result.worst_error.max(error);
        error_sum += error;
        let step = view.position - previous.position;
        let jerk = (step - (previous.position - before.position)).length();
        result.worst_jerk = result.worst_jerk.max(jerk);
        // The same at the same ticks, on A's screen: a boost pad or a bump jerks there too.
        if let (Some(t0), Some(t1)) = (truth_at(before.tick), truth_at(previous.tick)) {
            let truth_jerk = ((truth - t1) - (t1 - t0)).length();
            result.worst_jerk_excess = result.worst_jerk_excess.max(jerk - truth_jerk);
        }
        result.worst_step = result.worst_step.max(step.length());
        delay_sum += (view.now - view.tick) * TICK_MS;
        result.frames += 1;
    }
    result.mean_error = error_sum / result.frames.max(1) as f32;
    result.mean_delay_ms = delay_sum / result.frames.max(1) as f64;
    let sync = bob.world().resource::<KartSync>();
    result.states = sync.remote(ANA).unwrap().received;
    result.lost = sync.simulated_losses().1;
    // A's history: its position at a tick, for the victim's check (issue #34).
    let history = ana.world().get::<KartHistory>(kart).expect("no history");
    let (tick, position) = truth[truth.len() - 10];
    assert_eq!(history.0.position_at(tick), Some(position));
    assert_eq!(history.0.len(), HISTORY_TICKS);
    // B shows A's animal and parts, with the color of A's seat.
    let proxy = sync.remote(ANA).unwrap().proxy.unwrap();
    assert!(bob.world().get::<RemoteKartProxy>(proxy).is_some());
    assert!(
        bob.world().get::<Kart>(proxy).is_none(),
        "not a kart that drives"
    );
    result
}

/// Issue #8 acceptance: the kart driven in A is seen in B where it was about 100 ms earlier,
/// within a few centimeters, and without a jump between two frames (120 per second).
#[test]
fn a_kart_driven_in_one_game_drives_smoothly_in_the_other() {
    let drive = drive(NetSimTuning::default(), 4.5);
    println!("clean network: {drive:#?}");
    assert!(drive.driven > 70.0, "A barely drove: {drive:?}");
    assert!(drive.frames > 400, "{drive:?}");
    // A state every 2 ticks: 4.5 s, 135 of them (those of the first frames after the load too).
    assert!(drive.states >= 130, "{drive:?}");
    assert!(drive.worst_error < 0.03, "{drive:?}");
    // At 28 m/s, a frame moves the kart 0.24 m; a jump would be several times that. The
    // remote kart's motion changes from frame to frame no more than A's own kart's does.
    assert!(drive.worst_jerk_excess < 0.01, "{drive:?}");
    assert!(drive.worst_jerk < 0.05, "{drive:?}");
    assert!(drive.worst_step < 0.35, "{drive:?}");
    // 100 ms of buffer plus the transport (one frame).
    assert!((100.0..130.0).contains(&drive.mean_delay_ms), "{drive:?}");
    assert!(drive.worst_extrapolation < 0.1, "{drive:?}");
}

/// Issue #8 acceptance: with 120 ms of latency, ±20 ms of jitter and 5 % of the states lost
/// (`?netsim=120,20,5`), the remote kart stays as smooth, a little later.
#[test]
fn a_remote_kart_stays_smooth_through_a_bad_network() {
    let netsim = NetSimTuning::parse("120,20,5").unwrap();
    // Down the straight and over its boost pad (37 m/s), before the bend.
    let drive = drive(netsim, 5.3);
    println!("120 ms, ±20 ms, 5 %: {drive:#?}");
    assert!(drive.driven > 100.0, "{drive:?}");
    assert!(drive.lost > 0, "{drive:?}");
    assert!(drive.worst_error < 0.03, "{drive:?}");
    assert!(drive.mean_error < 0.01, "{drive:?}");
    assert!(drive.worst_jerk_excess < 0.01, "{drive:?}");
    assert!(drive.worst_step < 0.35, "{drive:?}");
    // The fastest states take 100 ms, and are shown 100 ms later.
    assert!((190.0..250.0).contains(&drive.mean_delay_ms), "{drive:?}");
    // A tether pulls toward A's kart extrapolated over the latency: within a meter.
    assert!(drive.worst_extrapolation < 1.0, "{drive:?}");
}

/// Our kart (`PlayerKart`) on a flat slab, and the kinematic body of a remote kart driven by the
/// states the test makes up.
struct Arena {
    app: App,
    kart: Entity,
}

const REMOTE: PeerKey = PeerKey(7);

impl Arena {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            SimulationPlugin,
            KartSyncPlugin,
        ))
        .add_message::<LoadLevel>()
        .init_resource::<TrackChoice>()
        .init_resource::<RaceMode>()
        .init_resource::<RaceGrid>()
        .insert_resource(Roster::embedded())
        .insert_resource(PlayerStart(Transform::IDENTITY))
        .insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.finish();
        app.cleanup();
        let mut sync = KartSync::new(PeerKey(1), 0, 0);
        sync.clock = TickClock::with_offset(0);
        sync.set_players([(REMOTE, [0, 0, 0, 0], 1)]);
        app.insert_resource(sync);
        let world = app.world_mut();
        let mut commands = world.commands();
        spawn_static_box(
            &mut commands,
            Transform::from_xyz(0.0, -0.5, 0.0),
            Vec3::new(500.0, 0.5, 500.0),
            Surface::Road,
            0.0,
        );
        world.flush();
        let tuning = world.resource::<KartTuning>().clone();
        let kart = world
            .spawn((
                kart_body(&tuning, Transform::from_xyz(0.0, 0.55, 0.0)),
                PlayerKart,
            ))
            .id();
        app.update();
        let mut arena = Self { app, kart };
        arena.run(0.5, |_| None);
        arena
    }

    /// `seconds` of ticks; each one, the remote kart's state at that tick (if any) arrives.
    fn run(&mut self, seconds: f32, mut remote: impl FnMut(u32) -> Option<KartPose>) {
        for _ in 0..(seconds * 60.0).round() as usize {
            let tick = local_tick(&self.app);
            if let Some(mut pose) = remote(tick) {
                pose.tick = tick;
                let now = virtual_ms(&self.app);
                let tuning = self.app.world().resource::<NetTuning>().clone();
                self.app.world_mut().resource_mut::<KartSync>().receive(
                    REMOTE,
                    KartSnapshot::quantize(1, &pose),
                    now,
                    &tuning,
                );
            }
            self.app.update();
        }
    }

    fn velocity(&self) -> Vec3 {
        self.app.world().get::<Velocity>(self.kart).unwrap().linear
    }

    fn transform(&self) -> Transform {
        *self.app.world().get::<Transform>(self.kart).unwrap()
    }

    fn proxy(&self) -> Option<Entity> {
        self.app
            .world()
            .resource::<KartSync>()
            .remote(REMOTE)?
            .proxy
    }
}

/// A remote kart standing at `position`.
fn standing(position: Vec3) -> KartPose {
    KartPose {
        position,
        rotation: Quat::IDENTITY,
        ..Default::default()
    }
}

/// The worst of our kart while `run` goes: (speed, height, uprightness).
fn worst(
    arena: &mut Arena,
    seconds: f32,
    remote: impl FnMut(u32) -> Option<KartPose>,
) -> (f32, f32, f32) {
    let mut remote = remote;
    let (mut speed, mut height, mut up) = (0.0f32, 0.0f32, 1.0f32);
    for _ in 0..(seconds * 60.0) as usize {
        arena.run(1.0 / 60.0, &mut remote);
        let transform = arena.transform();
        speed = speed.max(arena.velocity().length());
        height = height.max(transform.translation.y);
        up = up.min(transform.up().dot(Vec3::Y));
        assert!(transform.translation.is_finite());
    }
    (speed, height, up)
}

/// Issue #8: our kart against the kinematic body of a remote kart. The remote kart drives into
/// its flank at 10 m/s: our kart is pushed away (each peer applies the bump to its own kart), no
/// faster than the remote kart drives, and stays on its wheels. A remote kart whose states jump
/// onto ours (a teleport) goes through it until it is clear; one whose states correct by more
/// than a meter into ours pushes it no faster than the kart's speed plus 5 m/s (its body moves
/// by its velocity, bounded).
#[test]
fn our_kart_bumps_into_a_remote_kart_without_being_thrown() {
    // The side shunt: from 6 m to the left, 10 m/s to the right, for 1 s; then it stops.
    let mut arena = Arena::new();
    let start = local_tick(&arena.app);
    let shunt = move |tick: u32| {
        let t = ((tick - start) as f32 / 60.0).min(1.0);
        let moving = (tick - start) < 60;
        Some(KartPose {
            velocity: if moving { Vec3::X * 10.0 } else { Vec3::ZERO },
            ..standing(Vec3::new(-6.0 + 10.0 * t, 0.55, 0.0))
        })
    };
    let (speed, height, up) = worst(&mut arena, 2.5, shunt);
    let pushed = arena.transform().translation.x;
    println!(
        "shunt: speed {speed:.2} m/s, height {height:.2} m, up {up:.3}, pushed to x = {pushed:.2}"
    );
    assert!(arena.proxy().is_some());
    assert!(pushed > 3.0, "our kart was not pushed: {pushed}");
    assert!(speed < 12.0, "thrown at {speed} m/s");
    assert!(height < 0.9, "lifted to {height} m");
    assert!(up > 0.9, "tilted: {up}");

    // A teleport onto our kart (the tow pelican drops it there): from 30 m away to 0.8 m from
    // its center. It goes through ours until it is clear of it, then it is solid again.
    let mut arena = Arena::new();
    let start = local_tick(&arena.app);
    let teleport = move |tick: u32| {
        let x = match tick - start {
            0..30 => 30.0,
            30..150 => 0.8,
            _ => 6.0,
        };
        Some(standing(Vec3::new(x, 0.55, 0.0)))
    };
    let (speed, height, up) = worst(&mut arena, 2.0, teleport);
    println!("teleport: speed {speed:.2} m/s, height {height:.2} m, up {up:.3}");
    assert!(speed < 0.5, "pushed at {speed} m/s");
    assert!(height < 0.6, "lifted to {height} m");
    let proxy = arena.proxy().unwrap();
    assert!(
        arena.app.world().get::<ColliderDisabled>(proxy).is_some(),
        "solid inside our kart"
    );
    arena.run(1.5, teleport);
    assert!(
        arena.app.world().get::<ColliderDisabled>(proxy).is_none(),
        "never solid again"
    );

    // A correction: standing 3 m to the side (solid), then 2.2 m, then its states say 1 m, into
    // our kart.
    let mut arena = Arena::new();
    let start = local_tick(&arena.app);
    let correction = move |tick: u32| {
        let x = match tick - start {
            0..30 => 3.0,
            30..60 => 2.2,
            _ => 1.0,
        };
        Some(standing(Vec3::new(x, 0.55, 0.0)))
    };
    let (speed, height, up) = worst(&mut arena, 2.0, correction);
    println!("correction: speed {speed:.2} m/s, height {height:.2} m, up {up:.3}");
    assert!(speed > 1.0, "the correction did not push: {speed} m/s");
    assert!(speed < 6.5, "thrown at {speed} m/s");
    assert!(height < 0.9 && up > 0.9, "{height} {up}");
}

/// Issue #8 (and #27): our kart's suspension rays never stand on a remote kart: lowered onto
/// its roof, it rests on its body, wheels in the air.
#[test]
fn a_remote_kart_is_never_ground() {
    let mut arena = Arena::new();
    let under = Vec3::new(0.0, 0.55, -20.0);
    arena.run(0.5, |_| Some(standing(under)));
    let proxy = arena.proxy().expect("no remote body");
    let proxy_at = arena
        .app
        .world()
        .get::<Transform>(proxy)
        .unwrap()
        .translation;
    assert!(proxy_at.distance(under) < 0.01, "{proxy_at}");
    {
        let world = arena.app.world_mut();
        world.get_mut::<Transform>(arena.kart).unwrap().translation = under + Vec3::Y * 0.6;
        *world.get_mut::<Velocity>(arena.kart).unwrap() = Velocity::zero();
    }
    for _ in 0..20 {
        arena.run(1.0 / 60.0, |_| Some(standing(under)));
        let wheels = *arena.app.world().get::<KartWheels>(arena.kart).unwrap();
        assert_eq!(wheels.wheels_on_ground(), 0, "{wheels:?}");
    }
    assert!(arena.transform().translation.y > 0.9, "fell through");
    // The remote kart was not pushed down (kinematic): still where its states say.
    let proxy_at = arena
        .app
        .world()
        .get::<Transform>(proxy)
        .unwrap()
        .translation;
    assert!(proxy_at.distance(under) < 0.01, "{proxy_at}");
    // Nothing drives it: our driving systems leave its input alone.
    assert!(arena.app.world().get::<KartInput>(proxy).is_none());
}

/// The states of a peer that left are dropped with its kart; a peer unknown is ignored.
#[test]
fn a_peer_that_leaves_takes_its_kart_away() {
    let mut arena = Arena::new();
    arena.run(0.3, |_| Some(standing(Vec3::new(5.0, 0.55, 0.0))));
    let proxy = arena.proxy().expect("no remote body");
    let bodies = |app: &mut App| {
        app.world_mut()
            .query::<&RemoteKartProxy>()
            .iter(app.world())
            .count()
    };
    assert_eq!(bodies(&mut arena.app), 1);
    {
        let tuning = arena.app.world().resource::<NetTuning>().clone();
        let mut sync = arena.app.world_mut().resource_mut::<KartSync>();
        // A stranger's state: no kart for it.
        sync.receive(PeerKey(99), KartSnapshot::default(), 0.0, &tuning);
        assert!(sync.remote(PeerKey(99)).is_none());
        sync.set_players([]);
        assert!(sync.remotes().is_empty());
    }
    arena.run(0.1, |_| None);
    assert!(arena.app.world().get_entity(proxy).is_err(), "still there");
    assert_eq!(bodies(&mut arena.app), 0);
}

/// A remote kart whose clock jumps back (a new host, with an earlier clock): its buffer and its
/// playback start over, and it is interpolated again at once, not frozen while the old lag
/// fades out of its window.
#[test]
fn a_remote_clock_that_jumps_back_starts_over() {
    let mut arena = Arena::new();
    let feed = |arena: &mut Arena, shift: i64, x: f32| {
        let tick = (i64::from(local_tick(&arena.app)) + shift) as u32;
        let pose = KartPose {
            tick,
            velocity: Vec3::X * 6.0,
            ..standing(Vec3::new(x, 0.55, 30.0))
        };
        let now = virtual_ms(&arena.app);
        let tuning = arena.app.world().resource::<NetTuning>().clone();
        arena.app.world_mut().resource_mut::<KartSync>().receive(
            REMOTE,
            KartSnapshot::quantize(1, &pose),
            now,
            &tuning,
        );
        arena.app.update();
    };
    let mut x = 0.0;
    for _ in 0..60 {
        x += 0.1;
        feed(&mut arena, 1_000, x);
    }
    let kind = |arena: &Arena| {
        let sync = arena.app.world().resource::<KartSync>();
        let remote = sync.remote(REMOTE).unwrap();
        let limits = arena.app.world().resource::<NetTuning>().limits();
        let tick = remote.shown_tick(virtual_ms(&arena.app)).unwrap();
        remote.buffer.sample(tick, &limits).unwrap().kind
    };
    assert_eq!(kind(&arena), SampleKind::Interpolated);
    // 10 s back.
    for _ in 0..20 {
        x += 0.1;
        feed(&mut arena, 1_000 - 600, x);
    }
    assert_eq!(kind(&arena), SampleKind::Interpolated);
    let sync = arena.app.world().resource::<KartSync>();
    assert!(sync.remote(REMOTE).unwrap().buffer.len() <= 20);
}

/// Issue #9: the remote karts are kept by network id, with the peer that has authority over each
/// one. A state is taken only from that peer, of the current session; a kart whose peer changes
/// (a new host takes the bots) starts its buffer over; a player who left keeps its kart, marked;
/// a new session (another race, free driving) drops every remote kart.
#[test]
fn the_roster_keeps_the_karts_by_id_and_authority() {
    const CYD: PeerKey = PeerKey(3 << 100);
    let tuning = NetTuning::default();
    let mut sync = KartSync::new(BOB, 5, 0);
    sync.set_session(7);
    let entry = |kart: KartId, peer: PeerKey| RosterEntry {
        kart,
        peer,
        loadout: Some([1, 0, 0, 0]),
        color: usize::from(kart),
        name: None,
        racer: true,
        left: false,
    };
    // Bots 0 and 1 are Ana's (she hosts), kart 4 is Ana's own.
    sync.set_roster([entry(0, ANA), entry(1, ANA), entry(4, ANA)]);
    assert_eq!(sync.remotes().len(), 3);
    let state = |kart: KartId, session: u8, tick: u32| {
        let pose = KartPose {
            tick,
            ..standing(Vec3::new(f32::from(kart), 0.55, 0.0))
        };
        KartSnapshot {
            session,
            ..KartSnapshot::quantize(kart, &pose)
        }
    };
    sync.receive(ANA, state(0, 7, 10), 0.0, &tuning);
    sync.receive(CYD, state(1, 7, 10), 0.0, &tuning);
    sync.receive(ANA, state(4, 0, 10), 0.0, &tuning);
    assert_eq!(sync.remote_kart(0).unwrap().received, 1);
    assert_eq!(sync.remote_kart(1).unwrap().received, 0, "not Cyd's kart");
    assert_eq!(sync.remote_kart(4).unwrap().received, 0, "another session");
    // Ana leaves: Cyd hosts, the bots are his; Ana's kart is kept, marked.
    let mut left = entry(4, ANA);
    left.left = true;
    left.loadout = None;
    sync.set_roster([entry(0, CYD), entry(1, CYD), left]);
    let bot = sync.remote_kart(0).unwrap();
    assert_eq!(bot.peer, CYD);
    assert!(bot.buffer.is_empty(), "its buffer starts over");
    sync.receive(ANA, state(0, 7, 12), 0.0, &tuning);
    sync.receive(CYD, state(0, 7, 12), 0.0, &tuning);
    assert_eq!(sync.remote_kart(0).unwrap().received, 2);
    assert_eq!(sync.remote_kart(0).unwrap().buffer.len(), 1);
    let ana = sync.remote_kart(4).unwrap();
    assert!(ana.left);
    assert_eq!(ana.loadout, [1, 0, 0, 0], "its parts are kept");
    // Our own states carry the session.
    sync.send(state(5, 0, 20));
    assert_eq!(sync.drain_outgoing().next().unwrap().session, 7);
    // A new session: every remote kart goes.
    sync.set_session(0);
    assert!(sync.remotes().is_empty());
}
