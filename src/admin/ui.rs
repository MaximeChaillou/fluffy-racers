//! The admin panel's egui interface (issue #36, see `super`). Only with a renderer: the headless
//! tests run the panel's logic without it.
//!
//! **Closed, it costs nothing.** The egui context only exists while the panel is open: it is put
//! on the game's camera when the panel opens, and removed with every egui component (and its font
//! texture) when it closes. Without a context, bevy_egui runs no pass, lays nothing out and draws
//! nothing, and the panel's own system (in egui's pass) does not run at all. What is left each
//! frame: the test of the P key and the small « admin » button of `bevy_ui`.

use std::collections::HashSet;

use bevy::picking::events::PointerClick;
use bevy::prelude::*;
use bevy::render::sync_world::RenderEntity;
use bevy::render::{Extract, ExtractSchedule, RenderApp};
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::render::EguiCameraView;
use bevy_egui::{
    EguiContext, EguiGlobalSettings, EguiManagedTextures, EguiPlugin, EguiPreUpdateSet,
    EguiPrimaryContextPass, EguiRenderOutput, PrimaryEguiContext,
};
use bevy_rapier3d::prelude::Velocity;

use super::tracks::{self, TEST_GROUND_LABEL, TrackEntry};
use super::tuning::{Field, Value, fields};
use super::{AdminOptions, AdminPanel, KeyboardCapture, Tunings, skip_countdown_now, storage};
use crate::camera::ChaseCamera;
use crate::croquettes::Croquettes;
use crate::kart::{
    Boost, DriftSide, KartState, KartTuning, KartWheels, MaxSpeedScale, PlayerKart, Slipstream,
    Stall, max_speed,
};
use crate::level::LoadLevel;
use crate::race::{Race, RaceGrid, RaceMode};
use crate::track::{GRID_SLOTS, TrackChoice};

/// Color of a modified value.
const MODIFIED: Color32 = Color32::from_rgb(255, 175, 60);

/// Values only read when the level is built: they apply at the next « Relancer » (prefixes of
/// `Resource.path`).
const ON_RESTART: &[&str] = &[
    "KartTuning.mass",
    "KartTuning.center_of_mass_height",
    "KartTuning.inertia",
    "RaceTuning.laps",
    "RaceTuning.grid_seconds",
    "RaceTuning.countdown_",
    "RaceTuning.go_display_seconds",
    "RaceTuning.finish_seconds",
    "RaceTuning.restart_delay_seconds",
    "RaceTuning.backward_window",
    "RaceTuning.gate_",
    "BotTuning.grid_difficulty",
    "BotTuning.pace_spread",
    "BotTuning.road_margin",
    "BotTuning.sand_margin",
    "BotTuning.line_margin",
];

/// What an index means, for the arrays of the tunings.
fn hint(path: &str) -> Option<&'static str> {
    if path.starts_with("skills[") {
        Some("skills[0] = Facile, [1] = Normal, [2] = Difficile")
    } else if path.starts_with("mini_turbo_") {
        Some("[0] = bleu, [1] = orange, [2] = violet")
    } else {
        None
    }
}

pub struct AdminUiPlugin;

impl Plugin for AdminUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.insert_resource(EguiGlobalSettings {
            // The context goes on the camera only while the panel is open.
            auto_create_primary_context: false,
            ..default()
        })
        .init_resource::<PanelState>()
        .add_systems(PreStartup, load_stored_tunings)
        .add_systems(Startup, (open_if_asked, spawn_admin_button))
        .add_systems(
            PreUpdate,
            attach_egui_context
                .before(EguiPreUpdateSet::InitContexts)
                .run_if(
                    resource_changed::<AdminPanel>.or_else(any_match_filter::<Added<ChaseCamera>>),
                ),
        )
        .add_systems(
            Update,
            show_admin_button.run_if(resource_changed::<AdminPanel>),
        )
        .add_systems(
            Last,
            browser_keys_while_typing.run_if(resource_changed::<KeyboardCapture>),
        )
        .add_systems(EguiPrimaryContextPass, panel);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.add_systems(ExtractSchedule, unlink_closed_egui_views);
        }
    }
}

/// bevy_egui links a camera to its egui view in the render world, and only drops that link when
/// the camera turns inactive. The panel removes the egui context from the game's camera, which
/// stays active: drop the link here, or the render world would look for the view of a context
/// that no longer exists (and panic).
fn unlink_closed_egui_views(
    mut commands: Commands,
    cameras: Extract<Query<(RenderEntity, Has<EguiRenderOutput>), With<Camera>>>,
    linked: Query<(), With<EguiCameraView>>,
) {
    for (render_entity, has_egui) in &cameras {
        if !has_egui && linked.contains(render_entity) {
            commands.entity(render_entity).remove::<EguiCameraView>();
        }
    }
}

/// What the panel remembers while the game runs (its egui context does not survive a close).
#[derive(Resource)]
pub(super) struct PanelState {
    circuits: Vec<TrackEntry>,
    /// Open sections and tunings.
    open: HashSet<&'static str>,
    filter: String,
    /// The export text, refreshed when a value changes.
    export: String,
    /// Changes not stored yet (stored once the slider is released).
    unsaved: bool,
    paste: String,
    message: String,
    /// When « Copier mes réglages » was last clicked (egui time, s).
    copied_at: Option<f64>,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            circuits: tracks::circuits(),
            open: HashSet::from(["Test", "Réglages", "KartTuning"]),
            filter: String::new(),
            export: String::new(),
            unsaved: false,
            paste: String::new(),
            message: String::new(),
            copied_at: None,
        }
    }
}

/// The values kept from the previous sessions, before anything is built.
fn load_stored_tunings(world: &mut World) {
    let Some(text) = storage::load() else {
        return;
    };
    world.resource_scope(
        |world, tunings: Mut<Tunings>| match tunings.import(world, &text) {
            Ok(report) => {
                info!(
                    "admin panel: {} stored tuning values loaded",
                    report.applied
                );
                for ignored in &report.ignored {
                    warn!("admin panel: stored value ignored: {ignored}");
                }
            }
            Err(error) => warn!("admin panel: stored tunings ignored: {error}"),
        },
    );
}

fn open_if_asked(mut panel: ResMut<AdminPanel>) {
    if storage::admin_requested() {
        panel.open = true;
    }
}

/// Puts the egui context on the game's camera when the panel opens, removes it (with every egui
/// component and the font texture) when it closes. Closing also gives the game back its normal
/// speed and the keyboard.
pub(super) fn attach_egui_context(
    panel: Res<AdminPanel>,
    mut commands: Commands,
    cameras: Query<(Entity, Has<PrimaryEguiContext>), With<ChaseCamera>>,
    mut textures: Option<ResMut<EguiManagedTextures>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut capture: ResMut<KeyboardCapture>,
    mut state: Option<ResMut<PanelState>>,
    mut time: Option<ResMut<Time<Virtual>>>,
) {
    for (camera, attached) in &cameras {
        if panel.open && !attached {
            commands.entity(camera).insert(PrimaryEguiContext);
        } else if !panel.open && attached {
            commands
                .entity(camera)
                .remove_with_requires::<PrimaryEguiContext>();
            if let Some(textures) = textures.as_mut() {
                textures.retain(|(owner, _), texture| {
                    if *owner != camera {
                        return true;
                    }
                    if let Some(images) = images.as_mut() {
                        images.remove(texture.handle.id());
                    }
                    false
                });
            }
        }
    }
    if !panel.open {
        capture.set_if_neq(KeyboardCapture(false));
        if let Some(state) = state.as_mut() {
            state.export.clear();
        }
        if let Some(time) = time.as_mut()
            && (time.is_paused() || time.relative_speed() != 1.0)
        {
            time.unpause();
            time.set_relative_speed(1.0);
        }
    }
}

/// The game's window stops the browser's default action for every key (`Window::
/// prevent_default_event_handling`): the browser then sends no `paste` event for Cmd+V or Ctrl+V,
/// and egui gets nothing to paste. While a text field of the panel has the keyboard, the browser
/// handles the keys again: pasting a text of settings works, as does copying a selection.
#[cfg(target_arch = "wasm32")]
fn browser_keys_while_typing(
    capture: Res<KeyboardCapture>,
    windows: Query<Entity, With<bevy::window::PrimaryWindow>>,
) {
    use winit::platform::web::WindowExtWebSys;
    let Ok(entity) = windows.single() else {
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|winit_windows| {
        if let Some(window) = winit_windows.get_window(entity) {
            window.set_prevent_default(!capture.0);
        }
    });
}

/// Natively the clipboard goes through the system: nothing to do.
#[cfg(not(target_arch = "wasm32"))]
fn browser_keys_while_typing() {}

/// The small button that opens the panel (bottom of the screen).
#[derive(Component, Clone, Copy, Debug)]
struct AdminButton;

fn spawn_admin_button(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Admin button row"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                bottom: px(4),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(9),
        ))
        .with_children(|row| {
            row.spawn((
                AdminButton,
                Node {
                    padding: UiRect::axes(px(8), px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            ))
            .with_children(|button| {
                button.spawn((
                    Text::new("admin (P)"),
                    TextFont::from_font_size(12.0),
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                    // The click is the button's, not its text's.
                    Pickable::IGNORE,
                ));
            })
            .observe(click_admin_button);
        });
}

fn click_admin_button(_click: On<PointerClick>, mut panel: ResMut<AdminPanel>) {
    panel.open = !panel.open;
}

fn show_admin_button(
    panel: Res<AdminPanel>,
    mut buttons: Query<&mut Visibility, With<AdminButton>>,
) {
    for mut visibility in &mut buttons {
        visibility.set_if_neq(if panel.open {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// The panel, in egui's pass (it only runs while the panel is open: there is no egui context
/// otherwise).
fn panel(world: &mut World) {
    let ctx = {
        let mut contexts = world.query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>();
        let Some(mut context) = contexts.iter_mut(world).next() else {
            return;
        };
        context.get_mut().clone()
    };
    // Sliders, check boxes and lists keep the keyboard focus after a click, and egui would then
    // move them with the arrow keys the player drives with: only text fields keep it.
    if let Some(focused) = ctx.memory(|m| m.focused())
        && !ctx.text_edit_focused()
    {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
    world.resource_scope(|world, mut state: Mut<PanelState>| {
        world.resource_scope(|world, tunings: Mut<Tunings>| {
            draw(&ctx, world, &mut state, &tunings);
        });
    });
    let capture = ctx.text_edit_focused() || ctx.any_popup_open();
    world
        .resource_mut::<KeyboardCapture>()
        .set_if_neq(KeyboardCapture(capture));
}

fn draw(ctx: &egui::Context, world: &mut World, state: &mut PanelState, tunings: &Tunings) {
    if state.export.is_empty() {
        state.export = tunings.export(world);
    }
    let mut changed = false;
    let mut open = true;
    let viewport = ctx.viewport_rect();
    egui::Window::new("Panneau d'admin")
        .id(egui::Id::new("fluffy-admin"))
        .open(&mut open)
        .default_pos(egui::pos2((viewport.width() - 470.0).max(0.0), 8.0))
        .default_width(460.0)
        .default_height((viewport.height() - 40.0).max(200.0))
        .resizable(true)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.small("P : ouvrir / fermer · ?admin dans l'adresse : ouvert au démarrage");
            section(ui, state, "Test", "Test", |ui, state| {
                test_section(ui, world, state, tunings, &mut changed);
            });
            section(
                ui,
                state,
                "Télémétrie",
                "Télémétrie (ton kart)",
                |ui, _| {
                    telemetry_section(ui, world);
                },
            );
            section(ui, state, "Réglages", "Réglages", |ui, state| {
                tunings_section(ui, world, state, tunings, &mut changed);
            });
            section(
                ui,
                state,
                "Partager",
                "Partager (export / import)",
                |ui, state| {
                    share_section(ui, world, state, tunings, &mut changed);
                },
            );
        });
    if !open {
        world.resource_mut::<AdminPanel>().open = false;
    }
    if changed {
        state.export = tunings.export(world);
        state.unsaved = true;
    }
    // Stored once the slider is released, not at every step of a drag.
    if state.unsaved && !ctx.egui_is_using_pointer() {
        state.unsaved = false;
        let nothing_modified = tunings.changes(world).is_empty();
        storage::save(if nothing_modified { "" } else { &state.export });
    }
}

/// A collapsible section whose open state survives the panel being closed.
fn section(
    ui: &mut egui::Ui,
    state: &mut PanelState,
    key: &'static str,
    title: impl Into<egui::WidgetText>,
    add: impl FnOnce(&mut egui::Ui, &mut PanelState),
) {
    let open = state.open.contains(key);
    let response = egui::CollapsingHeader::new(title)
        .id_salt(key)
        .open(Some(open))
        .show(ui, |ui| add(ui, state));
    if response.header_response.clicked() {
        toggle(&mut state.open, key);
    }
}

fn toggle(open: &mut HashSet<&'static str>, key: &'static str) {
    if !open.remove(key) {
        open.insert(key);
    }
}

fn test_section(
    ui: &mut egui::Ui,
    world: &mut World,
    state: &mut PanelState,
    tunings: &Tunings,
    changed: &mut bool,
) {
    let choice = world.resource::<TrackChoice>().clone();
    let mode = *world.resource::<RaceMode>();
    let mut request = None;

    egui::Grid::new("admin-test")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Circuit");
            let mut selected = choice.clone();
            egui::ComboBox::from_id_salt("admin-circuit")
                .width(220.0)
                .selected_text(tracks::label(&choice, &state.circuits))
                .show_ui(ui, |ui| {
                    for entry in &state.circuits {
                        ui.selectable_value(&mut selected, entry.choice(), &entry.name);
                    }
                    ui.selectable_value(&mut selected, TrackChoice::TestGround, TEST_GROUND_LABEL);
                });
            if selected != choice {
                request = Some(LoadLevel::on(selected, mode));
            }
            ui.end_row();

            ui.label("Mode");
            ui.horizontal(|ui| {
                let mut selected = mode;
                ui.radio_value(&mut selected, RaceMode::Race, "Course");
                ui.radio_value(&mut selected, RaceMode::FreeRoam, "Roulage libre");
                if selected != mode {
                    request = Some(LoadLevel::restart(selected));
                }
            });
            ui.end_row();

            ui.label("Bots");
            ui.horizontal(|ui| {
                let mut bots = world.resource::<RaceGrid>().bot_count();
                if ui
                    .add(egui::Slider::new(&mut bots, 0..=GRID_SLOTS - 1))
                    .changed()
                {
                    world.resource_mut::<RaceGrid>().bots = bots;
                }
                let difficulty = tunings
                    .get("BotTuning")
                    .and_then(|t| t.read(world))
                    .map(|r| fields(r.as_partial_reflect()))
                    .and_then(|f| f.into_iter().find(|f| f.path == "grid_difficulty"));
                if let Some(field) = difficulty
                    && let Some(value) =
                        variant_combo(ui, ("admin-difficulty", ""), &field, difficulty_label)
                    && tunings
                        .set(world, "BotTuning.grid_difficulty", value)
                        .is_ok()
                {
                    *changed = true;
                }
            });
            ui.end_row();
        });
    ui.small("Bots et niveau : appliqués au prochain « Relancer ».");

    ui.horizontal(|ui| {
        if ui.button("Relancer").clicked() {
            request = Some(LoadLevel::restart(mode));
        }
        let can_go = world
            .get_resource::<Race>()
            .is_some_and(|race| !race.started());
        if ui.add_enabled(can_go, egui::Button::new("GO !")).clicked() {
            skip_countdown_now(world);
        }
        let mut options = *world.resource::<AdminOptions>();
        if ui
            .checkbox(
                &mut options.skip_countdown,
                "Toujours sauter le compte à rebours",
            )
            .changed()
        {
            *world.resource_mut::<AdminOptions>() = options;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Vitesse du jeu");
        let (speed, paused) = {
            let time = world.resource::<Time<Virtual>>();
            (time.relative_speed(), time.is_paused())
        };
        for (value, label) in [(1.0, "100 %"), (0.5, "50 %"), (0.25, "25 %"), (0.1, "10 %")] {
            if ui
                .selectable_label((speed - value).abs() < 1e-3, label)
                .clicked()
            {
                world
                    .resource_mut::<Time<Virtual>>()
                    .set_relative_speed(value);
            }
        }
        let mut pause = paused;
        if ui.checkbox(&mut pause, "Pause").changed() {
            let mut time = world.resource_mut::<Time<Virtual>>();
            if pause {
                time.pause();
            } else {
                time.unpause();
            }
        }
    });
    ui.small("Fermer le panneau remet la vitesse normale.");

    if let Some(request) = request {
        world.write_message(request);
    }
}

fn difficulty_label(name: &str) -> &str {
    match name {
        "Easy" => "Facile",
        "Normal" => "Normal",
        "Hard" => "Difficile",
        other => other,
    }
}

fn telemetry_section(ui: &mut egui::Ui, world: &mut World) {
    let mut karts = world.query_filtered::<(
        &Velocity,
        &Transform,
        &KartState,
        &Boost,
        &Stall,
        &MaxSpeedScale,
        &Slipstream,
        &KartWheels,
        Option<&Croquettes>,
    ), With<PlayerKart>>();
    let tuning = world.resource::<KartTuning>();
    let Some((velocity, transform, state, boost, stall, scale, slipstream, wheels, croquettes)) =
        karts.iter(world).next()
    else {
        ui.label("Pas de kart joueur.");
        return;
    };
    let kmh = |ms: f32| ms * 3.6;
    let speed = velocity.linear.length();
    let forward = velocity.linear.dot(*transform.forward());
    let top = max_speed(tuning, wheels.offroad_ratio, boost) * scale.0;
    let side = |side: DriftSide| match side {
        DriftSide::Left => "gauche",
        DriftSide::Right => "droite",
    };
    let state_text = match *state {
        KartState::Grounded => "au sol".to_string(),
        KartState::Hop { ticks, .. } => format!("petit saut ({ticks} ticks)"),
        KartState::Drifting { side: s, charge } => {
            let tier = match state.mini_turbo(tuning, false) {
                Some(tier) => format!("{tier:?}"),
                None => "aucun palier".to_string(),
            };
            format!("drift à {} : charge {charge:.0} ({tier})", side(s))
        }
        KartState::Airborne => "en l'air".to_string(),
        KartState::Trick => "figure".to_string(),
        KartState::Respawning => "pélican".to_string(),
    };
    let thresholds = tuning.mini_turbo_thresholds;
    egui::Grid::new("admin-telemetry")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String| {
                ui.label(label);
                ui.monospace(value);
                ui.end_row();
            };
            row(
                "Vitesse",
                format!("{:5.1} km/h  ({speed:5.2} m/s)", kmh(speed)),
            );
            row("Vers l'avant", format!("{forward:5.2} m/s"));
            row(
                "Vitesse max",
                format!("{:5.1} km/h  (×{:.3})", kmh(top), scale.0),
            );
            row("État", state_text);
            row(
                "Paliers du drift",
                format!(
                    "{:.0} / {:.0} / {:.0}",
                    thresholds[0], thresholds[1], thresholds[2]
                ),
            );
            row(
                "Boost",
                if boost.is_active() {
                    format!(
                        "+{:.0} % encore {:.2} s",
                        boost.strength * 100.0,
                        boost.remaining
                    )
                } else {
                    "-".to_string()
                },
            );
            if stall.is_active() {
                row("Calage", format!("encore {:.2} s", stall.remaining));
            }
            row(
                "Aspiration",
                format!("{:.0} %", slipstream.fill(tuning) * 100.0),
            );
            row(
                "Roues au sol",
                format!(
                    "{} / 4, hors-piste {:.0} %",
                    wheels.wheels_on_ground(),
                    wheels.offroad_ratio * 100.0
                ),
            );
            if let Some(held) = croquettes {
                row("Croquettes", held.count.to_string());
            }
        });
}

fn tunings_section(
    ui: &mut egui::Ui,
    world: &mut World,
    state: &mut PanelState,
    tunings: &Tunings,
    changed: &mut bool,
) {
    let modified = tunings.changes(world).len();
    ui.horizontal(|ui| {
        let now = ui.input(|i| i.time);
        if ui.button("Copier mes réglages").clicked() {
            ui.ctx().copy_text(state.export.clone());
            state.copied_at = Some(now);
            state.open.insert("Partager");
        }
        if state.copied_at.is_some_and(|at| now - at < 4.0) {
            ui.label(RichText::new("copié !").color(MODIFIED));
        }
        if ui
            .add_enabled(modified > 0, egui::Button::new("Tout par défaut"))
            .clicked()
        {
            tunings.reset_all(world);
            *changed = true;
        }
        let text = match modified {
            0 => "aucune valeur modifiée".to_string(),
            1 => "1 valeur modifiée".to_string(),
            n => format!("{n} valeurs modifiées"),
        };
        ui.label(RichText::new(text).color(if modified > 0 {
            MODIFIED
        } else {
            ui.visuals().weak_text_color()
        }));
    });
    ui.horizontal(|ui| {
        ui.label("Filtrer");
        ui.add(egui::TextEdit::singleline(&mut state.filter).desired_width(160.0));
        if !state.filter.is_empty() && ui.small_button("effacer").clicked() {
            state.filter.clear();
        }
    });
    ui.small("* : s'applique au prochain « Relancer ». Survole un nom pour sa valeur par défaut.");

    let filter = state.filter.trim().to_lowercase();
    for tuning in &tunings.list {
        let Some(resource) = tuning.read(world) else {
            continue;
        };
        let current = fields(resource.as_partial_reflect());
        let shown: Vec<&Field> = current
            .iter()
            .filter(|f| filter.is_empty() || f.path.to_lowercase().contains(&filter))
            .collect();
        if shown.is_empty() {
            continue;
        }
        let modified_here = current
            .iter()
            .filter(|f| tuning.default_of(&f.path).map(|d| d.value) != Some(f.value))
            .count();
        let title = if modified_here > 0 {
            RichText::new(format!("{} ({modified_here} modifiée(s))", tuning.name)).color(MODIFIED)
        } else {
            RichText::new(tuning.name)
        };
        // A filter shows every tuning with a match.
        let open = !filter.is_empty() || state.open.contains(tuning.name);
        let mut edits: Vec<(String, Value)> = Vec::new();
        let mut reset = false;
        let response = egui::CollapsingHeader::new(title)
            .id_salt(tuning.name)
            .open(Some(open))
            .show(ui, |ui| {
                if ui
                    .add_enabled(
                        modified_here > 0,
                        egui::Button::new("défaut (tout)").small(),
                    )
                    .clicked()
                {
                    reset = true;
                }
                egui::Grid::new(("admin-fields", tuning.name))
                    .num_columns(3)
                    .striped(true)
                    .spacing([8.0, 2.0])
                    .show(ui, |ui| {
                        ui.spacing_mut().slider_width = 130.0;
                        for field in shown {
                            let default = tuning.default_of(&field.path);
                            let is_modified = default.map(|d| d.value) != Some(field.value);
                            let full = format!("{}.{}", tuning.name, field.path);
                            let on_restart = ON_RESTART.iter().any(|p| full.starts_with(p));
                            let mut name = field.path.clone();
                            if on_restart {
                                name.push_str(" *");
                            }
                            let label = if is_modified {
                                RichText::new(name).color(MODIFIED).strong()
                            } else {
                                RichText::new(name)
                            };
                            let mut tip = match default {
                                Some(d) => format!("défaut : {}", d.value),
                                None => "pas de valeur par défaut".to_string(),
                            };
                            if on_restart {
                                tip.push_str("\ns'applique au prochain « Relancer »");
                            }
                            if let Some(hint) = hint(&field.path) {
                                tip.push('\n');
                                tip.push_str(hint);
                            }
                            ui.label(label).on_hover_text(tip);
                            if let Some(value) =
                                edit(ui, (tuning.name, &field.path), field, default)
                            {
                                edits.push((field.path.clone(), value));
                            }
                            if ui
                                .add_enabled(is_modified, egui::Button::new("défaut").small())
                                .clicked()
                                && let Some(d) = default
                            {
                                edits.push((field.path.clone(), d.value));
                            }
                            ui.end_row();
                        }
                    });
            });
        if response.header_response.clicked() && filter.is_empty() {
            toggle(&mut state.open, tuning.name);
        }
        if reset {
            tuning.reset(world);
            *changed = true;
        }
        for (path, value) in edits {
            if tuning.set(world, &path, value).is_ok() {
                *changed = true;
            }
        }
    }
}

/// The widget of one value; returns the new value when it was edited.
fn edit(
    ui: &mut egui::Ui,
    id: (&str, &str),
    field: &Field,
    default: Option<&Field>,
) -> Option<Value> {
    match field.value {
        Value::Bool(mut on) => ui
            .checkbox(&mut on, "")
            .changed()
            .then_some(Value::Bool(on)),
        Value::Variant(_) => variant_combo(ui, id, field, |name| name),
        number => {
            let mut x = number.as_f64()?;
            let reference = default.and_then(|d| d.value.as_f64()).unwrap_or(x);
            let range = slider_range(reference, x);
            let mut slider = egui::Slider::new(&mut x, range).clamping(egui::SliderClamping::Never);
            slider = if number.is_integer() {
                slider.integer()
            } else {
                slider.max_decimals(4)
            };
            if ui.add(slider).changed() {
                number.with_number(x)
            } else {
                None
            }
        }
    }
}

/// Range of a slider: 0 to twice the default (around 0 for a negative default), widened to the
/// current value. Typing a value outside is still possible.
fn slider_range(default: f64, current: f64) -> std::ops::RangeInclusive<f64> {
    let span = if default == 0.0 {
        1.0
    } else {
        default.abs() * 2.0
    }
    .max(current.abs());
    if default < 0.0 || current < 0.0 {
        -span..=span
    } else {
        0.0..=span
    }
}

fn variant_combo(
    ui: &mut egui::Ui,
    id: (&str, &str),
    field: &Field,
    label: fn(&str) -> &str,
) -> Option<Value> {
    let Value::Variant(current) = field.value else {
        return None;
    };
    let mut selected = current;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(current))
        .show_ui(ui, |ui| {
            for variant in field.variants {
                ui.selectable_value(&mut selected, *variant, label(variant));
            }
        });
    (selected != current).then_some(Value::Variant(selected))
}

fn share_section(
    ui: &mut egui::Ui,
    world: &mut World,
    state: &mut PanelState,
    tunings: &Tunings,
    changed: &mut bool,
) {
    ui.label("Mes réglages, à coller dans une issue (seulement les valeurs modifiées) :");
    let mut export = state.export.as_str();
    ui.add(
        egui::TextEdit::multiline(&mut export)
            .font(egui::TextStyle::Monospace)
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
    if ui.button("Copier mes réglages").clicked() {
        ui.ctx().copy_text(state.export.clone());
        state.message = "Copié ! Sinon, sélectionne le texte ci-dessus (Cmd+A, Cmd+C).".to_string();
    }
    ui.separator();
    ui.label("Coller ici des réglages, puis « Charger » (tout le reste repasse par défaut) :");
    ui.add(
        egui::TextEdit::multiline(&mut state.paste)
            .font(egui::TextStyle::Monospace)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    if ui
        .add_enabled(!state.paste.trim().is_empty(), egui::Button::new("Charger"))
        .clicked()
    {
        state.message = match tunings.import(world, &state.paste) {
            Ok(report) => {
                *changed = true;
                let mut message = format!("{} valeur(s) chargée(s).", report.applied);
                for ignored in report.ignored {
                    message.push_str("\nIgnoré : ");
                    message.push_str(&ignored);
                }
                message
            }
            Err(error) => format!("Texte illisible, rien n'a changé : {error}"),
        };
    }
    if !state.message.is_empty() {
        ui.label(&state.message);
    }
}
