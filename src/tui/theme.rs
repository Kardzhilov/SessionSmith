//! Theme system: named colour roles resolved to `ratatui` styles. Ships a few
//! built-in themes and loads user themes from
//! `~/.config/sessionsmith/themes/*.toml`, switchable live in the TUI.

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub bg: Color,
    pub fg: Color,
    pub primary: Color,
    pub accent: Color,
    pub success: Color,
    pub warn: Color,
    pub error: Color,
    pub muted: Color,
    pub border: Color,
    pub border_focus: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
}

impl Theme {
    /// Base style for regular text on the theme background.
    pub fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }
    /// Style for the selected/highlighted row.
    pub fn selection(&self) -> Style {
        Style::default()
            .fg(self.selection_fg)
            .bg(self.selection_bg)
            .add_modifier(Modifier::BOLD)
    }
    pub fn border_style(&self, focused: bool) -> Style {
        Style::default().fg(if focused {
            self.border_focus
        } else {
            self.border
        })
    }
    pub fn title_style(&self, focused: bool) -> Style {
        let c = if focused { self.accent } else { self.primary };
        Style::default().fg(c).add_modifier(Modifier::BOLD)
    }
    pub fn muted_style(&self) -> Style {
        Style::default().fg(self.muted)
    }
    pub fn success_style(&self) -> Style {
        Style::default().fg(self.success)
    }
    pub fn warn_style(&self) -> Style {
        Style::default().fg(self.warn)
    }
    pub fn error_style(&self) -> Style {
        Style::default().fg(self.error)
    }
    pub fn accent_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
}

/// The built-in themes, always available.
pub fn builtins() -> Vec<Theme> {
    vec![
        Theme {
            name: "midnight".into(),
            bg: Color::Reset,
            fg: rgb(0xd0, 0xd0, 0xe0),
            primary: rgb(0x7a, 0xa2, 0xf7),
            accent: rgb(0xbb, 0x9a, 0xf7),
            success: rgb(0x9e, 0xce, 0x6a),
            warn: rgb(0xe0, 0xaf, 0x68),
            error: rgb(0xf7, 0x76, 0x8e),
            muted: rgb(0x56, 0x5f, 0x89),
            border: rgb(0x3b, 0x42, 0x61),
            border_focus: rgb(0x7a, 0xa2, 0xf7),
            selection_bg: rgb(0x2d, 0x3f, 0x76),
            selection_fg: rgb(0xff, 0xff, 0xff),
        },
        Theme {
            name: "solar".into(),
            bg: Color::Reset,
            fg: rgb(0x65, 0x7b, 0x83),
            primary: rgb(0x26, 0x8b, 0xd2),
            accent: rgb(0xd3, 0x36, 0x82),
            success: rgb(0x85, 0x99, 0x00),
            warn: rgb(0xb5, 0x89, 0x00),
            error: rgb(0xdc, 0x32, 0x2f),
            muted: rgb(0x93, 0xa1, 0xa1),
            border: rgb(0x93, 0xa1, 0xa1),
            border_focus: rgb(0x26, 0x8b, 0xd2),
            selection_bg: rgb(0x26, 0x8b, 0xd2),
            selection_fg: rgb(0xfd, 0xf6, 0xe3),
        },
        Theme {
            name: "gruvbox".into(),
            bg: Color::Reset,
            fg: rgb(0xeb, 0xdb, 0xb2),
            primary: rgb(0x83, 0xa5, 0x98),
            accent: rgb(0xfa, 0xbd, 0x2f),
            success: rgb(0xb8, 0xbb, 0x26),
            warn: rgb(0xfe, 0x80, 0x19),
            error: rgb(0xfb, 0x49, 0x34),
            muted: rgb(0x92, 0x83, 0x74),
            border: rgb(0x50, 0x49, 0x45),
            border_focus: rgb(0xfa, 0xbd, 0x2f),
            selection_bg: rgb(0x45, 0x40, 0x3d),
            selection_fg: rgb(0xfb, 0xf1, 0xc7),
        },
        Theme {
            name: "mono".into(),
            bg: Color::Reset,
            fg: Color::Gray,
            primary: Color::White,
            accent: Color::White,
            success: Color::White,
            warn: Color::White,
            error: Color::White,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            border_focus: Color::White,
            selection_bg: Color::White,
            selection_fg: Color::Black,
        },
        Theme {
            name: "paper".into(),
            bg: Color::Reset,
            fg: rgb(0x2a, 0x2a, 0x33),
            primary: rgb(0x1d, 0x4e, 0xd8),
            accent: rgb(0x7c, 0x3a, 0xed),
            success: rgb(0x15, 0x80, 0x3d),
            warn: rgb(0xb4, 0x53, 0x09),
            error: rgb(0xb9, 0x1c, 0x1c),
            muted: rgb(0x6b, 0x72, 0x80),
            border: rgb(0x9c, 0xa3, 0xaf),
            border_focus: rgb(0x1d, 0x4e, 0xd8),
            selection_bg: rgb(0xdb, 0xea, 0xfe),
            selection_fg: rgb(0x1e, 0x29, 0x3b),
        },
    ]
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// Load all available themes: built-ins plus any user themes found under
/// `<config_dir>/sessionsmith/themes/*.toml`.
pub fn load_all() -> Vec<Theme> {
    let mut all = builtins();
    if let Some(dir) = themes_dir() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(t) = toml::from_str::<ThemeFile>(&text) {
                        let default_name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "custom".into());
                        all.push(t.into_theme(default_name));
                    }
                }
            }
        }
    }
    all
}

/// Resolve a theme by name, falling back to the first built-in.
pub fn resolve(name: &str) -> (Vec<Theme>, usize) {
    let all = load_all();
    let idx = all.iter().position(|t| t.name == name).unwrap_or(0);
    (all, idx)
}

fn themes_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("sessionsmith").join("themes"))
}

/// Serde representation of a user theme file. All colours are `#rrggbb` hex or
/// named ratatui colours; missing roles inherit sensible defaults.
#[derive(Debug, Deserialize)]
struct ThemeFile {
    name: Option<String>,
    fg: Option<String>,
    primary: Option<String>,
    accent: Option<String>,
    success: Option<String>,
    warn: Option<String>,
    error: Option<String>,
    muted: Option<String>,
    border: Option<String>,
    border_focus: Option<String>,
    selection_bg: Option<String>,
    selection_fg: Option<String>,
}

impl ThemeFile {
    fn into_theme(self, default_name: String) -> Theme {
        let base = builtins().remove(0); // midnight as the fallback palette
        Theme {
            name: self.name.unwrap_or(default_name),
            bg: Color::Reset,
            fg: parse_color(self.fg).unwrap_or(base.fg),
            primary: parse_color(self.primary).unwrap_or(base.primary),
            accent: parse_color(self.accent).unwrap_or(base.accent),
            success: parse_color(self.success).unwrap_or(base.success),
            warn: parse_color(self.warn).unwrap_or(base.warn),
            error: parse_color(self.error).unwrap_or(base.error),
            muted: parse_color(self.muted).unwrap_or(base.muted),
            border: parse_color(self.border).unwrap_or(base.border),
            border_focus: parse_color(self.border_focus).unwrap_or(base.border_focus),
            selection_bg: parse_color(self.selection_bg).unwrap_or(base.selection_bg),
            selection_fg: parse_color(self.selection_fg).unwrap_or(base.selection_fg),
        }
    }
}

fn parse_color(s: Option<String>) -> Option<Color> {
    let s = s?;
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "white" => Some(Color::White),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_theme_is_available() {
        let paper = builtins().into_iter().find(|theme| theme.name == "paper");
        assert!(paper.is_some());
    }
}
