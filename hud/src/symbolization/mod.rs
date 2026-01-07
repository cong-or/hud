//! # Symbol Resolution and Address Translation
//!
//! Converts raw memory addresses from eBPF stack traces into human-readable
//! function names, file paths, and line numbers using DWARF debug information.
//!
//! ## Key Components
//!
//! - **`symbolizer`** - DWARF-based symbol resolution with caching
//! - **`memory_maps`** - Parse `/proc/pid/maps` for PIE/ASLR base address adjustment
//!
//! ## Address Translation
//!
//! 1. Get runtime address from eBPF stack trace (e.g., `0x55f3a2b4c780`)
//! 2. Parse `/proc/<pid>/maps` to find binary's memory range
//! 3. Calculate file offset: `runtime_addr - base_address`
//! 4. Look up in DWARF debug info to get function/file/line
//! 5. Demangle Rust symbol names
//!
//! See [Architecture docs](../../docs/ARCHITECTURE.md) for details on DWARF, PIE, and ASLR.

pub mod memory_maps;
pub mod symbolizer;

pub use memory_maps::{parse_memory_maps, MemoryRange};
pub use symbolizer::Symbolizer;
