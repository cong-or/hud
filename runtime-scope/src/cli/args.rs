//! CLI argument definitions

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct Args {
    #[arg(short, long, help = "Process ID to attach to")]
    pub pid: Option<i32>,

    #[arg(
        short,
        long,
        help = "Path to target binary (defaults to test-async-app)"
    )]
    pub target: Option<String>,

    #[arg(long, help = "Enable Chrome trace export")]
    pub trace: bool,

    #[arg(long, default_value = "30", help = "Duration to profile in seconds (when using --trace)")]
    pub duration: u64,

    #[arg(long, default_value = "trace.json", help = "Output path for trace JSON")]
    pub trace_output: PathBuf,

    #[arg(long, help = "Trace-only mode (no live event output)")]
    pub no_live: bool,

    #[arg(long, value_name = "TRACE_FILE", help = "Launch TUI to visualize a trace.json file")]
    pub tui: Option<PathBuf>,
}
