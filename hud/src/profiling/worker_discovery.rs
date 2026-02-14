//! Tokio worker thread discovery
//!
//! Identifies Tokio runtime worker threads by reading the `/proc` filesystem.
//!
//! ## Discovery strategy
//!
//! 1. **Explicit prefix** (`--workers <prefix>`): match threads whose comm
//!    starts with the given prefix. No fallback — if nothing matches, report
//!    diagnostics showing all thread names and a suggested prefix.
//! 2. **Default** (no `--workers`): try `tokio-runtime-w` first. If that
//!    fails, auto-discover by scanning all threads and picking the largest
//!    group of 2+ threads sharing a common base name.
//!
//! ## Why auto-discovery works
//!
//! Tokio worker threads follow predictable naming conventions:
//! - Default: `tokio-runtime-worker-{N}` (truncated to `tokio-runtime-w` in
//!   `/proc` due to the 15-char `TASK_COMM_LEN` limit)
//! - Custom: `{thread_name}-{N}` (e.g. `my-pool-0`, `my-pool-1`)
//!
//! Auto-discovery finds the largest group of threads sharing a common prefix,
//! which in a Tokio application is almost always the worker pool.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;

use crate::domain::{Pid, Tid};

/// Default thread name prefix for standard Tokio runtimes.
///
/// Tokio names workers `tokio-runtime-worker-{N}`, but `/proc/*/comm` truncates
/// at 15 characters (`TASK_COMM_LEN`), so all workers appear as `tokio-runtime-w`.
pub const DEFAULT_PREFIX: &str = "tokio-runtime-w";

/// Minimum number of threads in a group to qualify as a worker pool.
/// A single thread is never a pool; two is the minimum useful Tokio runtime.
const MIN_POOL_SIZE: usize = 2;

/// Maximum number of thread names to display in diagnostic output.
const MAX_DISPLAY_NAMES: usize = 10;

/// Information about a discovered Tokio worker thread.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// OS thread ID from `/proc/<pid>/task/<tid>`
    pub tid: Tid,
    /// Zero-based index assigned during discovery (not the kernel TID)
    pub worker_id: u32,
    /// Thread comm name as read from `/proc/<pid>/task/<tid>/comm`
    pub comm: String,
}

/// Read all thread comm names for a given process.
///
/// Scans `/proc/<pid>/task/` and reads each thread's `comm` file.
/// Threads that vanish between `readdir` and reading `comm` are silently
/// skipped — this is expected in a live process.
///
/// # Errors
/// Returns an error if `/proc/<pid>/task/` cannot be read.
pub fn list_process_threads(pid: Pid) -> Result<Vec<(u32, String)>> {
    let task_dir = format!("/proc/{}/task", pid.0);
    let entries = fs::read_dir(&task_dir).with_context(|| format!("Failed to read {task_dir}"))?;

    Ok(entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let tid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let comm_path = format!("/proc/{}/task/{}/comm", pid.0, tid);
            // Threads may exit between readdir and read — silently skip
            let mut comm = fs::read_to_string(comm_path).ok()?;
            // comm files end with \n — truncate in-place to avoid reallocation
            let trimmed_len = comm.trim_end().len();
            comm.truncate(trimmed_len);
            Some((tid, comm))
        })
        .collect())
}

/// Filter threads whose comm starts with `prefix` and assign sequential worker IDs.
///
/// Worker IDs are assigned in the order threads are encountered (0, 1, 2, ...).
/// This ordering is arbitrary but stable within a single scan.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn collect_workers(threads: &[(u32, String)], prefix: &str) -> Vec<WorkerInfo> {
    threads
        .iter()
        .filter(|(_, comm)| comm.starts_with(prefix))
        .enumerate()
        .map(|(idx, (tid, comm))| {
            log::info!("Found worker thread: TID {tid} ({comm}) → worker_id {idx}");
            WorkerInfo { tid: Tid(*tid), worker_id: idx as u32, comm: comm.clone() }
        })
        .collect()
}

/// Strip a trailing `-{digits}` suffix from a thread name.
///
/// Tokio appends `-{N}` to thread names (e.g. `my-pool-0`). This function
/// extracts the base name before that suffix.
///
/// Returns `Some("my-pool")` for `"my-pool-0"`, `None` for `"main"` or `"foo-"`.
fn strip_numeric_suffix(name: &str) -> Option<&str> {
    let dash_pos = name.rfind('-')?;
    let suffix = &name[dash_pos + 1..];

    // Suffix must be non-empty and purely ASCII digits (e.g. "0", "12").
    // Using bytes() avoids UTF-8 decode overhead since we only care about ASCII.
    if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) {
        Some(&name[..dash_pos])
    } else {
        None
    }
}

/// Auto-discover the most likely worker thread prefix.
///
/// Groups threads by their "base name" and returns the prefix of the largest
/// group that has at least [`MIN_POOL_SIZE`] members. Two naming patterns
/// are handled:
///
/// - **Numbered**: `my-pool-0`, `my-pool-1` → base name `my-pool`
/// - **Truncated**: `tokio-runtime-w`, `tokio-runtime-w` → base name
///   `tokio-runtime-w` (all workers share identical truncated comm)
#[must_use]
pub fn discover_worker_prefix(threads: &[(u32, String)]) -> Option<String> {
    // Count threads per base name. Borrows from `threads` to avoid allocation.
    let mut groups: HashMap<&str, usize> = HashMap::new();

    for (_, comm) in threads {
        // Strip trailing -N to get the base name, or use the full name
        // (handles /proc truncation where all workers share one name)
        let base = strip_numeric_suffix(comm).unwrap_or(comm.as_str());
        *groups.entry(base).or_default() += 1;
    }

    // Pick the largest group that could be a thread pool
    groups
        .into_iter()
        .filter(|(_, count)| *count >= MIN_POOL_SIZE)
        .max_by_key(|(_, count)| *count)
        .map(|(prefix, _)| prefix.to_owned())
}

/// Log diagnostic information when worker discovery fails.
///
/// Shows the user what threads exist in the process so they can identify the
/// right prefix. Suggests a `--workers` value if a likely candidate group was
/// found by auto-discovery.
fn log_discovery_failure(
    threads: &[(u32, String)],
    searched_prefix: &str,
    discovered: Option<&str>,
) {
    // Deduplicate and sort for clean, readable output
    let mut names: Vec<&str> = threads.iter().map(|(_, c)| c.as_str()).collect();
    names.sort_unstable();
    names.dedup();

    // Truncate long thread lists to keep the log manageable
    let display = if names.len() > MAX_DISPLAY_NAMES {
        format!(
            "{}, ... ({} more)",
            names[..MAX_DISPLAY_NAMES].join(", "),
            names.len() - MAX_DISPLAY_NAMES
        )
    } else {
        names.join(", ")
    };

    log::warn!(
        "No workers found matching prefix \"{searched_prefix}\". \
         Found {} threads: [{display}]",
        threads.len()
    );

    // If auto-discovery found a plausible group, suggest it
    match discovered {
        Some(hint) if hint != searched_prefix => {
            log::warn!("Hint: try --workers {hint}");
        }
        _ => {
            log::warn!("If your Tokio runtime uses custom thread names, pass --workers <prefix>.");
        }
    }
}

/// Identify Tokio worker threads by reading `/proc/<pid>/task/*/comm`.
///
/// # Discovery strategy
///
/// 1. Try the given prefix (or default `tokio-runtime-w`)
/// 2. If nothing matched **and** no explicit prefix was given, auto-discover
///    by finding the largest thread group in the process
/// 3. If still nothing, log diagnostics with all thread names and a hint
///
/// # Arguments
///
/// * `pid` - Target process ID to scan
/// * `name_prefix` - `Some("prefix")` for explicit matching, `None` for auto-detect
///
/// # Errors
///
/// Returns an error if `/proc` filesystem cannot be accessed or read.
pub fn identify_tokio_workers(pid: Pid, name_prefix: Option<&str>) -> Result<Vec<WorkerInfo>> {
    let threads = list_process_threads(pid)?;

    // Step 1: Try the explicit or default prefix
    let prefix = name_prefix.unwrap_or(DEFAULT_PREFIX);
    let workers = collect_workers(&threads, prefix);
    if !workers.is_empty() {
        return Ok(workers);
    }

    // Step 2: Auto-discover, but only when the user didn't pass --workers.
    // If they gave us an explicit prefix, respect it — don't silently override
    // with a different thread group.
    let discovered = discover_worker_prefix(&threads);

    if name_prefix.is_none() {
        if let Some(ref disc_prefix) = discovered {
            // Skip if discovery found the same prefix we already tried
            if disc_prefix != DEFAULT_PREFIX {
                let workers = collect_workers(&threads, disc_prefix);
                if !workers.is_empty() {
                    log::info!(
                        "Auto-discovered {} worker threads with prefix \"{}\"",
                        workers.len(),
                        disc_prefix
                    );
                    return Ok(workers);
                }
            }
        }
    }

    // Step 3: Nothing found — show diagnostics to help the user
    log_discovery_failure(&threads, prefix, discovered.as_deref());

    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── identify_tokio_workers integration tests ────────────────────────

    #[test]
    fn test_identify_workers_self_process() {
        // The test process doesn't run a Tokio runtime, so an explicit
        // prefix search should succeed (no /proc error) but find zero workers.
        #[allow(clippy::cast_possible_wrap)]
        let pid = Pid(std::process::id() as i32);
        let result = identify_tokio_workers(pid, Some("tokio-runtime-w"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_identify_workers_invalid_pid() {
        // Non-existent PID should fail when reading /proc
        let result = identify_tokio_workers(Pid(9_999_999), Some("tokio-runtime-w"));
        assert!(result.is_err());
    }

    // ── strip_numeric_suffix unit tests ─────────────────────────────────

    #[test]
    fn test_strip_suffix_with_numbers() {
        assert_eq!(strip_numeric_suffix("my-pool-0"), Some("my-pool"));
        assert_eq!(strip_numeric_suffix("my-pool-12"), Some("my-pool"));
        assert_eq!(strip_numeric_suffix("a-b-c-99"), Some("a-b-c"));
    }

    #[test]
    fn test_strip_suffix_without_numbers() {
        // No dash at all
        assert_eq!(strip_numeric_suffix("main"), None);
        // Suffix is not numeric
        assert_eq!(strip_numeric_suffix("signal-handler"), None);
        // Trailing dash with nothing after it
        assert_eq!(strip_numeric_suffix("foo-"), None);
        // Mixed suffix (not purely digits)
        assert_eq!(strip_numeric_suffix("foo-1bar"), None);
    }

    // ── discover_worker_prefix unit tests ───────────────────────────────

    #[test]
    fn test_discover_prefix_numbered_threads() {
        // Threads with sequential -N suffixes should group under the base name
        let threads = vec![
            (1, "main".to_string()),
            (2, "my-pool-0".to_string()),
            (3, "my-pool-1".to_string()),
            (4, "my-pool-2".to_string()),
            (5, "signal-handler".to_string()),
        ];
        assert_eq!(discover_worker_prefix(&threads), Some("my-pool".to_string()));
    }

    #[test]
    fn test_discover_prefix_truncated_comm() {
        // When /proc truncates at 15 chars, all workers share the same comm.
        // They should still be detected as a group via exact-name matching.
        let threads = vec![
            (1, "main".to_string()),
            (2, "tokio-runtime-w".to_string()),
            (3, "tokio-runtime-w".to_string()),
            (4, "tokio-runtime-w".to_string()),
            (5, "tokio-runtime-b".to_string()),
        ];
        assert_eq!(discover_worker_prefix(&threads), Some("tokio-runtime-w".to_string()));
    }

    #[test]
    fn test_discover_prefix_picks_largest_group() {
        // When multiple pools exist, the largest group should win
        let threads = vec![
            (1, "small-0".to_string()),
            (2, "small-1".to_string()),
            (3, "big-0".to_string()),
            (4, "big-1".to_string()),
            (5, "big-2".to_string()),
            (6, "big-3".to_string()),
        ];
        assert_eq!(discover_worker_prefix(&threads), Some("big".to_string()));
    }

    #[test]
    fn test_discover_prefix_no_group() {
        // All unique threads — no group reaches MIN_POOL_SIZE
        let threads = vec![
            (1, "main".to_string()),
            (2, "signal-handler".to_string()),
            (3, "logger".to_string()),
        ];
        assert_eq!(discover_worker_prefix(&threads), None);
    }
}
