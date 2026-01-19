# hud

[![CI](https://github.com/cong-or/hud/actions/workflows/ci.yml/badge.svg)](https://github.com/cong-or/hud/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Linux 5.8+](https://img.shields.io/badge/Linux-5.8%2B-yellow?logo=linux)](docs/ARCHITECTURE.md)

Find what's blocking your Tokio runtime. Zero-instrumentation eBPF profiler.

> **Linux only.** This tool uses eBPF, which is a Linux kernel feature. It does not work on macOS or Windows.

## The Problem

Tokio uses cooperative scheduling. Tasks yield at `.await` points, trusting that work between awaits is fast. When it isn't—CPU-heavy code, sync I/O, blocking locks—one task starves the rest.

These bugs are silent. No errors, no panics—just degraded throughput. hud makes them visible.

## How It Works

Watches the Linux scheduler via eBPF. When a worker thread stays on CPU too long, grabs a stack trace to show you what's blocking.

## Why hud?

| Feature | hud | tokio-console | tokio-blocked |
|---------|:---:|:-------------:|:-------------:|
| No code changes | ✓ | ✗ | ✗ |
| Attach to running process | ✓ | ✗ | ✗ |
| No recompilation | ✓ | ✗ | ✗ |
| Production-safe | ✓ | ⚠ | ⚠ |

- **[tokio-console](https://github.com/tokio-rs/console)** requires instrumenting your code with `console-subscriber`
- **[tokio-blocked](https://github.com/theduke/tokio-blocked)** requires rebuilding with `RUSTFLAGS="--cfg tokio_unstable"`

hud attaches to any running Tokio process. No instrumentation, no unstable features, no restart.

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

# Headless mode (CI/scripting)
sudo hud my-app --export trace.json --headless --duration 60
```

See [Tuning](docs/TUNING.md) for threshold selection guide.

## Demo

Try hud with the included demo server (requires Option B):

```bash
# Build demo server (debug build preserves function names)
cargo build --example demo-server
./target/debug/examples/demo-server &

# Profile it (auto-detects PID and binary)
sudo ./target/release/hud demo-server

# Generate load (another terminal)
./hud/examples/load.sh
```

The demo server has intentionally blocking endpoints (`/hash`, `/compress`, `/read`, `/dns`). You'll see `bcrypt` and `blowfish` hotspots from the `/hash` endpoint.

> **Tip**: Use debug builds for best results—release builds inline functions and hide your code. See [Troubleshooting](docs/TROUBLESHOOTING.md) if you see low Debug % in the status panel.

Press `Q` to quit hud.

## Docs

- [Tuning](docs/TUNING.md) — Threshold selection, debugging workflow
- [Exports](docs/EXPORTS.md) — JSON format, before/after analysis
- [Troubleshooting](docs/TROUBLESHOOTING.md) — Common issues
- [Architecture](docs/ARCHITECTURE.md) — How it works internally
- [Development](docs/DEVELOPMENT.md) — Contributing

## License

MIT or Apache-2.0
