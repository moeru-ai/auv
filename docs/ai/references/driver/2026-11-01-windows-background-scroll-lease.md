# Windows background-policy scroll lifetime

Status: bug-fix investigation. No new capability acceptance yet.
Base: PR #454 head `0a92ed6e6789927ce1f9e0b4dc1d32a6b3cd8727`.

The owner goal requires input leases across multi-step operations and refusal
after target loss. The common scroll executor pins the first selected delivery
path. Windows currently retains a recipient only for ForegroundPreferred at the
outer operation. BackgroundPreferred can select foreground but later samples
admit it again. Posted samples also lack a retained root/child lifetime.

This slice covers single-call, timed, and live wheel input under background
policies. It must retain the original root/child recipient, refuse minimized,
hidden, cloaked, or destroyed targets, and keep a selected foreground recipient
through samples and waits. Already submitted or unknown input must stop.
No screenshot-equality predicate or retired #429 API is involved.

The isolated `window-background-scroll --output DIR` evaluator reuses the
existing scroll receiver and fault protocol. It adds background-only and
background-preferred posted cases, plus explicit ForegroundHid selection through
BackgroundPreferred. That last profile isolates recipient lifetime; it does not
claim a real queue-rejection fallback. The separate posted-scroll evaluator
already covers the zero-post fallback boundary.

The baseline must check application wheel counts and viewport movement,
independent injected-input/foreground/pointer receipts, and owned cleanup.
Posted cases begin with B foreground and must leave it there. Every fault is
applied by A's GUI thread after an acknowledged sample or while the stream is
idle. Native tests run only on the disposable hosted Windows desktop.

Frame/coordinate freshness, direct posted click/held input, pre-admission window
generations, higher DPI, and complete Invoke/Runner/MCP workflows remain outside
this slice. They remain requirements of the full owner goal where applicable.
