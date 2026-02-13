//! CLI argument definitions

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "hud",
    about = "Detect blocking operations in Tokio applications",
    after_help = "\
EXAMPLES:
    sudo hud my-app                          Auto-detect PID and binary
    sudo hud --pid 1234                      Explicit PID, auto-detect binary
    sudo hud --pid 1234 --target ./myapp     Explicit PID and binary

THRESHOLD GUIDE:
    1ms     Low-latency (games, fintech, real-time APIs). At 50k req/s, 1ms blocks 50 requests.
    5ms     Web services, REST APIs. Good default for most applications.
    10ms    Background workers, async jobs. Tolerant of occasional delays.
    50ms+   Finding only severe blocks. Useful for initial debugging.

    Lower = more sensitive (more events, potential noise)
    Higher = less sensitive (only obvious problems)"
)]
pub struct Args {
    /// Process name to profile (auto-detects PID and binary)
    #[arg(value_name = "PROCESS")]
    pub process: Option<String>,

    /// Process ID to profile (binary path auto-detected from /proc)
    #[arg(short, long)]
    pub pid: Option<i32>,

    /// Path to binary for symbol resolution (optional, auto-detected if omitted)
    #[arg(short, long)]
    pub target: Option<String>,

    /// Export trace to file (for external analysis)
    #[arg(long, value_name = "FILE")]
    pub export: Option<PathBuf>,

    /// Stop profiling after N seconds (omit for unlimited)
    #[arg(long, default_value = "0", value_name = "SECS")]
    pub duration: u64,

    /// Run without TUI (requires --export)
    #[arg(long, requires = "export")]
    pub headless: bool,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,

    /// Blocking threshold in milliseconds
    #[arg(long, default_value = "5", value_name = "MS")]
    pub threshold: u64,

    /// Rolling time window in seconds (omit for all data)
    #[arg(long, default_value = "0", value_name = "SECS")]
    pub window: u64,

    /// Thread name prefix for worker discovery (auto-detected if omitted)
    #[arg(long, value_name = "PATTERN")]
    pub workers: Option<String>,
}
