# API Naming Normalization

Status: **approved by the owner on 2026-11-03.** This is a breaking
change, accepted because 0.0.40 has few users.

Related: [window management v1](../driver/2026-11-02-window-management-api-v1-design.md)
lands on top of this rename.

## Problem

1. Proto RPC names do not follow one rule: `InputKeyboard`, `Play`,
   `NextTrack`, `Enroll`, `OpenFrameBuffer` and `ListAudit` each use a
   different shape.
2. Three naming systems for one operation disagree. `presentation.name` is
   plural and snake_case (`windows.list`, `displays.capture_region`,
   `recognize_text`). invoke / MCP ids are singular camelCase and sometimes
   name another resource (`screen.findText`, `app.activate`,
   `mediaControl.play`).
3. Calls are grouped by capability in Proto (`CaptureService.CaptureWindow`,
   `InputService.ScrollWindowPoint`) but by resource in the JS SDK
   (`WindowClient.capture()`, `.scroll()`).
4. Run, Runner and RunnerClass are served under `/apis/auv/runtime/v1` but
   live in the Proto package `auv.api.daemon.v1`.

## Rules

1. **RPC method names follow AIP.** Standard methods are `List<Plural>`,
   `Get<Singular>`, `Create<Singular>`, `Update<Singular>`,
   `Delete<Singular>` (AIP-131 to 135). Other methods are verb + noun
   (AIP-136): `CaptureWindow`, `MinimizeWindow`. The noun is always written,
   even when the service already names it. A second collection in the same
   service is then just another method (`ListWindowTabs`), never a new
   package.
2. **Everything else is normalized, not forced into AIP.**
   - Resource references stay typed messages (`DeviceRef`, `WindowRef`) that
     carry ids, not AIP-122 resource-name strings. The compiler checks the
     reference type in Rust and TS.
   - `Get` over REST stays `POST …:get` (`NOTICE(tonic-rest-path)` in
     `device.proto`: the generator cannot take typed refs as path parameters).
   - No pagination, `FieldMask` or LRO in this wave.
3. **Packages are API groups, not resources**: `auv.api.driver.v1`,
   `auv.api.daemon.v1`, the new `auv.api.runtime.v1`, and the platform package
   `auv.api.driver.macos.v1`. A new package needs a new API group.
4. **Services are resources.** A call sits in the service of the resource it
   acts on. Input that takes any position stays in `InputService`.
5. **One operation id** for `presentation.name`, the invoke id and the MCP
   tool name: `<service resource>.<rpc without the resource noun>`, lower
   camelCase. `WindowService.SetWindowFrame` → `window.setFrame`,
   `WindowService.ListWindows` → `window.list`,
   `InputService.ClickPoint` → `input.clickPoint`. The rule is mechanical, so
   a test can check it.
6. **Removal** of a resource with an id is `Delete`. Taking a presentation off
   the screen is `Remove` (`RemoveOverlay`).

### Recorded exceptions to AIP-136 verb + noun

- `KeyDown`, `KeyUp`, `MouseDown`, `MouseUp`: Down/Up for hold and release,
  with `PressKeys` and `ClickPoint` for press-and-release, is the common
  input vocabulary: MaaFramework (`KeyDown`, `TouchDown`, `ClickKey`,
  `Click`), BetterGI (`keyDown`, `leftButtonDown`, `keyPress`), Playwright
  (`down`, `up`, `press`, `click`) and W3C WebDriver (`keyDown`,
  `pointerDown`). No reference uses Press for hold, so the names stay.
- `HealthService.Check`: the gRPC health-checking convention.

## Enforcement

`TODO(api-linter)`: run Google `api-linter` in CI with only its method-naming
rules enabled, and the exceptions above disabled in its config file with a
reason. If its rules cannot be scoped to naming, use a descriptor test in
`crates/auv-api-proto` instead. The test checks rule 5 (operation id derived
from the RPC) and that every discoverable RPC has an invoke command or an
explicit SDK-only marker.

`crates/auv-api-proto/build.rs` keys serde attributes by full Protobuf path,
and prost silently ignores a path that matches nothing. The `runtime` move
must update those paths by hand (see Decided).

## Driver mapping (`auv.api.driver.v1`)

| Old | New | Operation id | Old presentation / invoke |
| --- | --- | --- | --- |
| `DisplayService.ListDisplays` | unchanged | `display.list` | `displays.list` / `display.list` |
| `CaptureService.CaptureDisplay` | `DisplayService.CaptureDisplay` | `display.capture` | `displays.capture` / `display.capture` |
| `CaptureService.CaptureRegion` | `DisplayService.CaptureDisplayRegion` | `display.captureRegion` | `displays.capture_region` / `screen.captureRegion` |
| `TextRecognitionService.FindDisplayText` | `DisplayService.FindDisplayText` | `display.findText` | `displays.find_text` / `screen.findText` |
| `WindowService.ListWindows` | unchanged | `window.list` | `windows.list` / `window.list` |
| `WindowService.ResolveWindow` | unchanged | `window.resolve` | `windows.resolve` / none |
| `CaptureService.CaptureWindow` | `WindowService.CaptureWindow` | `window.capture` | same / same |
| `TextRecognitionService.FindWindowText` | `WindowService.FindWindowText` | `window.findText` | `window.find_text` / `window.findText` |
| `InputService.ScrollWindowPoint` | `WindowService.ScrollWindow` | `window.scroll` | `window.scroll` / `input.scroll` |
| `InputService.ScrollWindowPointMotion` | `WindowService.ScrollWindowWithMotion` | `window.scrollWithMotion` | `window.scroll_motion` / none |
| `InputService.StreamScroll` | `WindowService.StreamWindowScroll` | `window.streamScroll` | `window.scroll_stream` / none |
| `InputService.ScrollUntil` | `WindowService.ScrollWindowUntil` | `window.scrollUntil` | `window.scroll_until` / `input.scrollUntil` |
| `CaptureService.GetCaptureImage` | unchanged | `capture.getImage` | `captures.image` / none |
| `TextRecognitionService.RecognizeText` | `TextService.RecognizeText` | `text.recognize` | `recognize_text` / none |
| `RecentFramesService.OpenFrameBuffer` | `FrameBufferService.CreateFrameBuffer` | `frameBuffer.create` | `frames.open_buffer` / none |
| `RecentFramesService.GetRecentFrames` | `FrameBufferService.ListRecentFrames` | `frameBuffer.listRecentFrames` | `frames.recent` / none |
| `RecentFramesService.CloseFrameBuffer` | `FrameBufferService.DeleteFrameBuffer` | `frameBuffer.delete` | `frames.close_buffer` / none |
| `InputService.GetPointerPosition` | unchanged | `input.getPointerPosition` | `input.pointer_position` / `input.pointerPosition` |
| `InputService.CreateMouse` | unchanged | `input.createMouse` | `input.create_mouse` / none |
| `InputService.RemoveMouse` | `DeleteMouse` | `input.deleteMouse` | `input.remove_mouse` / none |
| `InputService.MouseDown` / `MouseUp` | unchanged (exception) | `input.mouseDown` / `input.mouseUp` | `input.mouse_down` / `input.mouse_up` / none |
| `InputService.MoveMouse` | unchanged | `input.moveMouse` | `input.move_mouse` / `input.moveMouse` |
| `InputService.DragMouse` | unchanged | `input.dragMouse` | `input.drag_mouse` / `input.drag` |
| `InputService.StreamMouseMotion` | unchanged | `input.streamMouseMotion` | `input.stream_mouse_motion` / none |
| `InputService.ClickPoint` | unchanged | `input.clickPoint` | `input.click` / `input.clickPoint` |
| `InputService.ScrollPoint` | unchanged | `input.scrollPoint` | `input.scroll` / `input.scrollPoint` |
| `InputService.InputKeyboard` | `PerformKeyboardInputs` | `input.performKeyboardInputs` | `input.keyboard` / `input.keyboard` |
| `InputService.PressKeys` | unchanged | `input.pressKeys` | `input.press_keys` / `input.pressKeys`, `input.keys` |
| `InputService.KeyDown` / `KeyUp` | unchanged (exception) | `input.keyDown` / `input.keyUp` | `input.key_down` / `input.key_up` / none |
| `InputService.HoldKeys` | unchanged | `input.holdKeys` | `input.hold_keys` / `input.holdKeys` |
| `InputService.TypeText` / `PasteText` | unchanged | `input.typeText` / `input.pasteText` | snake_case / same |
| `InputService.PressKey` | **removed** (deprecated) | none | `input.press_key` / `input.key` |
| `ApplicationService.ActivateApplication` | unchanged | `application.activate` | `applications.activate` / `app.activate` |
| `AccessibilityService.FocusText` | unchanged | `accessibility.focusText` | `accessibility.focus_text` / `input.focusText`, `input.axFocusText` |
| `OverlayService.ShowOverlay` / `RemoveOverlay` | unchanged | `overlay.show` / `overlay.remove` | same / none |
| `macos.v1.MediaControlService.GetNowPlaying` | `MediaService.GetNowPlaying` | `media.getNowPlaying` | `macos.media.now_playing` / `mediaControl.nowPlaying` |
| `…Play` / `Pause` | `PlayMedia` / `PauseMedia` | `media.play` / `media.pause` | `macos.media.*` / `mediaControl.*` |
| `…TogglePlayPause` | `ToggleMediaPlayback` | `media.togglePlayback` | same pattern |
| `…NextTrack` / `PreviousTrack` | `SkipToNextTrack` / `SkipToPreviousTrack` | `media.skipToNextTrack` / `media.skipToPreviousTrack` | same pattern |
| `macos.v1.PermissionService.ProbePermissions` | unchanged | `permission.probe` | `macos.permissions.probe` / `app.probePermissions` |

The media id drops the `macos` prefix so a future Windows Runner can serve the
same ids. The Proto service stays in `auv.api.driver.macos.v1` until then.

Invoke-only commands follow rule 5 with their resource:
`screen.clickText` / `screen.waitForText` → `display.clickText` /
`display.waitForText`. `window.clickText`, `window.waitForText`, `overlay.*`
and `scan.*` are unchanged. `input.key` goes with `PressKey`; `input.keys`
becomes `input.pressKeys` only; `input.drag` becomes `input.dragMouse`.

`screen` names only the global logical coordinate space (`ScreenPoint`,
`ScreenRect`). Region capture and text search read one display, so they are
display operations. `TODO(screen-resource)`: add a `screen` resource only when
a cross-display capture or search is approved.

## Daemon and runtime mapping

Run, Runner and RunnerClass move to `auv.api.runtime.v1`, matching their REST
group and discovery group. Their refs (`RunRef`, `RunnerRef`,
`RunnerClassRef`) move with them; `DeviceRef` stays in `auv.api.daemon.v1`.
REST paths do not change, except the Pairing colon form below.

| Old (`auv.api.daemon.v1`) | New | Note |
| --- | --- | --- |
| `DeviceService.ListUserSessions` / `GetUserSession` / `EnsureUserSessionUnlocked` / `EnsureUserSessionLocked` | `UserSessionService`, same method names | `UserSession` is its own resource |
| `DeviceLocalService.Enroll` | `CreateEnrollment` | standard method |
| `DeviceLocalService.RemoveEnrollment` | `DeleteEnrollment` | standard method |
| `DeviceLocalService.ListAudit` | `ListAuditEntries` | items are `AuditEntry` |
| `PairingService.RevokeDeviceCredential` | unchanged | HTTP `/pairing/credentials/revoke` → `/pairing/credentials:revoke` |
| `PairingService.SetPairedDeviceEnabled` | unchanged | HTTP `/pairing/devices/enabled` → `/pairing/devices:setEnabled` |
| `PairingService.UnpairDevice` | unchanged | HTTP `/pairing/devices/unpair` → `/pairing/devices:unpair` |
| `RunService`, `RunnerService`, `RunnerClassService` | package `auv.api.runtime.v1`, method names unchanged | |

Everything else in the daemon API already follows the rules. `SetPolicy` and
`SetPairedDeviceEnabled` keep `Set`: an AIP `Update` would need a
`FieldMask`, which rule 2 does not force.

## Must change together

- `proto/auv/api/**`: RPC moves and renames, `presentation.name`, the new
  `auv/api/runtime/v1/` files, the method docs under `proto/**/docs/` (file
  names are presentation names: `windows.list.md` → `window.list.md`,
  `window.find_text.md` → `window.findText.md`, `input.click.md` →
  `input.clickPoint.md`).
- `crates/auv-api-proto`: `build.rs` schema list and attribute paths,
  `src/lib.rs` module tree, tests.
- `crates/auv-cli/src/runner/local_driver.rs`: service impls regroup; the
  served-services list for reflection.
- `crates/auv-api-server`, `crates/auv-api-client`, `crates/auv-daemon`,
  `crates/auv` client and protocol modules, `crates/auv-cli-invoke` (ids and
  RPC calls), `crates/auv-cli/src/commands/devices.rs`, `auv-trace-atif`
  fixtures.
- `proto/buf.gen.yaml`: OpenAPI `types` list and `M…` options for the runtime
  package; regenerated `proto/openapi/auv-daemon.swagger.json`.
- `js/packages/sdk`: regenerated `src/gen`, `apis/auv/driver.ts`,
  `apis/auv-daemon/*`, tests. Public SDK method names do not change.
- `apps/repl-playground`: `$typeName` maps (driver type names keep
  `auv.api.driver.v1.*`, so only runtime types change).
- Method presentation: `TERMS_AND_CONCEPTS.md` ("Method presentation") and
  `2026-10-09-method-presentation-and-docs.md` define the API name as dotted
  lower_snake_case (`window.find_text`). Rule 5 changes it to lower camelCase
  (`window.findText`); update both.
- Docs: `TERMS_AND_CONCEPTS.md`, skills and READMEs that show invoke ids, the
  `core` group name in
  `2026-07-31-device-run-runner-aggregated-api-design.md` (code uses
  `daemon`).

Compatibility: none, by decision. Clients, daemons and Runners must run the
same release; neko-gpu-1, luoling-windows-11 and the LobeHub helper upgrade
together. Recorded traces keep old gRPC method names as opaque strings.

## Execution

One PR, one release. Generated code and its consumers change in the same
commit.

1. Driver RPC moves and renames, Rust and JS consumers.
2. Runtime package and daemon renames, REST paths, OpenAPI.
3. Operation id unification, `PressKey` removal, method doc renames, naming
   check (api-linter or descriptor test).
4. Docs.

Evidence before merge: `cargo test`, `buf lint`, JS tests, and a round trip
through the daemon on macOS, luoling-windows-11 and neko-gpu-1 for
`window.list`, `display.capture` and `input.clickPoint`.

## Out of scope

- Merging input RPCs (for example `TypeText`/`PasteText` into
  `PerformKeyboardInputs`, or the window scroll RPCs into one). These change
  behaviour, not names.
- AIP resource names, pagination, `FieldMask`, LRO (rule 2).
- `TODO(multi-button-hold)`: `MouseDown` with several buttons. The driver keeps
  one held button per logical mouse (`MouseCoordinator`, `held:
  Option<Held>`), so this is a behaviour change on three platforms.
- Folding `HoldKeys` into `PressKeys` with an optional hold duration. Neither
  MaaFramework nor BetterGI has a timed hold; it is a behaviour change, not a
  rename.

## Decided (2026-11-02)

- `HealthService.Check` is a recorded exception.
- Media verbs: `PlayMedia`, `PauseMedia`, `ToggleMediaPlayback`,
  `SkipToNextTrack`, `SkipToPreviousTrack`.
- Frame buffer is a standard resource: `CreateFrameBuffer`,
  `ListRecentFrames`, `DeleteFrameBuffer`.
- No `build.rs` attribute path check in this wave. The runtime package move
  must update every `.auv.api.daemon.v1.*` attribute path in `build.rs` by
  hand, and the review must confirm the `EnrollRequest.credential`
  `#[serde(skip)]` still applies.

## Implementation notes (2026-11-03)

Decisions taken while applying the tables above:

- **Service files.** `WindowService` and `DisplayService` are declared in
  `window_service.proto` and `display_service.proto`. Their request messages
  stay next to the capability types they share (capture, text, input), and
  `window.proto` / `display.proto` are imported by those files, so declaring
  the services there would be an import cycle (`NOTICE(service-files)`). Other
  file names are unchanged (`text_recognition.proto` holds `TextService`,
  `recent_frames.proto` holds `FrameBufferService`, `media_control.proto`
  holds `MediaService`).
- **Message names follow the RPC** (`buf lint` STANDARD):
  `CaptureDisplayRegionRequest`, `ScrollWindowRequest`,
  `ScrollWindowWithMotionRequest`, `StreamWindowScrollRequest`,
  `ScrollWindowUntilRequest`, `CreateFrameBufferRequest`,
  `ListRecentFramesRequest`, `DeleteFrameBufferRequest`, `DeleteMouseRequest`,
  `PerformKeyboardInputsRequest`, `PlayMediaRequest` (and the other media
  verbs), `CreateEnrollmentRequest`, `DeleteEnrollmentRequest`,
  `ListAuditEntriesRequest`, with matching responses. Event and option
  messages (`ScrollUntilBegin`, `StreamScrollBegin`, …) keep their names.
  The `build.rs` credential path is now
  `.auv.api.daemon.v1.CreateEnrollmentRequest.credential`; the existing
  `local_enrollment_credential_is_excluded_from_daemon_json` test covers it.
- **`UserSessionService`** and its messages live in
  `daemon/v1/user_session.proto`. It has no HTTP bindings
  (`TODO(device-entry-http)`), so it is not in the OpenAPI `types` list.
- **Invoke groups.** Help groups follow the operation id prefix: `display`
  (now with `captureRegion`, `findText`, `waitForText`, `clickText`), `window`
  (now with `scroll`, `scrollUntil`), `input`, `accessibility`, `application`,
  `permission`, `media`. The `app` namespace stays for app-local commands
  such as `app.textedit.document.write`. `input.axFocusText` and the
  `input.keys` alias are removed, so the registry has no alias.
- **Enforcement.** `proto/api-linter.yaml` enables only api-linter's naming
  rules (synonyms, verb-noun, prepositions, method and service case) with the
  recorded exceptions disabled per file, and CI runs it (`proto-naming` in
  `.github/workflows/check.yml`). api-linter has no rule for rule 5, so
  `presentation_name_is_derived_from_the_service_and_rpc` in
  `crates/auv-api-proto/src/lib_test.rs` checks it. The "every discoverable
  RPC has an invoke command or an SDK-only marker" check is not added
  (`TODO(api-linter)` at the test): the invoke registry lives in
  `auv-cli-invoke`.
