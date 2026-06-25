use crate::model::RemoveDepth;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User-tweakable options, persisted to `~/.config/plaza/settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Show the keybinding hints in the status bar.
    pub show_hotkeys: bool,
    /// Collapse all pacman repos into a single `[official]` provider (using the
    /// default/highest-priority repo) instead of showing each repo separately.
    pub collapse_repos: bool,
    /// Debounce before a search fires, in ms. Raise above your terminal's
    /// key-repeat delay so holding a key doesn't flash intermediate results.
    pub debounce_ms: u64,
    /// How aggressively `Remove` cleans up (`-R` / `-Rs` / `-Rns`).
    pub remove_depth: RemoveDepth,
    /// Active color palette name (built-in or a file in
    /// `~/.config/plaza/palettes/`).
    pub palette: String,
    /// Active skin name (built-in or a file in `~/.config/plaza/skins/`).
    pub skin: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            show_hotkeys: true,
            collapse_repos: false,
            debounce_ms: 400,
            remove_depth: RemoveDepth::WithDeps,
            palette: crate::theme::DEFAULT_PALETTE.to_string(),
            skin: crate::theme::DEFAULT_SKIN.to_string(),
        }
    }
}

/// The XDG config base (`$XDG_CONFIG_HOME` or `~/.config`), shared by the
/// settings file and the theme directories.
pub fn config_base() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

fn config_path() -> Option<PathBuf> {
    Some(config_base()?.join("plaza").join("settings.json"))
}

impl Settings {
    pub fn load() -> Settings {
        let Some(path) = config_path() else {
            return Settings::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Settings::default(),
        }
    }

    /// Best-effort persist; failures are ignored (options still apply in-session).
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_theme() {
        let s = Settings::default();
        assert_eq!(s.palette, "plaza-dusk");
        assert_eq!(s.skin, "soft");
    }

    #[test]
    fn roundtrip_keeps_theme() {
        let s = Settings {
            palette: "nord".into(),
            skin: "sharp".into(),
            ..Default::default()
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(back.palette, "nord");
        assert_eq!(back.skin, "sharp");
    }
}
