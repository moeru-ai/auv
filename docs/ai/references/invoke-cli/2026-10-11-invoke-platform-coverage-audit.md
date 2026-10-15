# Invoke platform coverage audit

> Migration notice: Windows `app.activate` has since been wired through the
> shared `ApplicationActivationResult` and
> `auv.api.driver.v1.ApplicationService/ActivateApplication`;
> `ApplicationControl::activate_process_name` and `ProcessActivationResult` no
> longer exist. The text below records the state when it was written.

Status: dated snapshot of [#211](https://github.com/moeru-ai/auv/issues/211) against the source at
`c24b8222` (2026-10-11). The live registry is the current answer: `auv invoke --help` and
`auv invoke <COMMAND> --help` show each command's cells, and MCP `x-auv-commands[].support` carries
the same declaration ([Command Support](../../../TERMS_AND_CONCEPTS.md#command-support), [mechanism](2026-10-11-invoke-command-support.md)).
This note records what that answer was on the day #211's checklist was brought up to date, and what
Windows still lacks.

## Result

- The issue's count of 37 command IDs matched the registry when it was written (checked at
  `5eb05d0b`, the last commit on `main` before #216). The registry has 42 now, and the checklist fell
  behind through work that landed after it (below). This audit did not re-check each older line.
- Windows moved from 13 of 37 IDs with a direct local path to 24 of 42. Of the 38 commands that
  drive a desktop, 21 have a path on Windows in full or in part.
- A path existing is not the same as a path having been run. Of the 21 Windows desktop commands
  with a path, the support matrix holds a live record for 8: the four `window.*` observation
  commands, `input.pointerPosition`, `input.scrollPoint`, `input.scroll` and `input.scrollUntil`.
  The other 13 are source and test claims only.
- Every remaining Windows gap has a different blocker. None is "write the missing code" alone:
  see [what blocks each gap](#what-blocks-each-windows-gap).

## What landed after the issue was written

#211 was last edited at 2026-10-01 18:36 (+08:00).

| Change | When | Effect on the checklist |
| --- | --- | --- |
| #216 Linux observation commands | 2026-10-01 23:43 | Linux `window.list`, `window.capture`, `screen.captureRegion`, `screen.findText`, `screen.waitForText` |
| #218 `input.drag` | 2026-10-03 | A command the checklist lacks |
| #238, #243 `input.scroll`, `input.scrollUntil` | 2026-10-06 | Two commands the checklist lacks |
| #257 `input.pointerPosition`, `input.scrollPoint` | 2026-10-10 | Two commands the checklist lacks |
| #336 Windows `window.*` | 2026-10-11 | Windows `window.list`, `window.capture`, `window.findText`, `window.waitForText` |
| #337 Windows `screen.*` text | 2026-10-11 | Windows `screen.findText`, `screen.waitForText`, primary display only |

## Counts

The direct local path exists in full or in part for the following. The 38-command figure drops the
two aliases (`input.axFocusText`, `input.pressKeys`) and the two fixture commands (`scan.frame`,
`scan.coverage`), which is the separate count #211 asked for. These are a dated snapshot from
`auv invoke --help`, not a published figure, and no API returns them.

| Platform | `supported` | `partial` | `unsupported` | Of the 38 desktop commands | Of all 42 IDs |
| --- | --- | --- | --- | --- | --- |
| macOS | 37 | 0 | 1 | 37 | 41 |
| Linux | 9 | 10 | 19 | 19 | 22 |
| Windows | 10 | 11 | 17 | 21 | 24 |

`input.scrollPoint` is the one command macOS lacks: the driver exposes only window-owned scrolling.
`input.pointerPosition` is unsupported on Linux, because the portal cannot report the pointer position.

## Windows, command by command

A cell is a statement about source. "Evidence" names the highest level the
[support matrix](../../../SUPPORT_MATRIX.md) claims for the Windows path; `none claimed` means the
matrix makes no Windows claim for it, whatever the source shows.

| Command | Windows | Limit | Evidence |
| --- | --- | --- | --- |
| `display.capture`, `display.list`, `screen.captureRegion` | supported | | none claimed |
| `screen.findText`, `screen.waitForText` | partial | primary display only | none claimed: no live run |
| `window.list`, `window.capture`, `window.findText`, `window.waitForText` | supported | | `live-validated` on 2026-10-11, one receiver the harness owns ([reference](2026-10-11-windows-window-commands.md)) |
| `input.typeText`, `input.key`, `input.keys`, `input.holdKeys`, `input.keyboard` | partial | foreground only; `input.keyboard` also lacks the `paste_text` action | none claimed |
| `input.pointerPosition` | supported | | `live-validated` 2026-10-09 |
| `input.moveMouse` | supported | | none claimed |
| `input.scrollPoint` | supported | | `live-validated` |
| `input.clickPoint`, `input.drag` | partial | window messages; an application may ignore them, by toolkit | none claimed through invoke; the posted-message mechanism is `live-validated` through the Rust background session only |
| `input.scroll`, `input.scrollUntil` | partial | same | `live-validated`; see the matrix rows for window scroll and scroll until, which name their limits |
| `scan.frame`, `scan.coverage` | supported (fixtures) | | `tested` |
| the other 17 | unsupported | next section | none |

## What blocks each Windows gap

The question for each is whether the driver already has what the command needs, and if so what
stops the command from using it. The source of each answer is named, so a reader can check it.

| Command | In the Windows driver today | What blocks the command |
| --- | --- | --- |
| `mediaControl.*` (6) | `SmtcMediaManager` with `current_session`, `now_playing`, and play, pause, toggle, next and previous on a session (`media.rs`) | The commands return `auv_media_macos::output::{NowPlayingOutput, MediaControlOutcome}`. There is no cross-platform media result, so a Windows path needs one (shared in `auv-driver`, or a per-platform result) before it can be wired. A design choice, not missing code. |
| `input.pasteText`, and the `paste_text` action of `input.keyboard` | clipboard `snapshot`, `restore`, `set_text` and a paste key press, but no `paste_text(PasteTextOptions)` capability as macOS and Linux have | Assemble that capability in the Windows session first. It restores the user's clipboard, so a live check needs a target the harness owns. |
| `input.focusText`, `input.axFocusText` | `accessibility::snapshot_window` and `focus_node` | Not audited: whether the Windows tree carries the visible text the command matches on. |
| `window.clickText` | `BackgroundSession`, a window-targeted click, reachable only from Rust | `TODO(invoke-window-click-text-windows)`: an owner decision on how an invoke command reaches a session. |
| `screen.clickText` | foreground `click_at` | `TODO(invoke-screen-click-text-windows)`: the click moves the real cursor, and the check cannot be run on a desktop someone is using. Evidence, not code. |
| `app.activate` | `ApplicationControl::activate_process_name` | A product decision: the command raises a window, and the Windows work so far, the background session, is built around never raising one. |
| `app.probePermissions` | `permission().probe()` | Not audited: whether `WindowsPermissionProbe` fits the shared result the command reports. |
| `overlay.*` (5) | `auv-driver-overlay-windows`, and live overlay following driver input | The commands are gated to macOS with the `overlay` feature. They draw on the desktop, so evidence cannot be taken without drawing on someone's screen. |
| Targeted keyboard delivery | none: the background session answers `unsupported` for keyboard | New driver work, not wiring. |

The gaps that need only evidence, not code, are `screen.findText` and `screen.waitForText` on the
primary display, the toolkit limits of `input.clickPoint` and `input.drag`, and the live Windows
matrix #211 lists as a separate task.

## Candidate next slices

Candidates only; none has an owner's approval.

1. `mediaControl.*` on Windows, after a decision on the media result. It is the largest block of
   commands with a driver capability already behind them. `nowPlaying` reads state and can be run
   safely; the five control commands act on whatever the user is playing.
2. `input.pasteText` on Windows: the `paste_text` capability, then the command.
3. Live evidence for `screen.findText` and `screen.waitForText` on Windows, when a desktop is free.
4. The Runner route, a second set of cells (`TODO(invoke-support-runner-route)`).

## Follow-up on 2026-10-12

The 13 Windows desktop commands that had a path and no run now have live evidence:
[screen and foreground commands](2026-10-12-windows-screen-and-foreground-commands.md). They are
`display.capture`, `display.list`, `screen.captureRegion`, `screen.findText`,
`screen.waitForText`, `input.moveMouse`, `input.clickPoint`, `input.drag`, `input.typeText`,
`input.key`, `input.keys`, `input.holdKeys` and `input.keyboard`.

All 21 Windows desktop commands with a path now have a live record. Every one ran against a window
the harness owns, on one machine at 100% scaling. The Evidence column above is left as it stood
on 2026-10-11; the support matrix holds the current claim.

The runs found no defect in the commands. They found two limits:

- With an active IME, key presses go into the composition.
- One OCR pass made right after a window was shown missed its text.

Candidate slice 3 above is done.

Later the same day candidate slice 2 was done. The Windows driver gained `paste_text`, so
`input.pasteText` and the `paste_text` action of `input.keyboard` run on Windows, foreground only
and `partial`, with live receiver evidence:
[Windows paste text](../driver/2026-10-12-windows-paste-text.md). The Windows counts become 10
`supported`, 12 `partial` and 16 `unsupported`: 22 of the 38 desktop commands, and 25 of all 42
IDs. The two tables above are left as they stood on 2026-10-11.

Candidate slice 1 followed. The media result is now one shared contract in
`auv-driver-common::media`, which `auv-media-macos` re-exports. The Windows SMTC backend maps
onto it, so `mediaControl.nowPlaying` is `partial` on Windows (no timeline) and the five controls
are `supported`. All six have a live run against QQ Music, the controls once each with the owner's
approval:
[Windows media control](../driver/2026-10-12-windows-media-control.md). The Windows counts become
15 `supported`, 13 `partial` and 10 `unsupported`: 28 of the 38 desktop commands, and 31 of all 42
IDs.

## Changes made to #211

On 2026-10-11 the issue body was edited to match the table above. The body before the edit is the
text of the issue on 2026-10-01.

- Ticked the Windows paths that now exist: `window.list`, `window.capture`, `window.findText`,
  `window.waitForText`, and `screen.findText` and `screen.waitForText` on the primary display, with
  an unticked line for other displays.
- Ticked the Linux paths that #216 added, which the issue still listed as missing.
- Added the five commands the tree lacked, marked "added", with all three platforms.
- Replaced the command and coverage counts, and ticked the separate count.
- Left the "Support information" section as it was. It describes the typed cells, which this change
  set introduces, and is ticked when that lands.

On 2026-10-12, after #339 landed, the body was cut back to the open gaps. The full per-command tree
is the table above, and `auv invoke --help` shows the current cells.

- Replaced the command tree with open gaps by platform, each with what blocks it.
- Ticked five of the six "Support information" items. The selected Runner route stays open
  (`TODO(invoke-support-runner-route)`).
- Ticked the `input.pressKeys` duplicate-count item: the registry marks the command as an alias.
- Left the Windows matrix evidence task open until the live runs land.

## Limits

- The cells were read from `cfg` gates in the command modules and from the drivers' explicit
  unsupported branches. Nothing checks a cell against the code it describes, and no cell was run
  on macOS or Linux for this audit.
- `supported` and a ticked checkbox mean a direct local path exists in source. They are not
  behavior evidence; the Evidence column and the support matrix say what has been run.
- A selected Runner is a separate route and is not covered.
