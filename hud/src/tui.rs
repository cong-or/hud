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

/// View mode for the TUI
#[derive(Debug, Clone, PartialEq)]
enum ViewMode {
    Analysis,
    DrillDown,
    Search,
    WorkerFilter,
}

/// Main TUI application state
pub struct App {
    data: TraceData,
    status_panel: StatusPanel,
    hotspot_view: HotspotView,
    workers_panel: WorkersPanel,
    timeline_view: TimelineView,
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
                    _ => {}
                }
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
            Line::from(vec![Span::styled(
                "Select Workers to Filter",
                Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD),
            )]),
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
                .style(Style::default().bg(BACKGROUND).fg(HUD_GREEN)),
        );

        f.render_widget(widget, popup_area);
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
                    .style(Style::default().bg(BACKGROUND).fg(HUD_GREEN)),
            )
            .style(Style::default().fg(CAUTION_AMBER));

        f.render_widget(search_widget, popup_area);
    }

    /// Render drill-down details for selected function
    fn render_drilldown(&self, f: &mut ratatui::Frame, area: Rect) {
        if let Some(hotspot) = self.hotspot_view.get_selected() {
            let mut lines = vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "FUNCTION DETAILS",
                    Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD),
                )]),
                Line::from("─".repeat(area.width as usize - 4)),
                Line::from(""),
            ];

            // Function name (full, not truncated)
            lines.push(Line::from(vec![
                Span::styled(
                    "Name: ",
                    Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(&hotspot.name, Style::default().fg(HUD_GREEN)),
            ]));
            lines.push(Line::from(""));

            // CPU usage
            lines.push(Line::from(vec![
                Span::styled(
                    "CPU Usage: ",
                    Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:.1}%", hotspot.percentage),
                    Style::default().fg(if hotspot.percentage > 40.0 {
                        CRITICAL_RED
                    } else if hotspot.percentage > 20.0 {
                        CAUTION_AMBER
                    } else {
                        HUD_GREEN
                    }),
                ),
                Span::raw(format!(" ({} samples)", hotspot.count)),
            ]));
            lines.push(Line::from(""));

            // Source location
            if let Some(ref file) = hotspot.file {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Location: ",
                        Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{}:{}", file, hotspot.line.unwrap_or(0)),
                        Style::default().fg(INFO_DIM),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Location: ",
                        Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("(no debug symbols)", Style::default().fg(INFO_DIM)),
                ]));
            }
            lines.push(Line::from(""));

            // Worker breakdown
            lines.push(Line::from(vec![Span::styled(
                "Worker Distribution:",
                Style::default().fg(CAUTION_AMBER).add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(""));

            let mut worker_list: Vec<_> = hotspot.workers.iter().collect();
            worker_list.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count descending

            for (worker_id, count) in worker_list {
                let percentage = (*count as f64 / hotspot.count as f64) * 100.0;
                let bar_width = 30;
                let filled = ((percentage / 100.0) * bar_width as f64) as usize;
                let empty = bar_width - filled;
                let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(empty));

                lines.push(Line::from(vec![
                    Span::raw(format!("  Worker {worker_id:2}: ")),
                    Span::styled(bar, Style::default().fg(HUD_GREEN)),
                    Span::raw(format!(" {percentage:.0}% ({count} samples)")),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from("─".repeat(area.width as usize - 4)));
            lines.push(Line::from(vec![
                Span::styled("[ESC]", Style::default().fg(CAUTION_AMBER)),
                Span::raw(" Back to Analysis"),
            ]));

            let paragraph = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Function Detail View")
                    .style(Style::default().bg(BACKGROUND)),
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

                // Header
                let header = Paragraph::new(vec![Line::from(vec![
                    Span::styled(
                        "hud",
                        Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" v0.1.0", Style::default().fg(INFO_DIM)),
                    Span::raw("    PID: "),
                    Span::styled("trace.json", Style::default().fg(INFO_DIM)),
                    Span::raw("    Duration: "),
                    Span::styled(
                        format!("{:.1}s", self.data.duration),
                        Style::default().fg(HUD_GREEN),
                    ),
                ])])
                .block(Block::default().borders(Borders::ALL));
                f.render_widget(header, outer_layout[0]);

                // Main content area - show different views based on mode
                let main_area = outer_layout[1];

                match self.view_mode {
                    ViewMode::Analysis | ViewMode::Search | ViewMode::WorkerFilter => {
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

                        // If in search mode, overlay search input box
                        if matches!(self.view_mode, ViewMode::Search) {
                            self.render_search_input(f, main_area);
                        }

                        // If in worker filter mode, overlay worker selection
                        if matches!(self.view_mode, ViewMode::WorkerFilter) {
                            self.render_worker_filter(f, main_area);
                        }
                    }
                    ViewMode::DrillDown => {
                        // Show drill-down view
                        self.render_drilldown(f, main_area);
                    }
                }

                // Status bar - dynamic based on mode
                let status_line = match &self.view_mode {
                    ViewMode::Analysis => {
                        let mut spans = vec![
                            Span::styled("[Q]", Style::default().fg(CAUTION_AMBER)),
                            Span::raw(" Quit  "),
                            Span::styled("[↑↓]", Style::default().fg(CAUTION_AMBER)),
                            Span::raw(" Nav  "),
                            Span::styled("[Enter]", Style::default().fg(CAUTION_AMBER)),
                            Span::raw(" Detail  "),
                            Span::styled("[/]", Style::default().fg(CAUTION_AMBER)),
                            Span::raw(" Search  "),
                            Span::styled("[F]", Style::default().fg(CAUTION_AMBER)),
                            Span::raw(" Filter  "),
                            Span::styled("[WASD]", Style::default().fg(CAUTION_AMBER)),
                            Span::raw(" Zoom/Pan  "),
                        ];

                        if self.hotspot_view.is_filtered() {
                            spans.push(Span::styled("[C]", Style::default().fg(CAUTION_AMBER)));
                            spans.push(Span::raw(" Clear    "));
                            spans
                                .push(Span::styled("FILTERED", Style::default().fg(CAUTION_AMBER)));
                        } else {
                            spans.push(Span::styled("ANALYSIS", Style::default().fg(HUD_GREEN)));
                        }

                        Line::from(spans)
                    }
                    ViewMode::DrillDown => Line::from(vec![
                        Span::styled("[ESC]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Back  "),
                        Span::styled("[Q]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Quit    "),
                        Span::styled("Mode: DRILL-DOWN", Style::default().fg(CAUTION_AMBER)),
                    ]),
                    ViewMode::Search => Line::from(vec![
                        Span::styled("[Enter]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Apply  "),
                        Span::styled("[ESC]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Cancel  "),
                        Span::styled("[Backspace]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Delete    "),
                        Span::styled("Mode: SEARCH", Style::default().fg(CAUTION_AMBER)),
                    ]),
                    ViewMode::WorkerFilter => Line::from(vec![
                        Span::styled("[↑↓]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Navigate  "),
                        Span::styled("[Space]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Toggle  "),
                        Span::styled("[A]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" All  "),
                        Span::styled("[N]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" None  "),
                        Span::styled("[Enter]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Apply    "),
                        Span::styled("Mode: WORKER FILTER", Style::default().fg(CAUTION_AMBER)),
                    ]),
                };

                let status =
                    Paragraph::new(vec![status_line]).block(Block::default().borders(Borders::ALL));
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

/// Run TUI in live mode, receiving events from a channel
///
/// # Errors
/// Returns an error if terminal setup or rendering fails
pub fn run_live(event_rx: Receiver<TraceEvent>, pid: Option<i32>) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Live data accumulator
    let mut live_data = LiveData::new();
    let mut should_quit = false;
    let mut last_update = std::time::Instant::now();
    const UPDATE_INTERVAL: Duration = Duration::from_millis(100); // Redraw every 100ms

    // Main loop
    loop {
        // Process incoming events (non-blocking)
        while let Ok(event) = event_rx.try_recv() {
            live_data.add_event(event);
        }

        // Convert to TraceData for rendering
        let trace_data = live_data.as_trace_data();

        // Only redraw if we have data and enough time has passed
        if !trace_data.events.is_empty() && last_update.elapsed() >= UPDATE_INTERVAL {
            // Rebuild panels with latest data
            let status_panel = StatusPanel::new(&trace_data);
            let hotspot_view = HotspotView::new(&trace_data);
            let workers_panel = WorkersPanel::new(&trace_data);
            let timeline_view = TimelineView::new(&trace_data);

            terminal.draw(|f| {
                let outer_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // Header
                        Constraint::Min(0),    // Main glass cockpit panels
                        Constraint::Length(3), // Status bar
                    ])
                    .split(f.area());

                // Header with live indicator
                let pid_display = pid.map_or_else(|| "unknown".to_string(), |p| p.to_string());
                let header = Paragraph::new(vec![Line::from(vec![
                    Span::styled(
                        "hud",
                        Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" v0.1.0", Style::default().fg(INFO_DIM)),
                    Span::raw("    PID: "),
                    Span::styled(pid_display, Style::default().fg(HUD_GREEN)),
                    Span::raw("    Duration: "),
                    Span::styled(
                        format!("{:.1}s", trace_data.duration),
                        Style::default().fg(HUD_GREEN),
                    ),
                    Span::raw("    Events: "),
                    Span::styled(
                        format!("{}", trace_data.events.len()),
                        Style::default().fg(CAUTION_AMBER),
                    ),
                    Span::raw("    "),
                    Span::styled(
                        "● LIVE",
                        Style::default().fg(CRITICAL_RED).add_modifier(Modifier::BOLD),
                    ),
                ])])
                .block(Block::default().borders(Borders::ALL));
                f.render_widget(header, outer_layout[0]);

                // Main content area - Glass Cockpit layout
                let main_area = outer_layout[1];

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
                status_panel.render(f, top_cols[0], &trace_data);
                hotspot_view.render(f, top_cols[1], &trace_data);
                workers_panel.render(f, bottom_cols[0], &trace_data);
                timeline_view.render(f, bottom_cols[1], &trace_data);

                // Status bar
                let status_line = Line::from(vec![
                    Span::styled("[Q]", Style::default().fg(CAUTION_AMBER)),
                    Span::raw(" Quit    "),
                    Span::styled(
                        "Mode: LIVE PROFILING",
                        Style::default().fg(CRITICAL_RED).add_modifier(Modifier::BOLD),
                    ),
                ]);

                let status =
                    Paragraph::new(vec![status_line]).block(Block::default().borders(Borders::ALL));
                f.render_widget(status, outer_layout[2]);
            })?;

            last_update = std::time::Instant::now();
        }

        // Handle keyboard input (non-blocking)
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let KeyCode::Char('q' | 'Q') = key.code {
                        should_quit = true;
                    }
                }
            }
        }

        if should_quit {
            break;
        }

        // Small sleep to avoid busy loop
        std::thread::sleep(Duration::from_millis(10));
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
