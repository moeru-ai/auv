# AUV macOS Now-Playing Capability (`auv-media-macos`) v0 Design

Date: 2026-06-04

Status: v0 design spec. Pins a new leaf crate `auv-media-macos` that reads
system now-playing via the macOS private MediaRemote framework. The crate is
**lib + binary**: it exposes the capability as a library, owns the agent-facing
`now-playing-v0` output contract, and ships its own thin `auv-now-playing`
binary. The existing `auv-netease-music` CLI gains a `now-playing` subcommand
that **delegates** to the crate, so both front doors emit one identical
contract.

Audience: owner, reviewers, and any agent (Codex, Claude, others) implementing
or reviewing the now-playing capability.

## Purpose

An agent loop that drives a music app needs to read "what is playing right
now" without OCR-ing the player bar. Today AUV verifies playback only by
capturing and recognizing the on-screen now-playing region. That is visual,
app-specific, and brittle.

macOS already aggregates now-playing state (title, artist, album, duration,
elapsed time, playback rate, owning app) for whatever app holds the system
Now Playing slot — the same data Control Center and the media keys use. This
spec exposes that state as a structured, agent-callable read.

## Feasibility (verified, not assumed)

The only macOS API that returns *another* app's now-playing state is the
**private** `MediaRemote.framework`. The public `MPNowPlayingInfoCenter`
exposes only the calling process's own now-playing info and cannot read a
third-party player.

Apple restricted MediaRemote now-playing reads in macOS 15.4+. Feasibility on
the current target (macOS 26.2) was therefore verified empirically with two
probes before this design was accepted:

- `MRMediaRemoteGetNowPlayingInfo` resolved and its callback fired with 13
  populated keys (title/artist/album/duration/elapsed/rate/timestamp/artwork
  metadata/content id).
- `MRMediaRemoteGetNowPlayingApplicationPID` resolved and, via
  `NSRunningApplication(processIdentifier:)`, returned the owning app's bundle
  identifier (`com.google.Chrome` at probe time).

Both worked from an ad-hoc, unentitled binary. This is a private-framework
dependency and is inherently fragile across OS releases; that risk is recorded
under Risks below.

## Source semantics (decided)

MediaRemote now-playing is **system-wide and app-agnostic**: it returns
whichever app currently owns the Now Playing slot (NetEase, Spotify, Music,
a browser tab — all identical). This capability does **not** filter to NetEase.
It reports whatever is playing and includes the owning app's bundle identifier
as a field so the caller can decide. The capability is therefore a generic crate
(`auv-media-macos`) with its own `auv-now-playing` binary; the netease-music
`now-playing` subcommand is an additional convenience front door (that is the
existing agent-facing product CLI), not because the read is NetEase-specific.

## Crate placement and layout

One new workspace member: a **leaf macOS capability crate**, not a module
inside `auv-netease-music` and not part of `auv-driver-macos`.

```text
crates/auv-media-macos/
  Cargo.toml               // one [[bin]] target: auv-now-playing
  build.rs                 // swift_bridge_build::parse_bridges + compile swift static lib
  native/
    swift/
      Package.swift
      Sources/AuvMediaNative/
        NowPlaying.swift    // dlopen MediaRemote: GetNowPlayingInfo + GetNowPlayingApplicationPID
  src/
    lib.rs                  // pub fn now_playing() -> Result<NowPlayingState, MediaError>; pub types
    ffi.rs                  // #[swift_bridge::bridge] flat result struct
    output.rs               // now-playing-v0 contract type + JSON/human builders (crate-owned)
    cli.rs                  // argv -> OutputMode, run() -> ExitCode (shared by the binary)
    bin/
      auv-now-playing.rs    // thin main -> auv_media_macos::cli::run()
```

The crate mirrors `auv-driver-macos`'s native harness (`swift-bridge` +
`swift-bridge-build`, a `build.rs` that compiles the Swift static library, and
a bridged result struct). This duplicates that harness deliberately: the owner
chose a standalone crate so this can seed a future macOS media subsystem rather
than live inside the input driver. The harness-duplication cost is accepted and
recorded under Risks.

Because the crate ships a binary and owns the agent-facing contract, the
`now-playing-v0` output object and its builders live **here** (`output.rs`),
not in `auv-netease-music`. This gives both front doors (the `auv-now-playing`
binary and the netease `now-playing` subcommand) a single contract definition,
avoiding two now-playing JSON shapings that could drift.

`Cargo.toml` depends on:

- `swift-bridge` + `swift-bridge-build` (workspace)
- `serde` + `serde_json` (the crate now emits the JSON contract)
- a CLI arg parser (`clap`, matching the repo's existing CLI style) for the
  binary's flags
- workspace standard dependencies

It does **not** depend on `auv-driver-macos`, `auv-cli`, or `auv-netease-music`
(leaf crate; depends only down/out of tree).

Registration: add `crates/auv-media-macos` to the root `Cargo.toml`
`[workspace].members`.

## Capability API

```rust
pub struct NowPlayingState {
    pub present: bool,                    // an app currently owns the now-playing slot
    pub source_bundle_id: Option<String>,
    pub source_name: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_seconds: Option<f64>,
    pub elapsed_seconds: Option<f64>,
    pub playback_rate: Option<f64>,
    pub is_playing: bool,                 // derived: playback_rate.map_or(false, |r| r > 0.0)
    pub content_item_id: Option<String>,
}

pub fn now_playing() -> Result<NowPlayingState, MediaError>;
```

The crate also owns the agent-facing contract and the binary entry:

```rust
// output.rs — the stable, versioned contract (crate-owned, shared front doors)
pub struct NowPlayingOutput {
    pub schema_version: &'static str, // "now-playing-v0"
    // ... flattened NowPlayingState fields ...
}
pub fn build_now_playing_output(state: &NowPlayingState) -> NowPlayingOutput;
pub fn render_human_summary(state: &NowPlayingState) -> String;

// cli.rs — the binary's run(); reused by the `auv-now-playing` bin target
pub fn run() -> std::process::ExitCode;
```

- **Swift bridge:** `NowPlaying.swift` performs the two MediaRemote calls
  (`MRMediaRemoteGetNowPlayingInfo`, `MRMediaRemoteGetNowPlayingApplicationPID`),
  resolves the bundle id via `NSRunningApplication`, and returns a single flat
  `swift-bridge` result struct. Optional fields are carried as
  `Option<String>` / presence flags, following the `auv-driver-macos` OCR
  result-struct convention (including `error_message` / `recovery_hint`). A 3s
  timeout on either callback surfaces as a `MediaError`.
- **Mapping:** `ffi.rs`'s bridged struct maps into the public
  `NowPlayingState`; `is_playing` is derived in Rust.
- **Non-macOS builds:** `now_playing()` returns `MediaError::Unsupported`
  ("only available on macOS"), matching the existing platform-gating pattern.
  No live procedure is compiled off-macOS.

## CLI surface (two front doors, one contract)

Both surfaces parse the same flags and emit the identical `now-playing-v0`
contract built in `auv-media-macos::output`.

```text
# 1. the crate's own binary (the capability's front door)
auv-now-playing
  --json                 emit JSON to stdout (default: human summary)
  --json-out <path>      write JSON to a file

# 2. the netease-music subcommand (delegates to the crate)
auv-netease-music now-playing        (auv-wyy = identical binary)
  --json
  --json-out <path>
```

- **Crate binary:** `src/bin/auv-now-playing.rs` is a thin `main` delegating to
  `auv_media_macos::cli::run()`.
- **netease subcommand:** a new `Command::NowPlaying` / `CliSubcommand::NowPlaying`
  variant whose handler calls `auv_media_macos::now_playing()` then
  `build_now_playing_output` / `render_human_summary` — it does **not** reshape
  the contract. Both `auv-netease-music` `[[bin]]` targets get it for free
  (shared `cli::run()`). `auv-netease-music` adds `auv-media-macos` as a
  `cfg(target_os = "macos")` dependency.

### Human output (default)

- Playing: `▶ <title> — <artist> [<album>]  (<source_bundle_id>)`
- Paused:  `⏸ <title> — <artist> [<album>]  (<source_bundle_id>)`
- Idle:    `Nothing playing`

(Absent optional fields are omitted, not printed as empty brackets.)

## Output contract (agent-facing)

- `--json` / `--json-out` produce a stable object carrying
  `schema_version: "now-playing-v0"` plus the `NowPlayingState` fields. The
  JSON output object and its builder live in `auv-media-macos`'s `output.rs`
  (crate-owned), so the `auv-now-playing` binary and the netease `now-playing`
  subcommand emit byte-identical contracts. (This differs from `playlist`,
  whose output is shaped in `auv-netease-music`, because now-playing has two
  front doors that must not drift.)
- Exit codes:
  - `0` — the read completed, **including the nothing-playing case**
    (`present: false`). "Nothing playing" is state, not an error — consistent
    with the `playlist` contract where a zero-result scan still exits `0`.
  - non-zero — MediaRemote/FFI failure (framework not loadable, symbol missing,
    callback timeout) or a non-macOS `Unsupported` result.
- An agent distinguishes "paused" from "idle" by reading `is_playing` together
  with `present`; it distinguishes "NetEase is playing" from "something else is
  playing" by reading `source_bundle_id`. It does not infer the source app from
  the track text.

## Testing

Pure-Rust unit tests (no live media required):

- bridged FFI struct → `NowPlayingState` mapping (all fields, optional-absence).
- `is_playing` derivation from `playback_rate` (None, 0.0, 1.0).
- `present: false` (idle) path.
- `auv-media-macos::output` builders: human summary (playing / paused / idle)
  and JSON (`schema_version` + fields) snapshots — these live in the crate
  (where the contract is owned), not in netease-music.
- `auv-media-macos::cli` arg parsing (`--json` / `--json-out` / none).

The live MediaRemote read is environmental (depends on what is playing) and is
macOS-gated; it is not a CI unit test. Its mechanism is already proven by the
two feasibility probes. This mirrors how existing live-driver procedures are
gated while their pure logic is unit-tested.

## Validation

Behavior change, so on completion run: `cargo fmt --check`, `cargo check`,
`cargo test`, `git diff --check`, plus CLI smoke checks on **both** front doors:
`auv-now-playing` (human + `--json`) and `auv-netease-music now-playing`
(human + `--json`, and `--help` listing it). Confirm the two emit identical
`now-playing-v0` JSON.

## Non-goals (v0)

Read-only, one-shot. Explicitly **not** in v0:

- transport commands (play / pause / next / seek) — the future media-control
  subsystem this crate seeds;
- artwork bytes in output (the artwork keys exist but are excluded; metadata or
  a dump flag can be added later);
- NetEase-specific filtering (source is reported, not gated);
- live-position extrapolation from `elapsed + timestamp + rate`;
- change subscription / streaming now-playing updates.

## Risks

- **Private framework.** MediaRemote is undocumented and Apple-restricted
  (locked down in 15.4+; verified working on 26.2 but not guaranteed across
  future releases). A break surfaces as a `MediaError` (non-zero exit), never a
  silent wrong answer. The existing player-bar OCR path remains the durable
  fallback for playback verification.
- **Harness duplication.** A standalone crate re-stands-up the `swift-bridge`
  build harness already present in `auv-driver-macos` for a single current
  consumer. Accepted as the cost of seeding an independent media subsystem; if
  no second consumer or transport scope materializes, folding this into
  `auv-driver-macos::observe` later is a cheap mechanical lift.
```
