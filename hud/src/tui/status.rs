use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Frame,
};

use std::collections::HashMap;

use super::theme::{
    gauge_bar, status_color, warning_color, CAUTION_AMBER, HUD_CYAN, HUD_GREEN, INFO_DIM,
};
use super::TraceData;
use crate::classification::diagnostics;

/// Master Status panel - tactical system overview
pub struct StatusPanel {
    has_warnings: bool,
    busiest_worker: Option<(u32, f64)>,
    total_events: usize,
    worker_count: usize,
    debug_info_coverage: f64,
    low_debug_coverage: bool,
}

impl StatusPanel {
    pub fn new(data: &TraceData) -> Self {
        // Aggregate worker activity: (total_samples, samples_with_function_names)
        let worker_activity = data.events.iter().fold(HashMap::new(), |mut acc, event| {
            let entry = acc.entry(event.worker_id).or_insert((0usize, 0usize));
            entry.0 += 1;
            if event.name != "execution" {
                entry.1 += 1;
            }
            acc
        });

        // Find busiest worker by percentage of samples with function names
        let busiest = worker_activity
            .iter()
            .filter(|(_, (total, _))| *total > 0)
            .map(|(&worker_id, &(total, with_funcs))| {
                let percentage = (with_funcs as f64 / total as f64) * 100.0;
                (worker_id, percentage)
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Get coverage once, derive low_coverage from it (avoids redundant atomic loads)
        let debug_info_coverage = diagnostics().debug_info_coverage();
        let low_debug_coverage = debug_info_coverage < 50.0;
        let has_warnings = busiest.is_some_and(|(_, pct)| pct > 50.0) || low_debug_coverage;

        Self {
            has_warnings,
            busiest_worker: busiest,
            total_events: data.events.len(),
            worker_count: data.workers.len(),
            debug_info_coverage,
            low_debug_coverage,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, _data: &TraceData) {
        // System status line with appropriate styling
        let (status_text, status_style) = if self.has_warnings {
            (
                "[!] CAUTION",
                Style::default()
                    .fg(CAUTION_AMBER)
                    .add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK),
            )
        } else {
            ("[-] NOMINAL", Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD))
        };

        let debug_color = status_color(self.low_debug_coverage);

        let mut lines = vec![
            Line::from(Span::styled(format!(" {status_text}"), status_style)),
            Line::from(""),
            Line::from(vec![
                Span::styled(" Events  ", Style::default().fg(INFO_DIM)),
                Span::styled(self.total_events.to_string(), Style::default().fg(HUD_GREEN)),
            ]),
            Line::from(vec![
                Span::styled(" Workers ", Style::default().fg(INFO_DIM)),
                Span::styled(self.worker_count.to_string(), Style::default().fg(HUD_GREEN)),
            ]),
            Line::from(vec![
                Span::styled(" Debug   ", Style::default().fg(INFO_DIM)),
                Span::styled(
                    format!("{:.0}%", self.debug_info_coverage),
                    Style::default().fg(debug_color),
                ),
            ]),
            Line::from(""),
        ];

        // Busiest worker with gauge (if any workers active)
        if let Some((worker_id, percentage)) = self.busiest_worker {
            let bar_color = warning_color(percentage);
            lines.extend([
                Line::from(vec![
                    Span::styled(" Hottest ", Style::default().fg(INFO_DIM)),
                    Span::styled(format!("W{worker_id}"), Style::default().fg(HUD_CYAN)),
                ]),
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(gauge_bar(percentage, 10), Style::default().fg(bar_color)),
                    Span::styled(format!(" {percentage:.0}%"), Style::default().fg(bar_color)),
                ]),
            ]);
        }

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .title("[ STATUS ]")
                .border_style(Style::default().fg(status_color(self.has_warnings))),
        );

        f.render_widget(paragraph, area);
    }
}
