# Windows parity consolidation review (#211)

Status: adopted by the owner on 2026-10-13. The owner accepted the verdict, the blocking items
(section 3.8, P0) and the three missing pieces this review ranks first: typed refusal, Runner
support cells, and `scroll_until` telling "never moved" from "stopped moving". The owner left the
remaining open choices to the reviewer.

- The changes land in separate pull requests. #352 and #353 redacted the private data and stopped
  the harnesses writing into committed evidence.
- The consolidation pull request folded the root slice documents into three driver notes:
  - [scroll](../driver/2026-10-12-windows-scroll-until-background.md);
  - [targeted keyboard](../driver/2026-10-12-windows-targeted-keyboard-probe.md);
  - [activation](../driver/2026-10-13-windows-app-activation-probe.md).
- The same pull request stated the principles of section 3.5 and the minimized-window rule of
  section 2.4.
- The `support` cell changes C1 to C6 follow with their code.

The text below is the review as written. A reference such as `settle-timing.md` §5 points at a
root document as it was before the consolidation, recoverable at `ed81787a`, `c265d461` and
`dc38e238`. Its raw records now live under `docs/ai/references/driver/evidence/`.

Baseline:

- `main` at `d4fc9d5e`, which includes #336 to #342 and #348 to #351.
- The unmerged branch `origin/feat/windows-clicktext` at `fdad977e`. It has no pull request.

I read the slice documents, every evidence record they cite, the `support = { ... }` cells, the
code those cells describe, `docs/SUPPORT_MATRIX.md`, `docs/TERMS_AND_CONCEPTS.md` and the
body of #211. I ran no experiment.

Disclosure: I wrote #340, #341 and #342 (the foreground runs, paste and media). Weigh what this
review says about those three with that in mind.

## Verdict

1. **No `support` cell needs to change state today.** The current cells already say what the
   slices proposed for scroll, typed text, paste and media. Where a slice proposed a different
   state, the proposal is wrong or needs code first:
   - **`input.key`, `input.keys`, `input.holdKeys` → Unsupported (#349).** Rejected. It would
     erase a live-validated foreground path to describe the targeted path, which the cell's
     `Partial` note already excludes.
   - **`window.clickText` → Partial.** Only correct after a wiring change. Today the command
     refuses on Windows.
   - **`screen.clickText` "unsupported by architecture".** Rejected as reasoning. It is a
     foreground command on every platform. It takes no `--input-policy`, so "under
     BackgroundOnly" does not apply to it.
   - **`app.activate` → Partial.** Needs the result-type decision that
     `TODO(app-activate-windows-cli)` already names.

   Four Unsupported cells should gain or change their note (change set in section 2).
2. **Part of the new evidence cannot back a claim.**
   - The clickText branch writes four `screen.clickText` "trials" as literal JSON and never runs
     them. Its Edge record states "0% effect" whatever the hashes say.
   - The keyboard trials (#349) were not run through `auv`, and their harness is not committed.
     Several records contradict their own fields.
   - The app-management trials (#350) drive the driver directly, not the command.

   These are probes, not command evidence (AGENTS.md: "A live probe is not a support claim").
3. **The Chromium/Electron "drops posted input" boundary is true for the wheel only.** Do not
   promote it as one principle across wheel, keys and clicks.
   - For **clicks**, the committed survey from #335 contradicts it. Edge's page counted posted
     clicks: 3 of 4, and 4 of 4 with occlusion tracking off.
   - For **keys**, AUV has no targeted route, and the Edge key probe posted into a compositor
     window.

   The principle worth stating centrally is a different one (section 3.5).
4. **Resolution B is coherent with the code once it is stated as a rule about input policy.**
   Foreground delivery may restore a minimized window, and background policies never do. Stated
   as "explicit user intent", it is not coherent. Its document also claims behavior that only
   the Rust background session has, and its evidence still labels the restore a redline
   conflict.
5. **There are blocking hygiene problems.**
   - **Private data is committed:** the user's track title, song lyrics, a browser tab title and
     a game window title.
   - **Placement:** nine slice documents sit at the repository root.
   - **The clickText branch would fail Windows CI.** Its live test is not `#[ignore]`d and
     opens `F:\auv\...` at startup.
   - **Private apps were driven:** several runs sent input to applications the user had open.
     #335's survey rule forbids that.
6. **The most valuable missing piece is typed refusal.** On Windows, the seven commands that
   refuse with "only available on macOS" (`app.*`, `input.focusText` and both `clickText`s,
   7 sites) reach callers as `FailureCode::CommandFailed`, not `FailureCode::Unsupported`. An MCP
   client cannot tell "this platform cannot" from "this run failed". Fixing that is the cheapest
   change with the largest effect on #211's purpose.

## 1. Unified Windows capability matrix

How to read the columns:

- **Cell** is the `support` declaration at `d4fc9d5e`. It is a statement about the source, not
  about behavior (`crates/auv-cli-invoke/src/support.rs:1`, `docs/TERMS_AND_CONCEPTS.md#command-support`).
- **Evidence** is the highest level justified for the command itself, in the support-matrix
  vocabulary:
  - **live-validated:** a dated run through `auv invoke`;
  - **probe only:** driver calls made outside the command;
  - **tested:** fixtures;
  - **none.**
- **Boundary** uses one set of words throughout:
  - **foreground** means `SendInput` into whatever has focus;
  - **targeted** means addressed to one window;
  - **posted** means the Windows targeted mechanism, `PostMessageW`;
  - **background** is reserved for the input policy.

### Display and screen

| Command | Cell | Evidence | Boundary |
| --- | --- | --- | --- |
| `display.list` | supported | live-validated, #340 ([note](2026-10-12-windows-screen-and-foreground-commands.md)) | One display at 100% scaling. |
| `display.capture` | supported | live-validated, #340 | As above. Pixels checked by decoding a harness window's state strip. |
| `screen.captureRegion` | supported | live-validated, #340 | As above. |
| `screen.findText` | partial: primary display only | live-validated, #337 and #340 | Primary display. A pass made while a window fades in can miss text. |
| `screen.waitForText` | partial: primary display only | live-validated, #337 and #340 | Primary display. Absorbs the fade-in race. |
| `screen.clickText` | unsupported (no note) | none. The four branch "trials" are literals (section 3.1) | Not wired. It refuses with a plain string (`screen.rs:296`, `TODO(invoke-screen-click-text-windows)`). Both halves, screen OCR and the foreground display click, are live-validated separately in #340. |

### Window

| Command | Cell | Evidence | Boundary |
| --- | --- | --- | --- |
| `window.list` | supported | live-validated, #336 ([note](2026-10-11-windows-window-commands.md)) | `--target` is the executable file name, matched ignoring case. |
| `window.capture` | supported | live-validated, #336 | Refuses a minimized window (`InvalidInput`, `capture.rs:188`). GPU-drawn content is not evidenced (`PrintWindow`). |
| `window.findText`, `window.waitForText` | supported | live-validated, #336 | Harness receiver. Windows OCR puts spaces between CJK characters (`scroll-until-qqmusic-textvisible.md`, #348). |
| `window.clickText` | unsupported (no note) | probe only, unmerged branch `fdad977e` | Not wired (`window.rs:384`). The driver click it needs exists: `WindowApi::click` posts under the background policies (`auv-driver-windows/src/session.rs:278`), and `input.clickPoint --relative-to window` already reaches it. |

### Text, focus and keys

| Command | Cell | Evidence | Boundary |
| --- | --- | --- | --- |
| `input.focusText` | unsupported | none | Not audited against the UI Automation tree. |
| `input.axFocusText` | alias of `input.focusText` | none | — |
| `input.typeText` | partial: foreground only | Foreground: live-validated, #340. Targeted: probe only, #349. | Foreground text is `KEYEVENTF_UNICODE`, which an IME does not compose. Targeted is refused (`TODO(windows-targeted-keyboard)`, `auv-driver-windows/src/input.rs:209`). |
| `input.pasteText` | partial: foreground only | Foreground: live-validated, #341 | Fails, and restores the clipboard, when nothing reads the text within 2 s. An uncommitted IME composition blocks Ctrl+V. |
| `input.key`, `input.keys`, `input.holdKeys` | partial: foreground only | Foreground: live-validated, #340. Targeted: probe only, #349. | With an active IME, presses go into the composition (#340 finding). |
| `input.pressKeys` | alias of `input.keys` | — | — |
| `input.keyboard` | partial: foreground only | Foreground: live-validated, #340 and #341 (paste action) | As for its actions. |

### Pointer

| Command | Cell | Evidence | Boundary |
| --- | --- | --- | --- |
| `input.pointerPosition` | supported | live-validated, 2026-10-09 ([note](../driver/2026-10-09-pointer-position.md)) | Needs an interactive desktop. |
| `input.moveMouse` | supported | live-validated, #340 | Moves the real pointer. |
| `input.scrollPoint` | supported | live-validated, 2026-10-09 (Edge) | Screen coordinates only. |
| `input.clickPoint` | partial: posted, depends on toolkit | live-validated, #340: foreground on the screen and display, posted relative to a window on the harness receiver. Per-application results come from the Rust background session (#335), not from this command. | Posted clicks: Win32 receiver yes. Character Map 1 of 4 frames. Calculator (XAML) no effect. Edge page counted 3 of 4 (4 of 4 with occlusion tracking off). |
| `input.drag` | partial: same | live-validated, #340 (harness pad) | Same posted mechanism. Other applications not evidenced. |
| `input.scroll` | partial: same | live-validated: 2026-10-06 (Edge, foreground) and #348 (QQ Music, posted). Explorer only through the driver bench `measure-settle`. | Posted wheel: QQ Music and Explorer moved. Edge did not move when not the foreground window (#335, 2026-10-06 probe, #348). Claude, NTQQ, Steam and VirtualBox show no motion in one uncontrolled run each (section 3.1). |
| `input.scrollUntil` | partial: same | live-validated: 2026-10-06 (Edge, foreground) and #348 (QQ Music, posted): text 4 of 4, end at head and tail, timed motion 4 runs | An app that ignores the wheel stops with `end_by_no_visual_progress`, the same reason as real end of content. Multi-pane windows need `--region`. Settle default 400 ms (section 2.1). |

### Applications, overlay, media and fixtures

| Command | Cell | Evidence | Boundary |
| --- | --- | --- | --- |
| `app.activate` | unsupported: driver path not used | probe only, #350 (driver `activate_window` on harness windows) | Not wired: the command's result is bundle-ID shaped (`TODO(app-activate-windows-cli)`, `app.rs:94`). |
| `app.probePermissions` | unsupported: driver probe not used | probe only, #350 | The command's result names macOS permissions (`auv-driver-common/src/permission.rs:23`). |
| `overlay.outline`, `.cursor`, `.status`, `.captureFrame`, `.clickTarget` | unsupported: adapter not wired | Rust API live-validated (2026-10-10 overlay motion); none through the commands | Gated to macOS. |
| `mediaControl.nowPlaying` | partial: no timeline | live-validated: #342 (direct) and #351 (Runner gRPC service) | Current SMTC session only. `source_bundle_id` carries the AppUserModelID. |
| `mediaControl.play`, `.pause`, `.togglePlayPause`, `.next`, `.previous` | supported | live-validated once each: #342 (direct) and #351 (Runner service) | Current session. Verification waits up to 3 s. Right after a skip the after-state can read "not playing". |
| `scan.frame`, `scan.coverage` | supported (fixtures) | tested | Never touch the desktop. |

### Counts

Over the 38 desktop commands, the source has 15 supported, 13 partial and 10 unsupported. Counting
the aliases and fixtures, 31 of the 42 IDs have a path. That matches the
[coverage audit follow-up](2026-10-11-invoke-platform-coverage-audit.md#follow-up-on-2026-10-12),
and I re-derived it from the cells above. The change set in section 2 changes no count. If the
three wiring changes it proposes land, the counts become 15 / 16 / 7, or 16 / 15 / 7 if
`app.activate` is wired as Supported.

### Runner route

The cells above cover the direct local path only. `TODO(invoke-support-runner-route)` is still
open (`support.rs:6`). `TODO(windows-media-runner)` was closed by #351.

On a selected Runner, `window.clickText` and `screen.clickText` have no platform gate. They call
`WindowService`, `TextRecognitionService` and `InputService` (`auv-cli-invoke/src/runner.rs:301`
and `:358`), and a Windows Runner serves all three (`local_driver.rs:2753`–`2761`). With no
`--input-policy`, `screen.clickText` defaults to `ForegroundPreferred` there (`runner.rs:616`).

So a Windows Runner may already execute two commands whose direct cell says Unsupported. This is
not evidenced either way (section 4).

## 2. Deferred decisions: resolutions and change set

| Item | Slice proposal | Recommendation | Cell change |
| --- | --- | --- | --- |
| `input.scroll`, `input.scrollUntil` | Partial, naming toolkits | Keep Partial and keep the existing app-agnostic note. Toolkit results belong in `SUPPORT_MATRIX.md`. Keep the 400 ms settle default. | none |
| `input.typeText` targeted | Partial, "mechanism proven, needs child HWND targeting" | It is already Partial (foreground only). Targeted delivery stays refused. | none |
| `input.key`, `input.keys`, `input.holdKeys` | Unsupported | Rejected: it would erase a live-validated foreground path. | none |
| `input.keyboard` | Partial or Unsupported | Keep Partial. | none |
| `input.pasteText` | Partial, foreground | Consistent with the keyboard cells. | none |
| `window.clickText` | Partial for Win32 | Agree on the end state, after a wiring change. Add a note now. | note now; state after W1 |
| `screen.clickText` | Unsupported, "architectural" | Reject the reason. Add a note now. Partial ("primary display only") after wiring. | note now; state after S1 |
| `app.activate` | Partial, Resolution B | Resolution B holds when restated as a policy rule. The cell waits on the result-type decision. | none now; state after A1 |
| `app.probePermissions` | Unsupported, "not applicable" | Agree on the state. Replace the note, which invites wiring that does not fit. | note |
| `mediaControl.*` on the Runner | Runner cells | No Runner cells exist; park this under `TODO(invoke-support-runner-route)`. Fix the stale support-matrix rows. | none |

### 2.1 Scroll and `scroll_until`

- **The cells stay as they are** (`crates/auv-cli-invoke/src/commands/input.rs:1492` and `:1763`).
  The note, "background delivery posts window messages, so whether an application acts on them
  depends on its toolkit", is the right altitude for a statement about source.
  - The proposed note, "accepted by Win32/DirectComposition, ignored by Chromium/Electron and
    Qt" (`windows-211-scroll-parity-completion.md`, `wheel-acceptance-matrix.md` §5, #348), is a
    behavior claim, so it does not belong in a cell.
  - It is also internally inconsistent. #348 calls QQ Music "DirectComposition / CEF"
    (`settle-timing.md` §4, `scroll-until-end.md` §4), and calls CEF a rejecting toolkit in the
    same PR.
  - The boundary is per application, not per toolkit label.
- **Keep the 400 ms default** (`crates/auv-scan/src/scroll_until.rs:94`).
  - The 50 ms proposal (`settle-timing.md` §5) measured the time for one wheel notch to reach a
    WGC frame on content that was already loaded.
  - The default exists for something else: the CLI help says settle "waits so lazy content can
    load" (`input.rs:1577`). Loading time was not measured.
  - Both 100 ms "cold" figures rest on one sample per surface.
  - Callers who know their surface can pass `--settle-ms 50`, as the #348 runs did. A
    per-platform default would need a measurement on a lazy-loading list and owner approval.
- **Update `docs/SUPPORT_MATRIX.md`, not the cells:**
  - Row 44, "Scroll until": add the QQ Music background runs from #348 as Windows posted-wheel
    evidence (text, end and motion), with their limits. Those limits are one application,
    `--region` required for multi-pane windows, and an end stop that cannot tell an ignoring
    application apart.
  - Leave rows 39, 41 and the by-application table unchanged for Claude, NTQQ, Steam and
    VirtualBox until a controlled rerun (section 3.1).

### 2.2 Keyboard and paste

- **No change** to `input.rs:343` (`typeText`), `:382` (`pasteText`), `:457` (`keys`), `:471`
  (`pressKeys`), `:521` (`holdKeys`), `:600` (`keyboard`) or `:609` (`key`).
  - Every one reads `Partial("foreground delivery only; targeted delivery is unsupported")`.
  - The foreground half is live-validated (#340, #341).
  - The targeted half is refused in source (`auv-driver-windows/src/input.rs:209`,
    `background_session.rs:590`).
  - #349's "Unsupported" verdicts for `key`, `keys` and `holdKeys` describe only the targeted
    half, which the note already excludes. #349's own final recommendation agrees: retain
    unsupported targeted keyboard (`keyboard-targeted.md` §5).
- **The proposed `typeText` note must not be used.**
  - It is half Chinese ("机制已证，需 child HWND targeting（未实现）"), and it would appear
    verbatim in `--help` and MCP.
  - It claims a mechanism the command cannot reach. `--target` resolves top-level windows, and
    #349's own probe shows a top-level `WM_CHAR` has no effect (trials 37 and 38).
- **Paste is consistent with the keyboard verdict, for a reason #349 supplies.**
  - A targeted paste cannot be a posted Ctrl+V: a posted `WM_KEYDOWN(VK_CONTROL)` does not change
    the target thread's `GetKeyState` (`keyboard-targeted.md` §4 Q2; this is a probe, but
    standard Win32 behavior).
  - `WM_PASTE` to a classic Edit child would avoid the modifier problem. It would still need the
    same two missing pieces: resolving a child control, and revalidating the recipient before
    each action (the existing TODO's reason).
  - It would also need the clipboard loan to attribute the read to the target process instead of
    the foreground process (`auv-driver-windows/src/input.rs:424`).

  The two cells therefore share one note on purpose.
- **Proposed TODO text (not applied).** Extend `TODO(windows-targeted-keyboard)` at
  `auv-driver-windows/src/input.rs:209` to name both gaps found since: child-control resolution,
  and modifier state that posted messages do not carry. Cite `keyboard-targeted.md` as a probe.
  Keep the trigger: an owner-approved recipient lease.

### 2.3 clickText

- **`window.clickText`, now.**
  - `crates/auv-cli-invoke/src/commands/window.rs:346`
  - Old: `support = { macos: Supported, linux: Unsupported, windows: Unsupported },`
  - New: `support = { macos: Supported, linux: Unsupported, windows: Unsupported("the Windows driver has a window-targeted click that this command does not use") },`
  - The note follows the style of the existing Windows notes (`app.rs:15`, `:53`, `input.rs:255`).
- **`window.clickText`, after wiring change W1.**
  - Same line, new cell:
    `windows: Partial("background delivery posts window messages, so whether an application acts on them depends on its toolkit")`.
  - That is the exact note `input.clickPoint` carries, because it is the same delivery.
  - W1 is code. Lift the `#[cfg(target_os = "macos")]` gates at `window.rs:333`, `:358` and
    `:471`, so that `click_recognized_window_text_with_session` runs on Windows.
  - Its evidence must be a run of the real binary in the `window` eval scenario
    (`evals/auv-base/platforms/desktop-windows/tasks/window_commands.rs`), judged by the
    receiver's own click count. The branch's harness does not qualify.
  - Replace the stale TODO at `window.rs:353`. It says "The Windows `WindowApi` has no
    window-targeted click", which has been false since `WindowApi::click`
    (`auv-driver-windows/src/session.rs:278`).
- **`screen.clickText`, now.**
  - `crates/auv-cli-invoke/src/commands/screen.rs:273`
  - Old: `support = { macos: Supported, linux: Unsupported, windows: Unsupported },`
  - New: `support = { macos: Supported, linux: Unsupported, windows: Unsupported("the Windows driver can click a screen point in the foreground, which this command does not use") },`
- **`screen.clickText`, after wiring change S1.**
  - New cell: `windows: Partial("primary display only")`, matching `screen.findText` (`screen.rs:148`).
  - The click moves the real pointer, as on macOS.
  - The evidence was the blocker (`screen.rs:293`). The `foreground` scenario from #340 can now
    take it on a harness window, on an idle desktop.
- **Why "architectural, under BackgroundOnly" is the wrong reason.**
  - `ClickScreenTextArgs` has no input policy (`screen.rs:263`).
  - The Runner defaults the command to `ForegroundPreferred` (`runner.rs:616`).
  - So the command is a foreground command by contract on every platform. Its Windows gap is
    wiring plus evidence, not an impossibility.

### 2.4 `app.activate` and Resolution B

- **The cell does not change now** (`app.rs:53`). Its note is accurate.
  - Wiring needs the result-type decision that `TODO(app-activate-windows-cli)` names
    (`app.rs:94`).
  - `ApplicationActivationResult` is bundle-ID shaped. The Windows driver returns
    `ProcessActivationResult` (`auv-driver-windows/src/application.rs:31`).
- **After A1** (a shared activation result, owner-approved), I recommend `windows: Supported`.
  - The foreground lock and the restore of a minimized window are runtime limits for the support
    matrix, not gaps in the path. macOS activation can also be refused at runtime.
  - On Windows, `--target app:<name>` should mean the executable name, as it already does for
    the window commands (row 47) and, since #348, for the input commands.
- **Resolution B is coherent with the code, restated as a rule about input policy.**
  - The code already behaves this way: an explicit foreground delivery may restore a minimized
    target, and the background policies never do.
  - Every `ForegroundPreferred` window-targeted click, drag or wheel calls `activate_window`
    (`session.rs:284`, `:354`, `:398`), and that always runs `ShowWindow(SW_RESTORE)`
    (`window.rs:288`).
  - `app.activate` is the same kind of explicit foreground step.
  - Framed as "explicit user intent" (`app-management.md` §3 Q3), the rule says nothing about
    `ForegroundPreferred`. It also leaves `BackgroundPreferred` unclear: a background-preferred
    wheel falls back to `ForegroundHid` after a posted-wheel error (`session.rs:350`–`360`) and
    would then restore.

  State it once, in `docs/TERMS_AND_CONCEPTS.md#input-mode`: *Only a foreground step restores a
  minimized window: `ForegroundPreferred` delivery, or an activation command. `background_only`
  never restores. A `background_preferred` fallback to foreground input may restore.* The last
  sentence is an owner decision. The alternative is that the fallback refuses minimized targets.

  Decided on 2026-10-13: the fallback may restore. The rule is in
  [input mode](../../../TERMS_AND_CONCEPTS.md#input-mode).
- **Inconsistencies to fix in the #350 material.**
  - **Background behavior is overstated.** `app-management.md` §3 Q3 says background channels,
    including `window.capture`, `window.click`, `input.scroll` and `input.wheel`, "continue to
    return typed refusals (`skipped_minimized`)". Only the Rust background session returns
    `skipped_minimized` (`background_session.rs:479`).
    - Direct `window.capture` returns `InvalidInput` (`capture.rs:188`).
    - Direct posted clicks and wheels do not check for a minimized window. They post and report
      success (`background_input.rs:39`–`64`).
    - `input.wheel` is not a command.
  - **The evidence contradicts the resolution.** Trials 5 and 6 in
    `2026-10-13-windows-app-management.jsonl` still say `REDLINE_CONFLICT` for the restore.
  - **The evidence contradicts the document.** `app-management.md` §3 Q1 says that "when an
    external game was active, `SetForegroundWindow` returned without raising the target window".
    The only recorded trial with the game in the foreground, trial 9, shows the activation
    winning. If an unrecorded run behaved differently, the document cannot rest on it.
  - **The adoption is unverified.** The document says Resolution B was "adopted". I cannot see an
    owner decision in the repository (section 4).

### 2.5 `app.probePermissions`

- **Change the note.**
  - `crates/auv-cli-invoke/src/commands/app.rs:15`
  - Old: `windows: Unsupported("the Windows driver has a permission probe that this command does not use")`
  - New: `windows: Unsupported("the result names macOS privacy permissions; the Windows driver's elevation, UIAccess and session probe has no field in it")`
- **Why.**
  - The old note invites wiring.
  - The shared `PermissionProbe` has four macOS fields (`auv-driver-common/src/permission.rs:23`),
    and `WindowsPermissionProbe` has three different ones (`auv-driver-windows/src/session.rs:660`).
  - Linux projects its probe onto the shared shape (`auv-driver-linux/examples/validate.rs:206`)
    and is still Unsupported. Windows staying Unsupported is consistent with that.
- **Do not repeat "Windows has no permission model"** (`app-management.md` §4.2) in public text.
  UIPI is a real gate for input: a process cannot post to a window of higher integrity. The
  readiness probe exists for that reason.

  A cross-platform readiness command is a candidate contract, not this cell.

### 2.6 Media

- **No change** to `media_control.rs:22`, `:56`, `:72`, `:88`, `:104` or `:120`.
- **Stale rows in `docs/SUPPORT_MATRIX.md`.** Rows 37 and 38 still say "The Runner's media
  service is macOS only." #351 made that false.
  - Proposed text: "The Runner's media service also runs on Windows (#351). One gRPC call of each
    command was made against the service on 2026-10-13; a selected-Runner `auv invoke` was not
    run."
  - The evidence client was an in-process tonic client (`2026-10-12-windows-media-control.md`,
    Runner section).
- **The Runner proposal table in that note** proposes cells that the registry has no place for.
  Mark it as input to `TODO(invoke-support-runner-route)`.
- **Two stale facts in the same note.** Its first line still says "implemented on branch
  `feat/windows-media-control`". It also does not mention that Windows now serves the
  macOS-namespaced proto `auv.api.driver.macos.v1.MediaControlService` (`local_driver.rs:2787`).
  The namespace is a contract question for later. The note should name it.

### 2.7 Consolidated change set (for approval)

| # | File:line at `d4fc9d5e` | Old → new | Depends on |
| --- | --- | --- | --- |
| C1 | `crates/auv-cli-invoke/src/commands/window.rs:346` | `windows: Unsupported` → `windows: Unsupported("the Windows driver has a window-targeted click that this command does not use")` | — |
| C2 | `crates/auv-cli-invoke/src/commands/screen.rs:273` | `windows: Unsupported` → `windows: Unsupported("the Windows driver can click a screen point in the foreground, which this command does not use")` | — |
| C3 | `crates/auv-cli-invoke/src/commands/app.rs:15` | note → `"the result names macOS privacy permissions; the Windows driver's elevation, UIAccess and session probe has no field in it"` | — |
| C4 | `window.rs:346` | → `windows: Partial("background delivery posts window messages, so whether an application acts on them depends on its toolkit")` | W1 code and eval run |
| C5 | `screen.rs:273` | → `windows: Partial("primary display only")` | S1 code and `foreground` eval run |
| C6 | `app.rs:53` | → `windows: Supported` | A1 result contract |
| — | `input.rs:343`, `:382`, `:457`, `:471`, `:521`, `:600`, `:609`, `:822`, `:1146`, `:1492`, `:1763`; `media_control.rs:22`–`120` | no change | — |

Non-cell edits that go with the change set:

- **The TODO at `window.rs:353`** states a false fact (2.3).
- **The TODO at `screen.rs:293`.** Its trigger, evidence that disturbs no one's desktop, can now
  be met by the `foreground` scenario.
- **`TODO(windows-targeted-keyboard)`** gains the two gaps (2.2).
- **`docs/SUPPORT_MATRIX.md`:** rows 37 and 38 (2.6), and row 44 (2.1).
- **`docs/TERMS_AND_CONCEPTS.md#input-mode`:** the minimized-window rule (2.4).
- **#211.** These are proposed edits for the owner to make. I did not edit the issue.
  - Tick paste and media; both landed.
  - Tick the "Run a native Windows build" evidence task; #340 landed.
  - Replace the `window.clickText`, `screen.clickText`, `app.*` and targeted-keyboard lines with
    the reasons above.
  - Do not paste the full command tree from `windows-211-update.md` or
    `windows-211-scroll-parity-completion.md`. #211 was restructured on 2026-10-12 to list open
    gaps only, and the full tree lives in the coverage audit.

## 3. Inconsistencies and gaps

### 3.1 Evidence that cannot back a claim

| Source | Problem |
| --- | --- |
| Branch `fdad977e`, `crates/auv-driver-windows/tests/clicktext_parity.rs`, trials 9–12 | Written as JSON literals. No command, OCR pass or capture ran. Hashes are the string `"display_hash_pre"`. Trial 12, a negative that never ran, says `"verified": true`. |
| Same file, trial 8 (Edge) | The `semantic_effect` "0% effect … Chromium discarded input" is a constant written whatever the hashes show. When Edge is not running, the else branch writes a "dropped" record with no run at all. It clicked a fixed point (100, 50), in the tab strip, of an Edge window the user had open. A frame hash cannot see an effect in a covered Chromium window: #335 showed that Chromium stops painting when covered (`SUPPORT_MATRIX.md` by-application table). |
| Same file, all trials | `compute_sha256` is `DefaultHasher` over the frame, plus a second hash of only the last 4096 bytes. That is why every hash in the file ends in the same 16 hex digits. It is not SHA-256, and its second half covers only the frame's bottom edge. |
| Same file, trials 1–7 | Driver calls composed in a test, not `auv invoke window.clickText`, which refuses on Windows. They show that the pieces compose on a Win32 button, and that #323's `point = origin + window_point` holds on Windows. That is useful as a probe. |
| `2026-10-12-windows-keyboard-targeted.jsonl` (#349) | Raw `PostMessageW` from a harness that is not committed, so the 38 trials cannot be reproduced. Records contradict themselves: trials 4 and 16 ("backspace", `semantic_effect: none`) show two characters deleted, while the document says backspace works. Trials 11 and 23 (holding a letter) record 7 repeated characters as `success`, beside a `stop_reason` saying repeats do not happen in the background. "Notepad" is a harness window of class `AUV_Notepad_Anchor_Thread_V2`, not Notepad. |
| Same file, Edge trials 25–36 | Posted to Edge's `Intermediate D3D Window`, a compositor surface, not its input window. Judged by the window title, which typing into a page does not change. These trials show nothing about Chromium and keys. They also sent text, Return and Ctrl+A/C into the user's own browser. |
| `2026-10-13-windows-app-management.jsonl` (#350) | Driver calls (`activate_window`, `probe_permissions`), not the commands. Trial 12's CLI error message is hard-coded, not observed. |
| `2026-10-12-windows-wheel-acceptance-matrix.jsonl` (#348), negative rows | One run of 2 steps per application, with no positive control: the same point under `ForegroundPreferred`. For a covered Chromium window, "zero motion" cannot separate a dropped wheel from a window that does not paint. VirtualBox's manager list may simply not have been scrollable. #335 left the Chromium mechanism open as a REVIEW: `RerouteMouseWheel` redirects the wheel to the window under the point. That hypothesis predicts something different from #348's uncited "focus gate". |
| `scroll-until-motion.md` (#348) | "Timing matches the requested duration" is the planned schedule (`scheduled_elapsed_ms`). Nothing measured the dispatch times. The quantizer arithmetic it "proves" is already unit-tested (`auv-driver-common/src/scroll_motion_test.rs`). What was observed live is that the frame moved. |

Clean evidence in the same set:

- The #348 `scroll_until` runs on QQ Music went through `auv invoke`, with run IDs and artifacts.
  They are command evidence for posted-wheel scrolling on one application.
- The #351 media records are redacted and consistent.

### 3.2 Contradictions with committed evidence or source

1. **Chromium clicks.** The clickText document (§1 item 3, §4 Q3) says Chromium discards posted
   clicks. `SUPPORT_MATRIX.md` (Edge rows, from #335) records the page counting them.
2. **The foreground lock.** `app-management.md` §3 Q1 says a game held the foreground against
   activation. The one recorded trial with the game in the foreground, trial 9, shows activation
   winning over it.
3. **Minimized windows.** "Background channels return `skipped_minimized`" is true only of the
   background session (2.4).
4. **Stale TODO.** The `window.rs:353` TODO says no Windows window click exists. It does.
5. **The clickText document's code quote.** It quotes `window.rs` lines 485–488 as the Windows
   path. Those lines sit inside a `#[cfg(target_os = "macos")]` function (`window.rs:471`).
6. **Stale support-matrix rows.** Rows 37 and 38 say the Runner media service is macOS only (2.6).
   Row 39's "Not evidenced: … Qt, Electron" now has uncontrolled wheel observations beside it.
7. **#211's structure.** `windows-211-update.md` and `windows-211-scroll-parity-completion.md`
   propose re-adding the full tree that #211 removed (2.7).
8. **#349 against itself.** The executive summary proposes Partial for `typeText`; its §5 final
   recommendation retains Unsupported for targeted input. The cell already encodes the second.

### 3.3 Terminology drift

| Term | Meanings found | Where | Proposal |
| --- | --- | --- | --- |
| background | (a) the input policy `background_only` / `background_preferred`; (b) "addressed to a window" in the pointer cell notes; (c) "any non-intent channel, including capture" | (a) TERMS `#input-mode`, #348 docs; (b) `input.rs:822`, `:1146`, `:1492`, `:1763`; (c) `app-management.md` §3 | Reserve it for the policy. Say **targeted** for "addressed to a window" (as the keyboard notes already do) and **posted** for the Windows mechanism. Changing the four pointer notes changes `--help`, so leave that to a docs pass. |
| Supported / Partial / Unsupported | Source-path states in `support.rs`; evidence verdicts in the slice documents ("Supported for Win32", "Unsupported under BackgroundOnly") | #348–#350 and the branch | Cells use the `support.rs` meaning only. Evidence uses the support-matrix levels. |
| verified | `InputActionResult.verified` (driver); media `verified` (before/after comparison); "Verified: Yes (exact)" (an OCR re-check); `effect_verified: true` on refusals (it means "foreground unchanged") | driver, #342, #348, #350 | Keep the two typed meanings. Evidence records should name their check instead. |
| Notepad, SHA-256, CEF, SysListView32 | A harness window; a non-SHA hash; QQ Music labeled CEF in one document and "Win32 custom container" in another; the Explorer list class (not captured in evidence) | #349, branch, #348 | Name what was observed. Do not name a toolkit without the class name in evidence. |
| Brief N | Engineering slice codes used as titles and navigation | the #348 documents, the driver INDEX entry | AGENTS.md forbids slice codes as navigation labels. |
| "proven", "100% operational", "universal", "permanent", "architecturally honest" | Advocacy wording | the #348–#350 documents | Use the repository's evidence voice: what ran, where, how many times, and what is not evidenced. |

### 3.4 Error-type consistency

| Situation | How it is typed today | Source | Proposal |
| --- | --- | --- | --- |
| Command has no Windows path | `FailureCode::CommandFailed` through `From<String>`, while other refusals use `FailureCode::Unsupported` | 17 "only available on …" sites, 7 of which fire on Windows: `app.rs:43`, `:100`, `input.rs:293`, `screen.rs:296`, `:351`, `window.rs:384`, `:453`. Conversion at `command.rs:235`; compare `input.rs:560`. | Return `InvokeFailure::new(FailureCode::Unsupported, …)` at every site. This is a bug fix, with a test that each Unsupported cell refuses with `Unsupported`. |
| Target is minimized | Five behaviors: `InvalidInput` (capture); `skipped_minimized` (session); not checked, success (direct posted click and wheel); restored (foreground and activate); `Backend` "may be minimized" (WGC) | `capture.rs:188`, `background_session.rs:479`, `background_input.rs:39`, `window.rs:288`, `wgc.rs:421` | One rule (2.4). Direct posted input should refuse a minimized target the way capture does. |
| Target is gone or the handle is invalid | `Backend` "SetForegroundWindow was refused" (activate on a dead handle, #350 trial 8); `target_lost` (session); `InvalidInput` (unparsable reference) | `window.rs:291`, `background_session.rs` | `activate_window` should check `IsWindow` first and answer `NotFound`. |
| Delivered, effect not seen | Paste: `Backend` error. Media: `Ok` with `verified: false`. Posted click and wheel: `Ok`, unverified. `scroll_until`: `end_by_no_visual_progress`. Activation: a verification enum. | `input.rs:422`, `media.rs`, `session.rs:335`, `scroll_until.rs:157` | Paste is deliberately fail-closed, because it must restore the user's clipboard. The real gap is `scroll_until`: an application that ignored every step looks like the end of content. Add a typed distinction, for example motion never seen since the first step. |

### 3.5 Principles to state centrally

Do **not** promote "Chromium/Electron drops posted input" (3.1, 3.2). Promote these, with each
one stated once:

1. **A posted message carries only what is in it.** It does not update the cursor position,
   focus, the foreground or the key-state table. An application that consults any of those
   diverges.
   - This explains every Windows result in one sentence:
     - posted chords degrade (`GetKeyState`, #349, probe);
     - Chromium redirects the wheel by the point (#335 REVIEW);
     - XAML ignores posted mouse input (#335).
   - Home: `docs/TERMS_AND_CONCEPTS.md#background-session`, which already says "Delivery is not
     effect".
2. **An effect check must be able to see the effect.** A covered Chromium window does not paint.
   A frame hash from it shows "no effect" whatever happened. Judge by the application's own
   state, or by a window known to paint. This is what #335 did and what the branch did not.
   - Home: the evals README (`evals/auv-base/platforms/desktop-windows/README.md`, "Delivery is
     not effect").
3. **Results per application live in one table.** Extend the support matrix's "Windows
   Background Session By Application" table instead of keeping the parallel
   `wheel-acceptance-matrix.md`, and add rows only from controlled runs.
4. **Runs never act on the user's own windows.** #335's survey rule is "an application whose
   process is already running is skipped". Make it a rule for every Windows eval and probe, not
   one scenario's convention (3.6).

### 3.6 Privacy and the user's desktop

- **The user's own track title is committed.**
  - In the `window_title` fields of four #348 JSONL files.
  - In `settle-timing.md`, `scroll-until-end.md`, `scroll-until-qqmusic-textvisible.md`,
    `wheel-acceptance-matrix.md` and `docs/ai/references/driver/windows-wheel-acceptance-matrix.md`.
  - Also, earlier, in `2026-10-07-spike-verify-experiments.jsonl`.
- **Lyrics are committed:** several lines of copyrighted lyrics, as OCR match text in the JSONL
  and as quoted blocks in `scroll-until-end.md` §3.
- **A browser tab title** from the user's Edge profile is committed in the keyboard JSONL (Edge
  trials).
- **The user's game window title** is committed in app-management trial 9.
- **Contrast:** #342 and #351 redact titles. This repository's media notes treat the user's
  music as private.
- **Proposal.**
  - Redact these at HEAD: keep a hash or `"<redacted>"`, and keep OCR match text only for text
    the harness drew.
  - Rewriting history is the owner's call.
- **Input sent to the user's own applications:**
  - posted keys, including Return, Ctrl+A and Ctrl+C, into their Edge Beta (#349);
  - a click into an open Edge window (branch);
  - wheels into Claude Desktop, NTQQ, Steam, VirtualBox, Explorer and QQ Music (#348);
  - an activation that took the foreground from a running game (#350, trial 9).

  None of the records says whether the owner allowed these runs (section 4).

### 3.7 Placement and repository hygiene

- **Documents at the repository root.** Nine slice documents were merged there (#348–#350):
  `app-management.md`, `keyboard-targeted.md`, `scroll-until-*.md` (3), `settle-timing.md`,
  `wheel-acceptance-matrix.md`, `windows-211-scroll-parity-completion.md` and
  `windows-211-update.md`. AGENTS.md puts working notes under
  `docs/ai/references/<responsibility>/`. The branch adds a tenth, `clicktext.md`.
  - `wheel-acceptance-matrix.md` is byte-identical to `docs/ai/references/driver/windows-wheel-acceptance-matrix.md`.
  - Proposal: fold them into three dated driver notes (scroll background, keyboard probe,
    activation), delete the root copies, and keep the two #211 proposal files out of the
    repository.
- **`docs/ai/references/driver/INDEX.md`** says "Count: **71**" and lists 72 entries.
  - Three of the entries are raw JSONL files.
  - The narrative documents for those JSONL files are not indexed, because they sit at the root.
  - Earlier slices kept raw records under `evidence/<date>-<slug>/`.
- **Two production fixes landed without regression tests,** inside #348, a PR otherwise made of
  documents and evidence:
  - `click_window_selector` now matches `App::name` on Windows (`input.rs:1916`).
  - `list_windows` now clears the last error and tolerates an `EnumWindows` failure when it has
    found some handles (`auv-driver-windows/src/window.rs:214`–`229`). This can hide a partial enumeration. The
    workaround comment lacks a `NOTICE:`.
- **`measure-settle`** is a `[[bin]]` in the published `auv-driver-windows` crate
  (`Cargo.toml`, #348). It is hard-coded to `QQMusic.exe` and `explorer.exe` and writes into a
  committed document path (`src/bin/measure_settle.rs:136`, `:168`). There is precedent for
  binaries there (`replay-qqmusic-operation`), but evaluations belong in `evals/auv-base`.
- **The branch's `clicktext_parity.rs` would break Windows CI.**
  - It is `#[test]` without `#[ignore]` and opens `F:\auv\docs\…` at startup.
  - `check.yml` runs `cargo test --workspace` on `windows-2025` (`check.yml:83`), where that path
    does not exist.
- **The live harnesses overwrite committed evidence.** Both the branch test and the merged
  `app_management_parity.rs` truncate the committed evidence file on every run. Write to an
  output directory instead, as the evals do.

### 3.8 What is still missing, by priority

P0, before any of this backs a public claim or the branch merges:

1. **Do not merge `feat/windows-clicktext` as it is.** It has the CI break, the literal records
   and the false code quote. Its trials 1–7 are worth keeping as a probe.
2. **Redact the private data at HEAD** (3.6).
3. **Correct the two claims the evidence contradicts:** Chromium clicks, and the game foreground
   lock.

P1:

4. **Typed platform refusal** (3.4, first row). A small bug fix that serves #211 user stories 7
   and 9 directly.
5. **Runner support cells** (`TODO(invoke-support-runner-route)`). A Windows Runner may run both
   clickText commands with no claim behind it (section 1, Runner route).
6. **`scroll_until` distinguishing "never moved" from "stopped moving"** (3.4).
7. **One rule for minimized targets** (2.4, 3.4).
8. **Regression tests** for the two fixes in #348.

P2:

9. **Wiring changes** W1 (`window.clickText`), S1 (`screen.clickText`) and A1 (`app.activate`
   result contract). Each comes with its cell change C4–C6 and an eval run through the real
   binary.
10. **Targeted keyboard:**
    - a recipient lease;
    - child-control resolution;
    - `WM_PASTE` as a candidate route for classic Edit controls.

    It stays refused until those exist.
11. **A controlled survey rerun:** wheel, click and keys for each application, in the `apps`
    scenario, with a foreground control at the same point and occlusion tracking off. Only the
    rerun's results enter the by-application table.
12. **IME-active metadata on key results** (#340 candidate), and the media skip transient and
    timeline (#342 candidates).
13. **Move `measure-settle`** into `evals/auv-base`.
14. **Audit `input.focusText` against the UI Automation tree.** Decide how overlay commands can
    be evidenced without drawing on someone's screen.

## 4. Could not verify from the repository alone

- **Owner approval** for the runs on the user's own applications, the game foreground (#350
  trial 9), and the `mediaControl.pause` before the settle benchmark (`settle-timing.md` §2).
- **Owner adoption of Resolution B.** The document says "adopted", and the commit says
  "finalize". I found no recorded owner decision. Resolved on 2026-10-13: the owner confirmed that
  another agent wrote "adopted", and that it was not an owner decision. The owner then accepted the
  rule as restated in section 2.4.
- **The mechanisms behind the drops.** Whether Chromium's wheel drop is a focus gate or a
  point-based reroute, and whether the Qt, Steam, Claude and NTQQ windows were covered when they
  showed no motion.
- **Toolkit labels.** QQ Music's and Explorer's toolkits; the evidence holds no window class
  names.
- **A Windows Runner running clickText.** Whether `window.clickText` or `screen.clickText`
  actually runs end to end through a selected Windows Runner.
- **The keyboard harness.** It was not committed, so the 38 trials cannot be rerun or audited
  beyond their records.

## Sources

- **Merged:**
  - #336 `5c171e69`, #337 `c24b8222`, #339 `3b36fd54`, #340 `72322117`, #341 `464c1119`,
    #342 `c24c2e7e`;
  - #348 `ed81787a`, #349 `c265d461`, #350 `dc38e238`, #351 `d4fc9d5e`;
  - #335 `40423508`, for the by-application survey.
- **Unmerged:** `origin/feat/windows-clicktext` `fdad977e`.
- **Documents:**
  - the root slice documents listed in 3.7;
  - `docs/ai/references/driver/`: the `2026-10-12-*` and `2026-10-13-*` JSONL files,
    `2026-10-12-windows-media-control.md`, `2026-10-10-windows-background-session.md` and `INDEX.md`;
  - in this folder, `2026-10-11-invoke-platform-coverage-audit.md` and
    `2026-10-12-windows-screen-and-foreground-commands.md`;
  - `docs/SUPPORT_MATRIX.md` and `docs/TERMS_AND_CONCEPTS.md`;
  - the #211 body as of 2026-10-13.
- **Code:**
  - `crates/auv-cli-invoke/src/{support.rs,command.rs,runner.rs,commands/*.rs}`;
  - `crates/auv-driver-windows/src/{session.rs,window.rs,background_input.rs,capture.rs,application.rs,input.rs}`;
  - `crates/auv-cli/src/runner/local_driver.rs`;
  - `crates/auv-scan/src/scroll_until.rs`;
  - `.github/workflows/check.yml`.
