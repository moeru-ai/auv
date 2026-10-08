# Playground scripts on the SDK transport

Date: 2026-10-08

Status: **proposed**, with a prototype (`sdk` and `device` script globals)
behind the existing script API. Names marked *provisional* are open.

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
  (`rpc:<Service>/<Method>`). Its effect (`input` vs `read`) comes from the
  Runner's method annotations via `discoverRunner`; request JSON comes from the
  SDK call (`jsonBody`); responses decode through the discovered descriptors.
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

Deferred, each marked in code:

- `TODO(playground-sdk-mock)`: the mock desktop answers SDK RPCs. It needs an
  RPC-level mock Runner (`defineInvokeHandlers` keyed by method).
- `TODO(playground-sdk-replay)`: replay of SDK calls. The recording becomes the
  ordered request/response frames; existing recordings would not replay.
- `TODO(playground-sdk-visualize)`: canvas and inspector rendering of RPC
  results by message type. The prototype records method, effect, JSON and
  timing only.
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

## Open questions

- Whether `auv` itself becomes the SDK client once `device` covers it.
- Whether replay keeps old recordings (migration) or starts fresh.
- Whether the mock desktop stays, or the playground requires a device.
