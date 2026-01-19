//! Activity panel - detailed per-worker statistics with thread IDs.
//!
//! # What This Shows
//!
//! A tabular view of each worker thread with more detail than the Workers panel:
//!
//! ```text
//! [ ACTIVITY ]
//! Duration 45.2s  Events 1234
//! ID  TID      Samples  Load
//! W0  12345    100/250  [||||      ] 40%
//! W1  12346     50/250  [||        ] 20%
//! ```
//!
//! # Columns
//!
//! - **ID**: Worker ID (W0, W1, W2...) assigned by Tokio
//! - **TID**: OS thread ID (useful for correlating with `htop`, `perf`, etc.)
//! - **Samples**: blocking/total samples for this worker
//! - **Load**: Visual gauge + percentage of blocking time
//!
//! # Color Thresholds
//!
//! - Green (< 20%): Healthy, mostly yielding
//! - Amber (20-50%): Some blocking, worth investigating
//! - Red (> 50%): Significant blocking, needs attention

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::theme::{
    gauge_bar, CAUTION_AMBER, CRITICAL_RED, CYAN_DIM, HUD_CYAN, HUD_GREEN, INFO_DIM,
};
use super::TraceData;

/// Timeline view - detailed per-worker statistics with OS thread IDs.
pub struct TimelineView {
    /// Per-worker statistics, keyed by worker ID
    worker_stats: HashMap<u32, WorkerStats>,
}

/// Detailed statistics for a single worker thread.
#[derive(Debug, Clone)]
struct WorkerStats {
    /// Total samples captured for this worker
    total_samples: usize,
    /// Samples with actual blocking function names
    samples_with_functions: usize,
    /// OS thread ID (from /proc or gettid)
    tid: u32,
}

impl WorkerStats {
    /// Calculate load percentage (samples with function names / total)
    fn load_percentage(&self) -> f64 {
        if self.total_samples > 0 {
            (self.samples_with_functions as f64 / self.total_samples as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get severity color based on load percentage
    fn load_color(&self) -> ratatui::style::Color {
        match self.load_percentage() {
            r if r > 50.0 => CRITICAL_RED,
            r if r > 20.0 => CAUTION_AMBER,
            _ => HUD_GREEN,
        }
    }
}

impl TimelineView {
    pub fn new(data: &TraceData) -> Self {
        let worker_stats = data.events.iter().fold(HashMap::new(), |mut acc, event| {
            let stats = acc.entry(event.worker_id).or_insert(WorkerStats {
                total_samples: 0,
                samples_with_functions: 0,
                tid: event.tid,
            });
            stats.total_samples += 1;
            if event.name != "execution" {
                stats.samples_with_functions += 1;
            }
            acc
        });

        Self { worker_stats }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, data: &TraceData) {
        let mut lines = vec![];

        // Header stats
        lines.push(Line::from(vec![
            Span::styled("Duration ", Style::default().fg(INFO_DIM)),
            Span::styled(format!("{:.1}s", data.duration), Style::default().fg(HUD_CYAN)),
            Span::raw("  "),
            Span::styled("Events ", Style::default().fg(INFO_DIM)),
            Span::styled(format!("{}", data.events.len()), Style::default().fg(HUD_GREEN)),
        ]));

        // Column header
        lines.push(Line::from(vec![Span::styled(
            "ID  TID      Samples  Load",
            Style::default().fg(INFO_DIM).add_modifier(Modifier::BOLD),
        )]));

        // Worker rows - use filter_map to skip workers without stats
        lines.extend(data.workers.iter().filter_map(|worker_id| {
            self.worker_stats.get(worker_id).map(|stats| {
                let rate = stats.load_percentage();
                let color = stats.load_color();

                Line::from(vec![
                    Span::styled(format!("W{worker_id:<2} "), Style::default().fg(HUD_CYAN)),
                    Span::styled(format!("{:<8} ", stats.tid), Style::default().fg(CYAN_DIM)),
                    Span::styled(
                        format!("{:>4}/{:<4} ", stats.samples_with_functions, stats.total_samples),
                        Style::default().fg(HUD_GREEN),
                    ),
                    Span::styled(gauge_bar(rate, 12), Style::default().fg(color)),
                    Span::styled(format!(" {rate:>3.0}%"), Style::default().fg(color)),
                ])
            })
        }));

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .title("[ ACTIVITY ]")
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }
}
