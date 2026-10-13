//! The selection screen's egui interface (issue #15, see `super`). Only with a renderer: the
//! headless tests go through the keys and the [`SelectionAction`]s, which its buttons write too.
//!
//! Like the admin panel (#36), it costs nothing once closed: the egui context only exists while
//! one of them is open (`admin::ui::attach_egui_context` puts it on the camera and removes it).

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};

use super::{PartKind, Roster, SelectionAction, SelectionScreen};
use crate::kart::{MAX_LEVEL, Stat, display_level};

/// The line being changed.
const HIGHLIGHT: Color32 = Color32::from_rgb(255, 190, 70);

pub struct CharacterUiPlugin;

impl Plugin for CharacterUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.insert_resource(EguiGlobalSettings {
            // The context goes on the camera only while a screen needs it.
            auto_create_primary_context: false,
            ..default()
        })
        .add_systems(Startup, open_at_launch)
        .add_systems(
            EguiPrimaryContextPass,
            selection_window.run_if(|screen: Res<SelectionScreen>| screen.open),
        );
    }
}

/// The game starts on the selection screen: Entrée races with the default loadout.
fn open_at_launch(mut actions: MessageWriter<SelectionAction>) {
    actions.write(SelectionAction::Open);
}

/// `3.25` written the French way: `3,25`.
fn french(value: f32) -> String {
    format!("{value:.2}").replace('.', ",")
}

fn selection_window(
    mut contexts: EguiContexts,
    screen: Res<SelectionScreen>,
    roster: Res<Roster>,
    mut actions: MessageWriter<SelectionAction>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // The arrows and Entrée are the screen's keys (`super::selection_keys`): no egui widget keeps
    // the keyboard focus, or egui would move it or click it with them too.
    if let Some(focused) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
    let stats = roster.stats(&screen.choice);
    egui::Window::new("Pilote et kart")
        .id(egui::Id::new("fluffy-selection"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("selection-parts")
                .num_columns(4)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    for kind in PartKind::ALL {
                        let current = screen.kind() == kind;
                        let mut label = RichText::new(kind.label());
                        if current {
                            label = label.strong().color(HIGHLIGHT);
                        }
                        if ui.selectable_label(current, label).clicked() {
                            let index = screen.choice.get(kind);
                            actions.write(SelectionAction::Pick(kind, index));
                        }
                        if ui.button("<").clicked() {
                            actions.write(SelectionAction::Change(kind, -1));
                        }
                        let index = screen.choice.get(kind);
                        let mut name = roster.name(kind, index).to_string();
                        if kind == PartKind::Character {
                            let class = roster.character(&screen.choice).class;
                            name = format!("{name} ({})", class.label());
                        }
                        ui.add_sized(
                            [200.0, 20.0],
                            egui::Label::new(RichText::new(name).strong().size(16.0)),
                        );
                        if ui.button(">").clicked() {
                            actions.write(SelectionAction::Change(kind, 1));
                        }
                        ui.end_row();
                    }
                });
            ui.separator();
            egui::Grid::new("selection-stats")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    let top = display_level(MAX_LEVEL);
                    for stat in Stat::ALL {
                        ui.label(stat.label());
                        let value = stats.display(stat);
                        ui.add(
                            egui::ProgressBar::new(value / top)
                                .desired_width(240.0)
                                .text(french(value)),
                        );
                        ui.end_row();
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Course !").strong().size(18.0))
                    .clicked()
                {
                    actions.write(SelectionAction::Confirm);
                }
                if ui.button("Annuler").clicked() {
                    actions.write(SelectionAction::Cancel);
                }
            });
            ui.small(
                "Flèches haut / bas : la ligne · gauche / droite : le choix · \
                 Entrée : course · Échap : annuler · G : revenir ici en jeu",
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_written_the_french_way() {
        assert_eq!(french(3.25), "3,25");
        assert_eq!(french(0.75), "0,75");
        assert_eq!(french(5.0), "5,00");
    }
}
