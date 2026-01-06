// Time conversions intentionally lose precision for display purposes
#![allow(clippy::cast_precision_loss, clippy::items_after_statements)]

use crate::domain::StackId;
use crate::profiling::StackResolver;
use aya::maps::{MapData, StackTraceMap};
use hud_common::TaskEvent;
use std::borrow::Borrow;

/// Display a blocking start event
pub fn display_blocking_start(event: &TaskEvent) {
    println!("🔴 [PID {} TID {}] Blocking started", event.pid, event.tid);
}

/// Display a blocking end event (marker detection)
#[allow(clippy::similar_names)]
pub fn display_blocking_end<T: Borrow<MapData>>(
    event: &TaskEvent,
    start_time: u64,
    stack_id: Option<i64>,
    stack_resolver: &StackResolver,
    stack_traces: &StackTraceMap<T>,
) {
    let duration_ns = event.timestamp_ns - start_time;
    let duration_ms = duration_ns as f64 / 1_000_000.0;

    println!("\n🔵 MARKER DETECTED");
    println!("   Duration: {:.2}ms {}", duration_ms, if duration_ms > 10.0 { "⚠️" } else { "" });
    println!("   Process: PID {}", event.pid);
    println!("   Thread: TID {}", event.tid);
    if event.task_id != 0 {
        println!("   Task ID: {}", event.task_id);
    }

    // Print stack trace from blocking start
    if let Some(stack_id) = stack_id {
        let _ = stack_resolver.resolve_and_print(StackId(stack_id), stack_traces);
    }

    println!();
}

/// Display a blocking end event with no matching start
pub fn display_blocking_end_no_start(event: &TaskEvent) {
    println!("  ✓ [PID {} TID {}] Blocking ended (no start time)", event.pid, event.tid);
}

/// Display a scheduler-detected event
#[allow(clippy::similar_names)]
pub fn display_scheduler_detected<T: Borrow<MapData>>(
    event: &TaskEvent,
    stack_resolver: &StackResolver,
    stack_traces: &StackTraceMap<T>,
) {
    let duration_ms = event.duration_ns as f64 / 1_000_000.0;

    println!("\n🟢 SCHEDULER DETECTED");
    println!(
        "   Duration: {:.2}ms (off-CPU) {}",
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
    println!("   State: {state_str}");

    // Print stack trace
    let _ = stack_resolver.resolve_and_print(StackId(event.stack_id), stack_traces);

    println!();
}

/// Display an execution event (trace start/end) in live mode
pub fn display_execution_event(event: &TaskEvent, is_start: bool) {
    let event_name = if is_start { "EXEC_START" } else { "EXEC_END" };

    println!(
        "🟣 {} [PID {} TID {} Worker {}]",
        event_name,
        event.pid,
        event.tid,
        if event.worker_id == u32::MAX { "N/A".to_string() } else { event.worker_id.to_string() }
    );
}

/// Statistics for detection methods
#[derive(Default)]
pub struct DetectionStats {
    pub marker_detected: u64,
    pub scheduler_detected: u64,
}

/// Display detection statistics
pub fn display_statistics(stats: &DetectionStats) {
    println!("\n📊 Detection Statistics (last 10s):");
    println!("   Marker:    {}", stats.marker_detected);
    println!("   Scheduler: {}", stats.scheduler_detected);
    println!();
}

/// Display progress for trace collection
pub fn display_progress(elapsed_secs: u64, duration: u64, remaining_secs: u64) {
    print!("\r   Progress: {elapsed_secs}s / {duration}s ({remaining_secs}s remaining)   ");
    use std::io::Write;
    std::io::stdout().flush().ok();
}
