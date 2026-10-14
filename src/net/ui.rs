//! The lobby window and the room asked for at startup (issue #7, see `super`). Only with a
//! renderer: the headless tests drive the network with [`NetAction`]s, which the window's
//! buttons write too.
//!
//! Like the other windows, it costs nothing once closed: the egui context only exists while one
//! of them is open (`admin::ui::attach_egui_context`). It does not pause the game: the race goes
//! on (solo, until issue #9) while the players gather.

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};

#[cfg(target_arch = "wasm32")]
use super::NetSystems;
use super::ice::{is_loopback_host, signal_host};
use super::{
    Diagnosis, LobbyWindow, MAX_PLAYERS, NetAction, NetSession, NetSettings, NetStatus, NetTuning,
    OnlineRace, Stuck, default_signal, invite_link, is_local_signal, native_command, now_ms,
};
use crate::admin::tracks::{TrackEntry, circuits};
use crate::characters::{PartKind, Roster};

const GOOD: Color32 = Color32::from_rgb(110, 210, 120);
const BAD: Color32 = Color32::from_rgb(240, 110, 90);
const HOST: Color32 = Color32::from_rgb(255, 190, 70);

/// « Copié ! » stays this long (s).
const COPIED_SECONDS: f64 = 2.0;

pub struct NetUiPlugin;

impl Plugin for NetUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        app.insert_resource(EguiGlobalSettings {
            // The context goes on the camera only while a window needs it.
            auto_create_primary_context: false,
            ..default()
        })
        .insert_resource(LobbyUiState {
            copied_at: None,
            circuits: circuits(),
        })
        .add_systems(Startup, join_launch_room)
        .add_systems(
            EguiPrimaryContextPass,
            lobby_window.run_if(|window: Res<LobbyWindow>| window.open),
        );
        #[cfg(target_arch = "wasm32")]
        app.add_systems(Update, page_address.after(NetSystems));
    }
}

#[derive(Resource, Default)]
struct LobbyUiState {
    /// When « Copier » was last clicked (real time, s).
    copied_at: Option<f64>,
    /// The circuits the host can choose (issue #9): `assets/tracks/index.ron`.
    circuits: Vec<TrackEntry>,
}

/// `?room=CODE&signal=…&pseudo=…&stun=…&turn=…&netsim=…` in the page address, or `--room CODE
/// --signal …` natively: the settings, and the room to join. Without `signal`, a page served on this machine
/// or the local network meets the others through `matchbox_server` on that machine
/// ([`default_signal`], issue #49).
fn join_launch_room(
    mut settings: ResMut<NetSettings>,
    tuning: Option<ResMut<NetTuning>>,
    mut actions: MessageWriter<NetAction>,
) {
    let options = platform::launch_options();
    // `?netsim=120,20,5` (issue #8): after the stored tunings of the admin panel (PreStartup).
    if let (Some(netsim), Some(mut tuning)) = (options.netsim, tuning) {
        info!("network: simulated network on the kart states received: {netsim:?}");
        tuning.simulator = netsim;
    }
    for unusable in options.unusable() {
        warn!("network: ignored, cannot be used: {unusable}");
    }
    if let Some(signal) = options.signal.clone() {
        settings.signal = signal;
    } else if let Some(host) = platform::page_host() {
        settings.signal = default_signal(Some(&host));
    }
    if let Some(stun) = options.stun.clone() {
        settings.stun = stun;
    }
    if let Some(turn) = options.turn_server() {
        settings.turn = Some(turn);
    }
    if options.pseudo.is_some() {
        settings.pseudo = options.pseudo;
    }
    if let Some(room) = options.room {
        actions.write(NetAction::Join(room));
    }
}

/// The room's code in the address bar (`?room=CODE`), so that a reload stays in it and the
/// address is the invitation; removed when the player leaves.
#[cfg(target_arch = "wasm32")]
fn page_address(session: Option<Res<NetSession>>, mut shown: Local<Option<super::RoomCode>>) {
    let room = session.map(|session| session.room.clone());
    if *shown != room {
        platform::show_room(room.as_ref());
        *shown = room;
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::super::{LaunchOptions, RoomCode, query_with_room};

    pub fn launch_options() -> LaunchOptions {
        web_sys::window()
            .and_then(|window| window.location().search().ok())
            .map(|query| LaunchOptions::from_query(&query))
            .unwrap_or_default()
    }

    /// The machine the page comes from (`192.168.1.20`, `localhost`...).
    pub fn page_host() -> Option<String> {
        web_sys::window()?.location().hostname().ok()
    }

    /// The game's page, without its query: the base of the invitation link.
    pub fn page() -> Option<String> {
        let location = web_sys::window()?.location();
        Some(format!(
            "{}{}",
            location.origin().ok()?,
            location.pathname().ok()?
        ))
    }

    pub fn show_room(room: Option<&RoomCode>) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let location = window.location();
        let (Ok(path), Ok(query), Ok(hash)) =
            (location.pathname(), location.search(), location.hash())
        else {
            return;
        };
        let address = format!("{path}{}{hash}", query_with_room(&query, room));
        if let Ok(history) = window.history() {
            let null = web_sys::wasm_bindgen::JsValue::NULL;
            let _ = history.replace_state_with_url(&null, "", Some(&address));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::super::LaunchOptions;

    pub fn launch_options() -> LaunchOptions {
        LaunchOptions::from_args(std::env::args().skip(1))
    }

    pub fn page_host() -> Option<String> {
        None
    }

    /// Natively, no page: the invitation is a command line.
    pub fn page() -> Option<String> {
        None
    }
}

fn lobby_window(
    mut contexts: EguiContexts,
    session: Option<Res<NetSession>>,
    settings: Res<NetSettings>,
    roster: Option<Res<Roster>>,
    race: Option<Res<OnlineRace>>,
    time: Res<Time<Real>>,
    mut state: ResMut<LobbyUiState>,
    mut actions: MessageWriter<NetAction>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // The game's keys (Espace, the arrows) must not click or move through the window's buttons:
    // no widget keeps the keyboard focus.
    if let Some(focused) = ctx.memory(|m| m.focused()) {
        ctx.memory_mut(|m| m.surrender_focus(focused));
    }
    let now = time.elapsed_secs_f64();
    egui::Window::new("En ligne")
        .id(egui::Id::new("fluffy-lobby"))
        .anchor(egui::Align2::LEFT_TOP, [10.0, 110.0])
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            match session.as_deref() {
                None => offline(ui, &settings, &mut actions),
                Some(session) => in_room(
                    ui,
                    session,
                    roster.as_deref(),
                    race.as_deref(),
                    now_ms(&time),
                    now,
                    &mut state,
                    &mut actions,
                ),
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Fermer").clicked() {
                    actions.write(NetAction::Close);
                }
                ui.small("L : ouvrir / fermer");
            });
        });
}

/// No room yet.
fn offline(ui: &mut egui::Ui, settings: &NetSettings, actions: &mut MessageWriter<NetAction>) {
    ui.label("Joue avec tes amis : crée une partie, puis envoie-leur le lien.");
    if ui
        .button(RichText::new("Créer une partie").strong().size(16.0))
        .clicked()
    {
        actions.write(NetAction::CreateRoom);
    }
    ui.small("Pour rejoindre une partie : ouvre le lien d'invitation (?room=CODE).");
    signal_hint(ui, &settings.signal);
}

fn signal_hint(ui: &mut egui::Ui, signal: &str) {
    ui.small(format!("Mise en relation : {signal}"));
    if let Some(host) = signal_host(signal).filter(|_| is_local_signal(signal)) {
        let machine = if is_loopback_host(host) {
            "ton ordinateur".to_string()
        } else {
            host.to_string()
        };
        ui.small(format!(
            "matchbox_server doit tourner sur {machine} (voir le README)."
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn in_room(
    ui: &mut egui::Ui,
    session: &NetSession,
    roster: Option<&Roster>,
    race: Option<&OnlineRace>,
    now_ms: u64,
    now: f64,
    state: &mut LobbyUiState,
    actions: &mut MessageWriter<NetAction>,
) {
    let room = &session.room;
    ui.label(RichText::new(format!("Partie {room}")).strong().size(18.0));
    let link = platform::page()
        .map(|page| invite_link(&page, room, &session.signal))
        .unwrap_or_else(|| native_command(room, &session.signal));
    ui.label(RichText::new(&link).monospace().small());
    ui.horizontal(|ui| {
        if ui.button("Copier le lien").clicked() {
            ui.ctx().copy_text(link.clone());
            state.copied_at = Some(now);
        }
        if state.copied_at.is_some_and(|at| now - at < COPIED_SECONDS) {
            ui.label(RichText::new("Copié !").color(GOOD));
        }
    });
    if platform::page_host().is_some_and(|host| is_loopback_host(&host)) {
        // The browser does not know the machine's address on the network (it hides it).
        ui.small(
            "Pour inviter quelqu'un sur ton réseau local, ouvre le jeu avec l'adresse IP de ton \
             ordinateur (http://192.168…:8080, voir le README).",
        );
    }
    ui.separator();
    match &session.status {
        NetStatus::Connecting => {
            ui.label("Connexion au serveur de mise en relation…");
            signal_hint(ui, &session.signal);
        }
        NetStatus::Failed(reason) => {
            ui.label(
                RichText::new(format!(
                    "Impossible de joindre le serveur de mise en relation {}.",
                    session.signal
                ))
                .color(BAD),
            );
            if is_local_signal(&session.signal) {
                ui.label("Lance matchbox_server (voir le README), puis réessaie.");
            }
            ui.small(reason);
            if ui.button("Réessayer").clicked() {
                actions.write(NetAction::Join(room.clone()));
            }
        }
        NetStatus::Full => {
            ui.label(
                RichText::new(format!(
                    "La partie {room} est pleine ({MAX_PLAYERS} joueurs)."
                ))
                .color(BAD),
            );
            if ui.button("Créer une autre partie").clicked() {
                actions.write(NetAction::CreateRoom);
            }
        }
        NetStatus::InRoom => {
            if let Some(lobby) = &session.lobby {
                lobby_list(ui, session, lobby, roster, now_ms, actions);
                room_controls(ui, lobby, race, &state.circuits, actions);
            }
        }
    }
    ui.separator();
    if ui.button("Quitter la partie").clicked() {
        actions.write(NetAction::Leave);
    }
}

fn lobby_list(
    ui: &mut egui::Ui,
    session: &NetSession,
    lobby: &super::Lobby,
    roster: Option<&Roster>,
    now_ms: u64,
    actions: &mut MessageWriter<NetAction>,
) {
    if !lobby.seated() {
        ui.label("En attente d'une place dans la partie…");
    } else if lobby.is_host() {
        ui.label("Tu es l'hôte de la partie.");
    }
    let rows = session.players(now_ms);
    let seated = rows.iter().filter(|row| row.seated).count();
    ui.small(format!("{seated} joueur(s) sur {MAX_PLAYERS}"));
    egui::Grid::new("lobby-players")
        .num_columns(4)
        .spacing([10.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Joueur").strong());
            ui.label(RichText::new("Animal").strong());
            ui.label(RichText::new("Ping").strong());
            ui.label(RichText::new("Prêt").strong());
            ui.end_row();
            for row in &rows {
                let mut name = if row.name.is_empty() {
                    "…".to_string()
                } else {
                    row.name.clone()
                };
                if row.me {
                    name.push_str(" (toi)");
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).strong());
                    ui.small(format!("#{}", row.key.tag()));
                    if row.host {
                        ui.label(RichText::new("hôte").color(HOST));
                    }
                });
                if let Some(version) = row.other_version {
                    ui.label(RichText::new(format!("autre version ({version})")).color(BAD));
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                    continue;
                }
                let animal = roster
                    .map(|r| r.name(PartKind::Character, usize::from(row.loadout[0])))
                    .unwrap_or("");
                ui.label(if row.seated || row.me {
                    animal
                } else {
                    "arrive…"
                });
                let ping = match (row.me, row.silent, row.ping_ms) {
                    (true, _, _) => RichText::new("—"),
                    (_, true, _) => RichText::new("muet").color(BAD),
                    (_, _, Some(ms)) => RichText::new(format!("{ms:.0} ms")),
                    (_, _, None) => RichText::new("…"),
                };
                ui.label(ping);
                ui.label(if row.ready {
                    RichText::new("prêt").color(GOOD)
                } else {
                    RichText::new("—")
                });
                ui.end_row();
            }
        });
    if rows.iter().any(|row| row.other_version.is_some()) {
        ui.small("Un joueur a une autre version du jeu : rechargez tous la page.");
    }
    connections(ui, session, now_ms);
    ui.add_space(4.0);
    let ready = lobby.profile().ready;
    let label = if ready { "Pas prêt" } else { "Prêt !" };
    if ui
        .add_enabled(
            lobby.seated(),
            egui::Button::new(RichText::new(label).strong().size(16.0)),
        )
        .clicked()
    {
        actions.write(NetAction::Ready(!ready));
    }
    if lobby.all_ready() {
        ui.label(RichText::new("Tout le monde est prêt !").color(GOOD));
    }
}

/// The room's circuit and race (issue #9): the host chooses the circuit and starts the race once
/// everyone is ready; the others see what is on.
fn room_controls(
    ui: &mut egui::Ui,
    lobby: &super::Lobby,
    race: Option<&OnlineRace>,
    circuits: &[TrackEntry],
    actions: &mut MessageWriter<NetAction>,
) {
    if !lobby.seated() {
        return;
    }
    ui.separator();
    let name = |track: u8| {
        circuits
            .get(usize::from(track))
            .map_or("?", |entry| entry.name.as_str())
    };
    let racing = lobby.announced().filter(|start| {
        race.is_none_or(|race| race.shared.race() != start.race || race.shared.over().is_none())
    });
    match (lobby.announced(), racing) {
        (Some(start), Some(_)) => {
            ui.label(format!(
                "Course n° {} en cours sur {} : {} tours, {} bots.",
                start.race,
                name(start.track),
                start.laps,
                start.bots
            ));
            if race.is_some_and(|race| !race.racing) {
                ui.small("Tu es arrivé pendant la course : tu roules librement, tu en seras à la prochaine.");
            }
        }
        (Some(start), None) => {
            ui.label(format!("Course n° {} terminée.", start.race));
        }
        (None, _) => {
            ui.label(format!("Roulage libre sur {}.", name(lobby.circuit())));
        }
    }
    if !lobby.is_host() {
        ui.small("L'hôte choisit le circuit et lance la course quand tout le monde est prêt.");
        return;
    }
    let free = racing.is_none();
    ui.horizontal(|ui| {
        ui.label("Circuit");
        let mut chosen = lobby.circuit();
        ui.add_enabled_ui(free, |ui| {
            egui::ComboBox::from_id_salt("lobby-circuit")
                .selected_text(name(chosen))
                .show_ui(ui, |ui| {
                    for (index, entry) in circuits.iter().enumerate() {
                        ui.selectable_value(&mut chosen, index as u8, &entry.name);
                    }
                });
        });
        if chosen != lobby.circuit() {
            actions.write(NetAction::ChooseCircuit(chosen));
        }
    });
    let ready = lobby.all_ready();
    if ui
        .add_enabled(
            free && ready,
            egui::Button::new(RichText::new("Lancer la course").strong().size(16.0)),
        )
        .clicked()
    {
        actions.write(NetAction::StartRace);
    }
    if free && !ready {
        ui.small("Il faut au moins deux joueurs, tous prêts.");
    }
    ui.small("Les bots complètent la grille de 12 ; tu les fais rouler pour tous.");
}

/// The Chrome setting that hides the local addresses behind `….local` names.
const CHROME_MDNS_FLAG: &str = "chrome://flags/#enable-webrtc-hide-local-ips-with-mdns";

/// The direct connections being made (issue #49): « en cours » after a moment, and the leads when
/// one does not come.
fn connections(ui: &mut egui::Ui, session: &NetSession, now_ms: u64) {
    match session.links.diagnosis(now_ms) {
        None => {}
        Some(Diagnosis::Connecting { peers }) => {
            ui.label(format!(
                "Connexion directe en cours avec {peers} joueur{}…",
                plural(peers)
            ));
        }
        Some(Diagnosis::Stuck { peers, cause }) => {
            ui.label(
                RichText::new(format!(
                    "La connexion directe avec {peers} joueur{} n'aboutit pas.",
                    plural(peers)
                ))
                .color(BAD),
            );
            let why = match cause {
                Stuck::NoCandidateHere => {
                    Some("Ton navigateur n'a donné aucune adresse réseau (aucun candidat ICE).")
                }
                Stuck::NoCandidateThere => Some(
                    "Le navigateur de l'autre joueur n'a donné aucune adresse réseau : \
                     les pistes sont pour lui.",
                ),
                Stuck::Failed => Some("Aucun chemin trouvé entre vous deux."),
                Stuck::Slow => None,
            };
            if let Some(why) = why {
                ui.small(why);
            }
            ui.small("Pistes, dans l'ordre :");
            ui.small(
                "1. macOS : Réglages Système > Confidentialité et sécurité > Réseau local : \
                 active ton navigateur, puis relance-le.",
            );
            ui.horizontal_wrapped(|ui| {
                ui.small(format!(
                    "2. Chrome : {CHROME_MDNS_FLAG} > Disabled, puis relance."
                ));
                if ui.small_button("Copier").clicked() {
                    ui.ctx().copy_text(CHROME_MDNS_FLAG.to_string());
                }
            });
            ui.small("3. Réseau qui bloque l'UDP : il faudra un relais TURN (prévu avec #10).");
            ui.small(
                "Détails : console (F12) et chrome://webrtc-internals (README, « Dépannage »).",
            );
        }
    }
    ui.small(format!("Mise en relation : {}", session.signal));
    ui.small(format!("Connexion directe : {}", session.ice.describe()));
}

fn plural(n: usize) -> &'static str {
    if n > 1 { "s" } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Natively, the invitation is the command line (no page address).
    #[test]
    fn natively_there_is_no_page() {
        assert_eq!(platform::page(), None);
        assert_eq!(platform::page_host(), None);
        // `cargo test` gives no `--room`.
        assert_eq!(platform::launch_options().room, None);
    }
}
