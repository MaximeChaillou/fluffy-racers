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

use super::NetSystems;
use super::interp::{
    Insert, Playback, PoseHistory, SampleKind, SampleLimits, SnapshotBuffer, TickClock,
};
use super::netsim::NetSim;
use super::protocol::{KartId, KartSnapshot, Message, PeerKey};
use super::snapshot::{KartLook, KartPose, LookParts};
use super::tuning::NetTuning;
use crate::bots::bot_color;
use crate::characters::{DriverLooks, Loadout, Roster};
use crate::interactions::Paw;
use crate::interpolation::{InterpolatedFrom, InterpolationSystems};
use crate::kart::{
    Boost, DriftMotion, Invincibility, Kart, KartAssists, KartState, KartSystems, KartTuning,
    KartVisualAssets, KartWheels, PlayerKart, Remote, Slipstream, Stall, StatEffects, StatsTuning,
    kart_collider, spawn_kart_visual_with_driver,
};
use crate::level::LevelScoped;
use crate::race::{RaceMode, Racer, RacerName};
use crate::track::{GRID_SLOTS, PlayerStart, TrackChoice};

/// Our states waiting to be sent, at most (the socket takes them every frame).
const MAX_OUTGOING: usize = 8;

/// Where our kart starts online: the grid slot of our seat in free driving, of our place on the
/// race's grid in a race (issue #9). `crate::spawn_player` reads it, and gives our kart this
/// network id ([`NetKart`]).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnlineSlot(pub usize);

/// A kart we have authority over online, and its network id (its grid slot): our player's kart,
/// and the host's bots (issue #9). Its states go to every peer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetKart(pub KartId);

/// What a peer shows online (issue #9, `online.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Showing {
    /// Not yet decided (just seated).
    Nothing,
    /// Free driving on the room's circuit (an index of `assets/tracks/index.ron`).
    Free(u8),
    /// The race of this number: on its grid, or watching it (not on it: free driving there).
    Race(u16),
}

/// We drive online: the level we were on before, to get back to it when we leave the room.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct OnlineLevel {
    /// Our id in the room we entered.
    pub me: PeerKey,
    pub solo_track: TrackChoice,
    pub solo_mode: RaceMode,
    /// The solo grid (bots, seed): a race online changes it.
    pub solo_grid: crate::race::RaceGrid,
    /// What we show now.
    pub showing: Showing,
}

/// Our kart's positions of the last 30 ticks, by common tick (issue #34: the victim compares
/// its position at the tick the striker saw).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KartHistory(pub PoseHistory);

/// The body of another peer's kart: kinematic, the collider of a kart, the look its states give
/// (`KartState`, `Boost`... only for its visual: no driving system touches it).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteKartProxy {
    /// The peer that drives it (its player, or the host for a bot).
    pub peer: PeerKey,
    pub kart: KartId,
}

/// A kart of another peer, as the roster of the room says (issue #9): see
/// [`KartSync::set_roster`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RosterEntry<'a> {
    /// Its network id: the seat in free driving, the grid slot in a race.
    pub kart: KartId,
    /// The peer that has authority over it.
    pub peer: PeerKey,
    /// Its four parts (`None`: those it had: a player who left).
    pub loadout: Option<[u8; 4]>,
    /// The color of its kart (`bots::bot_color`).
    pub color: usize,
    /// Its player's name (a bot: none; `None` for a player: the one it had, a player who left).
    pub name: Option<&'a str>,
    /// It races: a racer of our race (its progress comes from its peer's events).
    pub racer: bool,
    /// Its player left the race: kept for the ranking, hidden, nothing to bump into.
    pub left: bool,
}

/// A kart another peer drives, as we know it.
#[derive(Clone, Debug)]
pub struct RemoteKart {
    /// The peer that drives it.
    pub peer: PeerKey,
    /// Its four parts (its `Hello`): its animal on screen.
    pub loadout: [u8; 4],
    /// The color of its kart: its seat, or its grid slot (issue #9).
    pub slot: usize,
    /// The id it gives its kart on the wire.
    pub kart: KartId,
    /// Its player's name.
    pub name: Option<String>,
    /// A racer of our race (issue #9).
    pub racer: bool,
    /// Its player left the race.
    pub left: bool,
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
    fn new(entry: &RosterEntry) -> Self {
        Self {
            peer: entry.peer,
            loadout: entry.loadout.unwrap_or_default(),
            slot: entry.color,
            kart: entry.kart,
            name: entry.name.map(str::to_string),
            racer: entry.racer,
            left: entry.left,
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
    /// What the kart ids mean (issue #9): 0 in free driving, the race's in a race
    /// (`KartSnapshot::session`).
    session: u8,
    /// The room's common tick of our local ticks.
    pub clock: TickClock,
    outgoing: Vec<KartSnapshot>,
    remotes: Vec<RemoteKart>,
    sim: NetSim<(PeerKey, KartSnapshot)>,
    /// Bodies and visuals of karts gone, to despawn.
    gone: Vec<Entity>,
    /// The interactions between karts (issue #9): received from a peer, and to send to all.
    events_in: Vec<(PeerKey, Message)>,
    events_out: Vec<Message>,
    /// The strike answers and tether ends written this tick from the network (`online.rs`): not
    /// sent back.
    pub(super) delivered: [u16; 2],
}

impl KartSync {
    /// We are `me`, our kart is `kart` on the wire; the network simulator's draws come from
    /// `seed`.
    pub fn new(me: PeerKey, kart: KartId, seed: u64) -> Self {
        Self {
            me,
            kart,
            session: 0,
            clock: TickClock::default(),
            outgoing: Vec::with_capacity(MAX_OUTGOING * GRID_SLOTS),
            remotes: Vec::with_capacity(GRID_SLOTS),
            sim: NetSim::new(seed ^ me.0 as u64 ^ (me.0 >> 64) as u64),
            gone: Vec::new(),
            events_in: Vec::new(),
            events_out: Vec::new(),
            delivered: [0; 2],
        }
    }

    /// What the kart ids mean now (issue #9): 0 in free driving, the race's in a race.
    pub fn session(&self) -> u8 {
        self.session
    }

    /// A new session (free driving, or a race): the ids mean other karts, every remote kart
    /// starts over.
    pub fn set_session(&mut self, session: u8) {
        if session != self.session {
            self.session = session;
            let gone = &mut self.gone;
            for remote in self.remotes.drain(..) {
                gone.extend(remote.proxy.iter().chain(&remote.visual));
            }
        }
    }

    /// Our player's kart has this id now (its seat, or its grid slot).
    pub fn set_kart(&mut self, kart: KartId) {
        self.kart = kart;
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

    /// The kart of `peer` (its player's kart: the first it drives).
    pub fn remote(&self, peer: PeerKey) -> Option<&RemoteKart> {
        self.remotes.iter().find(|remote| remote.peer == peer)
    }

    /// The remote kart of network id `kart`.
    pub fn remote_kart(&self, kart: KartId) -> Option<&RemoteKart> {
        self.remotes.iter().find(|remote| remote.kart == kart)
    }

    /// The other players of the room in free driving: their id, their parts and their seat (the
    /// id of their kart, and its color). Those not listed any more are gone (their kart
    /// disappears).
    pub fn set_players(&mut self, players: impl IntoIterator<Item = (PeerKey, [u8; 4], usize)>) {
        let me = self.me;
        self.set_roster(players.into_iter().filter(|(peer, ..)| *peer != me).map(
            |(peer, loadout, slot)| RosterEntry {
                kart: slot.min(GRID_SLOTS - 1) as KartId,
                peer,
                loadout: Some(loadout),
                color: slot,
                name: None,
                racer: false,
                left: false,
            },
        ));
    }

    /// The other peers' karts (issue #9): every kart we show and do not drive, by its network
    /// id, with the peer that has authority over it now. Those not listed any more are gone
    /// (their kart disappears); a kart whose peer changed (a new host takes the bots) starts its
    /// buffer over; one whose parts changed is spawned again.
    pub fn set_roster<'a>(&mut self, entries: impl IntoIterator<Item = RosterEntry<'a>>) {
        for remote in &mut self.remotes {
            remote.present = false;
        }
        for entry in entries {
            match self
                .remotes
                .iter_mut()
                .find(|remote| remote.kart == entry.kart)
            {
                Some(remote) => {
                    remote.present = true;
                    if remote.peer != entry.peer {
                        remote.peer = entry.peer;
                        remote.buffer.clear();
                        remote.playback = Playback::default();
                    }
                    let loadout = entry.loadout.unwrap_or(remote.loadout);
                    if remote.loadout != loadout
                        || remote.slot != entry.color
                        || remote.racer != entry.racer
                        || remote.left != entry.left
                    {
                        remote.loadout = loadout;
                        remote.slot = entry.color;
                        remote.racer = entry.racer;
                        remote.left = entry.left;
                        remote.respawn = true;
                    }
                    // A player who left keeps its name (`None`: unknown now).
                    if let Some(name) = entry.name
                        && remote.name.as_deref() != Some(name)
                    {
                        remote.name = Some(name.to_string());
                        remote.respawn = true;
                    }
                }
                None => self.remotes.push(RemoteKart::new(&entry)),
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
        if state.session != self.session {
            return;
        }
        if tuning.simulator.active() {
            self.sim.push(now_ms, (from, state), &tuning.simulator);
        } else {
            deliver(&mut self.remotes, from, state, now_ms, tuning);
        }
    }

    /// An interaction between karts from `from` (issue #9): applied in the next tick.
    pub fn receive_event(&mut self, from: PeerKey, message: Message) {
        self.events_in.push((from, message));
    }

    /// The interactions received, oldest first.
    pub fn drain_events_in(&mut self) -> std::vec::Drain<'_, (PeerKey, Message)> {
        self.events_in.drain(..)
    }

    /// An interaction to tell every peer.
    pub fn send_event(&mut self, message: Message) {
        self.events_out.push(message);
    }

    /// The interactions to send, oldest first.
    pub fn drain_events_out(&mut self) -> std::vec::Drain<'_, Message> {
        self.events_out.drain(..)
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

    /// Our karts' state, to send to every peer (the newest, if nobody takes them), stamped with
    /// the session.
    pub fn send(&mut self, mut state: KartSnapshot) {
        if self.outgoing.len() >= MAX_OUTGOING * GRID_SLOTS {
            self.outgoing.remove(0);
        }
        state.session = self.session;
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
    let Some(remote) = remotes
        .iter_mut()
        .find(|remote| remote.kart == state.kart && remote.peer == from)
    else {
        return;
    };
    remote.received += 1;
    if remote.buffer.insert(state.pose()) == Insert::Restarted {
        // Its clock jumped back (a new host): its lag starts over too.
        remote.playback = Playback::default();
    }
    remote
        .playback
        .arrived(now_ms, state.tick, f64::from(tuning.lag_window_ms.max(1.0)));
}

/// The kart synchronization (issue #8), and the race online (issue #9, `online.rs`). Its systems
/// run only with a [`KartSync`] (online), and the switch to online driving only with a room:
/// solo is unchanged.
pub struct KartSyncPlugin;

impl Plugin for KartSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetTuning>()
            .add_systems(
                Update,
                (update_remote_karts, spawn_remote_karts, report_karts)
                    .chain()
                    .after(NetSystems)
                    .after(super::online::OnlineSystems)
                    .run_if(resource_exists::<KartSync>),
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
            )
            .add_plugins(super::online::OnlineRacePlugin);
    }
}

/// The local virtual time (ms): the clock of the playbacks. It stops while the game is paused
/// (solo only: online, nothing pauses, issue #9), like the karts.
pub(super) fn virtual_ms(time: &Time<Virtual>) -> f64 {
    time.elapsed_secs_f64() * 1_000.0
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
                    "network: kart {} of #{} at ({:.1}, {:.1}, {:.1}), tick {:.0}; {} states, the newest of tick {}",
                    remote.kart,
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
                "network: kart {} of #{}: not shown yet, {} states",
                remote.kart,
                remote.peer.tag(),
                remote.received
            ),
        }
    }
}

/// The console's report of the karts online, this often (s of game time).
const REPORT_SECONDS: f64 = 3.0;

/// Spawns the body and the visual of each remote kart once its first state arrived and the level
/// is built; again after a new level, or when its player changed parts; despawns those gone. In a
/// race (issue #9) it is a racer (`Racer`: its progress comes from its peer's events, `online.rs`)
/// with its player's name; a player who left keeps a body without visual, that nothing bumps into.
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
            .or_else(|| {
                // A racer that left before any state of it came: where its grid slot is.
                (remote.left && remote.racer).then(|| KartPose {
                    position: Vec3::new(0.0, -100.0, 0.0),
                    ..Default::default()
                })
            })
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
        let mut proxy = commands.spawn((
            Name::new(format!("En ligne {} #{}", remote.kart, remote.peer.tag())),
            RemoteKartProxy {
                peer: remote.peer,
                kart: remote.kart,
            },
            LevelScoped,
            transform,
            remote_body(),
            // Solid once clear of our karts (`move_remote_karts`); a player who left, never.
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
            // Issue #9: what the slipstream, the bots and the paws see of it; its paw, for the
            // visuals of what its peer tells.
            (
                Remote {
                    seen: shown.tick,
                    now: shown.position,
                    now_velocity: shown.velocity,
                },
                Paw::default(),
            ),
        ));
        if remote.racer {
            proxy.insert(Racer::new(usize::from(remote.kart)));
        }
        if let Some(name) = &remote.name {
            proxy.insert(RacerName(name.clone()));
        }
        let proxy = proxy.id();
        remote.proxy = Some(proxy);
        if remote.left {
            continue;
        }
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
        let Some(proxy) = remote.proxy.filter(|_| !remote.left) else {
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

/// After each tick: our karts' positions into their history, and every few ticks their state to
/// send, stamped with the common tick (nothing before the clock is set). Our karts: our player's,
/// and the bots when we host a race (issue #9: [`NetKart`]).
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
            Option<&NetKart>,
        ),
        (With<Kart>, Or<(With<PlayerKart>, With<NetKart>)>),
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
        net,
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
        let kart = net.map_or(sync.kart(), |net| net.0);
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
    for remote in sync.remotes().iter().filter(|remote| !remote.left) {
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
