//! Event processing logic extracted from main loop
//!
//! This module encapsulates all event handling logic, making the main loop cleaner
//! and more testable by separating concerns.

use aya::maps::StackTraceMap;
use crossbeam_channel::Sender;
use hud_common::{
    TaskEvent, EVENT_BLOCKING_END, EVENT_BLOCKING_START, EVENT_SCHEDULER_DETECTED,
    TRACE_EXECUTION_END, TRACE_EXECUTION_START,
};
use log::warn;

use super::{
    display_blocking_end, display_blocking_end_no_start, display_blocking_start,
    display_execution_event, display_scheduler_detected, DetectionStats, MemoryRange,
    StackResolver,
};
use crate::domain::StackId;
use crate::export::TraceEventExporter;
use crate::symbolization::Symbolizer;
use crate::trace_data::TraceEvent;

/// Blocking state tracking for marker-based detection
#[derive(Debug, Clone, Copy)]
struct BlockingState {
    start_time_ns: u64,
    stack_id: i64,
}

/// Encapsulates event processing logic and state
pub struct EventProcessor<'a> {
    // Configuration
    headless: bool,

    // Mutable state
    blocking_state: Option<BlockingState>,
    pub stats: DetectionStats,
    pub event_count: usize,

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
            blocking_state: None,
            stats: DetectionStats::default(),
            event_count: 0,
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
            EVENT_BLOCKING_START => self.handle_blocking_start(event),
            EVENT_BLOCKING_END => self.handle_blocking_end(event, stack_traces),
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

    fn handle_blocking_start(&mut self, event: TaskEvent) {
        self.blocking_state =
            Some(BlockingState { start_time_ns: event.timestamp_ns, stack_id: event.stack_id });

        if self.headless {
            display_blocking_start(&event);
        }
    }

    fn handle_blocking_end<T: std::borrow::Borrow<aya::maps::MapData>>(
        &mut self,
        event: TaskEvent,
        stack_traces: &StackTraceMap<T>,
    ) {
        if let Some(state) = self.blocking_state {
            self.stats.marker_detected += 1;

            if self.headless {
                display_blocking_end(
                    &event,
                    state.start_time_ns,
                    Some(state.stack_id),
                    &self.stack_resolver,
                    stack_traces,
                );
            }

            self.blocking_state = None;
        } else if self.headless {
            display_blocking_end_no_start(&event);
        }
    }

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

    fn handle_trace_execution<T: std::borrow::Borrow<aya::maps::MapData>>(
        &mut self,
        event: TaskEvent,
        stack_traces: &StackTraceMap<T>,
    ) {
        // Get the top frame address for symbol resolution
        let top_frame_addr =
            StackResolver::get_top_frame_addr(StackId(event.stack_id), stack_traces);

        // Add to trace exporter if enabled
        if let Some(ref mut exporter) = self.trace_exporter {
            exporter.add_event(&event, top_frame_addr);
        }

        // Send to TUI if running
        if let Some(ref tx) = self.event_tx {
            // Only send START events to reduce TUI load
            if event.event_type == TRACE_EXECUTION_START {
                let trace_event = self.convert_to_trace_event(&event, top_frame_addr);
                // Non-blocking send (drop if TUI is slow)
                let _ = tx.try_send(trace_event);
            }
        }

        // Optionally display in headless mode
        if self.headless {
            let is_start = event.event_type == TRACE_EXECUTION_START;
            display_execution_event(&event, is_start);
        }
    }

    /// Convert `TaskEvent` to `TraceEvent` with symbol resolution
    #[allow(clippy::cast_precision_loss)]
    fn convert_to_trace_event(&self, event: &TaskEvent, top_frame_addr: Option<u64>) -> TraceEvent {
        // Resolve symbol for event name using functional combinators
        let (name, file, line) = top_frame_addr.map_or_else(
            || ("execution".to_string(), None, None),
            |addr| {
                // Adjust address for PIE executables
                let file_offset = self
                    .memory_range
                    .filter(|range| range.contains(addr))
                    .map_or(addr, |range| addr - range.start);

                let resolved = self.symbolizer.resolve(file_offset);
                resolved.frames.first().map_or_else(
                    || (format!("0x{addr:x}"), None, None),
                    |frame| {
                        let func = frame.function.clone();
                        let file_path = frame.location.as_ref().and_then(|loc| loc.file.clone());
                        let line_num = frame.location.as_ref().and_then(|loc| loc.line);
                        (func, file_path, line_num)
                    },
                )
            },
        );

        TraceEvent {
            name,
            worker_id: event.worker_id,
            tid: event.tid,
            timestamp: event.timestamp_ns as f64 / 1_000_000.0, // ns to seconds
            cpu: event.cpu_id,
            detection_method: Some(u32::from(event.detection_method)),
            file,
            line,
        }
    }
}
