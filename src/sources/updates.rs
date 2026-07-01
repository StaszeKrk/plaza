use crate::model::SourceId;
use crate::sources::installed::count_lines;

/// Count the package-update lines emitted by `checkupdates`, `pacman -Qu`, or
/// `yay -Qua` (one upgradable package per line).
pub fn parse_update_count(output: &str) -> usize {
    count_lines(output)
}

/// Count `apt list --upgradable` package lines, skipping the `Listing...`
/// header. Package lines carry the `[upgradable from: ...]` marker.
pub fn parse_apt_upgradable_count(output: &str) -> usize {
    output.lines().filter(|l| l.contains("[upgradable")).count()
}

/// Parse `apt list --upgradable` into update entries. Each package line is
/// `name/suite new-version arch [upgradable from: old-version]`.
pub fn parse_apt_upgradable_list(output: &str) -> Vec<UpdateEntry> {
    output
        .lines()
        .filter(|l| l.contains("[upgradable"))
        .filter_map(|line| {
            let name = line.split('/').next()?.trim();
            if name.is_empty() {
                return None;
            }
            let new_version =
                line.split_whitespace().nth(1).unwrap_or_default().to_string();
            let old_version = line
                .rsplit_once("from:")
                .map(|(_, o)| o.trim().trim_end_matches(']').trim().to_string())
                .unwrap_or_default();
            Some(UpdateEntry {
                name: name.to_string(),
                old_version,
                new_version,
                source_id: SourceId::Apt,
            })
        })
        .collect()
}

/// One upgradable package with its current and target version, tagged with the
/// source it came from (repos vs AUR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateEntry {
    pub name: String,
    pub old_version: String,
    pub new_version: String,
    pub source_id: SourceId,
}

/// Parse the update list emitted by `checkupdates`, `pacman -Qu`, or `yay -Qua`,
/// tagging every entry with `source_id`. Each non-empty line is `name old ->
/// new`; versions default to empty when a line does not follow that shape.
pub fn parse_update_list(output: &str, source_id: SourceId) -> Vec<UpdateEntry> {
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
                source_id,
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
    fn counts_apt_upgradable() {
        let out = "Listing...\nvim/bookworm 2:9.0-2 amd64 [upgradable from: 2:9.0-1]\ncurl/bookworm 7.88-2 amd64 [upgradable from: 7.88-1]\n";
        assert_eq!(parse_apt_upgradable_count(out), 2);
        assert_eq!(parse_apt_upgradable_count("Listing...\n"), 0);
        assert_eq!(parse_apt_upgradable_count(""), 0);
    }

    #[test]
    fn parses_apt_upgradable_list() {
        let out = "Listing...\nvim/bookworm-security 2:9.0-2 amd64 [upgradable from: 2:9.0-1]\n";
        let list = parse_apt_upgradable_list(out);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "vim");
        assert_eq!(list[0].new_version, "2:9.0-2");
        assert_eq!(list[0].old_version, "2:9.0-1");
        assert_eq!(list[0].source_id, SourceId::Apt);
        assert!(parse_apt_upgradable_list("Listing...\n").is_empty());
    }

    #[test]
    fn parses_update_list_with_arrow() {
        let out = "firefox 140.0-1 -> 141.0-1\nlinux 6.9 -> 6.10\n\n";
        let list = parse_update_list(out, SourceId::Pacman);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "firefox");
        assert_eq!(list[0].old_version, "140.0-1");
        assert_eq!(list[0].new_version, "141.0-1");
        assert_eq!(list[0].source_id, SourceId::Pacman);
        assert_eq!(list[1].name, "linux");
        assert_eq!(list[1].new_version, "6.10");
    }

    #[test]
    fn parses_update_list_without_arrow() {
        // `pacman -Qu` without the arrow form still yields a name and old version.
        let out = "firefox 141.0-1\n";
        let list = parse_update_list(out, SourceId::Aur);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "firefox");
        assert_eq!(list[0].old_version, "141.0-1");
        assert_eq!(list[0].new_version, "");
        assert_eq!(list[0].source_id, SourceId::Aur);
        assert!(parse_update_list("", SourceId::Pacman).is_empty());
    }
}
