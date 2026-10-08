# Capture references and positions

> Naming migration (2026-10-08): `ScrollUntilObservation` → `ScrollUntilUpdate`; `ScrollUntilObserve` → `ScrollUntilOutputOptions`.
> This dated note may retain the former names. Prefer concrete domain results;
> reserve `observation` for information used to decide the next action.
> See the [migration and current mapping](../runtime/2026-10-08-domain-result-naming-migration.md) before implementing examples.

Status: Part A implemented (2026-10-07, branch `feat/capture-refs`); Part B
proposed. Names marked *provisional* are open for review.

This note proposes two related simplifications of the driver API:

- **Capture references.** Captured pixels stay in the Runner. Calls exchange
  references and metadata, and pixels come back only when a caller explicitly
  asks for them.
- **Positions.** Use one coordinate model across input, OCR and regions. It is
  built on the existing `Position` contract.

Backward compatibility is not a goal (AUV 0.0.29). Each slice changes the
Protobuf contract, the Runner, the Rust client, the JS SDK, and their callers
together.

## Evidence

The evidence comes from building `apps/repl-playground` (#249, #250; then `devtools/repl-playground`)
against a local daemon, with read-only measurements on macOS.

- **Captures are large.**
  - A Retina window capture is 3292×1932 RGBA, about 25 MB; a display capture
    is about 80 MB.
  - `capture()` takes 0.7–0.9 s over HTTP, plus about 60–70 ms of protobuf
    decode on the browser main thread.
  - The playground had to add a raw-pixel memory budget, decode at logical
    resolution, and keep a cached canvas layer to stay responsive.
- **`RecognizeText` takes the capture back as input.**
  `RecognizeTextRequest.capture` is a full `CapturedFrame`. OCR on a capture
  therefore downloads 25 MB and uploads the same 25 MB.
- **`FindWindowText`/`FindDisplayText` always return the capture.** The proto
  comment says this is so that "clients may persist it as an artifact". That
  pushes evidence storage onto every client.
- **`ScrollUntil` observations carry a capture and OCR by default** for every
  step.
- **Coordinates come in several flavors for one concept.**
  - Input is split by space: `ClickScreenPoint` versus `ClickWindowPoint`,
    `ScrollWindowPoint*` (window-local only), and `MoveMouse`/`DragMouse`
    (screen).
  - OCR bounds are screen rectangles (`capture.bounds.origin` plus pixels /
    scale on every driver); `origin` maps them into the capture's owning
    space. This note first described them as offsets, which was wrong: one
    consumer, scroll-until's `text_match`, made the same mistake and offset
    matches twice (fixed with Part B.2).
  - Regions are 0–1 fractions (`NormalizedRect`).
  - The playground converts screen areas to fractions and to window-local
    points.
  - `auv-netease-music` converts bounds to ratios (`bounds_to_ratio`).
  - The same normalized rectangle is defined three times: `RatioRect`
    (`auv-driver-common`), `NormalizedRegion` (`auv-core` client) and proto
    `NormalizedRect`.

The rule this design follows is now in `AGENTS.md` ("Image Payloads", #251).

## Part A — Capture references

### Terms

- **Capture reference** (`CaptureRef { capture_id }`, *provisional*): a
  Runner resource. It names one capture held in that Runner's capture store
  (see [resource reference scope](../../../TERMS_AND_CONCEPTS.md#resource-reference-scope)).
  Like `FrameBufferRef`, it is valid only on a Run-affine route to the same
  Runner. The local Runner (`auv.core.local`) is persistent, so local
  references work across Runs.
- **Capture store** (*provisional*): an in-memory, least-recently-used cache in
  the Runner, bounded by bytes
  (`crates/auv-cli/src/runner/capture_store.rs`).
  - It holds every capture the Runner produces: window, display and region
    captures, find-text evidence, and scroll-until steps.
  - An evicted, expired or unknown reference fails with `NOT_FOUND` ("… was
    evicted, expired, or produced by another Runner; capture again").
  - Default budget: 512 MiB (`AUV_CAPTURE_STORE_BUDGET_MIB`). That is about 20
    Retina window captures or 6 display captures.
  - Captures unused for 10 minutes expire (`AUV_CAPTURE_STORE_IDLE_SECONDS`); a
    60 s sweeper returns the memory of an idle Runner.
  - There is no release RPC. Eviction, expiry and Runner shutdown are the only
    ways a capture leaves the store.
- **Capture image fetch**: an explicit request for pixels, possibly bounded and
  encoded.

### Protobuf changes (`capture.proto`, `text_recognition.proto`, `input.proto`)

Shapes as of the cleanup on 2026-10-07 (`refactor/capture-api-shapes`), which
split pixel-carrying frames from references and gave RGBA one representation:

```proto
message CaptureRef { string capture_id = 1; }

// A capture the Runner holds: never pixels.
message CapturedFrame {
  reserved 1;                                // was `image`
  CaptureRef ref = 7;                        // always set
  ScreenRect bounds = 2;
  double scale_factor = 3;
  string backend = 4;
  optional string fallback_reason = 5;
  Position origin = 6;
  auv.api.image.v1.PixelSize pixel_size = 8;
}

// Pixels with screen placement: caller-owned images and recent frames only.
message ImageFrame {
  auv.api.image.v1.RgbaFrame image = 1;
  ScreenRect bounds = 2;
  double scale_factor = 3;
  string backend = 4;
  optional string fallback_reason = 5;
  Position origin = 6;
}

service CaptureService {
  rpc GetCaptureImage(GetCaptureImageRequest) returns (GetCaptureImageResponse);
}

message GetCaptureImageRequest {
  CaptureRef capture = 1;
  optional auv.api.image.v1.NormalizedRect region = 2; // crop first
  auv.api.image.v1.PixelSize max_size = 3;             // fit inside; absent = native
  ImageEncoding encoding = 4;                          // RGBA (default), PNG, JPEG, WEBP
}
message GetCaptureImageResponse {
  reserved 1, 2;                                       // were the rgba/encoded oneof
  auv.api.image.v1.EncodedImage image = 3;             // RGBA is raw rows
}

message RecognizeTextRequest {
  reserved 2;                                          // was `CapturedFrame capture`
  oneof source {
    CaptureRef capture_ref = 6;   // AUV-produced capture: no pixels sent back
    ImageFrame image = 7;         // caller-owned image only
  }
  ...
}
```

Clients mirror this:

- Rust's `CaptureImage` is one struct, `{ encoding, size, data }`, with
  `into_rgba_image()`. JS returns the same fields.
- `CaptureImageOptions.max_size` and `RunnerCapture.pixel_size` are
  `auv_driver::PixelSize`, not tuples or separate fields.
- The client no longer duplicates `DisplayCapture`, `RegionCapture` or
  `ScrollUntilObservation`. They are generic over the capture type
  (`DisplayCapture<RunnerCapture>`).

The capture RPCs, the find-text responses and `ScrollUntilObservation.capture`
return `CapturedFrame`: `ref` and metadata, never pixels.
`ScrollUntilObserve.omit_capture` is replaced by nothing: references cost
nothing to return. A caller that wants pixels makes an explicit
`GetCaptureImage` call.

Deferrals that keep this slice focused:

- `TODO(capture-store-run-release)`: the owner approved freeing a Run's
  captures when the Run ends, but Runners cannot see Runs (the daemon strips
  the `auv-run-id` header before forwarding). Idle expiry stands in until
  Runners receive Run identity and stop notifications.
- There is no explicit release RPC (owner decision, 2026-10-06). Add one only
  when a long-running client needs to free memory before eviction.
- `TODO(recent-frames-capture-refs)`: `GetRecentFrames` still returns frames
  with pixels. Frames should enter the capture store and travel as
  references. They will share that path with the planned video stream.

### Consumers

| Consumer | Today | After |
|---|---|---|
| Rust `auv-core` client | `WindowCapture.capture: auv_driver::Capture` (pixels) | A capture value with `reference`, bounds, origin, scale and pixel size; `runner.captures().image(&reference, options)` returns `auv_driver::Capture` when pixels are needed |
| Rust `recognize_text` | Takes `auv_driver::Capture` | Takes a capture reference, or a caller-owned `Capture` |
| `auv-cli-invoke` artifacts (`display.capture`, `screen.captureRegion`, find-text) | Writes a PNG from response pixels | Explicitly fetches PNG-encoded bytes via `GetCaptureImage` |
| `auv-game-balatro` OCR | Captures, then `recognize_text(capture)` (round trip) | `recognize_text(reference, region)` |
| `auv-scan` scroll-until | `observe.capture` opt-out; `capture: Option<Capture>` | Every observation carries its capture (the loop captures each step for motion anyway); `observe` keeps only `text` |
| JS SDK | `capture()` returns pixels; `recognizeText(frame)` | `capture()` returns metadata plus `ref`; `runner.captures.image(frame, { maxSize, encoding })`; `recognizeText(frame)` by reference, `recognizeText({ frame })` for caller-owned pixels |
| repl-playground | Decodes 25 MB RGBA per capture; keeps raw pixels within a budget for OCR | Fetches a logical-resolution JPEG; `createImageBitmap(blob)` decodes off the main thread; OCR by reference; the raw-frame budget is deleted; replay records images by reference outside the ordered call log |

Run recording keeps its current shape in this slice. `auv-cli-invoke`
persists PNG artifacts, now by explicit fetch. Persisting evidence on the AUV
side from references is a follow-up (`TODO(runner-side-capture-artifacts)`).

## Image encodings (measured 2026-10-07)

Encoder comparison on real macOS captures, release build of the `image` crate
0.25. The samples were three displays (6016×3384), a centered window-sized
crop of each (55%), and that crop at logical 1×. Screen contents were not
recorded.

| Encoding | Size vs PNG | Encode time (window-sized, 3308×1861) | Lossless |
| --- | --- | --- | --- |
| PNG (`image` default) | 1× | 5–25 ms | yes |
| WebP lossless | 0.26–0.71× | 15–32 ms | yes |
| QOI | 0.73–0.96× | 3–14 ms | yes |
| JPEG q85 | 0.15–0.82× (smallest on photo-heavy screens) | 27–34 ms | no |
| AVIF q80, speed 10 | 0.02–0.4× | 0.8–1.2 s (2.7–3.9 s per display) | no |

Decisions:

- Evidence artifacts (screenshots, OCR sources, overlays) are lossless WebP,
  through one shared encoder, `auv_tracing::image_artifact`, which replaces
  per-app PNG code in invoke, NetEase, Apple Music, Minecraft and Balatro.
- Capture evidence is stored at logical resolution (owner decision,
  2026-10-07: evidence is for review and feedback). Area averaging
  (`imageops::thumbnail`) took ~9 ms for a Retina window, versus 21–37 ms for
  Triangle, CatmullRom or Lanczos3. Encoding a quarter of the pixels then
  costs about what WebP took at native size, for a quarter of the pixels
  stored. These stay native: NetEase sidebar target probes (OCR region input),
  Balatro frames (detections in image pixels) and Minecraft screenshots (no
  backing scale).
- `GetCaptureImage` adds `WEBP` (lossless). JPEG 85 stays for display
  thumbnails: it is the smallest on photo-heavy screens.
- AVIF is rejected for call paths. It takes seconds per capture, and its
  lossy chroma subsampling blurs small colored text.
- QOI is the fastest lossless encoder. It is a candidate for compressing cold
  captures inside the store (capture-store preprocessing, not designed
  yet), not for evidence.

## Capture resolution (measured 2026-10-07)

macOS window captures used to come back at 1x on Retina displays. The
ScreenCaptureKit path passed the window's frame in points as
`SCStreamConfiguration.width/height`, which are output pixels. Display
captures (xcap) were 2x. The two capture paths disagreed, and every OCR on a
window read half the detail.

Decision (owner, 2026-10-07): captures default to native resolution, and
`CaptureResolution::Logical` is opt-in. Logical captures serve display-only
frames (playground live mode) and motion-only scroll-until loops.

Release-build window captures on a 6K display, three samples each:

| Capture | Before | After |
| --- | --- | --- |
| Native (2x, ~81 MB RGBA) | 3.7–4.7 s | 0.28–0.36 s |
| Logical (1x) | 1.2–1.5 s | 0.27–0.29 s |
| Display native (xcap) | ~50 ms | ~50 ms |
| Display logical (xcap + area-average downscale) | — | ~78 ms |

Most of the old window-capture time was swift-bridge copying pixels into a
`RustVec` one byte per FFI call. A bulk Rust constructor
(`native_byte_vec_from_raw`, `NOTICE(swift-bridge-bulk-bytes)`) removed it.
After that, native and logical window captures cost about the same.

Consumers validated on 1x keep 1x:

- NetEase flows were the exception until 2026-10-07. They now request
  `Native` (`NOTICE(netease-native-captures)`): a live sidebar comparison
  read about 24 of 103 rows wrong at 1x and none at 2x, at about twice the
  OCR time. Their pixel analyses (motion crops, the play-button classifier,
  icon templates) stay at 1x, and text read off cover thumbnails is dropped
  (`NOTICE(netease-cover-art-text)`).
- Scroll-until compares motion per logical point
  (`NOTICE(scroll-until-logical-motion)`): `ViewportPixelPolicy` was tuned on
  1x captures.

## Capture store preprocessing (2026-10-07)

Captures held by reference let the Runner reuse work on them
(`crates/auv-cli/src/runner/capture_store.rs`):

- **Dedupe.** Identical captures (blake3 of pixels and metadata) share one
  blob. This covers polling an unchanged window: `waitForText`, and the steps
  at the end of a scroll-until loop.
- **Derived caches** on the blob:
  - OCR results, keyed by region, custom words and languages;
  - `GetCaptureImage` results, keyed by region, max size and encoding. Raw
    unresized RGBA is not cached, because it would duplicate the pixels.
- **Cold packing.** Blobs idle for 30 s are packed losslessly as QOI (3-14 ms
  for a Retina window) and unpacked on the next read.
- **Budget order.** Drop derived caches, then pack the least recently used hot
  blobs, then evict the least recently used captures.

Release build on a 6K display, read-only:

| Call | First | Repeated |
| --- | --- | --- |
| OCR on half the display | 3231 ms | 1 ms |
| JPEG thumbnail (1440×900) | 59 ms | 1 ms |

Deferred, with markers in code:

- `TODO(capture-store-find-text-seed)`: find-text does not seed the OCR cache.
- `TODO(capture-store-prepared-images)`: images are encoded on first fetch,
  not prepared at capture time.

## Part B — Positions

The domain already has the right model:

- `Position` is a point plus its `CoordinateSpace`: screen, display or window.
- `Positional` supplies a position without IO.
- `Capture.origin` and `TextRecognition.origin` tie images and OCR results to
  their space, and `relative_to()` rebases them (see
  [Position and Positional](../../../TERMS_AND_CONCEPTS.md#position-and-positional)).
- #174 already merged CLI clicks into
  `input.clickPoint --relative-to screen|window|display`.

The wire and the SDK still split everything by space. Proposal:

1. **Input takes `Position`.** `ClickPoint { Position position; ClickOptions }`
   replaces `ClickScreenPoint` and `ClickWindowPoint`.
   - A window position uses window-targeted delivery, and a screen or display
     position uses global delivery.
   - Window-scoped calls (`WindowClient.click`/`scroll*`) accept a position in
     window or screen space. The Runner converts screen positions using the
     window's current frame, so callers stop converting by hand.
   - `MoveMouse`/`DragMouse` keep screen points.
2. **Everything AUV returns is in screen space.** Already true:
   `RecognizedText.bounds` and `TextMatch.bounds` are screen rectangles on
   every driver, in the space of the capture's `bounds` (for a caller-owned
   image, the `bounds` the caller supplied). Done (2026-10-07): scroll-until's
   `text_match` no longer adds the capture origin a second time, and the
   docs that called these offsets are corrected.
3. **Regions accept a screen rectangle.** Done (2026-10-07):
   `RecognizeTextRequest`, `FindWindowTextRequest`, `FindDisplayTextRequest`
   and `GetCaptureImageRequest` gain `ScreenRect screen_region`, exclusive with
   `region`. A sibling field was chosen over a `oneof`, because a `oneof` would
   make JS callers write `{ area: { case, value } }`. The Runner maps the
   rectangle into the image and clips it; a rectangle that misses the image is
   `INVALID_ARGUMENT`. The Rust client takes `ImageRegion::{Normalized,
   Screen}`; the playground passes its areas directly.
4. **One normalized rectangle.**
   - Delete `auv-core`'s `NormalizedRegion` in favor of `auv-driver-common`'s
     type.
   - Rename `RatioRect` to `NormalizedRect` (*provisional*), so that Rust,
     Protobuf and TypeScript use one name.
   - `auv-view`'s `ViewBounds` stays for its documented dependency direction.
5. **SDK geometry helpers.** The JS SDK exports small pure helpers that both
   apps and the playground use instead of local copies:
   - `center(rect)`;
   - `Position.screen(x, y)` and `Position.window(windowOrRef, x, y)`;
   - `contains`, and `intersect`/`clip`.

   The playground's `area()` builds on these.

## Slices and order

1. **Capture references (Part A).** Proto, Runner store, `GetCaptureImage`,
   `RecognizeText(capture_ref)`, Rust client, invoke artifacts, Balatro, JS SDK
   and playground. The playground's raw-frame budget and its main-thread RGBA
   decode are deleted.
2. **Screen-space results and screen regions (Part B.2–B.3).** This removes the
   playground's `NOTICE(ocr-region-space)` and its screen-to-normalized
   conversion.
3. **`Position` input (Part B.1)** and the duplicate-type cleanup (B.4), plus
   the SDK helpers (B.5).

Each slice updates `TERMS_AND_CONCEPTS.md`, the SDK README and
`SUPPORT_MATRIX.md` where they describe the changed shape.

## Documents this supersedes

- `TERMS_AND_CONCEPTS.md` → **Capture Frame** says the caller decides whether
  to persist pixels. After slice 1, a capture is a Runner resource, and pixels
  leave the Runner only on explicit request.
- `proto/auv/api/driver/v1/text_recognition.proto` comment on
  `FindWindowTextResponse.capture` ("clients may persist it as an artifact").
- `docs/archive/verticals/session-api/2026-07-31-daemon-session-api-architecture.md`
  (archived) describes `RecognizeText` consuming a typed capture. It is already
  archived and needs no change.

## Decisions and open questions

Decided by the owner on 2026-10-06:

- The store budget is 512 MiB and configurable on the Runner (environment
  variable, above).
- JPEG quality is fixed at 85 (`NOTICE(capture-jpeg-quality)`).
- No release RPC.
- Captures should be freed when their Run ends; deferred as
  `TODO(capture-store-run-release)` (see above).

Still open:

- **Names.** `CaptureRef`, "capture store" and `GetCaptureImage` are
  provisional.
