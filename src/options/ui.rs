//! The options menu's egui window and the options kept between sessions (issue #17, see
//! `super`). Only with a renderer: the headless tests drive the menu with its keys and the
//! [`OptionsAction`]s its buttons write too, and never touch the storage.
//!
//! Like the admin panel and the selection screen, it costs nothing once closed: the egui context
//! only exists while one of them is open (`admin::ui::attach_egui_context`).

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};

use super::{
    OPTIONS_STORAGE_KEY, OptionRow, OptionsAction, OptionsMenu, OptionsSystems, PlayerOptions,
    controls,
};
use crate::admin::storage;
use crate::kart::KeyboardLayout;

/// The line being changed.
const HIGHLIGHT: Color32 = Color32::from_rgb(255, 190, 70);

pub struct OptionsUiPlugin;

impl Plugin for OptionsUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.insert_resource(EguiGlobalSettings {
            // The context goes on the camera only while a window needs it.
            auto_create_primary_context: false,
            ..default()
        })
        .add_systems(PreStartup, load_stored_options)
        .add_systems(Update, store_options.after(OptionsSystems))
        .add_systems(
            EguiPrimaryContextPass,
            options_window.run_if(|menu: Res<OptionsMenu>| menu.open),
        );
    }
}

/// The options kept from the previous sessions, before anything is built. An unreadable text
/// is ignored (the defaults stay), with a warning.
fn load_stored_options(mut options: ResMut<PlayerOptions>) {
    let Some(text) = storage::load_key(OPTIONS_STORAGE_KEY) else {
        return;
    };
    match PlayerOptions::from_ron(&text) {
        Ok(stored) => *options = stored,
        Err(error) => warn!("options: stored options ignored: {error}"),
    }
}

/// Every change is kept at once (not the options loaded at startup: nothing changed).
fn store_options(options: Res<PlayerOptions>) {
    if options.is_changed() && !options.is_added() {
        storage::save_key(OPTIONS_STORAGE_KEY, &options.to_ron());
    }
}

fn options_window(
    mut contexts: EguiContexts,
    menu: Res<OptionsMenu>,
    options: Res<PlayerOptions>,
    mut actions: MessageWriter<OptionsAction>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // The arrows, Entrée and Espace are the menu's keys (`super::options_keys`): no egui widget
    // keeps the keyboard focus, or egui would move it or click it with them too.
    if let Some(focused) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
    let layout = options.keyboard;
    egui::Window::new("Options")
        .id(egui::Id::new("fluffy-options"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            for (index, row) in OptionRow::ALL.into_iter().enumerate() {
                let current = menu.row == index;
                ui.horizontal(|ui| {
                    let mut label = RichText::new(row.label());
                    if current {
                        label = label.strong().color(HIGHLIGHT);
                    }
                    ui.label(label);
                    match row {
                        OptionRow::Keyboard => {
                            for choice in KeyboardLayout::ALL {
                                let text = RichText::new(choice.label()).strong();
                                if ui.selectable_label(layout == choice, text).clicked() {
                                    actions.write(OptionsAction::Keyboard(choice));
                                }
                            }
                        }
                    }
                });
                ui.small(row.help());
                ui.add_space(4.0);
            }
            ui.separator();
            ui.label(RichText::new("Commandes").strong());
            egui::Grid::new("options-controls")
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for (action, keys) in controls(layout) {
                        ui.label(action);
                        ui.label(keys);
                        ui.end_row();
                    }
                });
            ui.separator();
            if ui.button("Fermer").clicked() {
                actions.write(OptionsAction::Close);
            }
            let [up, left, down, right] = layout.letters();
            ui.small(format!(
                "Haut / Bas ({up} / {down}) : la ligne · Gauche / Droite ({left} / {right}), \
                 Entrée : changer · Échap ou O : fermer"
            ));
        });
}
