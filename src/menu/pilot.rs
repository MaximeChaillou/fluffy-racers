//! The « Pilote » screen (issue #98), pure: its lines, its texts, the grid of the characters, the
//! stat bars, the soft loop of the driver in the 3D preview and where the preview's camera stands.
//! Its state and its keys are `characters::SelectionScreen`'s (headless); its tree is
//! `pilot_screen.rs`, its 3D preview and its portraits `preview.rs`.
//!
//! **The soft loop** ([`soft_pose`]): the driver hops in its seat, squashes when it lands and
//! stretches up, and waddles from side to side, one side a hop. Animation constants, like the
//! other animations of the screens (`anim.rs`), not tuning values of the gameplay.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::characters::{Roster, SelectionScreen, WeightClass};
use crate::kart::{KeyboardLayout, MAX_LEVEL, display_level};

/// The screen's title.
pub const TITLE: &str = "Pilote";

/// The label of a line (`SelectionScreen::row`).
pub fn row_label(row: usize) -> &'static str {
    match row {
        0 => "Personnage",
        1 => "Kart",
        2 => "Roues",
        3 => "Aile",
        SelectionScreen::COLOR_ROW => "Couleur",
        _ => "Pseudo",
    }
}

/// The label of a line of the character grid: its weight class.
pub fn class_label(class: WeightClass) -> &'static str {
    match class {
        WeightClass::Light => "Légers",
        WeightClass::Medium => "Moyens",
        WeightClass::Heavy => "Lourds",
    }
}

/// The two buttons under the stats: keep the choice, or leave it.
pub const CONFIRM_LABEL: &str = "Valider";
pub const BACK_LABEL: &str = "Retour";

/// The columns of the character grid: as many characters as the biggest weight class has.
pub const GRID_COLUMNS: usize = 3;

/// The character grid: one line per weight class (light, medium, heavy), their characters in the
/// order of the file (roster indices).
pub fn character_grid(roster: &Roster) -> [Vec<usize>; 3] {
    WeightClass::ALL.map(|class| roster.characters_of(class).collect())
}

/// Where the character `index` is in the grid: (line, column).
pub fn grid_cell(roster: &Roster, index: usize) -> (usize, usize) {
    for (line, characters) in character_grid(roster).iter().enumerate() {
        if let Some(column) = characters.iter().position(|c| *c == index) {
            return (line, column);
        }
    }
    (0, 0)
}

/// `3.25` written the French way: `3,25`.
pub fn french(value: f32) -> String {
    format!("{value:.2}").replace('.', ",")
}

/// How much of its track a stat's bar fills: its value `(level + 3) / 4` over the highest.
pub fn bar_share(value: f32) -> f32 {
    (value / display_level(MAX_LEVEL)).clamp(0.0, 1.0)
}

/// The keys at the bottom of the screen, with the letters of the player's keyboard: the
/// keyboard's line, then the gamepad's. On the pseudo's line, Entrée types; while one types, how
/// to stop.
pub fn hint(layout: KeyboardLayout, screen: &SelectionScreen) -> [String; 2] {
    let [up, left, down, right] = layout.letters();
    if screen.typing {
        return [
            "Tape ton pseudo (12 lettres au plus)  ·  Entrée ou Échap : c'est bon".to_string(),
            "Vide, c'est le nom de ton animal.".to_string(),
        ];
    }
    let enter = if screen.on_pseudo_row() {
        "Entrée : écrire"
    } else {
        "Entrée : valider"
    };
    [
        format!(
            "Haut / Bas ({up} / {down}) : la ligne  ·  Gauche / Droite ({left} / {right}) : \
             changer  ·  {enter}  ·  Échap : retour"
        ),
        "Manette : croix ou stick, A : valider, B : retour  ·  Souris : clique sur ton choix"
            .to_string(),
    ]
}

/// The preview's frame, in px of the reference window: the camera renders exactly this shape.
pub const PREVIEW_SIZE: Vec2 = Vec2::new(380.0, 236.0);

/// The kart of the preview turns its nose this much toward the left of the screen (rad): seen
/// from 3/4 front...
pub const PREVIEW_YAW: f32 = 0.62;
/// ...and sways this much more, slowly (rad, s).
pub const PREVIEW_SWAY: f32 = 0.12;
pub const PREVIEW_SWAY_PERIOD: f32 = 5.0;
/// The camera's vertical field of view (rad), how high it looks from (rad above the
/// horizontal), what it looks at (m, from the kart's center), and how far it stands from it:
/// close on the kart, further back once its wing is open (m).
pub const PREVIEW_FOV: f32 = 0.55;
pub const PREVIEW_PITCH: f32 = 0.22;
pub const PREVIEW_LOOK_AT: Vec3 = Vec3::new(0.0, 0.1, 0.0);
pub const PREVIEW_LOOK_AT_WING: Vec3 = Vec3::new(0.0, 0.35, 0.0);
pub const PREVIEW_DISTANCE: f32 = 5.6;
pub const PREVIEW_DISTANCE_WING: f32 = 8.0;

/// The kart's yaw in the preview `t` seconds after it opened.
pub fn preview_yaw(t: f32) -> f32 {
    PREVIEW_YAW + PREVIEW_SWAY * (TAU * t / PREVIEW_SWAY_PERIOD).sin()
}

/// Where the preview's camera stands, around the kart's center (its front is -Z): in front of
/// it, a little above, `wing` of the way (0 to 1) from the close view to the one that shows the
/// open wing.
pub fn preview_camera(wing: f32) -> Transform {
    let wing = wing.clamp(0.0, 1.0);
    let at = PREVIEW_LOOK_AT.lerp(PREVIEW_LOOK_AT_WING, wing);
    let distance = PREVIEW_DISTANCE + (PREVIEW_DISTANCE_WING - PREVIEW_DISTANCE) * wing;
    let from = at + Vec3::new(0.0, PREVIEW_PITCH.sin(), -PREVIEW_PITCH.cos()) * distance;
    Transform::from_translation(from).looking_at(at, Vec3::Y)
}

/// The soft loop of the driver: a hop this long (s)...
pub const HOP_SECONDS: f32 = 0.9;
/// ...of which this share in the air, the rest on the seat, landing...
pub const HOP_AIR: f32 = 0.62;
/// ...this high (m, for a medium driver: a share of its size)...
pub const HOP_HEIGHT: f32 = 0.07;
/// ...squashed down to this share of its height when it lands, stretched up by this share at the
/// top (its volume kept: wider when squashed)...
pub const HOP_SQUASH: f32 = 0.16;
pub const HOP_STRETCH: f32 = 0.07;
/// ...and waddling: it leans this much to one side then the other, one side a hop, and turns a
/// little with it (rad).
pub const WADDLE_LEAN: f32 = 0.13;
pub const WADDLE_TURN: f32 = 0.1;

/// The pose of the driver on its seat `t` seconds into its soft loop: how high it is above it
/// (m, for a medium driver), its scale (its height squashed or stretched, its volume kept), how
/// much it leans and turns (rad).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftPose {
    pub lift: f32,
    pub scale: Vec3,
    pub lean: f32,
    pub turn: f32,
}

impl SoftPose {
    /// The driver's transform on a seat at `seat`, for a driver of `size`.
    pub fn transform(&self, seat: Vec3, size: f32) -> Transform {
        Transform {
            translation: seat + Vec3::Y * self.lift * size,
            rotation: Quat::from_rotation_y(self.turn) * Quat::from_rotation_z(self.lean),
            scale: self.scale * size,
        }
    }
}

/// The soft loop (see the module's documentation): periodic over two hops (one a side).
pub fn soft_pose(t: f32) -> SoftPose {
    let phase = (t / HOP_SECONDS).rem_euclid(1.0);
    let (lift, height) = if phase < HOP_AIR {
        // In the air: up and down, stretched at the top.
        let q = phase / HOP_AIR;
        let up = (PI * q).sin();
        (
            HOP_HEIGHT * 4.0 * q * (1.0 - q),
            1.0 + HOP_STRETCH * up * up,
        )
    } else {
        // On the seat: it squashes, then comes back up.
        let g = (phase - HOP_AIR) / (1.0 - HOP_AIR);
        (0.0, 1.0 - HOP_SQUASH * (PI * g).sin())
    };
    let wide = 1.0 / height.sqrt();
    // One side a hop: zero on the seat, most at the top.
    let side = (PI * t / HOP_SECONDS).sin();
    SoftPose {
        lift,
        scale: Vec3::new(wide, height, wide),
        lean: WADDLE_LEAN * side,
        turn: WADDLE_TURN * side,
    }
}

/// The portraits of the characters: each one rendered once in a cell of this many pixels of one
/// sheet (`preview.rs`), the grid's lines and columns.
pub const PORTRAIT_PX: u32 = 128;
/// A cell is this wide in the portraits' scene (m): an elephant fits in it, ears and all.
pub const PORTRAIT_CELL: f32 = 1.9;
/// The animals turn this much from facing the camera (rad: 3/4 front, like the kart)...
pub const PORTRAIT_YAW: f32 = -0.6;
/// ...and lean back a little: seen from slightly above (rad).
pub const PORTRAIT_TILT: f32 = 0.22;
/// Their seat is this far under the middle of their cell (share of the cell).
pub const PORTRAIT_SEAT: f32 = 0.36;

/// The middle of the cell (line, column) in the portraits' scene, around its center.
pub fn portrait_center(line: usize, column: usize) -> Vec3 {
    Vec3::new(
        (column as f32 - 1.0) * PORTRAIT_CELL,
        (1.0 - line as f32) * PORTRAIT_CELL,
        0.0,
    )
}

/// The pixels of the cell (line, column) in the sheet.
pub fn portrait_rect(line: usize, column: usize) -> Rect {
    let px = PORTRAIT_PX as f32;
    let min = Vec2::new(column as f32 * px, line as f32 * px);
    Rect::from_corners(min, min + Vec2::splat(px))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characters::{PartKind, Roster};

    #[test]
    fn values_are_written_the_french_way() {
        assert_eq!(french(3.25), "3,25");
        assert_eq!(french(0.75), "0,75");
        assert_eq!(french(5.0), "5,00");
        assert_eq!(bar_share(display_level(MAX_LEVEL)), 1.0);
        assert_eq!(bar_share(display_level(0)), 0.75 / 5.75);
        assert!((bar_share(display_level(10)) - 3.25 / 5.75).abs() < 1e-6);
    }

    /// Issue #98: every animal is in the grid once, on the line of its weight class, light ones
    /// first, in the order of the file; the grid has room for every class.
    #[test]
    fn the_grid_has_every_animal_once_by_weight_class() {
        let roster = Roster::embedded();
        let grid = character_grid(&roster);
        let mut seen = vec![0; roster.characters.len()];
        for (line, characters) in grid.iter().enumerate() {
            assert!(characters.len() <= GRID_COLUMNS, "{characters:?}");
            assert!(characters.windows(2).all(|w| w[0] < w[1]), "file order");
            for (column, index) in characters.iter().enumerate() {
                seen[*index] += 1;
                assert_eq!(roster.characters[*index].class, WeightClass::ALL[line]);
                assert_eq!(grid_cell(&roster, *index), (line, column));
            }
        }
        assert!(seen.iter().all(|n| *n == 1), "{seen:?}");
        let names: Vec<&str> = grid
            .iter()
            .flatten()
            .map(|i| roster.name(PartKind::Character, *i))
            .collect();
        assert_eq!(
            names,
            [
                "Poussin",
                "Pingouin",
                "Perroquet",
                "Chat",
                "Lapin",
                "Renard",
                "Cochon",
                "Vache",
                "Éléphant"
            ]
        );
        assert_eq!(class_label(WeightClass::Heavy), "Lourds");
    }

    /// The labels are short (`docs/UI_CHARTE.md` « Ton des textes »), the lines in the order of
    /// the screen, the pseudo's above the character's.
    #[test]
    fn the_lines_have_short_labels_in_the_order_of_the_screen() {
        let labels: Vec<&str> = SelectionScreen::SHOWN
            .iter()
            .map(|r| row_label(*r))
            .collect();
        assert_eq!(
            labels,
            ["Pseudo", "Personnage", "Kart", "Roues", "Aile", "Couleur"]
        );
        for row in 0..SelectionScreen::ROWS {
            assert!(SelectionScreen::SHOWN.contains(&row));
            assert!(row_label(row).chars().count() <= 16);
        }
        // Going down the screen is going down the lines (around: the pseudo's is the last).
        for pair in SelectionScreen::SHOWN.windows(2) {
            assert_eq!((pair[0] + 1) % SelectionScreen::ROWS, pair[1]);
        }
    }

    /// The keys at the bottom: the letters of the keyboard, Entrée that types on the pseudo's
    /// line, and how to stop typing.
    #[test]
    fn the_keys_line_says_what_each_key_does_here() {
        let mut screen = SelectionScreen {
            open: true,
            ..default()
        };
        let [keys, pad] = hint(KeyboardLayout::Azerty, &screen);
        assert!(
            keys.contains("(Z / S)") && keys.contains("(Q / D)"),
            "{keys}"
        );
        assert!(keys.contains("Entrée : valider") && keys.contains("Échap : retour"));
        assert!(
            pad.contains("A : valider") && pad.contains("B : retour"),
            "{pad}"
        );
        let [keys, _] = hint(KeyboardLayout::Qwerty, &screen);
        assert!(
            keys.contains("(W / S)") && keys.contains("(A / D)"),
            "{keys}"
        );
        screen.row = SelectionScreen::PSEUDO_ROW;
        assert!(hint(KeyboardLayout::Azerty, &screen)[0].contains("Entrée : écrire"));
        screen.typing = true;
        let [keys, _] = hint(KeyboardLayout::Azerty, &screen);
        assert!(keys.contains("Tape ton pseudo") && keys.contains("Entrée ou Échap"));
    }

    /// Issue #98: the driver's soft loop. It hops (never under its seat, at most its hop), lands
    /// squashed and stretches up, its volume kept; it waddles one side then the other; it loops
    /// every two hops; and it moves smoothly (no jump from a frame to the next at 30 fps).
    #[test]
    fn the_driver_hops_squashes_stretches_and_waddles_softly() {
        let frames = (0..=(4.0 * HOP_SECONDS * 120.0) as usize).map(|i| i as f32 / 120.0);
        let (mut lowest, mut highest, mut squashed, mut stretched) =
            (1.0f32, 0.0f32, 1.0f32, 1.0f32);
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for t in frames {
            let pose = soft_pose(t);
            assert!(
                pose.lift >= 0.0 && pose.lift <= HOP_HEIGHT + 1e-6,
                "{t}: {pose:?}"
            );
            let volume = pose.scale.x * pose.scale.y * pose.scale.z;
            assert!((volume - 1.0).abs() < 1e-4, "{t}: {pose:?}");
            assert!(pose.lean.abs() <= WADDLE_LEAN + 1e-6);
            lowest = lowest.min(pose.lift);
            highest = highest.max(pose.lift);
            squashed = squashed.min(pose.scale.y);
            stretched = stretched.max(pose.scale.y);
            left = left.max(pose.lean);
            right = right.min(pose.lean);
            let later = soft_pose(t + 2.0 * HOP_SECONDS);
            assert!((later.lift - pose.lift).abs() < 1e-4, "periodic at {t}");
            assert!((later.lean - pose.lean).abs() < 1e-3, "periodic at {t}");
            let next = soft_pose(t + 1.0 / 30.0);
            assert!((next.scale.y - pose.scale.y).abs() < 0.06, "{t}: a jump");
            assert!((next.lift - pose.lift).abs() < 0.02, "{t}: a jump");
            assert!((next.lean - pose.lean).abs() < 0.03, "{t}: a jump");
        }
        assert_eq!(lowest, 0.0, "it lands on its seat");
        assert!(highest > 0.9 * HOP_HEIGHT, "{highest}");
        assert!(squashed < 1.0 - 0.9 * HOP_SQUASH, "it squashes: {squashed}");
        assert!(
            stretched > 1.0 + 0.9 * HOP_STRETCH,
            "it stretches: {stretched}"
        );
        assert!(
            left > 0.9 * WADDLE_LEAN && right < -0.9 * WADDLE_LEAN,
            "both sides"
        );
        // Squashed only on its seat, stretched only in the air.
        for i in 0..200 {
            let t = i as f32 * HOP_SECONDS / 200.0;
            let pose = soft_pose(t);
            if pose.scale.y < 0.999 {
                assert_eq!(pose.lift, 0.0, "{t}");
            }
        }
        // On its seat: the transform scales around the seat, with the driver's size.
        let pose = soft_pose(HOP_SECONDS * (HOP_AIR + (1.0 - HOP_AIR) / 2.0));
        let at = pose.transform(Vec3::new(0.0, 0.4, 0.2), 1.25);
        assert_eq!(at.translation, Vec3::new(0.0, 0.4, 0.2));
        assert!(
            (at.scale.y - 1.25 * (1.0 - HOP_SQUASH)).abs() < 1e-4,
            "{at:?}"
        );
    }

    /// Issue #98: the preview's camera sees the whole kart from 3/4 front, its driver (the
    /// biggest) and, once it is open, the whole wing, whatever the sway: every corner of their
    /// boxes inside the frame, with a margin, in front of the camera. The kart fills the frame
    /// (not a dot in the middle).
    #[test]
    fn the_preview_frames_the_kart_and_its_open_wing() {
        // The kart's visual (m, around its center; its front is -Z): its wheels on the road (the
        // tractor tyres' too), the biggest driver (1.25 m tall on its seat, which is about the
        // center's height), the wing's sail and struts.
        let kart = (Vec3::new(-0.85, -0.62, -1.2), Vec3::new(0.85, 1.4, 1.2));
        let wing = (Vec3::new(-2.25, -0.62, -1.2), Vec3::new(2.25, 2.2, 1.2));
        let aspect = PREVIEW_SIZE.x / PREVIEW_SIZE.y;
        let fit = |camera: Transform, (min, max): (Vec3, Vec3), yaw: f32| {
            let view = camera.to_matrix().inverse();
            let turn = Quat::from_rotation_y(yaw);
            let mut extent = Vec2::ZERO;
            for i in 0..8 {
                let corner = Vec3::new(
                    if i & 1 == 0 { min.x } else { max.x },
                    if i & 2 == 0 { min.y } else { max.y },
                    if i & 4 == 0 { min.z } else { max.z },
                );
                let p = view.transform_point3(turn * corner);
                assert!(p.z < -0.5, "behind the camera: {p}");
                let ndc = Vec2::new(
                    p.x / (-p.z * (PREVIEW_FOV / 2.0).tan() * aspect),
                    p.y / (-p.z * (PREVIEW_FOV / 2.0).tan()),
                );
                extent = extent.max(ndc.abs());
            }
            extent
        };
        for yaw in [
            PREVIEW_YAW - PREVIEW_SWAY,
            PREVIEW_YAW,
            PREVIEW_YAW + PREVIEW_SWAY,
        ] {
            let close = fit(preview_camera(0.0), kart, yaw);
            assert!(close.x < 0.92 && close.y < 0.92, "{yaw}: {close}");
            assert!(close.max_element() > 0.7, "it fills the frame: {close}");
            let open = fit(preview_camera(1.0), wing, yaw);
            assert!(open.x < 0.96 && open.y < 0.96, "{yaw}: {open}");
            assert!(open.max_element() > 0.7, "{open}");
        }
        // The camera is in front of the kart and above it: 3/4 front.
        let camera = preview_camera(0.0).translation;
        assert!(camera.z < -4.0 && camera.y > 1.0, "{camera}");
        // Further back once the wing is open, and there whatever the share beyond 0 to 1.
        assert!(preview_camera(1.0).translation.length() > camera.length() + 1.0);
        assert_eq!(preview_camera(2.0), preview_camera(1.0));
        assert!(preview_yaw(0.0) > 0.4 && preview_yaw(0.0) < 0.9);
    }

    /// The portraits' cells: side by side in the scene and in the sheet, the first line on top.
    #[test]
    fn the_portraits_cells_tile_the_sheet() {
        assert_eq!(portrait_center(1, 1), Vec3::ZERO);
        assert!(portrait_center(0, 0).y > 0.0 && portrait_center(0, 0).x < 0.0);
        assert_eq!(
            portrait_center(2, 2) - portrait_center(1, 1),
            Vec3::new(PORTRAIT_CELL, -PORTRAIT_CELL, 0.0)
        );
        let rect = portrait_rect(2, 1);
        assert_eq!(rect.min, Vec2::new(128.0, 256.0));
        assert_eq!(rect.size(), Vec2::splat(128.0));
    }
}
