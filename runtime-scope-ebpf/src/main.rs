#![no_std]
#![no_main]

use aya_ebpf::{
    macros::{map, tracepoint, uprobe},
    maps::{HashMap, RingBuf, StackTrace},
    programs::{ProbeContext, TracePointContext},
    helpers::{bpf_ktime_get_ns, bpf_get_current_pid_tgid},
    EbpfContext,
};
use runtime_scope_common::{
    TaskEvent, ThreadState, WorkerInfo, SchedSwitchArgs,
    EVENT_BLOCKING_START, EVENT_BLOCKING_END, EVENT_SCHEDULER_DETECTED,
};

// Ring buffer for sending events to userspace
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0); // 256KB buffer

// Stack trace map for storing stack traces
#[map]
static STACK_TRACES: StackTrace = StackTrace::with_max_entries(1024, 0);

// Map: Thread ID (TID) → Tokio Task ID
// Tracks which task is currently running on each thread
#[map]
static THREAD_TASK_MAP: HashMap<u32, u64> = HashMap::with_max_entries(1024, 0);

// Phase 3a: New maps for scheduler-based detection

// Map: Thread ID (TID) → Thread execution state
// Tracks when threads go on/off CPU for blocking detection
#[map]
static THREAD_STATE: HashMap<u32, ThreadState> = HashMap::with_max_entries(4096, 0);

// Map: Thread ID (TID) → Worker metadata
// Tracks which threads are Tokio worker threads
#[map]
static TOKIO_WORKER_THREADS: HashMap<u32, WorkerInfo> = HashMap::with_max_entries(256, 0);

// Map: Config key → Config value
// Configuration from userspace (threshold, etc.)
#[map]
static CONFIG: HashMap<u32, u64> = HashMap::with_max_entries(16, 0);

/// Hook: trace_blocking_start()
#[uprobe]
pub fn trace_blocking_start_hook(ctx: ProbeContext) -> u32 {
    match try_trace_blocking_start(&ctx) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

fn try_trace_blocking_start(ctx: &ProbeContext) -> Result<(), i64> {
    // Get current process/thread info
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;  // Process ID (TGID)
    let tid = pid_tgid as u32;           // Thread ID (PID)
    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture stack trace - use BPF_F_USER_STACK (0x100) flag
    let stack_id = unsafe {
        STACK_TRACES
            .get_stackid(ctx, 0x100)
            .unwrap_or(-1)
    };

    // Look up current task ID for this thread
    let task_id = unsafe {
        THREAD_TASK_MAP
            .get(&tid)
            .map(|id| *id)
            .unwrap_or(0)
    };

    // Create event
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: EVENT_BLOCKING_START,
        stack_id,
        task_id,
        duration_ns: 0,          // Will be calculated in userspace
        thread_state: 0,         // Not applicable for marker detection
        detection_method: 1,     // 1 = marker-based
        _padding: [0u8; 7],
    };

    // Send to userspace via ring buffer
    EVENTS.output(&event, 0).map_err(|_| 1i64)?;

    Ok(())
}

/// Hook: trace_blocking_end()
#[uprobe]
pub fn trace_blocking_end_hook(ctx: ProbeContext) -> u32 {
    match try_trace_blocking_end(&ctx) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

fn try_trace_blocking_end(ctx: &ProbeContext) -> Result<(), i64> {
    // Get current process/thread info
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;  // Process ID (TGID)
    let tid = pid_tgid as u32;           // Thread ID (PID)
    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture stack trace - use BPF_F_USER_STACK (0x100) flag
    let stack_id = unsafe {
        STACK_TRACES
            .get_stackid(ctx, 0x100)
            .unwrap_or(-1)
    };

    // Look up current task ID for this thread
    let task_id = unsafe {
        THREAD_TASK_MAP
            .get(&tid)
            .map(|id| *id)
            .unwrap_or(0)
    };

    // Create event
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: EVENT_BLOCKING_END,
        stack_id,
        task_id,
        duration_ns: 0,          // Will be calculated in userspace
        thread_state: 0,         // Not applicable for marker detection
        detection_method: 1,     // 1 = marker-based
        _padding: [0u8; 7],
    };

    // Send to userspace via ring buffer
    EVENTS.output(&event, 0).map_err(|_| 1i64)?;

    Ok(())
}

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
        THREAD_TASK_MAP
            .insert(&tid, &task_id, 0)
            .map_err(|_| 1i64)?;
    }

    Ok(())
}

/// Hook: sched_switch tracepoint (Phase 3a: Scheduler-based detection)
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
    // Get or create thread state
    let mut thread_state = unsafe {
        THREAD_STATE
            .get(&tid)
            .map(|s| *s)
            .unwrap_or_default()
    };

    thread_state.last_off_cpu_ns = now;
    thread_state.state_when_switched = state;

    unsafe {
        THREAD_STATE.insert(&tid, &thread_state, 0)?;
    }

    Ok(())
}

fn handle_thread_on_cpu(tid: u32, now: u64, ctx: &TracePointContext) -> Result<(), i64> {
    // Early exit: Only process Tokio worker threads
    let is_worker = unsafe { TOKIO_WORKER_THREADS.get(&tid).is_some() };
    if !is_worker {
        return Ok(());
    }

    // Get thread state
    let mut thread_state = unsafe {
        THREAD_STATE
            .get(&tid)
            .map(|s| *s)
            .unwrap_or_default()
    };

    // Calculate how long thread was OFF CPU
    if thread_state.last_off_cpu_ns > 0 {
        thread_state.off_cpu_duration = now - thread_state.last_off_cpu_ns;

        // BLOCKING DETECTION HEURISTIC
        let threshold_ns = get_threshold_ns();

        // Only report if:
        // 1. Duration exceeds threshold (5ms)
        // 2. Thread was in TASK_RUNNING state (CPU blocking, not I/O wait)
        if thread_state.off_cpu_duration > threshold_ns
            && thread_state.state_when_switched == 0 {  // TASK_RUNNING

            let task_id = unsafe {
                THREAD_TASK_MAP.get(&tid).map(|id| *id).unwrap_or(0)
            };

            let stack_id = unsafe {
                STACK_TRACES.get_stackid(ctx, 0x100).unwrap_or(-1)
            };

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
    unsafe {
        CONFIG.get(&0).map(|v| *v).unwrap_or(5_000_000)
    }
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
        task_id,
        duration_ns,
        thread_state,
        detection_method: 2,  // 2 = scheduler-based
        _padding: [0u8; 7],
    };

    unsafe {
        EVENTS.output(&event, 0).map_err(|_| 1i64)?;
    }

    Ok(())
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
