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

    pub fn is_installed(&self, name: &str) -> bool {
        self.versions.contains_key(name)
    }

    pub fn version(&self, name: &str) -> Option<&str> {
        self.versions.get(name).map(String::as_str)
    }
}

/// Count non-empty lines (used for installed counts and update counts).
pub fn count_lines(output: &str) -> usize {
    output.lines().filter(|l| !l.trim().is_empty()).count()
}

/// One installed package: name, version, and where it came from (a repo name
/// like "extra" or "aur" for foreign packages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPkg {
    pub name: String,
    pub version: String,
    pub origin: String,
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

/// Build the full installed list from `pacman -Qn` (native) and `-Qm` (foreign),
/// labelling each native package with its repo (via `repos`) and each foreign
/// package as "aur". Sorted by name.
pub fn parse_installed_list(
    native: &str,
    foreign: &str,
    repos: &HashMap<String, String>,
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
                Some(InstalledPkg { name: name.to_string(), version, origin })
            })
            .collect()
    };

    let mut list = parse(native, false);
    list.extend(parse(foreign, true));
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parses_installed_list_with_origin() {
        let native = "firefox 141.0-1\nneovim 0.10.2-1\n";
        let foreign = "yay 12.4.0-1\n";
        let mut repos = HashMap::new();
        repos.insert("firefox".to_string(), "extra".to_string());
        let list = parse_installed_list(native, foreign, &repos);
        assert_eq!(list.len(), 3);
        // sorted by name: firefox, neovim, yay
        assert_eq!(list[0].name, "firefox");
        assert_eq!(list[0].origin, "extra");
        assert_eq!(list[1].name, "neovim");
        assert_eq!(list[1].origin, "repo"); // unknown repo falls back
        assert_eq!(list[2].name, "yay");
        assert_eq!(list[2].origin, "aur"); // foreign
        assert!(parse_installed_list("", "", &repos).is_empty());
    }
}
