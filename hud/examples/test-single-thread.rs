//! Single-threaded test app - Shows WORST CASE blocking
//!
//! This uses Tokio's single-threaded runtime, so blocking
//! completely freezes ALL tasks.
//!
//! Compare this to test-async-app (multi-threaded) to see
//! the difference work stealing makes.
//!
//! Run with: cargo run --example test-single-thread

use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")] // ← Single thread!
async fn main() {
    println!("🚀 Single-Threaded Test App");
    println!("   WARNING: Blocking will freeze EVERYTHING\n");

    // Spawn well-behaved tasks
    for i in 0..3 {
        tokio::spawn(well_behaved_task(i));
    }

    // The villain - blocks the ONLY thread
    tokio::spawn(blocking_task());

    // More well-behaved tasks
    for i in 3..6 {
        tokio::spawn(well_behaved_task(i));
    }

    // Main loop
    for round in 0..10 {
        println!("\n[Round {round}] Spawning tasks...");

        for i in 0..5 {
            tokio::spawn(quick_task(round, i));
        }

        sleep(Duration::from_secs(2)).await;
    }

    println!("\n✓ Main loop complete");
    sleep(Duration::from_secs(2)).await;
}

async fn well_behaved_task(id: u32) {
    println!("  ✓ Task {id} starting");

    for i in 0..20 {
        sleep(Duration::from_millis(100)).await;

        if i % 5 == 0 {
            println!("  ✓ Task {id} checkpoint {i}/20");
        }
    }

    println!("  ✓ Task {id} complete");
}

async fn blocking_task() {
    println!("  ⚠️  Blocking task starting");

    for round in 0..5 {
        sleep(Duration::from_secs(1)).await;

        println!("\n  🔴 BLOCKING for 450ms (EVERYTHING will freeze!)");

        let start = std::time::Instant::now();
        let mut result = 0u64;

        while start.elapsed() < Duration::from_millis(450) {
            result = result.wrapping_add((0..10000).sum::<u64>());
        }

        println!("  🔴 Blocking complete (result: {result})");
        println!("      ^ ALL tasks were frozen for 450ms!");
    }

    println!("  ⚠️  Blocking task done");
}

async fn quick_task(round: u32, id: u32) {
    sleep(Duration::from_millis(10)).await;
    if id == 0 {
        println!("    → Batch {round} quick tasks done");
    }
}
