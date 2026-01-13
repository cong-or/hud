# Development Guide

Contributing to hud.

## Project Structure

```
hud/
├── hud/              # Main userspace application
│   └── src/
│       ├── main.rs           # Entry point
│       ├── lib.rs            # Library exports
│       ├── profiling/        # eBPF setup & event processing
│       ├── symbolization/    # DWARF symbol resolution
│       ├── tui/              # Terminal UI (ratatui)
│       ├── analysis/         # Hotspot analysis
│       ├── export/           # JSON export (Chrome trace format)
│       └── trace_data/       # Event data structures
│
├── hud-ebpf/         # Kernel-side eBPF programs
│   └── src/
│       └── main.rs           # eBPF hooks (sched_switch, perf_event, uprobes)
│
├── hud-common/       # Shared types between eBPF and userspace
│   └── src/
│       └── lib.rs            # TaskEvent, constants, maps
│
└── xtask/            # Build automation
    └── src/
        └── main.rs           # cargo xtask build-ebpf
```

## Prerequisites

**System Requirements:**
- Linux 5.8+ (eBPF ring buffer support)
- x86_64 or aarch64 architecture
- Root/CAP_BPF privileges for running

**Rust Toolchains:**
```bash
# Stable for userspace
rustup toolchain install stable

# Nightly for eBPF compilation
rustup toolchain install nightly --component rust-src
```

**Dependencies:**
```bash
# Fedora/RHEL
sudo dnf install llvm-devel clang libffi-devel

# Ubuntu/Debian (adjust version to match your LLVM)
sudo apt install llvm-dev libclang-dev

# BPF linker (use matching LLVM version, e.g., llvm-18, llvm-19, llvm-21)
cargo install bpf-linker
```

## Building

### Complete Build

```bash
# Build eBPF programs (requires nightly)
cargo xtask build-ebpf --release

# Build userspace (uses stable)
cargo build --release

# Binary output
./target/release/hud
```

### Development Builds

```bash
# Fast iteration (debug mode)
cargo xtask build-ebpf  # Still release (eBPF requirement)
cargo build             # Debug userspace

# Run tests
cargo test --workspace --exclude hud-ebpf
```

**Note:** eBPF programs must always be built in release mode. Debug builds include formatting code incompatible with BPF linker.

## Testing

### Unit Tests

```bash
# Run all tests (excludes eBPF)
cargo test --workspace --exclude hud-ebpf

# Run specific module tests
cargo test --package hud --lib profiling
cargo test --package hud --lib tui::hotspot
```

### Integration Testing

```bash
# Build demo application
cargo build --release --example demo-server

# Run demo server in background
./target/release/examples/demo-server &
TEST_PID=$!

# Profile it
sudo -E ./target/release/hud \
  --pid $TEST_PID \
  --target ./target/release/examples/demo-server \
  --duration 10

# Cleanup
kill $TEST_PID
```

### Manual Testing Checklist

- [ ] Live TUI mode displays hotspots
- [ ] Drill-down shows timeline
- [ ] Worker filter toggles correctly
- [ ] Search filters hotspots
- [ ] Export creates valid trace.json
- [ ] Replay loads trace.json
- [ ] Headless mode outputs events
- [ ] Symbol resolution shows file:line

## Code Organization

### Key Modules

**hud/src/profiling/**
- `ebpf_setup.rs` - Load and attach eBPF programs
- `event_processor.rs` - Process events from ring buffer
- `worker_discovery.rs` - Find Tokio worker threads
- `mod.rs` - Public API and display functions

**hud/src/symbolization/**
- `symbolizer.rs` - DWARF symbol resolution with caching
- `memory_maps.rs` - Parse /proc/pid/maps for PIE adjustment

**hud/src/tui/**
- `mod.rs` - Main TUI loop and view routing
- `hotspot.rs` - Hotspot list view
- `timeline.rs` - Execution timeline visualization
- `workers.rs` - Worker statistics panel
- `status.rs` - Status bar

**hud-ebpf/src/**
- `main.rs` - All eBPF programs (uprobes, tracepoints, perf events)

### eBPF Development

eBPF code has strict limitations:

**Restrictions:**
- No heap allocation
- No unbounded loops
- Limited stack (512 bytes)
- No floating point
- No standard library

**Verification:**
```bash
# Check BPF verifier output
RUST_LOG=debug cargo xtask build-ebpf 2>&1 | grep -i verif
```

**Debugging:**
```bash
# Enable eBPF logging
# In eBPF code: use aya_log_ebpf::info!()
# In userspace: EbpfLogger::init(&mut bpf)

# View logs
sudo cat /sys/kernel/debug/tracing/trace_pipe
```

## Code Style

**Formatting:**
```bash
cargo fmt --all

# Check without modifying
cargo fmt --all -- --check
```

**Linting:**
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

**Pre-commit:**
```bash
# Run before committing
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --exclude hud-ebpf
```

## CI/CD

GitHub Actions runs on push/PR:

1. Format check (`cargo fmt --check`)
2. Clippy (`cargo clippy -D warnings`)
3. Build eBPF (`cargo xtask build-ebpf`)
4. Build userspace (`cargo build --release`)
5. Tests (`cargo test --workspace --exclude hud-ebpf`)

See `.github/workflows/ci.yml` for details.

## Adding Features

### New eBPF Program

1. Add program to `hud-ebpf/src/main.rs`:
   ```rust
   #[tracepoint]
   pub fn my_hook(ctx: TracePointContext) -> u32 {
       // Implementation
   }
   ```

2. Add map if needed:
   ```rust
   #[map]
   static MY_MAP: HashMap<u32, u64> = HashMap::with_max_entries(1024, 0);
   ```

3. Attach in `hud/src/profiling/ebpf_setup.rs`:
   ```rust
   let program: &mut TracePoint = bpf
       .program_mut("my_hook")?
       .try_into()?;
   program.load()?;
   program.attach("category", "event_name")?;
   ```

4. Process events in `hud/src/profiling/event_processor.rs`

### New TUI View

1. Create module in `hud/src/tui/`:
   ```rust
   pub struct MyView {
       // State
   }

   impl MyView {
       pub fn render(&self, frame: &mut Frame, area: Rect) {
           // Render using ratatui widgets
       }
   }
   ```

2. Add to view router in `hud/src/tui/mod.rs`

3. Add keyboard handler

### New Event Type

1. Define constant in `hud-common/src/lib.rs`:
   ```rust
   pub const MY_EVENT: u32 = 20;
   ```

2. Emit from eBPF in `hud-ebpf/src/main.rs`

3. Handle in `event_processor.rs::process_event()`

## Documentation

**Inline docs:**
```rust
/// Brief description
///
/// Longer explanation with examples.
///
/// # Arguments
/// * `param` - Description
///
/// # Errors
/// Returns error if...
pub fn my_function(param: u32) -> Result<()> {
    // Implementation
}
```

**Generate docs:**
```bash
cargo doc --open --no-deps
```

**Module docs:**
- Add `//!` at top of file for module-level docs
- Include architecture diagrams with ASCII art
- Explain key concepts and tradeoffs

## Performance Profiling

### Profile the Profiler

```bash
# CPU profiling
cargo install samply
samply record ./target/release/hud --pid <PID> --target <BINARY> --duration 10

# Memory profiling
valgrind --tool=massif ./target/release/hud --pid <PID> --target <BINARY> --duration 10
```

### Benchmarks

```bash
# Run benchmarks
cargo bench

# Compare before/after
cargo bench --bench event_processing > before.txt
# Make changes
cargo bench --bench event_processing > after.txt
# Compare
diff before.txt after.txt
```

## Release Process

1. Update version in all `Cargo.toml` files
2. Update CHANGELOG.md
3. Tag release: `git tag -a v0.2.0 -m "Release v0.2.0"`
4. Push: `git push --tags`
5. CI builds and creates GitHub release

## Troubleshooting Development Issues

**eBPF build fails:**
- Ensure nightly toolchain with rust-src
- Verify bpf-linker installed
- Check LLVM version compatibility

**Tests fail:**
- Run `cargo clean`
- Rebuild eBPF: `cargo xtask build-ebpf --release`
- Check kernel version (5.15+)

**Linker errors:**
```bash
# Reinstall bpf-linker
cargo install bpf-linker --features llvm-21 --force
```

## Getting Help

- Open issue on GitHub
- Check existing issues/PRs
- See [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
- Review inline documentation

## License

MIT or Apache-2.0 (dual licensed).

When contributing, you agree to license your contributions under the same terms.
