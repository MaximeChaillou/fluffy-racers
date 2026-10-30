//! Headless scenario of the cream on the screen (issue #194): the whole game (`GamePlugin`) on
//! Prairie Pompon, the player alone, its race started; the cream's drawing added by hand
//! ([`CreamPlugin`]: the game adds it only with a renderer).

use bevy::prelude::*;

use super::cream::{CreamPiece, PIECES, PieceKind};
use super::*;
use crate::admin::{Tunings, skip_countdown_now};
use crate::kart::ItemEffects;
use crate::race::Race;
use crate::tests::{game_app_unloaded, player, wait_for_the_player};
use crate::track::TrackChoice;

/// Nodes of the cream laid out again since the count was reset.
#[derive(Resource, Default)]
struct Relayouts(usize);

fn count_relayouts(
    mut count: ResMut<Relayouts>,
    changed: Query<(), (Changed<Node>, With<CreamPiece>)>,
) {
    count.0 += changed.iter().count();
}

fn pieces(app: &mut App) -> Vec<(CreamPiece, UiTransform, f32)> {
    let world = app.world_mut();
    world
        .query::<(&CreamPiece, &UiTransform, &ImageNode)>()
        .iter(world)
        .map(|(piece, transform, image)| (*piece, *transform, image.color.alpha()))
        .collect()
}

fn overlay(app: &mut App) -> Visibility {
    let world = app.world_mut();
    *world
        .query_filtered::<&Visibility, With<CreamOverlay>>()
        .single(world)
        .unwrap()
}

/// Issue #194: in the game, the HUD's plain blobs give way to the drawn pieces (every one of
/// them, one texture for all, laid out once); creamed, they hit, move and fade out with the
/// cream's 4 s, by their `UiTransform` and tint only (nothing laid out again); then they are gone.
#[test]
fn the_cream_splats_on_the_screen_and_goes() {
    let mut app = game_app_unloaded(TrackChoice::default(), 0);
    app.add_plugins(CreamPlugin)
        .init_resource::<Relayouts>()
        .add_systems(PostUpdate, count_relayouts);
    assert!(
        app.world()
            .resource::<Tunings>()
            .get("CreamTuning")
            .is_some()
    );
    wait_for_the_player(&mut app);
    let kart = player(&mut app);
    for _ in 0..10 {
        if app.world().get_resource::<Race>().is_some() {
            break;
        }
        app.update();
    }
    app.update();
    app.update();
    assert!(skip_countdown_now(app.world_mut()));
    app.update();
    let all = pieces(&mut app);
    assert_eq!(all.len(), PIECES);
    {
        let world = app.world_mut();
        assert_eq!(
            world.query::<&CreamBlob>().iter(world).count(),
            0,
            "plain blobs"
        );
        let images: Vec<_> = world
            .query::<(&CreamPiece, &ImageNode)>()
            .iter(world)
            .map(|(_, image)| image.image.id())
            .collect();
        assert!(images.windows(2).all(|w| w[0] == w[1]), "one texture");
    }
    assert!(all.iter().all(|(_, _, alpha)| *alpha == 0.0));
    assert_eq!(overlay(&mut app), Visibility::Hidden);

    // Creamed (as a chantilly from behind does: 4 s).
    {
        let mut effects = app.world_mut().get_mut::<ItemEffects>(kart).unwrap();
        effects.creamed = 4.0;
        effects.creamed_for = 4.0;
    }
    app.world_mut().resource_mut::<Relayouts>().0 = 0;
    let splat = |pieces: &[(CreamPiece, UiTransform, f32)]| {
        pieces
            .iter()
            .find(|(piece, ..)| piece.kind == PieceKind::Splat(0))
            .copied()
            .unwrap()
    };
    let mut seen = Vec::new();
    for _ in 0..120 {
        app.update();
        seen.push(splat(&pieces(&mut app)));
    }
    assert_eq!(overlay(&mut app), Visibility::Inherited);
    // It hit small and bounced back to its size, then it slid down.
    assert!(seen[0].1.scale.x < 0.8, "{:?}", seen[0]);
    let (_, settled, alpha) = seen[60];
    assert!(
        (settled.scale - Vec2::ONE).abs().max_element() < 0.05,
        "{settled:?}"
    );
    assert!(alpha > 0.9);
    let down = |t: &UiTransform| match t.translation.y {
        Val::Vh(y) => y,
        other => panic!("{other:?}"),
    };
    assert!(down(&seen[119].1) > down(&seen[40].1));
    // Fading out in its last second, then gone.
    for _ in 0..80 {
        app.update();
    }
    let (_, _, fading) = splat(&pieces(&mut app));
    assert!(fading > 0.0 && fading < 0.9, "{fading}");
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(overlay(&mut app), Visibility::Hidden);
    assert!(pieces(&mut app).iter().all(|(_, _, alpha)| *alpha == 0.0));
    assert_eq!(app.world().resource::<Relayouts>().0, 0, "laid out again");
}
