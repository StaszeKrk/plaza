//! The shape axis: everything non-color. Borders (numeric thickness/radius plus
//! an optional explicit glyph override), the icon/glyph set, and the highlight
//! and badge render modes. The default is `soft` (light rounded, nerd icons,
//! bar highlight, chip badges).
//!
//! Terminal honesty: a character grid has no pixels, so border "thickness" is
//! the box-drawing weight that exists (none / light / heavy) and "radius" is
//! square vs the single rounded arc glyph (light weight only). Anything outside
//! that is reachable through `[border.glyphs]`.

use ratatui::symbols::border;
use ratatui::widgets::Borders;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HighlightMode {
    Bar,
    Reversed,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BadgeMode {
    Brackets,
    Bare,
    Chip,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BorderGlyphs {
    pub top_left: String,
    pub top_right: String,
    pub bottom_left: String,
    pub bottom_right: String,
    pub horizontal: String,
    pub vertical: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Border {
    /// 0 none, 1 light, 2 heavy.
    pub thickness: u8,
    /// 0 square, 1 rounded (honored only at thickness 1).
    pub radius: u8,
    /// Two-line style; supersedes weight when true.
    pub double: bool,
    /// Explicit per-piece override; wins over the numeric knobs entirely.
    pub glyphs: Option<BorderGlyphs>,
}

impl Default for Border {
    fn default() -> Self {
        Border { thickness: 1, radius: 1, double: false, glyphs: None }
    }
}

impl Border {
    pub fn set(&self) -> border::Set {
        let base = if self.double {
            border::DOUBLE
        } else if self.thickness >= 2 {
            border::THICK
        } else if self.radius >= 1 {
            border::ROUNDED
        } else {
            border::PLAIN
        };
        match &self.glyphs {
            None => base,
            Some(g) => border::Set {
                top_left: intern(&g.top_left),
                top_right: intern(&g.top_right),
                bottom_left: intern(&g.bottom_left),
                bottom_right: intern(&g.bottom_right),
                vertical_left: intern(&g.vertical),
                vertical_right: intern(&g.vertical),
                horizontal_top: intern(&g.horizontal),
                horizontal_bottom: intern(&g.horizontal),
            },
        }
    }

    pub fn borders(&self) -> Borders {
        if self.thickness == 0 {
            Borders::NONE
        } else {
            Borders::ALL
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Icons {
    /// When false, the UI uses plain unicode literals instead of these glyphs.
    pub enabled: bool,
    pub package: String,
    pub repo: String,
    pub aur: String,
    pub flatpak: String,
    pub installed: String,
    pub update: String,
    pub running: String,
    pub success: String,
    pub fail: String,
    pub cursor: String,
    pub lock: String,
    pub search: String,
}

impl Default for Icons {
    fn default() -> Self {
        Icons {
            enabled: true,
            package: "\u{f487}".into(),
            repo: "\u{f233}".into(),
            aur: "\u{f303}".into(),
            // Flathub logo from the font-logos block (U+F300 + offset 36).
            flatpak: "\u{f324}".into(),
            installed: "\u{f00c}".into(),
            update: "\u{f062}".into(),
            running: "\u{f110}".into(),
            success: "\u{f00c}".into(),
            fail: "\u{f00d}".into(),
            cursor: "\u{25b8}".into(), // ▸
            lock: "\u{f023}".into(),
            search: "\u{f002}".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Skin {
    pub border: Border,
    pub highlight: HighlightMode,
    pub badge: BadgeMode,
    pub icons: Icons,
}

impl Default for Skin {
    fn default() -> Self {
        Skin {
            border: Border::default(),
            highlight: HighlightMode::Bar,
            badge: BadgeMode::Chip,
            icons: Icons::default(),
        }
    }
}

/// Intern a runtime border glyph to a `&'static str` (ratatui's `border::Set`
/// fields are `&'static str`). Leaks at most once per distinct glyph ever used,
/// so repeated theme reloads do not accumulate.
fn intern(s: &str) -> &'static str {
    fn pool() -> &'static Mutex<HashSet<&'static str>> {
        static POOL: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
        POOL.get_or_init(|| Mutex::new(HashSet::new()))
    }
    let mut p = pool().lock().unwrap();
    if let Some(found) = p.get(s) {
        return found;
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    p.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_is_rounded() {
        let s = Skin::default();
        assert_eq!(s.border.set().top_left, border::ROUNDED.top_left); // ╭
        assert!(s.icons.enabled);
        assert_eq!(s.highlight, HighlightMode::Bar);
        assert_eq!(s.badge, BadgeMode::Chip);
    }

    #[test]
    fn heavy_square() {
        let b = Border { thickness: 2, radius: 1, double: false, glyphs: None };
        assert_eq!(b.set().top_left, border::THICK.top_left); // radius ignored
    }

    #[test]
    fn double_wins() {
        let b = Border { thickness: 1, radius: 1, double: true, glyphs: None };
        assert_eq!(b.set().top_left, border::DOUBLE.top_left);
    }

    #[test]
    fn no_border() {
        let b = Border { thickness: 0, radius: 0, double: false, glyphs: None };
        assert_eq!(b.borders(), Borders::NONE);
    }

    #[test]
    fn glyph_override_wins() {
        let g = BorderGlyphs {
            top_left: "X".into(),
            top_right: "\u{256e}".into(),
            bottom_left: "\u{2570}".into(),
            bottom_right: "\u{256f}".into(),
            horizontal: "\u{2500}".into(),
            vertical: "\u{2502}".into(),
        };
        let b = Border { thickness: 1, radius: 1, double: false, glyphs: Some(g) };
        assert_eq!(b.set().top_left, "X");
    }

    #[test]
    fn partial_skin_defaults() {
        let s: Skin = toml::from_str("highlight = \"reversed\"").unwrap();
        assert_eq!(s.highlight, HighlightMode::Reversed);
        assert_eq!(s.badge, Skin::default().badge); // inherited
    }

    #[test]
    fn partial_border_table_defaults() {
        let s: Skin = toml::from_str("[border]\nthickness = 2\n").unwrap();
        assert_eq!(s.border.thickness, 2);
        assert!(!s.border.double); // inherited default
    }
}
