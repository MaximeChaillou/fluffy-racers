//! The models of the game (issue #11): every model a circuit, a biome or the kart names is in
//! `assets/models/`, with its pack's licence and its line in `assets/CREDITS.md`, and nothing
//! else is there; each one loads through Bevy's glTF loader and bakes (headless: the glTF plugin
//! without a renderer); a whole game with the models dresses both circuits and the karts, and no
//! piece of decor comes near the track.

use std::collections::BTreeSet;
use std::path::Path;

use bevy::gltf::GltfPlugin;

use super::*;
use crate::kart::model::{KART_MODEL, kart_model_data};
use crate::track::biome::{biomes, pack_of};
use crate::track::data::TrackData;
use crate::track::decor::decor_models;
use crate::track::mesh::{WALL_THICKNESS, wall_offset};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// The circuits of the game (`assets/tracks/index.ron`).
fn circuit_files() -> Vec<TrackData> {
    let index = std::fs::read_to_string(format!("{ASSETS}/tracks/index.ron")).unwrap();
    crate::admin::tracks::parse_manifest(&index)
        .unwrap()
        .iter()
        .map(|entry| {
            let text = std::fs::read_to_string(format!("{ASSETS}/tracks/{}", entry.file)).unwrap();
            let data = TrackData::from_ron(&text).unwrap();
            data.validate().unwrap();
            data
        })
        .collect()
}

/// Every model the game names: the kart's, every biome's, every circuit's props.
fn named_models() -> BTreeSet<String> {
    let mut all: BTreeSet<String> = BTreeSet::new();
    all.insert(KART_MODEL.to_string());
    for biome in biomes() {
        all.extend(biome.models().iter().map(|m| m.to_string()));
    }
    for data in circuit_files() {
        all.extend(decor_models(data.biome(), &data.props));
    }
    all
}

/// Issue #11: each model the game names is a file of `assets/models/`, each pack has its licence
/// (CC0) and a line of `assets/CREDITS.md` naming its files, its author and its page; the
/// textures the files point at are there; no other file is in the folder.
#[test]
fn every_model_is_there_with_its_licence_and_its_credits() {
    let named = named_models();
    let credits = std::fs::read_to_string(format!("{ASSETS}/CREDITS.md")).unwrap();
    let packs: BTreeSet<&str> = named.iter().map(|m| pack_of(m)).collect();
    assert!(packs.len() >= 4, "{packs:?}");
    for pack in &packs {
        let licence = std::fs::read_to_string(format!("{ASSETS}/models/{pack}/License.txt"))
            .unwrap_or_else(|_| panic!("assets/models/{pack}/License.txt"));
        assert!(
            licence.contains("CC0") && licence.contains("Kenney"),
            "{pack}"
        );
        let line = credits
            .lines()
            .find(|line| line.contains(&format!("`models/{pack}/")))
            .unwrap_or_else(|| panic!("no line for models/{pack}/ in assets/CREDITS.md"));
        assert!(
            line.contains("Kenney")
                && line.contains("CC0")
                && line.contains(&format!("https://kenney.nl/assets/{pack}")),
            "{line}"
        );
        for model in named.iter().filter(|m| pack_of(m) == *pack) {
            let file = model.split('/').nth(1).unwrap();
            assert!(
                line.contains(&format!("{file}.glb")),
                "{file}.glb not in: {line}"
            );
        }
    }

    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for model in &named {
        let path = format!("{ASSETS}/{}", crate::track::biome::model_path(model));
        let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("{path} is missing"));
        wanted.insert(format!("models/{model}.glb"));
        let gltf =
            bevy::gltf::gltf::Gltf::from_slice(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        for image in gltf.images() {
            if let bevy::gltf::gltf::image::Source::Uri { uri, .. } = image.source() {
                let texture = format!("models/{}/{uri}", pack_of(model));
                assert!(
                    Path::new(&format!("{ASSETS}/{texture}")).exists(),
                    "{model}: {texture} is missing"
                );
                wanted.insert(texture);
            }
        }
    }
    for pack in &packs {
        wanted.insert(format!("models/{pack}/License.txt"));
    }
    let mut present: BTreeSet<String> = BTreeSet::new();
    let mut folders = vec![format!("{ASSETS}/models")];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path.to_string_lossy().into_owned());
            } else {
                let relative = path.strip_prefix(ASSETS).unwrap().to_string_lossy();
                present.insert(relative.trim_start_matches('/').to_string());
            }
        }
    }
    let unused: Vec<&String> = present.difference(&wanted).collect();
    assert!(unused.is_empty(), "files nothing uses: {unused:?}");
}

/// Lets `app` load glTF files without a renderer: the glTF plugin, the image loader (a renderer
/// registers it) and the assets the glTF loader makes.
fn load_gltf_headless(app: &mut App) {
    use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats, ImageLoader};
    app.insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
        .add_plugins((bevy::image::ImagePlugin::default(), GltfPlugin::default()))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE));
    // What `App::run` does once the plugins are built: the glTF loader is registered then.
    Plugin::finish(&GltfPlugin::default(), app);
}

/// An App that loads glTF files without a renderer.
fn loading_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>();
    load_gltf_headless(&mut app);
    app
}

/// Updates until every model asked for is baked or missing.
fn wait_for_the_models(app: &mut App, ids: &[String]) {
    for _ in 0..3000 {
        app.update();
        let models = app.world().resource::<Models>();
        if ids.iter().all(|id| models.settled(id)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the models never finished loading");
}

/// Each model loads through Bevy's glTF loader and bakes: triangles, and colors from their
/// materials and textures (not white). The kart is a kart: its shell to paint, the rest, four
/// wheels of about 0.3 m.
#[test]
fn every_model_loads_and_bakes() {
    let mut app = loading_app();
    app.add_plugins(ModelPlugin);
    app.update();
    let ids: Vec<String> = named_models().into_iter().collect();
    {
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let mut models = world.resource_mut::<Models>();
        for id in &ids {
            models.request(id, Keep::Always, Some(&server));
        }
    }
    wait_for_the_models(&mut app, &ids);
    let models = app.world().resource::<Models>();
    for id in &ids {
        let model = models
            .get(id)
            .unwrap_or_else(|| panic!("{id} did not load"));
        assert!(model.triangle_count() > 0, "{id}");
        let (lo, hi) = model.bounds().unwrap();
        assert!(hi.y - lo.y > 0.01, "{id}: {lo} {hi}");
        let colors: BTreeSet<[u32; 3]> = model
            .parts
            .iter()
            .flat_map(|p| &p.mesh.colors)
            .map(|c| [c[0], c[1], c[2]].map(|v| (v * 255.0) as u32))
            .collect();
        // Colored by its materials or its texture: not all white (a bush has a single green).
        assert!(colors.iter().any(|c| *c != [255; 3]), "{id}: {colors:?}");
        for c in model.parts.iter().flat_map(|p| &p.mesh.colors) {
            assert!(c.iter().all(|v| (0.0..=1.0).contains(v)), "{id}: {c:?}");
        }
    }
    let kart = kart_model_data(models.get(KART_MODEL).unwrap()).expect("the kart's parts");
    assert!(kart.paint.triangle_count() > 50 && kart.trim.triangle_count() > 50);
    assert!(
        (kart.wheel_radius - 0.29).abs() < 0.02,
        "{}",
        kart.wheel_radius
    );
    // As long as the collider (2 m), as wide as the kart (1.3 m), a seat in the middle.
    let (lo, hi) = kart
        .paint
        .positions
        .iter()
        .chain(&kart.trim.positions)
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
    assert!(
        (hi.z - lo.z - 2.0).abs() < 0.05 && (hi.x - lo.x - 1.3).abs() < 0.1,
        "{lo} {hi}"
    );
    assert!(
        kart.seat.z.abs() < 0.5 && kart.seat.y > lo.y,
        "{}",
        kart.seat
    );
}

/// A missing file is missing, not a panic: what wanted it stands in primitives.
#[test]
fn a_missing_model_is_missing() {
    let mut app = loading_app();
    app.add_plugins(ModelPlugin);
    app.update();
    let id = "nature-kit/no_such_tree".to_string();
    let server = app.world().resource::<AssetServer>().clone();
    app.world_mut()
        .resource_mut::<Models>()
        .request(&id, Keep::Level, Some(&server));
    wait_for_the_models(&mut app, std::slice::from_ref(&id));
    let models = app.world().resource::<Models>();
    assert!(models.missing(&id) && models.get(&id).is_none());
    // Headless, without a loader: missing at once.
    let mut alone = Models::default();
    alone.request("nature-kit/tree_oak", Keep::Level, None);
    assert!(alone.missing("nature-kit/tree_oak"));
    // Another circuit's models are dropped, the karts' stay.
    alone.request(KART_MODEL, Keep::Always, None);
    alone.keep_for_the_level(&[]);
    assert_eq!(alone.counts(), (1, 0));
}

/// The whole game with the models (the glTF plugin, still headless): on both circuits, the decor
/// is dressed from every model (none in primitives) into its two meshes, and not one of its
/// vertices is over the road, the curbs, the shoulders or the walls of any part of the circuit;
/// the 12 karts wear the kart model, each body still with its color's shared material.
#[test]
fn the_game_dresses_both_circuits_and_the_karts() {
    for (track, name) in [
        ("tracks/prairie.track.ron", "Prairie Pompon"),
        ("tracks/anneau.track.ron", "Anneau Guimauve"),
    ] {
        let mut app =
            crate::tests::game_app_unloaded(crate::track::TrackChoice::Circuit(track.into()), 11);
        load_gltf_headless(&mut app);
        let mut dressed = None;
        for _ in 0..3000 {
            app.update();
            let world = app.world_mut();
            let decor = world
                .query::<&crate::track::CircuitDecor>()
                .iter(world)
                .next()
                .copied();
            let karts = world
                .query_filtered::<(), With<crate::kart::KartDressed>>()
                .iter(world)
                .count();
            if let Some(decor) = decor
                && karts == 12
            {
                dressed = Some(decor);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let decor = dressed.unwrap_or_else(|| panic!("{name}: never dressed"));
        assert_eq!(decor.stand_ins, 0, "{name}");
        assert!(
            decor.pieces > 50 && decor.triangles > 5_000,
            "{name}: {decor:?}"
        );

        let world = app.world_mut();
        let layout = world.resource::<crate::track::TrackLayout>().clone();
        assert_eq!(layout.name, name);
        let meshes: Vec<Handle<Mesh>> = world
            .query_filtered::<&Mesh3d, With<ChildOf>>()
            .iter(world)
            .map(|m| m.0.clone())
            .collect();
        let mut decor_meshes =
            world.query_filtered::<(&Mesh3d, &ChildOf), Without<crate::track::CircuitDecor>>();
        let roots: Vec<Entity> = world
            .query_filtered::<Entity, With<crate::track::CircuitDecor>>()
            .iter(world)
            .collect();
        let decor_handles: Vec<Handle<Mesh>> = decor_meshes
            .iter(world)
            .filter(|(_, parent)| roots.contains(&parent.parent()))
            .map(|(mesh, _)| mesh.0.clone())
            .collect();
        assert_eq!(decor_handles.len(), 2, "{name}: two meshes");
        assert!(meshes.len() > decor_handles.len());
        let assets = world.resource::<Assets<Mesh>>();
        let samples = layout.center.samples();
        for handle in &decor_handles {
            let mesh = assets.get(handle).unwrap();
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("positions");
            };
            for p in positions {
                let p = Vec3::from(*p);
                for s in samples {
                    let edge = wall_offset(s) + WALL_THICKNESS;
                    assert!(
                        s.position.xz().distance(p.xz()) > edge,
                        "{name}: a vertex of the decor at {p} is {:.2} m from the centerline \
                         at {:.0} m (walls' outer face at {edge:.2} m)",
                        s.position.xz().distance(p.xz()),
                        s.distance
                    );
                }
            }
        }

        // The karts: each body's shell wears its kart's color, the shared material.
        let visuals = world.resource::<crate::kart::KartVisualAssets>().clone();
        assert!(visuals.has_model());
        let bodies: Vec<(crate::kart::BodyPaint, Handle<StandardMaterial>)> = world
            .query::<(&crate::kart::BodyPaint, &MeshMaterial3d<StandardMaterial>)>()
            .iter(world)
            .map(|(paint, material)| (*paint, material.0.clone()))
            .collect();
        assert_eq!(bodies.len(), 12);
        for (paint, material) in bodies {
            assert_eq!(material, visuals.paint(paint.shown));
        }
    }
}

/// Measure (issue #11): entities, entities with a mesh (shown: neither it nor a parent hidden) and
/// different (mesh, material) pairs shown (what the renderer batches into one draw call at best)
/// on Prairie Pompon with 12 karts, with the models and without them (headless, primitives).
/// `cargo test --lib measure_entities_with_the_models -- --ignored --nocapture`
#[test]
#[ignore]
fn measure_entities_with_the_models() {
    for with_models in [false, true] {
        let mut app = crate::tests::game_app_unloaded(crate::track::TrackChoice::default(), 11);
        if with_models {
            load_gltf_headless(&mut app);
        }
        for _ in 0..3000 {
            app.update();
            let world = app.world_mut();
            let decor = world
                .query::<&crate::track::CircuitDecor>()
                .iter(world)
                .count();
            let karts = world
                .query_filtered::<(), With<crate::kart::KartDressed>>()
                .iter(world)
                .count();
            let bots = world.query::<&crate::kart::Kart>().iter(world).count();
            if (with_models && decor == 1 && karts == 12) || (!with_models && bots == 12) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        for _ in 0..10 {
            app.update();
        }
        let world = app.world_mut();
        let entities = world.query::<Entity>().iter(world).count();
        // Shown: neither it nor a parent hidden.
        let hidden = |world: &World, mut entity: Entity| loop {
            if world.get::<Visibility>(entity) == Some(&Visibility::Hidden) {
                return true;
            }
            match world.get::<ChildOf>(entity) {
                Some(parent) => entity = parent.parent(),
                None => return false,
            }
        };
        let all: Vec<(Entity, AssetId<Mesh>, AssetId<StandardMaterial>)> = world
            .query::<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>()
            .iter(world)
            .map(|(e, m, mat)| (e, m.0.id(), mat.0.id()))
            .collect();
        let meshes: Vec<(AssetId<Mesh>, AssetId<StandardMaterial>)> = all
            .iter()
            .filter(|(e, _, _)| !hidden(world, *e))
            .map(|(_, m, mat)| (*m, *mat))
            .collect();
        let pairs: BTreeSet<String> = meshes.iter().map(|p| format!("{p:?}")).collect();
        println!(
            "{}: {entities} entities, {} with a mesh ({} shown), {} different (mesh, material) \
             shown",
            if with_models {
                "with the models"
            } else {
                "primitives"
            },
            all.len(),
            meshes.len(),
            pairs.len()
        );
    }
}
