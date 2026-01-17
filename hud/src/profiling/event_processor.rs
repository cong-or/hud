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
use hud_common::{TaskEvent, EVENT_SCHEDULER_DETECTED, TRACE_EXECUTION_END, TRACE_EXECUTION_START};
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
    /// Cache for resolved stack traces (bounded by eBPF's 1024 unique stacks)
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
            EVENT_SCHEDULER_DETECTED => self.handle_scheduler_detected(event, stack_traces),
            TRACE_EXECUTION_START | TRACE_EXECUTION_END => {
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
        // Get the top frame address for symbol resolution (for exporter)
        let top_frame_addr =
            StackResolver::get_top_frame_addr(StackId(event.stack_id), stack_traces);

        // Add to trace exporter if enabled
        if let Some(ref mut exporter) = self.trace_exporter {
            exporter.add_event(&event, top_frame_addr);
        }

        // Send to TUI if running (only START events to reduce TUI load)
        let should_send_to_tui =
            self.event_tx.is_some() && event.event_type == TRACE_EXECUTION_START;

        if should_send_to_tui {
            let trace_event = self.convert_to_trace_event(&event, stack_traces);
            // Non-blocking send (drop if TUI is slow)
            if let Some(ref tx) = self.event_tx {
                let _ = tx.try_send(trace_event);
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

        // Extract top frame info from call_stack if available, otherwise fall back
        let (name, file, line) = call_stack.as_ref().and_then(|stack| stack.first()).map_or_else(
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
