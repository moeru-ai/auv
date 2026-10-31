# Windows posted-wheel progress and fallback

Status: scoped native acceptance passed. Base: PR #452 revision
`2a6640ce02cb32151a12932b99906ca3c709fb65`.

This bug-fix slice covers posted wheel progress and candidate fallback. Windows
posts the vertical axis before the horizontal axis. At the base revision, the
candidate loop discarded an error and could resend both axes through foreground
input, even when the vertical message was already accepted. A partial or unknown delivery must
stop; only a confirmed not-delivered refusal can permit another route.

The isolated `window-posted-scroll --output DIR` evaluator pauses its owned
Win32 application's GUI queue with named events. The independent controller
fills the queue until the real `PostMessageW` quota error, then asks that GUI
thread to remove exactly one filler. The actor can queue one axis before the
second post fails. Another case leaves no free slot. No production fault switch
or registry change is used. [PostMessageW queue semantics](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew).

Three event acknowledgements establish pause, optional one-slot removal, and
release after filler removal. Event ownership releases both waits on early
failure. GUI waits have bounded automatic release, which final acceptance must
reject. Application replies acknowledge actual wheel messages; injected input,
foreground, pointer, modifiers, captures, and process/window cleanup are checked
separately. Native execution is limited to an isolated hosted desktop.

Background click/held-message progress and recipient leases are separate next
slices. This change must not claim those capabilities, atomic native insertion,
coordinate freshness, scaled DPI, or complete cross-frontend workflows.

## Baseline and regression

[Native baseline 41221707553](https://github.com/moeru-ai/auv/actions/runs/41221707553),
job `124597445079`, checked out test-only revision
`1f79de21db85cb82c3cf00f64068b016b577635a`. The workflow verified an empty
production-crate diff against the base. The real queue limit was 10,000.
Five of six new cases failed. Background-only partial delivery produced one
vertical and no horizontal message, but returned generic Unsupported. After
the same prefix, BackgroundPreferred returned success, injected three global
events, changed foreground, and waited for the fixture's automatic release.
Its final application did not acknowledge an extra axis, so this is evidence of
an unsafe retry and false success, not proof of doubled application scrolling.
Zero-post failures also lost their not-delivered cause. The original foreground
fallback blocked an already-foreground paused GUI until its automatic release.

The prior 77 GUI cases and 56 selected pure regressions passed. All twelve new
receivers exited and destroyed their windows. Physical input was zero, and the
desktop was restored. Artifact `13050522055` was downloaded and verified:
SHA-256 `b08dd2ee38109290d8923f1da6020c5f5eb62689a707973212abeb0a073eb591`.

Two pure IO-boundary tests reproduced missing posted-prefix progress and the
foreground retry before production behavior changed. Both now pass. Additional
cases cover zero-post refusal, cancellation before/after the first post,
unknown/uncertain failure, invalidated targets, missing fallback candidates,
and preservation of prior attempt diagnostics. The public pre-cancelled
background scroll boundary also reports NotDelivered.

## Result contract

The posted axis loop counts only calls accepted by PostMessageW. A first-call
refusal is `InputActionProgress::NotDelivered`; a later refusal or cancellation
retains `Submitted` with a WindowTargetedWheel action and diagnostic prefix
count. A queued message never proves application effect. Per-axis fields are
deferred until a shared scroll result has an approved producer and consumer.

The candidate policy stops on Submitted, DeliveryUncertain, missing progress,
target loss, and cancellation. Only a NotDelivered backend/unsupported refusal
permits another route from the caller's strategy. No foreground candidate means
the original refusal is returned. Permitted foreground fallback uses the existing
window-recipient guard, including activation refusal and native progress. Prior
failed attempts are retained on typed partial or uncertain errors.

Public Driver signatures, production CLI syntax, Protobuf, and Runner messages
are unchanged. Callers that matched generic Unsupported text must handle the
existing `DriverError::InputAction` and inspect application state before retrying
a submitted or uncertain operation. The evaluator adds
`background-session-eval window-posted-scroll --output DIR` and the Windows eval
library exports `desktop_windows::window_posted_scroll::run(&Path) -> anyhow::Result<bool>`.

`BackgroundSession::scroll` keeps its existing `Outcome::Failed` shape. It carries
native progress in the failure message and never retries automatically. Structured
progress fields require a separate approved public Outcome and consumer migration.
The existing Invoke Driver-error conversion retains typed progress for normal
failures. Its cancellation branch still returns unknown progress. Native Invoke,
Runner, and MCP parity for this queue-limit case remains untested.

BackgroundPreferred timed/live schedules still need a retained chosen-route
lease, including when their first sample selects foreground fallback. This
slice prevents per-call partial replay; it does not claim those whole-gesture
leases or background root/child identity and minimized-window admission.

## Accepted native evidence

[Run 41223224740](https://github.com/moeru-ai/auv/actions/runs/41223224740),
job `124601918758`, checked out `ca02962baf3dde5066954db57cb1813733963df0`.
Workflow revision: `dfaaa18cd722b18f541da8c6c21370c7bfc55932`.
Artifact `13050862646` was verified before extraction:
SHA-256 `1b4f975768d4d8461e3c70e195e9437deb1fd2a5379b836f650fd86423e0a065`.

Independent audit accepted all six posted cases, 77 prior GUI cases, and 69
selected pure regressions. Partial cases delivered one vertical and zero
horizontal messages, retained Submitted, and used no global input or foreground
change. Zero-post refusals retained NotDelivered. The permitted foreground
fallback delivered each axis once, used exactly three injected mouse events,
and retained its failed background attempt. Every queue gate was released by the
controller after the actor returned. No queue gate used automatic release.

All 166 owned receivers exited and destroyed their windows. Physical input was
zero; modifiers were released and the original pointer/foreground were restored.
Ten inherited minimized/closed captures were explicitly refused. All 126 capture
contents were reviewed: 115 exact hashes match reviewed contents; eleven captures
have four new contents, all inspected. The main application oracle proves actual
vertical scrolling and axis-message receipt. Horizontal viewport support for
arbitrary applications is not inferred from its message counter.

This replay also exercised the inherited drag-close witness classifier: B
received one same-position frame message, while the independent hook recorded
only the expected five injected events and the cursor stayed at the fault point.
The prior unexercised marker now points to this measured case.

The [permanent manifest](evidence/2026-11-01-windows-posted-scroll-progress/acceptance.json)
retains application/gate state, typed progress, source/workflow/binary hashes,
capture hashes, visual-review groups, and prior regression acceptance. The raw
hosted archive has fourteen-day retention; the ignored local archive is retained.

Local checks passed: thirteen new pure tests, existing invalid-window/public
scroll regressions, 167 Invoke tests with one ignored, and 29 eval library tests
with one ignored. Affected all-target checks, Windows all-features check, clippy,
format/diff checks, evaluator help, and production input.scroll help passed.
Clippy retains existing warnings. Final changes after the native revision move
the test module, remove a redundant struct update, update evidence comments, and
record docs/manifest; delivery behavior is unchanged.

The full owner goal remains incomplete. This is a directly measured Win32 Driver
slice at 96 DPI, with no new broad toolkit or frontend support claim.
