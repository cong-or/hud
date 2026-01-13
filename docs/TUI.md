# TUI Guide

## Keyboard

| Key | Action |
|-----|--------|
| `q` | Quit |
| `↑/↓` | Navigate/scroll |
| `Enter` | Select/drill down |
| `Esc` | Back/cancel |
| `/` | Search hotspots |
| `w` | Worker filter |
| `Space` | Toggle (in filter mode) |

## Views

### Analysis (default)

```
┌───────────────────────────────────────────────────────┐
│ Status: 1,234 events | 5 workers | 23 hotspots | ●LIVE│
├───────────────────────────────────────────────────────┤
│ HOTSPOTS (sorted by total duration)                   │
│  ▼ spawn_blocking   3,245ms  (52 hits)                │
│    file_io::read    1,892ms  (34 hits)                │
│    compute_hash     1,023ms  (12 hits)                │
├───────────────────────────────────────────────────────┤
│ WORKERS                                               │
│  Worker 0: ████████████░░░  75% (12 blocks)           │
│  Worker 1: ███████░░░░░░░░  50% (8 blocks)            │
└───────────────────────────────────────────────────────┘
```

- `▼` marks selected hotspot
- Press `Enter` to drill down, `/` to search, `w` to filter workers

### DrillDown

Shows timeline and stack traces for selected hotspot.

- Timeline bars: height = duration, color = severity (green/yellow/red)
- Clustered bars = bursty blocking, evenly spaced = periodic
- `Esc` to return

### Search

Type to filter hotspots by function name or file path. Case-insensitive, partial matches.

### Worker Filter

Toggle workers with `Space` to isolate specific threads. Filtered workers excluded from all views.

## Colors

| Color | Blocking Duration |
|-------|-------------------|
| Green | < 10ms |
| Yellow | 10-50ms |
| Red | > 50ms |

## Export & Replay

```bash
# Capture
sudo -E ./hud --pid <PID> --target <BINARY> --export trace.json

# Replay
./hud --replay trace.json
```

Compatible with [Perfetto](https://perfetto.dev) and [Speedscope](https://speedscope.app).
