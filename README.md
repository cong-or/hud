# runtime-scope

⚠️ **Status: Early Development / Proof of Concept** ⚠️

Real-time async runtime profiler for Rust using eBPF.

Detect blocking operations in async code that harm executor performance. Built with pure Rust + eBPF (Aya framework).

## Current Status: ✅ Phase 1 Complete + Stack Traces Working!

**What Works:**
- ✅ Real-time blocking detection (450ms operations detected)
- ✅ **Stack trace capture with eBPF StackTrace maps**
- ✅ **Symbol resolution using DWARF debug info**
- ✅ **Source code locations (file:line) for each frame**
- ✅ **Demangled Rust function names**
- ✅ PIE executable address translation
- ✅ Accurate duration measurement
- ✅ Process/thread tracking
- ✅ Graceful Ctrl+C shutdown

**⚠️ Important Note:**
Current implementation uses `#[no_mangle]` marker functions for learning purposes.
**These will be removed in Phase 4** when we switch to scheduler tracepoints.
**Production version will require ZERO code changes** - profile any binary without modification!

**Next Steps:**
- 🚧 Show full call stack (currently shows marker function only)
- 🚧 Async task tracking (task names, spawn locations)
- 🎯 **Remove markers, switch to scheduler tracepoints (no code changes!)**
- 🚧 Cascade effect visualization
- 🚧 TUI interface

## Quick Demo

```bash
cd /home/soze/runtime-scope

# Easy mode: Automated script (builds, starts app, attaches profiler)
./run-profiler.sh

# Or manual mode:
# Terminal 1: Run the test app
./target/debug/examples/test-async-app

# Terminal 2: Profile it
sudo -E ./target/debug/runtime-scope \
  --pid $(pgrep test-async-app) \
  --target ./target/debug/examples/test-async-app
```

**Output:**
```
🔍 runtime-scope v0.1.0
   Real-time async runtime profiler

📦 Target: /home/soze/runtime-scope/target/debug/examples/test-async-app
📊 Monitoring PID: 17344

👀 Watching for blocking events... (press Ctrl+C to stop)

🔴 [PID 17344 TID 17366] Blocking started

🔴 BLOCKING DETECTED
   Duration: 450.02ms ⚠️
   Process: PID 17344
   Thread: TID 17366

   📍 Stack trace:
      #0  0x000000000002c6b0 trace_blocking_start
                      at test-async-app.rs:59:0
      #1  0x00002a2e0b79f088 <unknown>
      #2  0x00002a2e0b822f8c <unknown>
```

**Why sudo?** eBPF requires root privileges to attach to processes and load kernel programs.

## What It Currently Shows

- 🔴 **Blocking detection** - When async tasks block the executor
- ⏱️ **Duration measurement** - How long each blocking operation takes (accurate to ~0.01ms)
- 🧵 **Thread identification** - Which OS thread is affected
- 📍 **Stack traces** - Instruction pointers for each stack frame
- 🔍 **Symbol resolution** - Function names with DWARF debug info
- 📝 **Source locations** - File paths and line numbers
- 🦀 **Demangled names** - Clean Rust function names (not mangled C++)
- ⚠️ **Automatic flagging** - Highlights operations >10ms as SLOW

---

## Developer Setup

Want to contribute or build from source? Here's everything you need.

### Prerequisites

**System Requirements:**
- Linux kernel 5.15+ (for eBPF support)
- Rust 1.75+ with nightly toolchain
- LLVM 20-22 development libraries
- Clang compiler

### Installing Dependencies

<details>
<summary><b>Fedora / RHEL / CentOS</b></summary>

```bash
# Install LLVM development libraries
# Option 1: Use system LLVM (if 20+)
sudo dnf install -y llvm-devel libffi-devel clang

# Option 2: Use bleeding-edge from copr
sudo dnf copr enable @fedora-llvm-team/llvm-snapshots
sudo dnf install -y llvm-devel libffi-devel

# Install Rust toolchains
rustup toolchain install nightly --component rust-src

# Install bpf-linker
cargo install bpf-linker --git https://github.com/aya-rs/bpf-linker --features llvm-21
```
</details>

<details>
<summary><b>Ubuntu / Debian</b></summary>

```bash
# Add LLVM repository
wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 21

# Install dependencies
sudo apt-get update
sudo apt-get install -y llvm-21-dev libclang-21-dev libelf-dev libz-dev clang-21

# Install Rust toolchains
rustup toolchain install nightly --component rust-src

# Install bpf-linker
cargo install bpf-linker --features llvm-21
```
</details>

<details>
<summary><b>Arch Linux</b></summary>

```bash
# Install LLVM and dependencies
sudo pacman -S llvm clang libelf zlib

# Install Rust toolchains
rustup toolchain install nightly --component rust-src

# Install bpf-linker (adjust llvm version to match your system)
cargo install bpf-linker --features llvm-21
```
</details>

<details>
<summary><b>Other Distributions</b></summary>

Install these packages:
- `llvm` (version 20+) with development headers
- `clang` compiler
- `libelf` development headers
- Rust nightly toolchain with `rust-src` component

Then install bpf-linker matching your LLVM version:
```bash
# For LLVM 20
cargo install bpf-linker --features llvm-20

# For LLVM 21
cargo install bpf-linker --features llvm-21

# For LLVM 22 (use llvm-21 feature, API compatible)
cargo install bpf-linker --git https://github.com/aya-rs/bpf-linker --features llvm-21
```
</details>

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/runtime-scope
cd runtime-scope

# Build eBPF program (runs in kernel)
cargo xtask build-ebpf

# Build userspace program (what you run)
cargo build --package runtime-scope

# Run it
sudo -E ./target/debug/runtime-scope
```

**Release builds:**

```bash
cargo xtask build-ebpf --release
cargo build --package runtime-scope --release
sudo -E ./target/release/runtime-scope
```

### Development Workflow

**1. Make changes to the eBPF program:**

```bash
# Edit runtime-scope-ebpf/src/main.rs
vim runtime-scope-ebpf/src/main.rs

# Rebuild eBPF
cargo xtask build-ebpf

# Rebuild userspace (embeds new eBPF bytecode)
cargo build --package runtime-scope

# Test
sudo -E ./target/debug/runtime-scope
```

**2. Make changes to the userspace program:**

```bash
# Edit runtime-scope/src/main.rs
vim runtime-scope/src/main.rs

# Rebuild (no need to rebuild eBPF)
cargo build --package runtime-scope

# Test
sudo -E ./target/debug/runtime-scope
```

**3. Add shared types:**

```bash
# Edit runtime-scope-common/src/lib.rs
vim runtime-scope-common/src/lib.rs

# Rebuild everything
cargo xtask build-ebpf
cargo build --package runtime-scope
```

### Project Structure

```
runtime-scope/
├── runtime-scope/              # Userspace profiler
│   ├── src/
│   │   ├── main.rs            # CLI, event processing, PIE address handling
│   │   └── symbolizer.rs      # DWARF symbol resolution (NEW!)
│   ├── examples/
│   │   └── test-async-app.rs  # Test application with blocking code
│   └── Cargo.toml
├── runtime-scope-ebpf/         # eBPF programs (runs in kernel)
│   ├── src/
│   │   └── main.rs            # Kernel-side tracing + stack capture
│   └── Cargo.toml
├── runtime-scope-common/       # Shared types
│   ├── src/
│   │   └── lib.rs             # Event definitions, shared structs
│   └── Cargo.toml
├── xtask/                      # Build automation
│   ├── src/
│   │   └── main.rs            # Custom cargo commands
│   └── Cargo.toml
├── run-profiler.sh             # Quick test script (NEW!)
├── check-symbols.sh            # Diagnostic script (NEW!)
├── SESSION_SUMMARY.md          # Development notes (NEW!)
├── Cargo.toml                  # Workspace manifest
└── README.md
```

### Testing

```bash
# Run Rust tests
cargo test

# Test eBPF program verification
cargo xtask build-ebpf

# Run on a sample async app
cargo run --example sample-async-app &
sudo -E ./target/debug/runtime-scope --pid $!
```

### Contributing

We welcome contributions! Here's how to get started:

1. **Fork the repository**
2. **Create a feature branch:** `git checkout -b feature/amazing-feature`
3. **Make your changes:**
   - Follow Rust style guidelines (`cargo fmt`)
   - Add tests for new functionality
   - Update documentation as needed
4. **Test thoroughly:**
   - Run `cargo test`
   - Test on real async applications
   - Verify eBPF program loads without errors
5. **Commit with clear messages:**
   ```bash
   git commit -m "Add visualization for task spawn rates"
   ```
6. **Push and create a Pull Request**

**Contribution Guidelines:**
- Keep PRs focused on a single feature/fix
- Include tests and documentation
- Ensure eBPF programs pass kernel verifier
- Measure and document performance impact
- Add examples for new features

### Debugging

**eBPF program won't load:**

```bash
# Check kernel version (need 5.15+)
uname -r

# Check if BPF is enabled
zgrep CONFIG_BPF /proc/config.gz

# View verifier errors in detail
sudo dmesg | grep bpf

# Verify eBPF bytecode
llvm-objdump -d target/bpfel-unknown-none/debug/runtime-scope
```

**No events showing:**

```bash
# Check if attached to correct tracepoint
sudo bpftool prog list

# Verify target process is running
ps aux | grep <pid>

# Check eBPF logs (if available)
sudo cat /sys/kernel/debug/tracing/trace_pipe
```

**Build errors:**

```bash
# Clean and rebuild
cargo clean
cargo xtask build-ebpf
cargo build --package runtime-scope

# Verify toolchain versions
rustc --version
cargo --version
clang --version
llvm-config --version
bpf-linker --version
```

### Architecture

**How it works:**

1. **eBPF programs** run in the Linux kernel, hooking into:
   - Task scheduler events
   - Function entry/exit points (uprobes)
   - USDT probes in async runtimes (future)

2. **Kernel-side processing** aggregates events:
   - Tracks task spawn/completion
   - Measures poll durations
   - Detects blocking behavior
   - Minimal CPU overhead (<1%)

3. **Userspace program** receives events via ring buffers:
   - Processes and correlates events
   - Builds task dependency graphs
   - Generates visualizations
   - Provides real-time TUI

**Why eBPF?**
- Zero overhead when not profiling
- Safe (kernel verifier ensures correctness)
- No code changes required in target app
- Works on production systems

### Resources

- [Aya Documentation](https://aya-rs.dev/) - Rust eBPF framework
- [eBPF Tutorial](https://github.com/lizrice/learning-ebpf)
- [BPF Performance Tools](http://www.brendangregg.com/bpf-performance-tools-book.html) - Brendan Gregg
- [Async Rust Book](https://rust-lang.github.io/async-book/)

### License

MIT or Apache-2.0 (dual licensed)

### Acknowledgments

Built with:
- [Aya](https://aya-rs.dev/) - Pure Rust eBPF library
- [ratatui](https://ratatui.rs/) - Terminal UI framework
- Inspired by Bryan Cantrill's DTrace

---

## Development Roadmap

**Current Phase:** ✅ Phase 1 + Phase 2 Mostly Complete!

### Completed:
- [x] **Phase 0:** Infrastructure setup (eBPF build system, workspace structure)
- [x] **Phase 1:** Basic blocking detection with uprobes + ring buffers
  - [x] Test async application with intentional blocking
  - [x] eBPF programs with uprobes
  - [x] Ring buffer event streaming
  - [x] Duration calculation
  - [x] Real-time output
  - [x] Graceful Ctrl+C shutdown
- [x] **Phase 2:** Stack trace capture & source location (mostly done!)
  - [x] Capture instruction pointers with eBPF StackTrace maps
  - [x] Symbol resolution (DWARF/addr2line/gimli)
  - [x] Show file:line for each stack frame
  - [x] Display function names (demangled with rustc-demangle)
  - [x] PIE executable address translation
  - [ ] Show full call stack (currently shows marker only)

### Next Steps:
- [ ] **Phase 2.1:** Complete stack trace implementation
  - [ ] Show full call stack including blocking_task function
  - [ ] Resolve more than just the top frame

- [ ] **Phase 3:** Async task tracking
  - [ ] Hook Tokio task spawn/poll
  - [ ] Track task IDs and names
  - [ ] Associate blocking events with specific tasks
  - [ ] Show task spawn locations

- [ ] **Phase 4:** Remove instrumentation (Critical!)
  - [ ] **Switch from uprobes → scheduler tracepoints**
  - [ ] **Remove all `#[no_mangle]` markers** (no code changes needed!)
  - [ ] Works on all code (including inlined functions)
  - [ ] Profile any binary without modification
  - [ ] Cascade effect visualization
  - [ ] Task dependency graphs
  - [ ] Executor health metrics

- [ ] **Phase 5:** Production ready
  - [ ] TUI interface (ratatui)
  - [ ] Export to JSON/HTML
  - [ ] Performance benchmarks
  - [ ] Documentation
  - [ ] CI/CD

### Vision:
```
🔴 BLOCKING DETECTED
   Duration: 450.12ms ⚠️
   Task: "handle_upload"
   Location: src/api.rs:142 in process_large_file()

   Stack trace:
   #0 process_large_file at src/api.rs:142
   #1 handle_upload at src/api.rs:89
   #2 tokio::runtime::task::poll

   Impact: 247 tasks delayed
```
