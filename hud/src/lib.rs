//! # Runtime Scope - eBPF-based Tokio Async Runtime Profiler
//!
//! Runtime Scope is a low-overhead profiling tool that detects blocking operations
//! in Tokio async runtimes using eBPF (extended Berkeley Packet Filter). It helps
//! identify performance bottlenecks caused by synchronous operations blocking the
//! async executor's worker threads.
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                       User Application                          │
//! │                    (Tokio Async Runtime)                        │
//! └───────────────────────┬─────────────────────────────────────────┘
//!                         │ blocking operations
//!                         ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     eBPF Programs (Kernel)                      │
//! │  • Uprobes: trace_blocking_{start,end}, set_task_id            │
//! │  • Tracepoints: sched_switch (scheduler-based detection)        │
//! │  • Perf Events: CPU sampling at 99 Hz (stack traces)            │
//! └───────────────────────┬─────────────────────────────────────────┘
//!                         │ ring buffer events
//!                         ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                 Runtime Scope (This Crate)                      │
//! │                                                                 │
//! │  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐      │
//! │  │  Profiling   │──▶│    Event     │──▶│     TUI      │      │
//! │  │   (eBPF)     │   │  Processor   │   │  (Terminal)  │      │
//! │  └──────────────┘   └──────────────┘   └──────────────┘      │
//! │         │                   │                                  │
//! │         │                   ▼                                  │
//! │         │           ┌──────────────┐                          │
//! │         │           │ Symbolizer   │                          │
//! │         │           │  (DWARF)     │                          │
//! │         │           └──────────────┘                          │
//! │         │                                                      │
//! │         ▼                                                      │
//! │  ┌──────────────┐   ┌──────────────┐                         │
//! │  │   Analysis   │   │    Export    │                         │
//! │  │  (Hotspots)  │   │ (trace.json) │                         │
//! │  └──────────────┘   └──────────────┘                         │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Module Structure
//!
//! ### Core Pipeline Modules
//!
//! - [`profiling`]: eBPF program management, event collection, and worker discovery
//!   - `ebpf_setup`: Load and attach eBPF programs (uprobes, tracepoints, perf events)
//!   - `event_processor`: Process events from eBPF ring buffer
//!   - `worker_discovery`: Identify Tokio worker threads via `/proc` inspection
//!
//! - [`symbolization`]: Convert raw addresses to human-readable function names
//!   - Uses DWARF debug information via `addr2line` crate
//!   - Handles PIE (Position Independent Executable) address adjustment
//!
//! - [`analysis`]: Post-processing and hotspot detection
//!   - Aggregate blocking events by location (function/file/line)
//!   - Calculate total duration and frequency statistics
//!
//! - [`export`]: Generate Chrome Trace Event Format JSON for visualization
//!   - Compatible with Perfetto, Speedscope, Chrome's `chrome://tracing`
//!
//! ### UI and Data Modules
//!
//! - [`tui`]: Terminal UI with multiple view modes (Live, Hotspots, Raw Events)
//!   - Real-time event streaming with ratatui
//!   - Interactive keyboard navigation
//!
//! - [`cli`]: Command-line argument parsing and configuration
//!
//! - [`trace_data`]: Shared data structures for trace events
//!
//! - [`domain`]: Core domain types (Pid, Tid, StackId, CpuId)
//!
//! ## Detection Methods
//!
//! Runtime Scope supports three complementary detection methods:
//!
//! ### 1. Marker-Based Detection (Explicit Instrumentation)
//! - Requires application to call `trace_blocking_start()` / `trace_blocking_end()`
//! - **Pros**: Zero false positives, precise attribution
//! - **Cons**: Requires code modification
//!
//! ### 2. Scheduler-Based Detection (Implicit, Threshold-Based)
//! - Monitors `sched_switch` events when Tokio workers are scheduled out
//! - Detects blocking when off-CPU time exceeds threshold (default: 5ms)
//! - **Pros**: No code changes needed
//! - **Cons**: False positives from legitimate preemption
//!
//! ### 3. Sampling-Based Detection (Statistical Profiling)
//! - CPU sampling at 99 Hz via perf events
//! - Captures stack traces during execution
//! - **Pros**: Low overhead, whole-program visibility
//! - **Cons**: Statistical (may miss short events)
//!
//! ## Operational Modes
//!
//! 1. **Live TUI Mode** (default): Real-time monitoring with terminal interface
//! 2. **Headless Mode** (`--headless`): Log events to stdout without TUI
//! 3. **Replay Mode** (`--replay trace.json`): Analyze previously recorded traces
//!
//! ## Typical Usage
//!
//! ```bash
//! # Profile a running Tokio application
//! sudo ./hud --pid <PID>
//!
//! # Export trace for offline analysis
//! sudo ./hud --pid <PID> --export trace.json
//!
//! # Replay and analyze a previously recorded trace
//! ./hud --replay trace.json
//! ```
//!
//! ## Key Concepts
//!
//! - **eBPF**: Linux kernel technology for safe, high-performance instrumentation
//! - **Uprobes**: Dynamic tracing of userspace functions
//! - **Ring Buffer**: Lock-free kernel→userspace event queue
//! - **PIE/ASLR**: Position-independent executables require address relocation
//! - **DWARF**: Debug information format for source-level symbolization
//! - **Stack Traces**: Call chain captured via kernel's `bpf_get_stackid()`

// Expose modules for testing
pub mod analysis;
pub mod cli;
pub mod domain;
pub mod export;
pub mod profiling;
pub mod symbolization;
pub mod trace_data;
pub mod tui;
