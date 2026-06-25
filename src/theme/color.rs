//! Parse a color string from a theme file into a ratatui `Color`.
//!
//! Accepts three forms so themes can be art-directed (`#rrggbb`) or made to
//! track the terminal palette (ANSI names / 256-index):
//!   - hex: `#rgb` or `#rrggbb`
//!   - ANSI name: `cyan`, `darkgray`, `lightblue`, ... (case-insensitive)
//!   - 256-index: `13` or `color13`

use ratatui::style::Color;

pub fn parse(s: &str) -> Result<Color, String> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(c) = ansi(t) {
        return Ok(c);
    }
    let idx = t.strip_prefix("color").unwrap_or(t);
    if let Ok(n) = idx.parse::<u8>() {
        return Ok(Color::Indexed(n));
    }
    Err(format!("unrecognized color: {s}"))
}

fn parse_hex(h: &str) -> Result<Color, String> {
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_string(),
        _ => return Err(format!("bad hex: #{h}")),
    };
    let n = u32::from_str_radix(&full, 16).map_err(|_| format!("bad hex: #{h}"))?;
    Ok(Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

fn ansi(s: &str) -> Option<Color> {
    Some(match s.to_ascii_lowercase().as_str() {
        "reset" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_long() {
        assert_eq!(parse("#7aa2f7").unwrap(), Color::Rgb(0x7a, 0xa2, 0xf7));
    }
    #[test]
    fn hex_short() {
        assert_eq!(parse("#abc").unwrap(), Color::Rgb(0xaa, 0xbb, 0xcc));
    }
    #[test]
    fn ansi_name() {
        assert_eq!(parse("cyan").unwrap(), Color::Cyan);
    }
    #[test]
    fn ansi_name_ci() {
        assert_eq!(parse("DarkGray").unwrap(), Color::DarkGray);
    }
    #[test]
    fn indexed() {
        assert_eq!(parse("13").unwrap(), Color::Indexed(13));
    }
    #[test]
    fn indexed_prefixed() {
        assert_eq!(parse("color200").unwrap(), Color::Indexed(200));
    }
    #[test]
    fn bad() {
        assert!(parse("nope").is_err());
        assert!(parse("#12").is_err());
    }
}
