#!/bin/bash
set -e

echo "🧪 Phase 3a: Dual Detection Mode Test"
echo ""

# Build
echo "🔧 Building..."
cargo xtask build-ebpf --release > /dev/null 2>&1
cargo build --release -p hud > /dev/null 2>&1
cargo build --release --example test-async-app > /dev/null 2>&1
echo "✓ Build complete"
echo ""

# Start test app (output to log file)
echo "🚀 Starting test app..."
./target/release/examples/test-async-app > /tmp/test-app.log 2>&1 &
TEST_PID=$!
sleep 2
echo "✓ Test app running (PID: $TEST_PID)"
echo ""

# Run profiler (you'll see output here)
echo "📊 Starting profiler... (Press Ctrl+C to stop)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

sudo -E ./target/release/hud \
  --pid $TEST_PID \
  --target ./target/release/examples/test-async-app \
  --duration 30

# Cleanup
echo ""
echo "🧹 Cleaning up..."
kill $TEST_PID 2>/dev/null || true
pkill -9 test-async-app 2>/dev/null || true
echo "✓ Done"
