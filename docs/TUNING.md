# Tuning

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

```bash
sudo hud my-app --threshold 1 --duration 60 --export before.json --headless
# deploy fix
sudo hud my-app --threshold 1 --duration 60 --export after.json --headless
```

Compare event counts and hotspot distribution before/after.

## Environment considerations

### Container overhead

Container runtimes add scheduling latency. You may see more events at low thresholds that aren't present on bare metal. Consider raising threshold by 1-2ms in containerized environments.

### NUMA effects

Cross-NUMA scheduling can add microseconds of latency. If you see inconsistent results, check if workers are pinned or migrating across nodes.

### Virtualization

VMs add jitter. Nested eBPF (VM inside VM) may not work. EC2/GCP instances generally work fine; local VMs (VirtualBox, etc.) may have issues.

### Busy systems

On systems with high CPU utilization (>80%), legitimate preemption increases. Raise threshold to filter scheduler contention from actual blocking.

## Working with exports

The live TUI shows functions disappearing from the hotspot list after a fix — that's useful for quick iteration. For empirical validation, use `--export` to capture data you can compare.

### Before/after workflow

**Step 1: Capture baseline under load**

```bash
# Start your app, generate realistic load, then:
sudo hud my-app --threshold 5 --duration 60 --export before.json --headless
```

**Step 2: Deploy your fix**

**Step 3: Capture again with same load**

```bash
sudo hud my-app --threshold 5 --duration 60 --export after.json --headless
```

**Step 4: Compare event counts**

```bash
$ jq '[.traceEvents[] | select(.ph=="B")] | length' before.json
847

$ jq '[.traceEvents[] | select(.ph=="B")] | length' after.json
312
```

Event count dropped from 847 to 312 — that's a 63% reduction in blocking events.

**Step 5: Check if your target function is gone**

```bash
$ jq -r '.traceEvents[] | select(.ph=="B") | .name' before.json | grep -c "sync_write"
156

$ jq -r '.traceEvents[] | select(.ph=="B") | .name' after.json | grep -c "sync_write"
0
```

`sync_write` went from 156 events to 0 — the fix worked.

**Step 6: Check you didn't introduce new hotspots**

```bash
$ jq -r '.traceEvents[] | select(.ph=="B") | .name' before.json | sort | uniq -c | sort -rn | head -5
156 my_app::sync_write
 89 my_app::parse_config
 45 serde_json::from_str

$ jq -r '.traceEvents[] | select(.ph=="B") | .name' after.json | sort | uniq -c | sort -rn | head -5
 89 my_app::parse_config
 45 serde_json::from_str
```

`sync_write` is gone. Other functions stayed the same — no new problems introduced.

### What success looks like

| Metric | Before | After | Interpretation |
|--------|--------|-------|----------------|
| Total events | 847 | 312 | Fewer blocking events overall |
| Target function | 156 | 0 | Fixed function no longer blocks |
| Top 5 functions | Changed | Same (minus fix) | No new hotspots |

### Format

`--export` writes Chrome Trace Event format. Open in:

- [Perfetto](https://ui.perfetto.dev) — drag and drop
- [Speedscope](https://www.speedscope.app) — drag and drop
- Chrome — `chrome://tracing`

### JSON structure

```json
{
  "traceEvents": [
    {
      "name": "your_code::handler",
      "cat": "execution",
      "ph": "B",
      "ts": 1234.56,
      "pid": 12345,
      "tid": 12346,
      "args": { "worker_id": 0, "detection_method": 2 }
    }
  ]
}
```

| Field | Meaning |
|-------|---------|
| `name` | Function where blocking detected |
| `ph` | Phase: `B` = block started, `E` = block ended |
| `ts` | Timestamp (microseconds since trace start) |
| `tid` | Thread ID (Tokio worker) |
| `args.worker_id` | Which Tokio worker (0, 1, 2...) |
| `args.detection_method` | `2` = exceeded off-CPU threshold |
