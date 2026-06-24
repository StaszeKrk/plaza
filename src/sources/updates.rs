use crate::sources::installed::count_lines;

/// Count the package-update lines emitted by `checkupdates`, `pacman -Qu`, or
/// `yay -Qua` (one upgradable package per line).
pub fn parse_update_count(output: &str) -> usize {
    count_lines(output)
}

/// One upgradable package with its current and target version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateEntry {
    pub name: String,
    pub old_version: String,
    pub new_version: String,
}

/// Parse the update list emitted by `checkupdates`, `pacman -Qu`, or `yay -Qua`.
/// Each non-empty line is `name old -> new`; versions default to empty when a
/// line does not follow that shape.
pub fn parse_update_list(output: &str) -> Vec<UpdateEntry> {
    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let old_version = parts.next().unwrap_or_default().to_string();
            // Skip the "->" arrow if present.
            let mut rest = parts;
            let after = rest.next().unwrap_or_default();
            let new_version = if after == "->" {
                rest.next().unwrap_or_default().to_string()
            } else {
                after.to_string()
            };
            Some(UpdateEntry {
                name: name.to_string(),
                old_version,
                new_version,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_update_lines() {
        let out = "firefox 140.0-1 -> 141.0-1\nlinux 6.9 -> 6.10\n";
        assert_eq!(parse_update_count(out), 2);
        assert_eq!(parse_update_count(""), 0);
    }

    #[test]
    fn parses_update_list_with_arrow() {
        let out = "firefox 140.0-1 -> 141.0-1\nlinux 6.9 -> 6.10\n\n";
        let list = parse_update_list(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "firefox");
        assert_eq!(list[0].old_version, "140.0-1");
        assert_eq!(list[0].new_version, "141.0-1");
        assert_eq!(list[1].name, "linux");
        assert_eq!(list[1].new_version, "6.10");
    }

    #[test]
    fn parses_update_list_without_arrow() {
        // `pacman -Qu` without the arrow form still yields a name and old version.
        let out = "firefox 141.0-1\n";
        let list = parse_update_list(out);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "firefox");
        assert_eq!(list[0].old_version, "141.0-1");
        assert_eq!(list[0].new_version, "");
        assert!(parse_update_list("").is_empty());
    }
}
