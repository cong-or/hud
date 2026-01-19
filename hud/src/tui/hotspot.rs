//! Hotspot panel - displays functions ranked by blocking time.
//!
//! # Architecture
//!
//! This module renders the main hotspot list in the TUI. It supports two view modes:
//! - **Functions view**: Individual functions ranked by CPU% (default)
//! - **Files view**: Functions grouped by source file (toggle with 'G' key)
//!
//! # Data Flow
//!
//! ```text
//! TraceData → analyze_hotspots() → FunctionHotspot[] → HotspotView → render()
//!                                         ↓
//!                                  group_by_file() → FileGroup[]
//! ```
//!
//! # Key Types
//!
//! - [`HotspotView`] - Main UI component, holds state (selection, scroll, filter)
//! - [`FunctionHotspot`] - Single function with stats (from analysis module)
//! - [`FileGroup`] - Aggregated stats for all hotspots in one source file
//!
//! # Rendering Pattern
//!
//! Each item takes 2 lines:
//! ```text
//! Line 1: [marker] function_name     42.3%
//! Line 2:          filename:line
//! ```
//!
//! The marker indicates severity: `[-]` green, `[!]` amber, `[X]` red

use std::collections::HashMap;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Frame,
};

use super::format_duration_human;
use super::theme::{severity_marker, CAUTION_AMBER, HUD_GREEN, INFO_DIM, SEL_LEFT, SEL_RIGHT};
use crate::analysis::{analyze_hotspots, FunctionHotspot};
use crate::trace_data::TraceData;

/// View mode for hotspot display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Show individual functions (default)
    #[default]
    Functions,
    /// Show grouped by source file
    Files,
}

/// Aggregated hotspots for a single source file
#[derive(Debug, Clone)]
pub struct FileGroup {
    /// Source file path (or "<unknown>" for functions without debug info)
    pub file: String,
    /// Total percentage across all functions in this file
    pub percentage: f64,
    /// Number of hotspot functions in this file
    pub count: usize,
    /// Individual hotspots in this file
    pub hotspots: Vec<FunctionHotspot>,
}

/// Find the topmost user code file from a hotspot's call stacks.
///
/// When grouping by file, we want to attribute blocking to the USER's code,
/// not the library where blocking actually occurs. For example, if user code
/// calls `serde_json::to_string()` which blocks, we want to group by the
/// user's file, not `serde_json`'s source.
///
/// This walks the call stack looking for the first frame marked as user code.
fn find_user_code_file(hotspot: &FunctionHotspot) -> Option<String> {
    hotspot
        .call_stacks
        .iter()
        .flat_map(|stack| stack.iter())
        .find(|frame| frame.origin.is_user_code() && frame.file.is_some())
        .and_then(|frame| frame.file.clone())
}

/// Group hotspots by user source file (finds caller, not library code)
fn group_by_file(hotspots: &[FunctionHotspot]) -> Vec<FileGroup> {
    let mut groups: HashMap<String, Vec<FunctionHotspot>> = HashMap::new();

    for h in hotspots {
        // Prefer user code file from call stack, fall back to hotspot's own file
        let file = find_user_code_file(h)
            .or_else(|| h.file.clone())
            .unwrap_or_else(|| "<unknown>".to_string());
        groups.entry(file).or_default().push(h.clone());
    }

    let mut result: Vec<FileGroup> = groups
        .into_iter()
        .map(|(file, hotspots)| {
            let percentage = hotspots.iter().map(|h| h.percentage).sum();
            let count = hotspots.len();
            FileGroup { file, percentage, count, hotspots }
        })
        .collect();

    // Sort by percentage descending
    result.sort_by(|a, b| b.percentage.total_cmp(&a.percentage));
    result
}

// Pure data operations (filtering logic separated from UI state)

/// Truncate a string for display, adding "..." if too long
fn truncate_for_display(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

/// Calculate scroll offset to keep selected item visible
fn visible_scroll_offset(selected: usize, current_offset: usize, visible_count: usize) -> usize {
    // Scroll down if selected is beyond visible window
    if selected >= current_offset + visible_count {
        selected.saturating_sub(visible_count - 1)
    } else {
        current_offset
    }
}

/// Calculate how many items fit in the visible area (2 lines per item)
fn visible_item_count(area: Rect, total_items: usize) -> usize {
    let available_height = area.height.saturating_sub(2) as usize;
    (available_height / 2).max(1).min(total_items)
}

/// Render a single item's main line (marker, name, percentage)
fn render_item_line(
    is_selected: bool,
    marker: &'static str,
    severity_color: ratatui::style::Color,
    display_name: &str,
    percentage: f64,
) -> Line<'static> {
    let (sel_l, sel_r) = if is_selected { (SEL_LEFT, SEL_RIGHT) } else { (" ", " ") };
    let base_style = Style::default().fg(severity_color);
    let name_style = if is_selected {
        base_style.add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        base_style
    };

    Line::from(vec![
        Span::styled(sel_l, Style::default().fg(CAUTION_AMBER)),
        Span::styled(marker, Style::default().fg(severity_color)),
        Span::raw(" "),
        Span::styled(display_name.to_string(), name_style),
        Span::styled(format!(" {percentage:>5.1}%"), Style::default().fg(severity_color)),
        Span::styled(sel_r, Style::default().fg(CAUTION_AMBER)),
    ])
}

/// Filter hotspots by function name (case-insensitive substring match)
fn filter_by_name(hotspots: &[FunctionHotspot], query: &str) -> Vec<FunctionHotspot> {
    match query {
        "" => hotspots.to_vec(),
        q => {
            let query_lower = q.to_lowercase();
            hotspots
                .iter()
                .filter(|h| h.name.to_lowercase().contains(&query_lower))
                .cloned()
                .collect()
        }
    }
}

// =============================================================================
// UI COMPONENT
// =============================================================================

/// Hotspot view showing top functions by sample count.
///
/// # State Management
///
/// This component maintains its own UI state separate from the data:
/// - `selected_index` - Currently highlighted item (preserved across data updates)
/// - `scroll_offset` - First visible item (for scrolling long lists)
/// - `filter_active` - Whether search filter is applied
/// - `view_mode` - Functions vs Files grouping
///
/// # Live Updates
///
/// During live profiling, the parent rebuilds this view frequently with new data.
/// To preserve user's position, we keep `all_hotspots` separate from `hotspots`
/// (filtered list), and restore `selected_index` after rebuild.
pub struct HotspotView {
    /// First visible item index (for virtual scrolling)
    scroll_offset: usize,
    /// Currently selected item (highlighted with < > brackets)
    pub selected_index: usize, // Public for testing
    /// Filtered/displayed hotspots (may be subset of `all_hotspots`)
    pub hotspots: Vec<FunctionHotspot>, // Public for testing
    /// Complete unfiltered list (for filter reset)
    all_hotspots: Vec<FunctionHotspot>,
    /// True when search filter is active
    filter_active: bool,
    /// Current display mode (functions or files)
    view_mode: ViewMode,
    /// Pre-computed file groupings (rebuilt when hotspots change)
    file_groups: Vec<FileGroup>,
}

impl HotspotView {
    #[must_use]
    pub fn new(data: &TraceData) -> Self {
        // Use analysis module to compute hotspots
        let hotspots = analyze_hotspots(data);

        let file_groups = group_by_file(&hotspots);
        Self {
            scroll_offset: 0,
            selected_index: 0,
            all_hotspots: hotspots.clone(),
            hotspots,
            filter_active: false,
            view_mode: ViewMode::default(),
            file_groups,
        }
    }

    /// Create a `HotspotView` from pre-computed hotspots (e.g., from `HotspotStats`)
    #[must_use]
    pub fn from_hotspots(hotspots: Vec<FunctionHotspot>) -> Self {
        let file_groups = group_by_file(&hotspots);
        Self {
            scroll_offset: 0,
            selected_index: 0,
            all_hotspots: hotspots.clone(),
            hotspots,
            filter_active: false,
            view_mode: ViewMode::default(),
            file_groups,
        }
    }

    /// Toggle between function and file view modes
    pub fn toggle_view(&mut self) {
        self.set_view_mode(match self.view_mode {
            ViewMode::Functions => ViewMode::Files,
            ViewMode::Files => ViewMode::Functions,
        });
    }

    /// Set the view mode directly
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        if self.view_mode != mode {
            self.view_mode = mode;
            self.selected_index = 0;
            self.scroll_offset = 0;
        }
    }

    /// Get the current view mode
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn scroll_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
        // Adjust scroll if selection goes above visible area
        self.scroll_offset = self.scroll_offset.min(self.selected_index);
    }

    pub fn scroll_down(&mut self) {
        let max_index = match self.view_mode {
            ViewMode::Functions => self.hotspots.len(),
            ViewMode::Files => self.file_groups.len(),
        };
        // Increment if not at end (saturating to max - 1)
        self.selected_index = (self.selected_index + 1).min(max_index.saturating_sub(1));
    }

    /// Get selected file group (when in Files view mode)
    #[must_use]
    pub fn get_selected_file_group(&self) -> Option<&FileGroup> {
        self.file_groups.get(self.selected_index)
    }

    #[must_use]
    pub fn get_selected(&self) -> Option<&FunctionHotspot> {
        self.hotspots.get(self.selected_index)
    }

    pub fn apply_filter(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_filter();
            return;
        }

        self.hotspots = filter_by_name(&self.all_hotspots, query);
        self.file_groups = group_by_file(&self.hotspots);
        self.filter_active = true;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn clear_filter(&mut self) {
        self.hotspots = self.all_hotspots.clone();
        self.file_groups = group_by_file(&self.hotspots);
        self.filter_active = false;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    #[must_use]
    pub fn is_filtered(&self) -> bool {
        self.filter_active
    }

    pub fn render(&self, f: &mut Frame, area: Rect, data: &TraceData) {
        let lines = match self.view_mode {
            ViewMode::Functions => self.render_functions_view(area),
            ViewMode::Files => self.render_files_view(area),
        };

        // Format duration for title
        let duration_str = format_duration_human(data.duration);
        let view_indicator = match self.view_mode {
            ViewMode::Functions => "",
            ViewMode::Files => " FILES",
        };
        let title = if self.filter_active {
            let shown = self.hotspots.len();
            let total = self.all_hotspots.len();
            format!("[ HOTSPOTS{view_indicator} {duration_str} {shown}/{total} ]")
        } else {
            format!("[ HOTSPOTS{view_indicator} {duration_str} ]")
        };

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .title(title)
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }

    fn render_functions_view(&self, area: Rect) -> Vec<Line<'static>> {
        let mut lines = vec![];
        let max_name_len = (area.width as usize).saturating_sub(20).min(50);

        // 2 lines per item: name+pct, location
        let display_count = visible_item_count(area, self.hotspots.len());
        let scroll_offset =
            visible_scroll_offset(self.selected_index, self.scroll_offset, display_count);

        for (display_idx, hotspot) in
            self.hotspots.iter().skip(scroll_offset).take(display_count).enumerate()
        {
            let is_selected = scroll_offset + display_idx == self.selected_index;
            let (marker, severity_color) = severity_marker(hotspot.percentage);
            let display_name = truncate_for_display(&hotspot.name, max_name_len);

            // Line 1: <marker name percentage>
            lines.push(render_item_line(
                is_selected,
                marker,
                severity_color,
                &display_name,
                hotspot.percentage,
            ));

            // Line 2: location (if available) or sample count
            let detail = hotspot
                .file
                .as_ref()
                .and_then(|file| {
                    hotspot.line.map(|line| {
                        let filename = std::path::Path::new(file)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(file);
                        format!("{filename}:{line}")
                    })
                })
                .unwrap_or_else(|| format!("{} samples", hotspot.count));

            lines.push(Line::from(vec![
                Span::raw("        "),
                Span::styled(detail, Style::default().fg(INFO_DIM)),
            ]));
        }

        lines
    }

    fn render_files_view(&self, area: Rect) -> Vec<Line<'static>> {
        let mut lines = vec![];
        let max_name_len = (area.width as usize).saturating_sub(20).min(50);

        // 2 lines per item: file+pct, function count
        let display_count = visible_item_count(area, self.file_groups.len());
        let scroll_offset =
            visible_scroll_offset(self.selected_index, self.scroll_offset, display_count);

        for (display_idx, group) in
            self.file_groups.iter().skip(scroll_offset).take(display_count).enumerate()
        {
            let is_selected = scroll_offset + display_idx == self.selected_index;
            let (marker, severity_color) = severity_marker(group.percentage);

            // Extract just the filename from the path
            let display_file = std::path::Path::new(&group.file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&group.file);
            let display_name = truncate_for_display(display_file, max_name_len);

            // Line 1: <marker filename percentage>
            lines.push(render_item_line(
                is_selected,
                marker,
                severity_color,
                &display_name,
                group.percentage,
            ));

            // Line 2: function count
            let fn_label = match group.count {
                1 => "function",
                _ => "functions",
            };
            lines.push(Line::from(vec![
                Span::raw("        "),
                Span::styled(format!("{} {fn_label}", group.count), Style::default().fg(INFO_DIM)),
            ]));
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::FunctionHotspot;

    fn make_hotspot(name: &str, file: Option<&str>, percentage: f64) -> FunctionHotspot {
        FunctionHotspot {
            name: name.to_string(),
            count: 100,
            percentage,
            file: file.map(String::from),
            line: Some(10),
            workers: std::collections::HashMap::new(),
            call_stacks: vec![],
        }
    }

    #[test]
    fn test_group_by_file() {
        let hotspots = vec![
            make_hotspot("foo", Some("src/main.rs"), 30.0),
            make_hotspot("bar", Some("src/main.rs"), 20.0),
            make_hotspot("baz", Some("src/lib.rs"), 15.0),
            make_hotspot("qux", None, 10.0),
        ];

        let groups = group_by_file(&hotspots);

        assert_eq!(groups.len(), 3);
        // Should be sorted by percentage descending
        assert_eq!(groups[0].file, "src/main.rs");
        assert!((groups[0].percentage - 50.0).abs() < 0.01);
        assert_eq!(groups[0].count, 2);

        assert_eq!(groups[1].file, "src/lib.rs");
        assert!((groups[1].percentage - 15.0).abs() < 0.01);
        assert_eq!(groups[1].count, 1);

        assert_eq!(groups[2].file, "<unknown>");
        assert!((groups[2].percentage - 10.0).abs() < 0.01);
        assert_eq!(groups[2].count, 1);
    }

    #[test]
    fn test_view_mode_toggle() {
        let hotspots = vec![make_hotspot("foo", Some("src/main.rs"), 50.0)];
        let mut view = HotspotView::from_hotspots(hotspots);

        assert_eq!(view.view_mode(), ViewMode::Functions);

        view.toggle_view();
        assert_eq!(view.view_mode(), ViewMode::Files);

        view.toggle_view();
        assert_eq!(view.view_mode(), ViewMode::Functions);
    }

    #[test]
    fn test_scroll_respects_view_mode() {
        let hotspots = vec![
            make_hotspot("foo", Some("src/a.rs"), 40.0),
            make_hotspot("bar", Some("src/b.rs"), 30.0),
            make_hotspot("baz", Some("src/c.rs"), 20.0),
        ];
        let mut view = HotspotView::from_hotspots(hotspots);

        // In functions mode, 3 items
        assert_eq!(view.selected_index, 0);
        view.scroll_down();
        view.scroll_down();
        assert_eq!(view.selected_index, 2);
        view.scroll_down(); // Should not go beyond
        assert_eq!(view.selected_index, 2);

        // Switch to files mode - also 3 files, resets selection
        view.toggle_view();
        assert_eq!(view.selected_index, 0);
        view.scroll_down();
        view.scroll_down();
        assert_eq!(view.selected_index, 2);
    }
}
