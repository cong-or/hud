#!/bin/bash
set -e

echo "🔍 runtime-scope - Chrome Trace Export Test"
echo "=========================================="
echo ""

# Authenticate sudo early
echo "🔐 Requesting sudo access..."
sudo -v
echo ""

# Build everything
echo "📦 Building eBPF program..."
cargo xtask build-ebpf --release

echo ""
echo "📦 Building runtime-scope..."
cargo build --release -p runtime-scope

echo ""
echo "📦 Building test-async-app..."
cargo build --release --example test-async-app

echo ""
echo "🚀 Starting test-async-app..."
./target/release/examples/test-async-app > /tmp/test-async-app.log 2>&1 &
APP_PID=$!
echo "   PID: $APP_PID"
echo "   (App output redirected to /tmp/test-async-app.log)"

# Give the app time to start
sleep 2

echo ""
echo "🔬 Running profiler with trace export (10 seconds)..."
echo "   Output: trace.json"
echo ""

# Refresh sudo credentials (should not prompt now)
sudo -v

sudo -E ./target/release/runtime-scope \
  --pid $APP_PID \
  --target ./target/release/examples/test-async-app \
  --trace \
  --duration 10 \
  --trace-output trace.json \
  --no-live

echo ""
echo "🧹 Cleaning up..."
kill -9 $APP_PID 2>/dev/null || true
sleep 1

echo ""
echo "✅ Test complete!"
echo ""
echo "📊 Trace file info:"
ls -lh trace.json
echo ""
echo "💡 To visualize the trace:"
echo "   1. Open chrome://tracing in Chrome/Chromium"
echo "   2. Click 'Load' and select trace.json"
echo "   3. Use W/A/S/D to zoom/pan the timeline"
echo "   4. Click on execution spans to see details"
echo ""
