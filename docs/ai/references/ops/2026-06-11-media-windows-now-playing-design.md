# AUV Windows Now-Playing Capability (`auv-media-windows`) v0 Design

Date: 2026-06-11

Status: v0 design spec, pending review

A new leaf crate `auv-media-windows` reads the Windows system now-playing state
via the **public** `Windows.Media.Control` WinRT API
(`GlobalSystemMediaTransportControlsSessionManager`, "GSMTC"), the same data the
Windows volume/media overlay and SMTC surface. It is the Windows sibling of
`auv-media-macos`: a **lib + binary** crate that exposes the capability as a
library, reuses the existing agent-facing `now-playing-v0` output contract, and
ships an `auv-media-windows` binary with `now-playing` (read) plus transport
controls. `auv-netease-music` gains the same delegation path on Windows that it
has on macOS, so the read front door emits one identical contract on both
platforms.

Audience: owner, reviewers, and any agent (Codex, Claude, others) implementing
or reviewing the Windows now-playing capability.

## Purpose

Same agent need as the macOS spec: an agent loop driving a music app must read
"what is playing right now" without OCR-ing the player bar. Windows aggregates
now-playing state (title, artist, album, timeline, playback status, owning app)
for whatever app currently integrates with the System Media Transport Controls.
This spec exposes that state as a structured, agent-callable read, plus
fire-and-forget transport controls — mirroring `auv-media-macos` so the two
platforms present the same `now-playing-v0` contract.

This aligns with the `2026-06-11-windows-driver-feasibility-and-delivery-paths`
direction: Windows NetEase Music is the primary cross-platform validation
target, and now-playing is a shared capability that NetEase workflows can read
without app-specific OCR.

## Why the Windows approach is *simpler* than macOS (verified)

This is the load-bearing fact of this design, and it is the **inverse** of the
macOS situation.

- **macOS:** the only API that returns another app's now-playing state is the
  **private** `MediaRemote.framework`, gated since macOS 15.4 to Apple-signed
  processes. `auv-media-macos` must borrow `/usr/bin/perl` as an Apple-signed
  vehicle, vendor a BSD submodule, cmake-build a framework, and embed/unpack it.
  Apple could close the hole at any release.
- **Windows:** `GlobalSystemMediaTransportControlsSessionManager` is a
  **public, documented, supported** WinRT API in the `Windows.Media.Control`
  namespace. Reading *another* app's now-playing state is the API's stated
  purpose ("Represents a playback session from another app providing info about
  that session and possibly allowing control" — Microsoft Learn). No private
  framework, no borrowed vehicle, no vendored native source, no cmake, no
  embed/unpack.

Verified requirements (Microsoft Learn,
`GlobalSystemMediaTransportControlsSession`):

- Device family: **Windows 10, version 1809 (10.0.17763.0)** and later.
- API contract: `Windows.Foundation.UniversalApiContract` v7.0.
- App capability: `globalMediaControl` — this is a **packaged/UWP manifest**
  capability. An unpackaged Win32 console binary (what `auv-media-windows` is)
  generally calls GSMTC without a manifest capability; this must be confirmed on
  a real target, but it is the common case for desktop tooling.

Consequence for crate structure: `auv-media-windows` is a **pure-Rust** crate
that depends only on the first-party `windows` crate (windows-rs) with the
`Media_Control` (plus `Foundation`, `Storage_Streams`) features, gated behind
`cfg(target_os = "windows")`. No `build.rs`, no submodule, no FFI shim.

## Source semantics (decided)

GSMTC is **system-wide and app-agnostic**, exactly like MediaRemote: it returns
whichever app currently owns the session (Spotify, the NetEase Windows client, a
browser tab via the HTML Media Session API, Groove, etc.). This capability does
**not** filter to NetEase; it reports whatever is playing and includes the
owning app's identity so the caller can decide.

The Windows identity is `GlobalSystemMediaTransportControlsSession.SourceAppUserModelId`
(an AUMID / executable-derived id), which maps onto the contract's
`source_bundle_id` slot. The field name in the contract stays `source_bundle_id`
for cross-platform stability; on Windows it carries the AUMID. (A future
contract slice may rename it to a neutral `source_app_id`; out of scope here, and
consistent with how the Windows driver feasibility spec defers macOS-shaped name
cleanups.)

## Crate placement and layout

One new workspace member: a **leaf, lib + binary, pure-Rust** crate, mirroring
`auv-media-macos` but without any native build step.

```text
crates/auv-media-windows/
  Cargo.toml               // one [[bin]]: auv-media-windows
                           //   deps: serde, serde_json, clap
                           //   [target.'cfg(windows)'.dependencies] windows = { features = [
                           //     "Media_Control", "Foundation", "Storage_Streams" ] }
  src/
    lib.rs                 // NowPlayingState, parse path, now_playing(), MediaCommand, send_command(), seek()
    gsmtc.rs               // (windows) call GSMTC: RequestAsync -> GetCurrentSession ->
                           //   GetPlaybackInfo / GetTimelineProperties / TryGetMediaPropertiesAsync;
                           //   Try*Async controls
    output.rs              // reuses now-playing-v0 contract type + JSON/human builders
    cli.rs                 // subcommands (now-playing + transport/seek), run() -> ExitCode
    error.rs               // MediaError
    bin/
      auv-media-windows.rs // thin main -> auv_media_windows::cli::run()
```

There is **no** `build.rs` and **no** `vendor/`. The only platform dependency is
the `windows` crate. Off-Windows, the `gsmtc` module is not compiled and
`now_playing()` returns `MediaError::Unsupported`, matching how `auv-media-macos`
returns `Unsupported` off-macOS.

`Cargo.toml` depends only on `serde` + `serde_json` + `clap` (cross-platform)
plus the `cfg(windows)`-gated `windows` crate. It does **not** depend on
`auv-driver`, `auv-driver-windows`, `auv-cli`, or `auv-netease-music` (leaf
crate, same boundary discipline as `auv-media-macos`). Registered in the root
`Cargo.toml` `[workspace].members`.

### Shared contract type (decision)

`NowPlayingState` and the `now-playing-v0` output contract are **identical**
across platforms. To avoid a third copy, the recommended path is to factor the
platform-neutral contract (`NowPlayingState`, `NowPlayingOutput`,
`SCHEMA_VERSION`, builders, `MediaCommand`) into the existing
`auv-media-macos::output` consumers or a tiny shared `auv-media-core` leaf, and
have both `auv-media-macos` and `auv-media-windows` depend on it. **Open
decision for the owner** (see Open Questions): share via a new `auv-media-core`
crate vs. duplicate the small contract type per platform crate. v0 can duplicate
to stay narrow, but the contract bytes must match.

## Capability API

Identical surface to `auv-media-macos`:

```rust
pub struct NowPlayingState {
  pub present: bool,
  pub source_bundle_id: Option<String>,   // Windows: SourceAppUserModelId (AUMID)
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,       // (EndTime - StartTime) from TimelineProperties
  pub elapsed_seconds: Option<f64>,        // Position from TimelineProperties
  pub playback_rate: Option<f64>,          // PlaybackInfo.PlaybackRate (Option<f64>)
  pub is_playing: bool,                    // PlaybackInfo.PlaybackStatus == Playing
  pub content_item_id: Option<String>,     // no stable GSMTC equivalent -> None (see note)
  pub supports_like: Option<bool>,         // GSMTC has no like affordance -> always None
  pub is_liked: Option<bool>,              // always None on Windows
}

pub fn now_playing() -> Result<NowPlayingState, MediaError>;
```

Windows mapping (verified against `Windows.Media.Control`):

| Contract field | GSMTC source |
| --- | --- |
| `present` | `GetCurrentSession()` is non-null and yields valid media properties |
| `source_bundle_id` | `GlobalSystemMediaTransportControlsSession.SourceAppUserModelId` |
| `title` | `TryGetMediaPropertiesAsync().Title` |
| `artist` | `...Artist` |
| `album` | `...AlbumTitle` |
| `is_playing` | `GetPlaybackInfo().PlaybackStatus == Playing` |
| `playback_rate` | `GetPlaybackInfo().PlaybackRate` (nullable double) |
| `duration_seconds` | `GetTimelineProperties().EndTime - StartTime` (TimeSpan, ticks→seconds) |
| `elapsed_seconds` | `GetTimelineProperties().Position` (TimeSpan, ticks→seconds) |
| `content_item_id` | no stable GSMTC field; `None` in v0 |
| `supports_like` / `is_liked` | no GSMTC affordance; always `None` |

`PlaybackStatus` is the enum `Closed | Opened | Changing | Stopped | Playing |
Paused`. An idle/no-session result (`GetCurrentSession()` returns null) maps to
the default `NowPlayingState` (`present: false`), exactly like the macOS adapter
emitting `null`.

WinRT async is awaited synchronously in the one-shot CLI: windows-rs exposes
`IAsyncOperation::get()` to block. `RequestAsync()?.get()?` →
`TryGetMediaPropertiesAsync()?.get()?`. COM apartment/threading init is an
implementation detail to confirm on target (a one-shot console binary typically
initializes MTA); the spec does not prescribe a runtime — **no `tokio`
dependency** (unlike the third-party `win-gsmtc` wrapper).

Transport controls (system-wide, app-agnostic — they act on whichever app owns
the session) call the GSMTC `Try*Async` methods on the current session:

```rust
pub enum MediaCommand { Play, Pause, TogglePlayPause, NextTrack, PreviousTrack }
pub fn send_command(command: MediaCommand) -> Result<(), MediaError>;
pub fn seek(position: std::time::Duration) -> Result<(), MediaError>;
```

Mapping: `Play→TryPlayAsync`, `Pause→TryPauseAsync`,
`TogglePlayPause→TryTogglePlayPauseAsync`, `NextTrack→TrySkipNextAsync`,
`PreviousTrack→TrySkipPreviousAsync`. Each returns `IAsyncOperation<bool>`. As on
macOS, controls are **fire-and-forget** and return `Result<(), MediaError>` —
**not** a new action-result schema. The GSMTC bool only reports whether the
request was accepted, not whether playback changed; a verifier must **poll**
`now_playing()`, not read once.

`seek(Duration)` maps to `TryChangePlaybackPositionAsync(i64 ticks)`. **Unit
caveat (verified):** GSMTC seek is in **ticks (100-ns units)**, not microseconds
(the macOS adapter used microseconds). The Windows layer converts:
`ticks = duration.as_nanos() / 100`. The public Rust `seek(Duration)` signature
stays identical across platforms; only the internal conversion differs.

## CLI surface (two front doors, one contract)

Mirrors `auv-media-macos`:

```text
# the crate's own binary (read + transport controls)
auv-media-windows now-playing [--format summary|json] [--json-out <path>]
auv-media-windows play | pause | toggle | next | previous
auv-media-windows seek <seconds>

# the netease-music subcommands (delegate to the crate, scoped to one app)
auv-netease-music now-playing [--format summary|json] [--json-out <path>] [--app-id <aumid>]
auv-netease-music play | pause | toggle | next | previous [--app-id <aumid>]
auv-netease-music seek <seconds> [--app-id <aumid>]
```

Scoping semantics are identical to the macOS spec: `auv-media-windows` is
app-agnostic; `auv-netease-music` scopes to a single app via `--app-id`
(matching `SourceAppUserModelId`) and **refuses** transport/seek unless the
scoped app owns the session, so `auv-netease-music pause` never pauses an
unrelated browser tab. The NetEase default `--app-id` is the Windows NetEase
client AUMID (to be captured on a real target; the macOS default is
`com.netease.163music`). The same intentional contract divergence applies: the
netease front door omits the like fields (always null on Windows anyway).

`auv-netease-music` depends on `auv-media-windows` as a `cfg(target_os =
"windows")` dependency, peer to its existing `cfg(macos)` dependency on
`auv-media-macos`. On unsupported targets the subcommand prints "only available
on Windows/macOS" and exits non-zero.

## Output contract (agent-facing)

Unchanged from `now-playing-v0`:

- `--format json` / `--json-out <path>` produces the stable object carrying
  `schema_version: "now-playing-v0"` plus the `NowPlayingState` fields.
- Exit codes: `0` for a completed read **including the nothing-playing case**
  (`present: false`); non-zero for GSMTC/WinRT failure or a non-Windows
  `Unsupported` result.
- An agent distinguishes "paused" from "idle" via `is_playing` + `present`, and
  the source app via `source_bundle_id`.

Human output is identical to the macOS spec (`▶`/`⏸`/`Nothing playing`).

## Known limitations and risks (verified, skeptical)

- **Interactive session required.** GSMTC (a UWP/WinRT API) is **not available
  to a service or the SYSTEM account in a non-interactive session on Windows
  11** (confirmed via Microsoft Q&A). `auv-media-windows` must run in the
  interactive user session. Any non-interactive caller gets a `MediaError`, not
  a silent empty result.
- **App must integrate SMTC.** GSMTC only sees apps that publish to the System
  Media Transport Controls. Most mainstream players and Chromium/Edge (via the
  HTML Media Session API) do; a player that does not integrate is invisible to
  this read — the player-bar OCR path remains the durable fallback, same as
  macOS. **Must verify the Windows NetEase client publishes to SMTC** before
  treating this as NetEase's primary now-playing source; if it does not, NetEase
  on Windows stays on the visual probe and this crate serves other players.
- **Seek/fast-forward are app-dependent.** Reported community finding: some apps
  honor `TryPlayAsync`/`TryPauseAsync` but ignore
  `TryChangePlaybackPositionAsync`/`TryFastForwardAsync`. Consistent with the
  fire-and-forget contract — the caller must poll to confirm and accept that
  some apps refuse seek.
- **No like/favorite.** GSMTC exposes no like affordance, so `supports_like` /
  `is_liked` are always `None` on Windows (NetEase's 红心 would again require a
  separate UI-automation seam, not this crate).
- **AUMID stability.** `SourceAppUserModelId` shape varies (packaged AUMID vs.
  unpackaged executable-derived id); the NetEase `--app-id` default must be
  captured empirically on the real Windows client and may need substring/loose
  matching rather than exact equality.

These are honest-failure risks (surface as `MediaError` / non-zero exit), never
silent wrong answers.

## Testing

Pure-Rust unit tests (no live media, no Windows required — run on any host):

- The GSMTC → `NowPlayingState` mapping logic should be factored so the
  *parsing/normalization* (ticks→seconds, PlaybackStatus→`is_playing`, nullable
  field handling, idle→default) is a pure function over a small intermediate
  struct, unit-tested without WinRT. Mirror the macOS `parse_get` test set:
  null/idle, mapped object, paused (present, not playing), missing fields, garbage.
- `output`: `now-playing-v0` JSON carries schema version + fields; human summary
  playing / paused / idle / omitted-empty-fields. (Reuse the shared contract
  tests if the type is shared.)
- `MediaCommand` maps to the correct GSMTC method (table test).

The live GSMTC read/control is environmental and Windows-gated; not a CI unit
test — proven by running the compiled binary on Windows (read confirmed;
`play`/`pause` confirmed to flip `is_playing` after a short settle, polling).
This mirrors the macOS spec's env-gated live proof.

## Validation

Docs-only validation for this spec:

- `git diff --check`

Future implementation validation (behavior change):

- `cargo fmt --check`, `cargo check`, `cargo test`, `git diff --check`.
- Cross-compile / `cargo check --target x86_64-pc-windows-msvc` to prove the
  `cfg(windows)` gating and that the cross-platform contract logic still builds
  off-Windows (where `now_playing()` is `Unsupported`).
- CLI smoke checks on **both** front doors on a real Windows host
  (`auv-media-windows now-playing` and `auv-netease-music now-playing`,
  `--format summary` and `--format json`, `--help`), confirming the media-windows
  JSON carries the (always-null) like fields and the netease JSON omits them, and
  that `auv-media-windows play`/`pause` flip playback (polling to settle).

## Scope

Implemented (v0): the now-playing **read** (one-shot) and **transport + seek
controls** (play, pause, toggle, next, previous, seek) via GSMTC, exposed as
`MediaCommand` + `send_command`/`seek`, emitting `now-playing-v0`.

Explicitly **not** in scope:

- thumbnail/artwork bytes (GSMTC exposes `Thumbnail` as a stream; ignored, a
  possible follow-up — same posture as macOS `artworkData`);
- shuffle / repeat / playback-rate / record / channel controls (GSMTC supports
  them; deferred);
- multi-session enumeration / per-session targeting beyond current session +
  `--app-id` scoping (`GetSessions()` exists; deferred);
- change subscription / streaming (the `*Changed` events exist; deferred — v0 is
  one-shot poll, matching macOS);
- send-then-verify (controls are fire-and-forget; callers poll);
- a UIAccess/elevated worker or any service-context operation (explicitly out;
  interactive session only).

## Open Questions (owner decisions)

1. **Shared contract crate vs. duplication.** Factor `NowPlayingState` /
   `now-playing-v0` into a new `auv-media-core` leaf shared by both platform
   crates, or duplicate the small type in `auv-media-windows` for v0? (Bytes must
   match either way.) Recommendation: duplicate for the v0 slice to stay narrow;
   extract `auv-media-core` only when a third consumer appears.
2. **NetEase Windows scoping is unproven.** Whether the Windows NetEase client
   publishes to SMTC at all is unverified. If it does not, the netease delegation
   on Windows is a no-op and NetEase stays on the visual probe. Confirm on a real
   target before committing the netease front door on Windows.
3. **`source_bundle_id` naming.** Keep the macOS-shaped field name carrying the
   AUMID (cross-platform stability), or introduce a neutral `source_app_id` now?
   The Windows driver feasibility spec defers analogous renames; recommend
   deferring here too.
