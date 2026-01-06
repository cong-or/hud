//! Trace export functionality
//!
//! This module provides functionality for exporting profiling data to various formats.
//! Currently supports Trace Event Format for visualization in tools like Perfetto, Speedscope, and others.

pub mod trace_event;

pub use trace_event::TraceEventExporter;
