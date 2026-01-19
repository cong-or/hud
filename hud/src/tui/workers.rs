use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::theme::{gauge_bar, warning_color, HUD_CYAN, HUD_GREEN};
use super::TraceData;

/// Workers panel - tactical thread load display
pub struct WorkersPanel {
    worker_stats: HashMap<u32, WorkerStats>,
}

#[derive(Debug, Clone, Default)]
struct WorkerStats {
    total_samples: usize,
    samples_with_functions: usize,
}

impl WorkerStats {
    /// Calculate blocking percentage (samples with function names / total)
    fn blocking_percentage(&self) -> f64 {
        if self.total_samples > 0 {
            (self.samples_with_functions as f64 / self.total_samples as f64) * 100.0
        } else {
            0.0
        }
    }
}

impl WorkersPanel {
    pub fn new(data: &TraceData) -> Self {
        let worker_stats = data.events.iter().fold(HashMap::new(), |mut acc, event| {
            let stats: &mut WorkerStats = acc.entry(event.worker_id).or_default();
            stats.total_samples += 1;
            if event.name != "execution" {
                stats.samples_with_functions += 1;
            }
            acc
        });

        Self { worker_stats }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, data: &TraceData) {
        let lines: Vec<Line> = data
            .workers
            .iter()
            .filter_map(|worker_id| {
                self.worker_stats.get(worker_id).map(|stats| {
                    let percentage = stats.blocking_percentage();
                    let bar_color = warning_color(percentage);
                    Line::from(vec![
                        Span::styled(format!("W{worker_id} "), Style::default().fg(HUD_CYAN)),
                        Span::styled(gauge_bar(percentage, 10), Style::default().fg(bar_color)),
                        Span::styled(format!(" {percentage:>3.0}%"), Style::default().fg(bar_color)),
                    ])
                })
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .title("[ WORKERS ]")
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }
}
