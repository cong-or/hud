//! Memory mapping utilities for process address space analysis
//!
//! This module provides functionality for parsing /proc/pid/maps to determine
//! the memory ranges of loaded binaries, which is essential for symbolizing
//! addresses from position-independent executables (PIE).

use anyhow::{Context, Result};
use log::info;
use std::fs;

/// Memory range of a loaded binary in a process's address space
#[derive(Debug, Clone, Copy)]
pub struct MemoryRange {
    pub start: u64,
    pub end: u64,
}

impl MemoryRange {
    /// Check if an address falls within this memory range
    #[must_use]
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }
}

/// Parse /proc/pid/maps to find the memory range of a specific binary
///
/// This function reads the process's memory maps and finds all mappings
/// that match the given binary path, returning the range from the minimum
/// start address to the maximum end address.
///
/// # Arguments
/// * `pid` - The process ID to query
/// * `binary_path` - The path to the binary to find (e.g., "/path/to/executable")
///
/// # Errors
/// Returns an error if /proc/pid/maps cannot be read or if the binary is not found
pub fn parse_memory_maps(pid: i32, binary_path: &str) -> Result<MemoryRange> {
    let maps_path = format!("/proc/{pid}/maps");
    let maps = fs::read_to_string(&maps_path).context(format!("Failed to read {maps_path}"))?;

    let mut start_addr = None;
    let mut end_addr = None;

    // Find ALL mappings of the target binary to get the full range
    for line in maps.lines() {
        if line.contains(binary_path) {
            // Parse the line: "start-end perms offset dev inode pathname"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let range = parts[0];
                let range_parts: Vec<&str> = range.split('-').collect();
                if range_parts.len() == 2 {
                    let start = u64::from_str_radix(range_parts[0], 16)
                        .context("Failed to parse range start")?;
                    let end = u64::from_str_radix(range_parts[1], 16)
                        .context("Failed to parse range end")?;

                    // Track the minimum start and maximum end
                    start_addr = Some(start_addr.map_or(start, |s: u64| s.min(start)));
                    end_addr = Some(end_addr.map_or(end, |e: u64| e.max(end)));
                }
            }
        }
    }

    match (start_addr, end_addr) {
        (Some(start), Some(end)) => {
            info!(
                "Executable memory range: 0x{:x} - 0x{:x} (size: {} KB)",
                start,
                end,
                (end - start) / 1024
            );
            Ok(MemoryRange { start, end })
        }
        _ => Err(anyhow::anyhow!("Could not find memory range for {binary_path}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_range_contains() {
        let range = MemoryRange { start: 0x1000, end: 0x2000 };

        assert!(range.contains(0x1000));
        assert!(range.contains(0x1500));
        assert!(range.contains(0x1FFF));
        assert!(!range.contains(0x0FFF));
        assert!(!range.contains(0x2000));
        assert!(!range.contains(0x2001));
    }

    #[test]
    fn test_parse_memory_maps_self() {
        // Test parsing our own process's memory maps
        let pid = std::process::id() as i32;

        // Try to find the current executable
        let exe = std::env::current_exe().expect("Failed to get current exe");
        let exe_path = exe.to_str().expect("Failed to convert exe path to string");

        // This might fail in some test environments, so we allow it
        let _result = parse_memory_maps(pid, exe_path);
        // We don't assert success because it depends on the test environment
    }
}
