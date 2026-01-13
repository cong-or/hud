use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::theme::{gauge_bar, CAUTION_AMBER, HUD_GREEN};
use super::TraceData;

/// Workers panel - tactical thread load display
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

        for worker_id in data.workers.iter() {
            if let Some(stats) = self.worker_stats.get(worker_id) {
                let percentage = if stats.total_samples > 0 {
                    (stats.samples_with_functions as f64 / stats.total_samples as f64) * 100.0
                } else {
                    0.0
                };

                let bar_color = if percentage > 50.0 { CAUTION_AMBER } else { HUD_GREEN };

                lines.push(Line::from(vec![
                    Span::styled(format!("W{worker_id} "), Style::default().fg(bar_color)),
                    Span::styled(gauge_bar(percentage, 10), Style::default().fg(bar_color)),
                    Span::styled(format!(" {percentage:>3.0}%"), Style::default().fg(bar_color)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Workers")
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }
}
