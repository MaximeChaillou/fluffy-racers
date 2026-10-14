//! Bots (GD § 12, issue #6): karts driven by the computer, to fill the grid of 12.
//!
//! A bot is a kart with a [`Bot`] component. Every tick, in `KartSystems::Input` like the
//! keyboard for the player, its driver (`driver.rs`, pure logic) writes the kart's `KartInput`
//! from what it sees: its position on the racing line of the circuit (`line.rs`), the turns
//! ahead, its drift charge and the karts around. It never touches the kart otherwise: the kart
//! rules, the race rules and the tow pelican treat it like any racer. The only extra is the
//! light catch-up of GD § 12, a top speed factor (`MaxSpeedFactors::catch_up`) within ±3 %.
//!
//! Three difficulties ([`BotDifficulty`]): trajectory precision, drift tier aimed for and
//! anticipation of the turns, all in [`BotTuning`]. Each bot also has its own lane, pace and
//! random choices from a deterministic generator seeded by the race seed and its grid slot
//! ([`bot_seed`]: variety, no single file, reproducible tests). They also try the rocket start
//! and the tricks (issue #13), through their `KartInput` like a player. They take item boxes
//! (a little detour on the straights when a slot is free) and use their items (issue #16,
//! `items.rs`): the item button of their `KartInput` too, more or less often with their level
//! (`BotSkill::item_delay`). And they use their paw (issue #34, `interactions.rs`): a strike in a
//! share of the occasions, a grab on a faster kart passing, a jump to break free when held, with
//! their level (`BotSkill::strike_chance`, `grab_chance`, `escape_delay`).
//!
//! Each bot also has its own character and parts (issue #15, [`bot_loadout`]): the weight
//! classes take turns along the grid, so that the 9 animals line up and every stretch of the
//! grid mixes light, medium and heavy karts; the kart, wheels and wing come from a generator
//! seeded by its seed. A bot drives with its kart's own tuning (its stats applied): it plans its
//! drifts with its own radii and aims at its own tiers.
//!
//! The race seed (issue #40, `RaceGrid::seed`) draws every bot of a race: same seed, same bots.
//! The game keeps one seed (`RaceGrid::GAME_SEED`, the bots of every race before #40); the race
//! tests run several, and the network will share the host's.

mod driver;
mod interactions;
mod items;
mod line;
mod rng;
mod tuning;

#[cfg(test)]
mod scenario_tests;

pub use driver::{BotBrain, DriftPlan, Neighbor, Senses, catch_up_scale, charge_per_radian, drive};
pub use interactions::{BotPawBrain, PawButtons, PawSenses, ReachKart, paw_buttons};
pub use items::{BotItemBrain, ItemButtons, ItemSenses, item_buttons};
pub use line::{Located, RacingLine, Turn};
pub use rng::BotRng;
pub use tuning::{BotDifficulty, BotSkill, BotTuning};

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::TICK_HZ;
use crate::characters::{DriverLooks, Loadout, PartKind, Roster, WeightClass};
use crate::croquettes::{CroquetteTuning, Croquettes};
use crate::interactions::{InteractionTuning, Paw, PawMotion};
use crate::items::{ItemBox, ItemSlots, ItemTuning, Slot};
use crate::kart::{
    Boost, Kart, KartAssists, KartInput, KartState, KartSystems, KartTuning, KartVisualAssets,
    MaxSpeedFactors, MaxSpeedScale, PlayerKart, Remote, StatEffects, StatsTuning, TrickTracker,
    kart_body_with_stats, spawn_kart_visual_with_driver,
};
use crate::race::{DrivesOnAfterFinish, Race, RaceGrid, RaceMode, RaceProgress, Racer};
use crate::track::TrackLayout;

/// A kart driven by the computer.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bot {
    pub difficulty: BotDifficulty,
}

/// Body colors of the karts by grid slot (primitive placeholders until the real models, #11).
/// The player's kart is pink.
pub const KART_COLORS: [Color; 12] = [
    Color::srgb(0.90, 0.20, 0.20), // red
    Color::srgb(1.00, 0.55, 0.15), // orange
    Color::srgb(0.95, 0.85, 0.20), // yellow
    Color::srgb(0.55, 0.85, 0.25), // lime
    Color::srgb(0.15, 0.60, 0.30), // green
    Color::srgb(0.10, 0.70, 0.70), // teal
    Color::srgb(0.35, 0.70, 1.00), // sky blue
    Color::srgb(0.20, 0.30, 0.85), // blue
    Color::srgb(0.55, 0.30, 0.85), // violet
    Color::srgb(0.55, 0.35, 0.20), // brown
    Color::srgb(0.95, 0.95, 0.95), // white
    Color::srgb(0.25, 0.25, 0.30), // charcoal
];

/// Color of the bot on grid slot `slot`.
pub fn bot_color(slot: usize) -> Color {
    KART_COLORS[slot % KART_COLORS.len()]
}

/// Seed of the bot on grid slot `slot` in a race of seed `race_seed` (issue #40,
/// `RaceGrid::seed`): all its random draws come from it (its driving, its rocket starts and
/// tricks, its kart parts). With the game's seed (`RaceGrid::GAME_SEED` = 0), the seeds the
/// bots always had: one per slot (issue #6). Another race seed changes the seed of every bot.
pub fn bot_seed(race_seed: u64, slot: usize) -> u64 {
    (0xF1_0FF1 + slot as u64) ^ race_seed.wrapping_mul(RACE_SEED_SPREAD)
}

/// Spreads a race seed over all 64 bits (an odd constant, the golden ratio): neighbouring race
/// seeds give unrelated bot seeds, and race seed 0 leaves them as they were.
const RACE_SEED_SPREAD: u64 = 0x9E37_79B9_7F4A_7C15;

/// Mixed into the seed of a bot's parts (their own generator: drawing them does not
/// shift the bot's driving choices).
const LOADOUT_SEED: u64 = 0x15_CA27_5EED;

/// Weight class of the bot on grid slot `slot` (issue #15): the classes take turns, shifted by
/// one every three slots (light, medium, heavy, then medium, heavy, light, then heavy, light,
/// medium...). Any three neighbouring slots, and the slots taken one in three, mix the three
/// classes: no stretch of the grid is all heavy or all light.
pub fn bot_class(slot: usize) -> WeightClass {
    WeightClass::ALL[(slot + slot / 3) % 3]
}

/// The character and parts of the bot on grid slot `slot` in a race of seed `race_seed`
/// (issue #15). Its character: the next animal of its weight class ([`bot_class`]), so the
/// first nine slots show the nine animals, whatever the seed; its kart, wheels and wing: drawn
/// from a generator seeded by its [`bot_seed`]. Always the same for a slot and a race seed.
pub fn bot_loadout(roster: &Roster, race_seed: u64, slot: usize) -> Loadout {
    let class = bot_class(slot);
    let turn = (0..slot).filter(|s| bot_class(*s) == class).count();
    let animals: Vec<usize> = roster.characters_of(class).collect();
    let mut loadout = roster.default_loadout();
    if !animals.is_empty() {
        loadout.character = animals[turn % animals.len()];
    }
    let mut rng = BotRng::new(bot_seed(race_seed, slot) ^ LOADOUT_SEED);
    for kind in [PartKind::Kart, PartKind::Wheels, PartKind::Wing] {
        let count = roster.count(kind).max(1) as u64;
        loadout.set(kind, (rng.next_u64() % count) as usize);
    }
    loadout
}

/// What turns a racing kart into a bot of `difficulty`, drawing with `seed` (its [`bot_seed`]).
/// Bots keep driving after the finish line, so that they do not block it.
pub fn bot(difficulty: BotDifficulty, seed: u64, tuning: &BotTuning) -> impl Bundle {
    (
        Bot { difficulty },
        BotBrain::new(seed, tuning),
        BotItemBrain::new(seed),
        BotPawBrain::new(seed),
        DrivesOnAfterFinish,
    )
}

pub struct BotPlugin;

impl Plugin for BotPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BotTuning>()
            .add_systems(
                Update,
                // No bot in free driving (admin panel, issue #36).
                (build_racing_line, spawn_bots)
                    .run_if(resource_added::<TrackLayout>)
                    .run_if(not(resource_equals(RaceMode::FreeRoam))),
            )
            .add_systems(
                FixedUpdate,
                (
                    catch_up.run_if(resource_exists::<Race>),
                    drive_bots.run_if(resource_exists::<RacingLine>),
                )
                    .in_set(KartSystems::Input),
            );
    }
}

fn build_racing_line(mut commands: Commands, layout: Res<TrackLayout>, tuning: Res<BotTuning>) {
    commands.insert_resource(RacingLine::new(
        &layout,
        tuning.road_margin,
        tuning.sand_margin,
        tuning.line_margin,
    ));
}

/// Fills the front of the grid with bots once the circuit is built (`RaceGrid`), each with its
/// own character and parts ([`bot_loadout`]) and its seed, from the race seed.
fn spawn_bots(
    mut commands: Commands,
    layout: Res<TrackLayout>,
    grid: Res<RaceGrid>,
    tuning: Res<BotTuning>,
    kart_tuning: Res<KartTuning>,
    stats_tuning: Res<StatsTuning>,
    roster: Res<Roster>,
    looks: Option<Res<DriverLooks>>,
    visuals: Option<Res<KartVisualAssets>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    for slot in grid.bot_slots() {
        let Some(spawn) = layout.grid.get(slot).copied() else {
            break;
        };
        let loadout = bot_loadout(&roster, grid.seed, slot);
        let body = commands
            .spawn((
                Name::new(format!("Bot {}", slot + 1)),
                kart_body_with_stats(&kart_tuning, &stats_tuning, roster.stats(&loadout), spawn),
                Racer::new(slot),
                bot(tuning.grid_difficulty, bot_seed(grid.seed, slot), &tuning),
            ))
            .id();
        if let (Some(visuals), Some(materials)) = (&visuals, materials.as_mut()) {
            let driver = looks.as_ref().and_then(|l| l.get(loadout.character));
            spawn_kart_visual_with_driver(
                &mut commands,
                visuals,
                materials,
                body,
                bot_color(slot),
                driver,
            );
        }
    }
}

/// GD § 12: bots far behind the reference get a little more top speed, bots far ahead a little
/// less. The reference is the best placed local player; with no player (a race of bots only),
/// the field's average race distance. It is the catch-up factor of the kart's top speed; the
/// kart combines it with the others (croquettes...) into its `MaxSpeedScale`.
fn catch_up(
    tuning: Res<BotTuning>,
    racers: Query<(&RaceProgress, Has<PlayerKart>), With<Racer>>,
    mut bots: Query<(&RaceProgress, &mut MaxSpeedFactors), With<Bot>>,
) {
    let (mut player, mut sum, mut count) = (None::<f32>, 0.0, 0);
    for (progress, is_player) in &racers {
        sum += progress.distance;
        count += 1;
        if is_player {
            player = Some(player.map_or(progress.distance, |d| d.max(progress.distance)));
        }
    }
    if count == 0 {
        return;
    }
    let reference = player.unwrap_or(sum / count as f32);
    for (progress, mut factors) in &mut bots {
        let s = if progress.finished() {
            1.0
        } else {
            catch_up_scale(reference - progress.distance, &tuning)
        };
        if factors.catch_up != s {
            factors.catch_up = s;
        }
    }
}

/// The closest item box worth going for (issue #16): available, ahead within `range` and at
/// most `range` / 8 to the side of a kart at `position` heading `forward`. `(ahead, right)` (m).
fn item_box_ahead(
    boxes: &[Vec3],
    position: Vec3,
    forward: Vec3,
    right: Vec3,
    range: f32,
) -> Option<(f32, f32)> {
    boxes
        .iter()
        .map(|at| {
            let to = *at - position;
            (to.dot(forward), to.dot(right))
        })
        .filter(|(ahead, side)| *ahead > 6.0 && *ahead < range && side.abs() < range / 8.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// The closest kart ahead within `ahead_range` (distance, angle off the heading) and behind
/// within `behind_range` (distance, angle off straight back), for the items (issue #16).
fn karts_around(
    me: Entity,
    position: Vec3,
    forward: Vec3,
    neighbors: &[Neighbor],
    ahead_range: f32,
    behind_range: f32,
) -> (Option<(f32, f32)>, Option<(f32, f32)>) {
    let flat = forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let mut ahead: Option<(f32, f32)> = None;
    let mut behind: Option<(f32, f32)> = None;
    for other in neighbors {
        let to = other.position - position;
        if other.entity == me || to.y.abs() > 3.0 {
            continue;
        }
        let distance = to.with_y(0.0).length();
        if distance < 1e-3 {
            continue;
        }
        let angle = flat.angle_between(to.with_y(0.0));
        let side = flat.cross(to).y.signum();
        if angle < std::f32::consts::FRAC_PI_2 {
            if distance < ahead_range && ahead.is_none_or(|(d, _)| distance < d) {
                ahead = Some((distance, angle * -side));
            }
        } else if distance < behind_range && behind.is_none_or(|(d, _)| distance < d) {
            behind = Some((distance, (std::f32::consts::PI - angle) * side));
        }
    }
    (ahead, behind)
}

/// Every bot writes its kart's input for this tick: its driving, and its item button (issue
/// #16). The other karts and the boxes are gathered once in buffers kept between ticks (no
/// allocation per tick). Online (issue #9), the host's bots see the other players' karts too.
fn drive_bots(
    time: Res<Time>,
    line: Res<RacingLine>,
    tuning: Res<BotTuning>,
    kart_tuning: Res<KartTuning>,
    item_tuning: Option<Res<ItemTuning>>,
    interaction_tuning: Option<Res<InteractionTuning>>,
    croquette_tuning: Option<Res<CroquetteTuning>>,
    race: Option<Res<Race>>,
    mut neighbors: Local<Vec<Neighbor>>,
    mut boxes: Local<Vec<Vec3>>,
    mut in_reach: Local<Vec<ReachKart>>,
    karts: Query<(Entity, &Transform, &Velocity), Or<(With<Kart>, With<Remote>)>>,
    item_boxes: Query<(&Transform, &ItemBox)>,
    mut bots: Query<(
        Entity,
        &Bot,
        (&mut BotBrain, &mut BotItemBrain, Option<&mut BotPawBrain>),
        &Transform,
        &Velocity,
        &KartState,
        &KartAssists,
        &MaxSpeedScale,
        &StatEffects,
        &TrickTracker,
        &Boost,
        (
            Option<&ItemSlots>,
            Option<&RaceProgress>,
            Option<&Croquettes>,
            Option<&Paw>,
        ),
        &mut KartInput,
    )>,
) {
    let racing = race.as_ref().is_none_or(|race| race.started());
    // The countdown, for the rocket start: seconds since the "2", until GO.
    let since_two = race
        .as_ref()
        .filter(|race| !race.started())
        .and_then(|race| race.flow.ticks_since_two(&race.timings))
        .map(|ticks| ticks as f32 / TICK_HZ as f32);
    let trick_window = (kart_tuning.trick_window * TICK_HZ as f32).round() as u32;
    neighbors.clear();
    neighbors.extend(karts.iter().map(|(entity, transform, velocity)| Neighbor {
        entity,
        position: transform.translation,
        velocity: velocity.linear,
    }));
    // Online too (issue #53): the host's bots take boxes and use their items as solo.
    let items_on = item_tuning.as_ref().is_some_and(|t| t.enabled);
    boxes.clear();
    if items_on {
        boxes.extend(
            item_boxes
                .iter()
                .filter(|(_, item_box)| item_box.available())
                .map(|(transform, _)| transform.translation),
        );
    }
    let max_croquettes = croquette_tuning.map_or(10, |t| t.max_held);
    let interactions = interaction_tuning.as_deref().filter(|t| t.enabled);
    for (
        entity,
        bot,
        (mut brain, mut item_brain, paw_brain),
        transform,
        velocity,
        state,
        assists,
        scale,
        effects,
        tricks,
        boost,
        (slots, progress, croquettes, paw),
        mut input,
    ) in &mut bots
    {
        let has_room = slots.is_some_and(|s| s.has_room());
        let senses = Senses {
            entity,
            position: transform.translation,
            forward: *transform.forward(),
            right: *transform.right(),
            velocity: velocity.linear,
            yaw_rate: -velocity.angular.dot(*transform.up()),
            boost_left: boost.remaining,
            state: *state,
            assisted: assists.assisted_steering,
            speed_scale: scale.0,
            racing,
            since_two,
            trick_window: tricks.window_open(trick_window),
            dt: time.delta_secs(),
            neighbors: &neighbors,
            item_box: if has_room {
                item_box_ahead(
                    &boxes,
                    transform.translation,
                    *transform.forward(),
                    *transform.right(),
                    tuning.item_box_range,
                )
            } else {
                None
            },
        };
        // The bot knows its own kart: its stats applied (drift radii, tiers, steering).
        let skill = tuning.skill(bot.difficulty);
        let mut new = drive(
            &mut brain,
            skill,
            &tuning,
            &effects.tuning(&kart_tuning),
            &line,
            &senses,
        );
        // Its items (issue #16): its item button, after its driving.
        if let Some(slots) = slots.filter(|_| items_on) {
            let (ahead, behind) = karts_around(
                entity,
                transform.translation,
                *transform.forward(),
                &neighbors,
                tuning.item_ahead_range,
                tuning.item_behind_range,
            );
            let straight = brain
                .sample
                .and_then(|index| {
                    let look = (tuning.item_piment_straight / line.step()).ceil() as usize;
                    line.turn_ahead(index, look)
                })
                .map_or(f32::MAX, |(_, samples)| samples as f32 * line.step());
            let hand = match slots.hand {
                Slot::Ready { item, count } => Some((item, count)),
                _ => None,
            };
            let senses = ItemSenses {
                hand,
                held_out: slots.held_out(),
                dragging: slots.dragging.is_some(),
                ahead,
                behind,
                rank: progress.map_or(0, |p| p.rank),
                croquettes: croquettes.map_or(0, |c| c.count),
                max_croquettes,
                straight,
                boosting: boost.is_active(),
                can_act: racing
                    && !state.is_hit()
                    && *state != KartState::Respawning
                    && !progress.is_some_and(|p| p.finished()),
                dt: time.delta_secs(),
            };
            let buttons = item_buttons(&mut item_brain, skill, &tuning, &senses);
            new.item = buttons.item;
            new.look_back = buttons.back;
        }
        // Its paw (issue #34): the Interaction button, a jump to break free, a throw's stick.
        if let (Some(interactions), Some(paw), Some(mut paw_brain)) = (interactions, paw, paw_brain)
        {
            let forward = transform.forward().with_y(0.0).normalize_or(Vec3::NEG_Z);
            let speed = velocity.linear.dot(forward);
            // The side of the track's nearer edge, from its own place across the road (right
            // positive), seen along its heading (it drives along the track).
            let edge = if brain.lateral.abs() > 0.5 {
                brain.lateral.signum()
            } else {
                0.0
            };
            in_reach.clear();
            in_reach.extend(
                neighbors
                    .iter()
                    .filter(|o| o.entity != entity)
                    .filter_map(|other| {
                        let reach = crate::interactions::reach(
                            interactions,
                            transform.translation,
                            forward,
                            other.position,
                        )?;
                        let side = if reach.lateral >= 0.0 { 1.0 } else { -1.0 };
                        Some(ReachKart {
                            entity: other.entity,
                            side,
                            ahead: reach.ahead,
                            faster_by: other.velocity.dot(forward) - speed,
                            toward_edge: edge != 0.0 && side == edge,
                        })
                    }),
            );
            let seconds = |ticks: u32| ticks as f32 / TICK_HZ as f32;
            let senses = PawSenses {
                can_act: racing
                    && !state.is_hit()
                    && *state != KartState::Respawning
                    && !progress.is_some_and(|p| p.finished()),
                ready: paw.ready(),
                drifting: matches!(state, KartState::Hop { .. } | KartState::Drifting { .. }),
                grounded: *state == KartState::Grounded,
                reaching: matches!(paw.motion, PawMotion::Reaching { .. }),
                holding: match paw.motion {
                    PawMotion::Holding { ticks, side, .. } => Some((seconds(ticks), side)),
                    _ => None,
                },
                held_for: paw.held.map(|held| seconds(held.ticks)),
                tap_time: interactions.tap_time,
                in_reach: &in_reach,
                dt: time.delta_secs(),
            };
            let buttons = paw_buttons(&mut paw_brain, skill, &tuning, &senses);
            new.interact = buttons.interact;
            new.jump_drift |= buttons.jump;
            if let Some(steer) = buttons.steer {
                new.steer = steer;
            }
        }
        input.set_if_neq(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kart_has_its_own_color_and_seed() {
        for (i, color) in KART_COLORS.iter().enumerate() {
            assert_ne!(*color, crate::PLAYER_COLOR, "slot {i}");
            for other in &KART_COLORS[i + 1..] {
                assert_ne!(color, other);
            }
        }
        assert_eq!(bot_color(3), KART_COLORS[3]);
        let mut seeds: Vec<u64> = (0..12).map(|slot| bot_seed(0, slot)).collect();
        seeds.dedup();
        assert_eq!(seeds.len(), 12);
    }

    /// Issue #40: the race seed draws the bots. The game's seed gives every bot the seed it had
    /// before (the game is unchanged); another race seed gives others, never the same twice;
    /// the same race seed gives the same bots (parts, lane, pace).
    #[test]
    fn the_race_seed_draws_the_bots() {
        use crate::race::RaceGrid;
        for slot in 0..12 {
            assert_eq!(bot_seed(RaceGrid::GAME_SEED, slot), 0xF1_0FF1 + slot as u64);
        }
        let mut seeds: Vec<u64> = (0..2000)
            .flat_map(|race| (0..12).map(move |slot| bot_seed(race, slot)))
            .collect();
        let count = seeds.len();
        seeds.sort();
        seeds.dedup();
        assert_eq!(seeds.len(), count);

        let roster = Roster::embedded();
        let grid = |race: u64| -> Vec<Loadout> {
            (0..11)
                .map(|slot| bot_loadout(&roster, race, slot))
                .collect()
        };
        assert_eq!(grid(7), grid(7));
        assert_ne!(grid(0), grid(1));
        assert_ne!(grid(1), grid(2));
        // The animals follow the grid slot, whatever the seed (the nine animals line up).
        for race in 1..5 {
            for (a, b) in grid(0).iter().zip(&grid(race)) {
                assert_eq!(a.character, b.character);
            }
        }
        let t = BotTuning::default();
        let brain = |race, slot| BotBrain::new(bot_seed(race, slot), &t);
        assert_eq!(brain(3, 4).lane, brain(3, 4).lane);
        assert_ne!(brain(0, 4).lane, brain(1, 4).lane);
        assert_ne!(brain(0, 4).pace, brain(1, 4).pace);
    }

    /// Issue #15: the bots' characters and parts vary along the grid, always the same for a
    /// slot: the nine animals on the first nine slots, the three weight classes in any three
    /// neighbouring slots and in the slots taken one in three, several karts, wheels and wings.
    #[test]
    fn bots_get_varied_and_deterministic_loadouts() {
        let roster = Roster::embedded();
        let loadouts: Vec<Loadout> = (0..11).map(|slot| bot_loadout(&roster, 0, slot)).collect();
        let again: Vec<Loadout> = (0..11).map(|slot| bot_loadout(&roster, 0, slot)).collect();
        assert_eq!(loadouts, again);
        let mut animals: Vec<usize> = loadouts[..9].iter().map(|l| l.character).collect();
        animals.sort();
        assert_eq!(animals, (0..9).collect::<Vec<_>>());
        let class = |slot: usize| roster.character(&loadouts[slot]).class;
        for start in 0..9 {
            let mut three: Vec<WeightClass> = (start..start + 3).map(class).collect();
            three.dedup();
            assert_eq!(three.len(), 3, "slots {start}..{}", start + 3);
            assert_eq!(bot_class(start), class(start));
        }
        for first in 0..3 {
            let classes: Vec<WeightClass> = (first..9).step_by(3).map(class).collect();
            for c in WeightClass::ALL {
                assert!(classes.contains(&c), "slots {first}, {}, ...", first + 3);
            }
        }
        for kind in [PartKind::Kart, PartKind::Wheels, PartKind::Wing] {
            let mut used: Vec<usize> = loadouts.iter().map(|l| l.get(kind)).collect();
            used.sort();
            used.dedup();
            assert_eq!(used.len(), roster.count(kind), "{kind:?}: {used:?}");
        }
        // Different stats along the grid.
        let mut speeds: Vec<u8> = loadouts
            .iter()
            .map(|l| roster.stats(l).level(crate::kart::Stat::Speed))
            .collect();
        speeds.sort();
        speeds.dedup();
        assert!(speeds.len() >= 4, "{speeds:?}");
    }
}
