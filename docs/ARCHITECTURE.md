# Architecture

How hud works under the hood.

## Overview

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

### sched_switch (tracepoint)

Fires on scheduler context switches. Detects when Tokio workers go ON/OFF CPU and reports blocking when off-CPU duration exceeds the threshold (default: 5ms, configurable via `--threshold`).

### perf_event (99 Hz sampling)

Periodic CPU sampling for stack traces. 99 Hz avoids aliasing with 100 Hz system timer. Captures what's executing on Tokio worker threads.

## Detection Methods

**Scheduler-based:** Monitor off-CPU duration (time in run queue). When a worker returns to CPU after waiting longer than threshold (default 5ms), capture stack trace. High off-CPU time indicates something else was monopolizing the worker—blocking.
- Pros: No code changes, whole-program visibility
- Cons: Measures symptom not cause directly; false positives from system CPU pressure

**Sampling-based:** CPU sampling at 99 Hz for flame graphs.
- Pros: Low overhead
- Cons: Statistical, may miss short events

## Symbol Resolution

eBPF captures raw addresses. To get function names:

1. Parse `/proc/<pid>/maps` for binary base address (PIE/ASLR)
2. Calculate file offset: `runtime_addr - base_addr`
3. Look up in DWARF debug info
4. Demangle Rust symbols

## Worker Discovery

Identifies Tokio worker threads using a 4-step fallback chain:

1. **Explicit prefix** (`--workers <prefix>`): Match threads whose comm starts with the given prefix. No fallback.
2. **Default prefixes**: Try `tokio-runtime-w` (Tokio ≤ 1.x) and `tokio-rt-worker` (Tokio 1.44+). Both are the result of `/proc`'s 15-char truncation of the full thread name.
3. **Stack-based discovery**: Start the perf-event sampler, collect stack traces for 500ms, and classify each thread by Tokio frame signatures — `scheduler::multi_thread::worker` identifies workers, `blocking::pool::Inner::run` (without worker frames) identifies the blocking pool. This catches custom-named runtimes that the default prefix misses.
4. **Largest thread group**: Scan `/proc/<pid>/task/*/comm` and pick the biggest group of threads sharing a `{name}-{N}` pattern.

Matched threads are registered in an eBPF map for filtering. The blocking pool filter uses the same frame signatures at runtime to suppress `spawn_blocking` noise from the TUI.

## Event Flow

```
eBPF event → ring buffer → EventProcessor → symbol resolution → TUI/export
```

TUI runs in separate thread with non-blocking crossbeam channel. Neither thread blocks the other.

## Overhead

< 5% in typical workloads. Sampling at 99 Hz, symbol resolution cached after first lookup.

## Limitations

- Linux 5.8+ only (eBPF)
- Tokio only (thread naming, configurable via `--workers`)
- Debug symbols required (DWARF)
- Root privileges required
- x86_64/aarch64 only
