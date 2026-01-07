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
# Using curl (no extra install)
for i in {1..500}; do curl -s -X POST -H "Content-Type: application/json" -d '{"data":"hello"}' http://localhost:3000/process > /dev/null; done

# Or with hey (if installed)
hey -n 1000 -c 20 -m POST -H "Content-Type: application/json" -d '{"data":"hello"}' http://localhost:3000/process
```

### What you'll see

With `process_bad`: Workers at high utilization, `process_bad` showing as a hotspot.

To fix: Edit `demo-server.rs`, swap the route to use `process_good`, rebuild and restart.

With `process_good`: Workers stay free, blocking work offloaded to threadpool.

---

## test-async-app

A self-contained test application that generates its own blocking workload.

### What it does

- **10 well-behaved async tasks**: Lots of `await` calls, minimal CPU work
- **1 blocking task**: Does 500ms of CPU work without yielding
- **200+ quick tasks**: Spawned in bursts to create varied activity
- **Runs continuously** - no separate load generator needed

### Running

**Terminal 1: Start the app**
```bash
cargo build --release --example test-async-app
./target/release/examples/test-async-app
```

**Terminal 2: Profile with hud**
```bash
sudo ./target/release/hud --pid $(pgrep test-async-app) --target ./target/release/examples/test-async-app
```

No need to generate load - the app creates its own blocking work every second.

### Export and replay

```bash
# Capture to file
sudo ./target/release/hud \
  --pid $(pgrep test-async-app) \
  --target ./target/release/examples/test-async-app \
  --export trace.json \
  --duration 30

# Replay later
./target/release/hud --replay trace.json
```

---

## test-single-thread

Single-threaded variant showing worst-case blocking.

Uses `#[tokio::main(flavor = "current_thread")]` - only one executor thread, so blocking freezes everything.

```bash
cargo build --release --example test-single-thread
./target/release/examples/test-single-thread
```

Good for demonstrating why `spawn_blocking()` matters.
