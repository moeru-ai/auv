# Windows core convergence

Status: Phase 0 source audit complete. Phase 1 passed isolated Win32 receiver acceptance on one hosted Windows desktop.

## Current direction (2026-10-30)

PR #429 was closed without merging on 2026-10-28 UTC.
The [maintainer's decision](https://github.com/moeru-ai/auv/pull/429#issuecomment-6481267529)
rejects screenshot comparison as proof that a click's decision premise remains valid.
Equal pixels cannot rule out intervening changes; unequal pixels can concern unrelated content.
Waiting for a new capture also refuses static streams and adds capture latency.
The capture-guard review below is historical and does not approve that design.

Dependent PR #441 is closed and its capture/focus integration branch is retired.
Its 14 passing controlled workflows describe that implementation's behavior only.
They do not establish the rejected premise or complete Phase 2.
The [raw experiment](https://github.com/moeru-ai/auv/actions/runs/40731551833/artifacts/12835702553)
remains available as historical evidence, with SHA-256 `2003532fbe310a5e23b0349a8d7ce2476b2b0321a3e01acbd89252b82cb61cec`.

Semantic focus, cancellation admission, and delivery-progress receipts do not depend on #429.
WGC queued-frame freshness is also an independent capture bug.
Replacement click admission needs a separate owner-reviewed contract for computable state, event coverage, missed changes, and unsupported platforms.
No screenshot predicate, subscription API, or replacement token is approved by this note.

## Baseline and evidence

The audit uses upstream `main@68c4cf7c4a0646fd471f7db48241cffb2fc37329` on 2026-10-28.
Before publication, the implementation rebased onto `main@5b722b027c5270074b11c0296134c29584060f4e` after #406 merged.
The four affected Rust library suites and SDK typecheck passed again on that integration base.
The implementation worktree is separate from the owner's benchmark worktree.
The benchmark worktree contains local MCP, Pixi, and evaluation changes.
Those changes remain outside this work.

| Item | Current fact | Decision |
| --- | --- | --- |
| #409 | Merged as `8bf7e4f1`; three Win32 workflow routes have isolated hosted-desktop evidence. | Reuse its receiver and independent application checks. |
| #425, #428, #430, #431, #432, #433, #434 | Present in the audited main history. | Start from main instead of the older benchmark checkout. |
| Capture guard #429 | Closed without merging; reviewed source was `7cce1d93bbcdd2ccf8405dd943879f6e8528282b`, closed head was `e56b0ecc8f419c0925da9208727952042b71ed55`. | Retire the screenshot predicate. Preserve the review as history; replacement admission requires a new contract. |
| Track A | Local `88807d0e7c9f37b9106b480a3b0f0ba2a75fad35`; not an ancestor of main. | Keep separate. Window-target deduplication is not a Phase 1 dependency and overlaps #429. |
| Track B | Local `6452920493d936c1b084c69cc41dfaa2c0d46f95`; not an ancestor of main. | Reuse and revise the matcher before connecting native focus. |
| Local isolated VM | VirtualBox lists one inaccessible registration and no running VM. | No foreground or DPI experiment on the owner's desktop. |

The [Win32 evidence](2026-10-24-windows-gui-workflow-evidence.md) applies to its recorded hosted Windows configuration.
It does not establish Edge, Electron, Qt, WPF, or other DPI support.
The older [platform issue](https://github.com/moeru-ai/auv/issues/211) contains gaps that later commits already address.
Current source, registry metadata, and dated evidence take precedence over that checklist.

## Phase 1 contract

This is the approved Windows semantic-focus feature from the owner's goal.
The first implementation uses existing `FocusTextOptions`, `AxTextSelector`, `AxFocusResult`, and `InputActionResult` contracts.
It does not add another action-result model.

- Application selection uses the existing Windows window selector.
- UIA roles use programmatic control-type names, independent of the Windows display language.
- Query and path selection both retain role and usable-target constraints.
- Ambiguous best matches refuse selection.
- A fresh UIA element identity check precedes `SetFocus`.
- Focus readback is separate from delivery. Only confirmed keyboard focus can set `verified`.
- Failure does not trigger pointer input, text writes, or an automatic retry.

`AxNode.control_type` retains its localized provider label for existing Apple Music readers.
The new `AxNode.role` field carries the programmatic UIA name.
Snapshots expose truncation, and focus rejects incomplete trees because they cannot establish an unambiguous match.
Windows native focus accepts queries only.
Raw paths remain unsupported until a typed reference can retain a control's identity across calls.
The Phase 1 baseline preserved error categories without structured delivery progress.
The Phase 2 follow-up below adds that progress. The operation never retries.

Track B currently uses localized roles and chooses the first equal-score match.
Its path branch also bypasses the query branch's role and bounds filters.
Its referenced 2026-10-25 audit file is absent from main and from the candidate commit.
This note replaces that missing reference for this implementation.

Before this change, the Runner focus service belonged to `auv.api.driver.macos.v1`.
The shared Runner contract required a namespace migration before Windows could use it.
The implementation includes Rust and JavaScript consumers, discovery, permissions, and protocol checks.

### Runner namespace migration

The existing schema explicitly marks the macOS focus API as experimental and unstable.
The owner approved migration of its existing request, result, and service into `auv.api.driver.v1` on 2026-10-28.
It retains all field numbers and the existing typed Rust options and results.
It changes the RPC method to `/auv.api.driver.v1.AccessibilityService/FocusText`.
The SDK accessors become `runner.accessibility().focus_text(...)` and `runner.accessibility.focusText(...)`.
The old macOS RPC and SDK accessor disappear, so clients and Runners must rebuild together.
An older Runner must return unsupported. The client must not retry through another focus or pointer path.

This is an intentional break of the experimental protocol and SDK namespace.
The implementation updates producers, consumers, discovery, and generated bindings together.
The Buf breaking report must retain the expected namespace violations.

The existing `InputActionResult` wire shape omitted `verified` and restored false on every remote projection.
A failing producer/reader roundtrip reproduced that loss before the repair.
The shared message adds `bool verified = 6`; both Rust projections preserve it.
Older messages omit the field and remain unconfirmed.
This fulfills the existing wire-verification deferral for the owner-named UIA producer and selected Runner consumer.

## Current validation

| Check | Result and evidence level |
| --- | --- |
| Track B regression reproduction | Three tests failed before repair: tied matches, role-filter bypass, and unusable path bounds. |
| Input-result wire reproduction | A confirmed action decoded as unconfirmed before field 6 and both projections changed. |
| `cargo test --locked -p auv-driver-windows --lib accessibility:: -- --skip snapshot_window_captures_root_node_for_a_live_window` | 29 passed; no desktop input. Includes native property-error classification without COM calls. |
| `cargo test --locked -p auv-api-proto -p auv-core -p auv-cli-invoke -p auv-cli --lib` | 412 passed, two existing tests ignored. Transport, parser, result, tracing, and failure-contract evidence. |
| Workspace all-target check | Passed with native property-error refusal and the shared Runner namespace. |
| Buf format, lint, and complete generation | Passed with the pinned Buf 1.72.0 toolchain. |
| Buf breaking against main `68c4cf7c` | Expected failure: the old experimental macOS accessibility schema file was removed. No global breaking rule was weakened. |
| SDK typecheck and driver tests | Passed; 22 driver tests include shared-RPC selection, focus readback, and refusal without an older-endpoint retry. |
| Existing workflow no-desktop unit tests | Nine passed, including input interruption, timeout, and cleanup receipts. |
| Strict driver Clippy | Eleven failures remain in unchanged capture, desktop, device-session, latency, OCR, executor, and WGC code. The new matcher lint was repaired. |

On this Windows host, Buf needs Git's `diff.exe` on the process PATH.
An isolated `BUF_CACHE_DIR` and temporary directory avoid the existing cache's cross-volume rename failure.
These are validation-process settings, not repository or user configuration changes.
OpenAPI generation changed only comment line endings on Windows; the canonical daemon artifact remains unchanged.

## Isolated receiver acceptance

The existing Win32 workflow now accepts `--semantic-focus` and `--routes`.
The default route set remains `rust,invoke,runner`.
Every selected route writes its own record and must complete its own workflow.

```powershell
background-session-eval.exe workflow --output NEW_DIRECTORY --auv auv.exe --rounds 2 --semantic-focus
```

This functional scenario needs an isolated interactive desktop.
It first requires a real missing query to return `not_found` without changing focus, text, pointer, or injected input counts.
Then UIA focuses the control whose current text identifies the fixture's input.
The driver result must name the fixture PID and report confirmed focus.
The receiver must independently report `GetFocus()` on its own UI thread.
The desktop witness must record no pointer movement or injected mouse or keyboard input during focus.
The remaining existing workflow verifies typed text, scrolling, submission, painted pixels, and cleanup.
Failure stops the scenario without pointer fallback or focus retries.

### Isolated receiver evidence (2026-10-28)

The [native run](https://github.com/moeru-ai/auv/actions/runs/40683082392) checked out source
`2d30101922551769eff9ecec2bfa23934dbe20d0` exactly.
Its separate evidence workflow is commit `adfccdf1d39e590a0eb4ad62f46f5e67449c6b50`.
Preflight recorded Windows Server 2025 build 26100, interactive session 2, an accessible input desktop, and one 1024 x 768 display.
Display scaling was not recorded; this is not acceptance for the Phase 4 DPI matrix.

| Route | Original workflow rounds 1 / 2 | Semantic-focus workflow rounds 1 / 2 |
| --- | --- | --- |
| Direct Rust | passed / passed | passed / passed |
| Local Invoke | passed / passed | passed / passed |
| Selected Runner | passed / passed | passed / passed |

Independent inspection of all twelve raw records confirmed:

- Every complete summary reports `passed: true`, with no physical input, quiet violation, or inconclusive quiet operation.
- All six semantic cases retain the fixture PID and programmatic `Edit` role, with one `AxFocus` delivery and confirmed driver readback.
- The receiver independently reports keyboard focus. Text, pointer, and injected mouse/key counts remain unchanged during focus.
- All six missing queries return `not_found` without changing receiver state, foreground, pointer, or injected input counts.
- All twelve workflows verify selection, exact text, scrolling, one submission, and both negative controls.
- All twenty-four accepted captures match the independently reported application state and window identity.
- All eighteen OCR click destinations equal their exact preflight points.
- All twelve fixtures close and remove their stores. All four Runner cleanups verify the owned process tree without a root-only fallback.
- Every summary reports zero input-action retries and restored pointer and foreground state.

The first original Rust round returned one initial blank PrintWindow frame.
The existing bounded read-only retry recovered it on attempt two, before input.
That failed capture remains in the raw record. No input action was retried.
This run does not establish reliable first-frame capture.

The [raw artifact](https://github.com/moeru-ai/auv/actions/runs/40683082392/artifacts/12814680810)
has SHA-256 `180627456f92cae9ff9517a2d2eb4195f376f54358142696d01b1400049ba43e`.
The downloaded archive matched this digest. GitHub retention ends 2026-11-11.

The [standard PR Check run](https://github.com/moeru-ai/auv/actions/runs/40681134116)
passed the full workspace suite on Windows, macOS, and Linux.
The [JS matrix](https://github.com/moeru-ai/auv/actions/runs/40681134076) also passed.
Its first Windows attempt timed out in the unchanged five-second child-connection test.
The exact file passed locally, and the unchanged CI rerun passed.
No timeout or test assertion was weakened.

This evidence applies to native Win32 EDIT query focus on the recorded hosted desktop.
It does not prove general application support, live MCP parity, provider cancellation, or other DPI behavior.

## Historical capture guard review

The reviewed [#429 revision](https://github.com/moeru-ai/auv/commit/7cce1d93bbcdd2ccf8405dd943879f6e8528282b) used a dedicated guarded-click RPC.
An older Runner therefore cannot silently ignore a new request field.
Admission holds the shared desktop input reservation across recapture and delivery.
Its tests cover changed pixels, moved or resized windows, target mismatches, unavailable evidence, and cancellation with zero delivery.
This is a source review, not a new Windows native run.

Exact full-frame comparison can refuse unchanged intended controls because other pixels change.
The final native capture and OS input call are not atomic.
Phase 2 must preserve these limits and must never retry a refused guard through ordinary clicking.

## Phase 2: focus cancellation boundary

The first follow-up binds Windows Runner focus to the existing `run_input_blocking` boundary.
The RPC thread remains available while UIA calls run on one COM-initialized worker.
Dropping the RPC sets that worker's existing input-cancellation signal.
The worker retains its native input reservation until it exits.

Focus admission checks cancellation before the final identity read and immediately before `SetFocus`.
A cancellation during that read must refuse delivery.
Two regressions failed before the change.
An already-cancelled request still read the target.
Cancellation during identity readback still delivered once.
After the change, all 31 focus tests pass.
The 53 Runner boundary tests pass, including a readiness/release test for cancellation before worker completion.
Affected CLI and Windows Driver targets compile.

This is cancellation admission, not an interruption guarantee for synchronous providers.
A provider call already in progress can still block until it returns.
Cancellation after the last check can race with `SetFocus`.
The shared progress contract below records known delivery boundaries; provider deadlines remain deferred at the native call site.
No cancellation result authorizes a retry of an uncertain action.
The macOS native thread contract remains unchanged.

The [worker regression run](https://github.com/moeru-ai/auv/actions/runs/40686205760)
checked out `d7b5494153096f83de32b0f44dd4b00a4bc29ba3` exactly, based on `4242acd5`.
Its evidence workflow is `ae51342290c55a979f06bcb0deaa1b9601c9dd6e`.
Preflight again recorded build 26100, interactive session 2, an accessible input desktop, and one 1024 x 768 display.
All twelve original and semantic workflows passed across the three routes.
Independent inspection confirmed six focus readbacks, six zero-input missing-query refusals, twenty-four matching pixel/state captures, and eighteen exact planned clicks.
All fixture and Runner cleanups passed, with no root-only fallback.
No capture or input action needed a retry in this run.

The [raw worker artifact](https://github.com/moeru-ai/auv/actions/runs/40686205760/artifacts/12815018810)
matched SHA-256 `ec88f3664e272d3048fae01baffa3edcba77cdaa256f7399e687b45763cdda09` after download.
GitHub retention ends 2026-11-11.
These receipts validate the changed native thread path.
Cancellation during a blocked provider read has deterministic unit evidence, not native fault-injection evidence.

## Phase 2: failure progress

Classification: approved feature from the owner's Phase 2 input-recovery goal.
This slice connects Windows query focus to shared Driver errors, Runner rich errors, Invoke output, and recorded command failures.
The names below remain provisional during review.

An error code does not identify whether input already crossed the native boundary.
The existing `KeyboardInputProgress` describes completed batch actions and repetitions.
It cannot describe one UIA call without inventing a keyboard action index.
This slice therefore adds single-action progress while reusing `InputActionResult` for every action fact.

| Native boundary | Progress | Action evidence | Recovery boundary |
| --- | --- | --- | --- |
| Refusal before `SetFocus` | `not_delivered` | No input action | Read current state before another decision. |
| `SetFocus` returns an error | `delivery_uncertain` | One failed AxFocus attempt; unverified | Stop and inspect state. The provider may have acted. |
| `SetFocus` returns S_OK, but readback fails | `submitted` | One submitted AxFocus attempt; unverified | Stop and inspect state. Submission does not prove focus. |
| Focus readback succeeds | Normal success | Existing verified AxFocus result | The requested focus effect is confirmed. |
| No recognizable progress receipt | Unknown | Absent | Stop and inspect state. Absence never means zero input. |

The domain contract uses one `InputActionProgress` enum beside the existing input types.
The two post-delivery variants carry the existing `InputActionResult`.
`DriverError::InputAction` retains the original error as its source and adds this progress.
The producer must preserve an existing post-delivery error when adding outer preflight context.
It must never replace uncertain delivery with `not_delivered`.

Runner errors use `google.rpc.Status` and one typed `InputActionFailure` in `Any`.
The message uses a `oneof`; an absent or unknown case is not `not_delivered`.
The original gRPC category and message remain available.
The shared protocol crate owns this encoding, following its existing Device-entry and capture-guard precedents.
Rust clients project known details into `CapabilityError`; SDK readers use their existing rich-error bytes.
Unknown or malformed details preserve the original failure and leave progress unknown.

Invoke adds optional `input_progress` to `failure_details`.
Its existing command-finished event serializes the complete failure, so progress survives run recording.
No new execution service, action-result schema, or client roundtrip for evidence is needed.

The first producer is Windows `focus_text` only.
Legacy raw-path focus, selection, keyboard batches, pointer operations, and provider deadlines stay outside this slice.
Their existing result contracts remain in place.
Tests must reproduce missing post-delivery facts before the producer changes.
They must check direct error progress, wire roundtrips, Invoke JSON and tracing, SDK decoding, and refusal without retries.

Three producer regressions reproduced lost progress before the native changes.
Both a failed `SetFocus` call and failed readback could report the same permission error.
The follow-up preserves their distinct delivery facts and original causes.
The 32 focus tests pass.
All 423 affected Rust library tests pass, including rich-error decoding and recorded failure progress.
The SDK's 35 Driver, Device-entry, and failure-progress tests pass.
SDK typecheck, changed-file lint, Buf format/lint/generation, and breaking detection against `1c72f553` pass.
The shared SDK status reader replaces the existing Device-entry envelope loop; healthy Device-entry behavior remains covered by its tests.

Rust callers must handle the new `DriverError::InputAction` and `CapabilityError::InputAction` variants in exhaustive matches.
`InvokeFailure` also adds an optional `input_progress` field, so explicit Rust struct literals need that field.
Success result signatures stay unchanged.
The wire addition is compatible: older readers can ignore the new rich-error detail, and missing details remain unknown.
Native post-delivery fault injection is not claimed by these unit and transport tests.
The updated receiver scenario requires an explicit `not_delivered` receipt plus independent zero-input checks for missing queries.

The [isolated Windows acceptance run](https://github.com/moeru-ai/auv/actions/runs/40699276058)
passed at source `927201cdfa18efc1851933e85f93ea6b10651189`, based on `1c72f553a78f4f0e6527b89155f492a84cccefdd`.
The temporary workflow used evidence commit `f5a99fd4e1eb239a12590ecc505345068433323b`.
Independent artifact checks confirmed 12 complete workflows across direct Rust, local Invoke, and selected Runner, each run twice in baseline and semantic modes.
The six semantic refusals retain raw `not_found` / `not_delivered` receipts with independent zero-input checks.
The six successful focus operations retain application-owned focus readback.
All 24 captures matched application state, all 18 pointer deliveries matched their planned coordinates, and all four Runner cleanup receipts passed.
No capture retry or input-action retry occurred; physical input remained zero and system-state restoration passed.

The [raw progress artifact](https://github.com/moeru-ai/auv/actions/runs/40699276058/artifacts/12819959927)
matched SHA-256 `db18a1899b96189bd675e899de75501b9ce5feb8f9364ac9cf2ce724f6174133` after download.
GitHub retention ends 2026-11-11.
The environment was Windows Server 2025 build 26100, interactive session 2, with one 1024 by 768 display.
Actual DPI was not recorded; these receipts do not establish scaled-coordinate coverage.
The source's [Check run](https://github.com/moeru-ai/auv/actions/runs/40699687747)
and [JavaScript run](https://github.com/moeru-ai/auv/actions/runs/40699687353) passed.
At that source, post-delivery refusal had unit and transport evidence only.
The following scenario adds native submission/readback evidence.

## Phase 2: native focus refusal scenario

Classification: test-only, scoped to the owner's failure-recovery acceptance.
The existing semantic workflow now arms its receiver to redirect text-box focus to its own button.
The receiver counts received and redirected focus on its GUI thread and reports its final `GetFocus` state.
After the single focus call fails, the evaluator requires an unverified `submitted` or `delivery_uncertain` receipt with one AxFocus attempt.
It also requires unchanged text, keys, clicks, pointer position, and injected mouse/key counts.
An explicit zero-delivery receipt fails this check because the receiver has independently confirmed a focus side effect.
The scenario adds no fallback, input retry, Driver branch, RPC, or command flag.

The receiver and evaluation binaries pass all-target compilation.
All nine no-desktop workflow regression tests pass; formatting and whitespace checks pass.
The [isolated Windows run](https://github.com/moeru-ai/auv/actions/runs/40704562930)
passed at source `46c2ba723c83fc1b66595d5733d979ee794100ff`, based on `1e80a5fd8b56043195160d773891dcb94239899a`.
The evidence workflow used commit `d7dd586f6ae70c7e547763f2b11496b4b50c22f1`.
Independent artifact checks confirmed all 12 workflows and six post-delivery refusals.
Every refusal retained `backend` / `submitted`, one AxFocus attempt, and `verified: false` through its selected route.
The application confirmed received and redirected focus, with final focus on its own button.
Text, keys, clicks, pointer position, and injected mouse/key counts stayed unchanged during each refusal.
The six missing-query refusals still retained `not_found` / `not_delivered` with zero-input checks.
All 30 captures matched application state; all 18 pointer deliveries matched their planned coordinates.
All fixture and four Runner cleanups passed, physical input was zero, and pointer/foreground restoration passed.
No capture or input-action retry occurred.

The [raw fault artifact](https://github.com/moeru-ai/auv/actions/runs/40704562930/artifacts/12824311947)
matched SHA-256 `e42912c430ba15a4beeceee8313eeef3aa1bbb911cdeb650bcca725407b77d57` after download.
GitHub retention ends 2026-11-11.
The environment remained Windows Server 2025 build 26100, interactive session 2, with one 1024 by 768 display and no recorded actual DPI.
This fixture validates native `submitted` readback failure; failed-HRESULT `delivery_uncertain` still has unit and wire evidence only.
The scenario does not reproduce a blocked UIA provider or an in-flight cancellation.

## Independent Phase 2 repairs

The following proposed PRs are based on `main@d9d2bdcc6a27da0c1a70a17c4b265c56cbd0f7f3`.
They are independent of the focus stack and the retired screenshot guard; this focus branch does not contain their production changes.

- [#443](https://github.com/moeru-ai/auv/pull/443) moves on-demand WGC window/display images onto the existing continuously drained stream.
  A test-only native run returned red after independent paint receipts confirmed yellow.
  At repaired source `366d7e01`, the unchanged first-read regression returned yellow; all 19 WGC capture, COM, and unit checks passed.
  The [repair artifact](https://github.com/moeru-ai/auv/actions/runs/40916911481/artifacts/12915803148)
  matched SHA-256 `3b5ef713f2348452f433833b75a4d9913757bc86cba6c1ebfbe43565d69c1058` after download.
  Latest received pixels do not establish post-request painting or click-premise validity; the separate health sampler remains unchanged.
- [#444](https://github.com/moeru-ai/auv/pull/444) stops global click, wheel, and drag after foreground activation errors.
  Before repair, all three closed-target calls delivered mouse messages to another owned receiver and moved the pointer.
  At repaired source `8656b95a`, all three returned the original error with empty receiver messages and unchanged pointer/foreground.
  Three valid-window controls and 18 existing button/count/modifier cases also passed; five native tests completed.
  The [repair artifact](https://github.com/moeru-ai/auv/actions/runs/40921807996/artifacts/12919061447)
  matched SHA-256 `7d381a995b6c57b523da0610256f1c992bd86402db778f341e0cb9918384ad37` after download.
  This is activation-error admission, not an exact recipient lease or a full-desktop teardown receipt.

Both repair runs recorded build 26100, image `20261021.278.1`, interactive session 2, one 1024 by 768 display, and 96 DPI.
Their dated notes retain complete baseline/fix source identities and raw evidence links.
This does not complete the full Phase 2 acceptance or promote general platform support.

## Application and DPI environment preflight

The independent [environment run](https://github.com/moeru-ai/auv/actions/runs/40733696621)
used workflow `c858d081c31a00e83fe136bef483be2b6314be9b` and checked out source `c7d6b53f`.
It performed read-only environment queries without invoking the retired capture guard.
It recorded build 26100, image `20261021.278.1`, session 2, and one 1024 by 768 display.
An owned hidden per-monitor-v2 window reported 96 DPI; the probe destroyed it and restored its thread context without changing system scaling.

Edge `155.0.4283.62` was present at the checked Program Files path.
VS Code, VLC, qBittorrent, and ILSpy were absent from the common paths checked; the `code` command was also absent.
Windows Desktop .NET runtimes from the 8.0, 9.0, and 10.0 families were available.
These facts describe that hosted image, not application-workflow or scaled-coordinate acceptance.
Recheck application versions and DPI in each later functional run.
The [environment artifact](https://github.com/moeru-ai/auv/actions/runs/40733696621/artifacts/12835068803)
matched SHA-256 `3638b7fc54a57be25659429fd460d686a85f3b5c83e26fdc1b101b87ed78cf5e` after download; retention ends 2026-11-11.

## Remaining acceptance

Phase 1 has isolated receiver evidence for all three selected routes.
The typed Invoke path is available to MCP; no live MCP workflow evidence is claimed here.
Phase 2 remains incomplete after retirement of the screenshot guard.
The independent repairs above cover queued image frames and failed activation only.
Input-lifetime fault evidence, exact recipient leases, and an owner-reviewed replacement admission contract remain incomplete.
Phase 3 needs repeatable application-owned checks for each named toolkit.
Phase 4 needs disposable single-display environments at 100%, 125%, and 150% DPI.
Phase 5 needs equivalent direct, Invoke, selected Runner, and MCP records.

ValuePattern writes, multi-display work, and advanced overlay work remain deferred by the owner's goal.
No new platform-support claim follows from this audit or from compilation alone.
