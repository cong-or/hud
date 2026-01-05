//! Symbol resolution and memory mapping utilities
//!
//! This module provides functionality for:
//! - Resolving instruction pointers to source locations
//! - Parsing process memory maps
//! - Caching symbol resolutions for performance

pub mod symbolizer;
pub mod memory_maps;

pub use symbolizer::Symbolizer;
pub use memory_maps::{MemoryRange, parse_memory_maps};
