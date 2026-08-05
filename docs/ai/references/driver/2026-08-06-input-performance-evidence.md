# Input performance evidence

This document records measured latency for AUV input delivery. It is a live
probe at commit `232e0f737fe0d62e91a82cfd6059c65400cca1e1` with local uncommitted
changes on 2026-08-06. It is not a platform support claim, and it measures API
return latency rather than semantic UI success. The returned
`InputActionResult` values were delivery evidence with `verified = false`.

## Summary

- Direct macOS PID-targeted mouse and window-targeted Unicode keyboard delivery
  are already in the single-digit microsecond range. Rust/Swift FFI is not the
  limiting factor for these paths.
- Global macOS click, Chromium-compatible click, foreground click, and
  foreground keyboard operations are dominated by deliberate sleeps or a new
  `osascript`/System Events process, not Rust dispatch.
- Local daemon + gRPC over a Unix domain socket is inexpensive after the client
  channel and a run-affined Runner are warm: the measured routed validation
  path had a 0.066-0.502 ms p50 across repeated runs. This overhead is negligible
  next to a 100-250 ms foreground keyboard backend, but it dominates a direct
  6-7 microsecond PID-targeted operation in relative terms.
- Cold Runner creation is much more important than gRPC framing. On a fresh
  daemon, the first run-affined capability call took 97-117 ms. Calls without a
  `run_id` use ephemeral lifecycle policy and can repeatedly pay tens of
  milliseconds when no reusable Runner exists.
- Latency-sensitive workflows should reuse one `Client`, one running `Run`, and
  its routed Runner. Do not create a connection or an ephemeral route for every
  input event.

## Test environment and method

- Host: Apple M5 Max, macOS 26.3, Rust 1.96.1.
- Build: release mode, timed with `std::time::Instant` after warm-up.
- Direct target: a dedicated TextEdit document.
- Daemon transport: tonic HTTP/2 over an isolated Unix domain socket.
- Daemon measurements separate channel connection, daemon control RPCs,
  ephemeral capability routing, run-affined routing, and the platform backend.
- Side-effecting operations use small sample sets. p50 and p95 are more useful
  than p99 for these measurements.
- System Events timings changed substantially with machine state during the
  session. Transport-only measurements are therefore the reliable measure of
  gRPC overhead; keypress end-to-end measurements describe the whole path but
  do not isolate the transport.

## Direct macOS driver

All values are milliseconds unless a row explicitly uses microseconds.

| API | n | mean | p50 | p95 | Main included work |
|---|---:|---:|---:|---:|---|
| `window.resolve` | 30 | 5.824 | 3.547 | 13.178 | Native window enumeration |
| `input.current_position` | 500 | 0.000404 | 0.000375 | 0.000458 | `NSEvent.mouseLocation` |
| `input.move_to` | 100 | 0.116 | 0.068 | 0.169 | Warp and HID move event |
| `input.click_at(Single)` | 20 | 18.668 | 19.171 | 20.673 | Warp, fixed 15 ms settle, HID down/up, restore |
| `window.click(PidTargeted)` | 30 | 0.00595 | 0.00579 | 0.00829 | Window-stamped `postToPid` down/up |
| `window.click(ChromiumCompatible)` | 15 | 122.884 | 123.220 | 126.228 | Primer/target event pairs and fixed sleeps |
| `window.click(ForegroundPreferred)` | 12 | 205.185 | 204.859 | 212.741 | Activation, 50 ms settle, global click |
| `window.type_text("x")` | 40 | 0.00732 | 0.00717 | 0.00904 | One Unicode CGEvent down/up pair |
| `window.type_text(10 ASCII)` | 20 | 0.0699 | 0.0595 | 0.1215 | Ten Unicode CGEvent pairs |
| `input.type_text("y")` | 15 | 107.348 | 107.704 | 116.475 | Spawn and wait for System Events `osascript` |
| `input.type_text(10 ASCII)` | 10 | 253.366 | 250.329 | 266.759 | One script with ten System Events keystrokes |
| `input.press_key("z")` | 15 | 106.504 | 103.579 | 116.368 | System Events `osascript` |
| `input.press_key("cmd+z")` | 12 | 105.459 | 102.063 | 116.771 | System Events shortcut |
| `input.copy()` | 12 | 109.409 | 112.159 | 118.033 | System Events Command+C |
| `input.paste_text("p")` | 8 | 264.973 | 264.343 | 268.927 | Clipboard transaction, script, and settle |

Measured component baselines:

| Component | mean | p50 | p95 |
|---|---:|---:|---:|
| Empty `osascript -e 'return 1'` process | 32.444 ms | 31.915 ms | 35.657 ms |
| `sleep(15 ms)` | 17.420 ms | 17.440 ms | 18.768 ms |
| `sleep(50 ms)` | 53.221 ms | 54.209 ms | 55.026 ms |
| `sleep(100 ms)` | 103.568 ms | 104.537 ms | 105.015 ms |
| `sleep(150 ms)` | 153.786 ms | 154.510 ms | 155.021 ms |

The source matches the measurements:

- Global pointer delivery sleeps for 15 ms after moving the pointer in
  `native/swift/Sources/AuvMacosNative/Pointer.swift`.
- Chromium-compatible delivery contains 1 ms and 100 ms waits in the same
  native pointer implementation.
- Foreground preparation requests a 50 ms settle in `src/session.rs`.
- Foreground text, key, copy, and paste operations synchronously launch
  `/usr/bin/osascript` from `src/session.rs`.

These waits are wall time and do not appear as CPU samples. Time Profiler is
useful for native call attribution, but source-level wait accounting is needed
to explain the observed latency.

## Local daemon + gRPC

The local path is:

```text
client
  -> tonic HTTP/2 over Unix domain socket
  -> daemon route admission
  -> local Runner gRPC service
  -> auv-driver-macos
  -> macOS API or System Events
```

The daemon has two materially different capability-routing modes:

- A route with `run_id` creates an `UnlessShutdown` Runner and records affinity
  for that Run/Device/RunnerClass. Later calls reuse it.
- A route without `run_id` creates an `Ephemeral` Runner if no compatible Runner
  is already available. The Runner is stopped after the operation.

This behavior is owned by `crates/auv-daemon/src/daemon/mod.rs`; it is not an
intrinsic cost of gRPC.

### Transport and warm route cost

Repeated release runs produced the following representative values:

| Measurement | p50 | p95 | Meaning |
|---|---:|---:|---|
| Connect to local UDS | 0.023-0.079 ms | 0.037-0.136 ms | New tonic channel connection |
| Warm `ListDevices` control RPC | 0.036-0.089 ms | 0.057-0.183 ms | gRPC + daemon control handler, no Runner |
| Warm run-affined rejected `PressKey` | 0.066-0.502 ms | 0.107-2.420 ms | Both gRPC hops, route lookup, Runner service, validation; no OS input |
| Warm run-affined `ListDisplays` | 0.295-4.112 ms | 0.836-34.466 ms | Routed path plus real display enumeration |

The rejected `PressKey` case is intentionally a validation error and is not an
input success measurement. It is useful because it traverses the complete
routed input RPC path without adding `osascript` or an input side effect. On a
quiet warm run its p50 was 0.066-0.148 ms; the higher values above preserve the
observed machine-load variance instead of presenting only the best run.

### Cold and ephemeral route cost

On a fresh daemon with no reusable Runner:

| Measurement | observed latency |
|---|---:|
| First run-affined `ListDisplays` | 97-117 ms |
| Ephemeral `ListDisplays`, p50 | 97 ms |
| Ephemeral rejected `PressKey`, p50 | 24.7 ms |
| Ephemeral successful ASCII `PressKey`, p50 | 247.8 ms |

After a compatible Runner already existed, the same no-`run_id` probe could
reuse it and fell to approximately 0.9 ms for `ListDisplays` and 0.154 ms for
the rejected key. Therefore “ephemeral gRPC latency” is not one stable number;
it depends on Runner availability and lifecycle state.

### Successful key delivery

With a warm run-affined Runner, a quiet early run measured an ASCII keypress at
111.3 ms p50. The direct baseline from the same investigation was 103.6 ms p50.
Later, while System Events was slower, contemporaneous samples were 174.0 ms
p50 direct and 184.2 ms p50 through daemon + gRPC. These small sample sets do
not support attributing the 8-10 ms difference entirely to transport: the
transport-only routed probe was sub-millisecond, while `osascript` itself had
large variance.

The practical interpretation is:

- For foreground key/text/copy/paste, daemon + gRPC is not the bottleneck. The
  System Events backend and configured settles dominate.
- For direct PID-targeted click or window Unicode typing, warm gRPC changes the
  cost class from a few microseconds to roughly hundreds of microseconds. The
  relative multiplier is large, but absolute latency remains below 1 ms on a
  quiet local route.
- Cold Runner creation can dominate either kind of action and must be kept out
  of per-event loops.

## CLI `invoke`

`auv invoke` has two execution paths. They should not be reported as one
latency number:

```text
auv invoke ...
  -> parse + tracing store
  -> in-process typed invoke command
  -> new local driver session

auv --device-id ... invoke ...
  -> parse + tracing store
  -> resolve/create daemon Run
  -> gRPC route to a long-lived local Runner
  -> finish daemon Run
```

The unqualified form is direct and does not discover or contact a daemon.
Explicit `--device`, `--device-id`, or `--run` selection activates the shared
daemon execution model.

The following process-level benchmarks launched the release `auv` binary for
every sample. They include process startup, argument parsing, JSON projection,
opening and flushing an isolated file tracing store, operation execution, and
process exit. Output bytes were redirected to `/dev/null`, but serialization
still ran.

| CLI command/path | n | mean | p50 | p95 |
|---|---:|---:|---:|---:|
| `auv --version` process baseline | 100 | 5.216 ms | 5.010 ms | 7.798 ms |
| Direct `invoke display.list --dry-run --json` | 50 | 5.349 ms | 5.305 ms | 6.104 ms |
| Direct `invoke display.list --json` | 50 | 50.778 ms | 46.216 ms | 75.107 ms |
| Direct `invoke input.key a --json` | 15 | 119.928 ms | 118.988 ms | 128.635 ms |
| Daemon-selected rejected `input.key` | 30 | 7.630 ms | 7.354 ms | 8.512 ms |
| Daemon-selected `display.list --json` | 50 | 13.481 ms | 14.504 ms | 17.892 ms |
| Daemon-selected `input.key a --json` | 15 | 119.243 ms | 116.877 ms | 135.033 ms |

The rejected daemon key used an empty key and expected an error. It still
created/resolved the implicit Run, traversed the daemon and routed Runner input
service, flushed tracing, finalized the Run, and exited. It is therefore a
side-effect-free approximation of the warm selected-invoke fixed cost, not a
successful input measurement.

After explicitly stopping the reusable Runner between samples, three cold
daemon-selected `display.list` invocations took 48.8, 56.2, and 69.1 ms. This
small cold sample confirms that Runner creation is visible at CLI level, but
the earlier raw gRPC probe's wider 97-117 ms cold range should also be retained;
cold startup varies with process and machine state.

Three consequences matter:

- A short direct no-op invocation is mostly the approximately 5 ms process and
  frontend floor. Keeping a CLI process alive through MCP or another frontend
  can avoid paying this floor for every operation.
- Warm daemon-selected `display.list` was faster than direct CLI
  `display.list` (14.5 ms versus 46.2 ms p50). The direct process opens a new
  local driver session for every invocation, while the daemon can reuse the
  long-lived Runner and its session. This does not mean gRPC is faster than a
  direct function call; it means lifecycle reuse outweighs IPC for this
  process-per-command comparison.
- `input.key` was effectively tied at about 117-119 ms p50 because both paths
  are dominated by the System Events subprocess. CLI and gRPC overhead are
  secondary on that operation.

## Usage guidance

For local daemon workflows:

1. Connect once and clone/reuse the tonic-backed `Client`; do not reconnect per
   action.
2. Create or attach to one running `Run`, route with its `run_id`, and reuse the
   resulting Runner across the workflow.
3. Send text as one `TypeText` request instead of one RPC per character.
4. Use the existing streaming mouse-motion RPC for a motion sequence instead
   of issuing one unary RPC per sample.
5. Keep semantic verification separate. A fast RPC response or successful
   `InputActionResult` still does not prove the application reached the intended
   state.
6. For high-frequency command use, prefer a persistent frontend such as MCP or
   a library client over spawning `auv invoke` for every small operation.

If the product needs consistently sub-millisecond end-to-end actions, the next
benchmark slice should alternate direct and run-affined calls against the same
backend state and record Runner lifecycle spans. Optimization should first
target Runner cold-start policy and the `osascript` process boundary, not
protobuf serialization.

## Linux context

The same investigation measured a GNOME Wayland host on a different commit, so
these values are architectural context rather than a same-commit comparison:

| Direct Linux API | mean | p50 | p95 |
|---|---:|---:|---:|
| Restored portal session + first key | 50.167 ms | - | - |
| Warm `input.press_key("x")` | 0.767 ms | 0.789 ms | 0.904 ms |
| Warm `input.type_text("y")` | 0.443 ms | 0.449 ms | 0.532 ms |
| Current-position click | 13.119 ms | 13.017 ms | 14.001 ms |
| `window.list()` | 1515.041 ms | 1536.780 ms | 1818.837 ms |

Warm portal input is mainly synchronous D-Bus request/reply work. The click
includes a deliberate 12 ms press duration. `window.list()` is dominated by
AT-SPI and GNOME Shell accessibility traversal. A target-position pointer probe
failed because the portal returned without moving the observed pointer, so no
successful Linux target-relative click latency is claimed.

## Reproduction artifacts

The ignored local investigation folder is
`docs/notes/neko/auv-input-performance-2026-08-06/`. It contains the standalone
release benchmark sources, raw logs, macOS Time Profiler export, Linux
`pprof-rs` flamegraph, and syscall summary. The local daemon benchmark is under
`grpc-bench/` and uses an isolated Unix socket and store directory. Its
`process_bench.rs` harness measures complete CLI process latency.
