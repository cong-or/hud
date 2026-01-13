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

/// Master Status panel - tactical system overview
pub struct StatusPanel {
    has_warnings: bool,
    busiest_worker: Option<(u32, f64)>,
    total_events: usize,
    worker_count: usize,
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

        let has_warnings = busiest.is_some_and(|(_, pct)| pct > 50.0);

        Self {
            has_warnings,
            busiest_worker: busiest,
            total_events: data.events.len(),
            worker_count: data.workers.len(),
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
