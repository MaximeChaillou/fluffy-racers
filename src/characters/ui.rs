//! The selection screen's egui interface (issue #15, see `super`). Only with a renderer: the
//! headless tests go through the keys and the [`SelectionAction`]s, which its buttons write too.
//!
//! Like the admin panel (#36), it costs nothing once closed: the egui context only exists while
//! one of them is open (`admin::ui::attach_egui_context` puts it on the camera and removes it).
//!
//! The player's color (issue #55) is kept here between sessions, like the options (#17): read
//! before anything is built, written when it changes. The headless tests never touch the storage.

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};

use super::{COLOR_STORAGE_KEY, PartKind, PlayerColor, Roster, SelectionAction, SelectionScreen};
use crate::admin::{keep_focus_for_text_only, storage};
use crate::kart::{KartColor, KeyboardLayout, MAX_LEVEL, Stat, display_level};

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
        .add_systems(PreStartup, load_stored_color)
        .add_systems(Update, store_color)
        .add_systems(
            EguiPrimaryContextPass,
            selection_window.run_if(|screen: Res<SelectionScreen>| screen.open),
        );
    }
}

/// The color kept from the previous sessions, before anything is built (the karts, the `Hello`
/// online). An unreadable text is ignored (pink stays), with a warning.
fn load_stored_color(mut color: ResMut<PlayerColor>) {
    let Some(text) = storage::load_key(COLOR_STORAGE_KEY) else {
        return;
    };
    match KartColor::from_stored(&text) {
        Ok(stored) => color.0 = stored,
        Err(error) => warn!("selection: stored color ignored: {error}"),
    }
}

/// A new color is kept at once (not the one loaded at startup: nothing changed).
fn store_color(color: Res<PlayerColor>) {
    if color.is_changed() && !color.is_added() {
        storage::save_key(COLOR_STORAGE_KEY, &color.0.to_stored());
    }
}

/// The palette's color for egui.
fn egui_color(color: KartColor) -> Color32 {
    let [r, g, b] = color.rgb();
    Color32::from_rgb(r, g, b)
}

/// A swatch of `color`, the size of a line.
fn swatch(ui: &mut egui::Ui, color: KartColor) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    ui.painter().rect_filled(rect, 4.0, egui_color(color));
    response
}

/// `3.25` written the French way: `3,25`.
fn french(value: f32) -> String {
    format!("{value:.2}").replace('.', ",")
}

fn selection_window(
    mut contexts: EguiContexts,
    screen: Res<SelectionScreen>,
    roster: Res<Roster>,
    layout: Option<Res<KeyboardLayout>>,
    mut actions: MessageWriter<SelectionAction>,
) {
    let [up, left, down, right] = layout
        .map_or_else(KeyboardLayout::default, |l| *l)
        .letters();
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // The arrows and Entrée are the screen's keys (`super::selection_keys`): no egui widget keeps
    // the keyboard focus, or egui would move it or click it with them too; only a text field of
    // another window (issue #80: the lobby's pseudo), which takes the keys from the screen.
    keep_focus_for_text_only(ctx);
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
                        let current = !screen.on_color_row() && screen.kind() == kind;
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
                    // Issue #55: the kart's color, with a swatch.
                    let current = screen.on_color_row();
                    let mut label = RichText::new("Couleur");
                    if current {
                        label = label.strong().color(HIGHLIGHT);
                    }
                    if ui.selectable_label(current, label).clicked() {
                        actions.write(SelectionAction::PickColor(screen.color));
                    }
                    if ui.button("<").clicked() {
                        actions.write(SelectionAction::Color(-1));
                    }
                    ui.horizontal(|ui| {
                        ui.set_min_width(200.0);
                        swatch(ui, screen.color);
                        ui.label(RichText::new(screen.color.title()).strong().size(16.0));
                    });
                    if ui.button(">").clicked() {
                        actions.write(SelectionAction::Color(1));
                    }
                    ui.end_row();
                });
            // Every color of the palette, to click on.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for color in KartColor::ALL {
                    let response = swatch(ui, color).on_hover_text(color.title());
                    if color == screen.color {
                        ui.painter().rect_stroke(
                            response.rect.expand(2.0),
                            4.0,
                            egui::Stroke::new(2.0, HIGHLIGHT),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.clicked() {
                        actions.write(SelectionAction::PickColor(color));
                    }
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
            ui.small(format!(
                "Haut / Bas ({up} / {down}) : la ligne · Gauche / Droite ({left} / {right}) : \
                 le choix · Entrée : course · Échap : annuler · G : revenir ici en jeu · \
                 N : circuit, Grand Prix, classe, miroir · O : options (clavier)"
            ));
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
