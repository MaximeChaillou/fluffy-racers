//! The race online (issue #9, `docs/ARCHITECTURE.md` § « Course en ligne »): once seated in a
//! room, everyone drives freely on the room's circuit; the host starts a race, and every peer
//! races it on the same grid, on the room's clock.
//!
//! - **The room** ([`online_mode`]): free driving on the host's circuit (`Lobby::circuit`), or the
//!   race it announced (`Lobby::announced`): its circuit, its grid (the bots in front, the players
//!   behind in the host's order, [`OnlineSlot`]), its seed (the same bots everywhere). Issue #99:
//!   with the host's mirror and items (`Lobby::setup`, `RaceStart::mirror` and `items`:
//!   `track::MirrorMode`, `track::NoItemBoxes`), and without bots if it says so. A peer that
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

use super::championship::{Championship, grid_lineup};
use super::interp::TICK_MS;
use super::items_online::OnlineItems;
use super::protocol::{
    Grab, KartId, Message, PeerKey, Progress, RaceStart, RoomSetup, Strike, StrikeReply,
    TetherBreak,
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
    PlayerKart, Remote, SpawnPoint, bot_colors,
};
use crate::level::{LoadLevel, Online};
use crate::race::{
    Course, GrandPrix, OnlineFlow, Race, RaceGrid, RaceMode, RaceProgress, RaceSystems, RaceTuning,
    Racer, ResultLine, Standing, close_results, seconds_to_ticks,
};
use crate::track::{GRID_SLOTS, MirrorMode, NoItemBoxes, TrackChoice, TrackLayout};

/// The race online in progress here (issue #9): from its announcement to the next one (or the
/// host's next circuit).
#[derive(Resource)]
pub struct OnlineRace {
    pub shared: SharedRace,
    /// We are on its grid (otherwise we watch it, driving freely).
    pub racing: bool,
    /// Its bots are driven here: we host it (`bots::BotPlugin` spawned them).
    pub bots_here: bool,
    /// The bots' parts (`bots::bot_loadout`: their number's, issue #122), by grid slot: their
    /// animals on our screen.
    bot_parts: Vec<Option<[u8; 4]>>,
    /// The events to tell every peer: our karts' checkpoints, the end (the host).
    outbox: Vec<Message>,
}

impl OnlineRace {
    pub fn new(start: RaceStart, me: PeerKey, hosting: bool, roster: &Roster) -> Self {
        // Issue #122: a bot is its number wherever it starts (a championship's grid).
        let bot_parts = (0..start.racers())
            .map(|slot| {
                let parts = bot_loadout(roster, start.seed, start.bot_at(slot)?);
                Some(
                    [parts.character, parts.kart, parts.wheels, parts.wing]
                        .map(|p| p.min(255) as u8),
                )
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
/// players behind the bots that fill the grid (issue #99: if the room has bots; issue #153, at
/// the room's level), the room's mirror and items. Issue #122: in a championship, its next race (`Lobby::championship_run`): its
/// circuit, its seed, its bots, its players still seated and its grid (the reverse of the
/// standings). `None` when we do not host, or do not know the room's clock yet.
pub fn race_to_start(
    lobby: &Lobby,
    now_ms: u64,
    seed: u64,
    laps: u32,
    tuning: &NetTuning,
) -> Option<RaceStart> {
    if !lobby.is_host() {
        return None;
    }
    let room_ms = lobby.room_time_ms(now_ms)?;
    let lead_ms = f64::from(tuning.start_lead_ms.max(0.0));
    let setup = lobby.setup();
    let mut start = RaceStart {
        race: lobby.next_race(),
        seed,
        track: setup.track,
        laps: laps.clamp(1, 255) as u8,
        bots: 0,
        // Issue #153: the level the host chose for the room's bots.
        difficulty: setup.difficulty,
        mirror: setup.mirror,
        items: setup.items,
        start_tick: ((room_ms as f64 + lead_ms) / TICK_MS).ceil() as u32,
        round: 0,
        lineup: Vec::new(),
        players: Vec::new(),
    };
    if let Some(run) = lobby.championship_run().filter(|run| !run.finished()) {
        let (players, lineup) = run.next_grid(lobby.seats());
        start.seed = run.state.seed;
        start.track = run.next_track()?;
        start.bots = run.state.bots;
        start.round = run.next_round()?;
        start.lineup = lineup;
        start.players = players;
        return Some(start);
    }
    start.players = lobby.seats().iter().take(GRID_SLOTS).copied().collect();
    // Issue #99: the room's settings: without bots, the players only.
    if setup.bots {
        start.bots = (GRID_SLOTS - start.players.len()) as u8;
    }
    Some(start)
}

/// Issue #122: the championship the host starts with its next race, when the room's mode is the
/// championship and none is in progress (or the last one is over): the room's program, `seed`
/// (the seed of its bots, for every race), the bots that fill the grid (if the room has bots) and
/// the players seated now, in their order.
pub fn championship_to_start(lobby: &Lobby, seed: u64) -> Option<Championship> {
    let setup = lobby.setup();
    if !lobby.is_host()
        || !setup.championship
        || setup.plan.program.is_empty()
        || lobby.championship_run().is_some_and(|run| !run.finished())
    {
        return None;
    }
    let players: Vec<PeerKey> = lobby.seats().iter().take(GRID_SLOTS).copied().collect();
    let bots = if setup.bots {
        GRID_SLOTS - players.len()
    } else {
        0
    };
    Some(Championship::new(setup.plan, seed, bots as u8, players))
}

/// The color of every kart of the race `start` (issue #55), by grid slot: each player's, as the
/// lobby says (`Lobby::color_of`; a player who left: its slot's bot color, it is hidden), and the
/// bots', which avoid the players' (`kart::bot_colors`): issue #122, each bot its number's,
/// wherever it starts. With the colors the players wear. The same on every peer that knows the
/// same `Hello`s; nothing allocated.
pub fn race_colors(lobby: &Lobby, start: &RaceStart) -> ([KartColor; PALETTE_SIZE], ColorSet) {
    let players: ColorSet = start
        .players
        .iter()
        .filter_map(|peer| lobby.color_of(*peer))
        .collect();
    let bots = usize::from(start.bots);
    let of_bots = bot_colors(bots, players);
    let mut colors = of_bots;
    for (slot, kart) in colors.iter_mut().enumerate().take(start.racers()) {
        let racer = start.racer_at(slot);
        if racer < bots {
            *kart = of_bots[racer % PALETTE_SIZE];
        } else if let Some(color) = start
            .players
            .get(racer - bots)
            .and_then(|peer| lobby.color_of(*peer))
        {
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

/// Issue #99: the circuits built from now on have their item boxes (`on`) or not
/// (`track::NoItemBoxes`).
fn items_on(commands: &mut Commands, on: bool) {
    if on {
        commands.remove_resource::<NoItemBoxes>();
    } else {
        commands.insert_resource(NoItemBoxes);
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
    (mut mirror, no_boxes, time_trial): (
        ResMut<MirrorMode>,
        Option<Res<NoItemBoxes>>,
        Option<Res<crate::modes::TimeTrial>>,
    ),
    roster: Option<Res<Roster>>,
    real: Res<Time<Real>>,
    mut time: ResMut<Time<Virtual>>,
    mut levels: MessageWriter<LoadLevel>,
    (layout, mut players): (
        Option<Res<TrackLayout>>,
        Query<(&mut NetKart, Option<&mut SpawnPoint>), With<PlayerKart>>,
    ),
    salon: Option<Res<crate::salon::SalonTuning>>,
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
            // Issue #122: a championship online is the room's (a solo one ended on arriving).
            commands.remove_resource::<GrandPrix>();
            if let Some(clock) = interaction_clock.as_mut() {
                clock.set_if_neq(InteractionClock::default());
            }
            *grid = level.solo_grid;
            mirror.set_if_neq(MirrorMode(level.solo_mirror));
            items_on(&mut commands, level.solo_items);
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
            // Issue #19: a Grand Prix in progress ends (its grid order with it). Issue #99: the
            // circuits mirrored or not, with items or not, as the host says (below).
            commands.remove_resource::<GrandPrix>();
            grid.lineup = None;
            // Issue #76: a time trial ends too, the player's bots back (kept for the way out).
            if let Some(trial) = &time_trial {
                commands.remove_resource::<crate::modes::TimeTrial>();
                grid.bots = trial.solo_bots;
            }
            commands.insert_resource(OnlineLevel {
                me,
                // Issue #101: never back to the lobby's scene out of a room (another room joined
                // from it, before the level before came back).
                solo_track: match &*choice {
                    TrackChoice::Salon => TrackChoice::default(),
                    other => other.clone(),
                },
                solo_mode: *mode,
                solo_grid: *grid,
                solo_mirror: mirror.0,
                solo_items: no_boxes.is_none(),
                showing: Showing::Nothing,
            });
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
                // Issue #136: a guest left the results of this race (« Retour au salon »): the
                // lobby's scene at once, before the host brings everyone back.
                Some(_) if salon.is_some() && lobby.left_the_results() => Showing::Salon,
                Some(start) => Showing::Race(start.race),
                // Issue #101: with the lobby in 3D (a renderer), its own scene.
                None if salon.is_some() => Showing::Salon,
                // The bots do not change free driving (it has none): no other level for them;
                // issue #122, nor the mode (free driving is on the room's circuit).
                None => Showing::Free(RoomSetup {
                    bots: true,
                    difficulty: RoomSetup::default().difficulty,
                    championship: false,
                    plan: Default::default(),
                    ..lobby.setup()
                }),
            };
            if level.showing != wanted {
                level.showing = wanted;
                let seat = seat_of(lobby);
                match lobby.announced() {
                    _ if wanted == Showing::Salon => {
                        info!("network: the lobby's scene, seat {seat}");
                        commands.insert_resource(OnlineSlot(seat));
                        commands.remove_resource::<OnlineRace>();
                        commands.remove_resource::<OnlineFlow>();
                        *grid = RaceGrid {
                            bots: 0,
                            lineup: None,
                            ..*grid
                        };
                        sync.set_session(0);
                        sync.set_kart(seat as KartId);
                        levels.write(LoadLevel::on(TrackChoice::Salon, RaceMode::FreeRoam));
                    }
                    None => {
                        let setup = lobby.setup();
                        info!(
                            "network: free driving on circuit {} (mirrored: {}, items: {}), from grid slot {seat}",
                            setup.track, setup.mirror, setup.items
                        );
                        // Issue #99: the host's mirror and items.
                        mirror.set_if_neq(MirrorMode(setup.mirror));
                        items_on(&mut commands, setup.items);
                        commands.insert_resource(OnlineSlot(seat));
                        commands.remove_resource::<OnlineRace>();
                        commands.remove_resource::<OnlineFlow>();
                        *grid = RaceGrid {
                            bots: 0,
                            lineup: None,
                            ..*grid
                        };
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
                        // Issue #99: the race's mirror and items, the same for everyone.
                        mirror.set_if_neq(MirrorMode(start.mirror));
                        items_on(&mut commands, start.items);
                        let slot = race.shared.slot_of(lobby.me());
                        // Issue #122: a race of the championship: its entrant on each slot (the
                        // host's bots spawn on theirs, the podium finds its three).
                        let lineup = lobby
                            .championship()
                            .filter(|_| start.round > 0)
                            .map(|state| grid_lineup(state, start));
                        *grid = RaceGrid {
                            bots: if lobby.is_host() {
                                usize::from(start.bots)
                            } else {
                                0
                            },
                            seed: start.seed,
                            lineup,
                            // Issue #153: the level of the room's bots (the host's spawn).
                            difficulty: BotDifficulty::from_index(start.difficulty)
                                .unwrap_or_default(),
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
                // Issue #101: on foot in the lobby: no kart of anyone (the characters are
                // `crate::salon`'s).
                (Showing::Salon, _) => sync.set_roster(std::iter::empty()),
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
                        // Issue #80: its name, for the tag over its kart.
                        let name = Some(profile.name.as_str()).filter(|name| !name.is_empty());
                        Some((*key, profile.loadout, slot, color, name))
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
            Driver::Bot => (
                race.bot_parts
                    .get(usize::from(kart.kart))
                    .copied()
                    .flatten(),
                None,
            ),
        };
        let bot = race.shared.start.bot_at(usize::from(kart.kart));
        Some(RosterEntry {
            kart: kart.kart,
            peer,
            loadout,
            color: colors[usize::from(kart.kart) % PALETTE_SIZE],
            name,
            bot: bot.map(|bot| bot as u8),
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
                let held_at = transforms.get(event.target).ok().map(|t| t.translation);
                if let Ok((transform, mut paw)) = proxies.get_mut(event.grabber) {
                    // Its arm leaves from the shoulder on the side of the kart held (issue #81).
                    let right = transform.forward().cross(Vec3::Y);
                    let side = held_at.map_or(1.0, |at| {
                        if right.dot(at - transform.translation) < 0.0 {
                            -1.0
                        } else {
                            1.0
                        }
                    });
                    paw.motion = PawMotion::Holding {
                        target: event.target,
                        ticks: 0,
                        rest: event.rest,
                        side,
                    };
                }
            }
            Message::TetherBreak(end) => {
                let Some(event) = end.to_event(entity) else {
                    continue;
                };
                // A remote grabber lets go in `interactions::end_tethers`, like ours (issue #81:
                // also when the end is ours).
                ends.write(event);
                counts[1] += 1;
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
