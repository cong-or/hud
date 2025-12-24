# Test Async Applications

Example applications for testing and demonstrating runtime-scope.

## test-async-app

A comprehensive test application with intentional good and bad async behavior.

### What it does

- **10 well-behaved tasks**: Lots of `await` calls, minimal CPU work
- **1 blocking task**: Does 450ms of CPU work without yielding (every second)
- **200+ quick tasks**: Spawned in bursts throughout the run
- **Runs for ~40 seconds** total
- **Marker functions**: Instrumented with `trace_blocking_start/end` for eBPF hooking

### Running the test

**Terminal 1: Start the test app**
```bash
cd /home/soze/runtime-scope
cargo run --example test-async-app
```

You'll see:
- Tasks starting and running smoothly
- Every ~1 second: blocking task announces it's doing 450ms CPU work
- Other tasks continue (but may be delayed during blocking)

**Terminal 2: Profile it with runtime-scope** ✅ WORKING!
```bash
cd /home/soze/runtime-scope

# Build everything first
cargo xtask build-ebpf
cargo build --package runtime-scope

# Run the profiler (requires sudo for eBPF)
sudo -E ./target/debug/runtime-scope \
  --pid $(pgrep test-async-app) \
  --target ./target/debug/examples/test-async-app
```

### Expected Output

```
🔍 runtime-scope v0.1.0
   Real-time async runtime profiler

📦 Target: /home/soze/runtime-scope/target/debug/examples/test-async-app
📊 Monitoring PID: 23646

👀 Watching for blocking events... (press Ctrl+C to stop)

🔴 [PID 23646 TID 23648] Blocking started at 5610682ms
  ✓ [PID 23646 TID 23648] Blocking ended - Duration: 450.01ms ⚠️  SLOW!
🔴 [PID 23646 TID 23648] Blocking started at 5612134ms
  ✓ [PID 23646 TID 23648] Blocking ended - Duration: 450.04ms ⚠️  SLOW!
🔴 [PID 23646 TID 23648] Blocking started at 5613585ms
  ✓ [PID 23646 TID 23648] Blocking ended - Duration: 450.01ms ⚠️  SLOW!
```

**What you're seeing:**
- ✅ Real-time detection of each blocking event
- ✅ Accurate duration measurement (~450ms)
- ✅ Process ID (PID) and Thread ID (TID)
- ✅ Automatic flagging of operations >10ms

### Use cases

1. ✅ **Testing basic tracing**: Verify eBPF uprobes attach correctly
2. ✅ **Testing blocking detection**: Detect the 450ms blocking operations
3. ✅ **Testing duration calculation**: Measure time between start/end events
4. 🚧 **Testing stack traces**: (Coming in Phase 2)
5. 🚧 **Testing cascade visualization**: (Coming in Phase 4)
6. ✅ **Performance baseline**: eBPF overhead is negligible

### Technical Details

**Marker Functions:**
The app uses `#[no_mangle]` marker functions to make hooking easy:
```rust
#[no_mangle]
#[inline(never)]
fn trace_blocking_start() { }

#[no_mangle]
#[inline(never)]
fn trace_blocking_end() { }
```

These are called before/after the blocking CPU work. In production, we'll use scheduler tracepoints instead (no code changes needed).

**Blocking Behavior:**
```rust
// This intentionally blocks the executor for ~450ms
let start = std::time::Instant::now();
let mut result = 0u64;
while start.elapsed() < Duration::from_millis(450) {
    result = result.wrapping_add((0..10000).sum::<u64>());
}
```

This simulates CPU-bound work that doesn't yield to the executor.

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
