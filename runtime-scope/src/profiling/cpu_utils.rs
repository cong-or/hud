//! CPU utility functions
//!
//! Utilities for querying CPU information from /sys filesystem.

use anyhow::{Context, Result};
use std::fs;

use crate::domain::CpuId;

/// Get list of online CPU IDs from /sys/devices/system/cpu/online
///
/// Returns a vector of CPU IDs (e.g., [0, 1, 2, 3] for a 4-core system).
/// The format in /sys is like "0-3" or "0-3,8-11" for NUMA systems.
pub fn online_cpus() -> Result<Vec<CpuId>> {
    let content = fs::read_to_string("/sys/devices/system/cpu/online")
        .context("Failed to read /sys/devices/system/cpu/online")?;

    let mut cpus = Vec::new();

    for range in content.trim().split(',') {
        if let Some((start, end)) = range.split_once('-') {
            // Range like "0-3"
            let start: u32 = start.parse()?;
            let end: u32 = end.parse()?;
            for cpu in start..=end {
                cpus.push(CpuId(cpu));
            }
        } else {
            // Single CPU like "5"
            let cpu: u32 = range.parse()?;
            cpus.push(CpuId(cpu));
        }
    }

    Ok(cpus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_online_cpus() {
        // This test relies on /sys being available (Linux only)
        let result = online_cpus();

        #[cfg(target_os = "linux")]
        {
            assert!(result.is_ok(), "Failed to read online CPUs");
            let cpus = result.unwrap();
            assert!(!cpus.is_empty(), "Should have at least one CPU");

            // CPU 0 should always exist
            assert!(cpus.contains(&CpuId(0)));

            // CPUs should be in ascending order
            for i in 1..cpus.len() {
                assert!(cpus[i].0 >= cpus[i - 1].0);
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // On non-Linux, this should fail
            assert!(result.is_err());
        }
    }
}
