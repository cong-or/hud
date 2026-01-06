//! Trace data models for TUI display
//!
//! This module contains the data structures for both live and replay modes

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// Represents a single trace event
#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub name: String,
    pub worker_id: u32,
    pub tid: u32,
    pub timestamp: f64,
    pub cpu: u32,
    pub detection_method: Option<u32>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Internal data model for profiler trace (immutable, loaded from file)
/// Uses Arc for cheap cloning when converting from `LiveData`
#[derive(Debug, Clone)]
pub struct TraceData {
    pub events: Arc<Vec<TraceEvent>>,
    pub workers: Arc<Vec<u32>>,
    pub duration: f64,
}

/// Live data model that grows as events arrive
/// Uses Arc internally for cheap conversion to `TraceData`
#[derive(Debug)]
pub struct LiveData {
    events: Arc<Vec<TraceEvent>>,
    workers_set: HashSet<u32>,
    workers: Arc<Vec<u32>>,
    pub duration: f64,
    start_time: Option<f64>,
}

impl Default for LiveData {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveData {
    /// Create a new empty live data set
    #[must_use]
    pub fn new() -> Self {
        Self {
            events: Arc::new(Vec::new()),
            workers_set: HashSet::new(),
            workers: Arc::new(Vec::new()),
            duration: 0.0,
            start_time: None,
        }
    }

    /// Add a new event to the live data
    pub fn add_event(&mut self, event: TraceEvent) {
        // Set start time from first event
        if self.start_time.is_none() {
            self.start_time = Some(event.timestamp);
        }

        // Track workers (update when new worker appears)
        if self.workers_set.insert(event.worker_id) {
            let mut workers_vec = Arc::make_mut(&mut self.workers).clone();
            workers_vec.push(event.worker_id);
            workers_vec.sort_unstable();
            self.workers = Arc::new(workers_vec);
        }

        // Update duration
        if let Some(start) = self.start_time {
            self.duration = event.timestamp - start;
        }

        // Add event to the Arc'd Vec
        Arc::make_mut(&mut self.events).push(event);
    }

    /// Get event count
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Convert to `TraceData` for compatibility with existing TUI code
    /// This is now a cheap clone (just Arc reference counting) instead of deep cloning
    #[must_use]
    pub fn as_trace_data(&self) -> TraceData {
        TraceData {
            events: Arc::clone(&self.events),
            workers: Arc::clone(&self.workers),
            duration: self.duration,
        }
    }
}

impl TraceData {
    /// Parse trace.json into our internal representation
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or parsed as JSON
    #[allow(clippy::cast_possible_truncation)]
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        // Parse trace events using functional pipeline: filter "B" events, then map to TraceEvent
        let events: Vec<TraceEvent> = json["traceEvents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|event| event["ph"].as_str() == Some("B"))
            .map(|event| {
                let name = event["name"].as_str().unwrap_or("unknown").to_string();
                let worker_id = event["args"]["worker_id"].as_u64().unwrap_or(0) as u32;
                let tid = event["tid"].as_u64().unwrap_or(0) as u32;
                let timestamp = event["ts"].as_f64().unwrap_or(0.0) / 1_000_000.0; // Convert µs to seconds
                let cpu = event["args"]["cpu_id"].as_u64().unwrap_or(0) as u32;
                let detection_method = event["args"]["detection_method"].as_u64().map(|v| v as u32);
                let file = event["args"]["file"].as_str().map(std::string::ToString::to_string);
                let line = event["args"]["line"].as_u64().map(|v| v as u32);

                TraceEvent {
                    name,
                    worker_id,
                    tid,
                    timestamp,
                    cpu,
                    detection_method,
                    file,
                    line,
                }
            })
            .collect();

        // Extract unique workers and max timestamp
        let mut workers: Vec<u32> = events
            .iter()
            .map(|e| e.worker_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        workers.sort_unstable();

        let max_timestamp = events
            .iter()
            .map(|e| e.timestamp)
            .fold(0.0f64, f64::max);

        Ok(TraceData {
            events: Arc::new(events),
            workers: Arc::new(workers),
            duration: max_timestamp,
        })
    }
}
