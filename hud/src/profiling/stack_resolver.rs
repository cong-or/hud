//! Stack trace resolution and display
//!
//! This module consolidates stack trace resolution logic that was previously
//! duplicated in 3 places in main.rs (~150 lines of duplication eliminated).

use anyhow::Result;
use aya::maps::{MapData, StackTraceMap};
use log::info;
use std::borrow::Borrow;

use crate::domain::StackId;
use crate::symbolization::{MemoryRange, Symbolizer};

/// Stack trace resolver - handles resolving and displaying stack traces
///
/// This type consolidates the logic for:
/// - Fetching stack traces from eBPF maps
/// - Adjusting addresses for PIE executables
/// - Resolving addresses to symbols
/// - Formatting and printing stack traces
pub struct StackResolver<'a> {
    symbolizer: &'a Symbolizer,
    memory_range: Option<MemoryRange>,
}

impl<'a> StackResolver<'a> {
    /// Create a new stack resolver
    pub fn new(symbolizer: &'a Symbolizer, memory_range: Option<MemoryRange>) -> Self {
        Self { symbolizer, memory_range }
    }

    /// Resolve and print a stack trace from an eBPF stack trace map
    ///
    /// This is the single source of truth for stack trace resolution.
    /// Previously this logic was duplicated in 3 places in main.rs.
    ///
    /// # Errors
    /// Returns an error if stack trace lookup from eBPF map fails
    pub fn resolve_and_print<T: Borrow<MapData>>(
        &self,
        stack_id: StackId,
        stack_traces: &StackTraceMap<T>,
    ) -> Result<()> {
        // Handle invalid stack IDs
        if !stack_id.is_valid() {
            println!("\n   ⚠️  No stack trace captured (stack_id = {})", stack_id.0);
            return Ok(());
        }

        // Fetch the stack trace from eBPF
        let stack_trace = match stack_traces.get(&stack_id.as_map_key(), 0) {
            Ok(trace) => trace,
            Err(e) => {
                println!("\n   ⚠️  Failed to read stack trace: {e}");
                return Ok(());
            }
        };

        let frames = stack_trace.frames();

        if frames.is_empty() {
            println!("\n   ⚠️  Empty stack trace");
            return Ok(());
        }

        println!("\n   📍 Stack trace:");
        info!("Stack trace has {} frames", frames.len());

        // Iterate through frames and resolve symbols
        for (i, stack_frame) in frames.iter().enumerate() {
            let addr = stack_frame.ip;

            // Stop at null addresses
            if addr == 0 {
                info!("Frame {i} has address 0, stopping");
                break;
            }

            // Adjust address and determine if it's in the main executable
            let (file_offset, in_executable) = self.adjust_address(addr);

            // Symbolize and print
            if in_executable {
                let resolved_frame = self.symbolizer.resolve(file_offset);
                println!("      {}", resolved_frame.format(i));
            } else {
                // Shared library - show address but don't symbolize
                println!("      #{i:<2} 0x{addr:016x} <shared library>");
            }
        }

        Ok(())
    }

    /// Adjust an address for PIE executables
    ///
    /// Returns (`adjusted_address`, `is_in_executable`)
    fn adjust_address(&self, addr: u64) -> (u64, bool) {
        if let Some(range) = self.memory_range {
            if range.contains(addr) {
                // Address is in main executable, adjust to file offset
                let adjusted = addr - range.start;
                info!("Address 0x{addr:016x} (in executable) -> 0x{adjusted:08x}");
                (adjusted, true)
            } else {
                // Address is outside executable (shared library)
                info!("Address 0x{addr:016x} (shared library, skipping)");
                (addr, false)
            }
        } else {
            // No range info, use address as-is
            (addr, true)
        }
    }

    /// Get the top frame address from a stack trace (for symbolization)
    ///
    /// Returns None if the stack ID is invalid or the stack trace is empty.
    pub fn get_top_frame_addr<T: Borrow<MapData>>(
        stack_id: StackId,
        stack_traces: &StackTraceMap<T>,
    ) -> Option<u64> {
        if !stack_id.is_valid() {
            return None;
        }

        match stack_traces.get(&stack_id.as_map_key(), 0) {
            Ok(stack_trace) => {
                let frames = stack_trace.frames();
                frames.iter().find(|f| f.ip != 0).map(|f| f.ip)
            }
            Err(_) => None,
        }
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
    fn test_adjust_address_in_executable() {
        let symbolizer = Symbolizer::new("/bin/ls").unwrap();
        let range = MemoryRange { start: 0x7f00_0000_0000, end: 0x7f00_0010_0000 };
        let resolver = StackResolver::new(&symbolizer, Some(range));

        let (adjusted, in_exec) = resolver.adjust_address(0x7f00_0005_0000);
        assert_eq!(adjusted, 0x5_0000);
        assert!(in_exec);
    }

    #[test]
    fn test_adjust_address_shared_library() {
        let symbolizer = Symbolizer::new("/bin/ls").unwrap();
        let range = MemoryRange { start: 0x7f00_0000_0000, end: 0x7f00_0010_0000 };
        let resolver = StackResolver::new(&symbolizer, Some(range));

        let (adjusted, in_exec) = resolver.adjust_address(0x7f00_0100_0000);
        assert_eq!(adjusted, 0x7f00_0100_0000); // Unchanged
        assert!(!in_exec);
    }

    #[test]
    fn test_adjust_address_no_range() {
        let symbolizer = Symbolizer::new("/bin/ls").unwrap();
        let resolver = StackResolver::new(&symbolizer, None);

        let (adjusted, in_exec) = resolver.adjust_address(0x1234_5678);
        assert_eq!(adjusted, 0x1234_5678); // Unchanged
        assert!(in_exec); // Assume in executable when no range
    }
}
