//! Demo server for hud video demonstration
//!
//! Shows a before/after scenario:
//! - BAD: CPU-bound work blocking async workers
//! - GOOD: Work offloaded to blocking threadpool
//!
//! ## Usage
//!
//! ```bash
//! # Build and run (bad version by default)
//! cargo build --release --example demo-server
//! ./target/release/examples/demo-server
//!
//! # In another terminal: profile with hud
//! sudo ./target/release/hud --pid $(pgrep demo-server) --target ./target/release/examples/demo-server
//!
//! # In another terminal: generate load
//! hey -n 1000 -c 20 -m POST -H "Content-Type: application/json" -d '{"data":"hello"}' http://localhost:3000/process
//!
//! # To test the fix: edit this file, swap the route handler, rebuild
//! ```

use axum::{routing::post, Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    data: String,
}

#[derive(Serialize)]
struct Output {
    processed: String,
}

// BAD: CPU-bound work blocking the async runtime
// This will show up as a hotspot in hud, with workers stuck at high utilization
#[allow(dead_code)]
async fn process_bad(Json(input): Json<Input>) -> Json<Output> {
    // Simulate expensive parsing/validation - this blocks the executor!
    let mut result = input.data.clone();
    for _ in 0..50_000 {
        result = result.chars().rev().collect();
    }
    Json(Output { processed: result })
}

// GOOD: Offload CPU-bound work to the blocking threadpool
// Workers stay free to handle other requests, work happens off the async runtime
#[allow(dead_code)]
async fn process_good(Json(input): Json<Input>) -> Json<Output> {
    let result = tokio::task::spawn_blocking(move || {
        let mut result = input.data.clone();
        for _ in 0..50_000 {
            result = result.chars().rev().collect();
        }
        result
    })
    .await
    .unwrap();
    Json(Output { processed: result })
}

#[tokio::main]
async fn main() {
    // Toggle between process_bad and process_good to demonstrate the fix:
    let app = Router::new().route("/process", post(process_bad));
    // let app = Router::new().route("/process", post(process_good));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Demo server listening on http://localhost:3000");
    println!("POST /process with JSON body: {{\"data\": \"hello\"}}");
    println!();
    println!("Generate load with:");
    println!("  hey -n 1000 -c 20 -m POST -H \"Content-Type: application/json\" -d '{{\"data\":\"hello\"}}' http://localhost:3000/process");
    axum::serve(listener, app).await.unwrap();
}
