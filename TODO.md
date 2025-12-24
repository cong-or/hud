# TODO: Critical Path to Production

## ⚠️ REMOVE TRAINING WHEELS (Phase 4)

**Current state:** Using `#[no_mangle]` marker functions
**Target state:** Zero code changes required
**Blocker:** Need to implement scheduler tracepoint approach

### Why markers are temporary:

**Problems with current approach:**
- ❌ Users must modify their code with `#[no_mangle]`
- ❌ Only traces explicitly marked functions
- ❌ Doesn't work on library code (can't modify Tokio)
- ❌ Not practical for production use

**Production approach (scheduler tracepoints):**
- ✅ Zero code changes required
- ✅ Works on ALL code (including inlined functions)
- ✅ Profile any binary without modification
- ✅ Same approach as `perf`, DTrace, etc.

### Implementation Plan:

```
Phase 1: ✅ DONE - Basic blocking detection with markers
Phase 2: 🚧 NEXT - Add stack traces (still with markers)
Phase 3: 🎯 TODO - Switch to scheduler tracepoints
Phase 4: 🚀 TODO - Production ready (zero instrumentation)
```

### Phase 3 Tasks (Remove Markers):

**Step 1: Hook scheduler tracepoints**
```c
// Instead of hooking user functions:
uprobe:trace_blocking_start  // ❌ Remove this

// Hook kernel scheduler:
tracepoint:sched:sched_switch  // ✅ Use this
tracepoint:sched:sched_wakeup  // ✅ And this
```

**Step 2: Detect blocking from scheduler events**
```c
// When a thread runs too long without yielding:
if (thread_cpu_time > 10ms && is_tokio_worker) {
    // ⚠️ Blocking detected!
    stack = bpf_get_stackid();
    report_blocking_event();
}
```

**Step 3: Remove all `#[no_mangle]` from test apps**
```diff
- #[no_mangle]
- fn trace_blocking_start() { }

// No markers needed at all!
```

**Step 4: Update documentation**
- Remove mentions of requiring code changes
- Update README with "zero instrumentation" messaging
- Add comparison to other profilers

### Success Criteria:

```bash
# User runs their app normally (no modifications)
cargo run --release

# Profile it (no code changes needed!)
sudo runtime-scope --pid $(pgrep your-app)

# Output shows exact blocking locations
# WITHOUT any #[no_mangle] markers!
```

---

## Phase 2 TODO (Next Session)

**Goal:** Show developers WHERE in code blocking happens

### Tasks:
1. [ ] Capture stack traces with `bpf_get_stackid()`
2. [ ] Store stack traces in eBPF maps
3. [ ] Send stack data to userspace
4. [ ] Resolve addresses to symbols (addr2line)
5. [ ] Display file:line for each frame
6. [ ] Show demangled function names

**Still using markers in Phase 2** - but learning stack unwinding.

---

## Long-term Vision

**What users will experience:**

```bash
# Download and run (no code changes!)
cargo install runtime-scope
sudo runtime-scope --pid $(pgrep your-app)
```

**Output:**
```
🔴 BLOCKING DETECTED
   Duration: 450ms
   Location: src/api.rs:142 in process_large_file()

   Stack trace:
   #0 process_large_file at src/api.rs:142
   #1 handle_upload at src/api.rs:89
   #2 tokio::runtime::task::poll

   💡 Fix: Use tokio::task::spawn_blocking()
```

**Zero code changes. Zero instrumentation. Just works.** 🎯

---

## References

**How other profilers do this:**
- `perf`: CPU sampling + scheduler hooks (no code changes)
- DTrace: Kernel tracepoints (no code changes)
- `py-spy`: Process memory inspection (no code changes)

**We'll do the same for Rust async!**

---

**Last Updated:** December 24, 2024
**Current Phase:** 1 (markers still in use)
**Target Phase:** 4 (zero instrumentation)
**ETA:** Phase 3-4 (after stack traces working)
