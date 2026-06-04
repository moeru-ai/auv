# AUV macOS Now-Playing Capability (`auv-media-macos`) v0 Design

Date: 2026-06-04

Status: v0 design spec, updated to the as-built architecture. A new leaf crate
`auv-media-macos` reads the macOS system now-playing state via the vendored
`mediaremote-adapter` (built from source), driven through Apple's
`/usr/bin/perl`. The crate is **lib + binary**: it exposes the capability as a
library, owns the agent-facing `now-playing-v0` output contract, and ships an
`auv-media-macos` binary with `now-playing` (read) plus transport-control
subcommands. The existing `auv-netease-music` CLI gains a `now-playing`
subcommand that **delegates** to the crate, so both read front doors emit one
identical contract.

Audience: owner, reviewers, and any agent (Codex, Claude, others) implementing
or reviewing the now-playing capability.

## Purpose

An agent loop that drives a music app needs to read "what is playing right
now" without OCR-ing the player bar. Today AUV verifies playback only by
capturing and recognizing the on-screen now-playing region. That is visual,
app-specific, and brittle.

macOS aggregates now-playing state (title, artist, album, duration, elapsed
time, playback rate, owning app) for whatever app holds the system Now Playing
slot — the same data Control Center and the media keys use. This spec exposes
that state as a structured, agent-callable read.

## Why the obvious approach does not work (verified)

The only macOS API that returns *another* app's now-playing state is the
**private** `MediaRemote.framework`. The public `MPNowPlayingInfoCenter` only
exposes the calling process's own info.

Since **macOS 15.4**, MediaRemote now-playing reads are gated: per the
mediaremote-adapter project, only a process whose **bundle identifier starts
with `com.apple.`** is permitted to read it. This was verified empirically and
is the load-bearing fact of this design:

- A `dlopen` of MediaRemote from our **compiled, ad-hoc-signed binary** returns
  an **empty** dict (no error) on macOS 26.2.
- The same calls from `/usr/bin/swift` (identifier `com.apple.dt…`) or
  `/usr/bin/perl` (`com.apple.perl`) return full data.

> Lesson recorded: an early probe run as `swift probe.swift` returned data and
> produced a **false positive** — it executed inside the Apple-signed swift
> toolchain process. Feasibility for a shippable capability must be tested with
> a *compiled standalone binary*, never `swift`-script. We cannot sign our own
> binary as `com.apple.*`, so we must borrow an Apple platform binary as the
> vehicle.

The chosen vehicle is **`/usr/bin/perl`** (present on every macOS; no Swift
toolchain needed at runtime) driving the BSD-licensed **mediaremote-adapter**,
which loads a small `MediaRemoteAdapter.framework` and prints now-playing JSON.
Confirmed working on macOS 26.2 from a compiled binary at ~20 ms/read (warm).

## Source semantics (decided)

MediaRemote now-playing is **system-wide and app-agnostic**: it returns
whichever app currently owns the Now Playing slot (NetEase, Spotify, Music, a
browser tab — all identical). This capability does **not** filter to NetEase;
it reports whatever is playing and includes the owning app's
`source_bundle_id` so the caller can decide. The capability is therefore a
generic crate with its own `auv-media-macos` binary; the netease-music
`now-playing` subcommand is an additional convenience front door (that is the
existing agent-facing product CLI), not because the read is NetEase-specific.

## Crate placement and layout

One new workspace member: a **leaf, lib + binary, pure-Rust** crate. No
swift-bridge, no in-process FFI, no native static lib linked into the binary.

```text
crates/auv-media-macos/
  Cargo.toml               // one [[bin]]: auv-media-macos; deps: serde, serde_json, clap
  build.rs                 // cmake-builds the vendored framework, tars it into OUT_DIR
  vendor/
    mediaremote-adapter/   // git submodule, pinned to upstream release v0.7.6 (BSD-3)
  src/
    lib.rs                 // NowPlayingState, parse_get(), now_playing(), MediaCommand, send_command(), seek()
    adapter.rs             // (macOS) embed framework+script, unpack to cache, run perl get/send/seek
    output.rs              // now-playing-v0 contract type + JSON/human builders
    cli.rs                 // subcommands (now-playing + transport/seek), run() -> ExitCode
    error.rs               // MediaError
    bin/
      auv-media-macos.rs   // thin main -> auv_media_macos::cli::run()
```

Build-time (`build.rs`, macOS only): runs `cmake` to build
`MediaRemoteAdapter.framework` from the submodule, then `tar`s the built bundle
into `OUT_DIR`. Off-macOS, `build.rs` is a no-op and the adapter module is not
compiled.

Runtime (`adapter.rs`, macOS only): the built framework tar is embedded via
`include_bytes!` and the perl driver via `include_str!`. On first use they are
unpacked to a content-keyed cache
(`~/Library/Caches/auv/mediaremote-adapter/<hash>/`, atomic rename), then the
read runs `/usr/bin/perl <script> <framework> get`. The binary is therefore
self-contained: it needs only stock `/usr/bin/perl`, no external file layout.

`Cargo.toml` depends only on `serde` + `serde_json` + `clap`. It does **not**
depend on `auv-driver-macos`, `auv-cli`, or `auv-netease-music` (leaf crate).
Registered in the root `Cargo.toml` `[workspace].members`.

Fresh checkouts must run `git submodule update --init --recursive`; `build.rs`
panics with that exact hint if the submodule is missing.

### Why this structure (recorded decisions)

- **Build adapter from source** (submodule + cmake), not a committed binary
  blob: reproducible, auditable, multi-arch, matches the Rust `-sys`-crate
  convention. The repo already needs a native toolchain (`auv-driver-macos`
  shells `swiftc`), so cmake is a peer ask, not a new burden.
- **Embed + unpack** (vs sibling files): yields a single self-contained binary
  that runs from anywhere and survives being moved.
- **Pin to a release tag** (`v0.7.6`): submodules pin a commit by construction;
  we pin the commit of an upstream *release* rather than a floating `main`.

## Capability API

```rust
pub struct NowPlayingState {
  pub present: bool,                 // an app owns the slot with valid content
  pub source_bundle_id: Option<String>,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,
  pub elapsed_seconds: Option<f64>,
  pub playback_rate: Option<f64>,
  pub is_playing: bool,              // from the adapter's `playing` flag
  pub content_item_id: Option<String>,
  pub supports_like: Option<bool>,   // app exposes a like/favorite affordance
  pub is_liked: Option<bool>,        // current like state (None if unreported)
}

pub fn now_playing() -> Result<NowPlayingState, MediaError>;
```

- `now_playing()` (macOS) runs the adapter `get` and feeds its JSON to a pure
  `parse_get(&str) -> Result<NowPlayingState, MediaError>`. The adapter emits
  the literal `null` when nothing valid is playing (→ idle `NowPlayingState`);
  otherwise an object whose mandatory keys are `bundleIdentifier`, `playing`,
  `title`. `artworkData` and other keys are intentionally ignored.
- **Non-macOS:** `now_playing()` returns `MediaError::Unsupported`.

Transport controls (system-wide, app-agnostic — they act on whichever app owns
the slot) run the adapter `send <MRCommand-id>` / `seek <microseconds>`:

```rust
pub enum MediaCommand { Play, Pause, TogglePlayPause, NextTrack, PreviousTrack }
pub fn send_command(command: MediaCommand) -> Result<(), MediaError>;
pub fn seek(position: std::time::Duration) -> Result<(), MediaError>;
```

`MediaCommand` maps to the MRCommand ids in
`vendor/mediaremote-adapter/include/MediaRemoteAdapter.h` (Play=0, Pause=1,
TogglePlayPause=2, NextTrack=4, PreviousTrack=5). Controls return a plain
`Result<(), MediaError>` — **not** a new action-result schema. Fire-and-forget:
a successful `send` does not re-read to verify. Note a ~100 ms async settle
between a `send` and the read reflecting it, so a verifier must **poll**
`now_playing()`, not read once.

The crate also owns the agent-facing contract and the binary entry:

```rust
// output.rs
pub const SCHEMA_VERSION: &str = "now-playing-v0";
pub struct NowPlayingOutput { /* schema_version + flattened NowPlayingState */ }
pub fn build_now_playing_output(state: &NowPlayingState) -> NowPlayingOutput;
pub fn render_human_summary(state: &NowPlayingState) -> String;

// cli.rs
pub fn run() -> std::process::ExitCode;
```

## CLI surface (two front doors, one contract)

The crate binary `auv-media-macos` is subcommand-structured (read + controls);
the netease subcommand and the binary's `now-playing` subcommand emit the
identical `now-playing-v0` contract built in `auv-media-macos::output`.

```text
# the crate's own binary (read + transport controls)
auv-media-macos now-playing [--format summary|json] [--json-out <path>]
auv-media-macos play | pause | toggle | next | previous
auv-media-macos seek <seconds>

# the netease-music subcommands (delegate to the crate, scoped to one app)
auv-netease-music now-playing [--format summary|json] [--json-out <path>] [--app-id <bundle>]   (auv-wyy = identical)
auv-netease-music play | pause | toggle | next | previous [--app-id <bundle>]
auv-netease-music seek <seconds> [--app-id <bundle>]
```

`--format` (default `summary`) selects the stdout rendering; `--json-out <path>`
writes the JSON object to a file (and takes precedence over `--format`).

**Contract divergence (intentional).** The two now-playing JSON outputs are *not*
byte-identical: `auv-media-macos` includes the like fields
(`supports_like` / `is_liked`), while `auv-netease-music` **omits** them — NetEase
never reports like, so they would always be null there. The netease CLI builds
its own subset output (`auv_netease_music::output::NowPlayingOutput`) rather than
reusing the crate's. Both carry `schema_version: "now-playing-v0"`; the like
fields are treated as optional extensions present only in the generic surface.

**Scoping (the key difference between the two front doors).** `auv-media-macos`
is app-agnostic — it reads/controls whatever owns the system slot.
`auv-netease-music` is **scoped to a single app** via `--app-id`, defaulting to
`com.netease.163music` (the crate's `DEFAULT_APP_ID`):

- `now-playing` reports the track only when the scoped app owns the slot; when
  another app (e.g. Chrome) owns it, it reports the **idle** state
  (`NowPlayingState::default()`, exposed via `#[derive(Default)]`) — `Nothing
  playing`, exit `0`.
- transport/seek **refuse to act** unless the scoped app owns the slot: they
  read now-playing first, and if the owner differs they print
  `skipped: <app-id> is not the current now-playing app (current: <other>)` and
  exit non-zero — so `auv-netease-music pause` never pauses some unrelated
  browser tab. On a match they print `ok: <command>`, exit `0`.

This makes `auv-netease-music` honestly NetEase-scoped while the generic
capability stays in `auv-media-macos`. (There is a small TOCTOU window between
the ownership check and the send; acceptable for this use.)

The netease subcommand calls `auv_media_macos::now_playing()` then the crate's
`build_now_playing_output` / `render_human_summary` — it does **not** reshape
the contract. On non-macOS it prints "only available on macOS" and exits
non-zero. `auv-netease-music` depends on `auv-media-macos` as a
`cfg(target_os = "macos")` dependency.

### Human output (default)

- Playing: `▶ <title> — <artist> [<album>]  (<source_bundle_id>)`
- Paused:  `⏸ <title> — <artist> [<album>]  (<source_bundle_id>)`
- Idle:    `Nothing playing`

(Absent / empty optional fields are omitted, not printed as empty brackets.)

## Output contract (agent-facing)

- `--format json` (or `--json-out <path>` to a file) produces a stable object
  carrying `schema_version: "now-playing-v0"` plus the `NowPlayingState` fields.
  `auv-media-macos::output` owns the full contract (with like fields);
  `auv-netease-music::output` owns its like-less subset (see Contract divergence
  above).
- Exit codes:
  - `0` — the read completed, **including the nothing-playing case**
    (`present: false`). "Nothing playing" is state, not an error — consistent
    with the `playlist` contract.
  - non-zero — adapter/perl failure (perl missing, adapter non-zero exit,
    malformed JSON) or a non-macOS `Unsupported` result.
- An agent distinguishes "paused" from "idle" via `is_playing` + `present`, and
  the source app via `source_bundle_id` (it does not infer the app from the
  track text).

## Testing

Pure-Rust unit tests (no live media, no perl required):

- `parse_get`: `null` → idle; mapped object; paused (present, not playing);
  garbage → error; like fields mapped when present and `None` when absent
  (6 tests).
- `output`: `now-playing-v0` JSON carries schema version + fields; human
  summary playing / paused / idle / omitted-empty-fields / liked-`♥` (6 tests).
- `MediaCommand::command_id` maps to the adapter's MRCommand id table (1 test).

The live adapter read/control is environmental and macOS-gated; not a CI unit
test — proven by running the compiled binary (read confirmed; `play`/`pause`
confirmed to flip `is_playing` after a ~100 ms settle). This mirrors how
existing live-driver procedures are gated while their pure logic is unit-tested.

## Validation

Behavior change, so on completion run: `cargo fmt --check`, `cargo check`,
`cargo test`, `git diff --check`, plus CLI smoke checks on **both** front doors
(`auv-media-macos now-playing` and `auv-netease-music now-playing`, `--format
summary` and `--format json`, `--help` listing subcommands) — confirming the
media-macos JSON carries the like fields and the netease JSON omits them, and
that `auv-media-macos play`/`pause` flip playback (polling to settle).

## Scope

Implemented: the now-playing **read** (one-shot) and **transport + seek
controls** (play, pause, toggle, next, previous, seek) — the latter via the
adapter's `send`/`seek`, exposed as `MediaCommand` + `send_command`/`seek`. This
is the "media subsystem" the standalone crate was positioned to seed.

Explicitly **not** in scope:

- artwork bytes (the adapter emits `artworkData`; we ignore it — a suppress-
  artwork flag to keep the pipe small is a possible follow-up);
- shuffle / repeat / speed controls (the adapter supports them; deferred);
- NetEase-specific filtering (source is reported, not gated);
- live-position extrapolation;
- change subscription / streaming (the adapter's `stream` exists; deferred);
- send-then-verify (controls are fire-and-forget; callers poll `now_playing()`
  if they need confirmation, accounting for the ~100 ms settle).

## Finding: like/favorite is not available for NetEase via MediaRemote

`supports_like` / `is_liked` are surfaced from the adapter's `supportsIsLiked` /
`isLiked`. Verified empirically (NetEase playing on macOS 26.2): the adapter's
raw `get` for `com.netease.163music` contains **no** like/ban/wishlist keys at
all — NetEase does not integrate with MediaRemote's like affordance. So a
favorite *control* via MediaRemote is a dead end for NetEase specifically
(`kMRLikeTrack` isn't in the adapter allowlist anyway, and upstream's TODO notes
it would need track/station identifiers). The like-state fields remain valid for
apps that do report them (e.g. Apple Music). NetEase's 红心 would require the
separate UI-automation seam (clicking the heart via AX), not this crate.

## Risks

- **Private framework via a borrowed vehicle.** The read depends on
  `/usr/bin/perl` remaining an Apple platform binary permitted to read
  MediaRemote, and on the private framework's behavior. Apple could close this
  (as it closed direct in-process access in 15.4). Any break surfaces as a
  `MediaError` (non-zero exit), never a silent wrong answer; the existing
  player-bar OCR path remains the durable fallback.
- **Vendored third-party dependency.** mediaremote-adapter (BSD-3) is pinned as
  a submodule at `v0.7.6`; bumping it is a manual, reviewable step. Its LICENSE
  is retained with the vendored source.
- **Build/runtime prerequisites.** Build needs `cmake` + the initialized
  submodule (clear panic otherwise). Runtime needs `/usr/bin/perl` (stock on
  macOS).
