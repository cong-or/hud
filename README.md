# hud

**F-35 inspired heads-up display for Rust async programs**

See what your async runtime is actually doing - zero instrumentation required.

```bash
# Profile any Tokio app
sudo ./hud --pid $(pgrep my-app) --target ./my-app --trace

# View results
./hud --tui trace.json
```

## What You'll See

**Glass Cockpit TUI** - Instant insights:
- 🎯 **Master Status** - CAUTION/NORMAL health at a glance
- 🔥 **Top Issues** - Hottest functions with CPU % and source location
- 📊 **Worker Bars** - Load distribution across threads
- ⏱️  **Timeline** - Execution flow over time

**Chrome Timeline** - Deep dive temporal analysis:
- When did things happen?
- What was running simultaneously?
- Worker utilization patterns over time

## Quick Start

```bash
# 1. Build everything
cargo xtask build-ebpf --release
cargo build --release

# 2. Run test app
cargo build --release --example test-async-app
./target/release/examples/test-async-app &

# 3. Profile it (30 seconds)
sudo -E ./target/release/hud \
  --pid $! \
  --target ./target/release/examples/test-async-app \
  --trace

# 4. View in TUI
./target/release/hud --tui trace.json

# Or view in Chrome
google-chrome chrome://tracing  # Load trace.json
```

**One-liner test:**
```bash
./test.sh  # Builds, profiles, generates trace.json
```

## How It Works

Uses eBPF to capture:
- **Scheduler events** - When workers start/stop (sched_switch)
- **CPU samples** - What's actually running (perf_event @ 99Hz)
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
# Profile and generate trace
hud --pid <PID> --target <BINARY> --trace [--duration 30]

# View trace in TUI
hud --tui trace.json

# Options
  --pid <PID>              Process to profile
  --target <PATH>          Binary path (for symbol resolution)
  --trace                  Generate trace.json
  --duration <SECS>        How long to profile (default: 30)
  --trace-output <FILE>    Output file (default: trace.json)
```

## Use Cases

**"What's slow?"** - TUI shows hot functions instantly
**"When did it get slow?"** - Timeline shows exact moments
**"Are workers balanced?"** - Worker bars show distribution
**"What's blocking?"** - See all workers idle except one
**"How does this code work?"** - Visual execution flow

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

## Project Status

✅ **Production Ready** - Core profiling and visualization complete
🎯 **Active Development** - Enhanced TUI features in progress

See [ROADMAP.md](docs/ROADMAP.md) for planned features.

## Why hud?

| Flamegraph | hud |
|------------|-----|
| What is slow? | What is slow + When + Why |
| Aggregated averages | Temporal patterns |
| No source locations | file:line in TUI |
| Overall stats | Worker-level insights |
| Static view | Timeline + Live TUI |

**You need both** - flamegraph for structure, hud for behavior.

## License

MIT or Apache-2.0 (dual licensed)

## Built With

[Aya](https://aya-rs.dev/) • [ratatui](https://ratatui.rs/) • [addr2line](https://github.com/gimli-rs/addr2line)

Inspired by F-35 glass cockpits, Chrome DevTools, and Brendan Gregg's performance tools.

---

**HUD = Heads-Up Display for async Rust** • Make the invisible visible • Profile with confidence
