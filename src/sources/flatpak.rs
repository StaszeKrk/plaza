use crate::model::{Action, CommandLine, PackageDetail, PackageHit, SourceId, SourceMeta};
use crate::sources::Source;
use async_trait::async_trait;
use tokio::process::Command;

pub struct FlatpakSource;

impl FlatpakSource {
    pub fn new() -> Self {
        FlatpakSource
    }
}

#[async_trait]
impl Source for FlatpakSource {
    fn id(&self) -> SourceId {
        SourceId::Flatpak
    }

    fn display_name(&self) -> &'static str {
        "flatpak"
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<PackageHit>> {
        // `flatpak search` exits non-zero with "No matches found"; that is not an
        // error. LC_ALL=C keeps the columns unlocalized.
        let output = Command::new("flatpak")
            .env("LC_ALL", "C")
            .arg("search")
            .arg(query)
            .output()
            .await?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(parse_search_output(&text))
    }

    fn action_command(&self, _action: Action, pkg: &str) -> CommandLine {
        // Not the install path (that goes through Provider::install_command); a
        // minimal default kept for the trait.
        CommandLine {
            program: "flatpak".into(),
            args: vec!["install".into(), "--user".into(), "flathub".into(), pkg.into()],
        }
    }
}

/// Parse `flatpak search <term>` output. Columns are tab-separated:
/// Name, Description, Application ID, Version, Branch, Remotes. The Version cell
/// may be empty; the Remotes cell may list several remotes (space-separated),
/// the first is used. Lines without an application-ID column are skipped.
pub fn parse_search_output(out: &str) -> Vec<PackageHit> {
    out.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 6 {
                return None;
            }
            let app_id = cols[2].trim();
            if app_id.is_empty() {
                return None;
            }
            let remote = cols[5].split_whitespace().next().map(str::to_string);
            Some(PackageHit {
                name: cols[0].trim().to_string(),
                version: cols[3].trim().to_string(),
                source_id: SourceId::Flatpak,
                description: cols[1].trim().to_string(),
                meta: SourceMeta {
                    canonical_id: Some(app_id.to_string()),
                    repo: remote,
                    ..Default::default()
                },
            })
        })
        .collect()
}

/// Parse `flatpak remote-info --user --cached <remote> <app-id>` output. It is a
/// block of `Key: value` lines. The cached form carries License and Version but
/// not URL or install size, so those stay `None`.
pub fn parse_remote_info(out: &str) -> PackageDetail {
    let mut d = PackageDetail::default();
    for line in out.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if v.is_empty() {
                continue;
            }
            if k == "License" {
                d.licenses = Some(v.to_string());
            }
        }
    }
    d
}

/// Parse `flatpak list --app --columns=application,version` into (app-id,
/// version) pairs. Tab-separated; lines without an app id are skipped.
pub fn parse_installed(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let id = cols.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let ver = cols.next().unwrap_or("").trim();
            Some((id.to_string(), ver.to_string()))
        })
        .collect()
}

/// Parse `flatpak remote-ls --user --app --updates` into the upgradable app IDs.
/// The app ID is the first non-empty tab/space column; lines without one drop.
pub fn parse_updates(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let id = line.split('\t').next()?.trim();
            (!id.is_empty()).then(|| id.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = "Firefox\tFast, Private & Safe Web Browser\torg.mozilla.firefox\t152.0.3\tstable\tflathub\n\
Nvidia VAAPI driver\tVA-API implementation\torg.freedesktop.Platform.VAAPI.nvidia\t\t25.08\tflathub\n\
junk line with no tabs\n";

    #[test]
    fn parses_search_with_empty_version_and_skips_junk() {
        let hits = parse_search_output(SEARCH);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "Firefox");
        assert_eq!(hits[0].version, "152.0.3");
        assert_eq!(hits[0].source_id, SourceId::Flatpak);
        assert_eq!(hits[0].meta.canonical_id.as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(hits[0].meta.repo.as_deref(), Some("flathub"));
        // empty version tolerated; app id still captured
        assert_eq!(hits[1].version, "");
        assert_eq!(
            hits[1].meta.canonical_id.as_deref(),
            Some("org.freedesktop.Platform.VAAPI.nvidia")
        );
    }

    const INFO: &str = "\nFirefox - Fast, Private & Safe Web Browser\n\n     ID: org.mozilla.firefox\n    Ref: app/org.mozilla.firefox/x86_64/stable\n   Arch: x86_64\n Branch: stable\nVersion: 152.0.3\nLicense: MPL-2.0\n\n Commit: 46ad1fe7\n";

    #[test]
    fn parses_remote_info_license_only() {
        let d = parse_remote_info(INFO);
        assert_eq!(d.licenses.as_deref(), Some("MPL-2.0"));
        assert!(d.url.is_none());
        assert!(d.install_size.is_none());
    }

    #[test]
    fn parses_installed_and_updates() {
        let list = "org.mozilla.firefox\t152.0.3\norg.gimp.GIMP\t3.2.4\n";
        assert_eq!(
            parse_installed(list),
            vec![
                ("org.mozilla.firefox".to_string(), "152.0.3".to_string()),
                ("org.gimp.GIMP".to_string(), "3.2.4".to_string()),
            ]
        );
        let ups = "org.mozilla.firefox\t152.1\tstable\n";
        assert_eq!(parse_updates(ups), vec!["org.mozilla.firefox".to_string()]);
    }

    #[test]
    fn source_identity() {
        let s = FlatpakSource::new();
        assert_eq!(s.id(), SourceId::Flatpak);
        assert_eq!(s.display_name(), "flatpak");
    }
}
