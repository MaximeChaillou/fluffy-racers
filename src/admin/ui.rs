//! The admin panel's egui interface (issue #36, see `super`). Only with a renderer: the headless
//! tests run the panel's logic without it.
//!
//! **Closed, it costs nothing.** The egui context only exists while the panel is open: it is put
//! on the game's camera when the panel opens, and removed with every egui component (and its font
//! texture) when it closes. Without a context, bevy_egui runs no pass, lays nothing out and draws
//! nothing, and the panel's own system (in egui's pass) does not run at all. What is left each
//! frame: the test of the P key, the small « admin » button of `bevy_ui`, and (issue #80) the
//! look for a text field of another window with the keyboard (none without a context).

use std::collections::HashSet;

use bevy::picking::events::PointerClick;
use bevy::prelude::*;
use bevy::render::sync_world::RenderEntity;
use bevy::render::{Extract, ExtractSchedule, RenderApp};
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::render::EguiCameraView;
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiManagedTextures, EguiPlugin,
    EguiPostUpdateSet, EguiPreUpdateSet, EguiPrimaryContextPass, EguiRenderOutput,
    PrimaryEguiContext,
};
use bevy_rapier3d::prelude::Velocity;

use super::feel::{KNOB_MAX, Knob, LINKS, MIDDLE};
use super::tracks::{self, TEST_GROUND_LABEL, TrackEntry};
use super::tuning::{Field, Value, fields};
use super::{AdminOptions, AdminPanel, KeyboardCapture, Tunings, skip_countdown_now, storage};
use crate::audio::{Group, PreviewSound, Slot as SoundSlot, SoundChoices};
use crate::bots::BotDifficulty;
use crate::camera::ChaseCamera;
use crate::characters::{PartKind, PlayerLoadout, Roster, SelectionScreen};
use crate::croquettes::Croquettes;
use crate::items::{GiveItem, ItemSlots, ItemStats, Prize, Slot};
use crate::kart::{
    Boost, DriftSide, HitReaction, Invincibility, ItemEffects, KartState, KartStats, KartTuning,
    KartWheels, MaxSpeedScale, PlayerKart, Slipstream, Stall, Stat, StatEffects, max_speed,
};
use crate::level::LoadLevel;
use crate::options::OptionsMenu;
use crate::race::{Race, RaceGrid, RaceMode};
use crate::track::{GRID_SLOTS, TrackChoice};

/// Color of a value modified by hand, and of a knob off the middle.
const MODIFIED: Color32 = Color32::from_rgb(255, 175, 60);
/// Issue #155: color of a value set by a « Ressenti » knob (off the middle).
const DRIVEN: Color32 = Color32::from_rgb(120, 190, 255);

/// Values only read when the level is built: they apply at the next « Relancer » (prefixes of
/// `Resource.path`).
const ON_RESTART: &[&str] = &[
    "DriverTuning.model_scale",
    "DriverTuning.seat_sink",
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
    "BotTuning.pace_spread",
    "BotTuning.road_margin",
    "BotTuning.sand_margin",
    "BotTuning.line_margin",
    // The stats are converted when the karts spawn (issue #15).
    "StatsTuning.",
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
                    resource_changed::<AdminPanel>
                        .or_else(resource_exists_and_changed::<SelectionScreen>)
                        .or_else(resource_exists_and_changed::<OptionsMenu>)
                        // Issue #99: « Jouer solo » (`bevy_ui`, no egui) holds the game too.
                        .or_else(resource_exists_and_changed::<crate::modes::RaceMenu>)
                        .or_else(any_match_filter::<Added<ChaseCamera>>),
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
        // Issue #80: a text field of another window (the lobby's pseudo) has the keyboard too.
        .add_systems(
            PostUpdate,
            capture_for_text_fields
                .after(EguiPostUpdateSet::EndPass)
                .run_if(|panel: Res<AdminPanel>| !panel.open),
        )
        // The egui context also exists for the selection screen (issue #15): the panel only
        // draws itself when it is open.
        .add_systems(
            EguiPrimaryContextPass,
            panel.run_if(|panel: Res<AdminPanel>| panel.open),
        );
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
    /// Issue #135: when « Copier mes sons » was last clicked (egui time, s).
    sounds_copied_at: Option<f64>,
    /// The item « Donner » gives (issue #18).
    gift: Prize,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            circuits: tracks::circuits(),
            // Issue #155: the test tools and the « Ressenti » knobs; « Avancé » folded.
            open: HashSet::from(["Test", "Ressenti", "KartTuning"]),
            filter: String::new(),
            export: String::new(),
            unsaved: false,
            paste: String::new(),
            message: String::new(),
            copied_at: None,
            sounds_copied_at: None,
            gift: Prize::Pelure,
        }
    }
}

/// The values kept from the previous sessions, before anything is built.
fn load_stored_tunings(world: &mut World) {
    let Some(text) = storage::load() else {
        return;
    };
    world.resource_scope(
        |world, mut tunings: Mut<Tunings>| match tunings.import(world, &text) {
            Ok(report) => {
                info!(
                    "admin panel: {} stored tuning values loaded",
                    report.applied
                );
                for ignored in &report.ignored {
                    warn!("admin panel: stored value ignored: {ignored}");
                }
                // A text from an older version, read differently (#170): kept as it reads now,
                // so that it is converted once.
                if !report.converted.is_empty() {
                    for converted in &report.converted {
                        info!("admin panel: stored value converted: {converted}");
                    }
                    let text = if tunings.untouched(world) {
                        String::new()
                    } else {
                        tunings.export(world)
                    };
                    storage::save(&text);
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
/// speed and the keyboard. The game stays paused while the options (issue #100: `bevy_ui`, no
/// egui any more), the « Pilote » screen (issue #98) or « Jouer solo » (issue #99) is open (not
/// the lobby). Issue #101: the lobby's window and its banner are `bevy_ui` too, no egui.
pub(super) fn attach_egui_context(
    panel: Res<AdminPanel>,
    selection: Option<Res<SelectionScreen>>,
    options: Option<Res<OptionsMenu>>,
    race_menu: Option<Res<crate::modes::RaceMenu>>,
    main_menu: Option<Res<crate::menu::MainMenu>>,
    mut commands: Commands,
    cameras: Query<(Entity, Has<PrimaryEguiContext>), With<ChaseCamera>>,
    mut textures: Option<ResMut<EguiManagedTextures>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut capture: ResMut<KeyboardCapture>,
    mut state: Option<ResMut<PanelState>>,
    mut time: Option<ResMut<Time<Virtual>>>,
) {
    let options = options.is_some_and(|menu| menu.open);
    let pilot = selection.as_deref().is_some_and(|screen| screen.open);
    let solo = race_menu.is_some_and(|menu| menu.open);
    let typing = pilot_typing(selection.as_deref());
    let choosing = options || pilot || solo;
    // Issue #101: the lobby's window and its banner are `bevy_ui` now (`crate::salon`): only the
    // panel needs egui.
    let wanted = panel.open;
    for (camera, attached) in &cameras {
        if wanted && !attached {
            commands.entity(camera).insert(PrimaryEguiContext);
        } else if !wanted && attached {
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
    // No egui at all: no egui text field has the keyboard (issue #98: the « Pilote » screen's
    // may). Otherwise the panel (open) or `capture_for_text_fields` (closed) says whether one has
    // it.
    if !wanted {
        capture.set_if_neq(KeyboardCapture(typing));
    }
    if !panel.open {
        if let Some(state) = state.as_mut() {
            state.export.clear();
        }
        if let Some(time) = time.as_mut() {
            if time.relative_speed() != 1.0 {
                time.set_relative_speed(1.0);
            }
            // Issue #56: the main menu (`bevy_ui`, no egui) pauses the game too.
            let held = main_menu.is_some_and(|menu| menu.holds_the_game());
            if time.is_paused() && !choosing && !held {
                time.unpause();
            }
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

/// No widget keeps the keyboard focus but a text field: buttons, sliders, check boxes and lists
/// would otherwise be clicked or moved by the game's keys (the arrows, Espace, Entrée). Every egui
/// window calls it at the start of its pass. A text field keeps it, even one of another window
/// (the panel's): the game then sees no key ([`KeyboardCapture`]).
pub fn keep_focus_for_text_only(ctx: &egui::Context) {
    if let Some(focused) = ctx.memory(|m| m.focused())
        && !ctx.text_edit_focused()
    {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
}

/// Issue #98: the « Pilote » screen's pseudo field (`bevy_ui`, `menu::pilot`) has the keyboard.
fn pilot_typing(selection: Option<&SelectionScreen>) -> bool {
    selection.is_some_and(|screen| screen.open && screen.typing)
}

/// Issue #80: while the panel is closed (open, it decides: its fields and its lists), a text
/// field of any other window takes the keyboard the same way: the game sees no key while one is
/// typed in. After egui's pass: its focus as the frame leaves it. No egui context (no window
/// open): no egui field. Issue #98: nor while the « Pilote » screen's field has it.
fn capture_for_text_fields(
    mut contexts: EguiContexts,
    selection: Option<Res<SelectionScreen>>,
    mut capture: ResMut<KeyboardCapture>,
) {
    let typing = contexts.ctx_mut().is_ok_and(|ctx| ctx.text_edit_focused())
        || pilot_typing(selection.as_deref());
    capture.set_if_neq(KeyboardCapture(typing));
}

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
    keep_focus_for_text_only(&ctx);
    world.resource_scope(|world, mut state: Mut<PanelState>| {
        world.resource_scope(|world, mut tunings: Mut<Tunings>| {
            draw(&ctx, world, &mut state, &mut tunings);
        });
    });
    let capture = ctx.text_edit_focused()
        || ctx.any_popup_open()
        || pilot_typing(world.get_resource::<SelectionScreen>());
    world
        .resource_mut::<KeyboardCapture>()
        .set_if_neq(KeyboardCapture(capture));
}

fn draw(ctx: &egui::Context, world: &mut World, state: &mut PanelState, tunings: &mut Tunings) {
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
            // Issue #155: the test tools first, compact; then the « Ressenti » knobs; every
            // detailed value folded in « Avancé ».
            section(ui, state, "Test", "Test", |ui, state| {
                test_section(ui, world, state);
            });
            section(
                ui,
                state,
                "Sons",
                "Sons (choisir, écouter)",
                |ui, state| {
                    sounds_section(ui, world, state);
                },
            );
            section(ui, state, "Ressenti", "Ressenti", |ui, state| {
                feel_section(ui, world, state, tunings, &mut changed);
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
            section(
                ui,
                state,
                "Avancé",
                "Avancé (tous les réglages)",
                |ui, state| {
                    tunings_section(ui, world, state, tunings, &mut changed);
                },
            );
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
        storage::save(if tunings.untouched(world) {
            ""
        } else {
            &state.export
        });
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

fn test_section(ui: &mut egui::Ui, world: &mut World, state: &mut PanelState) {
    if world.contains_resource::<crate::level::Online>() {
        // Issue #9: online, the host chooses the circuit and starts the races (lobby, L), and the
        // game never pauses nor slows down: every peer's race follows the room's clock.
        ui.label("En ligne : l'hôte choisit le circuit et lance les courses depuis le salon.");
        ui.small("Ni « Relancer », ni pause, ni ralenti en ligne.");
        items_row(ui, world, state);
        return;
    }
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

            ui.label("Bots")
                .on_hover_text("Le nombre et le niveau : au prochain « Relancer ».");
            ui.horizontal(|ui| {
                let mut bots = world.resource::<RaceGrid>().bot_count();
                if ui
                    .add(egui::Slider::new(&mut bots, 0..=GRID_SLOTS - 1))
                    .changed()
                {
                    world.resource_mut::<RaceGrid>().bots = bots;
                }
                // Issue #153: their level is the grid's, like their number: the same as « Jouer
                // solo »'s « Bots » line (the last one changed wins; the screen opens on it).
                let difficulty = world.resource::<RaceGrid>().difficulty;
                let mut selected = difficulty;
                egui::ComboBox::from_id_salt("admin-difficulty")
                    .selected_text(difficulty_label(difficulty))
                    .show_ui(ui, |ui| {
                        for level in BotDifficulty::ALL {
                            ui.selectable_value(&mut selected, level, difficulty_label(level));
                        }
                    });
                if selected != difficulty {
                    world.resource_mut::<RaceGrid>().difficulty = selected;
                }
            });
            ui.end_row();
        });

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
        ui.label("Vitesse du jeu")
            .on_hover_text("Fermer le panneau remet la vitesse normale.");
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

    // Issue #155: giving an item is a test tool too, on one line.
    items_row(ui, world, state);

    if let Some(request) = request {
        world.write_message(request);
    }
}

/// Issue #18: any item, ready in the player's hand (`items::GiveItem`), to try it at once (the
/// debug key goes through them all, GD § 8). Online, only in a debug build, and only what plays
/// online. Issue #106: the showcase, every model side by side in front of the camera. Issue
/// #155: one line of the « Test » section.
fn items_row(ui: &mut egui::Ui, world: &mut World, state: &mut PanelState) {
    let online = world.contains_resource::<crate::level::Online>();
    let can_give = !online || cfg!(debug_assertions);
    let playable = |prize: &Prize| !online || prize.online();
    if !playable(&state.gift) {
        state.gift = Prize::Pelure;
    }
    ui.horizontal(|ui| {
        ui.label("Objet");
        if can_give {
            egui::ComboBox::from_id_salt("admin-gift")
                .width(150.0)
                .selected_text(state.gift.name())
                .show_ui(ui, |ui| {
                    for prize in Prize::ALL.iter().filter(|p| playable(p)) {
                        ui.selectable_value(&mut state.gift, *prize, prize.name());
                    }
                });
            if ui
                .button("Donner")
                .on_hover_text("Dans ta main, prêt à servir.")
                .clicked()
            {
                world.write_message(GiveItem(state.gift));
            }
        } else {
            ui.label("aucun en ligne")
                .on_hover_text("En ligne, seulement dans une version de développement.");
        }
        // Issue #106 (with a renderer).
        if let Some(mut showcase) = world.get_resource_mut::<crate::items::ItemShowcase>() {
            let mut open = showcase.open;
            if ui
                .checkbox(&mut open, "Vitrine")
                .on_hover_text("Tous les objets côte à côte devant la caméra.")
                .changed()
            {
                showcase.open = open;
            }
        }
    });
}

/// Issue #135: each sound of the sound design (`audio::Slot`) with its candidates; the choice
/// plays at once (in a race too: the engine and the squeal change at once) and is kept in the
/// browser (`audio::SoundChoicesStoragePlugin`); « ▶ Écouter » plays the sound chosen, even out of
/// a race (the engine: a short rev); « Copier mes sons » gives the text to paste in an issue.
fn sounds_section(ui: &mut egui::Ui, world: &mut World, state: &mut PanelState) {
    let Some(choices) = world.get_resource::<SoundChoices>().cloned() else {
        ui.label("Pas de son ici (le jeu sans sortie audio).");
        return;
    };
    let mut chosen = choices.clone();
    let mut listen = None;
    let changed = choices.changed().count();
    ui.horizontal(|ui| {
        let now = ui.input(|i| i.time);
        if ui.button("Copier mes sons").clicked() {
            ui.ctx().copy_text(choices.export());
            state.sounds_copied_at = Some(now);
        }
        if state.sounds_copied_at.is_some_and(|at| now - at < 4.0) {
            ui.label(RichText::new("copié !").color(MODIFIED));
        }
        if ui
            .add_enabled(changed > 0, egui::Button::new("Tous par défaut"))
            .clicked()
        {
            chosen = SoundChoices::default();
        }
        let text = match changed {
            0 => "tous les sons par défaut".to_string(),
            1 => "1 son changé".to_string(),
            n => format!("{n} sons changés"),
        };
        ui.label(RichText::new(text).color(if changed > 0 {
            MODIFIED
        } else {
            ui.visuals().weak_text_color()
        }));
    });
    ui.small(
        "Le son choisi s'entend tout de suite, en course aussi, et reste dans le navigateur. \
         « ▶ Écouter » le joue, même hors course. Survole un nom pour le son par défaut.",
    );
    for group in Group::ALL {
        // Not the label: « Objets » is the key of the panel's own section of the items.
        let key = group.key();
        let open = state.open.contains(key);
        let response = egui::CollapsingHeader::new(group.label())
            .id_salt(("admin-sounds", key))
            .open(Some(open))
            .show(ui, |ui| {
                egui::Grid::new(("admin-sounds-grid", key))
                    .num_columns(3)
                    .striped(true)
                    .spacing([8.0, 3.0])
                    .show(ui, |ui| {
                        for slot in SoundSlot::ALL.into_iter().filter(|s| s.group() == group) {
                            let candidates = slot.candidates();
                            let index = chosen.index(slot);
                            let name = if index != 0 {
                                RichText::new(slot.label()).color(MODIFIED).strong()
                            } else {
                                RichText::new(slot.label())
                            };
                            ui.label(name)
                                .on_hover_text(format!("par défaut : {}", candidates[0].name));
                            let mut selected = index;
                            egui::ComboBox::from_id_salt(("admin-sound", slot.id()))
                                .width(280.0)
                                .selected_text(candidates[index].name)
                                .show_ui(ui, |ui| {
                                    for (i, candidate) in candidates.iter().enumerate() {
                                        ui.selectable_value(&mut selected, i, candidate.name);
                                    }
                                });
                            if selected != index && chosen.choose(slot, selected) {
                                // The new one, heard at once.
                                listen = Some(slot);
                            }
                            if ui.button("▶ Écouter").clicked() {
                                listen = Some(slot);
                            }
                            ui.end_row();
                        }
                    });
            });
        if response.header_response.clicked() {
            toggle(&mut state.open, key);
        }
    }
    ui.label("Mes sons, à coller dans une issue (seulement ceux qui ne sont pas par défaut) :");
    let export = choices.export();
    let mut shown = export.as_str();
    ui.add(
        egui::TextEdit::multiline(&mut shown)
            .font(egui::TextStyle::Monospace)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    if chosen != choices
        && let Some(mut choices) = world.get_resource_mut::<SoundChoices>()
    {
        *choices = chosen;
    }
    if let Some(slot) = listen {
        world.write_message(PreviewSound(slot));
    }
}

fn difficulty_label(difficulty: BotDifficulty) -> &'static str {
    match difficulty {
        BotDifficulty::Easy => "Facile",
        BotDifficulty::Normal => "Normal",
        BotDifficulty::Hard => "Difficile",
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
        &KartStats,
        &StatEffects,
        Option<&Croquettes>,
        (
            &Invincibility,
            Option<&ItemSlots>,
            Option<&ItemStats>,
            Option<&ItemEffects>,
        ),
    ), With<PlayerKart>>();
    let base = world.resource::<KartTuning>();
    let Some((
        velocity,
        transform,
        state,
        boost,
        stall,
        scale,
        slipstream,
        wheels,
        stats,
        effects,
        croquettes,
        (invincibility, items, item_stats, item_effects),
    )) = karts.iter(world).next()
    else {
        ui.label("Pas de kart joueur.");
        return;
    };
    // The player's kart drives with its stats applied (issue #15).
    let kart_tuning = effects.tuning(base);
    let tuning = &*kart_tuning;
    let loadout = world
        .get_resource::<Roster>()
        .zip(world.get_resource::<PlayerLoadout>())
        .map(|(roster, loadout)| {
            PartKind::ALL
                .map(|kind| roster.name(kind, loadout.get(kind)).to_string())
                .join(" · ")
        });
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
        KartState::Hit {
            reaction,
            elapsed,
            duration,
        } => format!(
            "touché ({}, {:.2} / {:.2} s)",
            match reaction {
                HitReaction::Spin => "tête-à-queue",
                HitReaction::Tumble => "culbute",
            },
            elapsed as f32 / 60.0,
            duration as f32 / 60.0
        ),
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
            if let Some(loadout) = &loadout {
                row("Pilote", loadout.clone());
            }
            row(
                "Stats (niveaux)",
                Stat::ALL
                    .iter()
                    .map(|stat| stats.level(*stat).to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
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
            if invincibility.is_active() {
                row(
                    "Invincible",
                    format!("encore {:.2} s", invincibility.remaining),
                );
            }
            if let Some(items) = items {
                let slot = |slot: &Slot| match *slot {
                    Slot::Empty => "-".to_string(),
                    Slot::Spinning { ticks, .. } => format!("roulette ({ticks} ticks)"),
                    Slot::Ready { item, count } => format!("{} x{count}", item.label()),
                };
                let mut text = format!("{} | {}", slot(&items.hand), slot(&items.reserve));
                if let Some((item, count)) = items.held_out() {
                    text.push_str(&format!(" ; dehors : {} x{count}", item.label()));
                }
                if items.golden > 0 {
                    text.push_str(&format!(
                        " ; piment doré encore {:.1} s",
                        items.golden as f32 / 60.0
                    ));
                }
                row("Objets (main | réserve)", text);
            }
            // Issue #18: what the items of phase 2 do to the kart.
            if let Some(e) = item_effects.filter(|e| e.is_active()) {
                let mut text = String::new();
                for (name, left) in [
                    ("super câlin", e.calin),
                    ("fusée", e.rocket),
                    ("dégonflé", e.deflated),
                    ("chantilly", e.creamed),
                ] {
                    if left > 0.0 {
                        if !text.is_empty() {
                            text.push_str(", ");
                        }
                        text.push_str(&format!("{name} {left:.1} s"));
                    }
                }
                row("Effets d'objets", text);
            }
            if let Some(counts) = item_stats {
                row(
                    "Boîtes, objets, touché",
                    format!(
                        "{} boîtes, {} utilisés, touché {} fois, {} coups réussis",
                        counts.boxes, counts.used, counts.hits_taken, counts.hits_landed
                    ),
                );
            }
        });
}

/// Issue #155: the « Ressenti » knobs, one per feeling. Each one drives several values (listed in
/// its tooltip, with what it gives them at 0, in the middle and at 100); the middle is the
/// game's defaults. « Copier mes réglages » gives the knobs off the middle and the values
/// modified by hand.
fn feel_section(
    ui: &mut egui::Ui,
    world: &mut World,
    state: &mut PanelState,
    tunings: &mut Tunings,
    changed: &mut bool,
) {
    let hand = tunings.changes(world);
    let moved = tunings.feel.moved().count();
    ui.horizontal(|ui| {
        let now = ui.input(|i| i.time);
        if ui.button("Copier mes réglages").clicked() {
            ui.ctx().copy_text(state.export.clone());
            state.copied_at = Some(now);
        }
        if state.copied_at.is_some_and(|at| now - at < 4.0) {
            ui.label(RichText::new("copié !").color(MODIFIED));
        }
        if ui
            .add_enabled(
                moved > 0 || !hand.is_empty(),
                egui::Button::new("Tout par défaut"),
            )
            .clicked()
        {
            tunings.reset_all(world);
            *changed = true;
        }
    });
    let status = match (moved, hand.len()) {
        (0, 0) => "Le milieu, c'est le jeu tel quel.".to_string(),
        (m, h) => format!(
            "{m} curseur{} bougé{}, {h} réglage{} à la main",
            plural(m),
            plural(m),
            plural(h)
        ),
    };
    ui.small(
        RichText::new(status).color(if moved > 0 || !hand.is_empty() {
            MODIFIED
        } else {
            ui.visuals().weak_text_color()
        }),
    );

    let mut moves: Vec<(Knob, u8)> = Vec::new();
    egui::Grid::new("admin-feel")
        .num_columns(3)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            ui.spacing_mut().slider_width = 180.0;
            for knob in Knob::ALL {
                let position = tunings.feel.position(knob);
                let by_hand = hand
                    .iter()
                    .filter(|c| {
                        tunings
                            .get(c.tuning)
                            .and_then(|t| t.knob_of(&c.path))
                            .is_some_and(|k| k == knob)
                    })
                    .count();
                let name = if position != MIDDLE {
                    RichText::new(knob.label()).color(MODIFIED).strong()
                } else {
                    RichText::new(knob.label()).strong()
                };
                ui.label(name)
                    .on_hover_ui(|ui| knob_tooltip(ui, world, tunings, knob));
                let mut value = position;
                if ui
                    .add(egui::Slider::new(&mut value, 0..=KNOB_MAX).integer())
                    .changed()
                {
                    moves.push((knob, value));
                }
                if ui
                    .add_enabled(position != MIDDLE, egui::Button::new("milieu").small())
                    .clicked()
                {
                    moves.push((knob, MIDDLE));
                }
                ui.end_row();

                ui.label("");
                ui.horizontal(|ui| {
                    // Under each end of the slider. egui's font has no arrows: the side says it.
                    let width = ui.spacing().slider_width;
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_width(width);
                            ui.small(RichText::new(end_text(knob.low())).weak());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.small(RichText::new(end_text(knob.high())).weak());
                                },
                            );
                        },
                    );
                    if by_hand > 0 {
                        ui.small(RichText::new(format!("{by_hand} à la main")).color(MODIFIED))
                            .on_hover_text(
                                "Modifiés à la main dans « Avancé » : ce curseur ne les change \
                                 plus (« défaut » les lui rend).",
                            );
                    }
                });
                ui.end_row();
            }
        });
    for (knob, position) in moves {
        tunings.set_knob(world, knob, position);
        *changed = true;
    }
}

/// A knob's end without its arrow (egui's font has none).
fn end_text(text: &str) -> &str {
    text.trim_matches(|c: char| c == '←' || c == '→' || c.is_whitespace())
}

fn plural(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}

/// What a knob does, and every value it drives with what it gives it at 0, in the middle and at
/// 100 (the values of the game's tunings only: those that exist here).
fn knob_tooltip(ui: &mut egui::Ui, world: &World, tunings: &Tunings, knob: Knob) {
    ui.label(knob.summary());
    ui.add_space(4.0);
    egui::Grid::new(("admin-feel-tip", knob.key()))
        .num_columns(2)
        .spacing([10.0, 0.0])
        .show(ui, |ui| {
            ui.small(RichText::new("réglage").weak());
            ui.small(RichText::new("à 0 · milieu · à 100").weak());
            ui.end_row();
            for tuning in &tunings.list {
                if tuning.read(world).is_none() {
                    continue;
                }
                for field in &tuning.defaults {
                    let full = format!("{}.{}", tuning.name, field.path);
                    let Some(link) = LINKS.iter().find(|l| l.knob == knob && l.matches(&full))
                    else {
                        continue;
                    };
                    ui.small(full);
                    ui.small(format!(
                        "{} · {} · {}",
                        short(link.value(field.value, 0)),
                        short(field.value),
                        short(link.value(field.value, KNOB_MAX))
                    ));
                    ui.end_row();
                }
            }
        });
}

/// A value in a few characters (3 decimals at most).
fn short(value: Value) -> String {
    match value {
        Value::F32(_) | Value::F64(_) => {
            let x = value.as_f64().unwrap_or_default();
            let text = format!("{x:.3}");
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        other => other.to_string(),
    }
}

/// Issue #155: every detailed value, as before #155 (folded by default). A value modified by hand
/// (off its baseline) stands out, « à la main », and its knob no longer changes it until
/// « défaut » gives it back; a value its knob has moved shows in blue.
fn tunings_section(
    ui: &mut egui::Ui,
    world: &mut World,
    state: &mut PanelState,
    tunings: &Tunings,
    changed: &mut bool,
) {
    let modified = tunings.changes(world).len();
    ui.horizontal(|ui| {
        let text = match modified {
            0 => "aucune valeur à la main".to_string(),
            1 => "1 valeur à la main".to_string(),
            n => format!("{n} valeurs à la main"),
        };
        ui.label(RichText::new(text).color(if modified > 0 {
            MODIFIED
        } else {
            ui.visuals().weak_text_color()
        }));
        ui.label("Filtrer");
        ui.add(egui::TextEdit::singleline(&mut state.filter).desired_width(120.0));
        if !state.filter.is_empty() && ui.small_button("effacer").clicked() {
            state.filter.clear();
        }
    });
    ui.small(RichText::new("En orange : à la main. En bleu : réglé par un curseur.").weak())
        .on_hover_text(
            "Une valeur à la main ne suit plus son curseur de « Ressenti » ; « défaut » la lui \
             rend.\n* : s'applique au prochain « Relancer ».\nSurvole un nom pour sa valeur par \
             défaut.",
        );

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
            .filter(|f| tuning.baseline(&tunings.feel, &f.path) != Some(f.value))
            .count();
        let title = if modified_here > 0 {
            RichText::new(format!("{} ({modified_here} à la main)", tuning.name)).color(MODIFIED)
        } else {
            RichText::new(tuning.name)
        };
        // A filter shows every tuning with a match.
        let open = !filter.is_empty() || state.open.contains(tuning.name);
        let mut edits: Vec<(String, Value)> = Vec::new();
        let mut resets: Vec<String> = Vec::new();
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
                            let baseline = tuning.baseline(&tunings.feel, &field.path);
                            let by_hand = baseline != Some(field.value);
                            let knob = tuning.knob_of(&field.path);
                            let knob_moved =
                                knob.is_some_and(|k| tunings.feel.position(k) != MIDDLE);
                            let full = format!("{}.{}", tuning.name, field.path);
                            let on_restart = ON_RESTART.iter().any(|p| full.starts_with(p));
                            let mut name = field.path.clone();
                            if on_restart {
                                name.push_str(" *");
                            }
                            let label = if by_hand {
                                name.push_str(" · à la main");
                                RichText::new(name).color(MODIFIED).strong()
                            } else if knob_moved {
                                RichText::new(name).color(DRIVEN)
                            } else {
                                RichText::new(name)
                            };
                            let mut tip = match default {
                                Some(d) => format!("défaut : {}", d.value),
                                None => "pas de valeur par défaut".to_string(),
                            };
                            if let Some(knob) = knob {
                                tip.push_str(&format!("\npiloté par « {} »", knob.label()));
                                if let (true, Some(b)) = (knob_moved, baseline) {
                                    tip.push_str(&format!(" : {b} maintenant"));
                                }
                                if by_hand {
                                    tip.push_str(
                                        "\nmodifié à la main : le curseur ne le change plus",
                                    );
                                }
                            } else if by_hand {
                                tip.push_str("\nmodifié à la main");
                            }
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
                                .add_enabled(by_hand, egui::Button::new("défaut").small())
                                .clicked()
                            {
                                resets.push(field.path.clone());
                            }
                            ui.end_row();
                        }
                    });
            });
        if response.header_response.clicked() && filter.is_empty() {
            toggle(&mut state.open, tuning.name);
        }
        if reset {
            tunings.reset_tuning(world, tuning.name);
            *changed = true;
        }
        for (path, value) in edits {
            if tuning.set(world, &path, value).is_ok() {
                *changed = true;
            }
        }
        for path in resets {
            if tunings.reset_field(world, tuning.name, &path).is_ok() {
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
    tunings: &mut Tunings,
    changed: &mut bool,
) {
    ui.label("Mes réglages, à coller dans une issue (curseurs bougés, valeurs à la main) :");
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
                for converted in report.converted {
                    message.push_str("\nConverti : ");
                    message.push_str(&converted);
                }
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
