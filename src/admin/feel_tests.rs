//! The « Ressenti » knobs over the game's real tunings (issue #155): every tuning resource of the
//! game in a `World`, as the panel sees them (the headless game has fewer: the sound, the music,
//! the particles, the lobby and the home are only there with a renderer).

use bevy::prelude::*;

use super::feel::{KNOB_MAX, Knob, LINKS, MIDDLE};
use super::tuning::{Tunings, Value, fields, parse};
use crate::audio::{MusicTuning, SoundTuning};
use crate::bots::BotTuning;
use crate::camera::CameraTuning;
use crate::characters::ragdoll::RagdollTuning;
use crate::characters::{DriverTuning, WobbleTuning};
use crate::croquettes::CroquetteTuning;
use crate::interactions::InteractionTuning;
use crate::items::ItemTuning;
use crate::kart::{KartTuning, StatsTuning};
use crate::menu::HomeTuning;
use crate::net::NetTuning;
use crate::race::{GrandPrixTuning, RaceTuning};
use crate::salon::SalonTuning;
use crate::vfx::VfxTuning;

/// Every tuning resource of the game, registered as the game does (its order, the renderer's
/// after).
pub(super) fn all_tunings() -> (World, Tunings) {
    let mut world = World::new();
    let mut tunings = Tunings::default();
    macro_rules! register {
        ($($ty:ty),*) => {
            $(
                world.init_resource::<$ty>();
                tunings.register::<$ty>();
            )*
        };
    }
    register!(
        KartTuning,
        CroquetteTuning,
        BotTuning,
        CameraTuning,
        RaceTuning,
        StatsTuning,
        ItemTuning,
        InteractionTuning,
        NetTuning,
        DriverTuning,
        WobbleTuning,
        GrandPrixTuning,
        VfxTuning,
        SoundTuning,
        MusicTuning,
        SalonTuning,
        RagdollTuning,
        HomeTuning
    );
    (world, tunings)
}

/// Every value of every tuning, by its `Resource.path`.
fn values(world: &World, tunings: &Tunings) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for tuning in &tunings.list {
        let resource = tuning.read(world).expect("registered");
        for field in fields(resource.as_partial_reflect()) {
            out.push((format!("{}.{}", tuning.name, field.path), field.value));
        }
    }
    out
}

/// Every default, by its `Resource.path`.
fn defaults(tunings: &Tunings) -> Vec<(String, Value)> {
    tunings
        .list
        .iter()
        .flat_map(|t| {
            t.defaults
                .iter()
                .map(move |f| (format!("{}.{}", t.name, f.path), f.value))
        })
        .collect()
}

/// The values a knob drives, with their default and the link.
fn driven_by(tunings: &Tunings, knob: Knob) -> Vec<(String, Value, &'static super::feel::Link)> {
    defaults(tunings)
        .into_iter()
        .filter_map(|(path, default)| {
            let link = LINKS.iter().find(|l| l.knob == knob && l.matches(&path))?;
            Some((path, default, link))
        })
        .collect()
}

fn value_of(world: &World, tunings: &Tunings, full: &str) -> Value {
    values(world, tunings)
        .into_iter()
        .find(|(path, _)| path == full)
        .unwrap_or_else(|| panic!("no {full}"))
        .1
}

fn number(value: Value) -> f64 {
    value.as_f64().expect("a number")
}

/// Acceptance criterion of #155: every knob in the middle gives the game's values back, field by
/// field and exactly (an equality, not a tolerance), whatever the knobs did before.
#[test]
fn in_the_middle_every_value_is_its_default_field_by_field() {
    let (mut world, mut tunings) = all_tunings();
    let expected = defaults(&tunings);
    assert_eq!(values(&world, &tunings), expected);
    assert!(tunings.untouched(&world));
    for tuning in &tunings.list {
        for field in &tuning.defaults {
            assert_eq!(
                tuning.baseline(&tunings.feel, &field.path),
                Some(field.value),
                "{}.{}",
                tuning.name,
                field.path
            );
        }
    }
    // Every knob to its ends and back.
    for knob in Knob::ALL {
        for position in [0, 13, KNOB_MAX, 77, MIDDLE] {
            tunings.set_knob(&mut world, knob, position);
        }
    }
    for (path, value) in values(&world, &tunings) {
        let default = expected.iter().find(|(p, _)| *p == path).unwrap().1;
        assert_eq!(value, default, "{path}");
    }
    // The resources themselves, as the game reads them.
    assert_eq!(*world.resource::<KartTuning>(), KartTuning::default());
    assert_eq!(*world.resource::<BotTuning>(), BotTuning::default());
    assert_eq!(*world.resource::<ItemTuning>(), ItemTuning::default());
    assert_eq!(*world.resource::<CameraTuning>(), CameraTuning::default());
    assert_eq!(
        *world.resource::<InteractionTuning>(),
        InteractionTuning::default()
    );
    assert_eq!(*world.resource::<VfxTuning>(), VfxTuning::default());
    assert_eq!(*world.resource::<SoundTuning>(), SoundTuning::default());
    assert_eq!(*world.resource::<MusicTuning>(), MusicTuning::default());
    assert!(tunings.untouched(&world));
    assert_eq!(parse(&tunings.export(&world)).unwrap().len(), 0);
}

/// No dead link (a renamed field would drive nothing without a word), no value driven by two
/// knobs, and only numbers are driven.
#[test]
fn every_link_drives_values_of_the_game_and_each_value_one_knob_at_most() {
    let (_, tunings) = all_tunings();
    let all = defaults(&tunings);
    for link in LINKS {
        let driven: Vec<_> = all.iter().filter(|(p, _)| link.matches(p)).collect();
        assert!(!driven.is_empty(), "{} drives nothing", link.pattern);
        for (path, value) in driven {
            assert!(value.as_f64().is_some(), "{path} is not a number");
            assert!(
                (link.min..=link.max).contains(&number(*value)),
                "{path}: its default is out of its bounds"
            );
        }
    }
    for (path, _) in &all {
        let knobs = LINKS.iter().filter(|l| l.matches(path)).count();
        assert!(knobs <= 1, "{path} is driven by {knobs} links");
    }
    // The registry knows them too.
    let kart = tunings.get("KartTuning").unwrap();
    assert_eq!(kart.knob_of("drift_radius_tight"), Some(Knob::Drift));
    assert_eq!(kart.knob_of("mini_turbo_durations[1]"), Some(Knob::Boosts));
    assert_eq!(kart.knob_of("mass"), None);
    let bots = tunings.get("BotTuning").unwrap();
    assert_eq!(bots.knob_of("skills[2].strike_chance"), Some(Knob::Bots));
}

/// Acceptance criterion of #155: each knob, at both ends, changes the values it announces the way
/// it announces (more above 1, less below), one way only along the slider, and nothing else.
#[test]
fn each_knob_at_its_ends_moves_its_values_the_announced_way() {
    for knob in Knob::ALL {
        let (mut world, mut tunings) = all_tunings();
        let driven = driven_by(&tunings, knob);
        assert!(!driven.is_empty(), "{knob:?}");
        for (end, position) in [(false, 0), (true, KNOB_MAX)] {
            tunings.set_knob(&mut world, knob, position);
            let mut moved = 0;
            for (path, default) in defaults(&tunings) {
                let value = value_of(&world, &tunings, &path);
                let Some((_, _, link)) = driven.iter().find(|(p, _, _)| *p == path) else {
                    assert_eq!(value, default, "{knob:?} moved {path}");
                    continue;
                };
                let factor = if end { link.high } else { link.low };
                let (v, d) = (number(value), number(default));
                let at_bound = d == link.max || d == link.min;
                if factor > 1.0 {
                    assert!(
                        v > d || (at_bound && v == d),
                        "{knob:?} {position}: {path} {v} / {d}"
                    );
                } else {
                    assert!(
                        v < d || (at_bound && v == d),
                        "{knob:?} {position}: {path} {v} / {d}"
                    );
                }
                moved += usize::from(v != d);
            }
            assert!(moved >= driven.len() / 2, "{knob:?}: {moved} moved");
            tunings.set_knob(&mut world, knob, MIDDLE);
        }
        // One way only, step by step.
        for (path, default, link) in &driven {
            let along: Vec<f64> = (0..=KNOB_MAX)
                .map(|p| number(link.value(*default, p)))
                .collect();
            let up = along.windows(2).all(|w| w[1] >= w[0]);
            let down = along.windows(2).all(|w| w[1] <= w[0]);
            assert!(up || down, "{path}: {along:?}");
            assert_eq!(up, link.high > 1.0, "{path}");
        }
    }
}

/// The ends stay playable: every knob at 0 together, then at 100 together, gives values the game
/// can drive with (the driving itself: `feel_scenarios`).
#[test]
fn every_knob_at_an_end_stays_within_playable_bounds() {
    for position in [0, KNOB_MAX] {
        let (mut world, mut tunings) = all_tunings();
        for knob in Knob::ALL {
            tunings.set_knob(&mut world, knob, position);
        }
        let k = world.resource::<KartTuning>().clone();
        let at = format!("all at {position}");
        assert!(
            (20.0..=36.0).contains(&k.max_speed),
            "{at}: {}",
            k.max_speed
        );
        assert!((1.5..=3.5).contains(&k.time_to_max_speed), "{at}");
        assert!(
            k.drift_radius_tight >= 12.0 && k.drift_radius_wide <= 80.0,
            "{at}"
        );
        assert!(k.drift_radius_tight < k.drift_radius_wide, "{at}");
        assert!(
            k.drift_slip_angle_tight <= 0.5 && k.drift_slip_angle_wide < k.drift_slip_angle_tight
        );
        assert!((0.3..=1.0).contains(&k.drift_slip_damping), "{at}");
        assert!(
            k.mini_turbo_thresholds.windows(2).all(|w| w[0] < w[1]),
            "{at}"
        );
        assert!(
            k.mini_turbo_durations.windows(2).all(|w| w[0] < w[1]),
            "{at}"
        );
        assert!(
            k.rocket_start_min_duration < k.rocket_start_max_duration,
            "{at}"
        );
        assert!(k.lateral_grip >= 12.0, "{at}");
        // Issue #193: 35 % in the middle now (Max), 28 % at the slippery end (35 % before).
        assert!((0.28..=1.0).contains(&k.offroad_speed_factor), "{at}");
        assert!((0.0..=1.0).contains(&k.bump_spin_keep), "{at}");
        // Untouched by the knobs: the drift keeps its speeds and the kart its body.
        assert!(k.drift_exit_speed < k.drift_min_speed);
        assert_eq!(k.mass, KartTuning::default().mass);
        // The strongest boost never catches up with a thrown pelote.
        let items = world.resource::<ItemTuning>().clone();
        let strongest = k
            .mini_turbo_strengths
            .into_iter()
            .chain([
                k.rocket_start_strength,
                k.boost_pad_strength,
                items.piment_strength,
            ])
            .fold(0.0, f32::max);
        assert!(
            items.pelote_speed > k.max_speed * (1.0 + strongest),
            "{at}: {} / {}",
            items.pelote_speed,
            k.max_speed * (1.0 + strongest)
        );
        assert!(items.roulette_slowdown < items.roulette_time, "{at}");
        assert!(
            items.box_respawn_time >= 0.5 && items.gap_scale > 0.0,
            "{at}"
        );
        let bots = world.resource::<BotTuning>().clone();
        for skill in &bots.skills {
            for chance in [
                skill.cruise_speed,
                skill.corner_margin,
                skill.drift_chance,
                skill.strike_chance,
                skill.grab_chance,
            ] {
                assert!((0.0..=1.0).contains(&chance), "{at}: {skill:?}");
            }
            assert!(
                skill.cruise_speed >= 0.85 && skill.corner_margin >= 0.8,
                "{at}"
            );
            assert!(skill.item_delay > 0.0, "{at}");
        }
        assert!(bots.catch_up_max <= 0.06, "{at}");
        let camera = world.resource::<CameraTuning>();
        assert!(
            camera.distance >= 4.0 && camera.height > camera.clearance,
            "{at}"
        );
        let sounds = world.resource::<SoundTuning>();
        for volume in [
            sounds.engine_volume,
            sounds.hit_volume,
            sounds.screen_volume,
        ] {
            assert!((0.0..=2.0).contains(&volume), "{at}");
        }
        let vfx = world.resource::<VfxTuning>();
        assert!(
            vfx.max_particles >= 100 && vfx.max_particles <= 1500,
            "{at}"
        );
    }
}

/// Acceptance criterion of #155: a value modified by hand (in « Avancé ») is no longer moved by
/// its knob, until it is back to its default (« défaut »): then the knob drives it again. The
/// knob's other values keep following it.
#[test]
fn a_value_modified_by_hand_is_no_longer_moved_by_its_knob() {
    let (mut world, mut tunings) = all_tunings();
    tunings.set_knob(&mut world, Knob::Drift, 80);
    let tight_at_80 = world.resource::<KartTuning>().drift_radius_tight;
    let wide_at_80 = world.resource::<KartTuning>().drift_radius_wide;
    assert!(tight_at_80 > KartTuning::default().drift_radius_tight);
    // By hand, as the panel's slider writes it.
    tunings
        .set(
            &mut world,
            "KartTuning.drift_radius_tight",
            Value::F32(15.0),
        )
        .unwrap();
    let changes = tunings.changes(&world);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].path, "drift_radius_tight");
    assert_eq!(
        changes[0].driven,
        Some((Knob::Drift, Value::F32(tight_at_80)))
    );
    // Moved by the knob, the other values follow; not this one.
    for position in [0, 30, KNOB_MAX] {
        tunings.set_knob(&mut world, Knob::Drift, position);
        let kart = world.resource::<KartTuning>();
        assert_eq!(kart.drift_radius_tight, 15.0, "at {position}");
        let link = LINKS
            .iter()
            .find(|l| l.matches("KartTuning.drift_radius_wide"))
            .unwrap();
        assert_eq!(
            Value::F32(kart.drift_radius_wide),
            link.value(
                Value::F32(KartTuning::default().drift_radius_wide),
                position
            ),
            "at {position}"
        );
    }
    assert_ne!(world.resource::<KartTuning>().drift_radius_wide, wide_at_80);
    // « défaut »: back to what the knob gives (at 100 now), and the knob drives it again.
    tunings
        .reset_field(&mut world, "KartTuning", "drift_radius_tight")
        .unwrap();
    assert!(tunings.changes(&world).is_empty());
    assert!(world.resource::<KartTuning>().drift_radius_tight > tight_at_80);
    tunings.set_knob(&mut world, Knob::Drift, MIDDLE);
    assert_eq!(*world.resource::<KartTuning>(), KartTuning::default());

    // « défaut (tout) » of a resource: its defaults, and what the knobs give the values they
    // drive; the values by hand of the other resources stay.
    tunings.set_knob(&mut world, Knob::Boosts, 0);
    tunings
        .set(&mut world, "KartTuning.boost_pad_duration", Value::F32(3.0))
        .unwrap();
    tunings
        .set(&mut world, "KartTuning.coast_decel", Value::F32(1.0))
        .unwrap();
    tunings
        .set(&mut world, "ItemTuning.piment_duration", Value::F32(9.0))
        .unwrap();
    tunings.reset_tuning(&mut world, "KartTuning");
    let kart = world.resource::<KartTuning>();
    assert_eq!(kart.coast_decel, KartTuning::default().coast_decel);
    assert!(kart.boost_pad_duration < KartTuning::default().boost_pad_duration);
    assert_eq!(world.resource::<ItemTuning>().piment_duration, 9.0);
    let changes = tunings.changes(&world);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].tuning, "ItemTuning");
}

/// Acceptance criterion of #155: the knobs are read back after a reload (the browser keeps the
/// text of « Copier mes réglages », `storage.rs`, and reads it at startup), with the values
/// modified by hand; the text is short: the knobs off the middle, then those values.
#[test]
fn the_knobs_and_the_values_by_hand_are_read_back_after_a_reload() {
    let (mut world, mut tunings) = all_tunings();
    tunings.set_knob(&mut world, Knob::Drift, 70);
    tunings.set_knob(&mut world, Knob::Effects, 0);
    tunings.set_knob(&mut world, Knob::Bots, 100);
    tunings
        .set(&mut world, "KartTuning.drift_radius_wide", Value::F32(60.0))
        .unwrap();
    tunings
        .set(&mut world, "KartTuning.coast_decel", Value::F32(4.5))
        .unwrap();
    let text = tunings.export(&world);
    let entries = parse(&text).unwrap();
    assert_eq!(entries.len(), 5, "{text}");
    assert!(text.contains("\"Ressenti.derapage\": 70, // de 0 à 100, milieu : 50"));
    assert!(text.contains("\"Ressenti.effets\": 0,"));
    assert!(text.contains("\"Ressenti.bots\": 100,"));
    assert!(text.contains("\"KartTuning.coast_decel\": 4.5, // défaut : 6.0"));
    assert!(
        text.contains("\"KartTuning.drift_radius_wide\": 60.0, // curseur Dérapage : "),
        "{text}"
    );
    // The driven values are not in it: the knobs give them.
    assert!(!text.contains("drift_radius_tight"), "{text}");

    // A reload: a new game, the text read at startup.
    let (mut reloaded, mut again) = all_tunings();
    let report = again.import(&mut reloaded, &text).unwrap();
    assert_eq!(report.applied, 5);
    assert!(report.ignored.is_empty(), "{:?}", report.ignored);
    assert_eq!(again.feel, tunings.feel);
    assert_eq!(values(&reloaded, &again), values(&world, &tunings));
    assert_eq!(again.export(&reloaded), text);
    // The value by hand is still by hand: the knob leaves it alone.
    again.set_knob(&mut reloaded, Knob::Drift, 10);
    assert_eq!(reloaded.resource::<KartTuning>().drift_radius_wide, 60.0);

    // A text from before #155 (no knob) loads as it did: knobs in the middle.
    let (mut old, mut old_tunings) = all_tunings();
    let report = old_tunings
        .import(&mut old, "{ \"KartTuning.max_speed\": 30.0 }")
        .unwrap();
    assert_eq!(report.applied, 1);
    assert!(old_tunings.feel.is_middle());
    assert_eq!(old.resource::<KartTuning>().max_speed, 30.0);

    // Knobs that do not fit are left out, with the reason.
    let report = old_tunings
        .import(
            &mut old,
            r#"{
                "Ressenti.turbo": 3,
                "Ressenti.vitesse": 150,
                "Ressenti.chocs": "fort",
                "Ressenti.camera": 20,
            }"#,
        )
        .unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(report.ignored.len(), 3, "{:?}", report.ignored);
    assert!(
        report
            .ignored
            .iter()
            .any(|i| i == "Ressenti.turbo : réglage inconnu"),
        "{:?}",
        report.ignored
    );
    assert_eq!(old_tunings.feel.position(Knob::Camera), 20);
    assert_eq!(old_tunings.feel.position(Knob::Speed), MIDDLE);
    // « Tout par défaut »: every knob in the middle, every value its default.
    old_tunings.reset_all(&mut old);
    assert!(old_tunings.untouched(&old));
    assert_eq!(*old.resource::<CameraTuning>(), CameraTuning::default());
}

/// The drift of before #170: « Dérapage »'s links over the defaults of #30, as #155 made them
/// (pattern, default, factor at 0, factor at 100, upper bound).
const DRIFT_BEFORE_170: [(&str, &[f32], f64, f64, f64); 10] = [
    (
        "KartTuning.drift_radius_tight",
        &[17.5],
        0.75,
        1.35,
        f64::INFINITY,
    ),
    (
        "KartTuning.drift_radius_wide",
        &[55.0],
        0.75,
        1.35,
        f64::INFINITY,
    ),
    (
        "KartTuning.drift_slip_angle_tight",
        &[0.3],
        0.7,
        1.5,
        f64::INFINITY,
    ),
    (
        "KartTuning.drift_slip_angle_wide",
        &[0.12],
        0.7,
        1.5,
        f64::INFINITY,
    ),
    (
        "KartTuning.drift_slip_frequency",
        &[9.0],
        1.3,
        0.7,
        f64::INFINITY,
    ),
    ("KartTuning.drift_slip_damping", &[0.7], 1.3, 0.6, 1.0),
    (
        "KartTuning.drift_curve_in_time",
        &[0.35],
        0.6,
        1.5,
        f64::INFINITY,
    ),
    (
        "KartTuning.drift_curve_out_time",
        &[0.3],
        0.6,
        1.5,
        f64::INFINITY,
    ),
    (
        "KartTuning.hop_steer_response",
        &[4.0],
        1.3,
        0.75,
        f64::INFINITY,
    ),
    (
        "KartTuning.mini_turbo_thresholds",
        &[270.0, 570.0, 870.0],
        0.8,
        1.2,
        f64::INFINITY,
    ),
];

/// Acceptance criterion of #170: the defaults are the values « Dérapage » gave at 80 before
/// (Max's drift of 2026-10-21), field by field: the same `f32` as the slider of #155 computed,
/// to the rounding of the literal (one `f32` step at most: 0.39 is written for 0.3 × 1.3).
/// They are the middle of the slider now (the test of the middle above holds with them), and
/// « Dérapage » still drives exactly these values.
#[test]
fn the_defaults_are_the_drift_of_max_the_old_slider_at_80() {
    let (world, tunings) = all_tunings();
    let driven = driven_by(&tunings, Knob::Drift);
    let patterns: Vec<&str> = LINKS
        .iter()
        .filter(|l| l.knob == Knob::Drift)
        .map(|l| l.pattern)
        .collect();
    assert_eq!(
        patterns,
        DRIFT_BEFORE_170.map(|(pattern, ..)| pattern),
        "« Dérapage » drives the same values as before"
    );
    let mut checked = 0;
    for (pattern, old_defaults, low, high, max) in DRIFT_BEFORE_170 {
        let old = super::feel::Link {
            knob: Knob::Drift,
            pattern,
            low,
            high,
            min: if max.is_finite() {
                0.0
            } else {
                f64::NEG_INFINITY
            },
            max,
        };
        let mine: Vec<_> = driven.iter().filter(|(p, ..)| old.matches(p)).collect();
        assert_eq!(mine.len(), old_defaults.len(), "{pattern}");
        for ((path, default, _), old_default) in mine.into_iter().zip(old_defaults) {
            let Value::F32(at_80) = old.value(Value::F32(*old_default), 80) else {
                panic!("{path}: not a float");
            };
            let Value::F32(new) = *default else {
                panic!("{path}: not a float");
            };
            assert!(
                (new - at_80).abs() <= at_80.abs() * f32::EPSILON,
                "{path}: {new} is not the old 80, {at_80}"
            );
            assert_ne!(new, *old_default, "{path} did not move");
            // What the game reads.
            assert_eq!(value_of(&world, &tunings, path), *default, "{path}");
            checked += 1;
        }
    }
    assert_eq!(checked, 12);
    assert_eq!(
        KartTuning::default().mini_turbo_thresholds,
        [302.4, 638.4, 974.4]
    );
}

/// #170: the recentred slider still goes both ways, as far on each side (give or take): more
/// than half and at most twice its amplitude on the other side, for each value it drives.
#[test]
fn the_recentred_drift_knob_goes_about_as_far_each_way() {
    for link in LINKS.iter().filter(|l| l.knob == Knob::Drift) {
        let (below, above) = ((1.0 - link.low).abs(), (link.high - 1.0).abs());
        let ratio = above / below;
        assert!(
            (0.5..=2.0).contains(&ratio),
            "{}: {} / {}",
            link.pattern,
            link.low,
            link.high
        );
    }
}

/// #170: a text kept from before (the header of #155) does not add its « Dérapage » to the new
/// defaults: its 80 is the new middle, so the knob goes back to the middle (any other position of
/// it too), the values by hand stay, the other knobs read as before, and the report says so.
/// A text of now reads its « Dérapage » as it is.
#[test]
fn a_drift_kept_from_before_170_goes_back_to_the_middle() {
    let old = format!(
        "{}\n{{\n    \"Ressenti.derapage\": 80, // de 0 à 100, milieu : 50\n    \
         \"Ressenti.vitesse\": 60, // de 0 à 100, milieu : 50\n    \
         \"KartTuning.drift_radius_wide\": 60.0, // curseur Dérapage : 66.55, défaut : 55.0\n}}\n",
        super::tuning::EXPORT_HEADER_BEFORE_170
    );
    let (mut world, mut tunings) = all_tunings();
    let report = tunings.import(&mut world, &old).unwrap();
    assert_eq!(report.applied, 2, "{report:?}");
    assert!(report.ignored.is_empty(), "{report:?}");
    assert_eq!(
        report.converted,
        [
            "Ressenti.derapage : 80 (avant #170) remis au milieu : le milieu est maintenant \
          l'ancien 80"
        ]
    );
    assert_eq!(tunings.feel.position(Knob::Drift), MIDDLE);
    assert_eq!(tunings.feel.position(Knob::Speed), 60);
    let kart = world.resource::<KartTuning>().clone();
    let defaults = KartTuning::default();
    assert_eq!(kart.drift_radius_wide, 60.0);
    assert_eq!(kart.drift_radius_tight, defaults.drift_radius_tight);
    assert_eq!(kart.drift_slip_angle_tight, defaults.drift_slip_angle_tight);
    assert_eq!(kart.mini_turbo_thresholds, defaults.mini_turbo_thresholds);
    // Written again (as the startup does), it reads the same and is converted no more.
    let text = tunings.export(&world);
    assert!(text.starts_with(super::tuning::EXPORT_HEADER), "{text}");
    assert!(!text.contains("derapage"), "{text}");
    let (mut again, mut again_tunings) = all_tunings();
    let report = again_tunings.import(&mut again, &text).unwrap();
    assert!(report.converted.is_empty() && report.ignored.is_empty());
    assert_eq!(values(&again, &again_tunings), values(&world, &tunings));

    // Another position of before: the middle too.
    let (mut world, mut tunings) = all_tunings();
    let report = tunings
        .import(&mut world, &old.replace("derapage\": 80", "derapage\": 30"))
        .unwrap();
    assert_eq!(report.converted.len(), 1, "{report:?}");
    assert!(tunings.feel.position(Knob::Drift) == MIDDLE);
    assert_eq!(
        world.resource::<KartTuning>().drift_radius_tight,
        defaults.drift_radius_tight
    );

    // A text of now: its 80 is an 80 of the recentred slider.
    let now = old.replace(
        super::tuning::EXPORT_HEADER_BEFORE_170,
        super::tuning::EXPORT_HEADER,
    );
    let (mut world, mut tunings) = all_tunings();
    let report = tunings.import(&mut world, &now).unwrap();
    assert_eq!(report.applied, 3);
    assert!(report.converted.is_empty());
    assert_eq!(tunings.feel.position(Knob::Drift), 80);
    assert!(world.resource::<KartTuning>().drift_radius_tight > defaults.drift_radius_tight);
}
