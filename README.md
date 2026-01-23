# hud

[![CI](https://github.com/cong-or/hud/actions/workflows/ci.yml/badge.svg)](https://github.com/cong-or/hud/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Linux 5.8+](https://img.shields.io/badge/Linux-5.8%2B-yellow?logo=linux)](docs/ARCHITECTURE.md)

Find what's blocking your Tokio runtime. Zero-instrumentation eBPF profiler.

> **Linux only.** This tool uses eBPF, which is a Linux kernel feature. It does not work on macOS or Windows.

![hud demo](docs/demo.gif)

## The Problem

Tokio uses cooperative scheduling. Tasks yield at `.await` points, trusting that work between awaits is fast. When it isn't—CPU-heavy code, sync I/O, blocking locks—one task starves the rest.

These bugs are silent. No errors, no panics—just degraded throughput. hud makes them visible.

## How It Works

Watches the Linux scheduler via eBPF. When a worker thread experiences high scheduling latency (time waiting in the run queue), captures a stack trace. High scheduling latency is a symptom of blocking—when one task monopolizes a worker, others queue up waiting.

## Why hud?

Unlike [tokio-console](https://github.com/tokio-rs/console) or [tokio-blocked](https://github.com/theduke/tokio-blocked), hud requires no code changes—attach to any running Tokio process.

### When to use what

| Tool | Best for | Trade-off |
|------|----------|-----------|
| **hud** | Quick triage of running processes | Measures symptoms, not direct cause |
| **tokio-console** | Precise task poll times | Requires code instrumentation |
| **perf + flamegraphs** | CPU profiling, broad analysis | Manual interpretation needed |
| **Custom metrics** | Production monitoring | Must know where to instrument |

Use hud to narrow down suspects, then dig deeper with instrumentation if needed.

## Requirements

**System:**
- Linux 5.8+
- x86_64 architecture
- Root privileges

**Your application needs debug symbols** (so hud can show function names):
```toml
# Cargo.toml
[profile.release]
debug = true
force-frame-pointers = true
```

> `debug = true` adds ~10-20% to binary size. `force-frame-pointers` adds ~1-2% runtime overhead. For production, you can swap in a debug-enabled binary temporarily for investigation.

## Install

**Option A: Pre-built binary** (no Rust toolchain needed)

```bash
curl -L https://github.com/cong-or/hud/releases/latest/download/hud-linux-x86_64.tar.gz | tar xz
sudo ./hud my-app
```

**Option B: Build from source**

```bash
git clone https://github.com/cong-or/hud.git && cd hud
cargo xtask build-ebpf --release && cargo build --release
sudo ./target/release/hud my-app
```

## Usage

```bash
# Profile by process name
sudo hud my-app

# Profile by PID
sudo hud --pid 1234

# Custom blocking threshold (default: 5ms)
sudo hud my-app --threshold 10   # less sensitive
sudo hud my-app --threshold 1    # more sensitive

# Rolling time window (only show last N seconds)
sudo hud my-app --window 30      # metrics decay when load stops

# Headless mode (CI/scripting) - run for 60 seconds then exit
sudo hud my-app --headless --export trace.json --duration 60
```

See [Tuning](docs/TUNING.md) for threshold selection guide.

## Demo

Try hud with the included demo server (requires Option B):

```bash
# Build demo server - MUST be debug build (release inlines functions)
cargo build --example demo-server
./target/debug/examples/demo-server &

# Profile it (auto-detects PID and binary)
sudo ./target/release/hud demo-server

# Generate load (another terminal)
./hud/examples/load.sh
```

The demo server has intentionally blocking endpoints (`/hash`, `/compress`, `/read`, `/dns`). You'll see `bcrypt` and `blowfish` hotspots from the `/hash` endpoint, with `demo-server.rs` highlighted as the entry point in call traces.

> **Important**: The demo-server **must** be a debug build. Release builds aggressively inline functions, hiding your code from stack traces. If you don't see `demo-server.rs` in drilldowns, rebuild without `--release`.

Press `Q` to quit hud.

## Limitations

- Measures scheduling latency (a *symptom* of blocking), not blocking directly
- System CPU pressure can cause false positives—look for consistent, repeatable traces
- Lock contention where threads sleep (not spin) may not appear
- See [Troubleshooting](docs/TROUBLESHOOTING.md) for common issues

## Docs

- [Tuning](docs/TUNING.md) — Threshold selection, debugging workflow
- [Exports](docs/EXPORTS.md) — JSON format, before/after analysis
- [Troubleshooting](docs/TROUBLESHOOTING.md) — Common issues
- [Architecture](docs/ARCHITECTURE.md) — How it works internally
- [Development](docs/DEVELOPMENT.md) — Contributing

## Further Reading

- [Async: What is blocking?](https://ryhl.io/blog/async-what-is-blocking/) — Alice Ryhl's deep dive on blocking in async Rust
- [tokio::task::spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) — Offload blocking I/O to a thread pool
- [rayon](https://docs.rs/rayon) — Better than spawn_blocking for CPU-heavy parallel work
- [Reducing tail latencies with automatic cooperative task yielding](https://tokio.rs/blog/2020-04-preemption) — Tokio's approach to preemption

## License

MIT or Apache-2.0
