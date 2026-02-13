# Working with Exports

The live TUI shows functions disappearing from the hotspot list after a fix — useful for quick iteration. For empirical validation (CI gates, performance reviews), use `--export`.

See [Tuning](TUNING.md) for threshold selection and debugging workflow.

## Headless Mode

For CI pipelines, automated testing, or unattended profiling sessions:

```bash
sudo hud my-app --headless --export trace.json --duration 60
```

| Flag | Required | Description |
|------|----------|-------------|
| `--headless` | Yes | No TUI, runs silently until complete |
| `--export <file>` | Yes* | Output file for trace data (required with `--headless`) |
| `--duration <secs>` | No | Stop after N seconds. Omit to run until Ctrl+C |
| `--threshold <ms>` | No | Blocking threshold. Default: 5ms |
| `--window <secs>` | No | Rolling window (usually not needed for exports) |
| `--workers <prefix>` | No | Thread name prefix for worker discovery. Auto-detected if omitted |

### Session length examples

```bash
# Quick smoke test (1 minute)
sudo hud my-app --headless --export trace.json --duration 60

# Standard profiling session (5 minutes)
sudo hud my-app --headless --export trace.json --duration 300

# Extended soak test (1 hour)
sudo hud my-app --headless --export trace.json --duration 3600

# Run indefinitely until Ctrl+C (omit --duration)
sudo hud my-app --headless --export trace.json
```

### CI pipeline example

```yaml
# GitHub Actions
- name: Profile under load
  run: |
    ./load-generator.sh &
    sudo timeout 120 ./hud my-app --headless --export profile.json --duration 60

- name: Check for blocking regressions
  run: |
    EVENT_COUNT=$(jq '[.traceEvents[] | select(.ph=="B")] | length' profile.json)
    if [ "$EVENT_COUNT" -gt 100 ]; then
      echo "FAIL: $EVENT_COUNT blocking events detected"
      exit 1
    fi
```

## Before/after workflow

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

## What success looks like

| Metric | Before | After | Interpretation |
|--------|--------|-------|----------------|
| Total events | 847 | 312 | Fewer blocking events overall |
| Target function | 156 | 0 | Fixed function no longer blocks |
| Top 5 functions | Changed | Same (minus fix) | No new hotspots |

## Format

`--export` writes Chrome Trace Event format. Open in:

- [Perfetto](https://ui.perfetto.dev) — drag and drop
- [Speedscope](https://www.speedscope.app) — drag and drop
- Chrome — `chrome://tracing`

## JSON structure

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
