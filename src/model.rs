#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceId {
    Pacman,
    Aur,
}

impl SourceId {
    pub fn badge(self) -> &'static str {
        match self {
            SourceId::Pacman => "repo",
            SourceId::Aur => "aur",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceMeta {
    pub votes: Option<u32>,
    pub maintained: bool,
    pub out_of_date: bool,
    pub repo: Option<String>,
    /// AUR `LastModified` (unix seconds): when the package (PKGBUILD) last changed.
    pub last_modified: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageHit {
    pub name: String,
    pub version: String,
    pub source_id: SourceId,
    pub description: String,
    pub meta: SourceMeta,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub source_id: SourceId,
    pub version: String,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub meta: SourceMeta,
}

impl Provider {
    /// Short label for this provider: the concrete pacman repo name
    /// (e.g. "world", "extra-x86-64-v3") or "aur".
    pub fn badge(&self) -> &str {
        match self.source_id {
            SourceId::Pacman => self.meta.repo.as_deref().unwrap_or("repo"),
            SourceId::Aur => "aur",
        }
    }

    /// The command that installs `name` specifically from THIS provider.
    /// Pacman targets are repo-qualified (`repo/pkg`) so a non-default repo can
    /// be chosen; the AUR goes through yay.
    pub fn install_command(&self, name: &str) -> CommandLine {
        match self.source_id {
            SourceId::Pacman => {
                let target = match &self.meta.repo {
                    Some(repo) => format!("{repo}/{name}"),
                    None => name.to_string(),
                };
                CommandLine {
                    program: "sudo".into(),
                    args: vec!["pacman".into(), "-S".into(), target],
                }
            }
            SourceId::Aur => CommandLine {
                program: "yay".into(),
                args: vec!["-S".into(), name.into()],
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageRow {
    pub name: String,
    pub providers: Vec<Provider>,
    pub best_description: String,
}

impl PackageRow {
    pub fn any_installed(&self) -> bool {
        self.providers.iter().any(|p| p.installed)
    }
    pub fn has_source(&self, id: SourceId) -> bool {
        self.providers.iter().any(|p| p.source_id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    /// Remove a package. `recursive` switches `-R` to `-Rns` (also drops
    /// now-unneeded dependencies and config files).
    Remove { recursive: bool },
    /// Upgrade every installed package (repos, and the AUR when yay is present).
    Upgrade,
}

impl Action {
    /// The verb shown in the confirm modal title ("install", "remove", ...).
    pub fn verb(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Remove { .. } => "remove",
            Action::Upgrade => "upgrade",
        }
    }
}

/// Command that removes `name`. With `recursive`, uses `-Rns` to also drop
/// now-unneeded dependencies and saved configuration; otherwise plain `-R`.
pub fn remove_command(name: &str, recursive: bool) -> CommandLine {
    let flag = if recursive { "-Rns" } else { "-R" };
    CommandLine {
        program: "sudo".into(),
        args: vec!["pacman".into(), flag.into(), name.into()],
    }
}

/// Command that upgrades everything. `yay -Syu` when yay is present (covers
/// repos and the AUR), otherwise `sudo pacman -Syu` for repos only.
pub fn upgrade_command(has_yay: bool) -> CommandLine {
    if has_yay {
        CommandLine {
            program: "yay".into(),
            args: vec!["-Syu".into()],
        }
    } else {
        CommandLine {
            program: "sudo".into(),
            args: vec!["pacman".into(), "-Syu".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionSpec {
    pub targets: Vec<String>,
    pub source_id: SourceId,
    pub action: Action,
    pub command: CommandLine,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstalledStats {
    pub repo: usize,
    pub foreign: usize,
}

impl InstalledStats {
    pub fn total(&self) -> usize {
        self.repo + self.foreign
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdatesInfo {
    pub repo: Option<usize>,
    pub aur: Option<usize>,
}

/// Whole days between `ts` and `now` (unix seconds), clamped at 0.
pub fn days_ago(ts: i64, now: i64) -> i64 {
    (now - ts).max(0) / 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_badges() {
        assert_eq!(SourceId::Pacman.badge(), "repo");
        assert_eq!(SourceId::Aur.badge(), "aur");
    }

    #[test]
    fn days_ago_basic() {
        let now = 1_700_000_000;
        assert_eq!(days_ago(now - 3 * 86_400, now), 3);
        assert_eq!(days_ago(now, now), 0);
        assert_eq!(days_ago(now + 86_400, now), 0); // future clamps to 0
    }

    #[test]
    fn installed_stats_total() {
        let s = InstalledStats { repo: 1208, foreign: 77 };
        assert_eq!(s.total(), 1285);
    }

    #[test]
    fn install_command_qualifies_pacman_repo() {
        let p = Provider {
            source_id: SourceId::Pacman,
            version: "1".into(),
            installed: false,
            installed_version: None,
            meta: SourceMeta {
                repo: Some("extra-x86-64-v3".into()),
                ..Default::default()
            },
        };
        let cmd = p.install_command("neovim");
        assert_eq!(cmd.program, "sudo");
        assert_eq!(cmd.args, vec!["pacman", "-S", "extra-x86-64-v3/neovim"]);

        let aur = Provider {
            source_id: SourceId::Aur,
            version: "1".into(),
            installed: false,
            installed_version: None,
            meta: SourceMeta::default(),
        };
        let cmd = aur.install_command("tty-clock");
        assert_eq!(cmd.program, "yay");
        assert_eq!(cmd.args, vec!["-S", "tty-clock"]);
    }

    #[test]
    fn remove_command_default_and_recursive() {
        let plain = remove_command("firefox", false);
        assert_eq!(plain.program, "sudo");
        assert_eq!(plain.args, vec!["pacman", "-R", "firefox"]);

        let recursive = remove_command("firefox", true);
        assert_eq!(recursive.args, vec!["pacman", "-Rns", "firefox"]);
    }

    #[test]
    fn upgrade_command_prefers_yay() {
        let with_yay = upgrade_command(true);
        assert_eq!(with_yay.program, "yay");
        assert_eq!(with_yay.args, vec!["-Syu"]);

        let no_yay = upgrade_command(false);
        assert_eq!(no_yay.program, "sudo");
        assert_eq!(no_yay.args, vec!["pacman", "-Syu"]);
    }

    #[test]
    fn action_verbs() {
        assert_eq!(Action::Install.verb(), "install");
        assert_eq!(Action::Remove { recursive: false }.verb(), "remove");
        assert_eq!(Action::Remove { recursive: true }.verb(), "remove");
        assert_eq!(Action::Upgrade.verb(), "upgrade");
    }

    #[test]
    fn package_row_any_installed() {
        let row = PackageRow {
            name: "firefox".into(),
            best_description: String::new(),
            providers: vec![
                Provider {
                    source_id: SourceId::Pacman,
                    version: "1".into(),
                    installed: true,
                    installed_version: Some("1".into()),
                    meta: SourceMeta::default(),
                },
                Provider {
                    source_id: SourceId::Aur,
                    version: "1".into(),
                    installed: false,
                    installed_version: None,
                    meta: SourceMeta::default(),
                },
            ],
        };
        assert!(row.any_installed());
        assert!(row.has_source(SourceId::Aur));
    }
}
