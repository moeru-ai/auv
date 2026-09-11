# Remote extension and Runner integration guide

Status: implemented pattern with live Balatro evidence on 2026-08-04. This is a
guide for adding another app-owned extension; it is not a general support claim
for every application or Linux desktop.

## Shared execution shape

```text
auv --device-id <id> <extension> <operation>
  -> root CLI resolves Device + Run
  -> root CLI launches the extension with AUV_CONTEXT
  -> extension parses app-local arguments
  -> auv::Client attaches to the inherited Run
  -> auv.core.local Runner provides capture, OCR, and input
  -> app Runner provides app-owned recognition or inference
  -> app operation maps InputActionResult into its typed result
  -> app operation verifies semantic state separately
```

The root CLI owns device selection, authentication, extension discovery, and
run-context lifecycle. The extension owns app vocabulary and operation policy.
The daemon owns Runner admission and supervision. A RunnerClass is the stable
capability identity; its executable or remote-gRPC provider is operator
configuration, not app protocol data.

An extension should use `auv::Client::from_context` or `from_env` when
`AUV_CONTEXT` is present. It must not resolve a second device or create a
parallel transport. `RunClient::finish_if_owned` allows the same code to create
a run when invoked directly and attach without finishing when invoked by the
root CLI.

## Two Runner classes, one operation

The Balatro proof uses:

- `auv.core.local` for display/window discovery, capture, OCR, and input;
- `auv.game.balatro` for app-owned object detection.

Both are acquired from the same inherited Run. App-generated protobuf clients
use `RunnerClient::transport()`; daemon lifecycle and authentication metadata
remain outside app messages. Image-bearing generated clients must configure an
explicit gRPC image message-size policy rather than relying on tonic's 4 MiB
default.

Custom Runner providers are registered with `auv serve --runner-provider`.
Registration is the operator trust decision: a provider may execute arbitrary
code on the Device. Do not send an executable path from the caller as an
operation argument.

## Assets belong to the execution host

Do not send a path resolved on the extension host to a remote Runner. Use one
of these explicit source forms:

- a Runner-host path, when the operator provisioned it there;
- a content-addressed or repository asset description that the Runner resolves.

The Balatro detector request uses a `oneof` between `runner_path` and a
Hugging Face repository asset. The Runner resolves and caches repository assets
inside `spawn_blocking`; the async transport thread never enters a blocking
Hugging Face runtime. Small caller-owned metadata such as class-name files may
be loaded on the caller in `spawn_blocking` and sent as typed request data.

## Capture and coordinates

Prefer a resolved Window capture when the platform can identify the app. A
Proton/LÖVE window may be visible but absent from AT-SPI. In that case the
Balatro operation records the explicit source
`daemon://display/primary?fallback=window_unavailable` and captures the primary
display.

Every action point must name its coordinate space. For a display fallback:

```text
screen.x = display.x + image.x / image.width  * display.width
screen.y = display.y + image.y / image.height * display.height
```

Never serialize a screen point as `WindowPoint`. App-owned action facts use a
coordinate-qualified point and retain the shared `InputActionResult`. The
shared `auv::client::runner::input_action_result_from_proto` converter exists
so extensions do not grow parallel delivery-result schemas.

Viewer previews may scale screenshots. Layout measurements must use artifact
pixel coordinates, not rendered preview coordinates.

## Delivery is not semantic success

The action seam remains:

```text
app operation -> Driver Runner -> InputActionResult -> app result
                                      + separate state verification
```

Linux/XWayland may accept a foreground-system-events click that only focuses a
window. A successful delivery attempt therefore remains `verified: false`.
The app operation re-observes and proves its own transition. Retry policy must
be based on the expected state transition, not merely the continued presence
of a detector label. Translucent overlays can expose controls underneath them.

For card submission, comparing against the pre-selection hand is invalid:
selection itself moves cards and changes crop fingerprints. Compare the
post-submit frame with the fully selected baseline, and accept fingerprint
replacement only when the Play/Discard selection controls also clear.

## Deployment checklist

1. Build the root CLI and app Runner with the repository Rust version.
2. Register the app Runner provider on the Device daemon.
3. Pass the graphical session environment (`DISPLAY`, `WAYLAND_DISPLAY`,
   `XDG_RUNTIME_DIR`, and user D-Bus address) to the daemon service.
4. Provision Runner-host model assets or use a repository asset source.
5. Provision requested OCR languages. If system installation is unavailable,
   use a user-owned `TESSDATA_PREFIX` containing all requested languages.
6. Confirm the paired Device is online with a read-only core operation.
7. Prove capture and its pixel-to-logical coordinate contract.
8. Prove an app read through both Runner classes.
9. Prove input delivery and semantic verification as separate result fields.
10. Inspect durable run records and artifacts; do not infer persistence from a
    successful RPC.

## Current tracing boundary

The live Balatro commands inherit a Run reference and all Driver/app RPCs route
through it. Direct `invoke display.capture` records a run and PNG artifact in
the root CLI store. App event emitters are feature-gated, however, and the
2026-08-04 remote daemon store contained only its device identity after live
plugin operations. Therefore durable remote plugin run/event persistence is
not yet evidenced.

TODO(remote-extension-run-persistence): connect inherited extension operations
to a durable daemon-side run/event/artifact consumer when the owner approves
that core tracing slice. The acceptance test must inspect the daemon store for
one extension Run containing app facts plus capture/input evidence; successful
transport calls are insufficient.

## Permission findings

No authentication relaxation was needed. The paired bearer profile resolved
the Device and routed Runners successfully. The Linux screenshot portal also
worked without a new persistent permission token in this session. Earlier
failures were stale-build/protocol, message-size, asset-host, and async-runtime
problems—not over-strict pairing permissions.
