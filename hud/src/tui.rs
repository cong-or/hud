//! # Terminal User Interface (TUI)
//!
//! Interactive terminal UI using `ratatui` for real-time visualization.
//!
//! ## View Modes
//!
//! - **Analysis** - Hotspot list + worker stats (default)
//! - **`DrillDown`** - Detailed view of selected function
//! - **Search** - Filter hotspots by name
//! - **`WorkerFilter`** - Select which workers to display
//!
//! ## Operational Modes
//!
//! - **Live** (`run_live()`) - Real-time profiling with event channel
//! - **Replay** (`App::run()`) - Offline analysis of trace.json
//!
//! ## Sub-Modules
//!
//! - `hotspot` - Hotspot list and sorting
//! - `timeline` - Per-worker execution timeline
//! - `workers` - Worker statistics panel
//! - `status` - Summary status bar
//! - `theme` - Color scheme
//!
//! See [TUI Guide](../../docs/TUI.md) for keyboard shortcuts and detailed usage.

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
use theme::{BACKGROUND, CAUTION_AMBER, CRITICAL_RED, HUD_GREEN, INFO_DIM};
use timeline::TimelineView;
use workers::WorkersPanel;

pub use crate::trace_data::{LiveData, TraceData, TraceEvent};

// =============================================================================
// STYLE CONSTANTS
// =============================================================================

/// Pre-computed styles for consistent UI rendering (const fn for zero runtime cost)
const STYLE_HEADING: Style = Style::new().fg(HUD_GREEN).add_modifier(Modifier::BOLD);
const STYLE_LABEL: Style = Style::new().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD);
const STYLE_DIM: Style = Style::new().fg(INFO_DIM);
const STYLE_KEY: Style = Style::new().fg(CAUTION_AMBER);
const STYLE_TEXT: Style = Style::new().fg(ratatui::style::Color::White);

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
    /// Checkbox list for filtering by worker thread
    WorkerFilter,
    /// Help overlay with keyboard shortcuts
    Help,
}

// =============================================================================
// REPLAY MODE (App)
// =============================================================================

/// TUI application for **replay mode** - analyzing a saved trace.json file
///
/// Replay mode loads all events upfront and provides navigation through
/// the historical data. Use `App::new()` to create and `App::run()` to start.
pub struct App {
    /// Loaded trace data (immutable after creation)
    data: TraceData,

    // UI panels
    status_panel: StatusPanel,
    hotspot_view: HotspotView,
    workers_panel: WorkersPanel,
    timeline_view: TimelineView,

    // UI state
    view_mode: ViewMode,
    search_query: String,
    selected_workers: Vec<u32>,
    worker_filter_cursor: usize,
    should_quit: bool,
}

impl App {
    #[must_use]
    pub fn new(data: TraceData) -> Self {
        let status_panel = StatusPanel::new(&data);
        let hotspot_view = HotspotView::new(&data);
        let workers_panel = WorkersPanel::new(&data);
        let timeline_view = TimelineView::new(&data);

        let all_workers = data.workers.as_ref().clone();

        Self {
            data,
            status_panel,
            hotspot_view,
            workers_panel,
            timeline_view,
            view_mode: ViewMode::Analysis,
            search_query: String::new(),
            selected_workers: all_workers,
            worker_filter_cursor: 0,
            should_quit: false,
        }
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: KeyCode) {
        match &self.view_mode {
            ViewMode::Analysis => {
                match key {
                    KeyCode::Char('q' | 'Q') => self.should_quit = true,
                    KeyCode::Up => self.hotspot_view.scroll_up(),
                    KeyCode::Down => self.hotspot_view.scroll_down(),
                    KeyCode::Enter => {
                        self.view_mode = ViewMode::DrillDown;
                    }
                    KeyCode::Char('/') => {
                        self.search_query.clear();
                        self.view_mode = ViewMode::Search;
                    }
                    KeyCode::Char('f' | 'F') => {
                        self.view_mode = ViewMode::WorkerFilter;
                    }
                    KeyCode::Char('c' | 'C') => {
                        self.search_query.clear();
                        self.hotspot_view.clear_filter();
                        // Reset to all workers
                        self.selected_workers = self.data.workers.as_ref().clone();
                        self.hotspot_view.filter_by_workers(&self.selected_workers, &self.data);
                    }
                    KeyCode::Char('w' | 'W') => {
                        self.timeline_view.zoom_in();
                    }
                    KeyCode::Char('s' | 'S') => {
                        self.timeline_view.zoom_out();
                    }
                    KeyCode::Char('a' | 'A') => {
                        self.timeline_view.pan_left();
                    }
                    KeyCode::Char('d' | 'D') => {
                        self.timeline_view.pan_right();
                    }
                    KeyCode::Char('?') => {
                        self.view_mode = ViewMode::Help;
                    }
                    _ => {}
                }
            }
            ViewMode::Help => {
                // Any key closes help
                self.view_mode = ViewMode::Analysis;
            }
            ViewMode::DrillDown => match key {
                KeyCode::Esc | KeyCode::Char('q' | 'Q') => {
                    self.view_mode = ViewMode::Analysis;
                }
                _ => {}
            },
            ViewMode::Search => match key {
                KeyCode::Esc => {
                    self.search_query.clear();
                    self.view_mode = ViewMode::Analysis;
                    self.hotspot_view.clear_filter();
                }
                KeyCode::Enter => {
                    self.view_mode = ViewMode::Analysis;
                    self.hotspot_view.apply_filter(&self.search_query);
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                _ => {}
            },
            ViewMode::WorkerFilter => {
                match key {
                    KeyCode::Esc => {
                        self.view_mode = ViewMode::Analysis;
                    }
                    KeyCode::Up => {
                        self.worker_filter_cursor = self.worker_filter_cursor.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if self.worker_filter_cursor + 1 < self.data.workers.len() {
                            self.worker_filter_cursor += 1;
                        }
                    }
                    KeyCode::Char(' ') => {
                        // Toggle worker selection
                        let worker_id = self.data.workers[self.worker_filter_cursor];
                        if let Some(pos) =
                            self.selected_workers.iter().position(|&w| w == worker_id)
                        {
                            self.selected_workers.remove(pos);
                        } else {
                            self.selected_workers.push(worker_id);
                            self.selected_workers.sort_unstable();
                        }
                    }
                    KeyCode::Char('a' | 'A') => {
                        // Select all workers
                        self.selected_workers = self.data.workers.as_ref().clone();
                    }
                    KeyCode::Char('n' | 'N') => {
                        // Select none
                        self.selected_workers.clear();
                    }
                    KeyCode::Enter => {
                        // Apply filter
                        self.view_mode = ViewMode::Analysis;
                        if self.selected_workers.is_empty() {
                            // If no workers selected, show all
                            self.selected_workers = self.data.workers.as_ref().clone();
                        }
                        self.hotspot_view.filter_by_workers(&self.selected_workers, &self.data);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Render worker filter overlay
    fn render_worker_filter(&self, f: &mut ratatui::Frame, area: Rect) {
        // Create centered popup
        let popup_area = {
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Min(0),
                    Constraint::Percentage(20),
                ])
                .split(area);

            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30),
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                ])
                .split(vertical[1])[1]
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled("Select Workers to Filter", STYLE_HEADING)),
            Line::from("─".repeat(popup_area.width.saturating_sub(4) as usize)),
            Line::from(""),
        ];

        // List workers with checkboxes
        for (idx, &worker_id) in self.data.workers.iter().enumerate() {
            let is_cursor = idx == self.worker_filter_cursor;
            let is_selected = self.selected_workers.contains(&worker_id);

            let cursor = if is_cursor { "▶ " } else { "  " };
            let checkbox = if is_selected { "[✓] " } else { "[ ] " };

            let style = if is_cursor {
                Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::REVERSED)
            } else if is_selected {
                Style::default().fg(HUD_GREEN)
            } else {
                Style::default().fg(INFO_DIM)
            };

            lines.push(Line::from(vec![
                Span::raw(cursor),
                Span::styled(format!("{checkbox}Worker {worker_id}"), style),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from("─".repeat(popup_area.width.saturating_sub(4) as usize)));
        lines.push(Line::from(vec![
            Span::styled("[Space]", Style::default().fg(CAUTION_AMBER)),
            Span::raw(" Toggle  "),
            Span::styled("[A]", Style::default().fg(CAUTION_AMBER)),
            Span::raw(" All  "),
            Span::styled("[N]", Style::default().fg(CAUTION_AMBER)),
            Span::raw(" None  "),
            Span::styled("[Enter]", Style::default().fg(CAUTION_AMBER)),
            Span::raw(" Apply"),
        ]));

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Worker Filter ({} selected)", self.selected_workers.len()))
                .style(Style::default().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
        );

        f.render_widget(ratatui::widgets::Clear, popup_area);
        f.render_widget(widget, popup_area);
    }

    /// Render help overlay
    #[allow(clippy::unused_self)]
    fn render_help(&self, f: &mut ratatui::Frame, area: Rect) {
        render_help_overlay(f, area);
    }

    /// Render search input overlay
    fn render_search_input(&self, f: &mut ratatui::Frame, area: Rect) {
        // Create centered popup
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

        let search_text = format!("Search: {}_", self.search_query);
        let search_widget = Paragraph::new(search_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Filter Functions")
                    .style(Style::default().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
            )
            .style(Style::default().fg(CAUTION_AMBER));

        f.render_widget(ratatui::widgets::Clear, popup_area);
        f.render_widget(search_widget, popup_area);
    }

    /// Render drill-down details for selected function
    fn render_drilldown(&self, f: &mut ratatui::Frame, area: Rect) {
        if let Some(hotspot) = self.hotspot_view.get_selected() {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled("FUNCTION DETAILS", STYLE_HEADING)),
                Line::from("─".repeat(area.width as usize - 4)),
                Line::from(""),
            ];

            // Severity color based on CPU percentage
            let severity_color = match hotspot.percentage {
                p if p > 40.0 => CRITICAL_RED,
                p if p > 20.0 => CAUTION_AMBER,
                _ => HUD_GREEN,
            };

            // Function name (full, not truncated)
            lines.push(Line::from(vec![
                Span::styled("Name: ", STYLE_LABEL),
                Span::styled(hotspot.name.as_str(), Style::new().fg(HUD_GREEN)),
            ]));
            lines.push(Line::from(""));

            // CPU usage
            lines.push(Line::from(vec![
                Span::styled("CPU Usage: ", STYLE_LABEL),
                Span::styled(
                    format!("{:.1}%", hotspot.percentage),
                    Style::new().fg(severity_color),
                ),
                Span::styled(format!(" ({} samples)", hotspot.count), STYLE_DIM),
            ]));
            lines.push(Line::from(""));

            // Source location
            let location = hotspot.file.as_ref().map_or_else(
                || "(no debug symbols)".into(),
                |f| format!("{}:{}", f, hotspot.line.unwrap_or(0)),
            );
            lines.push(Line::from(vec![
                Span::styled("Location: ", STYLE_LABEL),
                Span::styled(location, STYLE_DIM),
            ]));
            lines.push(Line::from(""));

            // Worker breakdown
            lines.push(Line::from(Span::styled("Worker Distribution:", STYLE_LABEL)));
            lines.push(Line::from(""));

            let mut worker_list: Vec<_> = hotspot.workers.iter().collect();
            worker_list.sort_unstable_by(|a, b| b.1.cmp(a.1));

            for (&worker_id, &count) in &worker_list {
                let percentage = (count as f64 / hotspot.count as f64) * 100.0;
                const BAR_WIDTH: usize = 30;
                let filled = ((percentage / 100.0) * BAR_WIDTH as f64) as usize;
                let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(BAR_WIDTH - filled));

                lines.push(Line::from(vec![
                    Span::raw(format!("  Worker {worker_id:2}: ")),
                    Span::styled(bar, Style::new().fg(HUD_GREEN)),
                    Span::styled(format!(" {percentage:.0}% ({count} samples)"), STYLE_DIM),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from("─".repeat(area.width as usize - 4)));
            lines.push(Line::from(vec![
                Span::styled("[ESC]", STYLE_KEY),
                Span::styled(" Back to Analysis", STYLE_DIM),
            ]));

            let paragraph = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Function Detail View")
                    .style(Style::new().bg(BACKGROUND)),
            );

            f.render_widget(paragraph, area);
        }
    }

    /// Run the TUI event loop
    ///
    /// # Errors
    /// Returns an error if terminal setup or rendering fails
    pub fn run(mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        loop {
            // Draw UI
            terminal.draw(|f| {
                let outer_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // Header
                        Constraint::Min(0),    // Main glass cockpit panels
                        Constraint::Length(3), // Status bar
                    ])
                    .split(f.area());

                // Header - tactical display
                let header = Paragraph::new(vec![Line::from(vec![
                    Span::styled("HUD", STYLE_HEADING),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled("REPLAY", Style::new().fg(CAUTION_AMBER)),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(format!("{:.1}s", self.data.duration), Style::new().fg(HUD_GREEN)),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(
                        format!("{} evts", self.data.events.len()),
                        Style::new().fg(HUD_GREEN),
                    ),
                ])])
                .block(
                    Block::default().borders(Borders::ALL).border_style(Style::new().fg(HUD_GREEN)),
                );
                f.render_widget(header, outer_layout[0]);

                // Main content area - show different views based on mode
                let main_area = outer_layout[1];

                match self.view_mode {
                    ViewMode::Analysis
                    | ViewMode::Search
                    | ViewMode::WorkerFilter
                    | ViewMode::Help => {
                        // Glass Cockpit: Four-panel layout (2x2 grid)

                        // Split into top and bottom rows
                        let rows = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Percentage(50), // Top row
                                Constraint::Percentage(50), // Bottom row
                            ])
                            .split(main_area);

                        // Top row: Status (left) | Hotspots (right)
                        let top_cols = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(30), // Status panel
                                Constraint::Percentage(70), // Hotspots panel
                            ])
                            .split(rows[0]);

                        // Bottom row: Workers (left) | Timeline (right)
                        let bottom_cols = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([
                                Constraint::Percentage(30), // Workers panel
                                Constraint::Percentage(70), // Timeline panel
                            ])
                            .split(rows[1]);

                        // Render all four panels
                        self.status_panel.render(f, top_cols[0], &self.data);
                        self.hotspot_view.render(f, top_cols[1], &self.data);
                        self.workers_panel.render(f, bottom_cols[0], &self.data);
                        self.timeline_view.render(f, bottom_cols[1], &self.data);

                        // Overlay based on mode
                        if self.view_mode == ViewMode::Search {
                            self.render_search_input(f, main_area);
                        } else if self.view_mode == ViewMode::WorkerFilter {
                            self.render_worker_filter(f, main_area);
                        } else if self.view_mode == ViewMode::Help {
                            self.render_help(f, main_area);
                        }
                    }
                    ViewMode::DrillDown => {
                        // Show drill-down view
                        self.render_drilldown(f, main_area);
                    }
                }

                // Status bar keybinds
                let status_line = match self.view_mode {
                    ViewMode::Analysis => {
                        let mode = if self.hotspot_view.is_filtered() {
                            Span::styled("[Filtered]", Style::default().fg(CAUTION_AMBER))
                        } else {
                            Span::styled("[Ready]", Style::default().fg(HUD_GREEN))
                        };
                        Line::from(vec![
                            Span::styled("Q", Style::default().fg(CAUTION_AMBER)),
                            Span::styled(":Quit ", Style::default().fg(INFO_DIM)),
                            Span::styled("/", Style::default().fg(CAUTION_AMBER)),
                            Span::styled(":Search ", Style::default().fg(INFO_DIM)),
                            Span::styled("F", Style::default().fg(CAUTION_AMBER)),
                            Span::styled(":Filter ", Style::default().fg(INFO_DIM)),
                            Span::styled("?", Style::default().fg(CAUTION_AMBER)),
                            Span::styled(":Help ", Style::default().fg(INFO_DIM)),
                            mode,
                        ])
                    }
                    ViewMode::DrillDown => Line::from(vec![
                        Span::styled("ESC", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":Back ", Style::default().fg(INFO_DIM)),
                        Span::styled("[Detail]", Style::default().fg(CAUTION_AMBER)),
                    ]),
                    ViewMode::Search => Line::from(vec![
                        Span::styled("Enter", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":Apply ", Style::default().fg(INFO_DIM)),
                        Span::styled("ESC", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":Cancel ", Style::default().fg(INFO_DIM)),
                        Span::styled("[Search]", Style::default().fg(CAUTION_AMBER)),
                    ]),
                    ViewMode::WorkerFilter => Line::from(vec![
                        Span::styled("Space", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":Toggle ", Style::default().fg(INFO_DIM)),
                        Span::styled("A", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":All ", Style::default().fg(INFO_DIM)),
                        Span::styled("N", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":None ", Style::default().fg(INFO_DIM)),
                        Span::styled("[Filter]", Style::default().fg(CAUTION_AMBER)),
                    ]),
                    ViewMode::Help => Line::from(vec![
                        Span::styled("Any key", Style::default().fg(CAUTION_AMBER)),
                        Span::styled(":Close ", Style::default().fg(INFO_DIM)),
                        Span::styled("[Help]", Style::default().fg(HUD_GREEN)),
                    ]),
                };

                let status = Paragraph::new(vec![status_line]).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(HUD_GREEN)),
                );
                f.render_widget(status, outer_layout[2]);
            })?;

            // Handle input
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code);
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        // Cleanup terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
        terminal.show_cursor()?;

        Ok(())
    }
}

// =============================================================================
// OVERLAY RENDERERS
// =============================================================================
//
// Standalone functions for rendering modal overlays (help, drilldown, search).
// These are shared between live and replay modes to avoid code duplication.

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
            "  operations that don't yield at .await — they hog the thread.",
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
            Span::styled("Thread utilization. All pegged = can't take more work.", STYLE_DIM),
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
        Line::from(Span::styled(
            "  • std::fs — sync file I/O (use tokio::fs)",
            STYLE_DIM,
        )),
        Line::from(Span::styled(
            "  • DNS lookup — std::net blocks (use tokio::net)",
            STYLE_DIM,
        )),
        Line::from(Span::styled(
            "  • compression — flate2/zstd (use spawn_blocking)",
            STYLE_DIM,
        )),
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

/// Create a centered popup area with given width percentage and height in lines
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

/// Render drilldown overlay for a selected hotspot (standalone version for live mode)
fn render_drilldown_overlay(
    f: &mut ratatui::Frame,
    area: Rect,
    hotspot: &crate::analysis::FunctionHotspot,
) {
    let popup_area = centered_popup(area, 70, 24);
    let separator = "─".repeat(popup_area.width.saturating_sub(4) as usize);

    // Severity color based on CPU percentage
    let severity_color = match hotspot.percentage {
        p if p > 40.0 => CRITICAL_RED,
        p if p > 20.0 => CAUTION_AMBER,
        _ => HUD_GREEN,
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled("FUNCTION DETAILS", STYLE_HEADING)),
        Line::from(separator.as_str()),
        Line::from(""),
        // Function name
        Line::from(vec![
            Span::styled("Name: ", STYLE_LABEL),
            Span::styled(hotspot.name.as_str(), Style::new().fg(HUD_GREEN)),
        ]),
        Line::from(""),
        // CPU usage
        Line::from(vec![
            Span::styled("CPU: ", STYLE_LABEL),
            Span::styled(format!("{:.1}%", hotspot.percentage), Style::new().fg(severity_color)),
            Span::styled(format!(" ({} samples)", hotspot.count), STYLE_DIM),
        ]),
        Line::from(""),
    ];

    // Source location
    let location = hotspot.file.as_ref().map_or_else(
        || "(no debug symbols)".into(),
        |f| format!("{}:{}", f, hotspot.line.unwrap_or(0)),
    );
    lines.push(Line::from(vec![
        Span::styled("Location: ", STYLE_LABEL),
        Span::styled(location, STYLE_DIM),
    ]));
    lines.push(Line::from(""));

    // Worker breakdown
    lines.push(Line::from(Span::styled("Worker Distribution:", STYLE_LABEL)));

    let mut worker_list: Vec<_> = hotspot.workers.iter().collect();
    worker_list.sort_unstable_by(|a, b| b.1.cmp(a.1));

    const BAR_WIDTH: usize = 20;
    for (&worker_id, &count) in worker_list.iter().take(6) {
        let pct = (count as f64 / hotspot.count as f64) * 100.0;
        let filled = ((pct / 100.0) * BAR_WIDTH as f64) as usize;
        let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(BAR_WIDTH - filled));
        lines.push(Line::from(vec![
            Span::raw(format!("  W{worker_id:2}: ")),
            Span::styled(bar, Style::new().fg(HUD_GREEN)),
            Span::styled(format!(" {pct:.0}%"), STYLE_DIM),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("ESC", STYLE_KEY),
        Span::styled(" to close", STYLE_DIM),
    ]));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Function Detail ")
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
            // Worker filter not implemented in live mode
            ViewMode::WorkerFilter => {}
        }
    }

    /// Rebuild hotspot view from trace data while preserving UI state
    ///
    /// This is called on each render cycle to reflect new events while
    /// maintaining the user's current selection and any active filters.
    fn update_hotspot_view(&mut self, trace_data: &TraceData) {
        let (old_selected, old_filter) = self
            .hotspot_view
            .as_ref()
            .map_or((0, false), |hv| (hv.selected_index, hv.is_filtered()));

        let mut new_view = HotspotView::new(trace_data);

        // Restore selection if still valid
        if old_selected < new_view.hotspots.len() {
            new_view.selected_index = old_selected;
        }

        // Re-apply filter if active
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
    const UPDATE_INTERVAL: Duration = Duration::from_millis(100); // 10 Hz refresh

    // -------------------------------------------------------------------------
    // Main Event Loop
    // -------------------------------------------------------------------------
    loop {
        // Drain all pending events from eBPF (non-blocking)
        while let Ok(event) = event_rx.try_recv() {
            app.live_data.add_event(event);
        }

        // Snapshot current data for rendering
        let trace_data = app.live_data.as_trace_data();

        // Redraw periodically
        if last_update.elapsed() >= UPDATE_INTERVAL {
            // Update hotspot view (preserves selection)
            app.update_hotspot_view(&trace_data);

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

                // Header - tactical live display
                let pid_display = pid.map_or_else(|| "---".to_string(), |p| p.to_string());
                let rate = if trace_data.duration > 0.0 {
                    trace_data.events.len() as f64 / trace_data.duration
                } else {
                    0.0
                };
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
                    Span::styled(
                        format!("{:.1}s", trace_data.duration),
                        Style::new().fg(HUD_GREEN),
                    ),
                    Span::styled(" | ", STYLE_DIM),
                    Span::styled(
                        format!("{} evts", trace_data.events.len()),
                        Style::new().fg(CAUTION_AMBER),
                    ),
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

        // Handle keyboard input
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

        std::thread::sleep(Duration::from_millis(10));
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
