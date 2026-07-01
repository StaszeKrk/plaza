use std::collections::HashMap;

/// Map of installed package name -> version, built from `pacman -Q`.
#[derive(Debug, Default, Clone)]
pub struct InstalledIndex {
    versions: HashMap<String, String>,
}

impl InstalledIndex {
    /// Parse `pacman -Q` output (`name version` per line).
    pub fn from_query_output(output: &str) -> Self {
        let mut versions = HashMap::new();
        for line in output.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
                versions.insert(name.to_string(), ver.to_string());
            }
        }
        InstalledIndex { versions }
    }

    /// Add or overwrite one entry (used to fold Flatpak app IDs into the index so
    /// search results show installed state for Flatpak too).
    pub fn insert(&mut self, name: String, version: String) {
        self.versions.insert(name, version);
    }

    pub fn is_installed(&self, name: &str) -> bool {
        self.versions.contains_key(name)
    }

    pub fn version(&self, name: &str) -> Option<&str> {
        self.versions.get(name).map(String::as_str)
    }
}

/// Per-package detail parsed from `pacman -Qi`, shown in the Manage detail pane.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PkgDetail {
    pub name: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub explicit: bool,
    pub install_date: String,
    pub build_date: String,
    pub size: String,
    pub required_by: Vec<String>,
    pub optional_for: Vec<String>,
    pub depends: Vec<String>,
}

/// Parse `pacman -Qi` output. Keys start at column 0 (`Name : value`); indented
/// lines continue the previous field. `None` lists become empty vecs.
pub fn parse_pkg_detail(qi: &str) -> PkgDetail {
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut current = String::new();
    for line in qi.lines() {
        if line.starts_with(char::is_whitespace) && !current.is_empty() {
            // continuation of the previous field's value
            if let Some(v) = fields.get_mut(&current) {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            current = k.trim().to_string();
            fields.insert(current.clone(), v.trim().to_string());
        }
    }
    let get = |k: &str| fields.get(k).cloned().unwrap_or_default();
    let list = |k: &str| -> Vec<String> {
        let v = get(k);
        if v == "None" || v.is_empty() {
            Vec::new()
        } else {
            v.split_whitespace().map(|s| s.to_string()).collect()
        }
    };
    PkgDetail {
        name: get("Name"),
        version: get("Version"),
        description: get("Description"),
        url: get("URL"),
        explicit: get("Install Reason").starts_with("Explicitly"),
        install_date: get("Install Date"),
        build_date: get("Build Date"),
        size: get("Installed Size"),
        required_by: list("Required By"),
        optional_for: list("Optional For"),
        depends: list("Depends On"),
    }
}

/// Parse `dpkg -s <pkg>` (RFC822 status) into the Manage detail pane. Only header
/// lines are read; indented long-description continuations are ignored, so
/// `Description` yields the short synopsis. Reverse deps, build date, and install
/// date are not in dpkg status, so those stay empty (the caller fills `explicit`
/// and `install_date` from the installed list).
pub fn parse_dpkg_status(text: &str) -> PkgDetail {
    let mut fields: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        if line.starts_with(char::is_whitespace) {
            continue; // long-description continuation
        }
        if let Some((k, v)) = line.split_once(':') {
            fields.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    let get = |k: &str| fields.get(k).cloned().unwrap_or_default();
    let depends = {
        let d = get("Depends");
        if d.is_empty() {
            Vec::new()
        } else {
            d.split(',')
                .map(|e| e.split('(').next().unwrap_or("").trim().to_string())
                .filter(|e| !e.is_empty())
                .collect()
        }
    };
    let size = fields
        .get("Installed-Size")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(crate::sources::apt::format_kib)
        .unwrap_or_default();
    PkgDetail {
        name: get("Package"),
        version: get("Version"),
        description: get("Description"),
        url: get("Homepage"),
        explicit: false, // set by the caller from the installed list
        install_date: String::new(),
        build_date: String::new(),
        size,
        required_by: Vec::new(),
        optional_for: Vec::new(),
        depends,
    }
}

/// Format a unix timestamp (secs) as "YYYY-MM-DD HH:MM" in UTC. Plaza has no date
/// library; this uses the standard days-to-civil algorithm (Howard Hinnant).
pub fn format_epoch_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let sod = secs.rem_euclid(86_400);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, m, d, sod / 3600, (sod % 3600) / 60)
}

/// Parse `apt-cache rdepends --installed <pkg>` into the reverse-dependency names
/// (packages that depend on it), for the Manage detail "required by" field.
/// Skips the echoed package name (unindented) and the "Reverse Depends:" header;
/// strips `|` alternation markers; dedups.
pub fn parse_rdepends(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        if !line.starts_with(char::is_whitespace) {
            continue; // the echoed query package name
        }
        let t = line.trim().trim_start_matches('|').trim();
        if t.is_empty() || t.contains(':') {
            continue;
        }
        if !out.iter().any(|n| n == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// Count non-empty lines (used for installed counts and update counts).
pub fn count_lines(output: &str) -> usize {
    output.lines().filter(|l| !l.trim().is_empty()).count()
}

/// One installed package: name, version, where it came from (a repo name like
/// "extra" or "aur" for foreign packages), and installation-reason flags.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InstalledPkg {
    /// The action target: package name for pacman/AUR, app ID for Flatpak.
    pub name: String,
    /// Friendly label shown in the list. Equals `name` for pacman/AUR; the human
    /// name for Flatpak (where `name` is the reverse-DNS app ID).
    pub display: String,
    pub version: String,
    pub origin: String,
    /// In `pacman -Qe`: the user installed this on purpose.
    pub explicit: bool,
    /// In `pacman -Qdt`: a dependency nothing requires anymore (an orphan).
    pub orphan: bool,
    /// Installed size in bytes (pacman `%SIZE%` / Flatpak list size). `None` if
    /// unknown.
    pub size: Option<u64>,
    /// Last install/upgrade time, unix epoch seconds. `None` if unknown.
    pub install_date: Option<i64>,
}

/// Parse one pacman local-db `desc` file. Each `%KEY%` header line is followed
/// by its value on the next line(s). Returns `%NAME%`, `%SIZE%` (bytes), and
/// `%INSTALLDATE%` (epoch); missing or non-numeric fields yield `None`.
pub fn parse_desc(contents: &str) -> (Option<String>, Option<u64>, Option<i64>) {
    let mut name = None;
    let mut size = None;
    let mut date = None;
    let mut lines = contents.lines();
    while let Some(line) = lines.next() {
        match line.trim() {
            "%NAME%" => name = lines.next().map(|l| l.trim().to_string()),
            "%SIZE%" => size = lines.next().and_then(|l| l.trim().parse::<u64>().ok()),
            "%INSTALLDATE%" => date = lines.next().and_then(|l| l.trim().parse::<i64>().ok()),
            _ => {}
        }
    }
    (name, size, date)
}

/// Read every `<db_dir>/*/desc` into a `name -> (size, install_date)` map. A
/// missing or unreadable db directory yields an empty map, so sorting still
/// works (the values just stay `None`).
pub fn read_local_db_meta(db_dir: &std::path::Path) -> HashMap<String, (Option<u64>, Option<i64>)> {
    let mut map = HashMap::new();
    let Ok(entries) = std::fs::read_dir(db_dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let desc = entry.path().join("desc");
        if let Ok(contents) = std::fs::read_to_string(&desc) {
            let (name, size, date) = parse_desc(&contents);
            if let Some(name) = name {
                map.insert(name, (size, date));
            }
        }
    }
    map
}

/// Parse a quiet name-per-line listing (`pacman -Qeq` / `-Qdtq`) into a set.
pub fn name_set(output: &str) -> std::collections::HashSet<String> {
    output
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(|s| s.to_string())
        .collect()
}

/// Build a `name -> repo` map from `pacman -Sl` output (`repo name version
/// [installed]` per line). `-Sl` lists repos in priority order, so the first
/// occurrence of a name is the repo it actually came from.
pub fn parse_sync_repos(sl_output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in sl_output.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(repo), Some(name)) = (parts.next(), parts.next()) {
            map.entry(name.to_string()).or_insert_with(|| repo.to_string());
        }
    }
    map
}

/// Distinct repo names from `pacman -Sl` output, in first-seen (priority) order.
/// Drives the repo-filter checkbox list.
pub fn ordered_repos(sl_output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in sl_output.lines() {
        if let Some(repo) = line.split_whitespace().next() {
            if !out.iter().any(|r| r == repo) {
                out.push(repo.to_string());
            }
        }
    }
    out
}

/// Build the full installed list from `pacman -Qn` (native) and `-Qm` (foreign),
/// labelling each native package with its repo (via `repos`) and each foreign
/// package as "aur". Sorted by name.
pub fn parse_installed_list(
    native: &str,
    foreign: &str,
    repos: &HashMap<String, String>,
    explicit: &std::collections::HashSet<String>,
    orphan: &std::collections::HashSet<String>,
) -> Vec<InstalledPkg> {
    let parse = |output: &str, foreign: bool| -> Vec<InstalledPkg> {
        output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let name = parts.next()?;
                let version = parts.next().unwrap_or_default().to_string();
                let origin = if foreign {
                    "aur".to_string()
                } else {
                    repos.get(name).cloned().unwrap_or_else(|| "repo".to_string())
                };
                Some(InstalledPkg {
                    explicit: explicit.contains(name),
                    orphan: orphan.contains(name),
                    display: name.to_string(), // pacman/AUR: label is the name
                    name: name.to_string(),
                    version,
                    origin,
                    size: None,         // enriched from the local db in main.rs
                    install_date: None, // enriched from the local db in main.rs
                })
            })
            .collect()
    };

    let mut list = parse(native, false);
    list.extend(parse(foreign, true));
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// Package name from a `/var/lib/dpkg/info` filename: strip the `.list`
/// extension and any `:arch` multi-arch suffix. Non-`.list` names are returned
/// unchanged (the caller only feeds `.list` files).
pub fn dpkg_info_name(file_name: &str) -> &str {
    match file_name.strip_suffix(".list") {
        Some(stem) => stem.split(':').next().unwrap_or(stem),
        None => file_name,
    }
}

/// Build the installed list from `dpkg-query -W -f='${Package}\t${Version}\t
/// ${Installed-Size}\n'`, flagging explicit (`apt-mark showmanual`) and orphan
/// (`apt list '?autoremovable'`). Size is KiB in dpkg, stored as bytes. Origin is
/// the flat "apt". Sorted by name. `install_date` is filled later from mtimes.
pub fn parse_installed_apt(
    dpkg: &str,
    manual: &std::collections::HashSet<String>,
    autoremovable: &std::collections::HashSet<String>,
) -> Vec<InstalledPkg> {
    let mut list: Vec<InstalledPkg> = dpkg
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let name = parts.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let version = parts.next().unwrap_or_default().trim().to_string();
            let size = parts
                .next()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|kib| kib * 1024);
            Some(InstalledPkg {
                explicit: manual.contains(name),
                orphan: autoremovable.contains(name),
                display: name.to_string(),
                name: name.to_string(),
                version,
                origin: "apt".to_string(),
                size,
                install_date: None,
            })
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// Extract package names from `apt list <pattern>` output (`name/suite ver arch
/// [flags]` per line), skipping the `Listing...` header (which has no `/`). Used
/// for the orphan set from `apt list '?autoremovable'`.
pub fn parse_apt_list_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|l| l.contains('/'))
        .filter_map(|l| l.split('/').next())
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

/// Map package name -> last install/upgrade time (epoch secs) from the mtimes of
/// `<dir>/*.list` files. dpkg rewrites a package's `.list` on install/upgrade. A
/// missing/unreadable dir yields an empty map (dates just stay None).
pub fn read_dpkg_info_mtimes(dir: &std::path::Path) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        if !file.ends_with(".list") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    map.insert(dpkg_info_name(file).to_string(), dur.as_secs() as i64);
                }
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_desc_extracts_size_and_date() {
        let s = "%NAME%\nfirefox\n\n%VERSION%\n1.0\n\n%SIZE%\n299876352\n\n%INSTALLDATE%\n1718185200\n";
        let (n, size, date) = parse_desc(s);
        assert_eq!(n.as_deref(), Some("firefox"));
        assert_eq!(size, Some(299_876_352));
        assert_eq!(date, Some(1_718_185_200));

        let missing = "%NAME%\nzoxide\n";
        let (n2, size2, date2) = parse_desc(missing);
        assert_eq!(n2.as_deref(), Some("zoxide"));
        assert_eq!(size2, None);
        assert_eq!(date2, None);
    }

    #[test]
    fn builds_index_from_pacman_q() {
        let out = "firefox 141.0-1\nbash 5.2.037-1\n";
        let idx = InstalledIndex::from_query_output(out);
        assert!(idx.is_installed("firefox"));
        assert_eq!(idx.version("firefox"), Some("141.0-1"));
        assert!(!idx.is_installed("nonexistent"));
        assert_eq!(idx.version("nonexistent"), None);
    }

    #[test]
    fn count_lines_ignores_blanks() {
        assert_eq!(count_lines("a\nb\n\nc\n"), 3);
        assert_eq!(count_lines(""), 0);
    }

    #[test]
    fn parses_sync_repos_keeps_first_occurrence() {
        let sl = "extra firefox 141.0-1 [installed]\nworld firefox 141.0-1\nextra neovim 0.10.2-1\n";
        let repos = parse_sync_repos(sl);
        // first occurrence (priority order) wins
        assert_eq!(repos.get("firefox").map(String::as_str), Some("extra"));
        assert_eq!(repos.get("neovim").map(String::as_str), Some("extra"));
    }

    #[test]
    fn ordered_repos_distinct_in_priority_order() {
        let sl = "extra firefox 1\nworld firefox 1\nextra neovim 1\nmultilib lib32-foo 1\n";
        assert_eq!(ordered_repos(sl), vec!["extra", "world", "multilib"]);
        assert!(ordered_repos("").is_empty());
    }

    #[test]
    fn parses_installed_list_with_origin() {
        let native = "firefox 141.0-1\nneovim 0.10.2-1\n";
        let foreign = "yay 12.4.0-1\n";
        let mut repos = HashMap::new();
        repos.insert("firefox".to_string(), "extra".to_string());
        let none = std::collections::HashSet::new();
        let list = parse_installed_list(native, foreign, &repos, &none, &none);
        assert_eq!(list.len(), 3);
        // sorted by name: firefox, neovim, yay
        assert_eq!(list[0].name, "firefox");
        assert_eq!(list[0].origin, "extra");
        assert_eq!(list[1].name, "neovim");
        assert_eq!(list[1].origin, "repo"); // unknown repo falls back
        assert_eq!(list[2].name, "yay");
        assert_eq!(list[2].origin, "aur"); // foreign
        assert!(parse_installed_list("", "", &repos, &none, &none).is_empty());
    }

    #[test]
    fn flags_explicit_and_orphan_from_sets() {
        let native = "firefox 141.0-1\nlibfoo 1.0-1\nldb 2.0-1\n";
        let repos = HashMap::new();
        let explicit = name_set("firefox\n");
        let orphan = name_set("ldb\n");
        let list = parse_installed_list(native, "", &repos, &explicit, &orphan);
        let by = |n: &str| list.iter().find(|p| p.name == n).unwrap().clone();
        assert!(by("firefox").explicit && !by("firefox").orphan);
        assert!(by("ldb").orphan && !by("ldb").explicit);
        assert!(!by("libfoo").explicit && !by("libfoo").orphan);
    }

    #[test]
    fn parses_qi_detail() {
        let qi = "\
Name            : firefox
Version         : 152.0.3-1
Description     : Fast web browser
URL             : https://www.mozilla.org/firefox/
Depends On      : alsa-lib  at-spi2-core  gtk3
Required By     : None
Optional For    : None
Installed Size  : 285.93 MiB
Build Date      : Fri 26 Jun 2026 07:00:16 AM CEST
Install Date    : Sat 12 Jun 2026 10:00:00 AM CEST
Install Reason  : Explicitly installed
";
        let d = parse_pkg_detail(qi);
        assert_eq!(d.name, "firefox");
        assert_eq!(d.version, "152.0.3-1");
        assert_eq!(d.description, "Fast web browser");
        assert_eq!(d.url, "https://www.mozilla.org/firefox/");
        assert!(d.explicit);
        assert_eq!(d.install_date, "Sat 12 Jun 2026 10:00:00 AM CEST");
        assert_eq!(d.size, "285.93 MiB");
        assert_eq!(d.depends, vec!["alsa-lib", "at-spi2-core", "gtk3"]);
        assert!(d.required_by.is_empty()); // None -> empty
        assert!(d.optional_for.is_empty());
    }

    #[test]
    fn qi_dependency_install_reason_not_explicit() {
        let qi = "Name : foo\nInstall Reason  : Installed as a dependency for an installed package\n";
        assert!(!parse_pkg_detail(qi).explicit);
    }

    #[test]
    fn qi_continuation_lines_append() {
        // pacman may wrap a long Required By across indented lines.
        let qi = "Required By     : aaa  bbb\n                  ccc  ddd\nInstalled Size  : 1 MiB\n";
        let d = parse_pkg_detail(qi);
        assert_eq!(d.required_by, vec!["aaa", "bbb", "ccc", "ddd"]);
        assert_eq!(d.size, "1 MiB");
    }

    #[test]
    fn format_epoch_date_utc() {
        assert_eq!(format_epoch_date(0), "1970-01-01 00:00");
        assert_eq!(format_epoch_date(1_700_000_000), "2023-11-14 22:13");
    }

    #[test]
    fn parse_rdepends_names() {
        let out = "bash\nReverse Depends:\n  pkg1\n  pkg2\n |pkg3\n  pkg1\n";
        assert_eq!(parse_rdepends(out), vec!["pkg1", "pkg2", "pkg3"]); // deduped, | stripped
        assert!(parse_rdepends("bash\nReverse Depends:\n").is_empty());
    }

    #[test]
    fn parse_dpkg_status_extracts_fields() {
        let s = "\
Package: bash
Status: install ok installed
Installed-Size: 6469
Maintainer: Someone <x@y.z>
Version: 5.2.15-2+b7
Depends: base-files (>= 2.1.12), debianutils (>= 2.15)
Description: GNU Bourne Again SHell
 Bash is an sh-compatible command language interpreter.
 More long text here.
Homepage: http://tiswww.case.edu/php/chet/bash/
";
        let d = parse_dpkg_status(s);
        assert_eq!(d.name, "bash");
        assert_eq!(d.version, "5.2.15-2+b7");
        assert_eq!(d.description, "GNU Bourne Again SHell"); // synopsis only
        assert_eq!(d.url, "http://tiswww.case.edu/php/chet/bash/");
        assert_eq!(d.size, "6.32 MiB"); // 6469 KiB
        assert_eq!(d.depends, vec!["base-files", "debianutils"]);
        assert!(d.required_by.is_empty());
    }

    #[test]
    fn parse_apt_list_names_skips_header() {
        let out = "Listing...\nlibfoo/now 1.0 amd64 [installed,auto]\nvim/stable 9.0 amd64 [installed]\n";
        assert_eq!(parse_apt_list_names(out), vec!["libfoo", "vim"]);
        assert!(parse_apt_list_names("Listing...\n").is_empty());
    }

    #[test]
    fn dpkg_info_name_strips_list_and_arch() {
        assert_eq!(dpkg_info_name("bash.list"), "bash");
        assert_eq!(dpkg_info_name("libc6:amd64.list"), "libc6");
        assert_eq!(dpkg_info_name("foo.md5sums"), "foo.md5sums"); // not a .list, unchanged
    }

    #[test]
    fn parse_installed_apt_flags_and_size() {
        let dpkg = "bash\t5.2-3\t2048\nlibfoo\t1.0\t512\nvim\t9.0\t1024\n";
        let manual: std::collections::HashSet<String> =
            ["bash".to_string(), "vim".to_string()].into_iter().collect();
        let autoremovable: std::collections::HashSet<String> =
            ["libfoo".to_string()].into_iter().collect();
        let list = parse_installed_apt(dpkg, &manual, &autoremovable);
        assert_eq!(list.len(), 3);
        let by = |n: &str| list.iter().find(|p| p.name == n).unwrap().clone();
        assert_eq!(by("bash").origin, "apt");
        assert_eq!(by("bash").display, "bash");
        assert_eq!(by("bash").version, "5.2-3");
        assert_eq!(by("bash").size, Some(2048 * 1024)); // KiB -> bytes
        assert!(by("bash").explicit && !by("bash").orphan);
        assert!(by("libfoo").orphan && !by("libfoo").explicit);
        assert!(by("vim").explicit);
        assert!(parse_installed_apt("", &manual, &autoremovable).is_empty());
    }

    #[test]
    fn name_set_collects_names() {
        let s = name_set("a 1.0\nb\n\n c\n");
        assert!(s.contains("a") && s.contains("b") && s.contains("c"));
        assert_eq!(s.len(), 3);
    }
}
