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
}
