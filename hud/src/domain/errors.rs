//! Structured error types for hud
//!
//! Using thiserror for automatic Display implementation and error chaining.

use super::types::Pid;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProfilerError {
    #[error("Failed to load eBPF program: {0}\n\nThis usually means:\n  - Not running as root (run with: sudo hud ...)\n  - Kernel too old (requires Linux 5.8+)\n  - eBPF disabled in kernel config")]
    EbpfLoadFailed(String),

    #[error("Process {0} not found\n\nIs the process still running? Check with: ps -p {0}")]
    ProcessNotFound(Pid),

    #[error("No Tokio worker threads found in process {0}\n\nPossible causes:\n  - Not a Tokio application\n  - Using single-threaded runtime (current_thread)\n  - Workers not yet spawned (try again after warmup)")]
    NoWorkersFound(Pid),

    #[error("Failed to attach {probe} to {binary}: {error}\n\nThis may indicate:\n  - Binary is stripped (rebuild with debug = true)\n  - Symbol was inlined in release build\n  - Wrong binary path specified")]
    ProbeAttachFailed { probe: String, binary: String, error: String },

    #[error("Symbol resolution failed: {0}\n\nEnsure the binary has debug symbols:\n  [profile.release]\n  debug = true")]
    SymbolizationFailed(String),

    #[error("Failed to read /proc/{0}/maps\n\nCheck that:\n  - Process {0} exists (ps -p {0})\n  - You have permission (try running with sudo)")]
    MemoryMapsParseFailed(Pid),

    #[error("Binary '{binary}' not found in process {pid} memory maps\n\nThe --target path may not match the running binary.\nCheck with: cat /proc/{pid}/maps | grep -i {binary}")]
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
        let msg = err.to_string();
        assert!(msg.contains("Process PID:1234 not found"));
        assert!(msg.contains("ps -p")); // Contains recovery hint
    }

    #[test]
    fn test_probe_attach_error() {
        let err = ProfilerError::ProbeAttachFailed {
            probe: "trace_blocking_start".to_string(),
            binary: "/usr/bin/my-app".to_string(),
            error: "symbol not found".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("trace_blocking_start"));
        assert!(msg.contains("/usr/bin/my-app"));
        assert!(msg.contains("debug = true")); // Contains recovery hint
    }
}
