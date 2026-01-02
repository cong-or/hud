#!/bin/bash
# Enable stack trace collection for eBPF
#
# This temporarily lowers the perf_event_paranoid setting to allow
# eBPF programs to capture user stack traces.
#
# Current value: 2 (very restrictive)
# New value: -1 (permissive, allows all perf events)
#
# Note: This change is temporary and will reset on reboot.
# To make it permanent, add this to /etc/sysctl.conf:
#   kernel.perf_event_paranoid = -1

echo "Current perf_event_paranoid: $(cat /proc/sys/kernel/perf_event_paranoid)"
echo "Setting to -1 to enable stack trace collection..."
sudo sysctl -w kernel.perf_event_paranoid=-1
echo "New value: $(cat /proc/sys/kernel/perf_event_paranoid)"
echo ""
echo "✓ Stack traces enabled! This setting will reset on reboot."
echo ""
echo "To make this permanent, add to /etc/sysctl.conf:"
echo "  kernel.perf_event_paranoid = -1"
