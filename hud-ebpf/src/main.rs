//! # Runtime Scope - eBPF Kernel-Side Instrumentation
//!
//! This module contains the eBPF programs that run **inside the Linux kernel**
//! to instrument Tokio applications with minimal overhead. eBPF (extended Berkeley
//! Packet Filter) allows safe, sandboxed code execution in the kernel for observability.
//!
//! ## What is eBPF?
//!
//! eBPF is a Linux kernel technology that allows running sandboxed programs inside
//! the kernel without modifying kernel code or loading kernel modules. Key features:
//!
//! - **Safety**: Verifier ensures programs terminate and don't crash the kernel
//! - **Performance**: JIT-compiled to native code, runs at near-native speed
//! - **Flexibility**: Can attach to various kernel events (tracepoints, kprobes, uprobes)
//! - **Observability**: Low-overhead instrumentation for profiling and tracing
//!
//! ## Architecture: Kernel ↔ Userspace Communication
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Linux Kernel                           │
//! │                                                             │
//! │  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐   │
//! │  │   Uprobes    │   │ Tracepoints  │   │ Perf Events  │   │
//! │  │  (marker)    │   │ (sched_sw)   │   │  (99 Hz)     │   │
//! │  └──────┬───────┘   └──────┬───────┘   └──────┬───────┘   │
//! │         │                  │                  │            │
//! │         └──────────────────┼──────────────────┘            │
//! │                            ▼                                │
//! │                  ┌──────────────────┐                      │
//! │                  │  eBPF Programs   │ (this file)          │
//! │                  │  • Hooks         │                      │
//! │                  │  • Event logic   │                      │
//! │                  │  • Stack traces  │                      │
//! │                  └────────┬─────────┘                      │
//! │                           │                                 │
//! │                           ▼                                 │
//! │                  ┌──────────────────┐                      │
//! │                  │   eBPF Maps      │                      │
//! │                  │  • EVENTS (ring) │ ◄─── Shared Memory   │
//! │                  │  • STACK_TRACES  │                      │
//! │                  │  • WORKER_INFO   │                      │
//! │                  └────────┬─────────┘                      │
//! └───────────────────────────┼─────────────────────────────────┘
//!                             │
//!                             │ mmap'd into userspace
//!                             ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Userspace (hud)                         │
//! │                                                             │
//! │   • Poll ring buffer for events (EVENTS.next())            │
//! │   • Read stack traces (STACK_TRACES.get())                 │
//! │   • Write config (CONFIG.insert())                         │
//! │   • Register workers (TOKIO_WORKER_THREADS.insert())       │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## eBPF Programs (Hooks)
//!
//! This file defines several eBPF programs that attach to different kernel events:
//!
//! ### 1. Uprobes (Userspace Function Tracing)
//!
//! Uprobes dynamically instrument userspace functions by setting breakpoints:
//!
//! - **`trace_blocking_start_hook`**: Fires when app calls `trace_blocking_start()`
//!   - Captures stack trace at blocking operation start
//!   - Records timestamp for duration calculation
//!   - **Detection Method**: Marker-based (explicit)
//!
//! - **`trace_blocking_end_hook`**: Fires when app calls `trace_blocking_end()`
//!   - Emits event with duration (calculated in userspace)
//!   - **Detection Method**: Marker-based (explicit)
//!
//! - **`set_task_id_hook`**: Fires when Tokio switches tasks on a worker thread
//!   - Tracks which Tokio task is executing on each thread
//!   - **Note**: May be inlined in release builds (opportunistic)
//!
//! ### 2. Tracepoints (Kernel Event Tracing)
//!
//! Tracepoints are stable kernel ABI for tracing kernel subsystems:
//!
//! - **`sched_switch_hook`**: Fires when Linux scheduler switches threads
//!   - Monitors when Tokio worker threads go ON/OFF CPU
//!   - Emits start/end events for timeline visualization
//!   - Detects blocking via threshold heuristic (off-CPU > 5ms)
//!   - **Detection Method**: Scheduler-based (implicit)
//!
//! ### 3. Perf Events (CPU Sampling)
//!
//! Perf events are high-frequency timers for statistical profiling:
//!
//! - **`on_cpu_sample`**: Fires at 99 Hz (every ~10ms) on each CPU
//!   - Captures stack traces of what's running
//!   - Filters by target PID and Tokio worker threads
//!   - **Detection Method**: Sampling-based (statistical)
//!
//! ## eBPF Maps (Shared Data Structures)
//!
//! eBPF maps are kernel data structures shared between kernel and userspace:
//!
//! ### Communication Maps
//!
//! - **`EVENTS` (RingBuf)**: Lock-free ring buffer (256KB) for sending events
//!   - Kernel writes events with `EVENTS.output()`
//!   - Userspace polls with `ring_buf.next()`
//!   - **Purpose**: High-throughput event stream to userspace
//!
//! - **`STACK_TRACES` (StackTrace)**: Stores stack traces by ID
//!   - Kernel captures with `get_stackid()` (deduplicates identical stacks)
//!   - Userspace resolves addresses to symbols via DWARF
//!   - **Purpose**: Efficient stack trace storage (IDs instead of full traces)
//!
//! ### Configuration Maps
//!
//! - **`CONFIG` (HashMap<u32, u64>)**: Configuration from userspace
//!   - Key 0: Blocking threshold in nanoseconds (default: 5ms)
//!   - Key 1: Target PID for perf_event filtering
//!   - **Purpose**: Runtime configuration without recompiling eBPF
//!
//! - **`TOKIO_WORKER_THREADS` (HashMap<TID, WorkerInfo>)**: Worker registry
//!   - Populated by userspace after discovering workers via `/proc`
//!   - Used to filter events to only Tokio workers
//!   - **Purpose**: Distinguish worker threads from other threads
//!
//! ### State Tracking Maps
//!
//! - **`THREAD_TASK_MAP` (HashMap<TID, TaskID>)**: Thread → Tokio Task mapping
//!   - Updated by `set_task_id_hook` when tasks switch
//!   - **Purpose**: Attribute blocking to specific async tasks
//!
//! - **`THREAD_STATE` (HashMap<TID, ThreadState>)**: Thread execution state
//!   - Tracks last ON/OFF CPU times, off-CPU duration
//!   - **Purpose**: Scheduler-based blocking detection
//!
//! - **`EXECUTION_SPANS` (HashMap<TID, ExecutionSpan>)**: Current execution span
//!   - Tracks what each worker is currently executing
//!   - **Purpose**: Timeline visualization (start/end pairing)
//!
//! ### Debug Counters
//!
//! - **`PERF_EVENT_COUNTER`**: Total perf_event invocations
//! - **`PERF_EVENT_PASSED_PID_FILTER`**: Events matching target PID
//! - **`PERF_EVENT_OUTPUT_SUCCESS/FAILED`**: Ring buffer output stats
//!
//! ## Detection Methods
//!
//! ### Marker-Based Detection (Method 1)
//! - **Hooks**: `trace_blocking_start_hook`, `trace_blocking_end_hook`
//! - **Mechanism**: Explicit instrumentation via uprobes on marker functions
//! - **Pros**: Zero false positives, precise attribution
//! - **Cons**: Requires code modification
//!
//! ### Scheduler-Based Detection (Method 2)
//! - **Hook**: `sched_switch_hook`
//! - **Mechanism**: Detects when thread is off-CPU > threshold (5ms) in TASK_RUNNING state
//! - **Pros**: No code changes needed
//! - **Cons**: False positives from legitimate preemption
//!
//! ### Sampling-Based Detection (Method 3)
//! - **Hook**: `on_cpu_sample`
//! - **Mechanism**: 99 Hz CPU sampling captures stack traces
//! - **Pros**: Low overhead, whole-program visibility
//! - **Cons**: Statistical (may miss short events)
//!
//! ## Stack Trace Capture
//!
//! Stack traces are captured using `bpf_get_stackid()` with flags:
//! - `BPF_F_USER_STACK (0x100)`: Capture userspace stack (not kernel)
//! - `BPF_F_FAST_STACK_CMP (0x200)`: Fast comparison for deduplication
//!
//! The verifier ensures stack unwinding is safe and bounded. Stack traces
//! are stored by ID (hash) in `STACK_TRACES` map, and userspace resolves
//! addresses to function names using DWARF debug information.
//!
//! ## Safety and Verification
//!
//! eBPF programs are verified by the kernel to ensure:
//! - **Termination**: No unbounded loops (all loops have provable bounds)
//! - **Memory Safety**: No out-of-bounds access, null pointer derefs
//! - **Resource Limits**: Limited stack size (512 bytes), instruction count
//! - **Privilege**: Cannot escalate privileges or bypass security
//!
//! Programs that fail verification are rejected at load time.
//!
//! ## Compilation
//!
//! eBPF programs are compiled to BPF bytecode using:
//! - **Toolchain**: Rust nightly with `bpfel-unknown-none` target
//! - **Build Command**: `cargo xtask build-ebpf` (always release mode)
//! - **Output**: `target/bpfel-unknown-none/release/hud` (bytecode)
//! - **Loaded by**: `Ebpf::load()` in userspace (hud/src/main.rs)
//!
//! **Note**: Always build in release mode because debug builds include
//! formatting code (`LowerHex`) that's incompatible with BPF linker.
//! Release mode uses LTO to eliminate dead code.

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
    ExecutionSpan, SchedSwitchArgs, TaskEvent, ThreadState, WorkerInfo, EVENT_BLOCKING_END,
    EVENT_BLOCKING_START, EVENT_SCHEDULER_DETECTED, TRACE_EXECUTION_END, TRACE_EXECUTION_START,
};

// ============================================================================
// eBPF Maps - Shared data structures between kernel and userspace
// ============================================================================

/// Ring buffer for sending events to userspace (lock-free, high-throughput)
///
/// - **Size**: 256KB (configurable)
/// - **Type**: LIFO ring buffer (overwrite oldest on overflow)
/// - **Usage**: Kernel writes with `EVENTS.output()`, userspace reads with `ring_buf.next()`
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0); // 256KB buffer

/// Stack trace map for storing deduplicated stack traces by ID
///
/// - **Max Entries**: 1024 unique stacks
/// - **Key**: Stack hash (computed by kernel)
/// - **Value**: Array of instruction pointers (addresses)
/// - **Usage**: Kernel captures with `get_stackid()`, userspace resolves with DWARF
#[map]
static STACK_TRACES: StackTrace = StackTrace::with_max_entries(1024, 0);

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

/// Map: Thread ID (TID) → Current execution span
///
/// Tracks what each worker is currently executing for timeline visualization.
/// Spans are created on thread ON-CPU (sched_switch) and completed on OFF-CPU.
/// - **start_time_ns**: When execution started
/// - **stack_id**: Stack trace ID at execution start
/// - **cpu_id**: CPU where execution is happening
#[map]
static EXECUTION_SPANS: HashMap<u32, ExecutionSpan> = HashMap::with_max_entries(256, 0);

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
    let pid = (pid_tgid >> 32) as u32; // Process ID (TGID)
    let tid = pid_tgid as u32; // Thread ID (PID)
    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture stack trace - use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200)
    let stack_id = unsafe { STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1) };

    // Look up current task ID for this thread
    let task_id = unsafe { THREAD_TASK_MAP.get(&tid).copied().unwrap_or(0) };

    // Create event
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: EVENT_BLOCKING_START,
        stack_id,
        duration_ns: 0, // Will be calculated in userspace
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0, // Not applicable for marker detection
        task_id,
        category: 0,         // 0 = general
        detection_method: 1, // 1 = marker-based
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
    let pid = (pid_tgid >> 32) as u32; // Process ID (TGID)
    let tid = pid_tgid as u32; // Thread ID (PID)
    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture stack trace - use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200)
    let stack_id = unsafe { STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1) };

    // Look up current task ID for this thread
    let task_id = unsafe { THREAD_TASK_MAP.get(&tid).copied().unwrap_or(0) };

    // Create event
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: EVENT_BLOCKING_END,
        stack_id,
        duration_ns: 0, // Will be calculated in userspace
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0, // Not applicable for marker detection
        task_id,
        category: 0,         // 0 = general
        detection_method: 1, // 1 = marker-based
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
        THREAD_TASK_MAP.insert(&tid, &task_id, 0).map_err(|_| 1i64)?;
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
        let span = unsafe { EXECUTION_SPANS.get(&tid).copied() };

        if let Some(span) = span {
            // Calculate execution duration
            let duration_ns = now - span.start_time_ns;

            // Emit TRACE_EXECUTION_END event
            emit_execution_end(tid, now, duration_ns, span.stack_id)?;

            // Clear execution span
            unsafe {
                EXECUTION_SPANS.remove(&tid)?;
            }
        }
    }

    // Update thread state (for legacy scheduler-based detection)
    let mut thread_state = unsafe { THREAD_STATE.get(&tid).copied().unwrap_or_default() };

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
    let stack_id = unsafe { STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1) };
    let cpu_id = get_cpu_id();

    // Create execution span
    let span = ExecutionSpan { start_time_ns: now, stack_id, cpu_id, _padding: [0; 4] };

    unsafe {
        EXECUTION_SPANS.insert(&tid, &span, 0)?;
    }

    // Emit TRACE_EXECUTION_START event
    emit_execution_start(tid, now, stack_id)?;

    // Get thread state (for legacy scheduler-based detection)
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

            let stack_id = unsafe { STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1) };

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
        category: 0,         // 0 = general
        detection_method: 2, // 2 = scheduler-based
        is_tokio_worker: 1,  // Only workers trigger scheduler detection
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

    let task_id = unsafe { THREAD_TASK_MAP.get(&tid).copied().unwrap_or(0) };

    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: TRACE_EXECUTION_START,
        stack_id,
        duration_ns: 0, // Not applicable for start event
        worker_id: get_worker_id(tid),
        cpu_id: get_cpu_id(),
        thread_state: 0,
        task_id,
        category: 0,         // 0 = general
        detection_method: 3, // 3 = trace
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

    let task_id = unsafe { THREAD_TASK_MAP.get(&tid).copied().unwrap_or(0) };

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
        category: 0,         // 0 = general
        detection_method: 3, // 3 = trace
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
    unsafe { TOKIO_WORKER_THREADS.get(&tid).map(|info| info.worker_id).unwrap_or(u32::MAX) }
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

    // DEBUG: Temporarily disable Tokio worker filter to test if perf_event fires
    // Once we confirm perf_event works, we'll re-enable this filter
    // if !is_tokio_worker(tid) {
    //     return Ok(());
    // }

    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Capture stack trace - perf_event context has pt_regs, so this should work!
    // Use BPF_F_USER_STACK (0x100) | BPF_F_FAST_STACK_CMP (0x200)
    let stack_id = unsafe { STACK_TRACES.get_stackid(ctx, 0x300).unwrap_or(-1) };

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
        detection_method: 4, // 4 = perf_event sampling
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
