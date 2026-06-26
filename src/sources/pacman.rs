use crate::model::{Action, CommandLine, PackageDetail, PackageHit, SourceId, SourceMeta};
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

/// Parse `pacman -Si repo/pkg` output into a `PackageDetail`.
///
/// pacman prints `Key${padding} : value` lines; long list values (e.g. `Depends
/// On`) wrap onto whitespace-indented continuation lines. Keys start at column
/// 0, so the field separator is the first ` : ` (space-colon-space) on a
/// non-indented line; that never collides with the `://` inside a URL value.
pub fn parse_si_output(text: &str) -> PackageDetail {
    let mut fields: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_key: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            last_key = None;
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            if let Some(idx) = line.find(" : ") {
                let key = line[..idx].trim().to_string();
                fields.insert(key.clone(), line[idx + 3..].trim().to_string());
                last_key = Some(key);
            } else {
                last_key = None;
            }
        } else if let Some(k) = &last_key {
            // Fold continuation lines with a newline so list values that wrap
            // (Optional Deps lists one entry per line) stay separable.
            let entry = fields.entry(k.clone()).or_default();
            entry.push('\n');
            entry.push_str(line.trim());
        }
    }

    // Scalar field: collapse any wrapped whitespace to single spaces. "None" is
    // pacman's empty-marker for several fields.
    let val = |k: &str| {
        fields
            .get(k)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty() && s != "None")
    };
    let depends = val("Depends On")
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    // Optional deps keep their reasons, so split per line, not per word.
    let optional_depends = fields
        .get("Optional Deps")
        .map(|s| {
            s.split('\n')
                .map(str::trim)
                .filter(|e| !e.is_empty() && *e != "None")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    // Package web page on archlinux.org needs repo + arch + name (all in -Si).
    let repo_url = match (val("Repository"), val("Architecture"), val("Name")) {
        (Some(repo), Some(arch), Some(name)) => {
            Some(format!("https://archlinux.org/packages/{repo}/{arch}/{name}/"))
        }
        _ => None,
    };

    PackageDetail {
        url: val("URL"),
        repo_url,
        licenses: val("Licenses"),
        install_size: val("Installed Size"),
        build_date: val("Build Date"),
        depends,
        optional_depends,
        maintainer: val("Packager"),
        popularity: None,
    }
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

    const SI_FIXTURE: &str = "\
Repository      : extra
Name            : firefox
Version         : 141.0-1
Description     : Standalone web browser from mozilla.org
Architecture    : x86_64
URL             : https://www.mozilla.org/firefox/
Licenses        : MPL-2.0
Depends On      : gtk3  libxss  nss  ttf-font  dbus  libpulse
                  alsa-lib  ffmpeg
Optional Deps   : networkmanager: Easily switch networks [installed]
                  hunspell: Spell checking
Installed Size  : 232.50 MiB
Packager        : Some Maintainer <pkg@example.org>
Build Date      : Wed 20 Jun 2026 10:00:00 AM
Validated By    : Signature
";

    #[test]
    fn parses_si_detail_with_wrapped_depends() {
        let d = parse_si_output(SI_FIXTURE);
        assert_eq!(d.url.as_deref(), Some("https://www.mozilla.org/firefox/"));
        assert_eq!(d.licenses.as_deref(), Some("MPL-2.0"));
        assert_eq!(d.install_size.as_deref(), Some("232.50 MiB"));
        assert_eq!(d.build_date.as_deref(), Some("Wed 20 Jun 2026 10:00:00 AM"));
        assert_eq!(d.maintainer.as_deref(), Some("Some Maintainer <pkg@example.org>"));
        assert_eq!(
            d.repo_url.as_deref(),
            Some("https://archlinux.org/packages/extra/x86_64/firefox/")
        );
        // continuation line folds into Depends On
        assert_eq!(
            d.depends,
            vec!["gtk3", "libxss", "nss", "ttf-font", "dbus", "libpulse", "alsa-lib", "ffmpeg"]
        );
        // optional deps keep their per-entry reason text, one entry per line
        assert_eq!(
            d.optional_depends,
            vec![
                "networkmanager: Easily switch networks [installed]",
                "hunspell: Spell checking",
            ]
        );
    }

    #[test]
    fn si_treats_none_as_empty() {
        let d = parse_si_output("URL             : None\nDepends On      : None\n");
        assert!(d.url.is_none());
        assert!(d.depends.is_empty());
    }
}
