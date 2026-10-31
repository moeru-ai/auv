# Windows posted-wheel progress and fallback

Status: reproduced and implemented; corrected native acceptance pending. Base: PR #452 revision
`2a6640ce02cb32151a12932b99906ca3c709fb65`.

This bug-fix slice covers posted wheel progress and candidate fallback. Windows
posts the vertical axis before the horizontal axis. The current candidate loop
discards an error and can resend both axes through foreground input, even when
the vertical message was already accepted. A partial or unknown delivery must
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

BackgroundPreferred timed/live schedules still need a retained chosen-route
lease, including when their first sample selects foreground fallback. This
slice prevents per-call partial replay; it does not claim those whole-gesture
leases or background root/child identity and minimized-window admission.
