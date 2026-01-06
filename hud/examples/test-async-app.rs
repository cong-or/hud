//! Test async application for hud profiling
//!
//! This app demonstrates:
//! - Well-behaved async tasks (lots of awaiting)
//! - Blocking tasks (CPU-bound work without yielding)
//! - Task spawn patterns
//!
//! Run with: cargo run --example test-async-app
//! Profile with: sudo hud --pid <pid>

use std::time::Duration;
use tokio::time::sleep;

// ==============================================================================
// MARKER FUNCTIONS FOR EBPF TRACING (PHASE 3a: DUAL DETECTION MODE)
// ==============================================================================
// 🔬 PHASE 3a: Running BOTH detection methods for validation!
//
// Current Status:
//   - Marker-based detection: ✅ Active (for comparison)
//   - Scheduler-based detection: ✅ Active (under validation)
//
// Marker Detection (Phase 1-2):
//   - Uses #[no_mangle] functions + eBPF uprobes
//   - Requires explicit trace_blocking_start/end calls
//   - Ground truth for validating scheduler detection
//
// Scheduler Detection (Phase 3a - NEW!):
//   - Uses sched_switch tracepoint (kernel scheduler events)
//   - Detects blocking automatically (no markers needed!)
//   - Monitors when Tokio worker threads go off-CPU
//   - Heuristic: thread off-CPU >5ms in TASK_RUNNING state = blocking
//
// How It Works (Phase 3a):
//   1. blocking_task() calls trace_blocking_start() → 🔵 MARKER DETECTED
//   2. blocking_task() blocks CPU for 450ms
//   3. Scheduler switches thread off-CPU → 🟢 SCHEDULER DETECTED
//   4. Both events shown in output for comparison!
//
// Success Metrics:
//   - Scheduler should detect same events as markers
//   - Both methods report similar durations
//   - <10% false positives/negatives
//
// Timeline:
//   Phase 1: ✅ Basic blocking detection with markers
//   Phase 2: ✅ Stack traces + async task tracking
//   Phase 3a: 🔬 Hybrid mode (CURRENT) - both methods active
//   Phase 3b: 🎯 Scheduler-only (markers optional)
//   Phase 3c: 🚀 Production (zero instrumentation)
//
// These markers will be removed in Phase 3c!

#[no_mangle]
#[inline(never)]
fn trace_task_start(task_id: u64) {
    // Empty - just a hook point for eBPF
    std::hint::black_box(task_id);
}

#[no_mangle]
#[inline(never)]
fn trace_task_end(task_id: u64) {
    std::hint::black_box(task_id);
}

#[no_mangle]
#[inline(never)]
fn trace_blocking_start() {
    std::hint::black_box(());
}

#[no_mangle]
#[inline(never)]
fn trace_blocking_end() {
    std::hint::black_box(());
}

#[tokio::main]
async fn main() {
    println!("🚀 Test Async Application Starting");
    println!("   This app has intentional good and bad async behavior\n");

    // Spawn well-behaved tasks
    for i in 0..5 {
        tokio::spawn(well_behaved_task(i));
    }

    // Spawn ONE blocking task (the villain)
    tokio::spawn(blocking_task());

    // Spawn more well-behaved tasks
    for i in 5..10 {
        tokio::spawn(well_behaved_task(i));
    }

    // Main loop - spawn tasks periodically
    for round in 0..20 {
        println!("\n[Round {round}] Spawning burst of tasks...");

        // Spawn a burst of quick tasks
        for i in 0..10 {
            tokio::spawn(quick_task(round, i));
        }

        sleep(Duration::from_secs(2)).await;
    }

    println!("\n✓ Main loop complete, waiting for tasks to finish...");
    sleep(Duration::from_secs(5)).await;
    println!("✓ Application shutting down");
}

/// Well-behaved async task - lots of awaiting, minimal CPU work
async fn well_behaved_task(id: u32) {
    println!("  ✓ Task {id} (well-behaved) starting");

    for i in 0..50 {
        // Simulate async I/O - this yields to the executor
        sleep(Duration::from_millis(100)).await;

        // Tiny bit of CPU work (good - less than 10ms)
        let _result = (0..1000).sum::<u64>();

        if i % 10 == 0 {
            println!("  ✓ Task {id} checkpoint {i}/50");
        }
    }

    println!("  ✓ Task {id} (well-behaved) complete");
}

/// Blocking task - does CPU work without yielding (BAD!)
#[inline(never)]
async fn blocking_task() {
    println!("  ⚠️  Blocking task starting (this will cause problems!)");

    for _round in 0..10 {
        sleep(Duration::from_secs(1)).await;

        println!("  🔴 Blocking task doing heavy CPU work...");

        // eBPF trace point: blocking starts
        trace_blocking_start();

        // TEST: Heavy CPU-bound work that will get preempted by scheduler
        // This should trigger TASK_RUNNING state detection!
        // We do enough work to take ~500ms, forcing scheduler preemptions
        let mut result = 0u64;
        let start = std::time::Instant::now();

        // Do heavy computation until we've burned ~500ms of CPU time
        while start.elapsed() < Duration::from_millis(500) {
            // Heavy work: lots of iterations to force real CPU usage
            for _ in 0..100_000 {
                result = result.wrapping_add(std::hint::black_box(1));
            }
        }

        // eBPF trace point: blocking ends
        trace_blocking_end();

        println!("  🔴 Blocking task finished CPU work (result: {result})");
        println!("      ^ This blocked the executor for ~450ms!");
    }

    println!("  ⚠️  Blocking task complete");
}

/// Quick task - spawns and completes quickly
async fn quick_task(round: u32, id: u32) {
    // Small amount of async work
    sleep(Duration::from_millis(10)).await;

    // Tiny CPU work
    let _result = (0..100).sum::<u64>();

    if id == 0 {
        println!("    → Quick task batch {round} complete");
    }
}
