//! Trace data models for TUI display
//!
//! This module contains the data structures for both live and replay modes

use anyhow::Result;
use std::path::Path;
use std::collections::HashSet;

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
#[derive(Debug)]
pub struct TraceData {
    pub events: Vec<TraceEvent>,
    pub workers: Vec<u32>,
    pub duration: f64,
}

/// Live data model that grows as events arrive
#[derive(Debug)]
pub struct LiveData {
    pub events: Vec<TraceEvent>,
    workers_set: HashSet<u32>,
    pub workers: Vec<u32>,
    pub duration: f64,
    start_time: Option<f64>,
}

impl LiveData {
    /// Create a new empty live data set
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            workers_set: HashSet::new(),
            workers: Vec::new(),
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

        // Track workers
        if self.workers_set.insert(event.worker_id) {
            self.workers.push(event.worker_id);
            self.workers.sort();
        }

        // Update duration
        if let Some(start) = self.start_time {
            self.duration = event.timestamp - start;
        }

        self.events.push(event);
    }

    /// Get event count
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Convert to TraceData for compatibility with existing TUI code
    pub fn as_trace_data(&self) -> TraceData {
        TraceData {
            events: self.events.clone(),
            workers: self.workers.clone(),
            duration: self.duration,
        }
    }
}

impl TraceData {
    /// Parse trace.json into our internal representation
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        let mut events = Vec::new();
        let mut workers = std::collections::HashSet::new();
        let mut max_timestamp = 0.0f64;

        if let Some(trace_events) = json["traceEvents"].as_array() {
            for event in trace_events {
                // Only process "B" (begin) events for now
                if event["ph"].as_str() != Some("B") {
                    continue;
                }

                let name = event["name"].as_str().unwrap_or("unknown").to_string();
                let worker_id = event["args"]["worker_id"].as_u64().unwrap_or(0) as u32;
                let tid = event["tid"].as_u64().unwrap_or(0) as u32;
                let timestamp = event["ts"].as_f64().unwrap_or(0.0) / 1_000_000.0; // Convert µs to seconds
                let cpu = event["args"]["cpu_id"].as_u64().unwrap_or(0) as u32;
                let detection_method = event["args"]["detection_method"].as_u64().map(|v| v as u32);
                let file = event["args"]["file"].as_str().map(|s| s.to_string());
                let line = event["args"]["line"].as_u64().map(|v| v as u32);

                workers.insert(worker_id);
                max_timestamp = max_timestamp.max(timestamp);

                events.push(TraceEvent {
                    name,
                    worker_id,
                    tid,
                    timestamp,
                    cpu,
                    detection_method,
                    file,
                    line,
                });
            }
        }

        let mut workers: Vec<u32> = workers.into_iter().collect();
        workers.sort();

        Ok(TraceData {
            events,
            workers,
            duration: max_timestamp,
        })
    }
}
