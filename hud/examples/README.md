# Example Applications

Example applications for testing and demonstrating hud.

## demo-server

**Before/after demo for videos.** An axum HTTP server with intentional blocking that you can fix and re-profile.

### The scenario

1. `process_bad` - CPU-bound work blocking async workers (the problem)
2. `process_good` - Same work offloaded to `spawn_blocking` (the fix)

### Running the demo

**Terminal 1: Start the server**
```bash
cargo build --release --example demo-server
./target/release/examples/demo-server
```

**Terminal 2: Profile with hud**
```bash
sudo ./target/release/hud --pid $(pgrep demo-server) --target ./target/release/examples/demo-server
```

**Terminal 3: Generate load**
```bash
hey -n 1000 -c 20 -m POST -H "Content-Type: application/json" \
    -d '{"data":"hello"}' http://localhost:3000/process
```

### What you'll see

With `process_bad`: Workers at high utilization, `process_bad` showing as a hotspot.

To fix: Edit `demo-server.rs`, swap the route to use `process_good`, rebuild and restart.

With `process_good`: Workers stay free, blocking work offloaded to threadpool.

---

## test-async-app

A comprehensive test application designed to exercise hud's profiling features.

### What it does

- **10 well-behaved async tasks**: Lots of `await` calls, minimal CPU work
- **1 blocking task**: Does 450ms of CPU work without yielding (creates hot function for profiling)
- **200+ quick tasks**: Spawned in bursts to create varied activity patterns
- **Runs continuously** until stopped

### Running the test

**Quick start (recommended):**

```bash
cd /home/soze/hud
./test.sh
```

This will:
1. Build everything (eBPF + userspace + test app)
2. Start test-async-app in the background
3. Profile it and generate `trace.json`
4. Press **Ctrl+C** when you want to stop

Then view the results:

```bash
# Glass Cockpit TUI (recommended for instant insights)
./target/release/hud --tui trace.json

# Or use a trace viewer (for deep temporal analysis)
# Visit https://ui.perfetto.dev or https://speedscope.app and load trace.json
```

**Manual mode:**

**Terminal 1: Start the test app**
```bash
cd /home/soze/hud
cargo build --release --example test-async-app
./target/release/examples/test-async-app
```

You'll see:
- Tasks starting and running smoothly
- Every ~1 second: blocking task does CPU work
- Activity continues with varied patterns

**Terminal 2: Profile with hud**
```bash
cd /home/soze/hud

# Build everything first
cargo xtask build-ebpf --release
cargo build --release -p hud

# Run the profiler (requires sudo for eBPF)
sudo -E ./target/release/hud \
  --pid $(pgrep test-async-app) \
  --target ./target/release/examples/test-async-app \
  --trace

# View in glass cockpit TUI
./target/release/hud --tui trace.json
```

### Expected Output

**During profiling:**
```
🔍 hud
📊 Profiling PID 12345 for 30 seconds...
⏱️  Collecting events... [████████░░] 20s
```

**In the Glass Cockpit TUI:**

```
┌─────────────────────┬──────────────────────────────────────┐
│   MASTER STATUS     │         TOP ISSUES                   │
│                     │                                      │
│   ⚠️  CAUTION       │  🟡 test_async_app::blocking_task... │
│                     │     48.5% CPU                        │
│   Busiest: W12      │     📍 test-async-app.rs:156         │
│   66.2% active      │                                      │
│                     │  🟡 core::cmp::impls::<impl core::...│
│                     │     43.1% CPU                        │
│                     │     📍 cmp.rs:1852                   │
├─────────────────────┼──────────────────────────────────────┤
│  EXECUTOR THREADS   │         TIMELINE                     │
│                     │                                      │
│  W0   ██░░░░░░░░ 25%│  Time series visualization showing   │
│  W1   ████░░░░░ 40% │  execution activity across workers   │
│  W12  ████████░ 66%⚠│  over the profiling duration         │
│                     │                                      │
└─────────────────────┴──────────────────────────────────────┘
```

**What you're seeing:**
- ✅ **Master Status** - System health (CAUTION/NORMAL), busiest worker
- ✅ **Top Issues** - Hottest functions with CPU % and source locations
- ✅ **Worker Bars** - Load distribution across executor threads
- ✅ **Timeline** - Execution flow visualization
- ✅ **Smart labels** - Distinguishes user code (📍), std lib (📚), scheduler events (⚙️)
- ✅ **HUD colors** - Red (>40%), amber (20-40%), green (<20%)
- ✅ **File:line locations** - DWARF symbol resolution working

### Use cases

1. ✅ **Testing Glass Cockpit TUI**: Verify four-panel layout and HUD colors
2. ✅ **Testing CPU profiling**: perf_event sampling captures hot functions
3. ✅ **Testing symbol resolution**: DWARF debug info → function names + file:line
4. ✅ **Testing hotspot detection**: Top Issues panel shows CPU % distribution
5. ✅ **Testing worker visualization**: Worker Bars show load across executor threads
6. ✅ **Testing timeline export**: Trace Event format for temporal analysis
7. ✅ **Testing smart labeling**: Distinguishes user code, std lib, scheduler events
8. ✅ **Performance baseline**: eBPF overhead is negligible (<5% CPU)

### Technical Details

**CPU Profiling via perf_event:**

hud uses **perf_event** for CPU sampling - no code instrumentation needed! The profiler:
1. Attaches perf_event to all Tokio worker threads
2. Samples at 99 Hz to capture what's executing
3. Captures user-space stack traces on each sample
4. Resolves symbols via DWARF debug info

**Blocking Behavior (for demonstration):**

The test app intentionally does CPU-bound work to create hot functions:
```rust
// This intentionally blocks the executor for ~450ms
let start = std::time::Instant::now();
let mut result = 0u64;
while start.elapsed() < Duration::from_millis(450) {
    result = result.wrapping_add((0..10000).sum::<u64>());
}
```

This simulates CPU-bound work that doesn't yield to the executor, making it easy to spot in the profiler.

## test-single-thread

Single-threaded test app showing worst-case blocking scenario.

**Key difference:** Uses `#[tokio::main(flavor = "current_thread")]`
- Only ONE executor thread
- Blocking completely freezes ALL tasks
- More dramatic impact than multi-threaded version

**Use this to demonstrate:**
- How blocking is catastrophic on single-threaded runtimes
- The difference between single vs multi-threaded executors
- Why `spawn_blocking()` is essential for CPU work
