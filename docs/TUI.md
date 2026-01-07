# TUI Guide

Interactive terminal interface for hud.

## Overview

The TUI provides real-time visualization of Tokio runtime behavior with four interactive view modes:

1. **Analysis** - Hotspot overview (default)
2. **DrillDown** - Detailed timeline for selected hotspot
3. **Search** - Filter hotspots by name
4. **WorkerFilter** - Select which workers to display

## View Modes

### Analysis Mode

Default view showing hotspots and worker statistics.

```
┌───────────────────────────────────────────────────────┐
│ Status: 1,234 events | 5 workers | 23 hotspots       │
├───────────────────────────────────────────────────────┤
│ HOTSPOTS (sorted by total duration)                  │
│ ┌─────────────────────────────────────────────────┐  │
│ │ ▼ spawn_blocking   3,245ms  (52 hits)          │  │
│ │   file_io::read    1,892ms  (34 hits)          │  │
│ │   compute_hash     1,023ms  (12 hits)          │  │
│ └─────────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────────┤
│ WORKERS                                               │
│ Worker 0: ████████████░░░  75% (12 blocks)           │
│ Worker 1: ███████░░░░░░░░  50% (8 blocks)            │
│ Worker 2: ████████████████ 100% (23 blocks)          │
└───────────────────────────────────────────────────────┘
```

**Keyboard:**
- `↑/↓` - Navigate hotspots
- `Enter` - Drill down into selected hotspot
- `w` - Open worker filter
- `/` - Search hotspots
- `q` - Quit

### DrillDown Mode

Detailed view of a specific hotspot with timeline and stack traces.

```
┌───────────────────────────────────────────────────────┐
│ HOTSPOT: spawn_blocking::block_on                    │
│ Total: 3,245ms | Hits: 52 | Avg: 62ms                │
├───────────────────────────────────────────────────────┤
│ TIMELINE (per-worker execution)                      │
│ Worker 0: ─▂▃▄▅▆▇█─────▂▃▄───                        │
│ Worker 1: ────▂▃▄▅▆▇█──────▂▃                        │
│ Worker 2: ▂▃▄────────▂▃▄▅▆▇█─                        │
├───────────────────────────────────────────────────────┤
│ STACK TRACES                                          │
│ #0 spawn_blocking at pool.rs:42                      │
│ #1 read_file at io.rs:123                            │
│ #2 process_handler at main.rs:567                    │
└───────────────────────────────────────────────────────┘
```

**Keyboard:**
- `Esc` - Return to Analysis mode
- `↑/↓` - Scroll timeline

**Timeline bars:**
- Height represents duration (taller = longer)
- Position shows when event occurred
- Color indicates severity (green/yellow/red)

### Search Mode

Filter hotspots by function name or file path.

```
┌───────────────────────────────────────────────────────┐
│ Search: file_io▂                                      │
├───────────────────────────────────────────────────────┤
│ RESULTS (3 matches)                                   │
│  file_io::read_file    1,892ms  (34 hits)             │
│  file_io::write_file     432ms  (12 hits)             │
│  file_io::delete_file     89ms  (3 hits)              │
└───────────────────────────────────────────────────────┘
```

**Keyboard:**
- Type to search (case-insensitive)
- `Enter` - Select result and drill down
- `Esc` - Return to Analysis mode
- `Backspace` - Delete characters

**Search matches:**
- Function names
- File paths
- Partial matches

### WorkerFilter Mode

Select which worker threads to display in views.

```
┌───────────────────────────────────────────────────────┐
│ WORKER FILTER (space to toggle, enter to apply)      │
│  [x] Worker 0 (1,234 events)                          │
│  [x] Worker 1 (987 events)                            │
│  [ ] Worker 2 (456 events) ← filtered out             │
│  [x] Worker 3 (789 events)                            │
└───────────────────────────────────────────────────────┘
```

**Keyboard:**
- `↑/↓` - Navigate workers
- `Space` - Toggle selection
- `Enter` - Apply filter
- `Esc` - Cancel (keep current filter)

**Effect:**
- Filtered workers hidden from all views
- Hotspot calculations exclude filtered workers
- Useful for isolating specific workers

## Color Scheme

hud uses color to indicate severity and status:

| Color | Meaning |
|-------|---------|
| Green | Normal operation |
| Yellow | Caution (moderate blocking) |
| Red | Critical (severe blocking) |
| Blue | Informational |
| Gray | Inactive/filtered |

**Severity thresholds:**
- Green: < 10ms blocking
- Yellow: 10-50ms blocking
- Red: > 50ms blocking

## Status Panel

Top status bar shows real-time statistics:

```
Status: 1,234 events | 5 workers | 23 hotspots | ● LIVE
```

**Fields:**
- **Events:** Total events processed
- **Workers:** Number of Tokio worker threads
- **Hotspots:** Distinct blocking locations found
- **● LIVE:** Indicator that profiling is active (pulsing)

In replay mode, shows **REPLAY** instead of **● LIVE**.

## Hotspot List

Central view showing blocking operations sorted by total duration.

**Columns:**
```
▼ spawn_blocking   3,245ms  (52 hits)
  ────────────────  ───────  ────────
  Function name     Total    Hit count
```

**Sort order:** Always by total duration (descending)

**Selection:** `▼` indicates currently selected hotspot

**Hit count:** Number of times this location blocked

## Workers Panel

Bottom panel showing per-worker statistics.

```
Worker 0: ████████████░░░  75% (12 blocks)
          ───────────────  ─── ───────────
          Activity bar     %   Block count
```

**Activity bar:**
- Filled blocks: Worker was active
- Empty blocks: Worker was idle
- Length proportional to time window

**Percentage:** Approximate utilization (active / total time)

**Block count:** Number of blocking events on this worker

## Live vs Replay Mode

### Live Mode

Real-time streaming profiling:

- Data updates continuously
- `● LIVE` indicator pulsing
- Events arrive as they happen
- Press `Q` to quit and optionally export

### Replay Mode

Viewing previously captured trace:

- Data is static (pre-loaded)
- `REPLAY` indicator (no pulsing)
- Can navigate freely
- No new events arrive

## Performance

**Frame Rate:** 60 FPS (16ms frame budget)

**Event Handling:**
- Non-blocking channel polling
- Drops events if TUI can't keep up
- Typical throughput: >10k events/sec

**Responsiveness:**
- Keyboard input checked every frame
- UI updates even during high event rate
- Smooth scrolling and navigation

## Tips

### Finding Hot Functions

1. Start in Analysis mode
2. Look for functions with high total duration
3. Check hit count - high count = frequent blocking
4. Press `Enter` to drill down for timeline

### Understanding Timeline

1. DrillDown mode shows execution patterns
2. Clustered bars = bursty blocking
3. Evenly spaced bars = periodic blocking
4. Check which workers are affected

### Filtering Noise

1. Use `/` to search for specific functions
2. Use `w` to filter out idle workers
3. Focus on top 5-10 hotspots (sorted by duration)

### Exporting for Analysis

```bash
# Capture and export
sudo -E ./hud --pid <PID> --target <BINARY> --export trace.json

# Later: replay and analyze
./hud --replay trace.json
```

Share `trace.json` with team or open in other tools (Perfetto, Speedscope).

## Troubleshooting

**TUI garbled/broken:**
- Use modern terminal (alacritty, kitty, iTerm2)
- Set `TERM=xterm-256color`
- Try headless mode: `--headless`

**High CPU from TUI:**
- Normal during high event rates
- TUI processing is bounded (60 FPS)
- Consider shorter `--duration` or `--headless`

**Missing events in live mode:**
- Expected behavior under extreme load
- TUI drops events to maintain responsiveness
- Use `--export` to capture all events

## Keyboard Reference

| Key | Action |
|-----|--------|
| `q` | Quit |
| `↑/↓` | Navigate/scroll |
| `Enter` | Select/drill down |
| `Esc` | Back/cancel |
| `/` | Search |
| `w` | Worker filter |
| `Space` | Toggle (in filter mode) |
