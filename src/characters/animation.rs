//! What a driver plays (issue #69): pure, from what its kart is going through, so that it is
//! tested without a renderer. `visual.rs` plays it on the animal (Bevy's `AnimationPlayer`).
//!
//! - **Idle** (`idle`): on the grid and in the race, a soft sway.
//! - **Shake** (`gesture-negative`, a shake of the head): hit by an item (`KartState::Hit`, as
//!   long as it spins or tumbles) or struck by a paw (`Paw::struck_ticks`, for
//!   [`DriverTuning::struck_seconds`]); then back to idle.
//! - **Dance** (`dance`): over the line among the first [`DriverTuning::cheer_places`], until
//!   the next race (the results screen included).
//! - **Nod** (`gesture-positive`): over the line further back, for
//!   [`DriverTuning::finish_nod_seconds`]; then idle.
//!
//! Online, the other players' karts play the same from what their states say (their
//! `KartState`, their paw's strikes and their finish, which already come over the network): no
//! message of its own.

use bevy::prelude::*;

/// An animation of a driver, and its clip in the Cube Pets' files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AnimalAnimation {
    #[default]
    Idle,
    Dance,
    Nod,
    Shake,
}

impl AnimalAnimation {
    pub const ALL: [Self; 4] = [
        AnimalAnimation::Idle,
        AnimalAnimation::Dance,
        AnimalAnimation::Nod,
        AnimalAnimation::Shake,
    ];

    /// Its clip's name in the file.
    pub fn clip(self) -> &'static str {
        match self {
            AnimalAnimation::Idle => "idle",
            AnimalAnimation::Dance => "dance",
            AnimalAnimation::Nod => "gesture-positive",
            AnimalAnimation::Shake => "gesture-negative",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// How fast it plays (1 = as in the file).
    pub fn speed(self, tuning: &DriverTuning) -> f32 {
        match self {
            AnimalAnimation::Idle => tuning.idle_speed,
            AnimalAnimation::Dance => tuning.dance_speed,
            AnimalAnimation::Nod => tuning.nod_speed,
            AnimalAnimation::Shake => tuning.shake_speed,
        }
    }
}

/// The drivers' looks (issue #69). Purely visual: nothing here touches the race. Reflected
/// (`Reflect`) so that the admin panel (issue #36) edits every field.
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct DriverTuning {
    /// How big the animal is at size 1, times its file (a Cube Pet's body is 1.25 m wide and
    /// 1.4 m high in it): 0.62 makes it about as big as the ball before it (0.9 m). Times the
    /// character's size (its weight class). Read when a driver sits in its kart (« Relancer »).
    pub model_scale: f32,
    /// How far below the seat the bottom of its body sits (m, at size 1): deeper in the kart.
    /// Read when a driver sits in its kart.
    pub seat_sink: f32,
    /// One animation fades into the next in this long (s).
    pub crossfade_seconds: f32,
    /// Struck by a paw: it shakes its head this long (s).
    pub struck_seconds: f32,
    /// Over the line among the first this many: it dances.
    pub cheer_places: usize,
    /// Over the line further back: it nods this long (s), then idles.
    pub finish_nod_seconds: f32,
    /// How fast each animation plays (1 = as in the file).
    pub idle_speed: f32,
    pub dance_speed: f32,
    pub nod_speed: f32,
    pub shake_speed: f32,
}

impl Default for DriverTuning {
    fn default() -> Self {
        Self {
            model_scale: 0.62,
            seat_sink: 0.0,
            crossfade_seconds: 0.2,
            struck_seconds: 1.0,
            cheer_places: 3,
            finish_nod_seconds: 3.0,
            idle_speed: 1.0,
            dance_speed: 1.0,
            nod_speed: 1.0,
            shake_speed: 1.0,
        }
    }
}

/// A racer over the line: its place (0 = first) and how long ago (race ticks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Finish {
    pub place: usize,
    pub ticks_ago: u32,
}

/// What a driver's kart is going through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DriverFacts {
    /// Hit by an item: spinning out or tumbling (`KartState::Hit`).
    pub hit: bool,
    /// Its last paw strike received, this many ticks ago (None: never struck).
    pub struck_ticks: Option<u32>,
    /// Over the line (None: still racing, or no race).
    pub finish: Option<Finish>,
}

/// What the driver plays: a hit or a strike first, then the finish, else idle.
pub fn driver_animation(facts: &DriverFacts, tuning: &DriverTuning) -> AnimalAnimation {
    let ticks = |seconds: f32| (seconds.max(0.0) * crate::TICK_HZ as f32).round() as u32;
    let struck = facts
        .struck_ticks
        .is_some_and(|t| t < ticks(tuning.struck_seconds));
    if facts.hit || struck {
        return AnimalAnimation::Shake;
    }
    match facts.finish {
        Some(finish) if finish.place < tuning.cheer_places => AnimalAnimation::Dance,
        Some(finish) if finish.ticks_ago < ticks(tuning.finish_nod_seconds) => AnimalAnimation::Nod,
        _ => AnimalAnimation::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished(place: usize, seconds: f32) -> DriverFacts {
        DriverFacts {
            finish: Some(Finish {
                place,
                ticks_ago: (seconds * 60.0) as u32,
            }),
            ..default()
        }
    }

    /// Issue #69: idle on the grid and in the race; a hit or a strike shakes its head, then
    /// it idles again; over the line, the first three dance (until the next race), the others
    /// nod for a while.
    #[test]
    fn each_moment_has_its_animation() {
        let tuning = DriverTuning::default();
        let play = |facts: DriverFacts| driver_animation(&facts, &tuning);
        assert_eq!(play(DriverFacts::default()), AnimalAnimation::Idle);
        let hit = DriverFacts {
            hit: true,
            ..default()
        };
        assert_eq!(play(hit), AnimalAnimation::Shake);
        // Struck by a paw: 1 s of it (60 ticks).
        let struck = |ticks| DriverFacts {
            struck_ticks: Some(ticks),
            ..default()
        };
        assert_eq!(play(struck(0)), AnimalAnimation::Shake);
        assert_eq!(play(struck(59)), AnimalAnimation::Shake);
        assert_eq!(play(struck(60)), AnimalAnimation::Idle);
        assert_eq!(play(struck(120)), AnimalAnimation::Idle);
        // Over the line.
        for place in 0..3 {
            assert_eq!(play(finished(place, 0.0)), AnimalAnimation::Dance);
            assert_eq!(play(finished(place, 60.0)), AnimalAnimation::Dance);
        }
        assert_eq!(play(finished(3, 0.0)), AnimalAnimation::Nod);
        assert_eq!(play(finished(11, 2.9)), AnimalAnimation::Nod);
        assert_eq!(play(finished(11, 3.0)), AnimalAnimation::Idle);
        // A hit after the line comes first, then the dance again.
        let mut hit_after = finished(0, 5.0);
        hit_after.hit = true;
        assert_eq!(play(hit_after), AnimalAnimation::Shake);
        hit_after.hit = false;
        assert_eq!(play(hit_after), AnimalAnimation::Dance);
        hit_after.struck_ticks = Some(10);
        assert_eq!(play(hit_after), AnimalAnimation::Shake);
    }

    #[test]
    fn the_tuning_moves_the_moments() {
        let tuning = DriverTuning {
            cheer_places: 1,
            finish_nod_seconds: 0.0,
            struck_seconds: 0.5,
            ..default()
        };
        let play = |facts: DriverFacts| driver_animation(&facts, &tuning);
        assert_eq!(play(finished(0, 0.0)), AnimalAnimation::Dance);
        assert_eq!(play(finished(1, 0.0)), AnimalAnimation::Idle);
        let struck = DriverFacts {
            struck_ticks: Some(30),
            ..default()
        };
        assert_eq!(play(struck), AnimalAnimation::Idle);
        let none = DriverTuning {
            cheer_places: 0,
            ..default()
        };
        assert_eq!(
            driver_animation(&finished(0, 0.0), &none),
            AnimalAnimation::Nod
        );
    }

    /// The clips of the Cube Pets' files, one per animation, each with its speed.
    #[test]
    fn every_animation_has_its_clip() {
        let clips: Vec<&str> = AnimalAnimation::ALL.iter().map(|a| a.clip()).collect();
        assert_eq!(
            clips,
            ["idle", "dance", "gesture-positive", "gesture-negative"]
        );
        for (i, animation) in AnimalAnimation::ALL.iter().enumerate() {
            assert_eq!(animation.index(), i);
        }
        let tuning = DriverTuning {
            shake_speed: 0.5,
            ..default()
        };
        assert_eq!(AnimalAnimation::Shake.speed(&tuning), 0.5);
        assert_eq!(AnimalAnimation::Idle.speed(&tuning), 1.0);
    }
}
