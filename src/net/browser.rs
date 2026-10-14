//! The page's WebRTC shim (`index.html`, issue #49), seen from the game. `matchbox_socket` creates
//! the browser's `RTCPeerConnection`s itself; the page replaces that constructor with a subclass
//! (`window.fluffyNet` holds its state) that:
//! - drops an ICE server entry without URL: `matchbox_socket` always gives one, and the browser
//!   refuses it empty (« Empty uri »), where no STUN server is wanted (a local network);
//! - stops waiting for the ICE candidates after `gatheringMaxMs` (`NetSettings`): the offer or
//!   the answer leaves with those found so far, the next ones follow;
//! - counts the connections the browser saw fail (`failures`), for the lobby.
//!
//! Natively there is no page: webrtc-rs takes an empty list as no server, and sends its
//! candidates one by one.

#[cfg(target_arch = "wasm32")]
mod platform {
    use web_sys::js_sys::{Object, Reflect};
    use web_sys::wasm_bindgen::{JsCast, JsValue};

    fn shim() -> Option<Object> {
        let window = web_sys::window()?;
        Reflect::get(&window, &JsValue::from_str("fluffyNet"))
            .ok()?
            .dyn_into::<Object>()
            .ok()
    }

    pub fn accepts_no_ice_server() -> bool {
        shim().is_some()
    }

    pub fn set_gathering_max_ms(ms: u32) {
        if let Some(shim) = shim() {
            let _ = Reflect::set(
                &shim,
                &JsValue::from_str("gatheringMaxMs"),
                &JsValue::from(ms),
            );
        }
    }

    pub fn ice_failures() -> u32 {
        shim()
            .and_then(|shim| Reflect::get(&shim, &JsValue::from_str("failures")).ok())
            .and_then(|value| value.as_f64())
            .map_or(0, |count| count as u32)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    pub fn accepts_no_ice_server() -> bool {
        true
    }

    pub fn set_gathering_max_ms(_ms: u32) {}

    pub fn ice_failures() -> u32 {
        0
    }
}

pub use platform::{accepts_no_ice_server, ice_failures, set_gathering_max_ms};
