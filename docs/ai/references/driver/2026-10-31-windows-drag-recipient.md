# Windows complete-drag recipient lifetime

Status: test-only reproduction in progress, based on PR #451 revision
`1f112bf145ddb23583b7aac83d0fa8e8cf1d29af`. No native result is accepted yet.

The assigned bug-fix slice is `WindowInput::drag` with
`InputPolicy::ForegroundPreferred`. A complete gesture must retain its native
recipient from admission through release. Foreground loss, including A-B-A,
minimization, and closing must stop subsequent moves. Cancellation must release
only the button owned by this gesture and retain any submitted input prefix.

The existing common mouse coordinator already owns desktop admission for a
complete drag. Native recipient admission must occur inside that admission;
wrapping the coordinator in a second desktop reservation would deadlock.

The `background-session-eval window-drag --output DIR` fixture requires
`AUV_ISOLATED_DESKTOP_EVAL=1` and a fresh output directory. It starts two owned,
overlapping Win32 applications. The recipient changes foreground only after
processing a held move. Its GUI thread records the pad and cursor at that
boundary. A cancellation observer waits for the same application receipt.
Independent desktop hooks record physical and injected input. The actor's
button state is read before a separate controller recovery release. Application
state, captures, terminal close receipts, and process/window cleanup are saved.

No test runs against the user's desktop. Native tests use an isolated hosted
interactive Windows job. Local checks only compile or exercise pure/fake IO.

The slice does not change cross-call `mouse_down`/`mouse_up` target semantics,
background drag policy, capture/coordinate freshness, physical input exclusion,
or native insertion atomicity. Those boundaries need their own approved evidence
and are not inferred from this fixture.
