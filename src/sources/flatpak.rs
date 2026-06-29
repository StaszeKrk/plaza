use crate::model::{Action, CommandLine, PackageDetail, PackageHit, SourceId, SourceMeta};
use crate::sources::installed::{InstalledPkg, PkgDetail};
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

/// Parse `flatpak remote-info --user <remote> <app-id>` output: a block of
/// `Key: value` lines (the networked form, which carries Installed Size and Date
/// that the `--cached` form omits). Flatpak has no homepage or dependency list
/// here, so those stay empty.
pub fn parse_remote_info(out: &str) -> PackageDetail {
    let mut d = PackageDetail::default();
    for line in out.lines() {
        // split_once on the first colon only, so a time value's colons stay in v.
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if v.is_empty() {
                continue;
            }
            match k {
                "License" => d.licenses = Some(v.to_string()),
                "Installed Size" => d.install_size = Some(v.to_string()),
                "Date" => d.build_date = Some(v.to_string()),
                // The app's Flathub page, the closest thing to a homepage link.
                "ID" => d.repo_url = Some(format!("https://flathub.org/apps/{v}")),
                _ => {}
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

/// Build Manage-view rows from
/// `flatpak list --app --columns=application,name,version` (tab-separated). The
/// app ID is the row name (remove/upgrade target it directly); the human name is
/// the display label; origin is "flatpak" (drives the filter and remove
/// routing). Flatpak apps are explicitly installed and never orphans.
pub fn parse_installed_pkgs(out: &str) -> Vec<InstalledPkg> {
    out.lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let id = cols.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let display = cols.next().unwrap_or("").trim();
            let version = cols.next().unwrap_or("").trim();
            Some(InstalledPkg {
                name: id.to_string(),
                display: if display.is_empty() { id.to_string() } else { display.to_string() },
                version: version.to_string(),
                origin: "flatpak".to_string(),
                explicit: true,
                orphan: false,
            })
        })
        .collect()
}

/// Parse `flatpak info <app-id>` (installed app) into the Manage detail struct.
/// Same `key: value` block as remote-info, plus a "Name - Description" title
/// line. Flatpak has no dependency or required-by lists here, so those stay
/// empty; apps are always explicitly installed.
pub fn parse_info(out: &str) -> PkgDetail {
    let mut d = PkgDetail { explicit: true, ..Default::default() };
    if let Some(title) = out.lines().find(|l| !l.trim().is_empty()) {
        match title.split_once(" - ") {
            Some((n, desc)) => {
                d.name = n.trim().to_string();
                d.description = desc.trim().to_string();
            }
            None => d.name = title.trim().to_string(),
        }
    }
    for line in out.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            if v.is_empty() {
                continue;
            }
            match k {
                "Version" => d.version = v.to_string(),
                "Installed Size" => d.size = v.to_string(),
                "Date" => d.build_date = v.to_string(),
                "ID" => d.url = format!("https://flathub.org/apps/{v}"),
                _ => {}
            }
        }
    }
    d
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

    const INFO: &str = "\nFirefox - Fast, Private & Safe Web Browser\n\n            ID: org.mozilla.firefox\n           Ref: app/org.mozilla.firefox/x86_64/stable\n          Arch: x86_64\n        Branch: stable\n       Version: 152.0.3\n       License: MPL-2.0\n    Collection: org.flathub.Stable\n Download Size: 122.0 MB\nInstalled Size: 325.3 MB\n       Runtime: org.freedesktop.Platform/x86_64/25.08\n\n        Commit: 46ad1fe7\n          Date: 2026-06-25 13:29:13 +0000\n";

    #[test]
    fn parses_remote_info_size_license_and_date() {
        let d = parse_remote_info(INFO);
        assert_eq!(d.licenses.as_deref(), Some("MPL-2.0"));
        assert_eq!(d.install_size.as_deref(), Some("325.3 MB"));
        // first-colon split keeps the time in the value
        assert_eq!(d.build_date.as_deref(), Some("2026-06-25 13:29:13 +0000"));
        assert_eq!(d.repo_url.as_deref(), Some("https://flathub.org/apps/org.mozilla.firefox"));
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
    fn builds_installed_pkgs_with_flatpak_origin_and_display() {
        // application \t name \t version
        let list = "org.mozilla.firefox\tFirefox\t152.0.3\ncom.example.NoName\t\t1.0\n";
        let pkgs = parse_installed_pkgs(list);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].name, "org.mozilla.firefox");
        assert_eq!(pkgs[0].display, "Firefox");
        assert_eq!(pkgs[0].version, "152.0.3");
        assert_eq!(pkgs[0].origin, "flatpak");
        assert!(pkgs[0].explicit && !pkgs[0].orphan);
        // missing human name falls back to the app ID
        assert_eq!(pkgs[1].display, "com.example.NoName");
    }

    const FLATPAK_INFO: &str = "\nFreedesktop Platform - Runtime platform for applications\n\n            ID: org.freedesktop.Platform\n          Arch: x86_64\n        Branch: 25.08\n       Version: freedesktop-sdk-25.08.13\n       License: MIT\n  Installation: user\nInstalled Size: 657.0 MB\n\n        Commit: 3f0cb4a\n          Date: 2026-06-20 09:14:53 +0000\n";

    #[test]
    fn parses_flatpak_info_into_detail() {
        let d = parse_info(FLATPAK_INFO);
        assert_eq!(d.name, "Freedesktop Platform");
        assert_eq!(d.description, "Runtime platform for applications");
        assert_eq!(d.version, "freedesktop-sdk-25.08.13");
        assert_eq!(d.size, "657.0 MB");
        assert_eq!(d.build_date, "2026-06-20 09:14:53 +0000");
        assert_eq!(d.url, "https://flathub.org/apps/org.freedesktop.Platform");
        assert!(d.explicit);
    }

    #[test]
    fn source_identity() {
        let s = FlatpakSource::new();
        assert_eq!(s.id(), SourceId::Flatpak);
        assert_eq!(s.display_name(), "flatpak");
    }
}
