# Example Applications

## demo-server

An axum HTTP server with intentional blocking for demonstrating hud.

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

### Export and replay

```bash
# Capture to file
sudo ./target/release/hud \
  --pid $(pgrep demo-server) \
  --target ./target/release/examples/demo-server \
  --export trace.json \
  --headless \
  --duration 30

# Replay later
./target/release/hud --replay trace.json
```
