# Wayland capture lifecycle evidence from OBS and Sunshine

Date: 2026-08-07

This note compares AUV's current xdg-desktop-portal/PipeWire capture path with
OBS Studio commit
[`88de106c`](https://github.com/obsproject/obs-studio/tree/88de106cff4bcdf2a511e2e3801ffa3de729f6bd)
and Sunshine commit
[`0784774`](https://github.com/LizardByte/Sunshine/tree/0784774fecb4ffcd7ff1bf1c26bba84af516590e).
Only upstream specifications and first-party source repositories are used.

## Finding

The live-capture stall is not explained by window/output resolution alone.
AUV does reuse the portal `ScreenCast` session, but it recreates the entire
PipeWire data plane for every requested frame. OBS and Sunshine open the
PipeWire remote once, keep a PipeWire core, stream, and event loop alive, and
consume frames from that long-lived stream.

The immediate AUV fix should therefore make the PipeWire receiver a durable
part of `ScreenCastSession`, with a bounded wait for a recent frame. It should
also bound and cancel portal requests. Caching only Wayland output geometry
would remove two compositor roundtrips, but would not fix repeated PipeWire
negotiation or an unbounded Screenshot fallback.

## Portal contract

The `ScreenCast` lifecycle is `CreateSession -> SelectSources -> Start ->
OpenPipeWireRemote`. `Start` returns one or more PipeWire streams, and
`OpenPipeWireRemote` returns an fd that is used to create a `pw_core` with
`pw_context_connect_fd`. The specification does not require clients to reopen
that remote for each frame. A session remains open until the client closes it
or the portal emits `Session.Closed`.
([ScreenCast specification](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html))

Restore tokens persist permission/source selection, not an active PipeWire
connection. They are single-use: after a token is consumed, the client must
store the replacement returned by the next successful `Start`. If restoration
is impossible, the token is ignored and normal user selection may be shown.
([`SelectSources` restore-token contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html#org.freedesktop.portal.ScreenCast.SelectSources))

Since ScreenCast version 6, the tuple's numeric PipeWire node ID is deprecated
for targeting because it can be reused after node destruction. A client should
prefer the returned `pipewire-serial` and set `PW_KEY_TARGET_OBJECT` when that
property is available.
([`Start` stream contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html#org.freedesktop.portal.ScreenCast.Start))

The portal supports monitor, window, and virtual-monitor source categories,
but `SelectSources` does not let a client identify an arbitrary application
window by title or PID. In practice the user selects the window, or a later
session restores the portal's opaque selection. The absence of a programmatic
name/process selector is tracked by the portal project as a proposed new API,
not as existing behavior.
([available source types](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html#org.freedesktop.portal.ScreenCast.AvailableSourceTypes),
[`xdg-desktop-portal` issue #1064](https://github.com/flatpak/xdg-desktop-portal/issues/1064))

Consequently, AUV cannot portably ask the portal for “the Balatro window” by
its AT-SPI/Wayland title. It can either:

- keep one user-approved monitor stream and crop it using separately observed
  window geometry; or
- ask the user to select a window once and restore that opaque portal choice
  later, without claiming that the token is a stable title/PID selector.

## OBS Studio

OBS creates a portal capture object that owns the session handle, restore
token, PipeWire connection, and PipeWire stream. After `Start`, it invokes
`OpenPipeWireRemote` once, connects the returned fd to PipeWire, and stores the
resulting `obs_pw` and `obs_pw_stream` in that capture object.
([portal capture fields and one-time open](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L44-L60),
[`OpenPipeWireRemote` callback](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L154-L210),
[`Start` response](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L214-L270))

The PipeWire connection owns a `pw_thread_loop`, context, core, and registered
stream. The stream remains connected while the OBS source exists; show/hide
only activates or deactivates it. OBS destroys the stream, PipeWire connection,
and portal session when the source is destroyed or the user explicitly reloads
the selection.
([PipeWire connection lifetime](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/pipewire.c#L1116-L1160),
[`pw_stream` connection](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/pipewire.c#L1192-L1254),
[source destruction](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L556-L578))

Frame processing drains queued buffers to the newest one rather than rebuilding
the stream for a snapshot. This is the useful architectural precedent for an
AUV latest-frame cache.
([OBS PipeWire frame processing](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/pipewire.c#L661-L709))

OBS requests persistence when ScreenCast version 4 is available, supplies the
previous token, and saves the replacement from `Start` into source settings.
([selection persistence](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L345-L382),
[replacement-token handling](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L253-L265),
[settings persistence](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L580-L590))

OBS's portal method calls use GLib's default/infinite timeout, so OBS is not a
good timeout value to copy. It does, however, attach a `GCancellable` to the
asynchronous calls. Cancelling also closes the portal `Request` object and
unsubscribes the pending response handler.
([cancellable portal request handling](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/portal.c#L129-L189),
[asynchronous call sites](https://github.com/obsproject/obs-studio/blob/88de106cff4bcdf2a511e2e3801ffa3de729f6bd/plugins/linux-pipewire/screencast-portal.c#L287-L304))

## Sunshine

Sunshine likewise runs portal setup once during display initialization:
`connect_to_portal` creates/starts the session and calls
`OpenPipeWireRemote`; `configure_stream` returns that fd and the chosen stream
identity to the owning `pipewire_display_t`.
([portal setup](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L228-L256),
[`OpenPipeWireRemote`](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L633-L648),
[display stream configuration](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L708-L758))

Its `pipewire_t` owns a threaded loop, context, core, stream, buffer state,
mutex, and condition variable for the lifetime of the display capture object.
The process callback retains the newest DMA-BUF or swaps a double-buffered CPU
copy, then wakes consumers. `snapshot` waits for a frame on that already-active
stream; the continuous capture loop gives each snapshot a 1000 ms deadline.
([persistent PipeWire owner](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/pipewire.cpp#L77-L114),
[thread-loop lifetime](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/pipewire.cpp#L138-L192),
[frame handoff](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/pipewire.cpp#L570-L620),
[bounded snapshot and capture loop](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/pipewire.cpp#L893-L921))

Sunshine also bounds initial format negotiation to 1500 ms and returns a typed
timeout/reinitialize/error outcome when frames stop or the stream dies.
([negotiation deadline](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/pipewire.cpp#L846-L881),
[capture outcomes](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/pipewire.cpp#L955-L1019),
[closed-session handling](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L760-L782))

Sunshine persists the restore token on disk, passes it to a persistent
ScreenCast-only selection, and replaces it with the next token returned by
`Start`.
([token store](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L38-L105),
[selection](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L469-L490),
[rotation after `Start`](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L522-L579))

Sunshine currently requests monitor sources only and matches returned portal
streams to Wayland monitor geometry/name. It does not demonstrate
programmatic portal window selection.
([monitor-only `SelectSources`](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L469-L489),
[stream-to-monitor matching](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L581-L628))

One Sunshine behavior should not be copied: its portal setup uses synchronous
D-Bus calls with an infinite timeout and waits in `g_main_loop_run` without an
explicit deadline or cancellable. Its bounded waits begin only after PipeWire
stream setup.
([unbounded response wait](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L694-L702),
[synchronous `Start`](https://github.com/LizardByte/Sunshine/blob/0784774fecb4ffcd7ff1bf1c26bba84af516590e/src/platform/linux/portalgrab.cpp#L522-L555))

## Current AUV delta

Current AUV behavior is split across
[`native/portal/screencast.rs`](../../../../crates/auv-driver-linux/src/native/portal/screencast.rs),
[`native/portal/request.rs`](../../../../crates/auv-driver-linux/src/native/portal/request.rs),
[`capture.rs`](../../../../crates/auv-driver-linux/src/capture.rs), and
[`capture/display.rs`](../../../../crates/auv-driver-linux/src/capture/display.rs).

| Concern | AUV now | OBS/Sunshine evidence | Required correction |
| --- | --- | --- | --- |
| Portal session | Cached in `LinuxDriverSessionState` | Long-lived | Keep this behavior |
| Restore token | Atomically rotates a single-use token | Both replace the consumed token | Keep this behavior |
| PipeWire remote | Reopened in every `capture_monitor_frame` | Opened once during source/display initialization | Store one remote/core per active session |
| PipeWire loop and stream | Created inside `read_pipewire_frame`, destroyed after first frame | Persistent threaded loop and stream | Move ownership into `ScreenCastSession` or a dedicated deep receiver module |
| Frame delivery | Waits for the first frame after each new negotiation | Callback maintains newest frame/buffer | Maintain latest frame plus sequence/freshness and wait only when no acceptable frame exists |
| PipeWire timeout | Five seconds per newly-created stream | Sunshine uses bounded negotiation and per-frame waits | Preserve a bounded wait, but apply it to an existing receiver and return a typed timeout/stall reason |
| Portal request timeout | `responses.next()` can block indefinitely | OBS is cancellable; Sunshine is also unbounded here | Add deadline/cancellation and call `Request.Close` on timeout |
| Screenshot fallback | Interactive fallback also waits indefinitely | Neither project uses a one-shot interactive screenshot as the normal frame path | Do not enter an unbounded fallback after a bounded stream failure |
| Stream targeting | Parses `pipewire-serial` but connects by numeric node ID | Portal v6 and Sunshine prefer object serial | Use `PW_KEY_TARGET_OBJECT` when available; retain node ID only as versioned compatibility |
| Output discovery | Opens a Wayland connection and performs two roundtrips for every display/region capture | Sunshine correlates monitor metadata during stream setup | Cache output metadata with an explicit invalidation/re-enumeration trigger |
| Window selection | Always requests monitor streams and crops | Portal cannot select by title/PID; OBS window source is user-selected/restored | Keep monitor crop as the automatic path; expose portal-window selection only with its consent/opaque-token boundary |

The observed long wall time can therefore be a composition of:

1. fresh Wayland output enumeration (two roundtrips);
2. a portal session start on the first call;
3. `OpenPipeWireRemote`, core/stream creation, and format negotiation on every
   call;
4. up to five seconds waiting for the first frame;
5. an unbounded interactive Screenshot request after the ScreenCast path
   fails.

The last two stages explain why a caller can exceed the PipeWire five-second
deadline. A trace that reports only the enclosing `window.capture` operation
cannot attribute that delay to “window resolution” safely.

## Recommended narrow fix

1. Make a started `ScreenCastSession` own the opened PipeWire fd/core, one
   connected receiver per selected stream, and the event-loop thread.
2. Have the receiver publish the latest decoded frame with a monotonically
   increasing sequence and arrival time. `capture_monitor_frame` selects the
   receiver and returns its latest acceptable frame, with a bounded wait only
   when necessary.
3. Observe PipeWire stream errors and portal `Session.Closed`; invalidate the
   cached session and allow one explicit reinitialization on the next capture.
4. Add deadlines to all portal response waits. On timeout, close the portal
   `Request`; do not immediately enter another unbounded interactive portal
   operation.
5. Do not hold the outer `LinuxDriverSessionState` mutex while waiting for a
   frame. The durable receiver should own its internal synchronization.
6. Cache Wayland output metadata separately. Treat hotplug/output change as an
   invalidation trigger rather than paying two roundtrips on every frame.

This slice should remain a CPU-frame correctness and lifecycle fix. DMA-BUF
zero-copy and direct GPU inference are valuable follow-ups, but neither is
required to remove the current setup and blocking costs.

## Implemented slice and live evidence

The 2026-08-07 fix makes `ScreenCastSession` own a receiver pool keyed by
portal stream ID. Each receiver owns a dedicated PipeWire worker thread and
keeps its main loop, context, core, listener, and stream connected. A failed
receiver is removed so the next capture can initialize it again.

The worker drains buffers while idle without converting the full display. On
capture it waits up to 100 ms for a newer buffer; if a static Wayland surface
produces no damage, it returns the latest decoded frame instead of waiting five
seconds and entering the Screenshot fallback. Focused regression tests cover
receiver reuse, invalidation after failure, and static-stream latest-frame
delivery.

Portal method calls and `Request.Response` waits now each have a ten-second
deadline. The Screenshot fallback remains available, but if it also fails the
returned error preserves both the primary ScreenCast failure and fallback
failure. No-reply `Request.Close`, output hotplug caching, moving frame waits
outside the outer driver mutex, and serial-based PipeWire targeting remain
explicitly deferred at their call sites.

Live probing on `neko-gpu-1` separated the phases:

- A successful authorized first `display.capture` completed in 2.65 seconds,
  including CLI/run recording, PNG encoding, and artifact persistence.
- An intermediate persistent-stream build reproduced the static-frame bug:
  capture 1 completed in 2.65 seconds, while capture 2 reached the caller's
  20-second timeout after the five-second fresh-frame wait entered the
  interactive Screenshot fallback. This motivated the latest-frame policy and
  its regression test.
- After the GNOME portal session was restarted, the stored restore token no
  longer completed unattended `Start`. Stage probes measured CreateSession and
  SelectSources method/response pairs below 2 ms each, followed by the full
  ten-second wait for `ScreenCast.Start` response. One interactive source
  authorization produced a replacement restore token; the next daemon restart
  then restored capture without another prompt.
- After authorization, ten complete remote `display.capture` CLI invocations
  measured 336 ms for the first sample and 86--132 ms for the next nine
  samples. The hot mean was 112 ms. These measurements include daemon routing,
  gRPC image transfer, 2560x1440 PNG encoding, run recording, and artifact
  persistence, not only the PipeWire snapshot.
- A separate Balatro probe showed that an unsuccessful AT-SPI window lookup
  still cost 2.44 seconds per observation on this GNOME Wayland/Wine desktop.
  Fixed-image inference through the persistent seven-model Runner averaged
  1.37 seconds, while the live path averaged 3.81 seconds. Linux Balatro now
  skips the unavailable accessibility-window lookup and uses the persistent
  display stream directly; eight live samples then averaged 1.40 seconds
  (1.12--1.52 seconds).

The accuracy samples from that final timing run are not model-quality
evidence: Steam was in front of Balatro in the captured desktop. They confirm
the capture and inference execution paths only.

A corrected foreground probe activated the existing Proton/XWayland Balatro
window and verified the result with a fresh Wayland PipeWire screenshot before
measurement. The frame contained eight visible hand cards. Twenty consecutive
CUDA observations returned the same eight readings (`S_K`, `C_Q`, `S_T`,
`S_8`, `D_8`, `S_7`, `H_4`, `D_2`) with no missing slots; the first observation
reported identity confidences from 0.973 to 0.999. This is single-frame
repeatability evidence, not a dataset-level accuracy rate.

The Cargo feature graph confirmed that `auv-game-balatro --features cuda`
enabled CUDA in `auv-inference-ultralytics`, `ultralytics-inference`, `ort`, and
`ort-sys`. The initial CPU fallback happened because the daemon-launched Runner
could not resolve the CUDA 12 runtime libraries required by ONNX Runtime;
`nvidia-smi` alone provides a working driver, not those user-space libraries.
After the Balatro Runner provider supplied the installed CUDA 12 library paths,
`nvidia-smi` reported `auv-runner-balatro` using 1416 MiB of GPU memory.

On that foreground frame, CUDA cold start took 2.68 seconds. Twenty warm live
observations averaged 1.413 seconds (1.260--1.567 seconds), versus 1.556 seconds
for six warm CPU observations. Fixed-image inference averaged 1.340 seconds on
CUDA and 1.453 seconds on CPU; ten complete display captures averaged 111 ms.
CUDA was therefore active but improved this seven-small-model pipeline by only
about nine percent. The remaining cost is inside inference and its CPU
pre/post-processing, image copies, and multi-session scheduling rather than
window lookup or OCR.
