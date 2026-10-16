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

/// Every model the game names: the kart's, every biome's, every circuit's props, every
/// character's (issue #69).
fn named_models() -> BTreeSet<String> {
    let mut all: BTreeSet<String> = BTreeSet::new();
    all.insert(KART_MODEL.to_string());
    all.extend(animal_models());
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
