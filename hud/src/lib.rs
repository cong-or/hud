//! # hud - Real-time async profiler for Tokio
//!
//! Zero-overhead eBPF profiling with live TUI. Captures scheduler events,
//! CPU samples, and stack traces to identify blocking operations in async code.
//!
//! ## Quick Start
//!
//! ```bash
//! sudo ./hud --pid <PID> --target <BINARY>
//! ```
//!
//! ## Documentation
//!
//! - [Architecture](../docs/ARCHITECTURE.md) - How it works internally
//! - [TUI Guide](../docs/TUI.md) - Using the interface
//! - [Development](../docs/DEVELOPMENT.md) - Contributing
//!
//! ## Core Modules
//!
//! - [`profiling`] - eBPF setup, event processing, worker discovery
//! - [`symbolization`] - DWARF symbol resolution with PIE/ASLR handling
//! - [`tui`] - Terminal UI with hotspot, timeline, and worker views
//! - [`analysis`] - Hotspot detection and aggregation
//! - [`export`] - Chrome Trace Event Format (JSON) export
//! - [`cli`] - Command-line argument parsing
//! - [`trace_data`] - Event data structures
//! - [`domain`] - Core types (Pid, Tid, StackId, CpuId)

// Expose modules for testing
pub mod analysis;
pub mod cli;
pub mod domain;
pub mod export;
pub mod profiling;
pub mod symbolization;
pub mod trace_data;
pub mod tui;
