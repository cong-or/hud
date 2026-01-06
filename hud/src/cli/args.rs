//! CLI argument definitions

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct Args {
    /// Process ID to attach to (for live profiling)
    #[arg(short, long)]
    pub pid: Option<i32>,

    /// Path to target binary for symbol resolution
    #[arg(short, long)]
    pub target: Option<String>,

    /// Replay mode: view a previously captured trace file
    #[arg(long, value_name = "TRACE_FILE", conflicts_with_all = &["pid", "target", "export"])]
    pub replay: Option<PathBuf>,

    /// Export trace data to file while profiling
    #[arg(long, value_name = "FILE", requires = "pid")]
    pub export: Option<PathBuf>,

    /// Duration to profile in seconds (0 = unlimited)
    #[arg(long, default_value = "0")]
    pub duration: u64,

    /// Headless mode: profile without TUI (requires --export)
    #[arg(long, requires = "export")]
    pub headless: bool,
}
