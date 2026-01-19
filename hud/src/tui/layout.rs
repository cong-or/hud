//! Responsive layout engine for the TUI.
//!
//! Adapts the layout based on terminal dimensions to ensure usability
//! at various sizes, from minimal (60x12) to full-screen.

use ratatui::layout::Constraint;

// Width breakpoints
const WIDTH_SINGLE_COLUMN: u16 = 60; // Below this: stack panels vertically
const WIDTH_NARROW: u16 = 100; // Below this: use tighter column split

// Height breakpoints
const HEIGHT_MINIMAL: u16 = 16; // Below this: header + hotspots only
const HEIGHT_COMPACT: u16 = 24; // Below this: hide workers panel

/// Terminal size classification for layout decisions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalSize {
    /// Height < 16: Header + hotspots only
    Minimal,
    /// Height 16-24: Hide workers panel, keep timeline
    Compact,
    /// Height > 24: Full layout with all panels
    Normal,
}

/// Computed layout configuration based on terminal dimensions.
///
/// Uses independent bools for panel visibility - these are rarely all true/false
/// together, and the explicit flags make render logic clearer than a state machine.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct LayoutConfig {
    /// Terminal size classification
    pub size: TerminalSize,

    /// Whether to show the status summary panel (top-left)
    pub show_status_panel: bool,

    /// Whether to show the workers panel (bottom-left)
    pub show_workers_panel: bool,

    /// Whether to show the status bar (bottom)
    pub show_status_bar: bool,

    /// Stack panels vertically instead of side-by-side
    pub single_column: bool,

    /// Left column percentage (0-100)
    pub left_col_pct: u16,

    /// Right column percentage (0-100)
    pub right_col_pct: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            size: TerminalSize::Normal,
            show_status_panel: true,
            show_workers_panel: true,
            show_status_bar: true,
            single_column: false,
            left_col_pct: 30,
            right_col_pct: 70,
        }
    }
}

impl LayoutConfig {
    /// Column constraints for horizontal splits.
    pub fn col_constraints(&self) -> [Constraint; 2] {
        [Constraint::Percentage(self.left_col_pct), Constraint::Percentage(self.right_col_pct)]
    }
}

/// Compute layout configuration based on terminal dimensions.
///
/// # Breakpoints
///
/// | Terminal Size | Behavior |
/// |---------------|----------|
/// | Width < 60    | Single-column layout (stack panels vertically) |
/// | Width 60-100  | Narrow mode: left panels at 25%, right at 75% |
/// | Width > 100   | Normal mode: 30/70 split |
/// | Height < 16   | Minimal: header + hotspots only |
/// | Height 16-24  | Compact: hide workers panel, keep timeline |
/// | Height > 24   | Full layout |
pub fn compute_layout(width: u16, height: u16) -> LayoutConfig {
    let mut config = LayoutConfig::default();

    // Width breakpoints
    if width < WIDTH_SINGLE_COLUMN {
        config.single_column = true;
        config.show_status_panel = false;
        config.show_workers_panel = false;
    } else if width <= WIDTH_NARROW {
        config.left_col_pct = 25;
        config.right_col_pct = 75;
    }

    // Height breakpoints
    if height < HEIGHT_MINIMAL {
        config.size = TerminalSize::Minimal;
        config.show_status_panel = false;
        config.show_workers_panel = false;
        config.show_status_bar = false;
    } else if height <= HEIGHT_COMPACT {
        config.size = TerminalSize::Compact;
        config.show_workers_panel = false;
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_layout() {
        let config = compute_layout(120, 40);
        assert_eq!(config.size, TerminalSize::Normal);
        assert!(config.show_status_panel);
        assert!(config.show_workers_panel);
        assert!(config.show_status_bar);
        assert!(!config.single_column);
        assert_eq!(config.left_col_pct, 30);
    }

    #[test]
    fn test_narrow_layout() {
        let config = compute_layout(80, 40);
        assert_eq!(config.left_col_pct, 25);
        assert_eq!(config.right_col_pct, 75);
        assert!(!config.single_column);
    }

    #[test]
    fn test_single_column_layout() {
        let config = compute_layout(50, 40);
        assert!(config.single_column);
        assert!(!config.show_status_panel);
        assert!(!config.show_workers_panel);
    }

    #[test]
    fn test_minimal_height() {
        let config = compute_layout(120, 12);
        assert_eq!(config.size, TerminalSize::Minimal);
        assert!(!config.show_status_bar);
        assert!(!config.show_workers_panel);
    }

    #[test]
    fn test_compact_height() {
        let config = compute_layout(120, 20);
        assert_eq!(config.size, TerminalSize::Compact);
        assert!(!config.show_workers_panel);
        assert!(config.show_status_bar);
    }
}
