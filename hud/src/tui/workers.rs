use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::{TraceData, CAUTION_AMBER, HUD_GREEN};

/// Workers panel - shows load bars for each worker
pub struct WorkersPanel {
    worker_stats: HashMap<u32, WorkerStats>,
}

#[derive(Debug, Clone)]
struct WorkerStats {
    total_samples: usize,
    samples_with_functions: usize,
}

impl WorkersPanel {
    pub fn new(data: &TraceData) -> Self {
        let worker_stats = data.events.iter().fold(HashMap::new(), |mut acc, event| {
            let stats = acc
                .entry(event.worker_id)
                .or_insert(WorkerStats { total_samples: 0, samples_with_functions: 0 });

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

        lines.push(Line::from(""));

        // Render each worker as a bar
        for worker_id in data.workers.iter() {
            if let Some(stats) = self.worker_stats.get(worker_id) {
                let percentage = if stats.total_samples > 0 {
                    (stats.samples_with_functions as f64 / stats.total_samples as f64) * 100.0
                } else {
                    0.0
                };

                // Create horizontal bar (10 characters wide)
                let bar_width = 10;
                let filled = ((percentage / 100.0) * bar_width as f64) as usize;
                let empty = bar_width - filled;
                let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

                let bar_color = if percentage > 50.0 { CAUTION_AMBER } else { HUD_GREEN };

                let marker = if percentage > 50.0 { " ⚠️" } else { "" };

                lines.push(Line::from(vec![
                    Span::raw(format!(" W{worker_id:<3} ")),
                    Span::styled(bar, Style::default().fg(bar_color)),
                    Span::raw(format!(" {percentage:.0}%{marker}")),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("EXECUTOR THREADS"));

        f.render_widget(paragraph, area);
    }
}
