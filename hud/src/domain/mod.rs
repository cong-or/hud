//! Domain model for hud
//!
//! This module contains core domain types and errors that provide:
//! - Compile-time safety via newtype pattern
//! - Self-documenting function signatures
//! - Structured error handling

pub mod errors;
pub mod types;

// Re-export common types for convenience
pub use types::{CpuId, Duration, FunctionName, Pid, StackId, Tid, Timestamp, WorkerId};

pub use errors::{ExportError, ProfilerError, TuiError};
