# Troubleshooting

## Quick Diagnostic

```bash
uname -r                           # Kernel 5.8+ required
ps -T -p <PID>                     # List all threads (look for worker pool)
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

## Workers: 0

If hud prints `workers: 0`, it couldn't find Tokio worker threads. No events will be captured.

**Step 1: Check what threads exist**
```bash
ps -T -p <PID>
```

Default Tokio threads are named `tokio-runtime-w`. Custom runtimes using `thread_name("my-pool")` produce `my-pool-0`, `my-pool-1`, etc.

**Step 2: Understand the discovery chain**

hud tries four methods in order:
1. Default prefix `tokio-runtime-w`
2. Stack-based classification (samples stack traces for 500ms, looks for Tokio scheduler frames)
3. Largest thread group heuristic (`{name}-{N}` patterns)

If all three fail, you see `workers: 0`. Stack-based discovery handles most custom thread names automatically, but requires the target process to be actively running Tokio work during the 500ms sampling window.

**Step 3: Override with --workers**
```bash
sudo hud my-app --workers my-pool
```

Pass the prefix that matches your worker threads (everything before the `-N` suffix).

**Step 4: Debug with RUST_LOG**
```bash
RUST_LOG=info sudo hud my-app
```

This shows each discovery step and which threads were found. Look for lines like `Default prefix found no workers, trying stack-based discovery...` to see where discovery stalled.

## No Events Captured

If `workers: N` looks correct but you still see zero events:

1. **Idle app:** Generate load — hud only captures events when workers are active
2. **Multiple matches:** Use explicit PID: `hud --pid <PID>`
3. **Threshold too high:** Try `--threshold 1` to catch shorter blocks

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
