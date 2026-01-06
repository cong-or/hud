//! Tokio worker thread discovery
//!
//! Identifies Tokio runtime worker threads by reading /proc filesystem.

use anyhow::{Context, Result};
use std::fs;

use crate::domain::{Pid, Tid};

/// Information about a discovered Tokio worker thread
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    pub tid: Tid,
    pub worker_id: u32,
    pub comm: String,
}

/// Identify Tokio worker threads by reading /proc/pid/task/*/comm
///
/// Finds threads with names starting with "tokio-runtime-w"
///
/// # Errors
/// Returns an error if /proc filesystem cannot be accessed or read
#[allow(clippy::cast_possible_truncation)]
pub fn identify_tokio_workers(pid: Pid) -> Result<Vec<WorkerInfo>> {
    let task_dir = format!("/proc/{}/task", pid.0);
    let mut workers = Vec::new();

    let entries = fs::read_dir(&task_dir).context(format!("Failed to read {task_dir}"))?;

    for entry in entries {
        let entry = entry?;
        let tid_str = entry.file_name().to_string_lossy().to_string();

        if let Ok(tid) = tid_str.parse::<u32>() {
            let comm_path = format!("/proc/{}/task/{}/comm", pid.0, tid);

            if let Ok(comm) = fs::read_to_string(comm_path) {
                let comm = comm.trim();
                if comm.starts_with("tokio-runtime-w") {
                    log::info!("Found Tokio worker thread: TID {tid} ({comm})");
                    workers.push(WorkerInfo {
                        tid: Tid(tid),
                        worker_id: workers.len() as u32, // 0-indexed
                        comm: comm.to_string(),
                    });
                }
            }
        }
    }

    Ok(workers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_workers_self_process() {
        // Try to identify workers in the test process itself
        // This should return empty since the test doesn't run a Tokio runtime
        #[allow(clippy::cast_possible_wrap)]
        let pid = Pid(std::process::id() as i32);
        let result = identify_tokio_workers(pid);

        // Should succeed (no error), but find no workers
        assert!(result.is_ok());
        let workers = result.unwrap();
        assert_eq!(workers.len(), 0);
    }

    #[test]
    fn test_identify_workers_invalid_pid() {
        // Invalid PID should return an error
        let result = identify_tokio_workers(Pid(9_999_999));
        assert!(result.is_err());
    }
}
