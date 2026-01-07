# Architecture

How hud works under the hood.

## Overview

hud uses eBPF (extended Berkeley Packet Filter) to instrument Tokio applications at the kernel level with zero overhead. It captures scheduler events, CPU samples, and stack traces without modifying application code.

```
┌─────────────────────────────────────────┐
│        Target Application               │
│        (Tokio Runtime)                  │
└──────────────┬──────────────────────────┘
               │ blocking operations
               ▼
┌─────────────────────────────────────────┐
│        Linux Kernel                     │
│  • sched_switch tracepoint              │
│  • perf_event (99 Hz)                   │
│  • Stack trace capture                  │
└──────────────┬──────────────────────────┘
               │ eBPF ring buffer
               ▼
┌─────────────────────────────────────────┐
│        hud (userspace)                  │
│  • Event processing                     │
│  • Symbol resolution (DWARF)            │
│  • TUI rendering / Export               │
└─────────────────────────────────────────┘
```

## eBPF Programs

hud loads three types of eBPF programs into the kernel:

### 1. Tracepoint: sched_switch

Fires when the Linux scheduler switches threads.

**Purpose:** Detect when Tokio worker threads go ON/OFF CPU.

**How it works:**
- Track when workers start executing (ON-CPU)
- Track when workers stop executing (OFF-CPU)
- Calculate off-CPU duration
- Detect blocking when duration > threshold (5ms)

**Key code:** `hud-ebpf/src/main.rs::sched_switch_hook()`

### 2. Perf Event: CPU Sampling

Fires at 99 Hz (every ~10ms) on each CPU core.

**Purpose:** Capture stack traces of what's currently executing.

**How it works:**
- Periodic timer interrupt
- Capture instruction pointer at sample time
- Walk stack to get full call chain
- Filter to only Tokio worker threads

**Why 99 Hz?** Prime number avoids aliasing with other periodic events (e.g., 100 Hz system timer).

**Key code:** `hud-ebpf/src/main.rs::on_cpu_sample()`

### 3. Uprobes (Optional)

Dynamic instrumentation of userspace functions.

**Purpose:** Marker-based detection for explicit blocking regions.

**How it works:**
- Attach to `trace_blocking_start()` function
- Attach to `trace_blocking_end()` function
- Calculate precise duration between calls

**Note:** Requires application code changes, but provides zero false positives.

**Key code:** `hud-ebpf/src/main.rs::trace_blocking_start_hook()`

## Data Structures

### eBPF Maps

Shared memory between kernel (eBPF) and userspace (hud):

- **EVENTS (RingBuf):** 256KB lock-free ring buffer for event stream
- **STACK_TRACES (StackTrace):** Deduplicated stack traces (1024 max)
- **TOKIO_WORKER_THREADS (HashMap):** Worker thread registry
- **CONFIG (HashMap):** Configuration (threshold, target PID)
- **THREAD_STATE (HashMap):** Per-thread execution state

### TaskEvent

Core event structure passed from kernel to userspace:

```rust
struct TaskEvent {
    pid: u32,              // Process ID
    tid: u32,              // Thread ID
    timestamp_ns: u64,     // Nanosecond timestamp
    event_type: u32,       // EVENT_* constant
    stack_id: i64,         // Stack trace ID
    duration_ns: u64,      // Duration (for span events)
    worker_id: u32,        // Tokio worker index
    cpu_id: u32,           // CPU core
    detection_method: u8,  // 1=marker, 2=sched, 3=trace, 4=sample
    // ...
}
```

## Detection Methods

hud implements three complementary detection approaches:

### Method 1: Marker-Based (Explicit)

Application explicitly marks blocking regions:

```rust
trace_blocking_start();
expensive_sync_operation();
trace_blocking_end();
```

**Pros:** Zero false positives, precise attribution
**Cons:** Requires code changes

### Method 2: Scheduler-Based (Threshold)

Monitors scheduler events to detect blocking:

```
if (off_cpu_duration > 5ms && state == TASK_RUNNING) {
    report_blocking();
}
```

**Pros:** No code changes, whole-program visibility
**Cons:** False positives from legitimate preemption

### Method 3: Sampling-Based (Statistical)

CPU sampling at 99 Hz for flame graphs:

```
timer_fires() -> capture_stack_trace()
```

**Pros:** Low overhead, great for flame graphs
**Cons:** Statistical (may miss short events)

## Symbol Resolution

Raw addresses from eBPF must be translated to function names.

### The Problem

eBPF captures addresses like `0x55f3a2b4c780`, but we need:
- Function: `tokio::runtime::blocking::pool::spawner::spawn_blocking`
- File: `pool.rs`
- Line: `42`

### The Solution: DWARF + PIE Adjustment

1. **Parse `/proc/<pid>/maps`** to find binary's base address (PIE/ASLR)
2. **Calculate file offset:** `offset = runtime_addr - base_addr`
3. **Look up in DWARF debug info** to resolve symbol
4. **Demangle Rust symbols** for readability

**Key code:** `hud/src/symbolization/`

## Worker Discovery

Before profiling, hud identifies Tokio worker threads:

```bash
# Inspects /proc/<pid>/task/<tid>/comm
/proc/12345/task/
  ├── 12345/comm → "main"
  ├── 12346/comm → "tokio-runtime-w"  # Worker 0
  ├── 12347/comm → "tokio-runtime-w"  # Worker 1
  └── 12348/comm → "blocking-1"       # Not a worker
```

Workers are registered in the `TOKIO_WORKER_THREADS` eBPF map for filtering.

**Key code:** `hud/src/profiling/worker_discovery.rs`

## Event Processing Pipeline

```
1. eBPF emits event → ring buffer
   ↓
2. Main thread polls ring_buf.next()
   ↓
3. EventProcessor routes by event_type
   ↓
4. Resolve symbols (DWARF lookup)
   ↓
5. Output to:
   • TUI (via crossbeam channel)
   • Export (trace.json)
   • Stdout (headless mode)
```

**Key code:** `hud/src/profiling/event_processor.rs`

## TUI Architecture

The terminal UI runs in a separate thread:

```
Main Thread              TUI Thread
    │                       │
    ├──[crossbeam]──────────┤
    │   channel              │
    │                        │
    │ eBPF events            │ try_recv()
    │ try_send() ────────────┤ (non-blocking)
    │                        │
    │                        │ Render @ 60 FPS
    │                        │ (ratatui)
```

**Non-blocking design:** Neither thread blocks the other.

**Key code:** `hud/src/tui.rs`

## Performance Characteristics

| Component | Overhead |
|-----------|----------|
| sched_switch | ~1-2μs per context switch |
| perf_event sampling | ~1% CPU at 99 Hz |
| Stack unwinding | ~5-10μs per stack |
| Symbol resolution | Cached (O(1) after first lookup) |
| TUI rendering | 60 FPS (~16ms frame budget) |

**Total:** < 5% overhead in typical workloads.

## Build Process

eBPF programs require special compilation:

1. **Compile eBPF:** `cargo +nightly build --target bpfel-unknown-none`
   - No std library (kernel context)
   - BPF verifier checks safety
   - Always release mode (LTO eliminates dead code)

2. **Embed bytecode:** `include_bytes_aligned!()` embeds compiled eBPF

3. **Load into kernel:** `Ebpf::load()` at runtime

4. **Attach programs:** Uprobes, tracepoints, perf events

**Key code:** `xtask/src/main.rs`, `hud/src/profiling/ebpf_setup.rs`

## Limitations

- **Linux only:** eBPF is Linux-specific (5.15+)
- **Tokio only:** Worker discovery assumes Tokio thread naming
- **Debug symbols required:** Need DWARF for function names
- **Root required:** eBPF needs elevated privileges
- **x86_64/aarch64:** Other architectures untested

## Further Reading

- [eBPF Documentation](https://ebpf.io/)
- [Linux Tracing Systems](https://www.kernel.org/doc/html/latest/trace/index.html)
- [DWARF Debugging Format](http://dwarfstd.org/)
- [Tokio Runtime Internals](https://tokio.rs/blog/2019-10-scheduler)
