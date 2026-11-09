use anyhow::{Context, Result};
use aya::maps::HashMap;
use aya::Ebpf;
use log::debug;

/// Log `perf_event` counter diagnostics (only shown with `RUST_LOG=debug`).
///
/// Dumps eBPF map counters to help debug event flow: how many times the
/// `perf_event` handler fired, how many passed the PID filter, and how many
/// events were output successfully vs dropped.
///
/// # Errors
/// Returns an error if the eBPF diagnostic maps cannot be accessed.
pub fn print_perf_event_diagnostics(bpf: &mut Ebpf) -> Result<()> {
    debug!("perf_event diagnostics:");

    // Total calls
    let counter_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_COUNTER").context("PERF_EVENT_COUNTER map not found")?,
    )?;
    if let Ok(count) = counter_map.get(&0u32, 0) {
        debug!("  handler called: {count} times");
    }

    // Passed PID filter
    let pid_filter_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_PASSED_PID_FILTER")
            .context("PERF_EVENT_PASSED_PID_FILTER map not found")?,
    )?;
    if let Ok(count) = pid_filter_map.get(&0u32, 0) {
        debug!("  passed PID filter: {count} times");
    } else {
        debug!("  passed PID filter: 0 times (all filtered out)");
    }

    // Output success
    let success_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_OUTPUT_SUCCESS").context("PERF_EVENT_OUTPUT_SUCCESS map not found")?,
    )?;
    if let Ok(count) = success_map.get(&0u32, 0) {
        debug!("  events output success: {count}");
    }

    // Output failed
    let failed_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_OUTPUT_FAILED").context("PERF_EVENT_OUTPUT_FAILED map not found")?,
    )?;
    if let Ok(count) = failed_map.get(&0u32, 0) {
        debug!("  events output failed: {count}");
    }

    Ok(())
}
