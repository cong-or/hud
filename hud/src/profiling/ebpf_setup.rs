//! # eBPF Program Loading and Attachment
//!
//! Loads compiled eBPF bytecode and attaches programs to kernel hook points.
//!
//! ## Functions
//!
//! - [`load_ebpf_program()`] - Load eBPF bytecode from embedded binary
//! - [`attach_task_id_uprobe()`] - Attach uprobe for task ID tracking
//! - [`register_tokio_workers()`] - Discover and register Tokio worker threads
//! - [`setup_scheduler_detection()`] - Attach tracepoints and perf events
//!
//! ## Attachment Points
//!
//! - **Uprobe**: `set_current_task_id()` (Tokio task tracking)
//! - **Tracepoint**: `sched/sched_switch` (context switches)
//! - **Perf Event**: CPU sampling at 99 Hz
//!
//! See [Architecture docs](../../docs/ARCHITECTURE.md) for details on eBPF attachment.

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

/// Attach task ID tracking uprobe (`set_current_task_id`)
/// Returns true if task ID tracking is available
///
/// # Errors
/// Returns an error if uprobe attachment fails
pub fn attach_task_id_uprobe(bpf: &mut Ebpf, target_path: &str, pid: Option<i32>) -> Result<bool> {
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

/// Setup scheduler-based blocking detection
/// Returns the number of worker threads registered
///
/// # Errors
/// Returns an error if eBPF map access, tracepoint attachment, or perf event setup fails
#[allow(clippy::cast_sign_loss)]
pub fn setup_scheduler_detection(bpf: &mut Ebpf, pid: i32) -> Result<usize> {
    println!("\n🔧 Setting up scheduler-based detection...");

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
