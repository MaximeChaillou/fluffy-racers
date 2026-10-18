//! Issue #105: the effects, as particles thrown into the pool (pure: no ECS, nothing allocated).
//! Each function throws one effect where a kart is shown ([`KartFrame`]: its visual's pose, its
//! drift's slip, its velocity) or at a point of the world. The systems (`systems.rs`) call them
//! with what they read of the game; the tests call them directly.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use super::cues::SparkStyle;
use super::pool::{Curve, Facing, Paint, Particle, ParticlePool, Shape};
use super::tuning::{BoostVfx, DriftVfx, ImpactVfx, VfxTuning, WorldVfx};

/// Where the sparks leave a rear wheel (the right one; the left one at −x), in the kart's frame
/// (its front is −Z): behind and under it, on the road.
pub const REAR_WHEEL: Vec3 = Vec3::new(0.62, -0.42, 0.92);
/// The front wheels (anti-gravity's sparks).
pub const FRONT_WHEEL: Vec3 = Vec3::new(0.62, -0.42, -0.75);
/// An exhaust (the right one), at the back of the body (`kart::visual`'s flames).
pub const EXHAUST: Vec3 = Vec3::new(0.3, -0.02, 1.05);
/// A wing tip of the glider (the right one), at its back edge.
pub const WING_TIP: Vec3 = Vec3::new(2.1, 2.05, 0.8);
/// The bottom of the kart, at its middle.
pub const UNDER: Vec3 = Vec3::new(0.0, -0.4, 0.0);
/// The top of the kart (where the cream lands).
pub const TOP: Vec3 = Vec3::new(0.0, 0.7, 0.0);

/// Where and how a kart is shown, this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KartFrame {
    /// Its visual (interpolated, or placed by the network): where it is, how it is turned.
    pub position: Vec3,
    pub rotation: Quat,
    /// Its body's slip in a drift, around the front axle (`kart::slip_pose`): the rear wheels
    /// are out of the turn.
    pub slip: Quat,
    pub slip_offset: Vec3,
    /// How fast it goes (m/s, world: measured on its visual).
    pub velocity: Vec3,
    /// How big it looks (deflated: smaller).
    pub scale: f32,
}

impl Default for KartFrame {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            slip: Quat::IDENTITY,
            slip_offset: Vec3::ZERO,
            velocity: Vec3::ZERO,
            scale: 1.0,
        }
    }
}

impl KartFrame {
    /// A point of the body (its frame: the slip turns it), in the world.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.position + self.rotation * (self.slip_offset + self.slip * (local * self.scale))
    }

    /// A direction of the body, in the world.
    pub fn direction(&self, local: Vec3) -> Vec3 {
        (self.rotation * (self.slip * local)).normalize_or_zero()
    }

    /// A direction of the kart's frame (no slip), in the world.
    pub fn axis(&self, local: Vec3) -> Vec3 {
        (self.rotation * local).normalize_or_zero()
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }
}

/// How many particles an emitter throws this frame: `rate` per second over `dt` s, the rest
/// carried in `carry` to the next frame (no particle lost to the rounding), at most `max`.
pub fn take(carry: &mut f32, rate: f32, dt: f32, max: u32) -> u32 {
    if rate <= 0.0 || dt <= 0.0 {
        *carry = 0.0;
        return 0;
    }
    *carry += rate * dt;
    let count = (carry.floor() as u32).min(max);
    *carry -= count as f32;
    // A long frame does not pile up for later.
    *carry = carry.min(1.0);
    count
}

/// Where the `i`-th of `n` particles of a frame starts: spread along the way the kart came this
/// frame, so that a trail stays a trail at 30 frames per second.
fn along(at: Vec3, travel: Vec3, i: u32, n: u32) -> Vec3 {
    at - travel * (1.0 - (i as f32 + 0.5) / n.max(1) as f32)
}

/// A drift's sparks out of a rear wheel (`side`: −1 left, 1 right): back, up and out of the
/// wheel, in a cone; streaks of their tier's color, falling, left behind by the kart.
pub fn drift_sparks(
    pool: &mut ParticlePool,
    frame: &KartFrame,
    side: f32,
    style: &SparkStyle,
    drift: &DriftVfx,
    count: u32,
    dt: f32,
) {
    let at = frame.point(REAR_WHEEL * Vec3::new(side, 1.0, 1.0));
    let out = frame.direction(Vec3::new(side * 0.45, 0.55, 1.0));
    for i in 0..count {
        let direction = pool.rng.cone(out, drift.spark_spread);
        let speed = style.speed * pool.rng.range(0.7, 1.15);
        let life = style.life * pool.rng.range(0.75, 1.1);
        let mut spark = Particle::new(
            Shape::Cube,
            style.paint,
            along(at, frame.velocity * dt, i, count),
            life,
            style.size,
        );
        spark.velocity = direction * speed;
        spark.carrier = frame.velocity * 0.85;
        spark.gravity = drift.spark_gravity;
        spark.drag = 3.0;
        spark.stretch = style.length / style.size.max(1e-3);
        spark.axis = direction;
        pool.spawn(spark);
    }
}

/// A tier reached (GD § 3.4): a burst of sparks of its color at each rear wheel, a white flash
/// at each, and a ring of its color around the back of the kart.
pub fn tier_flash(pool: &mut ParticlePool, frame: &KartFrame, tier: usize, drift: &DriftVfx) {
    let tier = tier.min(2);
    let paint = Paint::tier(tier);
    for side in [-1.0f32, 1.0] {
        let at = frame.point(REAR_WHEEL * Vec3::new(side, 1.0, 1.0));
        let out = frame.direction(Vec3::new(side * 0.6, 0.8, 0.6));
        for _ in 0..drift.flash_sparks[tier] {
            let direction = pool.rng.cone(out, 1.1);
            let mut spark = Particle::new(
                Shape::Cube,
                paint,
                at,
                drift.spark_life[tier] * pool.rng.range(1.0, 1.5),
                drift.spark_size[tier] * 1.2,
            );
            spark.velocity = direction * drift.spark_speed[tier] * pool.rng.range(1.2, 1.9);
            spark.carrier = frame.velocity * 0.9;
            spark.gravity = drift.spark_gravity;
            spark.drag = 2.5;
            spark.stretch = drift.spark_length[tier] / drift.spark_size[tier].max(1e-3);
            pool.spawn(spark);
        }
        let mut flash = Particle::new(
            Shape::Ball,
            Paint::Hot,
            at,
            drift.flash_time,
            drift.flash_size[tier],
        );
        flash.curve = Curve::Flash;
        flash.facing = Facing::Camera;
        flash.carrier = frame.velocity;
        pool.spawn(flash);
    }
    let mut ring = Particle::new(
        Shape::Ring,
        paint,
        frame.point(Vec3::new(0.0, -0.3, 0.7)),
        drift.flash_time * 1.4,
        drift.flash_ring[tier],
    );
    ring.curve = Curve::Flash;
    ring.facing = Facing::Flat;
    ring.axis = frame.up();
    ring.carrier = frame.velocity;
    pool.spawn(ring);
}

/// The tyres' smoke while drifting, out of a rear wheel: puffs that rise and stay behind.
pub fn drift_smoke(
    pool: &mut ParticlePool,
    frame: &KartFrame,
    side: f32,
    drift: &DriftVfx,
    count: u32,
    dt: f32,
) {
    let at = frame.point(REAR_WHEEL * Vec3::new(side, 1.0, 1.0) + Vec3::new(0.0, 0.1, 0.1));
    let up = frame.up();
    for i in 0..count {
        let mut puff = Particle::new(
            Shape::Ball,
            Paint::Smoke,
            along(at, frame.velocity * dt, i, count),
            drift.smoke_life * pool.rng.range(0.8, 1.2),
            drift.smoke_size * pool.rng.range(0.75, 1.25),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = up * drift.smoke_rise
            + frame.direction(Vec3::new(side * 0.6, 0.0, 0.5)) * pool.rng.range(0.3, 1.0);
        puff.carrier = frame.velocity * 0.25;
        puff.drag = 1.5;
        puff.axis = pool.rng.direction();
        puff.spin = pool.rng.signed() * 2.0;
        pool.spawn(puff);
    }
}

/// The release of a mini-turbo (« fwoosh », GD § 3.5): a burst of sparks of its tier out of the
/// exhaust, a white flash and a ring of its color blown out behind the kart.
pub fn release(pool: &mut ParticlePool, frame: &KartFrame, tier: usize, drift: &DriftVfx) {
    let tier = tier.min(2);
    let paint = Paint::tier(tier);
    let at = frame.point(Vec3::new(0.0, EXHAUST.y, EXHAUST.z));
    let back = frame.direction(Vec3::new(0.0, 0.25, 1.0));
    for _ in 0..drift.release_sparks[tier] {
        let direction = pool.rng.cone(back, 0.6);
        let mut spark = Particle::new(
            Shape::Cube,
            paint,
            at + pool.rng.direction() * 0.2,
            drift.spark_life[tier] * pool.rng.range(1.0, 1.6),
            drift.spark_size[tier] * 1.3,
        );
        spark.velocity = direction * drift.release_speed * pool.rng.range(0.6, 1.2);
        spark.carrier = frame.velocity * 0.6;
        spark.gravity = drift.spark_gravity * 0.5;
        spark.drag = 2.0;
        spark.stretch = drift.spark_length[tier] * 1.6 / drift.spark_size[tier].max(1e-3);
        pool.spawn(spark);
    }
    let mut flash = Particle::new(
        Shape::Ball,
        Paint::Hot,
        at,
        drift.flash_time,
        drift.flash_size[tier] * 1.4,
    );
    flash.curve = Curve::Flash;
    flash.carrier = frame.velocity * 0.8;
    pool.spawn(flash);
    let mut ring = Particle::new(
        Shape::Ring,
        paint,
        at + back * 0.3,
        drift.flash_time * 1.2,
        drift.release_ring[tier],
    );
    // It stays at the exhaust (blown out around it), never left behind across the screen.
    ring.curve = Curve::Flash;
    ring.facing = Facing::Flat;
    ring.axis = frame.forward();
    ring.velocity = back * 0.5;
    ring.carrier = frame.velocity;
    pool.spawn(ring);
}

/// A boost's trail out of the exhausts (a mini-turbo's: its tier's color): short streaks left
/// behind.
pub fn trail(pool: &mut ParticlePool, frame: &KartFrame, paint: Paint, count: u32, dt: f32) {
    for i in 0..count {
        let side = if (pool.spawned + u64::from(i)).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        let at = frame.point(EXHAUST * Vec3::new(side, 1.0, 1.0));
        let direction = pool.rng.cone(frame.direction(Vec3::Z), 0.25);
        let mut streak = Particle::new(
            Shape::Cube,
            paint,
            along(at, frame.velocity * dt, i, count),
            pool.rng.range(0.15, 0.25),
            0.07,
        );
        streak.velocity = direction * pool.rng.range(2.0, 4.0);
        streak.carrier = frame.velocity * 0.7;
        streak.drag = 2.0;
        streak.stretch = 5.0;
        pool.spawn(streak);
    }
}

/// Embers out of the exhausts during a boost (GD § 14: the flames' size, `flame` 0 to about 1.3,
/// follows the boost's strength and its time left): bigger and more of them for bigger flames.
pub fn embers(
    pool: &mut ParticlePool,
    frame: &KartFrame,
    paint: Paint,
    flame: f32,
    boost: &BoostVfx,
    count: u32,
    dt: f32,
) {
    for i in 0..count {
        let side = if (pool.spawned + u64::from(i)).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        let at = frame.point(EXHAUST * Vec3::new(side, 1.0, 1.0) + Vec3::Z * 0.3 * flame);
        let direction = pool.rng.cone(
            frame.direction(Vec3::new(0.0, 0.3, 1.0)),
            boost.ember_spread,
        );
        let mut ember = Particle::new(
            Shape::Ball,
            paint,
            along(at, frame.velocity * dt, i, count),
            boost.ember_life * pool.rng.range(0.7, 1.2),
            boost.ember_size * (0.5 + 0.5 * flame) * pool.rng.range(0.7, 1.2),
        );
        ember.velocity = direction * boost.ember_speed * (0.6 + 0.6 * flame);
        ember.carrier = frame.velocity * 0.75;
        ember.drag = 2.0;
        ember.gravity = -1.0;
        ember.facing = Facing::Tumble;
        pool.spawn(ember);
    }
}

/// The rocket start (GD § 4): a cloud of smoke blown out of the back of the kart, staying on the
/// grid.
pub fn rocket_smoke(pool: &mut ParticlePool, frame: &KartFrame, boost: &BoostVfx) {
    let at = frame.point(Vec3::new(0.0, -0.2, 1.1));
    let back = frame.axis(Vec3::new(0.0, 0.35, 1.0));
    for k in 0..boost.rocket_smoke {
        let direction = pool.rng.cone(back, 1.0);
        let mut puff = Particle::new(
            Shape::Ball,
            if k % 4 == 0 {
                Paint::Soot
            } else {
                Paint::Smoke
            },
            at + pool.rng.direction() * 0.3,
            boost.rocket_smoke_life * pool.rng.range(0.7, 1.2),
            boost.rocket_smoke_size * pool.rng.range(0.6, 1.3),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = direction * pool.rng.range(2.0, 5.0) + Vec3::Y * pool.rng.range(0.5, 1.5);
        puff.drag = 2.2;
        puff.gravity = -1.0;
        puff.axis = pool.rng.direction();
        puff.spin = pool.rng.signed() * 1.5;
        pool.spawn(puff);
    }
}

/// Puffs of soot out of the exhaust: the stalled engine chokes (GD § 4).
pub fn stall_puffs(pool: &mut ParticlePool, frame: &KartFrame, boost: &BoostVfx) {
    for k in 0..boost.stall_puffs {
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        let mut puff = Particle::new(
            Shape::Ball,
            Paint::Soot,
            frame.point(EXHAUST * Vec3::new(side, 1.0, 1.0)),
            pool.rng.range(0.5, 0.8),
            pool.rng.range(0.25, 0.4),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = pool
            .rng
            .cone(frame.direction(Vec3::new(0.0, 1.0, 0.6)), 0.5)
            * pool.rng.range(1.0, 2.5);
        puff.drag = 1.5;
        pool.spawn(puff);
    }
}

/// Sparkles (stars) bursting out of `at`: a trick, a croquette, a trick's boost.
pub fn sparkles(
    pool: &mut ParticlePool,
    at: Vec3,
    carrier: Vec3,
    paint: Paint,
    count: u32,
    size: f32,
    life: f32,
    speed: f32,
) {
    for _ in 0..count {
        let direction = pool.rng.cone(Vec3::Y, 1.4);
        let mut star = Particle::new(
            Shape::Star,
            paint,
            at + direction * 0.3,
            life * pool.rng.range(0.7, 1.2),
            size * pool.rng.range(0.7, 1.2),
        );
        star.curve = Curve::Pop;
        star.facing = Facing::Camera;
        star.velocity = direction * speed * pool.rng.range(0.5, 1.1);
        star.carrier = carrier;
        star.drag = 2.5;
        star.gravity = 1.5;
        star.angle = pool.rng.unit() * TAU;
        star.spin = pool.rng.signed() * 6.0;
        pool.spawn(star);
    }
}

/// Streaks of wind around the kart, shooting ahead of it: the slipstream's boost (GD § 6).
pub fn wind_burst(pool: &mut ParticlePool, frame: &KartFrame, count: u32) {
    let forward = frame.forward();
    for k in 0..count {
        let angle = k as f32 / count.max(1) as f32 * TAU;
        let around = frame.axis(Vec3::new(angle.cos() * 1.3, 0.5 + angle.sin() * 0.8, 0.0));
        let mut streak = Particle::new(
            Shape::Cube,
            Paint::Wind,
            frame.position + around * frame.scale + forward * pool.rng.range(-1.0, 1.0),
            pool.rng.range(0.2, 0.3),
            0.04,
        );
        streak.curve = Curve::Flash;
        streak.velocity = forward * pool.rng.range(8.0, 14.0);
        streak.carrier = frame.velocity;
        streak.stretch = 30.0;
        pool.spawn(streak);
    }
}

/// Sparks spraying flat all around the kart (the anti-gravity's spin boost), or out of its back
/// (a boost pad, a piment).
pub fn spray(pool: &mut ParticlePool, frame: &KartFrame, paint: Paint, count: u32, backward: bool) {
    let at = frame.point(UNDER);
    for k in 0..count {
        let direction = if backward {
            pool.rng
                .cone(frame.direction(Vec3::new(0.0, 0.3, 1.0)), 0.7)
        } else {
            let angle = k as f32 / count.max(1) as f32 * TAU + pool.rng.unit() * 0.3;
            frame.axis(Vec3::new(angle.cos(), 0.25, angle.sin()))
        };
        let mut spark = Particle::new(Shape::Cube, paint, at, pool.rng.range(0.25, 0.4), 0.08);
        spark.velocity = direction * pool.rng.range(5.0, 9.0);
        spark.carrier = frame.velocity * 0.8;
        spark.gravity = 6.0;
        spark.drag = 2.5;
        spark.stretch = 4.0;
        pool.spawn(spark);
    }
}

/// A shock at `at` (a bump, a wall, a paw strike: GD § 14): `stars` yellow stars bursting out,
/// mostly up and along `normal` (away from what was hit), and `puffs` white « pof » puffs.
pub fn shock(
    pool: &mut ParticlePool,
    at: Vec3,
    normal: Vec3,
    carrier: Vec3,
    stars: u32,
    puffs: u32,
    impact: &ImpactVfx,
) {
    let out = (normal.normalize_or_zero() * 0.6 + Vec3::Y).normalize();
    for _ in 0..stars {
        let direction = pool.rng.cone(out, 0.9);
        let mut star = Particle::new(
            Shape::Star,
            Paint::Star,
            at,
            impact.star_life * pool.rng.range(0.8, 1.2),
            impact.star_size * pool.rng.range(0.75, 1.25),
        );
        star.curve = Curve::Pop;
        star.facing = Facing::Camera;
        star.velocity = direction * impact.star_speed * pool.rng.range(0.6, 1.2);
        star.carrier = carrier * 0.8;
        star.gravity = impact.star_gravity;
        star.drag = 1.0;
        star.angle = pool.rng.unit() * TAU;
        star.spin = pool.rng.signed() * 9.0;
        pool.spawn(star);
    }
    for _ in 0..puffs {
        let direction = pool.rng.direction();
        let mut puff = Particle::new(
            Shape::Ball,
            Paint::Smoke,
            at + direction * 0.15,
            impact.pof_life * pool.rng.range(0.8, 1.2),
            impact.pof_size * pool.rng.range(0.7, 1.2),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = direction * pool.rng.range(1.5, 3.0);
        puff.carrier = carrier * 0.8;
        puff.drag = 4.0;
        pool.spawn(puff);
    }
}

/// An explosion's confetti (GD § 8 and 14) at `at`, `radius` its reach: they fly up and out,
/// tumble, fall and lie on the ground for a moment; and puffs of smoke.
pub fn confetti(pool: &mut ParticlePool, at: Vec3, radius: f32, impact: &ImpactVfx) {
    let floor = at.y - 0.45;
    let reach = (radius / 4.0).clamp(0.6, 1.6);
    for k in 0..impact.confetti {
        let direction = pool.rng.cone(Vec3::Y, 1.25);
        let mut piece = Particle::new(
            Shape::Cube,
            Paint::CONFETTI[k as usize % 5],
            at + direction * 0.3,
            impact.confetti_life * pool.rng.range(0.75, 1.2),
            impact.confetti_size * pool.rng.range(0.7, 1.3),
        );
        piece.curve = Curve::Pop;
        piece.facing = Facing::Tumble;
        piece.velocity = direction * impact.confetti_speed * reach * pool.rng.range(0.4, 1.0);
        piece.gravity = 6.0;
        piece.drag = 1.2;
        piece.stretch = 0.25;
        piece.axis = pool.rng.direction();
        piece.spin = pool.rng.range(6.0, 13.0);
        piece.floor = floor;
        pool.spawn(piece);
    }
    for _ in 0..impact.blast_puffs {
        let direction = pool.rng.cone(Vec3::Y, 1.4);
        let mut puff = Particle::new(
            Shape::Ball,
            Paint::Smoke,
            at + direction * radius * 0.25,
            pool.rng.range(0.5, 0.9),
            impact.pof_size * reach * pool.rng.range(1.0, 1.8),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = direction * pool.rng.range(2.0, 5.0) * reach;
        puff.drag = 3.0;
        puff.floor = floor;
        pool.spawn(puff);
    }
}

/// A chantilly on a kart: blobs of cream splash off its top and land on the road around it.
pub fn cream(pool: &mut ParticlePool, frame: &KartFrame, impact: &ImpactVfx) {
    let at = frame.point(TOP);
    let up = frame.up();
    let floor = frame.point(UNDER).y;
    for _ in 0..impact.cream_blobs {
        let direction = pool.rng.cone(up, 1.2);
        let mut blob = Particle::new(
            Shape::Ball,
            Paint::Cream,
            at + direction * 0.25,
            impact.cream_life * pool.rng.range(0.7, 1.2),
            impact.cream_size * pool.rng.range(0.6, 1.4),
        );
        blob.curve = Curve::Pop;
        blob.facing = Facing::Tumble;
        blob.velocity = direction * impact.cream_speed * pool.rng.range(0.4, 1.0);
        blob.carrier = frame.velocity * 0.6;
        blob.gravity = 9.0;
        blob.drag = 0.8;
        blob.stretch = 0.6;
        blob.axis = pool.rng.direction();
        blob.spin = pool.rng.signed() * 4.0;
        blob.floor = floor;
        pool.spawn(blob);
    }
}

/// Dust off the road (GD § 10), out of a rear wheel: puffs of the ground's color, left behind.
pub fn dust(
    pool: &mut ParticlePool,
    frame: &KartFrame,
    side: f32,
    world: &WorldVfx,
    count: u32,
    dt: f32,
) {
    let at = frame.point(REAR_WHEEL * Vec3::new(side, 1.0, 1.0));
    let up = frame.up();
    for i in 0..count {
        let mut puff = Particle::new(
            Shape::Ball,
            Paint::Dust,
            along(at, frame.velocity * dt, i, count),
            world.dust_life * pool.rng.range(0.7, 1.2),
            world.dust_size * pool.rng.range(0.6, 1.2),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = up * pool.rng.range(0.6, 1.4)
            + frame.direction(Vec3::new(side * 0.8, 0.0, 0.6)) * pool.rng.range(0.5, 1.5);
        puff.carrier = frame.velocity * 0.15;
        puff.drag = 2.0;
        puff.axis = pool.rng.direction();
        puff.spin = pool.rng.signed() * 2.0;
        pool.spawn(puff);
    }
}

/// A landing: puffs of dust blown out flat all around the kart.
pub fn landing(pool: &mut ParticlePool, frame: &KartFrame, world: &WorldVfx) {
    let at = frame.point(UNDER);
    for k in 0..world.landing_puffs {
        let angle = (k as f32 + pool.rng.unit() * 0.5) / world.landing_puffs.max(1) as f32 * TAU;
        let out = frame.axis(Vec3::new(angle.cos(), 0.1, angle.sin()));
        let mut puff = Particle::new(
            Shape::Ball,
            Paint::Dust,
            at + out * 0.7,
            pool.rng.range(0.4, 0.6),
            world.dust_size * pool.rng.range(0.7, 1.0),
        );
        puff.curve = Curve::Puff;
        puff.facing = Facing::Tumble;
        puff.velocity = out * pool.rng.range(2.5, 4.0);
        puff.carrier = frame.velocity * 0.5;
        puff.drag = 4.0;
        pool.spawn(puff);
    }
}

/// The anti-gravity's sparks (GD § 10): small cyan streaks off the wheels pivoted flat.
pub fn anti_gravity_sparks(pool: &mut ParticlePool, frame: &KartFrame, count: u32, dt: f32) {
    for i in 0..count {
        let wheel = match (pool.spawned + u64::from(i)) % 4 {
            0 => REAR_WHEEL,
            1 => REAR_WHEEL * Vec3::new(-1.0, 1.0, 1.0),
            2 => FRONT_WHEEL,
            _ => FRONT_WHEEL * Vec3::new(-1.0, 1.0, 1.0),
        };
        let direction = pool
            .rng
            .cone(frame.direction(Vec3::new(0.0, 0.2, 1.0)), 0.5);
        let mut spark = Particle::new(
            Shape::Cube,
            Paint::Cyan,
            along(frame.point(wheel), frame.velocity * dt, i, count),
            pool.rng.range(0.12, 0.2),
            0.04,
        );
        spark.velocity = direction * pool.rng.range(2.0, 4.0);
        spark.carrier = frame.velocity * 0.8;
        spark.drag = 2.0;
        spark.stretch = 5.0;
        pool.spawn(spark);
    }
}

/// The glider's wind (GD § 10): white streaks off its wing tips, left behind.
pub fn glide_wind(pool: &mut ParticlePool, frame: &KartFrame, count: u32, dt: f32) {
    let forward = frame.forward();
    for i in 0..count {
        let side = if (pool.spawned + u64::from(i)).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        };
        let mut streak = Particle::new(
            Shape::Cube,
            Paint::Wind,
            along(
                frame.point(WING_TIP * Vec3::new(side, 1.0, 1.0)),
                frame.velocity * dt,
                i,
                count,
            ),
            pool.rng.range(0.25, 0.4),
            0.03,
        );
        streak.curve = Curve::Flash;
        streak.carrier = frame.velocity * 0.7;
        streak.axis = forward;
        streak.stretch = 25.0;
        pool.spawn(streak);
    }
}

/// Speed lines (GD § 14) around the view of the camera at `eye`, looking along `forward`:
/// streaks ahead of it, around its axis (none below it: the road), rushing past it; `carrier`:
/// the kart's velocity (they keep up with the camera).
pub fn speed_lines(
    pool: &mut ParticlePool,
    eye: Vec3,
    forward: Vec3,
    carrier: Vec3,
    boost: &BoostVfx,
    count: u32,
) {
    let forward = forward.normalize_or(Vec3::NEG_Z);
    let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(forward);
    for _ in 0..count {
        // From the right, over the top, to the left: the bottom quarter is the road.
        let angle = pool.rng.range(-0.3 * PI, 1.3 * PI);
        let radius = pool
            .rng
            .range(boost.speed_line_radius[0], boost.speed_line_radius[1]);
        let ahead = pool
            .rng
            .range(boost.speed_line_ahead[0], boost.speed_line_ahead[1]);
        let width = boost.speed_line_width.max(1e-3);
        let mut line = Particle::new(
            Shape::Cube,
            Paint::Wind,
            eye + forward * ahead + (right * angle.cos() + up * angle.sin()) * radius,
            boost.speed_line_life * pool.rng.range(0.8, 1.2),
            width,
        );
        line.curve = Curve::Flash;
        line.velocity = -forward * boost.speed_line_speed;
        line.carrier = carrier;
        line.stretch = boost.speed_line_length / width;
        pool.spawn(line);
    }
}

/// The cap and the tuning's switch: the pool follows `VfxTuning::max_particles` (none when the
/// particles are off).
pub fn follow_the_cap(pool: &mut ParticlePool, tuning: &VfxTuning) {
    pool.set_cap(if tuning.enabled {
        tuning.max_particles
    } else {
        0
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::MiniTurbo;
    use crate::vfx::cues::spark_style;

    fn frame() -> KartFrame {
        KartFrame {
            velocity: Vec3::new(0.0, 0.0, -20.0),
            ..default()
        }
    }

    /// The emitters' counts: the rate over the frame, nothing lost to the rounding, at most the
    /// frame's maximum, nothing stored up by a long frame.
    #[test]
    fn an_emitter_throws_its_rate_whatever_the_frame_rate() {
        for fps in [30.0f32, 60.0, 144.0] {
            let mut carry = 0.0;
            let thrown: u32 = (0..(fps as usize * 4))
                .map(|_| take(&mut carry, 50.0, 1.0 / fps, 100))
                .sum();
            assert!((199..=201).contains(&thrown), "{fps} fps: {thrown}");
        }
        let mut carry = 0.0;
        assert_eq!(take(&mut carry, 1000.0, 0.5, 6), 6);
        assert!(carry <= 1.0);
        assert_eq!(take(&mut carry, 0.0, 0.1, 6), 0);
        assert_eq!(carry, 0.0);
    }

    /// The sparks leave the rear wheel of their side, behind the kart, in their tier's color, and
    /// are left behind by the kart: their velocity is less than the kart's.
    #[test]
    fn a_wheel_sprays_its_sparks_behind_and_out() {
        let drift = DriftVfx::default();
        let mut pool = ParticlePool::new(500);
        let style = spark_style(&drift, MiniTurbo::Orange, 0.5);
        drift_sparks(&mut pool, &frame(), 1.0, &style, &drift, 40, 1.0 / 60.0);
        assert_eq!(pool.live(), 40);
        for spark in pool.alive() {
            assert_eq!(spark.paint, Paint::Orange);
            assert!(spark.position.x > 0.3, "right wheel: {}", spark.position);
            assert!(spark.position.z > 0.5, "behind: {}", spark.position);
            assert!(spark.velocity.z > 0.0, "out to the back");
            assert!((spark.velocity + spark.carrier).z > -20.0, "left behind");
        }
    }

    /// Every burst stays within its counts (and the cap).
    #[test]
    fn the_bursts_throw_what_their_tuning_says() {
        let tuning = VfxTuning::default();
        let mut pool = ParticlePool::new(ParticlePool::SLOTS);
        let f = frame();
        tier_flash(&mut pool, &f, 2, &tuning.drift);
        assert_eq!(
            pool.live() as u32,
            2 * (tuning.drift.flash_sparks[2] + 1) + 1
        );
        pool.clear();
        release(&mut pool, &f, 0, &tuning.drift);
        assert_eq!(pool.live() as u32, tuning.drift.release_sparks[0] + 2);
        pool.clear();
        confetti(&mut pool, Vec3::ZERO, 4.0, &tuning.impact);
        assert_eq!(
            pool.live() as u32,
            tuning.impact.confetti + tuning.impact.blast_puffs
        );
        assert!(pool.alive().all(|p| p.floor == -0.45));
        pool.clear();
        shock(
            &mut pool,
            Vec3::ZERO,
            Vec3::X,
            Vec3::ZERO,
            5,
            3,
            &tuning.impact,
        );
        assert_eq!(pool.alive().filter(|p| p.shape == Shape::Star).count(), 5);
        assert_eq!(pool.live(), 8);
        let mut small = ParticlePool::new(10);
        rocket_smoke(&mut small, &f, &tuning.boost);
        assert_eq!(small.live(), 10, "never more than the cap");
    }
}
