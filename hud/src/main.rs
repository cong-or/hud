use anyhow::{Context, Result};
use aya::maps::{RingBuf, StackTraceMap};
use clap::Parser;
use log::{info, warn};
use hud_common::{
    TaskEvent,
    EVENT_BLOCKING_END, EVENT_BLOCKING_START, EVENT_SCHEDULER_DETECTED,
    TRACE_EXECUTION_START, TRACE_EXECUTION_END,
};
use std::fs::File;
use std::io::BufWriter;
use std::time::{Duration, Instant};
use hud::export::ChromeTraceExporter;
use hud::symbolization::{parse_memory_maps, Symbolizer};

// Import modules
use hud::cli::Args;
use hud::domain::StackId;
use hud::profiling::{
    attach_blocking_uprobes, init_ebpf_logger, load_ebpf_program,
    print_perf_event_diagnostics, setup_scheduler_detection, StackResolver,
    DetectionStats, display_blocking_start, display_blocking_end, display_blocking_end_no_start,
    display_scheduler_detected, display_execution_event, display_statistics, display_progress,
};
use hud::trace_data::TraceData;
use hud::tui::App;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // If --tui flag is provided, launch TUI instead of profiler
    if let Some(trace_path) = args.tui {
        println!("🎨 Launching TUI for trace: {}", trace_path.display());
        let data = TraceData::from_file(&trace_path)?;
        let app = App::new(data);
        return app.run();
    }

    println!("🔍 hud v0.1.0");
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
    let mut bpf = load_ebpf_program()?;

    // Initialize logging from eBPF
    init_ebpf_logger(&mut bpf);

    // Attach blocking marker uprobes
    let task_id_attached = attach_blocking_uprobes(&mut bpf, &target_path, args.pid)?;

    if !task_id_attached {
        println!("⚠️  Note: Task IDs will not be available (set_current_task_id inlined in release build)");
    }

    // Phase 3a: Setup scheduler-based detection
    if let Some(pid) = args.pid {
        let worker_count = setup_scheduler_detection(&mut bpf, pid)?;

        println!("📊 Monitoring PID: {}", pid);
        println!("   Marker detection: trace_blocking_start, trace_blocking_end");
        println!("   Scheduler detection: sched_switch tracepoint (5ms threshold)");
        println!("   Task tracking: set_current_task_id");
    } else {
        println!("📊 Monitoring all processes running: {}", target_path);
        println!("   Note: Scheduler-based detection requires --pid argument");
    }

    println!("\n👀 Watching for blocking events... (press Ctrl+C to stop)");
    println!("   💡 If no events appear, check that the target app is calling the marker functions\n");

    // Get memory range for PIE address resolution
    let memory_range = if let Some(pid) = args.pid {
        match parse_memory_maps(pid, &target_path) {
            Ok(range) => {
                info!("Found memory range: 0x{:x} - 0x{:x}", range.start, range.end);
                Some(range)
            }
            Err(e) => {
                warn!("Failed to get memory range: {}. Symbol resolution may not work.", e);
                None
            }
        }
    } else {
        None
    };

    // Create symbolizer for resolving stack traces
    let symbolizer = Symbolizer::new(&target_path)
        .context("Failed to create symbolizer")?;

    // Create stack resolver (deduplicates 150 lines of stack trace code!)
    let stack_resolver = StackResolver::new(&symbolizer, memory_range);

    // Initialize Chrome trace exporter if requested
    let mut trace_exporter = if args.trace {
        let mut exporter = ChromeTraceExporter::new(Symbolizer::new(&target_path)?);
        if let Some(range) = memory_range {
            exporter.set_memory_range(range);
        }
        println!("\n📊 Chrome trace export enabled");
        println!("   Duration: {}s", args.duration);
        println!("   Output: {}", args.trace_output.display());
        Some(exporter)
    } else {
        None
    };

    let live_display = !args.no_live;

    // Get the ring buffer
    let mut ring_buf = RingBuf::try_from(bpf.take_map("EVENTS").context("map not found")?)?;

    // Get the stack trace map
    let stack_traces: StackTraceMap<_> = StackTraceMap::try_from(
        bpf.take_map("STACK_TRACES").context("stack trace map not found")?
    )?;

    // Track blocking durations and stack IDs (for marker detection)
    let mut blocking_start_time: Option<u64> = None;
    let mut blocking_start_stack_id: Option<i64> = None;
    let mut event_count = 0;
    let mut last_status_time = Instant::now();

    // Phase 3a: Statistics tracking
    let mut stats = DetectionStats::default();
    let mut stats_timer = Instant::now();

    // Setup Ctrl+C handler
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    // Track start time for duration limit
    let profiling_start = Instant::now();
    let duration_limit = if args.trace {
        Some(Duration::from_secs(args.duration))
    } else {
        None
    };

    // Show progress indicator for trace mode
    if args.trace && !live_display {
        println!("\n⏱️  Collecting trace for {} seconds...", args.duration);
        println!("   (This will run silently until complete)");
        println!("   Start time: {:?}", profiling_start);
    }

    // Track progress updates
    let mut last_progress_time = Instant::now();

    // Read events from the ring buffer
    loop {
        // Check for duration timeout FIRST
        if let Some(limit) = duration_limit {
            let elapsed = profiling_start.elapsed();
            if elapsed >= limit {
                if args.trace && !live_display {
                    println!("\r   ✓ Collection complete! ({}s elapsed)                  ", elapsed.as_secs());
                } else {
                    println!("\n\n✓ Duration limit reached ({}s), shutting down", args.duration);
                }
                println!("  Processed {} events", event_count);
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
            let event = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const TaskEvent) };

            match event.event_type {
                EVENT_BLOCKING_START => {
                    blocking_start_time = Some(event.timestamp_ns);
                    blocking_start_stack_id = Some(event.stack_id);

                    display_blocking_start(&event);
                }
                EVENT_BLOCKING_END => {
                    if let Some(start_time) = blocking_start_time {
                        stats.marker_detected += 1;  // Phase 3a: Track marker stats

                        display_blocking_end(
                            &event,
                            start_time,
                            blocking_start_stack_id,
                            &stack_resolver,
                            &stack_traces,
                        );

                        blocking_start_time = None;
                        blocking_start_stack_id = None;
                    } else {
                        display_blocking_end_no_start(&event);
                    }
                }
                EVENT_SCHEDULER_DETECTED => {
                    // Phase 3a: Scheduler-based detection
                    stats.scheduler_detected += 1;

                    display_scheduler_detected(&event, &stack_resolver, &stack_traces);
                }
                TRACE_EXECUTION_START | TRACE_EXECUTION_END => {
                    // Get the top frame address for symbol resolution (using deduplicated method!)
                    let top_frame_addr = StackResolver::get_top_frame_addr(
                        StackId(event.stack_id),
                        &stack_traces,
                    );

                    // Add to trace exporter if enabled
                    if let Some(ref mut exporter) = trace_exporter {
                        exporter.add_event(&event, top_frame_addr);
                    }

                    // Optionally display live (unless --no-live)
                    if live_display {
                        let is_start = event.event_type == TRACE_EXECUTION_START;
                        display_execution_event(&event, is_start);
                    }
                }
                _ => {
                    warn!("Unknown event type: {}", event.event_type);
                }
            }
        }

        // Phase 3a: Print statistics every 10 seconds
        if stats_timer.elapsed() > Duration::from_secs(10) {
            display_statistics(&stats);
            stats_timer = Instant::now();
        }

        // Show progress when in quiet trace mode
        if args.trace && !live_display && last_progress_time.elapsed() > Duration::from_secs(2) {
            if let Some(limit) = duration_limit {
                let elapsed = profiling_start.elapsed();
                let remaining = limit.saturating_sub(elapsed);
                let elapsed_secs = elapsed.as_secs();
                let remaining_secs = remaining.as_secs();
                display_progress(elapsed_secs, args.duration, remaining_secs);
                last_progress_time = Instant::now();
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

    // DEBUG: Check perf_event counters to track event flow
    if args.pid.is_some() {
        print_perf_event_diagnostics(&mut bpf)?;
    }

    // Export trace if enabled
    if let Some(exporter) = trace_exporter {
        println!("\n📝 Exporting Chrome trace...");
        println!("   Events collected: {}", exporter.event_count());

        let file = File::create(&args.trace_output)
            .context("Failed to create trace output file")?;
        let writer = BufWriter::new(file);
        exporter.export(writer)
            .context("Failed to export trace")?;

        println!("   ✓ Trace exported to: {}", args.trace_output.display());
        println!("\n💡 To visualize:");
        println!("   1. Open chrome://tracing in Chrome/Chromium");
        println!("   2. Click 'Load' and select {}", args.trace_output.display());
        println!("   3. Use W/A/S/D to zoom/pan the timeline");
    }

    Ok(())
}
