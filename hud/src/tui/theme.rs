//! TUI color theme
//!
//! HUD-inspired color scheme for the terminal interface

use ratatui::style::Color;

// HUD color scheme (F-35 inspired)
pub const HUD_GREEN: Color = Color::Rgb(0, 255, 0);
pub const CRITICAL_RED: Color = Color::Rgb(255, 0, 0);
pub const CAUTION_AMBER: Color = Color::Rgb(255, 191, 0);
pub const INFO_DIM: Color = Color::Rgb(0, 180, 0);
pub const BACKGROUND: Color = Color::Rgb(0, 20, 0);

/// Get severity color based on percentage threshold
/// - Above 40%: Critical (Red)
/// - Above 20%: Caution (Amber)
/// - Otherwise: Normal (Green)
#[must_use]
#[allow(dead_code)] // Available for future use
pub fn severity_color(percentage: f64) -> Color {
    if percentage > 40.0 {
        CRITICAL_RED
    } else if percentage > 20.0 {
        CAUTION_AMBER
    } else {
        HUD_GREEN
    }
}

/// Get severity marker (emoji) and color based on percentage
/// Returns `(marker, color)` tuple for display
#[must_use]
pub fn severity_marker(percentage: f64) -> (&'static str, Color) {
    if percentage > 40.0 {
        ("🔴", CRITICAL_RED)
    } else if percentage > 20.0 {
        ("🟡", CAUTION_AMBER)
    } else {
        ("🟢", HUD_GREEN)
    }
}
