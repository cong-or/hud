#![no_std]

// Shared data structures between eBPF and userspace

/// Event types for runtime profiling
pub const EVENT_BLOCKING_START: u32 = 1;
pub const EVENT_BLOCKING_END: u32 = 2;

/// Event sent from eBPF to userspace
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskEvent {
    pub pid: u32,           // Process ID
    pub tid: u32,           // Thread ID
    pub timestamp_ns: u64,  // Timestamp in nanoseconds
    pub event_type: u32,    // Event type (see constants above)
}

#[cfg(feature = "user")]
use aya::Pod;

#[cfg(feature = "user")]
unsafe impl Pod for TaskEvent {}
