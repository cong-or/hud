#!/bin/bash
# Quick test script for runtime-scope

set -e

echo "======================================================================"
echo "  runtime-scope Test Script"
echo "======================================================================"
echo ""

# Build everything
echo "[1/4] Building eBPF program..."
cargo xtask build-ebpf 2>&1 | grep -E "(Compiling|Finished)" | tail -3

echo "[2/4] Building userspace program..."
cargo build --package runtime-scope 2>&1 | grep -E "(Compiling|Finished)" | tail -3

echo "[3/4] Building test app..."
cargo build --example test-async-app 2>&1 | grep -E "(Compiling|Finished)" | tail -3

echo ""
echo "======================================================================"
echo "  Starting test..."
echo "======================================================================"
echo ""

echo "[4/4] Running test app in background..."
./target/debug/examples/test-async-app &
TEST_PID=$!
echo "  Test app PID: $TEST_PID"

# Wait a moment for it to start
sleep 2

echo ""
echo "Now run runtime-scope with:"
echo "  sudo -E ./target/debug/runtime-scope --pid $TEST_PID"
echo ""
echo "Or kill the test app with:"
echo "  kill $TEST_PID"
echo ""
