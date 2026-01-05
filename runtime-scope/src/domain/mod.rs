//! Domain model for runtime-scope
//!
//! This module contains core domain types and errors that provide:
//! - Compile-time safety via newtype pattern
//! - Self-documenting function signatures
//! - Structured error handling

pub mod types;
pub mod errors;

// Re-export common types for convenience
pub use types::{
    WorkerId, Pid, Tid, CpuId, StackId,
    FunctionName, Timestamp, Duration,
};

pub use errors::{ProfilerError, ExportError, TuiError};
