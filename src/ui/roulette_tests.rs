//! Headless scenarios of the roulette's reel (issue #102): the whole game (`GamePlugin`) on
//! Prairie Pompon, the player alone, its race started, one fixed tick per update; the reel added
//! by hand ([`RoulettePlugin`]: the game adds it only with a renderer).

use bevy::prelude::*;

use super::roulette::{DING_TIME, ReelPlan};
use super::*;
use crate::admin::{Tunings, Value, skip_countdown_now};
use crate::audio::{RouletteSounds, SoundEffect};
use crate::items::{ItemKind, ItemSlots, ItemStats, ItemTuning, Slot};
use crate::race::Race;
use crate::tests::{app_on, game_app_unloaded, hold_keys, player, wait_for_the_player};
use crate::track::TrackChoice;

/// Nodes of the reel laid out again since the count was reset: the `Node`s of the slots, of
/// their cells and of the cells' icons that changed.
#[derive(Resource, Default)]
struct Relayouts(usize);

fn count_relayouts(
    mut count: ResMut<Relayouts>,
    changed: Query<(Entity, Option<&ChildOf>), Changed<Node>>,
    reel: Query<(), Or<(With<ReelCell>, With<ItemSlot>)>>,
) {
    for (entity, parent) in &changed {
        if reel.contains(entity) || parent.is_some_and(|p| reel.contains(p.parent())) {
            count.0 += 1;
        }
    }
}

/// The game with the reel, at GO.
fn race_with_reel() -> (App, Entity) {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.add_plugins(RoulettePlugin)
        .init_resource::<Relayouts>()
        .add_systems(PostUpdate, count_relayouts);
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    for _ in 0..10 {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        app.update();
    }
    app.update();
    app.update();
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    assert!(app.world().resource::<Race>().started());
    (app, kart)
}

/// The cells seen in a slot's window: their item and how far down they are (% of the window),
/// the lowest first (the one going out).
fn window(app: &mut App, index: usize) -> Vec<(ItemKind, f32)> {
    let world = app.world_mut();
    let mut seen: Vec<(ItemKind, f32)> = world
        .query::<(&ReelCell, &UiTransform, &Visibility)>()
        .iter(world)
        .filter(|(cell, _, visibility)| cell.index == index && **visibility != Visibility::Hidden)
        .map(|(cell, transform, _)| {
            let down = match transform.translation.y {
                Val::Percent(down) => down,
                Val::Px(0.0) => 0.0,
                other => panic!("{other:?}"),
            };
            (cell.item, down)
        })
        .collect();
    seen.sort_by(|a, b| b.1.total_cmp(&a.1));
    seen
}

/// A slot's scale and its ring (the ding).
fn ding(app: &mut App, index: usize) -> (Vec2, Color) {
    let world = app.world_mut();
    world
        .query::<(&ItemSlot, &UiTransform, &Outline)>()
        .iter(world)
        .find(|(slot, ..)| slot.index == index)
        .map(|(_, transform, outline)| (transform.scale, outline.color))
        .unwrap()
}

fn sounds(app: &mut App, sound: SoundEffect) -> usize {
    let world = app.world_mut();
    world
        .query::<&SoundEffect>()
        .iter(world)
        .filter(|s| **s == sound)
        .count()
}

fn slots(app: &App, kart: Entity) -> ItemSlots {
    *app.world().get::<ItemSlots>(kart).unwrap()
}

/// Up arrow until the player breaks a box (the first row of Prairie Pompon).
fn drive_into_a_box(app: &mut App, kart: Entity) {
    hold_keys(app, &[KeyCode::ArrowUp]);
    let mut ticks = 0;
    while app.world().get::<ItemStats>(kart).unwrap().boxes == 0 {
        app.update();
        ticks += 1;
        assert!(ticks < 600, "no box on the way");
    }
}

/// What a roulette showed in a slot's window, frame by frame: the frames on which a new item
/// came down into it (the lowest one seen changed), and the item drawn it stopped on.
struct Turned {
    arrivals: Vec<usize>,
    stopped_on: ItemKind,
    frames: usize,
}

/// Frames until slot `index`'s roulette stops: at every frame, one or two cells in its window,
/// the next one exactly a window above the first, both sliding down; then the item drawn alone,
/// in the middle, ready.
fn watch_the_reel(app: &mut App, kart: Entity, index: usize) -> Turned {
    let slot_of = |slots: ItemSlots| [slots.hand, slots.reserve][index];
    let mut arrivals = Vec::new();
    let mut last: Option<(ItemKind, f32)> = None;
    for frame in 0..120 {
        let seen = window(app, index);
        match slot_of(slots(app, kart)) {
            Slot::Spinning { .. } => {
                assert!((1..=2).contains(&seen.len()), "frame {frame}: {seen:?}");
                let (going, down) = seen[0];
                assert!((0.0..=100.0).contains(&down), "frame {frame}: {seen:?}");
                if let [_, (coming, above)] = seen[..] {
                    assert_ne!(going, coming);
                    assert!(
                        (down - above - 100.0).abs() < 1e-3,
                        "frame {frame}: {seen:?}"
                    );
                }
                match last {
                    // The same item, further down (or as far, stopped by the frame).
                    Some((item, before)) if item == going => {
                        assert!(down >= before - 1e-3, "frame {frame}: {down} < {before}");
                    }
                    Some(_) => arrivals.push(frame),
                    None => {}
                }
                last = Some((going, down));
            }
            Slot::Ready { item, .. } => {
                assert_eq!(seen, vec![(item, 0.0)], "it stops on the item drawn");
                if last.is_some_and(|(before, _)| before != item) {
                    arrivals.push(frame);
                }
                return Turned {
                    arrivals,
                    stopped_on: item,
                    frames: frame,
                };
            }
            Slot::Empty => panic!("frame {frame}: no roulette"),
        }
        app.update();
    }
    panic!("the roulette never stopped");
}

/// Issue #102 acceptance: the player drives into a box. In the hand's slot, the items come down
/// through the window one after the other, fast (a dozen a second), then slower and slower, and
/// the reel stops on the item drawn on the tick it is ready (90 ticks). Then the ding: the slot
/// squashed, bouncing back, a ring bursting out, back at rest in under half a second. A tick for
/// each item, a ding for the stop. Nothing of the reel is laid out again meanwhile (only
/// `UiTransform`s and colors change).
#[test]
fn the_reel_scrolls_slows_down_and_stops_on_the_item_drawn() {
    let (mut app, kart) = race_with_reel();
    // Built: sixteen cells a slot, in place of the single icon.
    let world = app.world_mut();
    assert_eq!(world.query::<&ReelCell>().iter(world).count(), 32);
    assert_eq!(world.query::<&Reel>().iter(world).count(), 2);
    assert_eq!(world.query::<&ItemIcon>().iter(world).count(), 0);
    assert!(world.get_resource::<RouletteSounds>().is_some());
    assert!(window(&mut app, 0).is_empty() && window(&mut app, 1).is_empty());
    app.world_mut().resource_mut::<Relayouts>().0 = 0;

    drive_into_a_box(&mut app, kart);
    assert!(matches!(
        slots(&app, kart).hand,
        Slot::Spinning { ticks: 90, .. }
    ));
    // Straight from the pole, the kart breaks two boxes of the row at once: the reserve's reel
    // turns too, half a reel further on (not the same items side by side).
    let both = slots(&app, kart);
    let (hand, reserve) = (window(&mut app, 0), window(&mut app, 1));
    println!("{both:?}: {hand:?} and {reserve:?}");
    match both.reserve {
        Slot::Spinning { ticks: 90, .. } => assert_ne!(hand[0].0, reserve[0].0),
        Slot::Empty => assert!(reserve.is_empty()),
        other => panic!("{other:?}"),
    }
    let turned = watch_the_reel(&mut app, kart, 0);
    println!(
        "{} frames, the items came in on frames {:?}, stopped on {:?}",
        turned.frames, turned.arrivals, turned.stopped_on
    );
    assert_eq!(
        turned.frames, 90,
        "ready on the 90th tick, the reel stopped with it"
    );
    // Every item of the plan came in, the last one being the item drawn.
    let plan = ReelPlan::of(&ItemTuning::default());
    assert_eq!(
        turned.arrivals.len() as u32,
        plan.cells,
        "{:?}",
        turned.arrivals
    );
    // Fast first (about 5 frames an item at 12 a second), slower and slower at the end: the
    // last item takes more than twice as long as the first ones.
    let gaps: Vec<usize> = turned.arrivals.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps[..4].iter().all(|gap| *gap <= 6), "{gaps:?}");
    let last = *gaps.last().unwrap();
    assert!(last >= 2 * gaps[0] + 2, "it slows down: {gaps:?}");
    assert!(
        gaps.windows(2)
            .skip(gaps.len() / 2)
            .all(|w| w[1] + 1 >= w[0]),
        "slower and slower: {gaps:?}"
    );
    assert_eq!(app.world().resource::<Relayouts>().0, 0, "no layout");

    // A tick for each item that came in before the last, the ding for the stop.
    assert_eq!(
        sounds(&mut app, SoundEffect::RouletteTick),
        plan.cells as usize - 1
    );
    assert_eq!(sounds(&mut app, SoundEffect::RouletteStop), 1);
    // The ding: squashed and wider at the stop, the ring out; settled before half a second.
    let (scale, ring) = ding(&mut app, 0);
    assert!(scale.y < 0.85 && scale.x > 1.05, "{scale}");
    assert!(ring.alpha() > 0.8, "{ring:?}");
    let mut tallest: f32 = 0.0;
    for _ in 0..(DING_TIME * 60.0).ceil() as usize + 1 {
        app.update();
        tallest = tallest.max(ding(&mut app, 0).0.y);
    }
    assert!(tallest > 1.05, "it bounces back past its size: {tallest}");
    assert_eq!(ding(&mut app, 0), (Vec2::ONE, Color::NONE));
    assert_eq!(ding(&mut app, 1), (Vec2::ONE, Color::NONE));
    assert_eq!(window(&mut app, 0), vec![(turned.stopped_on, 0.0)]);
    assert_eq!(sounds(&mut app, SoundEffect::RouletteStop), 1);
}

/// Issue #102: with an item in the hand, the box's roulette turns in the reserve's slot; the hand
/// keeps showing its item (and dings for nothing). The reserve stops on its item drawn, and
/// dings.
#[test]
fn the_reserve_turns_its_own_reel() {
    let (mut app, kart) = race_with_reel();
    app.world_mut()
        .get_mut::<ItemSlots>(kart)
        .unwrap()
        .give(ItemKind::Pelote, 1, 0);
    drive_into_a_box(&mut app, kart);
    assert!(matches!(
        slots(&app, kart).reserve,
        Slot::Spinning { ticks: 90, .. }
    ));
    let mut hand_seen = Vec::new();
    let mut frames = 0;
    while matches!(slots(&app, kart).reserve, Slot::Spinning { .. }) {
        hand_seen.push(window(&mut app, 0));
        app.update();
        frames += 1;
        assert!(frames <= 90);
    }
    assert!(
        hand_seen
            .iter()
            .all(|seen| *seen == vec![(ItemKind::Pelote, 0.0)]),
        "the hand shows its pelote"
    );
    let Slot::Ready { item, .. } = slots(&app, kart).reserve else {
        panic!("the reserve's item");
    };
    assert_eq!(window(&mut app, 1), vec![(item, 0.0)]);
    let (scale, _) = ding(&mut app, 1);
    assert!(scale.y < 0.85, "the reserve dings: {scale}");
    assert_eq!(ding(&mut app, 0).0, Vec2::ONE, "not the hand");
    assert_eq!(sounds(&mut app, SoundEffect::RouletteStop), 1);
}

/// The headless game (the bot races' world) has no reel: the HUD's single icon per slot, no
/// cell, no sound of the roulette (its icon shows the item in the middle of the reel:
/// `items::scenario_tests`, `a_keyboard_player_takes_a_box_and_uses_its_item`).
#[test]
fn the_headless_game_keeps_its_single_icon() {
    let mut app = app_on(TrackChoice::default());
    for _ in 0..20 {
        let world = app.world_mut();
        if world.query::<&HudRoot>().iter(world).next().is_some() {
            break;
        }
        app.update();
    }
    app.update();
    let world = app.world_mut();
    assert_eq!(world.query::<&ItemIcon>().iter(world).count(), 2);
    assert_eq!(world.query::<&ReelCell>().iter(world).count(), 0);
    assert_eq!(world.query::<&Reel>().iter(world).count(), 0);
    assert!(world.get_resource::<RouletteSounds>().is_none());
}

/// The reel's tuning is in the admin panel, with the roulette's length (`ItemTuning`): slower, or
/// slowing down longer, fewer items pass; the rules' roulette keeps its length.
#[test]
fn the_reel_follows_its_tuning_in_the_admin_panel() {
    let mut app = app_on(TrackChoice::default());
    let cells = |app: &App| ReelPlan::of(app.world().resource::<ItemTuning>()).cells;
    let before = cells(&app);
    app.world_mut()
        .resource_scope(|world, tunings: Mut<Tunings>| {
            tunings
                .set(world, "ItemTuning.roulette_scroll_speed", Value::F32(6.0))
                .unwrap();
        });
    let slower = cells(&app);
    assert!(slower < before, "{slower} < {before}");
    app.world_mut()
        .resource_scope(|world, tunings: Mut<Tunings>| {
            tunings
                .set(world, "ItemTuning.roulette_slowdown", Value::F32(1.5))
                .unwrap();
        });
    assert!(cells(&app) < slower);
    assert_eq!(app.world().resource::<ItemTuning>().roulette_time, 1.5);
}
