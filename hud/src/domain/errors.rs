//! Structured error types for hud
//!
//! Using thiserror for automatic Display implementation and error chaining.

use super::types::Pid;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProfilerError {
    #[error("Failed to load eBPF program: {0}")]
    EbpfLoadFailed(String),

    #[error("Process {0} not found")]
    ProcessNotFound(Pid),

    #[error("No Tokio workers discovered in process {0}")]
    NoWorkersFound(Pid),

    #[error("Failed to attach {probe} to {binary}: {error}")]
    ProbeAttachFailed { probe: String, binary: String, error: String },

    #[error("Symbol resolution failed: {0}")]
    SymbolizationFailed(String),

    #[error("Failed to read /proc/{0}/maps")]
    MemoryMapsParseFailed(Pid),

    #[error("No memory range found for binary {binary} in process {pid}")]
    NoMemoryRangeFound { pid: Pid, binary: String },

    #[error("Invalid stack trace ID: {0}")]
    InvalidStackId(i64),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Aya(#[from] aya::EbpfError),
}

#[derive(Error, Debug)]
pub enum ExportError {
    #[error("Failed to serialize trace data: {0}")]
    SerializationFailed(String),

    #[error("Failed to write trace file: {0}")]
    WriteFailed(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Error, Debug)]
pub enum TuiError {
    #[error("Failed to parse trace file: {0}")]
    TraceParseFailed(String),

    #[error("Invalid trace data: {0}")]
    InvalidTraceData(String),

    #[error("Terminal error: {0}")]
    TerminalError(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_error_display() {
        let err = ProfilerError::ProcessNotFound(Pid(1234));
        assert_eq!(err.to_string(), "Process PID:1234 not found");
    }

    #[test]
    fn test_probe_attach_error() {
        let err = ProfilerError::ProbeAttachFailed {
            probe: "trace_blocking_start".to_string(),
            binary: "/usr/bin/my-app".to_string(),
            error: "symbol not found".to_string(),
        };
        assert!(err.to_string().contains("trace_blocking_start"));
        assert!(err.to_string().contains("/usr/bin/my-app"));
    }
}
