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
use hud::domain::Pid;
use hud::preflight::{check_proc_access, check_process_exists, run_preflight_checks};
use hud::process_lookup::{find_process_by_name, resolve_exe_path};
use hud::profiling::{
    attach_sched_switch, attach_task_id_uprobe, discover_workers_from_stacks, display_statistics,
    init_ebpf_logger, load_ebpf_program, print_perf_event_diagnostics, register_workers_in_ebpf,
    start_perf_sampling, EventProcessor, StackResolver,
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
        let target = info.exe_path.to_string_lossy().into_owned();
        return Ok((info.pid, target));
    }

    // Mode B: Explicit PID provided
    if let Some(pid) = args.pid {
        let target = if let Some(ref t) = args.target {
            // Explicit target - resolve to absolute path
            std::fs::canonicalize(t)
                .with_context(|| format!("Failed to resolve path: {t}"))?
                .to_string_lossy()
                .into_owned()
        } else {
            // Auto-detect from /proc/<pid>/exe
            resolve_exe_path(pid)?.to_string_lossy().into_owned()
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

/// Discover Tokio worker threads using a 4-step fallback chain and register
/// them in the eBPF map.
///
/// 1. If `--workers <prefix>` was given, use that prefix exclusively.
/// 2. Try the default prefixes (`tokio-runtime-w`, `tokio-rt-worker`).
/// 3. Stack-based discovery: sample stack traces for 500ms and classify threads.
/// 4. Largest thread group heuristic (original fallback).
fn discover_and_register_workers(
    bpf: &mut aya::Ebpf,
    pid: i32,
    worker_prefix: Option<&str>,
    ring_buf: &mut RingBuf<aya::maps::MapData>,
    stack_traces: &StackTraceMap<aya::maps::MapData>,
    symbolizer: &Symbolizer,
    memory_range: Option<hud::symbolization::MemoryRange>,
) -> Result<usize> {
    use hud::profiling::worker_discovery;

    // Step (a): If --workers given, use that prefix only (no fallback)
    if let Some(prefix) = worker_prefix {
        let threads = worker_discovery::list_process_threads(Pid(pid))?;
        let workers = worker_discovery::collect_workers(&threads, prefix);
        if workers.is_empty() {
            warn!("No workers found matching prefix \"{prefix}\"");
        }
        return register_workers_in_ebpf(bpf, pid, &workers);
    }

    // Step (b): Try default prefixes (covers old and new Tokio naming)
    let threads = worker_discovery::list_process_threads(Pid(pid))?;
    for prefix in worker_discovery::DEFAULT_PREFIXES {
        let workers = worker_discovery::collect_workers(&threads, prefix);
        if !workers.is_empty() {
            return register_workers_in_ebpf(bpf, pid, &workers);
        }
    }

    // Step (c): Stack-based discovery (500ms sampling window)
    info!("Default prefix found no workers, trying stack-based discovery...");
    let stack_workers = discover_workers_from_stacks(
        ring_buf,
        stack_traces,
        symbolizer,
        memory_range,
        Pid(pid),
        Duration::from_millis(500),
    )?;
    if !stack_workers.is_empty() {
        info!("Stack-based discovery found {} worker threads", stack_workers.len());
        return register_workers_in_ebpf(bpf, pid, &stack_workers);
    }

    // Step (d): Fall back to largest thread group heuristic.
    // Reuse thread list from step (b) — worker pools are stable over 500ms.
    info!("Stack-based discovery found no workers, falling back to largest thread group...");
    if let Some(disc_prefix) = worker_discovery::discover_worker_prefix(&threads) {
        let workers = worker_discovery::collect_workers(&threads, &disc_prefix);
        if !workers.is_empty() {
            info!("Auto-discovered {} workers with prefix \"{}\"", workers.len(), disc_prefix);
            return register_workers_in_ebpf(bpf, pid, &workers);
        }
    }

    warn!("No Tokio worker threads found! Make sure the target is a Tokio app.");
    Ok(0)
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
        println!("hud v{}", env!("CARGO_PKG_VERSION"));
        println!("target: {target_path}");
        println!("pid: {pid}");
    }

    // ── Phase 1: Load eBPF and start perf sampling ──────────────────────
    let mut bpf = load_ebpf_program()?;
    init_ebpf_logger(&mut bpf);

    let task_id_attached = attach_task_id_uprobe(&mut bpf, &target_path, Some(pid))?;
    if !task_id_attached {
        eprintln!("warning: task IDs unavailable (symbol inlined in release build)");
    }

    // Start perf sampling early so stack-based discovery can collect samples
    start_perf_sampling(&mut bpf, pid, args.threshold)?;

    // ── Take maps early (needed for sampling window + main loop) ────────
    let mut ring_buf = RingBuf::try_from(bpf.take_map("EVENTS").context("map not found")?)?;
    let stack_traces: StackTraceMap<_> = StackTraceMap::try_from(
        bpf.take_map("STACK_TRACES").context("stack trace map not found")?,
    )?;

    // ── Symbolization setup (needed for stack-based discovery) ──────────
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
    let symbolizer = Symbolizer::new(&target_path).context("Failed to create symbolizer")?;

    // ── Worker discovery: 4-step fallback chain ─────────────────────────
    let worker_count = discover_and_register_workers(
        &mut bpf,
        pid,
        args.workers.as_deref(),
        &mut ring_buf,
        &stack_traces,
        &symbolizer,
        memory_range,
    )?;

    if !quiet {
        println!("workers: {worker_count}");
    }

    // ── Phase 2: Attach sched_switch (after workers are registered) ─────
    attach_sched_switch(&mut bpf)?;

    if !quiet {
        println!("CPU sampling: 99 Hz (every ~10ms)");
    }

    // Drain ring buffer to discard sampling window events
    while ring_buf.next().is_some() {}

    // ── Rest of setup (unchanged) ───────────────────────────────────────
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
        let window_secs = args.window;
        let handle = std::thread::spawn(move || tui::run_live(event_rx, tui_pid, window_secs));

        (Some(handle), Some(event_tx))
    };

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
            "\n{}: {:.1}s, {} events (perf: {}, stack_ok: {}, stack_fail: {}, sched: {}, pool_filtered: {}, tui: {} sent / {} no-user-code)",
            exit_reason,
            elapsed.as_secs_f64(),
            processor.event_count,
            processor.perf_sample_count,
            processor.perf_stack_ok,
            processor.perf_stack_fail,
            processor.scheduler_event_count,
            processor.blocking_pool_filtered,
            processor.tui_sent,
            processor.tui_no_user_code,
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
