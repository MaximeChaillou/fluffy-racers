//! The models of the game (issue #11): every model a circuit, a biome, the kart or a character
//! (issue #69) names is in `assets/models/`, with its pack's licence and its line in
//! `assets/CREDITS.md`, and nothing else is there; each one loads through Bevy's glTF loader and
//! bakes (headless: the glTF plugin without a renderer), the animals with their clips; a whole
//! game with the models dresses both circuits, the karts and their drivers, and no piece of
//! decor comes near the track; the drivers play what their karts go through.

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
        .circuits
        .iter()
        .map(|entry| {
            let text = std::fs::read_to_string(format!("{ASSETS}/tracks/{}", entry.file)).unwrap();
            let data = TrackData::from_ron(&text).unwrap();
            data.validate().unwrap();
            data
        })
        .collect()
}

/// The characters' models (issue #69), in the roster's order.
fn animal_models() -> Vec<String> {
    crate::characters::Roster::embedded()
        .characters
        .iter()
        .filter_map(|c| c.model.clone())
        .collect()
}

/// Every model the game names: the kart's, its parts' (issue #73), every biome's, every
/// circuit's props, every character's (issue #69), the lobby's (issue #101).
fn named_models() -> BTreeSet<String> {
    let mut all: BTreeSet<String> = BTreeSet::new();
    all.insert(KART_MODEL.to_string());
    all.extend(crate::kart::parts::PART_MODELS.map(str::to_string));
    all.extend(animal_models());
    for biome in biomes() {
        all.extend(biome.models().iter().map(|m| m.to_string()));
        // Issue #146: its far decor's.
        all.extend(biome.far.models().iter().map(|m| m.to_string()));
    }
    for data in circuit_files() {
        all.extend(decor_models(data.biome(), &data.props));
        all.extend(crate::track::dressing::circuit_models(&data));
    }
    // Issue #101: the lobby's clearing.
    all.extend(crate::salon::SALON_MODELS.map(str::to_string));
    // Issue #106: the items' models taken from files.
    all.extend(crate::items::MODEL_FILES.map(|(_, id)| id.to_string()));
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
    // Issue #69: the nine animals, from Kenney's Cube Pets.
    assert_eq!(animal_models().len(), 9);
    assert!(packs.contains("cube-pets"), "{packs:?}");
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
/// registers it), the assets the glTF loader makes, and Bevy's animations (issue #69: the
/// animals' clips, played as in the game).
fn load_gltf_headless(app: &mut App) {
    use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats, ImageLoader};
    app.insert_resource(CompressedImageFormatSupport(CompressedImageFormats::NONE))
        .add_plugins((
            bevy::image::ImagePlugin::default(),
            GltfPlugin::default(),
            AnimationPlugin,
        ))
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE));
    // What `App::run` does once the plugins are built: the glTF loader is registered then.
    Plugin::finish(&GltfPlugin::default(), app);
}

/// An App that loads glTF files without a renderer.
pub(crate) fn loading_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>();
    load_gltf_headless(&mut app);
    app
}

/// Updates until every model asked for is baked or missing.
pub(crate) fn wait_for_the_models(app: &mut App, ids: &[String]) {
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

/// Issue #73, on the real files: every part has its look and its wheels their radius. The soap
/// box is the Car Kit's kart and the marshmallow tyres its wheels; the tractor tyres load from
/// the Car Kit, around their hub (from it out toward -X), round, colored, their rim outside, the
/// front ones the same mesh scaled down; the thread spools are the smallest, the tractor's back
/// tyres the biggest. With every set of wheels on the road, no home-made body's shell reaches
/// into a wheel, and every body's driver sits in it.
#[test]
fn every_part_has_its_look_from_the_files() {
    use crate::kart::parts::{BodyLook, PART_MODELS, WheelLook, kart_parts, tractor_wheels};
    use crate::track::GRID_HEIGHT;
    let mut app = loading_app();
    app.add_plugins(ModelPlugin);
    app.update();
    let ids: Vec<String> = std::iter::once(KART_MODEL)
        .chain(PART_MODELS)
        .map(str::to_string)
        .collect();
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
    let kart = kart_model_data(models.get(KART_MODEL).unwrap()).unwrap();
    let tractor_file = models.get(PART_MODELS[0]).unwrap();
    let parts = kart_parts(&kart, Some(tractor_file));
    // The soap box and the marshmallow tyres: the kart of before.
    let soap_box = parts.body(BodyLook::SoapBox);
    assert_eq!(soap_box.paint, kart.paint);
    assert_eq!(soap_box.trim, kart.trim);
    assert_eq!((soap_box.hubs, soap_box.seat), (kart.hubs, kart.seat));
    // Issue #176: each set with its band painted across it (`paint_band`), nothing else changed.
    use crate::kart::parts::{WHEEL_BANDS, paint_band};
    let banded = |mut mesh: crate::track::mesh::MeshData, look: WheelLook| {
        paint_band(&mut mesh, WHEEL_BANDS[look.index()]);
        mesh
    };
    let marshmallow = parts.wheels(WheelLook::Marshmallow);
    assert_eq!(
        marshmallow.mesh,
        banded(kart.wheel.clone(), WheelLook::Marshmallow)
    );
    assert_ne!(marshmallow.mesh, kart.wheel, "its band");
    assert_eq!(marshmallow.radii, [kart.wheel_radius; 2]);
    // The tractor tyres, from their file.
    let tractor = parts.wheels(WheelLook::Tractor);
    let from_file = tractor_wheels(tractor_file).unwrap();
    assert_eq!(tractor.radii, from_file.radii);
    assert_eq!(tractor.mesh, banded(from_file.mesh, WheelLook::Tractor));
    let (lo, hi) = tractor
        .mesh
        .positions
        .iter()
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
            (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
        });
    let back = tractor.radii[1];
    assert!(hi.x.abs() < 1e-4 && lo.x < -0.3, "{lo} {hi}");
    assert!((hi.y - back).abs() < 1e-3 && (lo.y + back).abs() < 1e-3);
    assert!((hi.z - back).abs() < 1e-3 && (lo.z + back).abs() < 1e-3);
    let colors: BTreeSet<[u32; 3]> = tractor
        .mesh
        .colors
        .iter()
        .map(|c| [c[0], c[1], c[2]].map(|v| (v * 255.0) as u32))
        .collect();
    assert!(colors.len() > 2, "{colors:?}");
    // Its rim (lighter) on the outside, away from the kart.
    let shade = |outer: bool| {
        let (sum, n) = tractor
            .mesh
            .positions
            .iter()
            .zip(&tractor.mesh.colors)
            .filter(|(p, _)| (p[0] < lo.x / 2.0) == outer && p[1].abs() < back * 0.5)
            .fold((0.0, 0), |(sum, n), (_, c)| {
                (sum + c[0] + c[1] + c[2], n + 1)
            });
        sum / n.max(1) as f32
    };
    assert!(shade(true) > shade(false), "the rim outside");
    // The front ones: the same mesh, scaled down to their radius.
    assert!((tractor.scale(0) * back - tractor.radius(0)).abs() < 1e-6);
    assert!(tractor.scale(0) < 0.9 && tractor.scale(3) == 1.0);
    let radius = |look, index| parts.wheels(look).radius(index);
    assert!(radius(WheelLook::Spools, 2) < radius(WheelLook::Marshmallow, 0));
    assert!(radius(WheelLook::Marshmallow, 2) < radius(WheelLook::Tractor, 0));
    assert!(radius(WheelLook::Tractor, 0) < radius(WheelLook::Tractor, 2));
    // No home-made shell reaching into a wheel, every set of wheels on the road at rest (the
    // paint is what the camera sees most; the axles and the rims may touch).
    for body_look in BodyLook::ALL {
        let body = parts.body(body_look);
        for wheel_look in WheelLook::ALL {
            let wheels = parts.wheels(wheel_look);
            let file_width = -wheels
                .mesh
                .positions
                .iter()
                .map(|p| p[0])
                .fold(0.0, f32::min);
            for (i, hub) in body.hubs.iter().enumerate() {
                let radius = wheels.radius(i);
                let width = file_width * wheels.scale(i);
                let center = Vec3::new(hub.x, radius - GRID_HEIGHT, hub.z);
                let inside = body
                    .paint
                    .positions
                    .iter()
                    .map(|p| Vec3::from(*p) - center)
                    .filter(|d| {
                        // From the hub outward (a left wheel toward -X).
                        let out = d.x * hub.x.signum();
                        (0.02..width - 0.02).contains(&out) && d.yz().length() < radius * 0.9
                    })
                    .count();
                // The Car Kit's kart has its exhausts in line with its own back wheels (9 and 17
                // points of them in its marshmallow tyres): the home-made bodies, none.
                if body_look != BodyLook::SoapBox {
                    assert_eq!(
                        inside, 0,
                        "{body_look:?} with {wheel_look:?}: {inside} points of the shell in \
                         wheel {i}"
                    );
                }
            }
        }
        // The seat inside the body's footprint, above its lowest point.
        let (lo, hi) = body
            .paint
            .positions
            .iter()
            .chain(&body.trim.positions)
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            });
        let seat = body.seat;
        assert!(
            seat.x.abs() < 1e-4 && seat.z > lo.z && seat.z < hi.z && seat.y > lo.y,
            "{body_look:?}: {seat}"
        );
        assert!((seat.y - kart.seat.y).abs() < 0.1, "{body_look:?}: {seat}");
    }
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
/// the 12 karts wear the kart model, each body still with its color's shared material. Issue
/// #146: the far decor too, in its own mesh (no shadows, no collider), and the fog of the biome
/// on the chase camera.
#[test]
fn the_game_dresses_both_circuits_and_the_karts() {
    for (track, name) in [
        ("tracks/prairie.track.ron", "Prairie Pompon"),
        ("tracks/anneau.track.ron", "Anneau Guimauve"),
        ("tracks/crique.track.ron", "Crique Câline"),
        ("tracks/col.track.ron", "Col des Moufles"),
        ("tracks/lagon.track.ron", "Lagon Duvet"),
        ("tracks/ruban.track.ron", "Ruban Réglisse"),
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
        // Issue #146: a circuit dressed before its chase camera was there gets its fog a frame
        // after the camera.
        app.update();
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

        // Issue #146: the far decor, in one mesh without shadows nor collider, never near the
        // circuit either; the fog of its biome on the chase camera, in the sky's color.
        let far: Vec<(
            Entity,
            crate::track::dressing::CircuitFarDecor,
            Handle<Mesh>,
        )> = world
            .query::<(Entity, &crate::track::dressing::CircuitFarDecor, &Mesh3d)>()
            .iter(world)
            .map(|(entity, far, mesh)| (entity, *far, mesh.0.clone()))
            .collect();
        assert_eq!(far.len(), 1, "{name}: one far decor");
        let (entity, far, handle) = far[0].clone();
        assert!(far.pieces > 50 && far.triangles > 2_000, "{name}: {far:?}");
        assert!(world.get::<bevy::light::NotShadowCaster>(entity).is_some());
        assert!(
            world
                .get::<bevy_rapier3d::prelude::Collider>(entity)
                .is_none()
        );
        assert!(world.get::<crate::level::LevelScoped>(entity).is_some());
        let mesh = world.resource::<Assets<Mesh>>().get(&handle).unwrap();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        assert!(!positions.is_empty(), "{name}");
        for p in positions {
            let p = Vec3::from(*p);
            for s in samples {
                let edge = wall_offset(s) + WALL_THICKNESS;
                assert!(
                    s.position.xz().distance(p.xz()) > edge,
                    "{name}: a vertex of the far decor at {p} is within the walls at {:.0} m",
                    s.distance
                );
            }
        }
        let sky = crate::track::TrackData::from_ron(
            &std::fs::read_to_string(format!("{ASSETS}/{track}")).unwrap(),
        )
        .unwrap()
        .biome()
        .unwrap()
        .palette
        .sky;
        let fogs: Vec<bevy::pbr::DistanceFog> = world
            .query_filtered::<&bevy::pbr::DistanceFog, With<crate::camera::ChaseCamera>>()
            .iter(world)
            .cloned()
            .collect();
        assert_eq!(fogs.len(), 1, "{name}: the chase camera's fog");
        assert_eq!(fogs[0].color, Color::srgb(sky.0, sky.1, sky.2), "{name}");
        assert!(
            matches!(fogs[0].falloff, bevy::pbr::FogFalloff::Linear { start, end }
                if start >= 150.0 && end > start),
            "{name}"
        );

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

/// Issue #73: in the whole game with the models (headless, Prairie Pompon, 12 karts), each kart
/// wears its parts: the looks of its loadout (the player's, each bot's drawn from the race's
/// seed) on its pivot; its body's meshes (the shell in its color's material, the rest in the
/// models' material); each wheel the mesh and the radius of its set, put on the road with that
/// radius; its driver in its body's seat. The meshes are one per part, shared by the karts that
/// have it, and no kart has a material of its own. Out comes the glider: the feather's karts
/// show the feather (the models' material), the others the pillowcase.
#[test]
fn every_kart_wears_its_parts() {
    use crate::characters::{PartKind, PlayerLoadout, Roster};
    use crate::interpolation::InterpolatedFrom;
    use crate::kart::{
        BodyPaint, DriverVisual, Kart, KartBodyPivot, KartEnvironment, KartTuning,
        KartVisualAssets, KartWheels, PlayerKart, WHEEL_ANCHORS, WheelVisual, Wing, WingLook,
    };
    let mut app = crate::tests::game_app_unloaded(crate::track::TrackChoice::default(), 11);
    // The player in the bathtub, on thread spools, with the feather.
    let roster = Roster::embedded();
    let mut mine = roster.default_loadout();
    for (kind, name) in [
        (PartKind::Kart, "Baignoire"),
        (PartKind::Wheels, "Bobines de fil"),
        (PartKind::Wing, "Plume"),
    ] {
        mine.set(kind, roster.find(kind, name).unwrap());
    }
    app.insert_resource(PlayerLoadout(mine));
    load_gltf_headless(&mut app);
    for _ in 0..3000 {
        app.update();
        let world = app.world_mut();
        let karts = world
            .query_filtered::<(), With<crate::kart::KartDressed>>()
            .iter(world)
            .count();
        if karts == 12 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for _ in 0..10 {
        app.update();
    }
    let seed = app.world().resource::<crate::race::RaceGrid>().seed;
    let tuning = app.world().resource::<KartTuning>().clone();
    let world = app.world_mut();
    let visuals = world.resource::<KartVisualAssets>().clone();
    let model = visuals
        .model()
        .expect("the kart model and its parts")
        .clone();
    let shared = world.resource::<ModelMaterials>().vertex_colors.clone();
    assert_eq!(model.material(), &shared);
    // Each kart's loadout: the player's, a bot's from its name (« Bot N »: its entrant).
    let karts: Vec<(Entity, crate::characters::Loadout)> = world
        .query_filtered::<(Entity, Option<&Name>, Has<PlayerKart>), With<Kart>>()
        .iter(world)
        .map(|(entity, name, player)| {
            let loadout = if player {
                mine
            } else {
                let entrant: usize = name.unwrap().as_str()[4..].parse().unwrap();
                crate::bots::bot_loadout(&roster, seed, entrant - 1)
            };
            (entity, loadout)
        })
        .collect();
    assert_eq!(karts.len(), 12);
    let pivots: Vec<(Entity, KartBodyPivot)> = world
        .query::<(Entity, &KartBodyPivot)>()
        .iter(world)
        .map(|(e, p)| (e, *p))
        .collect();
    let children = |world: &World, entity: Entity| -> Vec<Entity> {
        world
            .get::<Children>(entity)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    };
    let mut paints: BTreeSet<AssetId<Mesh>> = BTreeSet::new();
    let mut wheel_meshes: BTreeSet<AssetId<Mesh>> = BTreeSet::new();
    let mut bodies_seen = BTreeSet::new();
    let mut wheels_seen = BTreeSet::new();
    for (kart, loadout) in &karts {
        let looks = roster.looks(loadout);
        let (pivot, data) = pivots.iter().find(|(_, p)| p.body == *kart).unwrap();
        assert_eq!(data.looks, looks, "{kart:?}");
        bodies_seen.insert(looks.body.index());
        wheels_seen.insert(looks.wheels.index());
        let (paint, trim) = model.body_meshes(looks.body);
        let wheels_state = *world.get::<KartWheels>(*kart).unwrap();
        let mut wheels = 0;
        for child in children(world, *pivot) {
            if let Some(body) = world.get::<BodyPaint>(child) {
                let color = world.get::<crate::kart::KartPaint>(*kart).unwrap().0;
                assert_eq!(body.shown, color);
                assert_eq!(world.get::<Mesh3d>(child).unwrap().0, *paint);
                assert_eq!(
                    world
                        .get::<MeshMaterial3d<StandardMaterial>>(child)
                        .unwrap()
                        .0,
                    visuals.paint(color)
                );
                paints.insert(paint.id());
                let rest = children(world, child);
                assert_eq!(rest.len(), 1);
                assert_eq!(world.get::<Mesh3d>(rest[0]).unwrap().0, *trim);
                assert_eq!(
                    world
                        .get::<MeshMaterial3d<StandardMaterial>>(rest[0])
                        .unwrap()
                        .0,
                    shared
                );
            } else if let Some(wheel) = world.get::<WheelVisual>(child) {
                let (mesh, radius, scale) = model.wheel(looks.wheels, wheel.index);
                assert_eq!(world.get::<Mesh3d>(child).unwrap().0, *mesh);
                assert_eq!(
                    world.get::<Transform>(child).unwrap().scale,
                    Vec3::splat(scale)
                );
                assert_eq!(
                    world
                        .get::<MeshMaterial3d<StandardMaterial>>(child)
                        .unwrap()
                        .0,
                    shared
                );
                assert_eq!(wheel.radius, radius);
                wheel_meshes.insert(mesh.id());
                // On the road with its radius: its bottom where its suspension ray touches.
                let y = world.get::<Transform>(child).unwrap().translation.y;
                let i = wheel.index;
                assert!(wheels_state.on_ground[i], "{kart:?}: wheel {i} on the grid");
                let contact = WHEEL_ANCHORS[i].y
                    - (tuning.suspension_rest_length - wheels_state.compression[i]);
                assert!(
                    (y - radius - contact).abs() < 1e-4,
                    "{y} {radius} {contact}"
                );
                wheels += 1;
            } else if world.get::<DriverVisual>(child).is_some() {
                let at = world.get::<Transform>(child).unwrap().translation;
                assert_eq!(at, model.seat(looks.body));
            }
        }
        assert_eq!(wheels, 4);
    }
    // A varied grid: the player's bathtub and spools, and the bots' draws.
    assert!(bodies_seen.len() >= 2 && wheels_seen.len() >= 2);
    // One mesh per part shown, shared by the karts that have it.
    assert_eq!(paints.len(), bodies_seen.len());
    assert_eq!(wheel_meshes.len(), wheels_seen.len());
    // No material of a kart's own: under each of the 12 visuals, a material is its color's (one
    // per color of the palette) or one that every visual has (the models', the primitives').
    let roots: Vec<Entity> = world
        .query::<(Entity, &InterpolatedFrom)>()
        .iter(world)
        .filter(|(_, from)| karts.iter().any(|(kart, _)| *kart == from.0))
        .map(|(entity, _)| entity)
        .collect();
    assert_eq!(roots.len(), 12);
    let palette: BTreeSet<AssetId<StandardMaterial>> = crate::kart::KartColor::ALL
        .iter()
        .map(|c| visuals.paint(*c).id())
        .collect();
    let per_visual: Vec<BTreeSet<AssetId<StandardMaterial>>> = roots
        .iter()
        .map(|root| {
            let mut materials = BTreeSet::new();
            let mut stack = vec![*root];
            while let Some(entity) = stack.pop() {
                if let Some(material) = world.get::<MeshMaterial3d<StandardMaterial>>(entity) {
                    materials.insert(material.0.id());
                }
                stack.extend(children(world, entity));
            }
            materials
        })
        .collect();
    for materials in &per_visual {
        assert_eq!(materials.intersection(&palette).count(), 1, "one paint");
        for material in materials.difference(&palette) {
            assert!(
                per_visual.iter().all(|other| other.contains(material)),
                "a material of one kart's own"
            );
        }
    }
    assert!(per_visual.iter().all(|m| m.contains(&shared.id())));

    // The glider: every kart opens its wing (time stands still: only the visuals run).
    world.resource_mut::<Time<Virtual>>().pause();
    for (kart, _) in &karts {
        world.get_mut::<KartEnvironment>(*kart).unwrap().gliding = true;
    }
    app.update();
    let world = app.world_mut();
    let mut wings_seen = BTreeSet::new();
    for (kart, loadout) in &karts {
        let looks = roster.looks(loadout);
        let (pivot, _) = pivots.iter().find(|(_, p)| p.body == *kart).unwrap();
        let wing = children(world, *pivot)
            .into_iter()
            .find(|c| world.get::<Wing>(*c).is_some())
            .expect("its wing");
        let parts = children(world, wing);
        assert_eq!(parts.len(), 3, "a sail on two struts");
        let sail = parts[0];
        let mesh = world.get::<Mesh3d>(sail).unwrap().0.clone();
        let material = world
            .get::<MeshMaterial3d<StandardMaterial>>(sail)
            .unwrap()
            .0
            .clone();
        match looks.wing {
            WingLook::Feather => {
                assert_eq!((&mesh, &material), (model.feather(), &shared));
            }
            WingLook::Pillowcase => {
                assert!(mesh != *model.feather() && material != shared);
            }
        }
        wings_seen.insert(looks.wing.index());
    }
    assert_eq!(wings_seen.len(), 2, "both wings on the grid");
}

/// Issue #69: each character's animal loads with its clips (the eight of the Cube Pets), is an
/// animal (a body, no leg), its nodes named as the clips name them, and sits in its seat: facing
/// the kart's front, its body's bottom on the seat, as big as its weight class says and never
/// much wider than the kart nor too high for the camera to see over it.
#[test]
fn every_animal_loads_with_its_clips_and_sits_in_its_seat() {
    use crate::characters::{AnimalAnimation, AnimalPart, DriverTuning, Roster, animal_model_data};
    let mut app = loading_app();
    app.add_plugins(ModelPlugin);
    app.update();
    let ids = animal_models();
    {
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let mut models = world.resource_mut::<Models>();
        for id in &ids {
            models.request_animated(id, Keep::Always, Some(&server));
        }
    }
    wait_for_the_models(&mut app, &ids);
    let roster = Roster::embedded();
    let tuning = DriverTuning::default();
    let models = app.world().resource::<Models>();
    let clips = app.world().resource::<Assets<AnimationClip>>();
    // The body's width at its size (m), by weight class.
    let mut widths: Vec<(crate::characters::WeightClass, f32)> = Vec::new();
    for character in &roster.characters {
        let id = character.model.as_deref().unwrap();
        let model = models
            .get(id)
            .unwrap_or_else(|| panic!("{id} did not load"));
        let names: Vec<&str> = models
            .clips(id)
            .unwrap()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "dance",
                "eat",
                "gesture-negative",
                "gesture-positive",
                "idle",
                "run",
                "static",
                "walk"
            ],
            "{id}"
        );
        let animal = animal_model_data(model).unwrap_or_else(|| panic!("{id}: not an animal"));
        assert!(
            animal.nodes.iter().all(|n| !n.name.starts_with("leg-")),
            "{id}: no leg"
        );
        let body = animal.node("body").unwrap();
        assert_eq!(animal.nodes[body].part, Some(AnimalPart::Body));
        assert!(animal.nodes[body].mesh.as_ref().unwrap().triangle_count() > 50);
        assert_eq!(animal.nodes[0].path, [id.split('/').nth(1).unwrap()]);
        // The clips move the nodes by the names we give them (the glTF loader's: the path from
        // the file's root).
        for animation in AnimalAnimation::ALL {
            let handle = &models
                .clips(id)
                .unwrap()
                .iter()
                .find(|(name, _)| name == animation.clip())
                .unwrap()
                .1;
            let clip = clips.get(handle).unwrap();
            assert!(clip.duration() > 0.0, "{id}");
            for name in ["root", "body"] {
                let node = &animal.nodes[animal.node(name).unwrap()];
                let target = bevy::animation::AnimationTargetId::from_iter(&node.path);
                assert!(
                    clip.curves_for_target(target).is_some(),
                    "{id}: {} does not move {:?}",
                    animation.clip(),
                    node.path
                );
            }
        }
        // In its seat: the middle of its body's bottom on it, its face toward the front.
        let placed = animal.placement(&tuning);
        let seat = placed.transform_point(animal.seat_point);
        assert!(seat.length() < 1e-4, "{id}: {seat}");
        assert!(placed.rotation.mul_vec3(Vec3::Z).z < -0.99);
        // Its size: the character's times the placement's.
        let mesh = animal.nodes[body].mesh.as_ref().unwrap();
        let (lo, hi) = mesh
            .positions
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| {
                (lo.min(Vec3::from(*p)), hi.max(Vec3::from(*p)))
            });
        let scale = tuning.model_scale * character.size;
        let (width, height) = ((hi.x - lo.x) * scale, (hi.y - lo.y) * scale);
        assert!(width < 1.6, "{id}: {width} m wide, the kart 1.3 m");
        assert!((0.5..1.25).contains(&height), "{id}: {height} m high");
        widths.push((character.class, (1.25 * scale)));
    }
    // The cube of a light one smaller than a medium one's, a medium one's than a heavy one's.
    use crate::characters::WeightClass::*;
    for (a, wa) in &widths {
        for (b, wb) in &widths {
            if matches!((a, b), (Light, Medium) | (Medium, Heavy) | (Light, Heavy)) {
                assert!(wa < wb, "{a:?} {wa} vs {b:?} {wb}");
            }
        }
    }
}

/// Waits until the game with the models (headless) has its 12 karts dressed and their 12 drivers
/// seated in them.
fn wait_for_the_drivers(app: &mut App) {
    for _ in 0..3000 {
        app.update();
        let world = app.world_mut();
        let drivers = world
            .query::<&crate::characters::AnimalRoot>()
            .iter(world)
            .count();
        if drivers == 12 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the drivers never sat in their karts");
}

/// What each driver plays, by kart.
fn playing(app: &mut App) -> Vec<(Entity, crate::characters::AnimalAnimation, bool)> {
    let world = app.world_mut();
    world
        .query::<(&crate::characters::AnimalRoot, &AnimationPlayer)>()
        .iter(world)
        .map(|(root, player)| {
            let node = root.nodes.unwrap()[root.playing.index()];
            (root.kart, root.playing, player.is_playing_animation(node))
        })
        .collect()
}

/// Issue #69: in the whole game with the models (still headless, Bevy's animations on), the 12
/// drivers sit in their karts: their primitives gone, each animal's meshes and graph shared by
/// the karts it drives (one graph per animal), the body and the tail marked for the wobble. They
/// idle on the grid; a kart hit by an item or struck by a paw shakes its head, then idles again;
/// one over the line among the first three dances. The kart model and the physics are untouched.
#[test]
fn the_drivers_sit_in_their_karts_and_play_what_happens() {
    use crate::characters::{AnimalAnimation, AnimalPart, AnimalRoot, PartKind, Roster};
    use crate::kart::{DriverPrimitive, DriverVisual, HitReaction, KartState};
    let mut app = crate::tests::game_app_unloaded(crate::track::TrackChoice::default(), 11);
    load_gltf_headless(&mut app);
    wait_for_the_drivers(&mut app);
    for _ in 0..5 {
        app.update();
    }
    let roster = app.world().resource::<Roster>().clone();
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<(), With<DriverPrimitive>>()
            .iter(world)
            .count(),
        0,
        "every primitive is gone"
    );
    // Each animal's graph and body mesh: one for the karts it drives.
    let roots: Vec<(AnimalRoot, AssetId<AnimationGraph>)> = world
        .query::<(&AnimalRoot, &AnimationGraphHandle)>()
        .iter(world)
        .map(|(root, graph)| (*root, graph.0.id()))
        .collect();
    assert_eq!(roots.len(), 12);
    let characters: BTreeSet<usize> = roots.iter().map(|(r, _)| r.character).collect();
    let graphs: BTreeSet<AssetId<AnimationGraph>> = roots.iter().map(|(_, g)| *g).collect();
    assert!(characters.len() >= 6, "{characters:?}");
    assert_eq!(graphs.len(), characters.len(), "one graph per animal");
    let mut bodies: Vec<(AssetId<Mesh>, AssetId<StandardMaterial>)> = Vec::new();
    let mut tails = 0;
    for (part, mesh, material) in world
        .query::<(&AnimalPart, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>()
        .iter(world)
    {
        match part {
            AnimalPart::Body => bodies.push((mesh.0.id(), material.0.id())),
            AnimalPart::Tail => tails += 1,
            // Issue #12: the birds' wings are marked too (the wobble's test counts them).
            AnimalPart::LeftWing | AnimalPart::RightWing => {}
        }
    }
    assert_eq!(bodies.len(), 12);
    assert!(tails > 0);
    let meshes: BTreeSet<AssetId<Mesh>> = bodies.iter().map(|(m, _)| *m).collect();
    assert_eq!(meshes.len(), characters.len(), "one body mesh per animal");
    // One material for every animal: the models' (vertex colors).
    let shared = world.resource::<ModelMaterials>().vertex_colors.id();
    assert!(bodies.iter().all(|(_, material)| *material == shared));
    // The drivers' sizes still show their weight class (the driver's scale).
    let sizes: BTreeSet<u32> = world
        .query::<&DriverVisual>()
        .iter(world)
        .map(|d| (d.size * 100.0) as u32)
        .collect();
    assert!(sizes.len() >= 6, "{sizes:?}");
    // The player's animal is the default one, the cat.
    let player = crate::tests::player(&mut app);
    let mine = roots.iter().find(|(r, _)| r.kart == player).unwrap().0;
    assert_eq!(roster.name(PartKind::Character, mine.character), "Chat");

    // On the grid, everyone idles.
    let now = playing(&mut app);
    assert_eq!(now.len(), 12);
    assert!(
        now.iter()
            .all(|(_, animation, on)| *animation == AnimalAnimation::Idle && *on),
        "{now:?}"
    );
    // Three bots: one hit by an item, one struck by a paw, one over the line.
    let bots: Vec<Entity> = roots
        .iter()
        .map(|(r, _)| r.kart)
        .filter(|k| *k != player)
        .take(3)
        .collect();
    let world = app.world_mut();
    *world.get_mut::<KartState>(bots[0]).unwrap() = KartState::Hit {
        reaction: HitReaction::Spin,
        elapsed: 0,
        duration: 30,
    };
    {
        let mut paw = world.get_mut::<crate::interactions::Paw>(bots[1]).unwrap();
        paw.struck_count += 1;
        paw.struck_ticks = 0;
    }
    world
        .get_mut::<crate::race::RaceProgress>(bots[2])
        .unwrap()
        .finish = Some(0);
    for _ in 0..3 {
        app.update();
    }
    let of = |app: &mut App, kart: Entity| {
        playing(app)
            .into_iter()
            .find(|(k, ..)| *k == kart)
            .map(|(_, animation, on)| (animation, on))
            .unwrap()
    };
    assert_eq!(of(&mut app, bots[0]), (AnimalAnimation::Shake, true));
    assert_eq!(of(&mut app, bots[1]), (AnimalAnimation::Shake, true));
    assert_eq!(of(&mut app, bots[2]), (AnimalAnimation::Dance, true));
    assert_eq!(of(&mut app, player), (AnimalAnimation::Idle, true));
    // The hit is over (half a second), and the strike a second later: back to idle, while the
    // one over the line still dances.
    for _ in 0..70 {
        app.update();
    }
    assert!(!matches!(
        app.world().get::<KartState>(bots[0]),
        Some(KartState::Hit { .. })
    ));
    assert_eq!(of(&mut app, bots[0]), (AnimalAnimation::Idle, true));
    assert_eq!(of(&mut app, bots[1]), (AnimalAnimation::Idle, true));
    assert_eq!(of(&mut app, bots[2]), (AnimalAnimation::Dance, true));
    // The clips move the animals: a body's rotation is not the file's at rest.
    let world = app.world_mut();
    let moved = world
        .query::<(&AnimalPart, &Transform)>()
        .iter(world)
        .any(|(part, t)| *part == AnimalPart::Body && t.rotation != Quat::IDENTITY);
    assert!(moved, "the clips move the bodies");

    // A restart spawns every kart again: from its very first frame, each new kart shows its
    // animal, never its primitives (the drivers are dressed in `PostUpdate`).
    app.world_mut()
        .write_message(crate::level::LoadLevel::restart(
            crate::race::RaceMode::Race,
        ));
    let mut frames = 0;
    for _ in 0..300 {
        app.update();
        let world = app.world_mut();
        let primitives = world
            .query_filtered::<(), With<DriverPrimitive>>()
            .iter(world)
            .count();
        assert_eq!(primitives, 0, "a driver in primitives on a frame");
        let new = world
            .query::<&AnimalRoot>()
            .iter(world)
            .filter(|root| !roots.iter().any(|(old, _)| old.kart == root.kart))
            .count();
        frames += 1;
        if new == 12 {
            break;
        }
    }
    assert!(frames < 300, "the karts never came back");
}

/// Whether `p` is inside `mesh` (a closed surface, maybe of a few shells): an odd number of its
/// triangles crossed by a ray from it, along a majority of three directions.
fn inside_mesh(mesh: &crate::track::mesh::MeshData, p: Vec3) -> bool {
    let crossings = |dir: Vec3| {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| {
                let [a, b, c] = t.map(|i| Vec3::from(mesh.positions[i as usize]));
                // Möller–Trumbore.
                let (e1, e2) = (b - a, c - a);
                let h = dir.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-9 {
                    return false;
                }
                let f = 1.0 / det;
                let s = p - a;
                let u = f * s.dot(h);
                let q = s.cross(e1);
                let v = f * dir.dot(q);
                (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && f * e2.dot(q) > 1e-6
            })
            .count()
    };
    let dirs = [
        Vec3::new(0.13, 0.97, 0.21).normalize(),
        Vec3::new(-0.31, 0.11, 0.94).normalize(),
        Vec3::new(0.27, -0.9, -0.33).normalize(),
    ];
    dirs.iter().filter(|d| crossings(**d) % 2 == 1).count() >= 2
}

/// Issue #169, the issue's criteria on the real files: **each animal's shoulder is on its body,
/// at every size** (inside its body's mesh, just under its side: a little further out is
/// outside), on both sides; and **each animal has its paw, its colors taken on its model** (its
/// front leg's fur, foot and sole, or a bird's wing), none of them the cream paw of before, no
/// two alike. The fixed shoulder of before was off the chick's body: the check catches it.
#[test]
fn every_animal_has_its_paw_out_of_its_body() {
    use crate::characters::{
        DriverTuning, PawColors, PawShape, Roster, animal_model_data, leg_colors, paw_model,
        rest_transform, wing_colors,
    };
    let mut app = loading_app();
    app.add_plugins(ModelPlugin);
    app.update();
    let ids = animal_models();
    {
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let mut models = world.resource_mut::<Models>();
        for id in &ids {
            models.request_animated(id, Keep::Always, Some(&server));
        }
    }
    wait_for_the_models(&mut app, &ids);
    let roster = Roster::embedded();
    let tuning = DriverTuning::default();
    let models = app.world().resource::<Models>();
    let cream = PawColors::cream();
    let mut seen: Vec<(String, PawShape, [f32; 4], [f32; 4])> = Vec::new();
    for character in &roster.characters {
        let name = &character.name;
        let id = character.model.as_deref().unwrap();
        let animal = animal_model_data(models.get(id).unwrap()).unwrap();
        let paw = paw_model(&animal, character.paw).unwrap_or_else(|| panic!("{name}: no paw"));
        let body = animal.node("body").unwrap();
        let mesh = animal.nodes[body].mesh.as_ref().unwrap();
        // On its body, just under its side, on both sides.
        for (shoulder, out) in [
            (paw.shoulder, Vec3::X),
            (paw.shoulder * Vec3::new(-1.0, 1.0, 1.0), Vec3::NEG_X),
        ] {
            assert!(
                inside_mesh(mesh, shoulder),
                "{name}: {shoulder} off its body"
            );
            assert!(
                !inside_mesh(mesh, shoulder + out * 0.2),
                "{name}: {shoulder} deep in its body"
            );
        }
        // At every size, in the kart: inside the box of its body as it sits, on its side, a
        // third of the way up.
        let rest = rest_transform(&animal, body);
        for size in [0.5, character.size, 1.0, 1.6] {
            let seat = Mat4::from_translation(crate::kart::DRIVER_SEAT)
                * Mat4::from_scale(Vec3::splat(size))
                * animal.placement(&tuning).to_matrix()
                * rest;
            let placed = crate::models::bake::transformed(mesh, seat);
            let (lo, hi) = crate::models::shape::bounds(&placed).unwrap();
            let shoulder = seat.transform_point3(paw.shoulder);
            assert!(
                shoulder.cmpge(lo).all() && shoulder.cmple(hi).all(),
                "{name} at {size}: {shoulder} out of {lo}..{hi}"
            );
            // Its left (+X in its file) on the kart's left, on its side.
            let middle = (lo + hi) / 2.0;
            assert!(
                shoulder.x < middle.x - 0.3 * (hi.x - middle.x),
                "{name} at {size}: {shoulder}"
            );
            let up = (shoulder.y - lo.y) / (hi.y - lo.y);
            assert!((0.2..0.5).contains(&up), "{name} at {size}: {up}");
            if name == "Poussin" && size == character.size {
                // The shoulder of before (the same for everyone): off a small one's body.
                let before = seat.inverse().transform_point3(Vec3::new(-0.4, 0.62, 0.15));
                assert!(!inside_mesh(mesh, before), "{before}");
            }
        }
        // Its colors, taken on its model.
        let expected = if character.paw == PawShape::Wing {
            let wing = animal
                .nodes
                .iter()
                .find(|n| n.name == "wing-left")
                .unwrap_or_else(|| panic!("{name}: no wing"));
            let colors = wing_colors(wing.mesh.as_ref().unwrap()).unwrap();
            // The hand is the wing itself, in its colors.
            let wing_mesh = wing.mesh.as_ref().unwrap();
            assert!(
                paw.hand.colors.iter().all(|c| wing_mesh.colors.contains(c)),
                "{name}: its wing"
            );
            colors
        } else {
            let leg = animal
                .legs
                .iter()
                .find(|n| n.name == "leg-front-left")
                .unwrap();
            let colors = leg_colors(leg.mesh.as_ref().unwrap()).unwrap();
            assert!(paw.hand.colors.contains(&colors.foot), "{name}: its foot");
            colors
        };
        assert_eq!(paw.colors, expected, "{name}");
        assert!(
            paw.arm.colors.contains(&expected.fur),
            "{name}: the arm of its fur"
        );
        let apart = |a: [f32; 4], b: [f32; 4]| (0..3).map(|i| (a[i] - b[i]).abs()).sum::<f32>();
        assert!(apart(expected.fur, cream.fur) > 0.05, "{name}: not cream");
        seen.push((name.clone(), character.paw, expected.fur, expected.foot));
    }
    assert_eq!(seen.len(), 9);
    for (i, a) in seen.iter().enumerate() {
        for b in &seen[i + 1..] {
            assert!(
                a.1 != b.1 || a.2 != b.2 || a.3 != b.3,
                "{} and {} have the same paw",
                a.0,
                b.0
            );
        }
    }
}

/// Issue #169, in the whole game with the models (headless, Bevy's animations on): every kart's
/// arm is its animal's (its arm's and its hand's meshes, one per animal, shared by the karts it
/// drives, in the models' shared material), and **comes out of its driver's body as it is this
/// frame** (its clip and its wobble moving it): the arm starts inside the body's mesh where the
/// body is drawn, whatever the driver's size; a bird's wing leaves its body on that side.
#[test]
fn every_arm_is_its_animal_s_and_comes_out_of_its_body() {
    use crate::characters::{AnimalRoot, Animals};
    use crate::interactions::{Paw, PawArm, PawMotion};
    let mut app = crate::tests::game_app_unloaded(crate::track::TrackChoice::default(), 11);
    load_gltf_headless(&mut app);
    wait_for_the_drivers(&mut app);
    for _ in 0..5 {
        app.update();
    }
    for frame in 0..4 {
        // Every paw raised (half of them on the left), as the button does.
        let world = app.world_mut();
        let karts: Vec<Entity> = world
            .query_filtered::<Entity, With<Paw>>()
            .iter(world)
            .collect();
        for (i, kart) in karts.iter().enumerate() {
            let mut paw = world.get_mut::<Paw>(*kart).unwrap();
            paw.motion = PawMotion::Raised { ticks: 1 };
            paw.aim = if i % 2 == 0 { -1.0 } else { 1.0 };
        }
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_relative_speed(0.0);
        app.update();
        let world = app.world_mut();
        let roots: Vec<AnimalRoot> = world.query::<&AnimalRoot>().iter(world).copied().collect();
        assert_eq!(roots.len(), 12);
        let animals = world.resource::<Animals>().clone();
        let shared = world.resource::<ModelMaterials>().vertex_colors.id();
        let arms: Vec<(
            PawArm,
            Visibility,
            GlobalTransform,
            AssetId<Mesh>,
            AssetId<StandardMaterial>,
        )> = world
            .query::<(
                &PawArm,
                &Visibility,
                &GlobalTransform,
                &Mesh3d,
                &MeshMaterial3d<StandardMaterial>,
            )>()
            .iter(world)
            .map(|(a, v, g, m, mat)| (*a, *v, *g, m.0.id(), mat.0.id()))
            .collect();
        let mut out = 0;
        for (arm, visibility, global, mesh, material) in &arms {
            if *visibility == Visibility::Hidden {
                continue;
            }
            let root = roots.iter().find(|r| r.kart == arm.body).unwrap();
            let animal = animals.get(root.character).unwrap();
            let paw = animal.paw.as_ref().unwrap();
            let wanted = if arm.paw { &paw.hand } else { &paw.arm };
            assert_eq!(*mesh, wanted.id(), "its animal's");
            assert_eq!(*material, shared);
            if arm.paw {
                continue;
            }
            out += 1;
            let body = world.get::<GlobalTransform>(root.body.unwrap()).unwrap();
            let shoulder = body
                .affine()
                .inverse()
                .transform_point3(global.translation());
            let body_mesh = animal.data.nodes[paw.body].mesh.as_ref().unwrap();
            assert!(
                inside_mesh(body_mesh, shoulder),
                "frame {frame}, character {}: the arm starts at {shoulder}, off its body",
                root.character
            );
            // A bird: the wing on that side leaves its body, the other stays.
            if let [Some(left), Some(right)] = root.wings {
                let side = global.translation() - body.translation();
                let on_left = body.affine().inverse().transform_vector3(side).x > 0.0;
                let (gone, kept) = if on_left {
                    (left, right)
                } else {
                    (right, left)
                };
                assert_eq!(world.get::<Visibility>(gone), Some(&Visibility::Hidden));
                assert_eq!(world.get::<Visibility>(kept), Some(&Visibility::Inherited));
            }
        }
        assert_eq!(out, 12, "frame {frame}: every arm out");
        // One arm mesh per animal.
        let meshes: BTreeSet<AssetId<Mesh>> = arms
            .iter()
            .filter(|(a, ..)| !a.paw)
            .map(|(.., m, _)| *m)
            .collect();
        let characters: BTreeSet<usize> = roots.iter().map(|r| r.character).collect();
        assert_eq!(meshes.len(), characters.len());
        app.world_mut()
            .resource_mut::<Time<Virtual>>()
            .set_relative_speed(1.0);
        for _ in 0..7 {
            app.update();
        }
    }
}

/// Issue #12: with the animals (headless, their clips playing), every body, tail and wing carries
/// its wobble's layer, for its own driver, and the driver knows where its tail and wings hang
/// (the elephant's trunk ahead, the other tails behind; the left wing on the kart's left). A kart
/// hit by an item: its driver ragdolls, its body leans, and its pose is its clip's with the
/// wobble laid over it: the clip goes on playing under it (its base moves from frame to frame).
/// A driver over the line dances under its wobble the same way.
#[test]
fn the_wobble_is_laid_over_the_clips() {
    use crate::characters::{
        AnimalAnimation, AnimalPart, AnimalRoot, DriverWobble, LayerPivot, Roster, WobbleLayer,
        WobbleSlot,
    };
    use crate::kart::{HitReaction, KartState};
    let mut app = crate::tests::game_app_unloaded(crate::track::TrackChoice::default(), 11);
    load_gltf_headless(&mut app);
    wait_for_the_drivers(&mut app);
    for _ in 0..5 {
        app.update();
    }
    let roster = Roster::embedded();
    let world = app.world_mut();
    // Each animal: its root, its kart, its driver.
    let roots: Vec<(AnimalRoot, Entity)> = world
        .query::<(&AnimalRoot, &ChildOf)>()
        .iter(world)
        .map(|(root, child_of)| (*root, child_of.parent()))
        .collect();
    assert_eq!(roots.len(), 12);
    let parts: Vec<(Entity, AnimalPart, WobbleLayer)> = world
        .query::<(Entity, &AnimalPart, &WobbleLayer)>()
        .iter(world)
        .map(|(e, part, layer)| (e, *part, *layer))
        .collect();
    let count = |part: AnimalPart| parts.iter().filter(|(_, p, _)| *p == part).count();
    assert_eq!(count(AnimalPart::Body), 12);
    assert_eq!(
        world.query::<&AnimalPart>().iter(world).count(),
        parts.len(),
        "a layer on every part"
    );
    for (_, part, layer) in &parts {
        assert_eq!(layer.slot, WobbleSlot::from(*part));
        assert_eq!(layer.pivot, LayerPivot::Own);
        assert!(roots.iter().any(|(_, driver)| *driver == layer.driver));
    }
    let mut birds = 0;
    for (root, driver) in &roots {
        let levers = world.get::<DriverWobble>(*driver).unwrap().0.levers;
        let has = |part: AnimalPart| {
            parts
                .iter()
                .any(|(_, p, layer)| *p == part && layer.driver == *driver)
        };
        let model = roster.characters[root.character].model.clone().unwrap();
        assert_eq!(levers.tail.is_some(), has(AnimalPart::Tail), "{model}");
        if let Some(tail) = levers.tail {
            if model.ends_with("elephant") {
                assert!(tail.z < -0.3 && tail.y < 0.0, "{model}: {tail}");
            } else {
                assert!(tail.z > 0.3, "{model}: {tail}");
            }
        }
        assert_eq!(levers.wings.is_some(), has(AnimalPart::LeftWing), "{model}");
        assert_eq!(
            levers.wings.is_some(),
            has(AnimalPart::RightWing),
            "{model}"
        );
        if let Some([left, right]) = levers.wings {
            assert!(left.x < -0.5 && right.x > 0.5, "{model}: {left} {right}");
            birds += 1;
        }
    }
    assert_eq!(count(AnimalPart::LeftWing), birds);

    // A bot hit by an item, another over the line.
    let player = crate::tests::player(&mut app);
    let bots: Vec<(AnimalRoot, Entity)> = roots
        .iter()
        .filter(|(root, _)| root.kart != player)
        .take(2)
        .copied()
        .collect();
    let world = app.world_mut();
    *world.get_mut::<KartState>(bots[0].0.kart).unwrap() = KartState::Hit {
        reaction: HitReaction::Spin,
        elapsed: 0,
        duration: 30,
    };
    world
        .get_mut::<crate::race::RaceProgress>(bots[1].0.kart)
        .unwrap()
        .finish = Some(0);
    let body_of = |app: &mut App, driver: Entity| {
        let (entity, ..) = *parts
            .iter()
            .find(|(_, part, layer)| *part == AnimalPart::Body && layer.driver == driver)
            .unwrap();
        let world = app.world();
        (
            *world.get::<WobbleLayer>(entity).unwrap(),
            *world.get::<Transform>(entity).unwrap(),
        )
    };
    let mut bases: Vec<Vec<Quat>> = vec![Vec::new(), Vec::new()];
    let mut leaned = 0.0f32;
    for frame in 0..20 {
        app.update();
        for (i, (_, driver)) in bots.iter().enumerate() {
            let state = app.world().get::<DriverWobble>(*driver).unwrap().0;
            let (layer, transform) = body_of(&mut app, *driver);
            // The clip's pose with the wobble laid over it, never instead.
            assert_eq!(transform, layer.over(&state.pose), "frame {frame}");
            bases[i].push(layer.base.rotation);
            if i == 0 {
                assert!(state.ragdoll > 0.0, "frame {frame}");
                leaned = leaned.max(transform.rotation.angle_between(layer.base.rotation));
            }
        }
    }
    assert!(leaned > 0.1, "the hit one's body leans: {leaned}");
    let world = app.world_mut();
    let playing: Vec<AnimalAnimation> = bots
        .iter()
        .map(|(root, _)| {
            world
                .query::<&AnimalRoot>()
                .iter(world)
                .find(|r| r.kart == root.kart)
                .unwrap()
                .playing
        })
        .collect();
    assert_eq!(playing, [AnimalAnimation::Shake, AnimalAnimation::Dance]);
    // Under the wobble, the clips play: the shake and the dance turn the bodies frame by frame.
    for rotations in &bases {
        let moved = rotations
            .windows(2)
            .filter(|w| w[0].angle_between(w[1]) > 1e-4)
            .count();
        assert!(moved > 10, "{moved} of {}", rotations.len());
    }
}

/// Measure (issue #11): entities, entities with a mesh (shown: neither it nor a parent hidden) and
/// different (mesh, material) pairs shown (what the renderer batches into one draw call at best)
/// on Prairie Pompon with 12 karts, with the models and without them (headless, primitives).
/// `cargo test --lib measure_entities_with_the_models -- --ignored --nocapture`; issue #119:
/// another circuit with `FLUFFY_MEASURE_CIRCUIT=tracks/echappee.track.ron` (and its tiles).
#[test]
#[ignore]
fn measure_entities_with_the_models() {
    let track = std::env::var("FLUFFY_MEASURE_CIRCUIT").map_or_else(
        |_| crate::track::TrackChoice::default(),
        crate::track::TrackChoice::Circuit,
    );
    for with_models in [false, true] {
        let mut app = crate::tests::game_app_unloaded(track.clone(), 11);
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
        // Issue #146: the decor's triangles, near and far.
        let near: Vec<crate::track::CircuitDecor> = world
            .query::<&crate::track::CircuitDecor>()
            .iter(world)
            .copied()
            .collect();
        let far: Vec<crate::track::dressing::CircuitFarDecor> = world
            .query::<&crate::track::dressing::CircuitFarDecor>()
            .iter(world)
            .copied()
            .collect();
        println!("  decor: {near:?}; far decor: {far:?}");
        // Issue #119: the meshes of the circuit and of its decor (tiles on an open circuit).
        let names: Vec<String> = world
            .query_filtered::<&Name, With<Mesh3d>>()
            .iter(world)
            .map(|n| n.as_str().split(':').next().unwrap_or_default().to_string())
            .collect();
        for kind in [
            "Track",
            "Decor (shadows)",
            "Decor (small, no shadows)",
            "Far decor (pieces)",
        ] {
            let count = names.iter().filter(|n| n.as_str() == kind).count();
            println!("  {kind}: {count} meshes");
        }
    }
}

/// Preview (issue #73): every body with every set of wheels (and the feather over one), the cat
/// or another animal in its seat, assembled from the real files as the game shows them at rest,
/// written as JSON for an offline renderer. Not a check.
/// `FLUFFY_PARTS_DUMP=/path/out.json cargo test --lib dump_kart_parts -- --ignored`
#[test]
#[ignore]
fn dump_kart_parts() {
    use crate::characters::{DriverTuning, Roster, animal_model_data};
    use crate::kart::parts::{BodyLook, PART_MODELS, WheelLook, kart_parts};
    use crate::track::GRID_HEIGHT;
    use crate::track::mesh::MeshData;
    let Ok(out) = std::env::var("FLUFFY_PARTS_DUMP") else {
        return;
    };
    let mut app = loading_app();
    app.add_plugins(ModelPlugin);
    app.update();
    let roster = Roster::embedded();
    let animals = animal_models();
    let mut ids: Vec<String> = vec![KART_MODEL.to_string()];
    ids.extend(PART_MODELS.iter().map(|s| s.to_string()));
    {
        let world = app.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let mut models = world.resource_mut::<Models>();
        for id in &ids {
            models.request(id, Keep::Always, Some(&server));
        }
        for id in &animals {
            models.request_animated(id, Keep::Always, Some(&server));
        }
    }
    let mut all = ids.clone();
    all.extend(animals.iter().cloned());
    wait_for_the_models(&mut app, &all);
    let models = app.world().resource::<Models>();
    let kart = kart_model_data(models.get(KART_MODEL).unwrap()).unwrap();
    let parts = kart_parts(&kart, Some(&**models.get(PART_MODELS[0]).unwrap()));
    for look in BodyLook::ALL {
        let body = parts.body(look);
        println!(
            "{look:?}: {} triangles",
            body.paint.triangle_count() + body.trim.triangle_count()
        );
    }
    for look in WheelLook::ALL {
        let wheels = parts.wheels(look);
        println!("{look:?}: {} triangles", wheels.mesh.triangle_count());
    }
    println!("Feather: {} triangles", parts.feather.triangle_count());
    let tuning = DriverTuning::default();
    let paint = Color::srgb(0.95, 0.42, 0.62).to_linear();
    let animal = |name: &str, mesh: &mut MeshData, seat: Vec3| {
        let index = roster
            .characters
            .iter()
            .position(|c| c.name == name)
            .unwrap();
        let character = &roster.characters[index];
        let data =
            animal_model_data(models.get(character.model.as_deref().unwrap()).unwrap()).unwrap();
        let base = Mat4::from_translation(seat)
            * Mat4::from_scale(Vec3::splat(character.size))
            * data.placement(&tuning).to_matrix();
        let mut world: Vec<Mat4> = Vec::new();
        for node in &data.nodes {
            let parent = node.parent.map_or(base, |p| world[p]);
            let at = parent * node.local.to_matrix();
            world.push(at);
            if let Some(m) = &node.mesh {
                bake::append_transformed(mesh, m, at);
            }
        }
    };
    let mut entries: Vec<(String, MeshData)> = Vec::new();
    for body_look in BodyLook::ALL {
        for wheel_look in WheelLook::ALL {
            for driver in ["Chat", "Éléphant", "Poussin"] {
                if driver != "Chat" && wheel_look != WheelLook::Marshmallow {
                    continue;
                }
                let body = parts.body(body_look);
                let wheels = parts.wheels(wheel_look);
                let mut mesh = MeshData::default();
                let mut shell = body.paint.clone();
                for c in &mut shell.colors {
                    *c = [c[0] * paint.red, c[1] * paint.green, c[2] * paint.blue, 1.0];
                }
                mesh.append(&shell);
                mesh.append(&body.trim);
                let turns = body.wheel_turns();
                for (i, (hub, turn)) in body.hubs.iter().zip(turns).enumerate() {
                    let radius = wheels.radius(i);
                    let at = Mat4::from_translation(Vec3::new(hub.x, radius - GRID_HEIGHT, hub.z))
                        * Mat4::from_quat(turn)
                        * Mat4::from_scale(Vec3::splat(wheels.scale(i)));
                    bake::append_transformed(&mut mesh, &wheels.mesh, at);
                }
                animal(driver, &mut mesh, body.seat);
                if body_look == BodyLook::SoapBox && wheel_look == WheelLook::Spools {
                    mesh.append(&parts.feather);
                }
                let name = format!("{body_look:?}-{wheel_look:?}-{driver}")
                    .replace('É', "E")
                    .to_lowercase();
                entries.push((name, mesh));
            }
        }
    }
    let mut json = String::from("{");
    for (i, (name, mesh)) in entries.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let floats = |v: &[[f32; 3]]| {
            v.iter()
                .map(|p| format!("[{:.4},{:.4},{:.4}]", p[0], p[1], p[2]))
                .collect::<Vec<_>>()
                .join(",")
        };
        let colors: Vec<[f32; 3]> = mesh.colors.iter().map(|c| [c[0], c[1], c[2]]).collect();
        let indices: Vec<String> = mesh.indices.iter().map(|i| i.to_string()).collect();
        json.push_str(&format!(
            "\"{name}\":{{\"positions\":[{}],\"colors\":[{}],\"indices\":[{}]}}",
            floats(&mesh.positions),
            floats(&colors),
            indices.join(",")
        ));
    }
    json.push('}');
    std::fs::write(&out, json).unwrap();
    println!("{} karts written to {out}", entries.len());
}

/// Issue #176: every set of wheels shows its turn: its band (`kart::parts::WHEEL_BANDS`) painted
/// on the baked meshes (the Car Kit's marshmallow tyres, the tractor tyre's file, the spools),
/// around one angle of the wheel only, on a part of it.
#[test]
fn every_set_of_wheels_wears_its_band() {
    use crate::kart::{KartVisualAssets, WheelLook};
    let mut app = crate::tests::game_app_unloaded(crate::track::TrackChoice::TestGround, 0);
    load_gltf_headless(&mut app);
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<KartVisualAssets>()
            .is_some_and(|v| v.model().is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let model = app
        .world()
        .resource::<KartVisualAssets>()
        .model()
        .expect("the kart model and its parts")
        .clone();
    let meshes = app.world().resource::<Assets<Mesh>>();
    for look in WheelLook::ALL {
        let band = crate::kart::parts::WHEEL_BANDS[look.index()];
        let (handle, radius, _) = model.wheel(look, 2);
        let mesh = meshes.get(handle).expect("its mesh");
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("{look:?}: its colors");
        };
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("{look:?}: its positions");
        };
        let banded: Vec<Vec3> = positions
            .iter()
            .zip(colors)
            .filter(|(_, c)| **c == band)
            .map(|(p, _)| Vec3::from(*p))
            .collect();
        assert!(
            banded.len() >= 6 && banded.len() * 4 < positions.len(),
            "{look:?}: {} of {} corners",
            banded.len(),
            positions.len()
        );
        println!(
            "{look:?}: {} corners of its band, of {}",
            banded.len(),
            positions.len()
        );
        // Around one angle, from near the hub out to the tread.
        let far: Vec<f32> = banded
            .iter()
            .filter(|p| p.yz().length() > radius * 0.5)
            .map(|p| p.z.atan2(p.y))
            .collect();
        assert!(!far.is_empty(), "{look:?}: out to the rim");
        assert!(
            far.iter().all(|a| a.abs() < 1.2),
            "{look:?}: around one angle: {far:?}"
        );
        assert!(
            banded.iter().any(|p| p.yz().length() > radius * 0.9),
            "{look:?}: on the tread"
        );
    }
}
