# runtime-scope

⚠️ **Status: Early Development / Proof of Concept** ⚠️

Real-time async runtime profiler for Rust using eBPF.

Detect blocking operations in async code that harm executor performance. Built with pure Rust + eBPF (Aya framework).

## Current Status: ✅ Phase 1 & 2 Complete, 🚧 Phase 3a In Progress!

**What Works:**
- ✅ Real-time blocking detection (450ms operations detected)
- ✅ **Complete stack trace capture** (55 frames with eBPF StackTrace maps)
- ✅ **Symbol resolution using DWARF debug info**
- ✅ **Source code locations (file:line) for each frame**
- ✅ **Demangled Rust function names**
- ✅ **Memory range detection** (separates executable from shared libraries)
- ✅ **Async task tracking** - Shows which Tokio task is blocking!
- ✅ **Thread→Task correlation** via `set_current_task_id` hook (optional, graceful degradation)
- ✅ PIE executable address translation
- ✅ Accurate duration measurement
- ✅ Process/thread tracking
- ✅ Graceful Ctrl+C shutdown
- 🚧 **Phase 3a: Dual detection mode** - Scheduler-based + marker-based running simultaneously
- 🚧 **Tokio worker thread identification** - Auto-detects tokio-runtime-w* threads
- 🚧 **sched_switch tracepoint** - Kernel scheduler event tracking
- 🚧 **CPU blocking detection** - Filters I/O waits, only reports TASK_RUNNING blocks >5ms

**⚠️ Phase 3a Validation Mode:**
Currently running **hybrid detection** - both marker-based (Phase 1/2) and scheduler-based (Phase 3) detection simultaneously to validate accuracy before removing markers completely.

**Next Steps:**
- 🚧 **Phase 3b: Make scheduler detection default** (markers optional)
- 🎯 **Phase 3c: Remove markers entirely** - profile ANY binary without modification!
- 🚧 Task names and spawn location tracking
- 🚧 Cascade effect visualization
- 🚧 TUI interface

## Quick Demo

```bash
cd /home/soze/runtime-scope

# Phase 3a: Automated dual-detection test (builds, starts app, attaches profiler)
./test.sh

# Or manual mode (Phase 3a requires release builds):
# Terminal 1: Build everything
cargo xtask build-ebpf --release
cargo build --release -p runtime-scope
cargo build --release --example test-async-app

# Terminal 2: Run the test app
./target/release/examples/test-async-app

# Terminal 3: Profile it
sudo -E ./target/release/runtime-scope \
  --pid $(pgrep test-async-app) \
  --target ./target/release/examples/test-async-app
```

**Output (Phase 3a Dual Detection Mode):**
```
🔍 runtime-scope v0.1.0 (Phase 3a: Dual Detection)
   Real-time async runtime profiler

📦 Target: /home/soze/runtime-scope/target/release/examples/test-async-app
📊 Monitoring PID: 24036
   Registered 24 Tokio worker threads
   ✓ Attached uprobe: trace_blocking_start_hook
   ✓ Attached uprobe: trace_blocking_end_hook
   ⚠ Attached uprobe: set_task_id_hook (optional, may be inlined)
   ✓ Attached tracepoint: sched_switch

👀 Watching for blocking events... (press Ctrl+C to stop)

🔵 MARKER DETECTED
   Duration: 450.23ms ⚠️
   Process: PID 24036
   Thread: TID 24038
   Task ID: 30 (if available)
   Detection: Instrumentation-based

   📍 Stack trace:
      #0  0x000000000002c6b0 trace_blocking_start
                      at test-async-app.rs:59:0
      #1  0x00000000000276e0 blocking_task::{{closure}}
                      at test-async-app.rs:134:9
      #2  0x000000000001d280 tokio::runtime::task::core::Core<T,S>::poll::{{closure}}
                      at task/core.rs:329:17
      ... (55 frames total showing complete call stack)

📊 Detection Statistics:
   Marker:    10
   Scheduler: 0  (Phase 3a validation in progress)
```

**Why sudo?** eBPF requires root privileges to attach to processes and load kernel programs.

## How Task Tracking Works (Phase 2)

One of the key challenges in profiling async Rust is the **many-to-many relationship** between OS threads and async tasks:
- Traditional profiling: 1 thread = 1 unit of work
- Async Rust: Many tasks share few threads, tasks migrate between threads

**Our solution:** Hook into Tokio's internal task scheduler to capture **which task is running on which thread** in real-time.

### The Hook: `set_current_task_id`

When Tokio assigns a task to a thread, it calls `set_current_task_id(task_id)`. We hook this function with eBPF:

```rust
// eBPF hook fires when Tokio switches tasks
#[uprobe]
pub fn set_task_id_hook(ctx: ProbeContext) -> u32 {
    let tid = get_current_tid();              // Which thread?
    let task_id: u64 = ctx.arg(0);            // Which task? (from function argument)
    THREAD_TASK_MAP[tid] = task_id;           // Store the mapping
}
```

### The Result

Now when blocking is detected:
1. We know the **thread ID** (from eBPF context)
2. We look up `THREAD_TASK_MAP[tid]` to find the **task ID**
3. We report: "Task 30 blocked for 450ms on thread 24038"

This bridges OS-level observability (threads) with application-level semantics (tasks), giving you actionable profiling data even as tasks migrate between threads!

## What It Currently Shows

- 🔴 **Blocking detection** - When async tasks block the executor
- ⏱️ **Duration measurement** - How long each blocking operation takes (accurate to ~0.01ms)
- 🎯 **Task identification** - Which Tokio task is blocking (Task ID)
- 🧵 **Thread identification** - Which OS thread is affected
- 🔗 **Thread→Task correlation** - Tracks task migration across threads
- 📍 **Complete stack traces** - Full 55-frame call stacks captured
- 🔍 **Symbol resolution** - Function names with DWARF debug info
- 📝 **Source locations** - File paths and line numbers
- 🦀 **Demangled names** - Clean Rust function names (not mangled C++)
- 🏠 **Memory range detection** - Separates executable from shared library frames
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
# NOTE: Phase 3a requires release builds due to eBPF limitations
cargo xtask build-ebpf --release

# Build userspace program (what you run)
cargo build --package runtime-scope --release

# Build test application
cargo build --release --example test-async-app

# Run it
sudo -E ./target/release/runtime-scope
```

**⚠️ Note on Debug Builds:**
Phase 3a (scheduler-based detection) requires **release builds only** due to eBPF verifier limitations with debug assertions and formatting code. Use `--release` flag for all builds.

### Development Workflow

**⚠️ Phase 3a Note:** All builds must use `--release` flag.

**1. Make changes to the eBPF program:**

```bash
# Edit runtime-scope-ebpf/src/main.rs
vim runtime-scope-ebpf/src/main.rs

# Rebuild eBPF (release required)
cargo xtask build-ebpf --release

# Rebuild userspace (embeds new eBPF bytecode)
cargo build --release --package runtime-scope

# Test
sudo -E ./target/release/runtime-scope
```

**2. Make changes to the userspace program:**

```bash
# Edit runtime-scope/src/main.rs
vim runtime-scope/src/main.rs

# Rebuild (no need to rebuild eBPF)
cargo build --release --package runtime-scope

# Test
sudo -E ./target/release/runtime-scope
```

**3. Add shared types:**

```bash
# Edit runtime-scope-common/src/lib.rs
vim runtime-scope-common/src/lib.rs

# Rebuild everything (release required)
cargo xtask build-ebpf --release
cargo build --release --package runtime-scope
```

### Project Structure

```
runtime-scope/
├── runtime-scope/              # Userspace profiler
│   ├── src/
│   │   ├── main.rs            # CLI, event processing, memory range detection, task tracking
│   │   └── symbolizer.rs      # DWARF symbol resolution
│   ├── examples/
│   │   └── test-async-app.rs  # Test application with blocking code
│   └── Cargo.toml
├── runtime-scope-ebpf/         # eBPF programs (runs in kernel)
│   ├── src/
│   │   └── main.rs            # Stack capture, task tracking (THREAD_TASK_MAP)
│   └── Cargo.toml
├── runtime-scope-common/       # Shared types
│   ├── src/
│   │   └── lib.rs             # Event definitions (TaskEvent with task_id)
│   └── Cargo.toml
├── xtask/                      # Build automation
│   ├── src/
│   │   └── main.rs            # Custom cargo commands
│   └── Cargo.toml
├── .cargo/
│   └── config.toml             # Force frame pointers for stack unwinding
├── test.sh                     # Phase 3a dual-detection test script
├── cleanup.sh                  # Emergency cleanup for stuck processes
├── check-symbols.sh            # Symbol diagnostic script
├── SESSION_SUMMARY.md          # Development notes
├── Cargo.toml                  # Workspace manifest
└── README.md
```

### Testing

```bash
# Run Rust tests
cargo test

# Test eBPF program verification (release required for Phase 3a)
cargo xtask build-ebpf --release

# Phase 3a: Run dual detection test
./test.sh

# Or manually run on the test app
cargo build --release --example test-async-app
./target/release/examples/test-async-app &
sudo -E ./target/release/runtime-scope --pid $! --target ./target/release/examples/test-async-app

# Cleanup stuck processes if needed
./cleanup.sh
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

**How it works (Phase 3a Dual Detection):**

1. **eBPF programs** run in the Linux kernel with **dual detection**:

   **Marker-based (Phase 1/2):**
   - `trace_blocking_start_hook` - Captures when blocking begins + stack trace
   - `trace_blocking_end_hook` - Captures when blocking ends (calculates duration)
   - `set_task_id_hook` - Tracks thread→task mappings (optional, graceful degradation)

   **Scheduler-based (Phase 3a):**
   - `sched_switch_hook` - Kernel tracepoint fires on every context switch
   - Automatically detects CPU blocking >5ms on Tokio worker threads
   - Filters TASK_RUNNING state (ignores I/O waits)
   - No instrumentation required in target binary!

2. **Kernel-side processing** captures events:
   - Stack traces (up to 127 frames using BPF StackTrace maps)
   - Thread→Task correlation (THREAD_TASK_MAP)
   - Thread state tracking (THREAD_STATE map for scheduler detection)
   - Tokio worker thread registry (TOKIO_WORKER_THREADS map)
   - Precise timestamps (nanosecond resolution)
   - Process/thread identifiers
   - Minimal CPU overhead (<5% with sched_switch)

3. **Userspace program** receives events via ring buffers:
   - Auto-detects Tokio worker threads by name pattern (`tokio-runtime-w*`)
   - Resolves stack traces using DWARF debug symbols
   - Correlates blocking start/end events
   - Handles PIE address translation with memory range detection
   - Demangles Rust function names
   - Dual detection statistics (marker vs scheduler)
   - Real-time output with color coding (🔵 marker, 🟢 scheduler)

**Why eBPF?**
- Zero overhead when not profiling
- Safe (kernel verifier ensures correctness)
- Can read CPU registers to extract function arguments
- Capture stack traces from running code
- Sub-microsecond latency
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

**Current Phase:** 🚧 Phase 3a In Progress (Dual Detection Validation)

### Completed:
- [x] **Phase 0:** Infrastructure setup (eBPF build system, workspace structure)
- [x] **Phase 1:** Basic blocking detection with uprobes + ring buffers
  - [x] Test async application with intentional blocking
  - [x] eBPF programs with uprobes
  - [x] Ring buffer event streaming
  - [x] Duration calculation
  - [x] Real-time output
  - [x] Graceful Ctrl+C shutdown
- [x] **Phase 2:** Stack trace capture & async task tracking
  - [x] Capture instruction pointers with eBPF StackTrace maps (55 frames!)
  - [x] Symbol resolution (DWARF/addr2line/gimli)
  - [x] Show file:line for each stack frame
  - [x] Display function names (demangled with rustc-demangle)
  - [x] PIE executable address translation
  - [x] Memory range detection (separate executable from shared libraries)
  - [x] Complete call stack including blocking_task function
  - [x] Force frame pointers for reliable stack unwinding
  - [x] **Async task tracking** - Hook `set_current_task_id`
  - [x] **Thread→Task correlation** - THREAD_TASK_MAP in eBPF
  - [x] **Display task IDs** - Know which task is blocking!

### In Progress:
- [~] **Phase 3a:** Dual detection mode (validation)
  - [x] sched_switch tracepoint implementation
  - [x] Tokio worker thread identification
  - [x] Thread state tracking (THREAD_STATE, TOKIO_WORKER_THREADS maps)
  - [x] CPU blocking detection (5ms threshold, TASK_RUNNING filter)
  - [x] Dual event display (🔵 marker, 🟢 scheduler)
  - [x] Detection statistics tracking
  - [ ] Validate scheduler detection accuracy (currently investigating)
  - [ ] Tune blocking detection heuristics

### Next Steps:
- [ ] **Phase 3b:** Make scheduler detection default
  - [ ] Add `--enable-marker-validation` CLI flag
  - [ ] Conditional marker attachment
  - [ ] Scheduler-only mode as default

- [ ] **Phase 3c:** Remove markers completely (Critical!)
  - [ ] **Remove all `#[no_mangle]` markers** (no code changes needed!)
  - [ ] Pure scheduler-based detection
  - [ ] Works on all code (including inlined functions)
  - [ ] Profile any binary without modification

- [ ] **Phase 4:** Enhanced task tracking
  - [ ] Track task names (capture from spawn)
  - [ ] Show task spawn locations
  - [ ] Task dependency graphs
  - [ ] Cascade effect visualization
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
   Task ID: 42 ✅ (Already implemented!)
   Task Name: "handle_upload" (Phase 4)
   Location: src/api.rs:142 in process_large_file()

   Stack trace: ✅ (Already implemented!)
   #0 process_large_file at src/api.rs:142
   #1 handle_upload at src/api.rs:89
   #2 tokio::runtime::task::poll

   Impact: 247 tasks delayed (Phase 4)
```
