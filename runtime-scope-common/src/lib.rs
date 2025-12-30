#![no_std]

// Shared data structures between eBPF and userspace

/// Event types for runtime profiling
pub const EVENT_BLOCKING_START: u32 = 1;
pub const EVENT_BLOCKING_END: u32 = 2;

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
}

#[cfg(feature = "user")]
use aya::Pod;

#[cfg(feature = "user")]
unsafe impl Pod for TaskEvent {}
