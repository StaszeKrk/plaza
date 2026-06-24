use crate::sources::installed::count_lines;

/// Count the package-update lines emitted by `checkupdates`, `pacman -Qu`, or
/// `yay -Qua` (one upgradable package per line).
pub fn parse_update_count(output: &str) -> usize {
    count_lines(output)
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
}
