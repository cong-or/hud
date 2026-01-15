# Development Guide

## Prerequisites

**System:**
- Linux 5.8+
- x86_64 or aarch64
- Root privileges for running

**Toolchains:**
```bash
rustup toolchain install stable
rustup toolchain install nightly --component rust-src
```

**Dependencies:**
```bash
# Fedora/RHEL
sudo dnf install llvm-devel clang libffi-devel

# Ubuntu/Debian
sudo apt install llvm-dev libclang-dev

# BPF linker
cargo install bpf-linker
```

## Building

```bash
# Full build
cargo xtask build-ebpf --release && cargo build --release

# Dev build (eBPF must always be release)
cargo xtask build-ebpf && cargo build

# Run
sudo ./target/release/hud my-app
```

## Testing

```bash
# Unit tests
cargo test --workspace --exclude hud-ebpf

# Integration test
./target/release/examples/demo-server &
sudo ./target/release/hud demo-server --duration 10
pkill demo-server
```

## Code Style

```bash
# Before committing
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --exclude hud-ebpf
```

## eBPF Notes

eBPF programs have restrictions: no heap, no unbounded loops, 512-byte stack, no std.

```bash
# Debug eBPF verifier issues
RUST_LOG=debug cargo xtask build-ebpf 2>&1 | grep -i verif

# View eBPF logs (requires aya_log_ebpf in code)
sudo cat /sys/kernel/debug/tracing/trace_pipe
```

## CI

Runs on push/PR: format check, clippy, build, tests. See `.github/workflows/ci.yml`.

## Troubleshooting

See [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
