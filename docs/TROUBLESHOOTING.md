# Troubleshooting

## Quick Diagnostic

```bash
uname -r                           # Kernel 5.8+ required
ps -T -p <PID> | grep tokio        # Verify Tokio workers exist
readlink -f /proc/<PID>/exe        # Get actual binary path
```

## No Function Names / Low Debug %

Functions show as `<unknown>` or hex addresses. The **Debug %** indicator in the status panel shows amber (below 50%).

### What's happening

hud uses DWARF debug symbols to translate memory addresses into function names and source locations. Without debug symbols:
- Function names fall back to prefix-based guessing (`tokio::`, `std::`, etc.)
- Source file and line numbers are unavailable
- Frames show ⚠ in the drilldown view

### Fix

Add debug symbols to target's `Cargo.toml`:
```toml
[profile.release]
debug = true
force-frame-pointers = true
```

Then rebuild your application. The Debug % should rise to 80-100%.

### Understanding the indicators

| Indicator | Meaning |
|-----------|---------|
| **Debug 100%** (green) | All frames have debug info - reliable classification |
| **Debug <50%** (amber) | Most frames lack debug info - rebuild with `debug = true` |
| **⚠ marker** | This specific frame is missing debug info |

### Still seeing low Debug %?

- Binary was stripped: Don't run `strip` on the binary
- Wrong binary path: Use `--target /path/to/binary` to specify the exact binary with symbols
- Shared libraries: System libraries won't have debug info (expected)

## Permission Denied

**Fix:** Run with sudo:
```bash
sudo ./hud my-app
```

## No Events Captured

1. **Not Tokio:** Check for workers: `ps -T -p <PID> | grep tokio-runtime-w`
2. **Idle app:** Generate load
3. **Multiple matches:** Use explicit PID: `hud --pid <PID>`

## eBPF Build Failures

**Missing bpf-linker:**
```bash
cargo install bpf-linker
```

**Missing rust-src:**
```bash
rustup toolchain install nightly --component rust-src
```

**LLVM issues:**
```bash
# Install LLVM (Fedora)
sudo dnf install llvm-devel clang

# Install LLVM (Ubuntu/Debian)
sudo apt install llvm-dev libclang-dev

# Reinstall bpf-linker
cargo install bpf-linker --force
```

## Incomplete Stack Traces

Only 1-2 frames showing. **Fix:** Add to target's `Cargo.toml`:
```toml
[profile.release]
force-frame-pointers = true
```

## TUI Issues

Garbled output. **Fix:** Use modern terminal or headless mode:
```bash
sudo ./hud my-app --headless --export trace.json
```

## Kernel Too Old

`BPF program verification failed`. Need Linux 5.8+ with BTF and ring buffer support.

```bash
uname -r  # Check version
```

## Debug Mode

```bash
RUST_LOG=debug sudo ./hud my-app
```
