//! Theming engine: two independent axes, both loadable from TOML.
//!
//! - [`palette`]: colors only.
//! - [`skin`]: everything non-color (borders, glyphs/icons, highlight + badge
//!   modes).
//!
//! Built-in presets are embedded and parsed through the same loader as user
//! files in `~/.config/plaza/{palettes,skins}/`. The default palette
//! (`plaza-dusk`) and skin (`soft`) are the `Default` impls and head each
//! registry; a user file with the same name overrides the built-in in place.

pub mod color;
pub mod palette;
pub mod skin;

use palette::{Palette, RawPalette};
use skin::Skin;
use std::path::{Path, PathBuf};

pub const DEFAULT_PALETTE: &str = "plaza-dusk";
pub const DEFAULT_SKIN: &str = "soft";

const BUILTIN_PALETTES: &[(&str, &str)] = &[
    ("gruvbox", include_str!("palettes/gruvbox.toml")),
    ("nord", include_str!("palettes/nord.toml")),
    ("dracula", include_str!("palettes/dracula.toml")),
    ("tokyo-night", include_str!("palettes/tokyo-night.toml")),
    ("solarized-dark", include_str!("palettes/solarized-dark.toml")),
    ("catppuccin-mocha", include_str!("palettes/catppuccin-mocha.toml")),
    ("ansi", include_str!("palettes/ansi.toml")),
];

const BUILTIN_SKINS: &[(&str, &str)] = &[
    ("sharp", include_str!("skins/sharp.toml")),
    ("plain", include_str!("skins/plain.toml")),
];

/// Build the ordered palette registry: `plaza-dusk` first, then the embedded
/// presets, then any user files (which override a same-named entry in place).
/// Returns the registry and a list of human-readable load errors (non-fatal).
pub fn palette_registry(user_dir: Option<&Path>) -> (Vec<(String, Palette)>, Vec<String>) {
    let mut out = vec![(DEFAULT_PALETTE.to_string(), Palette::default())];
    let mut errs = Vec::new();
    for (name, src) in BUILTIN_PALETTES {
        match parse_palette(src) {
            Ok(p) => upsert(&mut out, name.to_string(), p),
            Err(e) => errs.push(format!("builtin palette {name}: {e}")),
        }
    }
    if let Some(dir) = user_dir {
        for (stem, src) in read_toml_dir(dir) {
            match parse_palette(&src) {
                Ok(p) => upsert(&mut out, stem, p),
                Err(e) => errs.push(format!("palette {stem}: {e}")),
            }
        }
    }
    (out, errs)
}

/// Build the ordered skin registry: `soft` first, then embedded presets, then
/// user files (overriding same-named entries in place).
pub fn skin_registry(user_dir: Option<&Path>) -> (Vec<(String, Skin)>, Vec<String>) {
    let mut out = vec![(DEFAULT_SKIN.to_string(), Skin::default())];
    let mut errs = Vec::new();
    for (name, src) in BUILTIN_SKINS {
        match toml::from_str::<Skin>(src) {
            Ok(s) => upsert(&mut out, name.to_string(), s),
            Err(e) => errs.push(format!("builtin skin {name}: {e}")),
        }
    }
    if let Some(dir) = user_dir {
        for (stem, src) in read_toml_dir(dir) {
            match toml::from_str::<Skin>(&src) {
                Ok(s) => upsert(&mut out, stem, s),
                Err(e) => errs.push(format!("skin {stem}: {e}")),
            }
        }
    }
    (out, errs)
}

fn parse_palette(src: &str) -> Result<Palette, String> {
    toml::from_str::<RawPalette>(src)
        .map_err(|e| e.to_string())
        .and_then(|r| r.resolve())
}

/// Read `*.toml` files in `dir`, sorted by name, as `(file_stem, contents)`.
fn read_toml_dir(dir: &Path) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "toml").unwrap_or(false))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.to_string();
            let src = std::fs::read_to_string(&p).ok()?;
            Some((stem, src))
        })
        .collect()
}

fn upsert<T>(out: &mut Vec<(String, T)>, name: String, val: T) {
    match out.iter_mut().find(|(n, _)| *n == name) {
        Some(slot) => slot.1 = val,
        None => out.push((name, val)),
    }
}

pub fn resolve_palette(reg: &[(String, Palette)], name: &str) -> Palette {
    reg.iter()
        .find(|(n, _)| n == name)
        .map(|(_, p)| p.clone())
        .unwrap_or_default()
}

pub fn resolve_skin(reg: &[(String, Skin)], name: &str) -> Skin {
    reg.iter()
        .find(|(n, _)| n == name)
        .map(|(_, s)| s.clone())
        .unwrap_or_default()
}

/// The ordered list of names in a registry.
pub fn names<T>(reg: &[(String, T)]) -> Vec<String> {
    reg.iter().map(|(n, _)| n.clone()).collect()
}

/// Next name after `current`, wrapping. A miss (or empty) yields the first name
/// (or `current` if there are none).
pub fn next_name(names: &[String], current: &str) -> String {
    if names.is_empty() {
        return current.to_string();
    }
    match names.iter().position(|n| n == current) {
        Some(i) => names[(i + 1) % names.len()].clone(),
        None => names[0].clone(),
    }
}

pub fn user_palette_path(name: &str) -> Option<PathBuf> {
    Some(
        crate::config::config_base()?
            .join("plaza")
            .join("palettes")
            .join(format!("{name}.toml")),
    )
}

pub fn user_skin_path(name: &str) -> Option<PathBuf> {
    Some(
        crate::config::config_base()?
            .join("plaza")
            .join("skins")
            .join(format!("{name}.toml")),
    )
}

/// Read and parse a palette file (used by live-reload). `None` on any error.
pub fn load_palette_file(path: &Path) -> Option<Palette> {
    let src = std::fs::read_to_string(path).ok()?;
    parse_palette(&src).ok()
}

/// Read and parse a skin file (used by live-reload). `None` on any error.
pub fn load_skin_file(path: &Path) -> Option<Skin> {
    let src = std::fs::read_to_string(path).ok()?;
    toml::from_str::<Skin>(&src).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_present_and_ordered() {
        let (reg, errs) = palette_registry(None);
        assert!(errs.is_empty(), "{errs:?}");
        let ns = names(&reg);
        assert_eq!(ns[0], "plaza-dusk");
        assert!(ns.iter().any(|n| n == "gruvbox"));
        assert!(ns.iter().any(|n| n == "nord"));
    }

    #[test]
    fn all_builtin_palettes_parse() {
        let (reg, errs) = palette_registry(None);
        assert!(errs.is_empty(), "{errs:?}");
        // plaza-dusk + 7 presets
        assert_eq!(reg.len(), 8);
    }

    #[test]
    fn resolve_miss_is_default() {
        let (reg, _) = palette_registry(None);
        assert_eq!(resolve_palette(&reg, "does-not-exist"), Palette::default());
    }

    #[test]
    fn cycle_wraps() {
        let ns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(next_name(&ns, "a"), "b");
        assert_eq!(next_name(&ns, "c"), "a");
        assert_eq!(next_name(&ns, "x"), "a"); // miss -> first
    }

    #[test]
    fn skin_builtins() {
        let (reg, errs) = skin_registry(None);
        assert!(errs.is_empty(), "{errs:?}");
        let ns = names(&reg);
        assert_eq!(ns[0], "soft");
        assert!(ns.iter().any(|n| n == "sharp"));
        assert!(ns.iter().any(|n| n == "plain"));
    }
}
