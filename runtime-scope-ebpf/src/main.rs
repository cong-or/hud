#![no_std]
#![no_main]

use aya_ebpf::{
    macros::{map, uprobe},
    maps::{HashMap, RingBuf, StackTrace},
    programs::ProbeContext,
    helpers::{bpf_ktime_get_ns, bpf_get_current_pid_tgid},
};
use runtime_scope_common::{TaskEvent, EVENT_BLOCKING_START, EVENT_BLOCKING_END};

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

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
