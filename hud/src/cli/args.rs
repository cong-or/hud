//! CLI argument definitions

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "hud",
    about = "Detect blocking operations in Tokio applications",
    after_help = "\
EXAMPLES:
    sudo hud --pid 1234 --target ./myapp     Live profiling with TUI
    sudo hud -p 1234 -t ./myapp --headless --export trace.json
    hud --replay trace.json                  View saved trace"
)]
pub struct Args {
    /// Process ID to profile
    #[arg(short, long)]
    pub pid: Option<i32>,

    /// Path to binary (for symbol resolution)
    #[arg(short, long)]
    pub target: Option<String>,

    /// Replay a saved trace file
    #[arg(long, value_name = "FILE", conflicts_with_all = &["pid", "target", "export"])]
    pub replay: Option<PathBuf>,

    /// Export trace to file
    #[arg(long, value_name = "FILE", requires = "pid")]
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
