//! # Runtime Scope - Main Entry Point
//!
//! This binary serves as the main entry point for the Runtime Scope profiler,
//! supporting three distinct operational modes based on command-line arguments.
//!
//! ## Operational Modes
//!
//! ### Mode 1: Replay Mode (`--replay <trace.json>`)
//! - **Purpose**: Analyze previously recorded trace files offline
//! - **Requirements**: Only a trace file (no root privileges needed)
//! - **Behavior**:
//!   - Loads trace data from JSON file (Chrome Trace Event Format)
//!   - Launches TUI with hotspot analysis and event browsing
//!   - No eBPF or live profiling involved
//! - **Use Cases**: Post-mortem analysis, sharing traces with team members
//!
//! Example:
//! ```bash
//! ./hud --replay trace.json
//! ```
//!
//! ### Mode 2: Live TUI Mode (default)
//! - **Purpose**: Real-time profiling with interactive terminal interface
//! - **Requirements**: `--pid <PID> --target <BINARY>`, root/CAP_BPF privileges
//! - **Behavior**:
//!   - Attaches eBPF programs to target process
//!   - Streams events to TUI in separate thread
//!   - User can navigate between Live/Hotspots/Raw views
//!   - Optional `--export` saves trace to file on exit
//! - **Use Cases**: Interactive debugging, real-time monitoring
//!
//! Example:
//! ```bash
//! sudo ./hud --pid 12345 --target ./my_tokio_app
//! sudo ./hud --pid 12345 --target ./my_tokio_app --export trace.json
//! ```
//!
//! ### Mode 3: Headless Mode (`--headless`)
//! - **Purpose**: Non-interactive profiling for CI/CD or logging
//! - **Requirements**: Same as Live TUI + `--headless` flag
//! - **Behavior**:
//!   - Attaches eBPF programs to target process
//!   - Prints events and statistics to stdout
//!   - No TUI or interactive features
//!   - Requires `--export` or `--duration` for bounded execution
//! - **Use Cases**: Automated performance testing, log aggregation
//!
//! Example:
//! ```bash
//! sudo ./hud --pid 12345 --target ./my_tokio_app --headless --duration 60
//! ```
//!
//! ## Program Flow
//!
//! ```text
//! ┌─────────────────────┐
//! │  Parse CLI Args     │
//! └──────────┬──────────┘
//!            │
//!            ├─── --replay? ──────────────────┐
//!            │                                 ▼
//!            │                      ┌──────────────────────┐
//!            │                      │ Mode 1: Replay       │
//!            │                      │ • Load trace.json    │
//!            │                      │ • Launch TUI         │
//!            │                      └──────────────────────┘
//!            │
//!            └─── Live Profiling ────────────┐
//!                                             ▼
//!                              ┌──────────────────────────┐
//!                              │ Load & Attach eBPF       │
//!                              │ • Uprobes                │
//!                              │ • Tracepoints            │
//!                              │ • Perf Events (99Hz)     │
//!                              └─────────┬────────────────┘
//!                                        │
//!                              ┌─────────▼────────────────┐
//!                              │ Initialize Components    │
//!                              │ • Symbolizer (DWARF)     │
//!                              │ • Memory range (PIE)     │
//!                              │ • Event processor        │
//!                              │ • Optional: Exporter     │
//!                              └─────────┬────────────────┘
//!                                        │
//!                       ┌────────────────┼────────────────┐
//!                       │                │                │
//!              --headless?               │                │
//!                       │                │                │
//!            ┌──────────▼──────────┐     │     ┌─────────▼──────────┐
//!            │ Mode 3: Headless    │     │     │ Mode 2: Live TUI   │
//!            │ • No TUI thread     │     │     │ • Spawn TUI thread │
//!            │ • Log to stdout     │     │     │ • Create channel   │
//!            └──────────┬──────────┘     │     └─────────┬──────────┘
//!                       │                │               │
//!                       └────────────────┼───────────────┘
//!                                        │
//!                              ┌─────────▼────────────────┐
//!                              │ Main Event Loop          │
//!                              │ • Read ring buffer       │
//!                              │ • Process events         │
//!                              │ • Send to TUI (if live)  │
//!                              │ • Export (if enabled)    │
//!                              │ • Ctrl+C / --duration    │
//!                              └─────────┬────────────────┘
//!                                        │
//!                              ┌─────────▼────────────────┐
//!                              │ Cleanup & Export         │
//!                              │ • Wait for TUI thread    │
//!                              │ • Write trace.json       │
//!                              │ • Print statistics       │
//!                              └──────────────────────────┘
//! ```
//!
//! ## Key Components Initialization
//!
//! 1. **eBPF Setup** (`profiling::ebpf_setup`):
//!    - Load compiled eBPF bytecode from `target/bpfel-unknown-none/release/hud`
//!    - Attach uprobes for marker-based detection
//!    - Attach tracepoint for scheduler-based detection
//!    - Attach perf_events for stack sampling
//!
//! 2. **Symbolization** (`symbolization::Symbolizer`):
//!    - Load DWARF debug info from target binary
//!    - Parse `/proc/<pid>/maps` for PIE address adjustment
//!    - Enable resolution of raw addresses to function/file/line
//!
//! 3. **Event Processing** (`profiling::EventProcessor`):
//!    - Maintains blocking state machine
//!    - Routes events to TUI channel (live mode)
//!    - Routes events to exporter (if enabled)
//!    - Tracks detection statistics
//!
//! 4. **TUI Thread** (`tui::run_live`):
//!    - Runs in separate thread with crossbeam channel
//!    - Receives events asynchronously from main loop
//!    - Provides interactive views (Live/Hotspots/Raw)
//!    - Exits when channel closes
//!
//! ## Termination Conditions
//!
//! The main event loop exits when:
//! - **Ctrl+C**: User interrupts (SIGINT)
//! - **Duration limit**: `--duration <seconds>` expires
//! - **Error**: Unrecoverable error in eBPF or event processing

// Main function is intentionally long for clarity; time conversions lose precision for display
#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::items_after_statements
)]

use anyhow::{Context, Result};
use aya::maps::{RingBuf, StackTraceMap};
use clap::Parser;
use crossbeam_channel::bounded;
use hud::export::TraceEventExporter;
use hud::symbolization::{parse_memory_maps, Symbolizer};
use hud_common::TaskEvent;
use log::{info, warn};
use std::fs::File;
use std::io::BufWriter;
use std::time::{Duration, Instant};

// Import modules
use hud::cli::Args;
use hud::profiling::{
    attach_blocking_uprobes, display_statistics, init_ebpf_logger, load_ebpf_program,
    print_perf_event_diagnostics, setup_scheduler_detection, EventProcessor, StackResolver,
};
use hud::trace_data::TraceData;
use hud::tui::{self, App};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // Mode 1: Replay mode - load trace file and display in TUI
    if let Some(trace_path) = args.replay {
        println!("🎨 Launching replay mode: {}", trace_path.display());
        let data = TraceData::from_file(&trace_path)?;
        let app = App::new(data);
        return app.run();
    }

    // Mode 2 & 3: Live profiling (with or without TUI)
    // Requires --pid argument
    let pid = args.pid.ok_or_else(|| anyhow::anyhow!(
        "Missing required argument: --pid\n\nUsage:\n  hud --pid <PID> --target <BINARY>          # Live TUI profiling\n  hud --pid <PID> --target <BINARY> --export <FILE>  # Also save to file\n  hud --replay <FILE>                         # Replay saved trace"
    ))?;

    println!("🔍 hud v0.1.0");
    println!("   F-35 inspired profiler for async Rust\n");

    // Determine target binary and make it absolute
    let target_path = args.target.ok_or_else(|| {
        anyhow::anyhow!(
            "Missing required argument: --target\n\nSpecify the binary path for symbol resolution"
        )
    })?;

    // Convert to absolute path
    let target_path = std::fs::canonicalize(&target_path)
        .context(format!("Failed to resolve path: {target_path}"))?
        .to_string_lossy()
        .to_string();

    println!("📦 Target: {target_path}");
    println!("📊 PID: {pid}");

    // Load the eBPF program
    let mut bpf = load_ebpf_program()?;

    // Initialize logging from eBPF
    init_ebpf_logger(&mut bpf);

    // Attach blocking marker uprobes
    let task_id_attached = attach_blocking_uprobes(&mut bpf, &target_path, Some(pid))?;

    if !task_id_attached {
        println!("⚠️  Note: Task IDs unavailable (set_current_task_id inlined in release build)");
    }

    // Setup scheduler-based detection
    let worker_count = setup_scheduler_detection(&mut bpf, pid)?;
    println!("   Workers: {worker_count}");
    println!("   Detection: sched_switch (5ms threshold) + perf_event (99Hz)");

    // Get memory range for PIE address resolution
    let memory_range = match parse_memory_maps(pid, &target_path) {
        Ok(range) => {
            info!("Found memory range: 0x{:x} - 0x{:x}", range.start, range.end);
            Some(range)
        }
        Err(e) => {
            warn!("Failed to get memory range: {e}. Symbol resolution may not work.");
            None
        }
    };

    // Create symbolizer for resolving stack traces
    let symbolizer = Symbolizer::new(&target_path).context("Failed to create symbolizer")?;

    // Create stack resolver
    let stack_resolver = StackResolver::new(&symbolizer, memory_range);

    // Initialize trace event exporter if export requested
    let trace_exporter = args.export.as_ref().map(|_| {
        let mut exporter = TraceEventExporter::new(Symbolizer::new(&target_path).unwrap());
        if let Some(range) = memory_range {
            exporter.set_memory_range(range);
        }
        exporter
    });

    if let Some(ref export_path) = args.export {
        println!("💾 Export: {}", export_path.display());
    }

    // Launch TUI in separate thread if not headless
    let (tui_handle, event_tx) = if args.headless {
        println!("\n📊 Headless mode - collecting data...\n");
        (None, None)
    } else {
        println!("\n🎯 Launching live HUD...\n");

        let (event_tx, event_rx) = bounded(1000);

        // Spawn TUI thread
        let tui_pid = Some(pid);
        let handle = std::thread::spawn(move || tui::run_live(event_rx, tui_pid));

        (Some(handle), Some(event_tx))
    };

    // Get the ring buffer
    let mut ring_buf = RingBuf::try_from(bpf.take_map("EVENTS").context("map not found")?)?;

    // Get the stack trace map
    let stack_traces: StackTraceMap<_> = StackTraceMap::try_from(
        bpf.take_map("STACK_TRACES").context("stack trace map not found")?,
    )?;

    // Create event processor with all dependencies
    let mut processor = EventProcessor::new(
        args.headless,
        stack_resolver,
        &symbolizer,
        memory_range,
        trace_exporter,
        event_tx,
    );

    // Status tracking
    let mut last_status_time = Instant::now();
    let mut stats_timer = Instant::now();

    // Setup Ctrl+C handler
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    // Track start time for duration limit
    let profiling_start = Instant::now();
    let duration_limit =
        if args.duration > 0 { Some(Duration::from_secs(args.duration)) } else { None };

    // Main event processing loop
    loop {
        // Check for duration timeout
        if let Some(limit) = duration_limit {
            if profiling_start.elapsed() >= limit {
                println!("\n\n✓ Duration limit reached ({}s), shutting down", args.duration);
                println!("  Processed {} events", processor.event_count);
                break;
            }
        }

        // Print status every 10 seconds if no events
        if processor.event_count == 0 && last_status_time.elapsed() > Duration::from_secs(10) {
            info!("Still waiting for events... (no events received yet)");
            last_status_time = std::time::Instant::now();
        }

        // Process all available events
        while let Some(item) = ring_buf.next() {
            let bytes: &[u8] = &item;
            if bytes.len() < std::mem::size_of::<TaskEvent>() {
                warn!("Received incomplete event");
                continue;
            }

            // Parse the event
            // SAFETY: We verified the buffer size matches TaskEvent, and the eBPF program writes valid TaskEvent data
            #[allow(unsafe_code)]
            let event = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<TaskEvent>()) };

            // Delegate to event processor
            processor.process_event(event, &stack_traces);
        }

        // Print statistics every 10 seconds in headless mode
        if args.headless && stats_timer.elapsed() > Duration::from_secs(10) {
            display_statistics(&processor.stats);
            stats_timer = Instant::now();
        }

        // Use select to handle both sleep and Ctrl+C
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(100)) => {
                // Continue loop
            }
            _ = &mut ctrl_c => {
                println!("\n\n✓ Received Ctrl+C, shutting down gracefully");
                println!("  Processed {} events", processor.event_count);
                break;
            }
        }
    }

    // Wait for TUI to finish if it was running
    if let Some(handle) = tui_handle {
        // TUI will exit when event channel is closed (happens when this scope ends)
        handle.join().ok();
    }

    // DEBUG: Check perf_event counters
    print_perf_event_diagnostics(&mut bpf)?;

    // Export trace if enabled
    if let Some(exporter) = processor.take_exporter() {
        let export_path = args.export.unwrap(); // Safe because we checked earlier
        println!("\n📝 Exporting trace...");
        println!("   Events: {}", exporter.event_count());

        let file = File::create(&export_path).context("Failed to create trace output file")?;
        let writer = BufWriter::new(file);
        exporter.export(writer).context("Failed to export trace")?;

        println!("   ✓ Saved to: {}", export_path.display());
        println!("\n💡 To replay:");
        println!("   hud --replay {}", export_path.display());
    }

    Ok(())
}
