//! The kart's parts on screen (issue #73): each body, set of wheels and wing of the roster
//! (GD § 9, issue #15) has a look of its own, named in its RON file (`look: Bathtub`), and every
//! kart shows the parts it has (the player's, the bots', the other players' online: their
//! `Loadout`).
//!
//! - **Bodies**: the soap box is the kart of Kenney's Car Kit (issue #11, `model.rs`); no pack
//!   has a wicker basket nor a bathtub, so both are made here, low-poly like the packs: an oval
//!   basket woven in two shades, its braided rim, a gingham cushion and two handles; a clawfoot
//!   bathtub, its rolled rim, its tap, its golden feet and a rubber duck. Each body is split like
//!   the Car Kit's: its shell to paint (grey, times the kart's color: `KartVisualAssets::paint`,
//!   issue #55), the rest in its own vertex colors (the models' shared material); each has its
//!   seat (the driver's bottom, issue #69) and its wheels' hubs.
//! - **Wheels**: marshmallow tyres are the Car Kit kart's own wheels; tractor tyres the Car
//!   Kit's big tractor wheel, smaller at the front (the same mesh, scaled); thread spools are
//!   made here (two wooden flanges around a coil of pink thread), smaller. A set of wheels is one
//!   mesh, a left wheel around its hub (from the hub out toward -X; the right ones are turned
//!   half a turn), and its radii put its wheels on the road (`WheelVisual::radius`).
//! - **Wings**: the glider of issue #20, only out while gliding: the pillowcase is its puffy
//!   ellipsoid (`environment_visual.rs`), the feather a long white feather made here, its tip
//!   pale blue.
//!
//! Purely visual: the collider, the suspension and the stats are the same whatever the looks.
//! Pure here (no ECS); `visual.rs` makes the meshes once (shared by every kart that has the part)
//! when the kart model is baked.

use bevy::prelude::*;
use serde::Deserialize;

use super::model::KartModelData;
use crate::models::BakedModel;
use crate::models::bake::transformed;
use crate::track::GRID_HEIGHT;
use crate::track::mesh::MeshData;

/// The tractor tyres' file (Car Kit 3.1, like the kart): its big back wheel (the front ones are
/// the same, smaller: one mesh for the four).
pub const TRACTOR_MODEL: &str = "car-kit/wheel-tractor-back";
/// The models the parts need besides the kart's.
pub const PART_MODELS: [&str; 1] = [TRACTOR_MODEL];

/// A part's look, as its RON file names it (`look: WickerBasket`): a body in `karts.ron`, a set
/// of wheels in `wheels.ron`, a wing in `wings.ron`.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartLook {
    WickerBasket,
    SoapBox,
    Bathtub,
    Spools,
    Marshmallow,
    Tractor,
    Pillowcase,
    Feather,
}

impl PartLook {
    pub fn body(self) -> Option<BodyLook> {
        match self {
            PartLook::WickerBasket => Some(BodyLook::WickerBasket),
            PartLook::SoapBox => Some(BodyLook::SoapBox),
            PartLook::Bathtub => Some(BodyLook::Bathtub),
            _ => None,
        }
    }

    pub fn wheels(self) -> Option<WheelLook> {
        match self {
            PartLook::Spools => Some(WheelLook::Spools),
            PartLook::Marshmallow => Some(WheelLook::Marshmallow),
            PartLook::Tractor => Some(WheelLook::Tractor),
            _ => None,
        }
    }

    pub fn wing(self) -> Option<WingLook> {
        match self {
            PartLook::Pillowcase => Some(WingLook::Pillowcase),
            PartLook::Feather => Some(WingLook::Feather),
            _ => None,
        }
    }
}

/// The body of a kart (GD § 9: « kart »). The default is the Car Kit's kart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BodyLook {
    WickerBasket,
    #[default]
    SoapBox,
    Bathtub,
}

impl BodyLook {
    pub const ALL: [Self; 3] = [Self::WickerBasket, Self::SoapBox, Self::Bathtub];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A kart's set of wheels. The default is the Car Kit kart's own wheels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WheelLook {
    Spools,
    #[default]
    Marshmallow,
    Tractor,
}

impl WheelLook {
    pub const ALL: [Self; 3] = [Self::Spools, Self::Marshmallow, Self::Tractor];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A kart's glider wing (issue #20). The default is the pillowcase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WingLook {
    #[default]
    Pillowcase,
    Feather,
}

impl WingLook {
    pub const ALL: [Self; 2] = [Self::Pillowcase, Self::Feather];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// What a kart shows of its parts (from its `Loadout`, `characters::Roster::looks`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PartLooks {
    pub body: BodyLook,
    pub wheels: WheelLook,
    pub wing: WingLook,
}

/// A body, ready to draw, in the space of the visual's pivot (`KartBodyPivot`: the kart's body
/// center, its front toward -Z, the road `GRID_HEIGHT` below at rest).
#[derive(Clone, Debug, PartialEq)]
pub struct BodyData {
    /// The shell, grey (1 at its lightest, linear): times the kart's color.
    pub paint: MeshData,
    /// The rest, in its own colors.
    pub trim: MeshData,
    /// Where each wheel's hub is (the middle of its inner face) with the suspension at rest, in
    /// the order of `WHEEL_ANCHORS` (front left, front right, back left, back right).
    pub hubs: [Vec3; 4],
    /// Where the driver sits (its bottom).
    pub seat: Vec3,
}

impl BodyData {
    /// How each wheel's mesh is turned: a left wheel as it is, a right one half a turn.
    pub fn wheel_turns(&self) -> [Quat; 4] {
        self.hubs.map(|hub| {
            if hub.x < 0.0 {
                Quat::IDENTITY
            } else {
                Quat::from_rotation_y(std::f32::consts::PI)
            }
        })
    }
}

/// A set of wheels: one mesh for the four (a left wheel around its hub, out toward -X, as big as
/// the back wheels) and the radius of the front and the back wheels (m): the front ones are the
/// mesh scaled to theirs.
#[derive(Clone, Debug, PartialEq)]
pub struct WheelData {
    pub mesh: MeshData,
    pub radii: [f32; 2],
}

impl WheelData {
    /// The axle of the wheel `index` of `WHEEL_ANCHORS`: 0 at the front, 1 at the back.
    pub fn axle(index: usize) -> usize {
        usize::from(index >= 2)
    }

    /// The radius of the wheel `index` of `WHEEL_ANCHORS`.
    pub fn radius(&self, index: usize) -> f32 {
        wheel_radius(self.radii, index)
    }

    /// How much the mesh is scaled (around its hub) for the wheel `index`: 1 at the back.
    pub fn scale(&self, index: usize) -> f32 {
        wheel_scale(self.radii, index)
    }
}

/// The radius of the wheel `index` of `WHEEL_ANCHORS` of a set whose front and back wheels have
/// `radii`.
pub fn wheel_radius(radii: [f32; 2], index: usize) -> f32 {
    radii[WheelData::axle(index)]
}

/// How much the mesh of a set whose front and back wheels have `radii` (the mesh is a back
/// wheel) is scaled for the wheel `index`.
pub fn wheel_scale(radii: [f32; 2], index: usize) -> f32 {
    if radii[1] > 0.0 {
        wheel_radius(radii, index) / radii[1]
    } else {
        1.0
    }
}

/// Every part's look, ready to draw: by [`BodyLook::index`], [`WheelLook::index`], and the
/// feather (the pillowcase is a primitive, `environment_visual.rs`).
#[derive(Clone, Debug, PartialEq)]
pub struct KartParts {
    pub bodies: [BodyData; 3],
    pub wheels: [WheelData; 3],
    /// The feather's sail, in the wing's space (the kart's frame, the driver below it).
    pub feather: MeshData,
}

impl KartParts {
    pub fn body(&self, look: BodyLook) -> &BodyData {
        &self.bodies[look.index()]
    }

    pub fn wheels(&self, look: WheelLook) -> &WheelData {
        &self.wheels[look.index()]
    }
}

/// Every part from the kart model (the soap box and the marshmallow tyres) and the tractor
/// tyre's file (`None`: missing, the marshmallow tyres stand in).
///
/// Issue #176: each set of wheels has its band painted across it ([`paint_band`],
/// [`WHEEL_BANDS`]), so that it shows its turn.
pub fn kart_parts(kart: &KartModelData, tractor: Option<&BakedModel>) -> KartParts {
    let marshmallow = marshmallow(kart);
    let tractor = tractor
        .and_then(tractor_wheels)
        .unwrap_or_else(|| marshmallow.clone());
    let mut wheels = [spools(), marshmallow, tractor];
    for (set, color) in wheels.iter_mut().zip(WHEEL_BANDS) {
        paint_band(&mut set.mesh, color);
    }
    KartParts {
        bodies: [wicker_basket(), soap_box(kart), bathtub()],
        wheels,
        feather: feather(),
    }
}

// --- The band of the wheels (issue #176) --------------------------------------------------------

/// Issue #176: the color of the band across each set of wheels, by [`WheelLook::index`] (linear):
/// a blue thread on the pink spools, a strawberry stripe on the marshmallow tyres, a yellow one on
/// the tractor tyres.
pub const WHEEL_BANDS: [[f32; 4]; 3] = [
    [0.05, 0.2, 0.85, 1.0],
    [1.0, 0.32, 0.48, 1.0],
    [1.0, 0.62, 0.03, 1.0],
];

/// How far around the axle the band reaches on each side of where it starts (rad): the faces of
/// the mesh that overlap it take its color.
pub const BAND_HALF_ANGLE: f32 = 0.12;

/// Where the band is around the axle (rad from the top, +Y, toward +Z): a little off any corner,
/// so that it falls inside a column of faces.
const BAND_AT: f32 = 0.05;

/// The angle of `p` around the X axis, from `BAND_AT` (rad, -π to π), and how far it is from it.
fn around_axle(p: Vec3) -> (f32, f32) {
    let angle = p.z.atan2(p.y) - BAND_AT;
    let wrapped =
        (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    (wrapped, Vec2::new(p.y, p.z).length())
}

/// Issue #176: paints a band of `color` across a wheel turning around the X axis (its tread, its
/// faces from the hub to the rim): every face that overlaps the band around the axle
/// ([`BAND_HALF_ANGLE`] on each side) takes its color, on corners of its own (its neighbours keep
/// theirs). One band only: the wheel looks different at each angle of its turn (it never seems
/// to turn backward, as stripes all around would at speed). A face across the whole face of the
/// wheel (more than a quarter turn) is left as it is.
pub fn paint_band(mesh: &mut MeshData, color: [f32; 4]) {
    if mesh.colors.len() != mesh.positions.len() || mesh.normals.len() != mesh.positions.len() {
        return;
    }
    let triangles = mesh.indices.len() / 3;
    for t in 0..triangles {
        let corners = [0, 1, 2].map(|k| mesh.indices[3 * t + k] as usize);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for &c in &corners {
            let (angle, radius) = around_axle(Vec3::from(mesh.positions[c]));
            // A corner on the axle (the middle of a face) is at every angle.
            if radius > 1e-4 {
                lo = lo.min(angle);
                hi = hi.max(angle);
            }
        }
        if lo > hi || hi - lo > std::f32::consts::FRAC_PI_2 {
            continue;
        }
        if lo > BAND_HALF_ANGLE || hi < -BAND_HALF_ANGLE {
            continue;
        }
        for (k, &c) in corners.iter().enumerate() {
            let new = mesh.positions.len() as u32;
            mesh.positions.push(mesh.positions[c]);
            mesh.normals.push(mesh.normals[c]);
            mesh.colors.push(color);
            mesh.indices[3 * t + k] = new;
        }
    }
}

/// The soap box: the Car Kit's kart (issue #11).
pub fn soap_box(kart: &KartModelData) -> BodyData {
    BodyData {
        paint: kart.paint.clone(),
        trim: kart.trim.clone(),
        hubs: kart.hubs,
        seat: kart.seat,
    }
}

/// The marshmallow tyres: the Car Kit kart's own wheels.
pub fn marshmallow(kart: &KartModelData) -> WheelData {
    WheelData {
        mesh: kart.wheel.clone(),
        radii: [kart.wheel_radius; 2],
    }
}

// --- The tractor tyres -------------------------------------------------------------------------

/// The tractor tyres' size: radius (m), front and back, and width of the back ones (m; the front
/// ones in proportion).
const TRACTOR_RADII: [f32; 2] = [0.32, 0.42];
const TRACTOR_WIDTH: f32 = 0.34;

/// A wheel's file (centered on its axle, along X) as a left wheel around its hub, `radius` and
/// `width` big: turned like the kart's wheels (its outer side out), from its hub out toward -X.
fn wheel_from_file(model: &BakedModel, radius: f32, width: f32) -> Option<MeshData> {
    let part = model.parts.first()?;
    let (lo, hi) = model.bounds()?;
    let size = hi - lo;
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let center = (lo + hi) / 2.0;
    let scale = Vec3::new(width / size.x, 2.0 * radius / size.y, 2.0 * radius / size.y);
    let to_hub = Mat4::from_translation(Vec3::new(-width / 2.0, 0.0, 0.0))
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_scale(scale)
        * Mat4::from_translation(-center);
    Some(transformed(&part.mesh, to_hub * part.transform))
}

/// The tractor tyres: the Car Kit's big tractor wheel, at the back, and smaller at the front
/// (None: the file is not a wheel).
pub fn tractor_wheels(back: &BakedModel) -> Option<WheelData> {
    Some(WheelData {
        mesh: wheel_from_file(back, TRACTOR_RADII[1], TRACTOR_WIDTH)?,
        radii: TRACTOR_RADII,
    })
}

// --- Building blocks ---------------------------------------------------------------------------

/// An sRGB color as a vertex color (linear, like the baked models').
fn srgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    Color::srgb(r, g, b).to_linear().to_f32_array()
}

/// A grey of the paint (linear: times the kart's color).
fn grey(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// A flat face (3 or 4 corners, counter-clockwise seen from its front), `color` linear.
/// Corners that coincide count once (a quad with an edge on an axis is a triangle); a face of no
/// area is left out.
fn face(mesh: &mut MeshData, corners: &[Vec3], color: [f32; 4]) {
    let mut points: [Vec3; 4] = [Vec3::ZERO; 4];
    let mut count = 0;
    for corner in corners.iter().take(4) {
        if count == 0 || (points[count - 1] != *corner && points[0] != *corner) {
            points[count] = *corner;
            count += 1;
        }
    }
    if count < 3 {
        return;
    }
    let normal = (points[1] - points[0]).cross(points[2] - points[0]);
    if normal.length_squared() < 1e-12 {
        return;
    }
    let normal = normal.normalize();
    let base = mesh.positions.len() as u32;
    for point in &points[..count] {
        mesh.positions.push(point.to_array());
        mesh.normals.push(normal.to_array());
        mesh.colors.push(color);
    }
    for i in 1..count as u32 - 1 {
        mesh.indices
            .extend_from_slice(&[base, base + i, base + i + 1]);
    }
}

/// The ring of a superellipse `|x/a|^n + |z/b|^n = 1` around (0, `z`) in `segments` points,
/// going so that a wall from a lower ring up to an upper one faces out (at +Z it goes toward
/// +X).
fn ring(a: f32, b: f32, n: f32, z: f32, segments: usize) -> Vec<Vec2> {
    let power = |v: f32| v.signum() * v.abs().powf(2.0 / n);
    (0..segments)
        .map(|i| {
            let t = i as f32 / segments as f32 * std::f32::consts::TAU;
            Vec2::new(a * power(t.sin()), b * power(t.cos()) + z)
        })
        .collect()
}

/// A point of a ring at height `y`.
fn at(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// The wall between two rings of the same number of points (`lower` at `y0`, `upper` at `y1`),
/// facing out (`outward`) or in; `color` by segment.
fn wall(
    mesh: &mut MeshData,
    (y0, lower): (f32, &[Vec2]),
    (y1, upper): (f32, &[Vec2]),
    outward: bool,
    color: impl Fn(usize) -> [f32; 4],
) {
    let n = lower.len();
    for j in 0..n {
        let k = (j + 1) % n;
        let quad = [
            at(lower[j], y0),
            at(lower[k], y0),
            at(upper[k], y1),
            at(upper[j], y1),
        ];
        if outward {
            face(mesh, &quad, color(j));
        } else {
            face(mesh, &[quad[0], quad[3], quad[2], quad[1]], color(j));
        }
    }
}

/// The flat cap of a ring at height `y`, around `center`: facing up (`up`) or down.
fn cap(mesh: &mut MeshData, y: f32, points: &[Vec2], center: Vec2, up: bool, color: [f32; 4]) {
    let n = points.len();
    let middle = at(center, y);
    for j in 0..n {
        let (a, b) = (at(points[j], y), at(points[(j + 1) % n], y));
        // The ring goes counter-clockwise seen from above.
        if up {
            face(mesh, &[middle, a, b], color);
        } else {
            face(mesh, &[middle, b, a], color);
        }
    }
}

/// A box around `center`, `half` its half sizes, turned by `rotation`.
fn block(mesh: &mut MeshData, center: Vec3, half: Vec3, rotation: Quat, color: [f32; 4]) {
    let corner = |x: f32, y: f32, z: f32| center + rotation * (half * Vec3::new(x, y, z));
    let faces = [
        // +X, -X, +Y, -Y, +Z, -Z
        [(1., -1., 1.), (1., -1., -1.), (1., 1., -1.), (1., 1., 1.)],
        [
            (-1., -1., -1.),
            (-1., -1., 1.),
            (-1., 1., 1.),
            (-1., 1., -1.),
        ],
        [(-1., 1., 1.), (1., 1., 1.), (1., 1., -1.), (-1., 1., -1.)],
        [
            (-1., -1., -1.),
            (1., -1., -1.),
            (1., -1., 1.),
            (-1., -1., 1.),
        ],
        [(-1., -1., 1.), (1., -1., 1.), (1., 1., 1.), (-1., 1., 1.)],
        [
            (1., -1., -1.),
            (-1., -1., -1.),
            (-1., 1., -1.),
            (1., 1., -1.),
        ],
    ];
    for f in faces {
        let corners = f.map(|(x, y, z)| corner(x, y, z));
        face(mesh, &corners, color);
    }
}

/// A square beam from `a` to `b`, `half` its half thickness.
fn beam(mesh: &mut MeshData, a: Vec3, b: Vec3, half: f32, color: [f32; 4]) {
    let along = b - a;
    let rotation = Quat::from_rotation_arc(Vec3::Y, along.normalize_or(Vec3::Y));
    block(
        mesh,
        (a + b) / 2.0,
        Vec3::new(half, along.length() / 2.0 + half, half),
        rotation,
        color,
    );
}

/// A solid of revolution around the X axis: the `profile` (x, radius), walked along its
/// silhouette from its -X end to its +X end (so that its surface faces out), in `segments`
/// around; `color` by profile segment and segment around.
fn lathe(
    mesh: &mut MeshData,
    profile: &[(f32, f32)],
    segments: usize,
    color: impl Fn(usize, usize) -> [f32; 4],
) {
    let point = |(x, r): (f32, f32), j: usize| {
        let t = j as f32 / segments as f32 * std::f32::consts::TAU;
        Vec3::new(x, r * t.cos(), r * t.sin())
    };
    for (i, pair) in profile.windows(2).enumerate() {
        for j in 0..segments {
            // A quad with an end on the axle is a triangle (`face`).
            let quad = [
                point(pair[0], j),
                point(pair[0], j + 1),
                point(pair[1], j + 1),
                point(pair[1], j),
            ];
            face(mesh, &quad, color(i, j));
        }
    }
}

// --- The home-made bodies: the wicker basket and the bathtub --------------------------------------

/// The home-made bodies' wheels: their hubs (the middle of each wheel's inner face) just
/// outside the body, front and back; and the driver's seat, toward the back, like the soap box.
const HUB_X: f32 = 0.47;
const HUB_Z: [f32; 2] = [-0.55, 0.6];
/// Height of the hubs above the road at rest (the marshmallow tyres' radius: the suspension then
/// puts every wheel on the road with its own).
const HUB_HEIGHT: f32 = 0.294;
/// The driver's seat in them: the floor's height above the road (a little higher than the soap
/// box's seat, 0.43 m: the walls stand around the driver) and how far back.
const SEAT_HEIGHT: f32 = 0.48;
const SEAT_Z: f32 = 0.2;

/// A height above the road, in the pivot's space.
fn h(height: f32) -> f32 {
    height - GRID_HEIGHT
}

fn home_hubs() -> [Vec3; 4] {
    let y = h(HUB_HEIGHT);
    [
        Vec3::new(-HUB_X, y, HUB_Z[0]),
        Vec3::new(HUB_X, y, HUB_Z[0]),
        Vec3::new(-HUB_X, y, HUB_Z[1]),
        Vec3::new(HUB_X, y, HUB_Z[1]),
    ]
}

/// The short axles from the body's side out to each hub.
fn stub_axles(mesh: &mut MeshData, inside: f32, color: [f32; 4]) {
    for hub in home_hubs() {
        let from = Vec3::new(hub.x.signum() * inside, hub.y, hub.z);
        beam(mesh, from, hub, 0.035, color);
    }
}

/// Points around the wall of the bodies (the basket's weave needs more).
const AROUND: usize = 28;
const WEAVE_AROUND: usize = 36;
/// Rows of the basket's weave.
const WEAVE_ROWS: usize = 8;

/// The wicker basket: an oval basket woven in two shades of the kart's color, its rim braided in
/// natural wicker, a red and white gingham cushion inside (the seat), a handle at each end.
pub fn wicker_basket() -> BodyData {
    const N: f32 = 2.4;
    let mut paint = MeshData::default();
    let mut trim = MeshData::default();
    // The woven wall, from the bottom up: (height, half width, half length), rounding out.
    let rows: Vec<(f32, f32, f32)> = (0..=WEAVE_ROWS)
        .map(|k| {
            let t = k as f32 / WEAVE_ROWS as f32;
            let round = 1.0 - (1.0 - t) * (1.0 - t);
            (0.2 + 0.42 * t, 0.36 + 0.08 * round, 0.8 + 0.12 * round)
        })
        .collect();
    let ring_of = |(_, a, b): (f32, f32, f32)| ring(a, b, N, 0.0, WEAVE_AROUND);
    for (r, pair) in rows.windows(2).enumerate() {
        let (lo, hi) = (ring_of(pair[0]), ring_of(pair[1]));
        // The weave: a checker of two shades (over and under the stakes).
        wall(
            &mut paint,
            (h(pair[0].0), &lo),
            (h(pair[1].0), &hi),
            true,
            |j| grey(if (j + r) % 2 == 0 { 1.0 } else { 0.62 }),
        );
    }
    // Its woven bottom.
    let bottom = ring_of(rows[0]);
    cap(
        &mut paint,
        h(rows[0].0),
        &bottom,
        Vec2::ZERO,
        false,
        grey(0.5),
    );

    // The braided rim: out over the top of the wall, its top, and down inside.
    let wicker = [srgb(0.86, 0.66, 0.38), srgb(0.7, 0.5, 0.27)];
    let top = rows[WEAVE_ROWS];
    let rim: [(f32, f32, f32); 4] = [
        (top.0, top.1, top.2),
        (top.0 + 0.04, top.1 + 0.025, top.2 + 0.03),
        (top.0 + 0.09, top.1 + 0.015, top.2 + 0.02),
        (top.0 + 0.1, top.1 - 0.025, top.2 - 0.025),
    ];
    for (r, pair) in rim.windows(2).enumerate() {
        let (lo, hi) = (ring_of(pair[0]), ring_of(pair[1]));
        wall(
            &mut trim,
            (h(pair[0].0), &lo),
            (h(pair[1].0), &hi),
            true,
            |j| wicker[(j / 2 + r) % 2],
        );
    }
    // Inside: the wicker wall down to the cushion.
    let inner_top = rim[3];
    let seat_height = SEAT_HEIGHT;
    let inside: [(f32, f32, f32); 3] = [
        inner_top,
        (0.6, inner_top.1 - 0.02, inner_top.2 - 0.03),
        (seat_height, inner_top.1 - 0.045, inner_top.2 - 0.065),
    ];
    let inner_wicker = [srgb(0.78, 0.58, 0.32), srgb(0.64, 0.45, 0.24)];
    for (r, pair) in inside.windows(2).enumerate() {
        let (lo, hi) = (ring_of(pair[1]), ring_of(pair[0]));
        wall(
            &mut trim,
            (h(pair[1].0), &lo),
            (h(pair[0].0), &hi),
            false,
            |j| inner_wicker[(j + r) % 2],
        );
    }
    // The cushion: a gingham square of red and white, puffed up in its middle.
    let floor = ring_of(inside[2]);
    gingham(&mut trim, h(seat_height), &floor);

    // A handle at each end: an arch of wicker over the rim.
    for end in [-1.0f32, 1.0] {
        let z = end * (top.2 - 0.02);
        let rise = 0.2;
        let points: Vec<Vec3> = (0..=6)
            .map(|i| {
                let t = i as f32 / 6.0 * std::f32::consts::PI;
                Vec3::new(
                    -0.2 * t.cos(),
                    h(top.0 + 0.06) + rise * t.sin(),
                    z - end * 0.03 * t.sin(),
                )
            })
            .collect();
        for (i, pair) in points.windows(2).enumerate() {
            beam(&mut trim, pair[0], pair[1], 0.028, wicker[i % 2]);
        }
    }
    stub_axles(&mut trim, 0.35, srgb(0.3, 0.27, 0.25));
    BodyData {
        paint,
        trim,
        hubs: home_hubs(),
        seat: Vec3::new(0.0, h(seat_height), SEAT_Z),
    }
}

/// A gingham cushion over `ring` at `y`: red and white squares, its middle puffed up.
fn gingham(mesh: &mut MeshData, y: f32, ring: &[Vec2]) {
    let colors = [
        srgb(0.95, 0.94, 0.9),
        srgb(0.93, 0.55, 0.55),
        srgb(0.84, 0.22, 0.25),
    ];
    let (lo, hi) = ring.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), p| {
        (lo.min(*p), hi.max(*p))
    });
    // A grid over the ring's box, clipped to the ring (its cells inside), and its rim.
    const CELLS: usize = 6;
    let size = (hi - lo) / CELLS as f32;
    let inside = |p: Vec2| {
        let q = (p - (lo + hi) / 2.0) / ((hi - lo) / 2.0);
        q.x.abs().powf(2.4) + q.y.abs().powf(2.4) <= 1.0
    };
    let puff = |p: Vec2| {
        let q = (p - (lo + hi) / 2.0) / ((hi - lo) / 2.0);
        y + 0.04 * (1.0 - q.length_squared()).max(0.0)
    };
    for i in 0..CELLS {
        for j in 0..CELLS {
            let corners = [
                lo + size * Vec2::new(i as f32, j as f32),
                lo + size * Vec2::new(i as f32 + 1.0, j as f32),
                lo + size * Vec2::new(i as f32 + 1.0, j as f32 + 1.0),
                lo + size * Vec2::new(i as f32, j as f32 + 1.0),
            ];
            if !corners.iter().all(|c| inside(*c)) {
                continue;
            }
            let color = colors[(i % 2) + (j % 2)];
            let points = corners.map(|c| at(c, puff(c)));
            face(mesh, &[points[0], points[3], points[2], points[1]], color);
        }
    }
    // Under the grid's cells, the whole floor in the cushion's red (the cells cut off at its
    // edge leave no hole).
    cap(mesh, y - 0.005, ring, (lo + hi) / 2.0, true, colors[2]);
}

/// The bathtub: a clawfoot tub, its outside painted in the kart's color, its rolled rim and its
/// inside in white enamel, golden feet, a chrome tap at the front and a rubber duck on the rim.
pub fn bathtub() -> BodyData {
    const N: f32 = 2.6;
    let mut paint = MeshData::default();
    let mut trim = MeshData::default();
    let ring_of = |(_, a, b): (f32, f32, f32)| ring(a, b, N, 0.0, AROUND);
    // The painted outside, from the bottom up, flaring out.
    let outside: [(f32, f32, f32); 5] = [
        (0.22, 0.32, 0.78),
        (0.27, 0.37, 0.83),
        (0.4, 0.41, 0.88),
        (0.56, 0.43, 0.91),
        (0.7, 0.44, 0.925),
    ];
    for (r, pair) in outside.windows(2).enumerate() {
        let (lo, hi) = (ring_of(pair[0]), ring_of(pair[1]));
        // A slight gradient: darker toward the bottom.
        let shade = [0.72, 0.85, 0.95, 1.0][r];
        wall(
            &mut paint,
            (h(pair[0].0), &lo),
            (h(pair[1].0), &hi),
            true,
            |_| grey(shade),
        );
    }
    let bottom = ring_of(outside[0]);
    cap(
        &mut paint,
        h(outside[0].0),
        &bottom,
        Vec2::ZERO,
        false,
        grey(0.55),
    );

    // The rolled rim, white enamel.
    let enamel = srgb(0.97, 0.97, 0.95);
    let shadowed = srgb(0.85, 0.87, 0.88);
    let top = outside[4];
    let rim: [(f32, f32, f32); 5] = [
        (top.0, top.1, top.2),
        (top.0 + 0.045, top.1 + 0.025, top.2 + 0.03),
        (top.0 + 0.09, top.1 + 0.015, top.2 + 0.02),
        (top.0 + 0.1, top.1 - 0.01, top.2 - 0.01),
        (top.0 + 0.09, top.1 - 0.04, top.2 - 0.04),
    ];
    for pair in rim.windows(2) {
        let (lo, hi) = (ring_of(pair[0]), ring_of(pair[1]));
        wall(
            &mut trim,
            (h(pair[0].0), &lo),
            (h(pair[1].0), &hi),
            true,
            |_| enamel,
        );
    }
    // Inside: the enamel down to the floor (the seat).
    let seat_height = SEAT_HEIGHT;
    let lip = rim[4];
    let inside: [(f32, f32, f32); 4] = [
        lip,
        (0.66, lip.1 - 0.015, lip.2 - 0.02),
        (0.54, lip.1 - 0.05, lip.2 - 0.07),
        (seat_height, lip.1 - 0.1, lip.2 - 0.15),
    ];
    for (r, pair) in inside.windows(2).enumerate() {
        let (lo, hi) = (ring_of(pair[1]), ring_of(pair[0]));
        let color = if r == 0 { enamel } else { shadowed };
        wall(
            &mut trim,
            (h(pair[1].0), &lo),
            (h(pair[0].0), &hi),
            false,
            |_| color,
        );
    }
    let floor = ring_of(inside[3]);
    cap(
        &mut trim,
        h(seat_height),
        &floor,
        Vec2::ZERO,
        true,
        shadowed,
    );
    // The plughole, near the front.
    let hole = ring(0.035, 0.035, 2.0, -0.45, 8);
    cap(
        &mut trim,
        h(seat_height) + 0.004,
        &hole,
        Vec2::new(0.0, -0.45),
        true,
        srgb(0.55, 0.57, 0.6),
    );

    // Four golden lion's feet under it.
    let gold = srgb(0.96, 0.74, 0.25);
    let dark_gold = srgb(0.78, 0.55, 0.15);
    for x in [-0.22f32, 0.22] {
        for z in [-0.62f32, 0.62] {
            let leg = Vec3::new(x, h(0.24), z);
            let paw = Vec3::new(x * 1.25, h(0.06), z * 1.06);
            beam(&mut trim, leg, paw, 0.04, gold);
            block(
                &mut trim,
                paw + Vec3::Y * 0.01,
                Vec3::new(0.06, 0.04, 0.07),
                Quat::IDENTITY,
                dark_gold,
            );
        }
    }

    // The tap, at the front: its pipe up from the rim, its spout over the tub, two knobs.
    let chrome = srgb(0.86, 0.88, 0.92);
    let front = -(top.2 - 0.02);
    let foot = Vec3::new(0.0, h(top.0 + 0.1), front);
    let bend = Vec3::new(0.0, h(top.0 + 0.3), front);
    let spout = Vec3::new(0.0, h(top.0 + 0.3), front + 0.16);
    beam(&mut trim, foot, bend, 0.025, chrome);
    beam(&mut trim, bend, spout, 0.025, chrome);
    beam(&mut trim, spout, spout - Vec3::Y * 0.06, 0.03, chrome);
    for (x, color) in [
        (-0.1f32, srgb(0.3, 0.55, 0.95)),
        (0.1, srgb(0.95, 0.3, 0.3)),
    ] {
        let knob = Vec3::new(x, h(top.0 + 0.14), front + 0.01);
        block(&mut trim, knob, Vec3::splat(0.035), Quat::IDENTITY, color);
    }

    // The rubber duck, on the rim at the back.
    let yellow = srgb(1.0, 0.85, 0.2);
    let duck = Vec3::new(0.3, h(top.0 + 0.1), top.2 - 0.2);
    block(
        &mut trim,
        duck + Vec3::Y * 0.05,
        Vec3::new(0.06, 0.05, 0.08),
        Quat::IDENTITY,
        yellow,
    );
    block(
        &mut trim,
        duck + Vec3::new(0.0, 0.13, -0.04),
        Vec3::splat(0.04),
        Quat::IDENTITY,
        yellow,
    );
    block(
        &mut trim,
        duck + Vec3::new(0.0, 0.12, -0.095),
        Vec3::new(0.025, 0.012, 0.02),
        Quat::IDENTITY,
        srgb(1.0, 0.5, 0.1),
    );

    stub_axles(&mut trim, 0.36, srgb(0.3, 0.27, 0.25));
    BodyData {
        paint,
        trim,
        hubs: home_hubs(),
        seat: Vec3::new(0.0, h(seat_height), SEAT_Z),
    }
}

// --- The thread spools ------------------------------------------------------------------------

/// The thread spools: radius of the flanges, width, radius of the thread, thickness of a flange
/// (m).
const SPOOL_RADIUS: f32 = 0.22;
const SPOOL_WIDTH: f32 = 0.24;
const SPOOL_THREAD: f32 = 0.165;
const SPOOL_FLANGE: f32 = 0.035;
const SPOOL_HOLE: f32 = 0.05;
const SPOOL_SEGMENTS: usize = 14;

/// The thread spools: two wooden flanges around a coil of pink thread, smaller than the other
/// wheels.
pub fn spools() -> WheelData {
    let (r, w, t, f, hole) = (
        SPOOL_RADIUS,
        SPOOL_WIDTH,
        SPOOL_THREAD,
        SPOOL_FLANGE,
        SPOOL_HOLE,
    );
    // From the outer face (-X) to the hub's face, along the silhouette.
    let profile = [
        (-w, 0.0),
        (-w, hole),
        (-w, r),
        (-(w - f), r),
        (-(w - f), t),
        (-w / 2.0, t + 0.012),
        (-f, t),
        (-f, r),
        (0.0, r),
        (0.0, hole),
        (0.0, 0.0),
    ];
    let wood = srgb(0.87, 0.69, 0.46);
    let edge = srgb(0.72, 0.53, 0.33);
    let dark = srgb(0.3, 0.2, 0.13);
    // Issue #176: one pink all around (stripes all around would seem to turn backward at speed):
    // its band shows the turn (`paint_band`).
    let thread = srgb(0.96, 0.47, 0.62);
    let mut mesh = MeshData::default();
    lathe(&mut mesh, &profile, SPOOL_SEGMENTS, |i, _| match i {
        0 | 9 => dark,
        1 | 8 => wood,
        2 | 7 => edge,
        3 | 6 => wood,
        _ => thread,
    });
    WheelData {
        mesh,
        radii: [SPOOL_RADIUS; 2],
    }
}

// --- The feather ------------------------------------------------------------------------------

/// The feather's half span (m, like the pillowcase's) and its samples along it.
const FEATHER_HALF_SPAN: f32 = 2.25;
const FEATHER_SAMPLES: usize = 18;

/// Where the feather's shaft is at `x` (its middle, in the wing's space): arched up in its middle,
/// its ends swept back; the struts reach its underside at ±1.25 m.
fn feather_shaft(x: f32) -> Vec3 {
    let t = x / FEATHER_HALF_SPAN;
    Vec3::new(x, 1.9 + 0.1 * (1.0 - t * t), 0.05 + 0.25 * t * t)
}

/// How wide the feather's vane is at `t` (0 at the quill's end, 1 at its tip): bare at the quill,
/// widest a little past its middle, rounded at the tip.
fn feather_width(t: f32) -> f32 {
    if t < 0.06 {
        return 0.0;
    }
    let rise = (std::f32::consts::FRAC_PI_2 * ((t - 0.06) / 0.35).min(1.0)).sin();
    let tip = if t > 0.6 {
        (1.0 - ((t - 0.6) / 0.4).powi(2)).max(0.0).sqrt()
    } else {
        1.0
    };
    rise * tip
}

/// The feather: a long white feather across the kart, its quill on the left, its tip pale blue,
/// its back edge notched between barbs. Both faces drawn (seen from above and below).
pub fn feather() -> MeshData {
    let mut mesh = MeshData::default();
    let thickness = 0.012;
    let white = [srgb(0.98, 0.98, 0.96), srgb(0.9, 0.91, 0.92)];
    let blue = [srgb(0.62, 0.8, 0.96), srgb(0.55, 0.72, 0.9)];
    let quill = srgb(0.95, 0.9, 0.78);
    let samples: Vec<(f32, Vec3, f32, f32)> = (0..=FEATHER_SAMPLES)
        .map(|i| {
            let t = i as f32 / FEATHER_SAMPLES as f32;
            let x = -FEATHER_HALF_SPAN + 2.0 * FEATHER_HALF_SPAN * t;
            let width = feather_width(t);
            // The barbs: every other sample's back edge a little shorter.
            let notch = if i % 2 == 1 { 0.82 } else { 1.0 };
            (t, feather_shaft(x), 0.42 * width, 0.8 * width * notch)
        })
        .collect();
    // Across the vane, from the shaft out to an edge: how far (a fraction of its width) and how
    // much lower (m, per m of width): the vane curves down (a feather's camber), most at the back,
    // which the chasing camera then sees.
    const ACROSS: [f32; 3] = [0.0, 0.5, 1.0];
    const DROOP: [f32; 2] = [0.15, 0.45];
    let vane = |s: Vec3, width: f32, side: usize, f: f32| {
        let along = if side == 0 { -Vec3::Z } else { Vec3::Z };
        s + along * width * f - Vec3::Y * DROOP[side] * width * f * f
    };
    for (i, pair) in samples.windows(2).enumerate() {
        let (t0, s0, front0, back0) = pair[0];
        let (_, s1, front1, back1) = pair[1];
        let palette = if t0 > 0.72 { blue } else { white };
        let color = palette[i % 2];
        for (side, (w0, w1)) in [(front0, front1), (back0, back1)].into_iter().enumerate() {
            for across in ACROSS.windows(2) {
                let (inner0, inner1) =
                    (vane(s0, w0, side, across[0]), vane(s1, w1, side, across[0]));
                let (outer0, outer1) =
                    (vane(s0, w0, side, across[1]), vane(s1, w1, side, across[1]));
                // Counter-clockwise seen from above: the front vane's quad goes the other way.
                let quad = if side == 0 {
                    [inner0, inner1, outer1, outer0]
                } else {
                    [inner0, outer0, outer1, inner1]
                };
                for up in [true, false] {
                    let lift = Vec3::Y * thickness * if up { 1.0 } else { -1.0 };
                    let q = quad.map(|p| p + lift);
                    if up {
                        face(&mut mesh, &q, color);
                    } else {
                        face(&mut mesh, &[q[0], q[3], q[2], q[1]], color);
                    }
                }
            }
        }
        // The shaft: a thin cream ridge along it, over both faces.
        beam(
            &mut mesh,
            s0 + Vec3::Y * thickness,
            s1 + Vec3::Y * thickness,
            0.018,
            quill,
        );
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::WHEEL_ANCHORS;

    /// The box around a mesh.
    fn bounds(mesh: &MeshData) -> (Vec3, Vec3) {
        mesh.positions
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            })
    }

    /// Each triangle: its corners and its normal.
    fn triangles(mesh: &MeshData) -> impl Iterator<Item = ([Vec3; 3], Vec3)> + '_ {
        mesh.indices.as_chunks::<3>().0.iter().map(|t| {
            let p = t.map(|i| Vec3::from(mesh.positions[i as usize]));
            (p, Vec3::from(mesh.normals[t[0] as usize]))
        })
    }

    /// The highest point of `mesh` right under (or above) the vertical line through `at`, below
    /// `below` (None: nothing there).
    fn surface_under(mesh: &MeshData, at: Vec2, below: f32) -> Option<f32> {
        triangles(mesh)
            .filter_map(|(p, _)| {
                // Barycentric coordinates in the XZ plane.
                let [a, b, c] = p.map(|v| v.xz());
                let d = (b - a).perp_dot(c - a);
                if d.abs() < 1e-9 {
                    return None;
                }
                let u = (at - a).perp_dot(c - a) / d;
                let v = (b - a).perp_dot(at - a) / d;
                (u >= 0.0 && v >= 0.0 && u + v <= 1.0)
                    .then(|| p[0].y + u * (p[1].y - p[0].y) + v * (p[2].y - p[0].y))
            })
            .filter(|y| *y <= below)
            .reduce(f32::max)
    }

    fn home_bodies() -> [(&'static str, BodyData); 2] {
        [("basket", wicker_basket()), ("bathtub", bathtub())]
    }

    /// Issue #73: the wicker basket and the bathtub are bodies of a kart: within the kart's box
    /// (2 m long like the collider, no wider than the wheels), their shell to paint (grey, 1 at
    /// its lightest, some of it in shade) and the rest in their own colors, a few hundred
    /// triangles; their wheels' hubs where `WHEEL_ANCHORS` has them (left on the left, front at
    /// the front), on the road with the marshmallow tyres; nothing of them under the road.
    #[test]
    fn the_home_made_bodies_are_karts_to_paint() {
        for (name, body) in home_bodies() {
            assert!(body.paint.triangle_count() > 100, "{name}");
            assert!(body.trim.triangle_count() > 100, "{name}");
            let triangles = body.paint.triangle_count() + body.trim.triangle_count();
            assert!(triangles < 2500, "{name}: {triangles} triangles");
            assert!(
                body.paint
                    .colors
                    .iter()
                    .all(|c| c[0] == c[1] && c[1] == c[2] && c[3] == 1.0),
                "{name}: the paint is grey"
            );
            let lightest = body.paint.colors.iter().map(|c| c[0]).fold(0.0, f32::max);
            let darkest = body.paint.colors.iter().map(|c| c[0]).fold(1.0, f32::min);
            assert_eq!(lightest, 1.0, "{name}");
            assert!(darkest < 0.8, "{name}: some of it in shade");
            assert!(
                body.trim
                    .colors
                    .iter()
                    .any(|c| c[0] != c[1] || c[1] != c[2]),
                "{name}: the trim has its colors"
            );
            for mesh in [&body.paint, &body.trim] {
                assert_eq!(mesh.positions.len(), mesh.normals.len());
                assert_eq!(mesh.positions.len(), mesh.colors.len());
                assert!(
                    mesh.indices
                        .iter()
                        .all(|i| (*i as usize) < mesh.positions.len())
                );
                let (lo, hi) = bounds(mesh);
                assert!(lo.y >= -GRID_HEIGHT - 1e-4, "{name}: under the road {lo}");
                assert!(lo.z >= -1.0 && hi.z <= 1.0, "{name}: {lo} {hi}");
                assert!(hi.y < 0.6, "{name}: {hi}");
            }
            // The shell no wider than where the wheels start.
            let (lo, hi) = bounds(&body.paint);
            assert!(lo.x > -HUB_X && hi.x < HUB_X, "{name}: {lo} {hi}");
            for (hub, anchor) in body.hubs.iter().zip(WHEEL_ANCHORS) {
                assert_eq!(hub.x.signum(), anchor.x.signum(), "{name}");
                assert_eq!(hub.z.signum(), anchor.z.signum(), "{name}");
                assert!((hub.y - (HUB_HEIGHT - GRID_HEIGHT)).abs() < 1e-5);
            }
            let turns = body.wheel_turns();
            assert_eq!(turns[0], Quat::IDENTITY);
            assert_eq!(turns[1], Quat::from_rotation_y(std::f32::consts::PI));
        }
    }

    /// Issue #73: the driver sits in each home-made body: its seat on the body's floor (the
    /// cushion of the basket, the bottom of the tub) near the soap box's height, toward the back,
    /// in the middle; above it, room for the animal (nothing of the body in the column where it
    /// sits, up past the rim), and the rim around it (it sits in, not on, the body).
    #[test]
    fn the_driver_sits_on_the_floor_of_each_home_made_body() {
        for (name, body) in home_bodies() {
            let seat = body.seat;
            assert!(seat.x == 0.0 && (0.0..0.4).contains(&seat.z), "{name}");
            assert!(
                (seat.y - (SEAT_HEIGHT - GRID_HEIGHT)).abs() < 1e-5,
                "{name}"
            );
            let floor = surface_under(&body.trim, seat.xz(), seat.y + 0.1).unwrap();
            assert!(
                (floor - seat.y).abs() < 0.05,
                "{name}: the floor at {floor}, the seat at {}",
                seat.y
            );
            // Room for the animal: nothing between the floor and 1 m above it, 0.3 m around.
            for (dx, dz) in [(0.0, 0.0), (0.3, 0.0), (-0.3, 0.0), (0.0, 0.3), (0.0, -0.3)] {
                let at = seat.xz() + Vec2::new(dx, dz);
                for mesh in [&body.paint, &body.trim] {
                    let over = triangles(mesh).any(|(p, _)| {
                        let [a, b, c] = p.map(|v| v.xz());
                        let d = (b - a).perp_dot(c - a);
                        if d.abs() < 1e-9 {
                            return false;
                        }
                        let u = (at - a).perp_dot(c - a) / d;
                        let v = (b - a).perp_dot(at - a) / d;
                        u >= 0.0
                            && v >= 0.0
                            && u + v <= 1.0
                            && p.iter().all(|q| q.y > seat.y + 0.06 && q.y < seat.y + 1.0)
                    });
                    assert!(!over, "{name}: something over the seat at {at}");
                }
            }
            // The rim higher than the seat on every side.
            let (lo, hi) = bounds(&body.trim);
            assert!(hi.y > seat.y + 0.25, "{name}: {hi}");
            assert!(lo.x < -0.3 && hi.x > 0.3, "{name}");
        }
    }

    /// The walls face out, the insides in: every outward face of a wall leans away from the
    /// body's middle axis.
    #[test]
    fn the_walls_face_out() {
        let mut mesh = MeshData::default();
        let (lo, hi) = (ring(0.4, 0.8, 2.4, 0.0, 16), ring(0.45, 0.9, 2.4, 0.0, 16));
        wall(&mut mesh, (0.0, &lo), (0.5, &hi), true, |_| grey(1.0));
        let mut inner = MeshData::default();
        wall(&mut inner, (0.0, &lo), (0.5, &hi), false, |_| grey(1.0));
        for ((p, n), (_, m)) in triangles(&mesh).zip(triangles(&inner)) {
            let middle = (p[0] + p[1] + p[2]) / 3.0;
            assert!(n.xz().dot(middle.xz()) > 0.0, "{n} at {middle}");
            assert!(m.xz().dot(middle.xz()) < 0.0);
        }
        let mut top = MeshData::default();
        cap(&mut top, 0.5, &hi, Vec2::ZERO, true, grey(1.0));
        assert!(triangles(&top).all(|(_, n)| n.y > 0.99));
        let mut under = MeshData::default();
        cap(&mut under, 0.0, &lo, Vec2::ZERO, false, grey(1.0));
        assert!(triangles(&under).all(|(_, n)| n.y < -0.99));
        // A closed box faces out on every side.
        let mut cube = MeshData::default();
        block(
            &mut cube,
            Vec3::ONE,
            Vec3::splat(0.5),
            Quat::from_rotation_y(0.3),
            grey(1.0),
        );
        for (p, n) in triangles(&cube) {
            let middle = (p[0] + p[1] + p[2]) / 3.0;
            assert!(n.dot(middle - Vec3::ONE) > 0.0);
        }
    }

    /// The thread spools: a wheel around its hub (from it out toward -X), round, smaller than the
    /// marshmallow tyres (0.29 m) and the tractor's; its faces out; wood and pink thread.
    #[test]
    fn the_thread_spools_are_small_round_wheels() {
        let spools = spools();
        assert_eq!(spools.radii, [SPOOL_RADIUS; 2]);
        assert!(spools.radii[0] < 0.25 && TRACTOR_RADII.iter().all(|r| *r > 0.3));
        assert!((0..4).all(|i| spools.scale(i) == 1.0));
        let mesh = &spools.mesh;
        let (lo, hi) = bounds(mesh);
        assert!(
            (lo.x + SPOOL_WIDTH).abs() < 1e-5 && hi.x.abs() < 1e-5,
            "{lo} {hi}"
        );
        for p in &mesh.positions {
            let r = Vec2::new(p[1], p[2]).length();
            assert!(r <= SPOOL_RADIUS + 1e-4, "{r}");
        }
        assert!((hi.y - SPOOL_RADIUS).abs() < 1e-3 && (lo.y + SPOOL_RADIUS).abs() < 1e-3);
        // Faces out: the rims and the thread away from the axle, the flanges' faces along it.
        for (p, n) in triangles(mesh) {
            let middle = (p[0] + p[1] + p[2]) / 3.0;
            let radial = Vec2::new(middle.y, middle.z);
            let out = n.yz().dot(radial) > 1e-6 || n.x.abs() > 0.99;
            assert!(out, "{n} at {middle}");
            // The outer face of the outer flange faces -X, the hub's face +X.
            if (middle.x + SPOOL_WIDTH).abs() < 1e-4 {
                assert!(n.x < -0.99);
            }
            if middle.x.abs() < 1e-4 {
                assert!(n.x > 0.99);
            }
        }
        let pink = srgb(0.96, 0.47, 0.62);
        assert!(mesh.colors.contains(&pink));
    }

    /// Issue #176: the band of a set of wheels: across its tread and its faces (from its hub to
    /// its rim), around one angle only (so the wheel looks different at each angle of its turn),
    /// its faces still facing out, the rest of the wheel untouched.
    #[test]
    fn a_band_across_the_wheel_shows_its_turn() {
        let before = spools();
        let mut banded = before.clone();
        let band = WHEEL_BANDS[WheelLook::Spools.index()];
        paint_band(&mut banded.mesh, band);
        let total = triangles(&banded.mesh).count();
        assert_eq!(total, triangles(&before.mesh).count());
        let painted: Vec<([Vec3; 3], Vec3)> = banded
            .mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .zip(triangles(&banded.mesh))
            .filter(|(t, _)| t.iter().all(|i| banded.mesh.colors[*i as usize] == band))
            .map(|(_, triangle)| triangle)
            .collect();
        assert!(
            !painted.is_empty() && painted.len() * 6 < total,
            "{} of {total}",
            painted.len()
        );
        let (mut tread, mut outer) = (false, false);
        for (corners, n) in &painted {
            let middle = (corners[0] + corners[1] + corners[2]) / 3.0;
            let (angle, radius) = around_axle(middle);
            if radius > 0.02 {
                assert!(angle.abs() < 0.6, "around one angle: {angle}");
            }
            tread |= n.yz().length() > 0.9 && radius > SPOOL_THREAD - 0.01;
            outer |= n.x < -0.99 && (middle.x + SPOOL_WIDTH).abs() < 1e-4;
        }
        assert!(tread && outer, "across the tread and the outer face");
        // Nothing else changes: the same shape, the band's corners its own.
        let (lo, hi) = bounds(&banded.mesh);
        assert_eq!((lo, hi), bounds(&before.mesh));
        assert!(before.mesh.colors.iter().all(|c| *c != band));
        assert!(banded.mesh.colors.contains(&srgb(0.96, 0.47, 0.62)));
        // A mesh without its colors is left as it is.
        let mut bare = before.mesh.clone();
        bare.colors.clear();
        paint_band(&mut bare, band);
        assert!(bare.colors.is_empty() && bare.positions.len() == before.mesh.positions.len());
    }

    /// The feather: across the kart like the pillowcase (4.5 m), over the driver (its back vane
    /// curving down, never below 1.5 m), its shaft on the struts' tops at ±1.25 m (they hold it),
    /// drawn from above and from below, white and pale blue at the tip, no wider than its 1.3 m.
    #[test]
    fn the_feather_spans_the_kart_over_the_driver() {
        let mesh = feather();
        let (lo, hi) = bounds(&mesh);
        assert!(
            (hi.x - lo.x - 2.0 * FEATHER_HALF_SPAN).abs() < 0.05,
            "{lo} {hi}"
        );
        assert!(lo.y > 1.5 && hi.y < 2.1, "{lo} {hi}");
        assert!(hi.z - lo.z < 1.6, "{lo} {hi}");
        let up = triangles(&mesh).filter(|(_, n)| n.y > 0.5).count();
        let down = triangles(&mesh).filter(|(_, n)| n.y < -0.5).count();
        assert!(up > 30 && down > 30 && (up as i32 - down as i32).abs() < 40);
        for x in [-1.25f32, 1.25] {
            let shaft = feather_shaft(x);
            assert!((shaft.y - 1.97).abs() < 0.03, "{shaft}");
            assert!((shaft.z - 0.15).abs() < 0.05, "{shaft}");
        }
        assert!(mesh.colors.contains(&srgb(0.98, 0.98, 0.96)));
        assert!(mesh.colors.contains(&srgb(0.62, 0.8, 0.96)));
        // Bare at the quill, widest past the middle, rounded at the tip.
        assert_eq!(feather_width(0.0), 0.0);
        assert!((feather_width(0.5) - 1.0).abs() < 1e-5);
        assert!(feather_width(0.95) < 0.6 && feather_width(1.0) < 1e-3);
    }

    /// Each part's look is of its kind; the defaults are the looks of before (the Car Kit kart, its
    /// wheels, the pillowcase).
    #[test]
    fn each_look_has_its_kind() {
        use PartLook::*;
        let all = [
            WickerBasket,
            SoapBox,
            Bathtub,
            Spools,
            Marshmallow,
            Tractor,
            Pillowcase,
            Feather,
        ];
        for look in all {
            let kinds = [
                look.body().is_some(),
                look.wheels().is_some(),
                look.wing().is_some(),
            ];
            assert_eq!(kinds.iter().filter(|k| **k).count(), 1, "{look:?}");
        }
        assert_eq!(
            BodyLook::ALL.map(|b| b.index()),
            [0, 1, 2],
            "the index of each body"
        );
        assert_eq!(WheelLook::ALL.map(|w| w.index()), [0, 1, 2]);
        assert_eq!(WingLook::ALL.map(|w| w.index()), [0, 1]);
        let default = PartLooks::default();
        assert_eq!(default.body, BodyLook::SoapBox);
        assert_eq!(default.wheels, WheelLook::Marshmallow);
        assert_eq!(default.wing, WingLook::Pillowcase);
        assert_eq!(WheelData::axle(0), 0);
        assert_eq!(WheelData::axle(1), 0);
        assert_eq!(WheelData::axle(2), 1);
        assert_eq!(WheelData::axle(3), 1);
        // One mesh for a set, scaled down for smaller front wheels.
        let set = WheelData {
            mesh: MeshData::default(),
            radii: [0.3, 0.4],
        };
        assert_eq!(
            [set.scale(0), set.scale(1), set.scale(3)],
            [0.75, 0.75, 1.0]
        );
        assert_eq!([set.radius(1), set.radius(2)], [0.3, 0.4]);
    }
}
