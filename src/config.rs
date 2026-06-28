use crate::model::{AurHelper, HighlightMode, ReasonFilter, RemoveDepth};
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
    /// Which AUR helper to drive for install/upgrade. `Auto` picks paru, else yay.
    pub aur_helper: AurHelper,
    /// When on, the repo-filter box shows only while it is focused or while a
    /// filter is active. When off, the box is always on screen. On by default.
    pub hide_idle_filter: bool,
    /// Active color palette name (built-in or a file in
    /// `~/.config/plaza/palettes/`).
    pub palette: String,
    /// Active skin name (built-in or a file in `~/.config/plaza/skins/`).
    pub skin: String,
    /// How the matched substring is drawn in the package-name cell.
    pub highlight: HighlightMode,
    /// Repo ids hidden by default in the Search filter (restored at launch).
    pub default_search_filter_off: Vec<String>,
    /// Repo ids hidden by default in the Manage filter (restored at launch).
    pub default_manage_filter_off: Vec<String>,
    /// Default Manage installation-reason filter at launch.
    pub default_reason: ReasonFilter,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            show_hotkeys: true,
            collapse_repos: false,
            debounce_ms: 400,
            remove_depth: RemoveDepth::WithDeps,
            aur_helper: AurHelper::Auto,
            hide_idle_filter: true,
            palette: crate::theme::DEFAULT_PALETTE.to_string(),
            skin: crate::theme::DEFAULT_SKIN.to_string(),
            highlight: HighlightMode::default(),
            default_search_filter_off: Vec::new(),
            default_manage_filter_off: Vec::new(),
            default_reason: ReasonFilter::default(),
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
    fn default_aur_helper_is_auto() {
        assert_eq!(Settings::default().aur_helper, AurHelper::Auto);
    }

    #[test]
    fn old_settings_without_aur_helper_load_as_auto() {
        // A settings file written before this field existed must still load.
        let json = r#"{"show_hotkeys":true,"collapse_repos":false,"debounce_ms":400}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.aur_helper, AurHelper::Auto);
    }

    #[test]
    fn roundtrip_keeps_aur_helper() {
        let s = Settings { aur_helper: AurHelper::Paru, ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.aur_helper, AurHelper::Paru);
    }

    #[test]
    fn default_reason_is_all() {
        assert_eq!(Settings::default().default_reason, ReasonFilter::All);
    }

    #[test]
    fn old_settings_without_reason_load_as_all() {
        let json = r#"{"show_hotkeys":true,"debounce_ms":400}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.default_reason, ReasonFilter::All);
    }

    #[test]
    fn defaults_have_empty_filter_defaults() {
        let s = Settings::default();
        assert!(s.default_search_filter_off.is_empty());
        assert!(s.default_manage_filter_off.is_empty());
    }

    #[test]
    fn old_settings_without_filter_defaults_load_empty() {
        let json = r#"{"show_hotkeys":true,"debounce_ms":400}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(s.default_search_filter_off.is_empty());
        assert!(s.default_manage_filter_off.is_empty());
    }

    #[test]
    fn roundtrip_keeps_filter_defaults() {
        let s = Settings {
            default_manage_filter_off: vec!["multilib".into()],
            ..Default::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.default_manage_filter_off, vec!["multilib".to_string()]);
    }

    #[test]
    fn default_highlight_is_underline() {
        assert_eq!(Settings::default().highlight, HighlightMode::Underline);
    }

    #[test]
    fn old_settings_without_highlight_load_as_underline() {
        let json = r#"{"show_hotkeys":true,"debounce_ms":400}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.highlight, HighlightMode::Underline);
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
