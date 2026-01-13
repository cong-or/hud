use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::theme::{severity_marker, CAUTION_AMBER, HUD_GREEN, INFO_DIM, SEL_CURSOR};
use crate::analysis::{analyze_hotspots, FunctionHotspot};
use crate::trace_data::TraceData;

// Pure data operations (filtering logic separated from UI state)

/// Filter hotspots by function name (case-insensitive substring match)
fn filter_by_name(hotspots: &[FunctionHotspot], query: &str) -> Vec<FunctionHotspot> {
    if query.is_empty() {
        return hotspots.to_vec();
    }

    let query_lower = query.to_lowercase();
    hotspots.iter().filter(|h| h.name.to_lowercase().contains(&query_lower)).cloned().collect()
}

/// Rebuild hotspots from events filtered by worker IDs
fn rebuild_hotspots_for_workers(data: &TraceData, worker_ids: &[u32]) -> Vec<FunctionHotspot> {
    use std::collections::HashSet;

    let worker_set: HashSet<u32> = worker_ids.iter().copied().collect();

    // Single pass: aggregate function data and count samples
    // Filter out "execution" events (scheduler/idle time)
    let (function_data, total_samples) = data
        .events
        .iter()
        .filter(|event| worker_set.contains(&event.worker_id) && event.name != "execution")
        .fold((HashMap::new(), 0usize), |(mut acc, count), event| {
            let entry = acc
                .entry(event.name.clone())
                .or_insert_with(|| (HashMap::new(), event.file.clone(), event.line));
            *entry.0.entry(event.worker_id).or_insert(0) += 1;
            (acc, count + 1)
        });
    let mut hotspots: Vec<FunctionHotspot> = function_data
        .into_iter()
        .map(|(name, (workers, file, line))| {
            let count: usize = workers.values().sum();
            let percentage =
                if total_samples > 0 { (count as f64 / total_samples as f64) * 100.0 } else { 0.0 };
            FunctionHotspot { name, count, percentage, workers, file, line }
        })
        .collect();

    // Sort by count (descending) - unstable sort is faster
    hotspots.sort_unstable_by_key(|h| std::cmp::Reverse(h.count));

    hotspots
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

    pub fn filter_by_workers(&mut self, worker_ids: &[u32], data: &TraceData) {
        if worker_ids.len() == data.workers.len() {
            // All workers selected, no filtering needed
            self.clear_filter();
            return;
        }

        self.hotspots = rebuild_hotspots_for_workers(data, worker_ids);
        self.filter_active = true;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn render(&self, f: &mut Frame, area: Rect, _data: &TraceData) {
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

            // Selection cursor
            let cursor = if is_selected { SEL_CURSOR } else { "   " };
            let name_style = if is_selected {
                Style::default()
                    .fg(severity_color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(severity_color)
            };

            // Line 1: cursor + marker + name + percentage
            lines.push(Line::from(vec![
                Span::styled(cursor, Style::default().fg(CAUTION_AMBER)),
                Span::raw(" "),
                Span::styled(marker, Style::default().fg(severity_color)),
                Span::raw(" "),
                Span::styled(display_name, name_style),
                Span::styled(
                    format!(" {:>5.1}%", hotspot.percentage),
                    Style::default().fg(severity_color),
                ),
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

        let title = if self.filter_active {
            format!("Hotspots [{}/{}]", self.hotspots.len(), self.all_hotspots.len())
        } else {
            "Hotspots".to_string()
        };

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }
}
