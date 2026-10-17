//! The lobby window and the room asked for at startup (issue #7, see `super`). Only with a
//! renderer: the headless tests drive the network with [`NetAction`]s, which the window's
//! buttons write too.
//!
//! Like the other windows, it costs nothing once closed: the egui context only exists while one
//! of them is open (`admin::ui::attach_egui_context`). It does not pause the game: the race goes
//! on (solo, until issue #9) while the players gather.
//!
//! Issue #64: in a room, out of a race we run, a **banner** at the top of the screen says the
//! room, its players and what to do (`guide.rs`, [`LobbyWindow::banner`]): egui too, for its
//! accents (the HUD's font has none); no room, no banner, no context. The window puts the main
//! action first: the host's « Lancer la course », with why it waits, a client's « Prêt ! ».
//!
//! Issue #80: the window starts with the player's **pseudo** (`super::pseudo`: 12 characters,
//! those the HUD's font writes; empty: the character's name), kept in the browser (read here
//! before anything is built; `?pseudo=` wins over it). Issue #98: it is typed in the « Pilote »
//! screen (`menu::pilot`, G), which also stores it; each change goes to the others at once (the
//! `Hello`). While the « Pilote » screen is open, the window and the banner step aside: the
//! screen is in the middle of the window, one at a time.

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use bevy_egui::{EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};

#[cfg(target_arch = "wasm32")]
use super::NetSystems;
use super::ice::{is_loopback_host, signal_host};
use super::pseudo::{self, PSEUDO_STORAGE_KEY};
use super::{
    Diagnosis, LobbyWindow, MAX_PLAYERS, NetAction, NetSession, NetSettings, NetStatus, NetTuning,
    OnlineRace, RaceStage, RoomFacts, RowLink, Seated, Stage, Stuck, default_signal, invite_link,
    is_local_signal, native_command, now_ms,
};
use crate::admin::tracks::{TrackEntry, circuits};
use crate::admin::{keep_focus_for_text_only, storage};
use crate::characters::{PartKind, PlayerLoadout, Roster, SelectionScreen};
use crate::kart::KartColor;

const GOOD: Color32 = Color32::from_rgb(110, 210, 120);
const BAD: Color32 = Color32::from_rgb(240, 110, 90);
const HOST: Color32 = Color32::from_rgb(255, 190, 70);
/// The banner's background: the results screen's.
const BANNER_FILL: Color32 = Color32::from_rgba_premultiplied(18, 14, 37, 222);
/// The main buttons (« Lancer la course », « Prêt ! »).
const ACTION_FILL: Color32 = Color32::from_rgb(52, 140, 72);
const ACTION_SIZE: egui::Vec2 = egui::vec2(240.0, 44.0);

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
        .add_systems(PreStartup, load_stored_pseudo)
        .add_systems(Startup, join_launch_room)
        .add_systems(
            EguiPrimaryContextPass,
            (
                room_banner.run_if(|window: Res<LobbyWindow>| window.banner),
                lobby_window.run_if(|window: Res<LobbyWindow>| window.open),
            )
                // Issue #98: not over the « Pilote » screen (`bevy_ui`, under egui).
                .run_if(|screen: Option<Res<SelectionScreen>>| !screen.is_some_and(|s| s.open)),
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

/// Issue #80: the pseudo kept from the previous sessions, before anything is built (the `Hello`
/// online); `?pseudo=` comes after it (`join_launch_room`). An unreadable text is ignored (the
/// character's name then), with a warning.
fn load_stored_pseudo(mut settings: ResMut<NetSettings>) {
    let Some(text) = storage::load_key(PSEUDO_STORAGE_KEY) else {
        return;
    };
    match pseudo::from_stored(&text) {
        Ok(stored) => settings.pseudo = Some(stored),
        Err(error) => warn!("network: stored pseudo ignored: {error}"),
    }
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
    // Issue #80: `?pseudo=` wins over the kept pseudo (without replacing it), with the field's
    // rules; one with nothing left of it is ignored.
    if let Some(pseudo) = options.pseudo.as_deref().and_then(pseudo::clean) {
        settings.pseudo = Some(pseudo);
    }
    if let Some(room) = options.room {
        // Issue #63: `host=1` (`--host`) only in the creator's own address.
        actions.write(if options.host {
            NetAction::Host(room)
        } else {
            NetAction::Join(room)
        });
    }
}

/// The room's code in the address bar (`?room=CODE`), so that a reload stays in it; removed when
/// the player leaves. Issue #63: the host's says `&host=1` (a reload hosts the room again), never
/// the invitation link.
#[cfg(target_arch = "wasm32")]
fn page_address(
    session: Option<Res<NetSession>>,
    mut shown: Local<Option<(super::RoomCode, bool)>>,
) {
    let room = session.map(|session| (session.room.clone(), session.hosting));
    if *shown != room {
        platform::show_room(room.as_ref().map(|(room, host)| (room, *host)));
        *shown = room;
    }
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::super::{LaunchOptions, RoomCode, host_query, query_with_room};

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

    pub fn show_room(room: Option<(&RoomCode, bool)>) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let location = window.location();
        let (Ok(path), Ok(query), Ok(hash)) =
            (location.pathname(), location.search(), location.hash())
        else {
            return;
        };
        let query = match room {
            Some((room, true)) => host_query(&query, room),
            Some((room, false)) => query_with_room(&query, Some(room)),
            None => query_with_room(&query, None),
        };
        let address = format!("{path}{query}{hash}");
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

/// Issue #64: the room's banner, at the top of the screen (the HUD keeps its corners). It takes
/// neither the mouse nor the keys.
fn room_banner(
    mut contexts: EguiContexts,
    session: Option<Res<NetSession>>,
    race: Option<Res<OnlineRace>>,
    time: Res<Time<Real>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Some(banner) = RoomFacts::of(session.as_deref(), race.as_deref(), now_ms(&time))
        .and_then(|facts| facts.banner())
    else {
        return;
    };
    egui::Area::new(egui::Id::new("fluffy-room-banner"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 12.0])
        .interactable(false)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(BANNER_FILL)
                .corner_radius(12)
                .inner_margin(egui::Margin::symmetric(16, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if !banner.lead.is_empty() {
                            ui.label(RichText::new(&banner.lead).strong().size(18.0).color(HOST));
                            ui.label(RichText::new(" · ").size(18.0).color(Color32::WHITE));
                        }
                        ui.label(
                            RichText::new(&banner.action)
                                .size(18.0)
                                .color(Color32::WHITE),
                        );
                    });
                });
        });
}

fn lobby_window(
    mut contexts: EguiContexts,
    session: Option<Res<NetSession>>,
    settings: Res<NetSettings>,
    roster: Option<Res<Roster>>,
    loadout: Option<Res<PlayerLoadout>>,
    race: Option<Res<OnlineRace>>,
    time: Res<Time<Real>>,
    mut state: ResMut<LobbyUiState>,
    mut actions: MessageWriter<NetAction>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // The game's keys (Espace, the arrows) must not click or move through the window's buttons.
    keep_focus_for_text_only(ctx);
    let now = time.elapsed_secs_f64();
    egui::Window::new("En ligne")
        .id(egui::Id::new("fluffy-lobby"))
        .anchor(egui::Align2::LEFT_TOP, [10.0, 110.0])
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            // Issue #80: who we are, first; issue #98: typed in the « Pilote » screen.
            let name = settings.pseudo.clone().unwrap_or_else(|| {
                super::character_name(loadout.as_deref().map(|l| &l.0), roster.as_deref())
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ton pseudo").strong());
                ui.label(RichText::new(name).strong().color(HOST));
            });
            ui.small("Change-le dans Pilote (G).");
            ui.separator();
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
    ui.label("Joue avec tes amis : crée une partie (tu en es l'hôte), puis envoie-leur le lien.");
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
    ui.label(RichText::new(format!("Salon {room}")).strong().size(20.0));
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
            ui.label("Connexion à la partie…");
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
        // Issue #63: the host gone, the room is over (nobody takes over).
        NetStatus::HostLeft => {
            ui.label(RichText::new("L'hôte a quitté la partie.").color(BAD));
            ui.small("La course s'arrête : tu reviens au circuit d'avant.");
            ui.horizontal(|ui| {
                if ui.button("Rejoindre de nouveau").clicked() {
                    actions.write(NetAction::Join(room.clone()));
                }
                if ui.button("Créer une partie").clicked() {
                    actions.write(NetAction::CreateRoom);
                }
            });
            ui.small("« Rejoindre de nouveau » : si l'hôte a rechargé sa page, la partie reprend.");
        }
        NetStatus::HostTaken => {
            ui.label(RichText::new(format!("La partie {room} a déjà un hôte.")).color(BAD));
            ui.small(
                "Cette adresse est celle de l'hôte (« host=1 ») : pour la rejoindre, ouvre le lien \
                 d'invitation, sans « host=1 ».",
            );
            ui.horizontal(|ui| {
                if ui.button("La rejoindre").clicked() {
                    actions.write(NetAction::Join(room.clone()));
                }
                if ui.button("Créer une autre partie").clicked() {
                    actions.write(NetAction::CreateRoom);
                }
            });
        }
        NetStatus::InRoom if session.host_missing(now_ms) => {
            ui.label(RichText::new("Cette partie n'existe plus.").color(BAD));
            ui.small(format!(
                "Personne n'héberge la partie {room} : son hôte (celui qui l'a créée) est parti, \
                 ou ne l'a pas encore ouverte."
            ));
            if ui.button("Créer une partie").clicked() {
                actions.write(NetAction::CreateRoom);
            }
            connections(ui, session, now_ms);
        }
        NetStatus::InRoom => {
            let facts = RoomFacts::of(Some(session), race, now_ms);
            match (facts.map(|facts| facts.stage), &session.lobby) {
                (Some(Stage::Seated(seated)), Some(lobby)) => seated_room(
                    ui,
                    session,
                    lobby,
                    &seated,
                    roster,
                    &state.circuits,
                    now_ms,
                    actions,
                ),
                (_, lobby) => {
                    ui.label("Connexion à l'hôte de la partie…");
                    if lobby.is_some() {
                        player_list(ui, session, roster, now_ms);
                    } else {
                        connections(ui, session, now_ms);
                    }
                }
            }
        }
    }
    ui.separator();
    if ui.button("Quitter la partie").clicked() {
        actions.write(NetAction::Leave);
    }
}

/// A swatch of a kart's color, then its name (issue #55).
fn color_cell(ui: &mut egui::Ui, color: KartColor) {
    let [r, g, b] = color.rgb();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 3.0, Color32::from_rgb(r, g, b));
    ui.label(color.label());
}

/// What the lobby says when a color was taken (issue #55): `None` when every player wears the
/// color it chose.
pub fn color_notice(rows: &[super::PlayerRow]) -> Option<String> {
    let mine = rows
        .iter()
        .find(|row| row.me && row.seated && row.chosen != row.color)?;
    let (Some(chosen), Some(worn)) = (mine.chosen, mine.color) else {
        return None;
    };
    Some(format!(
        "Couleur déjà prise : tu es en {} au lieu de {}.",
        worn.label(),
        chosen.label()
    ))
}

/// Seated in the room (issue #64): who does what, the free driving meanwhile, the main action,
/// then the players.
#[allow(clippy::too_many_arguments)]
fn seated_room(
    ui: &mut egui::Ui,
    session: &NetSession,
    lobby: &super::Lobby,
    seated: &Seated,
    roster: Option<&Roster>,
    circuits: &[TrackEntry],
    now_ms: u64,
    actions: &mut MessageWriter<NetAction>,
) {
    ui.label(RichText::new(seated.role()).strong());
    if let Some(meanwhile) = seated.meanwhile() {
        ui.label(meanwhile);
    }
    ui.add_space(6.0);
    main_action(ui, lobby, seated, circuits, actions);
    ui.separator();
    player_list(ui, session, roster, now_ms);
}

/// What is on in the room (issue #9), then the main action (issue #64): the host chooses the
/// circuit and starts the race, a big button with why it waits; a client says « Prêt ! ».
fn main_action(
    ui: &mut egui::Ui,
    lobby: &super::Lobby,
    seated: &Seated,
    circuits: &[TrackEntry],
    actions: &mut MessageWriter<NetAction>,
) {
    let name = |track: u8| {
        circuits
            .get(usize::from(track))
            .map_or("?", |entry| entry.name.as_str())
    };
    let racing = matches!(seated.race, RaceStage::Racing | RaceStage::Watching);
    match (lobby.announced(), seated.race) {
        (Some(start), RaceStage::Racing | RaceStage::Watching) => {
            ui.label(format!(
                "Course n° {} en cours sur {} : {} tours, {} bots.",
                start.race,
                name(start.track),
                start.laps,
                start.bots
            ));
        }
        (Some(start), _) => {
            ui.label(format!("Course n° {} terminée.", start.race));
        }
        (None, _) => {
            ui.label(format!("Roulage libre sur {}.", name(lobby.circuit())));
        }
    }
    if seated.hosting {
        ui.horizontal(|ui| {
            ui.label("Circuit");
            let mut chosen = lobby.circuit();
            ui.add_enabled_ui(!racing, |ui| {
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
        if ui
            .add_enabled(seated.can_start(), action_button("Lancer la course"))
            .clicked()
        {
            actions.write(NetAction::StartRace);
        }
        match seated.start_blocker() {
            Some(reason) => ui.label(RichText::new(reason).color(HOST)),
            None => ui.label(RichText::new("Tout le monde est prêt !").color(GOOD)),
        };
        ui.small("Les bots complètent la grille de 12 ; tu les fais rouler pour tous.");
    } else {
        let clicked = if seated.ready {
            ui.button("Pas prêt").clicked()
        } else {
            ui.add(action_button("Prêt !")).clicked()
        };
        if clicked {
            actions.write(NetAction::Ready(!seated.ready));
        }
        let hint = RichText::new(seated.client_hint());
        ui.label(if seated.ready { hint.color(GOOD) } else { hint });
    }
}

/// The lobby's big button (issue #64).
fn action_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(
        RichText::new(text)
            .strong()
            .size(22.0)
            .color(Color32::WHITE),
    )
    .fill(ACTION_FILL)
    .min_size(ACTION_SIZE)
}

/// The players of the room, and the links with the host being made.
fn player_list(ui: &mut egui::Ui, session: &NetSession, roster: Option<&Roster>, now_ms: u64) {
    let rows = session.players(now_ms);
    let seated = rows.iter().filter(|row| row.seated).count();
    ui.small(format!(
        "{seated} joueur{} sur {MAX_PLAYERS}",
        plural(seated)
    ));
    egui::Grid::new("lobby-players")
        .num_columns(5)
        .spacing([10.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Joueur").strong());
            ui.label(RichText::new("Animal").strong());
            ui.label(RichText::new("Couleur").strong());
            // Issue #63: each player's link with the host.
            ui.label(RichText::new("Lien").strong());
            ui.label(RichText::new("Prêt").strong());
            ui.end_row();
            for row in &rows {
                let mut name = if row.name.is_empty() {
                    if row.host { "L'hôte" } else { "…" }.to_string()
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
                    ui.label("");
                    ui.end_row();
                    continue;
                }
                let animal = roster
                    .map(|r| r.name(PartKind::Character, usize::from(row.loadout[0])))
                    .unwrap_or("");
                ui.label(if row.seated || row.me {
                    animal
                } else if matches!(row.link, RowLink::Connecting | RowLink::Stuck(_)) {
                    ""
                } else {
                    "arrive…"
                });
                // Issue #55: the color it wears (the one it chose, if another took it first).
                ui.horizontal(|ui| match row.color {
                    Some(color) => {
                        color_cell(ui, color);
                        if let Some(chosen) = row.chosen.filter(|chosen| *chosen != color) {
                            ui.small(format!("({} pris)", chosen.label()));
                        }
                    }
                    None if matches!(row.link, RowLink::Connecting | RowLink::Stuck(_)) => {
                        ui.label("");
                    }
                    None => {
                        ui.label("…");
                    }
                });
                let link = match (row.link, row.silent, row.ping_ms) {
                    (RowLink::Me, _, _) => RichText::new("—"),
                    (RowLink::Connecting, _, _) => RichText::new("en cours…"),
                    (RowLink::Stuck(_), _, _) => RichText::new("échec").color(BAD),
                    // Through the host: connected to it (its link is the host's to show).
                    (RowLink::ViaHost, _, _) => RichText::new("connecté"),
                    (_, true, _) => RichText::new("muet").color(BAD),
                    (_, _, Some(ms)) => RichText::new(format!("connecté, {ms:.0} ms")),
                    (_, _, None) => RichText::new("connecté"),
                };
                ui.label(link);
                // Issue #64: the host is ready by starting the race.
                ui.label(if row.host {
                    RichText::new("")
                } else if row.ready {
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
    if let Some(notice) = color_notice(&rows) {
        ui.label(RichText::new(notice).color(HOST));
        ui.small("Change de couleur dans Pilote (G).");
    }
    connections(ui, session, now_ms);
}

/// The Chrome setting that hides the local addresses behind `….local` names.
const CHROME_MDNS_FLAG: &str = "chrome://flags/#enable-webrtc-hide-local-ips-with-mdns";

/// The direct connections being made (issue #49): « en cours » after a moment, and the leads when
/// one does not come, folded in « Dépannage » (issue #64: the network's words only there).
fn connections(ui: &mut egui::Ui, session: &NetSession, now_ms: u64) {
    match session.links.diagnosis(now_ms) {
        None => {}
        Some(Diagnosis::Connecting { .. }) if !session.hosting => {
            ui.label("Connexion à l'hôte en cours…");
        }
        Some(Diagnosis::Connecting { peers }) => {
            ui.label(format!(
                "Connexion en cours avec {peers} joueur{}…",
                plural(peers)
            ));
        }
        Some(Diagnosis::Stuck { peers, cause }) => {
            let text = if session.hosting {
                format!(
                    "La connexion avec {peers} joueur{} n'aboutit pas.",
                    plural(peers)
                )
            } else {
                "La connexion avec l'hôte n'aboutit pas.".to_string()
            };
            ui.label(RichText::new(text).color(BAD));
            egui::CollapsingHeader::new("Dépannage")
                .id_salt("lobby-troubleshooting")
                .default_open(false)
                .show(ui, |ui| troubleshooting(ui, cause));
        }
    }
    egui::CollapsingHeader::new("Détails de connexion")
        .id_salt("lobby-connection-details")
        .default_open(false)
        .show(ui, |ui| {
            ui.small(format!("Mise en relation : {}", session.signal));
            ui.small(format!("Connexion directe : {}", session.ice.describe()));
        });
}

/// The leads when a direct connection does not come (issue #49).
fn troubleshooting(ui: &mut egui::Ui, cause: Stuck) {
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
    ui.small("Détails : console (F12) et chrome://webrtc-internals (README, « Dépannage »).");
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
