use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::{TraceData, CAUTION_AMBER, CRITICAL_RED, HUD_GREEN, INFO_DIM};

/// Hotspot view showing top functions by sample count
pub struct HotspotView {
    scroll_offset: usize,
    hotspots: Vec<FunctionHotspot>,
}

#[derive(Debug, Clone)]
struct FunctionHotspot {
    name: String,
    count: usize,
    percentage: f64,
    workers: HashMap<u32, usize>, // worker_id -> count
    file: Option<String>,
    line: Option<u32>,
}

impl HotspotView {
    pub fn new(data: &TraceData) -> Self {
        // Aggregate events by function name, capturing file/line from first occurrence
        let mut function_data: HashMap<String, (HashMap<u32, usize>, Option<String>, Option<u32>)> = HashMap::new();

        for event in &data.events {
            let entry = function_data.entry(event.name.clone()).or_insert_with(|| {
                (HashMap::new(), event.file.clone(), event.line)
            });
            *entry.0.entry(event.worker_id).or_insert(0) += 1;
        }

        // Convert to vector and calculate percentages
        let total_samples = data.events.len();
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

        Self {
            scroll_offset: 0,
            hotspots,
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn render(&self, f: &mut Frame, area: Rect, _data: &TraceData) {
        let mut lines = vec![];

        lines.push(Line::from(""));

        // Show top functions (compact for glass cockpit)
        let display_count = 8.min(self.hotspots.len());
        for (idx, hotspot) in self.hotspots.iter().take(display_count).enumerate() {
            let rank = idx + 1;

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

            lines.push(Line::from(vec![
                Span::styled(format!(" {}  ", marker), Style::default()),
                Span::styled(
                    display_name,
                    Style::default().fg(severity_color).add_modifier(Modifier::BOLD),
                ),
            ]));

            // Sample percentage
            lines.push(Line::from(vec![
                Span::raw("     "),
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
                    let filename = file.rsplit('/').next().unwrap_or(file);
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

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("TOP ISSUES"));

        f.render_widget(paragraph, area);
    }
}
