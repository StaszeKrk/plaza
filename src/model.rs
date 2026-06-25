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
    /// Remove a package. The depth (`-R`/`-Rs`/`-Rns`) comes from settings.
    Remove,
    /// Upgrade packages: a single source, or every source chained together.
    Upgrade,
}

impl Action {
    /// The verb shown in the confirm modal title ("install", "remove", ...).
    pub fn verb(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Remove => "remove",
            Action::Upgrade => "upgrade",
        }
    }
}

/// How aggressively a removal cleans up. Maps to pacman's `-R` family. The
/// default is `WithDeps` (`-Rs`); the user can change it in Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RemoveDepth {
    /// `-R`: remove only the named package.
    Package,
    /// `-Rs`: also remove now-unneeded dependencies.
    WithDeps,
    /// `-Rns`: also remove saved config (`.pacsave`) files.
    Purge,
}

impl RemoveDepth {
    pub fn flag(self) -> &'static str {
        match self {
            RemoveDepth::Package => "-R",
            RemoveDepth::WithDeps => "-Rs",
            RemoveDepth::Purge => "-Rns",
        }
    }

    /// Short label for the options overlay.
    pub fn label(self) -> &'static str {
        match self {
            RemoveDepth::Package => "package only (-R)",
            RemoveDepth::WithDeps => "+ unused deps (-Rs)",
            RemoveDepth::Purge => "+ deps + config (-Rns)",
        }
    }

    /// Cycle order for the options overlay.
    pub fn next(self) -> RemoveDepth {
        match self {
            RemoveDepth::WithDeps => RemoveDepth::Purge,
            RemoveDepth::Purge => RemoveDepth::Package,
            RemoveDepth::Package => RemoveDepth::WithDeps,
        }
    }
}

/// Command that removes `name` at the given depth. Removal goes through pacman
/// for both native and foreign (AUR) packages; a foreign package is still
/// tracked in the local db.
pub fn remove_command(name: &str, depth: RemoveDepth) -> CommandLine {
    CommandLine {
        program: "sudo".into(),
        args: vec!["pacman".into(), depth.flag().into(), name.into()],
    }
}

/// The full-upgrade command for a single source.
/// - pacman: `sudo pacman -Syu` (sync + upgrade the repos)
/// - aur:    `yay -Sua` (upgrade AUR packages only)
pub fn source_upgrade_command(source_id: SourceId) -> CommandLine {
    match source_id {
        SourceId::Pacman => CommandLine {
            program: "sudo".into(),
            args: vec!["pacman".into(), "-Syu".into()],
        },
        SourceId::Aur => CommandLine {
            program: "yay".into(),
            args: vec!["-Sua".into()],
        },
    }
}

/// Chain commands into one `sh -c "a && b"` so they run as a single PTY task
/// (used by "upgrade all" to upgrade each source in order). A single command is
/// returned unwrapped; an empty slice yields a harmless `true`.
pub fn chain_commands(cmds: &[CommandLine]) -> CommandLine {
    match cmds {
        [] => CommandLine { program: "true".into(), args: vec![] },
        [one] => one.clone(),
        many => {
            let joined = many
                .iter()
                .map(|c| {
                    if c.args.is_empty() {
                        c.program.clone()
                    } else {
                        format!("{} {}", c.program, c.args.join(" "))
                    }
                })
                .collect::<Vec<_>>()
                .join(" && ");
            CommandLine {
                program: "sh".into(),
                args: vec!["-c".into(), joined],
            }
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
    fn remove_command_uses_depth_flag() {
        assert_eq!(
            remove_command("firefox", RemoveDepth::WithDeps).args,
            vec!["pacman", "-Rs", "firefox"]
        );
        assert_eq!(
            remove_command("firefox", RemoveDepth::Package).args,
            vec!["pacman", "-R", "firefox"]
        );
        assert_eq!(
            remove_command("firefox", RemoveDepth::Purge).args,
            vec!["pacman", "-Rns", "firefox"]
        );
    }

    #[test]
    fn remove_depth_cycles() {
        assert_eq!(RemoveDepth::WithDeps.next(), RemoveDepth::Purge);
        assert_eq!(RemoveDepth::Purge.next(), RemoveDepth::Package);
        assert_eq!(RemoveDepth::Package.next(), RemoveDepth::WithDeps);
    }

    #[test]
    fn source_upgrade_commands() {
        assert_eq!(
            source_upgrade_command(SourceId::Pacman).args,
            vec!["pacman", "-Syu"]
        );
        let aur = source_upgrade_command(SourceId::Aur);
        assert_eq!(aur.program, "yay");
        assert_eq!(aur.args, vec!["-Sua"]);
    }

    #[test]
    fn chain_commands_single_and_many() {
        let pac = source_upgrade_command(SourceId::Pacman);
        let aur = source_upgrade_command(SourceId::Aur);
        // single → unwrapped
        assert_eq!(chain_commands(std::slice::from_ref(&pac)), pac);
        // many → sh -c "a && b"
        let all = chain_commands(&[pac, aur]);
        assert_eq!(all.program, "sh");
        assert_eq!(
            all.args,
            vec!["-c".to_string(), "sudo pacman -Syu && yay -Sua".to_string()]
        );
    }

    #[test]
    fn action_verbs() {
        assert_eq!(Action::Install.verb(), "install");
        assert_eq!(Action::Remove.verb(), "remove");
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
