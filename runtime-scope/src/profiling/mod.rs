//! Profiling core modules
//!
//! This module contains the core profiling functionality extracted from main.rs:
//! - Stack trace resolution (deduplicates 150 lines of code!)
//! - Worker thread discovery
//! - CPU utilities
//! - eBPF program loading and setup

pub mod stack_resolver;
pub mod worker_discovery;
pub mod cpu_utils;
pub mod ebpf_setup;

// Re-export common types
pub use stack_resolver::StackResolver;
pub use worker_discovery::{WorkerInfo, identify_tokio_workers};
pub use cpu_utils::online_cpus;
pub use ebpf_setup::{load_ebpf_program, init_ebpf_logger, attach_blocking_uprobes, setup_scheduler_detection};

// Re-export MemoryRange from symbolization for convenience
pub use crate::symbolization::MemoryRange;
