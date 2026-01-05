use anyhow::{Context, Result};
use runtime_scope_common::{TaskEvent, TRACE_EXECUTION_END, TRACE_EXECUTION_START};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::io::Write;

use crate::symbolization::{MemoryRange, Symbolizer};

/// Chrome Trace Event format
/// Spec: https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU/preview
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChromeTraceEvent {
    /// Event name (usually function name)
    name: String,
    /// Category for filtering/coloring
    cat: String,
    /// Phase: "B" = begin, "E" = end, "X" = complete, "I" = instant
    ph: String,
    /// Timestamp in microseconds
    ts: f64,
    /// Process ID
    pid: u32,
    /// Thread ID
    tid: u32,
    /// Optional arguments (metadata)
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<HashMap<String, JsonValue>>,
}

/// Chrome Trace Format container
#[derive(Debug, Serialize)]
struct ChromeTrace {
    #[serde(rename = "traceEvents")]
    trace_events: Vec<ChromeTraceEvent>,
    #[serde(rename = "displayTimeUnit")]
    display_time_unit: String,
}

/// Chrome trace exporter for timeline visualization
pub struct ChromeTraceExporter {
    /// Collected trace events
    events: Vec<ChromeTraceEvent>,
    /// Symbolizer for resolving stack traces
    symbolizer: Symbolizer,
    /// Cache for resolved symbols (stack_id -> (function name, file, line))
    symbol_cache: HashMap<i64, (String, Option<String>, Option<u32>)>,
    /// Memory range for address adjustment
    memory_range: Option<MemoryRange>,
    /// Start timestamp for relative timing (in nanoseconds)
    start_timestamp_ns: Option<u64>,
}

impl ChromeTraceExporter {
    /// Create a new Chrome trace exporter
    pub fn new(symbolizer: Symbolizer) -> Self {
        Self {
            events: Vec::new(),
            symbolizer,
            symbol_cache: HashMap::new(),
            memory_range: None,
            start_timestamp_ns: None,
        }
    }

    /// Set the memory range for PIE address adjustment
    pub fn set_memory_range(&mut self, range: MemoryRange) {
        self.memory_range = Some(range);
    }

    /// Resolve a symbol from a stack trace
    /// Returns (function_name, file, line)
    fn resolve_symbol(&mut self, stack_id: i64, addr: u64) -> (String, Option<String>, Option<u32>) {
        // Check cache first
        if let Some(cached) = self.symbol_cache.get(&stack_id) {
            return cached.clone();
        }

        // Determine if address is in main executable and adjust accordingly
        let (file_offset, in_executable) = if let Some(range) = self.memory_range {
            if range.contains(addr) {
                // Address is in main executable, adjust to file offset
                (addr - range.start, true)
            } else {
                // Address is outside executable (shared library)
                (addr, false)
            }
        } else {
            // No range info, use address as-is
            (addr, true)
        };

        let (function_name, file, line) = if in_executable {
            // Resolve the symbol
            let resolved = self.symbolizer.resolve(file_offset);

            // Get the outermost (non-inlined) function name and location
            if let Some(frame) = resolved.frames.first() {
                let func = frame.function.clone();
                let file_path = frame.location.as_ref()
                    .and_then(|loc| loc.file.clone());
                let line_num = frame.location.as_ref()
                    .and_then(|loc| loc.line);

                (func, file_path, line_num)
            } else {
                (format!("0x{:x}", addr), None, None)
            }
        } else {
            // For shared libraries, just show address
            (format!("<shared:0x{:x}>", addr), None, None)
        };

        // Cache the complete result (function name, file, line)
        let result = (function_name, file, line);
        self.symbol_cache.insert(stack_id, result.clone());
        result
    }

    /// Add a task event to the trace
    pub fn add_event(&mut self, event: &TaskEvent, top_frame_addr: Option<u64>) {
        // Initialize start timestamp on first event
        if self.start_timestamp_ns.is_none() {
            self.start_timestamp_ns = Some(event.timestamp_ns);
        }

        let start_ts = self.start_timestamp_ns.unwrap();

        // Convert timestamp from nanoseconds to microseconds (relative to start)
        let ts_us = if event.timestamp_ns >= start_ts {
            (event.timestamp_ns - start_ts) as f64 / 1000.0
        } else {
            0.0
        };

        match event.event_type {
            TRACE_EXECUTION_START => {
                // Resolve function name and source location from stack trace
                let (function_name, file, line) = if event.stack_id < 0 {
                    // Stack capture failed (from sched_switch which can't capture user stacks)
                    ("execution".to_string(), None, None)
                } else if let Some(addr) = top_frame_addr {
                    self.resolve_symbol(event.stack_id, addr)
                } else {
                    (format!("trace_{}", event.stack_id), None, None)
                };

                // Create metadata args
                let mut args = HashMap::new();
                args.insert("worker_id".to_string(), serde_json::json!(event.worker_id));
                args.insert("cpu_id".to_string(), serde_json::json!(event.cpu_id));
                if event.task_id != 0 {
                    args.insert("task_id".to_string(), serde_json::json!(event.task_id));
                }
                // Add detection_method to distinguish between sched_switch and perf_event samples
                if event.detection_method != 0 {
                    args.insert("detection_method".to_string(), serde_json::json!(event.detection_method));
                }
                // Add source location if available
                if let Some(file_path) = file {
                    args.insert("file".to_string(), serde_json::json!(file_path));
                }
                if let Some(line_num) = line {
                    args.insert("line".to_string(), serde_json::json!(line_num));
                }

                self.events.push(ChromeTraceEvent {
                    name: function_name,
                    cat: "execution".to_string(),
                    ph: "B".to_string(),  // Begin
                    ts: ts_us,
                    pid: event.pid,
                    tid: event.tid,
                    args: Some(args),
                });
            }
            TRACE_EXECUTION_END => {
                // For end events, we use a generic name since we don't have stack trace
                // The Chrome viewer will match it with the corresponding Begin event
                // Add worker_id to help identify which worker this belongs to
                let mut args = HashMap::new();
                args.insert("worker_id".to_string(), serde_json::json!(event.worker_id));
                args.insert("cpu_id".to_string(), serde_json::json!(event.cpu_id));
                if event.detection_method != 0 {
                    args.insert("detection_method".to_string(), serde_json::json!(event.detection_method));
                }

                self.events.push(ChromeTraceEvent {
                    name: "execution".to_string(),
                    cat: "execution".to_string(),
                    ph: "E".to_string(),  // End
                    ts: ts_us,
                    pid: event.pid,
                    tid: event.tid,
                    args: Some(args),
                });
            }
            _ => {
                // Ignore other event types for now
            }
        }
    }

    /// Export the trace to any writer (file, stdout, buffer, etc.)
    ///
    /// This method accepts any type implementing `Write`, making it flexible
    /// for testing (using in-memory buffers) and production (files, stdout).
    ///
    /// # Example
    /// ```
    /// use runtime_scope::export::ChromeTraceExporter;
    /// use runtime_scope::symbolization::Symbolizer;
    /// use std::fs::File;
    /// use std::io::BufWriter;
    ///
    /// # fn example() -> anyhow::Result<()> {
    /// let binary_path = "/bin/ls";
    /// let symbolizer = Symbolizer::new(binary_path)?;
    /// let exporter = ChromeTraceExporter::new(symbolizer);
    ///
    /// // Write to file
    /// let file = File::create("trace.json")?;
    /// let writer = BufWriter::new(file);
    /// exporter.export(writer)?;
    ///
    /// // Or write to stdout
    /// // exporter.export(std::io::stdout())?;
    ///
    /// // Or write to buffer for testing
    /// let mut buffer = Vec::new();
    /// exporter.export(&mut buffer)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn export<W: Write>(&self, writer: W) -> Result<()> {
        // Add metadata events for thread names
        let mut all_events = self.events.clone();

        // Collect unique (pid, tid, worker_id) tuples
        let mut threads: HashMap<(u32, u32), u32> = HashMap::new();
        for event in &self.events {
            if let Some(ref args) = event.args {
                if let Some(worker_id) = args.get("worker_id").and_then(|v| v.as_u64()) {
                    threads.insert((event.pid, event.tid), worker_id as u32);
                }
            }
        }

        // Generate thread name metadata events
        for ((pid, tid), worker_id) in threads {
            let mut args = HashMap::new();
            args.insert("name".to_string(), serde_json::json!(format!("Worker {}", worker_id)));

            all_events.push(ChromeTraceEvent {
                name: "thread_name".to_string(),
                cat: "".to_string(),
                ph: "M".to_string(),  // Metadata
                ts: 0.0,
                pid,
                tid,
                args: Some(args),
            });
        }

        let trace = ChromeTrace {
            trace_events: all_events,
            display_time_unit: "ms".to_string(),
        };

        serde_json::to_writer_pretty(writer, &trace)
            .context("Failed to write trace JSON")?;

        Ok(())
    }

    /// Get the number of events collected
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}
