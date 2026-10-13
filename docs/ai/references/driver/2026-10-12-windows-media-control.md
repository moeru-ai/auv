# Windows media control

Status: implemented in #342 (the direct path) and #351 (the Runner service). This is a feature slice of
[#211](https://github.com/moeru-ai/auv/issues/211): the six `mediaControl.*` commands on Windows.
On 2026-10-12 the owner left the design to the implementer, and this note records the choice.

## Scope and boundaries

The slice moves the platform-neutral half of the macOS media contract into
`auv-driver-common::media`. It then maps the Windows driver's existing System Media Transport
Controls (SMTC) backend onto that contract, and routes the invoke commands to it on Windows.

| Command | Windows cell | Path |
| --- | --- | --- |
| `mediaControl.nowPlaying` | `partial`: no duration, elapsed time or playback rate | `auv_driver_windows::media::now_playing` |
| `mediaControl.play`, `.pause`, `.togglePlayPause`, `.next`, `.previous` | `supported` | `auv_driver_windows::media::control` |

Not in this slice:

- **The Runner's media service on Windows.** It came next, in #351 (see [Runner route](#runner-route-2026-10-13)),
  which closed `TODO(windows-media-runner)`. Support cells for the Runner route are a separate design
  task (`TODO(invoke-support-runner-route)`).
- **Seek, stop, volume and choosing a session by app.** The driver has them for the QQ Music
  tooling, but no invoke command asks for them.
- **The SMTC timeline.** It is behind `TODO(windows-media-timeline)`; see the mapping below.

## The decision: one media contract, owned by the driver layer

Before this slice, the commands returned `auv_media_macos::output::{NowPlayingOutput,
MediaControlOutcome}`. No Windows type had that shape.

An earlier design, the
[Windows now-playing design of 2026-06-11](../ops/2026-06-11-media-windows-now-playing-design.md),
was still pending review. It proposed a separate `auv-media-windows` crate, which was never built:
the SMTC backend landed in `auv-driver-windows::media` instead. It left two questions to the owner:

- Open question 1: share the contract in a new `auv-media-core` crate, or duplicate it per
  platform. That note recommended duplicating it.
- Open question 3: keep `source_bundle_id` or rename it.

This slice answers both. The contract is shared, and the name is kept (see the mapping below).

Duplication is no longer the narrow option. The contract now has three consumers: the macOS
provider, the Windows provider, and the invoke commands with the `auv` client and the Runner. A
second copy of the same types would be the duplicate primitive the contributor guide rules out.

Three options were considered.

1. **A per-platform result.** A Windows `NowPlayingOutput` would give one command id two result
   shapes. That breaks the rule that every frontend sees one execution model and one result
   schema. Rejected.
2. **A new `auv-media` crate.** It would have two providers and no consumer beyond them.
   `auv-driver-common` already owns the shared driver contracts (input results, capture, display),
   and the terms guide puts contracts in the Driver or media crate that owns them. Rejected as
   needless surface.
3. **`auv-driver-common::media`.** Chosen.

The module holds `NowPlayingState`, `MediaCommand` (with its label), `SCHEMA_VERSION`
(`now-playing-v0`), `NowPlayingOutput`, `build_now_playing_output` and `MediaControlOutcome`,
together with the rule for judging a command `verified`.

`auv-media-macos` re-exports these items at their old paths (`auv_media_macos::NowPlayingState`,
`auv_media_macos::output::*`), so its consumers compile unchanged: the `auv` client, the Runner and
the macOS CLI. It keeps the MediaRemote adapter, the adapter JSON parser, the MRCommand ids and the
human summary. The wire shape does not change, and `now-playing-v0` stays the schema.

The verification rule had no tests before. It now has its own (`media_test.rs` in
`auv-driver-common`), since the Windows provider depends on it to know when to stop waiting.

## Windows mapping

`now_playing` reads the **current** SMTC session, the one Windows' own media flyout acts on.

| Shared field | Windows value |
| --- | --- |
| `present` | A current session exists and its status is not `Closed`. |
| `is_playing` | The status is `Playing`. `Paused`, `Stopped`, `Opened` and `Changing` are present but not playing. |
| `source_bundle_id` | The session's `SourceAppUserModelId`, for example `QQMusic.exe`. |
| `title`, `artist`, `album` | The media properties' title, artist and album title. Empty strings are absent. |
| `duration_seconds`, `elapsed_seconds`, `playback_rate` | Absent (`TODO(windows-media-timeline)`). |
| `content_item_id`, `supports_like`, `is_liked` | Absent: SMTC has no equivalent. |

- **`source_bundle_id` keeps its v0 name.** On Windows it carries an AppUserModelID, which is not a
  bundle id. Renaming the field would break `now-playing-v0` readers, so the name stays, and
  `TODO(now-playing-v1)` on the field records the platform-neutral rename for a schema bump. The
  `app:` target, which already means an executable name on Windows, follows the same pattern.
- **The timeline is left out on purpose.** SMTC has `GetTimelineProperties` and a playback rate.
  Its position, however, is a snapshot that must be extrapolated from `LastUpdatedTime`, and no
  app's values were checked.
- **There is no fallback to "the first session".** The existing `SmtcMediaManager::now_playing`
  falls back that way for the QQ Music tooling. A control acts on the current session, so the read
  reports the same one.

`control(command)` works like this:

1. Take the current session. Without one, fail with "an active media session was not found".
2. Read the before-state.
3. Send `TryPlayAsync`, `TryPauseAsync`, `TryTogglePlayPauseAsync`, `TrySkipNextAsync` or
   `TrySkipPreviousAsync`. If the app answers `false`, it refused the command, and the call fails
   with "the media session "…" refused the … command".
4. Read the after-state **from the same session** every 50 ms. Stop when the shared rule judges the
   command verified, or after 3 s. `verified` says which way it ended.

The 3 s bound comes from measurements. SMTC reports a command's effect asynchronously. QQ Music's
track change was measured at about 75 ms on its fast path, 0.8 to 2.8 s on its slow path, and up
to 3.2 s for the first skip after a cold start
([spike](2026-10-07-verified-fast-path-spike.md)). The wait ends as soon as the effect shows, so
the bound only costs time when a command has no visible effect (`NOTICE` at
`CONTROL_VERIFY_TIMEOUT`).

macOS waits a fixed 200 ms and reads once.

## Tests

- `auv-driver-common` `media_test.rs` covers the verification rule:
  - play and pause are judged by the after-state;
  - nothing playing afterwards is unverified;
  - a toggle needs the playing state to change;
  - a skip needs the track identity to change;
  - the outcome carries the label and both states in v0.
- `auv-driver-windows` `media_test.rs` covers the SMTC mapping:
  - no session, or a closed session, is idle;
  - a playing session maps field by field;
  - the other statuses are present and not playing;
  - empty strings are absent.
- The existing invoke tests (`media_control_test.rs`) still pass. They check the report shape and
  reject `--target` before any platform access.

## Live evidence

Host: Windows 11 Pro 10.0.29648, on 2026-10-12, with QQ Music playing.

`auv invoke mediaControl.nowPlaying --json` ran three times (record:
[`evidence/2026-10-12-windows-media-control/now-playing.jsonl`](evidence/2026-10-12-windows-media-control/now-playing.jsonl)).

- Each run reported `present: true`, `is_playing: true`, `source_bundle_id: "QQMusic.exe"`, and a
  title, artist and album.
- The timeline, content id and like fields were `null`, as mapped.
- Each run took 88 to 100 ms through the whole `auv` process.
- The record keeps only whether each text field was reported. The titles are the user's own music.

The owner then approved one live run of each control command. They ran in sequence, with QQ Music
playing (record:
[`evidence/2026-10-12-windows-media-control/controls.jsonl`](evidence/2026-10-12-windows-media-control/controls.jsonl)).
The order gave every command a real effect:

- `pause`, then `play`: one flip each;
- `next`, then `previous` straight after it, so QQ Music went back a track instead of restarting
  the current one;
- `togglePlayPause`, last.

| Command | Before → after | Track changed | `verified` | Whole `auv` process |
| --- | --- | --- | --- | --- |
| `pause` | playing → paused | no | yes | 250 ms |
| `play` | paused → playing | no | yes | 157 ms |
| `next` | playing → not playing (see below) | yes | yes | 222 ms |
| `previous` | not playing → not playing | yes | yes | 1294 ms |
| `togglePlayPause` | playing → paused | no | yes | 144 ms |

- **All five exited 0, and every one was `verified`.** The source was `QQMusic.exe` throughout.
- **`previous` landed on the track that was playing before `next`.** `pause`, `play` and the toggle
  kept the original track. The record holds only these yes/no facts.
- **`previous` took 1.3 s.** Most of that was the wait for the track change to show, which fits
  the measured slow path.
- **Playback was left paused on the original track.** Three play/pause flips starting from
  playing end paused, and no extra command was sent to resume it.

### Finding: right after a skip, the after-state can read "not playing"

`next` returned as soon as the track changed, as the skip rule asks. At that moment QQ Music
reported a status other than `Playing` while it switched tracks, so the outcome says
`after.is_playing: false`. `previous` started from that same state. By the time
`togglePlayPause` ran, playback had resumed by itself.

A caller should not read `is_playing` from a skip's outcome as "playback stopped". The shared
state does not keep the raw SMTC status, so it is not known whether the status was `Changing` or
`Paused`.

## Not evidenced

- Apps other than QQ Music, including browsers, Spotify and the Media Player app.
- More than one run of each control command.
- A machine with no media session (expected: `present: false`).
- A session that refuses a command.
- The Runner path on platforms other than macOS/Windows.

## Runner route (2026-10-13)

Implemented in #351, closing `TODO(windows-media-runner)`.
The Runner's `LocalMediaControlService` gRPC service in `crates/auv-cli/src/runner/local_driver.rs`
is wired to `auv_driver_windows::media` on Windows, matching the direct invoke path from #342.

On Windows the Runner now serves the macOS-namespaced service
`auv.api.driver.macos.v1.MediaControlService`. The contract is platform-neutral since this slice
moved it to `auv-driver-common::media`, but the proto package still says `macos`. Whether to move
the service to a neutral package is a contract question for later.

### Implementation details

- **Dual-platform routing**: `read_now_playing()` and `control_media(command)` dispatch to
  `auv_driver_windows::media::{now_playing, control}` on `target_os = "windows"`, and
  `auv_media_macos` on `target_os = "macos"`.
- **Status mapping**:
  - `DriverError::Backend` maps to `Status::unknown` (non-retryable uncertain outcome; preserves
    fail-closed semantics against blind client retries when command delivery state is unknown).
  - `DriverError::NotFound` maps to `Status::not_found` (no active media session).
  - `DriverError::Unsupported` maps to `Status::unimplemented`.
- **Shared proto conversion**: Proto converters (`media_control_outcome_to_proto`,
  `now_playing_to_proto`, `now_playing_output_to_proto`) use platform-neutral
  `auv_driver::media::*` types.
- **Service registration**: `serve_local_driver` advertises and health-reports
  `MediaControlServiceServer` on Windows (`#[cfg(any(target_os = "macos", target_os = "windows"))]`).
- **Scope boundary preserved**: `TODO(invoke-support-runner-route)` (Runner route support cells)
  remains deferred for a separate design task.

### Live empirical evidence

Host: Windows 11 Pro 10.0.29648, 2026-10-13, with live QQ Music (PID 21772) running in the background.
Client: gRPC `MediaControlServiceClient` over local in-process tonic server.
Evidence record: [`evidence/2026-10-12-windows-media-control/runner.jsonl`](evidence/2026-10-12-windows-media-control/runner.jsonl)
(from the ignored test `live_windows_runner_media_control_parity` in
`crates/auv-cli/src/runner/local_driver_test.rs`).

#### `mediaControl.nowPlaying` (3 runs)

| Run | `present` | `is_playing` | `source_bundle_id` | Metadata fields | Wall latency |
| --- | --- | --- | --- | --- | --- |
| 1 | true | true | `"QQMusic.exe"` | title, artist, album reported; timeline null | 24 ms |
| 2 | true | true | `"QQMusic.exe"` | title, artist, album reported; timeline null | 6 ms |
| 3 | true | true | `"QQMusic.exe"` | title, artist, album reported; timeline null | 4 ms |

- All 3 runs reported `present: true`, `is_playing: true`, and the active process `QQMusic.exe`.
- Median latency is ~6 ms via the local Runner gRPC route.
- Metadata titles and artists are masked in the committed JSONL record.

#### Control sequence (5 commands)

| Step | Command | Before → after | Track changed | `verified` | Wall latency |
| --- | --- | --- | --- | --- | --- |
| 1 | `pause` | playing → paused | no | true | 58 ms |
| 2 | `play` | paused → playing | no | true | 56 ms |
| 3 | `next` | playing → not playing | yes | true | 106 ms |
| 4 | `previous` | not playing → not playing | yes (returned to pre-next) | true | 1126 ms |
| 5 | `togglePlayPause` | playing → paused | no | true | 57 ms |

- **All five RPCs succeeded with `verified: true`**.
- **Track parity**: `previous` returned to the exact track playing before `next`.
- **Settle timing & asynchronous buffering**: Like direct invoke (#342), immediately after `next`
  and `previous`, QQ Music's metadata changes within 100 ms – 1.1 s while the audio stream is
  loading, reporting `is_playing: false`. In automated sequences, waiting for the stream to settle
  into `Playing` before `togglePlayPause` ensures the toggle acts on an active stream.
- **Verification timeout behavior**: The 3 s `CONTROL_VERIFY_TIMEOUT` in `auv_driver_windows::media`
  was sufficient for all 5 operations (P50 ~57 ms for state toggles; ~1.1 s for slow-path track
  switch). Cold starts taking >3.0 s will report honest `verified: false` as documented in #342.
- **Privacy masking**: User track titles and artists were masked; only Boolean presence and
  `track_changed` are committed.

### Runner route support: input to `TODO(invoke-support-runner-route)`

The command registry has `support` cells for the direct local path only, so there is no Runner
cell to change. For the design behind `TODO(invoke-support-runner-route)`, the Runner route on
Windows has the same shape as the direct path: `nowPlaying` has no timeline, and the five controls
each ran once and were verified. That evidence is one in-process gRPC client, not a selected-Runner
`auv invoke`.

## Validation

```sh
cargo fmt --check
cargo check -p auv-cli
cargo test --lib -p auv-cli runner::local_driver
cargo test --lib -p auv-cli live_windows_runner_media_control_parity -- --ignored --nocapture
```

## Candidate next slices

- After a skip, keep reading until the status leaves `Changing`, so `after.is_playing` is not a
  transient value. First record the raw SMTC status during a skip, to learn whether QQ Music
  reports `Changing` or `Paused`.
- `TODO(invoke-support-runner-route)`: runner route's own support declaration system.
