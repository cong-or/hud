//! Stack-based worker discovery via eBPF sampling
//!
//! Identifies Tokio worker threads by analyzing stack traces collected from
//! the perf-event sampler, rather than relying solely on thread name heuristics.
//!
//! ## How it works
//!
//! 1. The perf-event sampler (started in phase 1) collects stack traces from
//!    the target process at 99 Hz.
//! 2. For each sampled thread, we resolve the stack trace and look for Tokio
//!    runtime frame signatures:
//!    - `scheduler::multi_thread::worker` → Worker thread
//!    - `blocking::pool::Inner::run` (without worker frames) → Blocking pool
//! 3. Threads classified as workers are returned with sequential IDs.
//!
//! This approach is more reliable than the "largest thread group" heuristic
//! because it uses Tokio's own frame signatures rather than thread names.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use aya::maps::{MapData, RingBuf, StackTraceMap};
use hud_common::TaskEvent;
use log::info;

use crate::domain::{Pid, StackId};
use crate::symbolization::{MemoryRange, Symbolizer};

use super::worker_discovery::{list_process_threads, WorkerInfo};

/// Classification of a thread based on its stack trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadClass {
    /// Tokio multi-thread scheduler worker
    Worker,
    /// Tokio blocking thread pool (`spawn_blocking`)
    BlockingPool,
    /// Could not be classified (no recognizable Tokio frames)
    Unknown,
}

/// Classify a single stack trace as Worker, `BlockingPool`, or Unknown.
///
/// Resolves the stack trace from the eBPF map and checks for Tokio runtime
/// frame signatures:
/// - Contains `scheduler::multi_thread::worker` → [`ThreadClass::Worker`]
/// - Contains `blocking::pool::Inner::run` without worker frames →
///   [`ThreadClass::BlockingPool`]
/// - Neither → [`ThreadClass::Unknown`]
pub fn classify_thread_stack<T: Borrow<MapData>>(
    stack_id: StackId,
    stack_traces: &StackTraceMap<T>,
    symbolizer: &Symbolizer,
    memory_range: Option<MemoryRange>,
) -> ThreadClass {
    if !stack_id.is_valid() {
        return ThreadClass::Unknown;
    }

    let Ok(stack_trace) = stack_traces.get(&stack_id.as_map_key(), 0) else {
        return ThreadClass::Unknown;
    };

    let frames = stack_trace.frames();
    if frames.is_empty() {
        return ThreadClass::Unknown;
    }

    let mut has_blocking_pool = false;

    for frame in frames {
        let addr = frame.ip;
        if addr == 0 {
            break;
        }

        // Determine file offset for symbolization
        let (file_offset, in_executable) = if let Some(range) = memory_range {
            if range.contains(addr) {
                (addr - range.start, true)
            } else {
                (addr, false)
            }
        } else {
            (addr, true)
        };

        if !in_executable {
            continue;
        }

        let resolved = symbolizer.resolve(file_offset);
        if let Some(frame_info) = resolved.frames.first() {
            // Worker always wins — no need to inspect remaining frames
            if frame_info.function.contains("scheduler::multi_thread::worker") {
                return ThreadClass::Worker;
            }
            if frame_info.function.starts_with("tokio::runtime::blocking::pool::Inner::run") {
                has_blocking_pool = true;
            }
        }
    }

    if has_blocking_pool {
        ThreadClass::BlockingPool
    } else {
        ThreadClass::Unknown
    }
}

/// Discover worker threads by sampling stack traces from the ring buffer.
///
/// Reads events from the ring buffer for `duration`, classifies each unique
/// TID by its stack trace, then reads thread names from `/proc` and returns
/// discovered workers with sequential IDs.
///
/// # Arguments
/// * `ring_buf` - The eBPF ring buffer (EVENTS map)
/// * `stack_traces` - The eBPF stack trace map
/// * `symbolizer` - DWARF symbolizer for the target binary
/// * `memory_range` - Memory range of the target binary (for PIE adjustment)
/// * `pid` - Target process ID
/// * `duration` - How long to sample before returning results
///
/// # Errors
/// Returns an error if `/proc` cannot be read
#[allow(clippy::cast_possible_truncation)]
pub fn discover_workers_from_stacks<T: Borrow<MapData>>(
    ring_buf: &mut RingBuf<MapData>,
    stack_traces: &StackTraceMap<T>,
    symbolizer: &Symbolizer,
    memory_range: Option<MemoryRange>,
    pid: Pid,
    duration: Duration,
) -> anyhow::Result<Vec<WorkerInfo>> {
    // Collect classifications: TID → best classification seen
    let mut tid_class: HashMap<u32, ThreadClass> = HashMap::new();
    // Cache: stack_id → classification. eBPF deduplicates identical stacks,
    // so the same stack_id appears across many events. Caching avoids
    // redundant DWARF resolution (the expensive part of classification).
    let mut stack_cache: HashMap<i64, ThreadClass> = HashMap::new();

    let start = Instant::now();
    while start.elapsed() < duration {
        while let Some(item) = ring_buf.next() {
            let bytes: &[u8] = &item;
            if bytes.len() < std::mem::size_of::<TaskEvent>() {
                continue;
            }

            // SAFETY: We verified the buffer size matches TaskEvent
            #[allow(unsafe_code)]
            let event = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<TaskEvent>()) };

            let class = *stack_cache.entry(event.stack_id).or_insert_with(|| {
                classify_thread_stack(
                    StackId(event.stack_id),
                    stack_traces,
                    symbolizer,
                    memory_range,
                )
            });

            // Upgrade classification: Worker > BlockingPool > Unknown
            tid_class
                .entry(event.tid)
                .and_modify(|existing| {
                    if class == ThreadClass::Worker {
                        *existing = ThreadClass::Worker;
                    } else if class == ThreadClass::BlockingPool
                        && *existing == ThreadClass::Unknown
                    {
                        *existing = ThreadClass::BlockingPool;
                    }
                })
                .or_insert(class);
        }

        // Brief sleep to avoid busy-spinning
        std::thread::sleep(Duration::from_millis(10));
    }

    // Filter to workers only
    let worker_tids: Vec<u32> = tid_class
        .into_iter()
        .filter(|(_, class)| *class == ThreadClass::Worker)
        .map(|(tid, _)| tid)
        .collect();

    if worker_tids.is_empty() {
        info!("Stack-based discovery: no worker threads found in sampling window");
        return Ok(vec![]);
    }

    // Read thread names from /proc for the discovered TIDs
    let threads = list_process_threads(pid)?;
    let thread_names: HashMap<u32, String> = threads.into_iter().collect();

    let mut workers: Vec<WorkerInfo> = worker_tids
        .into_iter()
        .filter_map(|tid| {
            let comm = thread_names.get(&tid)?.clone();
            Some(WorkerInfo {
                tid: crate::domain::Tid(tid),
                worker_id: 0, // assigned below
                comm,
            })
        })
        .collect();

    // Sort by TID for deterministic ordering, then assign sequential IDs
    workers.sort_by_key(|w| w.tid.0);
    for (idx, worker) in workers.iter_mut().enumerate() {
        worker.worker_id = idx as u32;
    }

    info!("Stack-based discovery: found {} worker threads", workers.len());

    Ok(workers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_class_equality() {
        assert_eq!(ThreadClass::Worker, ThreadClass::Worker);
        assert_ne!(ThreadClass::Worker, ThreadClass::BlockingPool);
        assert_ne!(ThreadClass::Worker, ThreadClass::Unknown);
    }
}
