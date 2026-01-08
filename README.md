# hud

A KISS Linux tool. One job: find what's blocking your Tokio runtime.

Minimal-overhead eBPF profiling. Attach to any running process, no code changes needed.

```bash
sudo ./hud --pid $(pgrep my-app) --target ./my-app
```

## The Problem

Tokio uses cooperative scheduling. Tasks yield at `.await` points, trusting that work between awaits is fast. When it isn't—CPU-heavy code, sync I/O, blocking locks—one task starves the rest.

```rust
async fn handle(req: Request) -> Response {
    let user = db.get_user(id).await;
    let report = generate_pdf(&user);  // CPU-bound, blocks worker
    Response::ok(report)
}
```

These bugs are silent. No errors, no panics—just degraded throughput. hud makes them visible.

## Quick Start

```bash
# Build
cargo xtask build-ebpf --release && cargo build --release

# Run test app
cargo build --release --example test-async-app
./target/release/examples/test-async-app &

# Profile (live TUI appears)
sudo -E ./target/release/hud --pid $! --target ./target/release/examples/test-async-app

# Press Q to quit
```

## Usage

```bash
# Live TUI
hud --pid <PID> --target <BINARY>

# Export for later
hud --pid <PID> --target <BINARY> --export trace.json

# Replay
hud --replay trace.json

# Headless
hud --pid <PID> --target <BINARY> --export trace.json --headless --duration 60
```

## What You See

- **Hotspots** - Functions sorted by total CPU time with file:line
- **Workers** - Per-thread utilization and blocking events
- **Timeline** - Execution flow as it happens
- **Status** - Health indicators and event counts

All updating in real-time as your code runs.

## Requirements

**System:**
- Linux 5.15+ (eBPF support)
- Root or CAP_BPF privileges (for eBPF attachment)

**Target binary must have debug symbols:**
```toml
# In your application's Cargo.toml
[profile.release]
debug = true
force-frame-pointers = true
```

> **Want to contribute?** See [DEVELOPMENT.md](docs/DEVELOPMENT.md) for build setup.

## How It Works

hud watches the Linux scheduler, sees a worker thread staying on CPU too long, and grabs a stack trace to show you what's guilty.

Under the hood:
- Scheduler events (sched_switch) — when workers go on/off CPU
- Stack traces with DWARF symbols — file:line resolution

## Docs

- [Architecture](docs/ARCHITECTURE.md) - Internals
- [TUI Guide](docs/TUI.md) - Interface details
- [Troubleshooting](docs/TROUBLESHOOTING.md) - Common issues
- [Development](docs/DEVELOPMENT.md) - Contributing

## License

MIT or Apache-2.0
