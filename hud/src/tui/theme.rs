//! TUI color theme
//!
//! F-35 glass cockpit inspired color scheme

use ratatui::style::Color;

// F-35 HUD color scheme
pub const HUD_GREEN: Color = Color::Rgb(0, 255, 0);
pub const HUD_CYAN: Color = Color::Rgb(0, 255, 255);
pub const CYAN_DIM: Color = Color::Rgb(0, 180, 180);
pub const CRITICAL_RED: Color = Color::Rgb(255, 0, 0);
pub const CAUTION_AMBER: Color = Color::Rgb(255, 191, 0);
pub const INFO_DIM: Color = Color::Rgb(0, 180, 0);

// Tactical symbols (ASCII-only for terminal compatibility)
pub const SEL_LEFT: &str = "<";
pub const SEL_RIGHT: &str = ">";
pub const MARKER_CRIT: &str = "[X]";
pub const MARKER_WARN: &str = "[!]";
pub const MARKER_OK: &str = "[-]";
pub const BAR_FULL: &str = "|";
pub const BAR_EMPTY: &str = " ";

/// Get tactical severity marker and color
#[must_use]
pub fn severity_marker(percentage: f64) -> (&'static str, Color) {
    if percentage > 40.0 {
        (MARKER_CRIT, CRITICAL_RED)
    } else if percentage > 20.0 {
        (MARKER_WARN, CAUTION_AMBER)
    } else {
        (MARKER_OK, HUD_GREEN)
    }
}

/// Generate a horizontal gauge bar
#[must_use]
pub fn gauge_bar(percentage: f64, width: usize) -> String {
    let filled = ((percentage / 100.0) * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", BAR_FULL.repeat(filled), BAR_EMPTY.repeat(empty))
}
