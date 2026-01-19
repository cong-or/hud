//! Workers panel - shows per-worker blocking load.
//!
//! # What This Shows
//!
//! Each Tokio worker thread gets a horizontal gauge bar showing what percentage
//! of samples had blocking function calls (vs idle/execution time).
//!
//! ```text
//! [ WORKERS ]
//! W0 [||||      ] 40%   <- 40% of samples were blocking calls
//! W1 [||        ] 20%
//! W2 [|         ] 10%
//! ```
//!
//! # Understanding the Numbers
//!
//! - **High %** (amber/red): Worker is frequently blocked, not yielding to scheduler
//! - **Low %** (green): Worker is mostly idle or yielding properly at `.await` points
//!
//! A healthy async app should show low percentages. High percentages indicate
//! blocking operations that should be moved to `spawn_blocking()` or made async.

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

/// Workers panel - tactical thread load display.
///
/// Shows a compact gauge for each Tokio worker thread indicating how much
/// time it spends in blocking operations vs yielding to the async scheduler.
pub struct WorkersPanel {
    /// Per-worker statistics, keyed by worker ID (0, 1, 2, ...)
    worker_stats: HashMap<u32, WorkerStats>,
}

/// Statistics for a single worker thread.
#[derive(Debug, Clone, Default)]
struct WorkerStats {
    /// Total samples captured for this worker
    total_samples: usize,
    /// Samples where we captured a blocking function (not "execution")
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
    /// Build worker stats by aggregating all trace events.
    ///
    /// We count "execution" events separately - these represent the worker
    /// running the async scheduler (polling futures), not blocking user code.
    /// Only events with actual function names count toward blocking percentage.
    pub fn new(data: &TraceData) -> Self {
        let worker_stats = data.events.iter().fold(HashMap::new(), |mut acc, event| {
            let stats: &mut WorkerStats = acc.entry(event.worker_id).or_default();
            stats.total_samples += 1;
            // "execution" = scheduler overhead, not blocking user code
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
                        Span::styled(
                            format!(" {percentage:>3.0}%"),
                            Style::default().fg(bar_color),
                        ),
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
