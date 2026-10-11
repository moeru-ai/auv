# Windows media control

Status: implemented on branch `feat/windows-media-control`. This is a feature slice of
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

- **The Runner's media service on Windows.** `MediaControlService` is the macOS-namespaced service
  and reads MediaRemote only, so the Runner still answers unsupported on Windows
  (`TODO(windows-media-runner)`, under `TODO(invoke-support-runner-route)`).
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

**The five control commands have not been run live.** They act on the music the user is playing,
so a run waits for the owner's go-ahead.

## Not evidenced

- Any control command, live.
- Apps other than QQ Music, including browsers, Spotify and the Media Player app.
- A machine with no media session (expected: `present: false`).
- A session that refuses a command.
- The Runner path, which is not wired.

## Validation

```sh
cargo fmt --check
cargo check
cargo clippy -p auv-driver-common -p auv-driver-windows -p auv-media-macos -p auv-cli-invoke --all-targets
cargo test --workspace --locked --no-fail-fast
cargo run --quiet -- invoke --help
git diff --check
auv invoke mediaControl.nowPlaying --json
```
