use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use std::collections::HashMap;

use super::theme::{gauge_bar, CAUTION_AMBER, HUD_GREEN, INFO_DIM};
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
        let worker_activity = data.events.iter().fold(HashMap::new(), |mut acc, event| {
            let entry = acc.entry(event.worker_id).or_insert((0, 0));
            entry.0 += 1;
            if event.name != "execution" {
                entry.1 += 1;
            }
            acc
        });

        let busiest = worker_activity
            .iter()
            .max_by_key(
                |(_, (total, with_funcs))| {
                    if *total > 0 {
                        (*with_funcs * 100) / *total
                    } else {
                        0
                    }
                },
            )
            .map(|(worker_id, (total, with_funcs))| {
                let percentage = if *total > 0 {
                    (f64::from(*with_funcs) / f64::from(*total)) * 100.0
                } else {
                    0.0
                };
                (*worker_id, percentage)
            });

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
        let mut lines = vec![];

        // System status line
        let (status_text, status_color) = if self.has_warnings {
            ("[!] CAUTION", CAUTION_AMBER)
        } else {
            ("[-] NOMINAL", HUD_GREEN)
        };

        lines.push(Line::from(Span::styled(
            format!(" {status_text}"),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Stats
        lines.push(Line::from(vec![
            Span::styled(" Events  ", Style::default().fg(INFO_DIM)),
            Span::styled(format!("{}", self.total_events), Style::default().fg(HUD_GREEN)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" Workers ", Style::default().fg(INFO_DIM)),
            Span::styled(format!("{}", self.worker_count), Style::default().fg(HUD_GREEN)),
        ]));

        // Debug info coverage indicator
        let debug_color = if self.low_debug_coverage { CAUTION_AMBER } else { HUD_GREEN };
        lines.push(Line::from(vec![
            Span::styled(" Debug   ", Style::default().fg(INFO_DIM)),
            Span::styled(
                format!("{:.0}%", self.debug_info_coverage),
                Style::default().fg(debug_color),
            ),
        ]));

        lines.push(Line::from(""));

        // Busiest worker with gauge
        if let Some((worker_id, percentage)) = self.busiest_worker {
            let bar_color = if percentage > 50.0 { CAUTION_AMBER } else { HUD_GREEN };
            lines.push(Line::from(vec![
                Span::styled(" Hottest ", Style::default().fg(INFO_DIM)),
                Span::styled(format!("W{worker_id}"), Style::default().fg(bar_color)),
            ]));
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(gauge_bar(percentage, 10), Style::default().fg(bar_color)),
                Span::styled(format!(" {percentage:.0}%"), Style::default().fg(bar_color)),
            ]));
        }

        let border_color = if self.has_warnings { CAUTION_AMBER } else { HUD_GREEN };
        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Status")
                .border_style(Style::default().fg(border_color)),
        );

        f.render_widget(paragraph, area);
    }
}
