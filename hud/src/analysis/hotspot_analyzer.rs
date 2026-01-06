//! Hotspot analysis for profiling data
//!
//! Aggregates trace events by function name to identify performance hotspots

// Percentage calculations intentionally convert usize to f64
#![allow(clippy::cast_precision_loss)]

use crate::trace_data::TraceData;
use std::collections::HashMap;

/// A function hotspot with aggregated statistics
#[derive(Debug, Clone)]
pub struct FunctionHotspot {
    pub name: String,
    pub count: usize,
    pub percentage: f64,
    pub workers: HashMap<u32, usize>, // worker_id -> count
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Aggregated function data: (worker counts, file, line)
type FunctionData = (HashMap<u32, usize>, Option<String>, Option<u32>);

/// Analyze trace data to identify function hotspots
///
/// This function aggregates trace events by function name, counts occurrences,
/// calculates percentages, and sorts by frequency (descending).
///
/// # Arguments
/// * `data` - The trace data to analyze
///
/// # Returns
/// A vector of function hotspots sorted by count (most frequent first)
#[must_use]
pub fn analyze_hotspots(data: &TraceData) -> Vec<FunctionHotspot> {
    // Aggregate events by function name, capturing file/line from first occurrence
    let mut function_data: HashMap<String, FunctionData> = HashMap::new();

    for event in data.events.iter() {
        let entry = function_data
            .entry(event.name.clone())
            .or_insert_with(|| (HashMap::new(), event.file.clone(), event.line));
        *entry.0.entry(event.worker_id).or_insert(0) += 1;
    }

    // Convert to vector and calculate percentages
    let total_samples = data.events.len();
    let mut hotspots: Vec<FunctionHotspot> = function_data
        .into_iter()
        .map(|(name, (workers, file, line))| {
            let count: usize = workers.values().sum();
            let percentage = (count as f64 / total_samples as f64) * 100.0;
            FunctionHotspot { name, count, percentage, workers, file, line }
        })
        .collect();

    // Sort by count (descending)
    hotspots.sort_by(|a, b| b.count.cmp(&a.count));

    hotspots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace_data::TraceEvent;

    fn create_test_data() -> TraceData {
        use std::sync::Arc;

        TraceData {
            events: Arc::new(vec![
                TraceEvent {
                    name: "function_a".to_string(),
                    worker_id: 0,
                    tid: 100,
                    timestamp: 1.0,
                    cpu: 0,
                    detection_method: None,
                    file: Some("src/a.rs".to_string()),
                    line: Some(10),
                },
                TraceEvent {
                    name: "function_a".to_string(),
                    worker_id: 1,
                    tid: 101,
                    timestamp: 2.0,
                    cpu: 1,
                    detection_method: None,
                    file: Some("src/a.rs".to_string()),
                    line: Some(10),
                },
                TraceEvent {
                    name: "function_b".to_string(),
                    worker_id: 0,
                    tid: 100,
                    timestamp: 3.0,
                    cpu: 0,
                    detection_method: None,
                    file: Some("src/b.rs".to_string()),
                    line: Some(20),
                },
            ]),
            workers: Arc::new(vec![0, 1]),
            duration: 3.0,
        }
    }

    #[test]
    fn test_analyze_hotspots_aggregates_by_function() {
        let data = create_test_data();
        let hotspots = analyze_hotspots(&data);

        assert_eq!(hotspots.len(), 2);
        assert_eq!(hotspots[0].name, "function_a"); // Most frequent first
        assert_eq!(hotspots[0].count, 2);
        assert_eq!(hotspots[1].name, "function_b");
        assert_eq!(hotspots[1].count, 1);
    }

    #[test]
    fn test_analyze_hotspots_calculates_percentages() {
        let data = create_test_data();
        let hotspots = analyze_hotspots(&data);

        assert!((hotspots[0].percentage - 66.666).abs() < 0.01); // 2/3 * 100
        assert!((hotspots[1].percentage - 33.333).abs() < 0.01); // 1/3 * 100
    }

    #[test]
    fn test_analyze_hotspots_tracks_workers() {
        let data = create_test_data();
        let hotspots = analyze_hotspots(&data);

        let func_a = &hotspots[0];
        assert_eq!(func_a.workers.len(), 2);
        assert_eq!(func_a.workers[&0], 1);
        assert_eq!(func_a.workers[&1], 1);

        let func_b = &hotspots[1];
        assert_eq!(func_b.workers.len(), 1);
        assert_eq!(func_b.workers[&0], 1);
    }

    #[test]
    fn test_analyze_hotspots_preserves_source_location() {
        let data = create_test_data();
        let hotspots = analyze_hotspots(&data);

        assert_eq!(hotspots[0].file, Some("src/a.rs".to_string()));
        assert_eq!(hotspots[0].line, Some(10));
        assert_eq!(hotspots[1].file, Some("src/b.rs".to_string()));
        assert_eq!(hotspots[1].line, Some(20));
    }
}
