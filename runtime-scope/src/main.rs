use anyhow::{Context, Result};
use aya::{include_bytes_aligned, maps::RingBuf, programs::UProbe, Ebpf};
use aya_log::EbpfLogger;
use clap::Parser;
use futures::FutureExt;
use log::{info, warn};
use runtime_scope_common::{TaskEvent, EVENT_BLOCKING_END, EVENT_BLOCKING_START};
use std::time::Duration;

#[derive(Parser)]
struct Args {
    #[arg(short, long, help = "Process ID to attach to")]
    pid: Option<i32>,

    #[arg(
        short,
        long,
        help = "Path to target binary (defaults to test-async-app)"
    )]
    target: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    println!("🔍 runtime-scope v0.1.0");
    println!("   Real-time async runtime profiler\n");

    // Determine target binary and make it absolute
    let target_path = args.target.unwrap_or_else(|| {
        "target/debug/examples/test-async-app".to_string()
    });

    // Convert to absolute path
    let target_path = std::fs::canonicalize(&target_path)
        .context(format!("Failed to resolve path: {}", target_path))?
        .to_string_lossy()
        .to_string();

    println!("📦 Target: {}", target_path);

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

    // Attach uprobe to trace_blocking_start
    let program: &mut UProbe = bpf
        .program_mut("trace_blocking_start_hook")
        .context("program not found")?
        .try_into()?;
    program.load()?;
    program.attach(
        Some("trace_blocking_start"),
        0,
        &target_path,
        args.pid,
    )?;

    info!("✓ Attached uprobe: trace_blocking_start");

    // Attach uprobe to trace_blocking_end
    let program: &mut UProbe = bpf
        .program_mut("trace_blocking_end_hook")
        .context("program not found")?
        .try_into()?;
    program.load()?;
    program.attach(
        Some("trace_blocking_end"),
        0,
        &target_path,
        args.pid,
    )?;

    info!("✓ Attached uprobe: trace_blocking_end");

    if let Some(pid) = args.pid {
        println!("📊 Monitoring PID: {}", pid);
    } else {
        println!("📊 Monitoring all processes running: {}", target_path);
    }

    println!("\n👀 Watching for blocking events... (press Ctrl+C to stop)\n");

    // Get the ring buffer
    let mut ring_buf = RingBuf::try_from(bpf.take_map("EVENTS").context("map not found")?)?;

    // Track blocking durations
    let mut blocking_start_time: Option<u64> = None;

    // Read events from the ring buffer
    loop {
        // Process all available events
        while let Some(item) = ring_buf.next() {
            let bytes: &[u8] = &item;
            if bytes.len() < std::mem::size_of::<TaskEvent>() {
                warn!("Received incomplete event");
                continue;
            }

            // Parse the event
            let event = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const TaskEvent) };

            match event.event_type {
                EVENT_BLOCKING_START => {
                    blocking_start_time = Some(event.timestamp_ns);
                    println!(
                        "🔴 [PID {} TID {}] Blocking started at {}ms",
                        event.pid,
                        event.tid,
                        event.timestamp_ns / 1_000_000
                    );
                }
                EVENT_BLOCKING_END => {
                    if let Some(start_time) = blocking_start_time {
                        let duration_ns = event.timestamp_ns - start_time;
                        let duration_ms = duration_ns as f64 / 1_000_000.0;
                        println!(
                            "  ✓ [PID {} TID {}] Blocking ended - Duration: {:.2}ms {}",
                            event.pid,
                            event.tid,
                            duration_ms,
                            if duration_ms > 10.0 {
                                "⚠️  SLOW!"
                            } else {
                                ""
                            }
                        );
                        blocking_start_time = None;
                    } else {
                        println!(
                            "  ✓ [PID {} TID {}] Blocking ended (no start time)",
                            event.pid, event.tid
                        );
                    }
                }
                _ => {
                    warn!("Unknown event type: {}", event.event_type);
                }
            }
        }

        // Small sleep to avoid busy-waiting
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check if we should exit
        if tokio::signal::ctrl_c().now_or_never().is_some() {
            break;
        }
    }

    println!("\n\n✓ Shutting down gracefully");

    Ok(())
}
