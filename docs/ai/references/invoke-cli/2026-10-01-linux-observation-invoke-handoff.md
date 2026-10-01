# Linux observation commands through invoke

Status: implemented. This is an owner-approved feature slice of
[#211](https://github.com/moeru-ai/auv/issues/211).

## Scope and boundaries

Connect five direct local commands to existing typed Linux Driver APIs:
`window.list`, `window.capture`, `screen.captureRegion`, `screen.findText`, and
`screen.waitForText`.
Reuse command results, reports, and tracing artifacts.
Keep selected Runner execution separate from direct local Driver calls.

Application targets retain their existing bundle/accessibility identifier meaning.
On Linux, `app:<id>` matches the AT-SPI `AccessibleId` exposed in `window.list`.
A title selector works when an application does not expose this identifier.
Application-name targets remain deferred under the existing target contract.

Window capture keeps the existing display-crop behavior and its safety checks.
It does not activate windows or isolate their content from occluding surfaces.
Window metadata is the selector snapshot; `capture.bounds` describes the captured region.
Descriptor refresh after movement between resolution and capture remains a candidate follow-up contract slice.
Linux overlay presentation remains unavailable and does not block observation.
Region capture requires one containing display.
OCR uses the existing Tesseract engine and logical coordinate projection.
Text waiting retains the existing timeout and polling defaults.

## Implemented path

The command modules call `auv::local::open()` and the existing Driver session APIs.
Window enumeration uses AT-SPI.
Window capture resolves the existing selector before capture and PNG publication.
Linux keeps the complete accessibility identifier as the application target.
It does not substitute an application name or unrelated window on failure.

Region capture uses `DisplayApi::capture_region` and the existing primary PNG receipt.
Screen OCR uses `VisionApi::find_text_in_capture` with the full capture region.
Its output contains logical screen bounds, not image-pixel bounds.
The terminal OCR source PNG remains a tracing artifact.
It is not attached to the direct result, matching the existing command contract.

Local OCR reuses the existing `runner::wait_for_selected_text` policy.
The helper, selected Runner calls, and existing wait tests retain their original names and locations.
The helper is now visible within the crate, with no public API change.
The default timeout is five seconds, with a 100-millisecond poll interval.
The helper checks the deadline after an empty observation.
Capture, OCR, and consent latency can therefore exceed the nominal timeout.
Cancellation interrupts the interval between observations.
Synchronous Driver calls finish before the command checks cancellation again.

The support metadata work in #211 remains a separate slice.
Template matching, new input backends, window OCR, and OCR clicks are outside this scope.

## Automated validation

The initial dry-run reproductions rejected Linux before the platform branches changed.
These temporary reproductions are not committed tests. They proved branch availability without native behavior.
New unit tests cover cancellation before observation, cancellation during the poll interval, and observation errors without retries.
The interval test polls the future before and after cancellation. It does not depend on elapsed wall-clock time.
The shared wait tests exercise empty-then-matching observations and timeout.
Existing tests cover PNG pixels and receipts, region validation, and typed window results.
The existing OCR serialization test now runs on every platform.
No integrated tests changed.
The new tests do not depend on a desktop environment, display identifiers, or live desktop permissions.
These tests cover contracts and polling policy. They do not establish native platform support.

```sh
cargo test -p auv-cli-invoke --all-targets
cargo clippy -p auv-cli-invoke --all-targets --all-features
cargo fmt --check
cargo check
git diff --check
cargo run --quiet -- invoke --help
```

The invoke suite has 95 passing tests after excluding one existing help-format failure.
`every_registered_command_keeps_examples_with_its_typed_help` fails because `input.holdKeys` uses `Example:` instead of `Examples:`.
That command and test remain unchanged.

Default `cargo test` retains three existing NixOS failures because daemon tests invoke the absent `/bin/kill`.
The other 85 default tests pass.
The affected tests are `root_device_and_run_flags_resolve_into_plugin_context`,
`local_daemon_routes_runner_grpc_without_claims_or_leases`, and `local_serve_and_devices_list_use_the_unix_daemon`.

Formatting, compilation, Clippy, and diff validation pass with existing unrelated warnings.
