//! # Terminal User Interface (TUI)
//!
//! Interactive terminal UI using `ratatui` for real-time profiling visualization.
//!
//! ## View Modes
//!
//! - **Analysis** - Hotspot list + worker stats (default)
//! - **`DrillDown`** - Detailed view of selected function (F-35 targeting UI)
//! - **Search** - Filter hotspots by name
//! - **Help** - Keyboard shortcuts and concepts
//!
//! ## Entry Point
//!
//! - `run_live()` - Real-time profiling with eBPF event channel
//!
//! ## Sub-Modules
//!
//! - `hotspot` - Hotspot list and sorting
//! - `timeline` - Per-worker execution timeline
//! - `workers` - Worker statistics panel
//! - `status` - Summary status bar
//! - `theme` - Color scheme

// TUI rendering intentionally uses precision-losing casts and long functions for clarity
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::needless_pass_by_value
)]

use anyhow::Result;
use crossbeam_channel::Receiver;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io;
use std::time::Duration;

pub mod hotspot; // Public for testing
mod status;
mod theme;
mod timeline;
mod workers;

use hotspot::HotspotView;
use status::StatusPanel;
use theme::{CAUTION_AMBER, CRITICAL_RED, HUD_GREEN, INFO_DIM};
use timeline::TimelineView;
use workers::WorkersPanel;

pub use crate::trace_data::{LiveData, TraceData, TraceEvent};

// =============================================================================
// STYLE CONSTANTS
// =============================================================================

/// Pre-computed styles for consistent UI rendering.
/// Using `const` ensures zero runtime cost - these are inlined at compile time.
const STYLE_HEADING: Style = Style::new().fg(HUD_GREEN).add_modifier(Modifier::BOLD);
const STYLE_LABEL: Style = Style::new().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD);
const STYLE_DIM: Style = Style::new().fg(INFO_DIM);
const STYLE_KEY: Style = Style::new().fg(CAUTION_AMBER); // Keyboard shortcut highlight
const STYLE_TEXT: Style = Style::new().fg(ratatui::style::Color::White);

/// Format a duration in seconds as a human-readable string (e.g., "2d 4h 23m")
fn format_duration_human(secs: f64) -> String {
    let total_secs = secs as u64;

    if total_secs == 0 {
        return "0s".to_string();
    }

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if mins > 0 {
        parts.push(format!("{mins}m"));
    }
    // Only show seconds if duration is less than an hour
    if secs > 0 && total_secs < 3600 {
        parts.push(format!("{secs}s"));
    }

    if parts.is_empty() {
        "0s".to_string()
    } else {
        parts.join(" ")
    }
}

// =============================================================================
// VIEW MODES
// =============================================================================

/// Current view mode determines what's displayed and how keys are handled
#[derive(Debug, Clone, Copy, PartialEq)]
enum ViewMode {
    /// Main view: hotspot list, workers panel, timeline
    Analysis,
    /// Detailed view of a single function (frozen snapshot)
    DrillDown,
    /// Text input for filtering hotspots by name
    Search,
    /// Help overlay with keyboard shortcuts
    Help,
}

// =============================================================================
// OVERLAY RENDERERS
// =============================================================================
//
// Standalone functions for rendering modal overlays (help, drilldown, search).

/// Render the help overlay explaining hud concepts and keyboard shortcuts
fn render_help_overlay(f: &mut ratatui::Frame, area: Rect) {
    let popup_area = centered_popup(area, 80, 32);

    let help_text = vec![
        Line::from(""),
        // What you're looking at
        Line::from(Span::styled("  What You're Looking At", STYLE_HEADING)),
        Line::from(Span::styled(
            "  hud shows functions blocking your Tokio async runtime. These are",
            STYLE_DIM,
        )),
        Line::from(Span::styled(
            "  operations that don't yield at .await — they block the thread.",
            STYLE_DIM,
        )),
        Line::from(""),
        // How to read it
        Line::from(Span::styled("  How to Read It", STYLE_HEADING)),
        Line::from(vec![
            Span::styled("  Hotspots  ", STYLE_LABEL),
            Span::styled("Functions ranked by blocking time. Fix the top ones.", STYLE_DIM),
        ]),
        Line::from(vec![
            Span::styled("  Workers   ", STYLE_LABEL),
            Span::styled(
                "OS threads running async tasks. High % = blocked, not yielding.",
                STYLE_DIM,
            ),
        ]),
        Line::from(vec![
            Span::styled("  Timeline  ", STYLE_LABEL),
            Span::styled("When blocking happened. Spikes show bursts of blocking.", STYLE_DIM),
        ]),
        Line::from(""),
        // Common culprits
        Line::from(Span::styled("  Common Culprits", STYLE_HEADING)),
        Line::from(Span::styled(
            "  • bcrypt/argon2 — password hashing (use spawn_blocking)",
            STYLE_DIM,
        )),
        Line::from(Span::styled("  • std::fs — sync file I/O (use tokio::fs)", STYLE_DIM)),
        Line::from(Span::styled("  • DNS lookup — std::net blocks (use tokio::net)", STYLE_DIM)),
        Line::from(Span::styled("  • compression — flate2/zstd (use spawn_blocking)", STYLE_DIM)),
        Line::from(""),
        // Keys
        Line::from(Span::styled("  Keys", STYLE_HEADING)),
        Line::from(vec![
            Span::styled("  ↑↓", STYLE_KEY),
            Span::styled(" Select   ", STYLE_TEXT),
            Span::styled("Enter", STYLE_KEY),
            Span::styled(" Inspect   ", STYLE_TEXT),
            Span::styled("/", STYLE_KEY),
            Span::styled(" Search   ", STYLE_TEXT),
            Span::styled("Q", STYLE_KEY),
            Span::styled(" Quit", STYLE_TEXT),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Press any key to close", STYLE_DIM)),
    ];

    let help_widget = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .style(Style::new().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
    );

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(help_widget, popup_area);
}

/// Create a centered popup area within the given bounds.
///
/// # Arguments
/// * `area` - The outer bounds to center within
/// * `width_percent` - Popup width as percentage of outer width (0-100)
/// * `height_lines` - Popup height in terminal lines
///
/// # Layout
/// Uses `Constraint::Fill(1)` for flexible vertical centering, which distributes
/// remaining space evenly above and below the popup.
fn centered_popup(area: Rect, width_percent: u16, height_lines: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(height_lines), Constraint::Fill(1)])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

/// Render drilldown overlay for a selected hotspot.
///
/// Styled as an F-35 targeting computer UI with:
/// - Severity-colored header and brackets (green/amber/red based on CPU%)
/// - Military-style labels: TGT (target), CPU, LOC (location), HIT (samples)
/// - Call trace showing the full call stack
/// - Per-worker distribution bars
///
/// # Severity Thresholds
/// - Green: < 20% CPU (nominal)
/// - Amber: 20-40% CPU (caution)
/// - Red: > 40% CPU (critical)
fn render_drilldown_overlay(
    f: &mut ratatui::Frame,
    area: Rect,
    hotspot: &crate::analysis::FunctionHotspot,
) {
    // Calculate popup height based on content
    let has_call_stack = !hotspot.call_stacks.is_empty();
    let call_stack_lines = if has_call_stack {
        hotspot.call_stacks.first().map_or(0, |s| s.len().min(5)) + 2 // header + frames + spacing
    } else {
        0
    };
    let worker_lines = hotspot.workers.len().min(4) + 1;
    let base_height = 16; // Header + stats + footer
    let popup_height = (base_height + call_stack_lines + worker_lines).min(35) as u16;

    let popup_area = centered_popup(area, 65, popup_height);
    let inner_width = popup_area.width.saturating_sub(4) as usize;

    // Severity color based on CPU percentage
    let severity_color = match hotspot.percentage {
        p if p > 40.0 => CRITICAL_RED,
        p if p > 20.0 => CAUTION_AMBER,
        _ => HUD_GREEN,
    };

    // Build CPU bar
    const BAR_WIDTH: usize = 20;
    let cpu_filled = ((hotspot.percentage / 100.0) * BAR_WIDTH as f64) as usize;
    let cpu_bar = format!(
        "{}{}",
        "█".repeat(cpu_filled.min(BAR_WIDTH)),
        "░".repeat(BAR_WIDTH.saturating_sub(cpu_filled))
    );

    // Truncate function name to fit
    let max_name_len = inner_width.saturating_sub(10);
    let name_display = if hotspot.name.len() > max_name_len {
        format!("{}…", &hotspot.name[..max_name_len.saturating_sub(1)])
    } else {
        hotspot.name.clone()
    };

    // Source location
    let location = hotspot
        .file
        .as_ref()
        .map_or_else(|| "—".into(), |f| format!("{}:{}", f, hotspot.line.unwrap_or(0)));

    // Build tactical display lines with F-35 HUD aesthetics
    let mut lines = vec![
        Line::from(""),
        // Targeting reticle header - diamonds indicate lock status
        Line::from(vec![
            Span::styled("  ◈ ", Style::new().fg(severity_color)),
            Span::styled(
                "TARGET ACQUIRED",
                Style::new().fg(severity_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ◈", Style::new().fg(severity_color)),
        ]),
        Line::from(""),
        // Box-drawing characters create targeting brackets
        Line::from(Span::styled("  ┌─", Style::new().fg(severity_color))),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(severity_color)),
            Span::styled("TGT  ", STYLE_DIM), // Target designation
            Span::styled(name_display, Style::new().fg(HUD_GREEN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(severity_color)),
            Span::styled("CPU  ", STYLE_DIM), // CPU utilization gauge
            Span::styled(cpu_bar, Style::new().fg(severity_color)),
            Span::styled(
                format!(" {:.1}%", hotspot.percentage),
                Style::new().fg(severity_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(severity_color)),
            Span::styled("LOC  ", STYLE_DIM), // Source location
            Span::styled(location, STYLE_DIM),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(severity_color)),
            Span::styled("HIT  ", STYLE_DIM), // Sample hit count
            Span::styled(format!("{} samples", hotspot.count), STYLE_DIM),
        ]),
        Line::from(Span::styled("  └─", Style::new().fg(severity_color))),
        Line::from(""),
    ];

    // Call trace section - inverted to show: your_code → library → blocking_fn
    const MAX_FRAMES: usize = 8;

    if let Some(call_stack) = hotspot.call_stacks.first() {
        lines.push(Line::from(Span::styled("  CALL TRACE", STYLE_DIM)));

        // Reverse stack: show caller (your code) first, blocking function last
        let frames: Vec<_> = call_stack.iter().rev().take(MAX_FRAMES).collect();
        let last_idx = frames.len().saturating_sub(1);

        for (i, frame) in frames.into_iter().enumerate() {
            let arrow = if i == last_idx { "└→" } else { "├→" };

            // Truncate long function names
            let max_len = inner_width.saturating_sub(20);
            let func_display = if frame.function.len() > max_len {
                format!("{}…", &frame.function[..max_len.saturating_sub(1)])
            } else {
                frame.function.clone()
            };

            // User code in green, library code dimmed
            let style = if frame.is_user_code { Style::new().fg(HUD_GREEN) } else { STYLE_DIM };

            // Format location as "file.rs:42" or empty string
            let location = frame.file.as_ref().map_or(String::new(), |path| {
                let filename =
                    std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
                frame.line.map_or(filename.to_string(), |ln| format!("{filename}:{ln}"))
            });

            lines.push(Line::from(vec![
                Span::styled(format!("    {arrow} "), STYLE_DIM),
                Span::styled(func_display, style),
                Span::styled(format!("  {location}"), STYLE_DIM),
            ]));
        }

        if call_stack.len() > MAX_FRAMES {
            lines.push(Line::from(Span::styled(
                format!("       ... +{} more frames", call_stack.len() - MAX_FRAMES),
                STYLE_DIM,
            )));
        }

        lines.push(Line::from(""));
    } else {
        // No call stack available
        lines.push(Line::from(Span::styled("  CALL TRACE", STYLE_DIM)));
        lines.push(Line::from(Span::styled("    ℹ No call stack captured", STYLE_DIM)));
        lines.push(Line::from(""));
    }

    // Worker breakdown with tactical styling
    if !hotspot.workers.is_empty() {
        lines.push(Line::from(Span::styled("  WORKER DISTRIBUTION", STYLE_DIM)));

        let mut worker_list: Vec<_> = hotspot.workers.iter().collect();
        worker_list.sort_unstable_by(|a, b| b.1.cmp(a.1));

        for (&worker_id, &count) in worker_list.iter().take(4) {
            let pct = (count as f64 / hotspot.count as f64) * 100.0;
            let filled = ((pct / 100.0) * 12.0) as usize;
            let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(12 - filled));
            lines.push(Line::from(vec![
                Span::styled(format!("    W{worker_id:<2} "), STYLE_DIM),
                Span::styled(bar, Style::new().fg(HUD_GREEN)),
                Span::styled(format!(" {pct:>3.0}%"), STYLE_DIM),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [ESC]", STYLE_KEY),
        Span::styled(" DISENGAGE", STYLE_DIM),
    ]));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ◈ LOCK ◈ ")
            .style(Style::new().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
    );

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(widget, popup_area);
}

/// Render search input overlay (standalone version)
fn render_search_overlay(f: &mut ratatui::Frame, area: Rect, query: &str) {
    let popup_area = {
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Length(3),
                Constraint::Percentage(60),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .split(vertical[1])[1]
    };

    let search_text = format!("Search: {query}_");
    let search_widget = Paragraph::new(search_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Filter Functions (Enter to apply, Esc to cancel)")
                .style(Style::default().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
        )
        .style(Style::default().fg(CAUTION_AMBER));

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(search_widget, popup_area);
}

// =============================================================================
// LIVE MODE (LiveApp)
// =============================================================================

/// TUI application for **live mode** - real-time profiling of a running process
///
/// Live mode receives events from an eBPF channel and updates the display
/// continuously. Key differences from replay mode:
/// - Data grows over time (events stream in)
/// - Hotspot rankings change dynamically
/// - `DrillDown` view freezes a snapshot to prevent flickering
struct LiveApp {
    /// Accumulates events as they arrive from eBPF
    live_data: LiveData,
    /// Hotspot statistics for efficient aggregation
    hotspot_stats: crate::analysis::HotspotStats,
    /// Hotspot view (rebuilt on each update, preserves selection)
    hotspot_view: Option<HotspotView>,

    // UI state
    view_mode: ViewMode,
    search_query: String,
    should_quit: bool,

    /// Frozen snapshot of hotspot for drilldown (prevents flicker during live updates)
    frozen_hotspot: Option<crate::analysis::FunctionHotspot>,
}

impl LiveApp {
    fn new() -> Self {
        Self {
            live_data: LiveData::new(),
            hotspot_stats: crate::analysis::HotspotStats::new(),
            hotspot_view: None,
            view_mode: ViewMode::Analysis,
            search_query: String::new(),
            should_quit: false,
            frozen_hotspot: None,
        }
    }

    /// Process keyboard input based on current view mode
    fn handle_key(&mut self, key: KeyCode) {
        match self.view_mode {
            // Main analysis view - navigate hotspots, open overlays
            ViewMode::Analysis => match key {
                KeyCode::Char('q' | 'Q') => self.should_quit = true,
                KeyCode::Up => {
                    if let Some(hv) = &mut self.hotspot_view {
                        hv.scroll_up();
                    }
                }
                KeyCode::Down => {
                    if let Some(hv) = &mut self.hotspot_view {
                        hv.scroll_down();
                    }
                }
                KeyCode::Enter => {
                    // Freeze the selected hotspot for drilldown view
                    self.frozen_hotspot =
                        self.hotspot_view.as_ref().and_then(|hv| hv.get_selected().cloned());
                    if self.frozen_hotspot.is_some() {
                        self.view_mode = ViewMode::DrillDown;
                    }
                }
                KeyCode::Char('/') => {
                    self.view_mode = ViewMode::Search;
                    self.search_query.clear();
                }
                KeyCode::Char('c' | 'C') => {
                    if let Some(hv) = &mut self.hotspot_view {
                        hv.clear_filter();
                    }
                }
                KeyCode::Char('?') => self.view_mode = ViewMode::Help,
                _ => {}
            },
            // Search overlay - text input for filtering
            ViewMode::Search => match key {
                KeyCode::Esc => {
                    self.view_mode = ViewMode::Analysis;
                    self.search_query.clear();
                }
                KeyCode::Enter => {
                    if let Some(hv) = &mut self.hotspot_view {
                        hv.apply_filter(&self.search_query);
                    }
                    self.view_mode = ViewMode::Analysis;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Char(c) => self.search_query.push(c),
                _ => {}
            },
            // Help overlay - any key closes
            ViewMode::Help => self.view_mode = ViewMode::Analysis,
            // DrillDown overlay - ESC/Q closes and clears frozen snapshot
            ViewMode::DrillDown => {
                if matches!(key, KeyCode::Esc | KeyCode::Char('q' | 'Q')) {
                    self.view_mode = ViewMode::Analysis;
                    self.frozen_hotspot = None;
                }
            }
        }
    }

    /// Rebuild hotspot view from trace data while preserving UI state.
    ///
    /// Called on each render cycle to reflect new events while maintaining
    /// the user's current selection and any active filters. This is critical
    /// for a smooth live experience - without state preservation, selection
    /// would jump around as rankings change.
    ///
    /// # State Preserved
    /// - `selected_index` - Cursor position in hotspot list
    /// - Active search filter query
    fn update_hotspot_view(&mut self) {
        // Capture current state before rebuilding
        let (old_selected, old_filter) = self
            .hotspot_view
            .as_ref()
            .map_or((0, false), |hv| (hv.selected_index, hv.is_filtered()));

        // Get hotspots from HotspotStats (efficient aggregation)
        let hotspots = self.hotspot_stats.to_hotspots();
        let mut new_view = HotspotView::from_hotspots(hotspots);

        // Restore selection index if still valid (rankings may have shifted)
        if old_selected < new_view.hotspots.len() {
            new_view.selected_index = old_selected;
        }

        // Re-apply filter if user had an active search
        if old_filter && !self.search_query.is_empty() {
            new_view.apply_filter(&self.search_query);
        }

        self.hotspot_view = Some(new_view);
    }
}

// =============================================================================
// LIVE MODE ENTRY POINT
// =============================================================================

/// Run TUI in live mode, receiving events from an eBPF channel
///
/// This is the main entry point for live profiling. It:
/// 1. Sets up the terminal in raw mode
/// 2. Receives events from the eBPF channel (non-blocking)
/// 3. Updates the display at 10Hz (100ms intervals)
/// 4. Handles keyboard input
/// 5. Cleans up terminal on exit
///
/// # Arguments
/// * `event_rx` - Channel receiving trace events from eBPF
/// * `pid` - Process ID being profiled (for display)
///
/// # Errors
/// Returns an error if terminal setup or rendering fails
pub fn run_live(event_rx: Receiver<TraceEvent>, pid: Option<i32>) -> Result<()> {
    // -------------------------------------------------------------------------
    // Terminal Setup
    // -------------------------------------------------------------------------
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // -------------------------------------------------------------------------
    // Application State
    // -------------------------------------------------------------------------
    let mut app = LiveApp::new();
    let mut last_update = std::time::Instant::now();

    // 10 Hz refresh rate balances responsiveness with CPU usage.
    // Higher rates (e.g., 30 Hz) cause unnecessary redraws; lower rates feel laggy.
    const UPDATE_INTERVAL: Duration = Duration::from_millis(100);

    // -------------------------------------------------------------------------
    // Main Event Loop
    // -------------------------------------------------------------------------
    loop {
        // Drain all pending events from eBPF (non-blocking)
        while let Ok(event) = event_rx.try_recv() {
            // Record to stats aggregator, then add to raw event storage
            app.hotspot_stats.record_event(&event);
            app.live_data.add_event(event);
        }

        // Snapshot current data for rendering
        let trace_data = app.live_data.as_trace_data();

        // Redraw periodically
        if last_update.elapsed() >= UPDATE_INTERVAL {
            // Rebuild hotspot view from aggregated stats (preserves selection)
            app.update_hotspot_view();

            let status_panel = StatusPanel::new(&trace_data);
            let workers_panel = WorkersPanel::new(&trace_data);
            let timeline_view = TimelineView::new(&trace_data);
            let has_events = !trace_data.events.is_empty();

            terminal.draw(|f| {
                let outer_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // Header
                        Constraint::Min(0),    // Main panels
                        Constraint::Length(3), // Status bar
                    ])
                    .split(f.area());

                // Header - tactical live display with session info
                let pid_display = pid.map_or_else(|| "---".to_string(), |p| p.to_string());
                let rate = if trace_data.duration > 0.0 {
                    trace_data.events.len() as f64 / trace_data.duration
                } else {
                    0.0
                };

                // Show session duration and sample count
                let session_str = format_duration_human(trace_data.duration);
                let sample_count = app.hotspot_stats.total_samples();

                let header = Paragraph::new(vec![Line::from(vec![
                    Span::styled("HUD", STYLE_HEADING),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(
                        "[LIVE]",
                        Style::new().fg(CRITICAL_RED).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(format!("PID:{pid_display}"), Style::new().fg(HUD_GREEN)),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(format!("duration:{session_str}"), Style::new().fg(HUD_GREEN)),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(format!("{sample_count} samples"), Style::new().fg(CAUTION_AMBER)),
                    Span::styled(format!(" ({rate:.0}/s)"), STYLE_DIM),
                ])])
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::new().fg(CRITICAL_RED)),
                );
                f.render_widget(header, outer_layout[0]);

                // Main content area
                let main_area = outer_layout[1];
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(main_area);

                let top_cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                    .split(rows[0]);

                let bottom_cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                    .split(rows[1]);

                // Render panels
                status_panel.render(f, top_cols[0], &trace_data);
                if let Some(ref hv) = app.hotspot_view {
                    hv.render(f, top_cols[1], &trace_data);
                }
                workers_panel.render(f, bottom_cols[0], &trace_data);
                timeline_view.render(f, bottom_cols[1], &trace_data);

                // Search overlay
                if app.view_mode == ViewMode::Search {
                    render_search_overlay(f, f.area(), &app.search_query);
                }

                // Help overlay
                if app.view_mode == ViewMode::Help {
                    render_help_overlay(f, f.area());
                }

                // DrillDown overlay (uses frozen snapshot)
                if app.view_mode == ViewMode::DrillDown {
                    if let Some(ref hotspot) = app.frozen_hotspot {
                        render_drilldown_overlay(f, f.area(), hotspot);
                    }
                }

                // Status bar keybinds
                let mode_indicator = match app.view_mode {
                    ViewMode::Search => Span::styled("[Search]", Style::new().fg(CAUTION_AMBER)),
                    ViewMode::DrillDown => Span::styled("[Detail]", Style::new().fg(CAUTION_AMBER)),
                    _ if has_events => Span::styled("[Live]", Style::new().fg(CRITICAL_RED)),
                    _ => Span::styled("[Waiting]", STYLE_DIM),
                };

                let status_line = Line::from(vec![
                    Span::styled("Q", STYLE_KEY),
                    Span::styled(":Quit ", STYLE_DIM),
                    Span::styled("Enter", STYLE_KEY),
                    Span::styled(":Detail ", STYLE_DIM),
                    Span::styled("/", STYLE_KEY),
                    Span::styled(":Search ", STYLE_DIM),
                    Span::styled("?", STYLE_KEY),
                    Span::styled(":Help ", STYLE_DIM),
                    mode_indicator,
                ]);

                let status = Paragraph::new(vec![status_line]).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(HUD_GREEN)),
                );
                f.render_widget(status, outer_layout[2]);
            })?;

            last_update = std::time::Instant::now();
        }

        // Handle keyboard input with short poll timeout for responsive feel
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code);
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Small sleep prevents busy-spinning when no events are coming in.
        // Combined with the 50ms poll timeout above, this keeps CPU usage low.
        std::thread::sleep(Duration::from_millis(10));
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
