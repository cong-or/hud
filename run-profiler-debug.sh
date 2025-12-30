#!/bin/bash
set -e

# Build first (before sudo)
echo "🔧 Building..."
cargo build -p runtime-scope 2>&1 | grep -E "(Compiling|Finished|error)" || true
echo "✓ Build complete"

# Now run with sudo
sudo bash -c '
set -e

# Kill any existing test-async-app
pkill -9 test-async-app 2>/dev/null || true
sleep 0.5

echo "🚀 Starting test-async-app..."
./target/debug/examples/test-async-app > /tmp/test-async-app.log 2>&1 &
APP_PID=$!
sleep 1

echo "✓ test-async-app running (PID: $APP_PID)"
echo ""
echo "📊 Starting profiler with DEBUG logging..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

cleanup() {
    echo ""
    echo "🧹 Cleaning up..."
    kill $APP_PID 2>/dev/null || true
}

trap cleanup EXIT INT TERM

# Run with debug logging
RUST_LOG=debug ./target/debug/runtime-scope \
    --pid $APP_PID \
    --target ./target/debug/examples/test-async-app
'
