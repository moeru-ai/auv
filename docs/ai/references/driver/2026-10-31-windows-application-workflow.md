# Windows application workflow acceptance

Status: test-only controls inventory; functional workflows are not accepted yet.

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

The internal `app-host` and `app-snapshot` subprocess commands are hidden evaluation helpers.
Their Rust entries and `run_controls` belong to the unpublished evaluation crate.
This slice adds no root AUV command, Driver capability, Protobuf field, or SDK API.

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
The controlled Edge page still needs its app-owned save/readback path.
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
Input and edit/save acceptance still require new evidence.
