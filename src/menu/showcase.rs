//! The home screen's background (issue #89): a few karts racing on a circuit and striking and
//! grabbing each other, filmed by a trailer camera (`trailer.rs`). **It is the real game**: a race
//! of bots, the circuit and its decor, the items, the croquettes, the pelican, the paws with their
//! mini-ragdoll, « pof » and elastic arms. Only what differs from a race is here:
//!
//! - **few karts** ([`HomeTuning::karts`], 5): the grid's bots are ours while they spawn
//!   (`RaceGrid::bots`, given back at once), and the player's kart becomes a bot too (the
//!   player's animal, parts and color are in the trailer);
//! - **busier paws**: every kart drives with the Hard bots' skill, its paws' chances from
//!   [`HomeTuning`] (`bots::OwnSkill`), and **the karts stay together**: each one's top speed is
//!   pulled toward the pack ([`pack_scale`]), so that they keep coming in each other's reach;
//! - **an endless race** ([`SHOWCASE_LAPS`]): nobody finishes, nothing ends; it starts at GO;
//! - **no race screen**: the HUD is hidden (and no sound of the race plays: they are the player's
//!   kart's, and there is none); the trailer camera moves the game's camera after the chase.
//!
//! **When**: while the home screen is open, or a screen it opened (`MainMenu::on_home`), out of a
//! room ([`want_the_background`]). **Gone with the level**: any other [`LoadLevel`] (a race
//! started from the home, « Relancer », a room) builds another level, and this resource and its
//! karts go with the old one; the race is then built as usual, with the player's grid. Nothing of
//! it is left in a race (its systems only run with [`Showcase`]).
//!
//! **Only with a renderer**, in the menu's plugin: the headless world of the bot races has none of
//! it (no resource, no system, no component).

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext, Velocity};

use super::MainMenu;
use super::trailer::{self, Follow, Reel};
use crate::bots::{Bot, BotDifficulty, BotSkill, BotTuning, OwnSkill, bot, bot_seed};
use crate::camera::ChaseCamera;
use crate::interpolation::InterpolatedFrom;
use crate::kart::{KartState, MaxSpeedFactors, PlayerKart};
use crate::level::{LoadLevel, Online};
use crate::modes::TimeTrial;
use crate::race::{
    Course, GrandPrix, Race, RaceGrid, RaceMode, RaceProgress, Racer, RocketStart, Tow,
};
use crate::track::{GRID_SLOTS, TrackChoice};
use crate::ui::HudRoot;

/// The background's race never ends: this many laps (about ten hours on Prairie Pompon).
pub const SHOWCASE_LAPS: u32 = 1000;
/// The trailer camera stays this far in front of a wall or a hill between it and the karts (m).
const CLEARANCE: f32 = 0.5;

/// The home screen's background (issue #89, admin panel): how many karts, how often their paws
/// fly, how close together they stay, how long a shot of the camera lasts.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct HomeTuning {
    /// Karts behind the home screen (2 to 12; 4 to 6 are enough, and cost little). A change
    /// builds the background again.
    pub karts: usize,
    /// Share of the occasions (a kart coming in reach, the paw ready) a kart strikes at: 0.6 for
    /// the Hard bots of a race.
    pub strike_chance: f32,
    /// Share of the occasions it grabs a faster kart passing it: 0.7 for the Hard bots.
    pub grab_chance: f32,
    /// Held, a kart jumps free after this long (s): 0.25 for the Hard bots; longer, the elastic
    /// arms are seen.
    pub escape_delay: f32,
    /// The pack: a kart this far ahead of the pack's middle, along the track (m)...
    pub pack_gap: f32,
    /// ...has its top speed lowered by this share; as far behind, raised by it.
    pub pack_pull: f32,
    /// A shot of the trailer camera lasts this long (s)...
    pub shot_seconds: f32,
    /// ...and the next one comes in over this long, moving around the karts (s).
    pub blend_seconds: f32,
}

impl Default for HomeTuning {
    fn default() -> Self {
        Self {
            karts: 5,
            strike_chance: 0.9,
            grab_chance: 0.8,
            escape_delay: 0.9,
            pack_gap: 12.0,
            pack_pull: 0.25,
            shot_seconds: 6.0,
            blend_seconds: 2.0,
        }
    }
}

impl HomeTuning {
    /// The karts, within what the grid holds.
    pub fn kart_count(&self) -> usize {
        self.karts.clamp(2, GRID_SLOTS)
    }

    /// The skill of the background's karts: the Hard bots' (their driving, their throw), with
    /// these paws, and no aim at the walls (a kart pushed against one stops: the pack would
    /// break up).
    pub fn skill(&self, bots: &BotTuning) -> BotSkill {
        BotSkill {
            strike_chance: self.strike_chance.clamp(0.0, 1.0),
            grab_chance: self.grab_chance.clamp(0.0, 1.0),
            escape_delay: self.escape_delay.max(0.0),
            strike_edge_bias: 0.0,
            ..*bots.skill(BotDifficulty::Hard)
        }
    }
}

/// On every kart of the background.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ShowcaseKart;

/// Where the background is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowcasePhase {
    /// Its level is asked (the id of that `LoadLevel`).
    Requested(usize),
    /// Its level is being built (the circuit, the karts).
    Building,
    /// Its karts race.
    Running,
}

/// The home screen's background, while it is there (see the module's documentation).
#[derive(Resource, Debug)]
pub struct Showcase {
    pub phase: ShowcasePhase,
    /// The karts asked.
    pub karts: usize,
    /// The `LoadLevel` it asked for last.
    request: usize,
    /// The grid's own bots while ours spawn (`RaceGrid::bots` is ours meanwhile).
    grid_bots: Option<usize>,
    /// The trailer camera: its loop of shots, its springs, the karts it films, how much of its
    /// distance an obstacle leaves it.
    pub reel: Reel,
    follow: Option<Follow>,
    /// The two karts it films, and the two of the shot before (while the next shot comes in).
    pair: Option<(Entity, Entity)>,
    previous: Option<(Entity, Entity)>,
    kept: f32,
}

impl Showcase {
    pub fn running(&self) -> bool {
        self.phase == ShowcasePhase::Running
    }

    /// Whether the `LoadLevel` of id `id` is the background's own.
    pub fn asked(&self, id: usize) -> bool {
        self.request == id
    }

    /// The grid's own bots back, if ours are still on it (nobody changed it since).
    fn give_back(&mut self, grid: &mut RaceGrid) {
        if let Some(bots) = self.grid_bots.take()
            && grid.bots == self.karts - 1
        {
            grid.bots = bots;
        }
    }
}

/// Whether the background's karts race.
pub(super) fn background_running(showcase: Option<Res<Showcase>>) -> bool {
    showcase.is_some_and(|showcase| showcase.running())
}

/// Whether the background's level is being built.
pub(super) fn background_building(showcase: Option<Res<Showcase>>) -> bool {
    showcase.is_some_and(|showcase| showcase.phase == ShowcasePhase::Building)
}

/// The top speed factor of a kart `ahead` m ahead of the pack's middle (behind: negative): lower
/// ahead, higher behind, by up to `pull` from `gap` on.
pub fn pack_scale(ahead: f32, gap: f32, pull: f32) -> f32 {
    1.0 - pull.clamp(0.0, 0.9) * (ahead / gap.max(0.1)).clamp(-1.0, 1.0)
}

/// The home screen is there (or a screen it opened), out of a room: its background is asked,
/// once, on the circuit of the moment (a circuit: the default one after the flat test ground),
/// as a plain race (a Grand Prix or a time trial ends, the player's bots back); again when the
/// number of its karts changes. After the menu's screens (`PostUpdate`): the level it asks for is
/// the last one of the frame.
#[allow(clippy::too_many_arguments)]
pub(super) fn want_the_background(
    mut commands: Commands,
    menu: Res<MainMenu>,
    tuning: Res<HomeTuning>,
    showcase: Option<Res<Showcase>>,
    online: Option<Res<Online>>,
    choice: Res<TrackChoice>,
    mut grid: ResMut<RaceGrid>,
    time_trial: Option<Res<TimeTrial>>,
    mut levels: MessageWriter<LoadLevel>,
) {
    let karts = tuning.kart_count();
    let wanted = menu.on_home() && online.is_none();
    let again = match showcase.as_deref() {
        None => false,
        Some(showcase) if showcase.running() && showcase.karts != karts => true,
        Some(_) => return,
    };
    if !wanted {
        return;
    }
    commands.remove_resource::<GrandPrix>();
    if let Some(trial) = &time_trial {
        commands.remove_resource::<TimeTrial>();
        grid.bots = trial.solo_bots;
    }
    grid.lineup = None;
    let track = match &*choice {
        TrackChoice::Circuit(_) => choice.clone(),
        TrackChoice::TestGround => TrackChoice::default(),
    };
    let own = grid.bots;
    grid.bots = karts - 1;
    let request = levels.write(LoadLevel::on(track, RaceMode::Race)).id;
    if again {
        info!("home: its background again, with {karts} karts");
    }
    commands.insert_resource(Showcase {
        phase: ShowcasePhase::Requested(request),
        karts,
        request,
        grid_bots: Some(own),
        reel: Reel::default(),
        follow: None,
        pair: None,
        previous: None,
        kept: 1.0,
    });
}

/// Before the level is built (`PreUpdate`): the level asked last is the background's (it is
/// being built), or another one (the background goes with the old level; the grid's bots are
/// given back first).
pub(super) fn watch_the_levels(
    mut commands: Commands,
    mut levels: MessageReader<LoadLevel>,
    showcase: Option<ResMut<Showcase>>,
    mut grid: ResMut<RaceGrid>,
) {
    let Some(last) = levels.read_with_id().last().map(|(_, id)| id.id) else {
        return;
    };
    let Some(mut showcase) = showcase else {
        return;
    };
    if showcase.phase == ShowcasePhase::Requested(last) {
        showcase.phase = ShowcasePhase::Building;
        return;
    }
    showcase.give_back(&mut grid);
    commands.remove_resource::<Showcase>();
}

/// Its level built (the circuit, the race, the bots and the player's kart): every kart becomes a
/// background kart (the player's a bot), the grid's bots are given back, the race never ends and
/// starts at GO.
#[allow(clippy::too_many_arguments)]
pub(super) fn start_the_background(
    mut commands: Commands,
    mut showcase: ResMut<Showcase>,
    players: Query<(Entity, &Racer), With<PlayerKart>>,
    bots: Query<Entity, With<Bot>>,
    (race, course): (Option<ResMut<Race>>, Option<ResMut<Course>>),
    mut grid: ResMut<RaceGrid>,
    (tuning, bot_tuning): (Res<HomeTuning>, Res<BotTuning>),
    mut starts: Query<&mut RocketStart>,
) {
    let (Ok((player, racer)), Some(mut race), Some(mut course)) = (players.single(), race, course)
    else {
        return;
    };
    if bots.iter().count() + 1 < showcase.karts {
        return;
    }
    let skill = OwnSkill(tuning.skill(&bot_tuning));
    let seed = bot_seed(grid.seed, racer.grid_slot);
    commands.entity(player).remove::<PlayerKart>().insert((
        bot(BotDifficulty::Hard, seed, &bot_tuning),
        skill,
        ShowcaseKart,
    ));
    for kart in &bots {
        commands.entity(kart).insert((skill, ShowcaseKart));
    }
    showcase.give_back(&mut grid);
    course.laps = SHOWCASE_LAPS;
    let race = &mut *race;
    if race.flow.skip_countdown(&race.timings) {
        for mut start in &mut starts {
            start.set_if_neq(RocketStart::default());
        }
    }
    showcase.phase = ShowcasePhase::Running;
    info!("home: its background races, {} karts", showcase.karts);
}

/// The paws' chances follow the tuning (the admin panel), live.
pub(super) fn follow_the_tuning(
    tuning: Res<HomeTuning>,
    bot_tuning: Res<BotTuning>,
    mut skills: Query<&mut OwnSkill, With<ShowcaseKart>>,
) {
    if !tuning.is_changed() && !bot_tuning.is_changed() {
        return;
    }
    let skill = tuning.skill(&bot_tuning);
    for mut own in &mut skills {
        if own.0 != skill {
            own.0 = skill;
        }
    }
}

/// No race screen behind the home: the HUD is not laid out nor drawn.
pub(super) fn hide_the_race_screen(mut huds: Query<&mut Node, With<HudRoot>>) {
    for mut node in &mut huds {
        if node.display != Display::None {
            node.display = Display::None;
        }
    }
}

/// The karts stay together (`FixedUpdate`, after the bots' input and their catch-up, before the
/// driving reads the top speed): each one's catch-up is the pack's ([`pack_scale`]), from its race
/// distance to the pack's middle (without the karts the pelican carries).
pub(super) fn keep_the_pack_together(
    tuning: Res<HomeTuning>,
    mut karts: Query<(&RaceProgress, &Tow, &mut MaxSpeedFactors), With<ShowcaseKart>>,
) {
    let (sum, count) = karts
        .iter()
        .filter(|(_, tow, _)| !tow.is_carrying())
        .fold((0.0, 0.0), |(sum, n), (progress, ..)| {
            (sum + progress.distance, n + 1.0)
        });
    if count == 0.0 {
        return;
    }
    let middle = sum / count;
    for (progress, _, mut factors) in &mut karts {
        let scale = pack_scale(
            progress.distance - middle,
            tuning.pack_gap,
            tuning.pack_pull,
        );
        if factors.catch_up != scale {
            factors.catch_up = scale;
        }
    }
}

/// The trailer camera (`PostUpdate`, after the chase, before the water's tint): it films the two
/// karts closest to each other (chosen at each shot) and the karts around them, from its shot of
/// the moment, kept in front of the walls and hills, the karts framed on the right of the screen.
#[allow(clippy::too_many_arguments)]
pub(super) fn film_the_background(
    time: Res<Time>,
    tuning: Res<HomeTuning>,
    mut showcase: ResMut<Showcase>,
    rapier: ReadRapierContext,
    karts: Query<(&Velocity, &KartState), With<ShowcaseKart>>,
    visuals: Query<(&Transform, &InterpolatedFrom), Without<ChaseCamera>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<ChaseCamera>>,
    (mut seen, mut positions): (Local<Vec<(Entity, Vec3, Vec3, Vec3)>>, Local<Vec<Vec3>>),
) {
    seen.clear();
    for (transform, source) in &visuals {
        // Not one the pelican carries.
        if let Ok((velocity, state)) = karts.get(source.0)
            && *state != KartState::Respawning
        {
            seen.push((
                source.0,
                transform.translation,
                velocity.linear,
                *transform.forward(),
            ));
        }
    }
    if seen.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let showcase = &mut *showcase;
    let new_shot = showcase.reel.advance(dt, tuning.shot_seconds);
    positions.clear();
    positions.extend(seen.iter().map(|kart| kart.1));
    let index = |entity: Entity| seen.iter().position(|kart| kart.0 == entity);
    let indices =
        |pair: Option<(Entity, Entity)>| pair.and_then(|(a, b)| Some((index(a)?, index(b)?)));
    if new_shot {
        showcase.previous = showcase.pair;
    }
    let pair = indices(showcase.pair)
        .filter(|_| !new_shot)
        .or_else(|| trailer::closest_pair(&positions))
        .unwrap_or((0, 0));
    showcase.pair = Some((seen[pair.0].0, seen[pair.1].0));
    let mut aim = trailer::subject(&positions, pair);
    // The shot coming in: from the last pair's karts to this one's, smoothly.
    if let (Some(s), Some(before)) = (
        showcase.reel.blend_weight(tuning.blend_seconds),
        indices(showcase.previous),
    ) {
        aim = trailer::subject(&positions, before).lerp(aim, s);
    }
    let speed = (seen[pair.0].2 + seen[pair.1].2) * 0.5;
    let heading = if speed.length() > 2.0 {
        speed
    } else {
        seen[pair.0].3
    };
    let follow = showcase
        .follow
        .get_or_insert_with(|| Follow::at(aim, speed, heading));
    follow.step(aim, speed, heading, dt);
    let rig = showcase.reel.rig(tuning.shot_seconds, tuning.blend_seconds);
    let mut view = trailer::view(&rig, follow.focus, follow.heading);
    // A wall or a hill between the karts and the camera: in front of it.
    let from = follow.focus + Vec3::Y;
    let to = view.eye - from;
    let length = to.length();
    let wanted = match rapier.single() {
        Ok(context) if length > 1e-3 => context
            .cast_ray(
                from,
                to / length,
                length + CLEARANCE,
                true,
                QueryFilter::only_fixed(),
            )
            .map_or(1.0, |(_, toi)| (toi - CLEARANCE).max(0.0) / length),
        _ => 1.0,
    };
    showcase.kept = trailer::pull(showcase.kept, wanted, dt);
    view.eye = from + to * showcase.kept;
    let Ok((mut transform, mut projection)) = cameras.single_mut() else {
        return;
    };
    let aspect = match &*projection {
        Projection::Perspective(perspective) => perspective.aspect_ratio,
        _ => 16.0 / 9.0,
    };
    let view = trailer::framed(view, aspect, trailer::FRAME_X);
    *transform = Transform::from_translation(view.eye).looking_at(view.look_at, Vec3::Y);
    if let Projection::Perspective(perspective) = &mut *projection {
        perspective.fov = view.fov_degrees.to_radians();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pack: a kart ahead of its middle slows down, one behind speeds up, by at most
    /// `pack_pull`, fully from `pack_gap` on; in the middle, nothing.
    #[test]
    fn the_karts_ahead_wait_and_the_karts_behind_catch_up() {
        let t = HomeTuning::default();
        assert_eq!(pack_scale(0.0, t.pack_gap, t.pack_pull), 1.0);
        assert!(
            (pack_scale(t.pack_gap, t.pack_gap, t.pack_pull) - (1.0 - t.pack_pull)).abs() < 1e-6
        );
        assert!(
            (pack_scale(-t.pack_gap, t.pack_gap, t.pack_pull) - (1.0 + t.pack_pull)).abs() < 1e-6
        );
        assert_eq!(
            pack_scale(10.0 * t.pack_gap, t.pack_gap, t.pack_pull),
            pack_scale(t.pack_gap, t.pack_gap, t.pack_pull)
        );
        let half = pack_scale(t.pack_gap / 2.0, t.pack_gap, t.pack_pull);
        assert!(half > 1.0 - t.pack_pull && half < 1.0);
        // Never stopped nor doubled, whatever the tuning.
        assert!(pack_scale(100.0, 0.0, 5.0) >= 0.1);
        assert!(pack_scale(-100.0, 0.0, 5.0) <= 1.9);
    }

    /// The background's karts drive like the Hard bots, with busier paws (the tuning's); 4 to 6
    /// karts by default, never more than the grid holds.
    #[test]
    fn the_background_karts_are_hard_bots_with_busy_paws() {
        let t = HomeTuning::default();
        assert!((4..=6).contains(&t.kart_count()));
        assert_eq!(
            HomeTuning {
                karts: 40,
                ..t.clone()
            }
            .kart_count(),
            GRID_SLOTS
        );
        assert_eq!(
            HomeTuning {
                karts: 0,
                ..t.clone()
            }
            .kart_count(),
            2
        );
        let bots = BotTuning::default();
        let hard = *bots.skill(BotDifficulty::Hard);
        let skill = t.skill(&bots);
        assert!(skill.strike_chance > hard.strike_chance);
        assert!(skill.grab_chance > hard.grab_chance);
        assert!(skill.escape_delay > hard.escape_delay, "the arms are seen");
        assert_eq!(skill.line_fidelity, hard.line_fidelity);
        assert_eq!(skill.throw, hard.throw);
        assert!(t.blend_seconds < t.shot_seconds);
    }
}
