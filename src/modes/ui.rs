//! The race menu's egui window (issue #19, see `super`). Only with a renderer: the headless tests
//! drive the menu with its keys and the [`RaceMenuAction`]s its buttons write too.
//!
//! Like the other menus, it costs nothing once closed: the egui context only exists while one of
//! them is open (`admin::ui::attach_egui_context`).

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};

use super::{GameMode, MenuRow, RaceMenu, RaceMenuAction};
use crate::admin::{keep_focus_for_text_only, tracks};
use crate::kart::KeyboardLayout;
use crate::race::GrandPrixTuning;

/// The line being changed.
const HIGHLIGHT: Color32 = Color32::from_rgb(255, 190, 70);

pub struct ModesUiPlugin;

impl Plugin for ModesUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.insert_resource(EguiGlobalSettings {
            // The context goes on the camera only while a window needs it.
            auto_create_primary_context: false,
            ..default()
        })
        .add_systems(
            EguiPrimaryContextPass,
            race_menu_window.run_if(|menu: Res<RaceMenu>| menu.open),
        );
    }
}

/// What a line is for, under it.
fn help(row: MenuRow) -> &'static str {
    match row {
        MenuRow::Mode => {
            "Course libre : une course sur le circuit choisi. Grand Prix : quatre courses, \
             des points à chaque course, puis le podium. Contre-la-montre : seul contre le \
             chrono, trois piments en main, et le fantôme de ton record."
        }
        MenuRow::Circuit => "Le Grand Prix court les quatre circuits, dans cet ordre.",
        MenuRow::Mirror => "Le circuit inversé gauche-droite.",
    }
}

fn race_menu_window(
    mut contexts: EguiContexts,
    menu: Res<RaceMenu>,
    gp_tuning: Res<GrandPrixTuning>,
    layout: Option<Res<KeyboardLayout>>,
    mut actions: MessageWriter<RaceMenuAction>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // The arrows and Entrée are the menu's keys (`super::menu_keys`): no egui widget keeps the
    // keyboard focus, or egui would move it or click it with them too; only a text field of
    // another window (issue #80: the lobby's pseudo), which takes the keys from the menu.
    keep_focus_for_text_only(ctx);
    let circuits = tracks::circuits();
    let setup = menu.setup;
    egui::Window::new("Nouvelle course")
        .id(egui::Id::new("fluffy-race-menu"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            for (index, row) in MenuRow::ALL.into_iter().enumerate() {
                let current = menu.row == index;
                ui.horizontal(|ui| {
                    let mut label = RichText::new(row.label());
                    if current {
                        label = label.strong().color(HIGHLIGHT);
                    }
                    ui.add_sized([70.0, 18.0], egui::Label::new(label));
                    match row {
                        MenuRow::Mode => {
                            for mode in GameMode::ALL {
                                let text = RichText::new(mode.label()).strong();
                                if ui.selectable_label(setup.mode == mode, text).clicked() {
                                    actions.write(RaceMenuAction::Set(super::RaceSetup {
                                        mode,
                                        ..setup
                                    }));
                                }
                            }
                        }
                        MenuRow::Circuit if setup.mode.picks_a_circuit() => {
                            for (i, entry) in circuits.iter().enumerate() {
                                let text = RichText::new(&entry.name).strong();
                                if ui.selectable_label(setup.circuit == i, text).clicked() {
                                    actions.write(RaceMenuAction::Set(super::RaceSetup {
                                        circuit: i,
                                        ..setup
                                    }));
                                }
                            }
                        }
                        MenuRow::Circuit => {
                            ui.label(setup.value(row, &circuits, gp_tuning.races as usize));
                        }
                        MenuRow::Mirror => {
                            for (mirror, name) in [(false, "Non"), (true, "Oui")] {
                                let text = RichText::new(name).strong();
                                if ui.selectable_label(setup.mirror == mirror, text).clicked() {
                                    actions.write(RaceMenuAction::Set(super::RaceSetup {
                                        mirror,
                                        ..setup
                                    }));
                                }
                            }
                        }
                    }
                });
                ui.small(help(row));
                ui.add_space(4.0);
            }
            ui.separator();
            ui.horizontal(|ui| {
                let start = match setup.mode {
                    GameMode::FreeRace => "Course !",
                    GameMode::GrandPrix => "Grand Prix !",
                    GameMode::TimeTrial => "Chrono !",
                };
                if ui
                    .button(RichText::new(start).strong().size(18.0))
                    .clicked()
                {
                    actions.write(RaceMenuAction::Confirm);
                }
                if ui.button("Annuler").clicked() {
                    actions.write(RaceMenuAction::Close);
                }
            });
            let [up, left, down, right] = layout
                .map_or_else(KeyboardLayout::default, |l| *l)
                .letters();
            ui.small(format!(
                "Haut / Bas ({up} / {down}) : la ligne · Gauche / Droite ({left} / {right}) : le \
                 choix · Entrée : c'est parti · Échap ou N : fermer · G : pilote et kart"
            ));
        });
}
