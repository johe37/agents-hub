//! VSCode Dark Modern colors, shared by the hub UI and the tmux status bar.

use ratatui::style::Color;

pub const BG: &str = "#1F1F1F";
pub const BAR: &str = "#181818";
pub const BORDER: &str = "#2B2B2B";
pub const FG: &str = "#CCCCCC";
pub const BRIGHT: &str = "#FFFFFF";
pub const MUTED: &str = "#9D9D9D";
pub const DIM: &str = "#6E7681";
pub const ACCENT: &str = "#0078D4";
pub const INFO: &str = "#3794FF";
pub const WARNING: &str = "#CCA700";

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

pub const C_BG: Color = hex(BG);
pub const C_BAR: Color = hex(BAR);
pub const C_BORDER: Color = hex(BORDER);
pub const C_FG: Color = hex(FG);
pub const C_BRIGHT: Color = hex(BRIGHT);
pub const C_MUTED: Color = hex(MUTED);
pub const C_DIM: Color = hex(DIM);
pub const C_ACCENT: Color = hex(ACCENT);
pub const C_INFO: Color = hex(INFO);
pub const C_WARNING: Color = hex(WARNING);
pub const C_ERROR: Color = hex("#F85149");
pub const C_SUCCESS: Color = hex("#2EA043");
pub const C_PURPLE: Color = hex("#C586C0");
pub const C_MODIFIED: Color = hex("#E2C08D");
pub const C_SELECTION: Color = hex("#04395E");
pub const C_INPUT: Color = hex("#313131");
pub const C_PANEL: Color = hex("#222222");
pub const C_CODE: Color = hex("#CE9178");

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
}
