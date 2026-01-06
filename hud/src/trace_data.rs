//! Trace data models for TUI display
//!
//! This module contains the data structures for parsed Chrome trace files

use anyhow::Result;
use std::path::Path;

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

/// Internal data model for profiler trace
#[derive(Debug)]
pub struct TraceData {
    pub events: Vec<TraceEvent>,
    pub workers: Vec<u32>,
    pub duration: f64,
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
