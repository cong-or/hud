#![no_std]
#![no_main]

use aya_ebpf::{
    macros::{map, perf_event, tracepoint, uprobe},
    maps::{HashMap, RingBuf, StackTrace},
    programs::{ProbeContext, PerfEventContext, TracePointContext},
    helpers::{bpf_ktime_get_ns, bpf_get_current_pid_tgid},
    EbpfContext,
};
use runtime_scope_common::{
    TaskEvent, ThreadState, WorkerInfo, SchedSwitchArgs, ExecutionSpan,
    EVENT_BLOCKING_START, EVENT_BLOCKING_END, EVENT_SCHEDULER_DETECTED,
    TRACE_EXECUTION_START, TRACE_EXECUTION_END,
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

// Phase 3+: Timeline visualization maps

// Map: Thread ID (TID) → Current execution span
// Tracks what each worker is currently executing for timeline viz
#[map]
static EXECUTION_SPANS: HashMap<u32, ExecutionSpan> = HashMap::with_max_entries(256, 0);

// DEBUG: Counters to verify perf_event is being called and track filtering
#[map]
static PERF_EVENT_COUNTER: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

#[map]
static PERF_EVENT_PASSED_PID_FILTER: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

#[map]
static PERF_EVENT_OUTPUT_SUCCESS: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

#[map]
static PERF_EVENT_OUTPUT_FAILED: HashMap<u32, u64> = HashMap::with_max_entries(1, 0);

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

    // Capture stack trace - use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200)
    let stack_id = unsafe {
        STACK_TRACES
            .get_stackid(ctx, 0x300)
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
        duration_ns: 0,          // Will be calculated in userspace
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0,         // Not applicable for marker detection
        task_id,
        category: 0,             // 0 = general
        detection_method: 1,     // 1 = marker-based
        is_tokio_worker: if is_tokio_worker(tid) { 1 } else { 0 },
        _padding: [0u8; 5],
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

    // Capture stack trace - use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200)
    let stack_id = unsafe {
        STACK_TRACES
            .get_stackid(ctx, 0x300)
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
        duration_ns: 0,          // Will be calculated in userspace
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0,         // Not applicable for marker detection
        task_id,
        category: 0,             // 0 = general
        detection_method: 1,     // 1 = marker-based
        is_tokio_worker: if is_tokio_worker(tid) { 1 } else { 0 },
        _padding: [0u8; 5],
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
    // Phase 3+: Emit execution end event for Tokio workers
    if is_tokio_worker(tid) {
        // Get execution span if it exists
        let span = unsafe { EXECUTION_SPANS.get(&tid).map(|s| *s) };

        if let Some(span) = span {
            // Calculate execution duration
            let duration_ns = now - span.start_time_ns;

            // Emit TRACE_EXECUTION_END event
            emit_execution_end(tid, now, duration_ns, span.stack_id)?;

            // Clear execution span
            unsafe { EXECUTION_SPANS.remove(&tid)?; }
        }
    }

    // Update thread state (for legacy scheduler-based detection)
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

    // Phase 3+: Track execution span and emit start event
    // Use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200) for better unwinding
    let stack_id = unsafe {
        STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1)
    };
    let cpu_id = get_cpu_id();

    // Create execution span
    let span = ExecutionSpan {
        start_time_ns: now,
        stack_id,
        cpu_id,
        _padding: [0; 4],
    };

    unsafe {
        EXECUTION_SPANS.insert(&tid, &span, 0)?;
    }

    // Emit TRACE_EXECUTION_START event
    emit_execution_start(tid, now, stack_id)?;

    // Get thread state (for legacy scheduler-based detection)
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

        // Only report CPU-bound blocking (TASK_RUNNING state)
        // This filters out async yields and I/O waits (TASK_INTERRUPTIBLE)
        // When scheduler preempts a CPU-bound task, state = TASK_RUNNING (0)
        if thread_state.off_cpu_duration > threshold_ns
            && thread_state.state_when_switched == 0 {  // TASK_RUNNING only

            let task_id = unsafe {
                THREAD_TASK_MAP.get(&tid).map(|id| *id).unwrap_or(0)
            };

            let stack_id = unsafe {
                STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1)
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
        duration_ns,
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state,
        task_id,
        category: 0,          // 0 = general
        detection_method: 2,  // 2 = scheduler-based
        is_tokio_worker: 1,   // Only workers trigger scheduler detection
        _padding: [0u8; 5],
    };

    unsafe {
        EVENTS.output(&event, 0).map_err(|_| 1i64)?;
    }

    Ok(())
}

// Helper: Emit TRACE_EXECUTION_START event
fn emit_execution_start(tid: u32, timestamp_ns: u64, stack_id: i64) -> Result<(), i64> {
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;

    let task_id = unsafe {
        THREAD_TASK_MAP.get(&tid).map(|id| *id).unwrap_or(0)
    };

    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: TRACE_EXECUTION_START,
        stack_id,
        duration_ns: 0,  // Not applicable for start event
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0,
        task_id,
        category: 0,             // 0 = general
        detection_method: 3,     // 3 = trace
        is_tokio_worker: 1,
        _padding: [0u8; 5],
    };

    unsafe {
        EVENTS.output(&event, 0).map_err(|_| 1i64)?;
    }

    Ok(())
}

// Helper: Emit TRACE_EXECUTION_END event
fn emit_execution_end(
    tid: u32,
    timestamp_ns: u64,
    duration_ns: u64,
    stack_id: i64,
) -> Result<(), i64> {
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;

    let task_id = unsafe {
        THREAD_TASK_MAP.get(&tid).map(|id| *id).unwrap_or(0)
    };

    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: TRACE_EXECUTION_END,
        stack_id,
        duration_ns,
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0,
        task_id,
        category: 0,             // 0 = general
        detection_method: 3,     // 3 = trace
        is_tokio_worker: 1,
        _padding: [0u8; 5],
    };

    unsafe {
        EVENTS.output(&event, 0).map_err(|_| 1i64)?;
    }

    Ok(())
}

// Helper: Get worker ID for a TID (or u32::MAX if not a worker)
fn get_worker_id(tid: u32) -> u32 {
    unsafe {
        TOKIO_WORKER_THREADS
            .get(&tid)
            .map(|info| info.worker_id)
            .unwrap_or(u32::MAX)
    }
}

// Helper: Check if TID is a Tokio worker
fn is_tokio_worker(tid: u32) -> bool {
    unsafe { TOKIO_WORKER_THREADS.get(&tid).is_some() }
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
        let current = PERF_EVENT_COUNTER.get(&key).map(|v| *v).unwrap_or(0);
        let _ = PERF_EVENT_COUNTER.insert(&key, &(current + 1), 0);
    }

    // Get current process/thread info
    let pid_tgid = unsafe { bpf_get_current_pid_tgid() };
    let pid = (pid_tgid >> 32) as u32;
    let tid = pid_tgid as u32;

    // Filter by target PID (since we're using AllProcessesOneCpu scope)
    // CONFIG[1] contains the target PID set by userspace
    let target_pid = unsafe {
        CONFIG.get(&1).map(|v| *v as u32).unwrap_or(0)
    };
    if target_pid != 0 && pid != target_pid {
        return Ok(());
    }

    // DEBUG: Track how many events pass PID filter
    unsafe {
        let key = 0u32;
        let current = PERF_EVENT_PASSED_PID_FILTER.get(&key).map(|v| *v).unwrap_or(0);
        let _ = PERF_EVENT_PASSED_PID_FILTER.insert(&key, &(current + 1), 0);
    }

    // DEBUG: Temporarily disable Tokio worker filter to test if perf_event fires
    // Once we confirm perf_event works, we'll re-enable this filter
    // if !is_tokio_worker(tid) {
    //     return Ok(());
    // }

    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture stack trace - perf_event context has pt_regs, so this should work!
    // Use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200)
    let stack_id = unsafe {
        STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1)
    };

    let worker_id = get_worker_id(tid);
    let cpu_id = get_cpu_id();

    // Get current task ID if available
    let task_id = unsafe {
        THREAD_TASK_MAP.get(&tid).map(|id| *id).unwrap_or(0)
    };

    // Emit a sample event
    // We'll use TRACE_EXECUTION_START with a special marker to indicate it's a sample
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: TRACE_EXECUTION_START,
        stack_id,
        duration_ns: 0,  // Samples don't have duration
        worker_id,
        cpu_id,
        thread_state: 0,
        task_id,
        category: 0,
        detection_method: 4,  // 4 = perf_event sampling
        is_tokio_worker: 1,
        _padding: [0u8; 5],
    };

    let output_result = unsafe {
        EVENTS.output(&event, 0)
    };

    // DEBUG: Track event output success/failure
    unsafe {
        let key = 0u32;
        if output_result.is_ok() {
            let current = PERF_EVENT_OUTPUT_SUCCESS.get(&key).map(|v| *v).unwrap_or(0);
            let _ = PERF_EVENT_OUTPUT_SUCCESS.insert(&key, &(current + 1), 0);
        } else {
            let current = PERF_EVENT_OUTPUT_FAILED.get(&key).map(|v| *v).unwrap_or(0);
            let _ = PERF_EVENT_OUTPUT_FAILED.insert(&key, &(current + 1), 0);
        }
    }

    output_result.map_err(|_| 1i64)?;
    Ok(())
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
