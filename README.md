# hud

**Real-time async profiler for Tokio applications**

Zero-overhead eBPF profiling with live TUI. Attach to any running process, no code changes needed.

```bash
sudo ./hud --pid $(pgrep my-app) --target ./my-app
```

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

Attaches eBPF programs to capture:
- Scheduler events (sched_switch) - when workers start/stop
- CPU samples (perf_event @ 99Hz) - what's executing now
- Stack traces with DWARF symbols - file:line resolution

Zero overhead, works on any Tokio app.

## Troubleshooting

| Issue | Solution |
|-------|----------|
| No function names? | Add `debug = true` to Cargo.toml |
| Permission denied? | Use `sudo -E` to preserve env |
| No events? | Target must be running Tokio app |

See [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) for details.

## vs Flamegraph

| Flamegraph | hud |
|------------|-----|
| Aggregated | Real-time |
| Static | Live updating |
| Overall | Per-worker |
| No locations | file:line |

Use both: flamegraph for structure, hud for behavior.

## Docs

- [Architecture](docs/ARCHITECTURE.md) - Internals
- [TUI Guide](docs/TUI.md) - Interface details
- [Development](docs/DEVELOPMENT.md) - Contributing

## License

MIT or Apache-2.0
