# Troubleshooting

## Quick Diagnostic

```bash
uname -r                           # Kernel 5.8+ required
ps -T -p <PID> | grep tokio        # Verify Tokio workers exist
readlink -f /proc/<PID>/exe        # Get actual binary path
```

## No Function Names

Functions show as `<unknown>` or hex addresses.

**Fix:** Add debug symbols to target's `Cargo.toml`:
```toml
[profile.release]
debug = true
force-frame-pointers = true
```

## Permission Denied

**Fix:** Run with sudo:
```bash
sudo -E ./hud --pid <PID> --target <BINARY>
```

## No Events Captured

1. **Not Tokio:** Check for workers: `ps -T -p <PID> | grep tokio-runtime-w`
2. **Idle app:** Generate load
3. **Wrong path:** Use `--target $(readlink -f /proc/<PID>/exe)`

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
sudo -E ./hud --pid <PID> --target <BINARY> --headless --export trace.json
```

## Kernel Too Old

`BPF program verification failed`. Need Linux 5.8+ with BTF and ring buffer support.

```bash
uname -r  # Check version
```

## Debug Mode

```bash
RUST_LOG=debug sudo -E ./hud --pid <PID> --target <BINARY>
```
