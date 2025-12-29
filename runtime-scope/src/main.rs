mod symbolizer;

use anyhow::{Context, Result};
use aya::{include_bytes_aligned, maps::{RingBuf, StackTraceMap}, programs::UProbe, Ebpf};
use aya_log::EbpfLogger;
use clap::Parser;
use futures::FutureExt;
use log::{info, warn};
use runtime_scope_common::{TaskEvent, EVENT_BLOCKING_END, EVENT_BLOCKING_START};
use std::fs;
use std::time::Duration;
use symbolizer::Symbolizer;

/// Get the base address of a binary from /proc/pid/maps
fn get_base_address(pid: i32, binary_path: &str) -> Result<u64> {
    let maps_path = format!("/proc/{}/maps", pid);
    let maps = fs::read_to_string(&maps_path)
        .context(format!("Failed to read {}", maps_path))?;

    // Find the FIRST mapping (offset 0) of the target binary
    // This is the actual base address where the ELF is loaded
    for line in maps.lines() {
        if line.contains(binary_path) {
            // Parse the line: "start-end perms offset dev inode pathname"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let offset = parts[2]; // The file offset
                // We want the mapping with offset 0 (the base)
                if offset == "00000000" {
                    let range = parts[0];
                    let start = range.split('-').next().unwrap_or("0");
                    return u64::from_str_radix(start, 16)
                        .context("Failed to parse base address");
                }
            }
        }
    }

    Err(anyhow::anyhow!("Could not find base address for {}", binary_path))
}

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
        println!("   Attached to functions: trace_blocking_start, trace_blocking_end");
    } else {
        println!("📊 Monitoring all processes running: {}", target_path);
    }

    println!("\n👀 Watching for blocking events... (press Ctrl+C to stop)");
    println!("   💡 If no events appear, check that the target app is calling the marker functions\n");

    // Get base address for PIE address resolution
    let base_addr = if let Some(pid) = args.pid {
        match get_base_address(pid, &target_path) {
            Ok(addr) => {
                info!("Found base address: 0x{:x}", addr);
                Some(addr)
            }
            Err(e) => {
                warn!("Failed to get base address: {}. Symbol resolution may not work.", e);
                None
            }
        }
    } else {
        None
    };

    // Create symbolizer for resolving stack traces
    let symbolizer = Symbolizer::new(&target_path)
        .context("Failed to create symbolizer")?;

    // Get the ring buffer
    let mut ring_buf = RingBuf::try_from(bpf.take_map("EVENTS").context("map not found")?)?;

    // Get the stack trace map
    let stack_traces: StackTraceMap<_> = StackTraceMap::try_from(
        bpf.take_map("STACK_TRACES").context("stack trace map not found")?
    )?;

    // Track blocking durations and stack IDs
    let mut blocking_start_time: Option<u64> = None;
    let mut blocking_start_stack_id: Option<i64> = None;
    let mut event_count = 0;
    let mut last_status_time = std::time::Instant::now();

    // Setup Ctrl+C handler
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    // Read events from the ring buffer
    loop {
        // Print status every 10 seconds if no events
        if event_count == 0 && last_status_time.elapsed() > Duration::from_secs(10) {
            info!("Still waiting for events... (no events received yet)");
            last_status_time = std::time::Instant::now();
        }
        // Process all available events
        while let Some(item) = ring_buf.next() {
            event_count += 1;
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
                    blocking_start_stack_id = Some(event.stack_id);

                    println!(
                        "🔴 [PID {} TID {}] Blocking started",
                        event.pid,
                        event.tid
                    );
                }
                EVENT_BLOCKING_END => {
                    if let Some(start_time) = blocking_start_time {
                        let duration_ns = event.timestamp_ns - start_time;
                        let duration_ms = duration_ns as f64 / 1_000_000.0;

                        println!("\n🔴 BLOCKING DETECTED");
                        println!("   Duration: {:.2}ms {}",
                            duration_ms,
                            if duration_ms > 10.0 { "⚠️" } else { "" }
                        );
                        println!("   Process: PID {}", event.pid);
                        println!("   Thread: TID {}", event.tid);

                        // Print stack trace from blocking start
                        if let Some(stack_id) = blocking_start_stack_id {
                            if stack_id >= 0 {
                                match stack_traces.get(&(stack_id as u32), 0) {
                                    Ok(stack_trace) => {
                                        let frames = stack_trace.frames();
                                        if !frames.is_empty() {
                                            println!("\n   📍 Stack trace:");
                                            for (i, stack_frame) in frames.iter().enumerate() {
                                                let addr = stack_frame.ip;
                                                if addr == 0 {
                                                    break;
                                                }

                                                // Adjust address for PIE executables
                                                let file_offset = if let Some(base) = base_addr {
                                                    // Only adjust if address is in the main executable range
                                                    // (addresses starting with 0x55... or 0x56... are typically PIE)
                                                    if addr >= base {
                                                        let adjusted = addr - base;
                                                        if i == 0 {
                                                            info!("Address adjustment: 0x{:x} - 0x{:x} = 0x{:x}", addr, base, adjusted);
                                                        }
                                                        adjusted
                                                    } else {
                                                        addr
                                                    }
                                                } else {
                                                    addr
                                                };

                                                let resolved = symbolizer.resolve(file_offset);
                                                println!("      {}", resolved.format(i));
                                            }
                                        } else {
                                            println!("\n   ⚠️  Empty stack trace");
                                        }
                                    }
                                    Err(e) => {
                                        println!("\n   ⚠️  Failed to read stack trace: {}", e);
                                    }
                                }
                            } else {
                                println!("\n   ⚠️  No stack trace captured (stack_id = {})", stack_id);
                            }
                        }

                        println!();

                        blocking_start_time = None;
                        blocking_start_stack_id = None;
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

        // Use select to handle both sleep and Ctrl+C
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                // Continue loop
            }
            _ = &mut ctrl_c => {
                println!("\n\n✓ Received Ctrl+C, shutting down gracefully");
                println!("  Processed {} events", event_count);
                break;
            }
        }
    }

    Ok(())
}
