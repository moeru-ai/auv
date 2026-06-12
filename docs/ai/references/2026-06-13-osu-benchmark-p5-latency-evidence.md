# P5 Dispatch Latency Evidence

Date: 2026-06-13

Status: local evidence note for the in-progress P5 slice

## Scope

This note records the real-app evidence collected while hardening osu benchmark
P5 dispatch latency. The slice goal is honest multi-object dispatch timing:
exclude one-time setup from the measured window, then determine whether any
remaining latency floor is first-object-only or per-click.

## Code change under test

Local code change in `crates/auv-game-osu/src/benchmark.rs`:

- warm the window-targeted typed click path before starting the measured clock
- keep `dispatch_error_ms = actual_dispatch_time_ms - scheduled_time_ms`
  unchanged
- leave capture-verify runs unchanged so P5 measures dispatch latency, not
  capture latency

## Real-app run after benchmark-layer hardening

Command shape:

```text
cargo run --quiet --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml -- osu dispatch /Users/liuziheng/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rosu-map-0.2.1/resources/sample-beatmap-osu.osu --target-app "osu!" --dispatch-limit 20 --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p5-after
```

Run id:

```text
run_1781298128793_4766_0
```

Output dir:

```text
.tmp-osu-dispatch-p5-after
```

Observed `latency_report.json`:

```json
{
  "run_mode": "typed_dispatch",
  "total_actions": 12,
  "mean_error_ms": 122.58333333333333,
  "p50_error_ms": 123,
  "p95_error_ms": 125,
  "p99_error_ms": 126,
  "max_error_ms": 126,
  "jitter_ms": 7,
  "missed_schedule_count": 12
}
```

Observed `dispatch_trace.json` pattern:

- object 0: `dispatch_error_ms = 121`
- object 1: `dispatch_error_ms = 124`
- object 2: `dispatch_error_ms = 124`
- object 3: `dispatch_error_ms = 120`
- object 4: `dispatch_error_ms = 119`
- object 5: `dispatch_error_ms = 119`
- object 6: `dispatch_error_ms = 125`
- object 7: `dispatch_error_ms = 125`
- object 8: `dispatch_error_ms = 123`
- object 9: `dispatch_error_ms = 123`
- object 10: `dispatch_error_ms = 126`
- object 11: `dispatch_error_ms = 122`

All 12 actions used:

```text
WindowTargetedMouse
```

No fallback reasons were recorded.

## Interpretation

The benchmark-layer warm-up removed the original symptom class that motivated
P5: the first object is no longer a unique outlier. After the change, object 0
is in the same `119ms-126ms` band as the rest of the run.

This means the remaining latency floor is not setup accidentally counted inside
only the first scheduled click. The floor is now a steady per-click cost on the
successful `WindowTargetedMouse` path.

Given the current codebase, the strongest remaining hypothesis is the native
`ChromiumCompatible` click strategy in
`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift`,
which intentionally performs fixed primer/move/sleep work even when the
window-targeted path succeeds.

## Acceptance status vs roadmap

Roadmap requirement status:

- first-object dispatch error is no longer an outlier class of its own: yes
- post-warm-up p95 at or under 16ms: no (`p95 = 125ms`)
- remaining floor explained with evidence: partially yes

So P5 is **not yet acceptable to close**. The current state proves the timing
boundary fix worked, but the remaining steady floor still needs either:

1. a narrow driver/native reduction on the successful click path, or
2. a stronger evidence-backed explanation that this floor is fundamental on the
   local machine for the chosen compatibility strategy.

## Real-app run after app-local switch to `PidTargeted`

Code under test:

- `crates/auv-game-osu/src/benchmark.rs` now uses `WindowClickStrategy::PidTargeted`
  for the osu benchmark typed dispatch path and its warm-up step
- `InputPolicy::ForegroundPreferred` remains unchanged

Command shape:

```text
cargo run --quiet --manifest-path /Users/liuziheng/https-github-com-moeru-ai-auv/Cargo.toml -- osu dispatch /Users/liuziheng/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rosu-map-0.2.1/resources/sample-beatmap-osu.osu --target-app "osu!" --dispatch-limit 20 --output-dir /Users/liuziheng/https-github-com-moeru-ai-auv/.tmp-osu-dispatch-p5-pid-targeted
```

Run id:

```text
run_1781299108760_5250_0
```

Output dir:

```text
.tmp-osu-dispatch-p5-pid-targeted
```

Observed `latency_report.json`:

```json
{
  "run_mode": "typed_dispatch",
  "total_actions": 12,
  "mean_error_ms": 0.0,
  "p50_error_ms": 0,
  "p95_error_ms": 0,
  "p99_error_ms": 0,
  "max_error_ms": 0,
  "jitter_ms": 0,
  "missed_schedule_count": 0
}
```

Observed `dispatch_trace.json` pattern:

- all 12 objects recorded `dispatch_error_ms = 0`
- all 12 actions used `WindowTargetedMouse`
- no fallback reasons were recorded

## Updated interpretation

The app-local switch from `ChromiumCompatible` to `PidTargeted` collapses the
previous steady `119ms-126ms` floor entirely on the tested local `osu!` setup.
This strongly confirms that the previous floor came from the compatibility
strategy itself rather than from benchmark timing semantics or unavoidable local
scheduler jitter.

Compared with the earlier evidence run `run_1781298128793_4766_0`:

- first-object outlier removal remains preserved
- successful-path steady floor disappears
- no fallback or delivery-path regression appears in the recorded trace

## Updated acceptance status vs roadmap

Roadmap requirement status after the `PidTargeted` run:

- first-object dispatch error is no longer an outlier class of its own: yes
- post-warm-up p95 at or under 16ms: yes (`p95 = 0ms`)
- remaining floor explained with evidence: yes

This means the current P5 slice is now acceptable to close on the tested local
machine, assuming no additional evidence requirement is imposed beyond the
existing roadmap gate.

## Related failed runs

Two earlier attempts in this session failed before producing comparable evidence
because the `osu!` window was not visible/resolvable at execution time:

- `run_1781297863113_4595_0`
- `run_1781298012418_4697_0`
