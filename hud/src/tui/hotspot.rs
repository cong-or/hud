use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Frame,
};

use super::theme::{severity_marker, CAUTION_AMBER, HUD_GREEN, INFO_DIM, SEL_LEFT, SEL_RIGHT};
use crate::analysis::{analyze_hotspots, FunctionHotspot};
use crate::trace_data::TraceData;

/// Format a duration in seconds as a human-readable string (e.g., "2d 4h 23m")
fn format_duration_human(secs: f64) -> String {
    let total_secs = secs as u64;

    if total_secs == 0 {
        return "0s".to_string();
    }

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs_rem = total_secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if mins > 0 {
        parts.push(format!("{mins}m"));
    }
    // Only show seconds if duration is less than an hour
    if secs_rem > 0 && total_secs < 3600 {
        parts.push(format!("{secs_rem}s"));
    }

    if parts.is_empty() {
        "0s".to_string()
    } else {
        parts.join(" ")
    }
}

// Pure data operations (filtering logic separated from UI state)

/// Filter hotspots by function name (case-insensitive substring match)
fn filter_by_name(hotspots: &[FunctionHotspot], query: &str) -> Vec<FunctionHotspot> {
    if query.is_empty() {
        return hotspots.to_vec();
    }

    let query_lower = query.to_lowercase();
    hotspots.iter().filter(|h| h.name.to_lowercase().contains(&query_lower)).cloned().collect()
}

// UI Component

/// Hotspot view showing top functions by sample count
pub struct HotspotView {
    scroll_offset: usize,
    pub selected_index: usize,          // Public for testing
    pub hotspots: Vec<FunctionHotspot>, // Public for testing
    all_hotspots: Vec<FunctionHotspot>, // Unfiltered list
    filter_active: bool,
}

impl HotspotView {
    #[must_use]
    pub fn new(data: &TraceData) -> Self {
        // Use analysis module to compute hotspots
        let hotspots = analyze_hotspots(data);

        Self {
            scroll_offset: 0,
            selected_index: 0,
            all_hotspots: hotspots.clone(),
            hotspots,
            filter_active: false,
        }
    }

    /// Create a `HotspotView` from pre-computed hotspots (e.g., from `HotspotStats`)
    #[must_use]
    pub fn from_hotspots(hotspots: Vec<FunctionHotspot>) -> Self {
        Self {
            scroll_offset: 0,
            selected_index: 0,
            all_hotspots: hotspots.clone(),
            hotspots,
            filter_active: false,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            // Adjust scroll if selection goes above visible area
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if self.selected_index + 1 < self.hotspots.len() {
            self.selected_index += 1;
            // We'll adjust scroll in render based on visible area
        }
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
        self.filter_active = true;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn clear_filter(&mut self) {
        self.hotspots = self.all_hotspots.clone();
        self.filter_active = false;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    #[must_use]
    pub fn is_filtered(&self) -> bool {
        self.filter_active
    }

    pub fn render(&self, f: &mut Frame, area: Rect, data: &TraceData) {
        let mut lines = vec![];

        // Calculate visible items (2 lines per item now: name+pct, location)
        let available_height = area.height.saturating_sub(2) as usize;
        let lines_per_item = 2;
        let display_count = (available_height / lines_per_item).max(1).min(self.hotspots.len());

        // Ensure selected item is visible
        let mut scroll_offset = self.scroll_offset;
        if self.selected_index >= scroll_offset + display_count {
            scroll_offset = self.selected_index.saturating_sub(display_count - 1);
        }

        // Compact tactical display
        for (display_idx, hotspot) in
            self.hotspots.iter().skip(scroll_offset).take(display_count).enumerate()
        {
            let absolute_idx = scroll_offset + display_idx;
            let is_selected = absolute_idx == self.selected_index;

            let (marker, severity_color) = severity_marker(hotspot.percentage);

            // Truncate function name for display
            let max_name_len = (area.width as usize).saturating_sub(20).min(50);
            let display_name = if hotspot.name.len() > max_name_len {
                format!("{}...", &hotspot.name[..max_name_len.saturating_sub(3)])
            } else {
                hotspot.name.clone()
            };

            // Selection brackets
            let (sel_l, sel_r) = if is_selected { (SEL_LEFT, SEL_RIGHT) } else { (" ", " ") };
            let name_style = if is_selected {
                Style::default()
                    .fg(severity_color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(severity_color)
            };

            // Line 1: <marker name percentage>
            lines.push(Line::from(vec![
                Span::styled(sel_l, Style::default().fg(CAUTION_AMBER)),
                Span::styled(marker, Style::default().fg(severity_color)),
                Span::raw(" "),
                Span::styled(display_name, name_style),
                Span::styled(
                    format!(" {:>5.1}%", hotspot.percentage),
                    Style::default().fg(severity_color),
                ),
                Span::styled(sel_r, Style::default().fg(CAUTION_AMBER)),
            ]));

            // Line 2: location (if available) or sample count
            let location = hotspot.file.as_ref().and_then(|file| {
                hotspot.line.map(|line| {
                    let filename = std::path::Path::new(file)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(file);
                    format!("{filename}:{line}")
                })
            });

            let detail = location.unwrap_or_else(|| format!("{} samples", hotspot.count));
            lines.push(Line::from(vec![
                Span::raw("        "),
                Span::styled(detail, Style::default().fg(INFO_DIM)),
            ]));
        }

        // Format duration for title
        let duration_str = format_duration_human(data.duration);
        let title = if self.filter_active {
            let shown = self.hotspots.len();
            let total = self.all_hotspots.len();
            format!("[ HOTSPOTS {duration_str} {shown}/{total} ]")
        } else {
            format!("[ HOTSPOTS {duration_str} ]")
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
}
