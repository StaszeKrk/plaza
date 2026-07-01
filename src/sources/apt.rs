use crate::model::{Action, CommandLine, PackageDetail, PackageHit, SourceId, SourceMeta};
use crate::sources::Source;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::process::Command;

pub struct AptSource;

impl AptSource {
    pub fn new() -> Self {
        AptSource
    }
}

#[async_trait]
impl Source for AptSource {
    fn id(&self) -> SourceId {
        SourceId::Apt
    }

    fn display_name(&self) -> &'static str {
        "apt"
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<PackageHit>> {
        // `apt-cache search` matches name + description but gives no version or
        // suite; a batched `apt-cache policy` over the matched names fills those.
        let search_out = Command::new("apt-cache").arg("search").arg(query).output().await?;
        let search = String::from_utf8_lossy(&search_out.stdout).into_owned();
        let names: Vec<String> =
            parse_search_output(&search).into_iter().map(|(n, _)| n).collect();
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let policy_out = Command::new("apt-cache").arg("policy").args(&names).output().await?;
        let policy = String::from_utf8_lossy(&policy_out.stdout);
        Ok(build_hits(&search, &policy))
    }

    fn action_command(&self, _action: Action, pkg: &str) -> CommandLine {
        CommandLine {
            program: "sudo".into(),
            args: vec!["apt-get".into(), "install".into(), pkg.into()],
        }
    }
}

/// Parse `apt-cache search <q>` lines: `name - description`. The description can
/// itself contain " - ", so split only on the first occurrence.
pub fn parse_search_output(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (name, desc) = line.split_once(" - ")?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), desc.trim().to_string()))
        })
        .collect()
}

/// Parse `apt-cache policy <names...>`. Each package block starts with `name:`
/// at column 0, then indented `Candidate:` and a version table. The suite is
/// taken from the first archive origin line (`<pin> <url> <suite>/<component>
/// <arch> Packages`), which is the Candidate's source in the common
/// single-origin case. A `(none)` candidate is not installable and is dropped.
pub fn parse_policy(output: &str) -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();
    let mut name: Option<String> = None;
    let mut candidate: Option<String> = None;
    let mut suite: Option<String> = None;

    fn flush(
        name: &mut Option<String>,
        candidate: &mut Option<String>,
        suite: &mut Option<String>,
        map: &mut HashMap<String, (String, String)>,
    ) {
        if let (Some(n), Some(c)) = (name.take(), candidate.take()) {
            if c != "(none)" {
                map.insert(n, (c, suite.clone().unwrap_or_default()));
            }
        }
        *suite = None;
    }

    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) && line.trim_end().ends_with(':') {
            flush(&mut name, &mut candidate, &mut suite, &mut map);
            name = Some(line.trim_end().trim_end_matches(':').to_string());
        } else if let Some(rest) = line.trim().strip_prefix("Candidate:") {
            candidate = Some(rest.trim().to_string());
        } else if suite.is_none() && line.contains("://") {
            // origin line: pin url suite/component arch Packages
            if let Some(tok) = line.split_whitespace().nth(2) {
                suite = Some(tok.to_string());
            }
        }
    }
    flush(&mut name, &mut candidate, &mut suite, &mut map);
    map
}

/// Join the search match set with policy versions/suites into apt PackageHits.
/// A searched name with no installable policy candidate is dropped.
pub fn build_hits(search: &str, policy: &str) -> Vec<PackageHit> {
    let pol = parse_policy(policy);
    parse_search_output(search)
        .into_iter()
        .filter_map(|(name, description)| {
            let (version, suite) = pol.get(&name)?.clone();
            Some(PackageHit {
                name,
                version,
                source_id: SourceId::Apt,
                description,
                meta: SourceMeta { repo: Some(suite), maintained: true, ..Default::default() },
            })
        })
        .collect()
}

/// Format an apt `Installed-Size` (in KiB) as a human string, matching pacman's
/// "MiB" style. Under 1 MiB stays in KiB.
pub fn format_kib(kib: u64) -> String {
    if kib >= 1024 {
        format!("{:.2} MiB", kib as f64 / 1024.0)
    } else {
        format!("{kib} KiB")
    }
}

/// Parse the FIRST paragraph of `apt-cache show <pkg>` (RFC822 `Key: value`,
/// continuation lines indented). Only the top (Candidate) paragraph is used.
/// apt-cache carries no license, build date, or popularity.
pub fn parse_show_output(text: &str) -> PackageDetail {
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            break; // end of the first paragraph
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(k) = &last {
                let e = fields.entry(k.clone()).or_default();
                e.push(' ');
                e.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_string();
            fields.insert(key.clone(), v.trim().to_string());
            last = Some(key);
        }
    }
    let val = |k: &str| fields.get(k).filter(|s| !s.is_empty()).cloned();
    // Depends: comma-separated, each entry `name (>= ver)` or `a | b`; keep the
    // bare package token (before any version constraint).
    let depends = val("Depends")
        .map(|s| {
            s.split(',')
                .map(|d| d.split('(').next().unwrap_or("").trim().to_string())
                .filter(|d| !d.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let install_size = fields
        .get("Installed-Size")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(format_kib);
    PackageDetail {
        url: val("Homepage"),
        repo_url: None,
        licenses: None,
        install_size,
        build_date: None,
        depends,
        optional_depends: Vec::new(),
        maintainer: val("Maintainer"),
        popularity: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_name_and_description() {
        let out = "firefox-esr - Mozilla Firefox web browser - ESR\nfonts-firacode - monospaced font with ligatures\n";
        let hits = parse_search_output(out);
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0],
            ("firefox-esr".to_string(), "Mozilla Firefox web browser - ESR".to_string())
        );
        assert_eq!(hits[1].0, "fonts-firacode");
        assert!(parse_search_output("").is_empty());
    }

    #[test]
    fn parses_policy_candidate_and_suite() {
        let out = "\
firefox-esr:
  Installed: (none)
  Candidate: 115.0esr-1
  Version table:
     115.0esr-1 500
        500 http://deb.debian.org/debian bookworm/main amd64 Packages
vim:
  Installed: 2:9.0-1
  Candidate: 2:9.0-2
  Version table:
 *** 2:9.0-1 500
        500 http://deb.debian.org/debian bookworm-backports/main amd64 Packages
";
        let m = parse_policy(out);
        assert_eq!(
            m.get("firefox-esr").unwrap(),
            &("115.0esr-1".to_string(), "bookworm/main".to_string())
        );
        assert_eq!(
            m.get("vim").unwrap(),
            &("2:9.0-2".to_string(), "bookworm-backports/main".to_string())
        );
    }

    #[test]
    fn policy_skips_uninstallable_candidate() {
        let out = "ghost:\n  Installed: (none)\n  Candidate: (none)\n  Version table:\n";
        assert!(parse_policy(out).get("ghost").is_none());
    }

    #[test]
    fn build_hits_joins_search_and_policy() {
        let search = "firefox-esr - Mozilla Firefox ESR\norphan - not in policy\n";
        let policy = "firefox-esr:\n  Candidate: 115.0esr-1\n  Version table:\n     115.0esr-1 500\n        500 http://deb.debian.org/debian bookworm/main amd64 Packages\n";
        let hits = build_hits(search, policy);
        assert_eq!(hits.len(), 1); // orphan without a policy candidate is dropped
        let h = &hits[0];
        assert_eq!(h.name, "firefox-esr");
        assert_eq!(h.version, "115.0esr-1");
        assert_eq!(h.source_id, SourceId::Apt);
        assert_eq!(h.description, "Mozilla Firefox ESR");
        assert_eq!(h.meta.repo.as_deref(), Some("bookworm/main"));
        assert!(h.meta.maintained);
    }

    #[test]
    fn formats_installed_size_kib() {
        assert_eq!(format_kib(250_000), "244.14 MiB");
        assert_eq!(format_kib(512), "512 KiB");
    }

    #[test]
    fn parses_show_first_paragraph() {
        let out = "\
Package: firefox-esr
Version: 115.0esr-1
Installed-Size: 250000
Maintainer: Maintainers <team@tracker.debian.org>
Depends: libc6 (>= 2.34), libgtk-3-0 (>= 3.9.10), libx11-6
Homepage: https://www.mozilla.org/firefox/
Description-en: Mozilla Firefox web browser
 Firefox delivers safe, easy web browsing.

Package: firefox-esr
Version: 114.0esr-1
";
        let d = parse_show_output(out);
        assert_eq!(d.url.as_deref(), Some("https://www.mozilla.org/firefox/"));
        assert_eq!(d.install_size.as_deref(), Some("244.14 MiB"));
        assert_eq!(d.maintainer.as_deref(), Some("Maintainers <team@tracker.debian.org>"));
        assert_eq!(d.depends, vec!["libc6", "libgtk-3-0", "libx11-6"]);
        assert!(d.licenses.is_none());
        assert!(d.build_date.is_none());
    }
}
