#!/bin/bash
set -e

echo "🔍 hud - Trace Export Test"
echo "=========================================="
echo ""

# Authenticate sudo early
echo "🔐 Requesting sudo access..."
sudo -v
echo ""

# Build everything with frame pointers for stack traces
echo "📦 Building eBPF program..."
RUSTFLAGS="-C force-frame-pointers=yes" cargo xtask build-ebpf --release

echo ""
echo "📦 Building hud..."
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --release -p hud

echo ""
echo "📦 Building test-async-app (with debug symbols)..."
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --release --example test-async-app

echo ""
echo "🚀 Starting test-async-app..."
./target/release/examples/test-async-app > /tmp/test-async-app.log 2>&1 &
APP_PID=$!
echo "   PID: $APP_PID"
echo "   (App output redirected to /tmp/test-async-app.log)"

# Give the app time to start
sleep 2

echo ""
echo "🔬 Running profiler with CPU sampling + trace export (10 seconds)..."
echo "   Method: perf_event sampling at 99 Hz"
echo "   Output: trace.json"
echo ""

# Refresh sudo credentials (should not prompt now)
sudo -v

sudo -E ./target/release/hud \
  --pid $APP_PID \
  --target ./target/release/examples/test-async-app \
  --export trace.json \
  --duration 10 \
  --headless

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
echo "   ./target/release/hud --replay trace.json"
echo ""
