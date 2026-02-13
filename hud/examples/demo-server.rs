//! Realistic demo server showing common async blocking mistakes
//!
//! Each endpoint demonstrates a different way developers accidentally
//! block the Tokio runtime. Use hud to identify these hotspots.
//!
//! ## Endpoints
//!
//! | Endpoint | Problem | Real-world example |
//! |----------|---------|-------------------|
//! | POST /hash | bcrypt in async | Password hashing |
//! | POST /parse | Large JSON parse | API request handling |
//! | POST /compress | Sync compression | Response compression |
//! | GET /read | `std::fs` blocking | Config file loading |
//! | GET /dns | Sync DNS lookup | Service discovery |
//!
//! ## Usage
//!
//! ```bash
//! # Terminal 1: Build and run server (use debug build for better stack traces)
//! cargo build --example demo-server
//! ./target/debug/examples/demo-server
//!
//! # Terminal 2: Profile with hud
//! sudo ./target/release/hud --pid $(pgrep demo-server) \
//!     --target ./target/debug/examples/demo-server
//!
//! # Terminal 3: Generate load
//! ./hud/examples/load.sh
//! ```
//!
//! ## Why Debug Build?
//!
//! Release builds aggressively inline functions, which can hide your code in
//! stack traces. Debug builds preserve function boundaries, so hud can show
//! exactly where in YOUR code the blocking call originates (marked with ◄).

use axum::{
    body::Bytes,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

// ============================================================================
// ENDPOINT 1: Password Hashing (bcrypt)
// Problem: bcrypt is intentionally slow (~100ms) and blocks the executor
// ============================================================================

#[derive(Serialize)]
struct HashResponse {
    hash: String,
}

/// BAD: bcrypt blocks the async runtime for ~100ms per request
async fn hash_password_bad(body: Bytes) -> Json<HashResponse> {
    let password = String::from_utf8_lossy(&body).to_string();

    // This blocks! bcrypt is CPU-intensive by design
    let hash = bcrypt::hash(&password, 10).unwrap_or_default();

    Json(HashResponse { hash })
}

/// GOOD: Offload to blocking threadpool
#[allow(dead_code)]
async fn hash_password_good(body: Bytes) -> Json<HashResponse> {
    let password = String::from_utf8_lossy(&body).to_string();

    let hash = tokio::task::spawn_blocking(move || bcrypt::hash(&password, 10).unwrap_or_default())
        .await
        .unwrap();

    Json(HashResponse { hash })
}

// ============================================================================
// ENDPOINT 2: Large JSON Parsing
// Problem: Parsing large payloads is CPU-bound
// ============================================================================

#[derive(Deserialize)]
struct LargePayload {
    items: Vec<String>,
}

#[derive(Serialize)]
struct ParseResponse {
    count: usize,
    first: Option<String>,
}

/// BAD: Large JSON parsing blocks the executor
async fn parse_json_bad(Json(payload): Json<LargePayload>) -> Json<ParseResponse> {
    // Simulate additional processing that magnifies the blocking
    let mut processed = Vec::with_capacity(payload.items.len());
    for item in &payload.items {
        // String operations that add CPU time
        let upper = item.to_uppercase();
        let reversed: String = upper.chars().rev().collect();
        processed.push(reversed);
    }

    // More CPU work: sort
    processed.sort();

    Json(ParseResponse { count: processed.len(), first: processed.first().cloned() })
}

// ============================================================================
// ENDPOINT 3: Compression
// Problem: Sync compression libraries block
// ============================================================================

#[derive(Serialize)]
struct CompressResponse {
    original_size: usize,
    compressed_size: usize,
}

/// BAD: flate2 compression is sync and blocks
async fn compress_bad(body: Bytes) -> Json<CompressResponse> {
    let original_size = body.len();

    // Simulate compressing the data multiple times (magnify blocking)
    let mut data = body.to_vec();
    for _ in 0..3 {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&data).unwrap();
        data = encoder.finish().unwrap();
    }

    Json(CompressResponse { original_size, compressed_size: data.len() })
}

/// GOOD: Offload compression to blocking threadpool
#[allow(dead_code)]
async fn compress_good(body: Bytes) -> Json<CompressResponse> {
    let original_size = body.len();

    let compressed_size = tokio::task::spawn_blocking(move || {
        let mut data = body.to_vec();
        for _ in 0..3 {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            encoder.write_all(&data).unwrap();
            data = encoder.finish().unwrap();
        }
        data.len()
    })
    .await
    .unwrap();

    Json(CompressResponse { original_size, compressed_size })
}

// ============================================================================
// ENDPOINT 4: File I/O
// Problem: std::fs blocks the executor
// ============================================================================

#[derive(Serialize)]
struct ReadResponse {
    size: usize,
    preview: String,
}

/// BAD: `std::fs::File` blocks the async runtime
async fn read_file_bad() -> Json<ReadResponse> {
    // Read /proc/meminfo multiple times to simulate file operations
    let mut content = String::new();
    for _ in 0..10 {
        let mut file = std::fs::File::open("/proc/meminfo").unwrap();
        let mut buf = String::new();
        file.read_to_string(&mut buf).unwrap();
        content.push_str(&buf);
    }

    Json(ReadResponse { size: content.len(), preview: content.chars().take(100).collect() })
}

/// GOOD: Use `tokio::fs` for async file I/O
#[allow(dead_code)]
async fn read_file_good() -> Json<ReadResponse> {
    let mut content = String::new();
    for _ in 0..10 {
        let buf = tokio::fs::read_to_string("/proc/meminfo").await.unwrap();
        content.push_str(&buf);
    }

    Json(ReadResponse { size: content.len(), preview: content.chars().take(100).collect() })
}

// ============================================================================
// ENDPOINT 5: DNS Lookup
// Problem: std::net DNS resolution is blocking
// ============================================================================

#[derive(Serialize)]
struct DnsResponse {
    addresses: Vec<String>,
}

/// BAD: `std::net::ToSocketAddrs` blocks
async fn dns_lookup_bad() -> Json<DnsResponse> {
    use std::net::ToSocketAddrs;

    // Multiple DNS lookups to magnify blocking
    let mut addresses = Vec::new();
    let hosts = ["localhost:80", "127.0.0.1:80", "0.0.0.0:80"];

    for host in hosts {
        if let Ok(addrs) = host.to_socket_addrs() {
            for addr in addrs {
                addresses.push(addr.to_string());
            }
        }
    }

    Json(DnsResponse { addresses })
}

/// GOOD: Use tokio's async DNS lookup
#[allow(dead_code)]
async fn dns_lookup_good() -> Json<DnsResponse> {
    let mut addresses = Vec::new();
    let hosts = ["localhost", "127.0.0.1", "0.0.0.0"];

    for host in hosts {
        if let Ok(addrs) = tokio::net::lookup_host(format!("{host}:80")).await {
            for addr in addrs {
                addresses.push(addr.to_string());
            }
        }
    }

    Json(DnsResponse { addresses })
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    // Allow overriding Tokio's thread name via THREAD_NAME env var.
    // Useful for testing hud's --workers flag and auto-discovery:
    //   THREAD_NAME=my-worker ./target/debug/examples/demo-server
    let thread_name = std::env::var("THREAD_NAME").unwrap_or_default();

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    if !thread_name.is_empty() {
        builder.thread_name(&thread_name);
    }
    let runtime = builder.build().unwrap();
    runtime.block_on(async_main());
}

async fn async_main() {
    // BAD versions (for demonstrating hud)
    let app = Router::new()
        .route("/hash", post(hash_password_bad))
        .route("/parse", post(parse_json_bad))
        .route("/compress", post(compress_bad))
        .route("/read", get(read_file_bad))
        .route("/dns", get(dns_lookup_bad));

    // GOOD versions (uncomment to verify fixes)
    // let app = Router::new()
    //     .route("/hash", post(hash_password_good))
    //     .route("/parse", post(parse_json_bad))  // Still shows some blocking
    //     .route("/compress", post(compress_good))
    //     .route("/read", get(read_file_good))
    //     .route("/dns", get(dns_lookup_good));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    println!("Demo server listening on http://localhost:3000");
    println!();
    println!("Endpoints (all intentionally blocking for demo):");
    println!("  POST /hash      - bcrypt password hashing");
    println!("  POST /parse     - JSON parsing + processing");
    println!("  POST /compress  - gzip compression");
    println!("  GET  /read      - file I/O");
    println!("  GET  /dns       - DNS lookup");
    println!();
    println!("Generate load:");
    println!("  ./hud/examples/load.sh        # continuous load");
    println!("  ./hud/examples/load.sh 20     # 20 req/s");

    axum::serve(listener, app).await.unwrap();
}
