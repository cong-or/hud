#!/bin/bash
# Diagnostic script to verify marker functions are present in the binary

BINARY="${1:-./target/debug/examples/test-async-app}"

echo "🔍 Checking for marker functions in: $BINARY"
echo ""

echo "1. Checking if binary exists and has symbols:"
if [ ! -f "$BINARY" ]; then
    echo "   ❌ Binary not found: $BINARY"
    exit 1
fi

if ! command -v nm &> /dev/null; then
    echo "   ⚠️  'nm' command not found, install binutils"
    exit 1
fi

echo "   ✓ Binary exists"
echo ""

echo "2. Looking for marker functions:"
for func in trace_blocking_start trace_blocking_end trace_task_start trace_task_end; do
    if nm "$BINARY" | grep -q " T $func$"; then
        addr=$(nm "$BINARY" | grep " T $func$" | awk '{print $1}')
        echo "   ✓ Found: $func at 0x$addr"
    else
        echo "   ❌ Missing: $func"
    fi
done
echo ""

echo "3. Checking if binary is PIE (Position Independent Executable):"
if readelf -h "$BINARY" 2>/dev/null | grep -q "Type:.*DYN"; then
    echo "   ✓ Binary is PIE (addresses will need adjustment)"
else
    echo "   ✓ Binary is not PIE (addresses can be used directly)"
fi
echo ""

echo "4. Checking DWARF debug info:"
if readelf -S "$BINARY" 2>/dev/null | grep -q "\.debug_"; then
    echo "   ✓ Binary has DWARF debug information"
else
    echo "   ⚠️  No DWARF debug info found (symbol resolution won't work)"
    echo "      Make sure you're using a debug build: cargo build (not --release)"
fi
echo ""

if [ -n "$2" ]; then
    PID="$2"
    echo "5. Checking process $PID memory maps:"
    if [ -f "/proc/$PID/maps" ]; then
        echo "   Binary mappings:"
        grep "$(basename "$BINARY")" "/proc/$PID/maps" | head -5
    else
        echo "   ❌ Process $PID not found"
    fi
fi
