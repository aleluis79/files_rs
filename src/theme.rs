use ratatui::prelude::Color;

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
}

/// Load a theme by name. Falls back to dark theme if name is invalid.
pub fn load_theme(name: &str) -> ThemeColors {
    match name.to_lowercase().as_str() {
        "light" => ThemeColors::light(),
        "solarized" => ThemeColors::solarized(),
        "dark" => ThemeColors::dark(),
        _ => {
            eprintln!("Theme '{}' not found; using dark", name);
            ThemeColors::dark()
        }
    }
}
