//! The color axis. A `Palette` is a set of semantic color roles. The default
//! is `plaza-dusk` (a bespoke deep-slate dark theme); user files may override
//! any subset and inherit the rest.

use super::color;
use ratatui::style::Color;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// `None` means "do not paint a background" (keep the terminal's own).
    pub bg: Option<Color>,
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    pub title: Color,
    pub section: Color,
    pub border_idle: Color,
    pub border_hover: Color,
    pub border_active: Color,
    pub highlight_fg: Color,
    pub highlight_bg: Color,
    pub badge_repo: Color,
    pub badge_aur: Color,
    pub badge_official: Color,
    pub installed: Color,
    pub update: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

impl Default for Palette {
    /// `plaza-dusk`: deep slate base, blue-violet accent, blue repo / magenta
    /// aur / green installed / amber update.
    fn default() -> Self {
        let accent = Color::Rgb(0x7a, 0xa2, 0xf7);
        let base = Color::Rgb(0x16, 0x18, 0x21);
        let installed = Color::Rgb(0x9e, 0xce, 0x6a);
        let update = Color::Rgb(0xe0, 0xaf, 0x68);
        Palette {
            bg: Some(base),
            fg: Color::Rgb(0xc8, 0xcc, 0xd4),
            muted: Color::Rgb(0x6b, 0x70, 0x89),
            accent,
            title: accent,
            section: Color::Rgb(0x9a, 0xa0, 0xb3),
            border_idle: Color::Rgb(0x2a, 0x2e, 0x3a),
            border_hover: update,
            border_active: accent,
            highlight_fg: base,
            highlight_bg: accent,
            badge_repo: accent,
            badge_aur: Color::Rgb(0xbb, 0x9a, 0xf7),
            badge_official: Color::Rgb(0x7d, 0xcf, 0xff),
            installed,
            update,
            success: installed,
            warning: update,
            danger: Color::Rgb(0xf7, 0x76, 0x8e),
        }
    }
}

/// The on-disk form: every field optional so a user file can override a subset.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawPalette {
    pub bg: Option<String>,
    pub fg: Option<String>,
    pub muted: Option<String>,
    pub accent: Option<String>,
    pub title: Option<String>,
    pub section: Option<String>,
    pub border_idle: Option<String>,
    pub border_hover: Option<String>,
    pub border_active: Option<String>,
    pub highlight_fg: Option<String>,
    pub highlight_bg: Option<String>,
    pub badge_repo: Option<String>,
    pub badge_aur: Option<String>,
    pub badge_official: Option<String>,
    pub installed: Option<String>,
    pub update: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub danger: Option<String>,
}

impl RawPalette {
    /// Parse into a full `Palette`, filling missing fields from the default and
    /// honoring `bg = "none"` (transparent).
    pub fn resolve(&self) -> Result<Palette, String> {
        let d = Palette::default();
        let pick = |raw: &Option<String>, def: Color| -> Result<Color, String> {
            match raw {
                Some(s) => color::parse(s),
                None => Ok(def),
            }
        };
        let bg = match &self.bg {
            None => d.bg,
            Some(s) if s.trim().eq_ignore_ascii_case("none") => None,
            Some(s) => Some(color::parse(s)?),
        };
        Ok(Palette {
            bg,
            fg: pick(&self.fg, d.fg)?,
            muted: pick(&self.muted, d.muted)?,
            accent: pick(&self.accent, d.accent)?,
            title: pick(&self.title, d.title)?,
            section: pick(&self.section, d.section)?,
            border_idle: pick(&self.border_idle, d.border_idle)?,
            border_hover: pick(&self.border_hover, d.border_hover)?,
            border_active: pick(&self.border_active, d.border_active)?,
            highlight_fg: pick(&self.highlight_fg, d.highlight_fg)?,
            highlight_bg: pick(&self.highlight_bg, d.highlight_bg)?,
            badge_repo: pick(&self.badge_repo, d.badge_repo)?,
            badge_aur: pick(&self.badge_aur, d.badge_aur)?,
            badge_official: pick(&self.badge_official, d.badge_official)?,
            installed: pick(&self.installed, d.installed)?,
            update: pick(&self.update, d.update)?,
            success: pick(&self.success, d.success)?,
            warning: pick(&self.warning, d.warning)?,
            danger: pick(&self.danger, d.danger)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_complete() {
        let p = Palette::default();
        assert_eq!(p.accent, Color::Rgb(0x7a, 0xa2, 0xf7));
        assert_eq!(p.bg, Some(Color::Rgb(0x16, 0x18, 0x21)));
    }

    #[test]
    fn partial_inherits_default() {
        let raw: RawPalette = toml::from_str("accent = \"#ff0000\"").unwrap();
        let p = raw.resolve().unwrap();
        assert_eq!(p.accent, Color::Rgb(0xff, 0, 0));
        assert_eq!(p.fg, Palette::default().fg);
    }

    #[test]
    fn bg_none() {
        let raw: RawPalette = toml::from_str("bg = \"none\"").unwrap();
        assert_eq!(raw.resolve().unwrap().bg, None);
    }

    #[test]
    fn bad_color_errs() {
        let raw: RawPalette = toml::from_str("accent = \"zzz\"").unwrap();
        assert!(raw.resolve().is_err());
    }
}
