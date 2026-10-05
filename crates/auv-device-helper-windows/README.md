# AUV Helper for Windows

This package builds `auv-helper.exe`, the privileged half of Windows existing-session
Device entry. `auv setup windows-helper install` extracts it from `auv.exe` into
`%ProgramFiles%\AUV` and registers it as the `AuvHelper` LocalSystem service.

The Helper Host has no network listener, pairing store, or Device policy. An
ordinary `auv serve` daemon, running as the logged-in user, owns those and
calls the Helper Host over the machine-local `\\.\pipe\auv-helper` pipe for:

- observing the physical console login (only LocalSystem can read its SID);
- storing, probing, and removing the caller's own PIN in the protected vault;
- locking or unlocking the caller's own existing console login.

The host serves an account-scoped request only when its target SID equals the
caller's token SID. The same executable is the one-shot worker that the host
starts inside the selected console session. A direct launch exits with status 2.

See `docs/ai/references/session-api/2026-10-06-windows-helper-daemon-split.md`.
