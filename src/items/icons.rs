//! The items' icons (issue #106): each item of the HUD's slots and of its roulette's reel is its
//! model (`models.rs`), seen in 3/4 and outlined, rendered once on the CPU (`models::raster`): at
//! startup from the models built in code, again when a file's model is baked (`visual.rs`,
//! [`ItemIcons::replace`]). Only with a renderer ([`ItemIconPlugin`], added by the reel's
//! `ui::RoulettePlugin`): the headless HUD keeps its simple shapes (`ui::icon_shape`).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::models::{ItemModel, Look, model};
use super::table::ItemKind;
use crate::models::raster::{Icon, IconStyle, IconView, render_icon};

/// How the icons are drawn: 128 px, two samples a side, a thick outline in the HUD's ink.
pub fn icon_style() -> IconStyle {
    IconStyle::default()
}

/// How each model is turned for its icon (its best side).
pub fn icon_view(look: Look) -> IconView {
    let view = |yaw: f32, pitch: f32, roll: f32| IconView { yaw, pitch, roll };
    match look {
        Look::Item(item) => match item {
            ItemKind::Pelure => view(1.0, 0.35, 0.0),
            ItemKind::Pelote => view(-0.5, 0.35, 0.0),
            ItemKind::Abeille => view(-0.75, 0.3, 0.0),
            // Lying along Z: seen from its side, slanted.
            ItemKind::Piment | ItemKind::PimentDore => view(-1.3, 0.5, 0.45),
            ItemKind::CroquetteDoree => view(-0.25, 0.2, 0.0),
            ItemKind::Bombe => view(-0.5, 0.3, 0.0),
            ItemKind::Chantilly => view(-0.4, 0.35, 0.0),
            ItemKind::SuperCalin => view(-0.3, 0.15, 0.0),
            ItemKind::Fusee => view(-0.4, 0.2, -0.55),
            ItemKind::Degonflage => view(-0.4, 0.2, 0.0),
            ItemKind::OreillerGeant => view(-0.35, 0.65, 0.0),
            ItemKind::KlaxonCanard => view(-0.9, 0.25, 0.0),
            ItemKind::PieVoleuse => view(-1.05, 0.25, 0.0),
            ItemKind::Pinata => view(-0.35, 0.2, 0.0),
            ItemKind::Boomerang => view(0.0, 0.9, 0.45),
        },
        Look::Croquette => view(-0.35, 0.2, 0.0),
        Look::GiftBox | Look::DoubleGiftBox => IconView::THREE_QUARTER,
    }
}

/// The icon of `look`'s `model`.
pub fn render(look: Look, model: &ItemModel) -> Icon {
    render_icon(&model.merged(), icon_view(look), &icon_style())
}

/// An icon as an image for the UI (sRGB, its pixels kept on the GPU only).
pub fn to_image(icon: &Icon) -> Image {
    Image::new(
        Extent3d {
            width: icon.size,
            height: icon.size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        icon.bytes(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The icon of each item, in the order of [`ItemKind::ALL`], and the croquette's (the HUD's
/// counter).
#[derive(Resource, Clone, Debug)]
pub struct ItemIcons {
    icons: [Handle<Image>; 16],
    pub croquette: Handle<Image>,
}

impl ItemIcons {
    pub fn get(&self, item: ItemKind) -> &Handle<Image> {
        let index = ItemKind::ALL.iter().position(|i| *i == item).unwrap_or(0);
        &self.icons[index]
    }

    /// Draws `item`'s icon again from `model` (a file's, once baked), in place: every image of
    /// the HUD showing it follows.
    pub fn replace(&self, images: &mut Assets<Image>, item: ItemKind, model: &ItemModel) {
        let icon = render(Look::Item(item), model);
        let _ = images.insert(self.get(item).id(), to_image(&icon));
    }
}

/// The icons, with a renderer (the HUD's reel).
pub struct ItemIconPlugin;

impl Plugin for ItemIconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, create_icons);
    }
}

/// Every item's icon from its model built in code, and the croquette's (once).
fn create_icons(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut icon = |look: Look| images.add(to_image(&render(look, &model(look))));
    let icons = ItemKind::ALL.map(|item| icon(Look::Item(item)));
    let croquette = icon(Look::Croquette);
    commands.insert_resource(ItemIcons { icons, croquette });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::models::{MODEL_FILES, from_file};
    use crate::models::{Keep, Models};
    use crate::track::biome::biomes;
    use crate::ui::roulette::REEL;

    fn icons() -> Vec<(Look, Icon)> {
        Look::all()
            .map(|look| (look, render(look, &model(look))))
            .collect()
    }

    /// Every icon shows its model whole: inside the picture, filling a good part of it, its
    /// outline around it.
    #[test]
    fn every_icon_shows_its_model() {
        for (look, icon) in icons() {
            let coverage = icon.coverage();
            assert!(
                (0.12..0.75).contains(&coverage),
                "{look:?}: covers {coverage}"
            );
            let size = icon.size as usize;
            for k in 0..size {
                for edge in [k, k * size, (size - 1) * size + k, k * size + size - 1] {
                    assert_eq!(icon.pixels[edge][3], 0, "{look:?}: touches its edge");
                }
            }
        }
    }

    /// GD § 14, read at a glance: no two items look alike: another silhouette, or other colors.
    #[test]
    fn no_two_icons_look_alike() {
        let icons: Vec<(Look, Icon)> = icons()
            .into_iter()
            .filter(|(look, _)| matches!(look, Look::Item(_)))
            .collect();
        for (i, (a, icon_a)) in icons.iter().enumerate() {
            for (b, icon_b) in &icons[i + 1..] {
                let overlap = icon_a.overlap(icon_b);
                let apart = icon_a.mean_color().distance(icon_b.mean_color());
                assert!(
                    overlap < 0.8 || apart > 0.3,
                    "{a:?} and {b:?}: overlap {overlap}, colors {apart}"
                );
            }
        }
    }

    /// Issue #102 on the real icons: two neighbours of the reel are always far apart in color
    /// (the colors they are seen in, as rendered), each one is seen passing.
    #[test]
    fn the_reels_neighbours_are_far_apart_in_color() {
        let color = |item: ItemKind| {
            let look = Look::Item(item);
            render(look, &model(look)).mean_color()
        };
        for (k, item) in REEL.iter().enumerate() {
            let next = REEL[(k + 1) % REEL.len()];
            let apart = color(*item).distance(color(next));
            assert!(apart > 0.25, "{item:?} then {next:?}: {apart}");
        }
    }

    /// GD § 14: what lies on the track or flies over it (the dangers, the boxes, the croquettes)
    /// stands out from the decor: its colors far from every biome's road, grass, shoulder and
    /// sand.
    #[test]
    fn the_items_on_the_track_stand_out_from_the_decor() {
        let on_track = [
            Look::Item(ItemKind::Pelure),
            Look::Item(ItemKind::Pelote),
            Look::Item(ItemKind::Abeille),
            Look::Item(ItemKind::Bombe),
            Look::Item(ItemKind::OreillerGeant),
            Look::Item(ItemKind::Boomerang),
            Look::Croquette,
            Look::GiftBox,
            Look::DoubleGiftBox,
        ];
        let mut too_close = Vec::new();
        for look in on_track {
            let seen = render(look, &model(look)).mean_color();
            let mut closest = (f32::MAX, String::new());
            for biome in biomes() {
                let p = biome.palette;
                for (name, (r, g, b)) in [
                    ("road", p.asphalt),
                    ("grass", p.ground),
                    ("shoulder", p.shoulder),
                    ("sand", p.sand),
                ] {
                    let apart = seen.distance(Vec3::new(r, g, b));
                    if apart < closest.0 {
                        closest = (apart, format!("{}'s {name}", biome.name));
                    }
                    if apart <= 0.2 {
                        too_close.push(format!("{look:?} on {}'s {name}: {apart}", biome.name));
                    }
                }
            }
            println!(
                "{look:?}: seen in {seen}, closest to {} ({})",
                closest.1, closest.0
            );
        }
        assert!(too_close.is_empty(), "{too_close:#?}");
    }

    /// Issue #137 (it was #132's « the pelure is a clementine peel, not a banana »): the ground
    /// trap is the Cac's, never the banana peel of the karting games of reference (CLAUDE.md; GD
    /// § 8, « Nos objets ») nor #132's clementine peel. It is seen brown: an orange hue (the
    /// banana of #106 was seen at 49°), dark (the clementine was seen at a lightness of 0.61, in
    /// (0.89, 0.60, 0.34)), saturated (not a road's grey). It is seen as its swirl: a mound as
    /// tall as wide, narrow at its tip and wide at its bottom (a flat peel or a curved banana
    /// are not), its coils one over the other down its middle (each lit on top, dark in the
    /// groove under it). And no golden curve reads as a banana either: both chilies show their
    /// green calyx (the piment doré had none).
    #[test]
    fn the_trap_is_a_brown_cacs_not_a_banana_nor_a_clementine() {
        let icon = |item: ItemKind| render(Look::Item(item), &model(Look::Item(item)));
        // The share of the model's own pixels (seen, sRGB) that `test` takes.
        let share = |icon: &Icon, test: fn(Vec3) -> bool| {
            let seen: Vec<Vec3> = icon
                .pixels
                .iter()
                .zip(&icon.body)
                .filter(|(_, body)| **body)
                .map(|(p, _)| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32) / 255.0)
                .collect();
            seen.iter().filter(|c| test(**c)).count() as f32 / seen.len().max(1) as f32
        };
        let cacs = icon(ItemKind::Pelure);
        let seen = cacs.mean_color();
        let hsla = Hsla::from(Srgba::rgb(seen.x, seen.y, seen.z));
        assert!(
            (15.0..40.0).contains(&hsla.hue),
            "the Cac's is seen at {}°",
            hsla.hue
        );
        assert!(hsla.lightness < 0.48, "too light: {hsla:?}");
        assert!(hsla.saturation > 0.3, "too grey: {hsla:?}");
        let clementine = Vec3::new(0.89, 0.6, 0.34);
        assert!(seen.distance(clementine) > 0.3, "{seen}");
        // Its silhouette, row by row (its width in pixels), and how bright it is down its middle.
        let size = cacs.size as usize;
        let rows: Vec<(usize, usize, usize)> = (0..size)
            .filter_map(|y| {
                let row = &cacs.body[y * size..(y + 1) * size];
                let first = row.iter().position(|b| *b)?;
                let last = row.iter().rposition(|b| *b)?;
                Some((y, first, last))
            })
            .collect();
        let tall = rows.len();
        let wide = rows.iter().map(|(_, a, b)| b + 1 - a).max().unwrap_or(0);
        assert!(tall as f32 > 0.8 * wide as f32, "{tall} tall, {wide} wide");
        let width = |from: f32, to: f32| {
            rows[(tall as f32 * from) as usize..(tall as f32 * to) as usize]
                .iter()
                .map(|(_, a, b)| b + 1 - a)
                .max()
                .unwrap_or(0)
        };
        let (tip, bottom) = (width(0.0, 0.25), width(0.5, 1.0));
        assert!(
            (tip as f32) < 0.6 * bottom as f32,
            "its tip {tip} px wide, its bottom {bottom}"
        );
        let middle = rows.iter().map(|(_, a, b)| a + b).sum::<usize>() / (2 * tall);
        let mut coils = 0;
        let mut darkest = f32::MAX;
        for (y, ..) in &rows {
            let x = middle.min(size - 1);
            if !cacs.body[y * size + x] {
                continue;
            }
            let p = cacs.pixels[y * size + x];
            let light = (0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32) / 255.0;
            darkest = darkest.min(light);
            if light > darkest + 0.15 {
                coils += 1;
                darkest = f32::MAX;
            }
        }
        assert!(coils >= 3, "{coils} coils seen down its middle");
        for item in [ItemKind::Piment, ItemKind::PimentDore] {
            let calyx = share(&icon(item), |c| c.y > c.x + 0.1 && c.y > c.z + 0.1);
            assert!(calyx > 0.08, "{item:?}'s calyx: {calyx}");
        }
    }

    /// Issue #106 acceptance: every item of `ItemKind::ALL` has a model and an icon; each file a
    /// model is taken from is in `assets/models/` with its pack's licence and its line in
    /// `assets/CREDITS.md` (`models::tests` checks it for every file the game names, these
    /// included); each one bakes and gives its model (in the game's colors) and its icon.
    #[test]
    fn every_item_has_a_model_and_an_icon_and_every_file_is_there_and_credited() {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
        let credits = std::fs::read_to_string(format!("{assets}/CREDITS.md")).unwrap();
        for item in ItemKind::ALL {
            let look = Look::Item(item);
            let built = model(look);
            assert!(built.triangle_count() > 0, "{item:?}");
            assert!(render(look, &built).coverage() > 0.1, "{item:?}");
        }
        assert!(!MODEL_FILES.is_empty());
        for (look, id) in MODEL_FILES {
            let (pack, name) = id.split_once('/').unwrap();
            let path = format!("{assets}/models/{pack}/{name}.glb");
            assert!(std::path::Path::new(&path).exists(), "{path} is missing");
            let licence = std::fs::read_to_string(format!("{assets}/models/{pack}/License.txt"))
                .unwrap_or_else(|_| panic!("no licence for {pack}"));
            assert!(licence.contains("CC0"), "{pack}");
            let line = credits
                .lines()
                .find(|line| line.contains(&format!("`models/{pack}/")))
                .unwrap_or_else(|| panic!("{pack} is not credited"));
            assert!(line.contains(&format!("{name}.glb")), "{look:?}: {line}");
        }
        // Each file bakes, and gives its model and its icon.
        let mut app = crate::models::tests::loading_app();
        app.add_plugins(crate::models::ModelPlugin);
        app.update();
        let ids: Vec<String> = MODEL_FILES.iter().map(|(_, id)| id.to_string()).collect();
        {
            let world = app.world_mut();
            let server = world.resource::<AssetServer>().clone();
            let mut models = world.resource_mut::<Models>();
            for id in &ids {
                models.request(id, Keep::Always, Some(&server));
            }
        }
        crate::models::tests::wait_for_the_models(&mut app, &ids);
        let models = app.world().resource::<Models>();
        for (look, id) in MODEL_FILES {
            let baked = models
                .get(id)
                .unwrap_or_else(|| panic!("{id} did not load"));
            let built = model(look);
            let made = from_file(look, baked, &built).unwrap_or_else(|| panic!("{id}"));
            assert!(made.body.triangle_count() > 20, "{look:?}");
            let icon = render(look, &made);
            assert!(
                (0.12..0.75).contains(&icon.coverage()),
                "{look:?}: {}",
                icon.coverage()
            );
            // In about the colors of the built model (the game's own).
            let apart = icon
                .mean_color()
                .distance(render(look, &built).mean_color());
            assert!(apart < 0.35, "{look:?}: {apart}");
            if matches!(look, Look::GiftBox | Look::DoubleGiftBox) {
                assert!(made.lid.is_some(), "{look:?}: its lid");
            }
        }
    }

    /// Writes every icon (and each file's) to `FLUFFY_ICON_DIR` as raw RGBA (`<name>.rgba`, 128
    /// × 128), to look at them: `FLUFFY_ICON_DIR=/tmp/icons cargo test --lib write_the_icons --
    /// --ignored`.
    #[test]
    #[ignore = "a tool: FLUFFY_ICON_DIR=/tmp/icons cargo test --lib write_the_icons -- --ignored"]
    fn write_the_icons() {
        let dir = std::env::var("FLUFFY_ICON_DIR").expect("FLUFFY_ICON_DIR");
        std::fs::create_dir_all(&dir).unwrap();
        // What the game does at startup: the sixteen items' icons.
        let started = std::time::Instant::now();
        for item in ItemKind::ALL {
            std::hint::black_box(render(Look::Item(item), &model(Look::Item(item))));
        }
        println!(
            "the 16 icons (their models built and rendered): {:.1} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
        for (look, icon) in icons() {
            let name = format!("{look:?}").replace(['(', ')'], "");
            std::fs::write(format!("{dir}/{name}.rgba"), icon.bytes()).unwrap();
        }
        let mut app = crate::models::tests::loading_app();
        app.add_plugins(crate::models::ModelPlugin);
        app.update();
        let ids: Vec<String> = MODEL_FILES.iter().map(|(_, id)| id.to_string()).collect();
        {
            let world = app.world_mut();
            let server = world.resource::<AssetServer>().clone();
            let mut models = world.resource_mut::<Models>();
            for id in &ids {
                models.request(id, Keep::Always, Some(&server));
            }
        }
        crate::models::tests::wait_for_the_models(&mut app, &ids);
        let models = app.world().resource::<Models>();
        for (look, id) in MODEL_FILES {
            if let Some(made) = models
                .get(id)
                .and_then(|baked| from_file(look, baked, &model(look)))
            {
                let name = format!("{look:?}-file").replace(['(', ')'], "");
                std::fs::write(format!("{dir}/{name}.rgba"), render(look, &made).bytes()).unwrap();
            }
        }
    }
}
