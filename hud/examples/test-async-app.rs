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

        // Heavy CPU-bound work that will get preempted by scheduler
        // This triggers TASK_RUNNING state detection via sched_switch
        let mut result = 0u64;
        let start = std::time::Instant::now();

        // Do heavy computation until we've burned ~500ms of CPU time
        while start.elapsed() < Duration::from_millis(500) {
            // Heavy work: lots of iterations to force real CPU usage
            for _ in 0..100_000 {
                result = result.wrapping_add(std::hint::black_box(1));
            }
        }

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
