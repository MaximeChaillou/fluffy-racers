//! Issue #105: the particles' tuning (admin panel, « à régler »): how many particles at most, how
//! dense, big, fast and long-lived each effect is, and their colors. Purely visual: nothing here
//! touches the race.

use bevy::prelude::*;

use super::pool::Paint;

/// Every knob of the visual effects (GD § 14), with its default. Sizes and distances in m,
/// speeds in m/s, rates in particles per second, lives and times in s, colors in sRGB (0 to 1).
#[derive(Resource, Reflect, Clone, Debug, PartialEq)]
#[reflect(Resource, Default)]
pub struct VfxTuning {
    /// The particles are on (off: none at all, the karts' own flames and glows stay).
    pub enabled: bool,
    /// **The global cap**: at most this many particles alive at once (at most 1 536, the pool's
    /// slots). Full, a new particle takes the oldest one's slot.
    pub max_particles: usize,
    /// A kart farther than this from the camera throws nothing (m): what is too far to be seen
    /// costs nothing.
    pub emit_range: f32,
    /// An emitter throws at most this many particles in a frame (a long frame does not throw a
    /// wave of them).
    pub max_per_frame: u32,
    /// Issue #174: a kart farther than this from the camera (m) throws only `lod_share` of its
    /// drift's sparks, flames, embers and smoke (what is small on screen costs less; its flashes
    /// and its halo stay whole).
    pub lod_range: f32,
    pub lod_share: f32,
    pub drift: DriftVfx,
    pub boost: BoostVfx,
    pub impact: ImpactVfx,
    pub world: WorldVfx,
    /// Issue #176: the wheels that turn, the tyres that spin and smoke at a standing start.
    pub wheels: WheelVfx,
    /// Issue #185: how every smoke, dust and foam looks and moves (soft sprites: `smoke.rs`,
    /// `batch.rs`).
    pub smoke: SmokeVfx,
    pub colors: VfxColors,
}

impl Default for VfxTuning {
    fn default() -> Self {
        Self {
            enabled: true,
            max_particles: 900,
            emit_range: 80.0,
            max_per_frame: 6,
            lod_range: 30.0,
            lod_share: 0.5,
            drift: DriftVfx::default(),
            boost: BoostVfx::default(),
            impact: ImpactVfx::default(),
            world: WorldVfx::default(),
            wheels: WheelVfx::default(),
            smoke: SmokeVfx::default(),
            colors: VfxColors::default(),
        }
    }
}

/// The drift (GD § 3): its sparks by tier, the flash of each tier, the smoke of the tyres, the
/// « fwoosh » of the release. Arrays by tier: blue, orange, purple.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct DriftVfx {
    /// Sparks thrown by each rear wheel per second, by tier: more and more of them...
    pub spark_rate: [f32; 3],
    /// ...and within a tier, the rate starts this much lower (0.35: at 65 %) and grows as the
    /// charge nears the next tier.
    pub spark_ramp: f32,
    /// ...bigger (their width)...
    pub spark_size: [f32; 3],
    /// ...longer (their streak)...
    pub spark_length: [f32; 3],
    /// ...faster (out of the wheel)...
    pub spark_speed: [f32; 3],
    /// ...and living longer.
    pub spark_life: [f32; 3],
    /// The sparks spray in a cone this wide (half-angle, rad), back and out of the wheel, and
    /// fall with this gravity.
    pub spark_spread: f32,
    pub spark_gravity: f32,
    /// Issue #174: how high the sparks are thrown, by tier (the up of their spray, against 1
    /// backward): the purple ones leap high.
    pub spark_rise: [f32; 3],
    /// Issue #174: how bright they glow, by tier (above 1, their core burns white: the color is
    /// at its edge).
    pub spark_glow: [f32; 3],
    /// Issue #174: a spark's streak is longer the faster it flies: this many seconds of its
    /// flight (m per m/s), on top of its length.
    pub spark_smear: f32,
    /// Issue #174: they bounce on the road (this much of their fall back up, the rest of their
    /// speed rubbed off), then lie there and die out.
    pub spark_bounce: f32,
    /// Issue #174: they cool as they die (white, then their tier's color, then its deep color:
    /// `VfxColors::deep`), this much of the way by their end.
    pub spark_cool: f32,
    /// Issue #174, **the flames licking the rear wheels**: tongues per wheel per second by tier
    /// (blue: none, its sparks only), their height (m) at the start of the tier (`flame_grow`
    /// times more as the charge nears the next tier), how long each one licks (s), how fast it
    /// rises (m/s), how bright it is.
    pub flame_rate: [f32; 3],
    pub flame_size: [f32; 3],
    pub flame_grow: f32,
    pub flame_life: f32,
    pub flame_rise: f32,
    pub flame_glow: [f32; 3],
    /// Issue #174, **the embers** floating up from the wheels (purple: real fire): per wheel per
    /// second by tier, their size, their life, how fast they float up.
    pub ember_rate: [f32; 3],
    pub ember_size: f32,
    pub ember_life: f32,
    pub ember_rise: f32,
    /// Issue #174, **the halo** on the ground under the kart (a false light: a glowing disc, the
    /// one sun stays the only light), by tier: how wide (m), how bright; it pulses this much
    /// (share of its size), this fast (rad/s). After the release, it stays while the mini-turbo
    /// lasts, fading with it.
    pub halo_size: [f32; 3],
    pub halo_glow: [f32; 3],
    pub halo_pulse: f32,
    pub halo_pulse_speed: f32,
    /// A tier reached: a burst of this many sparks of its color at each rear wheel...
    pub flash_sparks: [u32; 3],
    /// ...a white flash this big at each wheel, and a ring of its color this wide around the
    /// back of the kart, for this long.
    pub flash_size: [f32; 3],
    pub flash_ring: [f32; 3],
    pub flash_time: f32,
    /// Issue #174: ...and the kart gives a little jump (shown only: its visual), this high (m)
    /// for `hop_time` s.
    pub hop_height: [f32; 3],
    pub hop_time: f32,
    /// The tyres' smoke while drifting: puffs per rear wheel per second, their size, their life,
    /// how fast they rise.
    pub smoke_rate: f32,
    pub smoke_size: f32,
    pub smoke_life: f32,
    pub smoke_rise: f32,
    /// Issue #174: the smoke takes this much of the color of the ground under the wheels (the
    /// road's, its sand's, the shoulders' off the road: issue #130), and is this opaque at first
    /// (it thins out as it spreads).
    pub smoke_tint: f32,
    pub smoke_opacity: f32,
    /// The release (« fwoosh »): a burst of this many sparks out of the exhaust, by tier...
    pub release_sparks: [u32; 3],
    pub release_speed: f32,
    /// Issue #174: ...a blast of this many tongues of flame out of the exhausts, in its color...
    pub release_flames: [u32; 3],
    /// ...a ring this wide behind the kart...
    pub release_ring: [f32; 3],
    /// ...and while its mini-turbo lasts, a trail of its color out of the exhaust (per second;
    /// issue #174: a glowing streak, this long-lived, this wide).
    pub trail_rate: f32,
    pub trail_life: f32,
    pub trail_size: f32,
}

impl Default for DriftVfx {
    fn default() -> Self {
        Self {
            spark_rate: [26.0, 42.0, 66.0],
            spark_ramp: 0.35,
            spark_size: [0.06, 0.075, 0.09],
            spark_length: [0.28, 0.38, 0.5],
            spark_speed: [3.2, 4.4, 5.8],
            spark_life: [0.22, 0.26, 0.3],
            spark_spread: 0.55,
            spark_gravity: 9.0,
            spark_rise: [0.45, 0.6, 1.05],
            spark_glow: [2.6, 3.0, 3.4],
            spark_smear: 0.03,
            spark_bounce: 0.4,
            spark_cool: 0.85,
            flame_rate: [0.0, 34.0, 56.0],
            flame_size: [0.0, 0.18, 0.26],
            flame_grow: 1.5,
            flame_life: 0.17,
            flame_rise: 1.8,
            flame_glow: [1.6, 1.9, 2.2],
            ember_rate: [0.0, 0.0, 12.0],
            ember_size: 0.07,
            ember_life: 0.8,
            ember_rise: 1.3,
            halo_size: [2.2, 2.8, 3.6],
            halo_glow: [0.6, 0.9, 1.25],
            halo_pulse: 0.1,
            halo_pulse_speed: 15.0,
            flash_sparks: [12, 18, 26],
            flash_size: [0.45, 0.6, 0.75],
            flash_ring: [1.2, 1.35, 1.5],
            flash_time: 0.22,
            hop_height: [0.05, 0.07, 0.1],
            hop_time: 0.2,
            smoke_rate: 10.0,
            smoke_size: 0.55,
            smoke_life: 0.55,
            smoke_rise: 1.2,
            smoke_tint: 0.35,
            smoke_opacity: 0.6,
            release_sparks: [12, 20, 30],
            release_speed: 8.0,
            release_flames: [8, 14, 22],
            release_ring: [0.55, 0.7, 0.85],
            trail_rate: 120.0,
            trail_life: 0.3,
            trail_size: 0.13,
        }
    }
}

/// The boosts (GD § 2: one system, each source its look): the embers of the flames, the speed
/// lines and the field of view, and what each source adds.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct BoostVfx {
    /// Embers out of each exhaust per second at full flames (times the flames' size,
    /// `kart::flame_size`: they follow the boost's strength and its time left)...
    pub ember_rate: f32,
    pub ember_size: f32,
    pub ember_life: f32,
    pub ember_speed: f32,
    pub ember_spread: f32,
    /// Issue #174: how bright the exhaust's flames burn (their color times this: their core
    /// white).
    pub flame_glow: f32,
    /// **Speed lines** around the camera's kart while it boosts: per second at a boost of 0.3
    /// (+30 %), their length, their width, their life, how fast they rush past...
    pub speed_line_rate: f32,
    pub speed_line_length: f32,
    pub speed_line_width: f32,
    pub speed_line_life: f32,
    pub speed_line_speed: f32,
    /// ...how far around the view's axis (m, from-to), and ahead of the camera (m, from-to).
    pub speed_line_radius: [f32; 2],
    pub speed_line_ahead: [f32; 2],
    /// **The field of view**: this many more degrees while the camera's kart boosts (on top of
    /// `CameraTuning::boost_fov`, times the boost's strength over 0.3, from 0.6 to 1.5)...
    pub fov_extra: f32,
    /// ...and a punch of this many degrees when a boost starts, gone in `fov_punch_time`; a
    /// mini-turbo's by tier (times this), a croquette's none.
    pub fov_punch: f32,
    pub fov_punch_time: f32,
    pub fov_tier: [f32; 3],
    /// How fast the extra field of view follows (1/s).
    pub fov_response: f32,
    /// The rocket start: a cloud of this many puffs of smoke, this big, this long (issue #185:
    /// this opaque at most).
    pub rocket_smoke: u32,
    pub rocket_smoke_size: f32,
    pub rocket_smoke_life: f32,
    pub rocket_smoke_opacity: f32,
    /// A trick: a burst of gold sparkles when it starts, and when its boost starts on landing.
    pub trick_sparkles: u32,
    /// The slipstream's boost: a burst of wind streaks around the kart.
    pub slipstream_streaks: u32,
    /// The anti-gravity's spin boost: a burst of cyan sparks.
    pub spin_sparks: u32,
    /// A boost pad, a piment: a burst of sparks of its color out of the exhaust.
    pub pad_sparks: u32,
    pub piment_sparks: u32,
    /// The stalled engine (GD § 4): puffs of soot when it chokes...
    pub stall_puffs: u32,
    /// ...issue #185: and while it stays stalled, puffs of soot out of each exhaust per second
    /// (instead of the grey cubes of `kart::visual`), this opaque.
    pub stall_rate: f32,
    pub stall_opacity: f32,
}

impl Default for BoostVfx {
    fn default() -> Self {
        Self {
            ember_rate: 36.0,
            ember_size: 0.13,
            ember_life: 0.22,
            ember_speed: 3.0,
            ember_spread: 0.35,
            flame_glow: 2.4,
            speed_line_rate: 55.0,
            speed_line_length: 2.2,
            speed_line_width: 0.035,
            speed_line_life: 0.22,
            speed_line_speed: 30.0,
            speed_line_radius: [2.0, 4.2],
            speed_line_ahead: [4.0, 11.0],
            fov_extra: 4.0,
            fov_punch: 5.0,
            fov_punch_time: 0.35,
            fov_tier: [0.6, 0.8, 1.0],
            fov_response: 6.0,
            rocket_smoke: 18,
            rocket_smoke_size: 0.8,
            rocket_smoke_life: 0.9,
            rocket_smoke_opacity: 0.28,
            trick_sparkles: 12,
            slipstream_streaks: 14,
            spin_sparks: 18,
            pad_sparks: 10,
            piment_sparks: 14,
            stall_puffs: 6,
            stall_rate: 7.0,
            stall_opacity: 0.45,
        }
    }
}

/// The shocks and the items (GD § 8 and 14): stars and « pof » of a bump or a wall, confetti of
/// a blast, cream of a chantilly, the stars of a kart that is seeing them, the screen's shake.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct ImpactVfx {
    /// A bump between karts: one star per this many m/s of closing speed (from 3 to 8 stars)...
    pub bump_speed_per_star: f32,
    /// ...each this big, this fast, this long, falling with this gravity...
    pub star_size: f32,
    pub star_speed: f32,
    pub star_life: f32,
    pub star_gravity: f32,
    /// ...and this many white puffs (the « pof »), this big, this long (issue #185: this opaque).
    pub pof_puffs: u32,
    pub pof_size: f32,
    pub pof_life: f32,
    pub pof_opacity: f32,
    /// A wall: from this loss of speed in a tick (m/s), across the kart's up; this many stars and
    /// puffs.
    pub wall_speed: f32,
    pub wall_stars: u32,
    pub wall_puffs: u32,
    /// A paw strike landed: this many stars on the kart struck (its « pof » is the paw's).
    pub paw_stars: u32,
    /// An explosion: this many confetti (on top of the item's own), this fast, this long, this
    /// big, and puffs of smoke.
    pub confetti: u32,
    pub confetti_speed: f32,
    pub confetti_life: f32,
    pub confetti_size: f32,
    /// Issue #185: the explosion's cloud, this opaque; its flash (instead of the pale sphere of
    /// `items::visual`): a glowing fireball this bright, for this long (s), and a ring blown out
    /// on the ground to its reach.
    pub blast_puffs: u32,
    pub blast_opacity: f32,
    pub blast_glow: f32,
    pub blast_flash_time: f32,
    /// Issue #106: a gift box taken bursts into this many confetti (those of an explosion, a
    /// little shorter-lived), thrown out this fast, and carried along by the kart that took it
    /// (this share of its speed).
    pub gift_confetti: u32,
    pub gift_speed: f32,
    pub gift_carry: f32,
    /// A chantilly on a kart: this many blobs of cream splash off it, this big, this fast, this
    /// long.
    pub cream_blobs: u32,
    pub cream_size: f32,
    pub cream_speed: f32,
    pub cream_life: f32,
    /// **Seeing stars** around the head (« sonné »): this many, turning this fast (rad/s), this
    /// far around and this high in the driver's frame (issue #171: a ring around its head, which
    /// follows it: the seat, the size, the spin out and the tumble, the head going round), this
    /// big. During a spin out or a tumble and this long after it; after a paw strike or a bump
    /// from `dizzy_bump_speed`, this long.
    pub dizzy_stars: u32,
    pub dizzy_spin: f32,
    pub dizzy_radius: f32,
    pub dizzy_height: f32,
    pub dizzy_size: f32,
    pub dizzy_after_hit: f32,
    pub dizzy_after_shock: f32,
    pub dizzy_bump_speed: f32,
    /// Issue #171: the ring leans this much (rad, its front down), and each star twinkles (its
    /// size pulses by `dizzy_twinkle` of itself, this fast: rad/s).
    pub dizzy_tilt: f32,
    pub dizzy_twinkle: f32,
    pub dizzy_twinkle_speed: f32,
    /// Issue #171: only the moment of a bump counts: a kart that touched another one this
    /// recently (s, any bump) is still in the same contact, and its stars do not start again
    /// (two karts rubbing side by side bumped at every tick, and kept their stars for good).
    pub dizzy_contact_gap: f32,
    /// The player's camera shakes (m, then for `shake_time`): a bump (at 15 m/s of closing
    /// speed, more or less with it), a wall, an item hit.
    pub shake_bump: f32,
    pub shake_wall: f32,
    pub shake_hit: f32,
    pub shake_time: f32,
}

impl Default for ImpactVfx {
    fn default() -> Self {
        Self {
            bump_speed_per_star: 2.5,
            star_size: 0.3,
            star_speed: 4.5,
            star_life: 0.6,
            star_gravity: 7.0,
            pof_puffs: 6,
            pof_size: 0.5,
            pof_life: 0.35,
            pof_opacity: 0.6,
            wall_speed: 6.0,
            wall_stars: 4,
            wall_puffs: 4,
            paw_stars: 4,
            confetti: 40,
            confetti_speed: 9.0,
            confetti_life: 1.6,
            confetti_size: 0.16,
            blast_puffs: 10,
            blast_opacity: 0.7,
            blast_glow: 1.6,
            blast_flash_time: 0.3,
            gift_confetti: 28,
            gift_speed: 6.0,
            gift_carry: 0.5,
            cream_blobs: 16,
            cream_size: 0.22,
            cream_speed: 5.0,
            cream_life: 0.9,
            dizzy_stars: 5,
            dizzy_spin: 7.5,
            dizzy_radius: 0.62,
            dizzy_height: 0.72,
            dizzy_size: 0.2,
            dizzy_after_hit: 1.2,
            dizzy_after_shock: 0.9,
            dizzy_bump_speed: 12.0,
            dizzy_tilt: 0.3,
            dizzy_twinkle: 0.35,
            dizzy_twinkle_speed: 14.0,
            dizzy_contact_gap: 0.3,
            shake_bump: 0.12,
            shake_wall: 0.1,
            shake_hit: 0.18,
            shake_time: 0.3,
        }
    }
}

/// The croquettes (GD § 7) and the ground (GD § 10): sparkles of a croquette picked up or lost,
/// dust off the road and on a landing, the anti-gravity's sparks, the glider's wind.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct WorldVfx {
    /// A croquette picked up: this many gold sparkles; lost: this many per croquette lost.
    pub croquette_sparkles: u32,
    pub lost_sparkles: u32,
    pub sparkle_size: f32,
    pub sparkle_life: f32,
    /// Off the road: dust puffs per rear wheel per second at top speed (times the share of the
    /// wheels off the road), from this speed (m/s); their size and life.
    pub dust_rate: f32,
    pub dust_min_speed: f32,
    pub dust_size: f32,
    pub dust_life: f32,
    /// Issue #185: the dust is this opaque at most (off the road, on a landing, out of the tyres).
    pub dust_opacity: f32,
    /// A landing after this long in the air (s): this many puffs of dust around the kart.
    pub landing_air_time: f32,
    pub landing_puffs: u32,
    /// In anti-gravity: cyan sparks per wheel per second at top speed.
    pub anti_gravity_rate: f32,
    /// Gliding: wind streaks off each wing tip per second.
    pub glide_wind_rate: f32,
    /// Issue #130: the dust off the road is the color of the ground it comes from (its biome's
    /// shoulders, or its sand on the road), this much lighter (0: the ground's own color, 1:
    /// white). Off a circuit (the test ground), `VfxColors::dust`.
    pub ground_lighten: f32,
    /// Issue #130, the water (GD § 10): going in or out, a splash on the surface: this many drops
    /// thrown up and out this fast (m/s; coming out, a little less), this big, this long...
    pub splash_drops: u32,
    pub splash_speed: f32,
    pub splash_size: f32,
    pub splash_life: f32,
    /// ...a ring of foam on the water this wide (m), and this many puffs of foam.
    pub splash_ring: f32,
    pub splash_foam: u32,
    /// The wake: puffs of foam left on the water per second at top speed, out of the back of a
    /// kart at the surface (its center from `wake_above` m above it to `wake_below` m under it),
    /// from this speed (m/s); their size, their life.
    pub wake_rate: f32,
    pub wake_above: f32,
    pub wake_below: f32,
    pub wake_min_speed: f32,
    pub wake_size: f32,
    pub wake_life: f32,
    /// Issue #185: the foam (a splash's, the wake) is this opaque at most.
    pub foam_opacity: f32,
}

impl Default for WorldVfx {
    fn default() -> Self {
        Self {
            croquette_sparkles: 8,
            lost_sparkles: 4,
            sparkle_size: 0.16,
            sparkle_life: 0.45,
            dust_rate: 16.0,
            dust_min_speed: 4.0,
            dust_size: 0.32,
            dust_life: 0.45,
            dust_opacity: 0.6,
            landing_air_time: 0.35,
            landing_puffs: 8,
            anti_gravity_rate: 10.0,
            glide_wind_rate: 18.0,
            ground_lighten: 0.2,
            splash_drops: 26,
            splash_speed: 5.5,
            splash_size: 0.13,
            splash_life: 0.75,
            splash_ring: 1.8,
            splash_foam: 8,
            wake_rate: 24.0,
            wake_above: 0.45,
            wake_below: 0.9,
            wake_min_speed: 3.0,
            wake_size: 0.42,
            wake_life: 0.9,
            foam_opacity: 0.75,
        }
    }
}

/// Issue #176 (GD § 4 and 14): the wheels on screen (they turn with the kart, steer, spin at a
/// standing start) and what a standing start throws: the smoke and the dust of the tyres spinning
/// on the spot, the kart's little shakes, the cloud at GO by the start's quality, the puff of a
/// kart setting off again from a full stop. Purely visual: the race knows nothing of it.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct WheelVfx {
    /// The wheels roll at the kart's speed (rad/s: m/s over their radius), shown at most this fast
    /// (rad/s): their band (`kart::parts::paint_band`) then never turns more than half a turn in
    /// a frame down to 15 frames per second, so it never seems to turn backward.
    pub max_spin: f32,
    /// **Spinning on the spot**: the accelerator down while the karts are held on the grid (before
    /// GO), the back wheels' tyres go this fast (m/s) on top of the kart's speed...
    pub grid_slip: f32,
    /// ...and setting off in the race (the accelerator down, slow), this much (m/s), less and less
    /// as the kart gets going, none from `slip_fade_speed` (m/s).
    pub launch_slip: f32,
    pub slip_fade_speed: f32,
    /// A rocket start's boost (GD § 4) spins them this much more (m/s), none from this speed
    /// (m/s).
    pub rocket_slip: f32,
    pub rocket_fade_speed: f32,
    /// How fast the spin follows (1/s).
    pub slip_response: f32,
    /// **Steering**: the front wheels turn this far at full lock (rad), this fast (1/s); a kart
    /// without its driver's stick here (another player's, online) shows its turn: full lock at
    /// this yaw rate (rad/s).
    pub steer_angle: f32,
    pub steer_response: f32,
    pub steer_yaw_rate: f32,
    /// **The tyres' smoke** while they spin: puffs per back wheel per second at `grid_slip`
    /// (in proportion to the spin), their size, their life, how fast they rise and roll back; they
    /// take this much of the color of the ground (as the drift's: `DriftVfx::smoke_tint`) and are
    /// this opaque at first.
    pub smoke_rate: f32,
    pub smoke_size: f32,
    pub smoke_life: f32,
    pub smoke_rise: f32,
    pub smoke_push: f32,
    pub smoke_tint: f32,
    pub smoke_opacity: f32,
    /// ...and puffs thrown back off the tyres (per back wheel per second at `grid_slip`; their
    /// look is the dust's: `WorldVfx::dust_size`, `dust_life`): white smoke on the road, the dust
    /// of the ground off it.
    pub dust_rate: f32,
    /// **Little shakes** of the kart spinning on the spot (shown only: its visual), this much (m)
    /// at `grid_slip`, this fast (rad/s).
    pub shake: f32,
    pub shake_speed: f32,
    /// **At GO**, a cloud by the start's quality (GD § 4), out of the back wheels: this many puffs
    /// for a perfect rocket start (fewer for a later one: down to `go_rocket_least` of them for the
    /// shortest boost), for a normal start with the accelerator down, for a stall (it splutters);
    /// their size, their life, how fast they are blown back (m/s).
    pub go_rocket_puffs: u32,
    pub go_rocket_least: f32,
    pub go_normal_puffs: u32,
    pub go_stall_puffs: u32,
    pub go_size: f32,
    pub go_life: f32,
    pub go_push: f32,
    /// Issue #185: the cloud is this opaque at most (thin: it dresses the grid, it never hides
    /// it), its puffs this much wider than tall (flat on the road), and it rises this fast (m/s,
    /// from-to: it stays low).
    pub go_opacity: f32,
    pub go_width: f32,
    pub go_rise: [f32; 2],
    /// **Setting off again from a full stop** (after a tumble, a wall...): still (slower than
    /// `stop_speed`, m/s) for `stop_time` s, then off: a smaller puff, this many, `launch_size` big.
    pub stop_speed: f32,
    pub stop_time: f32,
    pub launch_puffs: u32,
    pub launch_size: f32,
}

impl Default for WheelVfx {
    fn default() -> Self {
        Self {
            max_spin: 45.0,
            grid_slip: 9.0,
            launch_slip: 4.0,
            slip_fade_speed: 9.0,
            rocket_slip: 14.0,
            rocket_fade_speed: 18.0,
            slip_response: 8.0,
            steer_angle: 0.42,
            steer_response: 14.0,
            steer_yaw_rate: 2.2,
            smoke_rate: 16.0,
            smoke_size: 0.55,
            smoke_life: 0.65,
            smoke_rise: 0.9,
            smoke_push: 2.2,
            smoke_tint: 0.2,
            smoke_opacity: 0.5,
            dust_rate: 6.0,
            shake: 0.022,
            shake_speed: 48.0,
            go_rocket_puffs: 26,
            go_rocket_least: 0.4,
            go_normal_puffs: 12,
            go_stall_puffs: 4,
            go_size: 1.1,
            go_life: 0.95,
            go_push: 4.5,
            go_opacity: 0.38,
            go_width: 1.6,
            go_rise: [0.15, 0.6],
            stop_speed: 1.0,
            stop_time: 0.5,
            launch_puffs: 6,
            launch_size: 0.5,
        }
    }
}

/// Issue #185 (GD § 14): **the smoke**, every puff of smoke, dust or foam of the game: a soft
/// sprite facing the camera (`smoke.rs`: a drawing of fractal noise, four of them), which grows,
/// turns slowly, rises or spreads with the wind of the kart that threw it, then thins out; lit
/// simply (lighter on the sun's side, darker below, its shade taking the biome's sky and its
/// ground); never in front of the camera's eye, its edge fading where it touches the ground.
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct SmokeVfx {
    /// A puff is never more opaque than this (each effect's own opacity under it): smoke is seen
    /// through.
    pub max_opacity: f32,
    /// Its opacity comes in over this share of its life (fast)...
    pub attack: f32,
    /// ...then goes slowly, as (1 − its age's share) to this power.
    pub fade: f32,
    /// It turns at most this fast (rad/s, either way), and rises this much (m/s², against
    /// gravity: hot air).
    pub spin: f32,
    pub rise: f32,
    /// It spreads out flat as it ages: this much wider than tall by its end (on top of its
    /// shape).
    pub spread: f32,
    /// Not drawn nearer the camera than the first distance (m), whole from the second: a puff
    /// thrown at the camera never fills the screen.
    pub near_fade: [f32; 2],
    /// Where it touches the ground, its corners fade out over this share of its size above it
    /// (no hard line where the ground cuts it).
    pub ground_fade: f32,
    /// Its side toward the sun is this color (sRGB, a multiplier of its own)...
    pub lit: [f32; 3],
    /// ...its side away from it and below this dark (0 to 1)...
    pub shade: f32,
    /// ...that shade taking this much of the biome's sky's hue, and this much of its ground's
    /// (the light it bounces).
    pub sky_tint: f32,
    pub ground_tint: f32,
}

impl Default for SmokeVfx {
    fn default() -> Self {
        Self {
            max_opacity: 0.8,
            attack: 0.08,
            fade: 1.2,
            spin: 0.7,
            rise: 0.9,
            spread: 0.35,
            near_fade: [1.2, 3.5],
            ground_fade: 0.45,
            lit: [1.0, 0.98, 0.94],
            shade: 0.8,
            sky_tint: 0.25,
            ground_tint: 0.12,
        }
    }
}

/// The colors of the particles (sRGB), one per material (`pool::Paint`).
#[derive(Reflect, Clone, Debug, PartialEq)]
pub struct VfxColors {
    /// The drift's tiers (GD § 3.4): blue, orange, purple.
    pub tiers: [[f32; 3]; 3],
    /// Issue #174: their deep colors: what their sparks cool to, the tips of their flames, their
    /// embers (blue to indigo, orange to red, purple to pink).
    pub deep: [[f32; 3]; 3],
    pub hot: [f32; 3],
    pub flame: [f32; 3],
    pub piment: [f32; 3],
    pub gold: [f32; 3],
    pub cyan: [f32; 3],
    pub wind: [f32; 3],
    pub star: [f32; 3],
    pub confetti: [[f32; 3]; 5],
    pub smoke: [f32; 3],
    pub dust: [f32; 3],
    pub soot: [f32; 3],
    pub cream: [f32; 3],
    /// Issue #130: the drops of a splash, the foam on the water.
    pub water: [f32; 3],
    pub foam: [f32; 3],
}

impl Default for VfxColors {
    fn default() -> Self {
        Self {
            tiers: [[0.3, 0.68, 1.0], [1.0, 0.58, 0.12], [0.8, 0.36, 1.0]],
            deep: [[0.22, 0.25, 1.0], [1.0, 0.18, 0.04], [1.0, 0.25, 0.7]],
            hot: [1.0, 0.97, 0.85],
            flame: [1.0, 0.65, 0.15],
            piment: [1.0, 0.22, 0.12],
            gold: [1.0, 0.82, 0.25],
            cyan: [0.2, 0.85, 1.0],
            wind: [0.95, 0.97, 1.0],
            star: [1.0, 0.92, 0.3],
            // The items' confetti (`items::visual`).
            confetti: [
                [1.0, 0.25, 0.45],
                [1.0, 0.85, 0.15],
                [0.25, 0.8, 1.0],
                [0.45, 0.95, 0.35],
                [0.75, 0.4, 1.0],
            ],
            smoke: [0.88, 0.88, 0.9],
            dust: [0.78, 0.66, 0.48],
            soot: [0.52, 0.5, 0.54],
            cream: [1.0, 0.97, 0.9],
            water: [0.55, 0.85, 1.0],
            foam: [0.94, 0.98, 1.0],
        }
    }
}

impl VfxColors {
    /// The color of `paint`.
    pub fn of(&self, paint: Paint) -> [f32; 3] {
        match paint {
            Paint::Blue => self.tiers[0],
            Paint::Orange => self.tiers[1],
            Paint::Purple => self.tiers[2],
            Paint::Hot => self.hot,
            Paint::Flame => self.flame,
            Paint::Piment => self.piment,
            Paint::Gold => self.gold,
            Paint::Cyan => self.cyan,
            Paint::Wind => self.wind,
            Paint::Star => self.star,
            Paint::Confetti0 => self.confetti[0],
            Paint::Confetti1 => self.confetti[1],
            Paint::Confetti2 => self.confetti[2],
            Paint::Confetti3 => self.confetti[3],
            Paint::Confetti4 => self.confetti[4],
            Paint::Smoke => self.smoke,
            Paint::Dust => self.dust,
            Paint::Soot => self.soot,
            Paint::Cream => self.cream,
            // Issue #130: the biome's ground on a circuit (`VfxAssets`), the dust's elsewhere.
            Paint::Ground | Paint::Sand | Paint::Road => self.dust,
            Paint::Water => self.water,
            Paint::Foam => self.foam,
        }
    }
}

/// Issue #130: the colors of the dust off the road on a circuit of `palette`'s biome: its
/// shoulders' (`Paint::Ground`) and its sand's (`Paint::Sand`), `lighten` of the way to white.
pub fn ground_dust(palette: &crate::track::biome::Palette, lighten: f32) -> [[f32; 3]; 2] {
    let t = if lighten.is_finite() {
        lighten.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let light = |(r, g, b): (f32, f32, f32)| [r, g, b].map(|c| c + (1.0 - c) * t);
    [light(palette.shoulder), light(palette.sand)]
}

/// Issue #174: the color of the road's dust on a circuit of `palette`'s biome (its asphalt,
/// `lighten` of the way to white, as `ground_dust`): the tint of the tyres' smoke on it.
pub fn road_dust(palette: &crate::track::biome::Palette, lighten: f32) -> [f32; 3] {
    let t = if lighten.is_finite() {
        lighten.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (r, g, b) = palette.asphalt;
    [r, g, b].map(|c| c + (1.0 - c) * t)
}
