# Tuning

## Rolling Window

`--window <secs>` limits the display to events from the last N seconds. Default: 0 (all data).

```bash
sudo hud my-app --window 30   # show last 30 seconds only
```

### Why use a window?

Without `--window`, hud accumulates all events since startup. This is useful for finding problems over long sessions, but has a downside: **metrics never decay**. If you generate load for 5 minutes then stop, the hotspot percentages stay frozen at their peak values.

With `--window 30`, only the last 30 seconds of data is shown. When load stops:
- Old events age out of the window
- Hotspot percentages naturally decay toward 0
- The display reflects *current* behavior, not historical

### When to use it

| Scenario | Recommendation |
|----------|----------------|
| Interactive debugging | `--window 30` — see real-time response to changes |
| Before/after comparisons | No window — capture full sessions for comparison |
| Long-running monitoring | `--window 60` — focus on recent behavior |
| Initial triage | No window — accumulate data to find patterns |

### Memory note

Window filtering happens at display time, not storage. All events are kept in memory regardless of window size. For very long sessions (hours), memory usage grows linearly with event count.

## Threshold

`--threshold <ms>` sets the minimum off-CPU duration before hud reports a blocking event. Default: 5ms.

```bash
sudo hud my-app --threshold 1   # sensitive
sudo hud my-app --threshold 20  # relaxed
```

### Detection mechanics

hud attaches to `sched_switch`. When a Tokio worker goes off-CPU with state `TASK_RUNNING` (preempted, not sleeping), then comes back on-CPU, the off-CPU duration is compared against the threshold. Exceeding it triggers a stack capture.

This means:
- Threshold is checked on the *return* to CPU, not continuously
- Very short blocks between scheduler ticks may not be captured
- Blocks during voluntary sleeps (`.await` on I/O) don't trigger — only busy-waiting or compute

### Choosing a value

| Threshold | Use case | Tradeoff |
|-----------|----------|----------|
| 1ms | Latency-critical paths | High event volume, includes scheduler noise |
| 5ms | General profiling | Good default, filters transient preemption |
| 10-20ms | Noisy environments, batch workloads | May miss smaller blocks |
| 50ms+ | Initial triage | Only severe issues surface |

At high request rates, blocking impact scales linearly:

```
affected_requests ≈ req/s × block_duration
```

A 5ms block at 10k req/s affects ~50 concurrent requests.

### Overhead

Threshold affects event volume, not sampling overhead. The eBPF programs run regardless — lower thresholds just emit more events to userspace. At 1ms on a busy system, expect 100-1000+ events/sec. At 50ms, maybe single digits.

Stack capture is the expensive part (~1-5μs per event). High event rates can add measurable CPU overhead to the profiler itself, not the target.

## Interpreting results

### Signal vs noise

**Real blocking** typically shows:
- Consistent stack traces across events
- User code in the call chain
- Known blocking operations (sync I/O, crypto, compression)

**Noise** typically shows:
- Random preemption points
- Stacks entirely in runtime/stdlib
- Single occurrences that don't repeat

### Common patterns

**Sync I/O in async context**
```
your_code::handler
  → std::fs::read
  → syscall
```
Fix: `spawn_blocking` or async alternative.

**Lock contention**
```
your_code::shared_state
  → std::sync::Mutex::lock
  → futex_wait
```
Fix: Reduce critical section, use async mutex, or shard state.

**Compute-bound work**
```
your_code::process
  → serde_json::from_str
  → (deep parse stack)
```
Fix: `spawn_blocking` for large payloads, or streaming parser.

**Accidental blocking in dependency**
```
your_code::handler
  → some_crate::init
  → std::fs::read (config file)
```
Fix: Initialize at startup, not per-request.

## Debugging workflow

### Triage (find the problem)

```bash
sudo hud my-app --threshold 50
```

Start high. Look for functions that appear repeatedly. These are your major offenders.

### Isolate (confirm the cause)

```bash
sudo hud my-app --threshold 5
```

Lower threshold, focus on the hotspot you identified. Check the full call stack in drilldown (`Enter` key). Is it your code or a dependency?

### Validate (verify the fix)

Use `--export` to capture before/after data for comparison. See [Exports](EXPORTS.md) for the full workflow.

```bash
sudo hud my-app --threshold 5 --duration 60 --export before.json --headless
# deploy fix
sudo hud my-app --threshold 5 --duration 60 --export after.json --headless
```

## Environment considerations

### Container overhead

Container runtimes add scheduling latency. You may see more events at low thresholds that aren't present on bare metal. Consider raising threshold by 1-2ms in containerized environments.

### NUMA effects

Cross-NUMA scheduling can add microseconds of latency. If you see inconsistent results, check if workers are pinned or migrating across nodes.

### Virtualization

VMs add jitter. Nested eBPF (VM inside VM) may not work. EC2/GCP instances generally work fine; local VMs (VirtualBox, etc.) may have issues.

### Busy systems

On systems with high CPU utilization (>80%), legitimate preemption increases. Raise threshold to filter scheduler contention from actual blocking.
