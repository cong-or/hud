//! Trace data models for TUI display
//!
//! This module contains the core data structures for live profiling mode:
//!
//! - [`TraceEvent`] - A single profiling sample with optional call stack
//! - [`LiveData`] - Accumulates events from eBPF
//! - [`TraceData`] - Immutable snapshot for rendering (cheap Arc clones)
//! - [`StackFrame`] - A single frame in a resolved call stack
//! - [`StackCache`] - Deduplicates resolved stacks by eBPF `stack_id`
//!
//! # Memory Model
//!
//! All events are kept for the entire session (unlimited). For typical profiling
//! sessions, memory usage is reasonable:
//!
//! - **Events**: ~150 bytes each, 100-500K events = 15-75 MB
//! - **Stack cache**: Bounded by eBPF's 1024 unique stacks (~1 MB)
//! - **DWARF symbolizer**: 10-50 MB (fixed)
//!
//! Total: ~100-150 MB for a multi-hour session.
//!
//! # Performance Considerations
//!
//! - `Arc<Vec<T>>` used for cheap cloning between threads (TUI runs separately)
//! - Stack traces share `Arc` references via `StackCache` deduplication

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use crate::classification::FrameOrigin;

// =============================================================================
// STACK FRAME AND CACHE
// =============================================================================

/// A single frame in a resolved call stack.
///
/// Represents one function call in the stack trace, with optional source location.
/// The `origin` field classifies the frame as user code vs library/runtime code,
/// allowing the UI to highlight user code more prominently.
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// Fully qualified function name (e.g., `myapp::handler::process_request`)
    pub function: String,

    /// Source file path, if debug info is available
    pub file: Option<String>,

    /// Line number in source file, if debug info is available
    pub line: Option<u32>,

    /// Classification of this frame's origin (user code, std lib, runtime, etc.)
    /// Used to distinguish user application code from statically-linked libraries.
    pub origin: FrameOrigin,

    /// True if this frame is user application code.
    /// Derived from `origin.is_user_code()` for backward compatibility.
    /// Used by the UI to highlight user code in green vs dim for libraries.
    pub is_user_code: bool,
}

/// Cache of resolved stack traces, keyed by eBPF `stack_id`.
///
/// # Why Cache?
///
/// eBPF automatically deduplicates identical stack traces, assigning them the
/// same `stack_id`. A typical application has 50-500 unique call paths, not
/// thousands. By caching resolved stacks, we:
///
/// 1. Avoid repeated DWARF symbol resolution (expensive)
/// 2. Share `Arc<Vec<StackFrame>>` across all events with the same stack
/// 3. Bound memory to O(unique stacks) not O(total events)
///
/// # Memory Bound
///
/// eBPF's `STACK_TRACES` map is configured with `max_entries = 1024`, so this
/// cache can hold at most 1024 unique stacks. At ~1 KB per stack (5-20 frames
/// × ~50 bytes each), maximum memory is ~1 MB.
#[derive(Debug, Default)]
pub struct StackCache {
    /// Map from eBPF `stack_id` to resolved frames.
    /// Uses `Arc` so multiple `TraceEvent`s can share the same stack.
    stacks: HashMap<i64, Arc<Vec<StackFrame>>>,
}

impl StackCache {
    /// Create a new empty stack cache.
    #[must_use]
    pub fn new() -> Self {
        Self { stacks: HashMap::new() }
    }

    /// Get a cached stack by ID, returning a cheap `Arc` clone.
    ///
    /// Returns `None` if this `stack_id` hasn't been resolved yet.
    #[must_use]
    pub fn get(&self, stack_id: i64) -> Option<Arc<Vec<StackFrame>>> {
        self.stacks.get(&stack_id).cloned()
    }

    /// Insert a newly resolved stack into the cache.
    ///
    /// Returns an `Arc` to the cached stack for immediate use.
    pub fn insert(&mut self, stack_id: i64, stack: Vec<StackFrame>) -> Arc<Vec<StackFrame>> {
        let arc = Arc::new(stack);
        self.stacks.insert(stack_id, Arc::clone(&arc));
        arc
    }

    /// Get a cached stack or resolve it using the provided closure.
    ///
    /// This is the primary API for stack resolution - it handles the
    /// cache-miss case automatically.
    ///
    /// # Example
    /// ```ignore
    /// let stack = cache.get_or_insert_with(stack_id, || {
    ///     resolve_stack_from_ebpf(stack_id, stack_traces)
    /// });
    /// ```
    pub fn get_or_insert_with<F>(&mut self, stack_id: i64, resolve_fn: F) -> Arc<Vec<StackFrame>>
    where
        F: FnOnce() -> Vec<StackFrame>,
    {
        if let Some(existing) = self.stacks.get(&stack_id) {
            // Cache hit - return cheap Arc clone
            Arc::clone(existing)
        } else {
            // Cache miss - resolve and insert
            self.insert(stack_id, resolve_fn())
        }
    }

    /// Number of unique stacks currently cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stacks.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stacks.is_empty()
    }
}

// =============================================================================
// TRACE EVENT
// =============================================================================

/// A single profiling sample captured from eBPF.
///
/// Represents one point-in-time observation of a Tokio worker thread.
/// Multiple events with the same function name are aggregated into
/// [`FunctionHotspot`](crate::analysis::FunctionHotspot) for display.
///
/// # Memory Layout
///
/// The `call_stack` field uses `Arc` to share resolved stacks across events.
/// This is critical for memory efficiency since the same stack trace may
/// appear in thousands of events.
#[derive(Debug, Clone)]
pub struct TraceEvent {
    /// Function name at the top of the stack (the "blocking" function).
    /// This is what appears in the hotspot list.
    pub name: String,

    /// Tokio worker thread index (0, 1, 2, ...).
    /// Used for per-worker breakdown in the drilldown view.
    pub worker_id: u32,

    /// Linux thread ID (from `gettid()`).
    /// Useful for correlating with external tools like `perf`.
    pub tid: u32,

    /// Timestamp in seconds since profiling started.
    /// Used for duration calculations.
    pub timestamp: f64,

    /// CPU core where this sample was taken.
    pub cpu: u32,

    /// How this event was detected:
    /// - 2 = scheduler-based (off-CPU threshold)
    /// - 3 = tracepoint-based (`sched_switch`)
    /// - 4 = sampling-based (`perf_event`)
    pub detection_method: Option<u32>,

    /// Source file of the top frame (if debug info available).
    pub file: Option<String>,

    /// Line number of the top frame (if debug info available).
    pub line: Option<u32>,

    /// Full resolved call stack, shared via `Arc` for memory efficiency.
    /// `None` if stack capture failed (e.g., kernel stack, recursion limit).
    pub call_stack: Option<Arc<Vec<StackFrame>>>,
}

// =============================================================================
// TRACE DATA (IMMUTABLE SNAPSHOT)
// =============================================================================

/// Immutable snapshot of trace data for rendering.
///
/// Created from `LiveData::as_trace_data()` for passing to the TUI.
/// Uses `Arc` for all collections so cloning is O(1) - just reference counting.
///
/// # Thread Safety
///
/// `TraceData` is safe to send to the TUI thread because:
/// - `Arc<Vec<T>>` is `Send + Sync`
/// - All fields are immutable after creation
/// - No interior mutability
#[derive(Debug, Clone)]
pub struct TraceData {
    /// All events collected during the session.
    pub events: Arc<Vec<TraceEvent>>,

    /// Sorted list of worker IDs seen during the session.
    pub workers: Arc<Vec<u32>>,

    /// Time span of events (seconds from first to last event).
    pub duration: f64,
}

// =============================================================================
// LIVE DATA (MUTABLE ACCUMULATOR)
// =============================================================================

/// Mutable accumulator for incoming trace events.
///
/// This is the "write side" of the data model. Events flow in from eBPF,
/// get added here, and periodically converted to `TraceData` for rendering.
///
/// All events are kept for the entire session (unlimited).
///
/// # Performance
///
/// - Adding events: O(1) amortized (`Vec::push`)
/// - Converting to `TraceData`: O(1) (`Arc::clone`)
#[derive(Debug, Default)]
pub struct LiveData {
    /// All events, wrapped in Arc for cheap cloning.
    /// We use `Arc::make_mut` for copy-on-write semantics.
    events: Arc<Vec<TraceEvent>>,

    /// Set of worker IDs seen (for O(1) duplicate check).
    workers_set: HashSet<u32>,

    /// Sorted list of worker IDs (for stable display order).
    workers: Arc<Vec<u32>>,

    /// Time span of events (seconds from first to last).
    pub duration: f64,

    /// Timestamp of the very first event (never changes).
    start_time: Option<f64>,

    /// Timestamp of the most recent event.
    latest_time: Option<f64>,
}

impl LiveData {
    /// Create a new empty live data accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new event to the accumulator.
    ///
    /// This is the hot path - called for every event from eBPF (~100-400/sec).
    /// Performance is critical here.
    ///
    /// # Complexity
    ///
    /// Amortized O(1) for append.
    pub fn add_event(&mut self, event: TraceEvent) {
        // First event establishes session start time
        if self.start_time.is_none() {
            self.start_time = Some(event.timestamp);
        }

        // Always track the latest timestamp
        self.latest_time = Some(event.timestamp);

        // Track workers - HashSet ensures O(1) duplicate detection
        if self.workers_set.insert(event.worker_id) {
            // New worker! Update the sorted list.
            let workers_vec = Arc::make_mut(&mut self.workers);
            workers_vec.push(event.worker_id);
            workers_vec.sort_unstable();
        }

        // Update duration
        self.update_duration();

        // Append the event
        Arc::make_mut(&mut self.events).push(event);
    }

    /// Recalculate the duration field from current events.
    fn update_duration(&mut self) {
        if let (Some(first), Some(latest)) = (self.start_time, self.latest_time) {
            self.duration = latest - first;
        }
    }

    /// Number of events collected.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Create an immutable snapshot for rendering.
    ///
    /// # Performance
    ///
    /// O(1) - just Arc reference count increments, no data copying.
    #[must_use]
    pub fn as_trace_data(&self) -> TraceData {
        TraceData {
            events: Arc::clone(&self.events),
            workers: Arc::clone(&self.workers),
            duration: self.duration,
        }
    }
}
