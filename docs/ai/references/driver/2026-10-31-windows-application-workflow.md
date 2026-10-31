# Windows application workflow acceptance

Status: controls inventory accepted; Edge and ISE have scoped edit/save acceptance.
Qt cleanup and recording corrections, plus visible VS Code readiness, await replay.

The owner goal names Edge, Electron, Qt, and WPF application validation.
The prepared cohort is Edge, VS Code 1.144.0, NotepadNext 0.15 with Qt 6.5.3,
and Windows PowerShell ISE. The
[environment and package preflight](https://github.com/moeru-ai/auv/actions/runs/41117632304)
checked installed binaries and portable archive hashes without launching them.

## Controller contract

`background-session-eval app-controls` requires `AUV_ISOLATED_DESKTOP_EVAL=1`,
a fresh output directory, and the four explicit executable paths.
Each application gets an owned fixture file. Chromium and Electron also get fresh profiles.
PowerShell ISE opens a comment-only file with `-NoProfile`; no script is executed.

One Windows job contains each launcher, application, and inspector.
Helpers wait for controller admission before creating an app or calling the Driver.
The controller assigns their process handles to its job before opening that gate.
CreateProcess descendants inherit job membership; no breakaway option is enabled.
Window discovery requires an actual job-membership check and the owned document marker.
Cleanup requires zero active job processes and confirmed exits for every helper root.
A kill request or object drop alone does not satisfy that gate.
See Microsoft's [job ownership contract](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).

The existing Driver captures each window's UIA tree and explicitly requested pixels.
An inspector process retains the observed HWND and PID and has a 25-second deadline.
Tree and capture errors remain separate in the evidence; neither invokes an input fallback.
Desktop witness counts, foreground restoration, pointer restoration, and process cleanup
remain separate from controls and pixels.
The existing keyboard modifier sampler is shared through `DesktopWatch::modifiers_released`.
Application cases require Ctrl, Shift, Alt, and both Windows keys to report released before commands and after cleanup.
This read sends no synthetic key-up events. A down modifier refuses further input instead of masking a stuck-key failure.
Permitted foreground OCR clicks can leave the pointer at their target.
Controller teardown uses the Driver to restore its original pointer only while physical-input counts remain unchanged.
The pre-restoration point, cleanup action, and independent position readback remain distinct from app-delivery facts.

The internal `app-host` and `app-snapshot` subprocess commands are hidden evaluation helpers.
Their Rust entries, `run_controls`, and `run_editor` belong to the unpublished evaluation crate.
This slice adds no root AUV command, Driver capability, Protobuf field, or SDK API.
It fixes existing local `window.clickText` recording: the result builder now publishes the same typed delivery artifact as the recorded caller.
Arguments and result JSON remain the same; the active run gains the missing `auv.driver.input_action_result` artifact.

## Acceptance gates

The controls inventory discovers real selectors before any input is planned.
Its summary always records `workflow_accepted=false`.
Compilation and a successful inventory cannot establish focus, editing, saving, or application compatibility.

The next functional cases must use the same typed focus and input execution surfaces:

- Focus a uniquely resolved editor, enter text, save, and compare the app-owned result.
- Reject an absent or ambiguous focus query with independent zero-input evidence.
- Stop later keyboard input after an owned foreground takeover; preserve partial delivery facts.
- Exercise scrolling and an owned file dialog, with application state and pixels as independent checks.
- Keep CLI, selected Runner, and MCP results, failure codes, cancellation, and durable records comparable.

For text editors, the saved fixture is the independent application result.
The controlled Edge page accepts a trusted Ctrl+S or save-button event.
It downloads the textarea's text to the owned profile's configured download directory.
The page's `save_requested` state is not evidence of persistence; the controller reads the file.
No ValuePattern write, automatic semantic-to-mouse fallback, screenshot equality guard,
or full application-support claim belongs to the inventory.
The retired #429 predicate remains retired.

## Verification

The first source passes Windows compilation and Clippy with existing warnings.
The [first controls task](https://github.com/moeru-ai/auv/actions/runs/41124132593)
at `bea0c294` captured Edge, VS Code, and NotepadNext.
All four application jobs reached zero active processes, all helper roots exited,
and the desktop's foreground and pointer were restored.
Artifact `13010366462` matched SHA-256
`84c3c3c3d7ef63cc5f27d9b7d76295d56ead0613386085acdd1e2165a2442f27` after download.
Independent review confirmed the three captures show the controlled editor or file.

The inventory failed because ISE kept the caption `Administrator: Windows PowerShell ISE`.
The owned process had a visible main window, but the test required its filename in the caption.
Selection now accepts exactly one such ISE main window within the controller's verified job.
Three pure regressions cover that case, duplicate refusal, and retention of marker checks for other apps.

Edge exposed a complete 27-node tree and its named editor.
NotepadNext exposed a complete 71-node tree; its focusable editor region had no name or text value.
VS Code exposed 173 nodes and its editor value, but marked the tree incomplete.
Its maximum depth was 17, below the Driver's depth limit; no traversal limit was relaxed.
The inspector now retains up to three read-only startup snapshots to distinguish transient tree changes.
The [corrected inventory](https://github.com/moeru-ai/auv/actions/runs/41125933697)
at `cbd34975` completed all four applications.
Artifact `13010443371` matched SHA-256
`d2560e1abf8e802d99fa0afe52e124f8c90f14ed639ca2b1998b1ae3e07e7ef1` after download.
All four captures were reviewed independently; job cleanup and desktop restoration passed.
Its trees were complete: Edge 27 nodes, VS Code 175, Qt 71, and ISE 116.
VS Code's initial incomplete tree was transient; no Driver traversal policy changed.

## Edit/save execution

`background-session-eval app-editor` uses the same four paths plus an explicit `--auv` executable.
Its optional `--application` selects `edge`, `vscode`, `notepadnext`, or `powershell-ise`.
Separate isolated desktops can test these cases independently without continuing after another app's uncertain input.
The summary records that selector and the actual application count; one accepted case does not accept the whole cohort.
It invokes AUV's JSON CLI inside the owned job; every step retains stdout, stderr, and its run store.
An absent semantic query must return `not_found` with `not_delivered`, leave the fixture unchanged,
and inject no input before the positive case proceeds.
Edge uses `AUV editor`, VS Code uses the controlled filename, and ISE uses `Text Editor`.
Qt uses an explicitly planned `window.clickText` of the visible controlled marker.
This strategy follows its unnamed UIA editor; it is never a retry after uncertain semantic delivery.
The controller reads focus independently before Ctrl+A, text entry, and Ctrl+S.
For ISE, the payload is comment-only and no execution shortcut is sent.

The saved file must equal the complete expected text, allowing only BOM decoding and CRLF normalization.
A new controls tree, window capture, desktop witness, and process cleanup remain separate evidence.
An uncertain or submitted failure stops the input run and preserves its partial records.
VS Code may reacquire one stale focus only after `not_delivered`, independent zero-input evidence,
and a fresh complete tree with one matching editor. This makes a new query decision; it never reuses a native element.
Two pure regressions reject missing, uncertain, submitted, and unrelated failure receipts.
These cases can accept `edit_save_accepted` while keeping
`workflow_accepted=false`; scrolling, dialogs, loss cases, DPI, and all frontend routes remain separate gates.

The [first edit/save run](https://github.com/moeru-ai/auv/actions/runs/41128561574)
at `7e7dd2a7` passed Edge's absent-query refusal, verified semantic focus, and native text entry.
The page received a trusted save event, but no saved file appeared. The input run stopped before the other apps.
The [download diagnostic](https://github.com/moeru-ai/auv/actions/runs/41174216160)
reproduced that failure on the same source in a fresh isolated desktop.
Its browser database recorded the expected target path, 43 total bytes, zero received bytes,
and interrupt reason 1 (`FILE_FAILED`). The page's state and CLI receipts did not override this failure.
Both runs restored the foreground and pointer, observed no physical input, and verified owned job cleanup.
At that revision, browser file logging retained the next diagnostic boundary; Edge edit/save remained unaccepted.

The [independent app runs](https://github.com/moeru-ai/auv/actions/runs/41181010921) at `86d36a1e`
accepted ISE's comment-only edit/save case. Independent readback verified its 53-byte file,
five actual command completions, and four input-result artifact bodies against their stored hashes and lengths.
VS Code stopped before input at an unavailable provider property (`stale_ui_reference/not_delivered`).
Qt stopped at CLI validation because the case supplied an unsupported `window:` target to `window.clickText`.
Its corrected call selects `app:` plus the owned document's title marker, following the current command contract.
These failures remain separate from ISE acceptance; complete cohort workflows remain open.

The [normalized-path Edge run](https://github.com/moeru-ai/auv/actions/runs/41181310496) at `5b910573`
accepted the same download workflow after serializing the download preference as a native absolute path.
The before/after browser executable hashes matched (`c2451a01...`, version 155.0.4283.62).
The old preference mixed separators; the corrected preference used backslashes throughout.
Independent audit read the 43-byte saved file and verified five actual command completions plus four input-result bodies.
Three Edge captures and three ISE captures were reviewed; their foreground/pointer restoration and zero-physical-input gates passed.
This does not establish the browser's internal failing function or compatibility at another version.

The [four-app replay](https://github.com/moeru-ai/auv/actions/runs/41182331780) at `591ef686`
saved correct files in all four apps. Edge and ISE passed; Qt's task failed its pointer-restoration gate.
Its foreground OCR click left the pointer at the clicked text, and the controller had not restored it.
The Qt reply retained its typed action, but local `window.clickText` recorded only the OCR source artifact.
A pure regression reproduced zero delivery artifacts in the local output path.
The shared output recorder now publishes exactly one typed result for local and recorded callers.

VS Code's file/run-store checks passed, but independent pixels showed the welcome modal covering its buffer.
That is not accepted as a complete visible editor workflow. The case now dismisses the recorded onboarding through
AUV's Escape key and requires a new complete tree without that blocker before typing.
Three Edge and three ISE captures from earlier acceptance, plus the new VS Code/Qt images, were reviewed independently.
The last replay did not exercise the optional VS Code stale-focus reacquisition branch; its eligibility has pure coverage only.
The local-output regression failed before the fix (zero artifacts instead of one) and passed after it.
The complete Invoke unit suite passed 167 tests with one ignored test; six evaluation discovery/readiness/reacquisition tests passed.
Native replay of the recording, pointer teardown, modifier readback, and visible-readiness changes is pending.
