//! # Event Processing
//!
//! Consumes events from eBPF ring buffer and routes them to appropriate handlers.
//!
//! ## Event Routing
//!
//! - `EVENT_SCHEDULER_DETECTED` → Off-CPU threshold exceeded (blocking detection)
//! - `TRACE_EXECUTION_{START,END}` → Timeline visualization
//!
//! ## Output Modes
//!
//! - **Headless**: Print events to stdout
//! - **Live TUI**: Send to TUI thread via channel
//! - **Export**: Add to trace.json exporter
//!
//! See [Architecture docs](../../docs/ARCHITECTURE.md) for event flow details.

use aya::maps::{MapData, StackTraceMap};
use crossbeam_channel::Sender;
use hud_common::{
    TaskEvent, DETECTION_PERF_SAMPLE, EVENT_SCHEDULER_DETECTED, TRACE_EXECUTION_END,
    TRACE_EXECUTION_START,
};
use log::warn;
use std::borrow::Borrow;
use std::sync::Arc;

use super::{
    display_execution_event, display_scheduler_detected, DetectionStats, MemoryRange, StackResolver,
};
use crate::classification::classify_frame;
use crate::domain::StackId;
use crate::export::TraceEventExporter;
use crate::symbolization::Symbolizer;
use crate::trace_data::{StackCache, StackFrame, TraceEvent};

/// Encapsulates event processing logic and state
pub struct EventProcessor<'a> {
    // Configuration
    headless: bool,

    // Mutable state
    pub stats: DetectionStats,
    pub event_count: usize,
    /// Per-event-type counters for diagnostics
    pub perf_sample_count: usize,
    pub perf_stack_ok: usize,
    pub perf_stack_fail: usize,
    pub scheduler_event_count: usize,
    pub blocking_pool_filtered: usize,
    /// TUI pipeline counters: worker events with valid stacks
    pub tui_worker_events: usize,
    /// TUI pipeline counters: worker events dropped (no user-code frames)
    pub tui_no_user_code: usize,
    /// TUI pipeline counters: events successfully sent to TUI channel
    pub tui_sent: usize,
    /// Cache for resolved stack traces (bounded by eBPF's 16384 unique stacks)
    stack_cache: StackCache,

    // Dependencies (readonly)
    stack_resolver: StackResolver<'a>,
    symbolizer: &'a Symbolizer,
    memory_range: Option<MemoryRange>,

    // Optional outputs
    trace_exporter: Option<TraceEventExporter>,
    event_tx: Option<Sender<TraceEvent>>,
}

impl<'a> EventProcessor<'a> {
    /// Create a new event processor
    #[must_use]
    pub fn new(
        headless: bool,
        stack_resolver: StackResolver<'a>,
        symbolizer: &'a Symbolizer,
        memory_range: Option<MemoryRange>,
        trace_exporter: Option<TraceEventExporter>,
        event_tx: Option<Sender<TraceEvent>>,
    ) -> Self {
        Self {
            headless,
            stats: DetectionStats::default(),
            event_count: 0,
            perf_sample_count: 0,
            perf_stack_ok: 0,
            perf_stack_fail: 0,
            scheduler_event_count: 0,
            blocking_pool_filtered: 0,
            tui_worker_events: 0,
            tui_no_user_code: 0,
            tui_sent: 0,
            stack_cache: StackCache::new(),
            stack_resolver,
            symbolizer,
            memory_range,
            trace_exporter,
            event_tx,
        }
    }

    /// Process a single event
    pub fn process_event<T: std::borrow::Borrow<aya::maps::MapData>>(
        &mut self,
        event: TaskEvent,
        stack_traces: &aya::maps::StackTraceMap<T>,
    ) {
        self.event_count += 1;

        match event.event_type {
            EVENT_SCHEDULER_DETECTED => {
                self.scheduler_event_count += 1;
                self.handle_scheduler_detected(event, stack_traces);
            }
            TRACE_EXECUTION_START | TRACE_EXECUTION_END => {
                if event.detection_method == DETECTION_PERF_SAMPLE {
                    self.perf_sample_count += 1;
                    if event.stack_id >= 0 {
                        self.perf_stack_ok += 1;
                    } else {
                        self.perf_stack_fail += 1;
                    }
                }
                self.handle_trace_execution(event, stack_traces);
            }
            _ => {
                warn!("Unknown event type: {}", event.event_type);
            }
        }
    }

    /// Take the trace exporter (for final export)
    pub fn take_exporter(&mut self) -> Option<TraceEventExporter> {
        self.trace_exporter.take()
    }

    // Private event handlers

    fn handle_scheduler_detected<T: std::borrow::Borrow<aya::maps::MapData>>(
        &mut self,
        event: TaskEvent,
        stack_traces: &StackTraceMap<T>,
    ) {
        // Skip events from Tokio's blocking thread pool (spawn_blocking).
        // These threads are expected to block — reporting them is noise.
        // Guard with worker_id check: registered workers (worker_id != u32::MAX)
        // always pass through, since the stack-based filter can false-positive on
        // worker threads whose scheduler frames weren't captured.
        if event.worker_id == u32::MAX
            && self
                .resolve_full_stack(event.stack_id, stack_traces)
                .as_ref()
                .is_some_and(|s| is_blocking_pool_stack(s))
        {
            return;
        }

        self.stats.scheduler_detected += 1;

        if self.headless {
            display_scheduler_detected(&event, &self.stack_resolver, stack_traces);
        }
    }

    fn handle_trace_execution<T: Borrow<MapData>>(
        &mut self,
        event: TaskEvent,
        stack_traces: &StackTraceMap<T>,
    ) {
        // Resolve stack early for blocking pool filtering.
        // Only START events carry meaningful stacks; END events pass through.
        // Guard with worker_id check: registered workers always pass through.
        if event.event_type == TRACE_EXECUTION_START
            && event.worker_id == u32::MAX
            && self
                .resolve_full_stack(event.stack_id, stack_traces)
                .as_ref()
                .is_some_and(|s| is_blocking_pool_stack(s))
        {
            self.blocking_pool_filtered += 1;
            return;
        }

        // Get the top frame address for symbol resolution (for exporter)
        let top_frame_addr =
            StackResolver::get_top_frame_addr(StackId(event.stack_id), stack_traces);

        // Add to trace exporter if enabled
        if let Some(ref mut exporter) = self.trace_exporter {
            exporter.add_event(&event, top_frame_addr);
        }

        // Send to TUI: only worker thread events with valid stacks and user code.
        // Filters out:
        //   - sched_switch events (stack_id=-1)
        //   - non-worker threads (main thread, blocking pool) via worker_id
        //   - pure runtime samples with no user code on the stack
        if self.event_tx.is_some()
            && event.event_type == TRACE_EXECUTION_START
            && event.stack_id >= 0
            && event.worker_id != u32::MAX
        {
            self.tui_worker_events += 1;
            let trace_event = self.convert_to_trace_event(&event, stack_traces);

            let has_user_code = trace_event
                .call_stack
                .as_ref()
                .is_some_and(|stack| stack.iter().any(|f| f.is_user_code));

            if has_user_code {
                // Non-blocking send (drop if TUI is slow)
                if let Some(ref tx) = self.event_tx {
                    let _ = tx.try_send(trace_event);
                    self.tui_sent += 1;
                }
            } else {
                self.tui_no_user_code += 1;
            }
        }

        // Optionally display in headless mode
        if self.headless {
            let is_start = event.event_type == TRACE_EXECUTION_START;
            display_execution_event(&event, is_start);
        }
    }

    /// Resolve full call stack from eBPF stack trace map.
    ///
    /// This is the core symbolization logic. It takes a raw `stack_id` from eBPF,
    /// fetches the instruction pointer addresses from the kernel's stack trace map,
    /// and resolves each address to a human-readable function name using DWARF.
    ///
    /// # Caching
    ///
    /// eBPF automatically deduplicates identical stacks (same `stack_id` = same call path).
    /// We leverage this by caching resolved stacks - if we've seen this `stack_id` before,
    /// we return the cached `Arc<Vec<StackFrame>>` immediately.
    ///
    /// Typical applications have 50-500 unique call paths, so the cache hit rate is high.
    ///
    /// # Performance
    ///
    /// - Cache hit: O(1) `HashMap` lookup + Arc clone
    /// - Cache miss: `O(stack_depth)` × O(DWARF lookup) - expensive but rare after warmup
    ///
    /// # Address Adjustment (PIE)
    ///
    /// Modern executables use Position-Independent Executables (PIE) for ASLR.
    /// The addresses in the stack trace are runtime addresses, but DWARF debug info
    /// uses file offsets. We subtract the executable's base address to convert.
    fn resolve_full_stack<T: Borrow<MapData>>(
        &mut self,
        stack_id: i64,
        stack_traces: &StackTraceMap<T>,
    ) -> Option<Arc<Vec<StackFrame>>> {
        // === CACHE CHECK ===
        // Fast path: return cached stack if we've resolved this stack_id before
        if let Some(cached) = self.stack_cache.get(stack_id) {
            return Some(cached);
        }

        // === VALIDATION ===
        // eBPF returns negative stack_id on capture failure (e.g., kernel stack,
        // recursion limit hit, or stack walking error)
        let stack_id_wrapped = StackId(stack_id);
        if !stack_id_wrapped.is_valid() {
            return None;
        }

        // === FETCH FROM EBPF ===
        // Read the stack trace from kernel's BPF_MAP_TYPE_STACK_TRACE map.
        // The map stores arrays of instruction pointer (IP) addresses.
        let Ok(stack_trace) = stack_traces.get(&stack_id_wrapped.as_map_key(), 0) else {
            return None; // Stack may have been evicted from kernel map
        };

        let frames = stack_trace.frames();
        if frames.is_empty() {
            return None;
        }

        // === RESOLVE EACH FRAME ===
        // Walk the stack from top (blocking function) to bottom (main/runtime entry)
        let mut resolved_frames = Vec::with_capacity(frames.len());

        for stack_frame in frames {
            let addr = stack_frame.ip; // Instruction pointer (return address)

            // Null address marks end of stack (padding in fixed-size kernel array)
            if addr == 0 {
                break;
            }

            // === ADDRESS CLASSIFICATION ===
            // Determine if this address is in the main executable (user code)
            // or in a shared library (system/third-party code).
            //
            // For user code: Convert runtime address to file offset for DWARF lookup
            // For library code: We can't symbolize (no debug info), just show address
            let (file_offset, is_user_code) = if let Some(range) = self.memory_range {
                if range.contains(addr) {
                    // Address is within main executable's memory range
                    // Subtract base address to get file offset for DWARF
                    (addr - range.start, true)
                } else {
                    // Address is outside main executable (shared library)
                    (addr, false)
                }
            } else {
                // No memory range info - assume everything is user code
                // This happens if we couldn't parse /proc/<pid>/maps
                (addr, true)
            };

            // === SYMBOLIZATION ===
            if is_user_code {
                // Look up function name, file, and line from DWARF debug info
                let resolved = self.symbolizer.resolve(file_offset);

                if let Some(frame) = resolved.frames.first() {
                    // Successfully resolved - use DWARF info
                    let file_str = frame.location.as_ref().and_then(|loc| loc.file.clone());
                    let origin = classify_frame(
                        &frame.function,
                        file_str.as_deref(),
                        true, // in_executable
                    );
                    resolved_frames.push(StackFrame {
                        function: frame.function.clone(),
                        file: file_str,
                        line: frame.location.as_ref().and_then(|loc| loc.line),
                        origin,
                        is_user_code: origin.is_user_code(),
                    });
                } else {
                    // DWARF lookup failed - show raw address (stripped binary?)
                    let origin = crate::classification::FrameOrigin::Unknown;
                    resolved_frames.push(StackFrame {
                        function: format!("0x{addr:x}"),
                        file: None,
                        line: None,
                        origin,
                        is_user_code: origin.is_user_code(),
                    });
                }
            } else {
                // Library code - can't symbolize without library debug info
                // Just show a placeholder with the address
                let origin = crate::classification::FrameOrigin::Unknown;
                resolved_frames.push(StackFrame {
                    function: format!("<library> 0x{addr:x}"),
                    file: None,
                    line: None,
                    origin,
                    is_user_code: false,
                });
            }
        }

        // Empty after filtering? Return None.
        if resolved_frames.is_empty() {
            return None;
        }

        // Cache and return
        Some(self.stack_cache.insert(stack_id, resolved_frames))
    }

    /// Convert `TaskEvent` to `TraceEvent` with full stack resolution
    #[allow(clippy::cast_precision_loss)]
    fn convert_to_trace_event<T: Borrow<MapData>>(
        &mut self,
        event: &TaskEvent,
        stack_traces: &StackTraceMap<T>,
    ) -> TraceEvent {
        // Resolve full call stack
        let call_stack = self.resolve_full_stack(event.stack_id, stack_traces);

        // Use the first user-code frame as the event name (answers "which of MY
        // functions is blocking?"). Falls back to top frame if no user code found.
        let (name, file, line) = call_stack
            .as_ref()
            .and_then(|stack| stack.iter().find(|f| f.is_user_code).or_else(|| stack.first()))
            .map_or_else(
                || ("execution".to_string(), None, None),
                |frame| (frame.function.clone(), frame.file.clone(), frame.line),
            );

        TraceEvent {
            name,
            worker_id: event.worker_id,
            tid: event.tid,
            timestamp: event.timestamp_ns as f64 / 1_000_000_000.0, // ns to seconds
            cpu: event.cpu_id,
            detection_method: Some(u32::from(event.detection_method)),
            file,
            line,
            call_stack,
        }
    }
}

/// Returns `true` if the call stack originates from Tokio's blocking thread pool
/// (`spawn_blocking`), as opposed to an async worker thread.
///
/// Both worker threads and blocking pool threads have `Inner::run` at the bottom
/// of their stacks because Tokio launches workers via the blocking pool mechanism.
/// The distinguishing factor is that worker threads also have the multi-thread
/// scheduler's `worker::` frames, while pure blocking pool threads do not.
fn is_blocking_pool_stack(call_stack: &[StackFrame]) -> bool {
    let has_blocking_pool = call_stack
        .iter()
        .any(|frame| frame.function.starts_with("tokio::runtime::blocking::pool::Inner::run"));
    let has_worker_scheduler =
        call_stack.iter().any(|frame| frame.function.contains("scheduler::multi_thread::worker"));
    // Genuine blocking pool: has Inner::run but NOT the worker scheduler
    has_blocking_pool && !has_worker_scheduler
}
