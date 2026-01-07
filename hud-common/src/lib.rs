//! # Shared Data Structures (eBPF ↔ Userspace)
//!
//! Defines data structures and constants shared between kernel-side eBPF programs
//! and userspace. All types use `#[repr(C)]` for consistent memory layout across
//! the kernel/userspace boundary.
//!
//! ## Detection Methods
//!
//! Two complementary approaches (see [Architecture](../../docs/ARCHITECTURE.md) for details):
//!
//! 1. **Scheduler-Based** - Monitor `sched_switch` for off-CPU > 5ms (no code changes)
//! 2. **Sampling-Based** - CPU sampling at 99 Hz (statistical profiling)
//!
//! ## Key Types
//!
//! - [`TaskEvent`] - Core event structure passed via ring buffer
//! - [`WorkerInfo`] - Tokio worker thread metadata
//! - [`ThreadState`] - Execution state for scheduler-based detection
//! - [`SchedSwitchArgs`] - Tracepoint arguments from `sched_switch`

#![no_std]

// ============================================================================
// Event Type Constants
// ============================================================================

/// **Scheduler-Based Detection**: Blocking detected via off-CPU threshold
///
/// Emitted by: `sched_switch_hook` tracepoint when OFF-CPU > 5ms
/// Detection Method: 2 (scheduler)
pub const EVENT_SCHEDULER_DETECTED: u32 = 3;

/// **Timeline Visualization**: Worker thread started executing
///
/// Emitted by: `sched_switch_hook` when Tokio worker goes ON-CPU
/// Paired with: `TRACE_EXECUTION_END`
/// Detection Method: 3 (trace)
pub const TRACE_EXECUTION_START: u32 = 10;

/// **Timeline Visualization**: Worker thread stopped executing
///
/// Emitted by: `sched_switch_hook` when Tokio worker goes OFF-CPU
/// Paired with: `TRACE_EXECUTION_START`
/// Detection Method: 3 (trace)
pub const TRACE_EXECUTION_END: u32 = 11;

/// **Sampling Profiler**: Periodic stack sample (unused, for future use)
///
/// Emitted by: `on_cpu_sample` `perf_event` hook
/// Detection Method: 4 (sampling)
pub const TRACE_FUNCTION_SAMPLE: u32 = 12;

/// **Worker Metadata**: Worker thread information event (unused, for future use)
///
/// Emitted by: Userspace during worker discovery
pub const TRACE_WORKER_METADATA: u32 = 13;

/// Maximum number of stack frames to capture
///
/// Kernel eBPF programs are limited to 127 frames due to verifier constraints.
/// Exceeding this limit will cause the eBPF program to fail verification.
pub const MAX_STACK_DEPTH: usize = 127;

// ============================================================================
// Shared Data Structures
// ============================================================================

/// Event sent from eBPF to userspace via ring buffer
///
/// This is the core event structure used for all communication between
/// kernel-side eBPF programs and userspace. Events are written to the
/// `EVENTS` ring buffer by eBPF and read by userspace in the main loop.
///
/// **Memory Layout**: `#[repr(C)]` ensures consistent layout across kernel/userspace
/// **Size**: Must be small to minimize ring buffer overhead (~72 bytes)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskEvent {
    // ========================================================================
    // Core Identification
    // ========================================================================
    /// Process ID (TGID in Linux terms)
    ///
    /// Identifies which process this event belongs to.
    /// Used to filter events when profiling multi-process applications.
    pub pid: u32,

    /// Thread ID (PID in Linux terms, TID in userspace)
    ///
    /// Identifies which thread within the process emitted this event.
    /// For Tokio workers: thread name is "tokio-runtime-w{N}"
    pub tid: u32,

    /// Timestamp in nanoseconds (from `bpf_ktime_get_ns()`)
    ///
    /// Monotonic clock, relative to system boot (not wall-clock time).
    /// Used for duration calculation and event ordering.
    pub timestamp_ns: u64,

    /// Event type (see constants: `EVENT_BLOCKING_START`, `EVENT_SCHEDULER_DETECTED`, etc.)
    pub event_type: u32,

    // ========================================================================
    // Stack Trace
    // ========================================================================
    /// Stack trace ID (from `STACK_TRACES` eBPF map)
    ///
    /// **Value**:
    /// - Positive: Valid stack trace ID (userspace looks up in `STACK_TRACES` map)
    /// - Negative: Failed to capture stack trace (e.g., stack unwinding error)
    ///
    /// Stack traces are captured with `bpf_get_stackid()` and deduplicated by
    /// the kernel (identical stacks share the same ID).
    pub stack_id: i64,

    // ========================================================================
    // Duration and Timing
    // ========================================================================
    /// Duration in nanoseconds (for span/end events)
    ///
    /// **Usage**:
    /// - `EVENT_BLOCKING_END`: Duration of blocking operation (calculated in userspace)
    /// - `EVENT_SCHEDULER_DETECTED`: Off-CPU duration that exceeded threshold
    /// - `TRACE_EXECUTION_END`: Execution duration (time on-CPU)
    /// - `TRACE_EXECUTION_START`: Always 0 (start events have no duration)
    pub duration_ns: u64,

    // ========================================================================
    // Worker Context (for timeline visualization)
    // ========================================================================
    /// Tokio worker ID (0-based index)
    ///
    /// **Value**:
    /// - `0..N`: Valid worker ID (corresponds to "tokio-runtime-w{N}")
    /// - `u32::MAX`: Not a Tokio worker thread (e.g., main thread, blocking pool)
    ///
    /// Used to render per-worker timelines in TUI.
    pub worker_id: u32,

    /// CPU core where event occurred (0-based)
    ///
    /// Indicates which physical/logical CPU core was executing this code.
    /// Useful for understanding CPU affinity and load distribution.
    pub cpu_id: u32,

    // ========================================================================
    // Thread State (Linux scheduler)
    // ========================================================================
    /// Linux thread state from `sched_switch` tracepoint
    ///
    /// **Values**:
    /// - `0` (`TASK_RUNNING`): Thread was preempted while running (CPU-bound)
    /// - `1` (`TASK_INTERRUPTIBLE`): Thread yielded (async await, legitimate)
    /// - `2` (`TASK_UNINTERRUPTIBLE`): Thread blocked on I/O
    ///
    /// Scheduler-based detection only reports `TASK_RUNNING` (state=0) to
    /// avoid false positives from legitimate async yields.
    pub thread_state: i64,

    // ========================================================================
    // Metadata
    // ========================================================================
    /// Tokio async task ID (0 if unknown)
    ///
    /// Captured from `tokio::runtime::context::set_current_task_id()` via uprobe.
    /// Allows attributing blocking operations to specific async tasks.
    ///
    /// **Note**: May be 0 if `set_task_id_hook` is inlined in release builds.
    pub task_id: u64,

    /// Event category (0=general, 1=database, 2=network, 3=compute)
    ///
    /// Currently always 0 (general). Reserved for future categorization.
    pub category: u8,

    /// Detection method that produced this event
    ///
    /// **Values**:
    /// - `2`: Scheduler-based (threshold detection via `sched_switch`)
    /// - `3`: Trace (timeline visualization via `sched_switch`)
    /// - `4`: Sampling (CPU sampling via `perf_event` at 99 Hz)
    pub detection_method: u8,

    /// Whether this thread is a Tokio worker (1) or not (0)
    ///
    /// Tokio workers are the threads that run async tasks. Other threads
    /// (main, blocking thread pool) are not workers.
    pub is_tokio_worker: u8,

    /// Padding for 8-byte alignment
    #[allow(clippy::pub_underscore_fields)]
    pub _padding: [u8; 5],
}

/// Thread execution state (for scheduler-based detection)
///
/// Tracks the ON/OFF CPU state of threads to detect blocking operations
/// via threshold heuristic. Stored in the `THREAD_STATE` eBPF map.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ThreadState {
    /// Timestamp when thread was last scheduled ON CPU
    ///
    /// Updated by `sched_switch` when this thread becomes `next_pid`.
    pub last_on_cpu_ns: u64,

    /// Timestamp when thread was last scheduled OFF CPU
    ///
    /// Updated by `sched_switch` when this thread becomes `prev_pid`.
    pub last_off_cpu_ns: u64,

    /// Duration thread was OFF CPU (nanoseconds)
    ///
    /// Calculated as: `(current_time - last_off_cpu_ns)`
    /// Compared against threshold (default 5ms) to detect blocking.
    pub off_cpu_duration: u64,

    /// Linux task state when switched off CPU
    ///
    /// From `sched_switch` tracepoint's `prev_state` field:
    /// - `0` (`TASK_RUNNING`): Preempted while running → potential blocking
    /// - `1` (`TASK_INTERRUPTIBLE`): Yielded (async) → NOT blocking
    /// - `2` (`TASK_UNINTERRUPTIBLE`): Blocked on I/O
    pub state_when_switched: i64,
}

/// Tokio worker thread metadata
///
/// Registered by userspace after discovering worker threads via `/proc`.
/// Stored in the `TOKIO_WORKER_THREADS` eBPF map for event filtering.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WorkerInfo {
    /// Worker thread index (0-based)
    ///
    /// Corresponds to Tokio's internal worker numbering.
    /// Thread name format: "tokio-runtime-w{worker_id}"
    pub worker_id: u32,

    /// Process ID (TGID)
    pub pid: u32,

    /// Thread name (up to 16 bytes, NUL-terminated)
    ///
    /// Example: "tokio-runtime-w" (may be truncated)
    pub comm: [u8; 16],

    /// Whether worker is currently active (1) or terminated (0)
    ///
    /// Currently always 1 (reserved for future worker lifecycle tracking).
    pub is_active: u8,

    /// Padding for 4-byte alignment
    #[allow(clippy::pub_underscore_fields)]
    pub _padding: [u8; 3],
}

/// Execution span tracking (for timeline visualization)
///
/// Tracks what each worker is currently executing. Created when worker
/// goes ON-CPU (`sched_switch` `next_pid`) and completed when goes OFF-CPU.
/// Stored in the `EXECUTION_SPANS` eBPF map.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExecutionSpan {
    /// Timestamp when execution started (nanoseconds)
    ///
    /// From `bpf_ktime_get_ns()` at ON-CPU event.
    pub start_time_ns: u64,

    /// Stack trace ID at execution start
    ///
    /// Captured via `bpf_get_stackid()` to identify what function is executing.
    pub stack_id: i64,

    /// CPU core where execution is happening
    pub cpu_id: u32,

    /// Padding for 8-byte alignment
    #[allow(clippy::pub_underscore_fields)]
    pub _padding: [u8; 4],
}

impl Default for ExecutionSpan {
    fn default() -> Self {
        Self { start_time_ns: 0, stack_id: -1, cpu_id: 0, _padding: [0; 4] }
    }
}

/// Tracepoint arguments for `sched/sched_switch`
///
/// Layout defined by the Linux kernel tracepoint ABI:
/// `/sys/kernel/debug/tracing/events/sched/sched_switch/format`
///
/// This struct is passed to eBPF programs attached to the `sched_switch`
/// tracepoint, which fires every time the Linux scheduler switches threads.
///
/// ## Usage
///
/// The scheduler-based detection method uses this tracepoint to:
/// 1. Detect when Tokio worker threads go ON/OFF CPU
/// 2. Calculate off-CPU duration for blocking detection
/// 3. Emit timeline visualization events (start/end)
///
/// ## Field Meanings
///
/// - **prev_***: The thread being switched OUT (going off-CPU)
/// - **next_***: The thread being switched IN (going on-CPU)
#[repr(C)]
pub struct SchedSwitchArgs {
    /// Unused padding (kernel tracepoint common fields)
    #[allow(clippy::pub_underscore_fields)]
    pub __unused__: u64,

    /// Command name of the thread being switched out
    ///
    /// Example: "tokio-runtime-w", "systemd", "kworker/0:1"
    pub prev_comm: [u8; 16],

    /// Thread ID (PID) of the thread being switched out
    pub prev_pid: i32,

    /// Priority of the thread being switched out
    pub prev_prio: i32,

    /// State of the thread being switched out
    ///
    /// **Critical for blocking detection**:
    /// - `0` (`TASK_RUNNING`): Thread was preempted while running (CPU-bound)
    ///   → **Potential blocking** if off-CPU time exceeds threshold
    /// - `1` (`TASK_INTERRUPTIBLE`): Thread yielded voluntarily (async await)
    ///   → **NOT blocking**, legitimate async behavior
    /// - `2` (`TASK_UNINTERRUPTIBLE`): Thread blocked on I/O
    ///
    /// We only report `TASK_RUNNING` to avoid false positives.
    pub prev_state: i64,

    /// Command name of the thread being switched in
    pub next_comm: [u8; 16],

    /// Thread ID (PID) of the thread being switched in
    pub next_pid: i32,

    /// Priority of the thread being switched in
    pub next_prio: i32,
}

#[cfg(feature = "user")]
use aya::Pod;

// These unsafe impls are required for eBPF <-> userspace communication
// Pod trait ensures types can be safely transmitted as plain bytes
#[cfg(feature = "user")]
#[allow(unsafe_code)]
unsafe impl Pod for TaskEvent {}

#[cfg(feature = "user")]
#[allow(unsafe_code)]
unsafe impl Pod for ThreadState {}

#[cfg(feature = "user")]
#[allow(unsafe_code)]
unsafe impl Pod for WorkerInfo {}

#[cfg(feature = "user")]
#[allow(unsafe_code)]
unsafe impl Pod for ExecutionSpan {}
