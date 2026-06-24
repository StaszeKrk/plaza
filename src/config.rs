use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User-tweakable options, persisted to `~/.config/plaza/settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Show the keybinding hints in the status bar.
    pub show_hotkeys: bool,
    /// Repo names whose providers are hidden from badges and the detail view.
    pub hidden_repos: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            show_hotkeys: true,
            hidden_repos: Vec::new(),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("plaza").join("settings.json"))
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

    pub fn is_repo_hidden(&self, repo: &str) -> bool {
        self.hidden_repos.iter().any(|r| r == repo)
    }

    pub fn toggle_repo(&mut self, repo: &str) {
        if let Some(i) = self.hidden_repos.iter().position(|r| r == repo) {
            self.hidden_repos.remove(i);
        } else {
            self.hidden_repos.push(repo.to_string());
        }
    }
}
