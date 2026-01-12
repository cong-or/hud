//! Pre-flight checks for hud
//!
//! Validates system requirements before attempting to load eBPF programs.
//! Provides clear, actionable error messages when requirements aren't met.

#![allow(unsafe_code)] // geteuid() requires unsafe

use anyhow::{bail, Context, Result};
use object::{Object, ObjectSection};
use std::path::Path;

/// Minimum kernel version required for eBPF features used by hud
const MIN_KERNEL_VERSION: (u32, u32) = (5, 8);

/// Run all pre-flight checks before eBPF loading
pub fn run_preflight_checks(target_path: &str, quiet: bool) -> Result<()> {
    check_privileges()?;
    check_kernel_version()?;
    check_binary_exists(target_path)?;
    check_debug_symbols(target_path, quiet)?;
    Ok(())
}

/// Check if running with sufficient privileges for eBPF
fn check_privileges() -> Result<()> {
    // Check if running as root
    if unsafe { libc::geteuid() } == 0 {
        return Ok(());
    }

    // Not root - check for CAP_BPF and CAP_PERFMON (Linux 5.8+)
    // For simplicity, we'll just require root for now since capability
    // checking requires additional dependencies
    bail!(
        "Permission denied: hud requires root privileges to load eBPF programs.\n\n\
         Run with: sudo hud ..."
    );
}

/// Check if the kernel version is sufficient for eBPF features
fn check_kernel_version() -> Result<()> {
    let version_str = std::fs::read_to_string("/proc/version")
        .context("Failed to read kernel version from /proc/version")?;

    // Parse version like "Linux version 5.15.0-generic ..." or "Linux version 6.1.0-arch1-1 ..."
    let release = version_str.split_whitespace().nth(2).unwrap_or("unknown");

    let version_parts: Vec<&str> = release.split('.').collect();
    if version_parts.len() < 2 {
        // Can't parse, assume it's fine
        return Ok(());
    }

    let major: u32 = version_parts[0].parse().unwrap_or(0);
    let minor: u32 = version_parts[1]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0);

    if (major, minor) < MIN_KERNEL_VERSION {
        bail!(
            "Kernel version {}.{} is too old.\n\n\
             hud requires Linux {}.{} or newer for eBPF ring buffer support.\n\
             Current kernel: {}",
            major,
            minor,
            MIN_KERNEL_VERSION.0,
            MIN_KERNEL_VERSION.1,
            release
        );
    }

    Ok(())
}

/// Check if the target binary exists and is readable
fn check_binary_exists(target_path: &str) -> Result<()> {
    let path = Path::new(target_path);
    if !path.exists() {
        bail!(
            "Binary not found: {}\n\n\
             Make sure the path is correct and the binary exists.",
            target_path
        );
    }
    if !path.is_file() {
        bail!(
            "Not a file: {}\n\n\
             --target must point to an executable file, not a directory.",
            target_path
        );
    }
    Ok(())
}

/// Check if the binary has debug symbols for proper stack trace resolution
fn check_debug_symbols(target_path: &str, quiet: bool) -> Result<()> {
    if quiet {
        return Ok(());
    }

    let file_data = std::fs::read(target_path)
        .with_context(|| format!("Failed to read binary: {target_path}"))?;

    let obj = match object::File::parse(&*file_data) {
        Ok(obj) => obj,
        Err(_) => {
            // Not a valid object file, let later stages handle it
            return Ok(());
        }
    };

    // Check for .debug_info section (DWARF debug info)
    let has_debug_info = obj.section_by_name(".debug_info").is_some_and(|s| s.size() > 0);

    // Check for .symtab (symbol table - present in non-stripped binaries)
    let has_symtab = obj.section_by_name(".symtab").is_some_and(|s| s.size() > 0);

    if !has_debug_info && !has_symtab {
        eprintln!("warning: binary stripped, stack traces will show addresses only");
    } else if !has_debug_info {
        eprintln!("warning: no DWARF debug info, source locations unavailable");
    }

    Ok(())
}

/// Check if the target process exists
pub fn check_process_exists(pid: i32) -> Result<()> {
    let proc_path = format!("/proc/{pid}");
    if !Path::new(&proc_path).exists() {
        bail!(
            "Process {pid} not found.\n\n\
             Is the process still running? Check with: ps -p {pid}"
        );
    }
    Ok(())
}

/// Check if we can read the process's memory maps
pub fn check_proc_access(pid: i32) -> Result<()> {
    let maps_path = format!("/proc/{pid}/maps");
    std::fs::read_to_string(&maps_path).with_context(|| {
        format!(
            "Cannot read {maps_path}\n\n\
             This usually means:\n\
             - The process doesn't exist (check: ps -p {pid})\n\
             - Permission denied (run with sudo)\n\
             - /proc is not mounted"
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_version_check() {
        // This should pass on any modern system
        let result = check_kernel_version();
        // Don't assert success since test might run on old kernel
        // Just ensure it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_binary_not_found() {
        let result = check_binary_exists("/nonexistent/path/to/binary");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Binary not found"));
    }

    #[test]
    fn test_process_not_found() {
        let result = check_process_exists(999_999_999);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }
}
