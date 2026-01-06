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
