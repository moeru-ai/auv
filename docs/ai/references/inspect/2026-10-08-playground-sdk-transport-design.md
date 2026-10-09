# Playground scripts on the SDK transport

Date: 2026-10-08

Status: **accepted and complete.** Since 2026-10-09 the script global `auv`
*is* the SDK Runner client, `device` is gone, and the per-method bindings are
deleted; see
[`../session-api/2026-10-09-sdk-ergonomics-design.md`](../session-api/2026-10-09-sdk-ergonomics-design.md)
Phase 3. The sections below record the design as decided, with `device` as the
interim name. Names marked *provisional* are open.

## Problem

Playground (`apps/repl-playground`) scripts call a curated script API
(`auv.*`, `WindowHandle`, …). Each capability is hand-wired four times: the
script API type, the worker proxy (`call('name', …)`), a host binding
(`runtime/bindings.ts`), and every backend (`auv`, `mock`, `replay`). Runner
RPCs that nobody wired (mouse primitives, held keys, AX focus, media, …) are
unreachable, and a playground script does not run as-is in Node.

The curation exists for three host features: the mock desktop, run recording
and replay, and visualization (canvas, inspector, camera).

## Direction

Scripts use `@auv-js/sdk` directly. The playground supplies only the
**transport**, and does its host work on the RPC stream instead of on a
per-method API:

```text
exec worker                                host (main thread)
  script → @auv-js/sdk RunnerClient
    → playground Transport  ──eventa──▶  forwarder ─▶ active backend transport
      (unary, duplex)        MessagePort      │         (daemon HTTP + credential;
                                              │          mock / replay: TODO)
                                              └─▶ call record (method, effect,
                                                  request/response JSON, timing,
                                                  calling line)
```

- **Same script in Node and the playground.** Only the connection differs:
  Node calls `connect(...)`; the playground hands the script a connected
  `device` (the Runner client for the selected Device and the current Run)
  and `sdk` (the SDK module), injected before every run.
- **No per-method host code.** Any Runner RPC the SDK can call works, and is
  recorded, without a binding.
- **Transport bridge: `@moeru/eventa`.** One unary invoke and one
  bidirectional stream invoke carry `{ method, headers, body }` frames over a
  dedicated `MessageChannel` port, separate from the existing `birpc` channel.
  Protobuf bytes cross as bytes. Abort signals map to eventa cancellation.
- **Credential stays on the host.** The forwarder adds the paired Device
  bearer; the worker never sees it. Routing headers (Device, Run,
  RunnerClass) come from the SDK route in the worker, which the host passes in
  the run request.
- **Recording by type, not by method.** The host records each RPC as a call
  (`<Service>/<Method>`, without the package). Its effect (`input` vs `read`)
  comes from the Runner's method annotations via `discoverRunner`; request JSON
  comes from the SDK call (`jsonBody`); responses decode through the discovered
  descriptors.
  Visualization (thumbnails for `CapturedFrame`, boxes for recognized text,
  receipts for `InputActionResult`) keys on message type names, so a new RPC
  returning a known type is drawn without new code.

## Prototype scope

Implemented:

1. `runtime/sdk-bridge.ts` (*provisional* name): eventa definitions, the
   worker-side `Transport`, and the host forwarder with call recording.
2. Script globals `sdk` and `device`, next to `auv`. `device` was chosen over
   `local` because the selected Device may be remote.
3. Host forwarding for the `auv` backend, including JSON-encoded HTTP
   bindings (the response text is decoded back in the worker, where the
   SDK's `decodeJson` lives).
4. Unit tests over a `MessageChannel`.

Timeline and replay (2026-10-08):

5. **Drawing by message type** (`runtime/rpc-resources.ts`). The host decodes
   each call's messages with the Runner's reflected descriptors
   (`DescribedRpcMethod.input` / `output`) and registers the playground's
   existing resources by protobuf type name:
   - `CapturedFrame` becomes a frame, labelled by the request's window or
     display;
   - `TextMatch` lists and `RecognizeTextResponse` become text, tied to the
     capture in the same message;
   - `InputActionResult` becomes an input receipt at `screen_point`, or at the
     window point plus the window origin;
   - `Window` and `Display` become outlines.

   Oneof events and repeated fields are walked, so each scroll-until step is
   a frame. The kind of an input receipt comes from the method name.
6. **Replay at the transport.** `RecordingBackend.sdk()` wraps the live
   transport and records each call as its request and response messages, in
   the order they crossed (`RecordedRpc`). `ReplayBackend` serves
   `ReplayTransport`, which works as follows:
   - A call matches the first unused recorded call with the same method and
     request bytes, so concurrent calls replay in any order.
   - Streams replay in lockstep, so a scroll-until decision is compared before
     the next update.
   - Errors replay as the same SDK error class.
   - An unknown call or stream message raises `ReplayDivergence`.
7. **Previews.** Frames and the first live frame of a display show the
   capture's ThumbHash (`CapturedFrame.thumbhash`, #286) and fade the pixels
   in over 750 ms (`preview.ts`).

Deferred, each marked in code:

- ~~`TODO(playground-sdk-mock)`~~ Done (2026-10-09): the mock desktop is a
  mock Runner (`createMockTransport`), so it serves SDK RPCs too; see
  `../session-api/2026-10-08-mock-runner-design.md`.
- `TODO(playground-sdk-stream-progress)`: a stream's resources appear when it
  ends; scroll-until steps could appear as they arrive.
- `TODO(playground-sdk-input-kinds)`: mouse primitives and held keys record
  no input receipt, because the script API's receipts do not name their kinds.
- `TODO(playground-sdk-types)`: SDK types in the editor's language service, so
  `device.` completes.
- Moving the bridge into `@auv-js/sdk` when a second host (Electron, iframe)
  needs it.

## Migration (after the prototype)

1. Keep `auv.*` working while `device` gains mock, replay and visualization.
2. Move the ergonomic pieces of `auv.*` (`area()`, `region()`, key strings,
   receipts) into a helper package both Node and the playground import. This
   joins the approved SDK geometry-helper work (B.4/B.5).
3. Remove the per-method bindings once `auv.*` is a thin layer over `device`.

## Decisions (owner, 2026-10-08)

- `auv` becomes the SDK client once `device` covers what scripts use today.
- Replay does not read the current recordings. Those are kept in page memory
  only (the last live run), so nothing persisted needs migrating.
- The mock desktop stays. It moves to the SDK layer: a mock Runner that
  answers SDK RPCs, usable from Node tests and the playground alike
  (implemented 2026-10-09).
