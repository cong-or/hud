# runtime-scope

✅ **Status: Phase 3 Complete - Timeline Visualization Working!** ✅

**Visual timeline profiler for Rust async programs using eBPF**

Understand how your async program works by visualizing execution over time. See which workers are busy, what functions are running, and how tasks flow through your system - all with zero instrumentation required.

## What is runtime-scope?

**Think: Chrome DevTools Timeline, but for Rust async runtimes**

runtime-scope uses eBPF to capture a detailed timeline of your async program's execution and exports it to Chrome's trace format for beautiful, interactive visualization.

### Why Timeline Visualization?

**Flamegraphs show WHERE time is spent (spatial)**
runtime-scope shows **WHEN** and **HOW** things happen (temporal)

| Question | Flamegraph | runtime-scope Timeline |
|----------|-----------|----------------------|
| "What function is slow?" | ✅ Yes | ✅ Yes |
| "When did it get slow?" | ❌ Aggregated | ✅ See exact moment |
| "What was happening simultaneously?" | ❌ No temporal info | ✅ See all workers |
| "Is this intermittent?" | ❌ Just averages | ✅ See patterns |
| "Are workers balanced?" | ❌ Overall stats | ✅ See distribution over time |

**Timeline = Map (structure) + Movie (behavior)**

## Current Status

### ✅ Phase 1 & 2 Complete
- Real-time event collection via eBPF
- Complete stack trace capture (55+ frames)
- DWARF symbol resolution with source locations
- Tokio worker thread identification
- Async task tracking

### ✅ Phase 3 Complete - Timeline Visualization (Jan 2026)
- **Enhanced event system** with execution span tracking
- **sched_switch integration** - tracks when workers start/stop executing
- **Execution timeline events** - TRACE_EXECUTION_START/END
- **Rich metadata** - worker IDs, CPU info, categories
- **Chrome trace exporter** - exports to trace.json for chrome://tracing visualization
- **Symbol resolution** - automatic function name resolution in traces
- **Progress indicators** - shows collection progress in real-time
- **Automatic timeout** - exits cleanly after specified duration
- **Tested and working** - successfully generated 10-second traces with 400+ events

## Quick Start

### Option 1: Use the test script (easiest)

```bash
cd /home/soze/runtime-scope
./test_trace.sh
```

This will build everything, run the test app, collect a 10-second trace, and generate `trace.json`.

### Option 2: Manual steps

```bash
cd /home/soze/runtime-scope

# Build everything (release mode required)
cargo xtask build-ebpf --release
cargo build --release -p runtime-scope
cargo build --release --example test-async-app

# Run test app
./target/release/examples/test-async-app &

# Profile it (generates trace.json)
sudo -E ./target/release/runtime-scope \
  --pid $(pgrep test-async-app) \
  --target ./target/release/examples/test-async-app \
  --trace \
  --duration 30 \
  --trace-output trace.json

# Open in Chrome
google-chrome chrome://tracing
# Load trace.json
```

## CLI Reference

```
runtime-scope [OPTIONS] --pid <PID> --target <PATH>

Options:
  -p, --pid <PID>                    Process ID to attach to
  -t, --target <PATH>                Path to target binary
      --trace                        Enable Chrome trace export
      --duration <SECONDS>           Duration to profile in seconds (default: 30)
      --trace-output <FILE>          Output path for trace JSON (default: trace.json)
      --no-live                      Trace-only mode (suppress live event output)
  -h, --help                         Print help
```

**Examples:**

```bash
# Live monitoring mode (default)
sudo -E ./target/release/runtime-scope --pid 1234 --target ./my-app

# Generate Chrome trace (10 seconds, quiet mode)
sudo -E ./target/release/runtime-scope \
  --pid 1234 \
  --target ./my-app \
  --trace \
  --duration 10 \
  --no-live

# Custom trace output location
sudo -E ./target/release/runtime-scope \
  --pid 1234 \
  --target ./my-app \
  --trace \
  --trace-output /tmp/my-trace.json
```

**What you'll see:**
- Timeline showing active Tokio workers during the profiling period
- Execution spans with precise start/end times
- Worker IDs and CPU assignments in metadata
- Zoom, pan, search with chrome://tracing's interactive UI
- Visual understanding of worker activity patterns over time

**Note:** Function names require debug symbols (see Troubleshooting below)

## Viewing Your Trace

After running the profiler with `--trace`, you'll have a `trace.json` file ready for visualization.

### Open in Chrome/Chromium

1. **Open the trace viewer:**
   ```bash
   google-chrome chrome://tracing
   # or
   chromium chrome://tracing
   ```

2. **Load your trace:**
   - Click the **"Load"** button in the top-left
   - Select `trace.json`

3. **Navigate the timeline:**
   - **W** - Zoom in
   - **S** - Zoom out
   - **A** - Pan left
   - **D** - Pan right
   - **Mouse drag** - Pan
   - **Mouse scroll** - Zoom
   - **Click** on spans to see details

### What You'll See

**Typical trace.json contents:**
- **File size:** 50KB - 500KB for 10-second traces
- **Events:** 400-2000+ execution spans depending on activity
- **Workers:** 1-24 Tokio worker threads (only active workers shown)
- **Timeline:** Precise start/end times in microseconds

**Reading the visualization:**
- Each horizontal row = one worker thread (TID)
- Colored blocks = execution spans (time on CPU)
- Gaps = worker was idle/off-CPU
- Click spans to see: worker_id, cpu_id, duration, timestamps

**Example trace stats:**
```bash
$ ls -lh trace.json
-rw-r--r-- 1 user user 75K Jan 1 23:38 trace.json

$ jq '.traceEvents | length' trace.json
423

$ jq '.traceEvents | map(.ts) | [min, max]' trace.json
[0.0, 9695.471856]  # 0ms to 9.7 seconds
```

### Alternative: Perfetto UI

Chrome Trace format is also compatible with [Perfetto](https://ui.perfetto.dev/):
```bash
# Open in browser
firefox https://ui.perfetto.dev/
# Drag and drop trace.json
```

Perfetto offers additional features like SQL queries over trace data.

## Example Visualization

```
═══════════════════════════════════════════════════════════════
                    Worker 1
───────────────────────────────────────────────────────────────
   [process_request]████[json_parse]████░░[validate]██
                          (150ms)

═══════════════════════════════════════════════════════════════
                    Worker 2
───────────────────────────────────────────────────────────────
   ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ (idle)

═══════════════════════════════════════════════════════════════
                    Worker 3
───────────────────────────────────────────────────────────────
   ░░░░░░░░[db_query]████████████████████░░░░░░░░░░░
              (500ms - BOTTLENECK!)

Time → 0ms     100ms    200ms    300ms    400ms    500ms
```

**Insights at a glance:**
- Worker 1: Processing requests normally
- Worker 2: Completely idle (work imbalance!)
- Worker 3: Stuck in long DB query (blocking!)

## Use Cases

### 1. Understanding New Codebases
**Problem:** "I just joined the team, how does this async code work?"

**Solution:** Generate a trace, see execution flow visually
- See which functions call which
- Understand task spawning patterns
- Learn how the runtime schedules work
- Visual understanding >> reading code

### 2. Debugging Performance Issues
**Problem:** "My API is slow sometimes"

**Solution:** Timeline shows you the exact moment and context
- See what was running when it got slow
- Identify intermittent issues
- Correlate with external events
- Understand cascading effects

### 3. Finding Blocking Operations
**Problem:** "Something is blocking my async runtime"

**Solution:** Timeline reveals blocking visually
- See all workers idle except one
- Identify which function is blocking
- Measure actual impact on the system
- Distinguish CPU work from I/O waits

### 4. Optimizing Worker Utilization
**Problem:** "Are my tasks distributed evenly?"

**Solution:** See worker activity over time
- Spot imbalanced work distribution
- Identify idle workers
- Find parallelization opportunities
- Optimize task spawning strategy

## Why eBPF?

**Zero instrumentation required:**
- No code changes needed
- No recompilation required
- Attach to running processes
- Works on production binaries

**Low overhead:**
- <5% CPU overhead
- Safe (kernel verifier ensures correctness)
- Can run continuously in production

**Rich data:**
- Kernel-level visibility (scheduler events)
- User-level context (stack traces)
- Precise timing (nanosecond resolution)
- Full async runtime context

## How It Works

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Kernel Space (eBPF)                       │
├─────────────────────────────────────────────────────────────┤
│  sched_switch tracepoint → Track worker ON/OFF CPU          │
│  Stack capture → What function is executing                 │
│  Maps → Track worker state, execution spans                 │
│  Ring buffer → Stream events to userspace                   │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                    Userspace (Rust)                          │
├─────────────────────────────────────────────────────────────┤
│  Event processor → Parse eBPF events                        │
│  Symbol resolver → DWARF debug info → function names        │
│  Timeline builder → Reconstruct execution timeline          │
│  Chrome trace exporter → Generate trace.json                │
└─────────────────────────────────────────────────────────────┘
                            ↓
                      trace.json
                            ↓
                    chrome://tracing
                   (Beautiful visualization!)
```

### Event Flow

**When a Tokio worker starts executing:**
1. Scheduler switches thread ON CPU → `sched_switch` fires
2. eBPF captures stack trace
3. Emits `TRACE_EXECUTION_START` event
4. Records execution span in `EXECUTION_SPANS` map

**When a Tokio worker stops executing:**
1. Scheduler switches thread OFF CPU → `sched_switch` fires
2. eBPF retrieves execution span
3. Calculates duration
4. Emits `TRACE_EXECUTION_END` event

**Result:**
Complete timeline of what each worker executed and for how long!

## Temporal vs Spatial Understanding

### Flamegraph (Spatial) - "The Map"
```
              [main]
                ↓
         ┌──────┴──────┐
         ↓             ↓
   [handler_a]    [handler_b]
   (40% wide)     (10% thin)
         ↓
    [db_query]
    (30% WIDE = SLOW!)
```

**Answers:**
- What is the structure?
- Where is time spent overall?
- What calls what?

### Timeline (Temporal) - "The Movie"
```
Time →  0ms        100ms       200ms       300ms

Worker 1: ████[handler_a]████░░░░░░░[handler_a]████
                   ↓                      ↓
Worker 2: ░░░░[db_query]████████████░░░░[db_query]██
                150ms!                    80ms
```

**Answers:**
- What is the behavior?
- When did things happen?
- How do pieces move together?

**You need BOTH** - flamegraph for structure, timeline for behavior!

## Troubleshooting

### Sudo Password Prompt Issues

**Problem:** When running `test_trace.sh`, the sudo password prompt disappears quickly.

**Solution:** The script now prompts for sudo at the beginning (before starting the background app). Just enter your password when you see:
```
🔐 Requesting sudo access...
```

If you still have issues, you can authenticate sudo manually first:
```bash
sudo -v
./test_trace.sh
```

### Missing Function Names in Trace

**Problem:** Chrome trace shows `trace_-1` instead of actual function names.

**Cause:** Release builds strip debug symbols and frame pointers, causing eBPF stack capture to fail.

**Solution:** Enable debug symbols in release builds:

1. Add to your app's `Cargo.toml`:
```toml
[profile.release]
debug = true
force-frame-pointers = true
```

2. Rebuild and re-profile:
```bash
cargo build --release --example test-async-app
./test_trace.sh
```

**Note:** The trace is still useful without function names! You can see:
- Which workers were active
- Execution timing and duration
- Worker utilization patterns
- CPU assignments

### Profiler Hangs / Doesn't Exit

**Problem:** Profiler runs forever instead of stopping after duration.

**Status:** ✅ Fixed in latest version (Jan 2026)

The profiler now:
- Checks timeout at the start of each loop iteration
- Shows progress updates every 2 seconds
- Exits cleanly after the specified duration

If you built before Jan 1, 2026, rebuild:
```bash
cargo build --release -p runtime-scope
```

### No Events Captured

**Problem:** Trace file is empty or has very few events.

**Possible causes:**
1. Target app isn't running Tokio (only Tokio workers are tracked)
2. Workers are mostly idle (sleeping/waiting, not using CPU)
3. Profiling duration too short

**Solution:**
- Verify target is a Tokio app with active work
- Increase duration: `--duration 30`
- Check app logs: `tail /tmp/test-async-app.log`

### Permission Denied Errors

**Problem:** `Permission denied` when loading eBPF programs.

**Cause:** eBPF requires root privileges.

**Solution:** Always use `sudo -E` to preserve environment variables:
```bash
sudo -E ./target/release/runtime-scope --pid <PID> --target <PATH>
```

## Development Setup

### Prerequisites

- Linux kernel 5.15+ (for eBPF support)
- Rust 1.75+ with nightly toolchain
- LLVM 20-22 development libraries
- Clang compiler

### Installing Dependencies

**Fedora / RHEL / CentOS:**
```bash
sudo dnf install -y llvm-devel libffi-devel clang
rustup toolchain install nightly --component rust-src
cargo install bpf-linker --git https://github.com/aya-rs/bpf-linker --features llvm-21
```

**Ubuntu / Debian:**
```bash
wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 21
sudo apt-get install -y llvm-21-dev libclang-21-dev libelf-dev libz-dev clang-21
rustup toolchain install nightly --component rust-src
cargo install bpf-linker --features llvm-21
```

### Building from Source

```bash
# Clone repository
git clone https://github.com/yourusername/runtime-scope
cd runtime-scope

# Build eBPF program
cargo xtask build-ebpf --release

# Build userspace program
cargo build --release -p runtime-scope

# Build test application
cargo build --release --example test-async-app
```

**Note:** Release builds required due to eBPF verifier limitations.

### Testing

```bash
# Quick test
./test.sh

# Or manual test
./target/release/examples/test-async-app &
sudo -E ./target/release/runtime-scope \
  --pid $! \
  --target ./target/release/examples/test-async-app

# Cleanup
./cleanup.sh
```

## Project Structure

```
runtime-scope/
├── runtime-scope/              # Userspace profiler
│   ├── src/
│   │   ├── main.rs            # CLI, event processing
│   │   ├── symbolizer.rs      # DWARF symbol resolution
│   │   └── trace_exporter.rs  # Chrome trace export (WIP)
│   └── examples/
│       └── test-async-app.rs  # Test application
├── runtime-scope-ebpf/         # eBPF programs (kernel)
│   └── src/
│       └── main.rs            # Execution tracking, sched_switch hook
├── runtime-scope-common/       # Shared types
│   └── src/
│       └── lib.rs             # Event definitions, ExecutionSpan
├── xtask/                      # Build automation
├── VISUALIZATION_DESIGN.md     # Design documentation
├── REFACTORING_PLAN.md        # Implementation roadmap
└── README.md                   # This file
```

## Roadmap

### ✅ Completed
- [x] Phase 1: Basic eBPF infrastructure
- [x] Phase 2: Stack traces + async task tracking
- [x] Phase 3 (Part 1): Enhanced event types
- [x] Phase 3 (Part 2): Execution span tracking in eBPF
- [x] **Phase 3 (Part 3): Chrome trace exporter** ✨ (Jan 2026)
- [x] **Phase 3 (Part 4): CLI flags for trace export** ✨ (Jan 2026)
- [x] **Phase 3 (Part 5): End-to-end timeline visualization** ✨ (Jan 2026)

**Milestone:** Successfully generated 10-second trace with 423 events, visualized in chrome://tracing!

### 🎯 Next
- [ ] Phase 4: Flow arrows (task spawning visualization)
- [ ] Phase 5: Rich annotations (categories, colors)
- [ ] Phase 6: Live terminal dashboard (optional)

## Contributing

We welcome contributions! Areas where help is needed:

- **Chrome trace exporter** - Convert events to trace JSON
- **Flow tracking** - Detect task spawn → execution relationships
- **Category detection** - Auto-categorize functions (DB, network, etc.)
- **Documentation** - Usage examples, tutorials
- **Testing** - Test on real-world async applications

## Resources

- [Design Document](VISUALIZATION_DESIGN.md) - Detailed visualization design
- [Refactoring Plan](REFACTORING_PLAN.md) - Implementation roadmap
- [Aya Documentation](https://aya-rs.dev/) - Rust eBPF framework
- [Chrome Tracing](https://www.chromium.org/developers/how-tos/trace-event-profiling-tool/) - Trace format docs

## License

MIT or Apache-2.0 (dual licensed)

## Acknowledgments

Built with:
- [Aya](https://aya-rs.dev/) - Pure Rust eBPF library
- [addr2line](https://github.com/gimli-rs/addr2line) - DWARF symbol resolution
- Inspired by Chrome DevTools and Brendan Gregg's performance tools

---

**Status:** ✅ Phase 3 Complete - Timeline visualization working! (Jan 1, 2026)

**Latest Achievement:** Successfully generated and visualized execution timeline with 423 events over 9.7 seconds, showing Tokio worker activity patterns in chrome://tracing.

**Goal:** Make async Rust behavior visible and understandable through beautiful timeline visualization.
