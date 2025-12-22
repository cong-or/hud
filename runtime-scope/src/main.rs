use anyhow::{Context, Result};
use aya::{
    include_bytes_aligned,
    programs::TracePoint,
    Ebpf,
};
use aya_log::EbpfLogger;
use clap::Parser;
use log::{info, warn};

#[derive(Parser)]
struct Args {
    #[arg(short, long, help = "Process ID to attach to (optional for now)")]
    pid: Option<i32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    println!("🔍 runtime-scope v0.1.0");
    println!("   Real-time async runtime profiler\n");

    // Load the eBPF program
    #[cfg(debug_assertions)]
    let mut bpf = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/runtime-scope"
    ))?;
    #[cfg(not(debug_assertions))]
    let mut bpf = Ebpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/runtime-scope"
    ))?;

    // Initialize logging from eBPF
    if let Err(e) = EbpfLogger::init(&mut bpf) {
        warn!("Failed to initialize eBPF logger: {}", e);
    }

    // Attach to a tracepoint (just for testing infrastructure)
    let program: &mut TracePoint = bpf
        .program_mut("runtime_scope")
        .context("program not found")?
        .try_into()?;
    program.load()?;
    program.attach("syscalls", "sys_enter_write")?;

    info!("✓ eBPF program loaded and attached");
    info!("  Tracepoint: syscalls:sys_enter_write");

    if let Some(pid) = args.pid {
        println!("📊 Monitoring PID: {}", pid);
    } else {
        println!("📊 Monitoring system-wide events");
    }

    println!("\n👀 Watching for events... (press Ctrl+C to stop)\n");

    // Keep running
    tokio::signal::ctrl_c().await?;
    println!("\n\n✓ Shutting down gracefully");

    Ok(())
}
