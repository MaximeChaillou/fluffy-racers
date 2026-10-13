//! What the admin panel keeps between two sessions (issue #36), and how it is asked for at
//! startup.
//!
//! - The modified tunings (the export text of `super::tuning`): in the browser's local storage
//!   on the web, in a file of the user's configuration folder natively
//!   (`~/.config/fluffy-racers/admin-tuning.ron`, `%APPDATA%\fluffy-racers\...` on Windows).
//!   Nothing modified: nothing stored.
//! - Opening the panel at startup: `?admin` in the page address, `--admin` natively.
//!
//! The same storage keeps other texts under their own key ([`load_key`], [`save_key`]): the
//! player's options (issue #17, `crate::options`).

/// Local storage key (web) / file name (native).
pub const STORAGE_KEY: &str = "fluffy-racers/admin-tuning";

/// Whether a page address query (`?admin`, `?x=1&admin`, `?admin=1`) asks for the panel.
/// `?admin=0` (or `false`, `non`) does not.
pub fn query_requests_admin(query: &str) -> bool {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            key.eq_ignore_ascii_case("admin").then_some(value)
        })
        .any(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "non"))
}

#[cfg(target_arch = "wasm32")]
mod platform {
    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    pub fn load(key: &str) -> Option<String> {
        storage()?.get_item(key).ok()?
    }

    pub fn save(key: &str, text: &str) {
        let Some(storage) = storage() else {
            return;
        };
        let result = if text.is_empty() {
            storage.remove_item(key)
        } else {
            storage.set_item(key, text)
        };
        if result.is_err() {
            bevy::log::warn!("could not write {key} in the browser's local storage");
        }
    }

    pub fn admin_requested() -> bool {
        web_sys::window()
            .and_then(|window| window.location().search().ok())
            .is_some_and(|query| super::query_requests_admin(&query))
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use std::path::PathBuf;

    fn path(key: &str) -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        let name = key.replace('/', "-");
        Some(base.join("fluffy-racers").join(format!("{name}.ron")))
    }

    pub fn load(key: &str) -> Option<String> {
        std::fs::read_to_string(path(key)?).ok()
    }

    pub fn save(key: &str, text: &str) {
        let Some(path) = path(key) else {
            return;
        };
        let result = if text.is_empty() {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        } else {
            path.parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, text))
        };
        if let Err(error) = result {
            bevy::log::warn!("could not write {}: {error}", path.display());
        }
    }

    pub fn admin_requested() -> bool {
        std::env::args().skip(1).any(|arg| arg == "--admin")
    }
}

/// The stored tunings, if any.
pub fn load() -> Option<String> {
    load_key(STORAGE_KEY)
}

/// Stores the tunings' `text` (an empty text removes what was stored).
pub fn save(text: &str) {
    save_key(STORAGE_KEY, text);
}

/// The text stored under `key` (`fluffy-racers/...`), if any.
pub fn load_key(key: &str) -> Option<String> {
    platform::load(key)
}

/// Stores `text` under `key` (an empty text removes what was stored).
pub fn save_key(key: &str, text: &str) {
    platform::save(key, text);
}

/// Whether the panel should be open at startup (`?admin`, `--admin`).
pub fn admin_requested() -> bool {
    platform::admin_requested()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_address_opens_the_panel() {
        for query in [
            "?admin",
            "?admin=1",
            "?x=2&admin",
            "?ADMIN",
            "admin",
            "?admin=oui",
        ] {
            assert!(query_requests_admin(query), "{query}");
        }
        for query in [
            "",
            "?",
            "?administrator",
            "?x=admin",
            "?admin=0",
            "?admin=false",
        ] {
            assert!(!query_requests_admin(query), "{query}");
        }
    }
}
