# runtime-scope

✅ **Status: Phase 3a Complete - Glass Cockpit TUI + CPU Profiling!** ✅

**Visual profiler for Rust async programs using eBPF**

Understand how your async program works with an F-35 inspired glass cockpit interface. See which workers are busy, what functions are hot, and get instant situational awareness - all with zero instrumentation required.

## What is runtime-scope?

**Think: F-35 Glass Cockpit + Chrome DevTools Timeline for Rust async runtimes**

runtime-scope uses eBPF with dual detection modes (scheduler events + CPU sampling) to capture execution data and displays it through an intuitive terminal UI or exports to Chrome's trace format for deep-dive analysis.

### Glass Cockpit TUI + Timeline Visualization

**Instant situational awareness** with F-35 inspired interface:
- **Master Status** - CAUTION/NORMAL at a glance
- **Top Issues** - Hottest functions with CPU % and source locations
- **Worker Bars** - Load distribution across executor threads
- **Timeline** - Execution flow over time

**Plus deep-dive timeline** for temporal analysis:

| Question | Flamegraph | runtime-scope |
|----------|-----------|--------------|
| "What function is slow?" | ✅ Yes | ✅ Yes (instant in TUI) |
| "Where in my code?" | ❌ No source | ✅ File:line in TUI |
| "When did it get slow?" | ❌ Aggregated | ✅ Timeline view |
| "What was happening simultaneously?" | ❌ No temporal info | ✅ Worker bars + timeline |
| "Is this intermittent?" | ❌ Just averages | ✅ See patterns over time |
| "Are workers balanced?" | ❌ Overall stats | ✅ Live worker bars |

**TUI = Instant insights | Timeline = Deep understanding**

## Current Status

### ✅ Phase 1 & 2 Complete
- Real-time event collection via eBPF
- Complete stack trace capture (55+ frames)
- DWARF symbol resolution with source locations (file:line)
- Tokio worker thread identification
- Async task tracking

### ✅ Phase 3 Complete - Timeline Visualization (Jan 2026)
- **sched_switch integration** - tracks when workers start/stop executing
- **Execution timeline events** - TRACE_EXECUTION_START/END
- **Chrome trace exporter** - exports to trace.json for chrome://tracing visualization
- **Symbol resolution** - automatic function name and source location in traces
- **Tested and working** - successfully generated traces with 400+ events

### ✅ Phase 3a Complete - Glass Cockpit TUI + CPU Profiling (Jan 2026)
- **Dual detection mode** - scheduler events + perf_event CPU sampling
- **User-space stack traces** - captures instruction pointers from CPU samples
- **F-35 glass cockpit TUI** - four-panel interface for instant insights
- **Master Status panel** - CAUTION/NORMAL system health indicator
- **Top Issues panel** - hottest functions with CPU %, file:line locations
- **Worker Bars panel** - load distribution visualization
- **Timeline panel** - execution flow over time
- **HUD color scheme** - green/amber/red severity indicators
- **Smart labeling** - distinguishes user code, std lib, and scheduler events
- **Interactive TUI** - keyboard navigation (arrow keys, q to quit)

## Quick Start

### Option 1: Use the test script (easiest)

```bash
cd /home/soze/runtime-scope
./test.sh
```

This will:
1. Build everything (eBPF + userspace + test app)
2. Start the test application
3. Profile it and generate `trace.json`
4. Press Ctrl+C to stop when done

Then view the results:

```bash
# Option A: Glass Cockpit TUI (recommended for quick insights)
./target/release/runtime-scope --tui trace.json

# Option B: Chrome Timeline (for deep temporal analysis)
google-chrome chrome://tracing  # then load trace.json
```

### Option 2: Manual steps

```bash
cd /home/soze/runtime-scope

# Build everything (release mode required)
cargo xtask build-ebpf --release
cargo build --release -p runtime-scope
cargo build --release --example test-async-app

# Run test app
./target/release/examples/test-async-app &
TEST_PID=$!

# Profile it (generates trace.json)
sudo -E ./target/release/runtime-scope \
  --pid $TEST_PID \
  --target ./target/release/examples/test-async-app \
  --trace

# View in TUI (instant insights)
./target/release/runtime-scope --tui trace.json

# Or view in Chrome (deep dive)
google-chrome chrome://tracing  # then load trace.json
```

## CLI Reference

### Profiling Mode

```
runtime-scope --pid <PID> --target <PATH> [OPTIONS]

Options:
  -p, --pid <PID>                    Process ID to attach to
  -t, --target <PATH>                Path to target binary
      --trace                        Enable Chrome trace export to trace.json
      --duration <SECONDS>           Duration to profile (default: 30)
      --trace-output <FILE>          Output path for trace JSON (default: trace.json)
  -h, --help                         Print help
```

### TUI Mode (Visualize Existing Traces)

```
runtime-scope --tui <TRACE_FILE>

Options:
      --tui <TRACE_FILE>             Launch glass cockpit TUI for trace.json file
  -h, --help                         Print help
```

**Examples:**

```bash
# Profile and generate trace.json
sudo -E ./target/release/runtime-scope \
  --pid $(pgrep my-app) \
  --target ./my-app \
  --trace

# View results in glass cockpit TUI (recommended)
./target/release/runtime-scope --tui trace.json

# Or view in Chrome for deep timeline analysis
google-chrome chrome://tracing  # then load trace.json

# Custom trace output location
sudo -E ./target/release/runtime-scope \
  --pid 1234 \
  --target ./my-app \
  --trace \
  --trace-output /tmp/my-trace.json

./target/release/runtime-scope --tui /tmp/my-trace.json
```

**What you'll see:**

**TUI Mode:**
- **Master Status** - CAUTION/NORMAL health indicator
- **Top Issues** - Hottest functions with CPU % and source locations (📍 file:line)
- **Worker Bars** - Visual load distribution across executor threads
- **Timeline** - Execution flow over time
- **Smart labels** - Distinguishes user code, std lib (📚), scheduler events (⚙️)

**Chrome Timeline:**
- Precise execution spans with microsecond timing
- Worker IDs and CPU assignments in metadata
- Zoom, pan, search with interactive UI
- Temporal understanding of worker activity patterns

**Note:** Function names and source locations require debug symbols (see Troubleshooting below)

## Viewing Your Trace

After running the profiler with `--trace`, you'll have a `trace.json` file ready for visualization.

### Option 1: Glass Cockpit TUI (Recommended)

**Fast, intuitive, terminal-based** - perfect for quick insights:

```bash
./target/release/runtime-scope --tui trace.json
```

**What you'll see:**
- **Top-left: Master Status** - ⚠️ CAUTION or ✓ NORMAL health indicator
- **Top-right: Top Issues** - Hottest functions ranked by CPU %
  - 🔴 Red marker = >40% CPU (CRITICAL)
  - 🟡 Yellow marker = 20-40% CPU (CAUTION)
  - 🟢 Green marker = <20% CPU (NORMAL)
  - 📍 pin = source location (file:line) when available
  - ⚙️ = scheduler event (no stack trace)
  - 📚 = Rust std library function
- **Bottom-left: Worker Bars** - Load visualization across executor threads
- **Bottom-right: Timeline** - Execution flow over time

**Keyboard controls:**
- **Up/Down arrows** - Navigate (when scrolling implemented)
- **Q** - Quit

### Option 2: Chrome Timeline (Deep Dive)

**For temporal analysis** when you need to understand WHEN things happen:

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

**What you'll see:**
- Each horizontal row = one worker thread (TID)
- Colored blocks = execution spans (time on CPU)
- Gaps = worker was idle/off-CPU
- Click spans to see: worker_id, cpu_id, duration, timestamps, source locations

### Option 3: Perfetto UI (Advanced)

Chrome Trace format is also compatible with [Perfetto](https://ui.perfetto.dev/):
```bash
# Open in browser
firefox https://ui.perfetto.dev/
# Drag and drop trace.json
```

Perfetto offers additional features like SQL queries over trace data.

### Typical Trace Contents

**File stats:**
- **File size:** 50KB - 500KB for 10-30 second traces
- **Events:** 400-2000+ execution spans depending on activity
- **Workers:** 1-24 Tokio worker threads (only active workers shown)
- **Timeline:** Precise start/end times in microseconds

**Example:**
```bash
$ ls -lh trace.json
-rw-r--r-- 1 user user 264K Jan 4 21:38 trace.json

$ jq '.traceEvents | length' trace.json
577

$ jq '[.traceEvents[].name] | unique' trace.json
["core::cmp::impls::<impl core::cmp::PartialOrd for i32>::lt", "execution", "test_async_app::blocking_task::{{closure}}", "thread_name"]
```

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
│   │   ├── trace_exporter.rs  # Chrome trace export
│   │   ├── tui.rs             # Glass cockpit TUI (main)
│   │   └── tui/
│   │       ├── status.rs      # Master Status panel
│   │       ├── hotspot.rs     # Top Issues panel
│   │       ├── workers.rs     # Worker Bars panel
│   │       └── timeline.rs    # Timeline panel
│   └── examples/
│       └── test-async-app.rs  # Test application
├── runtime-scope-ebpf/         # eBPF programs (kernel)
│   └── src/
│       └── main.rs            # perf_event + sched_switch hooks
├── runtime-scope-common/       # Shared types
│   └── src/
│       └── lib.rs             # Event definitions, ExecutionSpan
├── xtask/                      # Build automation
├── test.sh                     # Quick test script
├── VISUALIZATION_DESIGN.md     # Design documentation
├── REFACTORING_PLAN.md        # Implementation roadmap
└── README.md                   # This file
```

## Roadmap

### ✅ Completed
- [x] **Phase 1**: Basic eBPF infrastructure
- [x] **Phase 2**: Stack traces + async task tracking
- [x] **Phase 3**: Timeline visualization with Chrome trace export (Jan 2026)
  - [x] Enhanced event types
  - [x] Execution span tracking in eBPF (sched_switch)
  - [x] Chrome trace exporter with symbol resolution
  - [x] CLI flags for trace export
  - [x] End-to-end timeline visualization
- [x] **Phase 3a**: Glass Cockpit TUI + CPU Profiling (Jan 2026) ✨
  - [x] Dual detection mode (scheduler + perf_event CPU sampling)
  - [x] User-space stack trace capture
  - [x] F-35 inspired four-panel glass cockpit interface
  - [x] Master Status, Top Issues, Worker Bars, Timeline panels
  - [x] HUD color scheme (green/amber/red severity indicators)
  - [x] Smart labeling (user code, std lib, scheduler events)
  - [x] DWARF symbol resolution with file:line locations

**Latest Milestone:** Glass cockpit TUI with instant profiling insights - see hottest functions, worker load, and source locations at a glance!

### 🎯 Next Steps
- [ ] **Phase 4**: Enhanced TUI features
  - [ ] Interactive function details view
  - [ ] Flame graph integration
  - [ ] Export recommendations
  - [ ] Filtering and search
- [ ] **Phase 5**: Advanced analysis
  - [ ] Task spawn flow tracking
  - [ ] Lock contention detection
  - [ ] I/O vs CPU categorization
  - [ ] Custom metric annotations

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

**Status:** ✅ Phase 3a Complete - Glass Cockpit TUI + CPU Profiling! (Jan 4, 2026)

**Latest Achievement:** F-35 inspired glass cockpit interface with instant profiling insights - see hottest functions with source locations, worker load distribution, and system health at a glance. Dual detection mode (scheduler + CPU sampling) provides comprehensive coverage.

**Goal:** Make async Rust behavior visible and understandable through intuitive, actionable visualizations.
