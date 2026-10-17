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
    fn overlaps(&self, p: Vec2, radius: f32) -> bool {
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
fn sides(side: Side, inside: f32) -> &'static [f32] {
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

/// Whether a piece `height` meters high at `p` would hide the road out of a tight turn: inside
/// the circle of one, near the turn itself (closer to the sample than the radius: a gentle turn's
/// circle reaches far across the infield, where nothing hides that turn).
pub fn hides_a_turn(turns: &[TightTurn], p: Vec2, height: f32) -> bool {
    height > SIGHTLINE_HEIGHT
        && turns
            .iter()
            .any(|t| t.center.distance(p) < t.radius && t.at.distance(p) < t.radius)
}

/// Where a piece `gap` meters beyond the walls of `sample` stands, on side `side` (its center:
/// its footprint's edge at the gap).
fn beside(sample: &TrackSample, side: f32, gap: f32, radius: f32) -> Vec2 {
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
    let wet = |p: Vec2, radius: f32| {
        lakes.iter().any(|lake| {
            p.x + radius >= lake.min.x
                && p.x - radius <= lake.max.x
                && p.y + radius >= lake.min.y
                && p.y - radius <= lake.max.y
        })
    };
    let samples = center.samples();
    let inside = inside_sign(center);
    let turns = tight_turns(center);
    let line = StartLine::of(center);
    let mut plan = DecorPlan {
        models: decor_models(biome, props),
        ..default()
    };

    for prop in props {
        let sample = center.at(center.param_to_distance(prop.at));
        for &side in sides(prop.side, inside) {
            let p = beside(&sample, side, prop.gap, prop.radius);
            if !clear_of_the_circuit(samples, p, prop.radius)
                || !clear_of_the_arch(&line, p, prop.radius)
                || plan.overlaps(p, prop.radius)
                || hides_a_turn(&turns, p, prop.height)
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
            for &side in sides(row.side, inside) {
                let p = beside(&sample, side, row.gap, row.radius);
                if clear_of_the_circuit(samples, p, row.radius)
                    && clear_of_the_arch(&line, p, row.radius)
                    && !plan.overlaps(p, row.radius)
                    && !hides_a_turn(&turns, p, row.height)
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
    for group in &biome.scatter {
        for _ in 0..group.count {
            for _ in 0..ATTEMPTS {
                let sample = center.at(rng.range(0.0, center.length()));
                let options = sides(group.side, inside);
                let side = options[(rng.next_u64() % options.len() as u64) as usize];
                let gap = rng.range(group.distance.0, group.distance.1);
                let height = rng.range(group.height.0, group.height.1);
                let p = beside(&sample, side, gap, group.radius);
                if !clear_of_the_circuit(samples, p, group.radius)
                    || !clear_of_the_arch(&line, p, group.radius)
                    || plan.overlaps(p, group.radius)
                    || hides_a_turn(&turns, p, height)
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

    fn lakes_of(data: &TrackData) -> Vec<Lake> {
        data.lakes.iter().map(Lake::new).collect()
    }

    fn plan_of(file: &str) -> (TrackData, CenterLine, DecorPlan) {
        let (data, center) = circuit(file);
        let plan = place_decor(
            &center,
            data.biome(),
            &data.props,
            data.decor_seed,
            &lakes_of(&data),
        );
        (data, center, plan)
    }

    /// The tight turns: Prairie Pompon's hairpin (12 m at its apex) and banked turn, the oval's
    /// two half circles (35 m, a little less where the spline cuts them); not the straights.
    #[test]
    fn the_tight_turns_are_the_turns() {
        let (_, prairie) = circuit("prairie.track.ron");
        let turns = tight_turns(&prairie);
        let smallest = turns.iter().map(|t| t.radius).fold(f32::MAX, f32::min);
        assert!((8.0..16.0).contains(&smallest), "{smallest}");
        // The hairpin's center (18, -86) is inside, the middle of the start straight is not.
        assert!(hides_a_turn(&turns, Vec2::new(18.0, -88.0), 5.0));
        assert!(!hides_a_turn(&turns, Vec2::new(18.0, -88.0), 1.0));
        assert!(!hides_a_turn(&turns, Vec2::new(-20.0, 0.0), 5.0));
        let (_, oval) = circuit("anneau.track.ron");
        let turns = tight_turns(&oval);
        for t in &turns {
            assert!((20.0..40.0).contains(&t.radius), "{t:?}");
            let c = t.center;
            assert!(
                (c.x - 35.0).abs() < 10.0 && (c.y.abs() - 70.0).abs() < 10.0,
                "{t:?}"
            );
        }
        assert!(hides_a_turn(&turns, Vec2::new(35.0, -75.0), 3.0));
        assert!(!hides_a_turn(&turns, Vec2::new(35.0, 0.0), 3.0));
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
        // Réglisse too.
        for (file, sign) in [
            ("crique.track.ron", 1.0),
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
        for file in [
            "prairie.track.ron",
            "anneau.track.ron",
            "crique.track.ron",
            "col.track.ron",
            "lagon.track.ron",
            "ruban.track.ron",
        ] {
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
            let dense: Vec<TrackSample> = (0..samples.len() * 4)
                .map(|i| center.at(i as f32 * step / 4.0))
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
        for file in [
            "prairie.track.ron",
            "anneau.track.ron",
            "crique.track.ron",
            "col.track.ron",
            "lagon.track.ron",
            "ruban.track.ron",
        ] {
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
        for (file, biome) in [
            ("prairie.track.ron", "Prairie"),
            ("anneau.track.ron", "Guimauve"),
            ("crique.track.ron", "Plage"),
            ("col.track.ron", "Neige"),
            ("lagon.track.ron", "Lagon"),
            ("ruban.track.ron", "Guimauve"),
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
            let again = place_decor(
                &center,
                data.biome(),
                &data.props,
                data.decor_seed,
                &lakes_of(&data),
            );
            assert_eq!(plan, again, "{file}: the same plan every time");
            let other = place_decor(
                &center,
                data.biome(),
                &data.props,
                data.decor_seed + 1,
                &lakes_of(&data),
            );
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
            let dense: Vec<TrackSample> = (0..samples.len() * 4)
                .map(|i| center.at(i as f32 * step / 4.0))
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
            // Nothing high inside a tight turn: the road out of it stays in sight.
            let turns = tight_turns(&center);
            for piece in &plan.pieces {
                assert!(
                    !hides_a_turn(&turns, piece.position.xz(), piece.height),
                    "{file}: {} ({} m) inside a tight turn at {}",
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
    /// yaw) points at the centerline. Inside or outside, as the biome says.
    #[test]
    fn rows_face_the_track_on_their_side() {
        let (data, center, plan) = plan_of("prairie.track.ron");
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
    #[test]
    fn without_a_biome_only_the_props_and_a_prop_without_room_is_left_out() {
        let (data, center) = circuit("prairie.track.ron");
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
