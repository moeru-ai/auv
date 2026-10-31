# Windows posted-wheel progress and fallback

Status: test-only native reproduction in progress. Base: PR #452 revision
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
