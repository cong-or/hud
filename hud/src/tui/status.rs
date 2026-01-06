use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{TraceData, CAUTION_AMBER, CRITICAL_RED, HUD_GREEN};

/// Master Status panel - shows system health at a glance
pub struct StatusPanel {
    has_warnings: bool,
    busiest_worker: Option<(u32, f64)>, // (worker_id, activity_percentage)
}

impl StatusPanel {
    pub fn new(data: &TraceData) -> Self {
        // Find busiest worker using functional fold
        let worker_activity = data.events
            .iter()
            .fold(std::collections::HashMap::new(), |mut acc, event| {
                let entry = acc.entry(event.worker_id).or_insert((0, 0));
                entry.0 += 1; // total samples
                if event.name != "execution" {
                    entry.1 += 1; // samples with function names
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
                let percentage =
                    if *total > 0 { (f64::from(*with_funcs) / f64::from(*total)) * 100.0 } else { 0.0 };
                (*worker_id, percentage)
            });

        let has_warnings = busiest.is_some_and(|(_, pct)| pct > 50.0);

        Self { has_warnings, busiest_worker: busiest }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, _data: &TraceData) {
        let mut lines = vec![];

        // Master status indicator
        if self.has_warnings {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  ⚠️  CAUTION",
                Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        } else {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  ✓  NORMAL",
                Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }

        // Show busiest worker
        if let Some((worker_id, percentage)) = self.busiest_worker {
            if percentage > 50.0 {
                lines.push(Line::from(vec![
                    Span::raw("  Worker "),
                    Span::styled(
                        format!("{worker_id}"),
                        Style::default().fg(CRITICAL_RED).add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{percentage:.0}% BUSY"),
                        Style::default().fg(CAUTION_AMBER),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("  Worker "),
                    Span::styled(format!("{worker_id}"), Style::default().fg(HUD_GREEN)),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{percentage:.0}% active"),
                        Style::default().fg(HUD_GREEN),
                    ),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines).block(
            Block::default().borders(Borders::ALL).title("SYSTEM STATUS").border_style(
                Style::default().fg(if self.has_warnings { CAUTION_AMBER } else { HUD_GREEN }),
            ),
        );

        f.render_widget(paragraph, area);
    }
}
