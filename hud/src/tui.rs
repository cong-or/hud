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
    widgets::{block::BorderType, Block, Borders, Paragraph},
    Terminal,
};
use std::io;
use std::time::Duration;

pub mod hotspot; // Public for testing
mod layout;
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

/// Get severity color based on CPU percentage thresholds.
///
/// - Green: < 20% CPU (nominal)
/// - Amber: 20-40% CPU (caution)
/// - Red: > 40% CPU (critical)
const fn severity_color(percentage: f64) -> ratatui::style::Color {
    match percentage {
        p if p > 40.0 => CRITICAL_RED,
        p if p > 20.0 => CAUTION_AMBER,
        _ => HUD_GREEN,
    }
}

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
    /// Detailed view of all functions in a file (from Files view)
    FileDrillDown,
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

/// Minimum terminal size for overlay content.
const MIN_OVERLAY_WIDTH: u16 = 80;
const MIN_OVERLAY_HEIGHT: u16 = 24;

/// Render a "terminal too small" message if below minimum size.
/// Returns true if message was rendered (caller should return early).
fn render_size_warning(f: &mut ratatui::Frame, area: Rect, context: &str) -> bool {
    if area.width >= MIN_OVERLAY_WIDTH && area.height >= MIN_OVERLAY_HEIGHT {
        return false;
    }

    let popup_area = centered_popup(area, 90, 5);
    let msg = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled("Terminal too small", Style::new().fg(CAUTION_AMBER))),
        Line::from(Span::styled(format!("Increase window size to {context}"), STYLE_DIM)),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .style(Style::new().fg(HUD_GREEN)),
    )
    .alignment(ratatui::layout::Alignment::Center);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(msg, popup_area);
    true
}

/// Render the help overlay explaining hud concepts and keyboard shortcuts
fn render_help_overlay(f: &mut ratatui::Frame, area: Rect) {
    if render_size_warning(f, area, "view help") {
        return;
    }

    // Responsive sizing: expand on small terminals, clamp to available space
    let width_pct = if area.width < 80 { 95 } else { 80 };
    let height = 34_u16.min(area.height.saturating_sub(2));
    let popup_area = centered_popup(area, width_pct, height);

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
        // Debug info
        Line::from(Span::styled("  Debug Info", STYLE_HEADING)),
        Line::from(Span::styled(
            "  hud needs debug symbols to show function names and source locations.",
            STYLE_DIM,
        )),
        Line::from(Span::styled(
            "  The Debug % in status shows how many frames have this info.",
            STYLE_DIM,
        )),
        Line::from(Span::styled(
            "  If low, rebuild with: [profile.release] debug = true",
            STYLE_DIM,
        )),
        Line::from(Span::styled("  Frames marked ⚠ in drilldown are missing symbols.", STYLE_DIM)),
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
        Line::from(vec![
            Span::styled("  G", STYLE_KEY),
            Span::styled(" Toggle view (functions ↔ files)   ", STYLE_DIM),
            Span::styled("Y", STYLE_KEY),
            Span::styled(" Yank (in drilldown)", STYLE_DIM),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Press any key to close", STYLE_DIM)),
    ];

    let help_widget = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .title("[ HELP ]")
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
    live_percentage: Option<f64>,
) {
    if render_size_warning(f, area, "view details") {
        return;
    }

    // Use live percentage if available, otherwise frozen value
    let percentage = live_percentage.unwrap_or(hotspot.percentage);

    // Responsive thresholds based on terminal size
    let is_narrow = area.width < 60;
    let is_compact = area.height < 30 || is_narrow;
    let is_minimal = area.height < 20;

    // Adjust content based on available space
    let max_frames = if is_minimal {
        4
    } else if is_compact {
        8
    } else {
        12
    };
    let show_workers = !is_minimal && !hotspot.workers.is_empty();

    let has_call_stack = !hotspot.call_stacks.is_empty();
    let call_stack_lines = if has_call_stack {
        hotspot.call_stacks.first().map_or(0, |s| s.len().min(max_frames)) + 2
    } else {
        0
    };
    let worker_lines = if show_workers { hotspot.workers.len().min(4) + 1 } else { 0 };
    let base_height = if is_compact { 12 } else { 16 };
    let content_height = (base_height + call_stack_lines + worker_lines).min(45) as u16;

    // Responsive sizing: expand on small terminals, clamp to available space
    let width_pct = if is_narrow {
        98
    } else if area.width < 80 {
        95
    } else {
        65
    };
    let popup_height = content_height.min(area.height.saturating_sub(2));

    let popup_area = centered_popup(area, width_pct, popup_height);
    let inner_width = popup_area.width.saturating_sub(4) as usize;

    let sev_color = severity_color(percentage);

    // Build CPU bar - shorter on narrow terminals
    let bar_width = if is_narrow { 10 } else { 20 };
    let cpu_filled = ((percentage / 100.0) * bar_width as f64) as usize;
    let cpu_bar = format!(
        "{}{}",
        "█".repeat(cpu_filled.min(bar_width)),
        "░".repeat(bar_width.saturating_sub(cpu_filled))
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
            Span::styled("  ◈ ", Style::new().fg(sev_color)),
            Span::styled(
                "TARGET ACQUIRED",
                Style::new().fg(sev_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ◈", Style::new().fg(sev_color)),
        ]),
        Line::from(""),
        // Box-drawing characters create targeting brackets
        Line::from(Span::styled("  ┌─", Style::new().fg(sev_color))),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("TGT  ", STYLE_DIM), // Target designation
            Span::styled(name_display, Style::new().fg(HUD_GREEN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("CPU  ", STYLE_DIM), // CPU utilization gauge
            Span::styled(cpu_bar, Style::new().fg(sev_color)),
            Span::styled(
                format!(" {percentage:.1}%"),
                Style::new().fg(sev_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("LOC  ", STYLE_DIM), // Source location
            Span::styled(location, STYLE_DIM),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("HIT  ", STYLE_DIM), // Sample hit count
            Span::styled(format!("{} samples", hotspot.count), STYLE_DIM),
        ]),
        Line::from(Span::styled("  └─", Style::new().fg(sev_color))),
        Line::from(""),
    ];

    // Call trace section - inverted to show: your_code → library → blocking_fn
    if let Some(call_stack) = hotspot.call_stacks.first() {
        lines.push(Line::from(Span::styled("  CALL TRACE", STYLE_DIM)));

        // Reverse stack: show caller (your code) first, blocking function last
        let all_frames: Vec<_> = call_stack.iter().rev().collect();

        // Smart frame selection: always include user frames + context
        let frames: Vec<_> = select_frames_for_display(&all_frames, max_frames);
        let frames_shown = frames.len();
        let last_idx = frames_shown.saturating_sub(1);

        // Find the index of the first (topmost) user code frame for special highlighting
        let first_user_frame_idx = frames.iter().position(|f| f.is_user_code);

        for (i, frame) in frames.iter().enumerate() {
            let arrow = if i == last_idx { "└→" } else { "├→" };

            // Truncate long function names
            let max_len = inner_width.saturating_sub(20);
            let func_display = if frame.function.len() > max_len {
                format!("{}…", &frame.function[..max_len.saturating_sub(1)])
            } else {
                frame.function.clone()
            };

            // Determine if this is the topmost user frame (entry point)
            let is_entry_point = first_user_frame_idx == Some(i);

            // User code in green, library code dimmed
            // Entry point (first user frame) gets bold + marker
            let style = if is_entry_point {
                Style::new().fg(HUD_GREEN).add_modifier(Modifier::BOLD)
            } else if frame.is_user_code {
                Style::new().fg(HUD_GREEN)
            } else {
                STYLE_DIM
            };

            // Format location as "file.rs:42" or empty string
            let location = frame.file.as_ref().map_or(String::new(), |path| {
                let filename =
                    std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
                frame.line.map_or(filename.to_string(), |ln| format!("{filename}:{ln}"))
            });

            // Warning marker for frames without debug info (file path missing but has function name)
            let missing_debug_info = frame.file.is_none() && !frame.function.starts_with("0x");
            let warning_marker = if missing_debug_info {
                Span::styled("⚠ ", Style::new().fg(CAUTION_AMBER))
            } else {
                Span::raw("")
            };

            // Entry point gets flashing diamond targeting brackets
            if is_entry_point {
                let diamond_style = Style::new()
                    .fg(HUD_GREEN)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::SLOW_BLINK);
                lines.push(Line::from(vec![
                    Span::styled(format!("    {arrow} "), STYLE_DIM),
                    warning_marker,
                    Span::styled("◆ ", diamond_style),
                    Span::styled(func_display, style),
                    Span::styled(" ◆", diamond_style),
                    Span::styled(format!("  {location}"), STYLE_DIM),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {arrow} "), STYLE_DIM),
                    warning_marker,
                    Span::styled(func_display, style),
                    Span::styled(format!("  {location}"), STYLE_DIM),
                ]));
            }
        }

        let total = call_stack.len();
        if total > frames_shown {
            lines.push(Line::from(Span::styled(
                format!("       ... ({frames_shown} of {total} frames shown)"),
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

    // Worker breakdown with tactical styling (hidden on minimal terminals)
    if show_workers {
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
        Span::styled(" Close  ", STYLE_DIM),
        Span::styled("[Y]", STYLE_KEY),
        Span::styled(" Yank to clipboard", STYLE_DIM),
    ]));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .title("[ ◈ LOCK ◈ ]")
            .style(Style::new().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
    );

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(widget, popup_area);
}

/// Select frames for display, prioritizing user code visibility.
///
/// When a call stack is very deep (common with async runtimes), we can't show
/// all frames. This function selects the most relevant frames:
///
/// 1. All user code frames (the primary goal)
/// 2. All frames from last user frame to blocking function (the call path)
/// 3. Fill remaining slots from the beginning (runtime entry context)
///
/// This ensures users always see their code and how it leads to the blocking call.
fn select_frames_for_display<'a>(
    frames: &[&'a crate::trace_data::StackFrame],
    max_frames: usize,
) -> Vec<&'a crate::trace_data::StackFrame> {
    // Fast path: if we can show everything, do so
    if frames.len() <= max_frames {
        return frames.to_vec();
    }

    // Find user frame indices
    let user_indices: Vec<usize> =
        frames.iter().enumerate().filter_map(|(i, f)| f.is_user_code.then_some(i)).collect();

    // No user frames: show beginning + end of stack
    if user_indices.is_empty() {
        let half = max_frames / 2;
        let tail_start = frames.len().saturating_sub(max_frames - half);
        return frames.iter().take(half).chain(frames.iter().skip(tail_start)).copied().collect();
    }

    // Use BTreeSet for automatic sorting and deduplication
    let mut selected: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();

    // Priority 1: All user frames
    selected.extend(user_indices.iter().copied());

    // Priority 2: Path from last user frame to blocking function
    let last_user_idx = user_indices.last().copied().unwrap_or(0);
    selected.extend(last_user_idx..frames.len());

    // Priority 3: Fill from start until we hit max_frames
    for i in 0..frames.len() {
        if selected.len() >= max_frames {
            break;
        }
        selected.insert(i);
    }

    // Convert indices to frame references (BTreeSet keeps them sorted)
    selected.into_iter().map(|i| frames[i]).collect()
}

/// Format a hotspot's call trace as plain text for clipboard/debugging.
fn format_hotspot_for_yank(hotspot: &crate::analysis::FunctionHotspot) -> String {
    use crate::classification::FrameOrigin;
    use std::fmt::Write;

    let mut out = String::with_capacity(2048);

    // Header
    writeln!(out, "=== HOTSPOT: {} ===", hotspot.name).ok();
    writeln!(out, "CPU: {:.1}%", hotspot.percentage).ok();
    writeln!(out, "Samples: {}", hotspot.count).ok();

    if let Some(ref file) = hotspot.file {
        match hotspot.line {
            Some(ln) => writeln!(out, "Location: {file}:{ln}").ok(),
            None => writeln!(out, "Location: {file}").ok(),
        };
    }

    // Call trace
    writeln!(out).ok();
    writeln!(out, "CALL TRACE (caller → callee):").ok();

    if let Some(call_stack) = hotspot.call_stacks.first() {
        let last_idx = call_stack.len().saturating_sub(1);

        for (i, frame) in call_stack.iter().rev().enumerate() {
            let origin_tag = match frame.origin {
                FrameOrigin::UserCode => "[USER]",
                FrameOrigin::StdLib => "[STD]",
                FrameOrigin::RuntimeLib => "[RUNTIME]",
                FrameOrigin::ThirdParty => "[3RDPARTY]",
                FrameOrigin::Unknown => "[???]",
            };

            let location = format_frame_location(frame);
            let arrow = if i == last_idx { "└→" } else { "├→" };

            writeln!(out, "  {arrow} {origin_tag:<12} {}  {location}", frame.function).ok();
        }
    } else {
        writeln!(out, "  (no call stack captured)").ok();
    }

    // Worker distribution
    if !hotspot.workers.is_empty() {
        writeln!(out).ok();
        writeln!(out, "WORKERS:").ok();

        let mut worker_list: Vec<_> = hotspot.workers.iter().collect();
        worker_list.sort_unstable_by(|a, b| b.1.cmp(a.1));

        for (&worker_id, &count) in worker_list.iter().take(4) {
            let pct = (count as f64 / hotspot.count as f64) * 100.0;
            writeln!(out, "  W{worker_id}: {pct:.0}% ({count} samples)").ok();
        }
    }

    out
}

/// Format a frame's source location as "filename:line" or just "filename".
fn format_frame_location(frame: &crate::trace_data::StackFrame) -> String {
    frame.file.as_ref().map_or(String::new(), |path| {
        let filename =
            std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
        match frame.line {
            Some(ln) => format!("{filename}:{ln}"),
            None => filename.to_string(),
        }
    })
}

/// Copy hotspot info to system clipboard.
fn yank_hotspot_to_clipboard(hotspot: &crate::analysis::FunctionHotspot) -> Result<()> {
    let text = format_hotspot_for_yank(hotspot);
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(&text)?;
    Ok(())
}

/// Format a file group's info as plain text for clipboard.
fn format_file_group_for_yank(group: &hotspot::FileGroup) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(2048);

    // Header
    writeln!(out, "=== FILE: {} ===", group.file).ok();
    writeln!(out, "Total CPU: {:.1}%", group.percentage).ok();
    writeln!(out, "Functions: {}", group.count).ok();
    writeln!(out).ok();

    // List all functions in this file
    writeln!(out, "HOTSPOT FUNCTIONS:").ok();
    for hotspot in &group.hotspots {
        let location = hotspot
            .file
            .as_ref()
            .map_or(String::new(), |f| hotspot.line.map_or(f.clone(), |ln| format!("{f}:{ln}")));
        writeln!(out, "  {:.1}%  {}  {}", hotspot.percentage, hotspot.name, location).ok();
    }

    out
}

/// Copy file group info to system clipboard.
fn yank_file_group_to_clipboard(group: &hotspot::FileGroup) -> Result<()> {
    let text = format_file_group_for_yank(group);
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(&text)?;
    Ok(())
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
                .border_type(BorderType::Plain)
                .title("[ FILTER ] Enter=apply Esc=cancel")
                .style(Style::default().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
        )
        .style(Style::default().fg(CAUTION_AMBER));

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(search_widget, popup_area);
}

/// Render file drilldown overlay showing all hotspot functions in a file.
///
/// Styled as an F-35 targeting computer UI with:
/// - Severity-colored header based on aggregate CPU%
/// - File path as target designation
/// - Scrollable list of functions with selection highlight
/// - Per-function CPU percentages
fn render_file_drilldown_overlay(
    f: &mut ratatui::Frame,
    area: Rect,
    file_group: &hotspot::FileGroup,
    selected_idx: usize,
    hotspot_view: Option<&HotspotView>,
) {
    if render_size_warning(f, area, "view file details") {
        return;
    }

    // Responsive thresholds
    let is_narrow = area.width < 60;

    // Calculate popup size based on content
    let fn_count = file_group.hotspots.len();
    let visible_fns = fn_count.min(12); // Show up to 12 functions
    let base_height = 10; // Header + footer
    let content_height = (base_height + visible_fns * 2).min(40) as u16;

    let width_pct = if is_narrow {
        98
    } else if area.width < 80 {
        95
    } else {
        70
    };
    let popup_height = content_height.min(area.height.saturating_sub(2));

    let popup_area = centered_popup(area, width_pct, popup_height);
    let inner_width = popup_area.width.saturating_sub(4) as usize;

    let sev_color = severity_color(file_group.percentage);

    // Extract just filename for display
    let display_file = std::path::Path::new(&file_group.file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&file_group.file);

    // Truncate file path if needed
    let max_file_len = inner_width.saturating_sub(10);
    let file_display = if display_file.len() > max_file_len {
        format!("{}…", &display_file[..max_file_len.saturating_sub(1)])
    } else {
        display_file.to_string()
    };

    // Build CPU bar
    let bar_width = if is_narrow { 10 } else { 20 };
    let cpu_filled = ((file_group.percentage / 100.0) * bar_width as f64) as usize;
    let cpu_bar = format!(
        "{}{}",
        "█".repeat(cpu_filled.min(bar_width)),
        "░".repeat(bar_width.saturating_sub(cpu_filled))
    );

    // Build display lines
    let mut lines = vec![
        Line::from(""),
        // Targeting reticle header
        Line::from(vec![
            Span::styled("  ◈ ", Style::new().fg(sev_color)),
            Span::styled("FILE ANALYSIS", Style::new().fg(sev_color).add_modifier(Modifier::BOLD)),
            Span::styled(" ◈", Style::new().fg(sev_color)),
        ]),
        Line::from(""),
        // Targeting brackets
        Line::from(Span::styled("  ┌─", Style::new().fg(sev_color))),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("FILE ", STYLE_DIM),
            Span::styled(file_display, Style::new().fg(HUD_GREEN).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("CPU  ", STYLE_DIM),
            Span::styled(cpu_bar, Style::new().fg(sev_color)),
            Span::styled(
                format!(" {:.1}%", file_group.percentage),
                Style::new().fg(sev_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  │ ", Style::new().fg(sev_color)),
            Span::styled("FNS  ", STYLE_DIM),
            Span::styled(format!("{} hotspot functions", file_group.count), STYLE_DIM),
        ]),
        Line::from(Span::styled("  └─", Style::new().fg(sev_color))),
        Line::from(""),
    ];

    // Function list section
    lines.push(Line::from(Span::styled("  FUNCTIONS (Enter to inspect)", STYLE_DIM)));

    // Calculate scroll window to keep selection visible
    let scroll_offset = selected_idx.saturating_sub(visible_fns.saturating_sub(1));
    let max_name_len = inner_width.saturating_sub(15);

    for (idx, hotspot) in
        file_group.hotspots.iter().enumerate().skip(scroll_offset).take(visible_fns)
    {
        let is_selected = idx == selected_idx;

        // Truncate function name if needed
        let name_display = if hotspot.name.len() > max_name_len {
            format!("{}…", &hotspot.name[..max_name_len.saturating_sub(1)])
        } else {
            hotspot.name.clone()
        };

        // Look up live percentage if available, fall back to frozen value
        let percentage = hotspot_view
            .and_then(|hv| hv.hotspots.iter().find(|h| h.name == hotspot.name))
            .map_or(hotspot.percentage, |h| h.percentage);

        let fn_color = severity_color(percentage);
        let (sel_l, sel_r) = if is_selected { ("▶ ", " ◀") } else { ("  ", "  ") };

        let name_style = if is_selected {
            Style::new().fg(fn_color).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::new().fg(fn_color)
        };

        // Main line: selector, name, percentage
        lines.push(Line::from(vec![
            Span::styled(format!("   {sel_l}"), Style::new().fg(CAUTION_AMBER)),
            Span::styled(name_display, name_style),
            Span::styled(format!(" {percentage:>5.1}%"), Style::new().fg(fn_color)),
            Span::styled(sel_r, Style::new().fg(CAUTION_AMBER)),
        ]));

        // Detail line: source location
        let location = hotspot.line.map_or_else(String::new, |ln| format!("line {ln}"));
        lines.push(Line::from(vec![Span::raw("        "), Span::styled(location, STYLE_DIM)]));
    }

    // Scroll indicator when list is truncated
    if fn_count > visible_fns {
        let shown_end = (scroll_offset + visible_fns).min(fn_count);
        lines.push(Line::from(Span::styled(
            format!("       ... ({}-{} of {fn_count})", scroll_offset + 1, shown_end),
            STYLE_DIM,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [ESC]", STYLE_KEY),
        Span::styled(" Close  ", STYLE_DIM),
        Span::styled("[↑↓]", STYLE_KEY),
        Span::styled(" Navigate  ", STYLE_DIM),
        Span::styled("[Enter]", STYLE_KEY),
        Span::styled(" Inspect  ", STYLE_DIM),
        Span::styled("[Y]", STYLE_KEY),
        Span::styled(" Yank", STYLE_DIM),
    ]));

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .title("[ ◈ FILE LOCK ◈ ]")
            .style(Style::new().bg(ratatui::style::Color::Black).fg(HUD_GREEN)),
    );

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(widget, popup_area);
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
    /// Frozen file group for file drilldown view
    frozen_file_group: Option<hotspot::FileGroup>,
    /// Selected index within file drilldown's function list
    file_drilldown_selected: usize,
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
            frozen_file_group: None,
            file_drilldown_selected: 0,
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
                    // Branch based on hotspot view mode
                    if let Some(hv) = &self.hotspot_view {
                        match hv.view_mode() {
                            hotspot::ViewMode::Functions => {
                                // Freeze the selected hotspot for drilldown view
                                self.frozen_hotspot = hv.get_selected().cloned();
                                if self.frozen_hotspot.is_some() {
                                    self.view_mode = ViewMode::DrillDown;
                                }
                            }
                            hotspot::ViewMode::Files => {
                                // Freeze the selected file group for file drilldown view
                                self.frozen_file_group = hv.get_selected_file_group().cloned();
                                if self.frozen_file_group.is_some() {
                                    self.file_drilldown_selected = 0;
                                    self.view_mode = ViewMode::FileDrillDown;
                                }
                            }
                        }
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
                KeyCode::Char('g' | 'G') => {
                    if let Some(hv) = &mut self.hotspot_view {
                        hv.toggle_view();
                    }
                }
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
            // DrillDown overlay - ESC/Q closes, Y yanks to clipboard
            ViewMode::DrillDown => match key {
                KeyCode::Esc | KeyCode::Char('q' | 'Q') => {
                    self.view_mode = ViewMode::Analysis;
                    self.frozen_hotspot = None;
                }
                KeyCode::Char('y' | 'Y') => {
                    if let Some(ref hotspot) = self.frozen_hotspot {
                        if let Err(e) = yank_hotspot_to_clipboard(hotspot) {
                            log::warn!("Failed to copy to clipboard: {e}");
                        }
                    }
                }
                _ => {}
            },
            // FileDrillDown overlay - navigate functions in file, drill into function, yank
            ViewMode::FileDrillDown => match key {
                KeyCode::Esc | KeyCode::Char('q' | 'Q') => {
                    self.view_mode = ViewMode::Analysis;
                    self.frozen_file_group = None;
                    self.file_drilldown_selected = 0;
                }
                KeyCode::Up => {
                    if self.file_drilldown_selected > 0 {
                        self.file_drilldown_selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if let Some(ref group) = self.frozen_file_group {
                        if self.file_drilldown_selected + 1 < group.hotspots.len() {
                            self.file_drilldown_selected += 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    // Drill into selected function (nested drilldown)
                    if let Some(ref group) = self.frozen_file_group {
                        if let Some(hotspot) = group.hotspots.get(self.file_drilldown_selected) {
                            self.frozen_hotspot = Some(hotspot.clone());
                            self.view_mode = ViewMode::DrillDown;
                        }
                    }
                }
                KeyCode::Char('y' | 'Y') => {
                    if let Some(ref group) = self.frozen_file_group {
                        if let Err(e) = yank_file_group_to_clipboard(group) {
                            log::warn!("Failed to copy to clipboard: {e}");
                        }
                    }
                }
                _ => {}
            },
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
        let (old_selected, old_view_mode) = self
            .hotspot_view
            .as_ref()
            .map_or((0, hotspot::ViewMode::default()), |hv| (hv.selected_index, hv.view_mode()));

        // Get hotspots from HotspotStats (efficient aggregation)
        let hotspots = self.hotspot_stats.to_hotspots();
        let mut new_view = HotspotView::from_hotspots(hotspots);

        // Restore view mode
        new_view.set_view_mode(old_view_mode);

        // Re-apply search filter if active
        if !self.search_query.is_empty() {
            new_view.apply_filter(&self.search_query);
        }

        // Restore selection index if still valid (after filtering)
        if old_selected < new_view.hotspots.len() {
            new_view.selected_index = old_selected;
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
                let area = f.area();

                // Show message if terminal is too small
                if area.width < layout::MIN_WIDTH || area.height < layout::MIN_HEIGHT {
                    let msg = Paragraph::new(vec![
                        Line::from(""),
                        Line::from(Span::styled(
                            "Terminal too small",
                            Style::new().fg(CAUTION_AMBER),
                        )),
                        Line::from(Span::styled(
                            format!("Minimum size: {}x{}", layout::MIN_WIDTH, layout::MIN_HEIGHT),
                            STYLE_DIM,
                        )),
                        Line::from(Span::styled("Increase window size to continue", STYLE_DIM)),
                    ])
                    .alignment(ratatui::layout::Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Plain)
                            .style(Style::new().fg(HUD_GREEN)),
                    );
                    f.render_widget(msg, area);
                    return;
                }

                // Compute responsive layout based on terminal size
                let layout_cfg = layout::compute_layout(area.width, area.height);

                // Build outer layout constraints based on visibility
                let outer_constraints = if layout_cfg.show_status_bar {
                    vec![
                        Constraint::Length(3), // Header
                        Constraint::Min(0),    // Main panels
                        Constraint::Length(3), // Status bar
                    ]
                } else {
                    vec![
                        Constraint::Length(3), // Header
                        Constraint::Min(0),    // Main panels (no status bar)
                    ]
                };

                let outer_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(outer_constraints)
                    .split(area);

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
                        .border_type(BorderType::Plain)
                        .border_style(Style::new().fg(CRITICAL_RED)),
                );
                f.render_widget(header, outer_layout[0]);

                // Main content area - layout depends on terminal size
                let main_area = outer_layout[1];

                if layout_cfg.single_column {
                    // Narrow terminals: stack hotspots + timeline vertically
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                        .split(main_area);

                    if let Some(ref hv) = app.hotspot_view {
                        hv.render(f, rows[0], &trace_data);
                    }
                    timeline_view.render(f, rows[1], &trace_data);
                } else if layout_cfg.show_workers_panel {
                    // Full layout: 2x2 grid
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(main_area);

                    let top_cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(layout_cfg.col_constraints())
                        .split(rows[0]);

                    let bottom_cols = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(layout_cfg.col_constraints())
                        .split(rows[1]);

                    if layout_cfg.show_status_panel {
                        status_panel.render(f, top_cols[0], &trace_data);
                    }
                    if let Some(ref hv) = app.hotspot_view {
                        hv.render(f, top_cols[1], &trace_data);
                    }
                    workers_panel.render(f, bottom_cols[0], &trace_data);
                    timeline_view.render(f, bottom_cols[1], &trace_data);
                } else {
                    // Compact/minimal: hotspots + timeline, optionally with status panel
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                        .split(main_area);

                    if layout_cfg.show_status_panel {
                        let top_cols = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints(layout_cfg.col_constraints())
                            .split(rows[0]);

                        status_panel.render(f, top_cols[0], &trace_data);
                        if let Some(ref hv) = app.hotspot_view {
                            hv.render(f, top_cols[1], &trace_data);
                        }
                    } else if let Some(ref hv) = app.hotspot_view {
                        // Minimal: hotspots take full width
                        hv.render(f, rows[0], &trace_data);
                    }
                    timeline_view.render(f, rows[1], &trace_data);
                }

                // Search overlay
                if app.view_mode == ViewMode::Search {
                    render_search_overlay(f, area, &app.search_query);
                }

                // Help overlay
                if app.view_mode == ViewMode::Help {
                    render_help_overlay(f, area);
                }

                // DrillDown overlay (frozen snapshot with live CPU percentage)
                if app.view_mode == ViewMode::DrillDown {
                    if let Some(ref hotspot) = app.frozen_hotspot {
                        // Look up live percentage by function name
                        let live_pct = app.hotspot_view.as_ref().and_then(|hv| {
                            hv.hotspots
                                .iter()
                                .find(|h| h.name == hotspot.name)
                                .map(|h| h.percentage)
                        });
                        render_drilldown_overlay(f, area, hotspot, live_pct);
                    }
                }

                // FileDrillDown overlay (shows all functions in a file)
                if app.view_mode == ViewMode::FileDrillDown {
                    if let Some(ref file_group) = app.frozen_file_group {
                        render_file_drilldown_overlay(
                            f,
                            area,
                            file_group,
                            app.file_drilldown_selected,
                            app.hotspot_view.as_ref(),
                        );
                    }
                }

                // Status bar keybinds - show context-appropriate keys (only if visible)
                if layout_cfg.show_status_bar {
                    let status_line = match app.view_mode {
                        ViewMode::DrillDown => Line::from(vec![
                            Span::styled("ESC", STYLE_KEY),
                            Span::styled(":Close ", STYLE_DIM),
                            Span::styled("Y", STYLE_KEY),
                            Span::styled(":Yank ", STYLE_DIM),
                            Span::styled("[Detail]", Style::new().fg(CAUTION_AMBER)),
                        ]),
                        ViewMode::FileDrillDown => Line::from(vec![
                            Span::styled("ESC", STYLE_KEY),
                            Span::styled(":Close ", STYLE_DIM),
                            Span::styled("↑↓", STYLE_KEY),
                            Span::styled(":Nav ", STYLE_DIM),
                            Span::styled("Enter", STYLE_KEY),
                            Span::styled(":Inspect ", STYLE_DIM),
                            Span::styled("Y", STYLE_KEY),
                            Span::styled(":Yank ", STYLE_DIM),
                            Span::styled("[File]", Style::new().fg(CAUTION_AMBER)),
                        ]),
                        ViewMode::Search => Line::from(vec![
                            Span::styled("ESC", STYLE_KEY),
                            Span::styled(":Cancel ", STYLE_DIM),
                            Span::styled("Enter", STYLE_KEY),
                            Span::styled(":Apply ", STYLE_DIM),
                            Span::styled("[Search]", Style::new().fg(CAUTION_AMBER)),
                        ]),
                        _ => {
                            let mode = if has_events {
                                Span::styled("[Live]", Style::new().fg(CRITICAL_RED))
                            } else {
                                Span::styled("[Waiting]", STYLE_DIM)
                            };
                            Line::from(vec![
                                Span::styled("Q", STYLE_KEY),
                                Span::styled(":Quit ", STYLE_DIM),
                                Span::styled("Enter", STYLE_KEY),
                                Span::styled(":Detail ", STYLE_DIM),
                                Span::styled("G", STYLE_KEY),
                                Span::styled(":Group ", STYLE_DIM),
                                Span::styled("/", STYLE_KEY),
                                Span::styled(":Search ", STYLE_DIM),
                                Span::styled("?", STYLE_KEY),
                                Span::styled(":Help ", STYLE_DIM),
                                mode,
                            ])
                        }
                    };

                    let status = Paragraph::new(vec![status_line]).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Plain)
                            .border_style(Style::default().fg(HUD_GREEN)),
                    );
                    f.render_widget(status, outer_layout[2]);
                }
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
