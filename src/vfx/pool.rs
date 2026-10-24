//! Issue #105: the particle pool (pure, unit-tested). A fixed number of slots, made once: a new
//! particle takes the next slot of a ring, so the pool never holds more than its cap (the oldest
//! particle gives its slot up when it is full), and nothing is allocated once it exists. The
//! systems (`systems.rs`) give each slot an entity, made once too, and show the live ones.
//!
//! A particle flies on its own (a velocity, gravity, drag) on top of what it was thrown from (its
//! `carrier`: the kart's velocity, slowed by the same drag), so a spark sprays out of a wheel and
//! is left behind by the kart. It grows and shrinks along its [`Curve`], and turns as its
//! [`Facing`] says. Its looks are a [`Shape`] (a mesh) and a [`Paint`] (a material), shared by
//! every particle.
//!
//! Issue #174: the glowing shapes ([`Shape::Glint`], [`Shape::Glow`], [`Shape::Tongue`],
//! [`Shape::Shock`]) and the smoke ([`Shape::Haze`]) have no mesh of their own: each frame, every
//! one of them is a textured quad of one shared mesh (`batch.rs`: one draw for all the glows,
//! additive, one for the smoke), its color its own (its paint, how bright it still is, how far it
//! cooled), so they fade out instead of only shrinking.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

/// The meshes of the particles (one each, shared).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Shape {
    /// A unit cube: sparks, streaks, speed lines, confetti.
    #[default]
    Cube,
    /// A low-poly ball: puffs of smoke and dust, embers, flashes, cream.
    Ball,
    /// A five-pointed star: the stars of a shock, the sparkles of a trick or a croquette.
    Star,
    /// A flat ring: the shockwave of a mini-turbo or a tier.
    Ring,
    /// Issue #174, glowing (additive, batched): a spark, a thin streak with a white-hot core and
    /// an edge of its color, stretched along its flight (its head ahead, its tail behind).
    Glint,
    /// ...a soft round glow facing the camera: a flash, an ember, a fireball.
    Glow,
    /// ...a tongue of flame, from its base (where it is) up along its flight.
    Tongue,
    /// ...a glowing ring lying across its axis: the shockwave of a tier or of a release.
    Shock,
    /// Issue #174, the smoke (blended, batched): a soft puff facing the camera, which spreads,
    /// turns and thins out.
    Haze,
}

impl Shape {
    /// The shapes with a mesh of their own (each slot's entity shows them).
    pub const ALL: [Self; 4] = [Shape::Cube, Shape::Ball, Shape::Star, Shape::Ring];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Issue #174: drawn in the shared quads of `batch.rs` (no entity of its own): additive
    /// glows, or the smoke.
    pub fn batched(self) -> Option<Batch> {
        match self {
            Shape::Glint | Shape::Glow | Shape::Tongue | Shape::Shock => Some(Batch::Glow),
            Shape::Haze => Some(Batch::Haze),
            Shape::Cube | Shape::Ball | Shape::Star | Shape::Ring => None,
        }
    }
}

/// Issue #174: the two shared meshes of the batched particles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Batch {
    /// Additive: what glows (sparks, flames, flashes, rings, halos).
    Glow,
    /// Blended, drawn back to front: the smoke.
    Haze,
}

/// The materials of the particles (one each, shared; their colors are `VfxTuning`'s).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Paint {
    /// The drift's tiers (GD § 3.4): blue, orange, purple.
    #[default]
    Blue,
    Orange,
    Purple,
    /// White hot: the flash of a tier, the core of a « fwoosh ».
    Hot,
    /// The exhaust's flames (rocket start, boost pads).
    Flame,
    /// The piment's red.
    Piment,
    /// Gold: a trick, a croquette.
    Gold,
    /// Cyan: the anti-gravity (its pads, its spin boost).
    Cyan,
    /// White: the wind, the speed lines, the slipstream.
    Wind,
    /// Yellow: the stars of a shock.
    Star,
    /// The confetti of an explosion (five colors).
    Confetti0,
    Confetti1,
    Confetti2,
    Confetti3,
    Confetti4,
    /// Lit: the drift's smoke, the « pof » of a shock.
    Smoke,
    /// Lit: the dust off the road, on a landing.
    Dust,
    /// Lit: the dark smoke of a rocket start or a stalled engine.
    Soot,
    /// Lit: the chantilly's cream.
    Cream,
    /// Issue #130, lit: the dust off the road, the color of the biome's ground (its shoulders)...
    Ground,
    /// ...and of its sand (the patches on the road).
    Sand,
    /// Issue #130: the drops of a splash, glowing a little like the bubbles.
    Water,
    /// Issue #130, lit: the foam on the water (a splash's, the wake).
    Foam,
    /// Issue #174: the road's dust (its asphalt, lighter): the tint of the tyres' smoke on it.
    Road,
}

impl Paint {
    pub const COUNT: usize = 24;
    pub const ALL: [Self; Self::COUNT] = [
        Paint::Blue,
        Paint::Orange,
        Paint::Purple,
        Paint::Hot,
        Paint::Flame,
        Paint::Piment,
        Paint::Gold,
        Paint::Cyan,
        Paint::Wind,
        Paint::Star,
        Paint::Confetti0,
        Paint::Confetti1,
        Paint::Confetti2,
        Paint::Confetti3,
        Paint::Confetti4,
        Paint::Smoke,
        Paint::Dust,
        Paint::Soot,
        Paint::Cream,
        Paint::Ground,
        Paint::Sand,
        Paint::Water,
        Paint::Foam,
        Paint::Road,
    ];
    pub const CONFETTI: [Self; 5] = [
        Paint::Confetti0,
        Paint::Confetti1,
        Paint::Confetti2,
        Paint::Confetti3,
        Paint::Confetti4,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The tier's paint (0 blue, 1 orange, 2 purple).
    pub fn tier(index: usize) -> Self {
        [Paint::Blue, Paint::Orange, Paint::Purple][index.min(2)]
    }

    /// Lit by the sun (puffs and cream look round); the others glow (unlit).
    pub fn lit(self) -> bool {
        matches!(
            self,
            Paint::Smoke
                | Paint::Dust
                | Paint::Soot
                | Paint::Cream
                | Paint::Ground
                | Paint::Sand
                | Paint::Foam
                | Paint::Road
        )
    }
}

/// How a particle's size goes over its life (`t` from 0 to 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Curve {
    /// Full size at once, then shrinks away: sparks, embers.
    #[default]
    Shrink,
    /// Grows from 40 %, then fades: smoke, dust.
    Puff,
    /// Pops in with an overshoot, holds, then shrinks away: stars, confetti, cream.
    Pop,
    /// Out and back in (a half sine): flashes, shockwaves.
    Flash,
    /// Issue #174: a wave blown out: from a quarter of its size, fast, then slower to its full
    /// size (it dies out as it goes: `batch.rs`).
    Wave,
    /// Issue #174: full size most of its life, then shrinks away fast: a trail that stays a
    /// ribbon.
    Linger,
}

impl Curve {
    /// The size factor at `t` (0 to 1 of its life).
    pub fn factor(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Shrink => 1.0 - t,
            Curve::Puff => {
                let grow = 1.0 - (1.0 - (t * 2.5).min(1.0)).powi(2);
                (0.4 + 0.6 * grow) * (1.0 - t * t)
            }
            Curve::Pop => {
                let pop = if t < 0.12 {
                    t / 0.12 * 1.25
                } else if t < 0.25 {
                    1.25 - (t - 0.12) / 0.13 * 0.25
                } else {
                    1.0
                };
                let out = if t > 0.7 { (1.0 - t) / 0.3 } else { 1.0 };
                pop * out
            }
            Curve::Flash => (PI * t).sin(),
            Curve::Wave => 0.25 + 0.75 * (1.0 - (1.0 - t).powi(3)),
            Curve::Linger => 1.0 - t * t * t,
        }
    }
}

/// How a particle turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Facing {
    /// Along its own velocity (a streak: its length along it).
    #[default]
    Streak,
    /// Toward the camera, turning on itself (stars, rings, flashes).
    Camera,
    /// Tumbling around its axis (confetti, cream, puffs).
    Tumble,
    /// Flat on its axis (a ring lying across it), growing out.
    Flat,
}

/// One particle. `life` 0: the slot is free.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Particle {
    pub shape: Shape,
    pub paint: Paint,
    pub curve: Curve,
    pub facing: Facing,
    /// Where it is (world, m).
    pub position: Vec3,
    /// Its own velocity (m/s), and the velocity of what threw it (the kart's).
    pub velocity: Vec3,
    pub carrier: Vec3,
    /// Its age and its life (s).
    pub age: f32,
    pub life: f32,
    /// Its size at full size (m), and how much longer than wide it is along its velocity (a
    /// streak; 1: as long as wide).
    pub size: f32,
    pub stretch: f32,
    /// Gravity (m/s², down the world's Y) and drag (1/s) on it.
    pub gravity: f32,
    pub drag: f32,
    /// What it turns around (unit; a streak without velocity keeps it as its direction), its
    /// angle (rad) and its spin (rad/s).
    pub axis: Vec3,
    pub angle: f32,
    pub spin: f32,
    /// It does not fall below this height (world Y: the ground under a blast).
    pub floor: f32,
    /// Issue #174: landing on its floor, it bounces this much of its fall back up (the rest of
    /// its speed rubbed off by the ground); 0: it lies there.
    pub bounce: f32,
    /// Issue #174, the batched ones (`Shape::batched`): how bright it glows (its color times
    /// this: above 1, its core burns white) or, for the smoke, how opaque it is at first...
    pub glow: f32,
    /// ...how far it cools to its paint's deep color by its end (0: not at all)...
    pub cool: f32,
    /// ...how much longer its streak gets with its speed (m per m/s)...
    pub smear: f32,
    /// ...how much of `tint`'s color it takes (the smoke: the ground's)...
    pub tint: f32,
    pub tint_paint: Paint,
    /// ...and which drawing of its shape (two flames, two puffs).
    pub variant: u8,
}

impl Particle {
    /// A particle of `shape` and `paint` at `position`, living `life` seconds at `size` m; the
    /// rest at rest (no velocity, no gravity, no floor).
    pub fn new(shape: Shape, paint: Paint, position: Vec3, life: f32, size: f32) -> Self {
        Self {
            shape,
            paint,
            position,
            life,
            size,
            stretch: 1.0,
            axis: Vec3::Y,
            floor: f32::NEG_INFINITY,
            glow: 1.0,
            ..default()
        }
    }

    /// Issue #174: how far through its life it is (0 to 1).
    pub fn progress(&self) -> f32 {
        (self.age / self.life.max(1e-6)).clamp(0.0, 1.0)
    }

    pub fn alive(&self) -> bool {
        self.life > 0.0 && self.age < self.life
    }

    /// Its size now (m).
    pub fn current_size(&self) -> f32 {
        self.size * self.curve.factor(self.age / self.life.max(1e-6))
    }

    /// Moves it on by `dt` s. False once it is over (its slot is free then).
    pub fn step(&mut self, dt: f32) -> bool {
        self.age += dt;
        if self.age >= self.life {
            self.life = 0.0;
            return false;
        }
        let keep = (-self.drag * dt).exp();
        self.velocity = self.velocity * keep - Vec3::Y * self.gravity * dt;
        self.carrier *= keep;
        self.position += (self.velocity + self.carrier) * dt;
        if self.position.y < self.floor {
            self.position.y = self.floor;
            let falling = -(self.velocity.y + self.carrier.y);
            if self.bounce > 0.0 && falling > 0.8 {
                // Issue #174: it bounces (a spark on the road), the ground rubbing its speed.
                let along = (self.velocity + self.carrier).with_y(0.0) * 0.55;
                self.carrier = Vec3::ZERO;
                self.velocity = along + Vec3::Y * falling * self.bounce;
            } else {
                // Landed: it lies there until it is over.
                self.velocity = Vec3::ZERO;
                self.carrier = Vec3::ZERO;
                self.spin = 0.0;
            }
        }
        self.angle = (self.angle + self.spin * dt).rem_euclid(TAU);
        true
    }

    /// Its transform now, `camera` where the camera is (for those that face it).
    pub fn transform(&self, camera: Vec3) -> Transform {
        let size = self.current_size().max(0.0);
        let (rotation, scale) = match self.facing {
            Facing::Streak => {
                let along = self.velocity.normalize_or(self.axis);
                (
                    Quat::from_rotation_arc(Vec3::Z, along),
                    Vec3::new(size, size, size * self.stretch),
                )
            }
            Facing::Camera => {
                let to_camera = (camera - self.position).normalize_or(Vec3::Z);
                (
                    Quat::from_rotation_arc(Vec3::Z, to_camera) * Quat::from_rotation_z(self.angle),
                    Vec3::splat(size),
                )
            }
            Facing::Tumble => (
                Quat::from_axis_angle(self.axis, self.angle),
                Vec3::new(size, size, size * self.stretch),
            ),
            Facing::Flat => (
                // The ring's mesh lies in the XZ plane: its Y along the axis.
                Quat::from_rotation_arc(Vec3::Y, self.axis),
                Vec3::new(size, size * 0.3, size),
            ),
        };
        Transform {
            translation: self.position,
            rotation,
            scale,
        }
    }
}

/// A small random number generator (xorshift): the particles' sprays. Visual only, and never the
/// race's (`RaceGrid::seed`): the race draws exactly the same numbers with or without particles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sparkle(u32);

impl Default for Sparkle {
    fn default() -> Self {
        Self(0x9E37_79B9)
    }
}

impl Sparkle {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// From 0 to 1.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// From `a` to `b`.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }

    /// From −1 to 1.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// A direction in a cone of half-angle `spread` (rad) around `axis` (unit).
    pub fn cone(&mut self, axis: Vec3, spread: f32) -> Vec3 {
        let (a, b) = axis.any_orthonormal_pair();
        let angle = self.unit() * TAU;
        let tilt = spread * self.unit().sqrt();
        (axis * tilt.cos() + (a * angle.cos() + b * angle.sin()) * tilt.sin()).normalize_or(axis)
    }

    /// A unit vector, anywhere.
    pub fn direction(&mut self) -> Vec3 {
        self.cone(Vec3::Y, PI)
    }
}

/// The pool: [`ParticlePool::SLOTS`] slots at most, `cap` of them in use (the global cap of the
/// particles, `VfxTuning::max_particles`). A new particle takes the next free slot of the ring
/// (within [`ParticlePool::PROBE`] slots): never more than `cap` alive, the oldest giving its slot
/// up when they all are.
#[derive(Clone, Debug)]
pub struct ParticlePool {
    particles: Vec<Particle>,
    cap: usize,
    next: usize,
    live: usize,
    /// Particles thrown since the start, and how many took the slot of one still alive.
    pub spawned: u64,
    pub replaced: u64,
    pub rng: Sparkle,
}

impl Default for ParticlePool {
    fn default() -> Self {
        Self::new(Self::SLOTS)
    }
}

impl ParticlePool {
    /// The most slots a pool has (the most its cap can be): as many entities, made once.
    pub const SLOTS: usize = 1536;
    /// A new particle looks this far along the ring for a free slot before it takes the slot of
    /// one still alive.
    pub const PROBE: usize = 16;

    /// A pool of `cap` particles at most (at most [`Self::SLOTS`]).
    pub fn new(cap: usize) -> Self {
        Self {
            particles: vec![Particle::default(); Self::SLOTS],
            cap: cap.min(Self::SLOTS),
            next: 0,
            live: 0,
            spawned: 0,
            replaced: 0,
            rng: Sparkle::default(),
        }
    }

    /// Its cap: `cap` (at most [`Self::SLOTS`]). The particles beyond a lower cap are over.
    pub fn set_cap(&mut self, cap: usize) {
        let cap = cap.min(Self::SLOTS);
        if cap == self.cap {
            return;
        }
        if cap < self.cap {
            for particle in &mut self.particles[cap..] {
                if particle.alive() {
                    particle.life = 0.0;
                    self.live -= 1;
                }
            }
        }
        self.cap = cap;
        if self.next >= cap {
            self.next = 0;
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    /// How many are alive.
    pub fn live(&self) -> usize {
        self.live
    }

    /// Throws `particle`: in the next slot of the ring (the oldest, if they all are alive).
    /// False with a cap of 0.
    pub fn spawn(&mut self, particle: Particle) -> bool {
        if self.cap == 0 || !particle.alive() {
            return false;
        }
        // The next slot of the ring; if it is still alive (a long-lived particle), a free one a
        // little further on; if none, the next one all the same.
        let index = (0..Self::PROBE.min(self.cap))
            .map(|k| (self.next + k) % self.cap)
            .find(|&i| !self.particles[i].alive())
            .unwrap_or(self.next);
        let slot = &mut self.particles[index];
        if slot.alive() {
            self.replaced += 1;
        } else {
            self.live += 1;
        }
        *slot = particle;
        self.next = (index + 1) % self.cap;
        self.spawned += 1;
        true
    }

    /// Every particle moves on by `dt` s; those that are over free their slot.
    pub fn step(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        for particle in &mut self.particles[..self.cap] {
            if particle.alive() && !particle.step(dt) {
                self.live -= 1;
            }
        }
    }

    /// Every particle is over (another level).
    pub fn clear(&mut self) {
        for particle in &mut self.particles {
            particle.life = 0.0;
        }
        self.live = 0;
    }

    /// The slots (all of them: alive or not).
    pub fn slots(&self) -> &[Particle] {
        &self.particles
    }

    /// The live particles.
    pub fn alive(&self) -> impl Iterator<Item = &Particle> {
        self.particles.iter().filter(|p| p.alive())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spark(life: f32) -> Particle {
        Particle::new(Shape::Cube, Paint::Blue, Vec3::ZERO, life, 0.1)
    }

    /// Issue #105 acceptance: the pool never holds more than its cap, whatever is thrown at it;
    /// when it is full, a new particle takes the oldest one's slot.
    #[test]
    fn the_pool_never_holds_more_than_its_cap() {
        let mut pool = ParticlePool::new(50);
        for frame in 0..600 {
            // Bursts far bigger than the cap, of every life.
            for k in 0..(frame % 7) * 20 {
                pool.spawn(spark(0.1 + (k % 9) as f32 * 0.2));
                assert!(pool.live() <= 50);
            }
            pool.step(1.0 / 60.0);
            assert!(pool.live() <= 50, "{}", pool.live());
            assert_eq!(pool.live(), pool.alive().count());
        }
        assert!(pool.replaced > 0, "the cap was reached");
        // Full, the oldest one gives its slot up.
        let mut pool = ParticlePool::new(3);
        for life in [1.0, 2.0, 3.0, 4.0] {
            pool.spawn(spark(life));
        }
        let lives: Vec<f32> = pool.alive().map(|p| p.life).collect();
        assert_eq!(lives, [4.0, 2.0, 3.0]);
        assert_eq!(pool.replaced, 1);
        // A long-lived particle keeps its slot while there are free ones.
        let mut pool = ParticlePool::new(10);
        pool.spawn(spark(10.0));
        for _ in 0..100 {
            pool.spawn(spark(0.01));
            pool.step(0.02);
        }
        assert_eq!(pool.live(), 1);
        assert_eq!(pool.alive().next().unwrap().life, 10.0);
        assert_eq!(pool.replaced, 0);
    }

    /// The cap is a tuning: lowered, the particles beyond it are over; 0 throws nothing; never
    /// more than the pool's slots.
    #[test]
    fn the_cap_moves_with_its_tuning() {
        let mut pool = ParticlePool::new(100);
        for _ in 0..100 {
            pool.spawn(spark(1.0));
        }
        assert_eq!(pool.live(), 100);
        pool.set_cap(40);
        assert_eq!(pool.live(), 40);
        assert_eq!(pool.alive().count(), 40);
        for _ in 0..100 {
            pool.spawn(spark(1.0));
        }
        assert_eq!(pool.live(), 40);
        pool.set_cap(0);
        assert_eq!(pool.live(), 0);
        assert!(!pool.spawn(spark(1.0)));
        pool.set_cap(1_000_000);
        assert_eq!(pool.cap(), ParticlePool::SLOTS);
        pool.clear();
        assert_eq!(pool.live(), 0);
    }

    /// A particle flies, falls, slows down and is over after its life; it lies on its floor.
    #[test]
    fn a_particle_flies_falls_and_ends() {
        let mut p = spark(0.5);
        p.velocity = Vec3::new(0.0, 4.0, -2.0);
        p.carrier = Vec3::new(10.0, 0.0, 0.0);
        p.gravity = 9.81;
        p.drag = 2.0;
        let mut top = 0.0f32;
        let mut steps = 0;
        while p.step(1.0 / 60.0) {
            top = top.max(p.position.y);
            steps += 1;
        }
        assert!((28..=30).contains(&steps), "half a second: {steps}");
        assert!(!p.alive());
        assert!(top > 0.3 && top < 0.8, "{top}");
        // Carried along, but left behind by the kart (slowed by the drag).
        assert!(p.position.x > 2.0 && p.position.x < 5.0, "{}", p.position);
        let mut confetti = spark(2.0);
        confetti.velocity = Vec3::new(1.0, 0.0, 0.0);
        confetti.gravity = 6.0;
        confetti.floor = -0.5;
        confetti.spin = 5.0;
        for _ in 0..60 {
            confetti.step(1.0 / 60.0);
        }
        assert_eq!(confetti.position.y, -0.5);
        assert_eq!(confetti.velocity, Vec3::ZERO);
        assert_eq!(confetti.spin, 0.0);
    }

    /// Issue #174: a spark that bounces comes back up off the road, slower, a few times, then
    /// lies there; it never goes under its floor.
    #[test]
    fn a_spark_bounces_on_the_road_then_lies_there() {
        let mut spark = spark(1.5);
        spark.position = Vec3::new(0.0, 0.5, 0.0);
        spark.velocity = Vec3::new(0.0, 2.0, 3.0);
        spark.carrier = Vec3::new(0.0, 0.0, -20.0);
        spark.gravity = 9.0;
        spark.floor = 0.0;
        spark.bounce = 0.45;
        let (mut bounces, mut falling, mut tops) = (0, false, Vec::new());
        let mut top = 0.0f32;
        while spark.step(1.0 / 60.0) {
            assert!(spark.position.y >= 0.0);
            let rising = spark.velocity.y > 0.0;
            if falling && rising {
                bounces += 1;
                tops.push(top);
                top = 0.0;
            }
            top = top.max(spark.position.y);
            falling = spark.velocity.y < 0.0;
        }
        assert!(bounces >= 2, "{bounces}");
        assert!(
            tops.windows(2).all(|w| w[1] < w[0]),
            "lower each time: {tops:?}"
        );
        assert_eq!(spark.velocity, Vec3::ZERO, "lying on the road at the end");
    }

    /// Each curve starts, peaks and ends where it should.
    #[test]
    fn the_curves_grow_and_shrink() {
        assert_eq!(Curve::Shrink.factor(0.0), 1.0);
        assert_eq!(Curve::Shrink.factor(1.0), 0.0);
        assert!((Curve::Puff.factor(0.0) - 0.4).abs() < 1e-6);
        assert!(Curve::Puff.factor(0.4) > 0.8);
        assert_eq!(Curve::Puff.factor(1.0), 0.0);
        assert_eq!(Curve::Pop.factor(0.0), 0.0);
        assert!((Curve::Pop.factor(0.12) - 1.25).abs() < 1e-5);
        assert_eq!(Curve::Pop.factor(0.5), 1.0);
        assert!(Curve::Pop.factor(1.0).abs() < 1e-6);
        assert!(Curve::Flash.factor(0.0).abs() < 1e-6);
        assert!((Curve::Flash.factor(0.5) - 1.0).abs() < 1e-6);
        assert_eq!(Curve::Wave.factor(0.0), 0.25);
        assert!(Curve::Wave.factor(0.3) > 0.7 && Curve::Wave.factor(1.0) == 1.0);
    }

    /// A streak lies along its velocity, as long as its stretch; a star faces the camera.
    #[test]
    fn a_streak_follows_its_velocity_and_a_star_faces_the_camera() {
        let mut p = spark(1.0);
        p.velocity = Vec3::new(3.0, 0.0, 0.0);
        p.stretch = 4.0;
        let t = p.transform(Vec3::ZERO);
        assert!((t.rotation * Vec3::Z).distance(Vec3::X) < 1e-5);
        assert!((t.scale.z - 0.4).abs() < 1e-6 && (t.scale.x - 0.1).abs() < 1e-6);
        let mut star = Particle::new(Shape::Star, Paint::Star, Vec3::ZERO, 1.0, 0.3);
        star.facing = Facing::Camera;
        star.curve = Curve::Pop;
        star.age = 0.5;
        let t = star.transform(Vec3::new(0.0, 0.0, 10.0));
        assert!((t.rotation * Vec3::Z).distance(Vec3::Z) < 1e-5);
        assert!((t.scale.x - 0.3).abs() < 1e-6);
    }

    /// The sprays: inside their cone, between their bounds, the same from the same seed.
    #[test]
    fn the_sprays_stay_in_their_cone() {
        let mut rng = Sparkle::new(7);
        let axis = Vec3::new(0.0, 0.3, 1.0).normalize();
        for _ in 0..2000 {
            let d = rng.cone(axis, 0.5);
            assert!((d.length() - 1.0).abs() < 1e-4);
            assert!(d.angle_between(axis) <= 0.5 + 1e-3);
            let x = rng.range(2.0, 3.0);
            assert!((2.0..=3.0).contains(&x));
        }
        let mut a = Sparkle::new(3);
        let mut b = Sparkle::new(3);
        assert_eq!(a.next_u32(), b.next_u32());
    }
}
