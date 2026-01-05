use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::{CAUTION_AMBER, CRITICAL_RED, HUD_GREEN, INFO_DIM};
use crate::analysis::{FunctionHotspot, analyze_hotspots};
use crate::trace_data::TraceData;

/// Hotspot view showing top functions by sample count
pub struct HotspotView {
    scroll_offset: usize,
    pub selected_index: usize,  // Public for testing
    pub hotspots: Vec<FunctionHotspot>,  // Public for testing
    all_hotspots: Vec<FunctionHotspot>, // Unfiltered list
    filter_active: bool,
}

impl HotspotView {
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

    pub fn get_selected(&self) -> Option<&FunctionHotspot> {
        self.hotspots.get(self.selected_index)
    }

    pub fn apply_filter(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_filter();
            return;
        }

        let query_lower = query.to_lowercase();
        self.hotspots = self.all_hotspots
            .iter()
            .filter(|h| h.name.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();

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

    pub fn is_filtered(&self) -> bool {
        self.filter_active
    }

    pub fn filter_by_workers(&mut self, worker_ids: &[u32], data: &TraceData) {
        use std::collections::HashSet;

        if worker_ids.len() == data.workers.len() {
            // All workers selected, no filtering needed
            self.clear_filter();
            return;
        }

        let worker_set: HashSet<u32> = worker_ids.iter().copied().collect();

        // Rebuild hotspots from filtered events
        let mut function_data: HashMap<String, (HashMap<u32, usize>, Option<String>, Option<u32>)> = HashMap::new();

        for event in &data.events {
            if !worker_set.contains(&event.worker_id) {
                continue;
            }

            let entry = function_data.entry(event.name.clone()).or_insert_with(|| {
                (HashMap::new(), event.file.clone(), event.line)
            });
            *entry.0.entry(event.worker_id).or_insert(0) += 1;
        }

        // Convert to vector and calculate percentages
        let total_samples = data.events.iter().filter(|e| worker_set.contains(&e.worker_id)).count();
        let mut hotspots: Vec<FunctionHotspot> = function_data
            .into_iter()
            .map(|(name, (workers, file, line))| {
                let count: usize = workers.values().sum();
                let percentage = (count as f64 / total_samples as f64) * 100.0;
                FunctionHotspot {
                    name,
                    count,
                    percentage,
                    workers,
                    file,
                    line,
                }
            })
            .collect();

        // Sort by count (descending)
        hotspots.sort_by(|a, b| b.count.cmp(&a.count));

        self.hotspots = hotspots;
        self.filter_active = true;
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn render(&self, f: &mut Frame, area: Rect, _data: &TraceData) {
        let mut lines = vec![];

        lines.push(Line::from(""));

        // Calculate visible area (accounting for borders and header)
        let available_height = area.height.saturating_sub(3) as usize;
        let lines_per_item = 5; // Approximate lines per hotspot entry
        let display_count = (available_height / lines_per_item).min(self.hotspots.len());

        // Ensure selected item is visible
        let mut scroll_offset = self.scroll_offset;
        if self.selected_index >= scroll_offset + display_count {
            scroll_offset = self.selected_index.saturating_sub(display_count - 1);
        }

        // Show top functions (compact for glass cockpit)
        for (display_idx, hotspot) in self.hotspots
            .iter()
            .skip(scroll_offset)
            .take(display_count)
            .enumerate()
        {
            let absolute_idx = scroll_offset + display_idx;
            let is_selected = absolute_idx == self.selected_index;

            // Color based on severity
            let (marker, severity_color) = if hotspot.percentage > 40.0 {
                ("🔴", CRITICAL_RED)
            } else if hotspot.percentage > 20.0 {
                ("🟡", CAUTION_AMBER)
            } else {
                ("🟢", HUD_GREEN)
            };

            // Function name (with special handling for system events)
            let max_name_len = 40;
            let (display_name, info_message) = if hotspot.name == "execution" {
                // Generic name when stack capture failed (e.g., from sched_switch)
                ("⚙️  [scheduler event]".to_string(), Some("Stack trace unavailable"))
            } else if hotspot.name.starts_with("<shared:") {
                // Unresolved shared library address
                ("🔗 [shared library]".to_string(), Some("No debug symbols"))
            } else if (hotspot.name.starts_with("std::") ||
                       hotspot.name.starts_with("core::") ||
                       hotspot.name.starts_with("alloc::")) &&
                      hotspot.file.is_none() {
                // Resolved standard library function without source location
                let short_name = if hotspot.name.len() > max_name_len {
                    format!("{}...", &hotspot.name[..max_name_len - 3])
                } else {
                    hotspot.name.clone()
                };
                (format!("📚 {}", short_name), Some("Rust standard library"))
            } else {
                // Normal user code function
                let name = if hotspot.name.len() > max_name_len {
                    format!("{}...", &hotspot.name[..max_name_len - 3])
                } else {
                    hotspot.name.clone()
                };
                (name, None)
            };

            // Add selection indicator and highlight
            let selection_indicator = if is_selected { "▶ " } else { "  " };
            let name_style = if is_selected {
                Style::default()
                    .fg(severity_color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
                    .fg(severity_color)
                    .add_modifier(Modifier::BOLD)
            };

            lines.push(Line::from(vec![
                Span::styled(selection_indicator, Style::default().fg(CAUTION_AMBER)),
                Span::styled(format!("{}  ", marker), Style::default()),
                Span::styled(display_name, name_style),
            ]));

            // Sample percentage
            lines.push(Line::from(vec![
                Span::raw("       "),
                Span::styled(
                    format!("{:.1}% CPU", hotspot.percentage),
                    Style::default().fg(severity_color),
                ),
            ]));

            // Info message for special cases
            if let Some(msg) = info_message {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled("ℹ️  ", Style::default().fg(INFO_DIM)),
                    Span::styled(
                        msg,
                        Style::default().fg(INFO_DIM),
                    ),
                ]));
            }

            // File:line if available
            if let Some(ref file) = hotspot.file {
                if let Some(line) = hotspot.line {
                    // Extract just filename from path
                    let filename = std::path::Path::new(file)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(file);
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled("📍 ", Style::default().fg(INFO_DIM)),
                        Span::styled(
                            format!("{}:{}", filename, line),
                            Style::default().fg(INFO_DIM),
                        ),
                    ]));
                }
            }

            lines.push(Line::from(""));
        }

        let title = if self.filter_active {
            format!("TOP ISSUES (FILTERED: {} / {})", self.hotspots.len(), self.all_hotspots.len())
        } else {
            "TOP ISSUES".to_string()
        };

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title));

        f.render_widget(paragraph, area);
    }
}
