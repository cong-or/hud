//! Trace export functionality
//!
//! This module provides functionality for exporting profiling data to various formats.
//! Currently supports Chrome Trace Event Format for visualization in chrome://tracing.

pub mod chrome_trace;

pub use chrome_trace::ChromeTraceExporter;
