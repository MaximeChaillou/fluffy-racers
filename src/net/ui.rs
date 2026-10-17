//! The room asked for at startup (issue #7, see `super`), the pseudo kept in the browser (issue
//! #80) and the page's address. Only with a renderer: the headless tests drive the network with
//! [`NetAction`]s, which the lobby's buttons write too.
//!
//! Issue #101: the lobby's window and its banner (egui since issues #7 and #64) are now the
//! lobby's screen of the charter, `bevy_ui`, over its 3D scene (`crate::salon`): no egui context
//! for them any more.
//!
//! Issue #80: the player's **pseudo** (`super::pseudo`: 12 characters, those the HUD's font
//! writes; empty: the character's name) is kept in the browser (read here before anything is
//! built; `?pseudo=` wins over it). Issue #98: it is typed in the « Pilote » screen
//! (`menu::pilot`, G), which also stores it; each change goes to the others at once (the
//! `Hello`).

use bevy::prelude::*;

#[cfg(target_arch = "wasm32")]
use super::NetSystems;
use super::ice::is_loopback_host;
use super::pseudo::{self, PSEUDO_STORAGE_KEY};
use super::{
    NetAction, NetSession, NetSettings, NetTuning, default_signal, invite_link, native_command,
};
use crate::admin::storage;

pub struct NetUiPlugin;

impl Plugin for NetUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_stored_pseudo)
            .add_systems(Startup, join_launch_room);
        #[cfg(target_arch = "wasm32")]
        app.add_systems(Update, page_address.after(NetSystems));
    }
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

/// The invitation of `session`'s room: the page's link (`?room=CODE`, its signaling server), or
/// natively the command line.
pub fn invitation(session: &NetSession) -> String {
    platform::page()
        .map(|page| invite_link(&page, &session.room, &session.signal))
        .unwrap_or_else(|| native_command(&session.room, &session.signal))
}

/// The page is served from this machine's loopback (`localhost`): the browser does not know the
/// machine's address on the network, nobody else can open the link.
pub fn loopback_page() -> bool {
    platform::page_host().is_some_and(|host| is_loopback_host(&host))
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
