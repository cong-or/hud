# hud

[![CI](https://github.com/cong-or/runtime-scope/actions/workflows/ci.yml/badge.svg)](https://github.com/cong-or/runtime-scope/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Linux 5.8+](https://img.shields.io/badge/Linux-5.8%2B-yellow?logo=linux)](docs/ARCHITECTURE.md)

A KISS Linux tool. One job: find what's blocking your Tokio runtime.

Minimal-overhead eBPF profiling. Attach to any running process, no code changes needed.

```bash
sudo hud my-server
```

## The Problem

Tokio uses cooperative scheduling. Tasks yield at `.await` points, trusting that work between awaits is fast. When it isn't—CPU-heavy code, sync I/O, blocking locks—one task starves the rest.

These bugs are silent. No errors, no panics—just degraded throughput. hud makes them visible.

## How It Works

Watches the Linux scheduler via eBPF. When a worker thread stays on CPU too long, grabs a stack trace to show you what's blocking.

## Why hud?

| | hud | tokio-console | tokio-blocked |
|---|:---:|:---:|:---:|
| No code changes | ✓ | ✗ | ✗ |
| Attach to running process | ✓ | ✗ | ✗ |
| No recompilation | ✓ | ✗ | ✗ |
| Production-safe | ✓ | ⚠ | ⚠ |

**[tokio-console](https://github.com/tokio-rs/console)** requires instrumenting your code with `console-subscriber`. **[tokio-blocked](https://github.com/theduke/tokio-blocked)** requires rebuilding with `RUSTFLAGS="--cfg tokio_unstable"`.

hud attaches to any running Tokio process. No instrumentation, no unstable features, no restart.

## Requirements

**System:**
- Linux 5.8+ (eBPF ring buffer support)
- x86_64 or aarch64 architecture
- Root or CAP_BPF privileges

**Target binary must have debug symbols:**
```toml
# In your application's Cargo.toml
[profile.release]
debug = true
force-frame-pointers = true
```

## Quick Start

```bash
# Build hud and demo app
cargo xtask build-ebpf --release && cargo build --release --examples

# Run demo server
./target/release/examples/demo-server &

# Profile it
sudo ./target/release/hud demo-server

# Generate load (in another terminal)
curl -X POST http://localhost:3000/process -H "Content-Type: application/json" -d '{"data":"test"}'

# Press Q to quit hud
```

## Usage

All commands require root.

```bash
# Auto-detect PID and binary
hud my-app

# Explicit PID (binary auto-detected)
hud --pid 1234

# Export for later analysis
hud my-app --export trace.json

# Replay saved trace
hud --replay trace.json

# Headless mode for scripting
hud my-app --export trace.json --headless --duration 60
```

## What You See

Real-time TUI showing hotspots (functions by CPU time), worker utilization, and stack traces with file:line resolution.

> **Want to contribute?** See [DEVELOPMENT.md](docs/DEVELOPMENT.md)

## Docs

- [Architecture](docs/ARCHITECTURE.md) - Internals
- [Troubleshooting](docs/TROUBLESHOOTING.md) - Common issues
- [Development](docs/DEVELOPMENT.md) - Contributing

## License

MIT or Apache-2.0
