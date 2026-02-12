//! # eBPF Kernel-Side Instrumentation
//!
//! eBPF programs that run inside the Linux kernel to capture profiling events.
//!
//! ## Programs
//!
//! - **Tracepoint**: `sched_switch_hook` - Scheduler-based detection (off-CPU > 5ms)
//! - **Perf Event**: `on_cpu_sample` - CPU sampling at 99 Hz for stack traces
//!
//! ## Maps (Shared with Userspace)
//!
//! - `EVENTS` - Ring buffer (256KB) for event stream
//! - `STACK_TRACES` - Deduplicated stack traces by ID
//! - `TOKIO_WORKER_THREADS` - Worker thread registry
//! - `CONFIG` - Runtime configuration (threshold, target PID)
//!
//! ## Build
//!
//! Always compiled in release mode (debug includes incompatible formatting code):
//! ```bash
//! cargo xtask build-ebpf --release
//! ```
//!
//! See [Architecture docs](../../docs/ARCHITECTURE.md) for details on eBPF mechanics.

#![no_std]
#![no_main]
#![allow(unused_unsafe)]

use aya_ebpf::{
    helpers::{bpf_get_current_pid_tgid, bpf_ktime_get_ns},
    macros::{map, perf_event, tracepoint, uprobe},
    maps::{HashMap, RingBuf, StackTrace},
    programs::{PerfEventContext, ProbeContext, TracePointContext},
    EbpfContext,
};
use hud_common::{
    SchedSwitchArgs, TaskEvent, ThreadState, WorkerInfo, DETECTION_PERF_SAMPLE,
    DETECTION_SCHEDULER, EVENT_SCHEDULER_DETECTED, TRACE_EXECUTION_START,
};

// ============================================================================
// Constants
// ============================================================================

/// Stack capture flags for `bpf_get_stackid`:
///
/// - BPF_F_USER_STACK (0x100): Capture user-space stack (not kernel)
/// - BPF_F_FAST_STACK_CMP (0x200): Use stack hash for deduplication (faster, slight collision risk)
/// - BPF_F_REUSE_STACKID (0x400): Overwrite existing entry on hash collision instead of returning -EEXIST
const STACK_FLAGS: u64 = 0x100 | 0x200 | 0x400;

// ============================================================================
// eBPF Maps - Shared data structures between kernel and userspace
// ============================================================================

/// Ring buffer for sending events to userspace (lock-free, high-throughput)
///
/// - **Size**: 4MB
/// - **Type**: LIFO ring buffer (overwrite oldest on overflow)
/// - **Usage**: Kernel writes with `EVENTS.output()`, userspace reads with `ring_buf.next()`
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(4 * 1024 * 1024, 0); // 4MB buffer

/// Stack trace map for storing deduplicated stack traces by ID
///
/// - **Max Entries**: 16384 unique stacks
/// - **Key**: Stack hash (computed by kernel)
/// - **Value**: Array of instruction pointers (addresses)
/// - **Usage**: Kernel captures with `get_stackid()`, userspace resolves with DWARF
///
/// Sized to avoid exhaustion from perf_event samples across many CPUs.
/// A full map causes `bpf_get_stackid` to silently fail (-ENOMEM),
/// dropping all new stack captures.
#[map]
static STACK_TRACES: StackTrace = StackTrace::with_max_entries(16384, 0);

/// Map: Thread ID (TID) → Tokio Task ID
///
/// Tracks which async task is currently running on each thread.
/// Updated by `set_task_id_hook` when Tokio switches tasks.
/// Allows attributing blocking operations to specific tasks.
#[map]
static THREAD_TASK_MAP: HashMap<u32, u64> = HashMap::with_max_entries(1024, 0);

/// Map: Thread ID (TID) → Thread execution state
///
/// Tracks when threads go ON/OFF CPU for scheduler-based blocking detection.
/// - **last_on_cpu_ns**: Timestamp when thread was last scheduled
/// - **last_off_cpu_ns**: Timestamp when thread was last preempted
/// - **off_cpu_duration**: How long thread was off-CPU (for threshold check)
/// - **state_when_switched**: Linux task state (0=TASK_RUNNING, 1=TASK_INTERRUPTIBLE, etc.)
#[map]
static THREAD_STATE: HashMap<u32, ThreadState> = HashMap::with_max_entries(4096, 0);

/// Map: Thread ID (TID) → Worker metadata
///
/// Registry of Tokio worker threads, populated by userspace after discovery.
/// Used to filter events to only Tokio workers (not other threads).
/// - **worker_id**: Tokio worker index (0, 1, 2, ...)
/// - **pid**: Process ID (TGID)
/// - **comm**: Thread name (e.g., "tokio-runtime-w")
#[map]
static TOKIO_WORKER_THREADS: HashMap<u32, WorkerInfo> = HashMap::with_max_entries(256, 0);

/// Map: Config key → Config value
///
/// Configuration passed from userspace without recompiling eBPF.
/// - **Key 0**: Blocking threshold in nanoseconds (default: 5,000,000 = 5ms)
/// - **Key 1**: Target PID for perf_event filtering
#[map]
static CONFIG: HashMap<u32, u64> = HashMap::with_max_entries(16, 0);

// ============================================================================
// Debug Counters - Diagnostic metrics for perf_event monitoring
// ============================================================================

/// Total number of perf_event invocations (verifies hook is firing)
#[map]
static PERF_EVENT_COUNTER: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

/// Number of perf_events that passed PID filter (matched target process)
#[map]
static PERF_EVENT_PASSED_PID_FILTER: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

/// Number of successful ring buffer writes from perf_event
#[map]
static PERF_EVENT_OUTPUT_SUCCESS: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

/// Number of failed ring buffer writes from perf_event (ring buffer full)
#[map]
static PERF_EVENT_OUTPUT_FAILED: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

// ============================================================================
// eBPF Program Hooks
// ============================================================================

/// Hook: tokio::runtime::context::set_current_task_id
/// Called when a task starts executing on a thread
#[uprobe]
pub fn set_task_id_hook(ctx: ProbeContext) -> u32 {
    match try_set_task_id(&ctx) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

fn try_set_task_id(ctx: &ProbeContext) -> Result<(), i64> {
    // Get thread ID
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let tid = pid_tgid as u32;

    // Get task ID from function argument (first parameter in rdi register)
    // tokio::runtime::task::id::Id is a wrapper around u64
    let task_id: u64 = unsafe { ctx.arg(0).ok_or(1i64)? };

    // Store thread → task mapping
    unsafe {
        THREAD_TASK_MAP.insert(&tid, &task_id, 0).map_err(|_| 1i64)?;
    }

    Ok(())
}

/// Hook: sched_switch tracepoint for scheduler-based blocking detection
/// Fires when the Linux scheduler switches between threads
#[tracepoint]
pub fn sched_switch_hook(ctx: TracePointContext) -> u32 {
    match try_sched_switch(&ctx) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

fn try_sched_switch(ctx: &TracePointContext) -> Result<(), i64> {
    // Read tracepoint arguments
    // Layout from /sys/kernel/debug/tracing/events/sched/sched_switch/format
    let args: *const SchedSwitchArgs = ctx.as_ptr() as *const SchedSwitchArgs;
    let prev_pid = unsafe { (*args).prev_pid as u32 };
    let prev_state = unsafe { (*args).prev_state };
    let next_pid = unsafe { (*args).next_pid as u32 };

    let now = unsafe { bpf_ktime_get_ns() };

    // Handle thread going OFF CPU (prev_pid)
    handle_thread_off_cpu(prev_pid, prev_state, now)?;

    // Handle thread going ON CPU (next_pid)
    handle_thread_on_cpu(next_pid, now, ctx)?;

    Ok(())
}

fn handle_thread_off_cpu(tid: u32, state: i64, now: u64) -> Result<(), i64> {
    // Update thread state for blocking detection
    let mut thread_state = unsafe { THREAD_STATE.get(&tid).copied().unwrap_or_default() };

    thread_state.last_off_cpu_ns = now;
    thread_state.state_when_switched = state;

    unsafe {
        THREAD_STATE.insert(&tid, &thread_state, 0)?;
    }

    Ok(())
}

fn handle_thread_on_cpu(tid: u32, now: u64, _ctx: &TracePointContext) -> Result<(), i64> {
    // Early exit: Only process Tokio worker threads
    let is_worker = unsafe { TOKIO_WORKER_THREADS.get(&tid).is_some() };
    if !is_worker {
        return Ok(());
    }

    // Execution timeline events (TRACE_EXECUTION_START/END) are now handled
    // exclusively by perf_event samples, which capture correct stacks.
    // sched_switch only tracks state for blocking detection below.

    // Get thread state for blocking detection
    let mut thread_state = unsafe { THREAD_STATE.get(&tid).copied().unwrap_or_default() };

    // Calculate how long thread was OFF CPU
    if thread_state.last_off_cpu_ns > 0 {
        thread_state.off_cpu_duration = now - thread_state.last_off_cpu_ns;

        // BLOCKING DETECTION HEURISTIC
        let threshold_ns = get_threshold_ns();

        // Only report CPU-bound blocking (TASK_RUNNING state)
        // This filters out async yields and I/O waits (TASK_INTERRUPTIBLE)
        // When scheduler preempts a CPU-bound task, state = TASK_RUNNING (0)
        if thread_state.off_cpu_duration > threshold_ns && thread_state.state_when_switched == 0 {
            // TASK_RUNNING only

            let task_id = unsafe { THREAD_TASK_MAP.get(&tid).copied().unwrap_or(0) };

            // Stack from sched_switch is the outgoing thread's, not ours.
            // Perf_event samples provide correct stacks for blocking workers.
            let stack_id: i64 = -1;

            report_scheduler_blocking(
                tid,
                task_id,
                thread_state.off_cpu_duration,
                stack_id,
                thread_state.state_when_switched,
            )?;
        }
    }

    thread_state.last_on_cpu_ns = now;

    unsafe {
        THREAD_STATE.insert(&tid, &thread_state, 0)?;
    }

    Ok(())
}

fn get_threshold_ns() -> u64 {
    // Default to 5_000_000 ns (5ms)
    unsafe { CONFIG.get(&0).copied().unwrap_or(5_000_000) }
}

fn report_scheduler_blocking(
    tid: u32,
    task_id: u64,
    duration_ns: u64,
    stack_id: i64,
    thread_state: i64,
) -> Result<(), i64> {
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;

    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns: unsafe { bpf_ktime_get_ns() },
        event_type: EVENT_SCHEDULER_DETECTED,
        stack_id,
        duration_ns,
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state,
        task_id,
        category: 0, // 0 = general
        detection_method: DETECTION_SCHEDULER,
        is_tokio_worker: 1, // Only workers trigger scheduler detection
        _padding: [0u8; 5],
    };

    unsafe {
        EVENTS.output(&event, 0).map_err(|_| 1i64)?;
    }

    Ok(())
}

// Helper: Get worker ID for a TID (or u32::MAX if not a worker)
fn get_worker_id(tid: u32) -> u32 {
    unsafe { TOKIO_WORKER_THREADS.get(&tid).map(|info| info.worker_id).unwrap_or(u32::MAX) }
}

// Helper: Get CPU ID (using aya's helper when available)
fn get_cpu_id() -> u32 {
    // aya-ebpf doesn't expose bpf_get_smp_processor_id directly yet
    // For now, return 0 (we can add this later with raw bpf call)
    0
}

/// CPU Sampling Profiler - Captures stack traces via perf_event
/// This replaces sched_switch for timeline visualization
/// Samples at configurable frequency (e.g., 99 Hz)
#[perf_event]
pub fn on_cpu_sample(ctx: PerfEventContext) -> u32 {
    match try_on_cpu_sample(&ctx) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

fn try_on_cpu_sample(ctx: &PerfEventContext) -> Result<(), i64> {
    // DEBUG: Increment counter to verify perf_event is being called
    unsafe {
        let key = 0u32;
        let current = PERF_EVENT_COUNTER.get(&key).copied().unwrap_or(0);
        let _ = PERF_EVENT_COUNTER.insert(&key, &(current + 1), 0);
    }

    // Get current process/thread info
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;
    let tid = pid_tgid as u32;

    // Filter by target PID (since we're using AllProcessesOneCpu scope)
    // CONFIG[1] contains the target PID set by userspace
    let target_pid = unsafe { CONFIG.get(&1).map(|v| *v as u32).unwrap_or(0) };
    if target_pid != 0 && pid != target_pid {
        return Ok(());
    }

    // DEBUG: Track how many events pass PID filter
    unsafe {
        let key = 0u32;
        let current = PERF_EVENT_PASSED_PID_FILTER.get(&key).copied().unwrap_or(0);
        let _ = PERF_EVENT_PASSED_PID_FILTER.insert(&key, &(current + 1), 0);
    }

    // NOTE: Worker filter intentionally disabled here.
    // Blocking pool threads are filtered in userspace (is_blocking_pool_stack)
    // which is more precise than thread-level filtering. Enabling the worker
    // filter here breaks frame-pointer-based stack capture for perf_event samples.

    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture user-space stack trace (preserve raw error code for diagnostics)
    let stack_id = unsafe { STACK_TRACES.get_stackid(ctx, STACK_FLAGS).unwrap_or_else(|e| e) };

    let worker_id = get_worker_id(tid);
    let cpu_id = get_cpu_id();

    // Get current task ID if available
    let task_id = unsafe { THREAD_TASK_MAP.get(&tid).copied().unwrap_or(0) };

    // Emit a sample event
    // We'll use TRACE_EXECUTION_START with a special marker to indicate it's a sample
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: TRACE_EXECUTION_START,
        stack_id,
        duration_ns: 0, // Samples don't have duration
        worker_id,
        cpu_id,
        thread_state: 0,
        task_id,
        category: 0,
        detection_method: DETECTION_PERF_SAMPLE,
        is_tokio_worker: 1,
        _padding: [0u8; 5],
    };

    let output_result = unsafe { EVENTS.output(&event, 0) };

    // DEBUG: Track event output success/failure
    unsafe {
        let key = 0u32;
        if output_result.is_ok() {
            let current = PERF_EVENT_OUTPUT_SUCCESS.get(&key).copied().unwrap_or(0);
            let _ = PERF_EVENT_OUTPUT_SUCCESS.insert(&key, &(current + 1), 0);
        } else {
            let current = PERF_EVENT_OUTPUT_FAILED.get(&key).copied().unwrap_or(0);
            let _ = PERF_EVENT_OUTPUT_FAILED.insert(&key, &(current + 1), 0);
        }
    }

    output_result.map_err(|_| 1i64)?;
    Ok(())
}

#[cfg(all(not(test), target_os = "none"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
