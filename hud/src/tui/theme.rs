//! TUI color theme and visual constants.
//!
//! # Design Philosophy
//!
//! Inspired by F-35 glass cockpit HUDs - high contrast colors on dark background
//! for quick pattern recognition:
//!
//! - **Green**: Normal/healthy state, primary text
//! - **Cyan**: Identifiers, labels, worker IDs
//! - **Amber**: Warnings, caution states, selection highlights
//! - **Red**: Critical issues requiring immediate attention
//!
//! # Severity Thresholds
//!
//! CPU blocking percentages use consistent thresholds throughout the UI:
//!
//! | Range     | Color  | Marker | Meaning                    |
//! |-----------|--------|--------|----------------------------|
//! | 0-20%     | Green  | `[-]`  | Nominal, healthy           |
//! | 20-40%    | Amber  | `[!]`  | Caution, worth monitoring  |
//! | 40%+      | Red    | `[X]`  | Critical, needs attention  |
//!
//! # ASCII Compatibility
//!
//! All symbols are ASCII-only to ensure they render correctly in any terminal,
//! regardless of font or Unicode support.

use ratatui::style::Color;

// =============================================================================
// F-35 HUD COLOR PALETTE
// =============================================================================

/// Primary color for healthy/nominal state (bright green)
pub const HUD_GREEN: Color = Color::Rgb(0, 255, 0);
/// Identifier/label color (bright cyan)
pub const HUD_CYAN: Color = Color::Rgb(0, 255, 255);
/// Secondary cyan for less important identifiers
pub const CYAN_DIM: Color = Color::Rgb(0, 180, 180);
/// Critical alert color (bright red)
pub const CRITICAL_RED: Color = Color::Rgb(255, 0, 0);
/// Warning/caution color (amber/yellow)
pub const CAUTION_AMBER: Color = Color::Rgb(255, 191, 0);
/// Dimmed text for labels and secondary info
pub const INFO_DIM: Color = Color::Rgb(0, 180, 0);

// =============================================================================
// TACTICAL SYMBOLS (ASCII-only for terminal compatibility)
// =============================================================================

/// Selection bracket left (highlights current item)
pub const SEL_LEFT: &str = "<";
/// Selection bracket right
pub const SEL_RIGHT: &str = ">";
/// Critical severity marker (> 40% blocking)
pub const MARKER_CRIT: &str = "[X]";
/// Warning severity marker (20-40% blocking)
pub const MARKER_WARN: &str = "[!]";
/// OK/nominal marker (< 20% blocking)
pub const MARKER_OK: &str = "[-]";
/// Filled portion of gauge bar
pub const BAR_FULL: &str = "|";
/// Empty portion of gauge bar
pub const BAR_EMPTY: &str = " ";

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Get tactical severity marker and color based on CPU percentage thresholds.
///
/// Returns a tuple of (`marker_string`, `color`) for consistent severity display
/// across all panels. Used by hotspot list, drilldown, etc.
///
/// # Thresholds
/// - `> 40%`: Critical (red, `[X]`)
/// - `> 20%`: Warning (amber, `[!]`)
/// - Otherwise: OK (green, `[-]`)
#[must_use]
pub fn severity_marker(percentage: f64) -> (&'static str, Color) {
    match percentage {
        p if p > 40.0 => (MARKER_CRIT, CRITICAL_RED),
        p if p > 20.0 => (MARKER_WARN, CAUTION_AMBER),
        _ => (MARKER_OK, HUD_GREEN),
    }
}

/// Get color based on warning threshold (>50% = amber, else green).
///
/// Used for worker load gauges where 50% blocking is the concern threshold.
/// This is a `const fn` for zero runtime cost.
#[must_use]
pub const fn warning_color(percentage: f64) -> Color {
    if percentage > 50.0 {
        CAUTION_AMBER
    } else {
        HUD_GREEN
    }
}

/// Get color based on boolean warning state.
///
/// Convenience function for styling based on a pre-computed warning flag.
/// This is a `const fn` for zero runtime cost.
#[must_use]
pub const fn status_color(has_warning: bool) -> Color {
    if has_warning {
        CAUTION_AMBER
    } else {
        HUD_GREEN
    }
}

/// Generate a horizontal gauge bar like `[||||      ]`.
///
/// # Arguments
/// - `percentage`: Value from 0-100
/// - `width`: Number of characters inside the brackets
///
/// # Example
/// ```ignore
/// gauge_bar(40.0, 10) // Returns "[||||      ]"
/// ```
#[must_use]
pub fn gauge_bar(percentage: f64, width: usize) -> String {
    let filled = ((percentage / 100.0) * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", BAR_FULL.repeat(filled), BAR_EMPTY.repeat(empty))
}
