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
    sudo hud --pid 1234 --target ./myapp     Explicit PID and binary"
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

    /// Stop after N seconds (0 = unlimited)
    #[arg(long, default_value = "0")]
    pub duration: u64,

    /// Run without TUI (requires --export)
    #[arg(long, requires = "export")]
    pub headless: bool,

    /// Suppress non-essential output
    #[arg(short, long)]
    pub quiet: bool,
}
