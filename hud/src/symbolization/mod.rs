//! # Symbol Resolution and Address Translation
//!
//! This module handles the complex task of converting raw memory addresses
//! (instruction pointers) captured by eBPF into human-readable function names,
//! file paths, and line numbers. This process is called **symbolization** or
//! **symbol resolution**.
//!
//! ## The Symbolization Problem
//!
//! When eBPF captures a stack trace via `bpf_get_stackid()`, it records raw
//! memory addresses like `0x55f3a2b4c780`. These addresses are meaningless to
//! humans - we need to translate them to:
//! - **Function name**: `tokio::runtime::blocking::pool::spawner::spawn_blocking`
//! - **File path**: `/home/.cargo/registry/src/tokio-1.35.0/src/runtime/blocking/pool.rs`
//! - **Line number**: `42`
//!
//! This module provides that translation using **DWARF debug information**.
//!
//! ## Key Concepts
//!
//! ### DWARF Debug Information
//!
//! **DWARF** is a standardized debugging data format embedded in ELF binaries
//! (when compiled with debug symbols). It contains:
//! - Mapping from instruction addresses → function names
//! - Source file paths and line numbers
//! - Inline function information (for optimized code)
//! - Type information (not used by this profiler)
//!
//! **How to enable DWARF**:
//! ```toml
//! # Cargo.toml
//! [profile.release]
//! debug = true  # Include DWARF debug info in release builds
//! ```
//!
//! **Libraries used**:
//! - `gimli`: Low-level DWARF parser
//! - `addr2line`: High-level symbolization library built on gimli
//! - `object`: ELF binary parser
//!
//! ### PIE (Position Independent Executable)
//!
//! Modern Linux executables are compiled as **PIE** (Position Independent
//! Executables) for security. PIE enables **ASLR** (Address Space Layout
//! Randomization), which randomizes where the program is loaded in memory
//! on each execution. This makes exploits harder.
//!
//! **The Problem**: DWARF debug info uses **file offsets** (0x1000, 0x2000...),
//! but stack traces capture **runtime addresses** (0x55f3a2b4c000, 0x55f3a2b4d000...).
//!
//! **The Solution**: We must translate runtime addresses → file offsets:
//!
//! ```text
//! Runtime Address = Base Address + File Offset
//! File Offset = Runtime Address - Base Address
//! ```
//!
//! ### ASLR (Address Space Layout Randomization)
//!
//! ASLR is a security feature that randomizes the base address where executables
//! and libraries are loaded. This makes memory exploits harder because attackers
//! can't predict memory addresses.
//!
//! **Example**: Same binary loaded twice has different base addresses:
//! ```text
//! Run 1: Base = 0x55f3a2b4c000
//! Run 2: Base = 0x7f8b3c1a0000  (randomized!)
//! ```
//!
//! **How we handle it**: We parse `/proc/<pid>/maps` to find the actual
//! runtime base address for the target binary.
//!
//! ## Address Translation Flow
//!
//! ```text
//! 1. eBPF captures stack trace
//!    Raw addresses: [0x55f3a2b4c780, 0x55f3a2b4d120, ...]
//!
//! 2. Read /proc/<pid>/maps to find binary's memory range
//!    Binary loaded at: 0x55f3a2b4c000 - 0x55f3a2b5f000
//!    Base address: 0x55f3a2b4c000
//!
//! 3. Check if address is within binary's range
//!    if (addr >= 0x55f3a2b4c000 && addr < 0x55f3a2b5f000):
//!      address belongs to our binary
//!
//! 4. Calculate file offset
//!    file_offset = 0x55f3a2b4c780 - 0x55f3a2b4c000 = 0x780
//!
//! 5. Look up file offset in DWARF debug info
//!    0x780 → spawn_blocking() at pool.rs:42
//!
//! 6. Demangle Rust symbol names
//!    _ZN5tokio7runtime8blocking4pool8spawner14spawn_blocking17h...
//!      → tokio::runtime::blocking::pool::spawner::spawn_blocking
//! ```
//!
//! ## Module Structure
//!
//! - **`symbolizer`**: Core symbolization logic using DWARF
//!   - Loads DWARF debug info from binary
//!   - Resolves addresses to function/file/line
//!   - Caches resolved symbols for performance
//!   - Demangles Rust symbol names
//!
//! - **`memory_maps`**: Process memory map parsing
//!   - Parses `/proc/<pid>/maps`
//!   - Finds binary's base address (for PIE adjustment)
//!   - Returns memory range (start, end)
//!
//! ## Example: Symbolizing a Stack Trace
//!
//! ```rust,ignore
//! // 1. Parse memory maps to get PIE base address
//! let memory_range = parse_memory_maps(pid, "/path/to/binary")?;
//! // memory_range.start = 0x55f3a2b4c000
//!
//! // 2. Create symbolizer
//! let symbolizer = Symbolizer::new("/path/to/binary")?;
//!
//! // 3. Get raw address from eBPF stack trace
//! let runtime_addr = 0x55f3a2b4c780;
//!
//! // 4. Adjust for PIE if within binary's range
//! let file_offset = if memory_range.contains(runtime_addr) {
//!     runtime_addr - memory_range.start  // 0x780
//! } else {
//!     runtime_addr  // External library, use as-is
//! };
//!
//! // 5. Resolve to symbol
//! let resolved = symbolizer.resolve(file_offset);
//! // resolved.frames[0].function = "spawn_blocking"
//! // resolved.frames[0].location.file = "pool.rs"
//! // resolved.frames[0].location.line = 42
//! ```
//!
//! ## Performance Considerations
//!
//! - **Caching**: Symbolizer caches resolved addresses to avoid re-parsing DWARF
//! - **Lazy Resolution**: Only resolve addresses when displaying (not during capture)
//! - **DWARF Parsing**: O(log N) lookup in DWARF line number table
//!
//! ## Limitations
//!
//! - **Requires debug symbols**: Binary must be compiled with `debug = true`
//! - **PIE only**: Assumes modern PIE executables (pre-2015 binaries may differ)
//! - **Rust binaries**: Demangling assumes Rust symbol format
//! - **Inlining**: Optimized code may inline functions (DWARF tracks this)
//!
//! ## References
//!
//! - [DWARF Debugging Format](http://dwarfstd.org/)
//! - [PIE and ASLR](https://en.wikipedia.org/wiki/Address_space_layout_randomization)
//! - [Linux `/proc/pid/maps` format](https://man7.org/linux/man-pages/man5/proc.5.html)

pub mod memory_maps;
pub mod symbolizer;

pub use memory_maps::{parse_memory_maps, MemoryRange};
pub use symbolizer::Symbolizer;
