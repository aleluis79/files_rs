use std::{fs, path::{Path, PathBuf}};

use anyhow::{Context, Result};
use ratatui::prelude::Color;
use serde::{Deserialize, Serialize};

/// Semantic color palette for the application UI.
/// Each field corresponds to a visual intent rather than a specific color value.
#[derive(Clone, Debug)]
pub struct ThemeColors {
    pub header_fg: Color,
    pub border_active: Color,
    pub border_inactive: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub panel_bg: Color,
    pub panel_fg: Color,
    pub text_normal: Color,
    pub text_dim: Color,
    pub text_accent: Color,
    pub text_success: Color,
    pub text_warning: Color,
    pub text_error: Color,
    pub gauge_fill: Color,
    pub gauge_bg: Color,
    pub status_fg: Color,
}

#[derive(Debug, Serialize, Deserialize)]
struct ThemeFile {
    header_fg: String,
    border_active: String,
    border_inactive: String,
    selected_bg: String,
    selected_fg: String,
    panel_bg: String,
    panel_fg: String,
    text_normal: String,
    text_dim: String,
    text_accent: String,
    text_success: String,
    text_warning: String,
    text_error: String,
    gauge_fill: String,
    gauge_bg: String,
    status_fg: String,
}

impl ThemeColors {
    /// Dark theme matching current hardcoded colors
    pub fn dark() -> Self {
        Self {
            header_fg: Color::Yellow,
            border_active: Color::Green,
            border_inactive: Color::DarkGray,
            selected_bg: Color::Blue,
            selected_fg: Color::White,
            panel_bg: Color::Reset,
            panel_fg: Color::Reset,
            text_normal: Color::White,
            text_dim: Color::Gray,
            text_accent: Color::Cyan,
            text_success: Color::Green,
            text_warning: Color::Yellow,
            text_error: Color::LightRed,
            gauge_fill: Color::Green,
            gauge_bg: Color::Black,
            status_fg: Color::Cyan,
        }
    }

    /// Light theme with high contrast
    pub fn light() -> Self {
        Self {
            header_fg: Color::DarkGray,
            border_active: Color::Blue,
            border_inactive: Color::Gray,
            selected_bg: Color::Blue,
            selected_fg: Color::White,
            panel_bg: Color::White,
            panel_fg: Color::Black,
            text_normal: Color::Black,
            text_dim: Color::DarkGray,
            text_accent: Color::Blue,
            text_success: Color::Green,
            text_warning: Color::Yellow,
            text_error: Color::Red,
            gauge_fill: Color::Blue,
            gauge_bg: Color::Gray,
            status_fg: Color::Blue,
        }
    }

    /// Solarized theme using base16 palette
    pub fn solarized() -> Self {
        Self {
            header_fg: Color::Yellow,
            border_active: Color::Cyan,
            border_inactive: Color::DarkGray,
            selected_bg: Color::Blue,
            selected_fg: Color::White,
            panel_bg: Color::Reset,
            panel_fg: Color::Reset,
            text_normal: Color::White,
            text_dim: Color::DarkGray,
            text_accent: Color::Cyan,
            text_success: Color::Green,
            text_warning: Color::Yellow,
            text_error: Color::Red,
            gauge_fill: Color::Cyan,
            gauge_bg: Color::Black,
            status_fg: Color::Cyan,
        }
    }

    fn builtin(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "light" => Some(Self::light()),
            "solarized" => Some(Self::solarized()),
            "dark" => Some(Self::dark()),
            _ => None,
        }
    }

    fn to_file(&self) -> ThemeFile {
        ThemeFile {
            header_fg: serialize_color(self.header_fg),
            border_active: serialize_color(self.border_active),
            border_inactive: serialize_color(self.border_inactive),
            selected_bg: serialize_color(self.selected_bg),
            selected_fg: serialize_color(self.selected_fg),
            panel_bg: serialize_color(self.panel_bg),
            panel_fg: serialize_color(self.panel_fg),
            text_normal: serialize_color(self.text_normal),
            text_dim: serialize_color(self.text_dim),
            text_accent: serialize_color(self.text_accent),
            text_success: serialize_color(self.text_success),
            text_warning: serialize_color(self.text_warning),
            text_error: serialize_color(self.text_error),
            gauge_fill: serialize_color(self.gauge_fill),
            gauge_bg: serialize_color(self.gauge_bg),
            status_fg: serialize_color(self.status_fg),
        }
    }
}

impl ThemeFile {
    fn parse(self) -> Result<ThemeColors> {
        Ok(ThemeColors {
            header_fg: parse_color_field("header_fg", &self.header_fg)?,
            border_active: parse_color_field("border_active", &self.border_active)?,
            border_inactive: parse_color_field("border_inactive", &self.border_inactive)?,
            selected_bg: parse_color_field("selected_bg", &self.selected_bg)?,
            selected_fg: parse_color_field("selected_fg", &self.selected_fg)?,
            panel_bg: parse_color_field("panel_bg", &self.panel_bg)?,
            panel_fg: parse_color_field("panel_fg", &self.panel_fg)?,
            text_normal: parse_color_field("text_normal", &self.text_normal)?,
            text_dim: parse_color_field("text_dim", &self.text_dim)?,
            text_accent: parse_color_field("text_accent", &self.text_accent)?,
            text_success: parse_color_field("text_success", &self.text_success)?,
            text_warning: parse_color_field("text_warning", &self.text_warning)?,
            text_error: parse_color_field("text_error", &self.text_error)?,
            gauge_fill: parse_color_field("gauge_fill", &self.gauge_fill)?,
            gauge_bg: parse_color_field("gauge_bg", &self.gauge_bg)?,
            status_fg: parse_color_field("status_fg", &self.status_fg)?,
        })
    }
}

fn parse_color_field(field_name: &str, value: &str) -> Result<Color> {
    parse_color(value).with_context(|| format!("Color invalido en {}: '{}'", field_name, value))
}

fn parse_color(value: &str) -> Option<Color> {
    let normalized = value.trim();
    if let Some(hex) = normalized.strip_prefix('#') {
        if hex.len() == 6 {
            let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(red, green, blue));
        }
    }

    if let Some(indexed) = normalized
        .strip_prefix("ansi(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return indexed.parse::<u8>().ok().map(Color::Indexed);
    }

    match normalized.to_lowercase().as_str() {
        "reset" => Some(Color::Reset),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "dark_gray" | "dark-grey" | "darkgrey" => Some(Color::DarkGray),
        "lightred" | "light_red" | "light-red" => Some(Color::LightRed),
        "lightgreen" | "light_green" | "light-green" => Some(Color::LightGreen),
        "lightyellow" | "light_yellow" | "light-yellow" => Some(Color::LightYellow),
        "lightblue" | "light_blue" | "light-blue" => Some(Color::LightBlue),
        "lightmagenta" | "light_magenta" | "light-magenta" => Some(Color::LightMagenta),
        "lightcyan" | "light_cyan" | "light-cyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        _ => None,
    }
}

fn serialize_color(color: Color) -> String {
    match color {
        Color::Reset => "reset".to_string(),
        Color::Black => "black".to_string(),
        Color::Red => "red".to_string(),
        Color::Green => "green".to_string(),
        Color::Yellow => "yellow".to_string(),
        Color::Blue => "blue".to_string(),
        Color::Magenta => "magenta".to_string(),
        Color::Cyan => "cyan".to_string(),
        Color::Gray => "gray".to_string(),
        Color::DarkGray => "darkgray".to_string(),
        Color::LightRed => "lightred".to_string(),
        Color::LightGreen => "lightgreen".to_string(),
        Color::LightYellow => "lightyellow".to_string(),
        Color::LightBlue => "lightblue".to_string(),
        Color::LightMagenta => "lightmagenta".to_string(),
        Color::LightCyan => "lightcyan".to_string(),
        Color::White => "white".to_string(),
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        Color::Indexed(index) => format!("ansi({index})"),
    }
}

fn ensure_builtin_theme_files(themes_dir: &Path) -> Result<()> {
    fs::create_dir_all(themes_dir)
        .with_context(|| format!("No se pudo crear {}", themes_dir.display()))?;

    for name in ["dark", "light", "solarized"] {
        let path = themes_dir.join(format!("{name}.toml"));
        if path.exists() {
            continue;
        }

        let builtin = ThemeColors::builtin(name).expect("builtin theme must exist");
        let serialized = toml::to_string_pretty(&builtin.to_file())
            .with_context(|| format!("No se pudo serializar tema builtin '{name}'"))?;
        fs::write(&path, serialized)
            .with_context(|| format!("No se pudo escribir {}", path.display()))?;
    }

    Ok(())
}

fn resolve_theme_path(name: &str, themes_dir: &Path) -> PathBuf {
    let candidate = PathBuf::from(name);
    if candidate.is_absolute() {
        return candidate;
    }

    if candidate.components().count() > 1 || name.ends_with(".toml") {
        return themes_dir.join(candidate);
    }

    themes_dir.join(format!("{name}.toml"))
}

/// Load a theme from a TOML file. Falls back to the matching builtin theme or dark.
pub fn load_theme(name: &str, themes_dir: &Path) -> ThemeColors {
    if let Err(error) = ensure_builtin_theme_files(themes_dir) {
        eprintln!("No se pudieron preparar los temas builtin: {error:#}");
    }

    let path = resolve_theme_path(name, themes_dir);
    match fs::read_to_string(&path)
        .with_context(|| format!("No se pudo leer {}", path.display()))
        .and_then(|content| {
            toml::from_str::<ThemeFile>(&content)
                .with_context(|| format!("No se pudo parsear {}", path.display()))
        })
        .and_then(ThemeFile::parse)
    {
        Ok(theme) => theme,
        Err(error) => {
            if let Some(builtin) = ThemeColors::builtin(name) {
                eprintln!("No se pudo cargar el tema '{}' desde {}: {error:#}. Usando builtin.", name, path.display());
                builtin
            } else {
                eprintln!("No se pudo cargar el tema '{}' desde {}: {error:#}. Usando dark.", name, path.display());
                ThemeColors::dark()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("files-rs-theme-test-{unique}"));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn loads_custom_theme_from_themes_dir() {
        let themes_dir = make_temp_dir();
        let theme_path = themes_dir.join("mytheme.toml");

        fs::write(
            &theme_path,
            r#"header_fg = "yellow"
border_active = "yellow"
border_inactive = "darkgray"
selected_bg = "blue"
selected_fg = "white"
panel_bg = "reset"
panel_fg = "reset"
text_normal = "white"
text_dim = "darkgray"
text_accent = "cyan"
text_success = "green"
text_warning = "yellow"
text_error = "red"
gauge_fill = "cyan"
gauge_bg = "black"
status_fg = "red"
"#,
        )
        .expect("write theme file");

        let theme = load_theme("mytheme", &themes_dir);

        assert_eq!(theme.border_active, Color::Yellow);
        assert_eq!(theme.text_error, Color::Red);
        assert_eq!(theme.gauge_fill, Color::Cyan);
        assert_eq!(theme.status_fg, Color::Red);

        fs::remove_dir_all(themes_dir).expect("remove temp dir");
    }

    #[test]
    fn falls_back_to_dark_for_invalid_custom_theme() {
        let themes_dir = make_temp_dir();
        let theme_path = themes_dir.join("broken.toml");

        fs::write(
            &theme_path,
            r#"header_fg = "not-a-color"
border_active = "yellow"
border_inactive = "darkgray"
selected_bg = "blue"
selected_fg = "white"
panel_bg = "reset"
panel_fg = "reset"
text_normal = "white"
text_dim = "darkgray"
text_accent = "cyan"
text_success = "green"
text_warning = "yellow"
text_error = "red"
gauge_fill = "cyan"
gauge_bg = "black"
status_fg = "red"
"#,
        )
        .expect("write broken theme file");

        let theme = load_theme("broken", &themes_dir);

        assert_eq!(theme.border_active, ThemeColors::dark().border_active);
        assert_eq!(theme.text_error, ThemeColors::dark().text_error);

        fs::remove_dir_all(themes_dir).expect("remove temp dir");
    }
}
