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

Identifies Tokio workers by scanning `/proc/<pid>/task/<tid>/comm`. First tries the default prefix `tokio-runtime-w`. If no matches, auto-discovers by finding the largest thread group (threads matching `{name}-{N}` or sharing identical truncated names). Use `--workers <prefix>` to override auto-detection. Matched threads are registered in an eBPF map for filtering.

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
