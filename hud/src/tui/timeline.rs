use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::theme::{gauge_bar, CAUTION_AMBER, CRITICAL_RED, HUD_GREEN, INFO_DIM};
use super::TraceData;

/// Timeline view - tactical activity display
pub struct TimelineView {
    worker_stats: HashMap<u32, WorkerStats>,
    zoom_level: f64,
    pan_offset: f64,
}

#[derive(Debug, Clone)]
struct WorkerStats {
    total_samples: usize,
    samples_with_functions: usize,
    tid: u32,
}

impl TimelineView {
    pub fn new(data: &TraceData) -> Self {
        let mut worker_stats = HashMap::new();

        for event in data.events.iter() {
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

        Self { worker_stats, zoom_level: 1.0, pan_offset: 0.0 }
    }

    pub fn zoom_in(&mut self) {
        self.zoom_level = (self.zoom_level * 1.5).min(10.0);
    }

    pub fn zoom_out(&mut self) {
        self.zoom_level = (self.zoom_level / 1.5).max(1.0);
        if self.zoom_level <= 1.0 {
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

    pub fn render(&self, f: &mut Frame, area: Rect, data: &TraceData) {
        let mut lines = vec![];

        // Header stats
        let zoom_info = if self.zoom_level > 1.0 {
            format!(" Z:{:.1}x", self.zoom_level)
        } else {
            String::new()
        };

        lines.push(Line::from(vec![
            Span::styled("Duration ", Style::default().fg(INFO_DIM)),
            Span::styled(format!("{:.1}s", data.duration), Style::default().fg(HUD_GREEN)),
            Span::raw("  "),
            Span::styled("Events ", Style::default().fg(INFO_DIM)),
            Span::styled(format!("{}", data.events.len()), Style::default().fg(HUD_GREEN)),
            Span::styled(zoom_info, Style::default().fg(CAUTION_AMBER)),
        ]));

        // Column header
        lines.push(Line::from(vec![Span::styled(
            "ID  TID      Samples  Load",
            Style::default().fg(INFO_DIM).add_modifier(Modifier::BOLD),
        )]));

        // Worker rows
        for worker_id in data.workers.iter() {
            if let Some(stats) = self.worker_stats.get(worker_id) {
                let rate = if stats.total_samples > 0 {
                    (stats.samples_with_functions as f64 / stats.total_samples as f64) * 100.0
                } else {
                    0.0
                };

                let color = if rate > 50.0 {
                    CRITICAL_RED
                } else if rate > 20.0 {
                    CAUTION_AMBER
                } else {
                    HUD_GREEN
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("W{worker_id:<2} "), Style::default().fg(color)),
                    Span::styled(format!("{:<8} ", stats.tid), Style::default().fg(INFO_DIM)),
                    Span::styled(
                        format!("{:>4}/{:<4} ", stats.samples_with_functions, stats.total_samples),
                        Style::default().fg(HUD_GREEN),
                    ),
                    Span::styled(gauge_bar(rate, 12), Style::default().fg(color)),
                    Span::styled(format!(" {rate:>3.0}%"), Style::default().fg(color)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Activity")
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }
}
