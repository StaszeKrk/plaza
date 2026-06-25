pub mod aur;
pub mod installed;
pub mod pacman;
pub mod updates;

use crate::model::{Action, CommandLine, PackageHit, SourceId};
use async_trait::async_trait;

#[async_trait]
pub trait Source: Send + Sync {
    fn id(&self) -> SourceId;
    fn display_name(&self) -> &'static str;
    async fn search(&self, query: &str) -> anyhow::Result<Vec<PackageHit>>;
    fn action_command(&self, action: Action, pkg: &str) -> CommandLine;
}

/// Return true if `bin` is an executable found on `$PATH`.
pub fn which(bin: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    path.split(':')
        .map(|dir| std::path::Path::new(dir).join(bin))
        .any(|p| p.is_file())
}

/// Build the set of enabled sources based on which binaries are installed.
pub fn detect_sources() -> Vec<Box<dyn Source>> {
    let mut sources: Vec<Box<dyn Source>> = Vec::new();
    if which("pacman") {
        sources.push(Box::new(pacman::PacmanSource::new()));
        // AUR search is a plain RPC call and needs no local helper, so it shows
        // whenever we are on an Arch-like system (pacman present). Install and
        // upgrade are gated on a helper binary (yay/paru) separately.
        sources.push(Box::new(aur::AurSource::new()));
    }
    sources
}

/// Detect which AUR helper binaries are present on PATH, as `(yay, paru)`.
pub fn detect_aur_helpers() -> (bool, bool) {
    (which("yay"), which("paru"))
}

#[cfg(test)]
pub struct MockSource {
    pub id: SourceId,
    pub hits: Vec<PackageHit>,
}

#[cfg(test)]
#[async_trait]
impl Source for MockSource {
    fn id(&self) -> SourceId {
        self.id
    }
    fn display_name(&self) -> &'static str {
        "mock"
    }
    async fn search(&self, _query: &str) -> anyhow::Result<Vec<PackageHit>> {
        Ok(self.hits.clone())
    }
    fn action_command(&self, _action: Action, pkg: &str) -> CommandLine {
        CommandLine {
            program: "true".into(),
            args: vec![pkg.into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_a_ubiquitous_binary() {
        assert!(which("sh"));
        assert!(!which("definitely-not-a-real-binary-xyz"));
    }

    #[tokio::test]
    async fn mock_source_returns_its_hits() {
        let src = MockSource {
            id: SourceId::Aur,
            hits: vec![PackageHit {
                name: "x".into(),
                version: "1".into(),
                source_id: SourceId::Aur,
                description: String::new(),
                meta: Default::default(),
            }],
        };
        let hits = src.search("anything").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(src.id(), SourceId::Aur);
    }
}
