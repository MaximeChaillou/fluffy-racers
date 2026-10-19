//! Croquettes (GD § 7, issue #14): pure logic, no ECS. Everything here is unit-tested.
//!
//! - A kart holds 0 to `max_held` (10) croquettes, each worth `speed_per_croquette` (+1 %) of
//!   top speed. Picking one up gives a mini-boost; at the maximum it is still taken (and still
//!   boosts) but the count stays at 10.
//! - Hit by an item: `item_hit_loss` (3) lost; bumped by a kart: `bump_loss` (1). Never below 0.
//!   The lost croquettes fly out behind the kart and land on the ground, where anyone can pick
//!   them up (the kart that lost them a bit later), until they vanish.
//! - A croquette of the track comes back `respawn_time` (10 s) after being picked up.
//! - Pick-ups are found through a coarse grid ([`PickupGrid`]): a kart only looks at the few
//!   croquettes around it.

use std::ops::RangeInclusive;

use bevy::prelude::*;

use super::tuning::CroquetteTuning;

/// The croquettes a kart holds (GD § 7). Every kart has one when the croquettes are in the game
/// (`CroquettePlugin` makes it required by `Kart`).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Croquettes {
    pub count: u8,
    /// Seconds before a bump can cost croquettes again (`bump_grace`).
    pub bump_grace: f32,
}

impl Croquettes {
    /// Adds up to `n` croquettes without going over `max`; returns how many were added.
    pub fn gain(&mut self, n: u8, max: u8) -> u8 {
        let new = self.count.saturating_add(n).min(max).max(self.count);
        let gained = new - self.count;
        self.count = new;
        gained
    }

    /// Removes up to `n` croquettes, never below 0; returns how many were lost.
    pub fn lose(&mut self, n: u8) -> u8 {
        let lost = n.min(self.count);
        self.count -= lost;
        lost
    }

    /// The top speed factor of the croquettes held: 1 + 1 % per croquette, up to the maximum.
    pub fn speed_factor(&self, tuning: &CroquetteTuning) -> f32 {
        1.0 + tuning.speed_per_croquette * self.count.min(tuning.max_held) as f32
    }
}

/// Why a kart loses croquettes (GD § 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LossCause {
    /// Hit by an item (issue #16): 3.
    ItemHit,
    /// Bumped by another kart: 1.
    Bump,
    /// Struck by another kart's paw, or thrown by it (GD § 15, issue #34): 1, with the same 1 s
    /// of protection as a bump (a strike and the bump it causes cost one croquette).
    Strike,
    /// Issue #144: knocked over by a moving obstacle or a passer-by of the circuit: 2.
    Knocked,
}

impl LossCause {
    /// Croquettes this costs.
    pub fn croquettes(self, tuning: &CroquetteTuning) -> u8 {
        match self {
            LossCause::ItemHit => tuning.item_hit_loss,
            LossCause::Bump => tuning.bump_loss,
            LossCause::Strike => tuning.strike_loss,
            LossCause::Knocked => tuning.knock_loss,
        }
    }

    /// Whether this loss shares the bumps' grace period (`CroquetteTuning::bump_grace`): a bump
    /// and a paw strike do (GD § 15), an item hit does not (the items have their own rules).
    pub fn has_grace(self) -> bool {
        matches!(self, LossCause::Bump | LossCause::Strike)
    }
}

/// A croquette placed on the track by the circuit file. It is there to take when `respawn_in`
/// is 0.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackCroquette {
    /// Seconds before it comes back, 0 when it is there.
    pub respawn_in: f32,
}

impl TrackCroquette {
    pub fn available(&self) -> bool {
        self.respawn_in <= 0.0
    }

    /// Picked up: it comes back in `respawn_time`.
    pub fn take(&mut self, tuning: &CroquetteTuning) {
        self.respawn_in = tuning.respawn_time.max(f32::MIN_POSITIVE);
    }

    /// One tick of `dt` seconds; returns true on the tick it comes back. Half a tick of
    /// tolerance against the rounding of the sum of `dt`s: 10 s are exactly 600 ticks.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.available() {
            return false;
        }
        self.respawn_in -= dt;
        if self.respawn_in <= dt * 0.5 {
            self.respawn_in = 0.0;
            return true;
        }
        false
    }
}

/// A croquette a kart lost: it flies from `from` to `to` (on the ground), then waits there to be
/// picked up until it vanishes (`loose_lifetime`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct LooseCroquette {
    pub from: Vec3,
    pub to: Vec3,
    /// Seconds since it flew out.
    pub age: f32,
    /// The kart that lost it (it waits `owner_pickup_delay` to take it back).
    pub dropped_by: Entity,
    /// Issue #96: its hop goes up along this: the world's up, or in an anti-gravity zone the
    /// road's normal (on a wall, under a ceiling: it hops off the road and lands back on it).
    pub up: Vec3,
}

impl LooseCroquette {
    pub fn new(from: Vec3, to: Vec3, dropped_by: Entity) -> Self {
        Self {
            from,
            to,
            age: 0.0,
            dropped_by,
            up: Vec3::Y,
        }
    }

    /// The same, hopping along `up` (issue #96).
    pub fn along(self, up: Vec3) -> Self {
        Self { up, ..self }
    }

    /// Where it is `age` seconds after flying out.
    pub fn position_at(&self, tuning: &CroquetteTuning, age: f32) -> Vec3 {
        flight_position_along(
            self.from,
            self.to,
            self.up,
            tuning.scatter_height,
            tuning.scatter_flight_time,
            age,
        )
    }

    pub fn position(&self, tuning: &CroquetteTuning) -> Vec3 {
        self.position_at(tuning, self.age)
    }

    /// Whether `kart` may pick it up now.
    pub fn pickable_by(&self, tuning: &CroquetteTuning, kart: Entity) -> bool {
        let delay = if kart == self.dropped_by {
            tuning.owner_pickup_delay
        } else {
            tuning.loose_pickup_delay
        };
        self.age >= delay
    }

    /// Nobody picked it up in time: it vanishes.
    pub fn expired(&self, tuning: &CroquetteTuning) -> bool {
        self.age >= tuning.loose_lifetime
    }
}

/// Position on a parabolic hop from `from` to `to`, `height` above the straight line at its
/// middle, `flight_time` long; `to` after the landing.
pub fn flight_position(from: Vec3, to: Vec3, height: f32, flight_time: f32, age: f32) -> Vec3 {
    flight_position_along(from, to, Vec3::Y, height, flight_time, age)
}

/// The same hop, `height` above the straight line along `up` (issue #96: the road's normal in an
/// anti-gravity zone).
pub fn flight_position_along(
    from: Vec3,
    to: Vec3,
    up: Vec3,
    height: f32,
    flight_time: f32,
    age: f32,
) -> Vec3 {
    let t = (age / flight_time.max(1e-3)).clamp(0.0, 1.0);
    from.lerp(to, t) + up * (4.0 * height * t * (1.0 - t))
}

/// Horizontal directions of `n` lost croquettes flying out of a kart heading `forward`: spread
/// evenly over `spread` radians behind it, straight behind for a single one.
pub fn scatter_direction(forward: Vec3, i: u8, n: u8, spread: f32) -> Vec3 {
    let back = (-forward).with_y(0.0).normalize_or(Vec3::Z);
    Quat::from_rotation_y(scatter_angle(i, n, spread)) * back
}

/// Issue #96: the same across `up` (the road's normal of an anti-gravity zone): behind the kart,
/// along the road.
pub fn scatter_direction_along(forward: Vec3, up: Vec3, i: u8, n: u8, spread: f32) -> Vec3 {
    let back = -forward;
    let back = (back - up * back.dot(up)).normalize_or(up.any_orthonormal_vector());
    Quat::from_axis_angle(up, scatter_angle(i, n, spread)) * back
}

/// The angle of the `i`-th of `n` lost croquettes off straight behind (rad).
fn scatter_angle(i: u8, n: u8, spread: f32) -> f32 {
    if n <= 1 {
        0.0
    } else {
        -spread / 2.0 + spread * i as f32 / (n - 1) as f32
    }
}

/// Whether `point` (a croquette's center) touches a kart: inside its collision box (half size
/// `half_extents`, in the kart's frame) grown by `reach` on every side.
pub fn touches(kart: &Transform, half_extents: Vec3, reach: f32, point: Vec3) -> bool {
    let local = kart.rotation.inverse() * (point - kart.translation);
    let limit = half_extents + Vec3::splat(reach);
    local.x.abs() <= limit.x && local.y.abs() <= limit.y && local.z.abs() <= limit.z
}

/// A croquette that can be picked up this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickupEntry {
    pub entity: Entity,
    pub position: Vec3,
    /// This kart may not take it yet (it just lost it).
    pub not_for: Option<Entity>,
    /// Already taken this tick (by a kart iterated earlier).
    pub taken: bool,
}

/// Size of a cell of the [`PickupGrid`] (m): much larger than a kart's reach, so a kart looks at
/// 4 cells at most.
pub const PICKUP_CELL: f32 = 8.0;
/// Number of buckets of the [`PickupGrid`]: cells far apart may share one (the exact test sorts
/// them out).
pub const PICKUP_BUCKETS: usize = 64;

/// The croquettes that can be picked up this tick, bucketed on a coarse horizontal grid: each
/// kart only tests the croquettes of the cells around it, never every croquette. Rebuilt every
/// tick into buffers kept between ticks (no allocation once warmed up).
#[derive(Debug)]
pub struct PickupGrid {
    entries: Vec<PickupEntry>,
    /// Indices into `entries`, per bucket.
    buckets: Vec<Vec<u32>>,
}

impl Default for PickupGrid {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            buckets: vec![Vec::new(); PICKUP_BUCKETS],
        }
    }
}

fn cell_of(v: f32) -> i32 {
    (v / PICKUP_CELL).floor() as i32
}

fn bucket_of(x: i32, z: i32) -> usize {
    let hash = (x as u32).wrapping_mul(73_856_093) ^ (z as u32).wrapping_mul(19_349_663);
    hash as usize % PICKUP_BUCKETS
}

impl PickupGrid {
    pub fn clear(&mut self) {
        self.entries.clear();
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn insert(&mut self, entity: Entity, position: Vec3, not_for: Option<Entity>) {
        let index = self.entries.len() as u32;
        self.entries.push(PickupEntry {
            entity,
            position,
            not_for,
            taken: false,
        });
        self.buckets[bucket_of(cell_of(position.x), cell_of(position.z))].push(index);
    }

    /// Takes every croquette touching `kart` (body `transform`, collision box `half_extents`
    /// grown by `reach`) that is not taken yet nor kept away from it, and calls `take` with each.
    /// Returns how many croquettes were tested.
    pub fn take_touching(
        &mut self,
        kart: Entity,
        transform: &Transform,
        half_extents: Vec3,
        reach: f32,
        mut take: impl FnMut(&PickupEntry),
    ) -> usize {
        let radius = (half_extents + Vec3::splat(reach)).length();
        let p = transform.translation;
        let cells = |c: f32| -> RangeInclusive<i32> { cell_of(c - radius)..=cell_of(c + radius) };
        let Self { entries, buckets } = self;
        let mut tested = 0;
        for x in cells(p.x) {
            for z in cells(p.z) {
                for &index in &buckets[bucket_of(x, z)] {
                    let entry = &mut entries[index as usize];
                    tested += 1;
                    if entry.taken
                        || entry.not_for == Some(kart)
                        || !touches(transform, half_extents, reach, entry.position)
                    {
                        continue;
                    }
                    entry.taken = true;
                    take(entry);
                }
            }
        }
        tested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const HALF: Vec3 = Vec3::new(0.7, 0.25, 1.0);

    fn tuning() -> CroquetteTuning {
        CroquetteTuning::default()
    }

    #[test]
    fn a_kart_holds_ten_croquettes_at_most() {
        let t = tuning();
        let mut held = Croquettes::default();
        let mut gained = 0;
        for _ in 0..14 {
            gained += held.gain(1, t.max_held);
        }
        assert_eq!(held.count, 10);
        assert_eq!(gained, 10);
        assert_eq!(held.gain(1, t.max_held), 0, "full");
        assert_eq!(held.count, 10);
        // A golden croquette (+2, issue #16) at 9 only fills up to 10.
        let mut nine = Croquettes {
            count: 9,
            ..default()
        };
        assert_eq!(nine.gain(2, t.max_held), 1);
        assert_eq!(nine.count, 10);
        assert_eq!(held.gain(u8::MAX, t.max_held), 0);
    }

    #[test]
    fn each_croquette_adds_one_percent_of_top_speed_up_to_ten() {
        let t = tuning();
        let factor = |count: u8| Croquettes { count, ..default() }.speed_factor(&t);
        assert_eq!(factor(0), 1.0);
        assert!((factor(1) - 1.01).abs() < 1e-6);
        assert!((factor(5) - 1.05).abs() < 1e-6);
        assert!((factor(10) - 1.10).abs() < 1e-6);
        // Never more than the maximum's bonus.
        assert!((factor(30) - 1.10).abs() < 1e-6);
    }

    #[test]
    fn an_item_costs_three_a_bump_one_never_below_zero() {
        let t = tuning();
        let mut held = Croquettes {
            count: 5,
            ..default()
        };
        assert_eq!(held.lose(LossCause::ItemHit.croquettes(&t)), 3);
        assert_eq!(held.count, 2);
        assert_eq!(held.lose(LossCause::Bump.croquettes(&t)), 1);
        assert_eq!(held.count, 1);
        assert_eq!(
            held.lose(LossCause::ItemHit.croquettes(&t)),
            1,
            "only 1 left"
        );
        assert_eq!(held.count, 0);
        assert_eq!(held.lose(LossCause::Bump.croquettes(&t)), 0);
        assert_eq!(held.lose(LossCause::ItemHit.croquettes(&t)), 0);
        assert_eq!(held.count, 0);
    }

    #[test]
    fn a_track_croquette_comes_back_after_exactly_ten_seconds() {
        let t = tuning();
        let mut croquette = TrackCroquette::default();
        assert!(croquette.available());
        assert!(!croquette.step(DT), "already there");
        croquette.take(&t);
        assert!(!croquette.available());
        for tick in 1..600 {
            assert!(!croquette.step(DT), "back too early at tick {tick}");
            assert!(!croquette.available());
        }
        assert!(croquette.step(DT), "600 ticks = 10 s");
        assert!(croquette.available());
        assert_eq!(croquette.respawn_in, 0.0);
    }

    #[test]
    fn a_lost_croquette_hops_to_the_ground_and_waits_there() {
        let t = tuning();
        let kart = Entity::from_raw_u32(7).unwrap();
        let other = Entity::from_raw_u32(8).unwrap();
        let from = Vec3::new(0.0, 0.8, 0.0);
        let to = Vec3::new(0.0, 0.35, 2.5);
        let mut loose = LooseCroquette::new(from, to, kart);
        assert_eq!(loose.position(&t), from);
        // Higher than both ends in the middle of the flight.
        let middle = loose.position_at(&t, t.scatter_flight_time / 2.0);
        assert!(
            middle.y > from.y && (middle.z - 1.25).abs() < 1e-4,
            "{middle}"
        );
        assert_eq!(loose.position_at(&t, t.scatter_flight_time), to);
        assert_eq!(loose.position_at(&t, 5.0), to);
        // Others can take it first, the kart that lost it later; it vanishes in the end.
        assert!(!loose.pickable_by(&t, other));
        loose.age = t.loose_pickup_delay;
        assert!(loose.pickable_by(&t, other));
        assert!(!loose.pickable_by(&t, kart));
        loose.age = t.owner_pickup_delay;
        assert!(loose.pickable_by(&t, kart));
        assert!(!loose.expired(&t));
        loose.age = t.loose_lifetime;
        assert!(loose.expired(&t));
    }

    #[test]
    fn lost_croquettes_fly_out_behind_the_kart() {
        let spread = 100f32.to_radians();
        let forward = Vec3::NEG_Z;
        // One: straight behind.
        let one = scatter_direction(forward, 0, 1, spread);
        assert!(one.distance(Vec3::Z) < 1e-5, "{one}");
        // Three: behind left, behind, behind right, evenly spread.
        let three: Vec<Vec3> = (0..3)
            .map(|i| scatter_direction(forward, i, 3, spread))
            .collect();
        assert!(three[1].distance(Vec3::Z) < 1e-5);
        for d in &three {
            assert!(d.z > 0.0 && (d.length() - 1.0).abs() < 1e-5, "{d}");
            assert_eq!(d.y, 0.0);
        }
        assert!((three[0].angle_between(three[2]) - spread).abs() < 1e-4);
        assert!(three[0].x * three[2].x < 0.0, "one on each side");
        // A kart nosing up a slope still throws them horizontally.
        let up_slope = Vec3::new(0.0, 0.3, -1.0).normalize();
        assert_eq!(scatter_direction(up_slope, 0, 1, spread).y, 0.0);
    }

    #[test]
    fn a_kart_touches_croquettes_within_its_box_and_reach() {
        let reach = 0.45;
        // A kart turned 90° (looking down -X).
        let kart = Transform::from_xyz(10.0, 0.55, 5.0).looking_to(Vec3::NEG_X, Vec3::Y);
        let at = |x: f32, y: f32, z: f32| {
            touches(&kart, HALF, reach, kart.transform_point(Vec3::new(x, y, z)))
        };
        assert!(at(0.0, 0.0, 0.0));
        assert!(at(1.1, 0.05, 1.4), "corner");
        assert!(at(-1.1, -0.2, -1.4));
        assert!(!at(1.2, 0.0, 0.0), "beside it");
        assert!(!at(0.0, 0.0, 1.5), "behind it");
        assert!(!at(0.0, 0.75, 0.0), "above it (a jump)");
    }

    #[test]
    fn the_grid_finds_exactly_the_croquettes_a_kart_touches() {
        let reach = 0.45;
        let id = |i: u32| Entity::from_raw_u32(i).unwrap();
        let mut grid = PickupGrid::default();
        // A line of croquettes every meter along -Z, crossing cell borders, plus far away ones
        // that may share buckets.
        for i in 0..40 {
            grid.insert(id(i), Vec3::new(3.0, 0.6, -(i as f32)), None);
        }
        for i in 0..40 {
            grid.insert(
                id(100 + i),
                Vec3::new(200.0 + 8.0 * i as f32, 0.6, 51.0),
                None,
            );
        }
        assert_eq!(grid.len(), 80);
        let kart_entity = id(1000);
        let kart = Transform::from_xyz(3.0, 0.55, -16.0);
        let mut taken = Vec::new();
        let tested = grid.take_touching(kart_entity, &kart, HALF, reach, |e| taken.push(e.entity));
        // The kart (2 m long, plus the reach) touches the croquettes at z = -15, -16 and -17.
        let mut expected = vec![id(15), id(16), id(17)];
        expected.sort();
        taken.sort();
        assert_eq!(taken, expected);
        assert!(tested < 40, "tested {tested} of 80");
        // Taken this tick: not again.
        let mut again = 0;
        grid.take_touching(kart_entity, &kart, HALF, reach, |_| again += 1);
        assert_eq!(again, 0);
        // A croquette kept away from this kart is left to the others.
        grid.clear();
        assert!(grid.is_empty());
        grid.insert(id(1), Vec3::new(3.0, 0.6, -16.0), Some(kart_entity));
        let mut count = 0;
        grid.take_touching(kart_entity, &kart, HALF, reach, |_| count += 1);
        assert_eq!(count, 0);
        grid.take_touching(id(2000), &kart, HALF, reach, |_| count += 1);
        assert_eq!(count, 1);
        // Negative coordinates and cell borders: a croquette right on a border is found.
        grid.clear();
        grid.insert(id(3), Vec3::new(-8.0, 0.6, -8.0), None);
        let near = Transform::from_xyz(-8.3, 0.55, -7.6);
        let mut found = 0;
        grid.take_touching(kart_entity, &near, HALF, reach, |_| found += 1);
        assert_eq!(found, 1);
    }

    /// Cost of the pick-up search, run by hand (numbers go in STATUS.md):
    /// `cargo test --release --lib measure_pickup_cost -- --ignored --nocapture`.
    /// 12 karts in a pack and 60 croquettes along a 536 m loop (lines and scattered ones): the
    /// grid against testing every kart against every croquette.
    #[test]
    #[ignore]
    fn measure_pickup_cost() {
        let id = |i: u32| Entity::from_raw_u32(i).unwrap();
        let on_loop = |d: f32| {
            let a = d / 536.0 * std::f32::consts::TAU;
            Vec3::new(85.0 * a.cos(), 0.6, 85.0 * a.sin())
        };
        let croquettes: Vec<Vec3> = (0..60).map(|i| on_loop(9.0 * i as f32)).collect();
        let karts: Vec<Transform> = (0..12)
            .map(|k| Transform::from_translation(on_loop(100.0 + 4.0 * k as f32 + 1.3)))
            .collect();
        let rounds = 20_000;
        let mut grid = PickupGrid::default();
        let start = std::time::Instant::now();
        let mut tested = 0;
        for _ in 0..rounds {
            grid.clear();
            for (i, at) in croquettes.iter().enumerate() {
                grid.insert(id(i as u32), *at, None);
            }
            for (k, kart) in karts.iter().enumerate() {
                tested += grid.take_touching(id(1000 + k as u32), kart, HALF, 0.45, |_| {});
            }
        }
        let grid_us = start.elapsed().as_secs_f64() * 1e6 / rounds as f64;
        let start = std::time::Instant::now();
        let mut touching = 0;
        for _ in 0..rounds {
            for kart in &karts {
                for at in &croquettes {
                    touching += touches(kart, HALF, 0.45, *at) as usize;
                }
            }
        }
        let naive_us = start.elapsed().as_secs_f64() * 1e6 / rounds as f64;
        println!(
            "12 karts, 60 croquettes: grid {grid_us:.2} µs per tick ({} croquettes tested per \
             tick instead of 720), every pair {naive_us:.2} µs ({touching})",
            tested / rounds
        );
    }
}
