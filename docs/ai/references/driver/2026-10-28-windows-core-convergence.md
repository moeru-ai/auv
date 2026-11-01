# Windows core convergence

Status: Phase 0 source audit complete. Phase 1 passed isolated Win32 receiver acceptance on one hosted Windows desktop.

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
| Capture guard #429 | Closed without merge on 2026-10-30; dependent #441 closed. | Semantic focus does not depend on its screenshot predicate. Phase 2 admission proceeds independently. |
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
Readback failures preserve existing error categories but do not yet carry a structured input-delivered flag.
The operation never retries. Phase 2 owns the shared post-delivery progress contract.

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

## Capture guard review

> [!NOTE]
> Roadmap update (2026-10-30): PR #429 was closed without merging, and dependent #441 is closed.
> This semantic-focus stack is independent of its screenshot predicate. Phase 2 admission and input-lifetime work will proceed as an independent slice rather than reusing #429.

The [#429 head](https://github.com/moeru-ai/auv/pull/429) uses a dedicated guarded-click RPC.
An older Runner therefore cannot silently ignore a new request field.
Admission holds the shared desktop input reservation across recapture and delivery.
Its tests cover changed pixels, moved or resized windows, target mismatches, unavailable evidence, and cancellation with zero delivery.
This is a source review, not a new Windows native run.

Exact full-frame comparison can refuse unchanged intended controls because other pixels change.
The final native capture and OS input call are not atomic.
Phase 2 must preserve these limits and must never retry a refused guard through ordinary clicking.

## Phase 2: focus cancellation boundary

The first follow-up binds Windows Runner `FocusText` to the existing `run_input_blocking` boundary.
The scope is intentionally narrow: this protects the Windows Runner `FocusText` RPC route, without extending cancellation contracts to other direct native entry points (such as `focus_node()` or `select_node()`).
The RPC thread remains available while UIA calls run on one COM-initialized worker.
Dropping the RPC sets that worker's existing input-cancellation signal.
The worker retains its native input reservation until it exits.

Focus admission checks cancellation early at entry and before UIA tree walking, before the final identity read, and immediately before `SetFocus`.
A cancellation during that read must refuse delivery.
Two deterministic regressions failed before this change: an already-cancelled request still read the target, and cancellation during readback still delivered once.
After the change, all 32 focus tests pass (with dynamic property shift tolerance on focus covered alongside uncancelled signals).
The Runner boundary tests pass, verifying cancellation signaling before worker release and ensuring native execution and owned resources are retained until worker completion.
Affected CLI and Windows Driver targets compile.

This is cancellation admission, not an interruption guarantee for synchronous providers.
A provider call already in progress can still block until it returns; synchronous COM/OS calls cannot be forcibly aborted via Tokio timeouts.
Cancellation after the last check can race with `SetFocus`, and delivery errors still lack structured progress.
At the protocol boundary, cancellation occurs when the outer RPC future is dropped/aborted; because the transport response stream is already terminated, no artificial gRPC `CANCELLED` status mapping is exposed in `driver_status` (avoiding fragile string matching across error boundaries).
Dedicated end-to-end cancellation categorization remains deferred to a future cross-platform error contract slice.
Similarly, test evidence verifies in-process worker task drop and cancellation propagation; it does not simulate a full live network RPC disconnect against an active provider.
Those Phase 2 gaps remain marked at the call sites.
No cancellation result authorizes a retry of an uncertain action.
The macOS native thread contract remains unchanged.

The Phase 1 native receipts above predate this worker change and reflect historical HEADs.
New native acceptance is required for the changed Windows Runner path.

## Remaining acceptance

Phase 1 has isolated receiver evidence for all three selected routes.
The typed Invoke path is available to MCP; no live MCP workflow evidence is claimed here.
Phase 2 depends on independent admission and separately scoped input-lifetime work (no longer blocked on #429).
Phase 3 needs repeatable application-owned checks for each named toolkit.
Phase 4 needs disposable single-display environments at 100%, 125%, and 150% DPI.
Phase 5 needs equivalent direct, Invoke, selected Runner, and MCP records.

ValuePattern writes, multi-display work, and advanced overlay work remain deferred by the owner's goal.
No new platform-support claim follows from this audit or from compilation alone.
