//! Profiling core modules
//!
//! This module contains the core profiling functionality extracted from main.rs:
//! - Stack trace resolution (deduplicates 150 lines of code!)
//! - Worker thread discovery
//! - CPU utilities
//! - eBPF program loading and setup
//! - Debug diagnostics
//! - Event display formatting

pub mod cpu_utils;
pub mod diagnostics;
pub mod ebpf_setup;
pub mod event_display;
pub mod stack_resolver;
pub mod worker_discovery;

// Re-export common types
pub use cpu_utils::online_cpus;
pub use diagnostics::print_perf_event_diagnostics;
pub use ebpf_setup::{
    attach_blocking_uprobes, init_ebpf_logger, load_ebpf_program, setup_scheduler_detection,
};
pub use event_display::{
    display_blocking_end, display_blocking_end_no_start, display_blocking_start,
    display_execution_event, display_progress, display_scheduler_detected, display_statistics,
    DetectionStats,
};
pub use stack_resolver::StackResolver;
pub use worker_discovery::{identify_tokio_workers, WorkerInfo};

// Re-export MemoryRange from symbolization for convenience
pub use crate::symbolization::MemoryRange;
