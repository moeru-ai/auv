# Windows complete-drag recipient lifetime

Status: fix implemented; native acceptance pending. Based on PR #451 revision
`1f112bf145ddb23583b7aac83d0fa8e8cf1d29af`. No corrected native result is accepted yet.

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

## Reproduced failure

[Baseline run 41210680592](https://github.com/moeru-ai/auv/actions/runs/41210680592)
checked out test-only revision `dc4482ff35cb511396fedf0eb7b506e8850b2c02`.
The workflow also verified an empty production-crate diff against the base.
Job `124564805589` reproduced five failures in six cases. All four loss cases
continued to the final pointer position and returned success. Closing the
recipient let the alternate window process five held-mouse moves. Cancellation
returned a plain error with no prefix receipt. The watchdog eventually released
that button; this is not evidence of a permanently stuck native button.
All twelve owned receivers exited and destroyed their windows. Physical input
counts stayed zero, and the original foreground, pointer, and modifiers were
restored. The previous 27 batch-scroll, 22 click/scroll, 15 keyboard, and 34 pure
regressions passed in the same isolated job.

The retained artifact is `13046033258`, SHA-256
`9db668cfb70513b4bb31adec767ef9410d2097e82d73af6697091e30f8f873f4`.
Baseline images `drag/aba/recipient.png` and `drag/close/alternate.png` were
reviewed alongside the application counters and independent desktop witness.

## Implementation and API boundary

`MouseBackend::with_motion` is a provided, object-safe callback boundary. It
keeps native state on the admitting thread after pure geometry, clock, and
held-button validation. The default calls the supplied motion. The provided
`check_motion` defaults to success. Existing backend implementations need no
new methods. Native checks run outside the coordinator state mutex. Motion
waits check at most 25 milliseconds apart, including sparse schedules.

Windows selects its native Window recipient explicitly for foreground drags.
It retains process identity, foreground/destroy history, and the submission gate
through press, samples, and owned release. The posted Window route remains a
different policy. Click and scroll callers retain the same desktop-before-native
lock order after desktop admission moves out of their shared native helper.

An owned button-up bypasses recipient/cancellation refusal but still checks the
native insertion count. Until a release is confirmed, failed cleanup retains
the logical hold and blocks reuse. Explicit recovery remains available. If a
typed submitted or uncertain prefix exists, failed cleanup keeps
the original cause and returns `InputActionProgress::DeliveryUncertain` with a
failed release attempt. A successful movement followed by failed cleanup also
returns an uncertain receipt. A cause without native evidence remains unknown.

The public `WindowInput::drag`, `InputApi::drag_mouse`, and
`MouseCoordinator::motion` signatures are unchanged. No Protobuf or Runner API
changes. Runtime callers must inspect `DriverError::InputAction`; a submitted or
uncertain prefix must not be retried without checking application state.

The evaluator adds only `window-drag --output DIR`. It tests success, four
post-move target-loss modes, post-move cancellation, and seven pre-delivery
refusals. The production `auv` CLI grammar is unchanged.

The Windows eval library exports
`desktop_windows::window_drag::run(&Path) -> anyhow::Result<bool>`.
Its public receiver `Raw` adds `frame_move: u64`, `frame_up: u64`, and
`frame_move_at: Option<[i32; 2]>`. They count held moves and button-ups handled
by the frame and retain the last held-move position. Complete struct literals
must supply these fields, or use `..Default::default()`. Native JSON must contain
the new counters; missing counters are not treated as zero delivery. The public
watch `InjectedEvent` adds `mouse_message: Option<u32>`. No physical coordinates,
key codes, or text are collected by that field.

The first corrected run
[41212119240](https://github.com/moeru-ai/auv/actions/runs/41212119240)
passed 12/13 drag cases and all prior regressions. Its A-B-A fault reached B,
and the Driver stopped and released correctly, but A could not reclaim
foreground after the owned up reached B. The receipt said
`returned_owner=false`, so this run is not accepted as A-B-A evidence.
The fixture now asks the current foreground B to hand back A with its existing
owned-window handoff message. That bounded call runs without fixture mutexes;
keyboard, click, and scroll faults retain their existing behavior. Artifact
`13047145958`, SHA-256
`0dbfb20ed231101e2a395fbab63b5ca90d0712089fb73c9ffcf6332bb7cae54d`,
retains the failed fault receipt. Production code is unchanged from
`93696abc44666568720539a77ae0795beaae3665`.

The first handoff run
[41212882633](https://github.com/moeru-ai/auv/actions/runs/41212882633)
also retained 12/13 passes. Its final application state proved A was foreground
and B had yielded once, while the callback's immediate return flag was false.
[Microsoft explains cross-queue activation is asynchronous](https://devblogs.microsoft.com/oldnewthing/20161118-00/?p=94745).
The fixture now finalizes a pending return only from a later, read-only
foreground query on A's GUI thread. Acceptance also requires A foreground and
B's handoff count; no activation or input is retried by this readback. Artifact
`13047695164`, SHA-256
`a47bcb4371e011a6a7f0a19763d7b0e3cfda9f345bdf49c272b3416b688687f7`,
retains the unaccepted instantaneous receipt.

The queue-readback run
[41213714784](https://github.com/moeru-ai/auv/actions/runs/41213714784)
passed A-B-A, but close was rejected because B handled one held move despite a
frozen pointer and only five injected events. Its artifact `13046569017` has
SHA-256 `707bb0e23849d37b5b26b173eca26c5ae30db4a9293e9b13dfe4ed9f4bde46b5`.
[Windows can generate mouse-move messages after window changes](https://devblogs.microsoft.com/oldnewthing/20031001-00/?p=42343).
The witness now retains injected mouse message kinds. An interrupted gesture
must have exactly three injected moves, one down, and one up; no extra event is
accepted. Event kinds/counts are compared because millisecond timestamps do not
encode native ordering. B must still receive no down and start no drag. For
close only, one same-position frame message is allowed if its coordinate equals
the fault point and that exact injection-count evidence also passes. This does
not promise suppression of system-generated messages or atomic native input.

[Run 41214950459](https://github.com/moeru-ai/auv/actions/runs/41214950459)
stopped before GUI execution. The pure motion fixture mistook a brief watchdog
mutex owner for a lock retained by its caller. A forced foreign-owner local
reproduction produced the same `WouldBlock` failure before the test changed.
The fixture now reacquires normally; a completion channel detects a caller-owned
deadlock within two seconds. No production code changed. All 113 common tests
passed afterward. Artifact `13047504606`, SHA-256
`e66948ee7f09dd9c30c46f55c7dfc5a0b363ec8bc337ad90a99d3e8fff23d631`,
retains the exact failed pure-test log and is not GUI evidence.
