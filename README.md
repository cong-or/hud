# hud

**F-35 inspired heads-up display for Rust async programs**

See what your async runtime is actually doing - live, with zero instrumentation.

```bash
# Live profiling with real-time HUD
sudo ./hud --pid $(pgrep my-app) --target ./my-app
```

## What You'll See

**Live Glass Cockpit** - Real-time F-35 style HUD updating as your code runs:
- 🎯 **Master Status** - CAUTION/NORMAL health at a glance
- 🔥 **Top Hotspots** - Functions consuming CPU with file:line locations
- 📊 **Worker Load** - Distribution across runtime threads
- ⏱️  **Timeline** - Execution flow as events arrive
- 🔴 **● LIVE** indicator - Streaming profiling data in real-time

## Quick Start

```bash
# 1. Build everything
cargo xtask build-ebpf --release
cargo build --release

# 2. Run your app (or use test app)
cargo build --release --example test-async-app
./target/release/examples/test-async-app &

# 3. Attach profiler - live HUD appears immediately
sudo -E ./target/release/hud \
  --pid $! \
  --target ./target/release/examples/test-async-app

# Press Q to quit
```

**Modes:**

```bash
# Live HUD (default)
hud --pid <PID> --target <BINARY>

# Live HUD + export for later replay
hud --pid <PID> --target <BINARY> --export trace.json

# Replay previously captured session
hud --replay trace.json

# Headless data collection (no TUI)
hud --pid <PID> --target <BINARY> --export trace.json --headless
```

## How It Works

Uses eBPF to capture:
- **Scheduler events** - When workers start/stop (sched_switch)
- **CPU samples** - What's running right now (perf_event @ 99Hz)
- **Stack traces** - With DWARF symbols for file:line locations

Zero overhead, attach to any running process, no code changes needed.

## Requirements

**Enable debug symbols in release builds:**

```toml
# In your Cargo.toml
[profile.release]
debug = true
force-frame-pointers = true
```

Without this, you'll see timings but not function names.

## CLI Reference

```bash
# Live profiling with TUI
hud --pid <PID> --target <BINARY>

# Options
  --pid <PID>              Process to profile
  --target <PATH>          Binary path (for symbol resolution)
  --export <FILE>          Also save to file for replay
  --replay <FILE>          Load and view previously captured session
  --duration <SECS>        Auto-stop after N seconds (0 = unlimited)
  --headless               Profile without TUI (requires --export)
```

## Use Cases

**"What's slow?"** - Hotspots update in real-time
**"When did it get slow?"** - Timeline shows moments as they happen
**"Are workers balanced?"** - Worker bars show live distribution
**"What's blocking?"** - See idle workers immediately
**"How does this work?"** - Watch execution flow live

## Installation

**Prerequisites:**
- Linux kernel 5.15+
- Rust nightly with rust-src component
- LLVM 20-22 dev libraries

**Fedora/RHEL:**
```bash
sudo dnf install llvm-devel libffi-devel clang
rustup toolchain install nightly --component rust-src
cargo install bpf-linker --features llvm-21
```

**Ubuntu/Debian:**
```bash
sudo apt install llvm-21-dev libclang-21-dev
rustup toolchain install nightly --component rust-src
cargo install bpf-linker --features llvm-21
```

## Troubleshooting

**No function names?**
→ Add `debug = true` to `[profile.release]` in Cargo.toml

**Permission denied?**
→ Use `sudo -E` to preserve environment variables

**No events captured?**
→ Target must be a Tokio app with active workers

**Need help?**
→ See [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) for detailed solutions

## Documentation

- **Quick Start** - You're reading it
- [Architecture](docs/ARCHITECTURE.md) - How it works internally
- [TUI Guide](docs/TUI.md) - Using the glass cockpit interface
- [Troubleshooting](docs/TROUBLESHOOTING.md) - Common issues and solutions
- [Development](docs/DEVELOPMENT.md) - Contributing and building from source

## Why hud?

| Flamegraph | hud |
|------------|-----|
| What is slow? | What is slow + When + Live |
| Aggregated averages | Real-time patterns |
| No source locations | file:line in TUI |
| Overall stats | Worker-level insights |
| Static view | Live updating HUD |

**You need both** - flamegraph for structure, hud for behavior.

## License

MIT or Apache-2.0 (dual licensed)

---

**HUD = Heads-Up Display for async Rust** • Make the invisible visible • Profile with confidence
