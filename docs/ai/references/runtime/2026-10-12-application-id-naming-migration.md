# Application id naming migration

Status: owner-approved naming refactor, 2026-10-12. API compatibility aliases
are intentionally omitted, including proto/SDK names. This changes names and
serialized field names, not window listing, selection, activation, or input
delivery behavior.

## Naming decision

AUV identifies a window's application by its **application id**: the opaque id
that `window.list` reports for that application on the same platform (see
"Execution Target" in `docs/TERMS_AND_CONCEPTS.md`). The previous first-party
names said "bundle id", which is a macOS term. Linux drivers already reported
desktop application ids in those fields, so the old names were wrong on every
platform except macOS.

Platform-native names stay intact where they name the platform concept: the
macOS driver's native snapshot fields (`owner_bundle_id`,
`frontmost_app_bundle_id`) and the macOS media `source_bundle_id`. The macOS
`ActivateBundleId` RPC also keeps its name: it accepts only bundle ids.

## Source and API mapping

| Previous | Current |
| --- | --- |
| `Window.app_bundle_id` | `Window.application_id` |
| `AppSelector.bundle` | `AppSelector.id` |
| `App::bundle(..)`, `App::bundle_id(..)` | `App::id(..)` |
| `InputTarget::Application { bundle_id }` | `InputTarget::Application { id }` |
| `ReadinessProbeInput.app_bundle_id` | `ReadinessProbeInput.application_id` |
| `ViewMemory.app_bundle_id` (`auv-view`) | `ViewMemory.application_id` |
| `MemoryWriteInput.app_bundle_id` (`auv-view`) | `MemoryWriteInput.application_id` |
| JS `WindowQuery.bundleId` | `WindowQuery.applicationId` |
| Playground `WindowHandle.bundleId` | `WindowHandle.applicationId` |
| CLI `app:<bundle-id>` help text | `app:<application-id>` |
| `window.list --wide` column `BUNDLE` | `APP ID` |
| `window.capture` report field `Bundle ID` | `Application ID` |

`InputTarget::Application` uses `id`, matching the invoke
`ExecutionTarget::Application { id }`: the enum variant already names the
application, so the field does not repeat it. `AppSelector.id` follows the same
rule next to `name` and `process_id`. `Window.application_id` keeps the full
name because a window has several ids.

## Protocol and persistence changes

Field numbers are unchanged; names and JSON names change.

| Message | Previous field | Current field |
| --- | --- | --- |
| `auv.api.driver.v1.Window` | `application_bundle_id = 4` | `application_id = 4` |
| `auv.api.driver.v1.WindowSelector` (oneof `application`) | `application_bundle_id = 1` | `application_id = 1` |
| `auv.api.driver.v1.InputTarget` (oneof `recipient`) | `application_bundle_id = 2` | `application_id = 2` |
| NetEase `GetNowPlayingRequest`, `PlayRequest`, `PauseRequest`, `TogglePlayerRequest`, `NextRequest`, `PreviousRequest`, `SeekRequest`, `GetStatusRequest`, `PlayDailyRecommendedRequest`, `ScanOptions` | `application_bundle_id` | `application_id` |

JSON consumers see `applicationId` instead of `applicationBundleId` (Protobuf
JSON, the JS SDK and NetEase invoke inputs) and `application_id` instead of
`app_bundle_id` (serde JSON of `Window`, `ReadinessProbeInput` and
`view-memory-v0` records). `AppSelector` serializes `id` instead of `bundle`.

NOTICE: As in the
[domain result naming migration](2026-10-08-domain-result-naming-migration.md),
there are no Serde aliases or dual proto/SDK fields. The `view-memory-v0`
schema label is retained; historical evidence files under
`docs/ai/references/evidence/` keep the emitted `app_bundle_id` and are not
promised read compatibility.

## Deferred

- `TODO(windows-application-id)`: the Windows driver still reports no
  `Window.application_id`. Windows window selectors match an application id
  against the executable name that `window.list` reports as the application
  name. Reporting it as the application id changes `window.resolve` matching,
  so it needs its own owner-approved slice.
- `TODO(application-name-target)`: invoke application targets carry ids, never
  names. An application-name target needs a target-contract extension.

## Guidance for older documents

Dated notes that use the previous spelling carry a migration notice that points
here. Their text is historical; use the current names in new code and docs.

## Verification

Evidence level: source migration, generated bindings, compilation and automated
contract tests. This change does not claim new live platform capabilities. The
commit message and PR description record the exact commands and counts.
