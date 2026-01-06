use anyhow::{Context, Result};
use aya::maps::HashMap;
use aya::Ebpf;

/// Print debug diagnostics for `perf_event` counters
///
/// This displays various eBPF map counters to help debug event flow:
/// - How many times the `perf_event` handler was called
/// - How many events passed the PID filter
/// - How many events were successfully output
/// - How many events failed to output
///
/// # Errors
/// Returns an error if the eBPF diagnostic maps cannot be accessed
pub fn print_perf_event_diagnostics(bpf: &mut Ebpf) -> Result<()> {
    println!("\n🔍 DEBUG: perf_event diagnostics:");

    // Total calls
    let counter_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_COUNTER").context("PERF_EVENT_COUNTER map not found")?,
    )?;
    if let Ok(count) = counter_map.get(&0u32, 0) {
        println!("   - Handler called: {count} times");
    }

    // Passed PID filter
    let pid_filter_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_PASSED_PID_FILTER")
            .context("PERF_EVENT_PASSED_PID_FILTER map not found")?,
    )?;
    if let Ok(count) = pid_filter_map.get(&0u32, 0) {
        println!("   - Passed PID filter: {count} times");
    } else {
        println!("   - Passed PID filter: 0 times (ALL FILTERED OUT!)");
    }

    // Output success
    let success_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_OUTPUT_SUCCESS").context("PERF_EVENT_OUTPUT_SUCCESS map not found")?,
    )?;
    if let Ok(count) = success_map.get(&0u32, 0) {
        println!("   - Events output success: {count}");
    }

    // Output failed
    let failed_map: HashMap<_, u32, u64> = HashMap::try_from(
        bpf.map("PERF_EVENT_OUTPUT_FAILED").context("PERF_EVENT_OUTPUT_FAILED map not found")?,
    )?;
    if let Ok(count) = failed_map.get(&0u32, 0) {
        println!("   - Events output failed: {count}");
    }

    Ok(())
}
