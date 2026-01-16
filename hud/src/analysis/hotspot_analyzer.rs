//! Hotspot analysis for profiling data.
//!
//! This module aggregates trace events by function name to identify performance
//! hotspots - functions that consume the most CPU time and likely cause blocking.
//!
//! # Architecture
//!
//! - **`HotspotStats`** - Efficient aggregation as events stream in
//! - **`analyze_hotspots()`** - Batch analysis from a `TraceData` snapshot
//!
//! ## Data Flow
//!
//! ```text
//! eBPF Event
//!     │
//!     ├──► HotspotStats.record_event()  ← For efficient TUI updates
//!     │
//!     └──► LiveData.add_event()         ← Raw events for timeline/workers
//! ```
//!
//! # Performance
//!
//! - `record_event()`: O(1) amortized (HashMap insert/update)
//! - `to_hotspots()`: O(n log n) where n = unique functions (sorting)
//! - Memory: O(unique functions) × O(call stacks per function)

// Percentage calculations intentionally convert usize to f64
#![allow(clippy::cast_precision_loss)]

use crate::trace_data::{StackFrame, TraceData, TraceEvent};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Maximum unique call stacks to store per hotspot.
///
/// We limit this to avoid unbounded memory growth when a function is called
/// from many different locations. 5 is enough to show the user the main
/// call patterns without excessive memory usage.
const MAX_CALL_STACKS_PER_HOTSPOT: usize = 5;

// =============================================================================
// FUNCTION HOTSPOT (OUTPUT TYPE)
// =============================================================================

/// A function hotspot with aggregated statistics.
///
/// This is the "view model" returned by `analyze_hotspots()` or
/// `HotspotStats::to_hotspots()` for display in the TUI.
///
/// # Display in TUI
///
/// ```text
/// HOTSPOTS (2d 4h 23m)
/// ─────────────────────────────────
///   ▲ blowfish::encrypt       42.3%  ████████░░
///     serde_json::to_string   18.7%  ███░░░░░░░
///     std::fs::read           12.1%  ██░░░░░░░░
/// ```
#[derive(Debug, Clone)]
pub struct FunctionHotspot {
    /// Fully qualified function name (e.g., "myapp::crypto::encrypt").
    pub name: String,

    /// Total sample count for this function.
    pub count: usize,

    /// Percentage of total samples (0.0 - 100.0).
    pub percentage: f64,

    /// Per-worker breakdown: worker_id → sample count.
    /// Used for the "WORKER DISTRIBUTION" section in drilldown.
    pub workers: HashMap<u32, usize>,

    /// Source file of the function (if debug info available).
    pub file: Option<String>,

    /// Line number of the function (if debug info available).
    pub line: Option<u32>,

    /// Representative call stacks leading to this hotspot.
    ///
    /// Limited to `MAX_CALL_STACKS_PER_HOTSPOT` to bound memory.
    /// Sorted by frequency (most common call path first).
    /// Uses `Arc` for cheap cloning since stacks are shared.
    pub call_stacks: Vec<Arc<Vec<StackFrame>>>,
}

// =============================================================================
// HOTSPOT STATS (AGGREGATOR)
// =============================================================================

/// Efficient hotspot statistics aggregator.
///
/// Tracks statistics as events stream in, allowing efficient O(1) updates
/// rather than re-analyzing all events on each TUI refresh.
///
/// # Memory Usage
///
/// - O(unique function names) × O(workers) for the main HashMap
/// - O(unique call stacks) bounded by MAX_CALL_STACKS_PER_HOTSPOT
/// - Typical: 100-500 functions × ~1 KB each ≈ 0.5 MB
#[derive(Debug, Default)]
pub struct HotspotStats {
    /// Per-function statistics, keyed by function name.
    functions: HashMap<String, FunctionStats>,

    /// Total samples processed (excluding "execution" events).
    /// Used as denominator for percentage calculations.
    total_samples: u64,
}

/// Internal statistics for a single function.
#[derive(Debug, Clone)]
struct FunctionStats {
    /// Total sample count for this function.
    count: u64,

    /// Per-worker sample counts.
    workers: HashMap<u32, u64>,

    /// Source file (captured from first occurrence).
    file: Option<String>,

    /// Line number (captured from first occurrence).
    line: Option<u32>,

    /// Set of stack Arc pointer addresses we've seen.
    /// Used to deduplicate call stacks - if we've seen this exact Arc
    /// before, we just increment the count instead of storing it again.
    seen_stack_ids: HashSet<i64>,

    /// Representative call stacks with their occurrence counts.
    /// Limited to MAX_CALL_STACKS_PER_HOTSPOT entries.
    call_stacks: Vec<(Arc<Vec<StackFrame>>, u64)>,
}

impl HotspotStats {
    /// Create new empty statistics tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a trace event into the statistics aggregator.
    ///
    /// Called for every event to maintain up-to-date hotspot rankings.
    ///
    /// # Performance
    ///
    /// O(1) amortized - HashMap operations are constant time on average.
    /// The stack tracking uses Arc pointer comparison which is also O(1).
    pub fn record_event(&mut self, event: &TraceEvent) {
        // Skip "execution" events - these represent scheduler/idle time,
        // not actual blocking functions. We only count real function samples.
        if event.name == "execution" {
            return;
        }

        // Increment global sample counter (for percentage denominator)
        self.total_samples += 1;

        // Get or create stats entry for this function
        let stats = self.functions.entry(event.name.clone()).or_insert_with(|| FunctionStats {
            count: 0,
            workers: HashMap::new(),
            file: event.file.clone(),
            line: event.line,
            seen_stack_ids: HashSet::new(),
            call_stacks: Vec::new(),
        });

        stats.count += 1;
        *stats.workers.entry(event.worker_id).or_insert(0) += 1;

        // Track unique call stacks if available
        if let Some(ref stack) = event.call_stack {
            // Use pointer address as a proxy for stack identity (Arc deduplication)
            let stack_ptr = Arc::as_ptr(stack) as i64;
            if stats.seen_stack_ids.insert(stack_ptr) {
                // New unique stack - add to our collection if we have room
                if stats.call_stacks.len() < MAX_CALL_STACKS_PER_HOTSPOT {
                    stats.call_stacks.push((Arc::clone(stack), 1));
                }
            } else {
                // Existing stack - increment its count
                for (existing_stack, count) in &mut stats.call_stacks {
                    if Arc::ptr_eq(existing_stack, stack) {
                        *count += 1;
                        break;
                    }
                }
            }
        }
    }

    /// Get total samples recorded
    #[must_use]
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// Convert cumulative stats to hotspot list for display
    #[must_use]
    pub fn to_hotspots(&self) -> Vec<FunctionHotspot> {
        let mut hotspots: Vec<FunctionHotspot> = self
            .functions
            .iter()
            .map(|(name, stats)| {
                let percentage = if self.total_samples > 0 {
                    (stats.count as f64 / self.total_samples as f64) * 100.0
                } else {
                    0.0
                };

                // Sort call stacks by frequency
                let mut sorted_stacks = stats.call_stacks.clone();
                sorted_stacks.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(*count));

                FunctionHotspot {
                    name: name.clone(),
                    count: stats.count as usize,
                    percentage,
                    workers: stats.workers.iter().map(|(&k, &v)| (k, v as usize)).collect(),
                    file: stats.file.clone(),
                    line: stats.line,
                    call_stacks: sorted_stacks.into_iter().map(|(stack, _)| stack).collect(),
                }
            })
            .collect();

        hotspots.sort_unstable_by_key(|h| std::cmp::Reverse(h.count));
        hotspots
    }
}

/// Aggregated function data: (worker counts, file, line, call_stacks)
type FunctionData = (HashMap<u32, usize>, Option<String>, Option<u32>, Vec<Arc<Vec<StackFrame>>>);

/// Analyze trace data to identify function hotspots (batch analysis).
///
/// Aggregates trace events by function name, counts occurrences,
/// calculates percentages, and sorts by frequency (descending).
///
/// Note: "execution" events (scheduler/idle time) are filtered out to show
/// only actual function samples.
///
/// For efficient incremental updates during live profiling, use `HotspotStats`.
///
/// # Arguments
/// * `data` - The trace data to analyze
///
/// # Returns
/// A vector of function hotspots sorted by count (most frequent first)
#[must_use]
pub fn analyze_hotspots(data: &TraceData) -> Vec<FunctionHotspot> {
    // Aggregate events by function name, capturing file/line from first occurrence
    // Filter out "execution" events which represent scheduler/idle time
    let mut function_data: HashMap<String, FunctionData> = HashMap::new();
    let mut total_samples: usize = 0;
    // Track seen stacks per function to avoid duplicates
    let mut seen_stacks: HashMap<String, HashSet<usize>> = HashMap::new();

    for event in data.events.iter() {
        // Skip execution events - they're scheduler/idle time, not actual functions
        if event.name == "execution" {
            continue;
        }
        total_samples += 1;

        let entry = function_data
            .entry(event.name.clone())
            .or_insert_with(|| (HashMap::new(), event.file.clone(), event.line, Vec::new()));
        *entry.0.entry(event.worker_id).or_insert(0) += 1;

        // Collect unique call stacks (limited to MAX_CALL_STACKS_PER_HOTSPOT)
        if let Some(ref stack) = event.call_stack {
            let seen = seen_stacks.entry(event.name.clone()).or_default();
            let stack_ptr = Arc::as_ptr(stack) as usize;
            if seen.insert(stack_ptr) && entry.3.len() < MAX_CALL_STACKS_PER_HOTSPOT {
                entry.3.push(Arc::clone(stack));
            }
        }
    }

    // Convert to vector and calculate percentages
    let mut hotspots: Vec<FunctionHotspot> = function_data
        .into_iter()
        .map(|(name, (workers, file, line, call_stacks))| {
            let count: usize = workers.values().sum();
            let percentage =
                if total_samples > 0 { (count as f64 / total_samples as f64) * 100.0 } else { 0.0 };
            FunctionHotspot { name, count, percentage, workers, file, line, call_stacks }
        })
        .collect();

    // Sort by count (descending) - unstable sort is faster
    hotspots.sort_unstable_by_key(|h| std::cmp::Reverse(h.count));

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
                    call_stack: None,
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
                    call_stack: None,
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
                    call_stack: None,
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
