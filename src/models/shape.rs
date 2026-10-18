//! Soft shapes for the models built in code (issue #106: the items, the croquettes, the gift
//! box): ellipsoids, rounded boxes, tori and tubes along a path, each one a smooth surface (the
//! normal of each vertex is the mean of its triangles': round and soft, « mou »), colored per
//! vertex (linear, like the baked models': `models::bake`), appended to a [`MeshData`]. A model
//! is a handful of them merged into one mesh, drawn with the models' shared material. Pure, no
//! ECS.
//!
//! Every shape faces out: its triangles turn counter-clockwise seen from outside (Bevy culls their
//! back), its normals point out (the tests check both).

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::track::mesh::MeshData;

/// An sRGB color as a vertex color (linear, like the baked models').
pub fn srgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    Color::srgb(r, g, b).to_linear().to_f32_array()
}

/// A shape of a single color.
pub fn solid(color: [f32; 4]) -> impl Fn(Vec3) -> [f32; 4] {
    move |_| color
}

/// `a` toward `b` by `t` (0 to 1), two vertex colors.
pub fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// How the rows of a surface end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    /// Its first and last rows are single points: a closed blob (an ellipsoid, a drop).
    Poles,
    /// Its first and last rows are rings left open (a tube's open ends).
    Open,
    /// Its last row joins its first: a torus.
    Wrap,
}

/// How finely a shape is cut: points around, rows along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    pub around: usize,
    pub rows: usize,
}

impl Detail {
    pub const fn new(around: usize, rows: usize) -> Self {
        Self { around, rows }
    }
}

/// Adds a smooth surface of `rows` rows of `detail.around` points: `point(u, row)` in the shape's
/// own space (`u` from 0 to 1 around, wrapping), placed by `at`, each point colored by
/// `color(point)` (in the shape's own space). Its triangles face where `∂u × ∂row` points: out,
/// for every shape of this module. Each vertex's normal is the mean of its triangles' (weighted by
/// their area), in the placed space.
pub fn surface(
    mesh: &mut MeshData,
    at: Mat4,
    around: usize,
    rows: usize,
    ends: Ends,
    point: impl Fn(f32, usize) -> Vec3,
    color: impl Fn(Vec3) -> [f32; 4],
) {
    let around = around.max(3);
    let rows = rows.max(if ends == Ends::Poles { 3 } else { 2 });
    let mut local: Vec<Vec3> = Vec::new();
    // Where each row's points start (a pole is one point).
    let mut starts: Vec<usize> = Vec::with_capacity(rows);
    for row in 0..rows {
        starts.push(local.len());
        let pole = ends == Ends::Poles && (row == 0 || row == rows - 1);
        if pole {
            local.push(point(0.0, row));
        } else {
            local.extend((0..around).map(|i| point(i as f32 / around as f32, row)));
        }
    }
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let ring = |row: usize, i: usize| (starts[row] + i % around) as u32;
    let pairs = match ends {
        Ends::Wrap => rows,
        _ => rows - 1,
    };
    for a in 0..pairs {
        let b = (a + 1) % rows;
        let a_pole = ends == Ends::Poles && a == 0;
        let b_pole = ends == Ends::Poles && b == rows - 1;
        for i in 0..around {
            match (a_pole, b_pole) {
                (true, _) => triangles.push([starts[a] as u32, ring(b, i + 1), ring(b, i)]),
                (_, true) => triangles.push([ring(a, i), ring(a, i + 1), starts[b] as u32]),
                _ => {
                    triangles.push([ring(a, i), ring(a, i + 1), ring(b, i + 1)]);
                    triangles.push([ring(a, i), ring(b, i + 1), ring(b, i)]);
                }
            }
        }
    }
    let placed: Vec<Vec3> = local.iter().map(|p| at.transform_point3(*p)).collect();
    // A mirroring placement turns the triangles inside out: turned back.
    let mirrored = at.determinant() < 0.0;
    let mut normals = vec![Vec3::ZERO; placed.len()];
    for t in &mut triangles {
        if mirrored {
            t.swap(1, 2);
        }
        let [a, b, c] = t.map(|i| placed[i as usize]);
        let n = (b - a).cross(c - a);
        for i in *t {
            normals[i as usize] += n;
        }
    }
    let base = mesh.positions.len() as u32;
    for ((p, n), l) in placed.iter().zip(&normals).zip(&local) {
        mesh.positions.push(p.to_array());
        mesh.normals.push(n.normalize_or(Vec3::Y).to_array());
        mesh.colors.push(color(*l));
    }
    mesh.indices
        .extend(triangles.iter().flat_map(|t| t.map(|i| base + i)));
}

/// `x`'s sign times `|x|^power` (the superellipsoid's curves).
fn signed_power(x: f32, power: f32) -> f32 {
    x.signum() * x.abs().powf(power)
}

/// A point of the unit sphere: `u` around (from +X toward +Z), `v` from its top (+Y, 0) to its
/// bottom (1).
fn sphere_point(u: f32, v: f32) -> Vec3 {
    let (theta, phi) = (PI * v, TAU * u);
    Vec3::new(
        theta.sin() * phi.cos(),
        theta.cos(),
        theta.sin() * phi.sin(),
    )
}

/// An ellipsoid of `radii` (its poles along Y), placed by `at`.
pub fn ellipsoid(
    mesh: &mut MeshData,
    at: Mat4,
    radii: Vec3,
    detail: Detail,
    color: impl Fn(Vec3) -> [f32; 4],
) {
    let rows = detail.rows.max(3);
    surface(
        mesh,
        at,
        detail.around,
        rows,
        Ends::Poles,
        |u, row| radii * sphere_point(u, row as f32 / (rows - 1) as f32),
        color,
    );
}

/// A rounded box (a superellipsoid): `half` its half sizes, `round_y` and `round_xz` how round
/// its profile and its outline are (1: an ellipsoid; toward 0: a box with sharper edges).
pub fn rounded_box(
    mesh: &mut MeshData,
    at: Mat4,
    half: Vec3,
    round_y: f32,
    round_xz: f32,
    detail: Detail,
    color: impl Fn(Vec3) -> [f32; 4],
) {
    let rows = detail.rows.max(3);
    surface(
        mesh,
        at,
        detail.around,
        rows,
        Ends::Poles,
        |u, row| {
            let (theta, phi) = (PI * row as f32 / (rows - 1) as f32, TAU * u);
            let ring = signed_power(theta.sin(), round_y);
            half * Vec3::new(
                ring * signed_power(phi.cos(), round_xz),
                signed_power(theta.cos(), round_y),
                ring * signed_power(phi.sin(), round_xz),
            )
        },
        color,
    );
}

/// A torus lying flat (its axis along Y): `major` the radius of its middle circle, `minor` its
/// thickness'.
pub fn torus(
    mesh: &mut MeshData,
    at: Mat4,
    major: f32,
    minor: f32,
    detail: Detail,
    color: impl Fn(Vec3) -> [f32; 4],
) {
    let rows = detail.rows.max(3);
    surface(
        mesh,
        at,
        detail.around,
        rows,
        Ends::Wrap,
        |u, row| {
            let (phi, psi) = (TAU * u, TAU * row as f32 / rows as f32);
            let out = major + minor * psi.cos();
            Vec3::new(out * phi.cos(), -minor * psi.sin(), out * phi.sin())
        },
        color,
    );
}

/// A tube through `rings` (each one a center and its radius), cut `around` times: with
/// [`Ends::Poles`], its first and last rings are its tips (their radius is not used); with
/// [`Ends::Open`], its ends are open. `flat` squashes its section across `up` (1: round; 0.3: a
/// strip lying across `up`), `up` a direction across the tube at its start (followed along it).
#[allow(clippy::too_many_arguments)]
pub fn tube(
    mesh: &mut MeshData,
    at: Mat4,
    rings: &[(Vec3, f32)],
    ends: Ends,
    around: usize,
    flat: f32,
    up: Vec3,
    color: impl Fn(Vec3) -> [f32; 4],
) {
    let n = rings.len();
    if n < 2 {
        return;
    }
    // The tangent at each ring: from its neighbors (the last non-zero one where rings coincide).
    let mut tangents: Vec<Vec3> = Vec::with_capacity(n);
    let mut last = (rings[n - 1].0 - rings[0].0).normalize_or(Vec3::Y);
    for k in 0..n {
        let before = rings[k.saturating_sub(1)].0;
        let after = rings[(k + 1).min(n - 1)].0;
        let t = (after - before).normalize_or(last);
        tangents.push(t);
        last = t;
    }
    // Frames carried along the tube (parallel transport): no twist.
    let mut frames: Vec<(Vec3, Vec3)> = Vec::with_capacity(n);
    let first = tangents[0];
    let mut normal = (up - first * up.dot(first)).normalize_or(first.any_orthonormal_vector());
    for tangent in &tangents {
        normal = (normal - *tangent * normal.dot(*tangent)).normalize_or(normal);
        let binormal = tangent.cross(normal);
        frames.push((normal, binormal));
    }
    surface(
        mesh,
        at,
        around,
        n,
        ends,
        |u, row| {
            let (center, radius) = rings[row];
            let (normal, binormal) = frames[row];
            let phi = TAU * u;
            center + radius * (normal * (flat * phi.cos()) + binormal * phi.sin())
        },
        color,
    );
}

/// The rings of a cone (or a frustum) along +Y from `base` (its radius `bottom`) to `top` above
/// it (its radius `tip`, 0 for a point), `steps` rings along, a flat base: for [`tube`] with
/// [`Ends::Poles`].
pub fn cone_rings(
    base: Vec3,
    height: f32,
    bottom: f32,
    tip: f32,
    steps: usize,
) -> Vec<(Vec3, f32)> {
    let steps = steps.max(1);
    let mut rings = vec![(base, 0.0)];
    rings.extend((0..=steps).map(|k| {
        let t = k as f32 / steps as f32;
        (base + Vec3::Y * height * t, bottom + (tip - bottom) * t)
    }));
    rings.push((base + Vec3::Y * height, 0.0));
    rings
}

/// A placement: `scale`, then turned by `rotation`, then moved to `translation`.
pub fn place(translation: Vec3, rotation: Quat, scale: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(scale, rotation, translation)
}

/// A placement without scale.
pub fn at(translation: Vec3, rotation: Quat) -> Mat4 {
    Mat4::from_rotation_translation(rotation, translation)
}

/// The box around `mesh` (None: no vertex).
pub fn bounds(mesh: &MeshData) -> Option<(Vec3, Vec3)> {
    mesh.positions
        .iter()
        .map(|p| Vec3::from(*p))
        .fold(None, |b, p| match b {
            None => Some((p, p)),
            Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
        })
}

/// `mesh` moved by `transform` (its normals turned and renormalized; a mirroring transform keeps
/// its front outside).
pub fn moved(mesh: &MeshData, transform: Mat4) -> MeshData {
    crate::models::bake::transformed(mesh, transform)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each triangle of `mesh` faces away from `inside` (counter-clockwise seen from outside),
    /// and each normal points away from it too.
    fn faces_out(mesh: &MeshData, inside: impl Fn(Vec3) -> Vec3) {
        assert!(mesh.triangle_count() > 0);
        for t in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| Vec3::from(mesh.positions[i as usize]));
            let center = (a + b + c) / 3.0;
            let n = (b - a).cross(c - a);
            if n.length() < 1e-7 {
                continue;
            }
            assert!(
                n.dot(center - inside(center)) > 0.0,
                "a triangle faces in at {center}"
            );
        }
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            let (p, n) = (Vec3::from(*p), Vec3::from(*n));
            assert!((n.length() - 1.0).abs() < 1e-3);
            assert!(n.dot(p - inside(p)) > -1e-3, "a normal points in at {p}");
        }
    }

    #[test]
    fn an_ellipsoid_is_closed_smooth_and_faces_out() {
        let mut mesh = MeshData::default();
        let center = Vec3::new(1.0, 2.0, 3.0);
        let turn = Quat::from_rotation_x(0.7);
        ellipsoid(
            &mut mesh,
            at(center, turn),
            Vec3::new(1.0, 0.5, 2.0),
            Detail::new(12, 8),
            solid(srgb(1.0, 0.0, 0.0)),
        );
        // Two poles and six rings of twelve.
        assert_eq!(mesh.positions.len(), 2 + 6 * 12);
        assert_eq!(mesh.triangle_count(), 2 * 12 + 5 * 12 * 2);
        faces_out(&mesh, |_| center);
        // Closed: every edge is shared by two triangles.
        let mut edges = std::collections::HashMap::new();
        for t in mesh.indices.as_chunks::<3>().0 {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|n| *n == 2));
        let (lo, hi) = bounds(&mesh).unwrap();
        // About its width (no row exactly on its equator).
        assert!((hi.x - lo.x - 2.0).abs() < 0.1, "{lo} {hi}");
        // Its color is linear.
        assert!((mesh.colors[0][0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_rounded_box_fills_its_box_and_faces_out() {
        let mut mesh = MeshData::default();
        rounded_box(
            &mut mesh,
            Mat4::IDENTITY,
            Vec3::new(1.0, 0.4, 0.8),
            0.6,
            0.3,
            Detail::new(16, 12),
            solid([1.0; 4]),
        );
        faces_out(&mesh, |_| Vec3::ZERO);
        let (lo, hi) = bounds(&mesh).unwrap();
        assert!((hi - Vec3::new(1.0, 0.4, 0.8)).length() < 0.02, "{hi}");
        assert!((lo + Vec3::new(1.0, 0.4, 0.8)).length() < 0.02, "{lo}");
        // Boxier than an ellipsoid: a corner is reached far out along the diagonal.
        let corner = mesh
            .positions
            .iter()
            .map(|p| Vec3::from(*p).xz().dot(Vec2::new(0.8, 1.0).normalize()))
            .fold(0.0f32, f32::max);
        assert!(corner > 1.0, "{corner}");
    }

    #[test]
    fn a_torus_faces_out_of_its_ring() {
        let mut mesh = MeshData::default();
        torus(
            &mut mesh,
            Mat4::IDENTITY,
            1.0,
            0.3,
            Detail::new(16, 8),
            solid([1.0; 4]),
        );
        assert_eq!(mesh.positions.len(), 16 * 8);
        faces_out(&mesh, |p| Vec3::new(p.x, 0.0, p.z).normalize());
    }

    #[test]
    fn a_tube_along_a_bent_path_faces_out_with_its_tips() {
        let path: Vec<(Vec3, f32)> = (0..9)
            .map(|k| {
                let t = k as f32 / 8.0;
                (Vec3::new(t * 2.0, (t * PI).sin(), 0.0), 0.2)
            })
            .collect();
        let mut mesh = MeshData::default();
        tube(
            &mut mesh,
            Mat4::IDENTITY,
            &path,
            Ends::Poles,
            10,
            1.0,
            Vec3::Z,
            solid([1.0; 4]),
        );
        let nearest = |p: Vec3| {
            path.iter()
                .map(|(c, _)| *c)
                .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
                .unwrap()
        };
        faces_out(&mesh, nearest);
        // Flat: a strip across Z, as thick as `flat` says.
        let mut strip = MeshData::default();
        tube(
            &mut strip,
            Mat4::IDENTITY,
            &[(Vec3::ZERO, 1.0), (Vec3::X, 1.0)],
            Ends::Open,
            12,
            0.25,
            Vec3::Y,
            solid([1.0; 4]),
        );
        let (lo, hi) = bounds(&strip).unwrap();
        assert!((hi.z - lo.z - 2.0).abs() < 0.01 && (hi.y - lo.y - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_mirrored_shape_still_faces_out() {
        let mut mesh = MeshData::default();
        ellipsoid(
            &mut mesh,
            place(Vec3::ZERO, Quat::IDENTITY, Vec3::new(-1.0, 1.0, 1.0)),
            Vec3::ONE,
            Detail::new(8, 6),
            solid([1.0; 4]),
        );
        faces_out(&mesh, |_| Vec3::ZERO);
    }

    #[test]
    fn a_cone_has_a_flat_base_and_a_tip() {
        let mut mesh = MeshData::default();
        tube(
            &mut mesh,
            Mat4::IDENTITY,
            &cone_rings(Vec3::ZERO, 2.0, 1.0, 0.0, 3),
            Ends::Poles,
            12,
            1.0,
            Vec3::X,
            solid([1.0; 4]),
        );
        let (lo, hi) = bounds(&mesh).unwrap();
        assert!(lo.y.abs() < 1e-5 && (hi.y - 2.0).abs() < 1e-5);
        faces_out(&mesh, |p| Vec3::new(0.0, p.y.clamp(0.3, 1.0), 0.0));
    }
}
