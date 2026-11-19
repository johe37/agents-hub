//! Color themes, shared by the hub UI and the tmux status bar. Picked once at startup from the config.

use std::sync::OnceLock;

use ratatui::style::Color;

pub struct Theme {
    pub bg: Color,
    pub bar: Color,
    pub border: Color,
    pub fg: Color,
    pub bright: Color,
    pub muted: Color,
    pub dim: Color,
    pub accent: Color,
    /// Text on accent and other strong colored backgrounds.
    pub on_accent: Color,
    pub info: Color,
    pub warning: Color,
    pub error: Color,
    pub success: Color,
    pub purple: Color,
    pub modified: Color,
    pub selection: Color,
    pub input: Color,
    pub panel: Color,
    pub hover: Color,
    pub code: Color,
    pub claude: Color,
}

pub const fn hex(s: &str) -> Color {
    let b = s.as_bytes();
    const fn n(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => c - b'A' + 10,
        }
    }
    Color::Rgb(n(b[1]) * 16 + n(b[2]), n(b[3]) * 16 + n(b[4]), n(b[5]) * 16 + n(b[6]))
}

const CLAUDE: Color = hex("#D97757");

pub const THEMES: &[(&str, Theme)] = &[
    (
        "dark-modern",
        Theme {
            bg: hex("#1F1F1F"),
            bar: hex("#181818"),
            border: hex("#2B2B2B"),
            fg: hex("#CCCCCC"),
            bright: hex("#FFFFFF"),
            muted: hex("#9D9D9D"),
            dim: hex("#6E7681"),
            accent: hex("#0078D4"),
            on_accent: hex("#FFFFFF"),
            info: hex("#3794FF"),
            warning: hex("#CCA700"),
            error: hex("#F85149"),
            success: hex("#2EA043"),
            purple: hex("#C586C0"),
            modified: hex("#E2C08D"),
            selection: hex("#04395E"),
            input: hex("#313131"),
            panel: hex("#222222"),
            hover: hex("#2A2D2E"),
            code: hex("#CE9178"),
            claude: CLAUDE,
        },
    ),
    (
        "light",
        Theme {
            bg: hex("#FFFFFF"),
            bar: hex("#F3F3F3"),
            border: hex("#E5E5E5"),
            fg: hex("#3B3B3B"),
            bright: hex("#000000"),
            muted: hex("#616161"),
            dim: hex("#8B949E"),
            accent: hex("#005FB8"),
            on_accent: hex("#FFFFFF"),
            info: hex("#1A85FF"),
            warning: hex("#BF8803"),
            error: hex("#E51400"),
            success: hex("#388A34"),
            purple: hex("#AF00DB"),
            modified: hex("#895503"),
            selection: hex("#ADD6FF"),
            input: hex("#EDEDED"),
            panel: hex("#F8F8F8"),
            hover: hex("#F0F0F0"),
            code: hex("#A31515"),
            claude: CLAUDE,
        },
    ),
    (
        "catppuccin",
        Theme {
            bg: hex("#1E1E2E"),
            bar: hex("#181825"),
            border: hex("#313244"),
            fg: hex("#CDD6F4"),
            bright: hex("#FFFFFF"),
            muted: hex("#A6ADC8"),
            dim: hex("#6C7086"),
            accent: hex("#89B4FA"),
            on_accent: hex("#1E1E2E"),
            info: hex("#74C7EC"),
            warning: hex("#F9E2AF"),
            error: hex("#F38BA8"),
            success: hex("#A6E3A1"),
            purple: hex("#CBA6F7"),
            modified: hex("#FAB387"),
            selection: hex("#45475A"),
            input: hex("#313244"),
            panel: hex("#181825"),
            hover: hex("#313244"),
            code: hex("#FAB387"),
            claude: CLAUDE,
        },
    ),
    (
        "gruvbox",
        Theme {
            bg: hex("#282828"),
            bar: hex("#1D2021"),
            border: hex("#3C3836"),
            fg: hex("#EBDBB2"),
            bright: hex("#FBF1C7"),
            muted: hex("#A89984"),
            dim: hex("#7C6F64"),
            accent: hex("#83A598"),
            on_accent: hex("#282828"),
            info: hex("#8EC07C"),
            warning: hex("#FABD2F"),
            error: hex("#FB4934"),
            success: hex("#B8BB26"),
            purple: hex("#D3869B"),
            modified: hex("#FE8019"),
            selection: hex("#504945"),
            input: hex("#3C3836"),
            panel: hex("#32302F"),
            hover: hex("#32302F"),
            code: hex("#FE8019"),
            claude: CLAUDE,
        },
    ),
    (
        "tokyo-night",
        Theme {
            bg: hex("#1A1B26"),
            bar: hex("#16161E"),
            border: hex("#292E42"),
            fg: hex("#C0CAF5"),
            bright: hex("#FFFFFF"),
            muted: hex("#A9B1D6"),
            dim: hex("#565F89"),
            accent: hex("#7AA2F7"),
            on_accent: hex("#1A1B26"),
            info: hex("#7DCFFF"),
            warning: hex("#E0AF68"),
            error: hex("#F7768E"),
            success: hex("#9ECE6A"),
            purple: hex("#BB9AF7"),
            modified: hex("#FF9E64"),
            selection: hex("#283457"),
            input: hex("#292E42"),
            panel: hex("#1F2335"),
            hover: hex("#292E42"),
            code: hex("#FF9E64"),
            claude: CLAUDE,
        },
    ),
];

static CURRENT: OnceLock<&'static Theme> = OnceLock::new();

/// The theme in use; the first one until `select` runs.
pub fn colors() -> &'static Theme {
    CURRENT.get().copied().unwrap_or(&THEMES[0].1)
}

pub fn select(name: &str) -> Result<(), String> {
    let Some((_, theme)) = THEMES.iter().find(|(n, _)| *n == name) else {
        let names: Vec<&str> = THEMES.iter().map(|(n, _)| *n).collect();
        return Err(format!("unknown theme '{name}', pick one of: {}", names.join(", ")));
    };
    let _ = CURRENT.set(theme);
    Ok(())
}

/// `#RRGGBB`, for tmux styles.
pub fn css(c: Color) -> String {
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02X}{g:02X}{b:02X}"),
        _ => "default".into(),
    }
}

pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (a, b) else { return b };
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color::Rgb(m(r1, r2), m(g1, g2), m(b1, b2))
}

/// Color at t (wraps at 1.0) along a closed loop of stops.
pub fn gradient(stops: &[Color], t: f32) -> Color {
    let t = t.rem_euclid(1.0) * stops.len() as f32;
    let i = t as usize % stops.len();
    lerp(stops[i], stops[(i + 1) % stops.len()], t.fract())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_ends_and_middle() {
        let (a, b) = (Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50));
        assert_eq!(lerp(a, b, 0.0), a);
        assert_eq!(lerp(a, b, 1.0), b);
        assert_eq!(lerp(a, b, 0.5), Color::Rgb(100, 50, 25));
        assert_eq!(gradient(&[a, b], 1.0), a);
    }

    #[test]
    fn select_and_css() {
        assert!(select("nope").is_err());
        assert_eq!(css(hex("#0a1B2c")), "#0A1B2C");
    }
}

/// `c` mixed into `base` by `amount` (0–1), for soft tinted backgrounds.
pub fn tint(c: Color, base: Color, amount: f32) -> Color {
    let (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) = (c, base) else { return base };
    let mix = |x: u8, y: u8| (y as f32 + (x as f32 - y as f32) * amount).round() as u8;
    Color::Rgb(mix(r, br), mix(g, bg), mix(b, bb))
}
