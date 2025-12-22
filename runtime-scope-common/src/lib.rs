#![no_std]

// Shared data structures between eBPF and userspace

/// Event sent from eBPF to userspace when a task is spawned
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TaskEvent {
    pub pid: u32,
    pub tid: u32,
    pub timestamp: u64,
    pub event_type: u32, // 0 = spawn, 1 = poll_start, 2 = poll_end
}

#[cfg(feature = "user")]
use aya::Pod;

#[cfg(feature = "user")]
unsafe impl Pod for TaskEvent {}
