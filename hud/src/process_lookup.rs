//! Auto-detect process PID and binary path from process name.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Result of process lookup.
#[derive(Debug)]
pub struct ProcessInfo {
    pub pid: i32,
    pub exe_path: PathBuf,
    pub command: String,
}

/// Find a process by name.
///
/// Searches `/proc` for processes matching the given name.
/// Matches against the command name from `/proc/<pid>/stat` and
/// the executable basename from `/proc/<pid>/exe`.
///
/// # Errors
/// - No processes found
/// - Multiple processes found (ambiguous)
pub fn find_process_by_name(name: &str) -> Result<ProcessInfo> {
    let mut matches: Vec<ProcessInfo> = Vec::new();

    let proc_dir = fs::read_dir("/proc").context("Failed to read /proc")?;

    for entry in proc_dir.flatten() {
        let file_name = entry.file_name();
        let pid_str = file_name.to_string_lossy();

        let Ok(pid) = pid_str.parse::<i32>() else {
            continue;
        };

        // Skip kernel threads and inaccessible processes
        let exe_link = format!("/proc/{pid}/exe");
        let Ok(exe_path) = fs::read_link(&exe_link) else {
            continue;
        };

        // Get command name from stat
        let stat_path = format!("/proc/{pid}/stat");
        let Ok(stat_content) = fs::read_to_string(&stat_path) else {
            continue;
        };

        let Ok(command) = extract_comm(&stat_content) else {
            continue;
        };

        if is_match(&command, &exe_path, name) {
            matches.push(ProcessInfo { pid, exe_path, command });
        }
    }

    match matches.len() {
        0 => bail!(
            "No process matching '{name}' found.\n\
             Check running processes with: ps aux | grep {name}"
        ),
        1 => Ok(matches.remove(0)),
        _ => {
            let list: Vec<String> =
                matches.iter().map(|m| format!("  {} ({})", m.pid, m.command)).collect();
            bail!(
                "Multiple processes match '{name}':\n{}\n\n\
                 Specify PID explicitly: hud --pid <PID>",
                list.join("\n")
            )
        }
    }
}

/// Resolve binary path from PID via `/proc/<pid>/exe`.
///
/// # Errors
/// Returns error if the process doesn't exist or `/proc/<pid>/exe` is not readable.
pub fn resolve_exe_path(pid: i32) -> Result<PathBuf> {
    let exe_link = format!("/proc/{pid}/exe");
    fs::read_link(&exe_link).with_context(|| format!("Cannot read {exe_link}"))
}

/// Extract command name from `/proc/<pid>/stat`.
/// Format: "pid (comm) state ..."
fn extract_comm(stat_line: &str) -> Result<String> {
    let open = stat_line.find('(').context("Invalid stat format")?;
    let close = stat_line.rfind(')').context("Invalid stat format")?;
    if open >= close {
        bail!("Invalid stat format");
    }
    Ok(stat_line[open + 1..close].to_string())
}

/// Check if process matches the search pattern.
fn is_match(command: &str, exe_path: &Path, pattern: &str) -> bool {
    let exe_basename = exe_path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let pattern_basename =
        std::path::Path::new(pattern).file_name().and_then(|n| n.to_str()).unwrap_or(pattern);

    // Exact match on command or exe basename
    command == pattern_basename
        || exe_basename == pattern_basename
        // Substring match for flexibility
        || command.contains(pattern)
        || exe_basename.contains(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_comm() {
        let stat = "1234 (my-app) S 1 1234 1234 0 -1 4194304";
        assert_eq!(extract_comm(stat).unwrap(), "my-app");
    }

    #[test]
    fn test_extract_comm_with_parens() {
        // Command names can contain parentheses
        let stat = "1234 (app (v2)) S 1 1234";
        assert_eq!(extract_comm(stat).unwrap(), "app (v2)");
    }

    #[test]
    fn test_is_match() {
        let exe = Path::new("/usr/bin/my-server");
        assert!(is_match("my-server", exe, "my-server"));
        assert!(is_match("my-server", exe, "server"));
        assert!(!is_match("my-server", exe, "other"));
    }
}
