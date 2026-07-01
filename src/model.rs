#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SourceId {
    Pacman,
    Aur,
    Flatpak,
    Apt,
}

impl SourceId {
    pub fn badge(self) -> &'static str {
        match self {
            SourceId::Pacman => "repo",
            SourceId::Aur => "aur",
            SourceId::Flatpak => "flatpak",
            SourceId::Apt => "apt",
        }
    }
}

/// The cross-source merge key for a name: lowercased and trimmed, with a single
/// trailing `-bin`/`-git` removed when `strip_variants` is set. Internal spaces
/// are preserved so multi-word display names cannot false-merge onto short repo
/// names.
pub fn normalize_key(name: &str, strip_variants: bool) -> String {
    let mut s = name.trim().to_lowercase();
    if strip_variants {
        for suf in ["-bin", "-git"] {
            if let Some(base) = s.strip_suffix(suf) {
                if !base.is_empty() {
                    s = base.to_string();
                    break;
                }
            }
        }
    }
    s
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceMeta {
    pub votes: Option<u32>,
    pub maintained: bool,
    pub out_of_date: bool,
    pub repo: Option<String>,
    /// AUR `LastModified` (unix seconds): when the package (PKGBUILD) last changed.
    pub last_modified: Option<i64>,
    /// AppStream/Flatpak component ID when the source provides one (the Flatpak
    /// app ID). Drives cross-source merging; `None` for pacman/AUR until they
    /// gain AppStream readers. Case-sensitive, never normalized.
    pub canonical_id: Option<String>,
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
    /// This provider's own install identity: the real package name for
    /// pacman/AUR (e.g. "gimp-bin" even when the row is keyed "gimp"), or the
    /// Flatpak app ID. Install and detail target this, not the row name.
    pub target: String,
    pub meta: SourceMeta,
}

impl Provider {
    /// Short label for this provider: the concrete pacman repo name
    /// (e.g. "world", "extra-x86-64-v3") or "aur".
    pub fn badge(&self) -> &str {
        match self.source_id {
            SourceId::Pacman => self.meta.repo.as_deref().unwrap_or("repo"),
            SourceId::Aur => "aur",
            SourceId::Flatpak => "flatpak",
            SourceId::Apt => self.meta.repo.as_deref().unwrap_or("apt"),
        }
    }

    /// Cache key for this provider's fetched detail. Mirrors the install target:
    /// `repo/target` for pacman, `aur:target` for the AUR, `flatpak:app-id` for
    /// Flatpak. Keys on `self.target`, so a variant in a grouped row stays
    /// distinct from the row name. Unique per (source, repo).
    pub fn detail_key(&self, _name: &str) -> String {
        match self.source_id {
            SourceId::Pacman => match &self.meta.repo {
                Some(repo) => format!("{repo}/{}", self.target),
                None => self.target.clone(),
            },
            SourceId::Aur => format!("aur:{}", self.target),
            SourceId::Flatpak => format!("flatpak:{}", self.target),
            SourceId::Apt => format!("apt:{}", self.target),
        }
    }

    /// The command that installs THIS provider's target. Pacman targets are
    /// repo-qualified (`repo/pkg`) so a non-default repo can be chosen; the AUR
    /// goes through `aur_bin` (the resolved helper, yay/paru); Flatpak installs
    /// the app ID from its remote (`meta.repo`), `--user` so it needs no polkit.
    pub fn install_command(&self, _name: &str, aur_bin: &str) -> CommandLine {
        match self.source_id {
            SourceId::Pacman => {
                let target = match &self.meta.repo {
                    Some(repo) => format!("{repo}/{}", self.target),
                    None => self.target.clone(),
                };
                CommandLine {
                    program: "sudo".into(),
                    args: vec!["pacman".into(), "-S".into(), target],
                }
            }
            SourceId::Aur => CommandLine {
                program: aur_bin.into(),
                args: vec!["-S".into(), self.target.clone()],
            },
            SourceId::Flatpak => CommandLine {
                program: "flatpak".into(),
                args: vec![
                    "install".into(),
                    "--user".into(),
                    self.meta.repo.clone().unwrap_or_else(|| "flathub".into()),
                    self.target.clone(),
                ],
            },
            // apt installs unqualified (candidate suite only), interactive (no
            // -y) so the PTY surfaces apt's [Y/n] prompt.
            SourceId::Apt => CommandLine {
                program: "sudo".into(),
                args: vec!["apt-get".into(), "install".into(), self.target.clone()],
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

/// Extended per-provider package info, fetched lazily when the detail view is
/// opened (`pacman -Si repo/pkg` for repos, the AUR `info` RPC for the AUR).
/// Every field is optional: each source fills what it has, the UI shows what is
/// present.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageDetail {
    pub url: Option<String>,
    /// Package's page in its repository's web frontend (archlinux.org / AUR).
    pub repo_url: Option<String>,
    pub licenses: Option<String>,
    /// Human string as the source reports it (e.g. "232.50 MiB"). pacman only.
    pub install_size: Option<String>,
    /// pacman build date, as reported. pacman only.
    pub build_date: Option<String>,
    pub depends: Vec<String>,
    /// Optional dependencies, each as the source reports it ("name: reason").
    /// Kept apart from `depends` so the UI never conflates the two.
    pub optional_depends: Vec<String>,
    /// AUR maintainer handle. AUR only.
    pub maintainer: Option<String>,
    /// AUR popularity score. AUR only.
    pub popularity: Option<f64>,
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

/// Which installed packages the Manage list shows, by installation reason.
/// Default is `All`. Cycled in the Manage view (`e`) and as an option default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ReasonFilter {
    /// Every installed package.
    #[default]
    All,
    /// Only explicitly installed packages (`pacman -Qe`).
    Explicit,
    /// Only orphans: dependencies nothing requires (`pacman -Qdt`).
    Orphans,
}

impl ReasonFilter {
    /// Short label for the title and the options row.
    pub fn label(self) -> &'static str {
        match self {
            ReasonFilter::All => "all",
            ReasonFilter::Explicit => "explicit",
            ReasonFilter::Orphans => "orphans",
        }
    }

    /// Cycle order: All -> Explicit -> Orphans -> All.
    pub fn next(self) -> ReasonFilter {
        match self {
            ReasonFilter::All => ReasonFilter::Explicit,
            ReasonFilter::Explicit => ReasonFilter::Orphans,
            ReasonFilter::Orphans => ReasonFilter::All,
        }
    }
}

/// How the Manage installed list is ordered. `Updated` is the last
/// install/upgrade time (neither pacman nor Flatpak stores a robust original
/// install date), so it is labelled "updated" rather than "install date".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Updated,
}

impl SortKey {
    /// The direction a freshly chosen key reads best: names A to Z, but size and
    /// recency largest/newest first.
    pub fn default_dir(self) -> SortDir {
        match self {
            SortKey::Name => SortDir::Asc,
            SortKey::Size => SortDir::Desc,
            SortKey::Updated => SortDir::Desc,
        }
    }

    /// Short label for the filter-box row.
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::Updated => "updated",
        }
    }
}

/// Sort direction for the Manage list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    pub fn flip(self) -> SortDir {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        }
    }
}

/// How repeated same-source badges render for a grouped row: `Count` collapses
/// them to `aur ×3`; `Repeat` shows each badge separately (`aur aur aur`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VariantBadge {
    #[default]
    Count,
    Repeat,
}

impl VariantBadge {
    pub fn next(self) -> VariantBadge {
        match self {
            VariantBadge::Count => VariantBadge::Repeat,
            VariantBadge::Repeat => VariantBadge::Count,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            VariantBadge::Count => "count",
            VariantBadge::Repeat => "repeat",
        }
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    #[test]
    fn variant_badge_default_and_next() {
        assert_eq!(VariantBadge::default(), VariantBadge::Count);
        assert_eq!(VariantBadge::Count.next(), VariantBadge::Repeat);
        assert_eq!(VariantBadge::Repeat.next(), VariantBadge::Count);
        assert_eq!(VariantBadge::Count.label(), "count");
    }

    #[test]
    fn sort_key_default_dir_and_label() {
        assert_eq!(SortKey::default(), SortKey::Name);
        assert_eq!(SortDir::default(), SortDir::Asc);
        assert_eq!(SortKey::Name.default_dir(), SortDir::Asc);
        assert_eq!(SortKey::Size.default_dir(), SortDir::Desc);
        assert_eq!(SortKey::Updated.default_dir(), SortDir::Desc);
        assert_eq!(SortDir::Asc.flip(), SortDir::Desc);
        assert_eq!(SortDir::Desc.flip(), SortDir::Asc);
        assert_eq!(SortKey::Updated.label(), "updated");
    }
}

/// How the substring matching the search/filter text is drawn in the name cell.
/// Default is `Underline`. Cycled in Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum HighlightMode {
    /// No special styling on the match.
    Off,
    /// Recolor the match with the accent color.
    Color,
    /// Underline the match, keeping its color.
    #[default]
    Underline,
    /// Accent color and underline.
    Both,
}

impl HighlightMode {
    /// Short label for the options overlay.
    pub fn label(self) -> &'static str {
        match self {
            HighlightMode::Off => "off",
            HighlightMode::Color => "color",
            HighlightMode::Underline => "underline",
            HighlightMode::Both => "color + underline",
        }
    }

    /// Cycle order for the options overlay.
    pub fn next(self) -> HighlightMode {
        match self {
            HighlightMode::Off => HighlightMode::Color,
            HighlightMode::Color => HighlightMode::Underline,
            HighlightMode::Underline => HighlightMode::Both,
            HighlightMode::Both => HighlightMode::Off,
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

/// Which AUR helper Plaza drives for install/upgrade actions. `Auto` resolves at
/// runtime to whichever of paru/yay is installed (paru preferred when both are).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AurHelper {
    #[default]
    Auto,
    Yay,
    Paru,
}

impl AurHelper {
    /// Short label for the options overlay.
    pub fn label(self) -> &'static str {
        match self {
            AurHelper::Auto => "auto",
            AurHelper::Yay => "yay",
            AurHelper::Paru => "paru",
        }
    }
}

/// Resolve which AUR helper binary to run, given the configured preference and
/// which binaries are present on PATH. Returns the binary plus a `fell_back`
/// flag (true when a forced helper was missing and the other was used instead).
/// `None` when no helper is installed at all.
pub fn resolve_aur_helper(
    setting: AurHelper,
    yay: bool,
    paru: bool,
) -> Option<(&'static str, bool)> {
    match setting {
        // Auto has no configured target to "miss", so it never reports a fallback.
        AurHelper::Auto if paru => Some(("paru", false)),
        AurHelper::Auto if yay => Some(("yay", false)),
        AurHelper::Auto => None,
        AurHelper::Paru if paru => Some(("paru", false)),
        AurHelper::Paru if yay => Some(("yay", true)),
        AurHelper::Paru => None,
        AurHelper::Yay if yay => Some(("yay", false)),
        AurHelper::Yay if paru => Some(("paru", true)),
        AurHelper::Yay => None,
    }
}

/// Cycle the AUR helper setting through `Auto` plus only the installed helpers
/// (yay before paru). With neither installed, only `Auto` is reachable (no-op).
pub fn next_aur_helper(current: AurHelper, yay: bool, paru: bool) -> AurHelper {
    let mut ring = vec![AurHelper::Auto];
    if yay {
        ring.push(AurHelper::Yay);
    }
    if paru {
        ring.push(AurHelper::Paru);
    }
    let idx = ring.iter().position(|h| *h == current).unwrap_or(0);
    ring[(idx + 1) % ring.len()]
}

/// Upgrade a single package. A repo package upgrades unqualified
/// (`sudo pacman -S <name>`), so pacman picks its default repo's latest version;
/// the AUR goes through the resolved helper (`<aur_bin> -S <name>`). This is a
/// partial upgrade for repo packages, which Arch discourages, but it is what the
/// per-package upgrade action requests.
pub fn upgrade_one_command(name: &str, source_id: SourceId, aur_bin: &str) -> CommandLine {
    match source_id {
        SourceId::Pacman => CommandLine {
            program: "sudo".into(),
            args: vec!["pacman".into(), "-S".into(), name.into()],
        },
        SourceId::Aur => CommandLine {
            program: aur_bin.into(),
            args: vec!["-S".into(), name.into()],
        },
        SourceId::Flatpak => CommandLine {
            program: "flatpak".into(),
            args: vec!["update".into(), "--user".into(), "-y".into(), name.into()],
        },
        SourceId::Apt => CommandLine {
            program: "sudo".into(),
            args: vec!["apt-get".into(), "install".into(), "--only-upgrade".into(), name.into()],
        },
    }
}

/// Command that removes a Flatpak app by ID (`flatpak uninstall --user <id>`).
/// Kept apart from `remove_command` because Flatpak removal does not go through
/// pacman and has no `-R` depth family.
pub fn remove_command_flatpak(app_id: &str) -> CommandLine {
    CommandLine {
        program: "flatpak".into(),
        args: vec!["uninstall".into(), "--user".into(), app_id.into()],
    }
}

/// Remove an apt package at the given depth (no `-y`, interactive). apt has no
/// `-R` family, so `RemoveDepth` maps to apt verbs: `Package` -> remove,
/// `WithDeps` -> remove --auto-remove, `Purge` -> purge --auto-remove.
pub fn remove_command_apt(name: &str, depth: RemoveDepth) -> CommandLine {
    let mut args = vec!["apt-get".to_string()];
    match depth {
        RemoveDepth::Package => args.push("remove".into()),
        RemoveDepth::WithDeps => {
            args.push("remove".into());
            args.push("--auto-remove".into());
        }
        RemoveDepth::Purge => {
            args.push("purge".into());
            args.push("--auto-remove".into());
        }
    }
    args.push(name.to_string());
    CommandLine { program: "sudo".into(), args }
}

/// True when a PTY line looks like a prompt that has stopped to wait for input
/// (sudo password, pacman/AUR proceed and selection prompts). Used to flag a
/// background task as needing attention when the user is not on the task pane.
/// Kept tight so ordinary progress and download lines do not trip it.
pub fn looks_like_prompt(line: &str) -> bool {
    let l = line.trim().to_ascii_lowercase();
    if l.is_empty() {
        return false;
    }
    l.contains("[y/n")
        || l.contains("password")
        || (l.starts_with("::") && (l.ends_with(':') || l.ends_with('?')))
        || (l.starts_with("==>") && l.ends_with(':'))
}

/// The full-upgrade command for a single source.
/// - pacman: `sudo pacman -Syu` (sync + upgrade the repos)
/// - aur:    `<aur_bin> -Sua` (upgrade AUR packages only, via the resolved helper)
pub fn source_upgrade_command(source_id: SourceId, aur_bin: &str) -> CommandLine {
    match source_id {
        SourceId::Pacman => CommandLine {
            program: "sudo".into(),
            args: vec!["pacman".into(), "-Syu".into()],
        },
        SourceId::Aur => CommandLine {
            program: aur_bin.into(),
            args: vec!["-Sua".into()],
        },
        SourceId::Flatpak => CommandLine {
            program: "flatpak".into(),
            args: vec!["update".into(), "--user".into(), "-y".into()],
        },
        // update then upgrade as one shell command; interactive (no -y) so the
        // upgrade half surfaces apt's proceed prompt.
        SourceId::Apt => CommandLine {
            program: "sh".into(),
            args: vec!["-c".into(), "sudo apt-get update && sudo apt-get upgrade".into()],
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
    pub flatpak: usize,
    pub apt: usize,
}

impl InstalledStats {
    pub fn total(&self) -> usize {
        self.repo + self.foreign + self.flatpak + self.apt
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UpdatesInfo {
    pub repo: Option<usize>,
    pub aur: Option<usize>,
    pub flatpak: Option<usize>,
    pub apt: Option<usize>,
}

/// The bare package name from a dependency string, dropping version
/// constraints (`glibc>=2.36`) and optional-dep reasons (`foo: needed for X`).
/// Used to look a dependency up in the installed index.
pub fn dep_pkg_name(dep: &str) -> &str {
    let end = dep.find(['<', '>', '=', ':']).unwrap_or(dep.len());
    dep[..end].trim()
}

/// Whole days between `ts` and `now` (unix seconds), clamped at 0.
pub fn days_ago(ts: i64, now: i64) -> i64 {
    (now - ts).max(0) / 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_filter_cycles_all_explicit_orphans() {
        assert_eq!(ReasonFilter::All.next(), ReasonFilter::Explicit);
        assert_eq!(ReasonFilter::Explicit.next(), ReasonFilter::Orphans);
        assert_eq!(ReasonFilter::Orphans.next(), ReasonFilter::All);
        assert_eq!(ReasonFilter::default(), ReasonFilter::All);
    }

    #[test]
    fn looks_like_prompt_matches_arch_prompts_only() {
        // real prompts that block on input
        assert!(looks_like_prompt("[sudo] password for staszek:"));
        assert!(looks_like_prompt(":: Proceed with installation? [Y/n]"));
        assert!(looks_like_prompt(":: Replace foo with bar? [y/N]"));
        assert!(looks_like_prompt(":: Enter a number (default=1):"));
        assert!(looks_like_prompt("==> Packages to exclude:"));
        // ordinary output that must not trip
        assert!(!looks_like_prompt(""));
        assert!(!looks_like_prompt("downloading firefox-123.0-1 (90.2 MiB)"));
        assert!(!looks_like_prompt("(5/5) checking keys in keyring"));
        assert!(!looks_like_prompt("resolving dependencies..."));
    }

    #[test]
    fn source_badges() {
        assert_eq!(SourceId::Pacman.badge(), "repo");
        assert_eq!(SourceId::Aur.badge(), "aur");
        assert_eq!(SourceId::Flatpak.badge(), "flatpak");
        assert_eq!(SourceId::Apt.badge(), "apt");
    }

    #[test]
    fn apt_provider_commands() {
        let p = Provider {
            source_id: SourceId::Apt,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "firefox-esr".into(),
            meta: SourceMeta { repo: Some("bookworm/main".into()), ..Default::default() },
        };
        let c = p.install_command("firefox-esr", "");
        assert_eq!(c.program, "sudo");
        assert_eq!(c.args, vec!["apt-get", "install", "firefox-esr"]);
        assert_eq!(p.detail_key("firefox-esr"), "apt:firefox-esr");
        // the suite shows on the badge, like a pacman repo
        assert_eq!(p.badge(), "bookworm/main");
    }

    #[test]
    fn apt_upgrade_and_source_upgrade() {
        let one = upgrade_one_command("vim", SourceId::Apt, "");
        assert_eq!(one.program, "sudo");
        assert_eq!(one.args, vec!["apt-get", "install", "--only-upgrade", "vim"]);
        let all = source_upgrade_command(SourceId::Apt, "");
        assert_eq!(all.program, "sh");
        assert_eq!(
            all.args,
            vec!["-c".to_string(), "sudo apt-get update && sudo apt-get upgrade".to_string()]
        );
    }

    #[test]
    fn apt_remove_maps_depth() {
        assert_eq!(
            remove_command_apt("vim", RemoveDepth::Package).args,
            vec!["apt-get", "remove", "vim"]
        );
        assert_eq!(
            remove_command_apt("vim", RemoveDepth::WithDeps).args,
            vec!["apt-get", "remove", "--auto-remove", "vim"]
        );
        assert_eq!(
            remove_command_apt("vim", RemoveDepth::Purge).args,
            vec!["apt-get", "purge", "--auto-remove", "vim"]
        );
        assert_eq!(remove_command_apt("vim", RemoveDepth::Package).program, "sudo");
    }

    #[test]
    fn normalize_key_lowercases_trims_and_strips_variants() {
        assert_eq!(normalize_key("Firefox", false), "firefox");
        assert_eq!(normalize_key("  GIMP  ", false), "gimp");
        assert_eq!(normalize_key("gimp-bin", true), "gimp");
        assert_eq!(normalize_key("gimp-git", true), "gimp");
        assert_eq!(normalize_key("gimp-bin", false), "gimp-bin");
        // internal spaces kept: multi-word names cannot collapse onto short names
        assert_eq!(normalize_key("Visual Studio Code", true), "visual studio code");
        // only one suffix, only at the end, never the whole name
        assert_eq!(normalize_key("python-foo-git", true), "python-foo");
        assert_eq!(normalize_key("-git", true), "-git");
    }

    #[test]
    fn provider_installs_its_own_target_not_row_name() {
        // A row keyed "gimp" holds an AUR provider whose real package is "gimp-bin".
        let p = Provider {
            source_id: SourceId::Aur,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "gimp-bin".into(),
            meta: SourceMeta::default(),
        };
        assert_eq!(p.install_command("gimp", "yay").args, vec!["-S", "gimp-bin"]);
        assert_eq!(p.detail_key("gimp"), "aur:gimp-bin");
    }

    #[test]
    fn flatpak_install_remove_and_upgrade_commands() {
        let p = Provider {
            source_id: SourceId::Flatpak,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "org.mozilla.firefox".into(),
            meta: SourceMeta { repo: Some("flathub".into()), ..Default::default() },
        };
        let c = p.install_command("Firefox", "");
        assert_eq!(c.program, "flatpak");
        assert_eq!(c.args, vec!["install", "--user", "flathub", "org.mozilla.firefox"]);
        assert_eq!(p.detail_key("Firefox"), "flatpak:org.mozilla.firefox");
        assert_eq!(
            remove_command_flatpak("org.mozilla.firefox").args,
            vec!["uninstall", "--user", "org.mozilla.firefox"]
        );
        assert_eq!(
            source_upgrade_command(SourceId::Flatpak, "").args,
            vec!["update", "--user", "-y"]
        );
        assert_eq!(
            upgrade_one_command("org.mozilla.firefox", SourceId::Flatpak, "").args,
            vec!["update", "--user", "-y", "org.mozilla.firefox"]
        );
    }

    #[test]
    fn flatpak_install_defaults_remote_to_flathub() {
        let p = Provider {
            source_id: SourceId::Flatpak,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "org.gimp.GIMP".into(),
            meta: SourceMeta::default(), // no repo recorded
        };
        assert_eq!(
            p.install_command("GIMP", "").args,
            vec!["install", "--user", "flathub", "org.gimp.GIMP"]
        );
    }

    #[test]
    fn dep_pkg_name_strips_constraints_and_reasons() {
        assert_eq!(dep_pkg_name("gtk3"), "gtk3");
        assert_eq!(dep_pkg_name("glibc>=2.36"), "glibc");
        assert_eq!(dep_pkg_name("foo=1.0"), "foo");
        assert_eq!(dep_pkg_name("networkmanager: easily switch networks"), "networkmanager");
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
        let s = InstalledStats { repo: 1208, foreign: 77, flatpak: 12, apt: 3 };
        assert_eq!(s.total(), 1300);
    }

    #[test]
    fn detail_key_mirrors_install_target() {
        let pac = Provider {
            source_id: SourceId::Pacman,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "firefox".into(),
            meta: SourceMeta { repo: Some("extra".into()), ..Default::default() },
        };
        assert_eq!(pac.detail_key("firefox"), "extra/firefox");
        let aur = Provider {
            source_id: SourceId::Aur,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "firefox-git".into(),
            meta: SourceMeta::default(),
        };
        assert_eq!(aur.detail_key("firefox-git"), "aur:firefox-git");
    }

    #[test]
    fn install_command_qualifies_pacman_repo() {
        let p = Provider {
            source_id: SourceId::Pacman,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "neovim".into(),
            meta: SourceMeta {
                repo: Some("extra-x86-64-v3".into()),
                ..Default::default()
            },
        };
        let cmd = p.install_command("neovim", "yay");
        assert_eq!(cmd.program, "sudo");
        assert_eq!(cmd.args, vec!["pacman", "-S", "extra-x86-64-v3/neovim"]);

        let aur = Provider {
            source_id: SourceId::Aur,
            version: "1".into(),
            installed: false,
            installed_version: None,
            target: "tty-clock".into(),
            meta: SourceMeta::default(),
        };
        assert_eq!(aur.install_command("tty-clock", "yay").program, "yay");
        let cmd = aur.install_command("tty-clock", "paru");
        assert_eq!(cmd.program, "paru");
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
    fn upgrade_one_command_per_source() {
        // A single repo package upgrades unqualified (its default repo's latest),
        // avoiding a bogus "repo/" qualifier for packages with an unknown origin.
        let pac = upgrade_one_command("firefox", SourceId::Pacman, "yay");
        assert_eq!(pac.program, "sudo");
        assert_eq!(pac.args, vec!["pacman", "-S", "firefox"]);
        // The AUR goes through the resolved helper.
        let aur = upgrade_one_command("tty-clock", SourceId::Aur, "paru");
        assert_eq!(aur.program, "paru");
        assert_eq!(aur.args, vec!["-S", "tty-clock"]);
    }

    #[test]
    fn source_upgrade_commands() {
        assert_eq!(
            source_upgrade_command(SourceId::Pacman, "yay").args,
            vec!["pacman", "-Syu"]
        );
        let aur = source_upgrade_command(SourceId::Aur, "yay");
        assert_eq!(aur.program, "yay");
        assert_eq!(aur.args, vec!["-Sua"]);
        // The AUR upgrade honors the resolved helper binary.
        assert_eq!(source_upgrade_command(SourceId::Aur, "paru").program, "paru");
    }

    #[test]
    fn chain_commands_single_and_many() {
        let pac = source_upgrade_command(SourceId::Pacman, "yay");
        let aur = source_upgrade_command(SourceId::Aur, "yay");
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
    fn resolve_aur_helper_auto_prefers_paru() {
        assert_eq!(resolve_aur_helper(AurHelper::Auto, true, true), Some(("paru", false)));
        assert_eq!(resolve_aur_helper(AurHelper::Auto, true, false), Some(("yay", false)));
        assert_eq!(resolve_aur_helper(AurHelper::Auto, false, true), Some(("paru", false)));
        assert_eq!(resolve_aur_helper(AurHelper::Auto, false, false), None);
    }

    #[test]
    fn resolve_aur_helper_forced_falls_back_with_flag() {
        // Forced helper present: used, no fallback.
        assert_eq!(resolve_aur_helper(AurHelper::Yay, true, true), Some(("yay", false)));
        assert_eq!(resolve_aur_helper(AurHelper::Paru, true, true), Some(("paru", false)));
        // Forced helper missing but the other present: fall back, flag set.
        assert_eq!(resolve_aur_helper(AurHelper::Paru, true, false), Some(("yay", true)));
        assert_eq!(resolve_aur_helper(AurHelper::Yay, false, true), Some(("paru", true)));
        // Nothing installed: None regardless of the forced choice.
        assert_eq!(resolve_aur_helper(AurHelper::Yay, false, false), None);
        assert_eq!(resolve_aur_helper(AurHelper::Paru, false, false), None);
    }

    #[test]
    fn next_aur_helper_cycles_only_installed() {
        // Both installed: Auto -> yay -> paru -> Auto.
        assert_eq!(next_aur_helper(AurHelper::Auto, true, true), AurHelper::Yay);
        assert_eq!(next_aur_helper(AurHelper::Yay, true, true), AurHelper::Paru);
        assert_eq!(next_aur_helper(AurHelper::Paru, true, true), AurHelper::Auto);
        // Only paru: Auto <-> paru.
        assert_eq!(next_aur_helper(AurHelper::Auto, false, true), AurHelper::Paru);
        assert_eq!(next_aur_helper(AurHelper::Paru, false, true), AurHelper::Auto);
        // Neither: only Auto is reachable (no-op).
        assert_eq!(next_aur_helper(AurHelper::Auto, false, false), AurHelper::Auto);
        // Stale setting (paru selected but not installed) advances from Auto's slot.
        assert_eq!(next_aur_helper(AurHelper::Paru, true, false), AurHelper::Yay);
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
                    target: "firefox".into(),
                    meta: SourceMeta::default(),
                },
                Provider {
                    source_id: SourceId::Aur,
                    version: "1".into(),
                    installed: false,
                    installed_version: None,
                    target: "firefox".into(),
                    meta: SourceMeta::default(),
                },
            ],
        };
        assert!(row.any_installed());
        assert!(row.has_source(SourceId::Aur));
    }
}
