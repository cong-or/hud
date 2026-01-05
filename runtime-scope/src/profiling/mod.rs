//! Profiling core modules
//!
//! This module contains the core profiling functionality extracted from main.rs:
//! - Stack trace resolution (deduplicates 150 lines of code!)
//! - Worker thread discovery
//! - CPU utilities
//! - eBPF program loading (TODO)

pub mod stack_resolver;
pub mod worker_discovery;
pub mod cpu_utils;

// Re-export common types
pub use stack_resolver::StackResolver;
pub use worker_discovery::{WorkerInfo, identify_tokio_workers};
pub use cpu_utils::online_cpus;

// Re-export MemoryRange from symbolization for convenience
pub use crate::symbolization::MemoryRange;
