# OBS platform capture backends and AUV implications

Date: 2026-08-05

## Question

Which APIs does OBS Studio use for continuous screen, window, application, and
game capture on macOS, Windows, and Linux, and which of those approaches are
suitable for an AUV recording-first capture path?

This is research, not an approved implementation design. It records the
backend boundaries that a later owner-approved capture slice should preserve.

## Evidence snapshot

The source observations below use OBS Studio commit
[`0052d024`](https://github.com/obsproject/obs-studio/tree/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc).
Platform behavior can also depend on the Windows version, Linux display server,
desktop portal implementation, compositor, graphics driver, and target app.

## Short answer

OBS does not use one cross-platform capture API.

| Platform and scope | OBS path | Frame transport | Background/occlusion boundary |
| --- | --- | --- | --- |
| macOS display | ScreenCaptureKit display filter; legacy CGDisplayStream remains registered | `CMSampleBuffer` -> IOSurface -> graphics texture | Captures the composed display, but selected applications or windows can be excluded |
| macOS window | ScreenCaptureKit desktop-independent window filter | `CMSampleBuffer` -> CVPixelBuffer-backed IOSurface -> graphics texture | Explicitly preserves complete content while covered, offscreen, or moving between displays; pauses while minimized |
| macOS application | ScreenCaptureKit display-dependent application filter | Same IOSurface path | Includes the selected app's windows on one selected display; it is not an independent all-spaces app surface |
| Windows window | Windows Graphics Capture (WGC), with BitBlt as a compatibility path | WGC emits D3D11 textures through a long-lived frame pool | Independent capture of a covered window is the expected behavior but still needs AUV evidence; OBS refuses to initialize WGC for an invisible or minimized HWND, so minimized capture is not a supported invariant |
| Windows display | DXGI Desktop Duplication or WGC | D3D11 textures | Captures the composed desktop, including occlusion and overlays |
| Windows game | Injected OBS graphics hook for D3D/OpenGL/Vulkan | Shared GPU texture when possible, shared memory fallback | Captures in the target process before normal desktop composition; invasive and materially different from window capture |
| Linux X11 window | XComposite named window pixmap imported as a GL texture | Server pixmap -> GL texture | Handles ordinary occlusion, but OBS requires a viewable window and explicitly expects failure while minimized or on an offscreen workspace |
| Linux X11 display | XShm | Shared-memory image uploaded into an OBS texture each tick | Captures the composed X11 root display |
| Linux Wayland window/display | xdg-desktop-portal ScreenCast + PipeWire | DMA-BUF import when negotiated, memory buffer otherwise | Selection and persistence are portal/compositor policy; exact arbitrary-window selection and minimized updates are not portable guarantees |

## macOS

### Primary source: ScreenCaptureKit

OBS registers its ScreenCaptureKit source on macOS 12.5 and later. On macOS
13 and later it marks the older display, window, and desktop-audio sources as
deprecated in
[`plugin-main.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/mac-capture/plugin-main.c#L15-L33).
On macOS 12.5, OBS's ScreenCaptureKit source only enables window capture;
display, application, and application-audio capture take the complete
ScreenCaptureKit path on macOS 13 and later. If AUV supports macOS 13+, it does
not need to reproduce this transition as a new compatibility layer.
The legacy sources remain relevant as compatibility evidence: display capture
uses a continuous `CGDisplayStream` whose frames are IOSurfaces, while window
capture repeatedly calls `CGWindowListCreateImage` and outputs copied BGRA CPU
frames. They are not the architecture to copy for a new recording-first path.

Apple describes ScreenCaptureKit as its high-performance API for streaming
displays, applications, and windows. An `SCStream` combines an
`SCContentFilter` with an `SCStreamConfiguration`, then delivers
`CMSampleBuffer` frames and metadata to an `SCStreamOutput`. Apple's
[macOS capture sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos)
shows that each screen sample wraps a `CVPixelBuffer` backed by an IOSurface.
Apple's [WWDC22 advanced session](https://developer.apple.com/videos/play/wwdc2022/10155/)
also describes GPU-backed capture buffers, hardware-accelerated scaling and
format conversion, and lower CPU overhead than the older capture APIs.

### OBS filter semantics

OBS exposes three ScreenCaptureKit capture types in
[`mac-sck-video-capture.m`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/mac-capture/mac-sck-video-capture.m#L74-L194):

- **Display** creates a display filter. It can exclude OBS itself to avoid a
  hall-of-mirrors result, so "display capture" and "literal final composition"
  still need the applied filter recorded in metadata.
- **Window** creates
  [`SCContentFilter(desktopIndependentWindow:)`](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init%28desktopindependentwindow%3A%29).
  OBS enables child-window inclusion on macOS 14.2 and later. This is an
  independent window surface, not a crop from a display frame.
- **Application** creates a display-dependent filter that includes the selected
  `SCRunningApplication`. It automatically includes that application's windows
  and new child or popup windows on the selected display. It is not equivalent
  to one desktop-independent window or to a platform-wide app framebuffer.

OBS queries `SCShareableContent` and optionally asks for offscreen/hidden
windows by setting `onScreenWindowsOnly` to false in
[`mac-sck-common.m`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/mac-capture/mac-sck-common.m#L55-L76).
That flag controls discovery. It must not be mistaken for proof that every
discovered window is actively producing content. Newer ScreenCaptureKit also
distinguishes an [`SCWindow` being active](https://developer.apple.com/documentation/screencapturekit/scwindow/isactive)
from being on screen: an active window can still be streaming while offscreen.

### Covered, offscreen, and minimized windows

Apple documents the strongest independent-window semantics among the three
platforms studied here. Its WWDC22 ScreenCaptureKit session demonstrates and
states that a desktop-independent single-window stream:

- contains the complete window even when partially or fully covered;
- continues when the window is moved fully offscreen or to another display;
- is independent of display and Space placement;
- pauses when the source window is minimized and resumes when restored.

This makes ScreenCaptureKit directly suitable for operating a background app
whose window remains active or is merely covered. It still does not turn a
minimized app into a continuously rendering surface. AUV should keep the last
frame available for inspection if policy allows, but must report the stream as
stalled/minimized and must not present the old frame as a fresh capture.

Current ScreenCaptureKit also exposes
[`ignoreGlobalClipSingleWindow`](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/ignoreglobalclipsinglewindow),
which includes content moved past a display's clipping bounds when enabled.
Its default is false and OBS does not set it. AUV should enable it when the
contract promises a complete offscreen independent window, then validate that
behavior across supported macOS versions rather than relying only on the older
WWDC demonstration.

The behavior is specific to a desktop-independent window filter. A
display-dependent application or window filter loses content when it leaves
the selected display. Protected content is another independent failure axis;
Apple notes that some apps, such as Apple TV, may prevent their windows from
being recorded in its
[screen-recording guidance](https://support.apple.com/en-us/102618).

### Frame lifecycle and latest-frame behavior

OBS configures a long-lived `SCStream`, registers a screen output, and starts
capture once. It currently sets the server-side queue depth to 8, throttles
`minimumFrameInterval` to the OBS output rate, selects a 10-bit RGB pixel
format and Display P3, and enables application audio on macOS 13 and later.
See
[`mac-sck-video-capture.m`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/mac-capture/mac-sck-video-capture.m#L197-L270).

For each screen callback, OBS retrieves the `CVPixelBuffer`'s IOSurface,
retains it as `current`, and releases the previous unconsumed surface. On the
OBS video tick it moves `current` to `prev` and creates or rebinds a graphics
texture directly from that IOSurface. This is a latest-frame handoff rather
than a queue of CPU images. See
[`mac-sck-common.m`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/mac-capture/mac-sck-common.m#L213-L316)
and
[`mac-sck-video-capture.m`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/mac-capture/mac-sck-video-capture.m#L320-L349).
OBS's Metal backend binds the IOSurface with
`MTLDevice.makeTexture(descriptor:iosurface:plane:)`, so this rebind creates a
texture view over the surface rather than copying full-frame pixels. See
[`MetalTexture.swift`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/libobs-metal/MetalTexture.swift#L52-L73).

Apple says the default queue depth is 3 and it must not exceed 8. More queued
surfaces may avoid producer stalls but consume more WindowServer memory and can
increase latency. The application must release surfaces promptly; holding too
many exhausts the server-side pool. AUV should therefore copy OBS's
latest-frame ownership discipline, not blindly copy its maximum queue depth.
The appropriate depth for UI automation and osu requires measurement. See
Apple's [`queueDepth` documentation](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/queuedepth)
and the WWDC22 session above.

ScreenCaptureKit attaches frame status, display time, dirty rectangles,
content rectangle, content scale, and scale factor to the sample buffer.
Apple's sample accepts only `SCFrameStatus.complete`; the status vocabulary also
includes started, idle, blank, suspended, and stopped. In particular,
[`idle`](https://developer.apple.com/documentation/screencapturekit/scframestatus/idle)
means the display did not change. These are better inputs for AUV's
`content_advancing` and freshness fields than inferring health only from callback
arrival.

For AUV, only a valid `.complete` sample should advance `FrameRef.sequence`.
An idle sample updates source health without inventing a new frame; blank,
suspended, and stopped states must remain typed outcomes. A stream restart,
filter change, or native-source replacement increments `source_epoch` so an
old frame reference cannot be interpreted against a new producer.

Apple supports applying a new configuration or content filter to a running
stream without rebuilding it through
[`SCStream.updateConfiguration` and `updateContentFilter`](https://developer.apple.com/documentation/screencapturekit/scstream).
That supports daemon-owned leases and retargeting, although a first AUV slice
may reasonably keep one stable source per stream and defer retargeting policy.

### Permission and current AUV delta

ScreenCaptureKit requires Screen & System Audio Recording authorization. Apple
recommends the system content-sharing picker for user selection in GUI apps;
direct shareable-content enumeration still crosses the system recording
permission boundary. Permission denial, revocation, user-stopped stream, no
capture source, and protected content should remain distinct typed outcomes.
See Apple's [ScreenCaptureKit overview](https://developer.apple.com/documentation/screencapturekit)
and [macOS permission guidance](https://support.apple.com/guide/mac-help/mchld6aa7d23/mac).
The long-lived local Driver Runner should own the stable process identity that
holds this TCC authorization and the `SCStream`; the daemon should own its
lifecycle and routing rather than making each short-lived invoke process the
capture owner.

AUV currently uses `SCScreenshotManager.captureSampleBuffer` for a synchronous
window snapshot on macOS 14+, then converts the sample buffer through
`CIImage -> CGImage -> RGBA bytes`; display and region capture use one-shot
`xcap`. The implementation is in
[`Capture.swift`](../../../../crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Capture.swift).
That remains useful for `--capture native`, but it repeats content discovery,
snapshot setup, and CPU materialization. A recording-frame path should instead
keep `SCStream` alive and retain an IOSurface-backed frame reference until a
consumer actually requests CPU pixels or an encoded artifact.

For YOLO, the preferred path is IOSurface/CVPixelBuffer to the selected GPU or
Core ML inference representation with at most a device-local format conversion.
The normal inference path should not make PNG or RGBA materialization
mandatory. The exact zero-copy claim depends on the chosen inference runtime
and must be demonstrated rather than inferred from IOSurface availability.
It also only applies inside a process that can resolve the surface lease. An
opaque `FrameRef` sent to another Runner is not automatically usable there;
cross-process IOSurface transfer is a separate data-plane contract. A first
slice should colocate the macOS source and inference consumer in the local
Driver Runner, or explicitly account for one bytes transport.

## Windows

### Ordinary window capture: WGC first, BitBlt compatibility path

OBS exposes `Auto`, `BitBlt`, and `Windows Graphics Capture` methods in
[`window-capture.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/win-capture/window-capture.c).
Its WGC implementation creates a `GraphicsCaptureItem` for an HWND, a
`Direct3D11CaptureFramePool` with two buffers, and a capture session. A
`FrameArrived` callback calls `TryGetNextFrame`, obtains the frame's
`ID3D11Texture2D`, and copies it into an OBS texture. It recreates the frame
pool on size changes. See
[`winrt-capture.cpp`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/libobs-winrt/winrt-capture.cpp).

This is the same continuous, GPU-native shape described by Microsoft's
[screen capture documentation](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture): a frame pool raises
`FrameArrived`, and each frame carries a Direct3D surface and capture time.

Two details matter for AUV:

- The stream should stay alive. Recreating the capture item, device, frame pool,
  and session for every `capture` request discards the main latency advantage.
- The frame should remain GPU-backed until a consumer actually needs CPU bytes.
  OBS currently performs a GPU-to-GPU `CopyResource` and notes that an SRV-aware
  consumer could avoid that copy. AUV should not immediately encode every frame
  as PNG.

WGC window capture is independent of ordinary overlap by other windows, but
"background" must not be documented as "minimized or invisible." OBS includes
minimized windows while resolving a target, then declines to initialize WGC
when the HWND is iconic or invisible. AUV direct-result metadata therefore
needs observed visibility/minimized state and a backend-specific stall or
unavailable reason instead of a universal `background_capture=true` claim.

The BitBlt path calls `GetDC(window)` and copies pixels with `BitBlt` in
[`dc-capture.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/win-capture/dc-capture.c).
It is a compatibility backend, not the preferred low-latency architecture.

### Display capture: DXGI Desktop Duplication or WGC

OBS exposes DXGI and WGC display methods in
[`duplicator-monitor-capture.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/win-capture/duplicator-monitor-capture.c).
The DXGI implementation creates an output duplication object, repeatedly calls
`AcquireNextFrame(0, ...)`, receives an `ID3D11Texture2D`, copies it into the
OBS texture, and releases the frame in
[`d3d11-duplicator.cpp`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/libobs-d3d11/d3d11-duplicator.cpp).

Microsoft's [Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
also exposes dirty regions, move regions, and pointer metadata. Those are useful
for incremental transport or change detection, but are not necessary for the
first latest-frame cache.

### Game capture: injected graphics hook

OBS Game Capture is not a faster form of WGC. It injects an OBS hook DLL into
the target process and hooks graphics presentation paths for Direct3D, OpenGL,
and Vulkan. The host and hook coordinate through named events, mutexes, shared
memory, and—when supported—a shared GPU texture. The entry path is visible in
[`game-capture.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/win-capture/game-capture.c),
with API-specific hooks under
[`graphics-hook/`](https://github.com/obsproject/obs-studio/tree/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/win-capture/graphics-hook).

This is the closest OBS analogue to a future ultra-low-latency osu backend:
capture before desktop composition, keep frames on the GPU, and run inference
without a PNG round trip. It is not suitable as AUV's default capture backend:
DLL injection adds architecture/bitness handling, anti-cheat and security
concerns, app compatibility policy, and a much larger failure surface. It would
need an explicit, opt-in, Windows-only capability and its own evidence.

## Linux

### Wayland: xdg-desktop-portal + PipeWire

OBS creates a ScreenCast portal session, asks the portal to select a monitor or
window, starts the session, opens the PipeWire remote, and then keeps a
PipeWire stream connected. The implementation is in
[`screencast-portal.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/linux-pipewire/screencast-portal.c).

This mirrors the official
[xdg-desktop-portal ScreenCast contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html):
the portal supports monitor, window, and virtual sources; `Start` returns
PipeWire node IDs; `OpenPipeWireRemote` returns the restricted remote; and
versioned restore tokens can persist a previous selection.

OBS negotiates `SPA_DATA_DmaBuf` when possible and imports the DMA-BUF directly
as a graphics texture. Otherwise it accepts memory-backed buffers and creates a
texture from their data. It drains queued buffers to the newest available frame
before processing and returns buffers to PipeWire after use. See
[`pipewire.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/linux-pipewire/pipewire.c)
and the portal's [PipeWire integration notes](https://flatpak.github.io/xdg-desktop-portal/docs/pipewire.html).

For AUV this strongly favors keeping the portal session, PipeWire connection,
stream, and negotiated buffers alive in the platform driver. A capture request
should acquire a lease/reference to the newest frame, not reopen the remote and
create a one-frame stream.

Wayland deliberately does not give applications a portable API to silently
select any other application's surface. The desktop portal mediates selection
and the compositor supplies the stream. A restore token can reduce repeated
user interaction, but AUV must represent whether selection was interactive,
restored, or newly authorized. Whether an occluded or minimized selected window
continues producing useful frames is compositor behavior, not a portable AUV
guarantee.

### X11 window: XComposite

OBS's X11 window source redirects the window with XComposite, obtains a named
window pixmap, and imports that pixmap as a GL texture in
[`xcomposite-input.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/linux-capture/xcomposite-input.c).
This avoids copying the composed desktop and normally allows capture of a
window covered by other windows.

The implementation considers a window to exist only when its XCB map state is
`VIEWABLE`; its retry comment explicitly expects pixmap creation to fail for
minimized windows or windows on offscreen workspaces. Therefore XComposite is a
good independent-window stream backend, but not a promise that an app can be
fully hidden or minimized indefinitely.

### X11 display: XShm

OBS's X11 display source uses `xcb_shm_get_image` against the root window on
each video tick and uploads the shared-memory image into a dynamic texture in
[`xshm-input.c`](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/linux-capture/xshm-input.c).
This is continuous at the OBS source lifecycle level, but it is a CPU-memory
capture/upload path and does not provide independent window contents.

OBS core has no Linux equivalent of its Windows injected Game Capture backend.
Low-latency Linux capture therefore remains compositor/portal capture on
Wayland or XComposite/XShm on X11 unless AUV deliberately takes on a separate
game/plugin integration.

## Implications for an AUV recording-first seam

The shared abstraction should describe the lifecycle and evidence without
pretending the platform mechanisms have identical guarantees:

```text
daemon-owned driver session
  -> platform-owned live source
       macOS: ScreenCaptureKit SCStream
       Windows: WGC frame pool / DXGI duplicator
       Wayland: portal session + PipeWire stream
       X11: XComposite window texture / XShm display feed
  -> bounded latest-frame cache
  -> capture request leases a frame
  -> consumer chooses GPU inference, CPU pixels, or encoded artifact
```

The platform driver should own the native session and permission/recovery
policy. The daemon may own its lifetime, subscriptions, cache limits, and
cross-request references. This follows AUV's existing responsibility split and
does not require a catch-all runtime crate.

A direct result should distinguish at least:

- requested scope and delivered scope: display, window, or region;
- backend: ScreenCaptureKit, WGC, DXGI, PipeWire portal, XComposite, XShm, or a
  future opt-in game hook;
- stable source identity plus a source epoch that changes when the underlying
  producer is replaced or reconnected;
- composition semantics: composed desktop, independent window, or pre-compositor
  app/game surface;
- frame provenance: stream frame or synchronous native snapshot;
- freshness: frame capture timestamp, request timestamp, age, and whether the
  frame was repeated;
- stream health: advancing, stalled, minimized/unmapped, permission revoked,
  source closed, or unknown, plus dropped-frame count when available;
- target state known to the backend: visible, covered, minimized/unmapped,
  closed, or unknown;
- authorization/selection state, especially interactive versus restored portal
  selection;
- applied inclusion/exclusion filter, child-window policy, and protected-content
  outcome where the platform exposes them;
- storage form: GPU surface, DMA-BUF, CPU pixels, or encoded artifact, including
  conversions/copies performed.

`--capture recording-frame` can then mean "lease the freshest acceptable frame
from an already running source," while `--capture native` remains a deliberate
synchronous/high-fidelity path. Neither flag should imply display versus window
scope; scope and acquisition mode are separate axes.

## Recommended first slices

1. Define the capture provenance and freshness metadata before changing a
   backend. This prevents a stream frame from masquerading as a native snapshot.
2. On macOS, keep one ScreenCaptureKit display stream alive for the default
   recording frame, add a leased desktop-independent window stream, and retain
   only bounded IOSurface references. Preserve `SCScreenshotManager` as the
   explicit native snapshot path.
3. On Windows, keep a DXGI display source alive for the recording-first/default
   path, then prototype one leased WGC window source. Measure
   frame-arrival-to-consumer latency with GPU-to-CPU conversion deferred.
4. On Linux Wayland, reuse the existing portal session but keep the PipeWire
   stream alive and cache the newest buffer/frame reference. Record portal
   restore and selection outcomes.
5. Keep X11 XComposite and XShm as distinct capabilities rather than forcing
   them through Wayland terminology.
6. Treat injected game capture as a separate future experiment for osu, gated
   by explicit owner approval and security/anti-cheat analysis.

## Open questions requiring measurement

- Maximum stable frame age and cache depth for UI automation versus osu.
- Whether YOLO can consume the platform GPU texture/DMA-BUF directly in AUV's
  selected inference runtime, or whether one device-local conversion is needed.
- ScreenCaptureKit queue depth, pixel format, color-space conversion, and frame
  lease duration that minimize end-to-end inference latency without starving
  WindowServer's IOSurface pool.
- Whether one daemon-wide macOS display stream plus leased window streams has
  acceptable memory and energy cost, and what idle TTL should close a window
  stream.
- WGC and compositor behavior after a previously active target becomes
  minimized, occluded, moved to another workspace, or resized rapidly.
- Portal restore behavior across GNOME, KDE, wlroots-based compositors, and app
  restarts.
- Whether native snapshot mode actually provides higher resolution or merely a
  different composition/visibility semantic on each backend.
