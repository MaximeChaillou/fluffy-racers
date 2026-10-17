//! Issue #101: the lobby's characters online (`net::foot`, `docs/ARCHITECTURE.md` § « Le salon
//! en 3D »).
//!
//! - **Ours**: after each tick, its position goes into its history (the victim's check), and
//!   every second tick its state goes out (`net::FootState`, [`send_our_walker`]).
//! - **The others'**: one per other player of the room ([`follow_the_room`]: the lobby's seats,
//!   their animal, color and pseudo), once its first state came: a kinematic body ours bumps
//!   into ([`RemoteWalkerBody`], placed each tick where its states say) and a visual shown
//!   between its states each frame ([`show_remote_walkers`]), with the look of the state shown
//!   (tumbling, getting up, its paw).
//! - **Strikes** ([`take_strikes`]): those on ours, the victim decides: young enough, in reach of
//!   where ours was at the tick the striker saw, not down nor protected: it falls over. Everyone
//!   sees it by its states.
//! - **Back to the lobby after a race** ([`back_to_the_salon`]): the host, once it closed the
//!   race's results and they showed for `SalonTuning::back_after_results`, sends the room's
//!   settings again (`net::NetAction::ChooseSetup`): the race is over for everyone, and everyone
//!   is back in the lobby. Issue #122: not in a championship, which comes back after its podium.
//! - **The championship** (issue #122, [`show_the_championship`], [`run_the_championship`]):
//!   every peer shows the host's championship (`race::GrandPrix`, the screens and the podium of
//!   `crate::modes`: the standings after each race, the podium after the last); the host moves it
//!   on once the standings showed for `SalonTuning::standings_seconds` (or at its Entrée): the
//!   next race, or the podium; after the podium (`SalonTuning::podium_seconds`), the room's
//!   settings again (a new draw for a championship at random): everyone back in the lobby.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{RigidBody, Velocity};

use super::SalonTuning;
use super::walker::{PlayerWalker, Walker, WalkerHistory, WalkerLook, WalkerVisual, walker_body};
use crate::characters::{
    Knock, PlayerColor, PlayerLoadout, Ragdoll, RagdollPhase, RagdollTuning, Roster,
};
use crate::modes::Catalog;
use crate::net::championship::after_a_championship;
use crate::net::foot::{FootPose, direction_of, position_of};
use crate::net::{
    FootLook, FootState, FootSync, KartSync, NetAction, NetSession, NetSettings, NetStatus,
    NetTuning, OnlineRace, PeerKey, TICK_MS, WalkerEntry, character_name, room_seed, strike_lands,
};
use crate::race::{GrandPrix, GrandPrixTuning, RESTART_BUTTONS, RESTART_KEYS};

/// The kart states' session the lobby's karts would have (none: the characters on foot have
/// their own messages): free driving's.
pub const SALON_SESSION: u8 = 0;

/// The body of another player's character: kinematic, placed where its states say.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteWalkerBody {
    pub peer: PeerKey,
}

/// The local virtual time (ms): the playbacks' clock.
fn virtual_ms(time: &Time<Virtual>) -> f64 {
    time.elapsed_secs_f64() * 1_000.0
}

/// In a room, seated: the lobby's characters on the network ([`FootSync`], from our id), the
/// other players' (their animal, color and pseudo), their playbacks; ours follows its player's
/// animal, color and pseudo. Out of a room: none of it.
#[allow(clippy::too_many_arguments)]
pub fn follow_the_room(
    mut commands: Commands,
    session: Option<Res<NetSession>>,
    mut foot: Option<ResMut<FootSync>>,
    (tuning, salon): (Res<NetTuning>, Res<SalonTuning>),
    time: Res<Time<Virtual>>,
    (roster, loadout, color, settings): (
        Res<Roster>,
        Res<PlayerLoadout>,
        Option<Res<PlayerColor>>,
        Res<NetSettings>,
    ),
    mut ours: Query<&mut Walker, With<PlayerWalker>>,
) {
    let lobby = session
        .as_deref()
        .filter(|session| session.status == NetStatus::InRoom)
        .and_then(|session| session.lobby.as_ref())
        .filter(|lobby| lobby.seated());
    // Ours: its player's animal, color and pseudo (« Pilote » can change them here).
    let me = lobby.map_or(PeerKey(0), |lobby| lobby.me());
    let our_color = lobby
        .and_then(|lobby| lobby.color_of(me))
        .or(color.map(|c| c.0))
        .unwrap_or_default();
    for mut walker in &mut ours {
        let name = settings
            .pseudo
            .clone()
            .unwrap_or_else(|| character_name(Some(&loadout.0), Some(&roster)));
        if walker.character != loadout.character
            || walker.color != our_color
            || walker.name != name
            || walker.peer != me
        {
            walker.character = loadout.character;
            walker.size = roster
                .characters
                .get(loadout.character)
                .map_or(1.0, |c| c.size);
            walker.color = our_color;
            walker.name = name;
            walker.peer = me;
        }
    }
    let Some(lobby) = lobby else {
        if foot.is_some() {
            commands.remove_resource::<FootSync>();
        }
        return;
    };
    let Some(foot) = foot.as_deref_mut().filter(|foot| foot.me() == lobby.me()) else {
        commands.insert_resource(FootSync::new(lobby.me()));
        return;
    };
    foot.set_walkers(lobby.seats().iter().filter_map(|key| {
        let profile = lobby.peer(*key)?.profile.as_ref()?;
        Some(WalkerEntry {
            peer: *key,
            character: usize::from(profile.loadout[0]),
            color: lobby.color_of(*key).unwrap_or(profile.color),
            name: &profile.name,
        })
    }));
    foot.update(virtual_ms(&time), time.delta_secs_f64() * 1_000.0, &tuning);
    for entity in foot.take_gone() {
        commands.entity(entity).try_despawn();
    }
    let limits = tuning.limits();
    let now = virtual_ms(&time);
    for walker in foot.walkers_mut() {
        if walker.respawn {
            walker.respawn = false;
            for entity in walker.body.take().iter().chain(walker.visual.take().iter()) {
                commands.entity(*entity).try_despawn();
            }
        }
        if walker.body.is_some() {
            continue;
        }
        let Some((pose, look)) = walker.shown(now, &limits) else {
            continue;
        };
        let size = roster
            .characters
            .get(walker.character)
            .map_or(1.0, |c| c.size);
        let body = commands
            .spawn((
                Name::new(format!("Salon : {} #{}", walker.name, walker.peer.tag())),
                Walker {
                    peer: walker.peer,
                    character: walker.character,
                    color: walker.color,
                    name: walker.name.clone(),
                    size,
                },
                RemoteWalkerBody { peer: walker.peer },
                walker_body(pose.position, 0.0, size, &salon),
            ))
            // Kinematic: placed where its states say; ours bumps into it.
            .insert((
                RigidBody::KinematicPositionBased,
                Transform::from_translation(pose.position).with_rotation(pose.rotation),
                look_of(&look, pose.velocity, 0),
            ))
            .remove::<Ragdoll>()
            .id();
        let visual = commands
            .spawn((
                Name::new(format!("Salon : {} (visuel)", walker.name)),
                WalkerVisual { body },
                Transform::from_translation(pose.position).with_rotation(pose.rotation),
                Visibility::default(),
            ))
            .id();
        walker.body = Some(body);
        walker.visual = Some(visual);
    }
    // Their animal, color and pseudo, as the lobby has them (in place).
    for remote in foot.walkers() {
        let Some(body) = remote.body else {
            continue;
        };
        commands.queue(update_walker(body, remote.color, remote.name.clone()));
    }
}

/// The lobby's scene gone (a race's level): its characters on the network go too.
pub fn forget_the_room(mut commands: Commands) {
    commands.remove_resource::<FootSync>();
}

/// A command: the character of `body` wears `color` and writes `name` (if they changed).
fn update_walker(
    body: Entity,
    color: crate::kart::KartColor,
    name: String,
) -> impl FnOnce(&mut World) {
    move |world: &mut World| {
        if let Some(mut walker) = world.get_mut::<Walker>(body)
            && (walker.color != color || walker.name != name)
        {
            walker.color = color;
            walker.name = name;
        }
    }
}

/// The look of a character from its state's: its speed from its velocity, its falls counted.
fn look_of(look: &FootLook, velocity: Vec3, knocks: u32) -> WalkerLook {
    WalkerLook {
        phase: look.phase,
        phase_ticks: look.phase_ticks,
        airborne: look.airborne,
        swing: look.paw,
        speed: Vec3::new(velocity.x, 0.0, velocity.z).length(),
        velocity,
        knocks,
    }
}

/// The strikes on ours (`FootSync`: those the others sent): its ragdoll decides, from where it
/// was at the tick the striker saw.
pub fn take_strikes(
    tuning: Res<SalonTuning>,
    ragdoll_tuning: Res<RagdollTuning>,
    time: Res<Time<Fixed>>,
    sync: Option<Res<KartSync>>,
    foot: Option<ResMut<FootSync>>,
    mut ours: Query<(&WalkerHistory, &mut Ragdoll), With<PlayerWalker>>,
) {
    let (Some(mut foot), Some(sync)) = (foot, sync) else {
        return;
    };
    let local = (time.elapsed_secs_f64() * crate::TICK_HZ).round() as u32;
    let now = sync.clock.common(local);
    for (_, strike) in foot.drain_strikes_in() {
        let Some(now) = now else {
            continue;
        };
        for (history, mut ragdoll) in &mut ours {
            let seen = history.0.position_at(strike.seen);
            let reach = tuning.paw_reach + tuning.reach_tolerance;
            if !strike_lands(&strike, now, seen, reach, tuning.max_strike_age_ms) {
                // Why (rare: no cost to speak of), for the console.
                let age_ms = f64::from(now.saturating_sub(strike.tick)) * crate::net::TICK_MS;
                let from = position_of(strike.position);
                match seen {
                    Some(seen) => info!(
                        "lobby: a strike on us refused: {age_ms:.0} ms old, from {:.2} m of where we were",
                        Vec2::new(seen.x - from.x, seen.z - from.z).length()
                    ),
                    None => info!(
                        "lobby: a strike on us refused: {age_ms:.0} ms old, where we were then is forgotten"
                    ),
                }
                continue;
            }
            if ragdoll.knock(
                Knock {
                    direction: direction_of(strike.direction),
                    strength: 1.0,
                    twist_left: strike.tick.is_multiple_of(2),
                },
                &ragdoll_tuning,
            ) {
                info!("lobby: struck, we fall over");
            }
        }
    }
}

/// The others' bodies, each tick: where their states say at the end of this tick (kinematic:
/// ours bumps into them, they are never pushed).
pub fn move_remote_walkers(
    foot: Option<Res<FootSync>>,
    tuning: Res<NetTuning>,
    time: Res<Time<Fixed>>,
    mut bodies: Query<(&mut Transform, &mut Velocity), With<RemoteWalkerBody>>,
) {
    let Some(foot) = foot else {
        return;
    };
    let now = time.elapsed_secs_f64() * 1_000.0;
    let limits = tuning.limits();
    for walker in foot.walkers() {
        let Some(body) = walker.body else {
            continue;
        };
        let Ok((mut transform, mut velocity)) = bodies.get_mut(body) else {
            continue;
        };
        let Some((pose, _)) = walker.shown(now, &limits) else {
            continue;
        };
        transform.translation = pose.position;
        transform.rotation = pose.rotation;
        velocity.linear = pose.velocity;
    }
}

/// Ours after each tick: its position in its history, and every second tick its state to the
/// others (with the room's common tick).
pub fn send_our_walker(
    time: Res<Time<Fixed>>,
    tuning: Res<NetTuning>,
    sync: Option<Res<KartSync>>,
    foot: Option<ResMut<FootSync>>,
    mut ours: Query<(&Transform, &Velocity, &WalkerLook, &mut WalkerHistory), With<PlayerWalker>>,
) {
    let (Some(mut foot), Some(sync)) = (foot, sync) else {
        return;
    };
    let local = (time.elapsed_secs_f64() * crate::TICK_HZ).round() as u32;
    let Some(tick) = sync.clock.common(local) else {
        return;
    };
    for (transform, velocity, look, mut history) in &mut ours {
        history.0.record(tick, transform.translation);
        if !local.is_multiple_of(tuning.send_interval_ticks.max(1)) {
            continue;
        }
        let pose = FootPose {
            tick,
            position: transform.translation,
            rotation: transform.rotation,
            velocity: velocity.linear,
            spin: velocity.angular,
            look: FootLook {
                phase: look.phase,
                phase_ticks: look.phase_ticks,
                airborne: look.airborne,
                paw: look.swing,
            },
        };
        foot.send(FootState::quantize(&pose));
    }
}

/// The others' visuals, each frame: where their states say, a tick behind (like ours, shown
/// between its last two ticks), with the look of the state shown; a new fall counted.
pub fn show_remote_walkers(
    foot: Option<Res<FootSync>>,
    tuning: Res<NetTuning>,
    time: Res<Time<Virtual>>,
    mut visuals: Query<&mut Transform, With<WalkerVisual>>,
    mut looks: Query<&mut WalkerLook, With<RemoteWalkerBody>>,
) {
    let Some(foot) = foot else {
        return;
    };
    let now = virtual_ms(&time) - TICK_MS;
    let limits = tuning.limits();
    for walker in foot.walkers() {
        let (Some(body), Some(visual)) = (walker.body, walker.visual) else {
            continue;
        };
        let Some((pose, look)) = walker.shown(now, &limits) else {
            continue;
        };
        if let Ok(mut transform) = visuals.get_mut(visual) {
            transform.translation = pose.position;
            transform.rotation = pose.rotation;
        }
        if let Ok(mut shown) = looks.get_mut(body) {
            let fell =
                look.phase == RagdollPhase::Tumbling && shown.phase != RagdollPhase::Tumbling;
            let knocks = shown.knocks + u32::from(fell);
            shown.set_if_neq(look_of(&look, pose.velocity, knocks));
        }
    }
}

/// The console's report of the lobby, this often (s of game time).
const REPORT_SECONDS: f64 = 2.0;

/// Every few seconds in the lobby, in the console: where our character is, and where each other
/// one is shown, with its states received (to compare two screens, and for Max's reports).
pub fn report_walkers(
    time: Res<Time<Virtual>>,
    foot: Option<Res<FootSync>>,
    ours: Query<(&Transform, &Ragdoll), With<PlayerWalker>>,
    bodies: Query<(&Transform, &WalkerLook), With<RemoteWalkerBody>>,
    mut next: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now < *next {
        return;
    }
    *next = now + REPORT_SECONDS;
    for (transform, ragdoll) in &ours {
        let at = transform.translation;
        info!(
            "lobby: our character at ({:.2}, {:.2}, {:.2}), {:?}",
            at.x, at.y, at.z, ragdoll.phase
        );
    }
    let Some(foot) = foot else {
        return;
    };
    for walker in foot.walkers() {
        match walker.body.and_then(|body| bodies.get(body).ok()) {
            Some((transform, look)) => {
                let at = transform.translation;
                info!(
                    "lobby: {} shown at ({:.2}, {:.2}, {:.2}), {:?}, {} states",
                    walker.name, at.x, at.y, at.z, look.phase, walker.received
                );
            }
            None => info!(
                "lobby: {}: not shown yet, {} states",
                walker.name, walker.received
            ),
        }
    }
}

/// While the race's results are on: we host it, the lobby is ours (a renderer).
pub fn results_shown(race: Option<Res<OnlineRace>>, session: Option<Res<NetSession>>) -> bool {
    race.is_some_and(|race| race.shared.results().is_some())
        && session.is_some_and(|session| session.hosting)
}

/// The host, once the race's results are closed and showed for a while: the room's settings
/// again, for everyone (the race is over, everyone is back in the lobby).
pub fn back_to_the_salon(
    tuning: Res<SalonTuning>,
    race: Res<OnlineRace>,
    session: Res<NetSession>,
    time: Res<Time<Real>>,
    mut since: Local<Option<(u16, f64)>>,
    mut actions: MessageWriter<NetAction>,
) {
    let Some(lobby) = session.lobby.as_ref().filter(|lobby| lobby.is_host()) else {
        return;
    };
    // Only for the race still announced (a new one starts over); issue #122, not in a
    // championship (back after its podium, `run_the_championship`).
    if lobby
        .announced()
        .is_none_or(|start| start.race != race.shared.race())
        || lobby.championship().is_some()
    {
        return;
    }
    let now = time.elapsed_secs_f64();
    let number = race.shared.race();
    let start = match *since {
        Some((race, at)) if race == number => at,
        _ => {
            *since = Some((number, now));
            now
        }
    };
    if now - start >= f64::from(tuning.back_after_results.max(0.0)) {
        info!("lobby: race {number} over, everyone back in the lobby");
        actions.write(NetAction::ChooseSetup(lobby.setup()));
        *since = Some((number, f64::INFINITY));
    }
}

/// Issue #122: we host the room.
pub fn hosting(session: Option<Res<NetSession>>) -> bool {
    session.is_some_and(|session| session.hosting)
}

/// What [`show_the_championship`] last showed: the race (its number), the races scored, the
/// podium.
type Shown = (u16, u8, bool);

/// Issue #122: the name of a player of the championship we never knew (it left before we came).
pub const GONE: &str = "Joueur parti";

/// Issue #122: the championship in progress, as this peer shows it: the host's state for the race
/// of it we race (`race::GrandPrix`, online: our entrant, the names, the host's standings; the
/// screens and the podium of `crate::modes` follow its stage). Built again when the host tells
/// something new or another race is on; gone with the championship (the lobby again). In
/// `PreUpdate`: the screens of `Update` all see the new stage on the same frame.
#[allow(clippy::too_many_arguments)]
pub fn show_the_championship(
    mut commands: Commands,
    session: Res<NetSession>,
    race: Option<Res<OnlineRace>>,
    grand_prix: Option<Res<GrandPrix>>,
    cups: Option<Res<GrandPrixTuning>>,
    mut shown: Local<Option<Shown>>,
    mut catalog: Local<Option<Catalog>>,
) {
    let lobby = session.lobby.as_ref().filter(|lobby| lobby.seated());
    let state = lobby.and_then(|lobby| lobby.championship());
    let start = race
        .as_deref()
        .filter(|race| race.racing)
        .map(|race| &race.shared.start)
        .filter(|start| start.round > 0);
    let (Some(lobby), Some(state), Some(start)) = (lobby, state, start) else {
        if grand_prix.is_some_and(|gp| gp.online.is_some()) {
            commands.remove_resource::<GrandPrix>();
        }
        *shown = None;
        return;
    };
    let key = (start.race, state.scored, state.podium);
    if *shown == Some(key) && grand_prix.is_some() {
        return;
    }
    *shown = Some(key);
    let catalog = catalog.get_or_insert_with(|| {
        Catalog::game(cups.as_deref().map_or(4, |tuning| tuning.races as usize))
    });
    // The names: a bot's number, a player's pseudo (one who left: the one shown before).
    let before = grand_prix
        .as_deref()
        .and_then(|gp| gp.online.as_ref())
        .map(|online| online.names.clone())
        .unwrap_or_default();
    let names = (0..state.entrants())
        .map(|entrant| match state.player_of(entrant) {
            None => crate::net::championship::bot_name(entrant),
            Some(peer) if peer == lobby.me() => lobby.profile().name.clone(),
            Some(peer) => lobby
                .peer(peer)
                .and_then(|p| p.profile.as_ref())
                .map(|profile| profile.name.clone())
                .filter(|name| !name.is_empty())
                .or_else(|| before.get(entrant).cloned())
                .unwrap_or_else(|| GONE.to_string()),
        })
        .collect();
    let me = state.entrant_of(lobby.me());
    let view = crate::net::championship::standings_view(
        state,
        catalog,
        start.round,
        names,
        me,
        lobby.is_host(),
    );
    info!(
        "lobby: the championship, race {}/{}: {:?}",
        start.round,
        state.setup.program.len(),
        view.stage
    );
    commands.insert_resource(view);
}

/// The stage of the championship the host waits on, and since when (real time, s); `true` once
/// it moved it on.
type Waiting = (Shown, f64, bool);

/// Issue #122, the host: once a race of the championship was scored (its standings shown) for
/// `SalonTuning::standings_seconds`, or at its Entrée (Espace, A, Start): its next race, or the
/// podium after the last; once the podium showed for `SalonTuning::podium_seconds` (or at its
/// Entrée), the room's settings again (a championship at random drawn anew): everyone back in
/// the lobby. Not while another screen has the keys.
#[allow(clippy::too_many_arguments)]
pub fn run_the_championship(
    tuning: Res<SalonTuning>,
    mut session: ResMut<NetSession>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    gamepads: Query<&Gamepad>,
    (main_menu, options, selection): (
        Option<Res<crate::menu::MainMenu>>,
        Option<Res<crate::options::OptionsMenu>>,
        Option<Res<crate::characters::SelectionScreen>>,
    ),
    cups: Option<Res<GrandPrixTuning>>,
    time: Res<Time<Real>>,
    mut waiting: Local<Option<Waiting>>,
    mut actions: MessageWriter<NetAction>,
) {
    let seed = room_seed(&session.room);
    let Some(lobby) = session.lobby.as_mut().filter(|lobby| lobby.is_host()) else {
        return;
    };
    let (Some(state), Some(start)) = (lobby.championship(), lobby.announced()) else {
        *waiting = None;
        return;
    };
    // Its race scored (the standings), or its podium.
    if start.round == 0 || (state.scored < start.round && !state.podium) {
        return;
    }
    let key = (start.race, state.scored, state.podium);
    let now = time.elapsed_secs_f64();
    let (since, done) = match *waiting {
        Some((shown, since, done)) if shown == key => (since, done),
        _ => {
            *waiting = Some((key, now, false));
            (now, false)
        }
    };
    if done {
        return;
    }
    let menus = main_menu.is_some_and(|m| m.open)
        || options.is_some_and(|o| o.open)
        || selection.is_some_and(|s| s.open);
    let pressed = !menus
        && (keys.is_some_and(|keys| keys.any_just_pressed(RESTART_KEYS))
            || gamepads
                .iter()
                .any(|gamepad| gamepad.any_just_pressed(RESTART_BUTTONS)));
    let wait = if state.podium {
        tuning.podium_seconds
    } else {
        tuning.standings_seconds
    };
    if now - since < f64::from(wait.max(0.0)) && !pressed {
        return;
    }
    *waiting = Some((key, since, true));
    let finished = usize::from(state.scored) >= state.setup.program.len();
    if state.podium {
        let catalog = Catalog::game(cups.as_deref().map_or(4, |tuning| tuning.races as usize));
        let next = after_a_championship(lobby.setup(), seed, &catalog);
        info!("lobby: the championship is over, everyone back in the lobby");
        actions.write(NetAction::ChooseSetup(next));
    } else if finished {
        info!("lobby: the championship's podium");
        lobby.championship_podium();
    } else {
        info!("lobby: the championship's next race");
        actions.write(NetAction::StartRace);
    }
}
