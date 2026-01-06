//! Analysis logic for profiling data
//!
//! This module contains pure business logic for analyzing profiling traces,
//! separated from the TUI presentation layer.

pub mod hotspot_analyzer;

pub use hotspot_analyzer::{analyze_hotspots, FunctionHotspot};
