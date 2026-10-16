//! The race online (issue #9, `docs/ARCHITECTURE.md` § « Course en ligne »): once seated in a
//! room, everyone drives freely on the room's circuit; the host starts a race, and every peer
//! races it on the same grid, on the room's clock.
//!
//! - **The room** ([`online_mode`]): free driving on the host's circuit (`Lobby::circuit`), or the
//!   race it announced (`Lobby::announced`): its circuit, its grid (the bots in front, the players
//!   behind in the host's order, [`OnlineSlot`]), its seed (the same bots everywhere). A peer that
//!   is not on its grid (it came during the race) drives freely there and watches. Leaving the
//!   room brings back the level we were on. In the room, the game never pauses (`level::Online`).
//! - **The clock** ([`follow_room_clock`]): the race's flow follows the room's common tick
//!   (`race::OnlineFlow`): the grid, the countdown and GO on the same tick for everyone, the
//!   rocket start judged by each peer for its own kart.
//! - **The bots**: the host spawns them (`RaceGrid::bots`) and drives them; their states go out
//!   like its own kart's ([`NetKart`]). The others show them as remote karts. Issue #63: the host
//!   is the room's creator; if it leaves, the room is over (nobody takes the bots over).
//! - **Checkpoints** ([`record_progress`], [`follow_remote_progress`]): the peer that has
//!   authority over a kart tells everyone each checkpoint (reliable); the others give the remote
//!   kart the same laps, times and finish, and its live distance from where it is shown. Every
//!   peer ranks every racer.
//! - **The end** ([`arbitrate`]): the host ends the race when every player finished (or
//!   `NetTuning::finish_grace_s` after the first one did), with its final order: the same results
//!   on every screen. Issue #70: the results follow the karts still on track (their checkpoints
//!   keep coming), then the host closes them ([`close_results_for_everyone`]): its table, with
//!   the estimated times, is everyone's.
//! - **Interactions** ([`deliver_interactions`], [`forward_interactions`]): a strike or a grab on
//!   another peer's kart goes to everyone; the peer that has authority over the target decides
//!   (the victim decides), and its answer goes to everyone, for the visuals.
//! - **Colors** (issue #55, [`paint_online_karts`], [`race_colors`]): each player's kart wears the
//!   color the lobby gives it (`Lobby::color_of`: its choice, unless a player of an earlier seat
//!   chose it too), and the race's bots avoid the players' colors (`kart::bot_colors`). Every
//!   peer computes the same from the seats and the `Hello`s, and follows when they change.
//!
//! The items and the croquettes (issue #53) are shared by `items_online.rs`.

use bevy::prelude::*;

use super::interp::TICK_MS;
use super::items_online::OnlineItems;
use super::protocol::{
    Grab, KartId, Message, PeerKey, Progress, RaceStart, Strike, StrikeReply, TetherBreak,
};
use super::race::{Driver, SharedRace};
use super::sync::{
    KartSync, NetKart, OnlineLevel, OnlineSlot, RemoteKartProxy, RosterEntry, Showing, virtual_ms,
};
use super::tuning::NetTuning;
use super::{Lobby, NetSession, NetStatus, NetSystems, now_ms};
use crate::admin::tracks::circuits;
use crate::bots::{Bot, BotDifficulty, bot_loadout};
use crate::characters::Roster;
use crate::interactions::{
    InteractionClock, InteractionSystems, InteractionTuning, KartGrabbed, KartStruck, Paw,
    PawMotion, StrikeAnswer, Swing, SwingKind, TetherEnded, ticks,
};
use crate::interpolation::InterpolationSystems;
use crate::kart::{
    ColorSet, Kart, KartColor, KartPaint, KartSystems, OnlinePlayerColors, PALETTE_SIZE,
    PlayerKart, RaceClass, Remote, SpawnPoint, bot_colors,
};
use crate::level::{LoadLevel, Online};
use crate::race::{
    Course, GrandPrix, OnlineFlow, Race, RaceGrid, RaceMode, RaceProgress, RaceSystems, RaceTuning,
    Racer, ResultLine, Standing, close_results, seconds_to_ticks,
};
use crate::track::{GRID_SLOTS, MirrorMode, TrackChoice, TrackLayout};

/// The race online in progress here (issue #9): from its announcement to the next one (or the
/// host's next circuit).
#[derive(Resource)]
pub struct OnlineRace {
    pub shared: SharedRace,
    /// We are on its grid (otherwise we watch it, driving freely).
    pub racing: bool,
    /// Its bots are driven here: we host it (`bots::BotPlugin` spawned them).
    pub bots_here: bool,
    /// The bots' parts (`bots::bot_loadout`), by grid slot: their animals on our screen.
    bot_parts: Vec<[u8; 4]>,
    /// The events to tell every peer: our karts' checkpoints, the end (the host).
    outbox: Vec<Message>,
}

impl OnlineRace {
    pub fn new(start: RaceStart, me: PeerKey, hosting: bool, roster: &Roster) -> Self {
        let bot_parts = (0..usize::from(start.bots))
            .map(|slot| {
                let parts = bot_loadout(roster, start.seed, slot);
                [parts.character, parts.kart, parts.wheels, parts.wing].map(|p| p.min(255) as u8)
            })
            .collect();
        let racing = start.slot_of(me).is_some();
        Self {
            shared: SharedRace::new(start),
            racing,
            bots_here: hosting && racing,
            bot_parts,
            outbox: Vec::new(),
        }
    }

    /// The events to send, oldest first.
    pub fn drain_outbox(&mut self) -> std::vec::Drain<'_, Message> {
        self.outbox.drain(..)
    }
}

/// The kart states' session of race `race` (`KartSnapshot::session`): never 0 (free driving).
pub fn session_of(race: u16) -> u8 {
    (race % 255) as u8 + 1
}

/// The host starts a race now, at our `now_ms`: the room's circuit, a fresh seed, the seated
/// players behind the bots that fill the grid. `None` when we do not host, or do not know the
/// room's clock yet.
pub fn race_to_start(
    lobby: &Lobby,
    now_ms: u64,
    seed: u64,
    laps: u32,
    difficulty: BotDifficulty,
    tuning: &NetTuning,
) -> Option<RaceStart> {
    if !lobby.is_host() {
        return None;
    }
    let room_ms = lobby.room_time_ms(now_ms)?;
    let players: Vec<PeerKey> = lobby.seats().iter().take(GRID_SLOTS).copied().collect();
    let lead_ms = f64::from(tuning.start_lead_ms.max(0.0));
    Some(RaceStart {
        race: lobby.next_race(),
        seed,
        track: lobby.circuit(),
        laps: laps.clamp(1, 255) as u8,
        bots: (GRID_SLOTS - players.len()) as u8,
        difficulty: difficulty.index() as u8,
        start_tick: ((room_ms as f64 + lead_ms) / TICK_MS).ceil() as u32,
        players,
    })
}

/// The color of every kart of the race `start` (issue #55), by grid slot: each player's, as the
/// lobby says (`Lobby::color_of`; a player who left: its slot's bot color, it is hidden), and the
/// bots', which avoid the players' (`kart::bot_colors`). With the colors the players wear. The
/// same on every peer that knows the same `Hello`s; nothing allocated.
pub fn race_colors(lobby: &Lobby, start: &RaceStart) -> ([KartColor; PALETTE_SIZE], ColorSet) {
    let players: ColorSet = start
        .players
        .iter()
        .filter_map(|peer| lobby.color_of(*peer))
        .collect();
    let bots = usize::from(start.bots);
    let mut colors = bot_colors(bots, players);
    for (index, peer) in start.players.iter().enumerate() {
        if let (Some(color), Some(kart)) = (lobby.color_of(*peer), colors.get_mut(bots + index)) {
            *kart = color;
        }
    }
    (colors, players)
}

/// The circuit of index `track` in `assets/tracks/index.ron` (the first one if unknown).
fn circuit(track: u8) -> TrackChoice {
    circuits()
        .get(usize::from(track))
        .map_or_else(TrackChoice::default, |entry| entry.choice())
}

/// The online systems in `Update`, after the socket's (`NetSystems`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct OnlineSystems;

pub struct OnlineRacePlugin;

impl Plugin for OnlineRacePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                online_mode.run_if(
                    |session: Option<Res<NetSession>>, online: Option<Res<OnlineLevel>>| {
                        session.is_some() || online.is_some()
                    },
                ),
                tag_local_bots
                    .run_if(resource_exists::<OnlineRace>)
                    .run_if(resource_exists::<KartSync>)
                    .run_if(resource_exists::<NetSession>),
            )
                .chain()
                .after(NetSystems)
                .in_set(OnlineSystems),
        )
        .add_systems(
            FixedUpdate,
            (
                follow_room_clock
                    .run_if(resource_exists::<OnlineRace>)
                    .run_if(resource_exists::<OnlineFlow>)
                    .run_if(resource_exists::<KartSync>)
                    .before(RaceSystems::Flow),
                (update_remote_targets, deliver_interactions)
                    .chain()
                    .run_if(resource_exists::<KartSync>)
                    .run_if(resource_exists::<InteractionTuning>)
                    .after(KartSystems::Input)
                    .before(InteractionSystems),
                forward_interactions
                    .run_if(resource_exists::<KartSync>)
                    .run_if(resource_exists::<InteractionTuning>)
                    .after(InteractionSystems),
                (record_progress, follow_remote_progress)
                    .run_if(resource_exists::<OnlineRace>)
                    .run_if(resource_exists::<Race>)
                    .run_if(resource_exists::<Course>)
                    .after(RaceSystems::Rules)
                    .before(RaceSystems::Rank),
                (arbitrate, close_results_for_everyone)
                    .chain()
                    .run_if(resource_exists::<OnlineRace>)
                    .run_if(resource_exists::<Race>)
                    .run_if(resource_exists::<NetSession>)
                    .after(RaceSystems::Rank),
            ),
        )
        .add_systems(
            PostUpdate,
            // Before the visuals follow the karts' colors (`kart::KartVisualPlugin`), so that a
            // kart spawned this frame shows its color at once.
            paint_online_karts
                .run_if(resource_exists::<KartSync>)
                .run_if(resource_exists::<NetSession>)
                .before(InterpolationSystems),
        );
    }
}

/// Our seat's place (0 first), as a grid slot.
fn seat_of(lobby: &Lobby) -> usize {
    lobby
        .seats()
        .iter()
        .position(|key| *key == lobby.me())
        .unwrap_or(0)
        .min(GRID_SLOTS - 1)
}

/// Seated in a room: online driving (free on the room's circuit, or the host's race; see the
/// module's documentation); out of it: back to the level we were on. In the room, every frame:
/// the room's clock, the karts we show and who drives them, no pause.
#[allow(clippy::too_many_arguments)]
fn online_mode(
    mut commands: Commands,
    session: Option<Res<NetSession>>,
    mut online: Option<ResMut<OnlineLevel>>,
    mut sync: Option<ResMut<KartSync>>,
    race: Option<Res<OnlineRace>>,
    mut interaction_clock: Option<ResMut<InteractionClock>>,
    (choice, mode, mut grid): (Res<TrackChoice>, Res<RaceMode>, ResMut<RaceGrid>),
    (mut class, mut mirror): (ResMut<RaceClass>, ResMut<MirrorMode>),
    roster: Option<Res<Roster>>,
    real: Res<Time<Real>>,
    mut time: ResMut<Time<Virtual>>,
    mut levels: MessageWriter<LoadLevel>,
    (layout, mut players): (
        Option<Res<TrackLayout>>,
        Query<(&mut NetKart, Option<&mut SpawnPoint>), With<PlayerKart>>,
    ),
) {
    let lobby = session
        .as_deref()
        .filter(|session| session.status == NetStatus::InRoom)
        .and_then(|session| session.lobby.as_ref())
        .filter(|lobby| lobby.seated());
    // Our place: issue #63, in the host's list of seats, ours as soon as we are seated.
    let now = now_ms(&real);
    match (online.as_deref_mut(), lobby) {
        (Some(level), lobby) if lobby.is_none_or(|lobby| lobby.me() != level.me) => {
            info!("network: online driving ends, back to the level before");
            if let Some(sync) = &sync {
                for remote in sync.remotes() {
                    for entity in remote.proxy.iter().chain(&remote.visual) {
                        commands.entity(*entity).try_despawn();
                    }
                }
            }
            commands.remove_resource::<KartSync>();
            commands.remove_resource::<OnlineItems>();
            commands.remove_resource::<OnlineSlot>();
            commands.remove_resource::<OnlineLevel>();
            commands.remove_resource::<OnlineRace>();
            commands.remove_resource::<OnlineFlow>();
            commands.remove_resource::<Online>();
            commands.remove_resource::<OnlinePlayerColors>();
            if let Some(clock) = interaction_clock.as_mut() {
                clock.set_if_neq(InteractionClock::default());
            }
            *grid = level.solo_grid;
            class.set_if_neq(RaceClass(level.solo_class));
            mirror.set_if_neq(MirrorMode(level.solo_mirror));
            levels.write(LoadLevel::on(level.solo_track.clone(), level.solo_mode));
        }
        (None, Some(lobby)) => {
            let me = lobby.me();
            let seat = seat_of(lobby);
            info!("network: online driving, seat {seat}");
            commands.insert_resource(KartSync::new(me, seat as KartId, grid.seed));
            commands.insert_resource(OnlineItems::default());
            commands.insert_resource(OnlineSlot(seat));
            commands.insert_resource(Online);
            // Issue #19: online, 150cc on the circuits as they are, and a Grand Prix in progress
            // ends (its grid order with it).
            commands.remove_resource::<GrandPrix>();
            grid.lineup = None;
            commands.insert_resource(OnlineLevel {
                me,
                solo_track: choice.clone(),
                solo_mode: *mode,
                solo_grid: *grid,
                solo_class: class.0,
                solo_mirror: mirror.0,
                showing: Showing::Nothing,
            });
            class.set_if_neq(RaceClass::default());
            mirror.set_if_neq(MirrorMode::default());
            // The level follows on the next frame, once these are in.
        }
        (Some(level), Some(lobby)) => {
            let Some(sync) = sync.as_deref_mut() else {
                return;
            };
            // Online, the game never pauses nor slows down (issue #9): every peer's race follows
            // the room's clock.
            if time.is_paused() {
                time.unpause();
            }
            if time.relative_speed() != 1.0 {
                time.set_relative_speed(1.0);
            }
            if let Some(room_ms) = lobby.room_time_ms(now) {
                // The room's clock and ours now, in ticks: the common tick of our local ticks.
                sync.clock
                    .observe((room_ms as f64 - virtual_ms(&time)) / TICK_MS);
            }
            if let (Some(clock), Some(offset)) = (interaction_clock.as_mut(), sync.clock.offset()) {
                clock.set_if_neq(InteractionClock { offset });
            }
            let wanted = match lobby.announced() {
                Some(start) => Showing::Race(start.race),
                None => Showing::Free(lobby.circuit()),
            };
            if level.showing != wanted {
                level.showing = wanted;
                let seat = seat_of(lobby);
                match lobby.announced() {
                    None => {
                        info!(
                            "network: free driving on circuit {}, from grid slot {seat}",
                            lobby.circuit()
                        );
                        commands.insert_resource(OnlineSlot(seat));
                        commands.remove_resource::<OnlineRace>();
                        commands.remove_resource::<OnlineFlow>();
                        *grid = RaceGrid { bots: 0, ..*grid };
                        sync.set_session(0);
                        sync.set_kart(seat as KartId);
                        levels.write(LoadLevel::on(circuit(lobby.circuit()), RaceMode::FreeRoam));
                    }
                    Some(start) => {
                        let Some(roster) = roster.as_deref() else {
                            return;
                        };
                        let race =
                            OnlineRace::new(start.clone(), lobby.me(), lobby.is_host(), roster);
                        let slot = race.shared.slot_of(lobby.me());
                        *grid = RaceGrid {
                            bots: if lobby.is_host() {
                                usize::from(start.bots)
                            } else {
                                0
                            },
                            seed: start.seed,
                            lineup: None,
                        };
                        sync.set_session(session_of(start.race));
                        match slot {
                            Some(slot) => {
                                info!(
                                    "network: race {} on circuit {}, {} bots, from grid slot {slot}, starting at tick {}",
                                    start.race, start.track, start.bots, start.start_tick
                                );
                                commands.insert_resource(OnlineSlot(usize::from(slot)));
                                commands.insert_resource(OnlineFlow::default());
                                sync.set_kart(slot);
                                levels.write(LoadLevel::on(circuit(start.track), RaceMode::Race));
                            }
                            None => {
                                info!("network: race {} on, we watch it", start.race);
                                commands.insert_resource(OnlineSlot(GRID_SLOTS - 1));
                                commands.remove_resource::<OnlineFlow>();
                                sync.set_kart((GRID_SLOTS - 1) as KartId);
                                levels
                                    .write(LoadLevel::on(circuit(start.track), RaceMode::FreeRoam));
                            }
                        }
                        commands.insert_resource(race);
                        // The roster follows on the next frame, with the race.
                        return;
                    }
                }
            }
            match (level.showing, race.as_deref()) {
                (Showing::Race(number), Some(race)) if race.shared.race() == number => {
                    set_race_roster(sync, lobby, race);
                }
                (Showing::Race(_), _) => {}
                _ => {
                    // Our seat can change while we drive freely: a player before us left, or two
                    // peers who sat alone at first (a slow connection) met. Our kart's id and its
                    // place on the grid (R) follow it: the others look for our kart by its seat.
                    let seat = seat_of(lobby);
                    if sync.kart() != seat as KartId {
                        info!("network: our seat is {seat} now");
                        sync.set_kart(seat as KartId);
                        commands.insert_resource(OnlineSlot(seat));
                        let place = layout.as_deref().and_then(|l| l.grid.get(seat).copied());
                        for (mut net, spawn) in &mut players {
                            net.0 = seat as KartId;
                            if let (Some(mut spawn), Some(place)) = (spawn, place) {
                                spawn.0 = place;
                            }
                        }
                    }
                    sync.set_players(lobby.seats().iter().enumerate().filter_map(|(slot, key)| {
                        let profile = lobby.peer(*key)?.profile.as_ref()?;
                        let color = lobby.color_of(*key).unwrap_or(profile.color);
                        Some((*key, profile.loadout, slot, color))
                    }))
                }
            }
        }
        _ => {}
    }
}

/// The karts of the race we show and do not drive: the other players' (theirs), and the bots
/// unless we host (the host's).
fn set_race_roster(sync: &mut KartSync, lobby: &Lobby, race: &OnlineRace) {
    let (me, host, hosting) = (lobby.me(), lobby.host(), lobby.is_host());
    let (colors, _) = race_colors(lobby, &race.shared.start);
    sync.set_roster(race.shared.karts().iter().filter_map(|kart| {
        let ours = match kart.driver {
            Driver::Player(peer) => peer == me,
            Driver::Bot => hosting,
        };
        if ours && race.racing {
            return None;
        }
        let peer = race.shared.authority(kart.kart, host)?;
        let (loadout, name) = match kart.driver {
            Driver::Player(peer) => lobby
                .peer(peer)
                .and_then(|p| p.profile.as_ref())
                .map_or((None, None), |profile| {
                    (Some(profile.loadout), Some(profile.name.as_str()))
                }),
            Driver::Bot => (race.bot_parts.get(usize::from(kart.kart)).copied(), None),
        };
        Some(RosterEntry {
            kart: kart.kart,
            peer,
            loadout,
            color: colors[usize::from(kart.kart) % PALETTE_SIZE],
            name,
            racer: race.racing,
            left: kart.left,
        })
    }));
}

/// Online (issue #55), every frame: our kart wears the color the lobby gives it (another player
/// may have chosen ours first), the race's bots we drive theirs (they avoid the players'), and
/// the bots that will spawn here avoid the race's players' colors (`OnlinePlayerColors`, read by
/// `bots::BotPlugin`). The visuals follow (`kart::KartPaint`). Nothing changes, nothing is
/// written.
fn paint_online_karts(
    mut commands: Commands,
    session: Res<NetSession>,
    race: Option<Res<OnlineRace>>,
    online: Option<Res<OnlineLevel>>,
    worn: Option<Res<OnlinePlayerColors>>,
    mut ours: Query<&mut KartPaint, (With<PlayerKart>, With<Kart>)>,
    mut bots: Query<(&Racer, &mut KartPaint), (With<Bot>, With<Kart>, Without<PlayerKart>)>,
) {
    let Some(lobby) = session.lobby.as_ref() else {
        return;
    };
    if let Some(color) = lobby.color_of(lobby.me()) {
        for mut paint in &mut ours {
            paint.set_if_neq(KartPaint(color));
        }
    }
    let shown = race
        .as_deref()
        .filter(|race| online.is_some_and(|o| o.showing == Showing::Race(race.shared.race())));
    let players = match shown {
        Some(race) => {
            let (colors, players) = race_colors(lobby, &race.shared.start);
            for (racer, mut paint) in &mut bots {
                paint.set_if_neq(KartPaint(colors[racer.grid_slot % PALETTE_SIZE]));
            }
            players
        }
        None => ColorSet::default(),
    };
    if worn.is_none_or(|worn| worn.0 != players) {
        commands.insert_resource(OnlinePlayerColors(players));
    }
}

/// The bots we host get their network id (their grid slot): their states go out like our kart's.
fn tag_local_bots(
    mut commands: Commands,
    race: Res<OnlineRace>,
    bots: Query<(Entity, &Racer), (With<Bot>, With<Kart>, Without<NetKart>)>,
) {
    if !race.racing {
        return;
    }
    for (entity, racer) in &bots {
        commands
            .entity(entity)
            .insert(NetKart(racer.grid_slot.min(GRID_SLOTS - 1) as KartId));
    }
}

/// The local tick of this simulation tick (the fixed clock at the end of the step).
pub(super) fn local_tick(time: &Time<Fixed>) -> u32 {
    (time.elapsed_secs_f64() * crate::TICK_HZ).round() as u32
}

/// Before the race's flow: where the room's clock says the race is (`race::OnlineFlow`), the
/// host's end and final order once it came, and its closed results (issue #70). The race's laps
/// are the host's.
fn follow_room_clock(
    sync: Res<KartSync>,
    online: Res<OnlineRace>,
    race: Option<Res<Race>>,
    course: Option<ResMut<Course>>,
    time: Res<Time<Fixed>>,
    mut flow: ResMut<OnlineFlow>,
) {
    if let Some(mut course) = course
        && course.laps != u32::from(online.shared.start.laps)
    {
        course.laps = u32::from(online.shared.start.laps);
    }
    let Some(common) = sync.clock.common(local_tick(&time)) else {
        return;
    };
    let tick = (i64::from(common) - i64::from(online.shared.start.start_tick) + 1).max(0) as u32;
    if flow.flow_tick != tick {
        flow.flow_tick = tick;
    }
    if let (Some(over), Some(race)) = (online.shared.over(), race) {
        let over_at = Some(over.tick + race.timings.before_go());
        if flow.over_at != over_at {
            flow.over_at = over_at;
        }
        if flow.order.is_none() {
            flow.order = Some(over.order.iter().map(|kart| usize::from(*kart)).collect());
        }
    }
    if flow.results.is_none()
        && let Some(results) = online.shared.results()
    {
        flow.results = Some(
            results
                .lines
                .iter()
                .map(|line| ResultLine {
                    id: usize::from(line.id),
                    time: line.time,
                })
                .collect(),
        );
    }
}

/// Where each remote kart is for the paws (`kart::Remote`): the tick of the pose its body has
/// now (the one a strike judges), and its position extrapolated to the current tick (what a
/// tether pulls toward).
fn update_remote_targets(
    sync: Res<KartSync>,
    tuning: Res<NetTuning>,
    time: Res<Time<Fixed>>,
    mut targets: Query<&mut Remote, With<RemoteKartProxy>>,
) {
    let now = time.elapsed_secs_f64() * 1_000.0;
    let limits = tuning.limits();
    let common = sync.clock.common(local_tick(&time));
    for remote in sync.remotes() {
        let Some(mut target) = remote.proxy.and_then(|proxy| targets.get_mut(proxy).ok()) else {
            continue;
        };
        // The body is where the last step put it: the pose of one tick ago.
        let seen = remote
            .shown_tick(now - TICK_MS)
            .map_or(target.seen, |tick| tick.max(0.0).floor() as u32);
        let (position, velocity) = common
            .and_then(|tick| remote.buffer.sample(f64::from(tick), &limits))
            .map_or((target.now, target.now_velocity), |sample| {
                (sample.pose.position, sample.pose.velocity)
            });
        target.set_if_neq(Remote {
            seen,
            now: position,
            now_velocity: velocity,
        });
    }
}

/// The interactions the other peers told (issue #9), before ours run: a strike or a grab on one
/// of our karts is ours to judge (`interactions::receive_interactions`); the rest only shows on
/// the remote karts' paws (a swing, a "pof", an arm held out).
#[allow(clippy::too_many_arguments)]
fn deliver_interactions(
    mut sync: ResMut<KartSync>,
    tuning: Res<InteractionTuning>,
    local: Query<(Entity, &NetKart)>,
    transforms: Query<&Transform>,
    mut proxies: Query<(&Transform, &mut Paw), With<RemoteKartProxy>>,
    mut strikes: MessageWriter<KartStruck>,
    mut grabs: MessageWriter<KartGrabbed>,
    mut answers: MessageWriter<StrikeAnswer>,
    mut ends: MessageWriter<TetherEnded>,
    mut delivered: Local<Vec<(PeerKey, Message)>>,
) {
    delivered.clear();
    delivered.extend(sync.drain_events_in());
    let entity = |kart: KartId| -> Option<Entity> {
        local
            .iter()
            .find(|(_, net)| net.0 == kart)
            .map(|(entity, _)| entity)
            .or_else(|| sync.remote_kart(kart).and_then(|remote| remote.proxy))
    };
    let ours = |entity: Entity| local.contains(entity);
    let mut counts = [0u16; 2];
    for (_, message) in delivered.iter() {
        match message {
            Message::Strike(strike) => {
                let Some(event) = strike.to_event(entity) else {
                    continue;
                };
                if ours(event.target) {
                    strikes.write(event);
                }
                if let Ok((transform, mut paw)) = proxies.get_mut(event.striker) {
                    let right = transform.forward().cross(Vec3::Y);
                    let side = if right.dot(event.direction) < 0.0 {
                        -1.0
                    } else {
                        1.0
                    };
                    paw.swing = Some(Swing {
                        kind: if event.throw {
                            SwingKind::Throw
                        } else {
                            SwingKind::Strike
                        },
                        side,
                        target: Some(event.target),
                        ticks: 0,
                    });
                }
            }
            Message::StrikeReply(reply) => {
                let Some(answer) = reply.to_event(entity) else {
                    continue;
                };
                answers.write(answer);
                counts[0] += 1;
                if answer.landed {
                    let from = transforms.get(answer.striker).ok().map(|t| t.translation);
                    if let Ok((transform, mut paw)) = proxies.get_mut(answer.target) {
                        let push = from.map_or(Vec3::X, |from| {
                            (transform.translation - from)
                                .with_y(0.0)
                                .normalize_or(Vec3::X)
                        });
                        paw.struck_count += 1;
                        paw.struck_ticks = 0;
                        paw.struck_side = push;
                        // Protected for a while: our paws look elsewhere.
                        paw.protected = ticks(tuning.strike_protection);
                    }
                }
            }
            Message::Grab(grab) => {
                let Some(event) = grab.to_event(entity) else {
                    continue;
                };
                if ours(event.target) {
                    grabs.write(event);
                }
                if let Ok((_, mut paw)) = proxies.get_mut(event.grabber) {
                    paw.motion = PawMotion::Holding {
                        target: event.target,
                        ticks: 0,
                        rest: event.rest,
                        side: 1.0,
                    };
                }
            }
            Message::TetherBreak(end) => {
                let Some(event) = end.to_event(entity) else {
                    continue;
                };
                ends.write(event);
                counts[1] += 1;
                if let Ok((_, mut paw)) = proxies.get_mut(event.grabber)
                    && matches!(paw.motion, PawMotion::Holding { target, .. } if target == event.target)
                {
                    paw.motion = PawMotion::Idle;
                }
                if let Ok((_, mut paw)) = proxies.get_mut(event.target)
                    && paw.held.is_some_and(|held| held.by == event.grabber)
                {
                    paw.held = None;
                }
            }
            _ => {}
        }
    }
    sync.delivered = counts;
}

/// After the interactions: what our karts did to other peers' karts goes to everyone (a strike,
/// a grab), and what we decided for our karts (the answer to a strike, the end of a tether).
/// What came from the network this tick is not sent back.
fn forward_interactions(
    mut sync: ResMut<KartSync>,
    local: Query<&NetKart>,
    proxies: Query<&RemoteKartProxy>,
    mut strikes: MessageReader<KartStruck>,
    mut grabs: MessageReader<KartGrabbed>,
    mut answers: MessageReader<StrikeAnswer>,
    mut ends: MessageReader<TetherEnded>,
) {
    let id = |entity: Entity| -> Option<KartId> {
        local
            .get(entity)
            .map(|net| net.0)
            .or_else(|_| proxies.get(entity).map(|proxy| proxy.kart))
            .ok()
    };
    let ours = |entity: Entity| local.contains(entity);
    let [mut skip_answers, mut skip_ends] = std::mem::take(&mut sync.delivered);
    for strike in strikes.read() {
        if ours(strike.striker)
            && !ours(strike.target)
            && let Some(message) = Strike::from_event(strike, id)
        {
            sync.send_event(Message::Strike(message));
        }
    }
    for grab in grabs.read() {
        if ours(grab.grabber)
            && !ours(grab.target)
            && let Some(message) = Grab::from_event(grab, id)
        {
            sync.send_event(Message::Grab(message));
        }
    }
    for answer in answers.read() {
        if skip_answers > 0 {
            skip_answers -= 1;
            continue;
        }
        if ours(answer.target)
            && !ours(answer.striker)
            && let Some(message) = StrikeReply::from_event(answer, id)
        {
            sync.send_event(Message::StrikeReply(message));
        }
    }
    for end in ends.read() {
        if skip_ends > 0 {
            skip_ends -= 1;
            continue;
        }
        if (ours(end.grabber) != ours(end.target))
            && let Some(message) = TetherBreak::from_event(end, id)
        {
            sync.send_event(Message::TetherBreak(message));
        }
    }
}

/// Our karts' checkpoints (our player's, the bots we host), as they validate them: to every peer.
fn record_progress(
    mut online: ResMut<OnlineRace>,
    race: Res<Race>,
    course: Res<Course>,
    karts: Query<(&NetKart, &RaceProgress), With<Kart>>,
) {
    let online = &mut *online;
    if online.shared.to_finish().is_none() {
        online
            .shared
            .set_to_finish(course.checkpoints_to_finish().min(u32::from(u16::MAX)) as u16);
    }
    let tick = race.race_ticks();
    for (net, progress) in &karts {
        let passed = progress.passed.min(u32::from(u16::MAX)) as u16;
        let known = online
            .shared
            .kart(net.0)
            .map_or(u16::MAX, |kart| kart.passed());
        if passed > known && online.shared.record(net.0, passed, tick) {
            online.outbox.push(Message::Progress(Progress {
                race: online.shared.race(),
                kart: net.0,
                passed,
                tick,
            }));
        }
    }
}

/// The other peers' karts in the race: their checkpoints as their peer told (the same laps,
/// times and finish), their live race distance from where they are shown.
fn follow_remote_progress(
    online: Res<OnlineRace>,
    course: Res<Course>,
    mut proxies: Query<(&RemoteKartProxy, &Transform, &mut RaceProgress)>,
) {
    for (proxy, transform, mut progress) in &mut proxies {
        let Some(kart) = online.shared.kart(proxy.kart) else {
            continue;
        };
        while progress.passed < u32::from(kart.passed()) && !progress.finished() {
            let tick = kart.checkpoints[progress.passed as usize];
            progress.validate(&course, tick);
        }
        let projection = course.project(progress.passed, transform.translation);
        progress.follow(&course, &projection);
    }
}

/// The host ends the race (every player finished, or the grace after the first one is over), with
/// its order: everyone's results.
fn arbitrate(
    session: Res<NetSession>,
    mut online: ResMut<OnlineRace>,
    race: Res<Race>,
    tuning: Res<NetTuning>,
    racers: Query<(&Racer, &RaceProgress)>,
) {
    let hosting = session.lobby.as_ref().is_some_and(Lobby::is_host);
    if !hosting || !online.racing || !race.started() {
        return;
    }
    let tick = race.race_ticks();
    let grace = seconds_to_ticks(tuning.finish_grace_s);
    if !online.shared.should_end(tick, grace) {
        return;
    }
    let distance = |kart: KartId| {
        racers
            .iter()
            .find(|(racer, _)| racer.grid_slot == usize::from(kart))
            .map_or(f32::MIN, |(_, progress)| progress.distance)
    };
    let order = online.shared.final_order(distance);
    info!("network: the race is over at race tick {tick}: {order:?}");
    let over = online.shared.end(tick, order);
    online.outbox.push(Message::RaceOver(over));
}

/// Issue #70: the host closes the results `RaceTuning::results_close_seconds` after they
/// appeared, like solo (`race::close_results`): its final order then (the finishers by their
/// finish, those still on track by checkpoints and distance, those who left last), the finishers'
/// times, an estimate for the others, none for those who left. Its table goes to everyone.
fn close_results_for_everyone(
    session: Res<NetSession>,
    mut online: ResMut<OnlineRace>,
    race: Res<Race>,
    course: Res<Course>,
    tuning: Res<RaceTuning>,
    racers: Query<(&Racer, &RaceProgress)>,
) {
    let hosting = session.lobby.as_ref().is_some_and(Lobby::is_host);
    if !hosting
        || !online.racing
        || online.shared.over().is_none()
        || online.shared.results().is_some()
    {
        return;
    }
    let close = seconds_to_ticks(tuning.results_close_seconds);
    if !race.flow.results_closed(&race.timings, close) {
        return;
    }
    let of = |kart: KartId| {
        racers
            .iter()
            .find(|(racer, _)| racer.grid_slot == usize::from(kart))
    };
    let order = online
        .shared
        .final_order(|kart| of(kart).map_or(f32::MIN, |(_, progress)| progress.distance));
    let standings = order.iter().map(|kart| {
        let shared = online.shared.kart(*kart);
        let gave_up = shared.is_some_and(|k| k.left && k.finish.is_none());
        match of(*kart) {
            Some((_, progress)) => Standing::of(*kart, progress, &course, gave_up),
            None => Standing {
                id: *kart,
                finish: shared.and_then(|k| k.finish),
                gave_up: true,
                covered: 0.0,
                remaining: 0.0,
            },
        }
    });
    let mut lines = Vec::with_capacity(order.len());
    close_results(
        standings,
        race.race_ticks(),
        tuning.results_min_speed,
        &mut lines,
    );
    info!(
        "network: the results close at race tick {}: {lines:?}",
        race.race_ticks()
    );
    let results = online.shared.close(lines);
    online.outbox.push(Message::RaceResults(results));
}

/// What the race tuning gives a new race: its laps.
pub fn race_laps(tuning: Option<&RaceTuning>) -> u32 {
    tuning.map_or(3, |tuning| tuning.laps)
}
