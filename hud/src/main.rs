// Main function is intentionally long for clarity; time conversions lose precision for display
#![allow(clippy::too_many_lines, clippy::cast_precision_loss, clippy::cast_lossless)]

use anyhow::{Context, Result};
use aya::maps::{RingBuf, StackTraceMap};
use clap::Parser;
use crossbeam_channel::bounded;
use hud::export::TraceEventExporter;
use hud::symbolization::{parse_memory_maps, Symbolizer};
use hud_common::{
    TaskEvent, EVENT_BLOCKING_END, EVENT_BLOCKING_START, EVENT_SCHEDULER_DETECTED,
    TRACE_EXECUTION_END, TRACE_EXECUTION_START,
};
use log::{info, warn};
use std::fs::File;
use std::io::BufWriter;
use std::time::{Duration, Instant};

// Import modules
use hud::cli::Args;
use hud::domain::StackId;
use hud::profiling::{
    attach_blocking_uprobes, display_blocking_end, display_blocking_end_no_start,
    display_blocking_start, display_execution_event, display_scheduler_detected,
    display_statistics, init_ebpf_logger, load_ebpf_program, print_perf_event_diagnostics,
    setup_scheduler_detection, DetectionStats, StackResolver,
};
use hud::trace_data::{TraceData, TraceEvent};
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
    let mut trace_exporter = args.export.as_ref().map(|_| {
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

    // Track blocking durations and stack IDs (for marker detection)
    let mut blocking_start_time: Option<u64> = None;
    let mut blocking_start_stack_id: Option<i64> = None;
    let mut event_count = 0;
    let mut last_status_time = Instant::now();

    // Statistics tracking
    let mut stats = DetectionStats::default();
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
                println!("  Processed {event_count} events");
                break;
            }
        }

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
            // SAFETY: We verified the buffer size matches TaskEvent, and the eBPF program writes valid TaskEvent data
            #[allow(unsafe_code)]
            let event = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<TaskEvent>()) };

            match event.event_type {
                EVENT_BLOCKING_START => {
                    blocking_start_time = Some(event.timestamp_ns);
                    blocking_start_stack_id = Some(event.stack_id);

                    if args.headless {
                        display_blocking_start(&event);
                    }
                }
                EVENT_BLOCKING_END => {
                    if let Some(start_time) = blocking_start_time {
                        stats.marker_detected += 1;

                        if args.headless {
                            display_blocking_end(
                                &event,
                                start_time,
                                blocking_start_stack_id,
                                &stack_resolver,
                                &stack_traces,
                            );
                        }

                        blocking_start_time = None;
                        blocking_start_stack_id = None;
                    } else if args.headless {
                        display_blocking_end_no_start(&event);
                    }
                }
                EVENT_SCHEDULER_DETECTED => {
                    stats.scheduler_detected += 1;

                    if args.headless {
                        display_scheduler_detected(&event, &stack_resolver, &stack_traces);
                    }
                }
                TRACE_EXECUTION_START | TRACE_EXECUTION_END => {
                    // Get the top frame address for symbol resolution
                    let top_frame_addr =
                        StackResolver::get_top_frame_addr(StackId(event.stack_id), &stack_traces);

                    // Add to trace exporter if enabled
                    if let Some(ref mut exporter) = trace_exporter {
                        exporter.add_event(&event, top_frame_addr);
                    }

                    // Send to TUI if running
                    if let Some(ref tx) = event_tx {
                        // Convert TaskEvent to TraceEvent for TUI
                        if event.event_type == TRACE_EXECUTION_START {
                            // Resolve symbol for event name using symbolizer directly
                            let (name, file, line) = if let Some(addr) = top_frame_addr {
                                // Adjust address for PIE executables
                                let file_offset = if let Some(range) = memory_range {
                                    if range.contains(addr) {
                                        addr - range.start
                                    } else {
                                        addr
                                    }
                                } else {
                                    addr
                                };

                                let resolved = symbolizer.resolve(file_offset);
                                if let Some(frame) = resolved.frames.first() {
                                    let func = frame.function.clone();
                                    let file_path =
                                        frame.location.as_ref().and_then(|loc| loc.file.clone());
                                    let line_num = frame.location.as_ref().and_then(|loc| loc.line);
                                    (func, file_path, line_num)
                                } else {
                                    (format!("0x{addr:x}"), None, None)
                                }
                            } else {
                                ("execution".to_string(), None, None)
                            };

                            let trace_event = TraceEvent {
                                name,
                                worker_id: event.worker_id,
                                tid: event.tid,
                                timestamp: event.timestamp_ns as f64 / 1_000_000.0, // ns to seconds
                                cpu: event.cpu_id,
                                detection_method: Some(u32::from(event.detection_method)),
                                file,
                                line,
                            };

                            // Non-blocking send (drop if TUI is slow)
                            let _ = tx.try_send(trace_event);
                        }
                    }

                    // Optionally display in headless mode
                    if args.headless {
                        let is_start = event.event_type == TRACE_EXECUTION_START;
                        display_execution_event(&event, is_start);
                    }
                }
                _ => {
                    warn!("Unknown event type: {}", event.event_type);
                }
            }
        }

        // Print statistics every 10 seconds in headless mode
        if args.headless && stats_timer.elapsed() > Duration::from_secs(10) {
            display_statistics(&stats);
            stats_timer = Instant::now();
        }

        // Use select to handle both sleep and Ctrl+C
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(100)) => {
                // Continue loop
            }
            _ = &mut ctrl_c => {
                println!("\n\n✓ Received Ctrl+C, shutting down gracefully");
                println!("  Processed {event_count} events");
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
    if let Some(exporter) = trace_exporter {
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
