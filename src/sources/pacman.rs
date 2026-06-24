use crate::model::{Action, CommandLine, PackageHit, SourceId, SourceMeta};
use crate::sources::Source;
use async_trait::async_trait;
use tokio::process::Command;

pub struct PacmanSource;

impl PacmanSource {
    pub fn new() -> Self {
        PacmanSource
    }
}

#[async_trait]
impl Source for PacmanSource {
    fn id(&self) -> SourceId {
        SourceId::Pacman
    }

    fn display_name(&self) -> &'static str {
        "repo"
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<PackageHit>> {
        // `pacman -Ss` exits 1 when there are no matches; that is not an error.
        let output = Command::new("pacman").arg("-Ss").arg(query).output().await?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_search_output(&text))
    }

    fn action_command(&self, _action: Action, pkg: &str) -> CommandLine {
        CommandLine {
            program: "sudo".into(),
            args: vec!["pacman".into(), "-S".into(), pkg.into()],
        }
    }
}

/// Parse the output of `pacman -Ss <query>`.
///
/// Format is a header line (`repo/name version [markers]`) followed by one or
/// more indented description lines.
pub fn parse_search_output(output: &str) -> Vec<PackageHit> {
    let mut hits = Vec::new();
    let mut lines = output.lines().peekable();

    while let Some(line) = lines.next() {
        if line.is_empty() || line.starts_with(char::is_whitespace) {
            continue; // not a header line
        }

        let mut parts = line.split_whitespace();
        let Some(repo_name) = parts.next() else {
            continue;
        };
        let Some((repo, name)) = repo_name.split_once('/') else {
            continue;
        };
        let version = parts.next().unwrap_or_default().to_string();
        // `pacman` marks installed packages with "[installed]" or "[installed: x]"
        let _installed_marker = line.contains("[installed");

        let mut description = String::new();
        while let Some(next) = lines.peek() {
            if next.starts_with(char::is_whitespace) && !next.trim().is_empty() {
                if !description.is_empty() {
                    description.push(' ');
                }
                description.push_str(next.trim());
                lines.next();
            } else {
                break;
            }
        }

        hits.push(PackageHit {
            name: name.to_string(),
            version,
            source_id: SourceId::Pacman,
            description,
            meta: SourceMeta {
                repo: Some(repo.to_string()),
                maintained: true,
                ..Default::default()
            },
        });
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
extra/firefox 141.0-1 [installed]
    Standalone web browser from mozilla.org
extra/firefox-developer-edition 142.0b1-1
    Standalone web browser from mozilla.org — Developer Edition
multilib/lib32-mesa 25.0-1
    An open-source implementation of the OpenGL specification (32-bit)
";

    #[test]
    fn parses_name_version_repo_installed() {
        let hits = parse_search_output(FIXTURE);
        assert_eq!(hits.len(), 3);

        assert_eq!(hits[0].name, "firefox");
        assert_eq!(hits[0].version, "141.0-1");
        assert_eq!(hits[0].source_id, SourceId::Pacman);
        assert_eq!(hits[0].meta.repo.as_deref(), Some("extra"));
        assert_eq!(hits[0].description, "Standalone web browser from mozilla.org");

        assert_eq!(hits[1].name, "firefox-developer-edition");
        assert_eq!(hits[1].version, "142.0b1-1");

        assert_eq!(hits[2].name, "lib32-mesa");
        assert_eq!(hits[2].meta.repo.as_deref(), Some("multilib"));
    }

    #[test]
    fn empty_input_yields_no_hits() {
        assert!(parse_search_output("").is_empty());
    }
}
