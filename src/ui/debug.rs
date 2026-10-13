//! Debug overlay (issue #6), toggled with F3, hidden by default: frames per second, frame time,
//! number of karts and the cost of the simulation ticks. Text refreshed 4 times a second while
//! it is shown, written into the same string (no allocation once its capacity is reached).

use bevy::diagnostic::{DiagnosticPath, DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::platform::time::Instant;
use bevy::prelude::*;

use super::format::write_debug_overlay;
use crate::kart::Kart;

/// Shows / hides the overlay.
pub const DEBUG_OVERLAY_KEY: KeyCode = KeyCode::F3;
/// Seconds between two refreshes of the text.
const REFRESH_SECONDS: f32 = 0.25;
/// Weight of the newest tick in the smoothed simulation cost.
const COST_SMOOTHING: f64 = 0.05;

/// The overlay's root node (its visibility is the toggle).
#[derive(Component, Clone, Copy, Debug)]
pub struct DebugOverlay;

/// The overlay's text.
#[derive(Component, Clone, Copy, Debug)]
pub struct DebugOverlayText;

/// Wall-clock cost of one simulation tick (every fixed schedule, physics included), smoothed:
/// how much of the frame the simulation takes, as opposed to the rendering.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SimulationCost {
    started: Option<Instant>,
    /// Milliseconds per tick, once measured.
    pub average_ms: Option<f64>,
}

pub struct DebugOverlayPlugin;

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        app.init_resource::<SimulationCost>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(FixedFirst, start_tick_clock)
            .add_systems(FixedLast, stop_tick_clock)
            .add_systems(Update, (toggle_overlay, refresh_overlay).chain());
    }
}

fn spawn_overlay(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Debug overlay"),
            DebugOverlay,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: px(6),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(10),
        ))
        .with_children(|overlay| {
            overlay.spawn((
                DebugOverlayText,
                Text::new(String::with_capacity(64)),
                TextFont::from_font_size(18.0),
                TextColor(Color::srgb(0.6, 1.0, 0.6)),
                Node {
                    padding: UiRect::axes(px(10), px(4)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ));
        });
}

fn start_tick_clock(mut cost: ResMut<SimulationCost>) {
    cost.started = Some(Instant::now());
}

fn stop_tick_clock(mut cost: ResMut<SimulationCost>) {
    let Some(started) = cost.started.take() else {
        return;
    };
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    cost.average_ms = Some(match cost.average_ms {
        Some(average) => average + (ms - average) * COST_SMOOTHING,
        None => ms,
    });
}

fn toggle_overlay(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut overlays: Query<&mut Visibility, With<DebugOverlay>>,
) {
    if !keys.is_some_and(|keys| keys.just_pressed(DEBUG_OVERLAY_KEY)) {
        return;
    }
    for mut visibility in &mut overlays {
        *visibility = match *visibility {
            Visibility::Hidden => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
    }
}

fn refresh_overlay(
    time: Res<Time<Real>>,
    mut since_refresh: Local<Option<f32>>,
    diagnostics: Option<Res<DiagnosticsStore>>,
    cost: Res<SimulationCost>,
    karts: Query<(), With<Kart>>,
    overlays: Query<&Visibility, With<DebugOverlay>>,
    mut texts: Query<&mut Text, With<DebugOverlayText>>,
) {
    if !overlays.iter().any(|v| *v != Visibility::Hidden) {
        // Hidden: refresh as soon as it is shown again.
        *since_refresh = None;
        return;
    }
    let elapsed = since_refresh.map_or(REFRESH_SECONDS, |t| t + time.delta_secs());
    if elapsed < REFRESH_SECONDS {
        *since_refresh = Some(elapsed);
        return;
    }
    *since_refresh = Some(0.0);
    let read = |path: DiagnosticPath| {
        diagnostics
            .as_ref()
            .and_then(|store| store.get(&path))
            .and_then(|diagnostic| diagnostic.smoothed())
    };
    let fps = read(FrameTimeDiagnosticsPlugin::FPS);
    let frame_ms = read(FrameTimeDiagnosticsPlugin::FRAME_TIME);
    let karts = karts.iter().count();
    for mut text in &mut texts {
        write_debug_overlay(&mut text.0, fps, frame_ms, karts, cost.average_ms);
    }
}
