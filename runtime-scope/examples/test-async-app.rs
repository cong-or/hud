//! Test async application for runtime-scope profiling
//!
//! This app demonstrates:
//! - Well-behaved async tasks (lots of awaiting)
//! - Blocking tasks (CPU-bound work without yielding)
//! - Task spawn patterns
//!
//! Run with: cargo run --example test-async-app
//! Profile with: sudo runtime-scope --pid <pid>

use std::time::Duration;
use tokio::time::sleep;

// ==============================================================================
// MARKER FUNCTIONS FOR EBPF TRACING (TEMPORARY - PHASE 1 ONLY)
// ==============================================================================
// ⚠️ THESE ARE TRAINING WHEELS - WILL BE REMOVED IN PHASE 3!
//
// Current approach (Phase 1-2):
//   - Use #[no_mangle] marker functions
//   - Easy to learn eBPF uprobes
//   - Requires code modifications (not practical for users)
//
// Production approach (Phase 3+):
//   - Use scheduler tracepoints (sched_switch, sched_wakeup)
//   - NO code changes required
//   - Works on ALL code (including inlined functions)
//   - Profile any binary without modification
//
// Why we started with markers:
//   - Easier to understand for learning
//   - Get the basic pipeline working first
//   - Then switch to production approach
//
// Timeline:
//   Phase 1: ✅ Basic blocking detection with markers
//   Phase 2: 🚧 Add stack traces (still with markers)
//   Phase 3: 🎯 Remove markers, switch to scheduler tracepoints
//   Phase 4: 🚀 Production-ready profiler (zero instrumentation)
//
// #[no_mangle] prevents Rust from mangling the symbol names, making them
// easy to find with eBPF uprobes.

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
        println!("\n[Round {}] Spawning burst of tasks...", round);

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
    println!("  ✓ Task {} (well-behaved) starting", id);

    for i in 0..50 {
        // Simulate async I/O - this yields to the executor
        sleep(Duration::from_millis(100)).await;

        // Tiny bit of CPU work (good - less than 10ms)
        let _result = (0..1000).sum::<u64>();

        if i % 10 == 0 {
            println!("  ✓ Task {} checkpoint {}/50", id, i);
        }
    }

    println!("  ✓ Task {} (well-behaved) complete", id);
}

/// Blocking task - does CPU work without yielding (BAD!)
#[inline(never)]
async fn blocking_task() {
    println!("  ⚠️  Blocking task starting (this will cause problems!)");

    for _round in 0..10 {
        sleep(Duration::from_secs(1)).await;

        println!("  🔴 Blocking task doing CPU work (450ms without yielding)...");

        // eBPF trace point: blocking starts
        trace_blocking_start();

        // Simulate blocking CPU work - NO await, just pure computation
        // This will block the executor thread for ~450ms
        let start = std::time::Instant::now();
        let mut result = 0u64;

        // Busy loop for about 450ms
        while start.elapsed() < Duration::from_millis(450) {
            result = result.wrapping_add((0..10000).sum::<u64>());
        }

        // eBPF trace point: blocking ends
        trace_blocking_end();

        println!("  🔴 Blocking task finished CPU work (result: {})", result);
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
        println!("    → Quick task batch {} complete", round);
    }
}
