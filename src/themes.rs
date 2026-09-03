use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemePalette {
    pub id: String,
    pub name: String,
    pub bg: String,
    pub fg: String,
    pub primary: String,
    pub accent: String,
    pub success: String,
    pub warn: String,
    pub error: String,
    pub muted: String,
    pub border: String,
    pub border_focus: String,
    pub selection_bg: String,
    pub selection_fg: String,
}

pub fn load_all() -> Vec<ThemePalette> {
    let directory = dirs::config_dir().map(|path| path.join("sessionsmith/themes"));
    load_from(directory.as_deref())
}

fn load_from(directory: Option<&Path>) -> Vec<ThemePalette> {
    let mut themes = builtins();
    let Some(directory) = directory else {
        return themes;
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return themes;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = toml::from_str::<ThemeFile>(&text) else {
            continue;
        };
        if let Some(theme) = file.into_palette(id, &themes[1]) {
            if let Some(existing) = themes.iter().position(|theme| theme.id == id) {
                themes[existing] = theme;
            } else {
                themes.push(theme);
            }
        }
    }
    themes
}

pub fn builtins() -> Vec<ThemePalette> {
    vec![
        palette(
            "default",
            "SessionSmith default",
            [
                "#eef1e8", "#1d2a1f", "#25643e", "#a96214", "#2f7d49", "#a96214", "#ad423a",
                "#627063", "#d3dacd", "#25643e", "#e0ecdf", "#174b2c",
            ],
        ),
        palette(
            "midnight",
            "Midnight",
            [
                "#171927", "#d0d0e0", "#7aa2f7", "#bb9af7", "#9ece6a", "#e0af68", "#f7768e",
                "#7b84a8", "#3b4261", "#7aa2f7", "#2d3f76", "#ffffff",
            ],
        ),
        palette(
            "solar",
            "Solar",
            [
                "#fdf6e3", "#657b83", "#268bd2", "#d33682", "#859900", "#b58900", "#dc322f",
                "#839496", "#93a1a1", "#268bd2", "#d9edf5", "#073642",
            ],
        ),
        palette(
            "gruvbox",
            "Gruvbox",
            [
                "#282828", "#ebdbb2", "#83a598", "#fabd2f", "#b8bb26", "#fe8019", "#fb4934",
                "#a89984", "#504945", "#fabd2f", "#45403d", "#fbf1c7",
            ],
        ),
        palette(
            "paper",
            "Paper",
            [
                "#f8f7f2", "#2a2a33", "#1d4ed8", "#7c3aed", "#15803d", "#b45309", "#b91c1c",
                "#6b7280", "#9ca3af", "#1d4ed8", "#dbeafe", "#1e293b",
            ],
        ),
    ]
}

fn palette(id: &str, name: &str, values: [&str; 12]) -> ThemePalette {
    ThemePalette {
        id: id.into(),
        name: name.into(),
        bg: values[0].into(),
        fg: values[1].into(),
        primary: values[2].into(),
        accent: values[3].into(),
        success: values[4].into(),
        warn: values[5].into(),
        error: values[6].into(),
        muted: values[7].into(),
        border: values[8].into(),
        border_focus: values[9].into(),
        selection_bg: values[10].into(),
        selection_fg: values[11].into(),
    }
}

#[derive(Debug, Deserialize)]
struct ThemeFile {
    name: Option<String>,
    bg: Option<String>,
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
    fn into_palette(self, id: &str, fallback: &ThemePalette) -> Option<ThemePalette> {
        if id.is_empty()
            || id.chars().any(|character| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
            })
        {
            return None;
        }
        Some(ThemePalette {
            id: id.into(),
            name: self
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| id.into()),
            bg: color(self.bg, &fallback.bg)?,
            fg: color(self.fg, &fallback.fg)?,
            primary: color(self.primary, &fallback.primary)?,
            accent: color(self.accent, &fallback.accent)?,
            success: color(self.success, &fallback.success)?,
            warn: color(self.warn, &fallback.warn)?,
            error: color(self.error, &fallback.error)?,
            muted: color(self.muted, &fallback.muted)?,
            border: color(self.border, &fallback.border)?,
            border_focus: color(self.border_focus, &fallback.border_focus)?,
            selection_bg: color(self.selection_bg, &fallback.selection_bg)?,
            selection_fg: color(self.selection_fg, &fallback.selection_fg)?,
        })
    }
}

fn color(value: Option<String>, fallback: &str) -> Option<String> {
    let value = value.unwrap_or_else(|| fallback.into());
    let bytes = value.as_bytes();
    (bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit))
        .then(|| value.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_themes_inherit_roles_and_reject_unsafe_colors() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("custom.toml"),
            "name = \"Custom\"\naccent = \"#ABCDEF\"",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("unsafe.toml"),
            "accent = \"red; background: url(x)\"",
        )
        .unwrap();
        let themes = load_from(Some(directory.path()));
        let custom = themes.iter().find(|theme| theme.id == "custom").unwrap();
        assert_eq!(custom.accent, "#abcdef");
        assert_eq!(custom.bg, themes[1].bg);
        assert!(!themes.iter().any(|theme| theme.id == "unsafe"));
    }
}
