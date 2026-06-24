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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_badges() {
        assert_eq!(SourceId::Pacman.badge(), "repo");
        assert_eq!(SourceId::Aur.badge(), "aur");
    }

    #[test]
    fn installed_stats_total() {
        let s = InstalledStats { repo: 1208, foreign: 77 };
        assert_eq!(s.total(), 1285);
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
