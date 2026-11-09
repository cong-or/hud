//! Neighbours panel - who is stealing your workers' CPU (noisy-neighbour blame).
//!
//! # What This Shows
//!
//! When a runnable Tokio worker is involuntarily preempted off the CPU, the
//! thread that took the core is the "culprit". This panel ranks culprits by how
//! often they cut your workers off:
//!
//! ```text
//! [ NEIGHBOURS ]           preempted-while-runnable
//! log-flusher    4821 [|||||| ]  61×   ← top thief
//! metrics-agent   990 [|||    ]  26×
//! ─────────────────────────────────────
//! 87 preemptions this window
//! ```
//!
//! # Counts, not stolen time
//!
//! Each row is a **count** of preemptions, not a duration. The immediate
//! preemptor proves *who* cut in at that instant — not that it held the core for
//! the whole off-CPU gap — so attributing stolen milliseconds would over-claim.
//! Counts (and the visible N) are the honest, provable signal. The gauge shows
//! each culprit's *share of attributed preemptions*, which is a true proportion.
//!
//! # Clean state is a feature
//!
//! When nothing preempted your workers, the panel says so plainly rather than
//! rendering an empty box — the quiet case is a real, trustworthy result, not a
//! bug. Biasing toward "no clear thief" over a confident-but-wrong accusation is
//! deliberate: a false blame erodes trust faster than a miss.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Frame,
};

use super::theme::{gauge_bar, warning_color, CAUTION_AMBER, HUD_CYAN, HUD_GREEN, INFO_DIM};
use crate::trace_data::BlameData;

/// How many culprits to list before truncating.
const MAX_ROWS: usize = 6;

/// Neighbours panel - noisy-neighbour attribution display.
///
/// Owns its aggregated rows so it holds no borrow of the live blame state while
/// rendering (mirrors how `WorkersPanel` snapshots its data).
pub struct NeighboursPanel {
    /// `(pid, comm, count)` ranked by count, highest first.
    rows: Vec<(u32, String, u64)>,
    /// Total attributed preemptions this window (the denominator for shares).
    total: u64,
    /// Total victim off-CPU time across those preemptions (ns) — the severity.
    total_stall_ns: u64,
}

impl NeighboursPanel {
    /// Build the panel from accumulated blame data.
    #[must_use]
    pub fn new(data: &BlameData) -> Self {
        Self {
            rows: data.ranked(),
            total: data.total(),
            total_stall_ns: data.total_off_cpu_ns(),
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        // Legible subtitle up front; the precise mechanism ("preempted while
        // runnable") lives in --help/docs — details on demand.
        lines.push(Line::from(Span::styled(
            " stealing your workers' CPU",
            Style::default().fg(INFO_DIM),
        )));

        if self.rows.is_empty() {
            // Clean state: a trustworthy quiet result, rendered as a signal.
            lines.push(Line::from(Span::styled(
                " no clear thief — nothing preempted your workers",
                Style::default().fg(HUD_GREEN),
            )));
        } else {
            for (i, (pid, comm, count)) in self.rows.iter().take(MAX_ROWS).enumerate() {
                let share = if self.total > 0 {
                    (*count as f64 / self.total as f64) * 100.0
                } else {
                    0.0
                };
                let bar_color = warning_color(share);
                let name = if comm.is_empty() { "<unknown>" } else { comm.as_str() };

                let mut spans = vec![
                    Span::styled(format!(" {name:<14.14} {pid:<6} "), Style::default().fg(HUD_CYAN)),
                    Span::styled(gauge_bar(share, 8), Style::default().fg(bar_color)),
                    Span::styled(format!(" {count:>4}×"), Style::default().fg(bar_color)),
                ];
                if i == 0 {
                    spans.push(Span::styled("  ← top thief", Style::default().fg(CAUTION_AMBER)));
                }
                lines.push(Line::from(spans));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!(
                    " {} preemptions · workers lost {}",
                    self.total,
                    fmt_dur_ns(self.total_stall_ns)
                ),
                Style::default().fg(INFO_DIM),
            )));
        }

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .title("[ NEIGHBOURS ]")
                .border_style(Style::default().fg(HUD_GREEN)),
        );

        f.render_widget(paragraph, area);
    }
}

/// Format a nanosecond duration compactly for the panel footer.
fn fmt_dur_ns(ns: u64) -> String {
    if ns >= 1_000_000_000 {
        format!("{:.1}s", ns as f64 / 1e9)
    } else if ns >= 1_000_000 {
        format!("{}ms", ns / 1_000_000)
    } else {
        format!("{}µs", ns / 1_000)
    }
}

#[cfg(test)]
mod render_tests {
    use super::NeighboursPanel;
    use crate::trace_data::{BlameData, PreemptionEvent};
    use ratatui::{Terminal, backend::TestBackend};

    /// Build a `BlameData` from `(pid, comm, count)` triples.
    fn blame(culprits: &[(u32, &str, u64)]) -> BlameData {
        let mut b = BlameData::new();
        for &(pid, comm, n) in culprits {
            for _ in 0..n {
                b.record(PreemptionEvent {
                    worker_id: 0,
                    culprit_pid: pid,
                    culprit_comm: comm.to_string(),
                    off_cpu_ns: 4_000_000,
                });
            }
        }
        b
    }

    /// Render the panel into a `TestBackend` and return its cells as plain text.
    fn render_text(data: &BlameData, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let panel = NeighboursPanel::new(data);
        terminal.draw(|f| panel.render(f, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        let width = buf.area.width as usize;
        let mut out = String::new();
        for (i, cell) in buf.content.iter().enumerate() {
            out.push_str(cell.symbol());
            if (i + 1) % width == 0 {
                out.push('\n');
            }
        }
        out
    }

    #[test]
    fn renders_culprits_with_top_thief() {
        // Mirror the live validation: `yes` dominant, a kernel worker minor.
        let data = blame(&[(209_745, "yes", 1217), (134_053, "kworker/3:1", 36)]);
        let text = render_text(&data, 60, 10);
        for needle in
            ["NEIGHBOURS", "yes", "top thief", "kworker/3:1", "1253 preemptions", "workers lost"]
        {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
    }

    #[test]
    fn clean_state_says_no_thief() {
        let text = render_text(&BlameData::new(), 60, 6);
        assert!(text.contains("no clear thief"), "clean state missing in:\n{text}");
    }

    /// Visual snapshot — run with `--nocapture` to see the panel.
    #[test]
    fn visual_snapshot() {
        let data = blame(&[(209_745, "yes", 1217), (134_053, "kworker/3:1", 36)]);
        let mut terminal = Terminal::new(TestBackend::new(62, 9)).unwrap();
        let panel = NeighboursPanel::new(&data);
        terminal.draw(|f| panel.render(f, f.area())).unwrap();
        println!("\n--- NEIGHBOURS panel (dirty) ---\n{}", terminal.backend());

        let mut clean = Terminal::new(TestBackend::new(62, 6)).unwrap();
        let empty = NeighboursPanel::new(&BlameData::new());
        clean.draw(|f| empty.render(f, f.area())).unwrap();
        println!("\n--- NEIGHBOURS panel (clean) ---\n{}", clean.backend());
    }
}
