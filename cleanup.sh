#!/bin/bash
echo "🧹 Killing all hud processes..."
sudo pkill -9 hud 2>/dev/null
pkill -9 test-async-app 2>/dev/null
sudo pkill -9 test-async-app 2>/dev/null
echo "✓ Cleanup complete"
