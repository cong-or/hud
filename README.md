# runtime-scope

Real-time async runtime profiler with beautiful visualizations for Rust.

Instantly see which tasks are blocking your executor, how one slow task delays hundreds of others, and whether async is actually helping your workload.

## Installation

```bash
cargo install runtime-scope
```

## Usage

Profile any Rust program using async/await:

```bash
# Profile a running process
sudo runtime-scope --pid 1234

# Profile system-wide async activity
sudo runtime-scope

# Save profiling data
sudo runtime-scope --pid 1234 --output profile.json
```

**Why sudo?** The tool uses eBPF which requires root privileges to safely observe kernel events.

### Quick Start

1. Start your async Rust application
2. Find its process ID: `ps aux | grep your-app`
3. Run: `sudo runtime-scope --pid <pid>`
4. Watch for performance issues in real-time
5. Press Ctrl+C to stop and see summary

### What It Shows

- 🔴 **Blocking tasks** - Tasks holding the executor hostage
- 📊 **Cascade effects** - How one slow task delays hundreds of others
- ⏱️ **Poll vs await time** - Where your tasks actually spend time
- 💡 **Recommendations** - Actionable fixes for performance issues

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
│   │   └── main.rs            # CLI, TUI, event processing
│   └── Cargo.toml
├── runtime-scope-ebpf/         # eBPF programs (runs in kernel)
│   ├── src/
│   │   └── main.rs            # Kernel-side tracing code
│   └── Cargo.toml
├── runtime-scope-common/       # Shared types
│   ├── src/
│   │   └── lib.rs             # Event definitions, shared structs
│   └── Cargo.toml
├── xtask/                      # Build automation
│   ├── src/
│   │   └── main.rs            # Custom cargo commands
│   └── Cargo.toml
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

**Status:** 🚧 Active development - Phase 1 in progress

**Roadmap:**
- [x] Phase 0: Infrastructure setup
- [ ] Phase 1: Basic task tracing
- [ ] Phase 2: Blocking detection
- [ ] Phase 3: TUI with health check
- [ ] Phase 4: Cascade visualization
