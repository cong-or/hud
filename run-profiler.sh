#!/bin/bash
set -e

echo "🔧 Building everything..."
cargo xtask build-ebpf > /dev/null 2>&1
cargo build --example test-async-app > /dev/null 2>&1
cargo build -p runtime-scope > /dev/null 2>&1

echo "✓ Build complete"
echo ""

# Kill any existing test-async-app
pkill -9 test-async-app 2>/dev/null || true
sleep 0.5

echo "🚀 Starting test-async-app in background..."
./target/debug/examples/test-async-app > /tmp/test-async-app.log 2>&1 &
APP_PID=$!

# Wait for it to start
sleep 1

# Verify it's running
if ! kill -0 $APP_PID 2>/dev/null; then
    echo "❌ Failed to start test-async-app"
    echo "Log output:"
    cat /tmp/test-async-app.log
    exit 1
fi

echo "✓ test-async-app running (PID: $APP_PID)"
echo "  Log: /tmp/test-async-app.log"
echo ""

# Verify symbols
echo "🔍 Verifying marker functions..."
if ./check-symbols.sh ./target/debug/examples/test-async-app | grep -q "✓ Found: trace_blocking_start"; then
    echo "✓ Marker functions found"
else
    echo "❌ Marker functions not found!"
    ./check-symbols.sh ./target/debug/examples/test-async-app
    kill $APP_PID 2>/dev/null || true
    exit 1
fi
echo ""

echo "📊 Starting profiler (Ctrl+C to stop)..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo "🧹 Cleaning up..."
    kill $APP_PID 2>/dev/null || true
    echo "✓ Stopped test-async-app"
}

trap cleanup EXIT

# Run the profiler
sudo -E env RUST_LOG=info ./target/debug/runtime-scope \
    --pid $APP_PID \
    --target ./target/debug/examples/test-async-app

echo ""
echo "✓ Profiler stopped"
