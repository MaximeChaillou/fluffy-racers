//! Network, part 2 of 3 (issue #8, `docs/ARCHITECTURE.md` § « Synchronisation »): the other
//! players' karts drive on our screen, smoothly.
//!
//! - **Online free driving**: once seated in a room, everyone drives freely on Prairie Pompon
//!   (`RaceMode::FreeRoam`: no bots, no countdown), each from the grid slot of their seat
//!   ([`OnlineSlot`], once the seat order is everyone's). Leaving the room brings back the level
//!   we were on. The real race comes
//!   with issue #9. Without a room nothing here runs: solo is unchanged.
//! - **Our kart** (authority): after each tick its position goes into a history of 30 ticks
//!   ([`KartHistory`], for the victim's check of issue #34), and every second tick its state
//!   ([`KartSnapshot`], 47 bytes) goes to every peer, on the unreliable channel. Its tick is the
//!   room's common tick ([`TickClock`], from the host's clock).
//! - **The others' karts** ([`RemoteKart`]): their states fill a buffer each
//!   ([`SnapshotBuffer`]), shown about 100 ms after they arrive ([`Playback`]): interpolated,
//!   extrapolated for 250 ms at most, teleported across a jump. Each one is a **kinematic**
//!   Rapier body with the same collider as ours ([`RemoteKartProxy`]: our kart bumps into it, it
//!   is never pushed, and our suspension rays never take it for ground) and the same visual as
//!   any kart, with its player's animal and parts (`Hello`) and the color of its seat. The body
//!   follows the buffer each tick through its velocity (never faster than the kart drives, plus
//!   a margin: a correction never throws our kart around); put in place at once across a
//!   teleport, it goes through our karts until it is clear of them. The visual follows the
//!   buffer each frame.
//! - **Bumps**: each peer's physics pushes its own kart away from the others' bodies. The other
//!   peer sees its own kart pushed by ours: approximately symmetric, fun before exactness.
//! - **Network simulator** (debug): latency, jitter and losses on the states received
//!   (`NetTuning.simulator`, `?netsim=120,20,5`, `netsim.rs`).
//!
//! The headless tests link two games through an in-memory transport ([`KartSync::send`],
//! [`KartSync::drain_outgoing`], [`KartSync::receive`]); the socket does the same in
//! `poll_session`.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{
    CoefficientCombineRule, ColliderDisabled, Friction, PhysicsSet, Restitution, RigidBody,
    Sleeping, Velocity,
};

use super::interp::{
    Insert, Playback, PoseHistory, SampleKind, SampleLimits, SnapshotBuffer, TICK_MS, TickClock,
};
use super::netsim::NetSim;
use super::protocol::{KartId, KartSnapshot, PeerKey};
use super::snapshot::{KartLook, KartPose, LookParts};
use super::tuning::NetTuning;
use super::{NetSession, NetStatus, NetSystems, STUCK_MS, now_ms};
use crate::bots::bot_color;
use crate::characters::{DriverLooks, Loadout, Roster};
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{
    Boost, DriftMotion, Invincibility, Kart, KartAssists, KartState, KartSystems, KartTuning,
    KartVisualAssets, KartWheels, PlayerKart, Slipstream, Stall, StatEffects, StatsTuning,
    kart_collider, spawn_kart_visual_with_driver,
};
use crate::level::{LevelScoped, LoadLevel};
use crate::race::{RaceGrid, RaceMode};
use crate::track::{GRID_SLOTS, PlayerStart, TrackChoice};

/// Our states waiting to be sent, at most (the socket takes them every frame).
const MAX_OUTGOING: usize = 8;

/// Where our kart starts online: the grid slot of our seat (`crate::spawn_player` reads it).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnlineSlot(pub usize);

/// We drive online: the level we were on before, to get back to it when we leave the room.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct OnlineLevel {
    /// Our id in the room we entered.
    pub me: PeerKey,
    pub solo_track: TrackChoice,
    pub solo_mode: RaceMode,
}

/// Our kart's positions of the last 30 ticks, by common tick (issue #34: the victim compares
/// its position at the tick the striker saw).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KartHistory(pub PoseHistory);

/// The body of another peer's kart: kinematic, the collider of a kart, the look its states give
/// (`KartState`, `Boost`... only for its visual: no driving system touches it).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteKartProxy {
    pub peer: PeerKey,
}

/// A kart another peer drives, as we know it.
#[derive(Clone, Debug)]
pub struct RemoteKart {
    pub peer: PeerKey,
    /// Its four parts (its `Hello`): its animal on screen.
    pub loadout: [u8; 4],
    /// Its seat: the color of its kart.
    pub slot: usize,
    /// The id it gives its kart on the wire (its grid slot).
    pub kart: Option<KartId>,
    pub buffer: SnapshotBuffer,
    pub playback: Playback,
    /// Its body and its visual, once spawned (a new level despawns them: spawned again).
    pub proxy: Option<Entity>,
    pub visual: Option<Entity>,
    /// States received (the simulator's losses not counted).
    pub received: u32,
    /// Its parts changed: spawned again.
    respawn: bool,
    /// Still in the lobby at the last update.
    present: bool,
}

impl RemoteKart {
    fn new(peer: PeerKey, loadout: [u8; 4], slot: usize) -> Self {
        Self {
            peer,
            loadout,
            slot,
            kart: None,
            buffer: SnapshotBuffer::default(),
            playback: Playback::default(),
            proxy: None,
            visual: None,
            received: 0,
            respawn: false,
            present: true,
        }
    }

    /// The common tick shown at `now_ms` (local virtual time), once a state arrived: the tick a
    /// striker saw (`KartStruck::seen`, issue #34).
    pub fn shown_tick(&self, now_ms: f64) -> Option<f64> {
        self.playback.tick_at(now_ms)
    }

    /// Where the kart is at common tick `tick`, extrapolated from its newest state (bounded):
    /// what a tether pulls toward (issue #34, `docs/ARCHITECTURE.md` « Attrape »), not the pose
    /// shown 100 ms in the past.
    pub fn position_at(&self, tick: f64, limits: &SampleLimits) -> Option<Vec3> {
        self.buffer
            .sample(tick, limits)
            .map(|sample| sample.pose.position)
    }
}

/// The kart synchronization of a peer in a room (see the module's documentation). Exists only
/// online: inserted once seated.
#[derive(Resource)]
pub struct KartSync {
    me: PeerKey,
    kart: KartId,
    /// The room's common tick of our local ticks.
    pub clock: TickClock,
    outgoing: Vec<KartSnapshot>,
    remotes: Vec<RemoteKart>,
    sim: NetSim<(PeerKey, KartSnapshot)>,
    /// Bodies and visuals of karts gone, to despawn.
    gone: Vec<Entity>,
}

impl KartSync {
    /// We are `me`, our kart is `kart` on the wire; the network simulator's draws come from
    /// `seed`.
    pub fn new(me: PeerKey, kart: KartId, seed: u64) -> Self {
        Self {
            me,
            kart,
            clock: TickClock::default(),
            outgoing: Vec::with_capacity(MAX_OUTGOING),
            remotes: Vec::with_capacity(GRID_SLOTS),
            sim: NetSim::new(seed ^ me.0 as u64 ^ (me.0 >> 64) as u64),
            gone: Vec::new(),
        }
    }

    pub fn me(&self) -> PeerKey {
        self.me
    }

    /// Our kart's id on the wire.
    pub fn kart(&self) -> KartId {
        self.kart
    }

    pub fn remotes(&self) -> &[RemoteKart] {
        &self.remotes
    }

    pub fn remote(&self, peer: PeerKey) -> Option<&RemoteKart> {
        self.remotes.iter().find(|remote| remote.peer == peer)
    }

    /// The other players of the room: their id, their parts and their seat. Those not listed
    /// any more are gone (their kart disappears).
    pub fn set_players(&mut self, players: impl IntoIterator<Item = (PeerKey, [u8; 4], usize)>) {
        for remote in &mut self.remotes {
            remote.present = false;
        }
        for (peer, loadout, slot) in players {
            if peer == self.me {
                continue;
            }
            match self.remotes.iter_mut().find(|remote| remote.peer == peer) {
                Some(remote) => {
                    remote.present = true;
                    if remote.loadout != loadout || remote.slot != slot {
                        remote.loadout = loadout;
                        remote.slot = slot;
                        remote.respawn = true;
                    }
                }
                None => self.remotes.push(RemoteKart::new(peer, loadout, slot)),
            }
        }
        let gone = &mut self.gone;
        self.remotes.retain(|remote| {
            if !remote.present {
                gone.extend(remote.proxy.iter().chain(&remote.visual));
            }
            remote.present
        });
    }

    /// A state from `from` arrived at `now_ms` (local virtual time): into its kart's buffer,
    /// through the network simulator when it is on.
    pub fn receive(&mut self, from: PeerKey, state: KartSnapshot, now_ms: f64, tuning: &NetTuning) {
        if tuning.simulator.active() {
            self.sim.push(now_ms, (from, state), &tuning.simulator);
        } else {
            deliver(&mut self.remotes, from, state, now_ms, tuning);
        }
    }

    /// Once a frame, at `now_ms` (local virtual time), `dt_ms` after the last one: what the
    /// simulator held back arrives, the playbacks move on.
    pub fn update(&mut self, now_ms: f64, dt_ms: f64, tuning: &NetTuning) {
        let remotes = &mut self.remotes;
        self.sim.release(now_ms, |(from, state)| {
            deliver(remotes, from, state, now_ms, tuning)
        });
        let settings = tuning.playback();
        for remote in &mut self.remotes {
            remote.playback.advance(now_ms, dt_ms, &settings);
        }
    }

    /// Our kart's state, to send to every peer (the few newest, if nobody takes them).
    pub fn send(&mut self, state: KartSnapshot) {
        if self.outgoing.len() >= MAX_OUTGOING {
            self.outgoing.remove(0);
        }
        self.outgoing.push(state);
    }

    /// The states to send, oldest first.
    pub fn drain_outgoing(&mut self) -> std::vec::Drain<'_, KartSnapshot> {
        self.outgoing.drain(..)
    }

    /// The states the simulator holds back.
    pub fn simulated_pending(&self) -> usize {
        self.sim.pending()
    }

    /// The states the simulator received and lost.
    pub fn simulated_losses(&self) -> (u64, u64) {
        (self.sim.received, self.sim.lost)
    }
}

/// A state from `from` into its kart's buffer (a peer we do not know: dropped).
fn deliver(
    remotes: &mut [RemoteKart],
    from: PeerKey,
    state: KartSnapshot,
    now_ms: f64,
    tuning: &NetTuning,
) {
    let Some(remote) = remotes.iter_mut().find(|remote| remote.peer == from) else {
        return;
    };
    remote.received += 1;
    remote.kart = Some(state.kart);
    if remote.buffer.insert(state.pose()) == Insert::Restarted {
        // Its clock jumped back (a new host): its lag starts over too.
        remote.playback = Playback::default();
    }
    remote
        .playback
        .arrived(now_ms, state.tick, f64::from(tuning.lag_window_ms.max(1.0)));
}

/// The kart synchronization (issue #8). Its systems run only with a [`KartSync`] (online), and
/// the switch to online driving only with a room: solo is unchanged.
pub struct KartSyncPlugin;

impl Plugin for KartSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetTuning>()
            .add_systems(
                Update,
                (
                    online_mode.run_if(
                        |session: Option<Res<NetSession>>, online: Option<Res<OnlineLevel>>| {
                            session.is_some() || online.is_some()
                        },
                    ),
                    (update_remote_karts, spawn_remote_karts, report_karts)
                        .chain()
                        .run_if(resource_exists::<KartSync>),
                )
                    .chain()
                    .after(NetSystems),
            )
            .add_systems(
                FixedUpdate,
                move_remote_karts
                    .run_if(resource_exists::<KartSync>)
                    .after(KartSystems::Input)
                    .before(PhysicsSet::SyncBackend),
            )
            .add_systems(
                FixedLast,
                record_our_kart.run_if(resource_exists::<KartSync>),
            )
            .add_systems(
                PostUpdate,
                show_remote_karts
                    .run_if(resource_exists::<KartSync>)
                    .in_set(InterpolationSystems),
            );
    }
}

/// The local virtual time (ms): the clock of the playbacks. It stops while the game is paused
/// (a menu), like the karts.
fn virtual_ms(time: &Time<Virtual>) -> f64 {
    time.elapsed_secs_f64() * 1_000.0
}

/// Seated in a room: online free driving, on Prairie Pompon from our seat's grid slot (once the
/// seat order is everyone's: `Lobby::seats_agreed`); out of it: back to the level we were on. In the room, the players and the common clock are passed
/// to the synchronization.
#[allow(clippy::too_many_arguments)]
fn online_mode(
    mut commands: Commands,
    session: Option<Res<NetSession>>,
    online: Option<Res<OnlineLevel>>,
    sync: Option<ResMut<KartSync>>,
    choice: Res<TrackChoice>,
    mode: Res<RaceMode>,
    grid: Res<RaceGrid>,
    real: Res<Time<Real>>,
    time: Res<Time<Virtual>>,
    mut levels: MessageWriter<LoadLevel>,
) {
    let lobby = session
        .as_deref()
        .filter(|session| session.status == NetStatus::InRoom)
        .and_then(|session| session.lobby.as_ref())
        .filter(|lobby| lobby.seated());
    // Our place on the grid: once everyone shares the seat order, and the peers being connected
    // had their chance to sit with us (given up on after 10 s, like in the lobby). A link whose
    // peer the lobby already knows is not waited for: natively, a note of the signaling can come
    // after the connection and leave a stale link behind.
    let now = now_ms(&real);
    let connecting = session.as_deref().is_some_and(|session| {
        session.links.pending().iter().any(|link| {
            now.saturating_sub(link.since_ms) < STUCK_MS
                && lobby.is_none_or(|lobby| lobby.peer(link.peer).is_none())
        })
    });

    let agreed = lobby.is_some_and(|lobby| lobby.seats_agreed()) && !connecting;
    match (online.as_deref(), lobby) {
        (Some(online), lobby) if lobby.is_none_or(|lobby| lobby.me() != online.me) => {
            info!("network: online driving ends, back to the level before");
            if let Some(sync) = &sync {
                for remote in sync.remotes() {
                    for entity in remote.proxy.iter().chain(&remote.visual) {
                        commands.entity(*entity).try_despawn();
                    }
                }
            }
            commands.remove_resource::<KartSync>();
            commands.remove_resource::<OnlineSlot>();
            commands.remove_resource::<OnlineLevel>();
            levels.write(LoadLevel::on(online.solo_track.clone(), online.solo_mode));
        }
        (None, Some(lobby)) if agreed => {
            let me = lobby.me();
            let slot = lobby
                .seats()
                .iter()
                .position(|key| *key == me)
                .unwrap_or(0)
                .min(GRID_SLOTS - 1);
            info!("network: online driving, from grid slot {slot}");
            commands.insert_resource(KartSync::new(me, slot as KartId, grid.seed));
            commands.insert_resource(OnlineSlot(slot));
            commands.insert_resource(OnlineLevel {
                me,
                solo_track: choice.clone(),
                solo_mode: *mode,
            });
            levels.write(LoadLevel::on(TrackChoice::default(), RaceMode::FreeRoam));
        }
        (Some(_), Some(lobby)) => {
            let Some(mut sync) = sync else {
                return;
            };
            sync.set_players(lobby.seats().iter().enumerate().filter_map(|(slot, key)| {
                let profile = lobby.peer(*key)?.profile.as_ref()?;
                Some((*key, profile.loadout, slot))
            }));
            if let Some(host_ms) = lobby.host_time_ms(now) {
                // The host's clock and ours now, in ticks: the common tick of our local ticks.
                sync.clock
                    .observe((host_ms as f64 - virtual_ms(&time)) / TICK_MS);
            }
        }
        _ => {}
    }
}

/// The simulator's states arrive, the playbacks move on (once a frame).
fn update_remote_karts(
    mut sync: ResMut<KartSync>,
    tuning: Res<NetTuning>,
    time: Res<Time<Virtual>>,
) {
    sync.update(virtual_ms(&time), time.delta_secs_f64() * 1_000.0, &tuning);
}

/// Every few seconds online, in the console: where our kart is, and where each remote kart is
/// shown, with its states received (to compare two screens, and for Max's reports).
fn report_karts(
    sync: Res<KartSync>,
    time: Res<Time<Virtual>>,
    ours: Query<&Transform, With<PlayerKart>>,
    bodies: Query<&Transform, With<RemoteKartProxy>>,
    mut next: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now < *next {
        return;
    }
    *next = now + REPORT_SECONDS;
    let tick = sync
        .clock
        .common((now * crate::TICK_HZ).round() as u32)
        .unwrap_or(0);
    if let Ok(transform) = ours.single() {
        let at = transform.translation;
        info!(
            "network: our kart at ({:.1}, {:.1}, {:.1}), tick {tick}",
            at.x, at.y, at.z
        );
    }
    for remote in sync.remotes() {
        let shown = remote.proxy.and_then(|proxy| bodies.get(proxy).ok());
        let newest = remote.buffer.newest().map(|pose| pose.tick);
        match shown {
            Some(transform) => {
                let at = transform.translation;
                info!(
                    "network: kart of #{} at ({:.1}, {:.1}, {:.1}), tick {:.0}; {} states, the newest of tick {}",
                    remote.peer.tag(),
                    at.x,
                    at.y,
                    at.z,
                    remote.shown_tick(now * 1_000.0).unwrap_or(0.0),
                    remote.received,
                    newest.unwrap_or(0)
                );
            }
            None => info!(
                "network: kart of #{}: not shown yet, {} states",
                remote.peer.tag(),
                remote.received
            ),
        }
    }
}

/// The console's report of the karts online, this often (s of game time).
const REPORT_SECONDS: f64 = 3.0;

/// Spawns the body and the visual of each remote kart once its first state arrived and the level
/// is built; again after a new level, or when its player changed parts; despawns those gone.
#[allow(clippy::too_many_arguments)]
fn spawn_remote_karts(
    mut commands: Commands,
    mut sync: ResMut<KartSync>,
    start: Option<Res<PlayerStart>>,
    proxies: Query<(), With<RemoteKartProxy>>,
    roster: Res<Roster>,
    stats_tuning: Res<StatsTuning>,
    looks: Option<Res<DriverLooks>>,
    visuals: Option<Res<KartVisualAssets>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    tuning: Res<NetTuning>,
    time: Res<Time<Virtual>>,
) {
    let (now, limits) = (virtual_ms(&time), tuning.limits());
    for entity in sync.gone.drain(..) {
        commands.entity(entity).try_despawn();
    }
    if start.is_none() {
        // The level is being built.
        return;
    }
    for remote in &mut sync.remotes {
        let alive = remote.proxy.is_some_and(|proxy| proxies.contains(proxy));
        if alive && !remote.respawn {
            continue;
        }
        for entity in remote
            .proxy
            .take()
            .iter()
            .chain(remote.visual.take().iter())
        {
            commands.entity(*entity).try_despawn();
        }
        let Some(shown) = remote
            .playback
            .tick_at(now)
            .and_then(|tick| remote.buffer.sample(tick, &limits))
            .map(|sample| sample.pose)
            .or_else(|| remote.buffer.newest().copied())
        else {
            continue;
        };
        remote.respawn = false;
        let [character, kart, wheels, wing] = remote.loadout.map(usize::from);
        let loadout = Loadout {
            character,
            kart,
            wheels,
            wing,
        };
        let effects = StatEffects::new(&roster.stats(&loadout), &stats_tuning);
        let transform = Transform::from_translation(shown.position).with_rotation(shown.rotation);
        let proxy = commands
            .spawn((
                Name::new(format!("En ligne #{}", remote.peer.tag())),
                RemoteKartProxy { peer: remote.peer },
                LevelScoped,
                transform,
                remote_body(),
                // Solid once clear of our karts (`move_remote_karts`).
                ColliderDisabled,
                (
                    KartState::default(),
                    DriftMotion::default(),
                    Boost::default(),
                    Stall::default(),
                    Invincibility::default(),
                    Slipstream::default(),
                    KartWheels::default(),
                    KartAssists::default(),
                    effects,
                ),
            ))
            .id();
        remote.proxy = Some(proxy);
        if let (Some(visuals), Some(materials)) = (&visuals, materials.as_mut()) {
            let driver = looks.as_ref().and_then(|looks| looks.get(character));
            remote.visual = Some(spawn_kart_visual_with_driver(
                &mut commands,
                visuals,
                materials,
                proxy,
                bot_color(remote.slot),
                driver,
            ));
        }
    }
}

/// The physics of a remote kart: kinematic, moved by its velocity, the collider of a kart, the
/// same friction and bounce as ours.
pub fn remote_body() -> impl Bundle {
    (
        RigidBody::KinematicVelocityBased,
        kart_collider(),
        Velocity::default(),
        Friction {
            coefficient: 0.0,
            combine_rule: CoefficientCombineRule::Min,
        },
        Restitution {
            coefficient: 0.0,
            combine_rule: CoefficientCombineRule::Max,
        },
        Sleeping::disabled(),
    )
}

/// A remote kart's body never turns faster than this (rad/s; a drift turns at about 2).
const MAX_PROXY_TURN_RATE: f32 = 20.0;

/// Two karts whose centers are this far apart cannot touch (m): their boxes' half diagonals are
/// 1.22 m.
const GHOST_CLEARANCE: f32 = 2.5;

/// Before each physics step: each remote kart's body heads for where its buffer says it is at
/// the end of the step, through its velocity (bounded: the speed of its state plus a margin), or
/// is put there at once across a teleport or a big gap. Put there at once (or just spawned), it
/// goes through our karts (`ColliderDisabled`) until it is clear of them, like a kart the tow
/// pelican drops: it never lands inside ours.
fn move_remote_karts(
    mut commands: Commands,
    sync: Res<KartSync>,
    tuning: Res<NetTuning>,
    time: Res<Time<Fixed>>,
    karts: Query<&Transform, (With<Kart>, Without<RemoteKartProxy>)>,
    mut proxies: Query<
        (&mut Transform, &mut Velocity, Has<ColliderDisabled>),
        With<RemoteKartProxy>,
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    // The fixed clock is the virtual one, at the end of this step.
    let now = time.elapsed_secs_f64() * 1_000.0;
    let limits = tuning.limits();
    for remote in sync.remotes() {
        let Some(proxy) = remote.proxy else {
            continue;
        };
        let Ok((mut transform, mut velocity, ghost)) = proxies.get_mut(proxy) else {
            continue;
        };
        let Some(sample) = remote
            .playback
            .tick_at(now)
            .and_then(|tick| remote.buffer.sample(tick, &limits))
        else {
            continue;
        };
        let target = sample.pose;
        let gap = target.position - transform.translation;
        let still = matches!(sample.kind, SampleKind::Frozen | SampleKind::Oldest);
        if sample.kind == SampleKind::Teleport || gap.length() > tuning.proxy_snap_distance {
            transform.translation = target.position;
            transform.rotation = target.rotation;
            *velocity = Velocity::zero();
            if !ghost {
                commands.entity(proxy).insert(ColliderDisabled);
            }
            continue;
        }
        if ghost
            && karts
                .iter()
                .all(|kart| kart.translation.distance(transform.translation) > GHOST_CLEARANCE)
        {
            commands.entity(proxy).remove::<ColliderDisabled>();
        }
        let max_speed = if still {
            tuning.proxy_speed_margin
        } else {
            target.velocity.length() + tuning.proxy_speed_margin
        };
        let mut turn = target.rotation * transform.rotation.inverse();
        if turn.w < 0.0 {
            turn = -turn;
        }
        let linear = (gap / dt).clamp_length_max(max_speed);
        let angular = (turn.to_scaled_axis() / dt).clamp_length_max(MAX_PROXY_TURN_RATE);
        if velocity.linear != linear || velocity.angular != angular {
            velocity.linear = linear;
            velocity.angular = angular;
        }
    }
}

/// After each tick: our kart's position into its history, and every few ticks its state to
/// send, stamped with the common tick (nothing before the clock is set).
#[allow(clippy::type_complexity)]
fn record_our_kart(
    mut commands: Commands,
    mut sync: ResMut<KartSync>,
    tuning: Res<NetTuning>,
    kart_tuning: Res<KartTuning>,
    time: Res<Time<Fixed>>,
    mut karts: Query<
        (
            Entity,
            (&Transform, &Velocity),
            (&KartState, &KartAssists, &StatEffects, &DriftMotion),
            (&Boost, &Stall, &Invincibility, &Slipstream, &KartWheels),
            Option<&mut KartHistory>,
        ),
        With<PlayerKart>,
    >,
) {
    let local = (time.elapsed_secs_f64() * crate::TICK_HZ).round() as u32;
    let Some(tick) = sync.clock.common(local) else {
        return;
    };
    for (
        entity,
        (transform, velocity),
        (state, assists, effects, motion),
        (boost, stall, invincibility, slipstream, wheels),
        history,
    ) in &mut karts
    {
        match history {
            Some(mut history) => history.0.record(tick, transform.translation),
            None => {
                let mut history = PoseHistory::default();
                history.record(tick, transform.translation);
                commands.entity(entity).insert(KartHistory(history));
            }
        }
        if !local.is_multiple_of(tuning.send_interval_ticks.max(1)) {
            continue;
        }
        let own = effects.tuning(&kart_tuning);
        let look = KartLook::of(&LookParts {
            state,
            sparks: state.mini_turbo(&own, assists.assisted_steering),
            motion,
            boost,
            stall,
            invincibility,
            slipstream,
            slipstream_fill: slipstream.fill(&own),
            wheels,
        });
        let pose = KartPose {
            tick,
            position: transform.translation,
            rotation: transform.rotation,
            velocity: velocity.linear,
            spin: velocity.angular,
            look,
        };
        let kart = sync.kart();
        sync.send(KartSnapshot::quantize(kart, &pose));
    }
}

/// Each frame: each remote kart's visual where its buffer says, one tick behind like our karts'
/// visuals (`crate::interpolation`), and its body's look (drift, sparks, flames, hit...) for the
/// kart visuals.
#[allow(clippy::type_complexity)]
fn show_remote_karts(
    sync: Res<KartSync>,
    tuning: Res<NetTuning>,
    kart_tuning: Res<KartTuning>,
    time: Res<Time<Virtual>>,
    fixed: Res<Time<Fixed>>,
    mut bodies: Query<
        (
            (&mut KartState, &mut DriftMotion, &mut Boost),
            (
                &mut Stall,
                &mut Invincibility,
                &mut Slipstream,
                &mut KartWheels,
            ),
            &StatEffects,
        ),
        With<RemoteKartProxy>,
    >,
    mut visuals: Query<&mut Transform, (With<InterpolatedFrom>, Without<RemoteKartProxy>)>,
) {
    let now = virtual_ms(&time) - fixed.timestep().as_secs_f64() * 1_000.0;
    let limits = tuning.limits();
    for remote in sync.remotes() {
        let (Some(proxy), Some(sample)) = (
            remote.proxy,
            remote
                .playback
                .tick_at(now)
                .and_then(|tick| remote.buffer.sample(tick, &limits)),
        ) else {
            continue;
        };
        let pose = sample.pose;
        if let Some(mut transform) = remote.visual.and_then(|v| visuals.get_mut(v).ok()) {
            let shown = Transform::from_translation(pose.position).with_rotation(pose.rotation);
            transform.set_if_neq(shown);
        }
        let Ok((
            (mut state, mut motion, mut boost),
            (mut stall, mut invincibility, mut slipstream, mut wheels),
            effects,
        )) = bodies.get_mut(proxy)
        else {
            continue;
        };
        let look = pose.look;
        let own = effects.tuning(&kart_tuning);
        state.set_if_neq(look.kart_state(&own));
        let drift = look.kart_state(&own);
        motion.set_if_neq(DriftMotion {
            side: match drift {
                KartState::Drifting { side, .. } => Some(side),
                _ => None,
            },
            drifting: matches!(drift, KartState::Drifting { .. }),
            slip: look.slip,
            previous_slip: look.slip,
            ..*motion
        });
        boost.set_if_neq(look.boost());
        stall.set_if_neq(Stall {
            remaining: if look.stalled { 1.0 } else { 0.0 },
        });
        invincibility.set_if_neq(Invincibility {
            remaining: if look.invincible { 1.0 } else { 0.0 },
        });
        slipstream.set_if_neq(Slipstream {
            charge: look.slipstream.unwrap_or(0.0) * own.slipstream_charge_time,
            charging: look.slipstream.is_some(),
        });
        let shown_wheels = look.kart_wheels();
        if wheels.on_ground != shown_wheels.on_ground
            || wheels.compression != shown_wheels.compression
        {
            *wheels = shown_wheels;
        }
    }
}
