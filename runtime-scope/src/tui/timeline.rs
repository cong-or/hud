use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::TraceData;

/// Timeline view showing worker activity over time
pub struct TimelineView {
    scroll_offset: usize,
    worker_stats: HashMap<u32, WorkerStats>,
    zoom_level: f64,    // 1.0 = normal, >1.0 = zoomed in
    pan_offset: f64,    // 0.0 to 1.0, position in timeline
}

#[derive(Debug, Clone)]
struct WorkerStats {
    total_samples: usize,
    samples_with_functions: usize,
    tid: u32,
}

impl TimelineView {
    pub fn new(data: &TraceData) -> Self {
        // Calculate statistics for each worker
        let mut worker_stats: HashMap<u32, WorkerStats> = HashMap::new();

        for event in &data.events {
            let stats = worker_stats.entry(event.worker_id).or_insert(WorkerStats {
                total_samples: 0,
                samples_with_functions: 0,
                tid: event.tid,
            });

            stats.total_samples += 1;
            if event.name != "execution" {
                stats.samples_with_functions += 1;
            }
        }

        Self {
            scroll_offset: 0,
            worker_stats,
            zoom_level: 1.0,
            pan_offset: 0.0,
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn zoom_in(&mut self) {
        self.zoom_level = (self.zoom_level * 1.5).min(10.0);
    }

    pub fn zoom_out(&mut self) {
        self.zoom_level = (self.zoom_level / 1.5).max(1.0);
        // Reset pan when fully zoomed out
        if self.zoom_level == 1.0 {
            self.pan_offset = 0.0;
        }
    }

    pub fn pan_left(&mut self) {
        if self.zoom_level > 1.0 {
            self.pan_offset = (self.pan_offset - 0.1).max(0.0);
        }
    }

    pub fn pan_right(&mut self) {
        if self.zoom_level > 1.0 {
            let max_pan = 1.0 - (1.0 / self.zoom_level);
            self.pan_offset = (self.pan_offset + 0.1).min(max_pan);
        }
    }

    pub fn get_zoom_info(&self) -> (f64, f64) {
        (self.zoom_level, self.pan_offset)
    }

    pub fn render(&self, f: &mut Frame, area: Rect, data: &TraceData) {
        let mut lines = vec![];

        // Title and info with zoom level
        let zoom_text = if self.zoom_level > 1.0 {
            format!("  Zoom: {:.1}x  Pan: {:.0}%", self.zoom_level, self.pan_offset * 100.0)
        } else {
            String::new()
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("Duration: {:.1}s", data.duration),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("    Total events: "),
            Span::styled(
                data.events.len().to_string(),
                Style::default().fg(Color::Green),
            ),
            Span::styled(zoom_text, Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(""));

        // Column headers
        lines.push(Line::from(vec![
            Span::styled(
                "Worker",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                "TID",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("      "),
            Span::styled(
                "Samples",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                "Activity",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from("─".repeat(70)));

        // Worker rows
        for worker_id in &data.workers {
            if let Some(stats) = self.worker_stats.get(worker_id) {
                let success_rate = if stats.total_samples > 0 {
                    (stats.samples_with_functions as f64 / stats.total_samples as f64) * 100.0
                } else {
                    0.0
                };

                // Create a visual bar showing activity
                let bar_width = 30;
                let filled = ((success_rate / 100.0) * bar_width as f64) as usize;
                let empty = bar_width - filled;
                let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(empty));

                // Color based on activity level
                let (bar_color, marker) = if success_rate > 50.0 {
                    (Color::Red, " ⚠️ ")
                } else if success_rate > 20.0 {
                    (Color::Yellow, " ")
                } else {
                    (Color::Green, " ")
                };

                lines.push(Line::from(vec![
                    Span::raw(format!("{:<8}", format!("Worker {}", worker_id))),
                    Span::styled(
                        format!("{:<8}", stats.tid),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(format!(
                        "{:>4}/{:<4}",
                        stats.samples_with_functions, stats.total_samples
                    )),
                    Span::raw("  "),
                    Span::styled(bar, Style::default().fg(bar_color)),
                    Span::raw(format!(" {:.0}%", success_rate)),
                    Span::raw(marker),
                ]));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Legend: ", Style::default().fg(Color::DarkGray)),
            Span::styled("▓", Style::default().fg(Color::Red)),
            Span::raw(" Active (function known)  "),
            Span::styled("░", Style::default().fg(Color::Green)),
            Span::raw(" Idle/Generic  "),
            Span::styled("⚠️ ", Style::default()),
            Span::raw(" High CPU (>50%)"),
        ]));

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Worker Timeline"));

        f.render_widget(paragraph, area);
    }
}
