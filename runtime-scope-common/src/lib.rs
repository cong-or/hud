#![no_std]

// Shared data structures between eBPF and userspace

/// Event types for runtime profiling
pub const EVENT_BLOCKING_START: u32 = 1;
pub const EVENT_BLOCKING_END: u32 = 2;
pub const EVENT_SCHEDULER_DETECTED: u32 = 3;

/// Maximum number of stack frames to capture
pub const MAX_STACK_DEPTH: usize = 127;

/// Event sent from eBPF to userspace
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskEvent {
    pub pid: u32,           // Process ID
    pub tid: u32,           // Thread ID
    pub timestamp_ns: u64,  // Timestamp in nanoseconds
    pub event_type: u32,    // Event type (see constants above)
    pub stack_id: i64,      // Stack trace ID (from StackTrace map)
    pub task_id: u64,       // Tokio task ID (0 if unknown)
    pub duration_ns: u64,   // Duration of blocking (for scheduler detection)
    pub thread_state: i64,  // Linux thread state (prev_state from sched_switch)
    pub detection_method: u8, // 1=marker, 2=scheduler
    pub _padding: [u8; 7],  // Padding for alignment
}

/// Thread execution state (for scheduler-based detection)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThreadState {
    pub last_on_cpu_ns: u64,      // When thread was last scheduled on CPU
    pub last_off_cpu_ns: u64,     // When thread was last scheduled off CPU
    pub off_cpu_duration: u64,    // How long was off CPU
    pub state_when_switched: i64, // prev_state from sched_switch
}

impl Default for ThreadState {
    fn default() -> Self {
        Self {
            last_on_cpu_ns: 0,
            last_off_cpu_ns: 0,
            off_cpu_duration: 0,
            state_when_switched: 0,
        }
    }
}

/// Tokio worker thread metadata
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WorkerInfo {
    pub worker_id: u32,  // Worker thread index (0, 1, 2, ...)
    pub pid: u32,        // Process ID
    pub comm: [u8; 16],  // Thread name (e.g., "tokio-runtime-w")
    pub is_active: u8,   // 1 if currently active, 0 if terminated
    pub _padding: [u8; 3], // Padding for alignment
}

/// Tracepoint arguments for sched_switch
/// Layout from /sys/kernel/debug/tracing/events/sched/sched_switch/format
#[repr(C)]
pub struct SchedSwitchArgs {
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

#[cfg(feature = "user")]
unsafe impl Pod for TaskEvent {}

#[cfg(feature = "user")]
unsafe impl Pod for ThreadState {}

#[cfg(feature = "user")]
unsafe impl Pod for WorkerInfo {}
