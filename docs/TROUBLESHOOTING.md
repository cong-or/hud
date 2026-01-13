# Troubleshooting

Common issues and solutions for hud.

## No Function Names Displayed

**Symptom:** You see timings but function names show as `<unknown>` or hex addresses.

**Cause:** Missing debug symbols in the target binary.

**Solution:**
```toml
# Add to target's Cargo.toml
[profile.release]
debug = true
force-frame-pointers = true
```

Rebuild the target application and profile again.

## Permission Denied

**Symptom:** `Error: Permission denied` when attaching eBPF programs.

**Cause:** eBPF requires root or `CAP_BPF` capability.

**Solutions:**

1. Use sudo with `-E` flag (recommended):
   ```bash
   sudo -E ./hud --pid <PID> --target <BINARY>
   ```
   The `-E` preserves environment variables like `RUST_LOG`.

2. Grant CAP_BPF capability (persistent):
   ```bash
   sudo setcap cap_bpf+ep ./target/release/hud
   ./hud --pid <PID> --target <BINARY>
   ```

## No Events Captured

**Symptom:** Profiler starts but shows "Waiting for events..." indefinitely.

**Possible Causes:**

### 1. Not a Tokio Application
hud only profiles Tokio async applications.

**Check:**
```bash
# Look for tokio-runtime-w threads
ps -T -p <PID> | grep tokio-runtime-w
```

If no output, the target isn't using Tokio or has no active workers.

### 2. Workers Are Idle
If the application isn't processing work, no events fire.

**Solution:** Generate load or wait for activity.

### 3. Wrong Binary Path
The `--target` path must match the actual binary for symbol resolution.

**Check:**
```bash
# Get actual binary path
readlink -f /proc/<PID>/exe

# Use that path
sudo -E ./hud --pid <PID> --target $(readlink -f /proc/<PID>/exe)
```

## eBPF Build Failures

**Symptom:** `cargo xtask build-ebpf` fails with linker errors.

### Missing bpf-linker

**Error:** `error: linker 'rust-lld' not found`

**Solution:**
```bash
cargo install bpf-linker --features llvm-21
```

### Wrong LLVM Version

**Error:** `undefined symbol: LLVMGetStackMapSlotCount`

**Solution:** Ensure LLVM is installed and bpf-linker uses a compatible version:
```bash
# Fedora/RHEL
sudo dnf install llvm-devel clang

# Ubuntu/Debian (adjust version as needed)
sudo apt install llvm-dev libclang-dev

# Reinstall bpf-linker
cargo install bpf-linker --force
```

### Rust Nightly Issues

**Error:** `error[E0463]: can't find crate for 'core'`

**Solution:** Install rust-src for nightly:
```bash
rustup toolchain install nightly --component rust-src
```

## Target Binary Not Found

**Symptom:** `Error: Failed to read binary file`

**Cause:** The `--target` path is incorrect or file doesn't exist.

**Solution:**
```bash
# Use absolute path
sudo -E ./hud --pid <PID> --target /full/path/to/binary

# Or resolve relative paths
sudo -E ./hud --pid <PID> --target $(realpath ./my-app)
```

## Stack Traces Missing or Incomplete

**Symptom:** Only 1-2 stack frames shown instead of full call chain.

**Possible Causes:**

### 1. Missing Frame Pointers
**Solution:** Add to target's `Cargo.toml`:
```toml
[profile.release]
force-frame-pointers = true
```

### 2. Optimized Code
Aggressive optimizations inline functions, reducing stack depth.

**Solution:** Accept this tradeoff or reduce optimization level:
```toml
[profile.release]
opt-level = 2  # Instead of 3
```

## TUI Rendering Issues

**Symptom:** Garbled output or broken layout.

**Cause:** Terminal doesn't support required features.

**Solutions:**

1. Use modern terminal (alacritty, kitty, wezterm, iTerm2)
2. Set `TERM` environment variable:
   ```bash
   export TERM=xterm-256color
   sudo -E ./hud --pid <PID> --target <BINARY>
   ```
3. Use headless mode if TUI is problematic:
   ```bash
   sudo -E ./hud --pid <PID> --target <BINARY> --headless --export trace.json
   ```

## High CPU Usage from Profiler

**Symptom:** `hud` process consuming significant CPU.

**Cause:** Very high event rate overwhelming the profiler.

**Solutions:**

1. Use headless mode (lower overhead):
   ```bash
   sudo -E ./hud --pid <PID> --target <BINARY> --headless --export trace.json
   ```

2. Reduce profiling duration:
   ```bash
   sudo -E ./hud --pid <PID> --target <BINARY> --duration 10
   ```

3. The CPU sampling rate is fixed at 99 Hz, which is already low-overhead.

## Replay Mode Issues

**Symptom:** `Error: Failed to parse trace file` when using `--replay`.

**Cause:** Corrupted or incompatible trace.json format.

**Solutions:**

1. Ensure trace.json is valid JSON:
   ```bash
   jq . trace.json > /dev/null && echo "Valid" || echo "Invalid"
   ```

2. Re-export trace from live session:
   ```bash
   sudo -E ./hud --pid <PID> --target <BINARY> --export trace.json --duration 30
   ```

## Kernel Version Issues

**Symptom:** `Error: BPF program verification failed`

**Cause:** Kernel too old (< 5.15) or missing BPF features.

**Check kernel version:**
```bash
uname -r
```

**Solution:** Upgrade to Linux 5.8 or newer. eBPF features required:
- BTF (BPF Type Format)
- Ring buffers
- Stack trace support

## Still Having Issues?

1. Enable debug logging:
   ```bash
   RUST_LOG=debug sudo -E ./hud --pid <PID> --target <BINARY>
   ```

2. Check system compatibility:
   ```bash
   # Verify BPF is enabled
   cat /boot/config-$(uname -r) | grep CONFIG_BPF

   # Check available tracers
   cat /sys/kernel/debug/tracing/available_tracers
   ```

3. Open an issue with:
   - OS/kernel version
   - Full error output
   - RUST_LOG=debug output if possible
