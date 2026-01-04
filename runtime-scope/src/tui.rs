use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io;
use std::path::Path;

mod timeline;
mod hotspot;
mod status;
mod workers;

use timeline::TimelineView;
use hotspot::HotspotView;
use status::StatusPanel;
use workers::WorkersPanel;

// HUD color scheme (F-35 inspired)
const HUD_GREEN: Color = Color::Rgb(0, 255, 0);
const CRITICAL_RED: Color = Color::Rgb(255, 0, 0);
const CAUTION_AMBER: Color = Color::Rgb(255, 191, 0);
const INFO_DIM: Color = Color::Rgb(0, 180, 0);
const BACKGROUND: Color = Color::Rgb(0, 20, 0);

/// Represents a single trace event
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub name: String,
    pub worker_id: u32,
    pub tid: u32,
    pub timestamp: f64,
    pub cpu: u32,
    pub detection_method: Option<u32>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Internal data model for profiler trace
#[derive(Debug)]
pub struct TraceData {
    pub events: Vec<TraceEvent>,
    pub workers: Vec<u32>,
    pub duration: f64,
}

impl TraceData {
    /// Parse trace.json into our internal representation
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        let mut events = Vec::new();
        let mut workers = std::collections::HashSet::new();
        let mut max_timestamp = 0.0f64;

        if let Some(trace_events) = json["traceEvents"].as_array() {
            for event in trace_events {
                // Only process "B" (begin) events for now
                if event["ph"].as_str() != Some("B") {
                    continue;
                }

                let name = event["name"].as_str().unwrap_or("unknown").to_string();
                let worker_id = event["args"]["worker_id"].as_u64().unwrap_or(0) as u32;
                let tid = event["tid"].as_u64().unwrap_or(0) as u32;
                let timestamp = event["ts"].as_f64().unwrap_or(0.0) / 1_000_000.0; // Convert µs to seconds
                let cpu = event["args"]["cpu_id"].as_u64().unwrap_or(0) as u32;
                let detection_method = event["args"]["detection_method"].as_u64().map(|v| v as u32);
                let file = event["args"]["file"].as_str().map(|s| s.to_string());
                let line = event["args"]["line"].as_u64().map(|v| v as u32);

                workers.insert(worker_id);
                max_timestamp = max_timestamp.max(timestamp);

                events.push(TraceEvent {
                    name,
                    worker_id,
                    tid,
                    timestamp,
                    cpu,
                    detection_method,
                    file,
                    line,
                });
            }
        }

        let mut workers: Vec<u32> = workers.into_iter().collect();
        workers.sort();

        Ok(TraceData {
            events,
            workers,
            duration: max_timestamp,
        })
    }
}

/// Main TUI application state
pub struct App {
    data: TraceData,
    status_panel: StatusPanel,
    hotspot_view: HotspotView,
    workers_panel: WorkersPanel,
    timeline_view: TimelineView,
    should_quit: bool,
}

impl App {
    pub fn new(data: TraceData) -> Self {
        let status_panel = StatusPanel::new(&data);
        let hotspot_view = HotspotView::new(&data);
        let workers_panel = WorkersPanel::new(&data);
        let timeline_view = TimelineView::new(&data);

        Self {
            data,
            status_panel,
            hotspot_view,
            workers_panel,
            timeline_view,
            should_quit: false,
        }
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.should_quit = true,
            KeyCode::Up => self.hotspot_view.scroll_up(),
            KeyCode::Down => self.hotspot_view.scroll_down(),
            _ => {}
        }
    }

    /// Run the TUI event loop
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
                let header = Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("runtime-scope", Style::default().fg(HUD_GREEN).add_modifier(Modifier::BOLD)),
                        Span::styled(" v0.1.0", Style::default().fg(INFO_DIM)),
                        Span::raw("    PID: "),
                        Span::styled("trace.json", Style::default().fg(INFO_DIM)),
                        Span::raw("    Duration: "),
                        Span::styled(format!("{:.1}s", self.data.duration), Style::default().fg(HUD_GREEN)),
                    ]),
                ])
                .block(Block::default().borders(Borders::ALL));
                f.render_widget(header, outer_layout[0]);

                // Glass Cockpit: Four-panel layout (2x2 grid)
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
                self.status_panel.render(f, top_cols[0], &self.data);
                self.hotspot_view.render(f, top_cols[1], &self.data);
                self.workers_panel.render(f, bottom_cols[0], &self.data);
                self.timeline_view.render(f, bottom_cols[1], &self.data);

                // Status bar
                let status = Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("[Q]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Quit  "),
                        Span::styled("[↑↓]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Scroll  "),
                        Span::styled("[?]", Style::default().fg(CAUTION_AMBER)),
                        Span::raw(" Help    "),
                        Span::styled("Mode: ANALYSIS", Style::default().fg(HUD_GREEN)),
                    ]),
                ])
                .block(Block::default().borders(Borders::ALL));
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
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }
}
