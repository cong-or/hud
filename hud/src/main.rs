//! # hud - Main Entry Point
//!
//! Supports two operational modes:
//! - **Live TUI** (`--pid <PID>` or `hud <PROCESS>`): Real-time profiling with interactive UI
//! - **Headless** (`--headless --export trace.json`): Non-interactive profiling for CI/CD
//!
//! See [Architecture docs](../docs/ARCHITECTURE.md) for detailed program flow.

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
use hud::preflight::{check_proc_access, check_process_exists, run_preflight_checks};
use hud::process_lookup::{find_process_by_name, resolve_exe_path};
use hud::profiling::{
    attach_task_id_uprobe, display_statistics, init_ebpf_logger, load_ebpf_program,
    print_perf_event_diagnostics, setup_scheduler_detection, EventProcessor, StackResolver,
};
use hud::tui;

// Exit codes
const EXIT_SUCCESS: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NOPERM: i32 = 77;

fn main() {
    env_logger::init();
    std::process::exit(match run() {
        Ok(()) => EXIT_SUCCESS,
        Err(e) => {
            let code = exit_code_for(&e);
            eprintln!("error: {e}");
            code
        }
    });
}

fn exit_code_for(err: &anyhow::Error) -> i32 {
    let msg = err.to_string().to_lowercase();
    if msg.contains("permission denied") || msg.contains("requires root") {
        EXIT_NOPERM
    } else if msg.contains("missing required argument") {
        EXIT_USAGE
    } else {
        EXIT_ERROR
    }
}

/// Resolve PID and binary path from CLI arguments.
///
/// Supports three modes:
/// - `hud my-app` - find process by name, auto-detect binary
/// - `hud --pid 1234` - explicit PID, auto-detect binary from /proc
/// - `hud --pid 1234 --target ./app` - explicit PID and binary
fn resolve_pid_and_target(args: &Args) -> Result<(i32, String)> {
    // Mode A: Process name provided - auto-detect both
    if let Some(ref name) = args.process {
        if args.pid.is_some() || args.target.is_some() {
            anyhow::bail!(
                "Cannot use PROCESS argument with --pid or --target.\n\n\
                 Use either:\n  \
                 hud my-app              (auto-detect)\n  \
                 hud --pid 1234          (explicit PID)"
            );
        }
        let info = find_process_by_name(name)?;
        let target = info.exe_path.to_string_lossy().to_string();
        return Ok((info.pid, target));
    }

    // Mode B: Explicit PID provided
    if let Some(pid) = args.pid {
        let target = if let Some(ref t) = args.target {
            // Explicit target - resolve to absolute path
            std::fs::canonicalize(t)
                .with_context(|| format!("Failed to resolve path: {t}"))?
                .to_string_lossy()
                .to_string()
        } else {
            // Auto-detect from /proc/<pid>/exe
            resolve_exe_path(pid)?.to_string_lossy().to_string()
        };
        return Ok((pid, target));
    }

    // No PID or process name - show usage
    anyhow::bail!(
        "Missing required argument: PROCESS or --pid\n\n\
         Usage:\n  \
         hud my-app              Auto-detect PID and binary\n  \
         hud --pid 1234          Explicit PID, auto-detect binary\n\n\
         Run 'hud --help' for more options"
    )
}

#[tokio::main]
async fn run() -> Result<()> {
    let args = Args::parse();

    let quiet = args.quiet;

    // Live profiling (with or without TUI)
    // Resolve PID and target path from arguments
    let (pid, target_path) = resolve_pid_and_target(&args)?;

    // Run pre-flight checks before anything else
    run_preflight_checks(&target_path, quiet)?;
    check_process_exists(pid)?;
    check_proc_access(pid)?;

    if !quiet {
        println!("hud v0.1.0");
        println!("target: {target_path}");
        println!("pid: {pid}");
    }

    // Load the eBPF program
    let mut bpf = load_ebpf_program()?;

    // Initialize logging from eBPF
    init_ebpf_logger(&mut bpf);

    // Attach task ID tracking uprobe (optional - may be inlined in release builds)
    let task_id_attached = attach_task_id_uprobe(&mut bpf, &target_path, Some(pid))?;

    if !task_id_attached {
        eprintln!("warning: task IDs unavailable (symbol inlined in release build)");
    }

    // Setup scheduler-based detection
    let worker_count = setup_scheduler_detection(&mut bpf, pid, args.threshold)?;
    if !quiet {
        println!("workers: {worker_count}");
    }

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
    let trace_exporter = args
        .export
        .as_ref()
        .map(|_| -> Result<_> {
            let export_symbolizer = Symbolizer::new(&target_path)
                .context("Failed to create symbolizer for trace export")?;
            let mut exporter = TraceEventExporter::new(export_symbolizer);
            if let Some(range) = memory_range {
                exporter.set_memory_range(range);
            }
            Ok(exporter)
        })
        .transpose()?;

    if !quiet {
        if let Some(ref export_path) = args.export {
            println!("export: {}", export_path.display());
        }
    }

    // Launch TUI in separate thread if not headless
    let (tui_handle, event_tx) = if args.headless {
        (None, None)
    } else {
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

    // Pre-compute proc path for process liveness check
    let proc_path = format!("/proc/{pid}");

    // Track why we exited the loop
    let mut exit_reason = "interrupted";

    // Main event processing loop
    loop {
        // Check for duration timeout
        if let Some(limit) = duration_limit {
            if profiling_start.elapsed() >= limit {
                exit_reason = "duration limit reached";
                break;
            }
        }

        // Check if target process still exists
        if !std::path::Path::new(&proc_path).exists() {
            exit_reason = "process exited";
            break;
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
                break;
            }
        }
    }

    // Print summary (before TUI cleanup so it shows in headless mode)
    if !quiet || args.headless {
        let elapsed = profiling_start.elapsed();
        eprintln!(
            "\n{}: {:.1}s, {} events",
            exit_reason,
            elapsed.as_secs_f64(),
            processor.event_count
        );
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

        let file = File::create(&export_path).context("Failed to create trace output file")?;
        let writer = BufWriter::new(file);
        exporter.export(writer).context("Failed to export trace")?;

        if !quiet {
            println!("saved: {}", export_path.display());
        }
    }

    Ok(())
}
