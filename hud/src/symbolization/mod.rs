//! Symbol resolution and memory mapping utilities
//!
//! This module provides functionality for:
//! - Resolving instruction pointers to source locations
//! - Parsing process memory maps
//! - Caching symbol resolutions for performance

pub mod memory_maps;
pub mod symbolizer;

pub use memory_maps::{parse_memory_maps, MemoryRange};
pub use symbolizer::Symbolizer;
