//! Trace data models for TUI display
//!
//! This module contains the data structures for live profiling mode.

use std::collections::HashSet;
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

/// Internal data model for profiler trace.
/// Uses Arc for cheap cloning when converting from `LiveData`.
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
            let workers_vec = Arc::make_mut(&mut self.workers);
            workers_vec.push(event.worker_id);
            workers_vec.sort_unstable();
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
