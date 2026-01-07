//! # eBPF Program Loading and Attachment
//!
//! This module handles the initialization and setup of eBPF programs for
//! Runtime Scope profiling. It provides functions to:
//! - Load compiled eBPF bytecode into the kernel
//! - Attach uprobes to userspace functions
//! - Register Tokio worker threads for filtering
//! - Setup scheduler-based detection via tracepoints and perf events
//!
//! ## eBPF Attachment Overview
//!
//! eBPF programs must be **attached** to specific hook points in the kernel:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    User Application                         │
//! │                                                             │
//! │  trace_blocking_start()  ◄─── Uprobe (marker-based)       │
//! │  trace_blocking_end()    ◄─── Uprobe (marker-based)       │
//! │  set_current_task_id()   ◄─── Uprobe (task tracking)      │
//! └─────────────────────────────────────────────────────────────┘
//!                        │
//!                        │ context switch
//!                        ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Linux Kernel                             │
//! │                                                             │
//! │  sched/sched_switch      ◄─── Tracepoint (scheduler)       │
//! │  perf_event (99 Hz)      ◄─── Perf Event (sampling)        │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Uprobes (Userspace Probes)
//!
//! **Uprobes** are dynamic instrumentation points for userspace functions.
//! They work by:
//! 1. Parsing the target binary's symbol table (ELF)
//! 2. Setting a **breakpoint** (`int3` on x86) at the function's entry
//! 3. When the breakpoint hits, kernel traps and calls the eBPF program
//! 4. eBPF program executes, then returns control to the original function
//!
//! **Performance**: Each uprobe adds ~1-2μs overhead per invocation. This is
//! acceptable for infrequent operations (like blocking markers) but would be
//! prohibitive for hot paths (like all function calls).
//!
//! ### Marker-Based Detection Uprobes
//!
//! Runtime Scope attaches uprobes to two marker functions:
//! - `trace_blocking_start()`: Called before blocking operations
//! - `trace_blocking_end()`: Called after blocking operations
//!
//! **Application code**:
//! ```rust,ignore
//! #[no_mangle]
//! extern "C" fn trace_blocking_start() {}
//!
//! fn expensive_operation() {
//!     trace_blocking_start();
//!     // ... synchronous work ...
//!     trace_blocking_end();
//! }
//! ```
//!
//! **Symbol Resolution**: The uprobe attachment requires:
//! - Symbol must be **exported** (`#[no_mangle]`, `extern "C"`)
//! - Symbol name must be in the binary's symbol table (`nm <binary>`)
//! - Symbol must not be inlined (disable LTO for markers)
//!
//! ### Task ID Tracking Uprobe
//!
//! Tokio internally calls `set_current_task_id()` when switching tasks on a
//! worker thread. We attach a uprobe to this function to track which async
//! task is currently executing on each thread.
//!
//! **Challenge**: This function is often **inlined** in release builds, making
//! it impossible to attach a uprobe. Runtime Scope handles this gracefully by
//! detecting the attachment failure and continuing without task IDs.
//!
//! **Mangled Symbol**: Rust mangles function names:
//! ```text
//! tokio::runtime::context::set_current_task_id
//!   → _ZN5tokio7runtime7context19set_current_task_id17h88510a52941c215fE
//! ```
//!
//! ## Worker Thread Discovery
//!
//! Before attaching probes, we must identify which threads are Tokio workers.
//! This is done by inspecting `/proc/<pid>/task/<tid>/comm`:
//!
//! ```text
//! /proc/12345/task/
//!   ├── 12345/comm → "main"
//!   ├── 12346/comm → "tokio-runtime-w"  ✓ Worker thread 0
//!   ├── 12347/comm → "tokio-runtime-w"  ✓ Worker thread 1
//!   └── 12348/comm → "blocking-1"       ✗ Blocking thread pool
//! ```
//!
//! Workers are registered in the `TOKIO_WORKER_THREADS` eBPF map, which the
//! kernel-side eBPF programs use to filter events to only Tokio workers.
//!
//! ## Scheduler-Based Detection Setup
//!
//! Scheduler-based detection uses two kernel mechanisms:
//!
//! ### 1. Tracepoint: `sched/sched_switch`
//!
//! **What it is**: Stable kernel API for tracing scheduler events.
//! Fires every time the Linux scheduler switches threads (context switch).
//!
//! **Why we use it**: Detect when Tokio workers go ON/OFF CPU:
//! - **ON-CPU** (thread becomes `next_pid`): Start execution span
//! - **OFF-CPU** (thread becomes `prev_pid`): End execution span, check threshold
//!
//! **Overhead**: High - fires on every context switch (thousands per second).
//! Mitigated by filtering to only Tokio worker threads in eBPF.
//!
//! ### 2. Perf Event: CPU Sampling at 99 Hz
//!
//! **What it is**: Periodic timer-based sampling for CPU profiling.
//! Fires 99 times per second on each CPU core.
//!
//! **Why we use it**: Capture stack traces of what's currently executing
//! for statistical flame graph analysis.
//!
//! **Why 99 Hz**: Prime number to avoid aliasing with other periodic events
//! (like 100 Hz system timer). Standard for CPU profiling.
//!
//! **Filtering**: Perf events fire on **all processes**, so we filter by PID
//! in the eBPF program using the `CONFIG` map.
//!
//! ## Setup Sequence
//!
//! The typical initialization sequence in `main.rs`:
//!
//! ```rust,ignore
//! // 1. Load eBPF bytecode from embedded binary
//! let mut bpf = load_ebpf_program()?;
//!
//! // 2. Initialize eBPF logging (optional)
//! init_ebpf_logger(&mut bpf);
//!
//! // 3. Attach marker-based detection uprobes
//! let task_id_attached = attach_blocking_uprobes(&mut bpf, &target_path, Some(pid))?;
//!
//! // 4. Setup scheduler-based detection (tracepoints + perf events)
//! let worker_count = setup_scheduler_detection(&mut bpf, pid)?;
//!
//! // 5. Get ring buffer and start event processing
//! let ring_buf = RingBuf::try_from(bpf.take_map("EVENTS")?)?;
//! ```
//!
//! ## Configuration via eBPF Maps
//!
//! Configuration is passed to eBPF programs via the `CONFIG` map:
//! - **Key 0**: Blocking threshold in nanoseconds (default: 5,000,000 = 5ms)
//! - **Key 1**: Target PID for perf_event filtering
//!
//! This allows runtime configuration without recompiling the eBPF program.

use anyhow::{Context, Result};
use aya::{
    include_bytes_aligned,
    maps::HashMap,
    programs::{perf_event, PerfEvent, TracePoint, UProbe},
    Ebpf,
};
use aya_log::EbpfLogger;
use hud_common::WorkerInfo;
use log::{info, warn};

use crate::domain::Pid;
use crate::profiling::{identify_tokio_workers, online_cpus};

/// Load the eBPF program binary
///
/// Always uses the release build because debug builds with recent Rust nightlies (1.94+)
/// pull in formatting code (`LowerHex`) that's incompatible with BPF. The release build
/// uses LTO to eliminate dead code. eBPF programs are small and compile fast in release.
///
/// # Errors
/// Returns an error if the eBPF program binary cannot be loaded
pub fn load_ebpf_program() -> Result<Ebpf> {
    let bpf = Ebpf::load(include_bytes_aligned!("../../../target/bpfel-unknown-none/release/hud"))?;
    Ok(bpf)
}

/// Initialize eBPF logger
pub fn init_ebpf_logger(bpf: &mut Ebpf) {
    if let Err(e) = EbpfLogger::init(bpf) {
        warn!("Failed to initialize eBPF logger: {e}");
    }
}

/// Attach blocking marker uprobes (`trace_blocking_start`, `trace_blocking_end`, `set_task_id`)
/// Returns true if `task_id` tracking is available
///
/// # Errors
/// Returns an error if uprobe attachment fails
pub fn attach_blocking_uprobes(
    bpf: &mut Ebpf,
    target_path: &str,
    pid: Option<i32>,
) -> Result<bool> {
    // Attach uprobe to trace_blocking_start
    let program: &mut UProbe =
        bpf.program_mut("trace_blocking_start_hook").context("program not found")?.try_into()?;
    program.load()?;
    program.attach(Some("trace_blocking_start"), 0, target_path, pid)?;
    info!("✓ Attached uprobe: trace_blocking_start");

    // Attach uprobe to trace_blocking_end
    let program: &mut UProbe =
        bpf.program_mut("trace_blocking_end_hook").context("program not found")?.try_into()?;
    program.load()?;
    program.attach(Some("trace_blocking_end"), 0, target_path, pid)?;
    info!("✓ Attached uprobe: trace_blocking_end");

    // Attach uprobe to tokio::runtime::context::set_current_task_id
    // Note: This symbol may not exist in release builds (gets inlined)
    let task_id_attached = if let Some(program) = bpf.program_mut("set_task_id_hook") {
        match program.try_into() {
            Ok(program) => {
                let program: &mut UProbe = program;
                if let Err(e) = program.load() {
                    warn!("⚠️  Failed to load set_task_id_hook: {e}");
                    false
                } else {
                    match program.attach(
                        Some("_ZN5tokio7runtime7context19set_current_task_id17h88510a52941c215fE"),
                        0,
                        target_path,
                        pid,
                    ) {
                        Ok(_) => {
                            info!("✓ Attached uprobe: set_current_task_id");
                            true
                        }
                        Err(e) => {
                            warn!("⚠️  Could not attach set_task_id_hook: {e}");
                            warn!("   Task ID tracking unavailable (symbol likely inlined in release build)");
                            false
                        }
                    }
                }
            }
            Err(e) => {
                warn!("⚠️  Failed to convert set_task_id_hook: {e}");
                false
            }
        }
    } else {
        warn!("⚠️  set_task_id_hook program not found");
        false
    };

    Ok(task_id_attached)
}

/// Register Tokio worker threads in the `TOKIO_WORKER_THREADS` eBPF map
///
/// # Errors
/// Returns an error if worker discovery or eBPF map access fails
#[allow(clippy::cast_sign_loss)]
pub fn register_tokio_workers(bpf: &mut Ebpf, pid: i32) -> Result<usize> {
    let workers = identify_tokio_workers(Pid(pid))?;

    if workers.is_empty() {
        warn!("No Tokio worker threads found! Make sure the target is a Tokio app.");
        return Ok(0);
    }

    let mut map: HashMap<_, u32, WorkerInfo> = HashMap::try_from(
        bpf.map_mut("TOKIO_WORKER_THREADS").context("TOKIO_WORKER_THREADS map not found")?,
    )?;

    for worker in &workers {
        let mut comm = [0u8; 16];
        let bytes = worker.comm.as_bytes();
        let copy_len = bytes.len().min(16);
        comm[..copy_len].copy_from_slice(&bytes[..copy_len]);

        let info = WorkerInfo {
            worker_id: worker.worker_id,
            pid: pid as u32,
            comm,
            is_active: 1,
            _padding: [0u8; 3],
        };

        map.insert(worker.tid.0, info, 0)?;
    }

    info!("✓ Registered {} Tokio worker threads", workers.len());
    Ok(workers.len())
}

/// Setup scheduler-based blocking detection (Phase 3a)
/// Returns the number of worker threads registered
///
/// # Errors
/// Returns an error if eBPF map access, tracepoint attachment, or perf event setup fails
#[allow(clippy::cast_sign_loss)]
pub fn setup_scheduler_detection(bpf: &mut Ebpf, pid: i32) -> Result<usize> {
    println!("\n🔧 Phase 3a: Setting up scheduler-based detection...");

    // 1. Set configuration (5ms threshold and target PID)
    let mut config_map: HashMap<_, u32, u64> =
        HashMap::try_from(bpf.map_mut("CONFIG").context("CONFIG map not found")?)?;
    config_map.insert(0, 5_000_000, 0)?; // 5ms threshold in nanoseconds
    config_map.insert(1, pid as u64, 0)?; // target PID for perf_event filtering
    info!("✓ Set blocking threshold: 5ms");
    info!("✓ Set target PID: {pid}");

    // 2. Identify and register Tokio worker threads
    let worker_count = register_tokio_workers(bpf, pid)?;

    // 3. Attach sched_switch tracepoint
    let program: &mut TracePoint = bpf
        .program_mut("sched_switch_hook")
        .context("sched_switch_hook program not found")?
        .try_into()?;
    program.load()?;
    program.attach("sched", "sched_switch")?;
    info!("✓ Attached tracepoint: sched/sched_switch");

    // 4. Attach CPU sampling perf_event for stack traces
    let program: &mut PerfEvent =
        bpf.program_mut("on_cpu_sample").context("on_cpu_sample program not found")?.try_into()?;
    program.load()?;

    // Attach perf_event sampler at 99 Hz on all CPUs
    let cpus = online_cpus()?;
    info!(
        "Attaching perf_event sampler to {} CPUs at 99 Hz (filtering for PID {})",
        cpus.len(),
        pid
    );
    for cpu in &cpus {
        program.attach(
            perf_event::PerfTypeId::Software,
            perf_event::perf_sw_ids::PERF_COUNT_SW_CPU_CLOCK as u64,
            perf_event::PerfEventScope::AllProcessesOneCpu { cpu: cpu.0 },
            perf_event::SamplePolicy::Frequency(99),
            false,
        )?;
    }
    info!(
        "✓ Attached perf_event sampler to {} CPUs at 99 Hz (filtering for PID {})",
        cpus.len(),
        pid
    );

    println!("✅ Scheduler-based detection active");
    println!("   Monitoring {worker_count} Tokio worker threads");
    println!("   CPU sampling: 99 Hz (every ~10ms)\n");

    Ok(worker_count)
}
