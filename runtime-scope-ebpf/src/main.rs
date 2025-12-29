#![no_std]
#![no_main]

use aya_ebpf::{
    macros::{map, uprobe},
    maps::{RingBuf, StackTrace},
    programs::ProbeContext,
    helpers::{bpf_ktime_get_ns, bpf_get_current_pid_tgid},
    EbpfContext,
};
use runtime_scope_common::{TaskEvent, EVENT_BLOCKING_START, EVENT_BLOCKING_END};

// Ring buffer for sending events to userspace
#[map]
static EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0); // 256KB buffer

// Stack trace map for storing stack traces
#[map]
static STACK_TRACES: StackTrace = StackTrace::with_max_entries(1024, 0);

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

    // Create event
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: EVENT_BLOCKING_START,
        stack_id,
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

    // Create event
    let event = TaskEvent {
        pid,
        tid,
        timestamp_ns,
        event_type: EVENT_BLOCKING_END,
        stack_id,
    };

    // Send to userspace via ring buffer
    EVENTS.output(&event, 0).map_err(|_| 1i64)?;

    Ok(())
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
