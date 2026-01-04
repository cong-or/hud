mod symbolizer;
mod trace_exporter;
mod tui;

use anyhow::{Context, Result};
use aya::{
    include_bytes_aligned,
    maps::{HashMap, RingBuf, StackTraceMap},
    programs::{perf_event, PerfEvent, TracePoint, UProbe},
    Ebpf,
};
use aya_log::EbpfLogger;
use clap::Parser;
use log::{info, warn};
use runtime_scope_common::{
    TaskEvent, WorkerInfo,
    EVENT_BLOCKING_END, EVENT_BLOCKING_START, EVENT_SCHEDULER_DETECTED,
    TRACE_EXECUTION_START, TRACE_EXECUTION_END,
};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use symbolizer::Symbolizer;
use trace_exporter::{ChromeTraceExporter, MemoryRange as ExporterMemoryRange};

/// Memory range of a loaded binary
#[derive(Debug, Clone, Copy)]
struct MemoryRange {
    start: u64,
    end: u64,
}

impl MemoryRange {
    fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }
}

/// Get the memory range of a binary from /proc/pid/maps
fn get_memory_range(pid: i32, binary_path: &str) -> Result<MemoryRange> {
    let maps_path = format!("/proc/{}/maps", pid);
    let maps = fs::read_to_string(&maps_path)
        .context(format!("Failed to read {}", maps_path))?;

    let mut start_addr = None;
    let mut end_addr = None;

    // Find ALL mappings of the target binary to get the full range
    for line in maps.lines() {
        if line.contains(binary_path) {
            // Parse the line: "start-end perms offset dev inode pathname"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let range = parts[0];
                let range_parts: Vec<&str> = range.split('-').collect();
                if range_parts.len() == 2 {
                    let start = u64::from_str_radix(range_parts[0], 16)
                        .context("Failed to parse range start")?;
                    let end = u64::from_str_radix(range_parts[1], 16)
                        .context("Failed to parse range end")?;

                    // Track the minimum start and maximum end
                    start_addr = Some(start_addr.map_or(start, |s: u64| s.min(start)));
                    end_addr = Some(end_addr.map_or(end, |e: u64| e.max(end)));
                }
            }
        }
    }

    match (start_addr, end_addr) {
        (Some(start), Some(end)) => {
            info!("Executable memory range: 0x{:x} - 0x{:x} (size: {} KB)",
                start, end, (end - start) / 1024);
            Ok(MemoryRange { start, end })
        }
        _ => Err(anyhow::anyhow!("Could not find memory range for {}", binary_path))
    }
}

/// Get list of online CPU IDs
fn online_cpus() -> Result<Vec<u32>> {
    let content = fs::read_to_string("/sys/devices/system/cpu/online")
        .context("Failed to read online CPUs")?;

    let mut cpus = Vec::new();
    for range in content.trim().split(',') {
        if let Some((start, end)) = range.split_once('-') {
            let start: u32 = start.parse()?;
            let end: u32 = end.parse()?;
            for cpu in start..=end {
                cpus.push(cpu);
            }
        } else {
            cpus.push(range.parse()?);
        }
    }
    Ok(cpus)
}

/// Identify Tokio worker threads by reading /proc/pid/task/*/comm
/// Phase 3a: Finds threads with names starting with "tokio-runtime-w"
fn identify_tokio_workers(pid: i32) -> Result<Vec<u32>> {
    let task_dir = format!("/proc/{}/task", pid);
    let mut worker_tids = Vec::new();

    let entries = fs::read_dir(&task_dir)
        .context(format!("Failed to read {}", task_dir))?;

    for entry in entries {
        let entry = entry?;
        let tid_str = entry.file_name().to_string_lossy().to_string();

        if let Ok(tid) = tid_str.parse::<u32>() {
            let comm_path = format!("/proc/{}/task/{}/comm", pid, tid);

            if let Ok(comm) = fs::read_to_string(comm_path) {
                let comm = comm.trim();
                if comm.starts_with("tokio-runtime-w") {
                    worker_tids.push(tid);
                    info!("Found Tokio worker thread: TID {} ({})", tid, comm);
                }
            }
        }
    }

    Ok(worker_tids)
}

/// Register Tokio worker threads in the TOKIO_WORKER_THREADS eBPF map
/// Phase 3a: Populates the map so eBPF can filter sched_switch events
fn register_tokio_workers(bpf: &mut Ebpf, pid: i32) -> Result<usize> {
    let worker_tids = identify_tokio_workers(pid)?;

    if worker_tids.is_empty() {
        warn!("No Tokio worker threads found! Make sure the target is a Tokio app.");
        return Ok(0);
    }

    let mut map: HashMap<_, u32, WorkerInfo> = HashMap::try_from(
        bpf.map_mut("TOKIO_WORKER_THREADS")
            .context("TOKIO_WORKER_THREADS map not found")?
    )?;

    for (idx, tid) in worker_tids.iter().enumerate() {
        let mut comm = [0u8; 16];
        let comm_str = format!("tokio-runtime-w");
        let bytes = comm_str.as_bytes();
        let copy_len = bytes.len().min(16);
        comm[..copy_len].copy_from_slice(&bytes[..copy_len]);

        let info = WorkerInfo {
            worker_id: idx as u32,
            pid: pid as u32,
            comm,
            is_active: 1,
            _padding: [0u8; 3],
        };

        map.insert(*tid, info, 0)?;
    }

    info!("✓ Registered {} Tokio worker threads", worker_tids.len());
    Ok(worker_tids.len())
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

    #[arg(long, help = "Enable Chrome trace export")]
    trace: bool,

    #[arg(long, default_value = "30", help = "Duration to profile in seconds (when using --trace)")]
    duration: u64,

    #[arg(long, default_value = "trace.json", help = "Output path for trace JSON")]
    trace_output: PathBuf,

    #[arg(long, help = "Trace-only mode (no live event output)")]
    no_live: bool,

    #[arg(long, value_name = "TRACE_FILE", help = "Launch TUI to visualize a trace.json file")]
    tui: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    // If --tui flag is provided, launch TUI instead of profiler
    if let Some(trace_path) = args.tui {
        println!("🎨 Launching TUI for trace: {}", trace_path.display());
        let data = tui::TraceData::from_file(&trace_path)?;
        let app = tui::App::new(data);
        return app.run();
    }

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

    // Attach uprobe to tokio::runtime::context::set_current_task_id
    // Note: This symbol may not exist in release builds (gets inlined)
    // Task ID tracking will be unavailable if the symbol is not found
    let task_id_attached = match bpf.program_mut("set_task_id_hook") {
        Some(program) => {
            match program.try_into() {
                Ok(program) => {
                    let program: &mut UProbe = program;
                    if let Err(e) = program.load() {
                        warn!("⚠️  Failed to load set_task_id_hook: {}", e);
                        false
                    } else {
                        match program.attach(
                            Some("_ZN5tokio7runtime7context19set_current_task_id17h88510a52941c215fE"),
                            0,
                            &target_path,
                            args.pid,
                        ) {
                            Ok(_) => {
                                info!("✓ Attached uprobe: set_current_task_id");
                                true
                            }
                            Err(e) => {
                                warn!("⚠️  Could not attach set_task_id_hook: {}", e);
                                warn!("   Task ID tracking unavailable (symbol likely inlined in release build)");
                                false
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("⚠️  Failed to convert set_task_id_hook: {}", e);
                    false
                }
            }
        }
        None => {
            warn!("⚠️  set_task_id_hook program not found");
            false
        }
    };

    if !task_id_attached {
        println!("⚠️  Note: Task IDs will not be available (set_current_task_id inlined in release build)");
    }

    // Phase 3a: Setup scheduler-based detection
    if let Some(pid) = args.pid {
        println!("\n🔧 Phase 3a: Setting up scheduler-based detection...");

        // 1. Set configuration (5ms threshold and target PID)
        let mut config_map: HashMap<_, u32, u64> = HashMap::try_from(
            bpf.map_mut("CONFIG").context("CONFIG map not found")?
        )?;
        config_map.insert(0, 5_000_000, 0)?;  // 5ms threshold in nanoseconds
        config_map.insert(1, pid as u64, 0)?;  // target PID for perf_event filtering
        info!("✓ Set blocking threshold: 5ms");
        info!("✓ Set target PID: {}", pid);

        // 2. Identify and register Tokio worker threads
        let worker_count = register_tokio_workers(&mut bpf, pid)?;

        // 3. Attach sched_switch tracepoint
        let program: &mut TracePoint = bpf
            .program_mut("sched_switch_hook")
            .context("sched_switch_hook program not found")?
            .try_into()?;
        program.load()?;
        program.attach("sched", "sched_switch")?;
        info!("✓ Attached tracepoint: sched/sched_switch");

        // 4. Attach CPU sampling perf_event for stack traces
        let program: &mut PerfEvent = bpf
            .program_mut("on_cpu_sample")
            .context("on_cpu_sample program not found")?
            .try_into()?;
        program.load()?;

        // Attach perf_event sampler at 99 Hz on all CPUs
        // Using AllProcessesOneCpu with PID filtering in eBPF (more reliable)
        let online_cpus = online_cpus()?;
        info!("Attaching perf_event sampler to {} CPUs at 99 Hz (filtering for PID {})", online_cpus.len(), pid);
        for cpu in &online_cpus {
            program.attach(
                perf_event::PerfTypeId::Software,
                perf_event::perf_sw_ids::PERF_COUNT_SW_CPU_CLOCK as u64,
                perf_event::PerfEventScope::AllProcessesOneCpu { cpu: *cpu },  // Sample all processes, filter in eBPF
                perf_event::SamplePolicy::Frequency(99),  // 99 Hz sampling
                false,  // Don't inherit to child processes
            )?;
        }
        info!("✓ Attached perf_event sampler to {} CPUs at 99 Hz (filtering for PID {})", online_cpus.len(), pid);

        println!("✅ Scheduler-based detection active");
        println!("   Monitoring {} Tokio worker threads", worker_count);
        println!("   CPU sampling: 99 Hz (every ~10ms)\n");

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
        match get_memory_range(pid, &target_path) {
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

    // Initialize Chrome trace exporter if requested
    let mut trace_exporter = if args.trace {
        let mut exporter = ChromeTraceExporter::new(Symbolizer::new(&target_path)?);
        if let Some(range) = memory_range {
            exporter.set_memory_range(ExporterMemoryRange {
                start: range.start,
                end: range.end,
            });
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
    #[derive(Default)]
    struct DetectionStats {
        marker_detected: u64,
        scheduler_detected: u64,
    }
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
    let mut timeout_reached = false;

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

                        stats.marker_detected += 1;  // Phase 3a: Track marker stats

                        println!("\n🔵 MARKER DETECTED");
                        println!("   Duration: {:.2}ms {}",
                            duration_ms,
                            if duration_ms > 10.0 { "⚠️" } else { "" }
                        );
                        println!("   Process: PID {}", event.pid);
                        println!("   Thread: TID {}", event.tid);
                        if event.task_id != 0 {
                            println!("   Task ID: {}", event.task_id);
                        }

                        // Print stack trace from blocking start
                        if let Some(stack_id) = blocking_start_stack_id {
                            if stack_id >= 0 {
                                match stack_traces.get(&(stack_id as u32), 0) {
                                    Ok(stack_trace) => {
                                        let frames = stack_trace.frames();
                                        if !frames.is_empty() {
                                            println!("\n   📍 Stack trace:");
                                            info!("Stack trace has {} frames", frames.len());

                                            for (i, stack_frame) in frames.iter().enumerate() {
                                                let addr = stack_frame.ip;
                                                if addr == 0 {
                                                    info!("Frame {} has address 0, stopping", i);
                                                    break;
                                                }

                                                // Determine if address is in main executable and adjust accordingly
                                                let (file_offset, in_executable) = if let Some(range) = memory_range {
                                                    if range.contains(addr) {
                                                        // Address is in main executable, adjust to file offset
                                                        let adjusted = addr - range.start;
                                                        info!("Frame {}: 0x{:016x} (in executable) -> 0x{:08x}",
                                                            i, addr, adjusted);
                                                        (adjusted, true)
                                                    } else {
                                                        // Address is outside executable (shared library)
                                                        info!("Frame {}: 0x{:016x} (shared library, skipping)", i, addr);
                                                        (addr, false)
                                                    }
                                                } else {
                                                    // No range info, use address as-is
                                                    (addr, true)
                                                };

                                                // Only try to symbolize addresses in the main executable
                                                if in_executable {
                                                    let resolved = symbolizer.resolve(file_offset);
                                                    println!("      {}", resolved.format(i));
                                                } else {
                                                    // Show the address but don't try to symbolize shared libraries
                                                    println!("      #{:<2} 0x{:016x} <shared library>", i, addr);
                                                }
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
                EVENT_SCHEDULER_DETECTED => {
                    // Phase 3a: Scheduler-based detection
                    stats.scheduler_detected += 1;

                    let duration_ms = event.duration_ns as f64 / 1_000_000.0;

                    println!("\n🟢 SCHEDULER DETECTED");
                    println!("   Duration: {:.2}ms (off-CPU) {}",
                        duration_ms,
                        if duration_ms > 10.0 { "⚠️" } else { "" }
                    );
                    println!("   Process: PID {}", event.pid);
                    println!("   Thread: TID {}", event.tid);
                    if event.task_id != 0 {
                        println!("   Task ID: {}", event.task_id);
                    }

                    // Decode thread state
                    let state_str = match event.thread_state {
                        0 => "TASK_RUNNING (CPU blocking)",
                        1 => "TASK_INTERRUPTIBLE (I/O wait)",
                        2 => "TASK_UNINTERRUPTIBLE",
                        _ => "UNKNOWN",
                    };
                    println!("   State: {}", state_str);

                    // Print stack trace
                    if event.stack_id >= 0 {
                        match stack_traces.get(&(event.stack_id as u32), 0) {
                            Ok(stack_trace) => {
                                let frames = stack_trace.frames();
                                if !frames.is_empty() {
                                    println!("\n   📍 Stack trace:");
                                    info!("Stack trace has {} frames", frames.len());

                                    for (i, stack_frame) in frames.iter().enumerate() {
                                        let addr = stack_frame.ip;
                                        if addr == 0 {
                                            info!("Frame {} has address 0, stopping", i);
                                            break;
                                        }

                                        let (file_offset, in_executable) = if let Some(range) = memory_range {
                                            if range.contains(addr) {
                                                let adjusted = addr - range.start;
                                                info!("Frame {}: 0x{:016x} (in executable) -> 0x{:08x}",
                                                    i, addr, adjusted);
                                                (adjusted, true)
                                            } else {
                                                info!("Frame {}: 0x{:016x} (shared library, skipping)", i, addr);
                                                (addr, false)
                                            }
                                        } else {
                                            (addr, true)
                                        };

                                        if in_executable {
                                            let resolved = symbolizer.resolve(file_offset);
                                            println!("      {}", resolved.format(i));
                                        } else {
                                            println!("      #{:<2} 0x{:016x} <shared library>", i, addr);
                                        }
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
                        println!("\n   ⚠️  No stack trace captured (stack_id = {})", event.stack_id);
                    }

                    println!();
                }
                TRACE_EXECUTION_START | TRACE_EXECUTION_END => {
                    // Get the top frame address for symbol resolution
                    let top_frame_addr = if event.stack_id >= 0 {
                        match stack_traces.get(&(event.stack_id as u32), 0) {
                            Ok(stack_trace) => {
                                let frames = stack_trace.frames();
                                frames.iter().find(|f| f.ip != 0).map(|f| f.ip)
                            }
                            Err(_) => None,
                        }
                    } else {
                        None
                    };

                    // Add to trace exporter if enabled
                    if let Some(ref mut exporter) = trace_exporter {
                        exporter.add_event(&event, top_frame_addr);
                    }

                    // Optionally display live (unless --no-live)
                    if live_display {
                        let event_name = if event.event_type == TRACE_EXECUTION_START {
                            "EXEC_START"
                        } else {
                            "EXEC_END"
                        };

                        println!(
                            "🟣 {} [PID {} TID {} Worker {}]",
                            event_name,
                            event.pid,
                            event.tid,
                            if event.worker_id != u32::MAX {
                                event.worker_id.to_string()
                            } else {
                                "N/A".to_string()
                            }
                        );
                    }
                }
                _ => {
                    warn!("Unknown event type: {}", event.event_type);
                }
            }
        }

        // Phase 3a: Print statistics every 10 seconds
        if stats_timer.elapsed() > Duration::from_secs(10) {
            println!("\n📊 Detection Statistics (last 10s):");
            println!("   Marker:    {}", stats.marker_detected);
            println!("   Scheduler: {}", stats.scheduler_detected);
            println!();
            stats_timer = Instant::now();
        }

        // Show progress when in quiet trace mode
        if args.trace && !live_display && last_progress_time.elapsed() > Duration::from_secs(2) {
            if let Some(limit) = duration_limit {
                let elapsed = profiling_start.elapsed();
                let remaining = limit.saturating_sub(elapsed);
                let elapsed_secs = elapsed.as_secs();
                let remaining_secs = remaining.as_secs();
                print!("\r   Progress: {}s / {}s ({}s remaining)   ",
                    elapsed_secs, args.duration, remaining_secs);
                use std::io::Write;
                std::io::stdout().flush().ok();
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
        println!("\n🔍 DEBUG: perf_event diagnostics:");

        // Total calls
        let counter_map: HashMap<_, u32, u64> = HashMap::try_from(
            bpf.map("PERF_EVENT_COUNTER").context("PERF_EVENT_COUNTER map not found")?
        )?;
        if let Ok(count) = counter_map.get(&0u32, 0) {
            println!("   - Handler called: {} times", count);
        }

        // Passed PID filter
        let pid_filter_map: HashMap<_, u32, u64> = HashMap::try_from(
            bpf.map("PERF_EVENT_PASSED_PID_FILTER").context("PERF_EVENT_PASSED_PID_FILTER map not found")?
        )?;
        if let Ok(count) = pid_filter_map.get(&0u32, 0) {
            println!("   - Passed PID filter: {} times", count);
        } else {
            println!("   - Passed PID filter: 0 times (ALL FILTERED OUT!)");
        }

        // Output success
        let success_map: HashMap<_, u32, u64> = HashMap::try_from(
            bpf.map("PERF_EVENT_OUTPUT_SUCCESS").context("PERF_EVENT_OUTPUT_SUCCESS map not found")?
        )?;
        if let Ok(count) = success_map.get(&0u32, 0) {
            println!("   - Events output success: {}", count);
        }

        // Output failed
        let failed_map: HashMap<_, u32, u64> = HashMap::try_from(
            bpf.map("PERF_EVENT_OUTPUT_FAILED").context("PERF_EVENT_OUTPUT_FAILED map not found")?
        )?;
        if let Ok(count) = failed_map.get(&0u32, 0) {
            println!("   - Events output failed: {}", count);
        }
    }

    // Export trace if enabled
    if let Some(exporter) = trace_exporter {
        println!("\n📝 Exporting Chrome trace...");
        println!("   Events collected: {}", exporter.event_count());

        exporter.export(&args.trace_output)
            .context("Failed to export trace")?;

        println!("   ✓ Trace exported to: {}", args.trace_output.display());
        println!("\n💡 To visualize:");
        println!("   1. Open chrome://tracing in Chrome/Chromium");
        println!("   2. Click 'Load' and select {}", args.trace_output.display());
        println!("   3. Use W/A/S/D to zoom/pan the timeline");
    }

    Ok(())
}
