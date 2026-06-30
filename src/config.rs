use crate::model::{
    AurHelper, HighlightMode, ReasonFilter, RemoveDepth, SortDir, SortKey, SourceId, VariantBadge,
};
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
    /// Collapse name variants (`gimp`/`gimp-bin`/`gimp-git`) into one row, picking
    /// the edition in the detail view. On by default; off restores one row per
    /// exact name. Aliased from the former `group_variants` key.
    #[serde(alias = "group_variants")]
    pub stack_variants: bool,
    /// Fold a name-matching Flatpak into the same row as its repo/AUR package.
    /// On by default; off keeps the Flatpak as its own row.
    pub group_flatpak: bool,
    /// How repeated same-source badges render: count (`aur ×3`) or repeat.
    pub variant_badge: VariantBadge,
    /// Sources the user has turned off. A disabled source is never detected,
    /// searched, stat-counted, or update-checked. Any source may be disabled,
    /// including all of them (honest empty state, not a blocked one).
    pub disabled_sources: Vec<SourceId>,
    /// Show the reverse-DNS app ID instead of the human name for Flatpak rows in
    /// the Manage list. Off by default (human names).
    pub flatpak_app_id: bool,
    /// Default Manage sort key at launch.
    pub default_manage_sort_key: SortKey,
    /// Default Manage sort direction at launch.
    pub default_manage_sort_dir: SortDir,
    /// Default for "float upgradable packages to the top" in Manage (on).
    pub default_manage_float_updates: bool,
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
            stack_variants: true,
            group_flatpak: true,
            variant_badge: VariantBadge::Count,
            disabled_sources: Vec::new(),
            flatpak_app_id: false,
            default_manage_sort_key: SortKey::Name,
            default_manage_sort_dir: SortDir::Asc,
            default_manage_float_updates: true,
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

/// Parse a settings file, recovering as much as possible when it does not fully
/// deserialize. A clean parse is returned as-is. Otherwise (a field renamed,
/// retyped, or carrying a now-invalid enum value across a plaza upgrade) each
/// stored field is reapplied one at a time and kept only if it still
/// deserializes, so a single incompatible field can no longer wipe the whole
/// config. Missing fields fall back to their defaults via `#[serde(default)]`.
fn parse_or_recover(s: &str) -> Settings {
    if let Ok(settings) = serde_json::from_str::<Settings>(s) {
        return settings;
    }
    let Ok(serde_json::Value::Object(stored)) = serde_json::from_str::<serde_json::Value>(s) else {
        return Settings::default();
    };
    let mut kept = serde_json::Map::new();
    for (key, value) in stored {
        let mut trial = kept.clone();
        trial.insert(key.clone(), value.clone());
        if serde_json::from_value::<Settings>(serde_json::Value::Object(trial)).is_ok() {
            kept.insert(key, value);
        }
    }
    serde_json::from_value(serde_json::Value::Object(kept)).unwrap_or_default()
}

impl Settings {
    pub fn load() -> Settings {
        let Some(path) = config_path() else {
            return Settings::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => parse_or_recover(&s),
            Err(_) => Settings::default(),
        }
    }

    /// Best-effort persist; failures are ignored (options still apply in-session).
    /// Writes to a sibling temp file and renames it into place so a crash mid-write
    /// cannot truncate an existing config into an empty/partial file (which would
    /// then load as all-defaults).
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(s) = serde_json::to_string_pretty(self) else { return };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &s).is_ok() && std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
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
    fn one_unparseable_field_does_not_wipe_the_rest() {
        // A settings file from a plaza version whose schema differs: one field
        // (here `highlight`) carries a value this build cannot deserialize. The
        // whole config must not reset to default; every still-valid field is kept
        // and only the bad one falls back. This is the "options reset to default
        // after upgrading plaza" guard.
        let json = r#"{
            "show_hotkeys": false,
            "palette": "catppuccin-mocha",
            "skin": "sharp",
            "debounce_ms": 250,
            "highlight": "ThisVariantNoLongerExists",
            "default_manage_sort_key": "Size"
        }"#;
        let s = parse_or_recover(json);
        assert!(!s.show_hotkeys, "show_hotkeys was wiped");
        assert_eq!(s.palette, "catppuccin-mocha", "palette was wiped");
        assert_eq!(s.skin, "sharp", "skin was wiped");
        assert_eq!(s.debounce_ms, 250, "debounce_ms was wiped");
        assert_eq!(s.default_manage_sort_key, SortKey::Size, "sort key was wiped");
        // The single bad field falls back to its default.
        assert_eq!(s.highlight, HighlightMode::default());
    }

    #[test]
    fn fully_valid_file_parses_through_recover_unchanged() {
        let s = Settings { palette: "nord".into(), show_hotkeys: false, ..Default::default() };
        let json = serde_json::to_string(&s).unwrap();
        let back = parse_or_recover(&json);
        assert_eq!(back.palette, "nord");
        assert!(!back.show_hotkeys);
    }

    #[test]
    fn garbage_file_falls_back_to_default() {
        assert_eq!(parse_or_recover("not json at all").palette, Settings::default().palette);
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
    fn legacy_group_variants_aliases_to_stack_variants() {
        let json = r#"{"group_variants":false}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(!s.stack_variants);
        assert!(s.group_flatpak); // new field defaults on
    }

    #[test]
    fn sort_defaults() {
        let s = Settings::default();
        assert_eq!(s.default_manage_sort_key, SortKey::Name);
        assert_eq!(s.default_manage_sort_dir, SortDir::Asc);
        assert!(s.default_manage_float_updates);
    }

    #[test]
    fn old_settings_without_sort_load_defaults() {
        let json = r#"{"show_hotkeys":true,"debounce_ms":400}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.default_manage_sort_key, SortKey::Name);
        assert!(s.default_manage_float_updates);
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
    fn default_group_variants_is_on_and_no_disabled_sources() {
        let s = Settings::default();
        assert!(s.stack_variants);
        assert!(s.group_flatpak);
        assert!(s.disabled_sources.is_empty());
    }

    #[test]
    fn old_settings_without_grouping_fields_load_as_defaults() {
        let json = r#"{"show_hotkeys":true,"debounce_ms":400}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert!(s.stack_variants);
        assert!(s.group_flatpak);
        assert!(s.disabled_sources.is_empty());
    }

    #[test]
    fn roundtrip_keeps_disabled_sources() {
        let s = Settings { disabled_sources: vec![SourceId::Aur], ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.disabled_sources, vec![SourceId::Aur]);
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
