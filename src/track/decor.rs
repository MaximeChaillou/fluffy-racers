//! Where the decor of a circuit stands (issue #11): its biome's rows and scattered pieces, and
//! the props of its file. Pure and deterministic (the circuit's `decor_seed`): the same pieces at
//! the same places on every load, on every peer. No piece stands on the road, the curbs, the
//! shoulders or the walls of any part of the circuit, nor on another piece, nor on a post of the
//! start line's arch (issue #79). The models are only
//! drawn by `crate::models` (`dressing.rs`): this file knows nothing of them but their names.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::biome::{BiomeData, Side};
use super::data::PropData;
use super::layout::{CenterLine, TrackSample};
use super::mesh::{GROUND_HEIGHT, WALL_THICKNESS, wall_offset};
use super::shortcut::Shortcut;
use super::start_line::StartLine;
use super::water::Lake;
use crate::bots::BotRng;

/// Room kept between a piece's footprint and the walls' outer face, on top of what the file asks
/// (m): the walls follow the centerline's samples, 1 m apart, and a piece between two of them
/// must still be clear.
pub const CLEARANCE: f32 = 0.3;

/// Attempts at finding room for a scattered piece before leaving it out.
const ATTEMPTS: usize = 12;

/// Below this turning radius (m), a turn is tight (the hairpin, the banked turn, the oval's half
/// circles)...
pub const TIGHT_TURN: f32 = 40.0;
/// ...and no piece higher than this (m) stands inside it: the road out of the turn stays in
/// sight over the walls (#47: the chase camera looks over them, 2.4 m above the kart).
pub const SIGHTLINE_HEIGHT: f32 = 1.6;

/// A piece of decor: a model, standing on the ground at `position`, turned by `yaw` around the
/// vertical (0: the model's front, glTF's +Z, toward world +Z), `height` meters high and within
/// `radius` meters of `position` on the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorPiece {
    /// Index into [`DecorPlan::models`].
    pub model: u16,
    pub position: Vec3,
    pub yaw: f32,
    pub height: f32,
    pub radius: f32,
    pub shadows: bool,
}

/// Every piece of a circuit, and the models it may use (each once, [`decor_models`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecorPlan {
    pub models: Vec<String>,
    pub pieces: Vec<DecorPiece>,
    /// The props of the file left out for lack of room (their model and where), for the console.
    pub left_out: Vec<String>,
}

impl DecorPlan {
    fn model(&mut self, id: &str) -> u16 {
        match self.models.iter().position(|m| m == id) {
            Some(i) => i as u16,
            None => {
                self.models.push(id.to_string());
                (self.models.len() - 1) as u16
            }
        }
    }

    /// The mirror image of the plan (issue #19, the mirror mode): every piece across the X = 0
    /// plane (x → -x), turned the other way (its front, `(sin yaw, cos yaw)` on the ground,
    /// mirrored too). The models themselves are not mirrored.
    pub fn mirrored(mut self) -> Self {
        for piece in &mut self.pieces {
            piece.position.x = -piece.position.x;
            piece.yaw = -piece.yaw;
        }
        self
    }

    /// Whether a footprint at `p` (ground plane) overlaps a piece already placed.
    pub(super) fn overlaps(&self, p: Vec2, radius: f32) -> bool {
        self.pieces
            .iter()
            .any(|piece| piece.position.xz().distance(p) < piece.radius + radius)
    }
}

/// Every model `biome` and `props` may place, each once, in the order they are placed: what to
/// load for the circuit.
pub fn decor_models(biome: Option<&BiomeData>, props: &[PropData]) -> Vec<String> {
    let mut models: Vec<String> = Vec::new();
    let all = props
        .iter()
        .map(|p| p.model.as_str())
        .chain(biome.into_iter().flat_map(|b| b.models()));
    for id in all {
        if !models.iter().any(|m| m == id) {
            models.push(id.to_string());
        }
    }
    models
}

/// The sign of the loop's inside: +1 when the inside of the loop is on the driver's right (the
/// circuit turns right overall, like both circuits of the game), -1 on its left.
pub fn inside_sign(center: &CenterLine) -> f32 {
    let samples = center.samples();
    let n = samples.len();
    let flat = |s: &TrackSample| Vec2::new(s.forward.x, s.forward.z).normalize_or_zero();
    // Seen from above with x to the right and z down, a right turn turns counterclockwise.
    let turned: f32 = (0..n)
        .map(|i| flat(&samples[i]).angle_to(flat(&samples[(i + 1) % n])))
        .sum();
    if turned >= 0.0 { 1.0 } else { -1.0 }
}

/// The sides (signs: -1 left of the driver, +1 right) that `side` covers.
pub(super) fn sides(side: Side, inside: f32) -> &'static [f32] {
    const LEFT: &[f32] = &[-1.0];
    const RIGHT: &[f32] = &[1.0];
    const BOTH: &[f32] = &[-1.0, 1.0];
    let toward = |sign: f32| if sign > 0.0 { RIGHT } else { LEFT };
    match side {
        Side::Both => BOTH,
        Side::Left => LEFT,
        Side::Right => RIGHT,
        Side::Inside => toward(inside),
        Side::Outside => toward(-inside),
    }
}

/// Whether a footprint of `radius` at `p` (ground plane) is clear of every part of the circuit:
/// beyond the walls' outer face (plus [`CLEARANCE`]) of every sample, the loop passing close to
/// itself included.
pub fn clear_of_the_circuit(samples: &[TrackSample], p: Vec2, radius: f32) -> bool {
    samples.iter().all(|s| {
        let keep_out = wall_offset(s) + WALL_THICKNESS + CLEARANCE + radius;
        s.position.xz().distance_squared(p) > keep_out * keep_out
    })
}

/// A tight turn where a sample of the centerline is: the circle the centerline follows there (its
/// center on the ground plane, its radius), measured over 8 m of track, and the sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TightTurn {
    pub center: Vec2,
    pub radius: f32,
    pub at: Vec2,
}

/// The tight turns of a circuit, sample by sample.
pub fn tight_turns(center: &CenterLine) -> Vec<TightTurn> {
    const HALF_SPAN: usize = 4;
    let samples = center.samples();
    let n = samples.len();
    let step = center.length() / n as f32;
    let flat = |s: &TrackSample| Vec2::new(s.forward.x, s.forward.z).normalize_or_zero();
    (0..n)
        .filter_map(|i| {
            let a = flat(&samples[(i + n - HALF_SPAN) % n]);
            let b = flat(&samples[(i + HALF_SPAN) % n]);
            // Positive: a right turn (see `inside_sign`).
            let turn = a.angle_to(b);
            let radius = 2.0 * HALF_SPAN as f32 * step / turn.abs().max(1e-6);
            (radius < TIGHT_TURN).then(|| {
                let s = &samples[i];
                let right = s.forward.cross(Vec3::Y).normalize_or(Vec3::X).xz();
                TightTurn {
                    center: s.position.xz() + right * turn.signum() * radius,
                    radius,
                    at: s.position.xz(),
                }
            })
        })
        .collect()
}

/// Issue #191: where the chase camera looks through the tight turns: from every sample of the
/// road from [`SIGHT_REACH`] meters before a tight turn to its end (every [`SIGHT_KART_STEP`]
/// meters, the kart across the road, [`SIGHT_KARTS`]), the camera where the chase puts it (`terrain`'s
/// `CHASE_DISTANCE` behind the kart, on the ground plane), to the road ahead (every
/// [`SIGHT_STEP`] meters up to [`SIGHT_REACH`], across it, [`SIGHT_TARGETS`]): the reach and the
/// spread of the camera test (`track::scenario_tests`, 45 m). A piece higher than
/// [`SIGHTLINE_HEIGHT`] whose footprint one of them crosses would hide the road out of the turn;
/// elsewhere in the turn's infield, even a giant one hides nothing (before #191, nothing higher
/// stood anywhere inside a tight turn: the giant needle, pine or tree in the middle of a bulge was
/// always left out).
///
/// Built once per circuit (at its load) as the cells of [`SIGHT_CELL`] meters they pass through
/// (every cell's length along each line): a footprint crosses a line where it reaches the middle
/// of one of these cells, a cell's diagonal more (a little more than the line itself).
#[derive(Clone, Debug, Default)]
pub struct TurnSightLines {
    /// The corner of the grid's first cell (ground plane), its columns and rows.
    origin: Vec2,
    cols: usize,
    rows: usize,
    /// The cells a line passes through, row by row.
    crossed: Vec<bool>,
    lines: usize,
}

/// Issue #191: how far ahead the chase camera keeps the road out of a tight turn in sight, and
/// how far before the turn it starts (m): the camera test's reach.
pub const SIGHT_REACH: f32 = 45.0;
/// ...from the kart every this many meters along the road...
pub const SIGHT_KART_STEP: f32 = 3.0;
/// ...to the road ahead every this many meters...
pub const SIGHT_STEP: f32 = 3.0;
/// ...the kart across the road (shares of its half width)...
pub const SIGHT_KARTS: [f32; 3] = [-0.6, 0.0, 0.6];
/// ...and the road ahead across it (shares of its half width, its edges too).
pub const SIGHT_TARGETS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];
/// The cells of the grid (m).
const SIGHT_CELL: f32 = 1.0;

impl TurnSightLines {
    /// The sight lines through the tight turns of `center` (`turns`, [`tight_turns`]).
    pub fn new(center: &CenterLine, turns: &[TightTurn]) -> Self {
        let samples = center.samples();
        let n = samples.len();
        if n == 0 || turns.is_empty() {
            return Self::default();
        }
        let step = center.length() / n as f32;
        // The samples in a tight turn, then those up to the reach before one.
        let mut near = vec![false; n];
        let mut k = 0;
        // `tight_turns` lists them in the order of the samples.
        for (i, s) in samples.iter().enumerate() {
            if k < turns.len() && turns[k].at == s.position.xz() {
                k += 1;
                let before = (SIGHT_REACH / step).ceil() as usize;
                for j in 0..=before {
                    near[(i + n - j) % n] = true;
                }
            }
        }
        let flat = |s: &TrackSample| Vec2::new(s.forward.x, s.forward.z).normalize_or_zero();
        let every = (SIGHT_KART_STEP / step).round().max(1.0) as usize;
        let ahead = (SIGHT_REACH / SIGHT_STEP).round() as usize;
        let mut lines = Vec::new();
        for (i, sample) in samples.iter().enumerate().step_by(every) {
            if !near[i] || !center.on_the_road(sample.distance) {
                continue;
            }
            let eyes = SIGHT_KARTS.map(|kart| {
                sample.point(kart * sample.half_width, 0.0).xz()
                    - flat(sample) * super::terrain::CHASE_DISTANCE
            });
            for k in 1..=ahead {
                let distance = sample.distance + k as f32 * SIGHT_STEP;
                if !center.on_the_road(center.wrap(distance)) {
                    continue;
                }
                let s = center.at(distance);
                for across in SIGHT_TARGETS {
                    let target = s.point(across * s.half_width, 0.0).xz();
                    for eye in eyes {
                        lines.push([eye, target]);
                    }
                }
            }
        }
        if lines.is_empty() {
            return Self::default();
        }
        let (lo, hi) = lines.iter().flatten().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let origin = lo - Vec2::splat(SIGHT_CELL);
        let cols = ((hi.x - origin.x) / SIGHT_CELL).ceil() as usize + 2;
        let rows = ((hi.y - origin.y) / SIGHT_CELL).ceil() as usize + 2;
        let mut grid = Self {
            origin,
            cols,
            rows,
            crossed: vec![false; cols * rows],
            lines: lines.len(),
        };
        for [a, b] in &lines {
            let steps = (a.distance(*b) / SIGHT_CELL).ceil().max(1.0) as usize;
            for k in 0..=steps {
                if let Some(i) = grid.index(a.lerp(*b, k as f32 / steps as f32)) {
                    grid.crossed[i] = true;
                }
            }
        }
        grid
    }

    /// The cell of `p`, if it is in the grid.
    fn index(&self, p: Vec2) -> Option<usize> {
        let c = ((p - self.origin) / SIGHT_CELL).floor();
        (c.x >= 0.0 && c.y >= 0.0 && (c.x as usize) < self.cols && (c.y as usize) < self.rows)
            .then(|| c.y as usize * self.cols + c.x as usize)
    }

    /// How many sight lines there are.
    pub fn len(&self) -> usize {
        self.lines
    }

    /// Whether there is none (a circuit without a tight turn).
    pub fn is_empty(&self) -> bool {
        self.lines == 0
    }

    /// Whether a footprint of `radius` at `p` (ground plane) crosses a sight line: reaches the
    /// middle of a cell one passes through, a cell's diagonal more.
    pub fn crossed(&self, p: Vec2, radius: f32) -> bool {
        if self.crossed.is_empty() {
            return false;
        }
        let reach = radius + SIGHT_CELL * std::f32::consts::SQRT_2;
        let cells = (reach / SIGHT_CELL).ceil() as i32 + 1;
        let here = ((p - self.origin) / SIGHT_CELL).floor();
        (-cells..=cells).any(|dy| {
            (-cells..=cells).any(|dx| {
                let c = here + Vec2::new(dx as f32, dy as f32);
                let middle = self.origin + (c + Vec2::splat(0.5)) * SIGHT_CELL;
                middle.distance_squared(p) <= reach * reach
                    && self.index(middle).is_some_and(|i| self.crossed[i])
            })
        })
    }

    /// Whether a piece `height` meters high, of `radius`, at `p` would hide the road out of a
    /// tight turn from the chase camera: higher than [`SIGHTLINE_HEIGHT`] (the walls' height and
    /// a little more: the camera looks over them), and on one of its sight lines.
    pub fn hidden_by(&self, p: Vec2, radius: f32, height: f32) -> bool {
        height > SIGHTLINE_HEIGHT && self.crossed(p, radius)
    }
}

/// Where a piece `gap` meters beyond the walls of `sample` stands, on side `side` (its center:
/// its footprint's edge at the gap).
pub(super) fn beside(sample: &TrackSample, side: f32, gap: f32, radius: f32) -> Vec2 {
    let flat_right = sample.forward.cross(Vec3::Y).normalize_or(Vec3::X).xz();
    // A centimeter more: at no gap, the footprint just clears the walls of its own sample.
    let reach = wall_offset(sample) + WALL_THICKNESS + CLEARANCE + gap + radius + 0.01;
    sample.position.xz() + flat_right * side * reach
}

/// The yaw that turns a model's front (+Z) from `p` toward the track at `sample`.
fn facing(sample: &TrackSample, p: Vec2) -> f32 {
    let to = sample.position.xz() - p;
    to.x.atan2(to.y)
}

fn on_the_ground(p: Vec2) -> Vec3 {
    Vec3::new(p.x, GROUND_HEIGHT, p.y)
}

/// Whether a footprint of `radius` at `p` (ground plane) is clear of the posts of the start line's
/// arch (issue #79), by [`CLEARANCE`].
pub fn clear_of_the_arch(line: &StartLine, p: Vec2, radius: f32) -> bool {
    line.footprints()
        .iter()
        .all(|(post, reach)| post.distance(p) >= reach + radius + CLEARANCE)
}

/// Issue #119: the same for every arch of the circuit (an open circuit has two: its start and
/// its finish, `StartLine::all`).
pub fn clear_of_the_arches(lines: &[StartLine], p: Vec2, radius: f32) -> bool {
    lines.iter().all(|line| clear_of_the_arch(line, p, radius))
}

/// Places the decor of a circuit: first its file's `props` (where it says, left out where there
/// is no room), then its biome's rows along the walls, then the biome's scattered pieces
/// (`seed`), each one where it does not touch the circuit, the posts of its start line's arch
/// (issue #79) nor a piece already there, never a high one inside a tight turn, and never in a
/// lake (issue #20: on the water, or in its basin).
pub fn place_decor(
    center: &CenterLine,
    biome: Option<&BiomeData>,
    props: &[PropData],
    seed: u64,
    lakes: &[Lake],
) -> DecorPlan {
    place_decor_around(center, biome, props, seed, lakes, &[])
}

/// The same, around the circuit's `shortcuts` too (issue #107): no piece on their roads, their
/// grass paths, nor under the flight of a jump.
pub fn place_decor_around(
    center: &CenterLine,
    biome: Option<&BiomeData>,
    props: &[PropData],
    seed: u64,
    lakes: &[Lake],
    shortcuts: &[Shortcut],
) -> DecorPlan {
    // Issue #119: on an open circuit, only its road (nothing is built on its way back: the decor
    // may stand there, and never along it).
    let road = center.road_samples();
    let samples = &road[..];
    let clear_of_the_circuit = |samples: &[TrackSample], p: Vec2, radius: f32| {
        clear_of_the_circuit(samples, p, radius)
            && shortcuts
                .iter()
                .all(|shortcut| clear_of_the_circuit(shortcut.samples(), p, radius))
    };
    let wet = |p: Vec2, radius: f32| {
        lakes.iter().any(|lake| {
            p.x + radius >= lake.min.x
                && p.x - radius <= lake.max.x
                && p.y + radius >= lake.min.y
                && p.y - radius <= lake.max.y
        })
    };
    let inside = inside_sign(center);
    let sight = TurnSightLines::new(center, &tight_turns(center));
    let lines = StartLine::all(center);
    let mut plan = DecorPlan {
        models: decor_models(biome, props),
        ..default()
    };

    for prop in props {
        let sample = center.at(center.param_to_distance(prop.at));
        for &side in sides(prop.side, inside) {
            let p = beside(&sample, side, prop.gap, prop.radius);
            if !clear_of_the_circuit(samples, p, prop.radius)
                || !clear_of_the_arches(&lines, p, prop.radius)
                || plan.overlaps(p, prop.radius)
                || sight.hidden_by(p, prop.radius, prop.height)
                || wet(p, prop.radius)
            {
                plan.left_out
                    .push(format!("{} at {} (side {side})", prop.model, prop.at));
                continue;
            }
            let model = plan.model(&prop.model);
            plan.pieces.push(DecorPiece {
                model,
                position: on_the_ground(p),
                yaw: facing(&sample, p) + prop.yaw.to_radians(),
                height: prop.height,
                radius: prop.radius,
                shadows: prop.shadows,
            });
        }
    }

    let Some(biome) = biome else {
        return plan;
    };
    for row in &biome.rows {
        let mut turn = 0;
        let mut along = row.offset;
        while along < center.length() {
            let sample = center.at(along);
            if !center.on_the_road(along) {
                along += row.spacing;
                continue;
            }
            for &side in sides(row.side, inside) {
                let p = beside(&sample, side, row.gap, row.radius);
                if clear_of_the_circuit(samples, p, row.radius)
                    && clear_of_the_arches(&lines, p, row.radius)
                    && !plan.overlaps(p, row.radius)
                    && !sight.hidden_by(p, row.radius, row.height)
                    && !wet(p, row.radius)
                {
                    let model = plan.model(&row.models[turn % row.models.len()]);
                    turn += 1;
                    plan.pieces.push(DecorPiece {
                        model,
                        position: on_the_ground(p),
                        yaw: facing(&sample, p) + row.yaw.to_radians(),
                        height: row.height,
                        radius: row.radius,
                        shadows: row.shadows,
                    });
                }
            }
            along += row.spacing;
        }
    }

    let mut rng = BotRng::new(seed);
    // Issue #119: as dense along an open circuit's long road as along a lap.
    let scale = center.decor_scale();
    for group in &biome.scatter {
        for _ in 0..(group.count as f32 * scale).round() as u32 {
            for _ in 0..ATTEMPTS {
                let sample = center.at(rng.range(0.0, center.length()));
                // Issue #119: beside the road only (never along an open circuit's way back).
                if !center.on_the_road(sample.distance) {
                    continue;
                }
                let options = sides(group.side, inside);
                let side = options[(rng.next_u64() % options.len() as u64) as usize];
                let gap = rng.range(group.distance.0, group.distance.1);
                let height = rng.range(group.height.0, group.height.1);
                let p = beside(&sample, side, gap, group.radius);
                if !clear_of_the_circuit(samples, p, group.radius)
                    || !clear_of_the_arches(&lines, p, group.radius)
                    || plan.overlaps(p, group.radius)
                    || sight.hidden_by(p, group.radius, height)
                    || wet(p, group.radius)
                {
                    continue;
                }
                let pick = (rng.next_u64() % group.models.len() as u64) as usize;
                let model = plan.model(&group.models[pick]);
                plan.pieces.push(DecorPiece {
                    model,
                    position: on_the_ground(p),
                    yaw: rng.range(0.0, TAU),
                    height,
                    radius: group.radius,
                    shadows: group.shadows,
                });
                break;
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::data::TrackData;

    fn circuit(file: &str) -> (TrackData, CenterLine) {
        let text = std::fs::read_to_string(format!(
            "{}/assets/tracks/{file}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let data = TrackData::from_ron(&text).unwrap();
        data.validate().unwrap();
        let center = CenterLine::of(&data);
        (data, center)
    }

    /// Issue #191: every circuit of the game (the manifest's files), not six of them.
    fn every_circuit() -> Vec<String> {
        let files: Vec<String> = crate::admin::tracks::circuits()
            .into_iter()
            .map(|entry| entry.file)
            .collect();
        assert!(files.len() >= 12, "{files:?}");
        files
    }

    fn lakes_of(data: &TrackData) -> Vec<Lake> {
        data.lakes.iter().map(Lake::new).collect()
    }

    /// The decor of a circuit of the game, placed as the game places it (`dressing`): around its
    /// shortcuts too (issue #107; issue #108: Prairie Pompon has some).
    fn plan_of(file: &str) -> (TrackData, CenterLine, DecorPlan) {
        let (data, center) = circuit(file);
        let plan = plan_around(&data, &center, data.decor_seed);
        (data, center, plan)
    }

    fn plan_around(data: &TrackData, center: &CenterLine, seed: u64) -> DecorPlan {
        let shortcuts: Vec<Shortcut> = data
            .shortcuts
            .iter()
            .map(|s| Shortcut::new(center, s))
            .collect();
        place_decor_around(
            center,
            data.biome(),
            &data.props,
            seed,
            &lakes_of(data),
            &shortcuts,
        )
    }

    /// Issue #108: the test circuit (the layout Prairie Pompon had before #108, with the
    /// Prairie's biome) and its decor.
    fn test_circuit_plan() -> (TrackData, CenterLine, DecorPlan) {
        let data = crate::track::test_circuit();
        let center = CenterLine::of(&data);
        let plan = place_decor(
            &center,
            data.biome(),
            &data.props,
            data.decor_seed,
            &lakes_of(&data),
        );
        (data, center, plan)
    }

    /// The tight turns: the test circuit's hairpin (12 m at its apex) and banked turn (issue
    /// #108: the layout Prairie Pompon had before #108), the oval's two half circles (35 m, a
    /// little less where the spline cuts them; issue #148: the layout the Anneau Guimauve had
    /// before #148, `crate::track::test_oval`); not the straights.
    #[test]
    fn the_tight_turns_are_the_turns() {
        let prairie = CenterLine::of(&crate::track::test_circuit());
        let turns = tight_turns(&prairie);
        let smallest = turns.iter().map(|t| t.radius).fold(f32::MAX, f32::min);
        assert!((8.0..16.0).contains(&smallest), "{smallest}");
        // The hairpin's island (its center at (18, -86), 12 m) is on the chase camera's sight
        // lines out of it (issue #191): a high piece there hides the road, a low one does not;
        // the middle of the start straight is on none of them.
        let sight = TurnSightLines::new(&prairie, &turns);
        assert!(sight.len() > 10_000, "{}", sight.len());
        assert!(sight.hidden_by(Vec2::new(18.0, -88.0), 1.0, 5.0));
        assert!(!sight.hidden_by(Vec2::new(18.0, -88.0), 1.0, 1.0));
        assert!(!sight.hidden_by(Vec2::new(-20.0, 0.0), 1.0, 5.0));
        let oval = CenterLine::of(&crate::track::test_oval());
        let turns = tight_turns(&oval);
        for t in &turns {
            assert!((20.0..40.0).contains(&t.radius), "{t:?}");
            let c = t.center;
            assert!(
                (c.x - 35.0).abs() < 10.0 && (c.y.abs() - 70.0).abs() < 10.0,
                "{t:?}"
            );
        }
        // Issue #191: a wide turn's sight lines cut across its infield near the road, never
        // through the middle of its circle: a high piece just inside the apex hides the road
        // out of it, one at the circle's center (where the old rule refused it) hides nothing.
        let sight = TurnSightLines::new(&oval, &turns);
        let apex = turns
            .iter()
            .min_by(|a, b| a.radius.total_cmp(&b.radius))
            .unwrap();
        let inward = (apex.center - apex.at).normalize();
        let just_inside = apex.at + inward * 10.0;
        assert!(sight.hidden_by(just_inside, 1.0, 3.0), "{just_inside}");
        assert!(!sight.hidden_by(apex.center, 1.0, 3.0), "{}", apex.center);
        assert!(!sight.hidden_by(Vec2::new(35.0, 0.0), 1.0, 3.0));
    }

    /// Both circuits of issue #11 turn right: their infield is on the driver's right.
    #[test]
    fn both_circuits_have_their_infield_on_the_right() {
        for file in ["prairie.track.ron", "anneau.track.ron"] {
            let (_, center) = circuit(file);
            assert_eq!(inside_sign(&center), 1.0, "{file}");
        }
        // The same loop driven the other way round has it on the left.
        let (mut data, _) = circuit("anneau.track.ron");
        data.points.reverse();
        let reversed = CenterLine::of(&data);
        assert_eq!(inside_sign(&reversed), -1.0);
        // Issue #19: the Crique Câline turns right, the Col des Moufles left; their mirror
        // images the other way. Issue #20: the Lagon Duvet turns right; issue #78: the Ruban
        // Réglisse too. Issue #148: the Crique Câline redrawn turns left (round its cove).
        for (file, sign) in [
            ("crique.track.ron", -1.0),
            ("col.track.ron", -1.0),
            ("lagon.track.ron", 1.0),
            ("ruban.track.ron", 1.0),
        ] {
            let (data, center) = circuit(file);
            assert_eq!(inside_sign(&center), sign, "{file}");
            let mirrored = data.mirrored();
            let center = CenterLine::of(&mirrored);
            assert_eq!(inside_sign(&center), -sign, "{file} (mirror)");
        }
    }

    /// Issue #19: the decor of the mirror mode is the mirror image of the file's, piece by piece
    /// (across the X = 0 plane, each one turned the other way: its front mirrored too), on every
    /// circuit, and it stays clear of the mirrored circuit (every sample and three points between
    /// two samples).
    #[test]
    fn the_mirrored_decor_is_the_mirror_image_and_never_on_the_track() {
        let flip = |v: Vec3| Vec3::new(-v.x, v.y, v.z);
        for file in &every_circuit() {
            let (data, _, plan) = plan_of(file);
            let mirrored = plan.clone().mirrored();
            assert_eq!(mirrored.models, plan.models);
            assert_eq!(mirrored.pieces.len(), plan.pieces.len());
            for (piece, image) in plan.pieces.iter().zip(&mirrored.pieces) {
                assert_eq!(image.position, flip(piece.position));
                let front = |yaw: f32| Quat::from_rotation_y(yaw) * Vec3::Z;
                assert!(front(image.yaw).distance(flip(front(piece.yaw))) < 1e-5);
                assert_eq!(
                    (image.model, image.height, image.radius),
                    (piece.model, piece.height, piece.radius)
                );
            }
            let data = data.mirrored();
            let center = CenterLine::of(&data);
            let samples = center.samples();
            let step = center.length() / samples.len() as f32;
            // Issue #119: on an open circuit, along its road (nothing is built on its way back,
            // where the decor may stand; issue #191: the Échappée Pissenlit and the Périple
            // Polochon are checked too).
            let dense: Vec<TrackSample> = (0..samples.len() * 4)
                .map(|i| center.at(i as f32 * step / 4.0))
                .filter(|s| center.on_the_road(s.distance))
                .collect();
            for piece in &mirrored.pieces {
                for s in &dense {
                    let edge = wall_offset(s) + WALL_THICKNESS;
                    let distance = s.position.xz().distance(piece.position.xz());
                    assert!(
                        distance - piece.radius > edge,
                        "{file} (mirror): a piece at {} reaches the walls",
                        piece.position
                    );
                }
            }
        }
    }

    /// Issue #79: on every circuit and its mirror, no piece of the decor stands on a post of the
    /// start line's arch (their footprints [`CLEARANCE`] apart at least), and every prop of the
    /// file still finds room (the flags at the line stand beyond the posts).
    #[test]
    fn the_decor_keeps_clear_of_the_arch() {
        for file in &every_circuit() {
            let (data, center, plan) = plan_of(file);
            assert!(plan.left_out.is_empty(), "{file}: {:?}", plan.left_out);
            let mirrored = data.mirrored();
            let mirrored_center = CenterLine::of(&mirrored);
            for (center, plan, side) in [
                (&center, plan.clone(), ""),
                (&mirrored_center, plan.clone().mirrored(), " (mirror)"),
            ] {
                let line = StartLine::of(center);
                for piece in &plan.pieces {
                    for (post, reach) in line.footprints() {
                        let apart = post.distance(piece.position.xz()) - reach - piece.radius;
                        assert!(
                            apart >= CLEARANCE - 1e-4,
                            "{file}{side}: {} at {} is {apart:.2} m from a post of the arch",
                            plan.models[piece.model as usize],
                            piece.position
                        );
                    }
                }
            }
        }
    }

    /// Issue #11: the decor of both circuits, from their biome and their props: the same plan on
    /// every load, another one with another seed; every piece of the file and of the biome where
    /// it says; no footprint on the road, the curbs, the shoulders or the walls of any part of the
    /// circuit (measured against every sample, and against points between them), none on another
    /// piece, all on the ground slab around the circuit.
    #[test]
    fn the_decor_is_the_same_every_time_and_never_on_the_track() {
        // Issue #191: every circuit of the game (six before), each with its biome.
        for (file, biome) in [
            ("prairie.track.ron", "Prairie"),
            ("anneau.track.ron", "Guimauve"),
            ("crique.track.ron", "Plage"),
            ("col.track.ron", "Neige"),
            ("lagon.track.ron", "Lagon"),
            ("ruban.track.ron", "Guimauve"),
            ("corniche.track.ron", "Corniche"),
            ("echappee.track.ron", "Prairie"),
            ("foret.track.ron", "Sous-bois"),
            ("falaises.track.ron", "Falaises"),
            ("coteau.track.ron", "Citrouilles"),
            ("periple.track.ron", "Royaume"),
        ] {
            let (data, center, plan) = plan_of(file);
            assert_eq!(data.biome.as_deref(), Some(biome));
            // Issue #20: nothing in a lake (on its water, or in its basin).
            for lake in lakes_of(&data) {
                for piece in &plan.pieces {
                    let p = piece.position.xz();
                    let r = piece.radius;
                    assert!(
                        p.x + r < lake.min.x
                            || p.x - r > lake.max.x
                            || p.y + r < lake.min.y
                            || p.y - r > lake.max.y,
                        "{file}: {} at {p} in the lake",
                        plan.models[piece.model as usize]
                    );
                }
            }
            let again = plan_around(&data, &center, data.decor_seed);
            assert_eq!(plan, again, "{file}: the same plan every time");
            let other = plan_around(&data, &center, data.decor_seed + 1);
            assert_ne!(
                plan.pieces, other.pieces,
                "{file}: another seed, other places"
            );
            assert!(plan.left_out.is_empty(), "{file}: {:?}", plan.left_out);
            assert_eq!(
                plan.models,
                decor_models(data.biome(), &data.props),
                "{file}"
            );

            // Every prop of the file, and nearly all the scattered pieces, found room.
            let props: usize = data
                .props
                .iter()
                .map(|p| if p.side == Side::Both { 2 } else { 1 })
                .sum();
            let scattered: u32 = data.biome().unwrap().scatter.iter().map(|g| g.count).sum();
            let placed = plan.pieces.len();
            assert!(
                placed as f32 >= props as f32 + 0.9 * scattered as f32,
                "{file}: {placed} pieces for {props} props and {scattered} scattered"
            );

            // Clear of the circuit, densely: every sample and three points between two samples.
            let samples = center.samples();
            let step = center.length() / samples.len() as f32;
            // Issue #119: on an open circuit, along its road (nothing is built on its way back,
            // where the decor may stand; issue #191: the Échappée Pissenlit and the Périple
            // Polochon are checked too).
            let dense: Vec<TrackSample> = (0..samples.len() * 4)
                .map(|i| center.at(i as f32 * step / 4.0))
                .filter(|s| center.on_the_road(s.distance))
                .collect();
            for piece in &plan.pieces {
                let p = piece.position.xz();
                for s in &dense {
                    let edge = wall_offset(s) + WALL_THICKNESS;
                    let distance = s.position.xz().distance(p);
                    assert!(
                        distance - piece.radius > edge,
                        "{file}: {} at {p} reaches {:.2} m from the centerline at {:.0} m, the \
                         walls' outer face is at {edge:.2} m",
                        plan.models[piece.model as usize],
                        distance - piece.radius,
                        s.distance
                    );
                }
                assert_eq!(piece.position.y, GROUND_HEIGHT);
                assert!(piece.height > 0.0 && piece.radius > 0.0);
            }
            // Nothing high on a sight line of the chase camera through a tight turn (issue #191:
            // before, nothing high anywhere inside one): the road out of it stays in sight.
            let sight = TurnSightLines::new(&center, &tight_turns(&center));
            for piece in &plan.pieces {
                assert!(
                    !sight.hidden_by(piece.position.xz(), piece.radius, piece.height),
                    "{file}: {} ({} m) on a sight line through a tight turn at {}",
                    plan.models[piece.model as usize],
                    piece.height,
                    piece.position
                );
            }
            for (i, a) in plan.pieces.iter().enumerate() {
                for b in &plan.pieces[..i] {
                    assert!(
                        a.position.distance(b.position) >= a.radius + b.radius - 1e-4,
                        "{file}: two pieces overlap at {}",
                        a.position
                    );
                }
            }
            // On the ground slab: it reaches 80 m beyond the centerline's bounds.
            let (min, max) = samples.iter().fold(
                (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
                |(lo, hi), s| (lo.min(s.position.xz()), hi.max(s.position.xz())),
            );
            for piece in &plan.pieces {
                let p = piece.position.xz();
                assert!(
                    p.cmpge(min - 79.0).all() && p.cmple(max + 79.0).all(),
                    "{p}"
                );
            }
        }
    }

    /// The rows stand along the walls, facing the track: the model's front (+Z, turned by the
    /// yaw) points at the centerline. Inside or outside, as the biome says. Issue #108: on the
    /// test circuit (the layout Prairie Pompon had before #108, with the Prairie's biome): the
    /// new Prairie Pompon, a serpentine, has other legs of the track beyond many of its walls.
    #[test]
    fn rows_face_the_track_on_their_side() {
        let (data, center, plan) = test_circuit_plan();
        let biome = data.biome().unwrap();
        let row = &biome.rows[0];
        assert_eq!(row.side, Side::Outside);
        let row_models: Vec<u16> = row
            .models
            .iter()
            .map(|m| plan.models.iter().position(|x| x == m).unwrap() as u16)
            .collect();
        let pieces: Vec<&DecorPiece> = plan
            .pieces
            .iter()
            .filter(|p| row_models.contains(&p.model))
            .collect();
        let expected = (center.length() / row.spacing) as usize;
        assert!(pieces.len() + 3 >= expected, "{} flags", pieces.len());
        let inside = inside_sign(&center);
        for piece in pieces {
            let p = piece.position;
            let sample = center.samples()[center.nearest_index(p)];
            let front = Quat::from_rotation_y(piece.yaw) * Vec3::Z;
            let to_track = (sample.position - p).with_y(0.0).normalize();
            assert!(front.dot(to_track) > 0.99, "{p}: front {front}");
            let side = (p - sample.position).dot(sample.right).signum();
            assert_eq!(side, -inside, "{p}: outside the loop");
        }
    }

    /// Without a biome, only the file's props stand; a prop with no room is left out and named.
    /// Issue #108: on the test circuit (the layout Prairie Pompon had before #108).
    #[test]
    fn without_a_biome_only_the_props_and_a_prop_without_room_is_left_out() {
        let data = crate::track::test_circuit();
        let center = CenterLine::of(&data);
        let plan = place_decor(&center, None, &data.props, 0, &[]);
        assert_eq!(plan.pieces.len(), {
            data.props
                .iter()
                .map(|p| if p.side == Side::Both { 2 } else { 1 })
                .sum::<usize>()
        });
        // A prop as wide as the hairpin's island, inside it, finds no room.
        let tight = PropData {
            model: "racing-kit/tentClosed".into(),
            at: 6.0,
            side: Side::Right,
            height: 3.0,
            radius: 12.0,
            gap: 0.0,
            yaw: 0.0,
            shadows: true,
        };
        let plan = place_decor(&center, None, std::slice::from_ref(&tight), 0, &[]);
        assert!(plan.pieces.is_empty());
        assert_eq!(plan.left_out.len(), 1);
        assert!(plan.left_out[0].contains("racing-kit/tentClosed"));
    }
}
