#![no_std]

// Shared data structures between eBPF and userspace

/// Event types for runtime profiling
///
/// Legacy events (Phase 1-2): Marker-based blocking detection
pub const EVENT_BLOCKING_START: u32 = 1;
pub const EVENT_BLOCKING_END: u32 = 2;
pub const EVENT_SCHEDULER_DETECTED: u32 = 3;

/// New events (Phase 3+): Timeline visualization
pub const TRACE_EXECUTION_START: u32 = 10; // Worker starts executing
pub const TRACE_EXECUTION_END: u32 = 11; // Worker stops executing
pub const TRACE_FUNCTION_SAMPLE: u32 = 12; // Periodic stack sample
pub const TRACE_WORKER_METADATA: u32 = 13; // Worker info event

/// Maximum number of stack frames to capture
pub const MAX_STACK_DEPTH: usize = 127;

/// Event sent from eBPF to userspace
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskEvent {
    // Core identification
    pub pid: u32,          // Process ID
    pub tid: u32,          // Thread ID
    pub timestamp_ns: u64, // Timestamp in nanoseconds
    pub event_type: u32,   // Event type (see constants above)

    // Stack trace
    pub stack_id: i64, // Stack trace ID (from StackTrace map)

    // Duration and timing
    pub duration_ns: u64, // Duration (for span events)

    // Worker context (NEW for timeline viz)
    pub worker_id: u32, // Tokio worker ID (0-23, or u32::MAX if not a worker)
    pub cpu_id: u32,    // CPU core where event occurred

    // Thread state
    pub thread_state: i64, // Linux thread state (prev_state from sched_switch)

    // Metadata
    pub task_id: u64,         // Tokio task ID (0 if unknown)
    pub category: u8,         // Category: 0=general, 1=database, 2=network, 3=compute
    pub detection_method: u8, // 1=marker, 2=scheduler, 3=trace
    pub is_tokio_worker: u8,  // 1 if Tokio worker thread, 0 otherwise

    #[allow(clippy::pub_underscore_fields)]
    pub _padding: [u8; 5], // Padding for alignment
}

/// Thread execution state (for scheduler-based detection)
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ThreadState {
    pub last_on_cpu_ns: u64,      // When thread was last scheduled on CPU
    pub last_off_cpu_ns: u64,     // When thread was last scheduled off CPU
    pub off_cpu_duration: u64,    // How long was off CPU
    pub state_when_switched: i64, // prev_state from sched_switch
}

/// Tokio worker thread metadata
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WorkerInfo {
    pub worker_id: u32, // Worker thread index (0, 1, 2, ...)
    pub pid: u32,       // Process ID
    pub comm: [u8; 16], // Thread name (e.g., "tokio-runtime-w")
    pub is_active: u8,  // 1 if currently active, 0 if terminated
    #[allow(clippy::pub_underscore_fields)]
    pub _padding: [u8; 3], // Padding for alignment
}

/// Execution span tracking (for timeline visualization)
/// Tracks when a worker starts executing and what it's executing
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExecutionSpan {
    pub start_time_ns: u64, // When execution started
    pub stack_id: i64,      // Stack trace at start
    pub cpu_id: u32,        // CPU core
    #[allow(clippy::pub_underscore_fields)]
    pub _padding: [u8; 4], // Padding for alignment
}

impl Default for ExecutionSpan {
    fn default() -> Self {
        Self { start_time_ns: 0, stack_id: -1, cpu_id: 0, _padding: [0; 4] }
    }
}

/// Tracepoint arguments for `sched_switch`
/// Layout from `/sys/kernel/debug/tracing/events/sched/sched_switch/format`
#[repr(C)]
pub struct SchedSwitchArgs {
    #[allow(clippy::pub_underscore_fields)]
    pub __unused__: u64,
    pub prev_comm: [u8; 16],
    pub prev_pid: i32,
    pub prev_prio: i32,
    pub prev_state: i64,
    pub next_comm: [u8; 16],
    pub next_pid: i32,
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
