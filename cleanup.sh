#!/bin/bash
echo "🧹 Killing all runtime-scope processes..."
sudo pkill -9 runtime-scope 2>/dev/null
pkill -9 test-async-app 2>/dev/null
sudo pkill -9 test-async-app 2>/dev/null
echo "✓ Cleanup complete"
